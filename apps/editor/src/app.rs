mod actions;
mod editing;
mod events;
#[cfg(test)]
mod gesture_tests;
mod keybinds;
#[cfg(test)]
mod library_tests;
mod prompts;
mod view;

use crate::{
    AppWindow, Store,
    features::designer::{Document, Field},
    platform::{
        client::{self, Client, Options},
        dialogs, frame, preview,
    },
};
use anyhow::{Context, Result};
use crosshair_core::{protocol::*, *};
use crosshair_renderer::LayerBounds;
use slint::{ComponentHandle, SharedString};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::{Rc, Weak},
    sync::Arc,
};
use windows::Win32::{
    Foundation::HWND,
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
};

thread_local! { static APPLICATION: RefCell<Weak<RefCell<App>>> = const { RefCell::new(Weak::new()) }; }
thread_local! { static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) }; }
enum Event {
    Engine(u64, client::Event),
    Preview(preview::Event),
    Appearance(crate::platform::accessibility::Appearance),
    WorkArea([f32; 2]),
    Maximized(bool),
    CaptionHover(i32),
    Ui(UiEvent),
}
enum UiEvent {
    Action(String, String),
    Property(String, String, String, i32),
    Select(String, bool),
    Canvas(i32, Point, i32, bool),
    Shortcut(String, bool, bool, bool),
    CaptureRegion([f32; 4]),
}
fn ui_event(event: UiEvent) {
    EVENTS.with(|events| events.borrow_mut().push_back(Event::Ui(event)));
    drain_events();
}
fn post(event: Event) {
    let _ = slint::invoke_from_event_loop(move || {
        EVENTS.with(|events| events.borrow_mut().push_back(event));
        drain_events();
    });
}
pub(crate) fn system_appearance_changed() {
    let _ = slint::invoke_from_event_loop(|| {
        if let Ok(appearance) = crate::platform::accessibility::current() {
            EVENTS.with(|events| events.borrow_mut().push_back(Event::Appearance(appearance)));
            drain_events();
        }
    });
}
pub(crate) fn work_area_changed(hwnd: HWND) {
    if let Ok(size) = frame::work_area(hwnd) {
        post(Event::WorkArea(size));
    }
}
pub(crate) fn window_maximized_changed(maximized: bool) {
    post(Event::Maximized(maximized));
}
pub(crate) fn caption_hover_changed(index: i32) {
    post(Event::CaptionHover(index));
}
fn drain_events() {
    APPLICATION.with(|application| {
        let Some(application) = application.borrow().upgrade() else {
            return;
        };
        let Ok(mut app) = application.try_borrow_mut() else {
            return;
        };
        while let Some(event) = EVENTS.with(|events| events.borrow_mut().pop_front()) {
            if let Err(error) = app.event(event) {
                app.error(error.to_string());
            }
        }
        if let Err(error) = app.flush_close_if_ready() {
            app.error(error.to_string());
        }
    });
}
fn dispatch(action: impl FnOnce(&mut App) -> Result<()>) {
    APPLICATION.with(|application| {
        if let Some(application) = application.borrow().upgrade()
            && let Ok(mut app) = application.try_borrow_mut()
            && let Err(error) = action(&mut app)
        {
            app.error(error.to_string());
        }
    });
    drain_events();
}

fn finish_input(ui: &AppWindow) {
    ui.invoke_finish_input();
    // Focus-loss handlers are deferred until Slint updates its property trackers.
    slint::platform::update_timers_and_animations();
}

#[derive(Clone)]
enum Capture {
    Global,
    Existing(Id),
    New(BindingAction),
}
enum Modal {
    None,
    Menu(Vec<(String, String)>),
    RenamePreset(Id),
    RenameLayer(Id),
    NewProfile,
    RenameProfile(Id),
    SavePosition,
    DeletePreset(Id, Id),
    DeleteProfile(Id, Id),
    ReplaceData(std::path::PathBuf),
    Reset,
    Close,
    OpenDocument(Id),
    Install,
    Conflict(Capture, InputChord, Option<Id>),
    DraftConflict,
    Recovery,
}
enum ImageTarget {
    New {
        document: Id,
    },
    Layer {
        document: Id,
        layer: Id,
        prefix: String,
    },
    Arm {
        document: Id,
        layer: Id,
    },
}
#[derive(Clone)]
enum Drag {
    Move {
        origin: Point,
        before: Preset,
    },
    Resize {
        origin: Point,
        center: Point,
        span: f32,
        before: Preset,
    },
    Rotate {
        angle: f32,
        center: Point,
        before: Preset,
    },
    Pan {
        origin: Point,
        pan: Point,
    },
    Draw {
        layer: Id,
        before: Preset,
        stroke: Stroke,
    },
}

