mod commands;
mod completion;
mod output;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender},
    },
    time::Instant,
};

use anyhow::{Result, ensure};
use crosshair_core::{protocol::*, *};
use crosshair_platform::identity::{Identity, control_pipe_name};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
            LibraryLoader::GetModuleHandleW,
            RemoteDesktop::{
                NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
                WTSUnRegisterSessionNotification,
            },
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};

use crate::{
    app::{App, Connection},
    input, ipc,
    lifecycle::{self, Options},
    overlay::{
        DesktopOverlay, enumerate_monitors, render,
        widget::{self, Widget},
    },
    storage::{Store, default_data_directory},
    tray,
    updates::{self, Updates},
    work::{self, Job, Work, WorkReply},
};
use commands::{After, Pending, SaveNext};

const WORK: u32 = WM_APP + 1;
const DISPLAYS: u32 = WM_APP + 2;
const SESSION: u32 = WM_APP + 3;
const POWER: u32 = WM_APP + 4;
const END_SESSION: u32 = WM_APP + 5;
const SAVE_TIMER: usize = 1;
const CAPTURE_TIMER: usize = 2;
const CLOSE_TIMER: usize = 3;
const OUTPUT_TIMER: usize = 4;

enum Event {
    Ipc(ipc::Event),
    Widget(widget::Status),
    Update(UpdateStatus),
    Work(work::Completed),
    Render(render::Completed),
    Installed(Result<()>),
    Tray(tray::Action),
}

#[derive(Clone)]
struct Wake {
    sender: SyncSender<Event>,
    hwnd: usize,
}
impl Wake {
    fn send(&self, event: Event) -> bool {
        if self.sender.try_send(event).is_err() {
            return false;
        }
        self.post()
    }
    fn complete(&self, event: Event) {
        if self.sender.send(event).is_ok() {
            self.post();
        }
    }
    fn post(&self) -> bool {
        unsafe { PostMessageW(Some(HWND(self.hwnd as *mut _)), WORK, WPARAM(0), LPARAM(0)).is_ok() }
    }
}

struct Window(HWND);
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = WTSUnRegisterSessionNotification(self.0);
            let _ = DestroyWindow(self.0);
        }
    }
}
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

struct Runtime {
    app: App,
    root: PathBuf,
    window: Window,
    overlay: DesktopOverlay,
    input: Option<input::Registration>,
    tray: tray::Worker,
    server: ipc::Server,
    events: Receiver<Event>,
    wake: Wake,
    options: Options,
    stopped: bool,
    frozen: bool,
    shutdown_pending: bool,
    suspended: bool,
    last_image: Option<(Arc<crosshair_renderer::Raster>, [i32; 2])>,
    capture_timer: bool,
    widget: Option<Widget>,
    widget_center: bool,
    updates: Updates,
    work: Work,
    pending: BTreeMap<u64, Pending>,
    next_ticket: u64,
    render: render::Worker,
    rendering: Option<render::Key>,
    failed_render: BTreeSet<render::Key>,
    cache: render::Cache,
    warm: Vec<render::Key>,
    warmed_profile: Option<(Id, Vec<render::Key>)>,
    close_request: Option<SaveNext>,
    change: Option<output::Change>,
    widget_status: widget::Status,
    widget_clear_required: Option<u64>,
}

