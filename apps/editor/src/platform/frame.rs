use std::{mem::size_of, os::windows::io::OwnedHandle};

use anyhow::{Result, bail};
use crosshair_platform::identity::{Identity, owned, wide};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::{
    Win32::{
        Foundation::{
            ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
        },
        Graphics::{
            Dwm::{
                DWMWA_BORDER_COLOR, DWMWA_CAPTION_BUTTON_BOUNDS, DWMWA_CAPTION_COLOR,
                DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
                DWMWCP_ROUND, DwmDefWindowProc, DwmExtendFrameIntoClientArea,
                DwmGetWindowAttribute, DwmSetWindowAttribute,
            },
            Gdi::{
                CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST,
                MONITORINFO, MonitorFromWindow, ScreenToClient, SetWindowRgn,
            },
        },
        System::Threading::CreateMutexW,
        UI::{
            Controls::MARGINS,
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, PCWSTR},
};

pub struct Instance {
    _mutex: OwnedHandle,
}

impl Instance {
    pub fn acquire(scope: &str) -> Result<Option<Self>> {
        let identity = Identity::current()?;
        let name = wide(&format!(
            "Local\\crosshair-rs.editor.{}.{}.{}",
            identity.user_sid, identity.session, scope
        ));
        let mutex = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr()))? };
        let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let mutex = unsafe { owned(mutex) };
        if existing {
            let property = wide(&format!("crosshair-rs.editor.scope.{scope}"));
            unsafe {
                let _ = EnumWindows(Some(focus_existing), LPARAM(property.as_ptr() as isize));
            }
            return Ok(None);
        }
        Ok(Some(Self { _mutex: mutex }))
    }
}

unsafe extern "system" fn focus_existing(hwnd: HWND, scope: LPARAM) -> BOOL {
    unsafe {
        if GetPropW(hwnd, PCWSTR(scope.0 as *const u16)).is_invalid() {
            return true.into();
        }
        let mut title = [0u16; 128];
        let length = GetWindowTextW(hwnd, &mut title);
        if String::from_utf16_lossy(&title[..length.max(0) as usize]) == "crosshair-rs" {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if let (Ok(current), Ok(peer)) = (Identity::current(), Identity::process(pid))
                && current
                    .verify_peer(&peer, &["crosshair-editor.exe"])
                    .is_ok()
            {
                focus(hwnd);
                return false.into();
            }
        }
        true.into()
    }
}

pub fn handle(window: &slint::Window) -> Result<HWND> {
    match window.window_handle().window_handle()?.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
        _ => bail!("The editor requires a Windows window"),
    }
}

pub fn install(window: &slint::Window, scope: &str) -> Result<()> {
    let hwnd = handle(window)?;
    unsafe {
        let property = wide(&format!("crosshair-rs.editor.scope.{scope}"));
        SetPropW(
            hwnd,
            PCWSTR(property.as_ptr()),
            Some(windows::Win32::Foundation::HANDLE(std::ptr::dangling_mut())),
        )?;
        let dark = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        );
        apply_colors(
            hwnd,
            crate::platform::accessibility::current()
                .ok()
                .and_then(|appearance| appearance.colors),
        );
        let corners = DWMWCP_ROUND.0;
        let native_corners = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corners as *const i32).cast(),
            size_of::<i32>() as u32,
        )
        .is_ok();
        if !SetWindowSubclass(
            hwnd,
            Some(procedure),
            0x435248,
            usize::from(!native_corners),
        )
        .as_bool()
        {
            bail!("Could not enable native window controls");
        }
        extend_frame(hwnd);
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        )?;
        crate::app::work_area_changed(hwnd);
        let mut monitor = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        )
        .as_bool()
        {
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect)?;
            let width = (rect.right - rect.left).min(monitor.rcWork.right - monitor.rcWork.left);
            let height = (rect.bottom - rect.top).min(monitor.rcWork.bottom - monitor.rcWork.top);
            let x = rect
                .left
                .clamp(monitor.rcWork.left, monitor.rcWork.right - width);
            let y = rect
                .top
                .clamp(monitor.rcWork.top, monitor.rcWork.bottom - height);
            SetWindowPos(hwnd, None, x, y, width, height, SWP_NOZORDER)?;
        }
        if !native_corners {
            round_legacy_frame(hwnd);
        }
    }
    Ok(())
}

