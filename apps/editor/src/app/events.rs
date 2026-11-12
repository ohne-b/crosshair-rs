use super::*;

impl App {
    pub(super) fn event(&mut self, event: Event) -> Result<()> {
        if let Event::Engine(epoch, _) = &event {
            if !self.client.accepts(*epoch) {
                return Ok(());
            }
            if self.connection_epoch != *epoch {
                self.connection_epoch = *epoch;
                self.new_connection = true;
                self.status = EngineStatus::default();
            }
        }
        match event {
            Event::Maximized(value) => self.window()?.global::<Store>().set_window_maximized(value),
            Event::CaptionHover(value) => {
                let ui = self.window()?;
                let store = ui.global::<Store>();
                if store.get_caption_hover() != value {
                    store.set_caption_hover(value);
                }
            }
            Event::Ui(event)
                if (self.group_ending
                    || (self.group_open && self.finish_group)
                    || (self.finish_group && self.preview_pending))
                    && !matches!(&event, UiEvent::Action(action, _) if action.starts_with("window-") || matches!(action.as_str(), "navigate" | "dismiss-error" | "modal-cancel" | "capture-cancel" | "reconnect")) =>
            {
                defer_ui(&mut self.deferred_ui, event)?;
            }
            Event::Ui(event) => match event {
                UiEvent::Action(action, value) => self.action(&action, &value)?,
                UiEvent::Property(selection, key, value, phase) => {
                    if selection == self.selection_token() {
                        self.property_edit(&key, &value, phase)?;
                    }
                }
                UiEvent::Select(id, extend) => {
                    if self.group_open && self.window()?.global::<Store>().get_connected() {
                        defer_ui(&mut self.deferred_ui, UiEvent::Select(id, extend))?;
                        self.finish_group = true;
                        self.send_document()?;
                        self.finish_edit()?;
                    } else {
                        self.document.select(id.parse()?, extend);
                        self.refresh_document();
                    }
                }
                UiEvent::Canvas(kind, point, button, shift) => {
                    self.canvas_event(kind, point, button, shift)?
                }
                UiEvent::Shortcut(text, control, shift, alt) => {
                    self.shortcut(&text, control, shift, alt)?;
                }
                UiEvent::CaptureRegion(region) => self.capture_region = Some(region),
            },
            Event::Appearance(appearance) => {
                let ui = self.window()?;
                if let Ok(hwnd) = frame::handle(ui.window()) {
                    frame::apply_colors(hwnd, appearance.colors);
                }
                let theme = ui.global::<crate::Theme>();
                theme.set_text_scale(appearance.text_scale);
                theme.set_animations(appearance.animations);
                let color = |rgb: u32| {
                    slint::Color::from_rgb_u8(rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8)
                };
                if let Some([background, foreground, accent, accent_text]) = appearance.colors {
                    theme.set_canvas(color(background));
                    theme.set_preview(color(background));
                    theme.set_panel(color(background));
                    theme.set_field(color(background));
                    theme.set_raised(color(background));
                    theme.set_hover(color(background));
                    theme.set_border(color(foreground));
                    theme.set_text(color(foreground));
                    theme.set_muted(color(foreground));
                    theme.set_orange(color(accent));
                    theme.set_on_accent(color(accent_text));
                } else {
                    theme.set_canvas(slint::Color::from_rgb_u8(23, 23, 23));
                    theme.set_preview(slint::Color::from_rgb_u8(20, 20, 20));
                    theme.set_panel(slint::Color::from_rgb_u8(30, 30, 30));
                    theme.set_field(slint::Color::from_rgb_u8(37, 37, 37));
                    theme.set_raised(slint::Color::from_rgb_u8(41, 41, 41));
                    theme.set_hover(slint::Color::from_rgb_u8(51, 51, 51));
                    theme.set_border(slint::Color::from_rgb_u8(48, 48, 48));
                    theme.set_text(slint::Color::from_rgb_u8(244, 244, 245));
                    theme.set_muted(slint::Color::from_rgb_u8(161, 161, 170));
                    theme.set_orange(slint::Color::from_rgb_u8(249, 115, 22));
                    theme.set_on_accent(slint::Color::from_rgb_u8(24, 24, 24));
                }
            }
            Event::WorkArea([width, height]) => {
                let ui = self.window()?;
                let store = ui.global::<Store>();
                store.set_work_width(width);
                store.set_work_height(height);
            }
            Event::Engine(_, client::Event::Reused) => self.close_now()?,
            Event::Engine(_, client::Event::Activity(activity)) => self
                .window()?
                .global::<Store>()
                .set_activity(activity.unwrap_or_default().into()),
            Event::Engine(_, client::Event::Connected) => {
                self.document.reconnected();
                let ui = self.window()?;
                ui.global::<Store>().set_connected(true);
                self.new_connection = true;
                self.recovery_shown = false;
                ui.global::<Store>().set_error(if self.document.dirty {
                    "Reconnected. This editor has unsent changes; retry saving or export the draft."
                        .into()
                } else {
                    SharedString::default()
                });
            }
            Event::Engine(_, client::Event::State(state)) => {
                if state.revision < self.state.revision && !self.new_connection {
                    return Ok(());
                }
                self.new_connection = false;
                if let Some(id) = self.document.draft.as_ref().map(|preset| preset.id) {
                    if let Some(preset) = state.preset(id) {
                        if self.document.dirty
                            && self.inflight.is_none()
                            && self
                                .document
                                .draft
                                .as_ref()
                                .is_some_and(|draft| draft.revision != preset.revision)
                        {
                            self.document.conflict = true;
                            self.error("The crosshair changed while this draft was unsent. Review the draft before retrying.".into());
                        }
                        self.document.receive(preset);
                    } else if !self.document.dirty {
                        self.document.draft = None;
                    }
                }
                self.state = state;
                if let Some(id) = self.open_after_refresh.take() {
                    self.open_document(id)?;
                }
                if self.document.draft.is_none()
                    && let Some(preset) = self.state.selected_preset()
                {
                    self.document.open(preset.clone())?;
                }
                if let Some((id, width, height)) = self.image_ready.take() {
                    self.add_imported_image(id, width, height)?;
                }
                self.refresh();
                self.render();
            }
            Event::Engine(_, client::Event::Message(message)) => match message {
                ServerMessage::Changed {
                    revision, status, ..
                } => {
                    let selection_changed = self.status.selected_preset != status.selected_preset;
                    if !accept_status(&mut self.status, status) {
                        return Ok(());
                    }
                    self.refresh_status();
                    if selection_changed {
                        self.refresh_library();
                    }
                    if revision > self.state.revision && self.inflight.is_none() {
                        self.client.refresh();
                    }
                    if self.status.recovery_required && !self.recovery_shown {
                        self.recovery_shown = true;
                        self.modal = Modal::Recovery;
                        self.show_modal(1, "Settings need recovery", "The engine could not read the saved configuration. Restore the last valid backup, or reset after preserving the unreadable data.", "Restore backup", "Reset with backup", "");
                    }
                }
                ServerMessage::Captured {
                    token,
                    chord,
                    cancelled,
                } => {
                    if self.capture_token != Some(token) {
                        return Ok(());
                    }
                    if cancelled {
                        self.clear_capture()?;
                    } else {
                        self.complete_capture(chord)?;
                    }
                }
                ServerMessage::Update { status } => {
                    if status.revision >= self.status.update.revision {
                        self.status.update = status;
                        self.refresh_status();
                    }
                }
                ServerMessage::FocusEditor => {
                    frame::focus(self.hwnd()?);
                }
                ServerMessage::PrepareClose { updating } => {
                    self.shutdown_requested = Some(updating);
                    self.request_close()?;
                }
                ServerMessage::CloseCancelled => {
                    self.shutdown_requested = None;
                    self.closing = false;
                    self.close_flush_pending = false;
                    self.install_requested = false;
                    self.window()?.global::<Store>().set_read_only(false);
                    self.hide_modal();
                    self.client.refresh();
                }
                ServerMessage::Closing => {
                    if self.document.dirty || self.inflight.is_some() {
                        self.request_close()?;
                    } else {
                        self.close_now()?;
                    }
                }
                ServerMessage::Response(_) => {}
            },
            Event::Engine(_, client::Event::Completed { tag, reply }) => {
                if tag == "preview" {
                    self.preview_sent = self.preview_inflight.take();
                    self.preview_scale = self.preview_inflight_scale;
                    self.send_document()?;
                } else if tag == "clear-preview" {
                } else if tag == "save-cancel" {
                    self.client.refresh();
                    EVENTS.with(|events| {
                        events
                            .borrow_mut()
                            .extend(self.deferred_ui.drain(..).map(Event::Ui))
                    });
                } else if tag == "save-flush" {
                    if let Some(id) = self.open_after_save.take() {
                        self.open_document(id)?;
                    }
                    self.refresh_document();
                } else if tag == "document" {
                    if let Some(sent) = self.inflight.take() {
                        let version = match reply {
                            Reply::Ack {
                                document_revision: Some(version),
                            } => version,
                            _ => {
                                self.document.dirty = true;
                                anyhow::bail!(
                                    "The engine did not acknowledge the document revision. Reconnect before retrying."
                                );
                            }
                        };
                        self.document.acknowledge(&sent, version);
                        if let Some(stored) = self.state.preset_mut(sent.id) {
                            *stored = sent;
                            stored.revision = version;
                        }
                    }
                    self.send_document()?;
                    self.finish_edit()?;
                    self.refresh_document();
                } else if tag == "begin-edit" {
                } else if tag == "recover-cancel" {
                    self.group_open = false;
                    self.group_ending = false;
                    self.cancel_group = false;
                    self.document.gesture = None;
                    self.reapply_draft()?;
                    EVENTS.with(|events| {
                        events
                            .borrow_mut()
                            .extend(self.deferred_ui.drain(..).map(Event::Ui))
                    });
                } else if tag == "end-edit" || tag == "cancel-edit" {
                    let more_edits = self.document.dirty && tag != "cancel-edit";
                    self.group_open = false;
                    self.group_ending = false;
                    self.cancel_group = false;
                    self.document.finish_gesture();
                    self.draw_base = None;
                    self.save_requested = false;
                    if tag == "cancel-edit" {
                        self.document.dirty = false;
                    }
                    self.send("save-flush", Command::Flush)?;
                    if let Some(command) = self.pending_command.take() {
                        self.send("pending", command)?;
                    }
                    if self.install_requested && !more_edits {
                        self.send("install-flush", Command::Flush)?;
                    }
                    self.client.refresh();
                    EVENTS.with(|events| {
                        events
                            .borrow_mut()
                            .extend(self.deferred_ui.drain(..).map(Event::Ui))
                    });
                } else if tag == "close-flush" {
                    if self.shutdown_requested.is_some() {
                        self.send("confirm-quit", Command::ConfirmQuit)?;
                        self.window()?.global::<Store>().set_read_only(true);
                    } else {
                        self.close_now()?;
                    }
                } else if tag == "confirm-quit" {
                } else if tag == "install-flush" {
                    self.install_requested = false;
                    self.send("update", Command::InstallUpdate)?;
                } else if tag == "image" {
                    if let Reply::Asset { id, width, height } = reply {
                        self.image_ready = Some((id, width, height));
                        self.client.refresh();
                    }
                } else if tag == "new-preset" || tag == "duplicate-open" {
                    if let Reply::Created { id } = reply {
                        self.open_after_refresh = Some(id);
                        self.client.refresh();
                    }
                } else if let Reply::Preset { preset } = reply {
                    self.document.open(preset)?;
                    self.refresh_document();
                    self.render();
                } else {
                    self.client.refresh();
                }
            }
            Event::Engine(
                _,
                client::Event::Failed {
                    tag,
                    message,
                    conflict,
                },
            ) => {
                if tag == "preview" {
                    let failed = self.preview_inflight.take();
                    self.preview_sent = failed;
                    self.preview_scale = self.preview_inflight_scale;
                    self.document.conflict |= conflict;
                    self.send_document()?;
                }
                if tag == "save-flush" {
                    self.document.dirty = true;
                    self.document.save_paused = true;
                    self.closing = false;
                    self.open_after_save = None;
                }
                if tag == "document" || tag == "begin-edit" {
                    self.inflight = None;
                    self.document.dirty = true;
                    self.document.conflict = conflict;
                    self.save_requested = false;
                    self.document.save_paused = true;
                    if self.group_open {
                        self.group_open = false;
                        self.group_ending = false;
                        self.finish_group = false;
                        self.cancel_group = false;
                        self.send("save-cancel", Command::CancelEdit)?;
                    }
                }
                if tag == "begin-edit" {
                    self.group_open = false;
                }
                if tag == "end-edit" || tag == "cancel-edit" || tag == "recover-cancel" {
                    self.group_ending = false;
                    self.finish_group = false;
                    self.cancel_group = false;
                    EVENTS.with(|events| {
                        events
                            .borrow_mut()
                            .extend(self.deferred_ui.drain(..).map(Event::Ui))
                    });
                }
                if tag
                    .strip_prefix("capture:")
                    .and_then(|value| value.parse::<Id>().ok())
                    .is_some_and(|token| self.capture_token == Some(token))
                {
                    self.capture = None;
                    self.capture_token = None;
                    self.window()?
                        .global::<Store>()
                        .set_capture_id(SharedString::default());
                }
                if tag == "image" {
                    self.image_target = None;
                }
                if tag == "close-flush" || tag == "confirm-quit" {
                    self.closing = false;
                    self.close_flush_pending = false;
                    self.window()?.global::<Store>().set_read_only(false);
                    if self.shutdown_requested.take().is_some() {
                        let _ = self.send("cancel-quit", Command::CancelQuit);
                    }
                }
                if tag == "install-flush" || tag == "document" || tag == "begin-edit" {
                    self.install_requested = false;
                }
                if self.closing
                    && matches!(
                        tag.as_str(),
                        "document" | "begin-edit" | "end-edit" | "cancel-edit" | "recover-cancel"
                    )
                {
                    self.closing = false;
                    if self.shutdown_requested.take().is_some() {
                        let _ = self.send("cancel-quit", Command::CancelQuit);
                    }
                }
                self.error(message);
                self.refresh_document();
            }
            Event::Engine(_, client::Event::Disconnected(message)) => {
                let close_clean =
                    self.closing && !self.document.dirty && self.deferred_ui.is_empty();
                self.window()?.global::<Store>().set_connected(false);
                self.document.save_paused = self.document.dirty || self.inflight.is_some();
                self.inflight = None;
                self.preview_inflight = None;
                self.preview_sent = None;
                self.save_requested = false;
                self.group_open = false;
                self.group_ending = false;
                self.capture = None;
                self.capture_token = None;
                self.cancel_group = false;
                self.finish_group = false;
                self.pending_command = None;
                self.install_requested = false;
                self.shutdown_requested = None;
                self.closing = false;
                self.close_flush_pending = false;
                self.window()?.global::<Store>().set_read_only(false);
                if !self.document.dirty {
                    self.document.gesture = None;
                    self.draw_base = None;
                    self.preview_pending = false;
                }
                EVENTS.with(|events| {
                    events
                        .borrow_mut()
                        .extend(self.deferred_ui.drain(..).map(Event::Ui))
                });
                self.window()?
                    .global::<Store>()
                    .set_capture_id(SharedString::default());
                self.error(message);
                if close_clean {
                    self.close_now()?;
                } else {
                    self.refresh_document();
                }
            }
            Event::Preview(preview::Event::Document {
                scale,
                generation,
                id,
                raster,
                bounds,
                inverses,
                anchors,
                adjusted,
            }) => {
                if generation != self.preview_generation
                    || self.document.draft.as_ref().map(|preset| preset.id) != Some(id)
                {
                    return Ok(());
                }
                self.bounds = bounds;
                self.inverses = inverses;
                self.anchors = anchors;
                self.preview_pending = false;
                if let Some(mut adjusted) = adjusted {
                    if let Some(current) = &self.document.draft {
                        adjusted.revision = current.revision;
                    }
                    self.document.draft = Some(adjusted);
                }
                self.last_valid = self.document.draft.clone();
                self.send_document()?;
                self.finish_edit()?;
                let ui = self.window()?;
                let store = ui.global::<Store>();
                store.set_preview(preview::image(&raster));
                store.set_preview_x(raster.origin[0] as f32 / scale);
                store.set_preview_y(raster.origin[1] as f32 / scale);
                store.set_preview_width(raster.width as f32 / scale);
                store.set_preview_height(raster.height as f32 / scale);
                self.refresh_bounds();
                self.refresh_status();
            }
            Event::Preview(preview::Event::Thumbnail {
                id,
                revision,
                raster,
            }) => {
                self.thumbnails
                    .insert(id, (revision, preview::image(&raster)));
                self.refresh_library();
                self.refresh_status();
            }
            Event::Preview(preview::Event::Fonts(fonts)) => {
                self.fonts = fonts;
                self.refresh_document();
            }
            Event::Preview(preview::Event::DocumentFailed {
                generation,
                id,
                message,
            }) => {
                if generation != self.preview_generation
                    || self.document.draft.as_ref().map(|document| document.id) != Some(id)
                {
                    return Ok(());
                }
                self.preview_pending = false;
                self.drag = None;
                self.draw_base = None;
                let restored = if let Some(valid) = self.last_valid.as_ref().filter(|preset| {
                    preset.id == id
                        && self.document.draft.as_ref().is_some_and(|draft| {
                            !crate::features::designer::same_design(draft, preset)
                        })
                }) {
                    let mut valid = valid.clone();
                    if let Some(current) = &self.document.draft {
                        valid.revision = current.revision;
                        valid.favorite = current.favorite;
                        valid.modified_order = current.modified_order;
                    }
                    self.document.draft = Some(valid);
                    self.document.update_dirty();
                    true
                } else {
                    self.cancel_document_gesture();
                    false
                };
                self.save_requested = false;
                self.finish_group = true;
                self.finish_edit()?;
                if restored {
                    self.render();
                }
                self.refresh_document();
                self.closing = false;
                self.install_requested = false;
                if self.shutdown_requested.take().is_some() {
                    let _ = self.send("cancel-quit", Command::CancelQuit);
                }
                self.preview_error = Some(message.clone());
                self.error(message);
            }
            Event::Preview(preview::Event::Error(message)) => {
                if self.closing {
                    self.closing = false;
                    if self.shutdown_requested.take().is_some() {
                        let _ = self.send("cancel-quit", Command::CancelQuit);
                    }
                }
                self.error(message);
            }
            Event::Preview(preview::Event::Anchored {
                original,
                mut changed,
            }) => {
                let current = self
                    .document
                    .draft
                    .as_ref()
                    .context("The crosshair has closed")?;
                anyhow::ensure!(
                    crate::features::designer::same_design(current, &original),
                    "The layer changed before its anchor could be adjusted. Retry."
                );
                if crate::features::designer::same_design(&original, &changed) {
                    return Ok(());
                }
                changed.revision = current.revision;
                changed.favorite = current.favorite;
                changed.modified_order = current.modified_order;
                self.begin_edit()?;
                self.document.draft = Some(changed);
                self.document.dirty = true;
                self.finish_group = true;
                self.render();
                self.refresh_document();
            }
            Event::Preview(preview::Event::Recovered {
                original,
                mut changed,
                state,
                action,
            }) => {
                anyhow::ensure!(
                    self.document.draft.as_ref().is_some_and(|draft| {
                        crate::features::designer::same_design(draft, &original)
                    }),
                    "The draft changed while recovery was preparing. Retry with the current draft."
                );
                self.state = state;
                match action {
                    preview::RecoveryAction::Reapply | preview::RecoveryAction::Save => {
                        let current = self.state.preset(changed.id).context(
                            "This crosshair was deleted. Export the draft to preserve it.",
                        )?;
                        if matches!(action, preview::RecoveryAction::Save)
                            && current.revision != original.revision
                        {
                            self.document.conflict = true;
                            anyhow::bail!(
                                "The saved crosshair changed. Review the draft before replacing it."
                            );
                        }
                        changed.revision = current.revision;
                        changed.favorite = current.favorite;
                        changed.modified_order = current.modified_order;
                        self.document.draft = Some(changed);
                        self.document.dirty = true;
                        self.document.conflict = false;
                        self.document.save_paused = false;
                        self.window()?
                            .global::<Store>()
                            .set_error(SharedString::default());
                        self.group_open = false;
                        self.group_ending = false;
                        self.finish_group = true;
                        self.save_requested = true;
                        self.render();
                    }
                    preview::RecoveryAction::Export {
                        destination,
                        format,
                    } => {
                        self.client.export_draft(changed, destination, format)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn accept_status(current: &mut EngineStatus, mut incoming: EngineStatus) -> bool {
    if incoming.revision < current.revision {
        return false;
    }
    if incoming.update.revision < current.update.revision {
        incoming.update = current.update.clone();
    }
    *current = incoming;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_subscription_status_cannot_undo_live_visibility_or_update_progress() {
        let mut current = EngineStatus {
            revision: 4,
            visible: true,
            update: UpdateStatus {
                revision: 8,
                phase: UpdatePhase::Downloading,
                downloaded: 4096,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!accept_status(&mut current, EngineStatus::default()));
        assert!(current.visible);
        assert!(accept_status(
            &mut current,
            EngineStatus {
                revision: 5,
                visible: false,
                ..Default::default()
            }
        ));
        assert!(!current.visible);
        assert_eq!(current.update.phase, UpdatePhase::Downloading);
        assert_eq!(current.update.downloaded, 4096);
    }
}
