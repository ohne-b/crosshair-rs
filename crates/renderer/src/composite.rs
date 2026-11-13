use anyhow::{Context, Result, ensure};
use tiny_skia::{
    BlendMode, FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, PathSegment, Pixmap,
    PixmapPaint, Rect, Transform,
};

use crate::{
    AssetSource,
    assets::AssetCache,
    geometry::{Element, PreparedLayer},
};

pub(crate) fn blur_padding(sigma: f32) -> f32 {
    box_radii(sigma).into_iter().sum::<usize>() as f32
}

fn box_radii(sigma: f32) -> [usize; 3] {
    if sigma <= 0.0 {
        return [0; 3];
    }
    let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
    let lower = ((ideal.floor() as usize).max(1) | 1).min(2047);
    let lower = if lower as f32 > ideal {
        lower.saturating_sub(2).max(1)
    } else {
        lower
    };
    let upper = lower + 2;
    let count = ((12.0 * sigma * sigma - 3.0 * (lower * lower) as f32 - 12.0 * lower as f32 - 9.0)
        / (-4.0 * lower as f32 - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    std::array::from_fn(|index| {
        if index < count {
            (lower - 1) / 2
        } else {
            (upper - 1) / 2
        }
    })
}

pub(crate) fn render_layer(
    layer: &PreparedLayer,
    origin: [i32; 2],
    size: [u32; 2],
    cache: &mut AssetCache,
    source: &impl AssetSource,
    cancelled: &impl Fn() -> bool,
) -> Result<Pixmap> {
    render(layer, origin, size, cache, source, cancelled, false)
}

pub(crate) fn render_outline(
    layer: &PreparedLayer,
    origin: [i32; 2],
    size: [u32; 2],
    cache: &mut AssetCache,
    source: &impl AssetSource,
) -> Result<Pixmap> {
    render(layer, origin, size, cache, source, &|| false, true)
}

fn render(
    layer: &PreparedLayer,
    origin: [i32; 2],
    size: [u32; 2],
    cache: &mut AssetCache,
    source: &impl AssetSource,
    cancelled: &impl Fn() -> bool,
    outline_only: bool,
) -> Result<Pixmap> {
    let mut fill = Pixmap::new(size[0], size[1]).context("Could not allocate layer")?;
    let mut geometry = Pixmap::new(size[0], size[1]).context("Could not allocate outline mask")?;
    let translation = Transform::from_translate(-origin[0] as f32, -origin[1] as f32);
    for group in &layer.groups {
        ensure!(!cancelled(), "Rendering cancelled");
        let mut group_fill =
            Pixmap::new(size[0], size[1]).context("Could not allocate layer group")?;
        for element in &group.elements {
            ensure!(!cancelled(), "Rendering cancelled");
            match element {
                Element::Path {
                    path,
                    color,
                    erase,
                    antialias,
                } => {
                    let aligned = (!*antialias).then(|| align_rectangles(path)).flatten();
                    let path = aligned.as_ref().unwrap_or(path);
                    let mut paint = Paint {
                        anti_alias: true,
                        blend_mode: if *erase {
                            BlendMode::DestinationOut
                        } else {
                            BlendMode::SourceOver
                        },
                        ..Default::default()
                    };
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    group_fill.fill_path(path, &paint, FillRule::Winding, translation, None);
                    paint.set_color_rgba8(255, 255, 255, if *erase { color.a } else { 255 });
                    geometry.fill_path(path, &paint, FillRule::Winding, translation, None);
                }
                Element::Image { image, transform } => {
                    let scale = (transform.sx * transform.sx + transform.ky * transform.ky).sqrt();
                    let requested = [
                        (image.width * scale).ceil().clamp(1.0, 1024.0) as u32,
                        (image.height * scale).ceil().clamp(1.0, 1024.0) as u32,
                    ];
                    let (pixels, crop) = cache.image(image.asset, requested, image.crop, source)?;
                    let sx = image.width / (pixels.width() as f32 * crop.width);
                    let sy = image.height / (pixels.height() as f32 * crop.height);
                    let matrix = Transform::from_row(
                        sx,
                        0.0,
                        0.0,
                        sy,
                        -image.width / 2.0 - crop.x * pixels.width() as f32 * sx,
                        -image.height / 2.0 - crop.y * pixels.height() as f32 * sy,
                    )
                    .post_concat(*transform)
                    .post_concat(translation);
                    let mut mask =
                        Mask::new(size[0], size[1]).context("Could not allocate crop mask")?;
                    let rect = Rect::from_xywh(
                        -image.width / 2.0,
                        -image.height / 2.0,
                        image.width,
                        image.height,
                    )
                    .context("Invalid crop dimensions")?;
                    mask.fill_path(
                        &PathBuilder::from_rect(rect),
                        FillRule::Winding,
                        true,
                        transform.post_concat(translation),
                    );
                    let paint = PixmapPaint {
                        quality: FilterQuality::Bilinear,
                        ..Default::default()
                    };
                    group_fill.draw_pixmap(
                        0,
                        0,
                        pixels.as_ref().as_ref(),
                        &paint,
                        matrix,
                        Some(&mask),
                    );
                    geometry.draw_pixmap(
                        0,
                        0,
                        pixels.as_ref().as_ref(),
                        &paint,
                        matrix,
                        Some(&mask),
                    );
                }
            }
        }
        blur(group_fill.data_mut(), size, group.blur, cancelled)?;
        fill.draw_pixmap(
            0,
            0,
            group_fill.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    blur(fill.data_mut(), size, layer.effects.blur, cancelled)?;
    let outline = &layer.effects.outline;
    let mut result = if outline.enabled && outline.thickness > 0.0 && outline.color.a > 0 {
        let alpha: Vec<u8> = geometry.data().chunks_exact(4).map(|p| p[3]).collect();
        drop(geometry);
        let coverage = outline_mask(&alpha, size, outline.thickness, cancelled)?;
        let mut result = Pixmap::new(size[0], size[1]).context("Could not allocate outline")?;
        for (pixel, coverage) in result.data_mut().chunks_exact_mut(4).zip(coverage) {
            let alpha = (u16::from(coverage) * u16::from(outline.color.a) + 127) / 255;
            pixel.copy_from_slice(&[
                ((u16::from(outline.color.r) * alpha + 127) / 255) as u8,
                ((u16::from(outline.color.g) * alpha + 127) / 255) as u8,
                ((u16::from(outline.color.b) * alpha + 127) / 255) as u8,
                alpha as u8,
            ]);
        }
        blur(result.data_mut(), size, outline.blur, cancelled)?;
        if !outline_only {
            result.draw_pixmap(
                0,
                0,
                fill.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        result
    } else {
        fill
    };
    if !outline_only && layer.opacity < 1.0 {
        for value in result.data_mut() {
            *value = (f32::from(*value) * layer.opacity).round() as u8;
        }
    }
    Ok(result)
}

// Snap whole-pixel rectangular spans consistently after all transforms; fractional
// spans and positions retain coverage antialiasing instead of disappearing.
pub(crate) fn align_rectangles(path: &Path) -> Option<Path> {
    let mut segments = path.segments();
    let mut aligned = PathBuilder::new();
    while let Some(first) = segments.next() {
        let (
            PathSegment::MoveTo(a),
            Some(PathSegment::LineTo(b)),
            Some(PathSegment::LineTo(c)),
            Some(PathSegment::LineTo(d)),
            Some(PathSegment::Close),
        ) = (
            first,
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
        )
        else {
            return None;
        };
        if !((a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x)
            || (a.x == b.x && b.y == c.y && c.x == d.x && d.y == a.y))
        {
            return None;
        }
        let snap_axis = |min: f32, max: f32| {
            let length = max - min;
            length >= 1.0 - 0.0001
                && (length - length.round()).abs() < 0.0001
                && (min * 2.0 - (min * 2.0).round()).abs() < 0.0001
        };
        let snap_x = snap_axis(a.x.min(c.x), a.x.max(c.x));
        let snap_y = snap_axis(a.y.min(c.y), a.y.max(c.y));
        for (index, p) in [a, b, c, d].into_iter().enumerate() {
            let x = if snap_x { (p.x + 0.5).floor() } else { p.x };
            let y = if snap_y { (p.y + 0.5).floor() } else { p.y };
            if index == 0 {
                aligned.move_to(x, y);
            } else {
                aligned.line_to(x, y);
            }
        }
        aligned.close();
    }
    aligned.finish()
}

fn blur(
    pixels: &mut [u8],
    size: [u32; 2],
    sigma: f32,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let radii = box_radii(sigma);
    if radii == [0; 3] {
        return Ok(());
    }
    let [width, height] = [size[0] as usize, size[1] as usize];
    let mut temporary = vec![0u8; pixels.len()];
    for radius in radii {
        if radius == 0 {
            continue;
        }
        let divisor = (2 * radius + 1) as u32;
        for y in 0..height {
            if y % 32 == 0 {
                ensure!(!cancelled(), "Rendering cancelled");
            }
            let mut sums = [0u32; 4];
            for x in 0..=radius.min(width - 1) {
                for channel in 0..4 {
                    sums[channel] += u32::from(pixels[(y * width + x) * 4 + channel]);
                }
            }
            for x in 0..width {
                for channel in 0..4 {
                    temporary[(y * width + x) * 4 + channel] =
                        ((sums[channel] + divisor / 2) / divisor) as u8;
                }
                if x >= radius {
                    for channel in 0..4 {
                        sums[channel] -= u32::from(pixels[(y * width + x - radius) * 4 + channel]);
                    }
                }
                if x + radius + 1 < width {
                    for channel in 0..4 {
                        sums[channel] +=
                            u32::from(pixels[(y * width + x + radius + 1) * 4 + channel]);
                    }
                }
            }
        }
        for x in 0..width {
            if x % 32 == 0 {
                ensure!(!cancelled(), "Rendering cancelled");
            }
            let mut sums = [0u32; 4];
            for y in 0..=radius.min(height - 1) {
                for channel in 0..4 {
                    sums[channel] += u32::from(temporary[(y * width + x) * 4 + channel]);
                }
            }
            for y in 0..height {
                for channel in 0..4 {
                    pixels[(y * width + x) * 4 + channel] =
                        ((sums[channel] + divisor / 2) / divisor) as u8;
                }
                if y >= radius {
                    for channel in 0..4 {
                        sums[channel] -=
                            u32::from(temporary[((y - radius) * width + x) * 4 + channel]);
                    }
                }
                if y + radius + 1 < height {
                    for channel in 0..4 {
                        sums[channel] +=
                            u32::from(temporary[((y + radius + 1) * width + x) * 4 + channel]);
                    }
                }
            }
        }
    }
    Ok(())
}

fn outline_mask(
    alpha: &[u8],
    size: [u32; 2],
    radius: f32,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<u8>> {
    let [width, height] = [size[0] as usize, size[1] as usize];
    let mut distances = vec![0.0; alpha.len()];
    let mut input = vec![0.0; width.max(height)];
    let mut output = vec![0.0; input.len()];
    let mut vertices = vec![0usize; input.len()];
    let mut edges = vec![0.0; input.len() + 1];
    for x in 0..width {
        if x % 32 == 0 {
            ensure!(!cancelled(), "Rendering cancelled");
        }
        for y in 0..height {
            let a = alpha[y * width + x];
            input[y] = if a == 0 {
                1e12
            } else {
                (1.0 - f32::from(a) / 255.0).powi(2)
            };
        }
        distance_line(
            &input[..height],
            &mut output[..height],
            &mut vertices,
            &mut edges,
        );
        for y in 0..height {
            distances[y * width + x] = output[y];
        }
    }
    let mut result = vec![0; alpha.len()];
    for y in 0..height {
        if y % 32 == 0 {
            ensure!(!cancelled(), "Rendering cancelled");
        }
        input[..width].copy_from_slice(&distances[y * width..(y + 1) * width]);
        distance_line(
            &input[..width],
            &mut output[..width],
            &mut vertices,
            &mut edges,
        );
        for (x, distance) in output.iter().take(width).enumerate() {
            let index = y * width + x;
            let expanded = (radius + 1.0 - distance.sqrt()).clamp(0.0, 1.0);
            result[index] = (expanded * f32::from(255 - alpha[index])).round() as u8;
        }
    }
    Ok(result)
}

// Squared Euclidean distance transform: two linear passes instead of a radius-sized search per pixel.
fn distance_line(input: &[f32], output: &mut [f32], vertices: &mut [usize], edges: &mut [f32]) {
    let mut count = 0;
    vertices[0] = 0;
    edges[0] = f32::NEG_INFINITY;
    edges[1] = f32::INFINITY;
    for q in 1..input.len() {
        let mut separation;
        loop {
            let p = vertices[count];
            separation = ((input[q] + (q * q) as f32) - (input[p] + (p * p) as f32))
                / (2.0 * (q - p) as f32);
            if separation > edges[count] || count == 0 {
                break;
            }
            count -= 1;
        }
        count += 1;
        vertices[count] = q;
        edges[count] = separation;
        edges[count + 1] = f32::INFINITY;
    }
    count = 0;
    for (q, value) in output.iter_mut().enumerate() {
        while edges[count + 1] < q as f32 {
            count += 1;
        }
        let distance = q as f32 - vertices[count] as f32;
        *value = distance * distance + input[vertices[count]];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_is_outside_the_shape_and_has_round_corners() {
        let mut alpha = vec![0u8; 25];
        alpha[12] = 255;
        let mask = outline_mask(&alpha, [5, 5], 1.0, &|| false).unwrap();
        assert_eq!(mask[12], 0);
        assert_eq!(mask[11], 255);
        assert!(mask[6] > 0 && mask[6] < 255);
        assert_eq!(mask[0], 0);
    }
    #[test]
    fn blur_preserves_premultiplication_and_spreads_alpha() {
        let mut pixels = vec![0u8; 9 * 9 * 4];
        pixels[(4 * 9 + 4) * 4..(4 * 9 + 5) * 4].copy_from_slice(&[128, 0, 0, 128]);
        blur(&mut pixels, [9, 9], 1.0, &|| false).unwrap();
        assert!(pixels[(4 * 9 + 3) * 4 + 3] > 0);
        assert!(
            pixels
                .chunks_exact(4)
                .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
        );
    }
}
