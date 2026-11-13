use std::{cell::Cell, io::Cursor, sync::Arc};

use crosshair_core::*;
use crosshair_renderer::{AssetData, Raster, Renderer, inspect_asset};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn no_assets(_: Id) -> anyhow::Result<AssetData> {
    anyhow::bail!("Unexpected asset request")
}

#[test]
fn thumbnails_preserve_aspect_and_premultiplied_color() -> anyhow::Result<()> {
    let raster = Raster {
        width: 48,
        height: 24,
        origin: [-24, -12],
        pixels: [80, 40, 20, 128].repeat(48 * 24),
    };
    let small = raster.thumbnail(24, 1.0)?;
    assert_eq!((small.width, small.height, small.origin), (24, 12, [0, 0]));
    assert!(
        small
            .pixels
            .chunks_exact(4)
            .all(|pixel| pixel == [80, 40, 20, 128])
    );
    assert_eq!(small.thumbnail(80, 1.0)?.pixels, small.pixels);
    assert_eq!(small.thumbnail(24, 1.25)?.width, 30);
    assert!(raster.thumbnail(0, 1.0).is_err());
    assert!(raster.thumbnail(24, f32::NAN).is_err());
    assert!(raster.thumbnail(24, 0.0).is_err());
    assert!(
        Raster {
            pixels: Vec::new(),
            ..raster
        }
        .thumbnail(24, 1.0)
        .is_err()
    );
    Ok(())
}

fn preset(content: Content) -> Preset {
    let mut preset = Preset::new("Test");
    preset.layers = vec![Layer::new("Layer", content)];
    preset
}

fn pixel(raster: &Raster, x: i32, y: i32) -> [u8; 4] {
    let x = x - raster.origin[0];
    let y = y - raster.origin[1];
    if x < 0 || y < 0 || x >= raster.width as i32 || y >= raster.height as i32 {
        return [0; 4];
    }
    let offset = (y as usize * raster.width as usize + x as usize) * 4;
    raster.pixels[offset..offset + 4].try_into().unwrap()
}

fn render(content: Content) -> Arc<Raster> {
    Renderer::new()
        .unwrap()
        .render(&preset(content), 1.0, &no_assets)
        .unwrap()
}

fn assert_premultiplied(raster: &Raster) {
    assert!(
        raster
            .pixels
            .chunks_exact(4)
            .all(|p| p[..3].iter().all(|c| *c <= p[3]))
    );
}

#[test]
fn default_crosshair_has_exact_pixel_geometry_and_no_center_fill() {
    let raster = render(Content::Crosshair(Crosshair::default()));
    assert_eq!(
        (raster.width, raster.height, raster.origin),
        (28, 28, [-14, -14])
    );
    assert_eq!(
        raster.pixels.chunks_exact(4).filter(|p| p[3] != 0).count(),
        80
    );
    assert_eq!(pixel(&raster, 4, 0), [249, 115, 22, 255]);
    assert_eq!(pixel(&raster, 0, 0), [0; 4]);
    assert_eq!(pixel(&raster, 13, -1)[3], 255);
    assert_eq!(pixel(&raster, 14, -1)[3], 0);
}