pub(crate) struct App {
    ui: slint::Weak<AppWindow>,
    client: Client,
    preview: preview::Preview,
    state: AppState,
    status: EngineStatus,
    document: Document,
    fonts: Vec<String>,
    fields: Vec<Field>,
    collapsed: BTreeSet<String>,
    thumbnails: BTreeMap<Id, (u64, slint::Image)>,
    palette: Vec<Preset>,
    browse: Vec<Preset>,
    bounds: Vec<(Id, LayerBounds)>,
    inverses: BTreeMap<Id, [Point; 3]>,
    anchors: BTreeMap<Id, Point>,
    preview_generation: u64,
    inflight: Option<Preset>,
    preview_inflight: Option<Preset>,
    preview_sent: Option<Preset>,
    preview_scale: u32,
    preview_inflight_scale: u32,
    last_valid: Option<Preset>,
    preview_error: Option<String>,
    save_requested: bool,
    group_open: bool,
    group_ending: bool,
    finish_group: bool,
    cancel_group: bool,
    pending_command: Option<Command>,
    open_after_refresh: Option<Id>,
    open_after_save: Option<Id>,
    deferred_ui: VecDeque<UiEvent>,
    image_target: Option<ImageTarget>,
    image_ready: Option<(Id, u32, u32)>,
    capture: Option<Capture>,
    capture_token: Option<Id>,
    capture_region: Option<[f32; 4]>,
    failed_shortcut: Option<Id>,
    modal: Modal,
    drag: Option<Drag>,
    draw_base: Option<Layer>,
    closing: bool,
    close_flush_pending: bool,
    recovery_shown: bool,
    connection_epoch: u64,
    install_requested: bool,
    shutdown_requested: Option<bool>,
    preview_pending: bool,
    new_connection: bool,
}