pub fn run(options: Options) -> Result<()> {
    let identity = Identity::current()?;
    if options.existing(&identity)? {
        return Ok(());
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;
    }
    let _com = Com;
    let root = options
        .test_data
        .clone()
        .map_or_else(default_data_directory, Ok)?;
    let recovery = updates::recover(&root)?;
    let store = Store::open(&root)?;
    let root = store.root().to_path_buf();
    let mut app = App::open(&store, options.restore_visibility || recovery.is_some())?;
    if let Some(recovery) = &recovery {
        app.state.base_visible = recovery.restart.visible;
    }
    if !app.status.recovery_required {
        match store.save(&app.state) {
            Ok(()) => {
                app.saved(app.state.revision);
                if let Some(recovery) = &recovery {
                    updates::acknowledge_recovery(&root, recovery)?;
                }
            }
            Err(error) => app.save_failed(format!("{error:#}")),
        }
    }
    let window = unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("CrosshairRs.Engine"),
            ..Default::default()
        };
        ensure!(
            RegisterClassW(&class) != 0,
            "Could not register the engine window"
        );
        Window(CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.lpszClassName,
            w!("crosshair-rs engine"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )?)
    };
    let overlay = DesktopOverlay::new()?;
    let input = if options.no_input {
        None
    } else {
        Some(input::Registration::new(window.0)?)
    };
    let (sender, events) = mpsc::sync_channel(64);
    let wake = Wake {
        sender,
        hwnd: window.0.0 as usize,
    };
    let tray_wake = wake.clone();
    let tray = tray::Worker::start(move |action| tray_wake.complete(Event::Tray(action)))?;
    let update_wake = wake.clone();
    let updates = Updates::new(root.clone(), move |status| {
        update_wake.complete(Event::Update(status));
    })?;
    if let Some(recovery) = &recovery {
        updates.report_recovery(recovery);
    }
    let work_wake = wake.clone();
    let work = Work::start(store, move |result| work_wake.complete(Event::Work(result)))?;
    let render_wake = wake.clone();
    let render = render::Worker::start(root.clone(), move |result| {
        render_wake.complete(Event::Render(result))
    })?;
    let widget = if identity.package.is_some() {
        let widget_wake = wake.clone();
        match Widget::start(identity.clone(), move |status| {
            widget_wake.complete(Event::Widget(status));
        }) {
            Ok(widget) => {
                app.status.widget.installed = true;
                Some(widget)
            }
            Err(error) => {
                app.status.render_error = Some(format!("Game Bar: {error:#}"));
                None
            }
        }
    } else {
        None
    };
    let control_wake = wake.clone();
    let server = ipc::Server::start(control_pipe_name(&options.scope)?, identity, move |event| {
        if matches!(event, ipc::Event::Disconnected(_)) {
            control_wake.complete(Event::Ipc(event));
            true
        } else {
            control_wake.send(Event::Ipc(event))
        }
    })?;
    unsafe {
        WTSRegisterSessionNotification(window.0, NOTIFY_FOR_THIS_SESSION)?;
    }
    let mut runtime = Runtime {
        app,
        root,
        window,
        overlay,
        input,
        tray,
        server,
        events,
        wake,
        options,
        stopped: false,
        frozen: false,
        shutdown_pending: false,
        suspended: false,
        last_image: None,
        capture_timer: false,
        widget,
        widget_center: true,
        updates,
        work,
        pending: BTreeMap::new(),
        next_ticket: 1,
        render,
        rendering: None,
        failed_render: BTreeSet::new(),
        cache: render::Cache::default(),
        warm: Vec::new(),
        warmed_profile: None,
        close_request: None,
        change: None,
        widget_status: widget::Status::default(),
        widget_clear_required: None,
    };
    if recovery.as_ref().is_some_and(|r| r.restart.reopen_editor) {
        runtime.open_editor()?;
    }
    runtime.displays();
    runtime.refresh();
    let mut message = MSG::default();
    while !runtime.stopped {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        ensure!(result != -1, "Windows message dispatch failed");
        if result == 0 {
            break;
        }
        if message.hwnd == runtime.window.0 {
            match message.message {
                WORK => {
                    for _ in 0..32 {
                        if let Ok(event) = runtime.events.try_recv() {
                            runtime.event(event);
                        } else {
                            break;
                        }
                        if runtime.stopped {
                            break;
                        }
                    }
                }
                WM_INPUT => {
                    let mut changed = false;
                    let _ = input::read(message.lParam, |event| {
                        if runtime.frozen {
                            let mouse_held = runtime.app.input.mouse_buttons_held();
                            runtime.app.input.observe_suspended(event);
                            changed |= mouse_held != runtime.app.input.mouse_buttons_held();
                        } else {
                            changed |= runtime.app.input(event);
                        }
                    });
                    if changed {
                        runtime.refresh();
                    }
                    unsafe {
                        DefWindowProcW(
                            message.hwnd,
                            message.message,
                            message.wParam,
                            message.lParam,
                        );
                    }
                }
                WM_INPUT_DEVICE_CHANGE => {
                    if message.wParam.0 == 2 {
                        runtime.app.input.remove_device(message.lParam.0 as u64);
                        runtime.refresh();
                    }
                }
                WM_TIMER => runtime.timer(message.wParam.0),
                DISPLAYS => {
                    runtime.displays();
                    runtime.refresh();
                }
                SESSION | POWER => {
                    runtime.app.reset_input();
                    runtime.suspended = if message.message == SESSION {
                        matches!(message.wParam.0, 4 | 7)
                    } else {
                        message.wParam.0 == PBT_APMSUSPEND as usize
                    };
                    runtime.displays();
                    runtime.refresh();
                }
                END_SESSION => {
                    let _ = runtime.save(None, SaveNext::Quit { tray: false });
                }
                WM_CLOSE => runtime.quit_from_tray(),
                _ => unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                },
            }
        } else {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    runtime.overlay.hide();
    if let Some(widget) = &runtime.widget {
        widget.close();
    }
    runtime.updates.cancel();
    runtime.render.cancel();
    runtime.app.broadcast(ServerMessage::Closing);
    for connection in runtime.app.clients.values() {
        connection.client.flush();
    }
    runtime.app.clients.clear();
    // Unblock background completion senders before joining the native transports.
    let (_, empty) = mpsc::sync_channel(1);
    runtime.events = empty;
    runtime.server.stop();
    runtime.work.join()?;
    Ok(())
}

