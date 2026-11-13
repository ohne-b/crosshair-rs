use std::fmt::Write;

use anyhow::{Context, Result, ensure};
use tiny_skia::{FilterQuality, PathSegment, Pixmap, PixmapPaint, Transform};

use crate::{
    AssetSource, LayerBounds,
    assets::AssetCache,
    geometry::{Element, PreparedLayer},
};

const MAX_SVG_BYTES: usize = 16 * 1024 * 1024;

fn check_size(size: usize, limit: usize) -> Result<()> {
    ensure!(
        size <= limit,
        "SVG export exceeds its size limit; use PNG or fewer image layers"
    );
    Ok(())
}

pub(crate) fn svg(
    layers: &[PreparedLayer],
    bounds: LayerBounds,
    cache: &mut AssetCache,
    assets: &impl AssetSource,
) -> Result<Vec<u8>> {
    let mut output = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{}" height="{}" viewBox="{} {} {} {}">"#,
        bounds.width(),
        bounds.height(),
        bounds.left,
        bounds.top,
        bounds.width(),
        bounds.height()
    );
    let mut id = 0usize;
    for layer in layers {
        let mut fill = String::new();
        for group in &layer.groups {
            let mut content = String::new();
            for element in &group.elements {
                match element {
                    Element::Path {
                        path,
                        color,
                        erase,
                        antialias,
                    } => {
                        let aligned = (!*antialias)
                            .then(|| crate::composite::align_rectangles(path))
                            .flatten();
                        let path = aligned.as_ref().unwrap_or(path);
                        let mut data = String::new();
                        for segment in path.segments() {
                            match segment {
                                PathSegment::MoveTo(p) => write!(data, "M{} {}", p.x, p.y)?,
                                PathSegment::LineTo(p) => write!(data, "L{} {}", p.x, p.y)?,
                                PathSegment::QuadTo(a, b) => {
                                    write!(data, "Q{} {} {} {}", a.x, a.y, b.x, b.y)?
                                }
                                PathSegment::CubicTo(a, b, c) => write!(
                                    data,
                                    "C{} {} {} {} {} {}",
                                    a.x, a.y, b.x, b.y, c.x, c.y
                                )?,
                                PathSegment::Close => data.push('Z'),
                            }
                            check_size(data.len(), MAX_SVG_BYTES)?;
                        }
                        let shape =
                            format!(r#"<path d="{data}" shape-rendering="geometricPrecision""#);
                        if *erase {
                            id += 1;
                            write!(
                                output,
                                r#"<defs><mask id="m{id}" maskUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><rect x="{}" y="{}" width="{}" height="{}" fill="white"/>{shape} fill="black" fill-opacity="{}"/></mask></defs>"#,
                                bounds.left,
                                bounds.top,
                                bounds.width(),
                                bounds.height(),
                                bounds.left,
                                bounds.top,
                                bounds.width(),
                                bounds.height(),
                                f32::from(color.a) / 255.0
                            )?;
                            content = format!(r#"<g mask="url(#m{id})">{content}</g>"#);
                        } else {
                            write!(
                                content,
                                r#"{shape} fill="rgb({},{},{})" fill-opacity="{}"/>"#,
                                color.r,
                                color.g,
                                color.b,
                                f32::from(color.a) / 255.0
                            )?;
                        }
                    }
                    Element::Image { image, transform } => {
                        let scale =
                            (transform.sx * transform.sx + transform.ky * transform.ky).sqrt();
                        let requested = [
                            (image.width * scale).ceil().clamp(1.0, 1024.0) as u32,
                            (image.height * scale).ceil().clamp(1.0, 1024.0) as u32,
                        ];
                        let (pixels, crop) =
                            cache.image(image.asset, requested, image.crop, assets)?;
                        // Embed only the visible crop at output resolution, not a 16 MP source.
                        let mut embedded_pixels = Pixmap::new(requested[0], requested[1])
                            .context("Could not allocate export image")?;
                        let sx = requested[0] as f32 / (pixels.width() as f32 * crop.width);
                        let sy = requested[1] as f32 / (pixels.height() as f32 * crop.height);
                        embedded_pixels.draw_pixmap(
                            0,
                            0,
                            pixels.as_ref().as_ref(),
                            &PixmapPaint {
                                quality: FilterQuality::Bilinear,
                                ..Default::default()
                            },
                            Transform::from_row(
                                sx,
                                0.0,
                                0.0,
                                sy,
                                -crop.x * pixels.width() as f32 * sx,
                                -crop.y * pixels.height() as f32 * sy,
                            ),
                            None,
                        );
                        id += 1;
                        write!(
                            output,
                            r#"<defs><clipPath id="c{id}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath></defs>"#,
                            -image.width / 2.0,
                            -image.height / 2.0,
                            image.width,
                            image.height
                        )?;
                        let embedded = format!(
                            r#"<g transform="matrix({} {} {} {} {} {})"><g clip-path="url(#c{id})"><image x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none" xlink:href="data:image/png;base64,{}"/></g></g>"#,
                            transform.sx,
                            transform.ky,
                            transform.kx,
                            transform.sy,
                            transform.tx,
                            transform.ty,
                            -image.width / 2.0,
                            -image.height / 2.0,
                            image.width,
                            image.height,
                            base64(&embedded_pixels.encode_png()?)
                        );
                        check_size(
                            output.len() + fill.len() + content.len() + embedded.len(),
                            MAX_SVG_BYTES,
                        )?;
                        content.push_str(&embedded);
                    }
                }
                check_size(output.len() + fill.len() + content.len(), MAX_SVG_BYTES)?;
            }
            fill.push_str(&blur(&mut output, &mut id, bounds, content, group.blur)?);
        }
        fill = blur(&mut output, &mut id, bounds, fill, layer.effects.blur)?;
        write!(output, r#"<g opacity="{}">"#, layer.opacity)?;
        let outline = &layer.effects.outline;
        if outline.enabled && outline.thickness > 0.0 && outline.color.a > 0 {
            // Preserve the renderer's Euclidean alpha-silhouette outline; the fill remains vector.
            let pixels = crate::composite::render_outline(
                layer,
                [bounds.left as i32, bounds.top as i32],
                [bounds.width() as u32, bounds.height() as u32],
                cache,
                assets,
            )?;
            write!(
                output,
                r#"<image x="{}" y="{}" width="{}" height="{}" xlink:href="data:image/png;base64,{}"/>"#,
                bounds.left,
                bounds.top,
                bounds.width(),
                bounds.height(),
                base64(&pixels.encode_png()?)
            )?;
        }
        output.push_str(&fill);
        output.push_str("</g>");
        check_size(output.len(), MAX_SVG_BYTES)?;
    }
    output.push_str("</svg>");
    check_size(output.len(), MAX_SVG_BYTES)?;
    Ok(output.into_bytes())
}

fn blur(
    output: &mut String,
    id: &mut usize,
    bounds: LayerBounds,
    content: String,
    sigma: f32,
) -> Result<String> {
    if sigma <= 0.0 {
        return Ok(content);
    }
    *id += 1;
    write!(
        output,
        r#"<defs><filter id="b{id}" filterUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="{sigma}"/></filter></defs>"#,
        bounds.left,
        bounds.top,
        bounds.width(),
        bounds.height()
    )?;
    Ok(format!(r#"<g filter="url(#b{id})">{content}</g>"#))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        result.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_size_limit_rejects_growth_at_the_boundary() {
        assert!(check_size(16, 16).is_ok());
        assert!(check_size(17, 16).is_err());
        assert!(check_size(usize::MAX, MAX_SVG_BYTES).is_err());
    }
}
