use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use crosshair_platform::identity::wide;
use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
            FOS_OVERWRITEPROMPT, FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog,
            SIGDN_FILESYSPATH, ShellExecuteW,
        },
    },
    core::{HRESULT, PCWSTR},
};

pub enum FileKind {
    Document,
    Image,
    Png,
    Svg,
}

pub fn choose(owner: HWND, save: bool, kind: FileKind, suggested: &str) -> Result<Option<PathBuf>> {
    let (description, filter, extension) = match kind {
        FileKind::Document => ("crosshair-rs files", "*.json;*.crosshair", "json"),
        FileKind::Image => ("Images", "*.png;*.jpg;*.jpeg;*.gif;*.svg", "png"),
        FileKind::Png => ("PNG image", "*.png", "png"),
        FileKind::Svg => ("SVG image", "*.svg", "svg"),
    };
    let description = wide(description);
    let filter = wide(filter);
    let types = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(description.as_ptr()),
        pszSpec: PCWSTR(filter.as_ptr()),
    }];
    unsafe {
        let result = if save {
            let dialog: IFileSaveDialog =
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_OVERWRITEPROMPT)?;
            dialog.SetFileTypes(&types)?;
            dialog.SetDefaultExtension(PCWSTR(wide(extension).as_ptr()))?;
            dialog.SetFileName(PCWSTR(wide(suggested).as_ptr()))?;
            match dialog.Show(Some(owner)) {
                Ok(()) => dialog.GetResult(),
                Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_FILEMUSTEXIST)?;
            dialog.SetFileTypes(&types)?;
            match dialog.Show(Some(owner)) {
                Ok(()) => dialog.GetResult(),
                Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            }
        }?;
        let path = result.GetDisplayName(SIGDN_FILESYSPATH)?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        Ok(Some(PathBuf::from(text?)))
    }
}

pub fn open_url(owner: HWND, url: &str) -> Result<()> {
    ensure!(
        url.starts_with("https://github.com/ohne-b/crosshair-rs")
            || url == "https://slint.dev"
            || url == "https://polyformproject.org/licenses/noncommercial/1.0.0/",
        "This link is not an application link"
    );
    let result = unsafe {
        ShellExecuteW(
            Some(owner),
            PCWSTR(wide("open").as_ptr()),
            PCWSTR(wide(url).as_ptr()),
            None,
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    ensure!(result.0 as usize > 32, "Windows could not open this link");
    Ok(())
}

struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

pub fn copy(owner: HWND, text: &str) -> Result<()> {
    unsafe {
        OpenClipboard(Some(owner)).context("The clipboard is in use. Retry.")?;
        let _clipboard = Clipboard;
        let data = wide(text);
        let memory = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2)?;
        let pointer = GlobalLock(memory);
        if pointer.is_null() {
            let _ = GlobalFree(Some(memory));
            anyhow::bail!("Could not allocate clipboard memory");
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), pointer.cast::<u16>(), data.len());
        let _ = GlobalUnlock(memory);
        if let Err(error) =
            EmptyClipboard().and_then(|()| SetClipboardData(13, Some(HANDLE(memory.0))).map(|_| ()))
        {
            let _ = GlobalFree(Some(memory));
            return Err(error.into());
        }
        Ok(())
    }
}

pub fn paste(owner: HWND) -> Result<String> {
    unsafe {
        OpenClipboard(Some(owner)).context("The clipboard is in use. Retry.")?;
        let _clipboard = Clipboard;
        let memory = HGLOBAL(GetClipboardData(13)?.0);
        let size = GlobalSize(memory);
        ensure!(
            size <= crosshair_core::MAX_CONFIG_BYTES * 2,
            "Clipboard exceeds the document limit"
        );
        let pointer = GlobalLock(memory);
        ensure!(!pointer.is_null(), "Could not read clipboard");
        let data = std::slice::from_raw_parts(pointer.cast::<u16>(), size / 2);
        let length = data
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(data.len());
        let text = String::from_utf16(&data[..length]);
        let _ = GlobalUnlock(memory);
        Ok(text?)
    }
}