impl Runtime {
    fn event(&mut self, event: Event) {
        match event {
            Event::Tray(action) => self.tray_action(action),
            Event::Work(result) => self.completed(result),
            Event::Render(result) => {
                if self.rendering == Some(result.key) {
                    self.rendering = None;
                }
                let active = self.change_needs(result.key)
                    || self
                        .app
                        .presentation()
                        .is_some_and(|(key, _)| key == result.key);
                match result.result {
                    Ok(raster) => {
                        self.failed_render.remove(&result.key);
                        if active {
                            self.cache.insert(result.key, raster);
                        } else if !self.cache.warm(result.key, raster) {
                            self.warm.clear();
                        }
                    }
                    Err(error) => {
                        self.failed_render.insert(result.key);
                        if active {
                            self.app.status.render_error = Some(error.to_string());
                            self.hide();
                        }
                    }
                }
                self.refresh();
                self.app.changed(None, false);
            }
            Event::Installed(result) => match result {
                Ok(()) => self.begin_shutdown(),
                Err(error) => {
                    self.frozen = false;
                    self.updates.installation_failed(format!("{error:#}"));
                    self.app.broadcast(ServerMessage::CloseCancelled);
                }
            },
            Event::Widget(status) => {
                self.app.status.widget = status.widget.clone();
                self.widget_status = status;
                self.acknowledge_widget_blank();
                self.refresh();
                self.app.changed(None, false);
            }
            Event::Update(status) => {
                self.app.status.update = status.clone();
                self.app.broadcast(ServerMessage::Update { status });
            }
            Event::Ipc(ipc::Event::Connected(client)) => {
                self.app.clients.insert(
                    client.id,
                    Connection {
                        client,
                        subscribed: false,
                        snapshot: None,
                    },
                );
            }
            Event::Ipc(ipc::Event::Disconnected(id)) => {
                self.app.disconnect(id);
                self.change_owner_disconnected(id);
                let _ = self.queue(
                    Job::Disconnect { owner: id },
                    Pending::internal(After::Ignore, self.app.state.revision),
                );
                if !self.app.clients.values().any(|c| c.subscribed) {
                    let _ = self.queue(
                        Job::ClearCaches,
                        Pending::internal(After::Ignore, self.app.state.revision),
                    );
                }
                if !self.app.clients.values().any(|c| c.subscribed)
                    && let Some(next) = self.close_request.take()
                    && let Err(error) = self.save(None, next)
                {
                    self.app.save_failed(error.to_string());
                }
                self.refresh();
            }
            Event::Ipc(ipc::Event::Request(owner, request)) => self.request(owner, request),
        }
    }

