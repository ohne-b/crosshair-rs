use super::*;

#[derive(Clone, Copy)]
pub(super) enum SaveNext {
    None,
    Quit { tray: bool },
    Install,
}

#[derive(Clone)]
pub(super) enum After {
    Reply,
    Add,
    Replace { expected_revision: u64 },
    Preview { expected_revision: u64 },
    State,
    Staged,
    Save(SaveNext),
    Restore,
    Commit,
    Ignore,
    Shutdown,
}

#[derive(Clone)]
pub(super) struct Pending {
    pub owner: Option<Id>,
    pub request: u64,
    pub base_revision: u64,
    pub after: After,
    pub document: Option<Id>,
    pub structure: bool,
    pub preview_generation: u64,
}

impl Pending {
    pub fn internal(after: After, revision: u64) -> Self {
        Self {
            owner: None,
            request: 0,
            base_revision: revision,
            after,
            document: None,
            structure: false,
            preview_generation: 0,
        }
    }
    pub fn changes_configuration(&self) -> bool {
        matches!(
            self.after,
            After::Add
                | After::Replace { .. }
                | After::State
                | After::Staged
                | After::Restore
                | After::Commit
        )
    }
}

impl Runtime {
    pub(super) fn queue(&mut self, job: Job, pending: Pending) -> Result<()> {
        let ticket = self.next_ticket;
        self.next_ticket = ticket
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Request counter exhausted"))?;
        self.work.try_submit(ticket, job)?;
        self.pending.insert(ticket, pending);
        Ok(())
    }

    pub(super) fn request(&mut self, owner: Id, request: Request) {
        if !self.app.clients.contains_key(&owner) {
            return;
        }
        if request.protocol != PROTOCOL_VERSION || request.version != PRODUCT_VERSION {
            self.respond(
                owner,
                request.id,
                Err(ServiceError {
                    code: ErrorCode::VersionMismatch,
                    message: "The editor and engine versions differ. Restart the application"
                        .into(),
                }),
            );
            return;
        }
        let revision = self.app.state.revision;
        if matches!(
            request.command,
            Command::PreviewPreset { .. } | Command::ClearPreview | Command::CommitStaging { .. }
        ) {
            self.preview_generation = self.preview_generation.wrapping_add(1);
        }
        let document = match &request.command {
            Command::ReplacePreset { preset, .. } => Some(preset.id),
            _ => None,
        };
        let structure = !matches!(
            request.command,
            Command::SelectPreset { .. }
                | Command::ToggleVisibility
                | Command::SetVisibility { .. }
                | Command::ReplacePreset { .. }
        );
        let pending = Pending {
            owner: Some(owner),
            request: request.id,
            base_revision: revision,
            after: After::Reply,
            document,
            structure,
            preview_generation: self.preview_generation,
        };
        match self.dispatch(owner, request.command, pending) {
            Ok(Some(reply)) => self.respond(owner, request.id, Ok(reply)),
            Ok(None) => {}
            Err(error) => self.respond(owner, request.id, Err(service_error(error))),
        }
        if revision != self.app.state.revision {
            self.app.dirty();
            self.app.changed(document, structure);
        }
        self.refresh();
    }

