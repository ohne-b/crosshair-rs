use std::mem::{size_of, transmute};

use anyhow::Result;
use crosshair_platform::identity::wide;
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, HWND},
        System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::*,
        },
    },
    core::{PCSTR, PCWSTR, w},
};

use super::MenuState;

type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type AllowDarkModeForWindow = unsafe extern "system" fn(HWND, bool) -> bool;
type FlushMenuThemes = unsafe extern "system" fn();

pub(super) struct Theme {
    library: HMODULE,
    set_mode: SetPreferredAppMode,
    allow_window: AllowDarkModeForWindow,
    flush: FlushMenuThemes,
}

impl Theme {
    pub fn new() -> Option<Self> {
        unsafe {
            let library =
                LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            // shortcut: Windows 11 dark menus need ordinal exports; replace when a public API exists.
            let exports = (
                GetProcAddress(library, PCSTR(135 as *const u8)),
                GetProcAddress(library, PCSTR(133 as *const u8)),
                GetProcAddress(library, PCSTR(136 as *const u8)),
            );
            if let (Some(set_mode), Some(allow_window), Some(flush)) = exports {
                Some(Self {
                    library,
                    set_mode: transmute::<unsafe extern "system" fn() -> isize, SetPreferredAppMode>(
                        set_mode,
                    ),
                    allow_window: transmute::<
                        unsafe extern "system" fn() -> isize,
                        AllowDarkModeForWindow,
                    >(allow_window),
                    flush: transmute::<unsafe extern "system" fn() -> isize, FlushMenuThemes>(
                        flush,
                    ),
                })
            } else {
                let _ = FreeLibrary(library);
                None
            }
        }
    }

    pub fn prepare(&self, hwnd: HWND) {
        unsafe {
            let mut contrast = HIGHCONTRASTW {
                cbSize: size_of::<HIGHCONTRASTW>() as u32,
                ..Default::default()
            };
            let dark = SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .is_ok()
                && !contrast.dwFlags.contains(HCF_HIGHCONTRASTON);
            (self.set_mode)(if dark { 2 } else { 0 }); // ForceDark / Default
            (self.allow_window)(hwnd, dark);
            (self.flush)();
        }
    }
}

impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.library);
        }
    }
}

pub(super) struct Popup(pub HMENU);

impl Popup {
    pub fn new(state: &MenuState) -> Result<Self> {
        unsafe {
            let menu = Self(CreatePopupMenu()?);
            let append =
                |flags, id, name: &str| AppendMenuW(menu.0, flags, id, PCWSTR(wide(name).as_ptr()));
            append(MF_STRING, 1, "Open menu")?;
            append(
                MF_STRING | if state.visible { MF_CHECKED } else { MF_STRING },
                2,
                "Show crosshair",
            )?;
            let presets = CreatePopupMenu()?;
            if let Err(error) = append(MF_POPUP, presets.0 as usize, "Crosshairs") {
                let _ = DestroyMenu(presets);
                return Err(error.into());
            }
            for (index, (id, name)) in state.presets.iter().enumerate() {
                let flags = MF_STRING
                    | if state.selected == Some(*id) {
                        MF_CHECKED
                    } else {
                        MF_STRING
                    };
                AppendMenuW(
                    presets,
                    flags,
                    100 + index,
                    PCWSTR(wide(&name.replace('&', "&&")).as_ptr()),
                )?;
            }
            append(MF_SEPARATOR, 0, "")?;
            append(MF_STRING, 3, "Quit")?;
            Ok(menu)
        }
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crosshair_core::Id;

    #[test]
    fn native_theme_applies_without_an_editor_or_visible_window() -> Result<()> {
        unsafe {
            let theme = Theme::new().expect("Supported Windows builds provide native menu theming");
            let owner = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("Tray theme test"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )?;
            theme.prepare(owner);
            let themed = windows::Win32::UI::Controls::IsAppThemed().as_bool();
            let themes_available = windows::Win32::UI::Controls::IsThemeActive().as_bool();
            assert!(!IsWindowVisible(owner).as_bool());
            DestroyWindow(owner)?;
            assert!(
                !themes_available || themed,
                "Native visual styles must be enabled when available"
            );
        }
        Ok(())
    }

    fn item(menu: HMENU, position: u32) -> Result<(MENUITEMINFOW, String)> {
        let mut text = [0u16; 256];
        let mut info = MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_FTYPE | MIIM_STATE | MIIM_SUBMENU | MIIM_STRING | MIIM_ID | MIIM_DATA,
            dwTypeData: windows::core::PWSTR(text.as_mut_ptr()),
            cch: text.len() as u32,
            ..Default::default()
        };
        unsafe {
            GetMenuItemInfoW(menu, position, true, &mut info)?;
        }
        Ok((info, String::from_utf16_lossy(&text[..info.cch as usize])))
    }

    #[test]
    fn windows_owns_menu_labels_checks_and_submenus() -> Result<()> {
        let first = Id::new_v4();
        let second = Id::new_v4();
        for visible in [false, true] {
            let popup = Popup::new(&MenuState {
                presets: vec![(first, "Dot & ring".into()), (second, "白い点".into())],
                selected: Some(second),
                visible,
                ..Default::default()
            })?;
            for position in 0..5 {
                let (info, _) = item(popup.0, position)?;
                assert!(!info.fType.contains(MFT_OWNERDRAW));
                assert_eq!(info.dwItemData, 0);
            }
            assert_eq!(item(popup.0, 0)?.0.wID, 1);
            let toggle = item(popup.0, 1)?.0;
            assert_eq!(toggle.wID, 2);
            assert_eq!(toggle.fState.contains(MFS_CHECKED), visible);
            let submenu = item(popup.0, 2)?.0.hSubMenu;
            let (unselected, name) = item(submenu, 0)?;
            assert_eq!(name, "Dot && ring");
            assert_eq!(unselected.wID, 100);
            assert!(!unselected.fState.contains(MFS_CHECKED));
            let (selected, name) = item(submenu, 1)?;
            assert_eq!(name, "白い点");
            assert_eq!(selected.wID, 101);
            assert!(selected.fState.contains(MFS_CHECKED));
            assert!(item(popup.0, 3)?.0.fType.contains(MFT_SEPARATOR));
            assert_eq!(item(popup.0, 4)?.0.wID, 3);
            drop(popup);
            assert!(!unsafe { IsMenu(submenu) }.as_bool());
        }
        Ok(())
    }
}
