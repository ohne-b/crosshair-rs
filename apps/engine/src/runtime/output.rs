use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ChangeKind {
    Settings,
    Import,
    Restore,
}

enum Phase {
    Prepare,
    Activate(u64),
    Blank(u64),
    Persist,
    Rollback {
        generation: u64,
        error: String,
        respond: bool,
    },
}
struct PreparedFrame {
    key: render::Key,
    origin: [i32; 2],
    visible: bool,
    raster: Arc<crosshair_renderer::Raster>,
    generation: u64,
}

pub(super) struct Change {
    before: AppState,
    candidate: AppState,
    history: Option<History>,
    kind: ChangeKind,
    context: Pending,
    phase: Phase,
    frame: Option<PreparedFrame>,
    old_hidden: bool,
    deadline: Instant,
}

impl Runtime {
    pub(super) fn begin_change(
        &mut self,
        mut candidate: AppState,
        context: Pending,
        kind: ChangeKind,
    ) -> Result<()> {
        ensure!(
            self.change.is_none(),
            "A settings operation is already running"
        );
        ensure!(
            self.app.history.editing().is_none(),
            "Finish the current edit before changing settings"
        );
        candidate.revision = candidate.revision.max(
            self.app
                .state
                .revision
                .checked_add(1)
                .ok_or(HistoryError::RevisionExhausted)?,
        );
        if kind == ChangeKind::Restore {
            for preset in &mut candidate.presets {
                if let Some(live) = self.app.state.preset(preset.id) {
                    if live.name == preset.name
                        && live.favorite == preset.favorite
                        && live.layers == preset.layers
                    {
                        preset.revision = live.revision;
                        preset.modified_order = live.modified_order;
                    } else {
                        preset.revision = preset
                            .revision
                            .max(live.revision)
                            .checked_add(1)
                            .ok_or(HistoryError::RevisionExhausted)?;
                        preset.modified_order = candidate.revision;
                    }
                }
            }
        }
        validate_state(&candidate)?;
        if candidate.settings.allow_prereleases != self.app.state.settings.allow_prereleases {
            self.updates.ensure_idle()?;
        }
        let history = match kind {
            ChangeKind::Settings => None,
            ChangeKind::Import => {
                let mut history = self.app.history.clone();
                history.record(&self.app.state, &candidate)?;
                Some(history)
            }
            ChangeKind::Restore => Some(History::default()),
        };
        if candidate.settings.output_mode == OutputMode::GameBar
            && candidate.settings.output_mode != self.app.state.settings.output_mode
        {
            ensure!(
                self.widget.is_some(),
                "Install the Windows package and pin its Game Bar widget before switching output"
            );
        }
        self.change = Some(Change {
            before: self.app.state.clone(),
            candidate,
            history,
            kind,
            context,
            phase: Phase::Prepare,
            frame: None,
            old_hidden: false,
            deadline: Instant::now() + std::time::Duration::from_secs(15),
        });
        self.frozen = false;
        unsafe {
            SetTimer(Some(self.window.0), OUTPUT_TIMER, 15_000, None);
        }
        Ok(())
    }

    pub(super) fn changing_output(&self) -> bool {
        self.change.as_ref().is_some_and(|change| {
            change.before.settings.output_mode != change.candidate.settings.output_mode
        })
    }

    pub(super) fn previous_output_hidden(&self) -> bool {
        self.change.as_ref().is_some_and(|change| change.old_hidden)
    }

    pub(super) fn change_needs(&self, key: render::Key) -> bool {
        self.change.as_ref().is_some_and(|change| {
            self.app
                .presentation_of(&change.candidate, true)
                .is_some_and(|(wanted, _)| wanted == key)
        })
    }

    pub(super) fn advance_change(&mut self) {
        let Some(mut change) = self.change.take() else {
            return;
        };
        if let Phase::Rollback {
            generation,
            ref error,
            respond,
        } = change.phase
        {
            if blank(&self.widget_status, generation) {
                let error = error.clone();
                self.finish_cancel(change, &error, respond);
            } else {
                self.change = Some(change);
            }
            return;
        }
        let result = self.advance(&mut change);
        let persisting = matches!(change.phase, Phase::Persist);
        self.change = Some(change);
        if let Err(error) = result {
            if persisting {
                self.app.status.render_error = Some(format!("{error:#}"));
            } else {
                self.cancel_change(&format!("{error:#}"), true);
            }
        }
    }

