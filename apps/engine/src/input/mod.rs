use std::mem::size_of;

use anyhow::{Result, ensure};
use crosshair_core::{AppState, InputEvent, InputKey};
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

    pub fn update(
        &mut self,
        state: &AppState,
        capture: bool,
        mouse_buttons_held: bool,
    ) -> Result<()> {
        let uses_mouse =
            |key: InputKey| matches!(key, InputKey::Mouse { .. } | InputKey::Wheel { .. });
        // Keep receiving releases after capture/profile changes remove the last mouse binding.
        let mouse = mouse_buttons_held
            || capture
            || state
                .settings
                .global_toggle
                .as_ref()
                .is_some_and(|c| uses_mouse(c.trigger))
            || state.active_profile().is_some_and(|p| {
                p.bindings
                    .iter()
                    .any(|b| b.enabled && uses_mouse(b.chord.trigger))
            });
        if mouse != self.mouse {
            register(self.hwnd, 2, mouse)?;
            self.mouse = mouse;
        }
        Ok(())
    }
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
