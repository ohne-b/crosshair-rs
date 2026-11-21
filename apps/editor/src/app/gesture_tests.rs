use super::*;
use crate::{FieldGroup, FieldRow};
use slint::{
    Model,
    platform::{Key, Platform, WindowAdapter, WindowEvent, software_renderer::*},
};

fn window() -> AppWindow {
    struct Headless;
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
        }
    }
    slint::platform::set_platform(Box::new(Headless)).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_document_open(true);
    store.set_selection_editable(true);
    store.on_format_number(|value| crate::features::designer::format_number(value).into());
    ui.show().unwrap();
    ui
}

pub(super) fn app(ui: &AppWindow) -> App {
    App {
        ui: ui.as_weak(),
        client: Client::disconnected_for_test(),
        preview: preview::Preview::idle_for_test(),
        state: AppState::default(),
        status: EngineStatus::default(),
        document: Document::default(),
        fonts: Vec::new(),
        fields: Vec::new(),
        animate: false,
        animation_trigger: AnimationTrigger::Fire,
        playback: Playback::default(),
        playback_started: std::time::Instant::now(),
        playback_timer: slint::Timer::default(),
        playback_generation: 0,
        playback_next: None,
        playing: false,
        collapsed: BTreeSet::new(),
        thumbnails: BTreeMap::new(),
        palette: Vec::new(),
        browse: Vec::new(),
        browse_save_feedback_timer: slint::Timer::default(),
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
        capture_profile: None,
        pending_aim: None,
        capture_region: None,
        failed_shortcut: None,
        modal: Modal::None,
        drag: None,
        draw_base: None,
        closing: false,
        close_flush_pending: false,
        recovery_shown: false,
        connection_epoch: 0,
        output_ack_revision: None,
        install_requested: false,
        shutdown_requested: None,
        preview_pending: false,
        new_connection: false,
    }
}

#[test]
fn startup_toggle_uses_windows_state_and_always_requests_reconciliation() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    app.state.settings.launch_at_login = true;
    app.status.startup_enabled = Some(false);
    app.refresh_status();
    assert!(!ui.global::<Store>().get_startup());
    assert!(ui.global::<Store>().get_startup_known());
    app.action("startup", "true")?;
    assert!(matches!(
        &commands()[..],
        [Command::SetStartup { enabled: true }]
    ));
    app.status.startup_enabled = Some(true);
    app.refresh_status();
    assert!(ui.global::<Store>().get_startup());
    app.status.startup_enabled = None;
    app.refresh_status();
    assert!(!ui.global::<Store>().get_startup());
    assert!(!ui.global::<Store>().get_startup_known());
    ui.hide()?;
    Ok(())
}

fn pointer(ui: &AppWindow, kind: i32, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    let button = slint::platform::PointerEventButton::Left;
    ui.window().dispatch_event(match kind {
        0 => WindowEvent::PointerPressed { position, button },
        1 => WindowEvent::PointerMoved { position },
        2 => WindowEvent::PointerReleased { position, button },
        _ => unreachable!(),
    });
}

#[test]
fn pointer_clicks_clear_keyboard_focus_styling() {
    let ui = window();
    let store = ui.global::<Store>();
    store.set_connected(true);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    assert!(store.get_keyboard_navigation());
    // The Designer navigation button keeps focus, but a pointer click must not
    // retain the keyboard highlight when the pointer leaves.
    pointer(&ui, 0, 28.0, 124.0);
    pointer(&ui, 2, 28.0, 124.0);
    pointer(&ui, 1, 600.0, 400.0);
    assert!(!store.get_keyboard_navigation());
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    assert!(store.get_keyboard_navigation());
    ui.hide().unwrap();
}

fn complete_preview(app: &mut App, adjusted: Option<Preset>) -> Result<()> {
    app.event(Event::Preview(preview::Event::Document {
        scale: 1.0,
        generation: app.preview_generation,
        id: app.document.draft.as_ref().unwrap().id,
        thumbnail: crosshair_renderer::Raster {
            width: 1,
            height: 1,
            origin: [0, 0],
            pixels: vec![0; 4],
        },
        raster: std::sync::Arc::new(crosshair_renderer::Raster {
            width: 1,
            height: 1,
            origin: [0, 0],
            pixels: vec![0; 4],
        }),
        bounds: app.bounds.clone(),
        inverses: app.inverses.clone(),
        anchors: app.anchors.clone(),
        adjusted,
    }))?;
    while let Some(event) = EVENTS.with(|events| events.borrow_mut().pop_front()) {
        app.event(event)?;
    }
    Ok(())
}

#[test]
fn animation_mode_rejects_queued_base_edits_and_undo_restores_inheritance() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let base = app.state.presets[0].clone();
    app.document.open(base.clone())?;
    let design_token = app.selection_token();
    app.action("designer-mode", "animate")?;
    assert!(!app.document.dirty);
    app.event(Event::Ui(UiEvent::Property(
        design_token,
        "/transform/x".into(),
        "100".into(),
        2,
    )))?;
    assert_eq!(app.document.draft.as_ref(), Some(&base));
    app.property_edit("@animated:/transform/x", "12", 2)?;
    complete_preview(&mut app, None)?;
    assert_eq!(
        app.document.draft.as_ref().unwrap().layers[0]
            .animations
            .first()
            .unwrap()
            .target
            .x,
        Some(12.0)
    );
    assert_eq!(
        app.document.draft.as_ref().unwrap().layers[0].transform.x,
        base.layers[0].transform.x
    );
    app.action("undo", "")?;
    assert_eq!(app.document.draft.as_ref(), Some(&base));
    ui.hide()?;
    Ok(())
}

