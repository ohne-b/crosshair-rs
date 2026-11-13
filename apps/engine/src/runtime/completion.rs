use super::*;
use crate::storage::Staged;

impl Runtime {
    pub(super) fn completed(&mut self, completed: work::Completed) {
        let Some(pending) = self.pending.remove(&completed.ticket) else {
            return;
        };
        let revision = self.app.state.revision;
        let disconnected = pending
            .owner
            .is_some_and(|id| !self.app.clients.contains_key(&id));
        if disconnected
            && matches!(
                pending.after,
                After::Add
                    | After::Replace { .. }
                    | After::Preview { .. }
                    | After::State
                    | After::Staged
                    | After::Restore
            )
        {
            if matches!(pending.after, After::Restore) {
                self.frozen = false;
            }
            self.refresh();
            self.try_shutdown();
            return;
        }
        let result = self.apply_work(&pending, completed.result);
        match (pending.owner, result) {
            (Some(owner), Ok(Some(reply))) => self.respond(owner, pending.request, Ok(reply)),
            (Some(owner), Err(error)) => {
                self.respond(owner, pending.request, Err(commands::service_error(error)))
            }
            (None, Err(error)) if matches!(pending.after, After::Save(_)) => {
                self.app.save_failed(format!("{error:#}"))
            }
            _ => {}
        }
        if revision != self.app.state.revision && !matches!(pending.after, After::Commit) {
            self.app.dirty();
        }
        if !matches!(pending.after, After::Preview { .. }) {
            self.app.changed(pending.document, pending.structure);
        }
        self.refresh();
        self.try_shutdown();
    }

