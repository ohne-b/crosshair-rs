use super::*;
use crate::{PresetGroup, PresetRow};
use slint::platform::{Key, Platform, WindowAdapter, WindowEvent, software_renderer::*};
use std::{cell::Cell, time::Duration};

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

fn timed_test_window(clock: Rc<Cell<Duration>>) -> Result<(AppWindow, Rc<MinimalSoftwareWindow>)> {
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
    slint::platform::set_platform(Box::new(Headless(window.clone(), clock)))?;
    let ui = AppWindow::new()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    Ok((ui, window))
}

#[test]
fn settings_tooltip_has_a_uniform_fill_inside_its_corners() -> Result<()> {
    let clock = Rc::new(Cell::default());
    let (ui, window) = timed_test_window(clock.clone())?;
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
fn sidebar_update_link_opens_updates_and_shows_the_offered_version() -> Result<()> {
    let clock = Rc::new(Cell::default());
    let (ui, _) = timed_test_window(clock.clone())?;
    let store = ui.global::<Store>();
    let app = RefCell::new(super::gesture_tests::app(&ui));
    store.on_action(move |action, value| app.borrow_mut().action(&action, &value).unwrap());
    ui.show()?;
    for (phase, version, connected, visible) in [
        (0, "", true, false),
        (1, "0.2.0", true, false),
        (2, "", true, false),
        (3, "0.2.0", true, true),
        (4, "0.2.0", true, true),
        (5, "0.2.0", true, true),
        (6, "0.2.0", true, false),
        (7, "", true, false),
        (7, "0.2.0", true, true),
        (3, "0.2.0", false, false),
    ] {
        store.set_page(0);
        store.set_settings_category(1);
        store.set_connected(connected);
        store.set_update_phase(phase);
        store.set_update_version(version.into());
        click(&ui, 28.0, 637.0); // The footer row immediately above Settings.
        assert_eq!(
            (store.get_page(), store.get_settings_category()),
            if visible { (3, 3) } else { (0, 1) }
        );
    }
    store.set_connected(true);
    store.set_update_phase(3);
    let tooltip_right_edge = |version: &str| -> Result<usize> {
        let hover = |x, y| {
            ui.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(x, y),
            })
        };
        hover(400.0, 600.0);
        store.set_update_version(version.into());
        hover(28.0, 637.0);
        let before = ui.window().take_snapshot()?;
        clock.set(clock.get() + Duration::from_secs(1));
        slint::platform::update_timers_and_animations();
        let after = ui.window().take_snapshot()?;
        let stride = after.width() as usize;
        (50..350)
            .rfind(|&x| {
                (638..705).any(|y| {
                    let at = y * stride + x;
                    before.as_slice()[at] != after.as_slice()[at]
                })
            })
            .context("No update tooltip above Settings")
    };
    let stable = tooltip_right_edge("0.2.0")?;
    assert!(tooltip_right_edge("0.2.0-rc.12")? > stable);
    ui.hide()?;
    Ok(())
}