#[test]
fn scaled_whole_pixel_crosshairs_match_native_pixels() {
    let mut renderer = Renderer::new().unwrap();
    for thickness in [1.0, 2.0, 3.0] {
        let native = preset(Content::Crosshair(Crosshair {
            length: 5.0,
            gap: 1.0,
            thickness,
            ..Default::default()
        }));
        let expected = renderer.render(&native, 1.0, &no_assets).unwrap();
        for scale in [0.25, 0.5, 0.67, 0.7, 1.25, 1.5, 2.0] {
            for output_scale in [false, true] {
                let mut changed = native.clone();
                let Content::Crosshair(cross) = &mut changed.layers[0].content else {
                    unreachable!()
                };
                cross.length /= scale;
                cross.gap /= scale;
                cross.thickness /= scale;
                if !output_scale {
                    changed.layers[0].transform.scale = scale;
                }
                let actual = renderer
                    .render(&changed, if output_scale { scale } else { 1.0 }, &no_assets)
                    .unwrap();
                for y in -8..8 {
                    for x in -8..8 {
                        assert_eq!(
                            pixel(&actual, x, y),
                            pixel(&expected, x, y),
                            "thickness {thickness}, scale {scale}, output {output_scale}, pixel {x},{y}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn combined_rotations_and_mixed_arms_keep_whole_pixel_strokes_solid() {
    let mut renderer = Renderer::new().unwrap();
    for (arms, arm_rotation, layer_rotation) in [(4, 45.0, 45.0), (4, 30.0, 60.0), (8, 0.0, 0.0)] {
        let mut document = preset(Content::Crosshair(Crosshair {
            arms,
            enabled_arms: vec![true; arms as usize],
            length: 20.0,
            gap: 8.0,
            thickness: 2.0,
            rotation: arm_rotation,
            ..Default::default()
        }));
        document.layers[0].transform.scale = 0.5;
        document.layers[0].transform.rotation = layer_rotation;
        let raster = renderer.render(&document, 1.0, &no_assets).unwrap();
        assert_eq!(pixel(&raster, 8, 0)[3], 255);
        assert_eq!(pixel(&raster, 0, 8)[3], 255);
        if arms == 8 {
            assert!(
                raster
                    .pixels
                    .chunks_exact(4)
                    .any(|p| p[3] > 0 && p[3] < 255)
            );
        } else {
            assert!(
                raster
                    .pixels
                    .chunks_exact(4)
                    .all(|p| p[3] == 0 || p[3] == 255)
            );
        }
    }
}

#[test]
fn fractional_and_rotated_strokes_keep_smooth_visible_coverage() {
    let mut renderer = Renderer::new().unwrap();
    for (thickness, scale, offset, rotation) in [
        (0.5, 1.0, 0.0, 0.0),
        (2.0, 0.25, 0.0, 0.0),
        (1.0, 1.0, 0.25, 0.0),
        (1.0, 1.0, 0.0, 30.0),
    ] {
        let mut document = preset(Content::Crosshair(Crosshair {
            length: 8.0,
            gap: 0.0,
            thickness,
            ..Default::default()
        }));
        document.layers[0].transform.scale = scale;
        document.layers[0].transform.y = offset;
        document.layers[0].transform.rotation = rotation;
        let raster = renderer.render(&document, 1.0, &no_assets).unwrap();
        assert!(
            raster
                .pixels
                .chunks_exact(4)
                .any(|p| p[3] > 0 && p[3] < 255)
        );
        assert_premultiplied(&raster);
    }
}

#[test]
fn pixel_alignment_preserves_alpha_and_fractional_arm_ends() {
    let mut document = preset(Content::Crosshair(Crosshair {
        length: 9.0,
        gap: 0.0,
        thickness: 2.0,
        color: Color::rgba(255, 0, 0, 128),
        ..Default::default()
    }));
    document.layers[0].transform.scale = 0.5;
    let mut renderer = Renderer::new().unwrap();
    let raster = renderer.render(&document, 1.0, &no_assets).unwrap();
    assert_eq!(pixel(&raster, 2, 0), [128, 0, 0, 128]);
    assert_eq!(
        pixel(&raster, 0, 0)[3],
        128,
        "Overlapping arms stay one fill"
    );
    assert!(pixel(&raster, 4, 0)[3] > 0 && pixel(&raster, 4, 0)[3] < 128);
    document.layers[0].transform.rotation = 90.0;
    let rotated = renderer.render(&document, 1.0, &no_assets).unwrap();
    assert_eq!(raster.pixels, rotated.pixels);
    document.layers[0].opacity = 0.5;
    let faded = renderer.render(&document, 1.0, &no_assets).unwrap();
    assert_eq!(pixel(&faded, 2, 0)[3], 64);
}

#[test]
fn odd_thickness_is_crisp_and_quarter_turns_do_not_add_fringe_pixels() {
    let mut crosshair = Crosshair {
        thickness: 3.0,
        ..Default::default()
    };
    let first = render(Content::Crosshair(crosshair.clone()));
    assert!(
        first
            .pixels
            .chunks_exact(4)
            .all(|p| p[3] == 0 || p[3] == 255)
    );
    assert_eq!(
        first.pixels.chunks_exact(4).filter(|p| p[3] != 0).count(),
        120
    );
    crosshair.rotation = 90.0;
    let rotated = render(Content::Crosshair(crosshair));
    assert_eq!(first.pixels, rotated.pixels);
}

#[test]
fn semitransparent_t_arms_have_no_overlap_seams() {
    let raster = render(Content::Crosshair(Crosshair {
        shape: ArmShape::TShape {
            width: 8.0,
            thickness: 2.0,
        },
        color: Color::rgba(255, 0, 0, 128),
        ..Default::default()
    }));
    assert!(
        raster
            .pixels
            .chunks_exact(4)
            .all(|p| p[3] == 0 || p[3] == 128)
    );
    assert_premultiplied(&raster);
}

#[test]
fn every_vector_primitive_and_dot_renders_real_geometry() {
    let shapes = [
        ArmShape::Rectangle,
        ArmShape::Triangle,
        ArmShape::Chevron {
            inward: false,
            depth: 3.0,
        },
        ArmShape::Chevron {
            inward: true,
            depth: 3.0,
        },
        ArmShape::Arc {
            radius: 10.0,
            sweep: 90.0,
        },
        ArmShape::TShape {
            width: 8.0,
            thickness: 2.0,
        },
    ];
    let mut signatures = Vec::new();
    for shape in shapes {
        let raster = render(Content::Crosshair(Crosshair {
            shape,
            ..Default::default()
        }));
        assert!(raster.pixels.chunks_exact(4).any(|p| p[3] != 0));
        assert_premultiplied(&raster);
        signatures.push((raster.width, raster.height, raster.pixels.clone()));
    }
    for (i, signature) in signatures.iter().enumerate() {
        assert!(
            !signatures[..i].contains(signature),
            "Different shapes produced identical geometry"
        );
    }
    for shape in [DotShape::Circle, DotShape::Square] {
        let raster = render(Content::Crosshair(Crosshair {
            enabled_arms: vec![false; 4],
            dot: Dot {
                enabled: true,
                shape,
                diameter: 6.0,
                ..Default::default()
            },
            ..Default::default()
        }));
        assert_eq!(pixel(&raster, 0, 0)[3], 255);
    }
    for content in [
        Content::Rectangle(Rectangle {
            radius: 3.0,
            ..Default::default()
        }),
        Content::Ellipse(Shape::default()),
        Content::Triangle(Shape::default()),
    ] {
        let raster = render(content);
        assert_eq!(pixel(&raster, 0, 0)[3], 255);
        assert!(pixel(&raster, -10, -10)[3] < 64);
    }
}

#[test]
fn layer_compositing_applies_opacity_once_and_preserves_order() {
    let rect = |color| {
        Content::Rectangle(Rectangle {
            width: 4.0,
            height: 4.0,
            color,
            ..Default::default()
        })
    };
    let mut document = preset(rect(Color::rgba(0, 0, 255, 255)));
    let mut top = Layer::new("Top", rect(Color::rgba(255, 0, 0, 128)));
    top.opacity = 0.5;
    document.layers.push(top);
    let raster = Renderer::new()
        .unwrap()
        .render(&document, 1.0, &no_assets)
        .unwrap();
    assert_eq!(pixel(&raster, 0, 0), [64, 0, 191, 255]);
    assert_premultiplied(&raster);
}

#[test]
fn anchor_transform_and_physical_scale_keep_the_origin_contract() {
    let mut document = preset(Content::Rectangle(Rectangle {
        width: 4.0,
        height: 2.0,
        ..Default::default()
    }));
    document.layers[0].transform = Transform {
        x: 10.0,
        y: -8.0,
        rotation: 90.0,
        scale: 2.0,
        anchor: Anchor::TopLeft,
    };
    let mut renderer = Renderer::new().unwrap();
    let raster = renderer.render(&document, 1.5, &no_assets).unwrap();
    // Pivot (-2,-1), then scale/rotate around it, then translate, then physical scale.
    assert_eq!(raster.origin, [6, -14]);
    assert_eq!((raster.width, raster.height), (6, 13));
    let bounds = renderer.layer_bounds(&document.layers[0], 1.5).unwrap();
    assert_eq!(
        (bounds.left, bounds.top, bounds.right, bounds.bottom),
        (6.0, -13.5, 12.0, -1.5)
    );
}

#[test]
fn changing_any_anchor_preserves_pixels_and_inverse_points() {
    let mut renderer = Renderer::new().unwrap();
    let mut document = preset(Content::Rectangle(Rectangle {
        width: 8.0,
        height: 4.0,
        ..Default::default()
    }));
    document.layers[0].transform = Transform {
        x: 10.0,
        y: -8.0,
        rotation: 90.0,
        scale: 2.0,
        ..Default::default()
    };
    let original = renderer.render(&document, 1.0, &no_assets).unwrap();
    for anchor in [
        Anchor::TopLeft,
        Anchor::Top,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Center,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::Bottom,
        Anchor::BottomRight,
    ] {
        renderer
            .set_anchor(&mut document.layers[0], anchor)
            .unwrap();
        let changed = renderer.render(&document, 1.0, &no_assets).unwrap();
        assert_eq!(changed.origin, original.origin);
        assert_eq!(changed.pixels, original.pixels);
        let point = renderer
            .local_point(&document.layers[0], Point { x: 8.0, y: -4.0 })
            .unwrap();
        assert_eq!(point, Point { x: 2.0, y: 1.0 });
        let layer = &document.layers[0];
        let pivot = renderer.anchor_point(layer).unwrap();
        let world_pivot = Point {
            x: layer.transform.x + pivot.x,
            y: layer.transform.y + pivot.y,
        };
        let local_pivot = renderer.local_point(layer, world_pivot).unwrap();
        assert!(
            (local_pivot.x - pivot.x).abs() < 0.0001 && (local_pivot.y - pivot.y).abs() < 0.0001
        );
    }
}

#[test]
fn freehand_eraser_is_local_to_its_layer_and_supports_single_point_strokes() {
    let mut document = preset(Content::Rectangle(Rectangle {
        width: 12.0,
        height: 12.0,
        color: Color::rgba(0, 0, 255, 255),
        ..Default::default()
    }));
    document.layers.push(Layer::new(
        "Drawing",
        Content::Freehand(Freehand {
            strokes: vec![
                Stroke {
                    points: vec![Point { x: -4.0, y: 0.0 }, Point { x: 4.0, y: 0.0 }],
                    width: 6.0,
                    color: Color::rgba(255, 0, 0, 255),
                    erase: false,
                },
                Stroke {
                    points: vec![Point { x: 0.0, y: 0.0 }],
                    width: 4.0,
                    color: Color::WHITE,
                    erase: true,
                },
            ],
        }),
    ));
    let raster = Renderer::new()
        .unwrap()
        .render(&document, 1.0, &no_assets)
        .unwrap();
    assert_eq!(pixel(&raster, 0, 0), [0, 0, 255, 255]);
    assert_eq!(pixel(&raster, 4, 0), [255, 0, 0, 255]);
}

#[test]
fn transparent_fill_can_have_a_visible_outline_and_blur_is_bounded() {
    let mut document = preset(Content::Rectangle(Rectangle {
        width: 4.0,
        height: 4.0,
        color: Color::rgba(255, 0, 0, 0),
        ..Default::default()
    }));
    document.layers[0].effects.outline = Outline {
        enabled: true,
        color: Color::rgba(0, 255, 0, 128),
        thickness: 2.0,
        blur: 1.0,
    };
    let raster = Renderer::new()
        .unwrap()
        .render(&document, 1.0, &no_assets)
        .unwrap();
    assert!(pixel(&raster, -3, 0)[3] > 0);
    assert!(pixel(&raster, -3, 0)[1] > 0);
    assert_premultiplied(&raster);
    assert!(raster.width < 32 && raster.height < 32);
    document.layers[0].effects.blur = 3.0;
    document.layers[0].effects.outline.enabled = false;
    if let Content::Rectangle(rect) = &mut document.layers[0].content {
        rect.color.a = 255;
    }
    let blurred = Renderer::new()
        .unwrap()
        .render(&document, 1.0, &no_assets)
        .unwrap();
    assert!(pixel(&blurred, -4, 0)[3] > 0);
    assert!(pixel(&blurred, 0, 0)[3] < 255);
}

fn encoded_image(format: ImageFormat) -> AssetData {
    let mut image = RgbaImage::new(4, 2);
    for (x, _, pixel) in image.enumerate_pixels_mut() {
        *pixel = if x < 2 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 255, 0, 255])
        };
    }
    let image = DynamicImage::ImageRgba8(image);
    let image = if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(image.into_rgb8())
    } else {
        image
    };
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    AssetData {
        kind: match format {
            ImageFormat::Png => AssetKind::Png,
            ImageFormat::Jpeg => AssetKind::Jpeg,
            ImageFormat::Gif => AssetKind::Gif,
            _ => unreachable!(),
        },
        bytes: Arc::from(bytes.into_inner()),
    }
}

#[test]
fn png_jpeg_gif_and_svg_assets_render_and_crop_from_immutable_sources() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Gif] {
        let data = encoded_image(format);
        assert_eq!(
            (
                inspect_asset(&data).unwrap().width,
                inspect_asset(&data).unwrap().height
            ),
            (4, 2)
        );
        let id = Id::new_v4();
        let mut image = Image::new(id, 8.0, 4.0);
        image.crop = Crop {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        let raster = Renderer::new()
            .unwrap()
            .render(&preset(Content::Image(image)), 1.0, &|requested| {
                assert_eq!(requested, id);
                Ok(data.clone())
            })
            .unwrap();
        let p = pixel(&raster, 2, 0);
        assert!(p[1] > 200 && p[0] < 40, "{format:?}: {p:?}");
        assert_premultiplied(&raster);
    }
    let data = AssetData { kind: AssetKind::Svg, bytes: Arc::from(b"<svg xmlns='http://www.w3.org/2000/svg' width='20' height='10'><rect width='10' height='10' fill='red'/><rect x='10' width='10' height='10' fill='#00ff00'/></svg>".as_slice()) };
    let mut image = Image::new(Id::new_v4(), 10.0, 10.0);
    image.crop = Crop {
        x: 0.5,
        y: 0.0,
        width: 0.5,
        height: 1.0,
    };
    let raster = Renderer::new()
        .unwrap()
        .render(&preset(Content::Image(image)), 1.0, &|_| Ok(data.clone()))
        .unwrap();
    assert_eq!(pixel(&raster, 0, 0), [0, 255, 0, 255]);
}

#[test]
fn gif_uses_only_the_first_frame() {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
            encoder
                .encode_frame(image::Frame::new(RgbaImage::from_pixel(4, 4, Rgba(color))))
                .unwrap();
        }
    }
    let data = AssetData {
        kind: AssetKind::Gif,
        bytes: Arc::from(bytes),
    };
    let raster = Renderer::new()
        .unwrap()
        .render(
            &preset(Content::Image(Image::new(Id::new_v4(), 4.0, 4.0))),
            1.0,
            &|_| Ok(data.clone()),
        )
        .unwrap();
    assert_eq!(pixel(&raster, 0, 0), [255, 0, 0, 255]);
}