pub fn focus(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn apply_colors(hwnd: HWND, colors: Option<[u32; 4]>) {
    let (border, caption, text) = colors.map_or((0x303030, 0x1e1e1e, 0xf5f4f4), |colors| {
        (colors[1], colors[0], colors[1])
    });
    for (attribute, value) in [
        (DWMWA_BORDER_COLOR, border),
        (DWMWA_CAPTION_COLOR, caption),
        (DWMWA_TEXT_COLOR, text),
    ] {
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                attribute,
                (&value as *const u32).cast(),
                size_of::<u32>() as u32,
            );
        }
    }
}
fn extend_frame(hwnd: HWND) {
    // DWM owns the caption buttons and the rounded outer edge.
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(
            hwnd,
            &MARGINS {
                cxLeftWidth: 1,
                cxRightWidth: 1,
                cyTopHeight: (48 * GetDpiForWindow(hwnd).max(96) / 96) as i32,
                cyBottomHeight: 1,
            },
        );
    }
}
pub fn pointer_position(hwnd: HWND) -> Result<[f32; 2]> {
    let mut point = windows::Win32::Foundation::POINT::default();
    unsafe {
        GetCursorPos(&mut point)?;
        anyhow::ensure!(
            windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point).as_bool(),
            "Could not locate the pointer in the editor"
        );
    }
    Ok([point.x as f32, point.y as f32])
}
pub fn work_area(hwnd: HWND) -> Result<[f32; 2]> {
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        anyhow::ensure!(
            GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &mut monitor
            )
            .as_bool(),
            "Could not read the monitor work area"
        );
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        Ok([
            (monitor.rcWork.right - monitor.rcWork.left) as f32 / scale,
            (monitor.rcWork.bottom - monitor.rcWork.top) as f32 / scale,
        ])
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    use_corner_region: usize,
) -> LRESULT {
    unsafe {
        // Keep native caption hit testing, hover states, Snap and system actions.
        let mut result = LRESULT(0);
        if DwmDefWindowProc(hwnd, message, wparam, lparam, &mut result).as_bool()
            && message != WM_NCHITTEST
        {
            return result;
        }
        match message {
            WM_ACTIVATE => extend_frame(hwnd),
            WM_SIZE if use_corner_region != 0 => round_legacy_frame(hwnd),
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                crate::app::system_appearance_changed();
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                extend_frame(hwnd);
                crate::app::work_area_changed(hwnd);
            }
            WM_GETMINMAXINFO => {
                if let Ok([width, height]) = work_area(hwnd) {
                    let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
                    let info = &mut *(lparam.0 as *mut MINMAXINFO);
                    info.ptMinTrackSize.x = (width.min(960.0) * scale).round() as i32;
                    info.ptMinTrackSize.y = (height.min(640.0) * scale).round() as i32;
                    return LRESULT(0);
                }
            }
            WM_NCCALCSIZE if wparam.0 != 0 => {
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                let top = params.rgrc[0].top;
                // Let Windows lay out its caption buttons before extending the client area.
                DefWindowProcW(hwnd, message, wparam, lparam);
                params.rgrc[0].top = top;
                if IsZoomed(hwnd).as_bool() {
                    let mut monitor = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if GetMonitorInfoW(
                        MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                        &mut monitor,
                    )
                    .as_bool()
                    {
                        params.rgrc[0] = monitor.rcWork;
                    }
                }
                return LRESULT(0);
            }
            WM_NCHITTEST => {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect).is_ok() {
                    let x = (lparam.0 as u16 as i16) as i32 - rect.left;
                    let y = ((lparam.0 >> 16) as u16 as i16) as i32 - rect.top;
                    let width = rect.right - rect.left;
                    let height = rect.bottom - rect.top;
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    let border = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    let mut client = RECT::default();
                    let mut point = POINT {
                        x: x + rect.left,
                        y: y + rect.top,
                    };
                    if GetClientRect(hwnd, &mut client).is_ok()
                        && ScreenToClient(hwnd, &mut point).as_bool()
                    {
                        let scale = dpi as f32 / 96.0;
                        let mut buttons = RECT::default();
                        let _ = DwmGetWindowAttribute(
                            hwnd,
                            DWMWA_CAPTION_BUTTON_BOUNDS,
                            (&mut buttons as *mut RECT).cast(),
                            size_of::<RECT>() as u32,
                        );
                        let size = if buttons.right > buttons.left && buttons.bottom > buttons.top {
                            [buttons.right - buttons.left, buttons.bottom - buttons.top]
                        } else {
                            [(138.0 * scale) as i32, (32.0 * scale) as i32]
                        };
                        // The maximized frame has an invisible resize inset; hit-test in client coordinates.
                        let caption = caption_hit_test(point.x, point.y, client.right, scale, size);
                        if matches!(caption, HTMINBUTTON | HTMAXBUTTON | HTCLOSE) {
                            return LRESULT(caption as isize);
                        }
                        result = LRESULT(caption as isize);
                    }
                    let hit =
                        resize_hit_test(x, y, width, height, border, IsZoomed(hwnd).as_bool());
                    return if hit == HTCLIENT {
                        result
                    } else {
                        LRESULT(hit as isize)
                    };
                }
            }
            // Winit injects a dummy mouse move for captions, which cancels our native drag loop.
            WM_NCLBUTTONDOWN if wparam.0 == HTCAPTION as usize => {
                return DefWindowProcW(hwnd, message, wparam, lparam);
            }
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(procedure), id);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