#[test]
fn layer_actions_center_their_highlight_and_keep_full_height_hit_targets() -> Result<()> {
    let (ui, window) = pixel_test_window()?;
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
    for (x, hit_y, action) in [(221, 158, "layer-visible"), (255, 193, "delete-layer")] {
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(x as f32, hit_y as f32),
        });
        let frame = ui.window().take_snapshot()?;
        let pixel = |y| frame.as_slice()[y * frame.width() as usize + x];
        // The 38 px selected row has 3 px of padding above and below the highlight.
        assert_eq!(pixel(159), pixel(192), "{action} vertical padding");
        assert_eq!(
            pixel(159),
            frame.as_slice()[159 * frame.width() as usize + 175]
        );
        assert_ne!(pixel(160), pixel(159), "{action} highlight top");
        assert_eq!(pixel(191), pixel(160), "{action} highlight bottom");
        if action == "delete-layer" {
            snapshot(&window, "layer-action-hover.png")?;
        }
        events.borrow_mut().clear();
        click(&ui, x as f32, hit_y as f32);
        assert_eq!(*events.borrow(), [(action.into(), "test".into())]);
    }
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
fn canvas_alignment_dots_only_appear_during_layer_moves() -> Result<()> {
    let (ui, _) = pixel_test_window()?;
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_zoom(8.0);
    let mut app = super::gesture_tests::app(&ui);
    let mut preset = app.state.presets[0].clone();
    let layer = &mut preset.layers[0];
    layer.content = Content::Rectangle(Rectangle {
        width: 16.0,
        height: 16.0,
        ..Default::default()
    });
    let mut renderer = crosshair_renderer::Renderer::new()?;
    app.bounds = vec![(layer.id, renderer.canvas_layer_bounds(layer)?)];
    app.anchors.insert(layer.id, renderer.anchor_point(layer)?);
    app.hits = super::gesture_tests::test_pick_map(&preset)?;
    app.document.open(preset)?;
    app.refresh_document();
    let app = RefCell::new(app);
    store.on_canvas_event(move |kind, x, y, button, shift| {
        app.borrow_mut()
            .canvas_event(kind, Point { x, y }, button, shift)
            .unwrap();
    });
    mark_canvas_origin(&store);
    ui.show()?;
    let frame = ui.window().take_snapshot()?;
    let stride = frame.width() as usize;
    let origin = frame
        .as_slice()
        .iter()
        .position(|p| (p.r, p.g, p.b) == (255, 0, 255))
        .unwrap();
    let pointer = |kind, dx, dy| {
        super::gesture_tests::pointer(
            &ui,
            kind,
            (origin % stride) as f32 + dx,
            (origin / stride) as f32 + dy,
        )
    };
    let has_dots = || -> Result<bool> {
        let frame = ui.window().take_snapshot()?;
        Ok((125..137).any(|i| {
            let p = frame.as_slice()[origin + i];
            (p.r, p.g, p.b) == (234, 179, 8)
        }))
    };
    for (dx, dy, moving) in [(64.0, 64.0, false), (0.0, -85.0, false), (0.0, 0.0, true)] {
        pointer(1, dx, dy);
        pointer(0, dx, dy);
        assert!(!has_dots()?);
        pointer(1, dx + 16.0, dy);
        assert_eq!(has_dots()?, moving);
        pointer(2, dx + 16.0, dy);
        assert!(!has_dots()?);
    }
    ui.hide()?;
    Ok(())
}