    fn apply_work(
        &mut self,
        pending: &Pending,
        result: Result<WorkReply>,
    ) -> Result<Option<Reply>> {
        ensure!(
            self.app.state.revision >= pending.base_revision,
            "The state revision changed unexpectedly"
        );
        let reply = match result {
            Ok(reply) => reply,
            Err(error) => {
                match pending.after {
                    After::Save(next) => {
                        self.app.save_failed(format!("{error:#}"));
                        self.frozen = false;
                        if !matches!(next, SaveNext::None) {
                            self.app.broadcast(ServerMessage::CloseCancelled);
                        }
                        if matches!(next, SaveNext::Quit { tray: true }) {
                            self.quit_failure(&error.to_string());
                        }
                    }
                    After::Restore => self.frozen = false,
                    After::Commit => self.cancel_change(&format!("{error:#}"), false),
                    _ => {}
                }
                return Err(error);
            }
        };
        match reply {
            WorkReply::Saved {
                revision,
                cleanup_error,
            } => {
                self.app.saved(revision);
                if let Some(error) = cleanup_error {
                    eprintln!("Unused image cleanup was deferred: {error}");
                }
                if let After::Save(next) = pending.after {
                    match next {
                        SaveNext::None => {}
                        SaveNext::Quit { .. } => self.begin_shutdown(),
                        SaveNext::Install => {
                            let updates = self.updates.clone();
                            let wake = self.wake.clone();
                            let restart = updates::RestartIntent {
                                visible: self.app.status().visible,
                                reopen_editor: self.app.clients.values().any(|c| c.subscribed),
                            };
                            if let Err(error) = std::thread::Builder::new()
                                .name("update handoff".into())
                                .spawn(move || {
                                    let result = updates
                                        .prepare_install(restart)
                                        .and_then(updates::launch_handoff)
                                        .map(|_| ());
                                    wake.complete(Event::Installed(result));
                                })
                            {
                                self.frozen = false;
                                return Err(error.into());
                            }
                        }
                    }
                }
            }
            WorkReply::Stage(stage) => {
                return Ok(Some(Reply::Staging {
                    token: stage.token,
                    path: stage.path.to_string_lossy().into(),
                    max_bytes: stage.max_bytes,
                }));
            }
            WorkReply::Chunk { bytes, complete } => {
                return Ok(Some(Reply::Chunk { bytes, complete }));
            }
            WorkReply::Validated {
                preset,
                scale,
                raster,
            } => {
                if matches!(pending.after, After::Add) {
                    return self
                        .app
                        .command(
                            pending.owner.ok_or(HistoryError::Missing)?,
                            Command::AddPreset { preset: *preset },
                        )
                        .map(Some);
                }
                if let After::Preview { expected_revision } = pending.after {
                    if self.preview_generation == pending.preview_generation {
                        let stored = self
                            .app
                            .state
                            .preset(preset.id)
                            .ok_or(HistoryError::Missing)?;
                        if stored.revision != expected_revision {
                            return Err(HistoryError::Conflict.into());
                        }
                        self.preview = Some(LivePreview {
                            owner: pending.owner.ok_or(HistoryError::Missing)?,
                            key: render::Key::new(stored, scale),
                            raster,
                        });
                    }
                    return Ok(Some(Reply::Ack {
                        document_revision: None,
                    }));
                }
                let After::Replace { expected_revision } = pending.after else {
                    anyhow::bail!("Unexpected validation reply");
                };
                let id = preset.id;
                let reply = self.app.command(
                    pending.owner.unwrap(),
                    Command::ReplacePreset {
                        preset: *preset,
                        expected_revision,
                    },
                )?;
                let stored = self.app.state.preset(id).ok_or(HistoryError::Missing)?;
                self.cache.insert(render::Key::new(stored, scale), raster);
                return Ok(Some(reply));
            }
            WorkReply::ValidatedState { state } => {
                let mut candidate = *state;
                preserve_runtime(&self.app.state, &mut candidate);
                self.app.commit(candidate)?;
            }
            WorkReply::Staged(Staged::Image { id, asset }) => {
                let (width, height) = (asset.width, asset.height);
                self.app
                    .history
                    .adopt_asset(&mut self.app.state, id, asset)?;
                return Ok(Some(Reply::Asset { id, width, height }));
            }
            WorkReply::Staged(Staged::Document {
                preset,
                expected_revision,
                preview,
            }) => {
                if preview {
                    let mut pending = pending.clone();
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
                return self
                    .app
                    .command(
                        pending.owner.unwrap(),
                        Command::ReplacePreset {
                            preset,
                            expected_revision,
                        },
                    )
                    .map(Some);
            }
            WorkReply::Staged(Staged::Import { candidate, replace }) => {
                let mut candidate = candidate;
                if !replace {
                    preserve_runtime(&self.app.state, &mut candidate);
                } else {
                    candidate.base_visible = self.app.state.base_visible;
                }
                self.begin_change(candidate, pending.clone(), output::ChangeKind::Import)?;
                return Ok(None);
            }
            WorkReply::Recovered { state } | WorkReply::Reset { state } => {
                self.frozen = false;
                self.begin_change(*state, pending.clone(), output::ChangeKind::Restore)?;
                return Ok(None);
            }
            WorkReply::Committed { state } => self.finish_change(*state)?,
            WorkReply::Stopped => self.stopped = true,
            WorkReply::Discarded { removed: _removed } => {}
            WorkReply::Disconnected { removed: _removed } => {}
            WorkReply::Staged(Staged::Exported) | WorkReply::Exported | WorkReply::Cleared => {}
        }
        Ok(Some(Reply::Ack {
            document_revision: None,
        }))
    }
}

fn preserve_runtime(live: &AppState, candidate: &mut AppState) {
    candidate.base_visible = live.base_visible;
    if candidate
        .profiles
        .iter()
        .any(|p| p.id == live.active_profile)
    {
        candidate.active_profile = live.active_profile;
    }
    for profile in &mut candidate.profiles {
        if let Some(previous) = live.profiles.iter().find(|p| p.id == profile.id)
            && candidate
                .presets
                .iter()
                .any(|p| p.id == previous.selected_preset)
        {
            profile.selected_preset = previous.selected_preset;
        }
    }
}