#[test]
fn middle_mouse_pan_works_during_animation_without_editing() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let base = app.state.presets[0].clone();
    app.document.open(base.clone())?;
    for (animate, playing) in [(false, false), (true, false), (true, true)] {
        app.animate = animate;
        app.playing = playing;
        ui.global::<Store>().set_pan_x(0.0);
        ui.global::<Store>().set_pan_y(0.0);
        app.canvas_event(0, Point { x: 10.0, y: 20.0 }, 3, false)?;
        app.canvas_event(1, Point { x: 40.0, y: 10.0 }, 3, false)?;
        assert_eq!(ui.global::<Store>().get_pan_x(), 30.0);
        assert_eq!(ui.global::<Store>().get_pan_y(), -10.0);
        app.canvas_event(
            0,
            Point {
                x: -500.0,
                y: -500.0,
            },
            1,
            false,
        )?;
        app.canvas_event(
            1,
            Point {
                x: -1000.0,
                y: -1000.0,
            },
            1,
            false,
        )?;
        app.canvas_event(
            2,
            Point {
                x: -1000.0,
                y: -1000.0,
            },
            1,
            false,
        )?;
        assert_eq!(ui.global::<Store>().get_pan_x(), 30.0);
        app.canvas_event(1, Point { x: 50.0, y: 10.0 }, 3, false)?;
        assert_eq!(ui.global::<Store>().get_pan_x(), 40.0);
        app.canvas_event(2, Point { x: 40.0, y: 10.0 }, 3, false)?;
        assert!(app.drag.is_none());
        app.canvas_event(0, Point { x: 0.0, y: 0.0 }, 3, false)?;
        app.canvas_event(3, Point { x: 0.0, y: 0.0 }, 1, false)?;
        assert!(app.drag.is_none());
        assert_eq!(app.document.draft.as_ref(), Some(&base));
        assert!(!app.document.dirty);
        assert!(!app.document.can_undo());
    }
    app.drag = Some(Drag::Move {
        origin: Point { x: 0.0, y: 0.0 },
        before: base.clone(),
    });
    app.group_open = true;
    app.canvas_event(0, Point { x: 10.0, y: 10.0 }, 3, false)?;
    app.canvas_event(2, Point { x: 20.0, y: 20.0 }, 3, false)?;
    assert!(matches!(app.drag, Some(Drag::Move { .. })));
    assert!(app.group_open);
    ui.hide()?;
    Ok(())
}

#[test]
fn animation_tabs_do_not_edit_and_reject_stale_field_events() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let base = app.state.presets[0].clone();
    app.document.open(base.clone())?;
    app.action("designer-mode", "animate")?;
    let fire_token = app.selection_token();
    app.action("animation-trigger", "movement")?;
    assert_eq!(app.document.draft.as_ref(), Some(&base));
    assert!(!app.document.dirty);
    assert!(!app.document.can_undo());
    assert_eq!(ui.global::<Store>().get_animation_trigger(), 2);
    app.event(Event::Ui(UiEvent::Property(
        fire_token,
        "@animated:/transform/x".into(),
        "100".into(),
        2,
    )))?;
    assert_eq!(app.document.draft.as_ref(), Some(&base));
    let store = ui.global::<Store>();
    assert!(!store.get_animation_target_needed());
    app.property_edit("/animation/enabled", "true", 2)?;
    complete_preview(&mut app, None)?;
    assert!(store.get_animation_target_needed());
    assert!(!store.get_can_preview_movement());
    app.action("undo", "")?;
    assert!(!store.get_animation_target_needed());
    app.property_edit("@animated:/transform/x", "20", 2)?;
    complete_preview(&mut app, None)?;
    assert_eq!(
        app.document.draft.as_ref().unwrap().layers[0].animations[0].trigger,
        AnimationTrigger::Movement
    );
    assert!(!store.get_can_preview_movement());
    app.property_edit("/animation/enabled", "true", 2)?;
    complete_preview(&mut app, None)?;
    assert!(store.get_can_preview_movement());
    assert!(!store.get_animation_target_needed());
    assert!(!store.get_can_preview_aim());
    assert!(!store.get_can_preview_fire());
    app.action("undo", "")?;
    assert!(!store.get_can_preview_movement());
    app.action("animation-trigger", "aim")?;
    app.action("undo", "")?;
    assert_eq!(app.document.draft.as_ref(), Some(&base));
    ui.hide()?;
    Ok(())
}

