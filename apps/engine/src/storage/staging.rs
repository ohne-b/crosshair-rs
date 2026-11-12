use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use crosshair_core::{
    AppState, Asset, Id, MAX_ASSET_BYTES, MAX_CONFIG_BYTES, MAX_PORTABLE_BYTES, Preset,
    protocol::StagingPurpose, validate_preset, validate_state,
};
use crosshair_renderer::Renderer;

use super::{
    Store, assets, portable,
    state::{open_read, read_bounded, regular_path},
};

#[derive(Debug)]
pub struct Stage {
    pub token: Id,
    pub path: PathBuf,
    pub max_bytes: u64,
}

#[derive(Debug)]
pub enum Staged {
    Image {
        id: Id,
        asset: Asset,
    },
    Document {
        preset: Preset,
        expected_revision: u64,
        preview: bool,
    },
    Import {
        candidate: AppState,
        replace: bool,
    },
    Exported,
}

pub(super) struct Lease {
    owner: Id,
    purpose: StagingPurpose,
    created: Instant,
}

impl Store {
    pub fn stage_begin(&mut self, owner: Id, purpose: StagingPurpose) -> Result<Stage> {
        self.expire_staging();
        ensure!(
            self.staging.len() < 16
                && self
                    .staging
                    .values()
                    .filter(|lease| lease.owner == owner)
                    .count()
                    < 4,
            "Too many pending imports; finish or cancel an existing transfer"
        );
        regular_path(&self.root.join("staging"), true)?;
        let token = Id::new_v4();
        let path = self.staging_path(token);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let max_bytes = limit(&purpose);
        self.staging.insert(
            token,
            Lease {
                owner,
                purpose,
                created: Instant::now(),
            },
        );
        Ok(Stage {
            token,
            path,
            max_bytes,
        })
    }

    pub fn discard_staging(&mut self, owner: Id, token: Id) -> Result<bool> {
        let Some(lease) = self.staging.get(&token) else {
            return Ok(false);
        };
        ensure!(
            lease.owner == owner,
            "This transfer belongs to another connection"
        );
        self.staging.remove(&token);
        let path = self.staging_path(token);
        if path.try_exists()? {
            regular_path(&path, false)?;
            fs::remove_file(path)?;
        }
        Ok(true)
    }