    fn advance(&mut self, change: &mut Change) -> Result<()> {
        merge_input(&mut change.before, &self.app.state, &mut change.candidate);
        if matches!(change.phase, Phase::Persist) {
            return self.present_committing(change);
        }
        if change.before.settings.output_mode == change.candidate.settings.output_mode {
            return self.persist_change(change);
        }
        ensure!(
            !self.suspended,
            "Unlock or wake Windows before changing output"
        );
        let (key, center) = self
            .app
            .presentation_of(&change.candidate, true)
            .ok_or_else(|| anyhow::anyhow!("No display is available for the selected output"))?;
        let visible = self
            .app
            .input
            .presentation(&change.candidate)
            .is_some_and(|p| p.visible);
        let Some(raster) = self.cache.get(key) else {
            ensure!(
                !self.failed_render.contains(&key),
                "The destination crosshair could not be rendered"
            );
            if self.rendering != Some(key) {
                let preset = change
                    .candidate
                    .preset(key.preset)
                    .ok_or(HistoryError::Missing)?;
                self.render.submit(render::Job {
                    key,
                    preset: preset.clone(),
                    assets: change.candidate.assets.clone(),
                });
                self.rendering = Some(key);
            }
            return Ok(());
        };
        let origin = [center[0] + raster.origin[0], center[1] + raster.origin[1]];
        let changed = change.frame.as_ref().is_none_or(|frame| {
            frame.key != key || frame.origin != origin || frame.visible != visible
        });
        if changed {
            change.phase = Phase::Prepare;
            let generation = if change.candidate.settings.output_mode == OutputMode::GameBar {
                self.widget
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Game Bar output is unavailable"))?
                    .prepare(raster.clone(), origin)
            } else {
                self.overlay
                    .prepare_at([raster.width, raster.height], &raster.pixels, origin)?;
                0
            };
            change.frame = Some(PreparedFrame {
                key,
                origin,
                visible,
                raster,
                generation,
            });
        }
        let frame = change
            .frame
            .as_ref()
            .expect("destination frame was prepared");
        match change.candidate.settings.output_mode {
            OutputMode::GameBar => {
                let status = &self.widget_status;
                match change.phase {
                    Phase::Prepare if ready(status, frame.generation, false) => {
                        self.overlay.hide();
                        self.last_image = None;
                        change.old_hidden = true;
                        if frame.visible {
                            let generation = self.widget.as_ref().unwrap().present(
                                Some(frame.raster.clone()),
                                frame.origin,
                                false,
                            );
                            change.phase = Phase::Activate(generation);
                        } else {
                            self.persist_change(change)?;
                        }
                    }
                    Phase::Activate(generation) if ready(status, generation, true) => {
                        self.persist_change(change)?
                    }
                    _ => {}
                }
            }
            OutputMode::Desktop => match change.phase {
                Phase::Prepare => {
                    let generation = self
                        .widget
                        .as_ref()
                        .map_or(0, |widget| widget.present(None, [0, 0], false));
                    change.old_hidden = true;
                    change.phase = Phase::Blank(generation);
                    if self
                        .widget
                        .as_ref()
                        .is_none_or(|widget| !widget.ever_connected())
                        || blank(&self.widget_status, generation)
                    {
                        self.activate_desktop(change)?;
                    }
                }
                Phase::Blank(generation) if blank(&self.widget_status, generation) => {
                    self.activate_desktop(change)?
                }
                _ => {}
            },
        }
        Ok(())
    }

    fn activate_desktop(&mut self, change: &mut Change) -> Result<()> {
        let frame = change.frame.as_ref().expect("desktop frame was prepared");
        if frame.visible {
            self.overlay.show();
            self.last_image = Some((frame.raster.clone(), frame.origin));
        }
        self.persist_change(change)
    }

    fn persist_change(&mut self, change: &mut Change) -> Result<()> {
        let mut context = change.context.clone();
        context.after = After::Commit;
        change.candidate.revision = change.candidate.revision.max(
            self.app
                .state
                .revision
                .checked_add(1)
                .ok_or(HistoryError::RevisionExhausted)?,
        );
        self.queue(
            Job::CommitState {
                state: Box::new(change.candidate.clone()),
                restore: change.kind == ChangeKind::Restore,
                startup_changed: change.candidate.settings.launch_at_login
                    != change.before.settings.launch_at_login,
            },
            context,
        )?;
        change.phase = Phase::Persist;
        unsafe {
            let _ = KillTimer(Some(self.window.0), OUTPUT_TIMER);
        }
        Ok(())
    }

