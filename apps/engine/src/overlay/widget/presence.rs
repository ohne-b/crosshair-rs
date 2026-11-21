use std::{ffi::c_void, fs::File, path::PathBuf};

use anyhow::Result;
use windows::{
    Win32::{
        Foundation::{ERROR_SHARING_VIOLATION, HANDLE, HWND, LPARAM, WPARAM},
        Storage::FileSystem::{
            FILE_NOTIFY_CHANGE_FILE_NAME, FindCloseChangeNotification,
            FindFirstChangeNotificationW, FindNextChangeNotification,
        },
        System::Threading::{
            CloseThreadpoolWait, CreateThreadpoolWait, PTP_CALLBACK_INSTANCE, PTP_WAIT,
            SetThreadpoolWait, WaitForThreadpoolWaitCallbacks,
        },
        UI::WindowsAndMessaging::{PostMessageW, WM_APP},
    },
    core::HSTRING,
};

pub const CHANGED: u32 = WM_APP + 6;

pub struct WindowPresence {
    marker: PathBuf,
    changes: HANDLE,
    wait: PTP_WAIT,
}

impl WindowPresence {
    pub fn for_package(session: u32, hwnd: HWND) -> Result<Self> {
        let folder = windows::Storage::ApplicationData::Current()?
            .TemporaryFolder()?
            .Path()?;
        Self::watch(folder.to_os_string().into(), session, hwnd)
    }

    fn watch(folder: PathBuf, session: u32, hwnd: HWND) -> Result<Self> {
        unsafe {
            let changes = FindFirstChangeNotificationW(
                &HSTRING::from(folder.as_os_str()),
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME,
            )?;
            let wait = match CreateThreadpoolWait(Some(notify), Some(hwnd.0), None) {
                Ok(wait) => wait,
                Err(error) => {
                    let _ = FindCloseChangeNotification(changes);
                    return Err(error.into());
                }
            };
            SetThreadpoolWait(wait, Some(changes), None);
            Ok(Self {
                marker: folder.join(format!("crosshair-widget-{session}.open")),
                changes,
                wait,
            })
        }
    }

    pub fn is_open(&self) -> bool {
        // Only a live widget holds this file exclusively. A leftover name alone proves nothing.
        File::open(&self.marker)
            .is_err_and(|e| e.raw_os_error() == Some(ERROR_SHARING_VIOLATION.0 as i32))
    }

    pub fn rearm(&self) -> Result<()> {
        unsafe {
            FindNextChangeNotification(self.changes)?;
            SetThreadpoolWait(self.wait, Some(self.changes), None);
        }
        Ok(())
    }
}

unsafe extern "system" fn notify(_: PTP_CALLBACK_INSTANCE, hwnd: *mut c_void, _: PTP_WAIT, _: u32) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd)), CHANGED, WPARAM(0), LPARAM(0));
    }
}

impl Drop for WindowPresence {
    fn drop(&mut self) {
        unsafe {
            SetThreadpoolWait(self.wait, None, None);
            WaitForThreadpoolWaitCallbacks(self.wait, true);
            CloseThreadpoolWait(self.wait);
            let _ = FindCloseChangeNotification(self.changes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::{
        Storage::FileSystem::FILE_FLAG_DELETE_ON_CLOSE,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetMessageW, HWND_MESSAGE, MSG, SetTimer, WM_TIMER,
        },
    };
    use windows::core::w;

    #[test]
    fn widget_open_and_close_notify_without_a_render_connection() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let session = 17;
        let marker = folder.path().join("crosshair-widget-17.open");
        std::fs::write(&marker, [])?;
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                None,
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )?
        };
        let watch = WindowPresence::watch(folder.path().into(), session, hwnd)?;
        assert!(!watch.is_open(), "A stale, unlocked marker is closed");
        assert_ne!(unsafe { SetTimer(Some(hwnd), 1, 5000, None) }, 0);
        let next_state = || -> Result<bool> {
            let mut msg = MSG::default();
            loop {
                anyhow::ensure!(unsafe { GetMessageW(&mut msg, Some(hwnd), 0, 0) }.0 > 0);
                anyhow::ensure!(msg.message != WM_TIMER, "No widget lifetime notification");
                if msg.message == CHANGED {
                    watch.rearm()?;
                    return Ok(watch.is_open());
                }
            }
        };
        std::fs::remove_file(&marker)?;
        let lease = File::options()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
            .open(&marker)?;
        assert!(next_state()?);
        // An engine starting while the widget is suspended can discover the existing window.
        let after_restart = WindowPresence::watch(folder.path().into(), session, hwnd)?;
        assert!(after_restart.is_open());
        drop(after_restart);
        drop(lease);
        assert!(!next_state()?);
        assert!(!marker.exists());
        drop(watch);
        unsafe {
            DestroyWindow(hwnd)?;
        }
        Ok(())
    }
}
