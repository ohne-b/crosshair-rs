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
fn layer_eye_click_dispatches_visibility_toggle() -> Result<()> {
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
    store.set_page(1);
    store.set_document_open(true);
    store.set_wide(true);
    store.set_layers(
        Rc::new(slint::VecModel::from(vec![crate::LayerRow {
            id: "test".into(),
            name: "Crosshair".into(),
            kind: "Crosshair".into(),
            shown: true,
            selected: true,
            ..Default::default()
        }]))
        .into(),
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    store.on_action(move |a, v| recorded.borrow_mut().push((a.to_string(), v.to_string())));
    ui.show()?;
    snapshot(&window, "layer-eye.png")?;
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(221.0, 176.0),
    });
    click(&ui, 221.0, 176.0);
    assert!(
        events
            .borrow()
            .contains(&("layer-visible".into(), "test".into())),
        "{:?}",
        events.borrow()
    );
    ui.hide()?;
    Ok(())
}

#[test]
fn header_crosshair_stays_centered_when_source_size_changes() -> Result<()> {
    struct Headless(Rc<MinimalSoftwareWindow>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let ui = AppWindow::new()?;
    ui.global::<Store>()
        .on_scale_thumbnail(preview::scale_thumbnail);
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    ui.show()?;
    for dpi in [1.0f32, 1.25, 1.5, 1.75, 2.0] {
        ui.window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: dpi });
        ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
        for size in [20u32, 21, 24, 25, 28, 31, 32, 48, 60, 80, 120, 256] {
            let mut source = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(size, size);
            let thickness = ((size / 4) & !1) + size % 2;
            let start = (size - thickness) / 2;
            for y in 0..size {
                for x in 0..size {
                    if (start..start + thickness).contains(&x)
                        || (start..start + thickness).contains(&y)
                    {
                        source.make_mut_slice()[(y * size + x) as usize] =
                            slint::Rgba8Pixel::new(255, 0, 0, 255);
                    }
                }
            }
            let raster = crosshair_renderer::Raster {
                width: size,
                height: size,
                origin: [0, 0],
                pixels: source.as_bytes().to_vec(),
            };
            ui.global::<Store>()
                .set_active_thumbnail(preview::image(&raster.thumbnail(24, 1.0)?));
            slint::platform::update_timers_and_animations();
            let physical = ui.window().size();
            let stride = physical.width as usize;
            let mut pixels = vec![slint::Rgb8Pixel::default(); stride * physical.height as usize];
            window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, stride);
            });
            let red = |x: usize, y: usize| {
                let p = pixels[y * stride + x];
                p.r > 160 && p.g < 60
            };
            let xs = (1004.0 * dpi) as usize..(1036.0 * dpi) as usize;
            let ys = (8.0 * dpi) as usize..(40.0 * dpi) as usize;
            let top = ys.clone().find(|&y| xs.clone().any(|x| red(x, y))).unwrap();
            let left = xs.clone().find(|&x| ys.clone().any(|y| red(x, y))).unwrap();
            let stem: Vec<_> = xs.filter(|&x| red(x, top)).collect();
            let center = (stem[0] + stem[stem.len() - 1] + 1) as f32 / 2.0;
            assert!(
                (center - 1020.0 * dpi).abs() <= 0.5,
                "{size}px source at {dpi} DPI: x center {center}"
            );
            let stem: Vec<_> = ys.filter(|&y| red(left, y)).collect();
            let center = (stem[0] + stem[stem.len() - 1] + 1) as f32 / 2.0;
            assert!(
                (center - 24.0 * dpi).abs() <= 0.5,
                "{size}px source at {dpi} DPI: y center {center}"
            );
        }
    }
    ui.hide()?;
    Ok(())
}

#[test]
fn library_tabs_sort_picker_and_browse_actions_are_independent() -> Result<()> {
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
    assert_eq!(
        *events.borrow(),
        [("equip-browse".into(), rows[0].id.to_string())]
    );
    for trigger in [Key::Return, Key::Space] {
        events.borrow_mut().clear();
        key(&ui, trigger);
        assert_eq!(
            *events.borrow(),
            [("equip-browse".into(), rows[0].id.to_string())]
        );
    }
    events.borrow_mut().clear();
    store.set_browse_saving(true);
    key(&ui, Key::Return);
    assert!(
        events.borrow().is_empty(),
        "Saving must not queue duplicate requests"
    );
    store.set_browse_saving(false);
    store.set_connected(false);
    click(&ui, 270.0, 246.0);
    key(&ui, Key::Return);
    assert!(
        events.borrow().is_empty(),
        "Disconnected cards cannot equip"
    );
    store.set_connected(true);
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
    store.set_modal_kind(0);
    store.set_page(3);
    store.set_settings_category(3);
    store.set_version("0.1.0-alpha.6".into());
    store.set_error("Launch at sign-in requires the installed application".into());
    snapshot(&window, "warning-large-text.png")?;
    ui.global::<crate::Theme>().set_text_scale(1.0);
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    snapshot(&window, "warning-and-updates.png")?;
    events.borrow_mut().clear();
    click(&ui, 1240.0, 89.0);
    assert_eq!(*events.borrow(), [("dismiss-error".into(), "".into())]);
    events.borrow_mut().clear();
    store.set_update_phase(1);
    click(&ui, 980.0, 330.0);
    assert!(
        events.borrow().is_empty(),
        "Changing update channel must wait for active work"
    );
    store.set_update_phase(0);
    click(&ui, 980.0, 330.0);
    assert_eq!(
        *events.borrow(),
        [("allow-prereleases".into(), "true".into())]
    );
    ui.hide()?;
    Ok(())
}
