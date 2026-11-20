use std::mem::size_of;

use anyhow::{Result, ensure};
use crosshair_core::{AimAction, AnimationTrigger, AppState, InputEvent, InputKey, Preset};
use windows::Win32::{
    Foundation::{HWND, LPARAM},
    UI::Input::{
        GetRawInputData, HRAWINPUT,
        KeyboardAndMouse::{MAPVK_VK_TO_VSC_EX, MapVirtualKeyW, VK_PAUSE},
        RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RAWKEYBOARD, RID_INPUT, RIDEV_DEVNOTIFY,
        RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE, RegisterRawInputDevices,
    },
};

pub struct Registration {
    hwnd: HWND,
    mouse: bool,
}

impl Registration {
    pub fn new(hwnd: HWND) -> Result<Self> {
        register(hwnd, 6, true)?;
        Ok(Self { hwnd, mouse: false })
    }

    pub fn update(&mut self, mouse: bool) -> Result<()> {
        if mouse != self.mouse {
            register(self.hwnd, 2, mouse)?;
            self.mouse = mouse;
        }
        Ok(())
    }
}

pub fn needs_mouse(state: &AppState, presets: &[&Preset], capture: bool, held: bool) -> bool {
    let mouse = |key| matches!(key, InputKey::Mouse { .. } | InputKey::Wheel { .. });
    let animated = |trigger| {
        state
            .presets
            .iter()
            .chain(presets.iter().copied())
            .any(|preset| {
                preset.layers.iter().any(|layer| {
                    layer
                        .animations
                        .iter()
                        .any(|a| a.effective_for(layer) && a.trigger == trigger)
                })
            })
    };
    held || capture
        || state
            .settings
            .global_toggle
            .as_ref()
            .is_some_and(|c| mouse(c.trigger))
        || state.active_profile().is_some_and(|profile| {
            profile
                .bindings
                .iter()
                .any(|b| b.enabled && mouse(b.chord.trigger))
                || profile
                    .actions
                    .hold_to_hide
                    .as_ref()
                    .is_some_and(|c| mouse(c.trigger))
                || profile.actions.aim.as_ref().is_some_and(|c| {
                    mouse(c.trigger)
                        && (profile.actions.aim_action != AimAction::None
                            || animated(AnimationTrigger::Aim))
                })
                || profile
                    .actions
                    .fire
                    .as_ref()
                    .is_some_and(|c| mouse(c.trigger) && animated(AnimationTrigger::Fire))
                || profile.actions.reload.as_ref().is_some_and(|c| {
                    mouse(c.trigger)
                        && (animated(AnimationTrigger::Aim)
                            || animated(AnimationTrigger::Fire)
                            || animated(AnimationTrigger::Movement))
                })
        })
}

impl Drop for Registration {
    fn drop(&mut self) {
        let _ = register(self.hwnd, 6, false);
        if self.mouse {
            let _ = register(self.hwnd, 2, false);
        }
    }
}

fn register(hwnd: HWND, usage: u16, enabled: bool) -> Result<()> {
    let device = RAWINPUTDEVICE {
        usUsagePage: 1,
        usUsage: usage,
        dwFlags: if enabled {
            RIDEV_INPUTSINK | RIDEV_DEVNOTIFY
        } else {
            RIDEV_REMOVE
        },
        hwndTarget: if enabled { hwnd } else { HWND::default() },
    };
    unsafe {
        RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32)?;
    }
    Ok(())
}

pub fn read(lparam: LPARAM, mut emit: impl FnMut(InputEvent)) -> Result<()> {
    // Keyboard/mouse input has a fixed native size; no allocation occurs for mouse motion.
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    let result = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as *mut _),
            RID_INPUT,
            Some((&mut raw as *mut RAWINPUT).cast()),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    ensure!(
        result != u32::MAX && result >= size_of::<RAWINPUTHEADER>() as u32,
        "Could not read native input"
    );
    let device = raw.header.hDevice.0 as u64;
    if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
        if let Some((key, down)) = keyboard(unsafe { raw.data.keyboard }) {
            emit(InputEvent { device, key, down });
        }
    } else if raw.header.dwType == RIM_TYPEMOUSE.0 {
        let mouse = unsafe { raw.data.mouse.Anonymous.Anonymous };
        for button in 1..=5 {
            let down = 1 << ((button - 1) * 2);
            if mouse.usButtonFlags & down != 0 {
                emit(InputEvent {
                    device,
                    key: InputKey::Mouse { button },
                    down: true,
                });
            }
            if mouse.usButtonFlags & (down << 1) != 0 {
                emit(InputEvent {
                    device,
                    key: InputKey::Mouse { button },
                    down: false,
                });
            }
        }
        if mouse.usButtonFlags & 0x400 != 0 && mouse.usButtonData != 0 {
            emit(InputEvent {
                device,
                key: InputKey::Wheel {
                    up: (mouse.usButtonData as i16) > 0,
                },
                down: true,
            });
        }
    }
    Ok(())
}

