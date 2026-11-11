//! Serialized durable work. The native window/input owner only submits jobs and
//! handles completions; image decoding and filesystem operations stay here.
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, ensure};
use crosshair_core::{
    AppState, Id, Preset,
    protocol::{ExportKind, StagingPurpose},
    validate_preset,
};
use crosshair_renderer::{Raster, Renderer};

use crate::storage::{Stage, Staged, Store};

const QUEUE_CAPACITY: usize = 4;

#[derive(Debug)]
pub enum Job {
    Save {
        state: Box<AppState>,
        history_assets: BTreeSet<Id>,
    },
    BeginStage {
        owner: Id,
        purpose: StagingPurpose,
    },
    CommitStage {
        owner: Id,
        token: Id,
        state: Box<AppState>,
    },
    DiscardStage {
        owner: Id,
        token: Id,
    },
    Disconnect {
        owner: Id,
    },
    ReadAssetChunk {
        state: Box<AppState>,
        asset: Id,
        offset: u64,
    },
    Export {
        state: Box<AppState>,
        kind: ExportKind,
        destination: PathBuf,
    },
    Recover,
    Reset {
        state: Box<AppState>,
    },
    CommitState {
        state: Box<AppState>,
        restore: bool,
        startup_changed: bool,
    },
    ValidatePreset {
        state: Box<AppState>,
        preset: Box<Preset>,
    },
    ValidateState {
        state: Box<AppState>,
    },
    ClearCaches,
    Shutdown,
}

#[derive(Debug)]
pub enum WorkReply {
    Saved {
        revision: u64,
        cleanup_error: Option<String>,
    },
    Stage(Stage),
    Staged(Staged),
    Discarded {
        removed: bool,
    },
    Disconnected {
        removed: usize,
    },
    Chunk {
        bytes: Vec<u8>,
        complete: bool,
    },
    Exported,
    Recovered {
        state: Box<AppState>,
    },
    Reset {
        state: Box<AppState>,
    },
    Committed {
        state: Box<AppState>,
    },
    Validated {
        preset: Box<Preset>,
        scale: f32,
        raster: Arc<Raster>,
    },
    ValidatedState {
        state: Box<AppState>,
    },
    Cleared,
    Stopped,
}

pub struct Completed {
    pub ticket: u64,
    pub result: Result<WorkReply>,
}
struct Pending {
    ticket: u64,
    job: Job,
}

pub struct Work {
    sender: SyncSender<Pending>,
    thread: JoinHandle<()>,
    closing: bool,
}

impl Work {
    /// Store's exclusive lock moves with it; no UI-thread filesystem owner remains.
    pub fn start(mut store: Store, notify: impl Fn(Completed) + Send + 'static) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Pending>(QUEUE_CAPACITY);
        let thread = thread::Builder::new()
            .name("crosshair-storage".into())
            .spawn(move || {
                let mut renderer = None;
                for Pending { ticket, job } in receiver {
                    if matches!(job, Job::Shutdown) {
                        // Release files, the writer lock, COM objects, and caches before
                        // reporting the shutdown barrier to the native owner.
                        drop(renderer);
                        drop(store);
                        notify(Completed {
                            ticket,
                            result: Ok(WorkReply::Stopped),
                        });
                        return;
                    }
                    let result = run(&mut store, &mut renderer, job);
                    if let Some(renderer) = &mut renderer {
                        renderer.clear_raster_cache();
                    }
                    notify(Completed { ticket, result });
                }
            })?;
        Ok(Self {
            sender,
            thread,
            closing: false,
        })
    }

    /// Queue saturation is reported immediately; the native owner must never wait
    /// behind a long import, decode, export, or flush.
    pub fn try_submit(&mut self, ticket: u64, job: Job) -> Result<()> {
        ensure!(!self.closing, "Storage worker is shutting down");
        let shutdown = matches!(job, Job::Shutdown);
        self.sender
            .try_send(Pending { ticket, job })
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    anyhow::anyhow!("Storage worker is busy; retry after pending work finishes")
                }
                TrySendError::Disconnected(_) => anyhow::anyhow!("Storage worker has stopped"),
            })?;
        self.closing = shutdown;
        Ok(())
    }

    /// Call after the Stopped completion. Closing the sender also lets a worker
    /// finish queued jobs if startup is aborted before a shutdown barrier.
    pub fn join(self) -> Result<()> {
        drop(self.sender);
        self.thread
            .join()
            .map_err(|_| anyhow::anyhow!("Storage worker terminated unexpectedly"))
    }
}