pub fn run() -> Result<()> {
    let options = Options::parse()?;
    let scope = options.scope.clone();
    let Some(_instance) = frame::Instance::acquire(&options.scope)
        .context("Could not initialize the editor instance")?
    else {
        return Ok(());
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .context("Could not initialize Windows COM")?;
    }
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _com = Com;
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attributes| attributes.with_transparent(true))
        .select()
        .context("Could not initialize the software window backend")?;
    let ui = AppWindow::new().context("Could not create the editor interface")?;
    let client = Client::start(
        options,
        Arc::new(|epoch, event| post(Event::Engine(epoch, event))),
    );
    let preview = preview::Preview::start(
        client.clone(),
        Arc::new(|event| post(Event::Preview(event))),
    );
    let app = Rc::new(RefCell::new(App {
        ui: ui.as_weak(),
        client,
        preview,
        state: AppState::default(),
        status: EngineStatus::default(),
        document: Document::default(),
        fonts: Vec::new(),
        fields: Vec::new(),
        collapsed: BTreeSet::new(),
        thumbnails: BTreeMap::new(),
        palette: crate::features::presets::presets(),
        browse: crate::features::browse::presets(),
        bounds: Vec::new(),
        inverses: BTreeMap::new(),
        anchors: BTreeMap::new(),
        preview_generation: 0,
        inflight: None,
        preview_inflight: None,
        preview_sent: None,
        preview_scale: 0,
        preview_inflight_scale: 0,
        last_valid: None,
        preview_error: None,
        save_requested: false,
        group_open: false,
        group_ending: false,
        finish_group: false,
        cancel_group: false,
        pending_command: None,
        open_after_refresh: None,
        open_after_save: None,
        deferred_ui: VecDeque::new(),
        image_target: None,
        image_ready: None,
        capture: None,
        capture_token: None,
        capture_region: None,
        failed_shortcut: None,
        modal: Modal::None,
        drag: None,
        draw_base: None,
        closing: false,
        close_flush_pending: false,
        recovery_shown: false,
        connection_epoch: 0,
        install_requested: false,
        shutdown_requested: None,
        preview_pending: false,
        new_connection: false,
    }));
    APPLICATION.with(|application| *application.borrow_mut() = Rc::downgrade(&app));
    let _accessibility = crate::platform::accessibility::Watcher::new(Arc::new(|appearance| {
        post(Event::Appearance(appearance))
    }))
    .context("Could not read Windows accessibility settings")?;
    ui.global::<Store>()
        .set_version(env!("CARGO_PKG_VERSION").into());
    ui.global::<Store>()
        .on_commit_focused_input(slint::platform::update_timers_and_animations);
    ui.global::<Store>()
        .on_action(|action, value| ui_event(UiEvent::Action(action.into(), value.into())));
    ui.global::<Store>()
        .on_format_number(|value| crate::features::designer::format_number(value).into());
    ui.global::<Store>()
        .on_property_edit(|selection, key, value, phase| {
            ui_event(UiEvent::Property(
                selection.into(),
                key.into(),
                value.into(),
                phase,
            ))
        });
    ui.global::<Store>()
        .on_select_layer(|id, extend| ui_event(UiEvent::Select(id.into(), extend)));
    ui.global::<Store>()
        .on_property_color(|selection, key, color, phase| {
            let value = format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                color.red(),
                color.green(),
                color.blue(),
                color.alpha()
            );
            if key == "@brush" {
                ui_event(UiEvent::Action("brush-color".into(), value));
            } else {
                ui_event(UiEvent::Property(
                    selection.into(),
                    key.into(),
                    value,
                    phase,
                ));
            }
        });
    ui.global::<Store>()
        .on_canvas_event(|kind, x, y, button, shift| {
            ui_event(UiEvent::Canvas(kind, Point { x, y }, button, shift))
        });
    ui.global::<Store>()
        .on_capture_region(|x, y, width, height| {
            ui_event(UiEvent::CaptureRegion([x, y, width, height]))
        });
    ui.global::<Store>()
        .on_shortcut(|text, control, shift, alt| {
            let mut handled = false;
            dispatch(|app| {
                handled = app.shortcut(&text, control, shift, alt)?;
                Ok(())
            });
            handled
        });
    let close_ui = ui.as_weak();
    ui.window().on_close_requested(move || {
        let close_ui = close_ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = close_ui.upgrade() {
                finish_input(&ui);
            }
            ui_event(UiEvent::Action("request-close".into(), String::new()));
        });
        slint::CloseRequestResponse::KeepWindowShown
    });
    use slint::winit_030::{EventResult, WinitWindowAccessor};
    let mut frame_ready = false;
    // A zero-delay timer may run before Winit has created its native window.
    ui.window().on_winit_window_event(move |window, event| {
        if let slint::winit_030::winit::event::WindowEvent::DroppedFile(path) = event {
            ui_event(UiEvent::Action(
                "drop-image".into(),
                path.to_string_lossy().into_owned(),
            ));
        }
        if !frame_ready && window.has_winit_window() {
            frame_ready = true;
            if let Err(error) = frame::install(window, &scope) {
                dispatch(|app| {
                    app.error(format!("Could not initialize the window frame: {error}"));
                    Ok(())
                });
            }
        }
        EventResult::Propagate
    });
    ui.show().context("Could not show the editor window")?;
    slint::run_event_loop()?;
    app.borrow().preview.stop();
    app.borrow().client.stop();
    ui.hide()?;
    Ok(())
}