#[test]
fn decoded_cache_keeps_multiple_immutable_images_across_edits() {
    let data = encoded_image(ImageFormat::Png);
    let calls = Cell::new(0);
    let source = |_| {
        calls.set(calls.get() + 1);
        Ok(data.clone())
    };
    let mut document = preset(Content::Image(Image::new(Id::new_v4(), 8.0, 4.0)));
    document.layers.push(Layer::new(
        "Second",
        Content::Image(Image::new(Id::new_v4(), 8.0, 4.0)),
    ));
    let mut renderer = Renderer::new().unwrap();
    renderer.render(&document, 1.0, &source).unwrap();
    document.layers[1].transform.x = 10.0;
    renderer.render(&document, 1.0, &source).unwrap();
    assert_eq!(calls.get(), 2);
}

#[test]
fn image_arms_repeat_cropped_source_and_preserve_the_gap() {
    let data = encoded_image(ImageFormat::Png);
    let image = Image::new(Id::new_v4(), 8.0, 4.0);
    let document = preset(Content::Crosshair(Crosshair {
        shape: ArmShape::Image(image),
        ..Default::default()
    }));
    let raster = Renderer::new()
        .unwrap()
        .render(&document, 1.0, &|_| Ok(data.clone()))
        .unwrap();
    assert_eq!(pixel(&raster, 0, 0), [0; 4]);
    for (x, y) in [(6, 0), (0, 6), (-6, 0), (0, -6)] {
        assert!(pixel(&raster, x, y)[3] > 0);
    }
}

