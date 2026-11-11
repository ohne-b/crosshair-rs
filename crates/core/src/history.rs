use std::collections::{BTreeSet, VecDeque};

use crate::{AppState, Id, Preset, ValidationError, validate_preset, validate_state};

pub const HISTORY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("An edit transaction is already active")]
    Editing,
    #[error("No edit transaction is active")]
    NotEditing,
    #[error("The preset no longer exists")]
    Missing,
    #[error("The document changed; reload it before applying this edit")]
    Conflict,
    #[error("This layer is locked")]
    Locked,
    #[error("This edit is too large for undo history")]
    TooLarge,
    #[error("The revision counter is exhausted")]
    RevisionExhausted,
    #[error(transparent)]
    Invalid(#[from] ValidationError),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
}

struct Entry {
    before: AppState,
    after: AppState,
    bytes: usize,
}

struct Transaction {
    target: Id,
    before: AppState,
}

/// History stores configuration only; imported binary assets remain immutable references.
pub struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    bytes: usize,
    limit: usize,
    transaction: Option<Transaction>,
}

impl Default for History {
    fn default() -> Self {
        Self::with_budget(HISTORY_BYTES)
    }
}

impl History {
    pub fn with_budget(bytes: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
            limit: bytes.min(HISTORY_BYTES),
            transaction: None,
        }
    }

    pub fn can_undo(&self) -> bool {
        self.transaction.is_none() && !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        self.transaction.is_none() && !self.redo.is_empty()
    }
    pub fn editing(&self) -> Option<Id> {
        self.transaction
            .as_ref()
            .map(|transaction| transaction.target)
    }
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    pub fn retained_asset_ids(&self) -> BTreeSet<Id> {
        self.undo
            .iter()
            .chain(&self.redo)
            .flat_map(|entry| {
                entry
                    .before
                    .assets
                    .keys()
                    .chain(entry.after.assets.keys())
                    .copied()
            })
            .chain(
                self.transaction
                    .iter()
                    .flat_map(|transaction| transaction.before.assets.keys().copied()),
            )
            .collect()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
        self.transaction = None;
    }

    /// Called around a validated atomic library/profile mutation; input-only changes are ignored.
    pub fn record(&mut self, before: &AppState, after: &AppState) -> Result<bool, HistoryError> {
        if self.transaction.is_some() {
            return Err(HistoryError::Editing);
        }
        if same_content(before, after) {
            return Ok(false);
        }
        validate_state(after)?;
        let bytes = entry_bytes(before, after)?;
        if bytes > self.limit {
            return Err(HistoryError::TooLarge);
        }
        self.clear_redo();
        self.trim_for(bytes);
        self.undo.push_back(Entry {
            before: before.clone(),
            after: after.clone(),
            bytes,
        });
        self.bytes += bytes;
        Ok(true)
    }

    pub fn begin(
        &mut self,
        state: &AppState,
        target: Id,
        expected_revision: u64,
    ) -> Result<(), HistoryError> {
        if self.transaction.is_some() {
            return Err(HistoryError::Editing);
        }
        let preset = state.preset(target).ok_or(HistoryError::Missing)?;
        if preset.revision != expected_revision {
            return Err(HistoryError::Conflict);
        }
        let reservation = entry_bytes(state, state)?;
        if reservation > self.limit {
            return Err(HistoryError::TooLarge);
        }
        self.trim_for(reservation);
        self.transaction = Some(Transaction {
            target,
            before: state.clone(),
        });
        Ok(())
    }

    pub fn update(
        &mut self,
        state: &mut AppState,
        mut replacement: Preset,
        expected_revision: u64,
    ) -> Result<u64, HistoryError> {
        let transaction = self.transaction.as_ref().ok_or(HistoryError::NotEditing)?;
        if replacement.id != transaction.target {
            return Err(HistoryError::Conflict);
        }
        let current = state.preset(replacement.id).ok_or(HistoryError::Missing)?;
        if current.revision != expected_revision {
            return Err(HistoryError::Conflict);
        }
        for locked in current.layers.iter().filter(|layer| layer.locked) {
            let mut previous = locked.clone();
            previous.locked = false;
            let mut next = replacement
                .layers
                .iter()
                .find(|layer| layer.id == locked.id)
                .ok_or(HistoryError::Locked)?
                .clone();
            next.locked = false;
            if previous != next {
                return Err(HistoryError::Locked);
            }
        }
        validate_preset(&replacement, &state.assets)?;
        if same_preset(current, &replacement) {
            return Ok(current.revision);
        }
        replacement.revision = next_revision(current.revision)?;
        let revision = replacement.revision;
        let target = replacement.id;
        let mut candidate = state.clone();
        *candidate.preset_mut(target).ok_or(HistoryError::Missing)? = replacement;
        candidate.revision = next_revision(state.revision)?;
        validate_state(&candidate)?;
        let size = entry_bytes(&transaction.before, &candidate)?;
        if size > self.limit {
            return Err(HistoryError::TooLarge);
        }
        self.trim_for(size);
        *state = candidate;
        Ok(revision)
    }

    pub fn end(&mut self, state: &AppState) -> Result<bool, HistoryError> {
        let transaction = self.transaction.take().ok_or(HistoryError::NotEditing)?;
        match self.record(&transaction.before, state) {
            Ok(recorded) => Ok(recorded),
            Err(error) => {
                self.transaction = Some(transaction);
                Err(error)
            }
        }
    }

    pub fn cancel(&mut self, state: &mut AppState) -> Result<(), HistoryError> {
        let transaction = self.transaction.as_ref().ok_or(HistoryError::NotEditing)?;
        let prior = transaction
            .before
            .preset(transaction.target)
            .ok_or(HistoryError::Missing)?;
        let current = state
            .preset(transaction.target)
            .ok_or(HistoryError::Missing)?;
        let mut restored = prior.clone();
        restored.revision = next_revision(current.revision)?;
        let revision = next_revision(state.revision)?;
        *state
            .preset_mut(transaction.target)
            .ok_or(HistoryError::Missing)? = restored;
        state.revision = revision;
        self.transaction = None;
        Ok(())
    }

    pub fn undo(&mut self, state: &mut AppState) -> Result<bool, HistoryError> {
        if self.transaction.is_some() {
            return Err(HistoryError::Editing);
        }
        let Some(entry) = self.undo.back() else {
            return Ok(false);
        };
        if !same_content(state, &entry.after) {
            return Err(HistoryError::Conflict);
        }
        let restored = restore(state, &entry.before, &entry.after)?;
        let entry = self.undo.pop_back().expect("entry was inspected");
        self.redo.push(entry);
        *state = restored;
        Ok(true)
    }

    pub fn redo(&mut self, state: &mut AppState) -> Result<bool, HistoryError> {
        if self.transaction.is_some() {
            return Err(HistoryError::Editing);
        }
        let Some(entry) = self.redo.last() else {
            return Ok(false);
        };
        if !same_content(state, &entry.before) {
            return Err(HistoryError::Conflict);
        }
        let restored = restore(state, &entry.after, &entry.before)?;
        let entry = self.redo.pop().expect("entry was inspected");
        self.undo.push_back(entry);
        *state = restored;
        Ok(true)
    }

    fn trim_for(&mut self, bytes: usize) {
        while self.bytes + bytes > self.limit {
            if let Some(entry) = self.undo.pop_front() {
                self.bytes -= entry.bytes;
            } else if !self.redo.is_empty() {
                self.bytes -= self.redo.remove(0).bytes;
            } else {
                break;
            }
        }
    }

    fn clear_redo(&mut self) {
        self.bytes -= self.redo.iter().map(|entry| entry.bytes).sum::<usize>();
        self.redo.clear();
    }
}

