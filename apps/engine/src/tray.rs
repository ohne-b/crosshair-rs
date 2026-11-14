mod menu;

use std::{
    mem::size_of,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

use anyhow::{Result, ensure};
use crosshair_core::{AppState, Id};
use crosshair_platform::identity::wide;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
                NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, PCWSTR, w},
};

const CALLBACK: u32 = WM_APP + 1;
const SAVE_ERROR: u32 = WM_APP + 2;

pub enum Action {
    Open,
    Toggle,
    Preset(Id),
    Quit,
    RetryQuit,
    DiscardQuit,
}

#[derive(Clone, Default)]
struct MenuState {
    revision: Option<u64>,
    presets: Vec<(Id, String)>,
    selected: Option<Id>,
    visible: bool,
    enabled: bool,
}
struct Shared {
    menu: Mutex<MenuState>,
    error: Mutex<Option<String>>,
    stopped: AtomicBool,
    dialog_open: AtomicBool,
}
struct Context {
    icon: Tray,
    shared: Arc<Shared>,
    notify: Box<dyn Fn(Action) + Send>,
    taskbar_created: u32,
}

// Native menus/dialogs run nested message loops. Keep them away from the input/state owner.
pub struct Worker {
    hwnd: usize,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(notify: impl Fn(Action) + Send + 'static) -> Result<Self> {
        let shared = Arc::new(Shared {
            menu: Mutex::new(MenuState::default()),
            error: Mutex::new(None),
            stopped: AtomicBool::new(false),
            dialog_open: AtomicBool::new(false),
        });
        let state = shared.clone();
        let (ready, receiver) = mpsc::sync_channel(1);
        let thread =
            thread::Builder::new()
                .name("crosshair tray".into())
                .spawn(move || unsafe {
                    let start = (|| -> Result<(HWND, Box<Context>)> {
                        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
                        let class = WNDCLASSW {
                            lpfnWndProc: Some(window_proc),
                            hInstance: instance,
                            lpszClassName: w!("CrosshairRs.Tray"),
                            ..Default::default()
                        };
                        ensure!(
                            RegisterClassW(&class) != 0,
                            "Could not register the tray window"
                        );
                        let hwnd = CreateWindowExW(
                            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                            class.lpszClassName,
                            w!("crosshair-rs"),
                            WS_POPUP,
                            0,
                            0,
                            0,
                            0,
                            None,
                            None,
                            Some(instance),
                            None,
                        )?;
                        let icon = match Tray::new(hwnd, CALLBACK) {
                            Ok(icon) => icon,
                            Err(error) => {
                                let _ = DestroyWindow(hwnd);
                                return Err(error);
                            }
                        };
                        let context = Box::new(Context {
                            icon,
                            shared: state,
                            notify: Box::new(notify),
                            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
                        });
                        SetWindowLongPtrW(
                            hwnd,
                            GWLP_USERDATA,
                            context.as_ref() as *const Context as isize,
                        );
                        Ok((hwnd, context))
                    })();
                    match start {
                        Ok((hwnd, context)) => {
                            let _ = ready.send(Ok(hwnd.0 as usize));
                            let mut message = MSG::default();
                            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                                let _ = TranslateMessage(&message);
                                DispatchMessageW(&message);
                            }
                            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                            drop(context);
                            let _ = DestroyWindow(hwnd);
                        }
                        Err(error) => {
                            let _ = ready.send(Err(error));
                        }
                    }
                })?;
        match receiver.recv() {
            Ok(Ok(hwnd)) => Ok(Self {
                hwnd,
                shared,
                thread: Some(thread),
            }),
            result => {
                let _ = thread.join();
                match result {
                    Ok(Err(error)) => Err(error),
                    _ => anyhow::bail!("The tray could not start"),
                }
            }
        }
    }

    pub fn update(
        &self,
        state: &AppState,
        status: &crosshair_core::protocol::EngineStatus,
        enabled: bool,
    ) {
        let mut menu = self.shared.menu.lock().unwrap();
        if menu.revision != Some(state.revision) {
            menu.presets = state
                .presets
                .iter()
                .map(|preset| (preset.id, preset.name.clone()))
                .collect();
            menu.revision = Some(state.revision);
        }
        menu.selected = status.selected_preset;
        menu.visible = status.visible;
        menu.enabled = enabled;
    }

    pub fn save_error(&self, error: &str) {
        *self.shared.error.lock().unwrap() = Some(error.into());
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                SAVE_ERROR,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

unsafe extern "system" fn close_dialog(hwnd: HWND, owner: LPARAM) -> BOOL {
    if unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|window| window.0 as isize == owner.0) {
        unsafe {
            SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
        }
    }
    BOOL(1)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Context;
    let Some(context) = (unsafe { pointer.as_ref() }) else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    if message == WM_CLOSE {
        context.shared.stopped.store(true, Ordering::Release);
        unsafe {
            let _ = EndMenu();
            let _ = EnumThreadWindows(
                GetCurrentThreadId(),
                Some(close_dialog),
                LPARAM(hwnd.0 as isize),
            );
            PostQuitMessage(0);
        }
        return LRESULT(0);
    }
    if context.shared.stopped.load(Ordering::Acquire) {
        return LRESULT(0);
    }
    if message == context.taskbar_created {
        let _ = context.icon.restore();
        return LRESULT(0);
    }
    if message == CALLBACK && !context.shared.dialog_open.load(Ordering::Acquire) {
        let event = lparam.0 as u32 & 0xffff;
        let menu = context.shared.menu.lock().unwrap().clone();
        let action = match event {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_USER => Some(Action::Open),
            value if value == WM_USER + 1 => Some(Action::Open),
            WM_CONTEXTMENU | WM_RBUTTONUP if menu.enabled => {
                context.icon.menu(&menu).ok().flatten()
            }
            _ => None,
        };
        if !context.shared.stopped.load(Ordering::Acquire)
            && let Some(action) = action
        {
            (context.notify)(action);
        }
        return LRESULT(0);
    }
    if message == SAVE_ERROR && !context.shared.dialog_open.swap(true, Ordering::AcqRel) {
        let error = context.shared.error.lock().unwrap().take();
        if let Some(error) = error {
            unsafe {
                let _ = EndMenu();
            }
            let text = wide(&format!(
                "{error}\n\nTry Again retries saving. Continue quits and discards unsaved changes. Cancel keeps the application running."
            ));
            let choice = unsafe {
                MessageBoxW(
                    Some(hwnd),
                    PCWSTR(text.as_ptr()),
                    w!("crosshair-rs"),
                    MB_CANCELTRYCONTINUE | MB_ICONERROR,
                )
            };
            if !context.shared.stopped.load(Ordering::Acquire) {
                if choice == IDTRYAGAIN {
                    (context.notify)(Action::RetryQuit);
                }
                if choice == IDCONTINUE {
                    (context.notify)(Action::DiscardQuit);
                }
            }
        }
        context.shared.dialog_open.store(false, Ordering::Release);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

struct Tray {
    data: NOTIFYICONDATAW,
    icon: HICON,
    theme: Option<menu::Theme>,
}

impl Tray {
    pub fn new(hwnd: HWND, message: u32) -> Result<Self> {
        let icon = unsafe {
            CreateIconFromResourceEx(
                include_bytes!("../../../packaging/windows/Assets/Square44x44Logo.png"),
                true,
                0x30000,
                32,
                32,
                IMAGE_FLAGS(0),
            )?
        };
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: message,
            hIcon: icon,
            ..Default::default()
        };
        let tip = wide("crosshair-rs");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let tray = Self {
            data,
            icon,
            theme: menu::Theme::new(),
        };
        tray.restore()?;
        Ok(tray)
    }

    pub fn restore(&self) -> Result<()> {
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &self.data).ok()?;
            let mut version = self.data;
            version.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &version).ok()?;
        }
        Ok(())
    }

    fn menu(&self, state: &MenuState) -> Result<Option<Action>> {
        unsafe {
            let mut point = POINT::default();
            GetCursorPos(&mut point)?;
            // Place the hidden owner on the tray's monitor so Windows uses its DPI.
            SetWindowPos(
                self.data.hWnd,
                None,
                point.x,
                point.y,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE,
            )?;
            if let Some(theme) = &self.theme {
                theme.prepare(self.data.hWnd);
            }
            let menu = menu::Popup::new(state)?;
            let _ = SetForegroundWindow(self.data.hWnd);
            let selected = TrackPopupMenu(
                menu.0,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                self.data.hWnd,
                None,
            )
            .0;
            let _ = PostMessageW(Some(self.data.hWnd), WM_NULL, WPARAM(0), LPARAM(0));
            Ok(match selected {
                1 => Some(Action::Open),
                2 => Some(Action::Toggle),
                3 => Some(Action::Quit),
                index if index >= 100 => state
                    .presets
                    .get(index as usize - 100)
                    .map(|p| Action::Preset(p.0)),
                _ => None,
            })
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            let _ = DestroyIcon(self.icon);
        }
    }
}