#[test]
fn animation_enabled_switch_updates_the_live_draft_and_save() -> Result<()> {
    let ui = window();
    let store = ui.global::<Store>();
    let mut app = app(&ui);
    let mut preset = app.state.presets[0].clone();
    preset.layers[0].animations = vec![Animation {
        target: AnimationTarget {
            x: Some(12.0),
            ..Default::default()
        },
        ..Default::default()
    }];
    app.document.open(preset)?;
    // Keep the receiver alive while checking the documents queued for the engine.
    let (client, _commands) = Client::recording_for_test();
    app.client = client;
    store.set_connected(true);
    store.on_property_edit(|selection, key, value, phase| {
        ui_event(UiEvent::Property(
            selection.into(),
            key.into(),
            value.into(),
            phase,
        ));
    });
    app.action("designer-mode", "animate")?;
    assert!(store.get_can_preview_fire());

    slint::platform::update_timers_and_animations();
    pointer(&ui, 0, 1238.0, 205.0);
    pointer(&ui, 2, 1238.0, 205.0);
    while let Some(event) = EVENTS.with(|events| events.borrow_mut().pop_front()) {
        app.event(event)?;
    }
    assert!(!app.document.draft.as_ref().unwrap().layers[0].animations[0].enabled);
    complete_preview(&mut app, None)?;
    let submitted = app.preview_inflight.as_ref().unwrap();
    assert!(!submitted.layers[0].animations[0].enabled);
    assert_eq!(submitted.layers[0].animations[0].target.x, Some(12.0));

    app.save_document()?;
    assert!(!app.inflight.as_ref().unwrap().layers[0].animations[0].enabled);
    ui.hide()?;
    Ok(())
}

#[test]
fn preview_frames_never_edit_the_document_and_stale_bitmaps_are_ignored() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let mut preset = app.state.presets[0].clone();
    preset.layers[0].animations = vec![Animation {
        trigger: AnimationTrigger::Movement,
        target: AnimationTarget {
            x: Some(12.0),
            ..Default::default()
        },
        ..Default::default()
    }];
    app.document.open(preset.clone())?;
    app.animate = true;
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    app.preview_input("movement-down")?;
    assert!(app.playing);
    assert!(
        !app.playback_timer.running(),
        "Wait for the worker before scheduling another frame"
    );
    let generation = app.playback_generation;
    let bitmap = || {
        Arc::new(crosshair_renderer::Raster {
            width: 1,
            height: 1,
            origin: [7, 9],
            pixels: vec![255; 4],
        })
    };
    app.event(Event::Preview(preview::Event::Bitmap {
        generation,
        id: preset.id,
        result: Ok((bitmap(), 1.0)),
    }))?;
    assert_eq!(ui.global::<Store>().get_preview_x(), 7.0);
    assert_eq!(app.document.draft.as_ref(), Some(&preset));
    assert!(!app.document.dirty);
    assert!(!app.document.can_undo());
    assert!(commands().is_empty());
    app.stop_playback();
    ui.global::<Store>().set_preview_x(42.0);
    app.event(Event::Preview(preview::Event::Bitmap {
        generation,
        id: preset.id,
        result: Ok((bitmap(), 1.0)),
    }))?;
    assert_eq!(ui.global::<Store>().get_preview_x(), 42.0);
    app.preview_input("movement-up")?;
    assert!(
        !app.playing,
        "A late release cannot restart a stopped session"
    );
    app.preview_input("movement-down")?;
    app.action("navigate", "2")?;
    let hidden_generation = app.playback_generation;
    app.action("navigate", "1")?;
    assert!(!app.playing);
    assert!(
        app.playback_generation > hidden_generation,
        "Returning to Designer requests the Animated target pose"
    );
    assert_eq!(app.document.draft.as_ref(), Some(&preset));
    ui.hide()?;
    Ok(())
}

#[test]
fn action_picker_cancellation_and_late_profile_events_do_not_commit() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    let target = app.state.presets[0].id.to_string();
    app.action("aim-action", "4")?;
    assert!(commands().is_empty());
    app.action("aim-cancel", "")?;
    app.action("aim-target", &target)?;
    assert!(commands().is_empty());
    app.action("aim-action", "5")?;
    app.action("aim-target", &target)?;
    let committed = commands();
    assert!(
        matches!(&committed[..], [Command::UpsertProfile { profile, .. }] if matches!(profile.actions.aim_action, AimAction::ShowPresetWhileHeld { preset } if preset.to_string() == target))
    );
    app.action("aim-action", "4")?;
    app.state.active_profile = Id::new_v4();
    app.action("aim-target", &target)?;
    assert!(commands().is_empty());
    ui.hide()?;
    Ok(())
}