    fn displays(&mut self) {
        self.widget_center = true;
        self.failed_render.clear();
        self.invalidate_output_geometry();
        match enumerate_monitors() {
            Ok(monitors) => {
                self.app.status.monitors = monitors
                    .into_iter()
                    .map(|m| DisplayInfo {
                        id: m.id,
                        name: m.name,
                        bounds: m.bounds,
                        primary: m.primary,
                    })
                    .collect()
            }
            Err(error) => {
                self.app.status.monitors.clear();
                self.app.status.render_error = Some(error.to_string());
            }
        }
    }

    fn hide(&mut self) {
        self.overlay.hide();
        self.last_image = None;
        if let Some(widget) = &self.widget {
            widget.present(None, [0, 0], false);
        }
    }

    fn refresh(&mut self) {
        self.tray
            .update(&self.app.state, self.app.status().visible, !self.frozen);
        if let Some(input) = &mut self.input
            && let Err(error) = input.update(
                &self.app.state,
                self.app.input.capturing(),
                self.app.input.mouse_buttons_held(),
            )
        {
            self.app.status.render_error = Some(error.to_string());
        }
        if self.suspended {
            self.hide();
        } else if self.widget_clear_required.is_some()
            && self.app.state.settings.output_mode == OutputMode::Desktop
        {
            self.overlay.hide();
            self.last_image = None;
            self.app.status.render_error = Some(
                "Reconnect the Game Bar widget so it can confirm its previous crosshair is hidden"
                    .into(),
            );
        } else if !self.previous_output_hidden() {
            if let Some((key, center)) = self.app.presentation() {
                if let Some(raster) = self.cache.get(key) {
                    let origin = [center[0] + raster.origin[0], center[1] + raster.origin[1]];
                    if self.app.state.settings.output_mode == OutputMode::GameBar {
                        self.overlay.hide();
                        self.last_image = None;
                        if let Some(widget) = &self.widget {
                            widget.present(Some(raster), origin, self.widget_center);
                            self.widget_center = false;
                            self.app.status.render_error = None;
                        } else {
                            self.app.status.render_error =
                                Some("Install the Windows package to use Game Bar output".into());
                        }
                    } else {
                        if !self.changing_output()
                            && let Some(widget) = &self.widget
                        {
                            widget.present(None, [0, 0], false);
                        }
                        if self.last_image.as_ref().is_none_or(|(previous, position)| {
                            !Arc::ptr_eq(previous, &raster) || *position != origin
                        }) {
                            match self.overlay.present_at(
                                [raster.width, raster.height],
                                &raster.pixels,
                                origin,
                            ) {
                                Ok(()) => {
                                    self.app.status.render_error = None;
                                    self.last_image = Some((raster, origin));
                                }
                                Err(error) => {
                                    self.hide();
                                    self.app.status.render_error = Some(error.to_string());
                                }
                            }
                        }
                    }
                } else if self.rendering != Some(key) && !self.failed_render.contains(&key) {
                    self.render_key(key);
                }
            } else {
                self.overlay.hide();
                self.last_image = None;
                if !self.changing_output()
                    && let Some(widget) = &self.widget
                {
                    widget.present(None, [0, 0], false);
                }
            }
        }
        self.advance_change();
        if self.rendering.is_none() && self.change.is_none() {
            self.prewarm();
        }
        unsafe {
            let _ = KillTimer(Some(self.window.0), SAVE_TIMER);
            if self.change.is_none()
                && let Some(when) = self.app.save_at
            {
                SetTimer(
                    Some(self.window.0),
                    SAVE_TIMER,
                    when.saturating_duration_since(Instant::now())
                        .as_millis()
                        .clamp(1, 2000) as u32,
                    None,
                );
            }
            if self.app.input.capturing() && !self.capture_timer {
                SetTimer(Some(self.window.0), CAPTURE_TIMER, 30_000, None);
                self.capture_timer = true;
            } else if !self.app.input.capturing() {
                let _ = KillTimer(Some(self.window.0), CAPTURE_TIMER);
                self.capture_timer = false;
            }
        }
    }