unsafe fn round_legacy_frame(hwnd: HWND) {
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            SetWindowRgn(hwnd, None, true);
        } else {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let diameter = (16 * GetDpiForWindow(hwnd).max(96) / 96) as i32;
                let region = CreateRoundRectRgn(
                    0,
                    0,
                    rect.right - rect.left + 1,
                    rect.bottom - rect.top + 1,
                    diameter,
                    diameter,
                );
                // Windows owns the region after a successful assignment.
                if !region.is_invalid() && SetWindowRgn(hwnd, Some(region), true) == 0 {
                    let _ = DeleteObject(region.into());
                }
            }
        }
    }
}

fn resize_hit_test(x: i32, y: i32, width: i32, height: i32, border: i32, maximized: bool) -> u32 {
    if !maximized {
        let left = x < border;
        let right = x >= width - border;
        let top = y < border;
        let bottom = y >= height - border;
        match (left, right, top, bottom) {
            (true, _, true, _) => return HTTOPLEFT,
            (_, true, true, _) => return HTTOPRIGHT,
            (true, _, _, true) => return HTBOTTOMLEFT,
            (_, true, _, true) => return HTBOTTOMRIGHT,
            (true, _, _, _) => return HTLEFT,
            (_, true, _, _) => return HTRIGHT,
            (_, _, true, _) => return HTTOP,
            (_, _, _, true) => return HTBOTTOM,
            _ => {}
        }
    }
    HTCLIENT
}

fn caption_hit_test(x: i32, y: i32, width: i32, scale: f32, buttons: [i32; 2]) -> u32 {
    if y >= 0 && y < buttons[1] && x >= width - buttons[0] && x < width {
        let index = ((width - 1 - x) * 3 / buttons[0]).min(2);
        return [HTCLOSE, HTMAXBUTTON, HTMINBUTTON][index as usize];
    }
    let logical_x = x as f32 / scale;
    let logical_y = y as f32 / scale;
    let logical_width = width as f32 / scale;
    // The 440px controls group matches shell/titlebar.slint. Windows owns the
    // remaining caption so a native move loop cannot strand a Slint pointer grab.
    if (0.0..48.0).contains(&logical_y) && logical_x >= 0.0 && logical_x < logical_width - 440.0 {
        return HTCAPTION;
    }
    HTCLIENT
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_keeps_resize_edges_and_header_controls_at_mixed_dpi() {
        assert_eq!(resize_hit_test(2, 2, 1280, 800, 8, false), HTTOPLEFT);
        assert_eq!(resize_hit_test(2, 2, 1280, 800, 8, true), HTCLIENT);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let width = (1280.0 * scale) as i32;
            let buttons = [(138.0 * scale) as i32, (30.0 * scale) as i32];
            for (offset, expected) in [(23.0, HTCLOSE), (69.0, HTMAXBUTTON), (115.0, HTMINBUTTON)] {
                assert_eq!(
                    caption_hit_test(
                        width - (offset * scale) as i32,
                        (14.0 * scale) as i32,
                        width,
                        scale,
                        buttons
                    ),
                    expected
                );
            }
            assert_eq!(
                caption_hit_test(
                    (900.0 * scale) as i32,
                    (20.0 * scale) as i32,
                    width,
                    scale,
                    buttons
                ),
                HTCLIENT
            );
            assert_eq!(
                caption_hit_test(
                    (500.0 * scale) as i32,
                    (20.0 * scale) as i32,
                    width,
                    scale,
                    buttons
                ),
                HTCAPTION
            );
            assert_eq!(
                caption_hit_test(
                    (500.0 * scale) as i32,
                    (200.0 * scale) as i32,
                    width,
                    scale,
                    buttons
                ),
                HTCLIENT
            );
        }
    }
}