#[test]
fn canvas_highlights_the_hovered_corner_and_keeps_it_during_resize() -> Result<()> {
    use slint::private_unstable_api::re_exports::{
        BuiltInMouseCursor::{EwResize, NeswResize, NsResize, NwseResize},
        MouseCursorInner::BuiltIn,
    };
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
        for (angle, same_sign, opposite_sign) in [
            (0.0_f32, NwseResize, NeswResize),
            (22.0, NwseResize, NeswResize),
            (23.0, NsResize, EwResize),
            (45.0, NsResize, EwResize),
            (68.0, NeswResize, NwseResize),
            (90.0, NeswResize, NwseResize),
            (-45.0, EwResize, NsResize),
            (225.0, NsResize, EwResize),
        ] {
            let (sin, cos) = angle.to_radians().sin_cos();
            let (x, y) = (dx as f32, dy as f32);
            let hit = store.invoke_canvas_handle_at(
                192.0,
                128.0,
                angle,
                x * cos - y * sin,
                x * sin + y * cos,
            );
            assert_eq!((hit.sx, hit.sy), (x.signum(), y.signum()));
            let expected = if dx.signum() == dy.signum() {
                same_sign
            } else {
                opposite_sign
            };
            assert_eq!(
                hit.cursor,
                BuiltIn(expected),
                "corner {i} at {angle} degrees"
            );
        }
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

fn assert_import_icon_hover(
    idle: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    hover: &slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    left: usize,
    top: usize,
) {
    let mut orange_pixels = 0;
    for y in 0..176 {
        for x in 0..280 {
            let index = (top + y) * hover.width() as usize + left + x;
            let pixel = hover.as_slice()[index];
            if (128..152).contains(&x) && (16..40).contains(&y) {
                if pixel.r > pixel.g.saturating_add(45) && pixel.r > 115 {
                    orange_pixels += 1;
                }
            } else {
                assert_eq!(
                    pixel,
                    idle.as_slice()[index],
                    "Only the import icon changes on hover"
                );
            }
        }
    }
    assert!(orange_pixels > 15);
}

#[test]
fn designer_image_popover_highlights_supported_drops_and_keeps_the_prompt() -> Result<()> {
    let clock = Rc::new(Cell::default());
    let (ui, window) = timed_test_window(clock.clone())?;
    let store = ui.global::<Store>();
    store.set_page(1);
    store.set_connected(true);
    store.set_document_open(true);
    store.set_document_name("Classic".into());
    let pointer = Rc::new(Cell::new((-1.0, -1.0)));
    let pos = pointer.clone();
    ui.on_file_pointer(move || pos.get());
    let actions = Rc::new(RefCell::new(Vec::new()));
    let recorded = actions.clone();
    store.on_action(move |name, value| recorded.borrow_mut().push((name, value)));
    ui.show()?;
    ui.window().take_snapshot()?;
    click(&ui, 544.0, 730.0);
    let center = store.get_menu_x() as usize;
    let bottom = store.get_menu_y() as usize;
    assert!((480..560).contains(&center) && (680..710).contains(&bottom));
    snapshot(&window, "designer-image-import.png")?;
    let idle = ui.window().take_snapshot()?;
    pointer.set((center as f32, bottom as f32 - 80.0));
    ui.invoke_file_dropped("preset.json".into(), false);
    assert!(actions.borrow().is_empty());
    ui.set_dragging_import_file(true);
    clock.set(clock.get() + Duration::from_millis(50));
    slint::platform::update_timers_and_animations();
    let hover = ui.window().take_snapshot()?;
    assert_import_icon_hover(&idle, &hover, center - 140, bottom - 176);
    snapshot(&window, "designer-image-import-hover.png")?;
    pointer.set((50.0, 50.0));
    ui.invoke_file_dropped("outside.jpg".into(), true);
    assert!(actions.borrow().is_empty());
    pointer.set((center as f32, bottom as f32 - 80.0));
    ui.set_dragging_import_file(false);
    ui.invoke_file_dropped("layer.PNG".into(), true);
    click(&ui, 544.0, 730.0);
    key(&ui, Key::Return);
    assert_eq!(
        *actions.borrow(),
        [
            ("drop-image".into(), "layer.PNG".into()),
            ("add-image".into(), "".into())
        ]
    );
    ui.hide()?;
    Ok(())
}

#[test]
fn dropped_files_follow_the_active_import_target() -> Result<()> {
    let (ui, _window) = timed_test_window(Rc::new(Cell::default()))?;
    let store = ui.global::<Store>();
    let actions = Rc::new(RefCell::new(Vec::new()));
    let recorded = actions.clone();
    store.on_action(move |name, value| recorded.borrow_mut().push((name, value)));
    store.set_browsing(true);
    ui.invoke_file_dropped("preset.json".into(), true);
    store.set_browsing(false);
    store.set_modal_kind(2);
    ui.invoke_file_dropped("preset.json".into(), true);
    store.set_modal_kind(0);
    store.set_page(1);
    ui.invoke_file_dropped("layer.png".into(), true);
    assert_eq!(
        *actions.borrow(),
        [("drop-image".into(), "layer.png".into())]
    );

    let mut app = super::gesture_tests::app(&ui);
    let (client, _commands) = Client::recording_for_test();
    app.client = client;
    app.action("import", "preset.JSON")?;
    app.action("import", "preset.crosshair")?;
    let error = app.action("import", "layer.png").unwrap_err();
    assert!(error.to_string().contains(".json or .crosshair"));
    Ok(())
}

#[test]
fn action_shortcut_fields_and_clear_buttons_follow_hover_and_focus() -> Result<()> {
    let (ui, _window) = timed_test_window(Rc::new(Cell::default()))?;
    let store = ui.global::<Store>();
    store.set_connected(true);
    store.set_page(2);
    let mut app = super::gesture_tests::app(&ui);
    app.state.profiles[0].actions.controller.fire = Some(GamepadControl::RightTrigger);
    app.refresh_bindings();
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    store.on_action(move |action, value| {
        recorded
            .borrow_mut()
            .push((action.to_string(), value.to_string()));
    });
    ui.show()?;

    for width in [1280.0, 960.0] {
        ui.window().set_size(slint::LogicalSize::new(width, 800.0));
        let right = width - 24.0;
        let column_width = (width - 106.0) * 0.26;
        let move_to = |x, y| {
            ui.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(x, y),
            });
        };
        // Sample the field and clear button backgrounds, away from the text and icon.
        let backgrounds = |edge: f32, y: usize| -> Result<_> {
            let frame = ui.window().take_snapshot()?;
            Ok([edge - 60.0, edge - 24.0]
                .map(|x| frame.as_slice()[(y + 10) * frame.width() as usize + x as usize]))
        };
        let keyboard = right - column_width;
        move_to(1.0, 1.0);
        let idle = backgrounds(keyboard, 341)?;
        assert_eq!(idle[0], idle[1]);
        move_to(keyboard - 70.0, 341.0);
        let hovered = backgrounds(keyboard, 341)?;
        assert_ne!(hovered[0], idle[0]);
        assert_eq!(hovered[0], hovered[1]);
        assert_eq!(backgrounds(right, 341)?, idle);
        move_to(keyboard - 24.0, 341.0);
        assert_ne!(backgrounds(keyboard, 341)?[0], idle[0]);
        events.borrow_mut().clear();
        click(&ui, keyboard - 24.0, 341.0);
        assert_eq!(*events.borrow(), [("clear-keyboard".into(), "fire".into())]);
        move_to(right - 70.0, 341.0);
        assert_eq!(backgrounds(keyboard, 341)?, idle);
        assert_eq!(backgrounds(right, 341)?, hovered);

        // Tab from the recording field onto its clear button with the pointer elsewhere.
        click(&ui, right - 70.0, 341.0);
        move_to(1.0, 1.0);
        assert_eq!(backgrounds(right, 341)?, idle);
        key(&ui, Key::Tab);
        let focused = backgrounds(right, 341)?;
        assert_ne!(focused[0], idle[0]);
        assert_ne!(focused[1], idle[1]);
        assert_ne!(focused[0], focused[1]);
        let theme = ui.global::<crate::Theme>();
        theme.set_high_contrast(true);
        let focused_clear = backgrounds(right, 341)?[1];
        let accent = theme.get_orange();
        assert_eq!(
            [focused_clear.r, focused_clear.g, focused_clear.b],
            [accent.red(), accent.green(), accent.blue()]
        );
        theme.set_high_contrast(false);
        events.borrow_mut().clear();
        key(&ui, Key::Return);
        assert_eq!(
            *events.borrow(),
            [("clear-controller".into(), "fire".into())]
        );

        // Movement reveals only the hovered direction. An empty action only records.
        let forward = width - 725.0 + 168.0;
        move_to(forward - 70.0, 502.0);
        assert_ne!(backgrounds(forward, 502)?[1], idle[1]);
        assert_eq!(backgrounds(forward + 174.0, 502)?, idle);
        events.borrow_mut().clear();
        click(&ui, right - 24.0, 233.0);
        assert_eq!(
            *events.borrow(),
            [("capture-controller".into(), "global".into())]
        );
    }
    ui.hide()?;
    Ok(())
}