#[test]
fn game_bar_setup_keeps_desktop_selected_and_allows_retry() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    app.status.widget.installed = true;
    let store = ui.global::<Store>();
    app.refresh_status();
    app.action("output-mode", "1")?;
    app.action("output-mode", "1")?;
    assert!(store.get_gamebar_setup() && store.get_switching_output());
    assert_eq!(store.get_output_mode(), 0);
    assert!(
        matches!(&commands()[..], [Command::SetSettings { settings, .. }] if settings.output_mode == OutputMode::GameBar)
    );
    app.event(Event::Engine(
        0,
        client::Event::Failed {
            tag: "output-mode".into(),
            message: app.status.widget.setup_hint().unwrap().into(),
            conflict: false,
        },
    ))?;
    assert!(!store.get_switching_output());
    assert!(store.get_error().is_empty());
    app.status.widget.connected = true;
    app.status.widget.pinned = true;
    app.status.widget.click_through = true;
    app.refresh_status();
    assert!(store.get_widget_ready());
    app.action("output-mode", "1")?;
    assert!(matches!(&commands()[..], [Command::SetSettings { .. }]));
    let revision = app.state.revision + 1;
    app.event(Event::Engine(
        0,
        client::Event::Completed {
            tag: "output-mode".into(),
            revision,
            reply: Reply::Ack {
                document_revision: None,
            },
        },
    ))?;
    app.action("output-mode", "1")?;
    assert!(commands().is_empty() && store.get_switching_output());
    let mut state = app.state.clone();
    state.revision = revision;
    state.settings.output_mode = OutputMode::GameBar;
    app.event(Event::Engine(0, client::Event::State(state)))?;
    assert!(!store.get_switching_output());
    assert_eq!(store.get_output_mode(), 1);
    Ok(())
}

#[test]
fn title_switch_reflects_master_state_and_clearing_its_key_only_unbinds() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    app.state.profiles[0].actions.aim_action = AimAction::ShowWhileHeld;
    app.status.enabled = true;
    app.status.visible = false;
    app.refresh_status();
    assert!(ui.global::<Store>().get_visible());
    assert_eq!(
        ui.global::<Store>().get_visibility_reason(),
        "Hidden until Aim is held"
    );
    app.action("toggle-visible", "")?;
    assert!(matches!(&commands()[..], [Command::ToggleVisibility]));
    app.action("binding-remove", "global")?;
    assert!(
        matches!(&commands()[..], [Command::SetBindings { profiles, global_toggle: None, .. }] if profiles[0].actions == app.state.profiles[0].actions)
    );
    assert!(app.status.enabled);
    ui.hide()?;
    Ok(())
}

#[test]
fn captures_pin_profiles_reject_pulses_and_replace_roles_atomically() -> Result<()> {
    use crate::features::keybinds::Target;
    let ui = window();
    let mut app = app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    let profile = app.state.active_profile;
    app.begin_capture(Capture::Role(Target::Aim))?;
    commands();
    assert!(
        app.complete_capture(Some(InputChord {
            trigger: InputKey::Pause,
            modifiers: vec![]
        }))
        .is_err()
    );
    assert!(commands().is_empty());
    app.begin_capture(Capture::Role(Target::Fire))?;
    commands();
    app.state.active_profile = Id::new_v4();
    app.complete_capture(Some(InputChord::key(0x20)))?;
    assert!(commands().is_empty());
    app.state.active_profile = profile;
    let mut second = app.state.profiles[0].clone();
    second.id = Id::new_v4();
    app.state.profiles.push(second);
    app.begin_capture(Capture::Global)?;
    commands();
    app.complete_capture(Some(InputChord::key(0x13)))?;
    assert!(matches!(app.modal, Modal::Conflict(..)));
    assert!(commands().is_empty());
    app.confirm_modal("", false)?;
    let committed = commands();
    assert!(
        matches!(&committed[..], [Command::SetBindings { profiles, global_toggle: Some(chord), .. }] if *chord == InputChord::key(0x13) && profiles.len() == 2 && profiles.iter().all(|profile| profile.actions.reload.is_none()))
    );
    ui.hide()?;
    Ok(())
}

#[test]
fn hold_to_hide_capture_binds_clears_and_replaces_conflicts() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    app.refresh_bindings();
    let store = ui.global::<Store>();
    let row = store
        .get_bindings()
        .iter()
        .filter(|row| row.global)
        .last()
        .unwrap();
    assert_eq!(row.action, "Hold to hide");
    assert_eq!(row.chord, "Unbound");
    assert_eq!(store.get_movement_bindings().row_count(), 4);

    app.action("capture", "hide")?;
    assert_eq!(store.get_capture_id(), "hide");
    assert!(matches!(&commands()[..], [Command::BeginCapture { .. }]));
    let g = InputChord::key(0x22);
    app.complete_capture(Some(g.clone()))?;
    assert!(
        matches!(&commands()[..], [Command::SetBindings { profiles, .. }]
        if profiles[0].actions.hold_to_hide.as_ref() == Some(&g))
    );
    app.state.profiles[0].actions.hold_to_hide = Some(g);

    app.action("capture", "hide")?;
    commands();
    let forward = InputChord::key(0x11);
    app.complete_capture(Some(forward.clone()))?;
    assert!(matches!(app.modal, Modal::Conflict(..)));
    assert!(commands().is_empty());
    app.confirm_modal("", false)?;
    assert!(
        matches!(&commands()[..], [Command::SetBindings { profiles, .. }]
        if profiles[0].actions.hold_to_hide.as_ref() == Some(&forward)
            && profiles[0].actions.movement[0].is_none())
    );

    app.action("binding-remove", "hide")?;
    assert!(
        matches!(&commands()[..], [Command::SetBindings { profiles, .. }]
        if profiles[0].actions.hold_to_hide.is_none())
    );
    ui.hide()?;
    Ok(())
}