fn keyboard(raw: RAWKEYBOARD) -> Option<(InputKey, bool)> {
    if raw.MakeCode == 0xff || raw.VKey == 0xff {
        return None;
    }
    let down = raw.Flags & 1 == 0;
    if raw.VKey == VK_PAUSE.0 {
        return down.then_some((InputKey::Pause, true));
    }
    let code = if raw.MakeCode == 0 {
        (unsafe { MapVirtualKeyW(raw.VKey as u32, MAPVK_VK_TO_VSC_EX) }) as u16
    } else {
        raw.MakeCode
    };
    let extended = raw.Flags & 2 != 0 || code & 0xff00 == 0xe000;
    let scan_code = code & 0xff;
    // PrintScreen's synthetic E0-shift packet is not a held Shift key.
    if scan_code == 0 || (extended && matches!(scan_code, 0x2a | 0x36)) {
        return None;
    }
    Some((
        InputKey::Keyboard {
            scan_code,
            extended,
        },
        down,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_animation_observes_held_mouse_before_selection() {
        let mut state = AppState::default();
        let selected = state.profiles[0].selected_preset;
        let target = state.presets.iter_mut().find(|p| p.id != selected).unwrap();
        target.layers[0].animations = vec![crosshair_core::Animation {
            target: crosshair_core::AnimationTarget {
                opacity: Some(0.5),
                ..Default::default()
            },
            ..Default::default()
        }];
        let target_id = target.id;
        assert!(needs_mouse(&state, &[], false, false));
        assert!(state.profiles[0].bindings.is_empty());
        let mut input = crosshair_core::InputState::default();
        input.handle(
            InputEvent {
                device: 1,
                key: InputKey::Mouse { button: 1 },
                down: true,
            },
            0,
            &mut state,
        );
        state.profiles[0].selected_preset = target_id;
        let target = state.preset(target_id).unwrap();
        let mut playback = crosshair_core::Playback::default();
        playback.select(
            target,
            [
                input.held(AnimationTrigger::Aim),
                input.held(AnimationTrigger::Fire),
                input.held(AnimationTrigger::Movement),
            ],
            0,
        );
        assert!(playback.sample(target, 75).preset.layers[0].opacity < 1.0);
        state
            .presets
            .iter_mut()
            .find(|p| p.id == target_id)
            .unwrap()
            .layers[0]
            .animations
            .first_mut()
            .unwrap()
            .enabled = false;
        assert!(!needs_mouse(&state, &[], false, false));
    }

    #[test]
    fn unused_defaults_do_not_register_mouse_but_actions_and_pending_releases_do() {
        let mut state = AppState::default();
        assert!(!needs_mouse(&state, &[&state.presets[0]], false, false));
        assert!(needs_mouse(&state, &[], true, false));
        assert!(needs_mouse(&state, &[], false, true));
        state.profiles[0].actions.aim_action = AimAction::HideWhileHeld;
        assert!(needs_mouse(&state, &[], false, false));
        state.profiles[0].actions.aim_action = AimAction::None;
        state.profiles[0].actions.hold_to_hide = Some(crosshair_core::InputChord {
            trigger: InputKey::Mouse { button: 5 },
            modifiers: vec![],
        });
        assert!(needs_mouse(&state, &[], false, false));
        state.profiles[0].actions.hold_to_hide = None;
        state.presets[0].layers[0].animations = vec![crosshair_core::Animation {
            target: crosshair_core::AnimationTarget {
                x: Some(10.0),
                ..Default::default()
            },
            ..Default::default()
        }];
        assert!(needs_mouse(&state, &[&state.presets[0]], false, false));
        state.presets[0].layers[0]
            .animations
            .first_mut()
            .unwrap()
            .trigger = AnimationTrigger::Movement;
        assert!(!needs_mouse(&state, &[&state.presets[0]], false, false));
        state.profiles[0].actions.reload = Some(crosshair_core::InputChord {
            trigger: InputKey::Mouse { button: 4 },
            modifiers: vec![],
        });
        assert!(needs_mouse(&state, &[&state.presets[0]], false, false));
        state.presets[0].layers[0]
            .animations
            .first_mut()
            .unwrap()
            .enabled = false;
        assert!(!needs_mouse(&state, &[&state.presets[0]], false, false));
    }
    #[test]
    fn keyboard_distinguishes_extended_keys_and_pause() {
        let key = |code, flags, vkey| {
            keyboard(RAWKEYBOARD {
                MakeCode: code,
                Flags: flags,
                VKey: vkey,
                ..Default::default()
            })
        };
        assert_eq!(
            key(0x1d, 2, 0x11),
            Some((
                InputKey::Keyboard {
                    scan_code: 0x1d,
                    extended: true
                },
                true
            ))
        );
        assert_eq!(
            key(0x1d, 1, 0x11),
            Some((
                InputKey::Keyboard {
                    scan_code: 0x1d,
                    extended: false
                },
                false
            ))
        );
        assert_eq!(key(0x45, 4, VK_PAUSE.0), Some((InputKey::Pause, true)));
        assert_eq!(key(0x45, 5, VK_PAUSE.0), None);
        assert_eq!(key(0x2a, 2, 0x10), None);
        assert_eq!(key(0xff, 0, 0xff), None);
    }
}
