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
fn settings_tooltip_has_a_uniform_fill_inside_its_corners() -> Result<()> {
    use std::{cell::Cell, time::Duration};

    struct Headless(Rc<MinimalSoftwareWindow>, Rc<Cell<Duration>>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
        fn duration_since_start(&self) -> Duration {
            self.1.get()
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let clock = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Headless(window.clone(), clock.clone())))?;
    let ui = AppWindow::new()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    ui.global::<Store>().set_connected(true);
    ui.show()?;
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(28.0, 681.0),
    });
    let before = ui.window().take_snapshot()?;
    clock.set(Duration::from_secs(1));
    slint::platform::update_timers_and_animations();
    let after = ui.window().take_snapshot()?;
    snapshot(&window, "settings-tooltip.png")?;

    let stride = after.width() as usize;
    let changed =
        |x: usize, y: usize| before.as_slice()[y * stride + x] != after.as_slice()[y * stride + x];
    let top = (681..750)
        .find(|&y| changed(45, y))
        .context("Settings tooltip did not appear")?;
    let bottom = (top..750).rfind(|&y| changed(45, y)).unwrap();
    let left = (0..45).find(|&x| changed(x, top + 10)).unwrap();
    let right = (45..180).rfind(|&x| changed(x, top + 10)).unwrap();
    for y in [top + 2, bottom - 2] {
        let pixel = |x: usize| after.as_slice()[y * stride + x];
        assert_eq!(pixel(left + 4), pixel(left + 12));
        assert_eq!(pixel(right - 4), pixel(right - 12));
    }
    let theme = ui.global::<crate::Theme>();
    theme.set_high_contrast(true);
    theme.set_raised(slint::Color::from_rgb_u8(255, 255, 255));
    theme.set_text(slint::Color::from_rgb_u8(0, 0, 0));
    let contrast = ui.window().take_snapshot()?;
    assert_eq!(
        contrast.as_slice()[(top + 5) * stride + left + 8],
        slint::Rgba8Pixel::new(255, 255, 255, 255)
    );
    assert!(
        (top + 5..bottom - 4)
            .any(|y| (left + 6..right - 5).any(|x| contrast.as_slice()[y * stride + x].r < 64))
    );
    ui.hide()?;
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