#[test]
fn saved_card_actions_hide_after_favorite_clicks_and_show_for_keyboard_focus() -> Result<()> {
    use slint::Model;

    let (ui, _window) = timed_test_window(Rc::new(Cell::default()))?;
    let store = ui.global::<Store>();
    store.set_connected(true);
    let rows = Rc::new(slint::VecModel::from(vec![PresetRow {
        id: "saved".into(),
        name: "Saved crosshair".into(),
        ..Default::default()
    }]));
    store.set_presets(rows.clone().into());
    store.set_preset_groups(
        Rc::new(slint::VecModel::from(vec![PresetGroup {
            items: rows.clone().into(),
        }]))
        .into(),
    );
    let saved = rows.clone();
    store.on_action(move |action, id| {
        if action == "favorite" && id == "saved" {
            let mut row = saved.row_data(0).unwrap();
            row.favorite = !row.favorite;
            saved.set_row_data(0, row);
        }
    });
    ui.show()?;

    let move_to = |x, y| {
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(x, y),
        })
    };
    let icons = || -> Result<[bool; 2]> {
        let frame = ui.window().take_snapshot()?;
        let stride = frame.width() as usize;
        // Check the star and trash glyphs, independently of the card's background.
        Ok([279, 471].map(|x| {
            (204..222).any(|y| {
                frame.as_slice()[y * stride + x - 9..y * stride + x + 9]
                    .iter()
                    .any(|p| p.r > 110)
            })
        }))
    };
    for favorite in [true, false] {
        move_to(279.0, 213.0);
        assert_eq!(icons()?, [true, true]);
        click(&ui, 279.0, 213.0);
        assert_eq!(rows.row_data(0).unwrap().favorite, favorite);
        move_to(700.0, 450.0);
        assert_eq!(icons()?, [false, false]);
    }
    key(&ui, Key::Tab);
    assert_eq!(icons()?, [true, true]);
    click(&ui, 350.0, 250.0);
    move_to(700.0, 450.0);
    assert_eq!(icons()?, [false, false]);
    ui.hide()?;
    Ok(())
}