    fn dispatch(
        &mut self,
        owner: Id,
        command: Command,
        mut pending: Pending,
    ) -> Result<Option<Reply>> {
        ensure!(
            !self.frozen || read_only(&command),
            "The application is finishing a data or update operation"
        );
        ensure!(
            (self.change.is_none() && !self.pending.values().any(Pending::changes_configuration))
                || read_only(&command)
                || matches!(
                    command,
                    Command::SelectPreset { .. }
                        | Command::SelectProfile { .. }
                        | Command::ToggleVisibility
                        | Command::SetVisibility { .. }
                        | Command::BeginCapture { .. }
                        | Command::CancelCapture { .. }
                ),
            "A document operation is still running"
        );
        if self.app.status.recovery_required {
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
                        | Command::OpenDataDirectory
                        | Command::OpenEditor
                        | Command::Quit {
                            discard_unsaved: true
                        }
                ),
                "Restore the settings backup or explicitly reset before making changes"
            );
        }
        match command {
            Command::ClearPreview => {
                if self
                    .preview
                    .as_ref()
                    .is_some_and(|preview| preview.owner == owner)
                {
                    self.preview = None;
                }
                return Ok(Some(Reply::Ack {
                    document_revision: None,
                }));
            }
            Command::PreviewPreset {
                preset,
                expected_revision,
            } => {
                ensure!(
                    self.app
                        .clients
                        .get(&owner)
                        .is_some_and(|client| client.subscribed),
                    "Subscribe before previewing a design"
                );
                if self
                    .app
                    .state
                    .preset(preset.id)
                    .ok_or(HistoryError::Missing)?
                    .revision
                    != expected_revision
                {
                    return Err(HistoryError::Conflict.into());
                }
                pending.after = After::Preview { expected_revision };
                pending.structure = false;
                self.queue(
                    Job::ValidatePreset {
                        state: Box::new(self.app.state.clone()),
                        preset: Box::new(preset),
                    },
                    pending,
                )?;
                return Ok(None);
            }
            Command::Subscribe => {
                if self
                    .app
                    .clients
                    .values()
                    .any(|c| c.subscribed && c.client.id != owner)
                {
                    self.app.broadcast(ServerMessage::FocusEditor);
                    return Err(HistoryError::Editing.into());
                }
                let reply = self.app.command(owner, Command::Subscribe)?;
                if self.change.is_none()
                    && self.app.state.settings.check_updates
                    && self.options.test_data.is_none()
                    && self.updates.status().phase == UpdatePhase::Idle
                {
                    let _ = self
                        .updates
                        .check(self.app.state.settings.allow_prereleases);
                }
                return Ok(Some(reply));
            }
            Command::AddPreset { preset } => {
                pending.after = After::Add;
                self.queue(
                    Job::ValidatePreset {
                        state: Box::new(self.app.state.clone()),
                        preset: Box::new(preset),
                    },
                    pending,
                )?;
                return Ok(None);
            }
            Command::ReplacePreset {
                preset,
                expected_revision,
            } => {
                let stored = self
                    .app
                    .state
                    .preset(preset.id)
                    .ok_or(HistoryError::Missing)?;
                if stored.revision != expected_revision {
                    return Err(HistoryError::Conflict.into());
                }
                ensure!(
                    self.app.edit_owner.is_none() || self.app.edit_owner == Some(owner),
                    "Another connection owns this edit"
                );
                pending.after = After::Replace { expected_revision };
                self.queue(
                    Job::ValidatePreset {
                        state: Box::new(self.app.state.clone()),
                        preset: Box::new(preset),
                    },
                    pending,
                )?;
                return Ok(None);
            }
            Command::UpsertProfile {
                profile,
                expected_revision,
            } => {
                self.app.expect_revision(expected_revision)?;
                ensure!(
                    self.app.history.editing().is_none(),
                    "Finish the current edit before changing profiles"
                );
                let mut candidate = self.app.state.clone();
                if let Some(stored) = candidate.profiles.iter_mut().find(|p| p.id == profile.id) {
                    *stored = profile;
                } else {
                    candidate.profiles.push(profile);
                }
                validate_state(&candidate)?;
                pending.after = After::State;
                self.queue(
                    Job::ValidateState {
                        state: Box::new(candidate),
                    },
                    pending,
                )?;
                return Ok(None);
            }
            Command::BeginStaging { purpose } => {
                self.queue(Job::BeginStage { owner, purpose }, pending)?;
                return Ok(None);
            }
            Command::CommitStaging { token } => {
                pending.after = After::Staged;
                self.queue(
                    Job::CommitStage {
                        owner,
                        token,
                        state: Box::new(self.app.state.clone()),
                    },
                    pending,
                )?;
                return Ok(None);
            }
            Command::DiscardStaging { token } => {
                self.queue(Job::DiscardStage { owner, token }, pending)?;
                return Ok(None);
            }
            Command::ReadAsset { asset, offset } => {
                self.queue(
                    Job::ReadAssetChunk {
                        state: Box::new(self.app.state.clone()),
                        asset,
                        offset,
                    },
                    pending,
                )?;
                return Ok(None);
            }
            Command::Export { kind, destination } => {
                self.queue(
                    Job::Export {
                        state: Box::new(self.app.state.clone()),
                        kind,
                        destination: destination.into(),
                    },
                    pending,
                )?;
                return Ok(None);
            }
            Command::Flush => {
                self.save(Some(pending), SaveNext::None)?;
                return Ok(None);
            }
            Command::RecoverBackup => {
                pending.after = After::Restore;
                self.queue(Job::Recover, pending)?;
                self.frozen = true;
                self.app.reset_input();
                return Ok(None);
            }
            Command::ResetSettings => {
                pending.after = After::Restore;
                self.queue(
                    Job::Reset {
                        state: Box::new(self.app.state.clone()),
                    },
                    pending,
                )?;
                self.frozen = true;
                self.app.reset_input();
                return Ok(None);
            }
            Command::Quit { discard_unsaved } => {
                if discard_unsaved {
                    self.begin_shutdown();
                } else {
                    self.request_close(SaveNext::Quit { tray: false })?;
                }
            }
            Command::InstallUpdate => {
                self.request_close(SaveNext::Install)?;
            }
            Command::ConfirmQuit => {
                ensure!(
                    self.app.clients.get(&owner).is_some_and(|c| c.subscribed),
                    "Only the active editor can finish closing"
                );
                let next = self
                    .close_request
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("The close request has expired"))?;
                unsafe {
                    let _ = KillTimer(Some(self.window.0), CLOSE_TIMER);
                }
                if let Err(error) = self.save(Some(pending), next) {
                    self.app.broadcast(ServerMessage::CloseCancelled);
                    return Err(error);
                }
                return Ok(None);
            }
            Command::CancelQuit => {
                ensure!(
                    self.app.clients.get(&owner).is_some_and(|c| c.subscribed),
                    "Only the active editor can cancel closing"
                );
                ensure!(
                    self.close_request.take().is_some(),
                    "There is no pending close request"
                );
                unsafe {
                    let _ = KillTimer(Some(self.window.0), CLOSE_TIMER);
                }
                self.app.broadcast(ServerMessage::CloseCancelled);
            }
            Command::CheckUpdate => {
                ensure!(
                    self.change.is_none(),
                    "Wait for the current settings operation to finish"
                );
                self.updates
                    .check(self.app.state.settings.allow_prereleases)?;
            }
            Command::DownloadUpdate => self.updates.download()?,
            Command::CancelUpdate => self.updates.cancel(),
            Command::OpenEditor => self.open_editor()?,
            Command::OpenDataDirectory => {
                return Ok(Some(lifecycle::open(
                    &self.root.to_string_lossy(),
                    self.window.0,
                )?));
            }
            Command::OpenGameBar => {
                return Ok(Some(lifecycle::open("ms-gamebar:", self.window.0)?));
            }
            Command::SetSettings {
                settings,
                expected_revision,
            } => {
                self.app.expect_revision(expected_revision)?;
                ensure!(
                    settings.global_toggle == self.app.state.settings.global_toggle,
                    "Change visibility shortcuts through Keybinds"
                );
                let mut candidate = self.app.state.clone();
                candidate.settings = settings.clone();
                validate_state(&candidate)?;
                self.begin_change(candidate, pending, output::ChangeKind::Settings)?;
                return Ok(None);
            }
            command => return Ok(Some(self.app.command(owner, command)?)),
        }
        Ok(Some(Reply::Ack {
            document_revision: None,
        }))
    }

    pub(super) fn save(&mut self, context: Option<Pending>, next: SaveNext) -> Result<()> {
        ensure!(
            !self.app.status.recovery_required,
            "Settings need recovery before saving"
        );
        ensure!(
            self.change.is_none(),
            "Wait for the current settings operation to finish"
        );
        if context.is_none()
            && matches!(next, SaveNext::None)
            && self
                .pending
                .values()
                .any(|p| matches!(p.after, After::Save(_)))
        {
            return Ok(());
        }
        ensure!(
            matches!(next, SaveNext::None)
                || (self.change.is_none()
                    && !self.pending.values().any(Pending::changes_configuration)),
            "Wait for the current document operation to finish"
        );
        let mut pending = context
            .unwrap_or_else(|| Pending::internal(After::Save(next), self.app.state.revision));
        pending.after = After::Save(next);
        self.queue(
            Job::Save {
                state: Box::new(self.app.state.clone()),
                history_assets: self.app.history.retained_asset_ids(),
            },
            pending,
        )?;
        self.app.save_at = None;
        if !matches!(next, SaveNext::None) {
            self.frozen = true;
            self.app.reset_input();
        }
        Ok(())
    }

    pub(super) fn request_close(&mut self, next: SaveNext) -> Result<()> {
        ensure!(
            self.close_request.is_none() && !self.frozen,
            "A close operation is already running"
        );
        if self.app.clients.values().any(|c| c.subscribed) {
            self.close_request = Some(next);
            self.app.broadcast(ServerMessage::PrepareClose {
                updating: matches!(next, SaveNext::Install),
            });
            unsafe {
                SetTimer(Some(self.window.0), CLOSE_TIMER, 120_000, None);
            }
            Ok(())
        } else {
            self.save(None, next)
        }
    }

    pub(super) fn begin_shutdown(&mut self) {
        self.frozen = true;
        self.shutdown_pending = true;
        self.app.reset_input();
        self.try_shutdown();
    }

    pub(super) fn try_shutdown(&mut self) {
        if self.shutdown_pending
            && self
                .queue(
                    Job::Shutdown,
                    Pending::internal(After::Shutdown, self.app.state.revision),
                )
                .is_ok()
        {
            self.shutdown_pending = false;
        }
    }

    pub(super) fn respond(
        &self,
        owner: Id,
        id: u64,
        result: std::result::Result<Reply, ServiceError>,
    ) {
        if let Some(connection) = self.app.clients.get(&owner) {
            connection.client.send(ServerMessage::Response(Response {
                id,
                revision: self.app.state.revision,
                result,
            }));
        }
    }
}

fn read_only(command: &Command) -> bool {
    matches!(
        command,
        Command::Subscribe
            | Command::Snapshot
            | Command::ReadSnapshot { .. }
            | Command::GetPreset { .. }
            | Command::ReadAsset { .. }
            | Command::OpenEditor
            | Command::OpenDataDirectory
            | Command::OpenGameBar
            | Command::Export { .. }
            | Command::CheckUpdate
            | Command::CancelUpdate
    )
}

pub(super) fn service_error(error: anyhow::Error) -> ServiceError {
    let code = match error.downcast_ref::<HistoryError>() {
        Some(HistoryError::Conflict) => ErrorCode::Conflict,
        Some(HistoryError::Editing) => ErrorCode::Busy,
        _ => {
            if error.to_string().contains("changed;") {
                ErrorCode::Conflict
            } else if error.to_string().contains("running") || error.to_string().contains("busy") {
                ErrorCode::Busy
            } else {
                ErrorCode::Invalid
            }
        }
    };
    ServiceError {
        code,
        message: format!("{error:#}"),
    }
}