#[test]
#[cfg(windows)]
fn text_arms_repeat_real_shaped_glyphs() {
    let raster = render(Content::Crosshair(Crosshair {
        gap: 12.0,
        shape: ArmShape::Text(Text {
            content: "O".into(),
            ..Default::default()
        }),
        ..Default::default()
    }));
    assert_eq!(pixel(&raster, 0, 0)[3], 0);
    assert!(raster.pixels.chunks_exact(4).filter(|p| p[3] > 0).count() > 100);
    assert_premultiplied(&raster);
}

#[test]
fn png_export_demultiplies_and_native_conversion_only_swaps_channels() {
    let raster = render(Content::Rectangle(Rectangle {
        width: 2.0,
        height: 2.0,
        color: Color::rgba(200, 100, 50, 128),
        ..Default::default()
    }));
    assert_eq!(pixel(&raster, 0, 0), [100, 50, 25, 128]);
    assert_eq!(&raster.to_bgra()[..4], &[25, 50, 100, 128]);
    let exported = image::load_from_memory_with_format(&raster.to_png().unwrap(), ImageFormat::Png)
        .unwrap()
        .into_rgba8();
    assert_eq!(exported.get_pixel(0, 0).0, [199, 100, 50, 128]);
}

#[test]
fn cached_render_is_reused_but_edits_and_scale_invalidate_it() {
    let mut renderer = Renderer::new().unwrap();
    let mut document = Preset::new("Cache");
    let first = renderer.render(&document, 1.0, &no_assets).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &renderer.render(&document, 1.0, &no_assets).unwrap()
    ));
    document.layers[0].transform.x = 10.0;
    let changed = renderer.render(&document, 1.0, &no_assets).unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    assert_eq!(changed.origin[0], first.origin[0] + 10);
    let scaled = renderer.render(&document, 2.0, &no_assets).unwrap();
    assert_eq!(scaled.width, changed.width * 2);
    renderer.clear_cache();
    assert!(!Arc::ptr_eq(
        &changed,
        &renderer.render(&document, 1.0, &no_assets).unwrap()
    ));
}

