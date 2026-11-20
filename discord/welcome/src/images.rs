use std::io::Cursor;

use image::{
    codecs::png::{CompressionType, FilterType, PngEncoder},
    imageops::{self, FilterType as ResizeFilter},
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits,
    RgbaImage,
};
use tiny_skia::{FillRule, IntSize, Mask, PathBuilder, Pixmap, Transform};

use crate::{style, Error, Result, MAX_AVATAR_BYTES};

const MAX_SIDE: u32 = 4096;
const MAX_DECODER_ALLOC: u64 = 128 * 1024 * 1024;
const LOGO: &[u8] = include_bytes!("../assets/logo.png");
const DEFAULT_AVATAR: &[u8] = include_bytes!("../assets/default-avatar.png");

pub(crate) fn decode(bytes: &[u8]) -> Result<RgbaImage> {
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(Error::InputTooLarge {
            kind: "image",
            limit: MAX_AVATAR_BYTES,
        });
    }
    let format = image::guess_format(bytes)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(Error::UnsupportedAvatarFormat);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODER_ALLOC);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder()?;
    decoder.set_limits(limits)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(Error::InvalidImageDimensions);
    }
    let orientation = decoder.orientation()?;
    let mut decoded = DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    Ok(decoded.to_rgba8())
}

pub(crate) fn avatar_mask() -> Result<Mask> {
    let side = style::pixels(style::AVATAR_RADIUS * 2.0);
    let mut mask =
        Mask::new(side, side).ok_or(Error::Internal("could not allocate avatar mask"))?;
    let radius = side as f32 / 2.0;
    let circle = PathBuilder::from_circle(radius, radius, radius)
        .ok_or(Error::Internal("invalid avatar circle"))?;
    mask.fill_path(&circle, FillRule::Winding, true, Transform::identity());
    Ok(mask)
}

pub(crate) fn avatar(bytes: &[u8], mask: &Mask) -> Result<Pixmap> {
    let source = decode(bytes)?;
    let side = source.width().min(source.height());
    let square = imageops::crop_imm(
        &source,
        (source.width() - side) / 2,
        (source.height() - side) / 2,
        side,
        side,
    )
    .to_image();
    let mut result = resize_premultiplied(square, mask.width(), mask.height())?;
    result.apply_mask(mask);
    Ok(result)
}

pub(crate) fn default_avatar(mask: &Mask) -> Result<Pixmap> {
    avatar(DEFAULT_AVATAR, mask)
}

pub(crate) fn logo() -> Result<Pixmap> {
    let source = decode(LOGO)?;
    let (mut left, mut top, mut right, mut bottom) = (source.width(), source.height(), 0, 0);
    for (x, y, pixel) in source.enumerate_pixels() {
        if pixel[3] != 0 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    if right <= left || bottom <= top {
        return Err(Error::TransparentLogo);
    }
    let cropped = imageops::crop_imm(&source, left, top, right - left, bottom - top).to_image();
    let scale =
        style::pixels(style::LOGO_SIZE) as f32 / cropped.width().max(cropped.height()) as f32;
    let width = (cropped.width() as f32 * scale).round().max(1.0) as u32;
    let height = (cropped.height() as f32 * scale).round().max(1.0) as u32;
    resize_premultiplied(cropped, width, height)
}

/// Resize *premultiplied* pixels. Resizing straight-alpha images introduces
/// colored fringes from otherwise invisible pixels along the avatar and gear.
fn resize_premultiplied(mut image: RgbaImage, width: u32, height: u32) -> Result<Pixmap> {
    for pixel in image.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let mut resized = imageops::resize(&image, width, height, ResizeFilter::Lanczos3);
    clamp_premultiplied(&mut resized);
    let size = IntSize::from_wh(width, height).ok_or(Error::Internal("invalid image size"))?;
    Pixmap::from_vec(resized.into_raw(), size).ok_or(Error::Internal("invalid pixmap buffer"))
}

/// Lanczos ringing may otherwise produce RGB > alpha, which is not a valid
/// premultiplied color. Keep that invariant before passing pixels to tiny-skia.
fn clamp_premultiplied(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = pixel[3];
        for channel in &mut pixel.0[..3] {
            *channel = (*channel).min(alpha);
        }
    }
}

pub(crate) fn finish(canvas: Pixmap) -> Result<RgbaImage> {
    let (width, height) = (canvas.width(), canvas.height());
    let high_res = RgbaImage::from_raw(width, height, canvas.take())
        .ok_or(Error::Internal("invalid canvas buffer"))?;
    let mut output = imageops::resize(
        &high_res,
        style::WIDTH,
        style::HEIGHT,
        ResizeFilter::Lanczos3,
    );
    clamp_premultiplied(&mut output);
    // PNG stores straight-alpha pixels, so unpremultiply only after resizing.
    for pixel in output.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = if alpha == 0 {
                0
            } else {
                ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8
            };
        }
    }
    Ok(output)
}

pub(crate) fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Default, FilterType::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn bundled_assets_are_decodable_and_logo_is_square() {
        let mark = logo().unwrap();
        assert_eq!((mark.width(), mark.height()), (930, 930));
        let mask = avatar_mask().unwrap();
        let avatar = default_avatar(&mask).unwrap();
        assert_eq!(avatar.pixel(0, 0).unwrap().alpha(), 0);
    }

    #[test]
    fn oversized_input_is_rejected_before_decoding() {
        let input = vec![0; MAX_AVATAR_BYTES + 1];
        assert!(matches!(decode(&input), Err(Error::InputTooLarge { .. })));
    }

    #[test]
    fn invalid_images_are_errors_not_panics() {
        assert!(decode(b"not an image").is_err());
    }

    #[test]
    fn transparent_rgb_cannot_bleed_into_resized_edges() {
        let mut input = RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 0]));
        for y in 2..6 {
            for x in 2..6 {
                input.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }
        let pixmap = resize_premultiplied(input, 32, 32).unwrap();
        for pixel in pixmap.pixels() {
            assert_eq!(pixel.red(), 0);
            assert!(pixel.blue() <= pixel.alpha());
        }
    }

    #[test]
    fn exif_orientation_is_applied_before_cropping() {
        let bytes = include_bytes!("../tests/fixtures/orientation-6.jpg");
        let decoded = decode(bytes).unwrap();
        assert_eq!(decoded.dimensions(), (20, 40));
        assert!(decoded.get_pixel(10, 5)[0] > 200);
        assert!(decoded.get_pixel(10, 35)[2] > 200);
    }

    #[test]
    fn unsupported_formats_are_rejected_without_decoding() {
        assert!(matches!(
            decode(b"GIF89a123456789"),
            Err(Error::UnsupportedAvatarFormat)
        ));
    }
}