fn pixel_test_window() -> Result<(AppWindow, Rc<MinimalSoftwareWindow>)> {
    struct Headless(Rc<MinimalSoftwareWindow>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let ui = AppWindow::new()?;
    Ok((ui, window))
}

#[test]
fn file_menu_fits_its_five_icon_rows_at_both_text_sizes() -> Result<()> {
    let (ui, _window) = pixel_test_window()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_document_open(true);
    store.set_document_id("draft".into());
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    store.on_action(move |action, argument| recorded.borrow_mut().push((action, argument)));
    ui.show()?;
    for (scale, row_height) in [(1.0, 32), (2.0, 40)] {
        ui.global::<crate::Theme>().set_text_scale(scale);
        let before = ui.window().take_snapshot()?;
        click(&ui, 315.0, 70.0);
        let menu = ui.window().take_snapshot()?;
        let left = store.get_menu_x().round() as usize;
        let top = store.get_menu_y().round() as usize;
        let stride = menu.width() as usize;
        if scale == 2.0 {
            let extended_row = (top + 10) * stride + left + 220;
            assert_ne!(
                menu.as_slice()[extended_row],
                before.as_slice()[extended_row]
            );
        }
        for i in 0..5 {
            let center = top + 8 + i * (row_height + 2) + row_height / 2;
            assert!(
                (center - 9..center + 9)
                    .flat_map(|y| &menu.as_slice()[y * stride + left + 18..y * stride + left + 36])
                    .filter(|pixel| pixel.r > 90)
                    .count()
                    >= 12,
                "Missing icon in row {i}"
            );
        }
        let bottom = top + 16 + 5 * row_height + 4 * 2;
        let outside = bottom * stride + left + 160;
        assert_ne!(
            menu.as_slice()[outside - 2 * stride],
            before.as_slice()[outside - 2 * stride]
        );
        assert_eq!(menu.as_slice()[outside], before.as_slice()[outside]);
        click(
            &ui,
            (left + 55) as f32,
            (bottom - 8 - row_height / 2) as f32,
        );
        assert_eq!(
            events.borrow_mut().pop(),
            Some(("export-draft".into(), "draft".into()))
        );
    }
    ui.hide()?;
    Ok(())
}

#[test]
fn share_menu_dispatches_each_draft_export_at_both_text_sizes() -> Result<()> {
    let (ui, _window) = pixel_test_window()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_document_open(true);
    store.set_document_id("draft".into());
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    store.on_action(move |action, argument| recorded.borrow_mut().push((action, argument)));
    ui.show()?;
    for (scale, height) in [(1.0, 32.0), (2.0, 40.0)] {
        ui.global::<crate::Theme>().set_text_scale(scale);
        for (row, expected) in [
            ("export-png", ""),
            ("export-svg", ""),
            ("export-draft", "draft"),
        ]
        .into_iter()
        .enumerate()
        {
            click(&ui, 1080.0, 70.0);
            let y = store.get_menu_y() + 8.0 + row as f32 * (height + 2.0) + height / 2.0;
            click(&ui, 1080.0, y);
            assert_eq!(
                events.borrow_mut().pop(),
                Some((expected.0.into(), expected.1.into())),
                "Share row {row} at scale {scale}"
            );
        }
    }
    ui.hide()?;
    Ok(())
}

fn mark_canvas_origin(store: &Store<'_>) {
    let dot = crosshair_renderer::Raster {
        width: 1,
        height: 1,
        origin: [0, 0],
        pixels: vec![255, 0, 255, 255],
    };
    store.set_canvas_tiles(
        Rc::new(slint::VecModel::from(vec![crate::CanvasTile {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            bitmap: preview::image(&dot),
        }]))
        .into(),
    );
}

#[test]
fn canvas_center_axes_stand_out_from_the_grid_at_one_physical_pixel() -> Result<()> {
    let (ui, _) = pixel_test_window()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_document_open(true);
    store.set_zoom(8.0);
    mark_canvas_origin(&store);
    ui.show()?;
    for high_contrast in [false, true] {
        if high_contrast {
            let theme = ui.global::<crate::Theme>();
            theme.set_high_contrast(true);
            theme.set_preview(slint::Color::from_rgb_u8(255, 255, 255));
            theme.set_text(slint::Color::from_rgb_u8(0, 0, 0));
        }
        let frame = ui.window().take_snapshot()?;
        let pixels = frame.as_slice();
        let stride = frame.width() as usize;
        let origin = pixels
            .iter()
            .position(|p| (p.r, p.g, p.b) == (255, 0, 255))
            .unwrap();
        // Sample outside the marker, between two perpendicular grid lines.
        for (axis, across) in [(origin + 19, stride), (origin + 19 * stride, 1)] {
            for neighbor in [axis - across, axis + across, axis + 8 * across] {
                assert!(pixels[axis].r.abs_diff(pixels[neighbor].r) >= 32);
            }
            if high_contrast {
                assert_eq!(pixels[axis].r, 0);
            }
        }
    }
    ui.hide()?;
    Ok(())
}

#[test]
fn canvas_highlights_the_hovered_corner_and_keeps_it_during_resize() -> Result<()> {
    let (ui, window) = pixel_test_window()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_document_open(true);
    store.on_format_number(|n| crate::features::designer::format_number(n).into());
    store.set_selection_bounds(crate::SelectionBounds {
        active: true,
        x: -12.0,
        y: -8.0,
        width: 24.0,
        height: 16.0,
        rotation: 0.0,
    });
    store.set_zoom(8.0);
    mark_canvas_origin(&store);
    ui.show()?;
    let image = ui.window().take_snapshot()?;
    let stride = image.width() as usize;
    let origin = image
        .as_slice()
        .iter()
        .position(|p| (p.r, p.g, p.b) == (255, 0, 255))
        .unwrap();
    let center = [(origin % stride) as i32, (origin / stride) as i32];
    let corners = [(-96, -64), (96, -64), (-96, 64), (96, 64)];
    let move_to = |dx, dy| {
        let position =
            slint::LogicalPosition::new((center[0] + dx) as f32, (center[1] + dy) as f32);
        ui.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        position
    };
    let check = |active| -> Result<()> {
        let frame = ui.window().take_snapshot()?;
        for (i, (dx, dy)) in corners.iter().enumerate() {
            let pixel =
                frame.as_slice()[(center[1] + dy) as usize * stride + (center[0] + dx) as usize];
            assert_eq!(
                (pixel.r, pixel.g, pixel.b) == (249, 115, 22),
                active == Some(i)
            );
        }
        Ok(())
    };
    for (i, &(dx, dy)) in corners.iter().enumerate() {
        move_to(dx, dy);
        check(Some(i))?;
        let hit = store.invoke_canvas_handle_at(192.0, 128.0, 0.0, dx as f32, dy as f32);
        assert_eq!(hit.descending, dx.signum() == dy.signum());
        let rotated = store.invoke_canvas_handle_at(192.0, 128.0, 90.0, -dy as f32, dx as f32);
        assert_eq!((hit.sx, hit.sy), (rotated.sx, rotated.sy));
        assert_ne!(rotated.descending, hit.descending);
    }
    let position = move_to(96, 64);
    let button = slint::platform::PointerEventButton::Left;
    ui.window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    let position = move_to(-96, -64);
    check(Some(3))?;
    ui.window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
    check(Some(0))?;
    move_to(0, 0);
    check(None)?;
    move_to(96, 64);
    snapshot(&window, "canvas-handles.png")?;
    store.set_selection_bounds(crate::SelectionBounds {
        x: -0.5,
        y: -0.5,
        width: 1.0,
        height: 1.0,
        ..store.get_selection_bounds()
    });
    store.set_zoom(1.0);
    move_to(-1, -1);
    let tiny = ui.window().take_snapshot()?;
    let grip = tiny.as_slice()[origin - stride - 1];
    // A grip centered between physical pixels blends its accent with the dark edge.
    assert!(grip.r.saturating_sub(grip.g) > 50 && grip.g.saturating_sub(grip.b) > 25);
    ui.hide()?;
    Ok(())
}

#[test]
fn canvas_zoom_counts_physical_pixels_across_windows_display_scales() -> Result<()> {
    let (ui, window) = pixel_test_window()?;
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_document_open(true);
    mark_canvas_origin(&store);
    ui.show()?;
    for dpi in [1.0, 1.25, 1.5, 1.75, 2.0] {
        ui.window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: dpi });
        ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
        for zoom in [1.0, 8.0] {
            store.set_zoom(zoom);
            let mut first = None;
            for pan in [0.0, 7.0] {
                store.set_pan_x(pan);
                slint::platform::update_timers_and_animations();
                let physical = ui.window().size();
                let stride = physical.width as usize;
                let mut pixels =
                    vec![slint::Rgb8Pixel::default(); stride * physical.height as usize];
                window.draw_if_needed(|renderer| {
                    renderer.render(&mut pixels, stride);
                });
                let mut painted = pixels
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.r == 255 && p.g == 0 && p.b == 255)
                    .map(|(i, _)| i);
                let start = painted.next().expect("the canvas dot is visible");
                assert_eq!(
                    1 + painted.count(),
                    (zoom * zoom) as usize,
                    "{dpi}x display, {zoom}x zoom"
                );
                if let Some(first) = first {
                    assert_eq!(start, first + 7, "pan offsets use physical pixels");
                }
                first = Some(start);
                assert_eq!(store.get_canvas_view().scale, zoom);
            }
        }
    }
    ui.hide()?;
    Ok(())
}

