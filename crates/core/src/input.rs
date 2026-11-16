use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::validation::{chord_specificity, modifiers_match};
use crate::{
    AimAction, AnimationEvent, AnimationTrigger, AppState, BindingAction, Id, InputChord, InputKey,
    Modifier,
};

pub const CAPTURE_LEASE_MS: u64 = 30_000;

#[derive(Clone, Copy, Debug)]
pub struct InputEvent {
    pub device: u64,
    pub key: InputKey,
    pub down: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presentation {
    pub preset: Id,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "chord", rename_all = "snake_case")]
pub enum CaptureOutcome {
    Captured(InputChord),
    Cleared,
    Cancelled,
    Expired,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputOutcome {
    pub state_changed: bool,
    pub presentation_changed: bool,
    pub preset_selected: bool,
    pub capture: Option<CaptureOutcome>,
    pub animation_events: Vec<AnimationEvent>,
}

struct Capture {
    deadline: u64,
    waiting_for_release: bool,
    modifier_candidate: Option<(u64, InputKey)>,
}

enum Dispatch {
    Binding(BindingAction),
    Aim(AimAction),
    Fire,
    Reload,
    Movement,
}

/// Passive input state: no OS calls, polling, key injection or event suppression.
#[derive(Default)]
pub struct InputState {
    pressed: BTreeSet<(u64, InputKey)>,
    blocked: BTreeSet<(u64, InputKey)>,
    capture: Option<Capture>,
    aim: Option<InputChord>,
    fire: Option<InputChord>,
    movement: Vec<InputChord>,
    latched_preset: Option<Id>,
}

impl InputState {
    pub fn presentation(&self, state: &AppState) -> Option<Presentation> {
        self.presentation_for(state, state.active_profile()?.selected_preset)
    }

    /// `base` may be a saved design or the engine's session-only Browse selection.
    pub fn presentation_for(&self, state: &AppState, base: Id) -> Option<Presentation> {
        let action = state.active_profile()?.actions.aim_action;
        let held = self.aim.is_some();
        let preset = if held && let AimAction::ShowPresetWhileHeld { preset } = action {
            preset
        } else {
            self.latched_preset.unwrap_or(base)
        };
        let visible = state.base_visible
            && match action {
                AimAction::HideWhileHeld => !held,
                AimAction::ShowWhileHeld => held,
                _ => true,
            };
        Some(Presentation { preset, visible })
    }

    pub fn held(&self, trigger: AnimationTrigger) -> bool {
        match trigger {
            AnimationTrigger::Aim => self.aim.is_some(),
            AnimationTrigger::Fire => self.fire.is_some(),
            AnimationTrigger::Movement => !self.movement.is_empty(),
        }
    }

    pub fn clear_preset_latch(&mut self) {
        self.latched_preset = None;
    }

    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }

    pub fn mouse_buttons_held(&self) -> bool {
        self.pressed
            .iter()
            .any(|(_, key)| matches!(key, InputKey::Mouse { .. }))
    }

    /// A save/update barrier disables actions, but must still consume releases.
    pub fn observe_suspended(&mut self, event: InputEvent) {
        let pair = (event.device, event.key);
        if event.down && !event.key.is_pulse() {
            self.pressed.insert(pair);
            self.blocked.insert(pair);
        } else if !event.down {
            self.pressed.remove(&pair);
            self.blocked.remove(&pair);
        }
        self.reconcile_holds();
    }

    pub fn begin_capture(&mut self, now_ms: u64) {
        self.reset();
        self.capture = Some(Capture {
            deadline: now_ms.saturating_add(CAPTURE_LEASE_MS),
            waiting_for_release: !self.pressed.is_empty(),
            modifier_candidate: None,
        });
    }

    pub fn cancel_capture(&mut self) -> Option<CaptureOutcome> {
        self.capture.take()?;
        self.blocked.extend(self.pressed.iter().copied());
        Some(CaptureOutcome::Cancelled)
    }

    pub fn expire_capture(&mut self, now_ms: u64) -> Option<CaptureOutcome> {
        if self
            .capture
            .as_ref()
            .is_some_and(|capture| now_ms >= capture.deadline)
        {
            self.cancel_capture();
            Some(CaptureOutcome::Expired)
        } else {
            None
        }
    }