fn rendering(renderer: &mut Option<Renderer>) -> Result<&mut Renderer> {
    if renderer.is_none() {
        *renderer = Some(Renderer::new()?);
    }
    Ok(renderer.as_mut().expect("renderer initialized"))
}

fn run(store: &mut Store, renderer: &mut Option<Renderer>, job: Job) -> Result<WorkReply> {
    Ok(match job {
        Job::Save {
            state,
            history_assets,
        } => {
            store.save(&state)?;
            WorkReply::Saved {
                revision: state.revision,
                cleanup_error: store
                    .collect_assets(&state, &history_assets)
                    .err()
                    .map(|error| error.to_string()),
            }
        }
        Job::BeginStage { owner, purpose } => WorkReply::Stage(store.stage_begin(owner, purpose)?),
        Job::CommitStage {
            owner,
            token,
            state,
        } => {
            let staged = store.stage_commit(owner, token, &state, rendering(renderer)?)?;
            if let Staged::Document { preset, .. } = &staged {
                validate_preset_scales(store, rendering(renderer)?, &state, preset)?;
            }
            WorkReply::Staged(staged)
        }
        Job::DiscardStage { owner, token } => WorkReply::Discarded {
            removed: store.discard_staging(owner, token)?,
        },
        Job::Disconnect { owner } => WorkReply::Disconnected {
            removed: store.disconnect(owner)?,
        },
        Job::ReadAssetChunk {
            state,
            asset,
            offset,
        } => {
            let (bytes, complete) = store.read_asset_chunk(&state, asset, offset)?;
            WorkReply::Chunk { bytes, complete }
        }
        Job::Export {
            state,
            kind,
            destination,
        } => {
            store.export(&state, kind, &destination, rendering(renderer)?)?;
            WorkReply::Exported
        }
        Job::Recover => {
            let state = store.backup_state()?;
            validate_library(store, rendering(renderer)?, &state)?;
            WorkReply::Recovered {
                state: Box::new(store.prepare_recovery()?),
            }
        }
        Job::Reset { state } => WorkReply::Reset {
            state: Box::new(store.prepare_reset(&state)?),
        },
        Job::CommitState {
            state,
            restore,
            startup_changed,
        } => {
            let publish = || {
                if restore {
                    store.install_recovery(&state)
                } else {
                    store.save(&state)
                }
            };
            if startup_changed || restore {
                crate::lifecycle::with_startup(state.settings.launch_at_login, publish)?;
            } else {
                publish()?;
            }
            WorkReply::Committed { state }
        }
        Job::ValidatePreset { state, preset } => {
            validate_preset(&preset, &state.assets)?;
            let scale = state
                .active_profile()
                .context("Active profile is missing")?
                .scale;
            let renderer = rendering(renderer)?;
            validate_preset_scales(store, renderer, &state, &preset)?;
            let raster = renderer.render(&preset, scale, &|id| store.read_asset(&state, id))?;
            WorkReply::Validated {
                preset,
                scale,
                raster,
            }
        }
        Job::ValidateState { state } => {
            crosshair_core::validate_state(&state)?;
            validate_library(store, rendering(renderer)?, &state)?;
            WorkReply::ValidatedState { state }
        }
        Job::ClearCaches => {
            *renderer = None;
            store.clear_read_cache();
            WorkReply::Cleared
        }
        Job::Shutdown => unreachable!("shutdown releases ownership before notification"),
    })
}

fn scales(state: &AppState) -> BTreeSet<u32> {
    std::iter::once(1.0f32.to_bits())
        .chain(state.profiles.iter().flat_map(|profile| {
            std::iter::once(profile.scale.to_bits()).chain(
                profile
                    .positions
                    .iter()
                    .map(|position| position.scale.to_bits()),
            )
        }))
        .collect()
}

fn validate_library(store: &Store, renderer: &mut Renderer, state: &AppState) -> Result<()> {
    for preset in &state.presets {
        validate_preset_scales(store, renderer, state, preset)?;
    }
    Ok(())
}

