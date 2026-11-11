use std::{io::Cursor, sync::Arc};

use crosshair_core::{AssetKind, Content, Id, Image, Layer, Preset};
use crosshair_renderer::{AssetData, Renderer, inspect_asset};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn asset(content: &str) -> AssetData {
    AssetData {
        kind: AssetKind::Svg,
        bytes: Arc::from(
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64'>{content}</svg>"
            )
            .into_bytes(),
        ),
    }
}

fn render(data: AssetData) -> anyhow::Result<Arc<crosshair_renderer::Raster>> {
    let mut preset = Preset::new("SVG");
    preset.layers = vec![Layer::new(
        "SVG",
        Content::Image(Image::new(Id::new_v4(), 64.0, 64.0)),
    )];
    Renderer::new()?.render(&preset, 1.0, &|_| Ok(data.clone()))
}

#[test]
fn static_filters_gradients_masks_and_patterns_render() {
    for content in [
        "<defs><filter id='f' x='-50%' y='-50%' width='200%' height='200%'><feGaussianBlur stdDeviation='2'/></filter></defs><circle cx='32' cy='32' r='12' fill='red' filter='url(#f)'/>",
        "<defs><linearGradient id='g'><stop stop-color='red'/><stop offset='1' stop-color='blue'/></linearGradient></defs><rect width='64' height='64' fill='url(#g)'/>",
        "<defs><mask id='m'><circle cx='32' cy='32' r='16' fill='white'/></mask></defs><rect width='64' height='64' fill='red' mask='url(#m)'/>",
        "<defs><pattern id='p' width='8' height='8' patternUnits='userSpaceOnUse'><rect width='4' height='4' fill='red'/></pattern></defs><rect width='64' height='64' fill='url(#p)'/>",
    ] {
        let raster = render(asset(content)).unwrap();
        assert!(raster.pixels.chunks_exact(4).any(|p| p[3] > 0));
        assert!(
            raster
                .pixels
                .chunks_exact(4)
                .all(|p| p[..3].iter().all(|channel| *channel <= p[3]))
        );
    }
}

#[test]
fn svg_text_uses_bundled_font_and_reports_missing_fonts() {
    let raster = render(asset(
        "<text x='4' y='44' font-family='Manrope' font-size='40' fill='red'>X</text>",
    ))
    .unwrap();
    assert!(raster.pixels.chunks_exact(4).filter(|p| p[3] > 0).count() > 100);
    assert!(
        inspect_asset(&asset("<text font-family='MissingRequestedFont'>X</text>"))
            .unwrap_err()
            .to_string()
            .contains("not installed")
    );
}

#[test]
fn embedded_png_is_decoded_without_external_resource_access() {
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])))
        .write_to(&mut png, ImageFormat::Png)
        .unwrap();
    let bytes = png.into_inner();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for group in bytes.chunks(3) {
        let n = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        for index in 0..4 {
            encoded.push(if index > group.len() {
                '='
            } else {
                alphabet[((n >> (18 - index * 6)) & 63) as usize] as char
            });
        }
    }
    let raster = render(asset(&format!(
        "<image width='64' height='64' href='data:image/png;base64,{encoded}'/>"
    )))
    .unwrap();
    let center = (32 * raster.width as usize + 32) * 4;
    assert_eq!(&raster.pixels[center..center + 4], &[255, 0, 0, 255]);
}

#[test]
fn abusive_surfaces_and_recursive_references_fail_cleanly() {
    let huge_filter = asset(
        "<defs><filter id='f' x='-100000' y='-100000' width='200000' height='200000' filterUnits='userSpaceOnUse'><feGaussianBlur stdDeviation='2'/></filter></defs><rect width='32' height='32' filter='url(#f)'/>",
    );
    assert!(
        render(huge_filter)
            .unwrap_err()
            .to_string()
            .contains("surface")
    );
    let huge_pattern = asset(
        "<defs><pattern id='p' width='100000' height='100000' patternUnits='userSpaceOnUse'><rect width='2' height='2'/></pattern></defs><rect width='64' height='64' fill='url(#p)'/>",
    );
    assert!(
        render(huge_pattern)
            .unwrap_err()
            .to_string()
            .contains("surface")
    );
    assert!(
        inspect_asset(&asset("<g id='a'><use href='#a'/></g>"))
            .unwrap_err()
            .to_string()
            .contains("circular")
    );
    assert!(inspect_asset(&asset("<filter><feTurbulence numOctaves='100'/></filter>")).is_err());
    assert!(
        inspect_asset(&asset(
            "<style>@font-face {font-family: bad; src:url(file:///secret)}</style>"
        ))
        .is_err()
    );
}