#[test]
fn movement_capture_rebinds_clears_and_replaces_conflicts() -> Result<()> {
    use crate::features::keybinds::Target;
    let ui = window();
    let mut app = app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    app.begin_capture(Capture::Role(Target::Forward))?;
    commands();
    app.complete_capture(Some(InputChord::key(0x21)))?;
    assert!(matches!(
        &commands()[..], [Command::SetBindings { profiles, .. }]
        if profiles[0].actions.movement[0] == Some(InputChord::key(0x21))
            && profiles[0].actions.movement[1..] == app.state.profiles[0].actions.movement[1..]
    ));
    app.begin_capture(Capture::Role(Target::Left))?;
    commands();
    assert!(
        app.complete_capture(Some(InputChord {
            trigger: InputKey::Pause,
            modifiers: vec![],
        }))
        .is_err()
    );
    assert!(commands().is_empty());
    app.action("binding-remove", "right")?;
    assert!(matches!(
        &commands()[..], [Command::SetBindings { profiles, .. }]
        if profiles[0].actions.movement[3].is_none()
    ));
    app.begin_capture(Capture::Role(Target::Fire))?;
    commands();
    app.complete_capture(Some(InputChord::key(0x11)))?;
    assert!(matches!(app.modal, Modal::Conflict(..)));
    assert!(commands().is_empty());
    app.confirm_modal("", false)?;
    assert!(matches!(
        &commands()[..], [Command::SetBindings { profiles, .. }]
        if profiles[0].actions.movement[0].is_none()
            && profiles[0].actions.fire == Some(InputChord::key(0x11))
    ));
    app.state.settings.global_toggle = Some(InputChord::key(0x11));
    app.state.profiles[0].actions.movement[0] = None;
    app.modal = Modal::NewProfile;
    app.confirm_modal("New profile", false)?;
    assert!(matches!(
        &commands()[..], [Command::UpsertProfile { profile, .. }]
        if profile.actions.movement[0].is_none()
            && profile.actions.movement[1..] == ActionBindings::default().movement[1..]
    ));
    ui.hide()?;
    Ok(())
}

#[test]
fn preview_buttons_pair_pointer_keyboard_and_focus_release_edges() -> Result<()> {
    let ui = window();
    let store = ui.global::<Store>();
    store.set_animate(true);
    store.set_single_selection(true);
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    store.on_action(move |action, value| {
        recorded
            .borrow_mut()
            .push((action.to_string(), value.to_string()))
    });
    pointer(&ui, 0, 1110.0, 223.0);
    pointer(&ui, 2, 1110.0, 223.0);
    slint::platform::update_timers_and_animations();
    assert!(events.borrow().is_empty());

    store.set_can_preview_aim(true);
    store.set_can_preview_fire(true);
    store.set_can_preview_movement(true);
    pointer(&ui, 0, 1110.0, 223.0);
    pointer(&ui, 2, 1110.0, 223.0);
    slint::platform::update_timers_and_animations();
    assert_eq!(
        &*events.borrow(),
        &[
            ("animation-preview".into(), "fire-down".into()),
            ("animation-preview".into(), "fire-up".into())
        ]
    );
    events.borrow_mut().clear();
    for _ in 0..2 {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Space.into(),
        });
    }
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Space.into(),
    });
    assert_eq!(
        events.borrow().len(),
        2,
        "Keyboard repeat must not add presses"
    );
    events.borrow_mut().clear();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Space.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    slint::platform::update_timers_and_animations();
    assert!(
        events.borrow().iter().any(|(_, value)| value == "fire-up"),
        "Moving focus must release held preview input: {:?}",
        events.borrow()
    );
    pointer(&ui, 0, 1030.0, 223.0);
    pointer(&ui, 2, 1030.0, 223.0);
    slint::platform::update_timers_and_animations();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    slint::platform::update_timers_and_animations();
    events.borrow_mut().clear();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Space.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Space.into(),
    });
    assert_eq!(
        &*events.borrow(),
        &[
            ("animation-preview".into(), "fire-down".into()),
            ("animation-preview".into(), "fire-up".into())
        ],
        "Tab reaches each preview button once"
    );
    events.borrow_mut().clear();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Space.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Space.into(),
    });
    assert_eq!(
        &*events.borrow(),
        &[
            ("animation-preview".into(), "movement-down".into()),
            ("animation-preview".into(), "movement-up".into())
        ]
    );
    ui.hide()?;
    Ok(())
}

#[test]
fn hidden_layers_remain_editable_and_visibility_is_undoable() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    app.document.open(app.state.presets[0].clone())?;
    let id = app.document.draft.as_ref().unwrap().layers[0].id;
    app.layer_action("layer-visible", &id.to_string())?;
    assert!(!app.document.draft.as_ref().unwrap().layers[0].visible);
    assert!(app.document.dirty);
    complete_preview(&mut app, None)?;
    app.action("undo", "")?;
    assert!(app.document.draft.as_ref().unwrap().layers[0].visible);
    app.action("redo", "")?;
    assert!(!app.document.draft.as_ref().unwrap().layers[0].visible);
    complete_preview(&mut app, None)?;
    app.layer_action("layer-visible", &id.to_string())?;
    assert!(app.document.draft.as_ref().unwrap().layers[0].visible);
    ui.hide()?;
    Ok(())
}

