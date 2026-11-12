use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::validation::{chord_specificity, modifiers_match};
use crate::{AppState, BindingAction, Id, InputChord, InputKey, Modifier};

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
    pub capture: Option<CaptureOutcome>,
}

struct Capture {
    deadline: u64,
    waiting_for_release: bool,
    modifier_candidate: Option<(u64, InputKey)>,
}

/// Passive input state: no OS calls, polling, key injection or event suppression.
#[derive(Default)]
pub struct InputState {
    pressed: BTreeSet<(u64, InputKey)>,
    blocked: BTreeSet<(u64, InputKey)>,
    capture: Option<Capture>,
}

impl InputState {
    pub fn presentation(&self, state: &AppState) -> Option<Presentation> {
        Some(Presentation {
            preset: state.active_profile()?.selected_preset,
            visible: state.base_visible,
        })
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
    }

    pub fn remove_device(&mut self, device: u64) {
        self.pressed.retain(|(id, _)| *id != device);
        self.blocked.retain(|(id, _)| *id != device);
        if let Some(capture) = &mut self.capture {
            capture.modifier_candidate = None;
            capture.waiting_for_release = !self.pressed.is_empty();
        }
    }

    pub fn toggle_visibility(&mut self, state: &mut AppState) {
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

        if self.capture.is_some() {
            result.capture = self.capture_event(event, repeated);
        } else if event.down && !repeated && !self.blocked.contains(&pair) {
            let bits = self.modifier_bits();
            let best = state
                .active_profile()
                .into_iter()
                .flat_map(|profile| &profile.bindings)
                .filter(|binding| {
                    binding.enabled
                        && binding.chord.trigger == event.key
                        && modifiers_match(&binding.chord.modifiers, bits)
                })
                .max_by_key(|binding| chord_specificity(&binding.chord));
            let global = state.settings.global_toggle.as_ref().filter(|chord| {
                chord.trigger == event.key && modifiers_match(&chord.modifiers, bits)
            });
            let action = if global
                .filter(|global| {
                    best.is_none_or(|binding| {
                        chord_specificity(global) > chord_specificity(&binding.chord)
                    })
                })
                .is_some()
            {
                Some(BindingAction::ToggleVisibility)
            } else {
                best.map(|binding| binding.action.clone())
            };
            if let Some(action) = action {
                result.state_changed = self.press_action(action, state);
            }
        }
        result.presentation_changed = before != self.presentation(state);
        result
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