    fn present_committing(&mut self, change: &Change) -> Result<()> {
        if !change.old_hidden {
            return Ok(());
        }
        if self.suspended {
            self.hide();
            return Ok(());
        }
        let Some((key, center)) = self.app.presentation_of(&change.candidate, false) else {
            self.hide();
            return Ok(());
        };
        let Some(raster) = self.cache.get(key) else {
            if self.rendering != Some(key) && !self.failed_render.contains(&key) {
                let preset = change
                    .candidate
                    .preset(key.preset)
                    .ok_or(HistoryError::Missing)?;
                self.render.submit(render::Job {
                    key,
                    preset: preset.clone(),
                    assets: change.candidate.assets.clone(),
                });
                self.rendering = Some(key);
            }
            return Ok(());
        };
        let origin = [center[0] + raster.origin[0], center[1] + raster.origin[1]];
        if change.candidate.settings.output_mode == OutputMode::GameBar {
            if let Some(widget) = &self.widget {
                widget.present(Some(raster), origin, false);
            }
        } else if self
            .last_image
            .as_ref()
            .is_none_or(|(old, position)| !Arc::ptr_eq(old, &raster) || *position != origin)
        {
            self.overlay
                .present_at([raster.width, raster.height], &raster.pixels, origin)?;
            self.last_image = Some((raster, origin));
        }
        Ok(())
    }

    pub(super) fn finish_change(&mut self, state: AppState) -> Result<()> {
        let mut change = self
            .change
            .take()
            .ok_or_else(|| anyhow::anyhow!("The settings operation was cancelled"))?;
        let saved = state.revision;
        merge_input(&mut change.before, &self.app.state, &mut change.candidate);
        let mut state = change.candidate;
        state.revision = state.revision.max(
            self.app
                .state
                .revision
                .checked_add(1)
                .ok_or(HistoryError::RevisionExhausted)?,
        );
        self.app.state = state;
        if change.before.settings.allow_prereleases != self.app.state.settings.allow_prereleases {
            self.updates.reset_offer();
        }
        if let Some(history) = change.history {
            self.app.history = history;
        }
        if !self.app.clients.values().any(|client| client.subscribed) {
            self.app.history.clear();
        }
        self.app.reset_input();
        if change.kind == ChangeKind::Restore {
            self.app.edit_owner = None;
        }
        self.app.status.recovery_required = false;
        self.app.status.render_error = None;
        if change.before.settings.output_mode != self.app.state.settings.output_mode {
            self.widget_clear_required = None;
        }
        self.app.saved(saved);
        self.frozen = false;
        self.widget_center = true;
        Ok(())
    }

    pub(super) fn cancel_change(&mut self, error: &str, respond: bool) {
        let Some(mut change) = self.change.take() else {
            return;
        };
        unsafe {
            let _ = KillTimer(Some(self.window.0), OUTPUT_TIMER);
        }
        if matches!(change.phase, Phase::Rollback { .. }) {
            if let Phase::Rollback {
                generation,
                error,
                respond,
            } = &change.phase
            {
                let error = error.clone();
                let respond = *respond;
                self.widget_clear_required = Some(*generation);
                if let Some(widget) = &self.widget {
                    widget.disconnect();
                }
                self.finish_cancel(change, &error, respond);
            }
            return;
        }
        if change.candidate.settings.output_mode == OutputMode::GameBar
            && change.before.settings.output_mode != change.candidate.settings.output_mode
            && let Some(widget) = &self.widget
        {
            let generation = widget.present(None, [0, 0], false);
            if change.old_hidden && !blank(&self.widget_status, generation) {
                change.phase = Phase::Rollback {
                    generation,
                    error: error.into(),
                    respond,
                };
                change.deadline = Instant::now() + std::time::Duration::from_secs(3);
                self.change = Some(change);
                unsafe {
                    SetTimer(Some(self.window.0), OUTPUT_TIMER, 3_000, None);
                }
                return;
            }
        }
        self.finish_cancel(change, error, respond);
    }

    fn finish_cancel(&mut self, change: Change, error: &str, respond: bool) {
        unsafe {
            let _ = KillTimer(Some(self.window.0), OUTPUT_TIMER);
        }
        if change.before.settings.output_mode == OutputMode::GameBar {
            self.overlay.hide();
            self.last_image = None;
        }
        self.widget_center = true;
        self.frozen = false;
        self.app.status.render_error = Some(error.into());
        if respond && let Some(owner) = change.context.owner {
            self.respond(
                owner,
                change.context.request,
                Err(commands::service_error(anyhow::anyhow!(error.to_owned()))),
            );
        }
        self.app.changed(None, false);
        self.refresh();
    }

    pub(super) fn change_owner_disconnected(&mut self, owner: Id) {
        if self.change.as_ref().is_some_and(|change| {
            change.context.owner == Some(owner) && !matches!(change.phase, Phase::Persist)
        }) {
            self.cancel_change(
                "The editor disconnected before its settings were applied",
                false,
            );
        }
    }

    pub(super) fn invalidate_output_geometry(&mut self) {
        if let Some(change) = &mut self.change
            && !matches!(change.phase, Phase::Persist | Phase::Rollback { .. })
        {
            change.frame = None;
        }
    }