#[test]
fn canvas_resize_reaches_odd_sizes_and_does_not_grow_past_the_center() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let preset = app.state.presets[0].clone();
    let mut renderer = crosshair_renderer::Renderer::new()?;
    let layer = &preset.layers[0];
    let bounds = renderer.layer_bounds(layer, 1.0)?;
    app.bounds = vec![(layer.id, bounds)];
    app.anchors.insert(layer.id, renderer.anchor_point(layer)?);
    app.document.open(preset)?;
    // The pointer can land off the exact corner within the handle's hit target.
    let origin = Point {
        x: bounds.right + 0.25,
        y: bounds.bottom + 0.25,
    };
    app.canvas_event(0, origin, 1, false)?;
    let span = bounds.width().max(bounds.height());
    for added in 0..6 {
        let ratio = (span + added as f32) / span;
        app.canvas_event(
            1,
            Point {
                x: origin.x * ratio,
                y: origin.y * ratio,
            },
            1,
            false,
        )?;
        let actual = renderer.layer_bounds(&app.document.draft.as_ref().unwrap().layers[0], 1.0)?;
        assert!((actual.width() - span - added as f32).abs() < 0.001);
        assert!((actual.left + actual.right).abs() < 0.001);
        assert!((actual.top + actual.bottom).abs() < 0.001);
    }
    app.canvas_event(
        1,
        Point {
            x: -origin.x,
            y: -origin.y,
        },
        1,
        false,
    )?;
    let actual = renderer.layer_bounds(&app.document.draft.as_ref().unwrap().layers[0], 1.0)?;
    assert!((actual.width() - 1.0).abs() < 0.001);
    ui.hide()?;
    Ok(())
}

#[test]
fn undo_and_keyboard_edits_stop_the_previous_canvas_drag() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let preset = app.state.presets[0].clone();
    let layer = &preset.layers[0];
    app.bounds = vec![(
        layer.id,
        crosshair_renderer::Renderer::new()?.layer_bounds(layer, 1.0)?,
    )];
    app.document.open(preset.clone())?;
    app.canvas_event(0, Point::default(), 1, false)?;
    app.canvas_event(1, Point { x: 2.0, y: 0.0 }, 1, false)?;
    complete_preview(&mut app, None)?;
    app.action("undo", "")?;
    app.canvas_event(1, Point { x: 5.0, y: 0.0 }, 1, false)?;
    assert!(crate::features::designer::same_design(
        app.document.draft.as_ref().unwrap(),
        &preset
    ));
    app.action("redo", "")?;
    complete_preview(&mut app, None)?;
    assert_eq!(
        app.document.draft.as_ref().unwrap().layers[0].transform.x,
        2.0
    );

    app.canvas_event(0, Point::default(), 1, false)?;
    app.canvas_event(1, Point { x: 1.0, y: 0.0 }, 1, false)?;
    complete_preview(&mut app, None)?;
    assert!(app.shortcut(
        SharedString::from(Key::RightArrow).as_str(),
        false,
        false,
        false
    )?);
    assert!(app.drag.is_none());
    app.canvas_event(1, Point { x: 10.0, y: 0.0 }, 1, false)?;
    assert_eq!(
        app.document.draft.as_ref().unwrap().layers[0].transform.x,
        4.0
    );
    app.action("undo", "")?;
    assert_eq!(
        app.document.draft.as_ref().unwrap().layers[0].transform.x,
        3.0
    );
    ui.hide()?;
    Ok(())
}

#[test]
fn keyboard_edits_wait_for_a_scaled_strokes_preview_and_keep_separate_history() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let mut preset = app.state.presets[0].clone();
    preset.layers[0].content = Content::Freehand(Default::default());
    preset.layers[0].transform.scale = 2.0;
    let id = preset.layers[0].id;
    app.document.open(preset.clone())?;
    ui.global::<Store>().set_drawing(true);
    app.inverses.insert(
        id,
        [
            Point::default(),
            Point { x: 0.5, y: 0.0 },
            Point { x: 0.0, y: 0.5 },
        ],
    );
    app.canvas_event(0, Point::default(), 1, false)?;
    app.canvas_event(1, Point { x: 20.0, y: 0.0 }, 1, false)?;
    let generation = app.preview_generation;
    assert!(app.preview_pending);
    app.shortcut(
        SharedString::from(Key::RightArrow).as_str(),
        false,
        false,
        false,
    )?;
    assert!(app.drag.is_none());
    assert!(app.draw_base.is_some());
    assert_eq!(app.preview_generation, generation);
    assert_eq!(app.deferred_ui.len(), 1);
    app.canvas_event(1, Point { x: 40.0, y: 0.0 }, 1, false)?;

    // At scale 2, the new intrinsic pivot at X=5 requires a +5 translation
    // from the preview worker before the queued one-pixel nudge can run.
    let mut adjusted = app.document.draft.as_ref().unwrap().clone();
    adjusted.layers[0].transform.x = 5.0;
    complete_preview(&mut app, Some(adjusted.clone()))?;
    assert!(app.draw_base.is_none());
    let layer = &app.document.draft.as_ref().unwrap().layers[0];
    let mut renderer = crosshair_renderer::Renderer::new()?;
    assert!(
        renderer
            .local_point(layer, Point { x: 1.0, y: 0.0 })?
            .x
            .abs()
            < 0.001
    );
    assert!((renderer.local_point(layer, Point { x: 21.0, y: 0.0 })?.x - 10.0).abs() < 0.001);
    app.action("undo", "")?;
    assert_eq!(app.document.draft.as_ref().unwrap(), &adjusted);
    app.action("undo", "")?;
    assert_eq!(app.document.draft.as_ref().unwrap(), &preset);
    complete_preview(&mut app, None)?;
    app.canvas_event(0, Point::default(), 1, false)?;
    app.canvas_event(1, Point { x: 20.0, y: 0.0 }, 1, false)?;
    app.action("undo", "")?;
    assert!(app.draw_base.is_some());
    assert_eq!(app.deferred_ui.len(), 1);
    complete_preview(&mut app, Some(adjusted.clone()))?;
    assert_eq!(app.document.draft.as_ref().unwrap(), &preset);
    app.action("redo", "")?;
    assert_eq!(app.document.draft.as_ref().unwrap(), &adjusted);
    Ok(())
}