#[test]
fn profile_picker_leaves_a_gap_between_selectable_rows() -> Result<()> {
    let (ui, window) = timed_test_window(Rc::new(Cell::default()))?;
    let store = ui.global::<Store>();
    store.set_connected(true);
    store.set_profiles(
        Rc::new(slint::VecModel::from(vec![
            "Default".into(),
            "Practice".into(),
        ]))
        .into(),
    );
    let selected = Rc::new(Cell::new(None));
    let result = selected.clone();
    store.on_action(move |action, value| {
        if action == "select-profile" {
            result.set(value.parse::<usize>().ok());
        }
    });
    ui.show()?;
    click(&ui, 910.0, 24.0);
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(950.0, 133.0),
    });
    snapshot(&window, "profile-picker-spacing.png")?;
    let frame = ui.window().take_snapshot()?;
    let pixel = |y: usize| frame.as_slice()[y * frame.width() as usize + 950];
    assert_ne!(pixel(113), pixel(114));
    assert!((115..118).all(|y| pixel(y) == pixel(114)));
    assert_ne!(pixel(118), pixel(117));
    click(&ui, 950.0, 116.0);
    assert_eq!(selected.get(), None);
    click(&ui, 950.0, 133.0);
    assert_eq!(selected.get(), Some(1));

    // Keyboard navigation scrolls the last eight profiles into view.
    let profiles = (1..=10)
        .map(|n| format!("Profile {n}").into())
        .collect::<Vec<_>>();
    store.set_profiles(Rc::new(slint::VecModel::from(profiles)).into());
    click(&ui, 910.0, 24.0);
    key(&ui, Key::End);
    key(&ui, Key::UpArrow);
    click(&ui, 950.0, 336.0);
    assert_eq!(selected.get(), Some(9));
    ui.hide()?;
    Ok(())
}

