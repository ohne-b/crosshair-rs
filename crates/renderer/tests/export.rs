use crosshair_core::*;
use crosshair_renderer::{AssetData, AssetSource, Renderer};
use std::sync::Arc;

fn no_assets(_: Id) -> anyhow::Result<AssetData> {
    anyhow::bail!("Unexpected asset read")
}

fn compare(preset: &Preset, source: &impl AssetSource, tolerance: u8) -> String {
    let mut renderer = Renderer::new().unwrap();
    let reference = renderer.render(preset, 1.0, source).unwrap();
    let svg = renderer.to_svg(preset, 1.0, source).unwrap();
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).unwrap();
    let mut pixels = tiny_skia::Pixmap::new(reference.width, reference.height).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    let worst = reference
        .pixels
        .iter()
        .zip(pixels.data())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(
        worst <= tolerance,
        "Pixel delta {worst} exceeds {tolerance}"
    );
    String::from_utf8(svg).unwrap()
}

#[test]
fn svg_keeps_shapes_and_text_as_paths_and_preserves_alpha_and_transforms() {
    let mut preset = Preset::new("<&\" ignored metadata");
    preset.layers = vec![Layer::new(
        "Shape",
        Content::Rectangle(Rectangle {
            width: 18.0,
            height: 10.0,
            color: Color::rgba(33, 99, 170, 128),
            ..Default::default()
        }),
    )];
    preset.layers[0].opacity = 0.5;
    preset.layers[0].transform.rotation = 90.0;
    preset.layers[0].transform.x = 123.0;
    let svg = compare(&preset, &no_assets, 1);
    assert!(svg.contains("<path"));
    assert!(!svg.contains("<image"));
    preset.layers[0].content = Content::Text(Text {
        content: "<&>".into(),
        ..Default::default()
    });
    let svg = compare(&preset, &no_assets, 2);
    assert!(!svg.contains("<text"));
    assert!(svg.contains("<path"));
}

#[test]
fn svg_blur_remains_a_filter_with_bounded_approximation_error() {
    let mut preset = Preset::new("Blur");
    preset.layers[0].effects.blur = 2.0;
    let svg = compare(&preset, &no_assets, 18);
    assert!(svg.contains("feGaussianBlur"));
    assert!(!svg.contains("<image"));
}

#[test]
fn svg_embeds_cropped_images_without_external_resources() {
    let mut pixmap = tiny_skia::Pixmap::new(4, 2).unwrap();
    for (index, pixel) in pixmap.data_mut().chunks_exact_mut(4).enumerate() {
        pixel.copy_from_slice(if index % 4 < 2 {
            &[255, 0, 0, 255]
        } else {
            &[0, 255, 0, 255]
        });
    }
    let data = AssetData {
        kind: AssetKind::Png,
        bytes: Arc::from(pixmap.encode_png().unwrap()),
    };
    let mut image = Image::new(Id::new_v4(), 8.0, 8.0);
    image.crop = Crop {
        x: 0.5,
        y: 0.0,
        width: 0.5,
        height: 1.0,
    };
    let mut preset = Preset::new("Image");
    preset.layers = vec![Layer::new("Image", Content::Image(image))];
    let svg = compare(&preset, &|_| Ok(data.clone()), 3);
    assert!(svg.contains("data:image/png;base64,"));
    assert!(svg.contains("clip-path"));
}

#[test]
fn measurement_checks_effects_and_union_before_reading_assets() {
    let mut renderer = Renderer::new().unwrap();
    let mut preset = Preset::new("Bounds");
    preset.layers = vec![Layer::new(
        "Rectangle",
        Content::Rectangle(Rectangle {
            width: 1024.0,
            height: 2.0,
            ..Default::default()
        }),
    )];
    assert_eq!(renderer.measure(&preset, 1.0).unwrap().width(), 1024.0);
    preset.layers[0].transform.x = 1000.0;
    assert_eq!(renderer.measure(&preset, 1.0).unwrap().width(), 1024.0);
    preset.layers[0].effects.blur = 1.0;
    assert!(renderer.measure(&preset, 1.0).is_err());
    assert!(renderer.to_svg(&preset, 1.0, &no_assets).is_err());
    preset.layers[0].effects.blur = 0.0;
    let mut distant = preset.layers[0].clone();
    distant.id = Id::new_v4();
    distant.transform.x = -1000.0;
    preset.layers.push(distant);
    assert!(renderer.measure(&preset, 1.0).is_err());
    assert!(renderer.render(&preset, 1.0, &no_assets).is_err());
}

#[test]
fn svg_preserves_round_outline_and_layer_local_eraser() {
    let mut preset = Preset::new("Outline");
    preset.layers = vec![Layer::new(
        "Rectangle",
        Content::Rectangle(Rectangle {
            width: 12.0,
            height: 8.0,
            color: Color::rgba(200, 50, 0, 80),
            ..Default::default()
        }),
    )];
    preset.layers[0].effects.outline.enabled = true;
    preset.layers[0].effects.outline.thickness = 2.0;
    preset.layers[0].effects.outline.blur = 1.0;
    preset.layers[0].opacity = 0.5;
    let svg = compare(&preset, &no_assets, 1);
    assert!(svg.contains("<path"));
    assert!(svg.contains("data:image/png;base64,"));
    preset.layers[0].effects.outline.enabled = false;
    preset.layers[0].content = Content::Freehand(Freehand {
        strokes: vec![
            Stroke {
                points: vec![Point { x: -4.0, y: 0.0 }, Point { x: 4.0, y: 0.0 }],
                width: 6.0,
                color: Color::ORANGE,
                erase: false,
            },
            Stroke {
                points: vec![Point { x: 0.0, y: 0.0 }],
                width: 2.0,
                color: Color::WHITE,
                erase: true,
            },
        ],
    });
    let svg = compare(&preset, &no_assets, 2);
    assert!(svg.contains("<mask"));
    assert!(!svg.contains("<image"));
}