fn next_revision(revision: u64) -> Result<u64, HistoryError> {
    revision
        .checked_add(1)
        .ok_or(HistoryError::RevisionExhausted)
}

fn entry_bytes(before: &AppState, after: &AppState) -> Result<usize, HistoryError> {
    // Serialized metadata with a conservative allowance for owned Rust containers.
    Ok((serde_json::to_vec(before)?.len() + serde_json::to_vec(after)?.len()).saturating_mul(2))
}

fn same_preset(a: &Preset, b: &Preset) -> bool {
    a.id == b.id && a.name == b.name && a.favorite == b.favorite && a.layers == b.layers
}

fn same_content(a: &AppState, b: &AppState) -> bool {
    a.assets == b.assets
        && a.presets.len() == b.presets.len()
        && a.presets
            .iter()
            .zip(&b.presets)
            .all(|(a, b)| same_preset(a, b))
        && a.profiles.len() == b.profiles.len()
        && a.profiles.iter().zip(&b.profiles).all(|(a, b)| {
            a.id == b.id
                && a.name == b.name
                && a.scale == b.scale
                && a.offset == b.offset
                && a.positions == b.positions
                && a.bindings == b.bindings
        })
}

fn restore(
    current: &AppState,
    source: &AppState,
    expected: &AppState,
) -> Result<AppState, HistoryError> {
    let mut restored = source.clone();
    restored.settings = current.settings.clone();
    restored.base_visible = current.base_visible;
    restored.revision = next_revision(current.revision)?;
    if restored
        .profiles
        .iter()
        .any(|profile| profile.id == current.active_profile)
    {
        restored.active_profile = current.active_profile;
    }
    for preset in &mut restored.presets {
        let live = current.preset(preset.id);
        if live.is_some_and(|live| same_preset(live, preset)) {
            preset.revision = live.expect("checked").revision;
        } else {
            preset.revision = next_revision(
                preset
                    .revision
                    .max(live.map_or(0, |live| live.revision))
                    .max(expected.preset(preset.id).map_or(0, |p| p.revision)),
            )?;
        }
    }
    for profile in &mut restored.profiles {
        let live = current.profiles.iter().find(|p| p.id == profile.id);
        let prior = expected.profiles.iter().find(|p| p.id == profile.id);
        if let (Some(live), Some(prior)) = (live, prior)
            && live.selected_preset != prior.selected_preset
            && restored
                .presets
                .iter()
                .any(|preset| preset.id == live.selected_preset)
        {
            profile.selected_preset = live.selected_preset;
        }
    }
    validate_state(&restored)?;
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Content;

    fn change_length(state: &AppState, length: f32) -> Preset {
        let mut replacement = state.presets[0].clone();
        if let Content::Crosshair(shape) = &mut replacement.layers[0].content {
            shape.length = length;
        }
        replacement
    }

    #[test]
    fn gesture_is_one_undo_entry_and_revisions_do_not_go_backwards() {
        let mut state = AppState::default();
        let initial = state.presets[0].clone();
        let mut history = History::default();
        history.begin(&state, initial.id, 0).unwrap();
        let replacement = change_length(&state, 11.0);
        history.update(&mut state, replacement, 0).unwrap();
        let replacement = change_length(&state, 12.0);
        history.update(&mut state, replacement, 1).unwrap();
        history.end(&state).unwrap();
        assert!(history.undo(&mut state).unwrap());
        assert_eq!(state.presets[0].layers, initial.layers);
        assert_eq!(state.presets[0].revision, 3);
        assert!(!history.undo(&mut state).unwrap());
        assert!(history.redo(&mut state).unwrap());
        assert_eq!(state.presets[0].revision, 4);
    }

    #[test]
    fn global_selection_and_visibility_are_not_undone() {
        let mut state = AppState::default();
        let target = state.presets[0].id;
        let mut history = History::default();
        history.begin(&state, target, 0).unwrap();
        let replacement = change_length(&state, 20.0);
        history.update(&mut state, replacement, 0).unwrap();
        history.end(&state).unwrap();
        let selected = state.presets[1].id;
        state.profiles[0].selected_preset = selected;
        state.base_visible = false;
        history.undo(&mut state).unwrap();
        assert_eq!(state.profiles[0].selected_preset, selected);
        assert!(!state.base_visible);
    }

    #[test]
    fn cancel_reverts_only_pinned_document_and_stale_updates_fail() {
        let mut state = AppState::default();
        let original = state.presets[0].clone();
        let mut history = History::default();
        history.begin(&state, original.id, 0).unwrap();
        let wrong = state.presets[1].clone();
        assert!(matches!(
            history.update(&mut state, wrong, 0),
            Err(HistoryError::Conflict)
        ));
        let replacement = change_length(&state, 20.0);
        history.update(&mut state, replacement.clone(), 0).unwrap();
        assert!(matches!(
            history.update(&mut state, replacement, 0),
            Err(HistoryError::Conflict)
        ));
        state.base_visible = false;
        history.cancel(&mut state).unwrap();
        assert_eq!(state.presets[0].layers, original.layers);
        assert!(!state.base_visible);
        assert!(!history.can_undo());
    }

    #[test]
    fn deletion_undo_restores_references_and_order() {
        let mut state = AppState::default();
        let before = state.clone();
        let deleted = state.presets.remove(0);
        state.profiles[0].selected_preset = state.presets[0].id;
        let mut history = History::default();
        history.record(&before, &state).unwrap();
        history.undo(&mut state).unwrap();
        assert_eq!(state.presets[0].id, deleted.id);
        assert_eq!(state.profiles[0].selected_preset, deleted.id);
        history.redo(&mut state).unwrap();
        assert!(state.preset(deleted.id).is_none());
    }

    #[test]
    fn history_is_bounded_and_rejects_locked_layer_changes() {
        let mut state = AppState::default();
        state.presets[0].layers[0].locked = true;
        let id = state.presets[0].id;
        let mut history = History::default();
        history.begin(&state, id, 0).unwrap();
        let replacement = change_length(&state, 20.0);
        assert!(matches!(
            history.update(&mut state, replacement, 0),
            Err(HistoryError::Locked)
        ));
        history.cancel(&mut state).unwrap();
        let mut tiny = History::with_budget(32);
        assert!(matches!(
            tiny.begin(&state, id, state.presets[0].revision),
            Err(HistoryError::TooLarge)
        ));
        assert_eq!(tiny.retained_bytes(), 0);
    }

    #[test]
    fn cancelled_gesture_preserves_redo_and_unrecorded_changes_are_not_overwritten() {
        let mut state = AppState::default();
        let mut history = History::default();
        let before = state.clone();
        state.presets[0].name = "Changed".into();
        history.record(&before, &state).unwrap();
        history.undo(&mut state).unwrap();
        let id = state.presets[0].id;
        let revision = state.presets[0].revision;
        history.begin(&state, id, revision).unwrap();
        history.cancel(&mut state).unwrap();
        assert!(history.can_redo());
        history.redo(&mut state).unwrap();
        state.presets[0].name = "External change".into();
        assert!(matches!(
            history.undo(&mut state),
            Err(HistoryError::Conflict)
        ));
        assert_eq!(state.presets[0].name, "External change");
    }
}