#[test]
fn library_tabs_sort_picker_and_browse_actions_are_independent() -> Result<()> {
    let clock = Rc::new(Cell::default());
    let (ui, window) = timed_test_window(clock.clone())?;
    let file_pointer = Rc::new(Cell::new((-1.0, -1.0)));
    let pointer = file_pointer.clone();
    ui.on_file_pointer(move || pointer.get());
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
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(1210.0, 207.0),
    });
    snapshot(&window, "sort-picker-spacing.png")?;
    let frame = ui.window().take_snapshot()?;
    let pixel = |y: usize| frame.as_slice()[y * frame.width() as usize + 1210];
    assert_ne!(pixel(188), pixel(189));
    assert!((190..193).all(|y| pixel(y) == pixel(189)));
    assert_ne!(pixel(193), pixel(192));
    click(&ui, 1210.0, 190.0);
    assert!(
        events.borrow().is_empty(),
        "The row gap must not select a sort order"
    );
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
            revision: app.state.revision + 1,
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
    click(&ui, 1135.0, 72.0);
    assert!(events.borrow().is_empty(), "Import first opens the popover");
    snapshot(&window, "library-import.png")?;
    key(&ui, Key::Escape);
    key(&ui, Key::Return);
    key(&ui, Key::Return);
    assert_eq!(*events.borrow(), [("import".into(), "".into())]);
    events.borrow_mut().clear();
    file_pointer.set((1100.0, 145.0));
    ui.invoke_file_dropped("closed.json".into(), true);
    assert!(events.borrow().is_empty());
    click(&ui, 1135.0, 72.0);
    let idle = ui.window().take_snapshot()?;
    let top = store.get_menu_y() as usize;
    let left = store.get_menu_x() as usize - 280;
    ui.set_dragging_import_file(true);
    clock.set(clock.get() + Duration::from_millis(50));
    slint::platform::update_timers_and_animations();
    let hover = ui.window().take_snapshot()?;
    assert_import_icon_hover(&idle, &hover, left, top);
    snapshot(&window, "library-import-hover.png")?;
    file_pointer.set((650.0, 450.0));
    clock.set(clock.get() + Duration::from_millis(50));
    slint::platform::update_timers_and_animations();
    assert_eq!(ui.window().take_snapshot()?.as_slice(), idle.as_slice());
    ui.invoke_file_dropped("outside.json".into(), true);
    assert!(events.borrow().is_empty());
    file_pointer.set((1100.0, 145.0));
    ui.set_dragging_import_file(false);
    ui.invoke_file_dropped("sample.crosshair".into(), true);
    assert_eq!(
        *events.borrow(),
        [("import".into(), "sample.crosshair".into())]
    );
    events.borrow_mut().clear();
    click(&ui, 1135.0, 72.0);
    click(&ui, 700.0, 450.0);
    assert!(events.borrow().is_empty());
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
    let raster = renderer.render(&crate::features::browse::presets()[0], 1.0, &|_| {
        anyhow::bail!("Unexpected image")
    })?;
    store.set_active_thumbnail(preview::image(&raster.thumbnail(24, 1.0)?));
    store.on_scale_thumbnail(preview::scale_thumbnail);
    let mut app = super::gesture_tests::app(&ui);
    let profile = &mut app.state.profiles[0];
    profile.actions.controller.fire = Some(GamepadControl::RightTrigger);
    profile.actions.controller.reload = Some(GamepadControl::West);
    profile.actions.controller.movement = [
        GamepadControl::LeftStickUp,
        GamepadControl::LeftStickLeft,
        GamepadControl::LeftStickDown,
        GamepadControl::LeftStickRight,
    ]
    .map(Some);
    let mut shortcut = Binding::new(
        InputChord::key(0x24),
        BindingAction::SelectPreset {
            preset: app.state.presets[0].id,
        },
    );
    shortcut.name = "Shotgun".into();
    shortcut.controller = Some(GamepadControl::DPadUp);
    let shortcut_id = shortcut.id.to_string();
    profile.bindings.push(shortcut);
    app.refresh_bindings();
    store.set_profiles(Rc::new(slint::VecModel::from(vec!["Default".into()])).into());
    store.set_visible(true);
    for (width, image) in [(1280.0, "shortcuts.png"), (960.0, "shortcuts-narrow.png")] {
        ui.window().set_size(slint::LogicalSize::new(width, 800.0));
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(1.0, 1.0),
        });
        snapshot(&window, image)?;
        let right = width - 24.0;
        let column_width = (width - 106.0) * 0.26;
        let keyboard_edge = right - column_width;
        let controller_edge = right - 44.0;
        let move_to = |x, y| {
            ui.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(x, y),
            });
        };
        let fill_at = |x: f32, y: usize| -> Result<_> {
            let frame = ui.window().take_snapshot()?;
            Ok(frame.as_slice()[y * frame.width() as usize + x as usize])
        };
        let idle = fill_at(controller_edge - 24.0, 777)?;
        move_to(keyboard_edge - 70.0, 341.0);
        let hover = fill_at(keyboard_edge - 60.0, 351)?;
        assert_ne!(hover, idle);
        if width == 1280.0 {
            snapshot(&window, "shortcuts-hover.png")?;
        }
        move_to(controller_edge - 70.0, 767.0);
        assert_eq!(fill_at(controller_edge - 60.0, 777)?, hover);
        assert_eq!(fill_at(controller_edge - 24.0, 777)?, hover);
        assert_eq!(fill_at(right - 22.0, 777)?, idle);
        if width == 1280.0 {
            snapshot(&window, "shortcuts-crosshair-hover.png")?;
        }
        move_to(right - 22.0, 767.0);
        assert_eq!(fill_at(right - 22.0, 777)?, hover);
        assert_eq!(fill_at(controller_edge - 24.0, 777)?, idle);
        if width == 1280.0 {
            snapshot(&window, "shortcuts-delete-hover.png")?;
        }
        events.borrow_mut().clear();
        for x in [
            keyboard_edge - 70.0,
            keyboard_edge - 24.0,
            controller_edge - 70.0,
            controller_edge - 24.0,
            right - 22.0,
        ] {
            move_to(x, 767.0);
            click(&ui, x, 767.0);
        }
        assert_eq!(
            *events.borrow(),
            [
                ("capture".into(), shortcut_id.clone()),
                ("clear-keyboard".into(), shortcut_id.clone()),
                ("capture-controller".into(), shortcut_id.clone()),
                ("clear-controller".into(), shortcut_id.clone()),
                ("binding-remove".into(), shortcut_id.clone()),
            ]
        );
        for (y, record, clear) in [
            (502.0, "capture", "clear-keyboard"),
            (556.0, "capture-controller", "clear-controller"),
        ] {
            for (i, direction) in ["forward", "left", "backward", "right"]
                .into_iter()
                .enumerate()
            {
                let chip = width - 725.0 + i as f32 * 174.0;
                events.borrow_mut().clear();
                click(&ui, chip + 58.0, y);
                click(&ui, chip + 152.0, y);
                assert_eq!(
                    *events.borrow(),
                    [
                        (record.into(), direction.into()),
                        (clear.into(), direction.into())
                    ],
                    "Movement controls at width {width}, direction {direction}, row {y}"
                );
            }
        }
    }
    ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
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