#[test]
fn header_crosshair_stays_centered_when_source_size_changes() -> Result<()> {
    let (ui, window) = pixel_test_window()?;
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
    use std::{cell::Cell, time::Duration};

    struct Headless(Rc<MinimalSoftwareWindow>, Rc<Cell<Duration>>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }

        fn duration_since_start(&self) -> Duration {
            self.1.get()
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let clock = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Headless(window.clone(), clock.clone())))?;
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
    click(&ui, 28.0, 681.0);
    click(&ui, 28.0, 725.0);
    key(&ui, Key::Return);
    click(&ui, 28.0, 769.0);
    assert!(events.borrow().ends_with(&[
        ("navigate".into(), "3".into()),
        ("discord".into(), "".into()),
        ("discord".into(), "".into()),
        ("github".into(), "".into()),
    ]));
    events.borrow_mut().clear();
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
    let mut app = super::gesture_tests::app(&ui);
    let (client, commands) = Client::recording_for_test();
    app.client = client;
    app.browse = crate::features::browse::presets();
    let id = rows[0].id.as_str();
    let tag = format!("save-browse:{id}");
    app.action("save-browse", id)?;
    assert!(
        matches!(&commands()[..], [Command::AddPreset { preset }] if preset.id.to_string() == id)
    );
    assert!(store.get_browse_saving());
    assert!(store.get_browse_saved().is_empty());
    app.event(Event::Engine(
        0,
        client::Event::Failed {
            tag: tag.clone(),
            message: "Save failed".into(),
            conflict: false,
        },
    ))?;
    assert!(!store.get_browse_saving());
    assert!(store.get_browse_saved().is_empty());
    store.set_error("".into());

    app.action("save-browse", id)?;
    app.event(Event::Engine(
        0,
        client::Event::Completed {
            tag,
            reply: Reply::Created { id: Id::new_v4() },
        },
    ))?;
    assert_eq!(store.get_browse_saved(), id);
    assert!(!store.get_browse_saving());
    events.borrow_mut().clear();
    click(&ui, 277.0, 213.0);
    assert!(
        events.borrow().is_empty(),
        "The checkmark only confirms the save"
    );

    let position = slint::LogicalPosition::new(277.0, 213.0);
    let button = slint::platform::PointerEventButton::Left;
    ui.window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    clock.set(Duration::from_millis(1500));
    slint::platform::update_timers_and_animations();
    assert!(store.get_browse_saved().is_empty());
    ui.window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
    assert!(
        events.borrow().is_empty(),
        "A click that began on the checkmark must not save"
    );

    click(&ui, 277.0, 213.0);
    assert_eq!(*events.borrow(), [("save-browse".into(), id.into())]);
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
    let movement = [
        ("forward", "W", "\u{2191}"),
        ("left", "A", "\u{2190}"),
        ("backward", "S", "\u{2193}"),
        ("right", "D", "\u{2192}"),
    ];
    store.set_movement_bindings(
        Rc::new(slint::VecModel::from(
            movement
                .map(|(id, chord, direction)| crate::BindingRow {
                    id: id.into(),
                    action: id.into(),
                    chord: chord.into(),
                    direction: direction.into(),
                    enabled: true,
                    ..Default::default()
                })
                .to_vec(),
        ))
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
    click(&ui, 1240.0, 71.0);
    assert_eq!(*events.borrow(), [("dismiss-error".into(), "".into())]);
    events.borrow_mut().clear();
    store.set_update_phase(1);
    click(&ui, 980.0, 294.0);
    assert!(
        events.borrow().is_empty(),
        "Changing update channel must wait for active work"
    );
    store.set_update_phase(0);
    click(&ui, 980.0, 294.0);
    assert_eq!(
        *events.borrow(),
        [("allow-prereleases".into(), "true".into())]
    );
    store.set_error("".into());
    store.set_page(1);
    store.set_document_open(true);
    store.set_animate(true);
    store.set_single_selection(true);
    snapshot(&window, "movement-preview-default.png")?;
    ui.window().set_size(slint::LogicalSize::new(960.0, 640.0));
    ui.global::<crate::Theme>().set_text_scale(2.0);
    snapshot(&window, "movement-preview-large-text.png")?;
    ui.hide()?;
    Ok(())
}