    /// Session changes block held triggers until they are released.
    pub fn reset(&mut self) {
        self.capture = None;
        self.blocked.extend(self.pressed.iter().copied());
        self.aim = None;
        self.fire = None;
        self.movement.clear();
        self.latched_preset = None;
    }

    pub fn remove_device(&mut self, device: u64) {
        self.pressed.retain(|(id, _)| *id != device);
        self.blocked.retain(|(id, _)| *id != device);
        self.reconcile_holds();
        self.latched_preset = None;
        if let Some(capture) = &mut self.capture {
            capture.modifier_candidate = None;
            capture.waiting_for_release = !self.pressed.is_empty();
        }
    }

    pub fn toggle_visibility(&mut self, state: &mut AppState) {
        self.reset();
        state.base_visible = !state.base_visible;
        state.revision = state.revision.saturating_add(1);
    }

    pub fn handle(&mut self, event: InputEvent, now_ms: u64, state: &mut AppState) -> InputOutcome {
        let before = self.presentation(state);
        let mut result = InputOutcome {
            capture: self.expire_capture(now_ms),
            ..Default::default()
        };
        let pair = (event.device, event.key);
        let pulse = event.key.is_pulse();
        let repeated = if event.down {
            !pulse && !self.pressed.insert(pair)
        } else {
            self.pressed.remove(&pair);
            self.blocked.remove(&pair);
            false
        };

        result.animation_events = self.reconcile_holds();
        if self.capture.is_some() {
            result.capture = self.capture_event(event, repeated);
        } else if event.down && !repeated && !self.blocked.contains(&pair) {
            let bits = self.modifier_bits();
            let matches = |chord: &InputChord| {
                chord.trigger == event.key && modifiers_match(&chord.modifiers, bits)
            };
            let bindings = state
                .active_profile()
                .into_iter()
                .flat_map(|profile| &profile.bindings)
                .filter(|binding| binding.enabled && matches(&binding.chord))
                .map(|binding| (&binding.chord, Dispatch::Binding(binding.action.clone())));
            let global = state
                .settings
                .global_toggle
                .as_ref()
                .filter(|chord| matches(chord))
                .map(|chord| (chord, Dispatch::Binding(BindingAction::ToggleVisibility)));
            let roles = state
                .active_profile()
                .into_iter()
                .flat_map(|profile| {
                    [
                        (
                            &profile.actions.aim,
                            Dispatch::Aim(profile.actions.aim_action),
                        ),
                        (&profile.actions.fire, Dispatch::Fire),
                        (&profile.actions.reload, Dispatch::Reload),
                    ]
                    .into_iter()
                    .chain(
                        profile
                            .actions
                            .movement
                            .iter()
                            .map(|chord| (chord, Dispatch::Movement)),
                    )
                })
                .filter_map(|(chord, role)| {
                    chord
                        .as_ref()
                        .filter(|chord| matches(chord))
                        .map(|chord| (chord, role))
                });
            let best = bindings
                .chain(global)
                .chain(roles)
                .max_by_key(|(chord, _)| chord_specificity(chord))
                .map(|(chord, role)| (chord.clone(), role));
            if let Some((chord, action)) = best {
                match action {
                    Dispatch::Binding(action) => {
                        if matches!(action, BindingAction::ToggleVisibility) {
                            result.animation_events.push(AnimationEvent::Reload);
                        }
                        result.preset_selected = matches!(action, BindingAction::SelectPreset { preset } if state.preset(preset).is_some());
                        if result.preset_selected {
                            self.clear_preset_latch();
                        }
                        result.state_changed = self.press_action(action, state);
                    }
                    Dispatch::Aim(AimAction::ToggleVisibility) => {
                        self.toggle_visibility(state);
                        result.state_changed = true;
                        result.animation_events.push(AnimationEvent::Reload);
                    }
                    Dispatch::Aim(action) if state.base_visible => {
                        let fresh = self.aim.is_none();
                        self.aim = Some(chord);
                        if fresh {
                            if let AimAction::TogglePreset { preset } = action {
                                self.latched_preset = if self.latched_preset == Some(preset) {
                                    None
                                } else {
                                    Some(preset)
                                };
                            }
                            result.animation_events.push(AnimationEvent::Trigger {
                                trigger: AnimationTrigger::Aim,
                                down: true,
                            });
                        }
                    }
                    Dispatch::Fire if state.base_visible => {
                        if self.fire.is_none() {
                            result.animation_events.push(AnimationEvent::Trigger {
                                trigger: AnimationTrigger::Fire,
                                down: true,
                            });
                        }
                        self.fire = Some(chord);
                    }
                    Dispatch::Movement if state.base_visible => {
                        if self.movement.is_empty() {
                            result.animation_events.push(AnimationEvent::Trigger {
                                trigger: AnimationTrigger::Movement,
                                down: true,
                            });
                        }
                        if !self.movement.contains(&chord) {
                            self.movement.push(chord);
                        }
                    }
                    Dispatch::Reload if state.base_visible => {
                        // Keep visibility/selection overrides; only animation triggers are rearmed.
                        result.animation_events.push(AnimationEvent::Reload);
                    }
                    _ => {}
                }
            }
        }
        result.presentation_changed = before != self.presentation(state);
        result
    }

