use super::*;
use crate::{PresetGroup, PresetRow};
use slint::platform::{Key, Platform, WindowAdapter, WindowEvent, software_renderer::*};

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

fn key(ui: &AppWindow, text: impl Into<SharedString>) {
    let text = text.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn snapshot(window: &MinimalSoftwareWindow, name: &str) -> Result<()> {
    if let Some(directory) = std::env::var_os("CROSSHAIR_UI_SNAPSHOT") {
        let size = window.window().size();
        let mut pixels =
            vec![slint::Rgb8Pixel::default(); size.width as usize * size.height as usize];
        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, size.width as usize);
        });
        let raster = crosshair_renderer::Raster {
            width: size.width,
            height: size.height,
            origin: [0, 0],
            pixels: pixels.iter().flat_map(|p| [p.r, p.g, p.b, 255]).collect(),
        };
        std::fs::write(
            std::path::Path::new(&directory).join(name),
            raster.to_png()?,
        )?;
    }
    Ok(())
}

#[test]
fn library_tabs_sort_picker_and_browse_save_use_the_correct_actions() -> Result<()> {
    struct Headless(Rc<MinimalSoftwareWindow>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let ui = AppWindow::new()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let store = ui.global::<Store>();
    store.set_connected(true);
    store.on_commit_focused_input(slint::platform::update_timers_and_animations);
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
            "sort" => store.set_sort(value.parse().unwrap()),
            "library-tab" => {
                store.set_browsing(value == "browse");
                store.set_search("".into());
            }
            "search" => store.set_search(value),
            "library-columns" => store.set_library_columns(value.parse().unwrap()),
            _ => {}
        }
    });
    ui.show()?;
    click(&ui, 1180.0, 130.0);
    key(&ui, Key::DownArrow);
    key(&ui, Key::Return);
    assert_eq!(store.get_sort(), 1, "events: {:?}", events.borrow());
    assert!(events.borrow().contains(&("sort".into(), "1".into())));
    // Manual reordering can change the mode after the picker has been used.
    store.set_sort(0);
    click(&ui, 1180.0, 130.0);
    key(&ui, Key::DownArrow);
    key(&ui, Key::Return);
    assert_eq!(store.get_sort(), 1);
    click(&ui, 1180.0, 130.0);
    key(&ui, Key::End);
    key(&ui, Key::Return);
    assert_eq!(store.get_sort(), 2);
    click(&ui, 200.0, 130.0);
    key(&ui, "x");
    assert_eq!(store.get_search(), "x");
    click(&ui, 330.0, 80.0);
    assert!(store.get_browsing(), "events: {:?}", events.borrow());
    click(&ui, 200.0, 130.0);
    key(&ui, "s");
    assert_eq!(
        store.get_search(),
        "s",
        "Switching tabs must clear the visible search field too"
    );
    store.set_search("".into());
    slint::platform::update_timers_and_animations();

    let mut renderer = crosshair_renderer::Renderer::new()?;
    let rows = crate::features::browse::presets()
        .iter()
        .map(|p| {
            let raster = renderer.render(p, 1.0, &|_| anyhow::bail!("Unexpected image"))?;
            let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &raster.pixels,
                raster.width,
                raster.height,
            );
            Ok(PresetRow {
                id: p.id.to_string().into(),
                name: p.name.clone().into(),
                thumbnail: slint::Image::from_rgba8_premultiplied(pixels),
                ..Default::default()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let columns = store.get_library_columns() as usize;
    store.set_presets(Rc::new(slint::VecModel::from(rows.clone())).into());
    store.set_preset_groups(
        Rc::new(slint::VecModel::from(
            rows.chunks(columns)
                .map(|rows| PresetGroup {
                    items: Rc::new(slint::VecModel::from(rows.to_vec())).into(),
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    events.borrow_mut().clear();
    click(&ui, 270.0, 246.0);
    assert!(
        events.borrow().is_empty(),
        "Browse body must not select a saved design"
    );
    key(&ui, Key::Return);
    assert_eq!(
        *events.borrow(),
        [("save-browse".into(), rows[0].id.to_string())]
    );
    events.borrow_mut().clear();
    store.set_browse_saving(true);
    key(&ui, Key::Return);
    assert!(
        events.borrow().is_empty(),
        "Saving must not queue duplicate requests"
    );
    store.set_browse_saving(false);
    key(&ui, Key::F2);
    key(&ui, Key::Delete);
    assert!(
        events.borrow().is_empty(),
        "Built-ins cannot be renamed or deleted"
    );
    click(&ui, 277.0, 213.0);
    assert_eq!(
        *events.borrow(),
        [("save-browse".into(), rows[0].id.to_string())]
    );
    snapshot(&window, "browse.png")?;
    click(&ui, 220.0, 80.0);
    assert!(!store.get_browsing());
    slint::platform::update_timers_and_animations();
    let mut saved = rows[..3].to_vec();
    saved[0].favorite = true;
    store.set_presets(Rc::new(slint::VecModel::from(saved.clone())).into());
    store.set_preset_groups(
        Rc::new(slint::VecModel::from(vec![PresetGroup {
            items: Rc::new(slint::VecModel::from(saved)).into(),
        }]))
        .into(),
    );
    store.set_total_presets(3);
    store.set_favorite_presets(1);
    events.borrow_mut().clear();
    snapshot(&window, "saved.png")?;
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(330.0, 80.0),
    });
    snapshot(&window, "tabs-hover.png")?;
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(279.0, 213.0),
    });
    snapshot(&window, "saved-hover.png")?;
    click(&ui, 279.0, 213.0);
    assert_eq!(
        *events.borrow(),
        [("favorite".into(), rows[0].id.to_string())],
        "Favoriting must not activate the card"
    );
    store.set_page(2);
    store.set_preset_thumbnails(
        Rc::new(slint::VecModel::from(vec![rows[0].thumbnail.clone()])).into(),
    );
    store.set_bindings(
        Rc::new(slint::VecModel::from(vec![crate::BindingRow {
            id: "shortcut".into(),
            shortcut: true,
            name: "Shotgun".into(),
            chord: "J".into(),
            enabled: true,
            ..Default::default()
        }]))
        .into(),
    );
    snapshot(&window, "shortcuts.png")?;
    store.set_modal_kind(1);
    store.set_modal_title("Delete crosshair?".into());
    store.set_modal_message("Delete Shotgun from your saved crosshairs?".into());
    store.set_modal_confirm("Delete".into());
    snapshot(&window, "delete-dialog.png")?;
    store.set_modal_title("Save this crosshair?".into());
    store.set_modal_message("Save your edits before closing, or discard them.".into());
    store.set_modal_alternate("Discard".into());
    store.set_modal_confirm("Save".into());
    snapshot(&window, "discard-dialog.png")?;
    ui.window().set_size(slint::LogicalSize::new(960.0, 640.0));
    ui.global::<crate::Theme>().set_text_scale(2.0);
    store.set_modal_kind(0);
    store.set_page(0);
    snapshot(&window, "narrow-large-text.png")?;
    store.set_modal_kind(1);
    snapshot(&window, "discard-large-text.png")?;
    ui.hide()?;
    Ok(())
}