#[test]
fn numeric_drag_commits_release_position_and_escape_restores_the_start() -> Result<()> {
    let ui = window();
    let store = ui.global::<Store>();
    let fields = Rc::new(slint::VecModel::from(vec![FieldGroup {
        first: FieldRow {
            selection: "test".into(),
            key: "/length".into(),
            label: "Length".into(),
            kind: 2,
            numeric: 10.0,
            maximum: 1000.0,
            step: 1.0,
            ..Default::default()
        },
        ..Default::default()
    }]));
    store.set_fields(fields.clone().into());
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    store.on_property_edit(move |_, _, value, phase| {
        let value: f32 = value.parse().unwrap();
        recorded.borrow_mut().push((value, phase));
        if phase == 2 {
            let mut row = fields.row_data(0).unwrap();
            row.first.numeric = value;
            fields.set_row_data(0, row);
        }
    });
    pointer(&ui, 0, 1120.0, 182.0);
    pointer(&ui, 1, 1126.0, 182.0);
    assert_eq!(events.borrow().last(), Some(&(16.0, 1)));
    pointer(&ui, 2, 1129.0, 182.0);
    assert_eq!(events.borrow().last(), Some(&(19.0, 2)));
    pointer(&ui, 0, 1120.0, 182.0);
    pointer(&ui, 1, 1125.0, 182.0);
    assert_eq!(events.borrow().last(), Some(&(24.0, 1)));
    let escape: SharedString = Key::Escape.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: escape.clone(),
    });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: escape });
    assert_eq!(events.borrow().last(), Some(&(19.0, 4)));
    events.borrow_mut().clear();
    pointer(&ui, 1, 1130.0, 182.0);
    pointer(&ui, 2, 1130.0, 182.0);
    assert!(
        events.borrow().is_empty(),
        "A cancelled drag must stay cancelled through release"
    );
    ui.hide()?;
    Ok(())
}

#[test]
fn cancelling_a_numeric_drag_restores_each_selected_layers_starting_value() -> Result<()> {
    let ui = window();
    let mut app = app(&ui);
    let mut preset = app.state.presets[0].clone();
    let mut second = preset.layers[0].clone();
    second.id = Id::new_v4();
    second.transform.x = 10.0;
    preset.layers.push(second);
    app.document.open(preset.clone())?;
    app.document.selected = preset.layers.iter().map(|layer| layer.id).collect();
    app.property_edit("/transform/x", "20", 1)?;
    assert!(
        app.document
            .draft
            .as_ref()
            .unwrap()
            .layers
            .iter()
            .all(|layer| layer.transform.x == 20.0)
    );
    app.property_edit("/transform/x", "", 4)?;
    assert_eq!(app.document.draft.as_ref().unwrap(), &preset);
    assert!(!app.document.dirty);
    assert!(!app.document.can_undo());
    Ok(())
}

#[test]
fn canvas_applies_the_release_position_before_finishing() -> Result<()> {
    let ui = window();
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    ui.global::<Store>()
        .on_canvas_event(move |kind, x, y, button, _| {
            if kind <= 3 {
                recorded.borrow_mut().push((kind, x, y, button));
            }
        });
    pointer(&ui, 0, 600.0, 400.0);
    pointer(&ui, 1, 608.0, 400.0);
    pointer(&ui, 2, 616.0, 400.0);
    let events = events.borrow();
    let release = &events[events.len() - 2..];
    assert_eq!(release[0].0, 1);
    assert_eq!(release[1].0, 2);
    assert_eq!(release[0].1, release[1].1);
    let pressed = events.iter().find(|event| event.0 == 0).unwrap();
    assert_eq!(release[0].1 - pressed.1, 2.0);
    ui.hide()?;
    Ok(())
}

