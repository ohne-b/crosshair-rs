use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use anyhow::{Result, bail, ensure};
use crosshair_core::{protocol::*, *};

use crate::app::App;

impl App {
    pub fn command(&mut self, owner: Id, command: Command) -> Result<Reply> {
        if self.status.recovery_required {
            ensure!(
                matches!(
                    command,
                    Command::Subscribe
                        | Command::Snapshot
                        | Command::ReadSnapshot { .. }
                        | Command::RecoverBackup
                        | Command::ResetSettings
                        | Command::Export {
                            kind: ExportKind::Recovery,
                            ..
                        }
                ),
                "Settings need recovery. Restore the backup or explicitly reset after exporting recovery data"
            );
        }
        match command {
            Command::Subscribe => {
                self.clients
                    .get_mut(&owner)
                    .ok_or_else(|| anyhow::anyhow!("Connection closed"))?
                    .subscribed = true;
                return Ok(Reply::Status {
                    status: self.status(),
                });
            }
            Command::Snapshot => {
                let bytes = serde_json::to_vec(&self.state)?;
                ensure!(
                    bytes.len() <= MAX_CONFIG_BYTES,
                    "Settings exceed the snapshot limit"
                );
                let token = Id::new_v4();
                let length = bytes.len() as u64;
                self.clients
                    .get_mut(&owner)
                    .ok_or_else(|| anyhow::anyhow!("Connection closed"))?
                    .snapshot = Some((token, bytes, Instant::now()));
                return Ok(Reply::Snapshot {
                    token,
                    revision: self.state.revision,
                    bytes: length,
                });
            }
            Command::ReadSnapshot { token, offset } => {
                let snapshot = self
                    .clients
                    .get_mut(&owner)
                    .and_then(|c| c.snapshot.as_mut())
                    .ok_or_else(|| anyhow::anyhow!("Request a fresh snapshot"))?;
                ensure!(
                    snapshot.0 == token && snapshot.2.elapsed() < Duration::from_secs(60),
                    "Snapshot expired"
                );
                let offset = usize::try_from(offset)?;
                ensure!(offset <= snapshot.1.len(), "Invalid snapshot offset");
                let end = (offset + CHUNK_BYTES).min(snapshot.1.len());
                return Ok(Reply::Chunk {
                    bytes: snapshot.1[offset..end].to_vec(),
                    complete: end == snapshot.1.len(),
                });
            }
            Command::GetPreset { preset } => {
                let preset = self.preset(preset)?.clone();
                ensure!(
                    serde_json::to_vec(&preset)?.len() < MAX_FRAME_BYTES - 512,
                    "This document requires a full snapshot transfer"
                );
                return Ok(Reply::Preset { preset });
            }
            Command::SelectPreset { preset } => {
                self.preset(preset)?;
                self.reset_input();
                self.browse_preset = None;
                self.state.active_profile_mut().unwrap().selected_preset = preset;
                self.bump()?;
            }
            Command::SelectBrowse { preset } => {
                ensure!(
                    self.state.preset(preset).is_none(),
                    "Crosshair ID is already saved"
                );
                let preset = browse::presets()
                    .into_iter()
                    .find(|p| p.id == preset)
                    .ok_or_else(|| anyhow::anyhow!("Unknown built-in crosshair"))?;
                self.reset_input();
                self.browse_preset = Some(preset);
                self.changed(None, false);
            }
            Command::ToggleVisibility => self.input.toggle_visibility(&mut self.state),
            Command::SetVisibility { visible } => {
                self.reset_input();
                self.state.base_visible = visible;
                self.bump()?;
            }
            Command::BeginEdit {
                preset,
                expected_revision,
            } => {
                ensure!(self.edit_owner.is_none(), "Another edit is already active");
                self.history.begin(&self.state, preset, expected_revision)?;
                self.edit_owner = Some(owner);
            }
            Command::ReplacePreset {
                preset,
                expected_revision,
            } => return self.replace_preset(owner, preset, expected_revision),
            Command::EndEdit | Command::CancelEdit => {
                ensure!(
                    self.edit_owner == Some(owner),
                    "This connection does not own the edit"
                );
                if matches!(command, Command::EndEdit) {
                    self.history.end(&self.state)?;
                } else {
                    self.history.cancel(&mut self.state)?;
                }
                self.edit_owner = None;
            }
            Command::Undo | Command::Redo => {
                let before = self
                    .state
                    .active_profile()
                    .map(|p| (p.id, p.selected_preset));
                if matches!(command, Command::Undo) {
                    self.history.undo(&mut self.state)?;
                } else {
                    self.history.redo(&mut self.state)?;
                }
                if before
                    != self
                        .state
                        .active_profile()
                        .map(|p| (p.id, p.selected_preset))
                {
                    self.browse_preset = None;
                }
                self.reset_input();
            }
            Command::NewPreset { name } => {
                let preset = Preset::new(name);
                let id = preset.id;
                let mut next = self.state.clone();
                next.presets.push(preset);
                next.active_profile_mut().unwrap().selected_preset = id;
                self.commit(next)?;
                return Ok(Reply::Created { id });
            }
            Command::AddPreset { preset } => {
                let copy = preset.duplicate(preset.name.clone());
                let id = copy.id;
                let mut next = self.state.clone();
                next.presets.push(copy);
                self.commit(next)?;
                return Ok(Reply::Created { id });
            }
            Command::DuplicatePreset { preset } => {
                let preset = self.preset(preset)?;
                let duplicate = preset.duplicate(format!(
                    "{} copy",
                    preset.name.chars().take(70).collect::<String>()
                ));
                let id = duplicate.id;
                let mut next = self.state.clone();
                next.presets.push(duplicate);
                next.active_profile_mut().unwrap().selected_preset = id;
                self.commit(next)?;
                return Ok(Reply::Created { id });
            }
            Command::DeletePreset {
                preset,
                replacement,
            } => {
                self.preset(preset)?;
                self.preset(replacement)?;
                ensure!(
                    preset != replacement && self.state.presets.len() > 1,
                    "Keep at least one crosshair"
                );
                let mut next = self.state.clone();
                next.presets.retain(|p| p.id != preset);
                for profile in &mut next.profiles {
                    if profile.selected_preset == preset {
                        profile.selected_preset = replacement;
                    }
                    for binding in &mut profile.bindings {
                        match &mut binding.action {
                            BindingAction::SelectPreset { preset: target } if *target == preset => {
                                *target = replacement
                            }
                            _ => {}
                        }
                    }
                }
                self.commit(next)?;
            }
            Command::ReorderPresets { order } => {
                let ids: BTreeSet<_> = order.iter().copied().collect();
                ensure!(
                    ids.len() == self.state.presets.len()
                        && order.len() == ids.len()
                        && self.state.presets.iter().all(|p| ids.contains(&p.id)),
                    "Order must contain every crosshair exactly once"
                );
                let mut next = self.state.clone();
                next.presets
                    .sort_by_key(|p| order.iter().position(|id| *id == p.id).unwrap());
                self.commit(next)?;
            }
            Command::SetFavorite { preset, favorite } => {
                self.preset(preset)?;
                let mut next = self.state.clone();
                let preset = next.preset_mut(preset).unwrap();
                preset.favorite = favorite;
                preset.revision = preset
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("Document revision exhausted"))?;
                self.commit(next)?;
            }
            Command::UpsertProfile {
                profile,
                expected_revision,
            } => {
                self.expect_revision(expected_revision)?;
                let mut next = self.state.clone();
                if let Some(stored) = next.profiles.iter_mut().find(|p| p.id == profile.id) {
                    *stored = profile;
                } else {
                    next.profiles.push(profile);
                }
                validate_state(&next)?;
                self.commit(next)?;
            }
            Command::SelectProfile { profile } => {
                ensure!(
                    self.state.profiles.iter().any(|p| p.id == profile),
                    "Profile does not exist"
                );
                self.reset_input();
                self.state.active_profile = profile;
                self.browse_preset = None;
                self.bump()?;
            }
            Command::SetBindings {
                profiles,
                global_toggle,
                expected_revision,
            } => {
                self.expect_revision(expected_revision)?;
                let mut next = self.state.clone();
                let mut seen = BTreeSet::new();
                for set in profiles {
                    ensure!(
                        seen.insert(set.profile),
                        "Profile shortcuts were supplied twice"
                    );
                    next.profiles
                        .iter_mut()
                        .find(|p| p.id == set.profile)
                        .ok_or_else(|| anyhow::anyhow!("Profile does not exist"))?
                        .bindings = set.bindings;
                }
                next.settings.global_toggle = global_toggle;
                self.commit(next)?;
            }
            Command::DeleteProfile {
                profile,
                replacement,
            } => {
                ensure!(
                    profile != replacement
                        && self.state.profiles.iter().any(|p| p.id == profile)
                        && self.state.profiles.iter().any(|p| p.id == replacement),
                    "Choose a remaining profile"
                );
                let mut next = self.state.clone();
                next.profiles.retain(|p| p.id != profile);
                if next.active_profile == profile {
                    next.active_profile = replacement;
                }
                self.commit(next)?;
            }
            Command::SetSettings {
                settings,
                expected_revision,
            } => {
                self.expect_revision(expected_revision)?;
                ensure!(
                    settings.global_toggle == self.state.settings.global_toggle,
                    "Change visibility shortcuts through the keybinding editor"
                );
                let mut next = self.state.clone();
                next.settings = settings;
                validate_state(&next)?;
                // Native startup changes are applied by the caller before this mutation.
                self.state = next;
                self.reset_input();
                self.bump()?;
            }
            Command::BeginCapture { token } => {
                ensure!(
                    self.capture_owner.is_none(),
                    "Another shortcut capture is active"
                );
                self.input.begin_capture(self.elapsed_ms());
                self.capture_owner = Some((owner, token));
            }
            Command::CancelCapture { token } => {
                ensure!(
                    self.capture_owner.is_none()
                        || self
                            .capture_owner
                            .is_some_and(|(client, _)| client == owner),
                    "This connection does not own shortcut capture"
                );
                if self.capture_owner == Some((owner, token))
                    && let Some(outcome) = self.input.cancel_capture()
                {
                    self.captured(outcome);
                }
            }
            _ => bail!("This command requires the native application host"),
        }
        Ok(Reply::Ack {
            document_revision: None,
        })
    }

    fn preset(&self, id: Id) -> Result<&Preset> {
        self.state
            .preset(id)
            .ok_or_else(|| anyhow::anyhow!("Crosshair does not exist"))
    }

    pub fn expect_revision(&self, revision: u64) -> Result<()> {
        ensure!(
            revision == self.state.revision,
            "Settings changed; reload before applying this edit"
        );
        Ok(())
    }

    pub fn bump(&mut self) -> Result<()> {
        self.state.revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Settings revision exhausted"))?;
        Ok(())
    }

    pub fn commit(&mut self, mut next: AppState) -> Result<()> {
        next.revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Settings revision exhausted"))?;
        for preset in &mut next.presets {
            if self.state.preset(preset.id).is_none_or(|previous| {
                previous.name != preset.name || previous.layers != preset.layers
            }) {
                preset.modified_order = next.revision;
            }
        }
        validate_state(&next)?;
        self.history.record(&self.state, &next)?;
        self.replace_state(next);
        self.reset_input();
        Ok(())
    }

    fn replace_preset(
        &mut self,
        owner: Id,
        preset: Preset,
        expected_revision: u64,
    ) -> Result<Reply> {
        ensure!(
            self.edit_owner.is_none() || self.edit_owner == Some(owner),
            "Another connection owns this edit"
        );
        if self.preset(preset.id)?.revision != expected_revision {
            return Err(HistoryError::Conflict.into());
        }
        let single = self.edit_owner.is_none();
        if single {
            self.history
                .begin(&self.state, preset.id, expected_revision)?;
        }
        let result = self
            .history
            .update(&mut self.state, preset, expected_revision);
        if single {
            self.history.end(&self.state)?;
        }
        Ok(Reply::Ack {
            document_revision: Some(result?),
        })
    }
}