#[test]
fn update_download_progress_and_actions_share_a_stable_row() -> Result<()> {
    use crate::features::settings;
    use crosshair_core::protocol::{UpdatePhase::*, UpdateStatus};

    struct Headless(Rc<MinimalSoftwareWindow>);
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone())))?;
    let ui = AppWindow::new()?;
    let store = ui.global::<Store>();
    store.set_connected(true);
    store.set_page(3);
    store.set_settings_category(3);
    store.set_version("0.1.0-alpha.7".into());
    store.set_update_version("0.1.0-alpha.8".into());
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    store.on_action(move |action, value| {
        recorded
            .borrow_mut()
            .push((action.to_string(), value.to_string()));
    });
    ui.show()?;
    for (layout, width, height, scale) in [
        ("normal", 1280.0, 800.0, 1.0),
        ("narrow", 960.0, 640.0, 1.0),
        ("large-text", 960.0, 640.0, 2.0),
    ] {
        ui.window().set_size(slint::LogicalSize::new(width, height));
        ui.global::<crate::Theme>().set_text_scale(scale);
        let mut progress_frames = Vec::new();
        for (name, phase, phase_index, downloaded, total, error) in [
            ("available", Available, 3, 0, None, None),
            ("zero", Downloading, 4, 0, Some(8_000_000), None),
            ("half", Downloading, 4, 4_000_000, Some(8_000_000), None),
            ("complete", Downloading, 4, 8_000_000, Some(8_000_000), None),
            (
                "over-total",
                Downloading,
                4,
                9_000_000,
                Some(8_000_000),
                None,
            ),
            ("unknown", Downloading, 4, 4_000_000, None, None),
            ("retry", Failed, 7, 0, None, Some("Download interrupted")),
            ("ready", Ready, 5, 8_000_000, Some(8_000_000), None),
            (
                "installing",
                Installing,
                6,
                8_000_000,
                Some(8_000_000),
                None,
            ),
        ] {
            let status = UpdateStatus {
                phase,
                version: Some("0.1.0-alpha.8".into()),
                downloaded,
                total,
                error: error.map(str::to_owned),
                ..Default::default()
            };
            store.set_update_phase(phase_index);
            store.set_update_progress(total.map_or(0.0, |total| downloaded as f32 / total as f32));
            store.set_update_total(total.is_some());
            store.set_update_status(settings::update_label(&status).into());
            store.set_update_bytes(settings::download_progress(&status).into());
            store.set_update_error(error.unwrap_or_default().into());
            snapshot(&window, &format!("updates-{layout}-{name}.png"))?;
            if phase_index == 4 {
                ui.window().request_redraw();
                let mut pixels = vec![slint::Rgb8Pixel::default(); (width * height) as usize];
                window.draw_if_needed(|renderer| {
                    renderer.render(&mut pixels, width as usize);
                });
                progress_frames.push(pixels);
            }
        }
        let orange = |p: &slint::Rgb8Pixel| (p.r, p.g, p.b) == (249, 115, 22);
        let longest_run = |pixels: &[slint::Rgb8Pixel]| {
            let mut longest = (0, 0, 0);
            for (y, row) in pixels.chunks_exact(width as usize).enumerate().skip(300) {
                let mut start = 0;
                for (x, pixel) in row.iter().enumerate() {
                    if !orange(pixel) {
                        start = x + 1;
                    } else if x + 1 - start > longest.2 {
                        longest = (start, y, x + 1 - start);
                    }
                }
            }
            longest
        };
        let full = longest_run(&progress_frames[2]);
        let half = longest_run(&progress_frames[1]);
        assert!(
            full.2 > if scale == 1.0 { 300 } else { 80 },
            "{layout}: progress must use the available row width"
        );
        assert_eq!(
            half.0, full.0,
            "{layout}: progress must start at the left edge"
        );
        let half_track = progress_frames[1]
            [half.1 * width as usize + half.0..(half.1 + 1) * width as usize]
            .iter()
            .take_while(|p| p.r >= 48 && p.g >= 48 && p.b <= 48)
            .count();
        assert!((half.2 as f32 - half_track as f32 / 2.0).abs() <= 1.5);
        let over_total = longest_run(&progress_frames[3]);
        assert_eq!((over_total.0, over_total.1), (full.0, full.1));
        assert!(
            over_total.2.abs_diff(full.2) <= (2.0 * scale) as usize,
            "Progress must stay bounded when the adjacent byte count changes"
        );
        for frame in [&progress_frames[0], &progress_frames[4]] {
            assert!(
                !frame[full.1 * width as usize + full.0..full.1 * width as usize + full.0 + full.2]
                    .iter()
                    .any(orange),
                "{layout}: zero/unknown totals must not show filled progress"
            );
        }
        store.set_update_status("Downloading update…".into());
        store.set_update_phase(4);
        store.set_update_error("".into());
        store.set_update_total(true);
        store.set_update_progress(0.5);
        let button_y = full.1 as f32 + 1.0;
        events.borrow_mut().clear();
        click(&ui, 300.0, button_y);
        assert_eq!(*events.borrow(), [("cancel-update".into(), "".into())]);
        events.borrow_mut().clear();
        let row_right = (236.0 + 800.0 * scale).min(width) - 40.0;
        click(&ui, row_right - 6.0, button_y);
        assert!(
            events.borrow().is_empty(),
            "Cancel must replace the left Download button"
        );
        for (phase, expected) in [
            (3, "download-update"),
            (7, "download-update"),
            (5, "install-update"),
        ] {
            store.set_update_phase(phase);
            events.borrow_mut().clear();
            click(&ui, 300.0, button_y);
            assert_eq!(
                *events.borrow(),
                [(expected.into(), "".into())],
                "{layout}: phase {phase}"
            );
        }
        store.set_update_phase(6);
        events.borrow_mut().clear();
        click(&ui, 300.0, button_y);
        assert!(
            events.borrow().is_empty(),
            "Installation must not start twice"
        );
    }
    ui.hide()?;
    Ok(())
}