    fn render_key(&mut self, key: render::Key) {
        if let Some(preset) = self
            .app
            .state
            .preset(key.preset)
            .filter(|p| p.revision == key.revision)
        {
            self.render.submit(render::Job {
                key,
                preset: preset.clone(),
                assets: self.app.state.assets.clone(),
            });
            self.rendering = Some(key);
        }
    }

    fn prewarm(&mut self) {
        let Some(profile) = self.app.state.active_profile() else {
            return;
        };
        let mut targets: Vec<_> = profile
            .bindings
            .iter()
            .filter(|b| b.enabled)
            .filter_map(|b| b.action.target())
            .chain(Some(profile.selected_preset))
            .collect();
        targets.sort_unstable();
        targets.dedup();
        let keys: Vec<_> = targets
            .into_iter()
            .filter_map(|id| {
                self.app
                    .state
                    .preset(id)
                    .map(|p| render::Key::new(p, profile.scale))
            })
            .collect();
        let signature = (profile.id, keys);
        if self.warmed_profile.as_ref() != Some(&signature) {
            self.cache.retain(&signature.1);
            self.failed_render.retain(|key| signature.1.contains(key));
            self.warm = signature.1.clone();
            self.warmed_profile = Some(signature);
        }
        while let Some(key) = self.warm.pop() {
            if self.cache.get(key).is_none() && !self.failed_render.contains(&key) {
                self.render_key(key);
                break;
            }
        }
    }

    fn timer(&mut self, timer: usize) {
        unsafe {
            let _ = KillTimer(Some(self.window.0), timer);
        }
        if timer == SAVE_TIMER {
            self.app.save_at = None;
            if let Err(error) = self.save(None, SaveNext::None) {
                self.app.save_failed(format!("{error:#}"));
                self.app.changed(None, false);
            }
        }
        if timer == CAPTURE_TIMER {
            self.capture_timer = false;
            if let Some(outcome) = self.app.input.expire_capture(self.app.elapsed_ms()) {
                self.app.captured(outcome);
            }
            self.refresh();
        }
        if timer == CLOSE_TIMER {
            self.close_request = None;
            self.app.broadcast(ServerMessage::CloseCancelled);
        }
        if timer == OUTPUT_TIMER {
            self.output_timeout();
        }
    }

    fn open_editor(&self) -> Result<()> {
        if self.app.clients.values().any(|c| c.subscribed) {
            self.app.broadcast(ServerMessage::FocusEditor);
            Ok(())
        } else {
            self.options.open_editor()
        }
    }

    fn tray_action(&mut self, action: tray::Action) {
        if self.frozen {
            return;
        }
        match action {
            tray::Action::Open => {
                let _ = self.open_editor();
            }
            tray::Action::Toggle if !self.app.status.recovery_required => {
                self.app.input.toggle_visibility(&mut self.app.state);
                self.app.dirty();
                self.app.changed(None, false);
            }
            tray::Action::Preset(preset) if !self.app.status.recovery_required => {
                if self
                    .app
                    .command(Id::nil(), Command::SelectPreset { preset })
                    .is_ok()
                {
                    self.app.dirty();
                    self.app.changed(None, false);
                }
            }
            tray::Action::Quit | tray::Action::RetryQuit => self.quit_from_tray(),
            tray::Action::DiscardQuit => self.begin_shutdown(),
            _ => {}
        }
        self.refresh();
    }

    fn quit_from_tray(&mut self) {
        if let Err(error) = self.request_close(SaveNext::Quit { tray: true }) {
            self.quit_failure(&error.to_string());
        }
    }

    fn quit_failure(&mut self, error: &str) {
        self.frozen = false;
        self.tray.save_error(error);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let forwarded = match message {
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => Some(DISPLAYS),
        WM_WTSSESSION_CHANGE => Some(SESSION),
        WM_POWERBROADCAST => Some(POWER),
        WM_ENDSESSION if wparam.0 != 0 => Some(END_SESSION),
        _ => None,
    };
    if let Some(forwarded) = forwarded {
        unsafe {
            let _ = PostMessageW(Some(hwnd), forwarded, wparam, lparam);
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