    pub(super) fn output_timeout(&mut self) {
        let Some(change) = &self.change else {
            return;
        };
        let Some(remaining) = timeout_remaining(&change.phase, change.deadline, Instant::now())
        else {
            return;
        };
        if !remaining.is_zero() {
            unsafe {
                SetTimer(
                    Some(self.window.0),
                    OUTPUT_TIMER,
                    remaining.as_millis().clamp(1, 15_000) as u32,
                    None,
                );
            }
            return;
        }
        self.cancel_change("The output did not become ready. Pin the widget, enable click-through and check its placement, then retry", true);
        self.refresh();
    }

    pub(super) fn acknowledge_widget_blank(&mut self) {
        if self
            .widget_clear_required
            .is_some_and(|generation| blank(&self.widget_status, generation))
        {
            self.widget_clear_required = None;
        }
    }
}

fn ready(status: &widget::Status, generation: u64, presented: bool) -> bool {
    status.generation == generation
        && status.prepared
        && status.presented == presented
        && status.widget.connected
        && status.widget.pinned
        && status.widget.click_through
        && status.widget.positioned
        && (!presented || status.widget.visible)
}

fn blank(status: &widget::Status, generation: u64) -> bool {
    status.widget.connected
        && status.generation == generation
        && !status.presented
        && !status.prepared
}

fn timeout_remaining(
    phase: &Phase,
    deadline: Instant,
    now: Instant,
) -> Option<std::time::Duration> {
    if matches!(phase, Phase::Persist) {
        None
    } else {
        Some(deadline.saturating_duration_since(now))
    }
}

fn merge_input(before: &mut AppState, live: &AppState, candidate: &mut AppState) {
    if live.base_visible != before.base_visible {
        candidate.base_visible = live.base_visible;
        before.base_visible = live.base_visible;
    }
    if live.active_profile != before.active_profile {
        if candidate
            .profiles
            .iter()
            .any(|p| p.id == live.active_profile)
        {
            candidate.active_profile = live.active_profile;
        }
        before.active_profile = live.active_profile;
    }
    for profile in &mut candidate.profiles {
        if let (Some(old), Some(now)) = (
            before.profiles.iter_mut().find(|p| p.id == profile.id),
            live.profiles.iter().find(|p| p.id == profile.id),
        ) && old.selected_preset != now.selected_preset
        {
            if candidate
                .presets
                .iter()
                .any(|p| p.id == now.selected_preset)
            {
                profile.selected_preset = now.selected_preset;
            }
            old.selected_preset = now.selected_preset;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_timers_cannot_cancel_a_durable_commit_or_a_newer_preparation() {
        let now = Instant::now();
        assert!(
            timeout_remaining(
                &Phase::Persist,
                now,
                now + std::time::Duration::from_secs(30)
            )
            .is_none()
        );
        assert_eq!(
            timeout_remaining(
                &Phase::Prepare,
                now + std::time::Duration::from_secs(15),
                now
            ),
            Some(std::time::Duration::from_secs(15))
        );
        assert_eq!(
            timeout_remaining(&Phase::Prepare, now, now),
            Some(std::time::Duration::ZERO)
        );
    }
    #[test]
    fn a_handoff_requires_the_exact_prepared_and_presented_generations() {
        let mut status = widget::Status {
            generation: 9,
            prepared: true,
            presented: false,
            widget: WidgetStatus {
                connected: true,
                pinned: true,
                click_through: true,
                positioned: true,
                visible: true,
                ..Default::default()
            },
        };
        assert!(ready(&status, 9, false));
        assert!(!ready(&status, 8, false));
        assert!(!ready(&status, 9, true));
        status.generation = 10;
        status.presented = true;
        assert!(ready(&status, 10, true));
        status.widget.positioned = false;
        assert!(!ready(&status, 10, true));
        status.widget.positioned = true;
        status.widget.click_through = false;
        assert!(!ready(&status, 10, true));
        status.presented = false;
        status.prepared = false;
        assert!(blank(&status, 10));
        status.widget.connected = false;
        assert!(!blank(&status, 10));
    }
    #[test]
    fn input_during_preparation_survives_without_overwriting_restored_selection() {
        let mut before = AppState::default();
        let original = before.clone();
        let mut restored = before.clone();
        restored.profiles[0].selected_preset = restored.presets[1].id;
        merge_input(&mut before, &original, &mut restored);
        assert_eq!(restored.profiles[0].selected_preset, restored.presets[1].id);
        let mut live = before.clone();
        live.profiles[0].selected_preset = live.presets[2].id;
        live.base_visible = false;
        merge_input(&mut before, &live, &mut restored);
        assert_eq!(restored.profiles[0].selected_preset, live.presets[2].id);
        assert!(!restored.base_visible);
        merge_input(&mut before, &original, &mut restored);
        assert_eq!(
            restored.profiles[0].selected_preset,
            original.profiles[0].selected_preset
        );
        assert!(restored.base_visible);
    }
}
