use std::{
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

use anyhow::{Context, Result};
use crosshair_core::{Anchor, AppState, ArmShape, Asset, Content, Id, Layer, Point, Preset};
use crosshair_renderer::{AssetData, LayerBounds, Raster, Renderer};

use super::client::Client;

pub struct Request {
    pub document: Option<Preset>,
    pub zoom: f32,
    pub presets: Vec<Preset>,
    pub assets: BTreeMap<Id, Asset>,
    pub draw_base: Option<Layer>,
    pub max_output_scale: f32,
}

pub enum Event {
    Document {
        scale: f32,
        generation: u64,
        id: Id,
        raster: Arc<Raster>,
        bounds: Vec<(Id, LayerBounds)>,
        inverses: BTreeMap<Id, [Point; 3]>,
        anchors: BTreeMap<Id, Point>,
        adjusted: Option<Preset>,
    },
    Thumbnail {
        id: Id,
        revision: u64,
        raster: Arc<Raster>,
    },
    Fonts(Vec<String>),
    Error(String),
    DocumentFailed {
        generation: u64,
        id: Id,
        message: String,
    },
    Anchored {
        original: Preset,
        changed: Preset,
    },
    Recovered {
        original: Preset,
        changed: Preset,
        state: AppState,
        action: RecoveryAction,
    },
}

#[derive(Clone)]
pub enum RecoveryAction {
    Reapply,
    Save,
    Export {
        destination: std::path::PathBuf,
        format: crosshair_core::protocol::DraftFormat,
    },
}

#[derive(Default)]
struct Pending {
    request: Mutex<Option<(u64, Request)>>,
    ready: Condvar,
    generation: AtomicU64,
    stopped: AtomicBool,
    anchor: Mutex<Option<(Preset, Vec<Id>, Anchor)>>,
    recovery: Mutex<Option<(Preset, RecoveryAction)>>,
    recovering: AtomicBool,
}

pub struct Preview {
    pending: Arc<Pending>,
}

impl Preview {
    #[cfg(test)]
    pub fn idle_for_test() -> Self {
        Self {
            pending: Arc::new(Pending::default()),
        }
    }
    pub fn start(client: Client, events: Arc<dyn Fn(Event) + Send + Sync>) -> Self {
        let pending = Arc::new(Pending::default());
        let work = pending.clone();
        thread::Builder::new()
            .name("editor-preview".into())
            .spawn(move || {
                if let Err(error) = worker(client, work, events.clone()) {
                    events(Event::Error(error.to_string()));
                }
            })
            .expect("Preview worker must start");
        Self { pending }
    }
    pub fn request(&self, request: Request) -> u64 {
        let generation = self.pending.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Ok(mut pending) = self.pending.request.lock() {
            *pending = Some((generation, request));
            self.pending.ready.notify_one();
        }
        generation
    }
    pub fn stop(&self) {
        self.pending.stopped.store(true, Ordering::Release);
        self.pending.ready.notify_one();
    }
    pub fn set_anchor(&self, document: Preset, selected: Vec<Id>, anchor: Anchor) {
        let _request = self.pending.request.lock();
        if let Ok(mut job) = self.pending.anchor.lock() {
            *job = Some((document, selected, anchor));
        }
        self.pending.ready.notify_one();
    }
    pub fn recover(&self, document: Preset, action: RecoveryAction) -> Result<()> {
        let _request = self
            .pending
            .request
            .lock()
            .map_err(|_| anyhow::anyhow!("Preview queue stopped"))?;
        let mut job = self
            .pending
            .recovery
            .lock()
            .map_err(|_| anyhow::anyhow!("Preview queue stopped"))?;
        anyhow::ensure!(
            !self.pending.recovering.swap(true, Ordering::AcqRel),
            "A draft operation is already preparing. Wait for it to finish before retrying."
        );
        *job = Some((document, action));
        self.pending.ready.notify_one();
        Ok(())
    }
}

fn worker(
    client: Client,
    pending: Arc<Pending>,
    events: Arc<dyn Fn(Event) + Send + Sync>,
) -> Result<()> {
    let mut renderer = Renderer::new()?;
    match renderer.font_families() {
        Ok(fonts) => events(Event::Fonts(fonts)),
        Err(error) => events(Event::Error(format!(
            "Could not list installed fonts: {error}"
        ))),
    }
    let cache = std::cell::RefCell::new(BTreeMap::<Id, AssetData>::new());
    let mut thumbnail_revisions = BTreeMap::new();
    loop {
        let mut queued = pending
            .request
            .lock()
            .map_err(|_| anyhow::anyhow!("Preview queue stopped"))?;
        while queued.is_none()
            && pending
                .anchor
                .lock()
                .map(|job| job.is_none())
                .unwrap_or(true)
            && pending
                .recovery
                .lock()
                .map(|job| job.is_none())
                .unwrap_or(true)
            && !pending.stopped.load(Ordering::Acquire)
        {
            queued = pending
                .ready
                .wait(queued)
                .map_err(|_| anyhow::anyhow!("Preview queue stopped"))?;
        }
        if pending.stopped.load(Ordering::Acquire) {
            break;
        }
        let render_job = queued.take();
        drop(queued);
        if let Some((original, action)) =
            pending.recovery.lock().ok().and_then(|mut job| job.take())
        {
            let result = recover(&client, &original);
            pending.recovering.store(false, Ordering::Release);
            match result {
                Ok((changed, state)) => events(Event::Recovered {
                    original,
                    changed,
                    state,
                    action,
                }),
                Err(error) => events(Event::Error(error.to_string())),
            }
        }
        let anchor = pending.anchor.lock().ok().and_then(|mut job| job.take());
        if let Some((original, selected, anchor)) = anchor {
            let mut changed = original.clone();
            let result = changed
                .layers
                .iter_mut()
                .filter(|layer| selected.contains(&layer.id) && !layer.locked)
                .try_for_each(|layer| renderer.set_anchor(layer, anchor));
            match result {
                Ok(()) => events(Event::Anchored { original, changed }),
                Err(error) => events(Event::Error(error.to_string())),
            }
        }
        let Some((generation, mut request)) = render_job else {
            continue;
        };
        let cancelled = || {
            pending.stopped.load(Ordering::Acquire)
                || pending.generation.load(Ordering::Acquire) != generation
        };
        let pinned_assets: std::collections::BTreeSet<_> = request
            .document
            .iter()
            .flat_map(|document| &document.layers)
            .filter_map(|layer| layer.content.asset_id())
            .collect();
        let source = |id: Id| -> Result<AssetData> {
            if let Some(asset) = cache.borrow().get(&id) {
                return Ok(asset.clone());
            }
            if let Some(asset) = client.cached_asset(id) {
                return Ok(asset);
            }
            let metadata = request
                .assets
                .get(&id)
                .context("Image is missing from the engine state")?
                .clone();
            let asset = client.asset(id, metadata)?;
            let mut cache = cache.borrow_mut();
            if cache.values().map(|asset| asset.bytes.len()).sum::<usize>() + asset.bytes.len()
                > 64 * 1024 * 1024
            {
                cache.retain(|id, _| pinned_assets.contains(id));
            }
            cache.insert(id, asset.clone());
            Ok(asset)
        };
        let mut adjusted = None;
        if let (Some(base), Some(document)) = (&request.draw_base, &mut request.document)
            && let Some(layer) = document.layers.iter_mut().find(|layer| layer.id == base.id)
        {
            let Ok(previous) = renderer.local_point(base, Point::default()) else {
                events(Event::DocumentFailed {
                    generation,
                    id: document.id,
                    message: "The drawing's transform is invalid".into(),
                });
                continue;
            };
            let Ok(current) = renderer.local_point(layer, Point::default()) else {
                events(Event::DocumentFailed {
                    generation,
                    id: document.id,
                    message: "This stroke exceeds the drawing bounds".into(),
                });
                continue;
            };
            let (sin, cos) = layer.transform.rotation.to_radians().sin_cos();
            let x = current.x - previous.x;
            let y = current.y - previous.y;
            layer.transform.x += layer.transform.scale * (cos * x - sin * y);
            layer.transform.y += layer.transform.scale * (sin * x + cos * y);
            adjusted = Some(document.clone());
        }
        if let Some(document) = &request.document {
            let rendered = renderer
                .measure(document, request.max_output_scale)
                .and_then(|_| {
                    render_preview(&mut renderer, document, request.zoom, &source, cancelled)
                });
            match rendered {
                Ok((raster, scale)) if !cancelled() => {
                    let bounds = document
                        .layers
                        .iter()
                        .filter_map(|layer| {
                            renderer
                                .layer_bounds(layer, 1.0)
                                .ok()
                                .map(|bounds| (layer.id, bounds))
                        })
                        .collect();
                    let inverses = document
                        .layers
                        .iter()
                        .filter_map(|layer| {
                            let points = [
                                Point::default(),
                                Point { x: 1.0, y: 0.0 },
                                Point { x: 0.0, y: 1.0 },
                            ];
                            let inverse = points.map(|point| renderer.local_point(layer, point));
                            match inverse {
                                [Ok(a), Ok(b), Ok(c)] => Some((layer.id, [a, b, c])),
                                _ => None,
                            }
                        })
                        .collect();
                    let anchors = document
                        .layers
                        .iter()
                        .filter_map(|layer| {
                            renderer
                                .anchor_point(layer)
                                .ok()
                                .map(|point| (layer.id, point))
                        })
                        .collect();
                    events(Event::Document {
                        scale,
                        generation,
                        id: document.id,
                        raster,
                        bounds,
                        inverses,
                        anchors,
                        adjusted,
                    });
                }
                Err(error) if !cancelled() => events(Event::DocumentFailed {
                    generation,
                    id: document.id,
                    message: error.to_string(),
                }),
                _ => {}
            }
        }
        for preset in &request.presets {
            if cancelled() {
                break;
            }
            if thumbnail_revisions.get(&preset.id) == Some(&preset.revision) {
                continue;
            }
            match renderer.render_cancellable(preset, 1.0, &source, cancelled) {
                Ok(raster) if !cancelled() => {
                    let raster = if raster.width.max(raster.height) > 80 {
                        match renderer.render_cancellable(
                            preset,
                            80.0 / raster.width.max(raster.height) as f32,
                            &source,
                            cancelled,
                        ) {
                            Ok(raster) => raster,
                            Err(error) => {
                                if !cancelled() {
                                    events(Event::Error(error.to_string()));
                                }
                                continue;
                            }
                        }
                    } else {
                        raster
                    };
                    thumbnail_revisions.insert(preset.id, preset.revision);
                    events(Event::Thumbnail {
                        id: preset.id,
                        revision: preset.revision,
                        raster,
                    });
                }
                Err(error) if !cancelled() => events(Event::Error(error.to_string())),
                _ => {}
            }
        }
        let ids: std::collections::BTreeSet<_> =
            request.presets.iter().map(|preset| preset.id).collect();
        thumbnail_revisions.retain(|id, _| ids.contains(id));
    }
    Ok(())
}

fn recover(client: &Client, original: &Preset) -> Result<(Preset, AppState)> {
    let state = client.state()?;
    let mut changed = original.clone();
    let mut remapped = BTreeMap::new();
    for layer in &original.layers {
        if let Some(id) = layer.content.asset_id()
            && !state.assets.contains_key(&id)
            && !remapped.contains_key(&id)
        {
            let data = client.cached_asset(id).context("An image source is unavailable after engine recovery. Keep this draft open and replace the missing image from its original file.")?;
            remapped.insert(id, client.restage_asset(data)?);
        }
    }
    for layer in &mut changed.layers {
        let image = match &mut layer.content {
            Content::Image(image) => Some(image),
            Content::Crosshair(crosshair) => match &mut crosshair.shape {
                ArmShape::Image(image) => Some(image),
                _ => None,
            },
            _ => None,
        };
        if let Some(image) = image
            && let Some(id) = remapped.get(&image.asset)
        {
            image.asset = *id;
        }
    }
    let state = if remapped.is_empty() {
        state
    } else {
        client.state()?
    };
    crosshair_core::validate_preset(&changed, &state.assets)?;
    Ok((changed, state))
}

pub fn image(raster: &Raster) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &raster.pixels,
        raster.width,
        raster.height,
    );
    slint::Image::from_rgba8_premultiplied(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_draft_operation_cannot_be_silently_replaced() {
        let preview = Preview {
            pending: Arc::new(Pending::default()),
        };
        let original = Preset::new("Original draft");
        preview
            .recover(original.clone(), RecoveryAction::Reapply)
            .unwrap();
        assert!(
            preview
                .recover(Preset::new("Another draft"), RecoveryAction::Reapply)
                .is_err()
        );
        assert_eq!(
            preview
                .pending
                .recovery
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .0
                .id,
            original.id
        );
        preview.pending.recovery.lock().unwrap().take();
        assert!(
            preview
                .recover(Preset::new("While running"), RecoveryAction::Reapply)
                .is_err()
        );
        preview.pending.recovering.store(false, Ordering::Release);
        assert!(preview.recover(original, RecoveryAction::Reapply).is_ok());
    }
}

