use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

use anyhow::Result;
use crosshair_core::{Asset, Id, Preset};
use crosshair_renderer::{Raster, Renderer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub preset: Id,
    pub revision: u64,
    pub scale: u32,
}
impl Key {
    pub fn new(preset: &Preset, scale: f32) -> Self {
        Self {
            preset: preset.id,
            revision: preset.revision,
            scale: scale.to_bits(),
        }
    }
}

pub struct Job {
    pub key: Key,
    pub preset: Preset,
    pub assets: BTreeMap<Id, Asset>,
}
pub struct Completed {
    pub key: Key,
    pub result: Result<Arc<Raster>>,
}
struct Shared {
    pending: Mutex<Option<(u64, Job)>>,
    changed: Condvar,
    generation: AtomicU64,
    stopped: AtomicBool,
}
pub struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(root: PathBuf, notify: impl Fn(Completed) + Send + 'static) -> Result<Self> {
        let shared = Arc::new(Shared {
            pending: Mutex::new(None),
            changed: Condvar::new(),
            generation: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
        });
        let work = shared.clone();
        let thread = thread::Builder::new()
            .name("crosshair rendering".into())
            .spawn(move || {
                let mut renderer = None;
                loop {
                    let (generation, job) = {
                        let mut pending = work.pending.lock().unwrap();
                        while pending.is_none() && !work.stopped.load(Ordering::Acquire) {
                            pending = work.changed.wait(pending).unwrap();
                        }
                        if work.stopped.load(Ordering::Acquire) {
                            break;
                        }
                        pending.take().unwrap()
                    };
                    let result = (|| -> Result<Arc<Raster>> {
                        if renderer.is_none() {
                            renderer = Some(Renderer::new()?);
                        }
                        let renderer = renderer.as_mut().unwrap();
                        let result = renderer.render_cancellable(
                            &job.preset,
                            f32::from_bits(job.key.scale),
                            &|id| crate::storage::read_asset_at(&root, &job.assets, id),
                            || {
                                work.generation.load(Ordering::Acquire) != generation
                                    || work.stopped.load(Ordering::Acquire)
                            },
                        );
                        renderer.clear_raster_cache();
                        result
                    })();
                    if work.generation.load(Ordering::Acquire) == generation
                        && !work.stopped.load(Ordering::Acquire)
                    {
                        notify(Completed {
                            key: job.key,
                            result,
                        });
                    }
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn submit(&self, job: Job) {
        let mut pending = self.shared.pending.lock().unwrap();
        let generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *pending = Some((generation, job));
        self.shared.changed.notify_one();
    }

    pub fn cancel(&self) {
        let mut pending = self.shared.pending.lock().unwrap();
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        *pending = None;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        {
            let _pending = self.shared.pending.lock().unwrap();
            self.shared.stopped.store(true, Ordering::Release);
            self.shared.changed.notify_one();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
pub struct Cache {
    entries: VecDeque<(Key, Arc<Raster>)>,
    bytes: usize,
}
impl Cache {
    pub fn retain(&mut self, keys: &[Key]) {
        self.entries.retain(|(key, _)| keys.contains(key));
        self.bytes = self
            .entries
            .iter()
            .map(|(_, raster)| raster.pixels.len())
            .sum();
    }

    pub fn warm(&mut self, key: Key, raster: Arc<Raster>) -> bool {
        let retained: usize = self
            .entries
            .iter()
            .filter(|(stored, _)| *stored != key)
            .map(|(_, raster)| raster.pixels.len())
            .sum();
        if retained + raster.pixels.len() > 32 * 1024 * 1024 {
            return false;
        }
        self.insert(key, raster);
        true
    }
    pub fn get(&mut self, key: Key) -> Option<Arc<Raster>> {
        let index = self.entries.iter().position(|(stored, _)| *stored == key)?;
        let entry = self.entries.remove(index)?;
        let raster = entry.1.clone();
        self.entries.push_back(entry);
        Some(raster)
    }
    pub fn insert(&mut self, key: Key, raster: Arc<Raster>) {
        if let Some(index) = self.entries.iter().position(|(stored, _)| *stored == key) {
            self.entries.remove(index);
        }
        self.entries.push_back((key, raster));
        self.bytes = self
            .entries
            .iter()
            .map(|(_, raster)| raster.pixels.len())
            .sum();
        while self.bytes > 32 * 1024 * 1024 {
            if let Some((_, raster)) = self.entries.pop_front() {
                self.bytes -= raster.pixels.len();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_transition_can_retain_two_versions_of_the_same_preset() {
        let mut cache = Cache::default();
        let mut preset = Preset::new("restored fixture");
        let old = Key::new(&preset, 1.0);
        preset.revision = 1;
        let next = Key::new(&preset, 1.5);
        let raster = Arc::new(Raster {
            width: 1,
            height: 1,
            origin: [0, 0],
            pixels: vec![0; 4],
        });
        cache.insert(old, raster.clone());
        cache.insert(next, raster);
        assert!(cache.get(old).is_some());
        assert!(cache.get(next).is_some());
        cache.retain(&[next]);
        assert!(cache.get(old).is_none());
        assert!(cache.get(next).is_some());
    }
    #[test]
    fn cache_is_bounded_and_keeps_most_recently_used_presets() {
        let mut cache = Cache::default();
        let presets: Vec<_> = (0..9).map(|_| Preset::new("cache fixture")).collect();
        for preset in &presets[..8] {
            cache.insert(
                Key::new(preset, 1.0),
                Arc::new(Raster {
                    width: 1024,
                    height: 1024,
                    origin: [0, 0],
                    pixels: vec![0; 4 * 1024 * 1024],
                }),
            );
        }
        assert!(cache.get(Key::new(&presets[0], 1.0)).is_some());
        cache.insert(
            Key::new(&presets[8], 1.0),
            Arc::new(Raster {
                width: 1024,
                height: 1024,
                origin: [0, 0],
                pixels: vec![0; 4 * 1024 * 1024],
            }),
        );
        assert!(cache.get(Key::new(&presets[1], 1.0)).is_none());
        assert!(cache.get(Key::new(&presets[0], 1.0)).is_some());
        assert_eq!(cache.bytes, 32 * 1024 * 1024);
        let extra = Key::new(&Preset::new("uncached shortcut"), 1.0);
        assert!(!cache.warm(
            extra,
            Arc::new(Raster {
                width: 1,
                height: 1,
                origin: [0, 0],
                pixels: vec![0; 4]
            })
        ));
        assert!(cache.get(Key::new(&presets[0], 1.0)).is_some());
        cache.retain(&[Key::new(&presets[0], 1.0)]);
        assert_eq!(cache.bytes, 4 * 1024 * 1024);
        assert!(cache.warm(
            extra,
            Arc::new(Raster {
                width: 1,
                height: 1,
                origin: [0, 0],
                pixels: vec![0; 4]
            })
        ));
    }
}
