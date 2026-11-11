use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use anyhow::Result;
use crosshair_core::{protocol::*, *};

use crate::{ipc::Client, overlay::render::Key, storage::Store};

pub struct Connection {
    pub client: Client,
    pub subscribed: bool,
    pub snapshot: Option<(Id, Vec<u8>, Instant)>,
}

pub struct App {
    pub state: AppState,
    pub history: History,
    pub input: InputState,
    pub status: EngineStatus,
    pub clients: BTreeMap<Id, Connection>,
    pub edit_owner: Option<Id>,
    pub capture_owner: Option<(Id, Id)>,
    pub started: Instant,
    pub dirty_since: Option<Instant>,
    pub save_at: Option<Instant>,
    pub saved_revision: u64,
}

impl App {
    pub fn open(store: &Store, restore_visibility: bool) -> Result<Self> {
        let loaded = store.load();
        let error = loaded.as_ref().err().map(|error| format!("{error:#}"));
        let mut state = loaded.unwrap_or_default();
        if error.is_none() && crate::storage::prune_asset_index(&mut state, &Default::default()) {
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Settings revision exhausted"))?;
        }
        if !restore_visibility {
            state.base_visible = state.settings.show_on_launch;
        }
        let status = EngineStatus {
            recovery_required: error.is_some(),
            save_error: error,
            ..Default::default()
        };
        let saved_revision = state.revision;
        Ok(Self {
            state,
            history: History::default(),
            input: InputState::default(),
            status,
            clients: BTreeMap::new(),
            edit_owner: None,
            capture_owner: None,
            started: Instant::now(),
            dirty_since: None,
            save_at: None,
            saved_revision,
        })
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }

    pub fn status(&self) -> EngineStatus {
        let presentation = self.input.presentation(&self.state);
        EngineStatus {
            active_profile: Some(self.state.active_profile),
            selected_preset: presentation.map(|p| p.preset),
            visible: !self.status.recovery_required && presentation.is_some_and(|p| p.visible),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            ..self.status.clone()
        }
    }

    pub fn dirty(&mut self) {
        let now = Instant::now();
        let first = *self.dirty_since.get_or_insert(now);
        self.save_at = Some((now + Duration::from_millis(300)).min(first + Duration::from_secs(2)));
        self.status.unsaved = true;
    }

    pub fn saved(&mut self, revision: u64) {
        self.saved_revision = revision;
        self.status.unsaved = revision != self.state.revision;
        self.status.save_error = None;
        if !self.status.unsaved {
            self.dirty_since = None;
            self.save_at = None;
        } else if self.save_at.is_none() {
            self.dirty();
        }
    }

    pub fn save_failed(&mut self, message: String) {
        self.status.save_error = Some(message);
        self.status.unsaved = true;
        self.save_at = None;
    }

    pub fn changed(&mut self, document: Option<Id>, structure: bool) {
        self.broadcast(ServerMessage::Changed {
            revision: self.state.revision,
            document,
            structure,
            status: self.status(),
        });
    }

    pub fn broadcast(&self, message: ServerMessage) {
        for connection in self.clients.values().filter(|c| c.subscribed) {
            connection.client.send(message.clone());
        }
    }

    pub fn input(&mut self, event: InputEvent) -> bool {
        if self.status.recovery_required {
            return false;
        }
        let mouse_held = self.input.mouse_buttons_held();
        let outcome = self.input.handle(event, self.elapsed_ms(), &mut self.state);
        let captured = outcome.capture.is_some();
        if let Some(capture) = outcome.capture {
            self.captured(capture);
        }
        if outcome.state_changed {
            self.dirty();
        }
        if outcome.state_changed || outcome.presentation_changed {
            self.changed(None, false);
        }
        captured
            || outcome.state_changed
            || outcome.presentation_changed
            || mouse_held != self.input.mouse_buttons_held()
    }

    pub fn captured(&mut self, outcome: CaptureOutcome) {
        let (chord, cancelled) = match outcome {
            CaptureOutcome::Captured(chord) => (Some(chord), false),
            CaptureOutcome::Cleared => (None, false),
            CaptureOutcome::Cancelled | CaptureOutcome::Expired => (None, true),
        };
        if let Some((owner, token)) = self.capture_owner.take()
            && let Some(owner) = self.clients.get(&owner)
        {
            owner.client.send(ServerMessage::Captured {
                token,
                chord,
                cancelled,
            });
        }
    }

    pub fn reset_input(&mut self) {
        if let Some(outcome) = self.input.cancel_capture() {
            self.captured(outcome);
        }
        self.input.reset();
    }

    pub fn disconnect(&mut self, id: Id) {
        self.clients.remove(&id);
        if self.capture_owner.is_some_and(|(owner, _)| owner == id) {
            self.input.cancel_capture();
            self.capture_owner = None;
        }
        if self.edit_owner == Some(id) {
            if let Err(error) = self.history.end(&self.state) {
                self.status.save_error = Some(error.to_string());
            }
            self.edit_owner = None;
        }
        if !self.clients.values().any(|c| c.subscribed) {
            self.history.clear();
        }
    }

    pub fn presentation(&self) -> Option<(Key, [i32; 2])> {
        if self.status.recovery_required {
            return None;
        }
        self.presentation_of(&self.state, false)
    }

    pub fn presentation_of(
        &self,
        state: &AppState,
        include_hidden: bool,
    ) -> Option<(Key, [i32; 2])> {
        let presentation = self
            .input
            .presentation(state)
            .filter(|p| include_hidden || p.visible)?;
        let profile = state.active_profile()?;
        let preset = state.preset(presentation.preset)?;
        let monitor = self
            .status
            .monitors
            .iter()
            .find(|m| state.settings.monitor.as_ref() == Some(&m.id))
            .or_else(|| self.status.monitors.iter().find(|m| m.primary))
            .or_else(|| self.status.monitors.first());
        let monitor = monitor?;
        let [left, top, right, bottom] = monitor.bounds;
        let center = [
            left + (right - left) / 2 + profile.offset.x.round() as i32,
            top + (bottom - top) / 2 + profile.offset.y.round() as i32,
        ];
        Some((Key::new(preset, profile.scale), center))
    }
}