// Bound editor bitmaps to the same 1024px budget as the overlay, even at 6400% zoom.
fn render_preview(
    renderer: &mut Renderer,
    document: &Preset,
    zoom: f32,
    source: &impl crosshair_renderer::AssetSource,
    cancelled: impl Fn() -> bool,
) -> Result<(Arc<Raster>, f32)> {
    let mut scale = zoom.clamp(1.0, 16.0);
    loop {
        anyhow::ensure!(!cancelled(), "Rendering cancelled");
        let bounds = document
            .layers
            .iter()
            .filter(|layer| layer.visible && layer.opacity > 0.0)
            .try_fold(None::<LayerBounds>, |bounds, layer| {
                renderer
                    .layer_bounds(layer, scale)
                    .map(|next| Some(bounds.map_or(next, |current| current.union(next))))
            });
        let fits = bounds.as_ref().is_ok_and(|bounds| {
            bounds.is_none_or(|bounds| {
                bounds.right.ceil() - bounds.left.floor() <= 1024.0
                    && bounds.bottom.ceil() - bounds.top.floor() <= 1024.0
            })
        });
        if fits || scale <= 1.0 {
            return renderer
                .render_cancellable(document, scale, source, cancelled)
                .map(|raster| (raster, scale));
        }
        scale = (scale / 2.0).max(1.0);
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use crosshair_core::{Color, Content, Shape};

    #[test]
    #[ignore = "manual preview timing benchmark; run with --nocapture"]
    fn preview_resize_timings() {
        let mut renderer = Renderer::new().unwrap();
        let mut preset = Preset::new("Timing fixture");
        let source = |_id| anyhow::bail!("No images in this fixture");
        let mut samples = Vec::new();
        for frame in 0..70 {
            preset.layers[0].transform.scale = 1.0 + frame as f32 * 0.02;
            let start = std::time::Instant::now();
            renderer.measure(&preset, 1.0).unwrap();
            let (raster, _) =
                render_preview(&mut renderer, &preset, 8.0, &source, || false).unwrap();
            assert!(raster.width <= 1024 && raster.height <= 1024);
            if frame >= 10 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "preview-render: p50={:.2}ms p95={:.2}ms worst={:.2}ms",
            samples[30], samples[57], samples[59]
        );
    }

    #[test]
    fn oversized_design_does_not_poison_the_next_preview() {
        let mut renderer = Renderer::new().unwrap();
        let mut preset = Preset::new("Size limit");
        preset.layers[0].transform.scale = 100.0;
        let source = |_: Id| -> Result<AssetData> { anyhow::bail!("No assets") };
        assert!(render_preview(&mut renderer, &preset, 16.0, &source, || false).is_err());
        preset.layers[0].transform.scale = 1.0;
        let (raster, _) = render_preview(&mut renderer, &preset, 16.0, &source, || false).unwrap();
        assert!(
            raster.width <= crosshair_renderer::MAX_DIMENSION
                && raster.height <= crosshair_renderer::MAX_DIMENSION
        );
        assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn zoomed_triangles_use_vector_geometry_with_bounded_bitmaps() {
        let mut renderer = Renderer::new().unwrap();
        let mut preset = Preset::new("Triangle");
        preset.layers = vec![Layer::new(
            "Triangle",
            Content::Triangle(Shape {
                width: 20.0,
                height: 20.0,
                color: Color {
                    r: 255,
                    g: 128,
                    b: 0,
                    a: 128,
                },
            }),
        )];
        let source = |_: Id| -> Result<AssetData> { anyhow::bail!("No assets") };
        let (raster, scale) =
            render_preview(&mut renderer, &preset, 8.0, &source, || false).unwrap();
        assert_eq!(scale, 8.0);
        assert!(raster.width >= 160 && raster.height >= 160);
        assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] == 128));
        assert!(raster.pixels.chunks_exact(4).all(|pixel| pixel[3] <= 128));
        if let Content::Triangle(shape) = &mut preset.layers[0].content {
            shape.width = 900.0;
            shape.height = 900.0;
        }
        let (raster, scale) =
            render_preview(&mut renderer, &preset, 64.0, &source, || false).unwrap();
        assert_eq!(scale, 1.0);
        assert!(raster.width <= 1024 && raster.height <= 1024);
    }
}
