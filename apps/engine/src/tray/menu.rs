use std::mem::size_of;

use anyhow::{Result, ensure};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT},
    Graphics::Gdi::*,
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_DISABLED, ODS_SELECTED, ODT_MENU},
        HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi},
        WindowsAndMessaging::*,
    },
};

struct Item {
    text: Vec<u16>,
    font: HFONT,
    width: u32,
    height: u32,
    inset: i32,
    checked: bool,
    separator: bool,
    submenu: bool,
}

pub(super) struct Style {
    background: HBRUSH,
    font: HFONT,
    scale: f32,
    // Menu item data points into these allocations for the entire native menu loop.
    #[allow(clippy::vec_box)]
    items: Vec<Box<Item>>,
}

impl Style {
    pub fn new(hwnd: HWND) -> Result<Option<Self>> {
        unsafe {
            let mut contrast = HIGHCONTRASTW {
                cbSize: size_of::<HIGHCONTRASTW>() as u32,
                ..Default::default()
            };
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )?;
            if contrast.dwFlags.contains(HCF_HIGHCONTRASTON) {
                return Ok(None);
            }
            let dpi = GetDpiForWindow(hwnd).max(96);
            let mut metrics = NONCLIENTMETRICSW {
                cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
                ..Default::default()
            };
            SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS.0,
                metrics.cbSize,
                Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()),
                0,
                dpi,
            )?;
            let mut style = Self {
                background: HBRUSH::default(),
                font: HFONT::default(),
                scale: dpi as f32 / 96.0,
                items: Vec::new(),
            };
            style.background = CreateSolidBrush(COLORREF(0x202020));
            style.font = CreateFontIndirectW(&metrics.lfMenuFont);
            ensure!(
                !style.background.is_invalid() && !style.font.is_invalid(),
                "Could not create tray menu resources"
            );
            Ok(Some(style))
        }
    }

    pub fn apply(&mut self, menu: HMENU) -> Result<()> {
        unsafe {
            SetMenuInfo(
                menu,
                &MENUINFO {
                    cbSize: size_of::<MENUINFO>() as u32,
                    fMask: MIM_BACKGROUND | MIM_STYLE,
                    hbrBack: self.background,
                    dwStyle: MNS_NOCHECK,
                    ..Default::default()
                },
            )?;
            for index in 0..GetMenuItemCount(Some(menu)) {
                let mut text = [0u16; 256];
                let mut info = MENUITEMINFOW {
                    cbSize: size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_FTYPE | MIIM_STATE | MIIM_SUBMENU | MIIM_STRING,
                    dwTypeData: windows::core::PWSTR(text.as_mut_ptr()),
                    cch: text.len() as u32,
                    ..Default::default()
                };
                GetMenuItemInfoW(menu, index as u32, true, &mut info)?;
                if !info.hSubMenu.is_invalid() {
                    self.apply(info.hSubMenu)?;
                }
                let text: Vec<u16> = text[..(info.cch as usize).min(text.len())].to_vec();
                let separator = info.fType.contains(MFT_SEPARATOR);
                let mut bounds = RECT::default();
                let dc = GetDC(None);
                let previous = SelectObject(dc, self.font.into());
                let mut measured = text.clone();
                DrawTextW(dc, &mut measured, &mut bounds, DT_CALCRECT | DT_SINGLELINE);
                SelectObject(dc, previous);
                ReleaseDC(None, dc);
                let item = Box::new(Item {
                    text,
                    font: self.font,
                    width: ((bounds.right - bounds.left) as f32 + 72.0 * self.scale)
                        .max(210.0 * self.scale) as u32,
                    height: ((if separator { 9.0 } else { 34.0 }) * self.scale) as u32,
                    inset: (28.0 * self.scale) as i32,
                    checked: info.fState.contains(MFS_CHECKED),
                    separator,
                    submenu: !info.hSubMenu.is_invalid(),
                });
                info.fMask = MIIM_FTYPE | MIIM_DATA;
                info.fType |= MFT_OWNERDRAW;
                info.dwItemData = item.as_ref() as *const Item as usize;
                self.items.push(item);
                SetMenuItemInfoW(menu, index as u32, true, &info)?;
            }
        }
        Ok(())
    }
}

impl Drop for Style {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.background.into());
        }
    }
}

pub(super) unsafe fn message(message: u32, lparam: LPARAM) -> Option<LRESULT> {
    unsafe {
        if message == WM_MEASUREITEM {
            let measure = &mut *(lparam.0 as *mut MEASUREITEMSTRUCT);
            if measure.CtlType != ODT_MENU || measure.itemData == 0 {
                return None;
            }
            let item = &*(measure.itemData as *const Item);
            measure.itemWidth = item.width;
            measure.itemHeight = item.height;
            return Some(LRESULT(1));
        }
        if message != WM_DRAWITEM {
            return None;
        }
        let draw = &*(lparam.0 as *const DRAWITEMSTRUCT);
        if draw.CtlType != ODT_MENU || draw.itemData == 0 {
            return None;
        }
        let item = &*(draw.itemData as *const Item);
        let background = CreateSolidBrush(COLORREF(if draw.itemState.0 & ODS_SELECTED.0 != 0 {
            0x333333
        } else {
            0x202020
        }));
        FillRect(draw.hDC, &draw.rcItem, background);
        let _ = DeleteObject(background.into());
        if item.separator {
            let line = CreateSolidBrush(COLORREF(0x383838));
            let center = (draw.rcItem.top + draw.rcItem.bottom) / 2;
            FillRect(
                draw.hDC,
                &RECT {
                    left: draw.rcItem.left + 10,
                    right: draw.rcItem.right - 10,
                    top: center,
                    bottom: center + 1,
                },
                line,
            );
            let _ = DeleteObject(line.into());
            return Some(LRESULT(1));
        }
        let saved = SaveDC(draw.hDC);
        SelectObject(draw.hDC, item.font.into());
        SetBkMode(draw.hDC, TRANSPARENT);
        SetTextColor(
            draw.hDC,
            COLORREF(if draw.itemState.0 & ODS_DISABLED.0 != 0 {
                0x808080
            } else {
                0xeeeeee
            }),
        );
        let mut text = item.text.clone();
        let mut bounds = draw.rcItem;
        bounds.left += item.inset;
        bounds.right -= item.inset;
        DrawTextW(
            draw.hDC,
            &mut text,
            &mut bounds,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        if item.checked {
            SetTextColor(draw.hDC, COLORREF(0x1678f9));
            let mut check: Vec<u16> = "✓".encode_utf16().collect();
            let mut bounds = draw.rcItem;
            bounds.right = bounds.left + item.inset;
            DrawTextW(
                draw.hDC,
                &mut check,
                &mut bounds,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER,
            );
        }
        if item.submenu {
            let mut arrow: Vec<u16> = "›".encode_utf16().collect();
            let mut bounds = draw.rcItem;
            bounds.left = bounds.right - item.inset;
            DrawTextW(
                draw.hDC,
                &mut arrow,
                &mut bounds,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER,
            );
        }
        let _ = RestoreDC(draw.hDC, saved);
        Some(LRESULT(1))
    }
}
