mod drawing;
mod style;
mod text;

use crosshair_renderer::Raster;
use tiny_skia::{FilterQuality, IntSize, Pixmap, PixmapPaint, Transform};

use crate::{Error, Result, ValidatedPreset};

pub use style::{HEIGHT, WIDTH};

/// Attribution is caller-supplied (e.g. authenticated Discord user), not
/// inferred from untrusted JSON. The card contains no download UI or badges.
pub struct ShowcaseCard<'a> {
    pub preset: &'a ValidatedPreset,
    pub author: Option<&'a str>,
}

/// Compact, single-panel image renderer. Branding, font faces, axes and
/// background grid are cached once and shared across many cards.
pub struct ShowcaseRenderer<'font> {
    typography: text::Typography<'font>,
    background: Pixmap,
}

impl<'font> ShowcaseRenderer<'font> {
    pub fn new(manrope: &'font [u8]) -> Result<Self> {
        let typography = text::Typography::new(manrope)?;
        let mut background = drawing::background()?;
        let logo = Pixmap::decode_png(include_bytes!("../../assets/logo.png"))
            .map_err(|error| Error::Render(error.to_string()))?;
        let scale = style::LOGO_SIZE / logo.width().max(logo.height()) as f32;
        background.draw_pixmap(
            0,
            0,
            logo.as_ref(),
            &PixmapPaint {
                quality: FilterQuality::Bilinear,
                ..Default::default()
            },
            Transform::from_row(scale, 0.0, 0.0, scale, style::LOGO_X, style::LOGO_Y),
            None,
        );
        typography.body(
            &mut background,
            "crosshair-rs",
            style::BRAND_X,
            style::BRAND_BASELINE,
            style::BRAND_WIDTH,
            18.0,
            style::MUTED,
        )?;
        Ok(Self {
            typography,
            background,
        })
    }

    pub fn render_png(&self, card: ShowcaseCard<'_>) -> Result<Vec<u8>> {
        let mut canvas = self.background.clone();
        let metadata = card.preset.metadata();
        self.typography.title(&mut canvas, &metadata.name)?;
        let subtitle = match card.author {
            Some(author) => format!("by {}", text::normalize(author)?),
            None => "Community preset".to_owned(),
        };
        self.typography.body(
            &mut canvas,
            &subtitle,
            style::LEFT,
            style::SUBTITLE_BASELINE,
            style::TITLE_WIDTH,
            21.0,
            style::MUTED,
        )?;

        // High-resolution preview from the real crosshair-renderer (created
        // at validation time). No nearest-neighbor stretching of 32px sprites.
        let raster = &card.preset.preview_raster;
        let size = IntSize::from_wh(raster.width, raster.height)
            .ok_or_else(|| Error::Render("invalid preview raster dimensions".into()))?;
        let preview = Pixmap::from_vec(raster.pixels.clone(), size)
            .ok_or_else(|| Error::Render("invalid preview raster buffer".into()))?;

        // Fit the VISIBLE ink, not the (usually mostly transparent) SVG canvas.
        // A 32x32 asset can contain just a 12px crosshair; fitting its full
        // 32x32 rectangle would unnecessarily shrink the sharp vector preview.
        let fit = visible_fit(raster);
        let origin_x = style::PREVIEW_X + style::PREVIEW_W / 2.0 + raster.origin[0] as f32 * fit;
        let origin_y = style::PREVIEW_Y + style::PREVIEW_H / 2.0 + raster.origin[1] as f32 * fit;
        canvas.draw_pixmap(
            0,
            0,
            preview.as_ref(),
            &PixmapPaint {
                quality: FilterQuality::Bilinear,
                ..Default::default()
            },
            Transform::from_row(fit, 0.0, 0.0, fit, origin_x, origin_y),
            None,
        );
        canvas
            .encode_png()
            .map_err(|error| Error::Render(error.to_string()))
    }
}

/// Fit pixels relative to the crosshair's actual design origin. Preserve the
/// axes as the origin even for asymmetrical assets and odd-sized designs.
fn visible_fit(raster: &Raster) -> f32 {
    let mut max_dx = 1.0_f32;
    let mut max_dy = 1.0_f32;
    for (index, pixel) in raster.pixels.chunks_exact(4).enumerate() {
        if pixel[3] < 5 {
            continue;
        }
        let x = (index as u32 % raster.width) as f32 + raster.origin[0] as f32;
        let y = (index as u32 / raster.width) as f32 + raster.origin[1] as f32;
        max_dx = max_dx.max(x.abs() + 1.0);
        max_dy = max_dy.max(y.abs() + 1.0);
    }
    ((style::PREVIEW_W / 2.0 - 26.0) / max_dx)
        .min((style::PREVIEW_H / 2.0 - 24.0) / max_dy)
        .min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_ignores_transparent_padding() {
        let mut raster = Raster {
            width: 512,
            height: 512,
            origin: [-256, -256],
            pixels: vec![0; 512 * 512 * 4],
        };
        for y in 180..332usize {
            for x in 180..332usize {
                raster.pixels[(y * 512 + x) * 4 + 3] = 255;
            }
        }
        assert_eq!(visible_fit(&raster), 1.0);
    }
}