fn validate_preset_scales(
    store: &Store,
    renderer: &mut Renderer,
    state: &AppState,
    preset: &Preset,
) -> Result<()> {
    for scale in scales(state) {
        renderer.render(preset, f32::from_bits(scale), &|id| {
            store.read_asset(state, id)
        })?;
        renderer.clear_raster_cache();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn writes_are_serial_and_shutdown_releases_the_cross_session_lock() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(directory.path())?;
        let (sender, receiver) = mpsc::channel();
        let mut work = Work::start(store, move |result| {
            let _ = sender.send(result);
        })?;
        let state = AppState {
            revision: 17,
            ..Default::default()
        };
        work.try_submit(
            1,
            Job::Save {
                state: Box::new(state.clone()),
                history_assets: BTreeSet::new(),
            },
        )?;
        let next = AppState {
            revision: 18,
            base_visible: !state.base_visible,
            ..state.clone()
        };
        work.try_submit(
            2,
            Job::Save {
                state: Box::new(next.clone()),
                history_assets: BTreeSet::new(),
            },
        )?;
        work.try_submit(3, Job::Shutdown)?;
        assert!(work.try_submit(4, Job::ClearCaches).is_err());
        let first = receiver.recv_timeout(Duration::from_secs(10))?;
        assert_eq!(first.ticket, 1);
        assert!(matches!(
            first.result?,
            WorkReply::Saved {
                revision: 17,
                cleanup_error: None
            }
        ));
        let second = receiver.recv_timeout(Duration::from_secs(10))?;
        assert_eq!(second.ticket, 2);
        assert!(matches!(
            second.result?,
            WorkReply::Saved {
                revision: 18,
                cleanup_error: None
            }
        ));
        let third = receiver.recv_timeout(Duration::from_secs(10))?;
        assert_eq!(third.ticket, 3);
        assert!(matches!(third.result?, WorkReply::Stopped));
        assert_eq!(Store::open(directory.path())?.load()?, next);
        work.join()?;
        Ok(())
    }

    #[test]
    fn saturated_queue_returns_immediately_and_jobs_keep_their_tickets() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store = Store::open(directory.path())?;
        let (started, received) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let mut work = Work::start(store, move |result| {
            if result.ticket == 0 {
                let _ = started.send(());
                let _ = blocked.recv();
            }
        })?;
        work.try_submit(0, Job::ClearCaches)?;
        received.recv_timeout(Duration::from_secs(5))?;
        for ticket in 1..=QUEUE_CAPACITY {
            work.try_submit(ticket as u64, Job::ClearCaches)?;
        }
        assert!(work.try_submit(10, Job::ClearCaches).is_err());
        release.send(())?;
        work.join()?;
        Ok(())
    }

    #[test]
    fn staged_documents_are_checked_at_saved_position_scales_before_publication() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(directory.path())?;
        let mut state = AppState::default();
        state.profiles[0]
            .positions
            .push(crosshair_core::SavedPosition {
                id: Id::new_v4(),
                name: "Large".into(),
                offset: Default::default(),
                scale: 2.0,
            });
        store.save(&state)?;
        let mut preset = state.presets[0].clone();
        preset.layers[0].content = crosshair_core::Content::Rectangle(crosshair_core::Rectangle {
            width: 600.0,
            height: 8.0,
            ..Default::default()
        });
        let owner = Id::new_v4();
        let stage = store.stage_begin(
            owner,
            StagingPurpose::Document {
                preset: preset.id,
                expected_revision: preset.revision,
            },
        )?;
        std::fs::write(&stage.path, serde_json::to_vec(&preset)?)?;
        assert!(
            run(
                &mut store,
                &mut None,
                Job::CommitStage {
                    owner,
                    token: stage.token,
                    state: Box::new(state.clone())
                }
            )
            .is_err()
        );
        assert_eq!(store.load()?, state);
        let mut candidate = state.clone();
        candidate.presets[0] = preset;
        assert!(
            run(
                &mut store,
                &mut None,
                Job::ValidateState {
                    state: Box::new(candidate)
                }
            )
            .is_err()
        );
        Ok(())
    }
}