impl App {
    fn window(&self) -> Result<AppWindow> {
        self.ui.upgrade().context("The editor window has closed")
    }
    fn hwnd(&self) -> Result<HWND> {
        frame::handle(self.window()?.window())
    }
    fn error(&self, message: String) {
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Store>().set_error(message.into());
        }
    }
    fn send(&self, tag: &str, command: Command) -> Result<()> {
        self.client.command(tag, command)
    }
    fn active_profile(&self) -> Result<Profile> {
        self.state
            .active_profile()
            .cloned()
            .context("No active profile")
    }
    fn document_id(&self) -> Result<Id> {
        Ok(self
            .document
            .draft
            .as_ref()
            .context("Open a crosshair first")?
            .id)
    }
    fn profile_edit(&mut self, profile: Profile) -> Result<()> {
        self.send(
            "profile",
            Command::UpsertProfile {
                profile,
                expected_revision: self.state.revision,
            },
        )
    }
    fn settings_edit(&mut self, settings: Settings) -> Result<()> {
        self.send(
            "settings",
            Command::SetSettings {
                settings,
                expected_revision: self.state.revision,
            },
        )
    }
    fn render(&mut self) {
        self.preview_pending = self.document.draft.is_some();
        if self.image_target.is_none() {
            self.client.pin_assets(self.document.asset_ids());
        }
        let presets = self
            .state
            .presets
            .iter()
            .chain(self.palette.iter())
            .chain(self.browse.iter())
            .filter(|preset| {
                self.thumbnails
                    .get(&preset.id)
                    .is_none_or(|(revision, _)| *revision != preset.revision)
            })
            .cloned()
            .collect();
        self.preview_generation = self.preview.request(preview::Request {
            zoom: self
                .ui
                .upgrade()
                .map(|ui| ui.global::<Store>().get_zoom() * ui.window().scale_factor())
                .unwrap_or(1.0),
            document: self.document.draft.clone(),
            presets,
            assets: self.state.assets.clone(),
            draw_base: self.draw_base.clone(),
            max_output_scale: self
                .state
                .profiles
                .iter()
                .flat_map(|profile| {
                    std::iter::once(profile.scale)
                        .chain(profile.positions.iter().map(|position| position.scale))
                })
                .fold(1.0f32, f32::max),
        });
    }
    fn request_close(&mut self) -> Result<()> {
        self.cancel_capture()?;
        if self.document.dirty
            || self.inflight.is_some()
            || self.document.conflict
            || !self.deferred_ui.is_empty()
        {
            self.modal = Modal::Close;
            self.show_modal(
                1,
                "Save this crosshair?",
                "Save your edits before closing, or discard them.",
                "Save",
                "Discard",
                "",
            );
            return Ok(());
        }
        self.closing = true;
        if self.group_open {
            self.finish_group = true;
            self.finish_edit()?;
        }
        Ok(())
    }
    fn flush_close_if_ready(&mut self) -> Result<()> {
        if self.closing
            && !self.close_flush_pending
            && !self.group_open
            && self.inflight.is_none()
            && !self.document.dirty
            && self.deferred_ui.is_empty()
        {
            if !self.window()?.global::<Store>().get_connected() {
                return self.close_now();
            }
            self.send("close-flush", Command::Flush)?;
            self.close_flush_pending = true;
            self.window()?.global::<Store>().set_read_only(true);
        }
        Ok(())
    }
    fn close_now(&mut self) -> Result<()> {
        self.closing = true;
        self.close_flush_pending = true;
        self.preview.stop();
        self.client.stop();
        slint::quit_event_loop()?;
        Ok(())
    }
    fn export(&self, kind: ExportKind) -> Result<()> {
        let png = matches!(kind, ExportKind::Png { .. });
        if let Some(path) = dialogs::choose(
            self.hwnd()?,
            true,
            if png {
                dialogs::FileKind::Png
            } else {
                dialogs::FileKind::Document
            },
            if png {
                "crosshair.png"
            } else {
                "crosshair-rs.json"
            },
        )? {
            self.send(
                "export",
                Command::Export {
                    kind,
                    destination: path.to_string_lossy().into(),
                },
            )?;
        }
        Ok(())
    }
}