#[test]
fn invalid_dimensions_and_cancelled_jobs_fail_before_loading_assets() {
    let mut renderer = Renderer::new().unwrap();
    let calls = Cell::new(0);
    let source = |_| {
        calls.set(calls.get() + 1);
        anyhow::bail!("must not load")
    };
    let document = preset(Content::Image(Image::new(Id::new_v4(), 1024.0, 1024.0)));
    assert!(renderer.render(&document, 2.0, &source).is_err());
    assert!(renderer.render(&document, f32::NAN, &source).is_err());
    assert!(
        renderer
            .render_cancellable(&document, 1.0, &source, || true)
            .is_err()
    );
    assert_eq!(calls.get(), 0);
    let mut document = Preset::new("Hidden");
    document.layers[0].visible = false;
    let raster = renderer.render(&document, 1.0, &source).unwrap();
    assert_eq!(
        (
            raster.width,
            raster.height,
            raster.origin,
            raster.pixels.as_slice()
        ),
        (1, 1, [0, 0], [0; 4].as_slice())
    );
}

#[test]
#[cfg(windows)]
fn directwrite_shapes_bundled_and_system_fonts_including_circular_text() {
    let mut renderer = Renderer::new().unwrap();
    let fonts = renderer.font_families().unwrap();
    assert!(fonts.iter().any(|name| name == "Manrope"));
    assert!(fonts.iter().any(|name| name == "Segoe UI"));
    let mut text = Text {
        content: "O".into(),
        size: 32.0,
        ..Default::default()
    };
    let ring = renderer
        .render(&preset(Content::Text(text.clone())), 1.0, &no_assets)
        .unwrap();
    assert!(ring.pixels.chunks_exact(4).any(|p| p[3] > 0));
    assert_eq!(pixel(&ring, 0, 0)[3], 0, "The O counter must remain empty");
    text.content = "AV office café العربية".into();
    text.font_family = "Segoe UI".into();
    text.size = 16.0;
    let straight = renderer
        .render(&preset(Content::Text(text.clone())), 1.0, &no_assets)
        .unwrap();
    assert!(straight.width > straight.height * 3);
    text.content = "test ".into();
    text.layout = TextLayout::Circle {
        radius: 32.0,
        start_angle: -90.0,
        repeat: true,
    };
    let circle = renderer
        .render(&preset(Content::Text(text.clone())), 1.0, &no_assets)
        .unwrap();
    assert!(circle.width > 60 && circle.height > 60);
    assert_eq!(pixel(&circle, 0, 0)[3], 0);
    text.font_family = "Not a real installed font 97f218".into();
    assert!(
        renderer
            .render(&preset(Content::Text(text)), 1.0, &no_assets)
            .unwrap_err()
            .to_string()
            .contains("not installed")
    );
}