#[test]
fn text_fields_follow_external_values_after_typing() -> Result<()> {
    let ui = window();
    let fields = Rc::new(slint::VecModel::from(vec![FieldGroup {
        first: FieldRow {
            kind: 1,
            label: "Text".into(),
            value: "Before".into(),
            selection: "layer-a".into(),
            ..Default::default()
        },
        ..Default::default()
    }]));
    ui.global::<Store>().set_fields(fields.clone().into());
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    ui.global::<Store>()
        .on_property_edit(move |_, _, value, phase| {
            recorded.borrow_mut().push((value.to_string(), phase));
        });
    pointer(&ui, 0, 1120.0, 182.0);
    pointer(&ui, 2, 1120.0, 182.0);
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "x".into() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "x".into() });
    assert!(!events.borrow().is_empty());
    let mut row = fields.row_data(0).unwrap();
    row.first.value = "Restored".into();
    fields.set_row_data(0, row);
    slint::platform::update_timers_and_animations();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Return.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Return.into(),
    });
    assert_eq!(events.borrow().last(), Some(&("Restored".into(), 2)));
    ui.hide()?;
    Ok(())
}

#[test]
fn color_opacity_drag_does_not_continue_on_a_new_selection() -> Result<()> {
    let ui = window();
    let fields = Rc::new(slint::VecModel::from(vec![FieldGroup {
        first: FieldRow {
            kind: 4,
            label: "Fill".into(),
            selection: "layer-a".into(),
            swatch: slint::Color::from_rgb_u8(255, 0, 0),
            value: "#FF0000FF".into(),
            ..Default::default()
        },
        ..Default::default()
    }]));
    ui.global::<Store>().set_fields(fields.clone().into());
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    ui.global::<Store>()
        .on_property_color(move |selection, _, _, phase| {
            recorded.borrow_mut().push((selection.to_string(), phase));
        });
    pointer(&ui, 0, 1220.0, 182.0);
    pointer(&ui, 1, 1214.0, 182.0);
    assert_eq!(events.borrow().last(), Some(&("layer-a".into(), 1)));
    let mut row = fields.row_data(0).unwrap();
    row.first.selection = "layer-b".into();
    row.first.swatch = slint::Color::from_argb_u8(128, 0, 0, 255);
    fields.set_row_data(0, row);
    slint::platform::update_timers_and_animations();
    events.borrow_mut().clear();
    pointer(&ui, 1, 1208.0, 182.0);
    pointer(&ui, 2, 1208.0, 182.0);
    assert!(events.borrow().is_empty());
    ui.hide()?;
    Ok(())
}

#[test]
fn color_picker_uses_the_final_pointer_position_on_release() -> Result<()> {
    let ui = window();
    ui.global::<Store>().set_fields(
        Rc::new(slint::VecModel::from(vec![FieldGroup {
            first: FieldRow {
                kind: 4,
                label: "Fill".into(),
                selection: "layer".into(),
                swatch: slint::Color::from_rgb_u8(255, 0, 0),
                value: "#FF0000FF".into(),
                ..Default::default()
            },
            ..Default::default()
        }]))
        .into(),
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    ui.global::<Store>()
        .on_property_color(move |_, _, value, phase| recorded.borrow_mut().push((value, phase)));
    pointer(&ui, 0, 1030.0, 182.0);
    pointer(&ui, 2, 1030.0, 182.0);
    // Release can arrive at a different position without an intervening move event.
    for (start, end) in [
        ((780.0, 335.0), (900.0, 335.0)),
        ((780.0, 180.0), (930.0, 260.0)),
    ] {
        events.borrow_mut().clear();
        pointer(&ui, 0, start.0, start.1);
        pointer(&ui, 2, end.0, end.1);
        let values = events.borrow();
        assert_eq!(values.len(), 2, "{values:?}");
        assert_eq!((values[0].1, values[1].1), (1, 2));
        assert_ne!(values[0].0, values[1].0);
    }
    ui.hide()?;
    Ok(())
}

#[test]
fn inspector_choices_follow_restored_document_values() -> Result<()> {
    let ui = window();
    let fields = Rc::new(slint::VecModel::from(vec![FieldGroup {
        first: FieldRow {
            kind: 5,
            label: "Anchor".into(),
            choices: slint::ModelRc::new(slint::VecModel::from(vec![
                "Center".into(),
                "Top".into(),
                "Bottom".into(),
            ])),
            ..Default::default()
        },
        ..Default::default()
    }]));
    ui.global::<Store>().set_fields(fields.clone().into());
    let updated = fields.clone();
    ui.global::<Store>()
        .on_property_edit(move |_, _, value, _| {
            let mut row = updated.row_data(0).unwrap();
            row.first.selected = value.parse().unwrap();
            updated.set_row_data(0, row);
        });
    let key = |key: Key| {
        let text: SharedString = key.into();
        ui.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        ui.window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    };
    pointer(&ui, 0, 1120.0, 182.0);
    pointer(&ui, 2, 1120.0, 182.0);
    key(Key::Escape);
    key(Key::DownArrow);
    assert_eq!(fields.row_data(0).unwrap().first.selected, 1);
    // Undo or switching layers replaces the authoritative property value.
    let mut row = fields.row_data(0).unwrap();
    row.first.selected = 0;
    fields.set_row_data(0, row);
    key(Key::DownArrow);
    assert_eq!(fields.row_data(0).unwrap().first.selected, 1);
    ui.hide()?;
    Ok(())
}