    fn reconcile_holds(&mut self) -> Vec<AnimationEvent> {
        let mut events = Vec::new();
        let bits = self.modifier_bits();
        for (held, trigger) in [
            (&mut self.aim, AnimationTrigger::Aim),
            (&mut self.fire, AnimationTrigger::Fire),
        ] {
            if held.as_ref().is_some_and(|chord| {
                !modifiers_match(&chord.modifiers, bits)
                    || !self
                        .pressed
                        .iter()
                        .any(|pair| pair.1 == chord.trigger && !self.blocked.contains(pair))
            }) {
                *held = None;
                events.push(AnimationEvent::Trigger {
                    trigger,
                    down: false,
                });
            }
        }
        let moving = !self.movement.is_empty();
        self.movement.retain(|chord| {
            modifiers_match(&chord.modifiers, bits)
                && self
                    .pressed
                    .iter()
                    .any(|pair| pair.1 == chord.trigger && !self.blocked.contains(pair))
        });
        if moving && self.movement.is_empty() {
            events.push(AnimationEvent::Trigger {
                trigger: AnimationTrigger::Movement,
                down: false,
            });
        }
        events
    }

    fn press_action(&mut self, action: BindingAction, state: &mut AppState) -> bool {
        match action {
            BindingAction::ToggleVisibility => {
                self.toggle_visibility(state);
                true
            }
            BindingAction::SelectPreset { preset } => {
                if state.preset(preset).is_none() {
                    return false;
                }
                let Some(selected) = state
                    .active_profile()
                    .map(|profile| profile.selected_preset)
                else {
                    return false;
                };
                if preset == selected {
                    return false;
                }
                if let Some(profile) = state.active_profile_mut() {
                    profile.selected_preset = preset;
                }
                state.revision = state.revision.saturating_add(1);
                true
            }
        }
    }

    fn capture_event(&mut self, event: InputEvent, repeated: bool) -> Option<CaptureOutcome> {
        let capture = self.capture.as_mut()?;
        if capture.waiting_for_release {
            capture.waiting_for_release = !self.pressed.is_empty();
            return None;
        }
        if repeated {
            return None;
        }
        let modifier = modifier_bit(event.key).is_some();
        if event.down && modifier {
            capture.modifier_candidate = Some((event.device, event.key));
            return None;
        }
        if !event.down
            && (!modifier || capture.modifier_candidate != Some((event.device, event.key)))
        {
            return None;
        }
        let outcome = match event.key {
            InputKey::Keyboard {
                scan_code: 0x01,
                extended: false,
            } => CaptureOutcome::Cancelled,
            InputKey::Keyboard {
                scan_code: 0x0e,
                extended: false,
            } => CaptureOutcome::Cleared,
            _ => CaptureOutcome::Captured(InputChord {
                trigger: event.key,
                modifiers: captured_modifiers(event.key, self.modifier_bits()),
            }),
        };
        self.capture = None;
        self.blocked.extend(self.pressed.iter().copied());
        Some(outcome)
    }