fn defer_ui(queue: &mut VecDeque<UiEvent>, event: UiEvent) -> Result<()> {
    let replace = matches!(
        (queue.back(), &event),
        (
            Some(UiEvent::Canvas(1, _, _, _)),
            UiEvent::Canvas(1, _, _, _)
        )
    ) || matches!((queue.back(), &event), (Some(UiEvent::Property(before_selection, before, _, phase)), UiEvent::Property(after_selection, after, _, next)) if before_selection == after_selection && before == after && phase == next && *phase < 2);
    if replace {
        queue.pop_back();
    }
    anyhow::ensure!(
        queue.len() < 256,
        "The editor is waiting for the engine. Retry after it responds."
    );
    queue.push_back(event);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_shortcut_records_in_its_own_row_and_can_be_cancelled() {
        use crate::{BindingRow, PresetRow};
        use slint::Model;
        use slint::platform::{Platform, WindowAdapter, WindowEvent, software_renderer::*};
        use std::time::Duration;
        struct Headless;
        impl Platform for Headless {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
                Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
            }
        }
        fn click(ui: &AppWindow, x: f32, y: f32) {
            let position = slint::LogicalPosition::new(x, y);
            for event in [
                WindowEvent::PointerPressed {
                    position,
                    button: slint::platform::PointerEventButton::Left,
                },
                WindowEvent::PointerReleased {
                    position,
                    button: slint::platform::PointerEventButton::Left,
                },
            ] {
                ui.window().dispatch_event(event);
            }
        }
        slint::platform::set_platform(Box::new(Headless)).unwrap();
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
        let store = ui.global::<Store>();
        store.set_connected(true);
        store.set_page(2);
        store.set_bindings(
            Rc::new(slint::VecModel::from(vec![
                BindingRow {
                    id: "global".into(),
                    global: true,
                    action: "Toggle visibility".into(),
                    ..Default::default()
                },
                BindingRow {
                    id: "saved".into(),
                    shortcut: true,
                    name: "Saved".into(),
                    chord: "J".into(),
                    ..Default::default()
                },
            ]))
            .into(),
        );
        store.set_preset_names(
            Rc::new(slint::VecModel::from(vec!["Classic".into(), "Dot".into()])).into(),
        );
        store.set_selector_presets(
            Rc::new(slint::VecModel::from(vec![
                PresetRow {
                    id: "classic".into(),
                    name: "Classic".into(),
                    ..Default::default()
                },
                PresetRow {
                    id: "dot".into(),
                    name: "Dot".into(),
                    ..Default::default()
                },
            ]))
            .into(),
        );
        let events = Rc::new(RefCell::new(Vec::new()));
        let recorded = events.clone();
        let weak = ui.as_weak();
        store.on_action(move |action, value| {
            recorded
                .borrow_mut()
                .push((action.to_string(), value.to_string()));
            let ui = weak.upgrade().unwrap();
            let store = ui.global::<Store>();
            match action.as_str() {
                "preset-keybind" => {
                    assert_eq!(value, "dot");
                    store.set_target_index(1);
                    store.set_pending_shortcut("new-shortcut".into());
                    store.set_request_shortcut(true);
                }
                "new-binding" => store.set_capture_id("new-shortcut".into()),
                "discard-shortcut" => {
                    store.set_capture_id("".into());
                    store.set_pending_shortcut("".into());
                }
                _ => {}
            }
        });
        let region = Rc::new(RefCell::new(None));
        let captured_region = region.clone();
        store.on_capture_region(move |x, y, width, height| {
            *captured_region.borrow_mut() = Some([x, y, width, height])
        });
        ui.show().unwrap();
        click(&ui, 200.0, 392.0);
        assert!(
            events
                .borrow()
                .iter()
                .any(|(action, _)| action == "selector-filter")
        );
        click(&ui, 710.0, 300.0);
        assert_eq!(store.get_target_index(), 1);
        slint::platform::update_timers_and_animations();
        std::thread::sleep(Duration::from_millis(20));
        slint::platform::update_timers_and_animations();
        assert_eq!(store.get_capture_id(), "new-shortcut");
        let [x, y, width, height] = region.borrow().unwrap();
        assert!(y > 420.0, "recording must be below the New row: {y}");
        events.borrow_mut().clear();
        for text in [
            slint::SharedString::from(slint::platform::Key::Return),
            " ".into(),
        ] {
            ui.window()
                .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
        assert!(
            events.borrow().is_empty(),
            "recording must not reopen the New picker"
        );
        click(&ui, x + width / 2.0, y + height / 2.0);
        // The engine captures Mouse 1 on press; release must not replace its token.
        assert!(events.borrow().is_empty());
        store.set_capture_id("".into());
        click(&ui, x + width / 2.0, y + height / 2.0);
        assert_eq!(*events.borrow(), [("new-binding".into(), "5".into())]);
        // Capture cancellation can arrive before the UI press or between press/release.
        for before_press in [true, false] {
            store.set_pending_shortcut("new-shortcut".into());
            store.set_capture_id("new-shortcut".into());
            events.borrow_mut().clear();
            let position = slint::LogicalPosition::new(x + width + 38.0, y + height / 2.0);
            if before_press {
                store.set_capture_id("".into());
            }
            ui.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
            store.set_capture_id("".into());
            ui.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
            assert_eq!(*events.borrow(), [("discard-shortcut".into(), "".into())]);
            assert_eq!(store.get_pending_shortcut(), "");
            assert_eq!(store.get_bindings().row_count(), 2);
        }
        store.set_pending_shortcut("saving-binding-id".into());
        events.borrow_mut().clear();
        click(&ui, x + width / 2.0, y + height / 2.0);
        click(&ui, x + width + 38.0, y + height / 2.0);
        assert!(
            events.borrow().is_empty(),
            "saving must not resubmit or discard the row"
        );
        ui.hide().unwrap();
    }

    #[test]
    fn profile_menu_selects_profiles_and_opens_settings() {
        use slint::platform::{Key, Platform, WindowAdapter, WindowEvent, software_renderer::*};
        struct Headless;
        impl Platform for Headless {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
                Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
            }
        }
        fn press(ui: &AppWindow, text: impl Into<SharedString>) {
            let text = text.into();
            ui.window()
                .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
        fn click(ui: &AppWindow, x: f32, y: f32) {
            let position = slint::LogicalPosition::new(x, y);
            ui.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
            ui.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
        }
        slint::platform::set_platform(Box::new(Headless)).unwrap();
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
        let store = ui.global::<Store>();
        store.set_connected(true);
        store.on_commit_focused_input(slint::platform::update_timers_and_animations);
        store.set_profiles(
            Rc::new(slint::VecModel::from(vec![
                "Default".into(),
                "Second".into(),
            ]))
            .into(),
        );
        let events = Rc::new(RefCell::new(Vec::new()));
        let recorded = events.clone();
        store.on_action(move |action, value| {
            recorded
                .borrow_mut()
                .push((action.to_string(), value.to_string()))
        });
        ui.show().unwrap();
        click(&ui, 900.0, 24.0);
        press(&ui, Key::DownArrow);
        press(&ui, Key::Return);
        assert_eq!(
            events.borrow().last(),
            Some(&("select-profile".into(), "1".into()))
        );
        events.borrow_mut().clear();
        click(&ui, 900.0, 24.0);
        press(&ui, Key::End);
        press(&ui, Key::Escape);
        assert!(events.borrow().is_empty());
        assert_eq!(store.get_settings_category(), 0);
        click(&ui, 900.0, 24.0);
        press(&ui, Key::End);
        press(&ui, Key::Return);
        assert_eq!(*events.borrow(), [("navigate".into(), "3".into())]);
        assert_eq!(store.get_settings_category(), 2);
        events.borrow_mut().clear();
        store.set_settings_category(0);
        click(&ui, 900.0, 24.0);
        click(&ui, 950.0, 168.0);
        assert_eq!(*events.borrow(), [("navigate".into(), "3".into())]);
        assert_eq!(store.get_settings_category(), 2);
        ui.hide().unwrap();
    }

    #[test]
    fn focused_name_commits_before_save_and_close() {
        use slint::platform::{Key, Platform, WindowAdapter, WindowEvent, software_renderer::*};
        struct Headless;
        impl Platform for Headless {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
                Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
            }
        }
        fn press(ui: &AppWindow, text: impl Into<SharedString>) {
            let text = text.into();
            ui.window()
                .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
        fn click(ui: &AppWindow, x: f32, y: f32) {
            let position = slint::LogicalPosition::new(x, y);
            ui.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
            ui.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
        }
        fn replace_name(ui: &AppWindow, name: &str) {
            click(ui, 160.0, 70.0);
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Control.into(),
            });
            press(ui, "a");
            ui.window().dispatch_event(WindowEvent::KeyReleased {
                text: Key::Control.into(),
            });
            press(ui, name);
        }
        slint::platform::set_platform(Box::new(Headless)).unwrap();
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
        let store = ui.global::<Store>();
        store.set_connected(true);
        store.set_page(1);
        store.set_document_name("Classic".into());
        store.on_commit_focused_input(slint::platform::update_timers_and_animations);
        let events = Rc::new(RefCell::new(Vec::new()));
        let committed = events.clone();
        store.on_action(move |action, value| {
            committed
                .borrow_mut()
                .push((action.to_string(), value.to_string()))
        });
        let saved = events.clone();
        store.on_shortcut(move |text, control, _, _| {
            if control && text == "s" {
                saved.borrow_mut().push(("save".into(), String::new()));
                true
            } else {
                false
            }
        });
        ui.show().unwrap();
        slint::platform::update_timers_and_animations();
        replace_name(&ui, "Typed name");
        assert!(events.borrow().is_empty());
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Control.into(),
        });
        press(&ui, "s");
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Control.into(),
        });
        assert_eq!(
            *events.borrow(),
            [
                ("rename-document".into(), "Typed name".into()),
                ("save".into(), String::new())
            ]
        );
        events.borrow_mut().clear();
        replace_name(&ui, "Clicked save");
        assert!(events.borrow().is_empty());
        click(&ui, 1000.0, 70.0);
        assert_eq!(
            *events.borrow(),
            [
                ("rename-document".into(), "Clicked save".into()),
                ("save".into(), String::new())
            ]
        );
        events.borrow_mut().clear();
        replace_name(&ui, "Pending close");
        assert!(events.borrow().is_empty());
        finish_input(&ui);
        assert_eq!(
            *events.borrow(),
            [("rename-document".into(), "Pending close".into())]
        );
        ui.hide().unwrap();
    }

    #[test]
    fn queued_pointer_moves_keep_gesture_boundaries_and_the_latest_position() {
        let mut queue = VecDeque::new();
        defer_ui(&mut queue, UiEvent::Canvas(0, Point::default(), 1, false)).unwrap();
        for x in 1..1000 {
            defer_ui(
                &mut queue,
                UiEvent::Canvas(
                    1,
                    Point {
                        x: x as f32,
                        y: 2.0,
                    },
                    1,
                    false,
                ),
            )
            .unwrap();
        }
        defer_ui(
            &mut queue,
            UiEvent::Canvas(2, Point { x: 999.0, y: 2.0 }, 1, false),
        )
        .unwrap();
        assert_eq!(queue.len(), 3);
        assert!(matches!(queue[0], UiEvent::Canvas(0, _, _, _)));
        assert!(
            matches!(&queue[1], UiEvent::Canvas(1, point, _, _) if point.x == 999.0 && point.y == 2.0)
        );
        assert!(matches!(queue[2], UiEvent::Canvas(2, _, _, _)));
    }

    #[test]
    fn queued_text_commits_and_selection_changes_are_not_coalesced_away() {
        let mut queue = VecDeque::new();
        for value in ["1", "12", "123"] {
            defer_ui(
                &mut queue,
                UiEvent::Property("layer-a".into(), "x".into(), value.into(), 0),
            )
            .unwrap();
        }
        defer_ui(
            &mut queue,
            UiEvent::Property("layer-a".into(), "x".into(), "123".into(), 2),
        )
        .unwrap();
        defer_ui(&mut queue, UiEvent::Select("layer-b".into(), false)).unwrap();
        defer_ui(
            &mut queue,
            UiEvent::Property("layer-b".into(), "x".into(), "4".into(), 0),
        )
        .unwrap();
        assert_eq!(queue.len(), 4);
        assert!(
            matches!(&queue[0], UiEvent::Property(selection, _, value, 0) if selection == "layer-a" && value == "123")
        );
        assert!(matches!(queue[1], UiEvent::Property(_, _, _, 2)));
        assert!(matches!(queue[2], UiEvent::Select(_, false)));
    }

    #[test]
    fn queued_shortcuts_preserve_save_and_undo_boundaries() {
        let mut queue = VecDeque::new();
        for key in ["s", "d", "d", "z", "s"] {
            defer_ui(
                &mut queue,
                UiEvent::Shortcut(key.into(), true, false, false),
            )
            .unwrap();
        }
        let keys: Vec<_> = queue
            .iter()
            .map(|event| match event {
                UiEvent::Shortcut(key, true, false, false) => key.as_str(),
                _ => panic!("shortcut changed while queued"),
            })
            .collect();
        assert_eq!(keys, ["s", "d", "d", "z", "s"]);
    }
}
