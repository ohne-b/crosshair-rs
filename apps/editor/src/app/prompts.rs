use super::*;

impl App {
    pub(super) fn confirm_modal(&mut self, value: &str, alternate: bool) -> Result<()> {
        let modal = std::mem::replace(&mut self.modal, Modal::None);
        self.hide_modal();
        match modal {
            Modal::RenamePreset(id) => {
                self.action("commit-preset-name", &format!("{id}:{value}"))?;
            }
            Modal::RenameLayer(id) => {
                self.document.select(id, false);
                self.property_edit("/name", value, 2)?;
            }
            Modal::NewProfile => {
                let selected = self
                    .state
                    .selected_preset()
                    .context("Select a crosshair")?
                    .id;
                self.profile_edit(Profile::new(value.trim(), selected))?;
            }
            Modal::RenameProfile(id) => {
                let mut profile = self
                    .state
                    .profiles
                    .iter()
                    .find(|profile| profile.id == id)
                    .context("This profile was deleted")?
                    .clone();
                profile.name = value.trim().into();
                self.profile_edit(profile)?;
            }
            Modal::SavePosition => {
                let mut profile = self.active_profile()?;
                profile.positions.push(SavedPosition {
                    id: Id::new_v4(),
                    name: value.trim().into(),
                    offset: profile.offset,
                    scale: profile.scale,
                });
                self.profile_edit(profile)?;
            }
            Modal::DeletePreset(preset, replacement) => self.send(
                "delete",
                Command::DeletePreset {
                    preset,
                    replacement,
                },
            )?,
            Modal::DeleteProfile(profile, replacement) => self.send(
                "delete",
                Command::DeleteProfile {
                    profile,
                    replacement,
                },
            )?,
            Modal::ReplaceData(path) => {
                self.client
                    .stage_file(path, StagingPurpose::Import { replace: true }, "import")?;
            }
            Modal::Reset => self.send("reset", Command::ResetSettings)?,
            Modal::Recovery => self.send(
                "recover",
                if alternate {
                    Command::ResetSettings
                } else {
                    Command::RecoverBackup
                },
            )?,
            Modal::Close => {
                if alternate {
                    self.cancel_document_gesture();
                    self.document.dirty = false;
                    self.document.conflict = false;
                    self.document.save_paused = false;
                    self.preview_pending = false;
                    self.drag = None;
                    self.draw_base = None;
                    self.deferred_ui.clear();
                    self.pending_command = None;
                    self.cancel_group = true;
                    self.finish_group = true;
                    self.closing = true;
                    if !self.window()?.global::<Store>().get_connected() {
                        self.close_now()?;
                    } else if self.group_open {
                        self.finish_edit()?;
                    }
                } else if !self.window()?.global::<Store>().get_connected() {
                    self.client.reconnect()?;
                    self.modal = Modal::Close;
                    self.show_modal(1, "Keep unsent changes?", "Reconnecting to the engine. Retry saving when connected, or keep this editor open.", "Retry", "Discard", "");
                } else {
                    self.closing = true;
                    if self.document.conflict {
                        self.action("retry-edit", "")?;
                    } else {
                        self.save_document()?;
                    }
                }
            }
            Modal::OpenDocument(id) => {
                if alternate {
                    self.document.dirty = false;
                    self.document.conflict = false;
                    self.document.gesture = None;
                    self.drag = None;
                    self.draw_base = None;
                    self.open_document(id)?;
                } else {
                    self.open_after_save = Some(id);
                    self.save_document()?;
                }
            }
            Modal::Install => {
                if alternate {
                    self.document.draft = self.document.saved.clone();
                    self.document.dirty = false;
                    self.document.gesture = None;
                    self.drag = None;
                    self.draw_base = None;
                    self.send("clear-preview", Command::ClearPreview)?;
                    self.send("install-flush", Command::Flush)?;
                } else {
                    self.save_document()?;
                }
            }
            Modal::Conflict(target, chord, conflict) => {
                self.apply_capture(target, Some(chord), Some(conflict))?
            }
            Modal::DraftConflict => {
                if alternate {
                    self.closing = false;
                    if self.shutdown_requested.take().is_some() {
                        self.send("cancel-quit", Command::CancelQuit)?;
                    }
                    self.export_draft(DraftFormat::Preset)?;
                } else {
                    if self.group_open {
                        self.cancel_group = true;
                        self.group_ending = true;
                        self.send("recover-cancel", Command::CancelEdit)?;
                    } else {
                        self.reapply_draft()?;
                    }
                }
            }
            Modal::None | Modal::Menu(_) => {}
        }
        Ok(())
    }
    pub(super) fn reapply_draft(&self) -> Result<()> {
        self.preview.recover(
            self.document
                .draft
                .clone()
                .context("The draft is unavailable")?,
            preview::RecoveryAction::Reapply,
        )
    }
    pub(super) fn export_draft(&self, format: DraftFormat) -> Result<()> {
        let document = self
            .document
            .draft
            .clone()
            .context("Open a crosshair first")?;
        if let Some(path) = dialogs::choose(
            self.hwnd()?,
            true,
            match format {
                DraftFormat::Png => dialogs::FileKind::Png,
                DraftFormat::Svg => dialogs::FileKind::Svg,
                DraftFormat::Preset => dialogs::FileKind::Document,
            },
            match format {
                DraftFormat::Png => "crosshair.png",
                DraftFormat::Svg => "crosshair.svg",
                DraftFormat::Preset => "crosshair-rs.json",
            },
        )? {
            self.preview.recover(
                document,
                preview::RecoveryAction::Export {
                    destination: path,
                    format,
                },
            )?;
        }
        Ok(())
    }
}