    fn modifier_bits(&self) -> u8 {
        self.pressed
            .iter()
            .filter_map(|(_, key)| modifier_bit(*key))
            .fold(0, |all, bit| all | bit)
    }
}

pub fn modifier_bit(key: InputKey) -> Option<u8> {
    match key {
        InputKey::Keyboard {
            scan_code: 0x2a,
            extended: false,
        } => Some(1),
        InputKey::Keyboard {
            scan_code: 0x36,
            extended: false,
        } => Some(2),
        InputKey::Keyboard {
            scan_code: 0x1d,
            extended: false,
        } => Some(4),
        InputKey::Keyboard {
            scan_code: 0x1d,
            extended: true,
        } => Some(8),
        InputKey::Keyboard {
            scan_code: 0x38,
            extended: false,
        } => Some(16),
        InputKey::Keyboard {
            scan_code: 0x38,
            extended: true,
        } => Some(32),
        InputKey::Keyboard {
            scan_code: 0x5b,
            extended: true,
        } => Some(64),
        InputKey::Keyboard {
            scan_code: 0x5c,
            extended: true,
        } => Some(128),
        _ => None,
    }
}

fn captured_modifiers(trigger: InputKey, bits: u8) -> Vec<Modifier> {
    let trigger_bit = modifier_bit(trigger).unwrap_or(0);
    let mut result = Vec::new();
    for (left_bit, right_bit, generic, left, right) in [
        (
            1,
            2,
            Modifier::Shift,
            Modifier::LeftShift,
            Modifier::RightShift,
        ),
        (
            4,
            8,
            Modifier::Control,
            Modifier::LeftControl,
            Modifier::RightControl,
        ),
        (16, 32, Modifier::Alt, Modifier::LeftAlt, Modifier::RightAlt),
        (
            64,
            128,
            Modifier::Windows,
            Modifier::LeftWindows,
            Modifier::RightWindows,
        ),
    ] {
        if trigger_bit & (left_bit | right_bit) != 0 {
            if bits & left_bit != 0 && trigger_bit != left_bit {
                result.push(left);
            }
            if bits & right_bit != 0 && trigger_bit != right_bit {
                result.push(right);
            }
        } else if bits & (left_bit | right_bit) != 0 {
            result.push(generic);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Binding;

    fn mouse(button: u8, device: u64, down: bool) -> InputEvent {
        InputEvent {
            device,
            key: InputKey::Mouse { button },
            down,
        }
    }

    #[test]
    fn movement_aggregates_keys_devices_and_repeats_without_changing_state() {
        let mut state = AppState::default();
        let before = state.clone();
        let mut input = InputState::default();
        let movement = |down| AnimationEvent::Trigger {
            trigger: AnimationTrigger::Movement,
            down,
        };
        assert_eq!(
            input
                .handle(event(0x11, true), 0, &mut state)
                .animation_events,
            [movement(true)]
        );
        assert!(
            input
                .handle(event(0x11, true), 1, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(
            input
                .handle(event(0x1e, true), 2, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(
            input
                .handle(event(0x1f, true), 3, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(
            input
                .handle(event(0x11, false), 4, &mut state)
                .animation_events
                .is_empty()
        );
        let second = InputEvent {
            device: 2,
            ..event(0x1e, true)
        };
        assert!(
            input
                .handle(second, 5, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(
            input
                .handle(event(0x1e, false), 6, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(
            input
                .handle(event(0x1f, false), 7, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(input.held(AnimationTrigger::Movement));
        assert_eq!(
            input
                .handle(
                    InputEvent {
                        down: false,
                        ..second
                    },
                    8,
                    &mut state
                )
                .animation_events,
            [movement(false)]
        );
        assert!(!input.held(AnimationTrigger::Movement));
        assert_eq!(state, before);
    }

    #[test]
    fn movement_obeys_shortcut_specificity_modifiers_and_capture_reset() {
        let mut state = AppState::default();
        let target = state.presets[1].id;
        state.profiles[0].bindings.push(Binding::new(
            InputChord {
                trigger: InputChord::key(0x11).trigger,
                modifiers: vec![Modifier::Shift],
            },
            BindingAction::SelectPreset { preset: target },
        ));
        crate::validate_state(&state).unwrap();
        let mut input = InputState::default();
        input.handle(event(0x2a, true), 0, &mut state);
        assert!(
            input
                .handle(event(0x11, true), 1, &mut state)
                .preset_selected
        );
        assert!(!input.held(AnimationTrigger::Movement));
        input.handle(event(0x11, false), 2, &mut state);
        input.handle(event(0x2a, false), 3, &mut state);
        input.handle(event(0x11, true), 4, &mut state);
        assert!(input.held(AnimationTrigger::Movement));
        input.begin_capture(5);
        assert!(!input.held(AnimationTrigger::Movement));
        input.cancel_capture();
        input.handle(event(0x11, true), 6, &mut state);
        assert!(!input.held(AnimationTrigger::Movement));
        input.handle(event(0x11, false), 7, &mut state);
        input.handle(event(0x11, true), 8, &mut state);
        assert!(input.held(AnimationTrigger::Movement));
        input.remove_device(event(0x11, true).device);
        assert!(!input.held(AnimationTrigger::Movement));
        state.profiles[0].actions.movement[0]
            .as_mut()
            .unwrap()
            .modifiers = vec![Modifier::Control];
        input.handle(event(0x1d, true), 9, &mut state);
        input.handle(event(0x11, true), 10, &mut state);
        assert!(input.held(AnimationTrigger::Movement));
        input.handle(event(0x1d, false), 11, &mut state);
        assert!(!input.held(AnimationTrigger::Movement));
    }

    #[test]
    fn held_alternate_restores_the_latest_saved_or_browse_base_without_writes() {
        let mut state = AppState::default();
        let target = state.presets[1].id;
        let original = state.profiles[0].selected_preset;
        state.profiles[0].actions.aim_action = AimAction::ShowPresetWhileHeld { preset: target };
        let mut input = InputState::default();
        let down = input.handle(mouse(2, 1, true), 0, &mut state);
        assert!(!down.state_changed);
        assert_eq!(input.presentation(&state).unwrap().preset, target);
        assert_eq!(state.profiles[0].selected_preset, original);
        let browse = Id::new_v4();
        assert_eq!(
            input.presentation_for(&state, browse).unwrap().preset,
            target
        );
        input.handle(mouse(2, 1, false), 1, &mut state);
        assert_eq!(
            input.presentation_for(&state, browse).unwrap().preset,
            browse
        );
        assert_eq!(state.revision, 0);
    }

    #[test]
    fn aim_toggle_latches_once_per_press_and_normal_selection_clears_it() {
        let mut state = AppState::default();
        let target = state.presets[1].id;
        let base = state.profiles[0].selected_preset;
        state.profiles[0].actions.aim_action = AimAction::TogglePreset { preset: target };
        state.profiles[0].bindings.push(Binding::new(
            InputChord::key(0x21),
            BindingAction::SelectPreset { preset: base },
        ));
        let mut input = InputState::default();
        assert_eq!(
            input
                .handle(mouse(2, 1, true), 0, &mut state)
                .animation_events
                .len(),
            1
        );
        assert!(
            input
                .handle(mouse(2, 1, true), 1, &mut state)
                .animation_events
                .is_empty()
        );
        input.handle(mouse(2, 1, false), 2, &mut state);
        assert_eq!(input.presentation(&state).unwrap().preset, target);
        input.handle(mouse(2, 1, true), 3, &mut state);
        assert_eq!(input.presentation(&state).unwrap().preset, base);
        input.handle(mouse(2, 1, false), 4, &mut state);
        input.handle(mouse(2, 1, true), 5, &mut state);
        let selected = input.handle(event(0x21, true), 6, &mut state);
        assert!(selected.preset_selected);
        assert!(!selected.state_changed);
        assert_eq!(input.presentation(&state).unwrap().preset, base);
    }

    #[test]
    fn visibility_masks_cannot_bypass_master_and_reenable_waits_for_release() {
        let mut state = AppState::default();
        state.profiles[0].actions.aim_action = AimAction::ShowWhileHeld;
        let mut input = InputState::default();
        assert!(!input.presentation(&state).unwrap().visible);
        input.handle(mouse(2, 1, true), 0, &mut state);
        assert!(input.presentation(&state).unwrap().visible);
        input.toggle_visibility(&mut state);
        assert!(!input.presentation(&state).unwrap().visible);
        assert!(!input.held(AnimationTrigger::Aim));
        input.toggle_visibility(&mut state);
        input.handle(mouse(2, 1, true), 1, &mut state);
        assert!(!input.presentation(&state).unwrap().visible);
        input.handle(mouse(2, 1, false), 2, &mut state);
        input.handle(mouse(2, 1, true), 3, &mut state);
        assert!(input.presentation(&state).unwrap().visible);
        state.profiles[0].actions.aim_action = AimAction::HideWhileHeld;
        assert!(!input.presentation(&state).unwrap().visible);
        input.handle(mouse(2, 1, false), 4, &mut state);
        assert!(input.presentation(&state).unwrap().visible);
        state.profiles[0].actions.aim_action = AimAction::ToggleVisibility;
        input.handle(mouse(2, 1, true), 5, &mut state);
        assert!(!state.base_visible);
        input.handle(mouse(2, 1, false), 6, &mut state);
        input.handle(mouse(2, 1, true), 7, &mut state);
        assert!(state.base_visible);
    }

    #[test]
    fn device_and_modifier_release_end_only_the_last_matching_hold() {
        let mut state = AppState::default();
        state.profiles[0].actions.fire.as_mut().unwrap().modifiers = vec![Modifier::Shift];
        let mut input = InputState::default();
        input.handle(event(0x2a, true), 0, &mut state);
        input.handle(mouse(1, 1, true), 1, &mut state);
        assert!(
            input
                .handle(mouse(1, 2, true), 2, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(
            input
                .handle(mouse(1, 1, false), 3, &mut state)
                .animation_events
                .is_empty()
        );
        assert!(input.held(AnimationTrigger::Fire));
        assert_eq!(
            input
                .handle(event(0x2a, false), 4, &mut state)
                .animation_events,
            vec![AnimationEvent::Trigger {
                trigger: AnimationTrigger::Fire,
                down: false
            }]
        );
        assert!(!input.held(AnimationTrigger::Fire));
        input.handle(event(0x2a, true), 5, &mut state);
        assert!(!input.held(AnimationTrigger::Fire));
        input.handle(mouse(1, 2, false), 6, &mut state);
        input.handle(mouse(1, 2, true), 7, &mut state);
        input.remove_device(2);
        assert!(!input.held(AnimationTrigger::Fire));
    }

    #[test]
    fn action_capture_and_reset_clear_runtime_without_rearming_held_keys() {
        let mut state = AppState::default();
        let target = state.presets[1].id;
        state.profiles[0].actions.aim_action = AimAction::TogglePreset { preset: target };
        let mut input = InputState::default();
        input.handle(mouse(2, 1, true), 0, &mut state);
        input.handle(mouse(1, 1, true), 1, &mut state);
        input.begin_capture(2);
        assert!(!input.held(AnimationTrigger::Fire));
        assert_eq!(
            input.presentation(&state).unwrap().preset,
            state.profiles[0].selected_preset
        );
        assert!(
            input
                .handle(mouse(1, 1, true), 3, &mut state)
                .animation_events
                .is_empty()
        );
        input.handle(mouse(1, 1, false), 4, &mut state);
        input.handle(mouse(2, 1, false), 5, &mut state);
        let captured = input.handle(event(0x20, true), 6, &mut state);
        assert!(matches!(
            captured.capture,
            Some(CaptureOutcome::Captured(_))
        ));
        assert!(captured.animation_events.is_empty());
        input.reset();
        assert!(
            input
                .handle(event(0x20, true), 7, &mut state)
                .animation_events
                .is_empty()
        );
    }

    fn event(scan_code: u16, down: bool) -> InputEvent {
        InputEvent {
            device: 1,
            key: InputKey::Keyboard {
                scan_code,
                extended: false,
            },
            down,
        }
    }

    #[test]
    fn mouse_capture_keeps_release_tracking_after_capture_ends() {
        let mut input = InputState::default();
        let mut state = AppState::default();
        input.begin_capture(0);
        let mouse = InputEvent {
            device: 2,
            key: InputKey::Mouse { button: 1 },
            down: true,
        };
        assert!(matches!(
            input.handle(mouse, 1, &mut state).capture,
            Some(CaptureOutcome::Captured(_))
        ));
        assert!(!input.capturing());
        assert!(input.mouse_buttons_held());
        input.handle(
            InputEvent {
                down: false,
                ..mouse
            },
            2,
            &mut state,
        );
        assert!(!input.mouse_buttons_held());
        input.begin_capture(3);
        assert!(matches!(
            input.handle(event(0x12, true), 4, &mut state).capture,
            Some(CaptureOutcome::Captured(_))
        ));
    }

    #[test]
    fn profile_reset_tracks_held_mouse_until_release_or_removal() {
        let mut input = InputState::default();
        let mut state = AppState::default();
        let mouse = InputEvent {
            device: 2,
            key: InputKey::Mouse { button: 1 },
            down: true,
        };
        input.handle(mouse, 0, &mut state);
        input.reset();
        assert!(input.mouse_buttons_held());
        input.handle(
            InputEvent {
                down: false,
                ..mouse
            },
            1,
            &mut state,
        );
        assert!(!input.mouse_buttons_held());
        input.handle(mouse, 2, &mut state);
        input.remove_device(mouse.device);
        assert!(!input.mouse_buttons_held());
    }

    #[test]
    fn a_failed_shutdown_does_not_leave_mouse_releases_or_keys_stuck() {
        let mut input = InputState::default();
        let mut state = AppState::default();
        let mouse = InputEvent {
            device: 2,
            key: InputKey::Mouse { button: 1 },
            down: true,
        };
        input.handle(mouse, 0, &mut state);
        input.reset();
        input.observe_suspended(InputEvent {
            down: false,
            ..mouse
        });
        assert!(!input.mouse_buttons_held());
        input.begin_capture(1);
        assert!(matches!(
            input.handle(event(0x12, true), 2, &mut state).capture,
            Some(CaptureOutcome::Captured(_))
        ));
    }

    #[test]
    fn selection_is_persistent_and_held_keys_do_not_override_it() {
        let mut state = AppState::default();
        let first = state.presets[1].id;
        let next = state.presets[2].id;
        state.profiles[0].bindings = vec![
            Binding::new(
                InputChord::key(0x12),
                BindingAction::SelectPreset { preset: first },
            ),
            Binding::new(
                InputChord::key(0x02),
                BindingAction::SelectPreset { preset: next },
            ),
        ];
        let mut input = InputState::default();
        assert!(input.handle(event(0x12, true), 0, &mut state).state_changed);
        assert!(!input.handle(event(0x12, true), 1, &mut state).state_changed);
        input.handle(event(0x02, true), 2, &mut state);
        assert_eq!(input.presentation(&state).unwrap().preset, next);
        input.handle(event(0x12, false), 3, &mut state);
        input.remove_device(1);
        assert_eq!(input.presentation(&state).unwrap().preset, next);
        input.toggle_visibility(&mut state);
        assert!(!input.presentation(&state).unwrap().visible);
        assert_eq!(input.presentation(&state).unwrap().preset, next);
    }

    #[test]
    fn plain_keys_match_sprint_but_specific_chord_wins() {
        let mut state = AppState::default();
        let a = state.presets[1].id;
        let b = state.presets[2].id;
        state.profiles[0].bindings = vec![
            Binding::new(
                InputChord::key(0x12),
                BindingAction::SelectPreset { preset: a },
            ),
            Binding::new(
                InputChord {
                    trigger: event(0x12, true).key,
                    modifiers: vec![Modifier::Shift],
                },
                BindingAction::SelectPreset { preset: b },
            ),
        ];
        let mut input = InputState::default();
        input.handle(event(0x2a, true), 0, &mut state);
        input.handle(event(0x12, true), 1, &mut state);
        assert_eq!(state.active_profile().unwrap().selected_preset, b);
        input.handle(event(0x12, false), 2, &mut state);
        state.profiles[0].bindings.pop();
        input.handle(event(0x12, true), 3, &mut state);
        assert_eq!(state.active_profile().unwrap().selected_preset, a);
    }

    #[test]
    fn capture_waits_for_opening_click_and_rearms_after_release() {
        let mut state = AppState::default();
        state.profiles[0].bindings.push(Binding::new(
            InputChord::key(0x12),
            BindingAction::ToggleVisibility,
        ));
        let mut input = InputState::default();
        let mouse = InputEvent {
            device: 2,
            key: InputKey::Mouse { button: 1 },
            down: true,
        };
        input.handle(mouse, 0, &mut state);
        input.begin_capture(1);
        assert!(
            input
                .handle(
                    InputEvent {
                        down: false,
                        ..mouse
                    },
                    2,
                    &mut state
                )
                .capture
                .is_none()
        );
        assert_eq!(
            input.handle(event(0x12, true), 3, &mut state).capture,
            Some(CaptureOutcome::Captured(InputChord::key(0x12)))
        );
        assert!(state.base_visible);
        input.handle(event(0x12, true), 4, &mut state);
        assert!(state.base_visible);
        input.handle(event(0x12, false), 5, &mut state);
        input.handle(event(0x12, true), 6, &mut state);
        assert!(!state.base_visible);
    }

    #[test]
    fn capture_cancel_clear_and_expire_do_not_fire_bindings() {
        let mut state = AppState::default();
        let mut input = InputState::default();
        input.begin_capture(0);
        assert_eq!(
            input.handle(event(1, true), 1, &mut state).capture,
            Some(CaptureOutcome::Cancelled)
        );
        input.handle(event(1, false), 2, &mut state);
        input.begin_capture(3);
        assert_eq!(
            input.handle(event(14, true), 4, &mut state).capture,
            Some(CaptureOutcome::Cleared)
        );
        input.handle(event(14, false), 5, &mut state);
        input.begin_capture(6);
        assert_eq!(input.expire_capture(30_006), Some(CaptureOutcome::Expired));
        assert!(!input.capturing());
    }

    #[test]
    fn wheel_pulses_fire_without_key_up() {
        let mut state = AppState::default();
        let key = InputKey::Wheel { up: true };
        state.profiles[0].bindings.push(Binding::new(
            InputChord {
                trigger: key,
                modifiers: vec![],
            },
            BindingAction::ToggleVisibility,
        ));
        let mut input = InputState::default();
        input.handle(
            InputEvent {
                device: 1,
                key,
                down: true,
            },
            0,
            &mut state,
        );
        assert!(!state.base_visible);
        input.handle(
            InputEvent {
                device: 1,
                key,
                down: true,
            },
            1,
            &mut state,
        );
        assert!(state.base_visible);
    }

    #[test]
    fn capture_modifier_alone_and_both_sides_remains_valid() {
        let mut state = AppState::default();
        let mut input = InputState::default();
        input.begin_capture(0);
        input.handle(event(0x2a, true), 1, &mut state);
        let result = input
            .handle(event(0x2a, false), 2, &mut state)
            .capture
            .unwrap();
        assert_eq!(result, CaptureOutcome::Captured(InputChord::key(0x2a)));
        input.begin_capture(3);
        input.handle(event(0x2a, true), 4, &mut state);
        input.handle(event(0x36, true), 5, &mut state);
        let Some(CaptureOutcome::Captured(chord)) =
            input.handle(event(0x36, false), 6, &mut state).capture
        else {
            panic!("expected chord");
        };
        assert_eq!(chord.modifiers, vec![Modifier::LeftShift]);
        crate::validation::validate_chord(&chord).unwrap();
    }

    #[test]
    fn session_reset_cancels_capture_and_blocks_held_keys() {
        let mut input = InputState::default();
        input.begin_capture(0);
        input.reset();
        assert!(!input.capturing());
    }
}