    pub fn disconnect(&mut self, owner: Id) -> Result<usize> {
        self.pending_assets
            .borrow_mut()
            .retain(|_, lease| *lease != Some(owner));
        let tokens: Vec<_> = self
            .staging
            .iter()
            .filter(|(_, lease)| lease.owner == owner)
            .map(|(&id, _)| id)
            .collect();
        let mut removed = 0;
        let mut failure = None;
        for token in tokens {
            match self.discard_staging(owner, token) {
                Ok(_) => removed += 1,
                Err(error) => failure = Some(error),
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(removed)
    }

    pub fn stage_commit(
        &mut self,
        owner: Id,
        token: Id,
        state: &AppState,
        renderer: &mut Renderer,
    ) -> Result<Staged> {
        self.expire_staging();
        let lease = self
            .staging
            .get(&token)
            .context("Transfer expired or does not exist")?;
        ensure!(
            lease.owner == owner,
            "This transfer belongs to another connection"
        );
        let lease = self.staging.remove(&token).expect("lease was checked");
        let path = self.staging_path(token);
        let previous_adoptions: std::collections::BTreeSet<_> =
            self.pending_assets.borrow().keys().copied().collect();
        let result = (|| {
            let file =
                open_read(&path).context("Close the staged file before committing the transfer")?;
            ensure!(
                file.metadata()?.len() <= limit(&lease.purpose),
                "Transfer exceeds its size limit"
            );
            match lease.purpose {
                StagingPurpose::Image => {
                    let (data, asset) =
                        assets::inspect(Arc::from(read_bounded(file, MAX_ASSET_BYTES)?), renderer)?;
                    if let Some((&id, existing)) = state
                        .assets
                        .iter()
                        .find(|(_, existing)| existing.sha256 == asset.sha256)
                    {
                        ensure!(
                            existing == &asset,
                            "Existing image metadata does not match its content"
                        );
                        self.read_asset(state, id)?;
                        return Ok(Staged::Image { id, asset });
                    }
                    let id = Id::new_v4();
                    let mut candidate = state.clone();
                    candidate.assets.insert(id, asset.clone());
                    validate_state(&candidate)?;
                    self.adopt(id, &data)?;
                    Ok(Staged::Image { id, asset })
                }
                StagingPurpose::Document {
                    preset,
                    expected_revision,
                    preview,
                } => {
                    ensure!(
                        state
                            .preset(preset)
                            .context("The edited preset was deleted")?
                            .revision
                            == expected_revision,
                        "The document changed; reload it before applying this edit"
                    );
                    let document: Preset =
                        serde_json::from_slice(&read_bounded(file, MAX_CONFIG_BYTES as u64)?)?;
                    ensure!(
                        document.id == preset,
                        "Staged document targets a different preset"
                    );
                    validate_preset(&document, &state.assets)?;
                    renderer.render(&document, 1.0, &|id| self.read_asset(state, id))?;
                    Ok(Staged::Document {
                        preset: document,
                        expected_revision,
                        preview,
                    })
                }
                StagingPurpose::ExportDraft {
                    destination,
                    format,
                } => {
                    let preset: Preset =
                        serde_json::from_slice(&read_bounded(file, MAX_CONFIG_BYTES as u64)?)?;
                    validate_preset(&preset, &state.assets)?;
                    let id = preset.id;
                    let mut snapshot = state.clone();
                    snapshot.presets = vec![preset];
                    snapshot.profiles = vec![crosshair_core::Profile::new("Export", id)];
                    snapshot.active_profile = snapshot.profiles[0].id;
                    snapshot.settings.global_toggle = None;
                    super::prune_asset_index(&mut snapshot, &Default::default());
                    validate_state(&snapshot)?;
                    let kind = match format {
                        crosshair_core::protocol::DraftFormat::Png => {
                            crosshair_core::protocol::ExportKind::Png { preset: id }
                        }
                        crosshair_core::protocol::DraftFormat::Svg => {
                            crosshair_core::protocol::ExportKind::Svg { preset: id }
                        }
                        crosshair_core::protocol::DraftFormat::Preset => {
                            crosshair_core::protocol::ExportKind::Preset { preset: id }
                        }
                    };
                    self.export(
                        &snapshot,
                        kind,
                        std::path::Path::new(&destination),
                        renderer,
                    )?;
                    Ok(Staged::Exported)
                }
                StagingPurpose::Import { replace } => Ok(Staged::Import {
                    candidate: portable::import(self, file, state, replace, renderer)?,
                    replace,
                }),
            }
        })();
        let new_adoptions: Vec<_> = self
            .pending_assets
            .borrow()
            .keys()
            .filter(|id| !previous_adoptions.contains(id))
            .copied()
            .collect();
        for id in new_adoptions {
            if result.is_ok() {
                self.pending_assets.borrow_mut().insert(id, Some(owner));
            } else {
                self.pending_assets.borrow_mut().remove(&id);
                let _ = fs::remove_file(self.asset_path(id));
            }
        }
        let _ = fs::remove_file(path);
        result
    }

    fn expire_staging(&mut self) {
        let expired: Vec<_> = self
            .staging
            .iter()
            .filter(|(_, lease)| lease.created.elapsed() > Duration::from_secs(300))
            .map(|(&id, lease)| (id, lease.owner))
            .collect();
        for (id, owner) in expired {
            let _ = self.discard_staging(owner, id);
        }
    }
}

fn limit(purpose: &StagingPurpose) -> u64 {
    match purpose {
        StagingPurpose::Image => MAX_ASSET_BYTES,
        StagingPurpose::Document { .. } | StagingPurpose::ExportDraft { .. } => {
            MAX_CONFIG_BYTES as u64
        }
        StagingPurpose::Import { .. } => MAX_PORTABLE_BYTES,
    }
}
