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

fn app(ui: &AppWindow) -> App {
    App {
        ui: ui.as_weak(),
        client: Client::disconnected_for_test(),
        preview: preview::Preview::idle_for_test(),
        state: AppState::default(),
        status: EngineStatus::default(),
        document: Document::default(),
        fonts: Vec::new(),
        fields: Vec::new(),
        collapsed: BTreeSet::new(),
        thumbnails: BTreeMap::new(),
        palette: Vec::new(),
        browse: Vec::new(),
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
    }
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

fn complete_preview(app: &mut App, adjusted: Option<Preset>) -> Result<()> {
    app.event(Event::Preview(preview::Event::Document {
        scale: 1.0,
        generation: app.preview_generation,
        id: app.document.draft.as_ref().unwrap().id,
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
    pointer(&ui, 0, 1120.0, 138.0);
    pointer(&ui, 1, 1126.0, 138.0);
    assert_eq!(events.borrow().last(), Some(&(16.0, 1)));
    pointer(&ui, 2, 1129.0, 138.0);
    assert_eq!(events.borrow().last(), Some(&(19.0, 2)));
    pointer(&ui, 0, 1120.0, 138.0);
    pointer(&ui, 1, 1125.0, 138.0);
    assert_eq!(events.borrow().last(), Some(&(24.0, 1)));
    let escape: SharedString = Key::Escape.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: escape.clone(),
    });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: escape });
    assert_eq!(events.borrow().last(), Some(&(19.0, 4)));
    events.borrow_mut().clear();
    pointer(&ui, 1, 1130.0, 138.0);
    pointer(&ui, 2, 1130.0, 138.0);
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
    pointer(&ui, 0, 1120.0, 138.0);
    pointer(&ui, 2, 1120.0, 138.0);
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
