use std::{ffi::c_void, mem::size_of, ptr};

use anyhow::{Result, bail, ensure};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
            CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP,
            HDC, HGDIOBJ, SelectObject,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, HTTRANSPARENT, MA_NOACTIVATE,
            RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow, ULW_ALPHA, UpdateLayeredWindow,
            WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
    core::w,
};

use super::centered_origin;

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pixels: *mut u8,
    size: [u32; 2],
}

impl Dib {
    fn new(size: [u32; 2]) -> Result<Self> {
        ensure!(
            size.iter().all(|&v| v > 0 && v <= 1024),
            "Invalid overlay dimensions"
        );
        // The selected DIB remains alive until it is deselected in Drop.
        unsafe {
            let dc = CreateCompatibleDC(None);
            ensure!(!dc.is_invalid(), "Could not create overlay device context");
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size[0] as i32,
                    biHeight: -(size[1] as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels: *mut c_void = ptr::null_mut();
            let bitmap =
                match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut pixels, None, 0) {
                    Ok(bitmap) => bitmap,
                    Err(error) => {
                        let _ = DeleteDC(dc);
                        return Err(error.into());
                    }
                };
            if pixels.is_null() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                bail!("Windows returned an empty bitmap");
            }
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
            if previous.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                bail!("Could not select overlay bitmap");
            }
            Ok(Self {
                dc,
                bitmap,
                previous,
                pixels: pixels.cast(),
                size,
            })
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct DesktopOverlay {
    hwnd: HWND,
    dib: Option<Dib>,
}

impl DesktopOverlay {
    pub fn new() -> Result<Self> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class_name = w!("CrosshairRs.DesktopOverlay");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class_name,
                ..Default::default()
            };
            // Multiple presentation windows can reuse this process-owned class.
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class_name,
                w!("crosshair-rs overlay"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )?;
            Ok(Self { hwnd, dib: None })
        }
    }

    pub fn present(&mut self, size: [u32; 2], rgba: &[u8], center: [i32; 2]) -> Result<()> {
        self.present_at(size, rgba, centered_origin(center, size))
    }

    pub fn present_at(&mut self, size: [u32; 2], rgba: &[u8], origin: [i32; 2]) -> Result<()> {
        self.prepare_at(size, rgba, origin)?;
        self.show();
        Ok(())
    }

    pub fn prepare_at(&mut self, size: [u32; 2], rgba: &[u8], origin: [i32; 2]) -> Result<()> {
        ensure!(
            size.iter().all(|&v| v > 0 && v <= 1024),
            "Invalid overlay dimensions"
        );
        ensure!(
            rgba.len() == size[0] as usize * size[1] as usize * 4,
            "Invalid overlay pixel length"
        );
        if self.dib.as_ref().is_none_or(|dib| dib.size != size) {
            self.dib = Some(Dib::new(size)?);
        }
        let dib = self.dib.as_mut().expect("bitmap was allocated");
        unsafe {
            let output = std::slice::from_raw_parts_mut(dib.pixels, rgba.len());
            for (source, destination) in rgba.chunks_exact(4).zip(output.chunks_exact_mut(4)) {
                destination.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
            }
            let destination = POINT {
                x: origin[0],
                y: origin[1],
            };
            let source = POINT::default();
            let extent = SIZE {
                cx: size[0] as i32,
                cy: size[1] as i32,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
                ..Default::default()
            };
            UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&destination),
                Some(&extent),
                Some(dib.dc),
                Some(&source),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )?;
        }
        Ok(())
    }

    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for DesktopOverlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
