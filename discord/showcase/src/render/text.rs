use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder, Tag};
use rustybuzz::{Direction, Face, GlyphBuffer, UnicodeBuffer, Variation};
use tiny_skia::{FillRule, PathBuilder, Pixmap, Transform};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use super::{drawing, style};
use crate::{Error, Result};

/// Font data is borrowed from the caller. No bundled font, hidden font search,
/// system fallback, self-referential state, or per-card font parsing.
pub(super) struct Typography<'font> {
    title: Face<'font>,
    body: Face<'font>,
}

struct Line {
    glyphs: GlyphBuffer,
    left: f32,
    width: f32,
}

impl<'font> Typography<'font> {
    pub(super) fn new(bytes: &'font [u8]) -> Result<Self> {
        if bytes.len() > style::MAX_FONT_BYTES {
            return Err(Error::Font("font exceeds 8 MiB"));
        }
        let mut title = Face::from_slice(bytes, 0).ok_or(Error::Font("could not parse font"))?;
        let weight = Tag::from_bytes(b"wght");
        if !title.variation_axes().into_iter().any(|axis| {
            axis.tag == weight
                && axis.min_value <= style::TEXT_WEIGHT
                && axis.max_value >= style::TITLE_WEIGHT
        }) {
            return Err(Error::Font("use the website's variable Manrope.ttf"));
        }
        for character in
            "crosshair-rs0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz?…×·".chars()
        {
            if title.glyph_index(character).is_none() {
                return Err(Error::Font("font lacks a required showcase character"));
            }
        }
        title.set_variations(&[Variation {
            tag: weight,
            value: style::TITLE_WEIGHT,
        }]);
        let mut body = title.clone();
        body.set_variations(&[Variation {
            tag: weight,
            value: style::TEXT_WEIGHT,
        }]);
        Ok(Self { title, body })
    }

    pub(super) fn title(&self, canvas: &mut Pixmap, title: &str) -> Result<()> {
        let name = self.supported(&normalize(title)?, &self.title);
        let (line, size) = fit(
            &self.title,
            &name,
            style::TITLE_WIDTH,
            style::TITLE_MAX_SIZE,
            style::TITLE_MIN_SIZE,
        );
        draw(
            canvas,
            &self.title,
            &line,
            size,
            (style::LEFT, style::TITLE_BASELINE),
            style::INK,
        );
        Ok(())
    }

    pub(super) fn body(
        &self,
        canvas: &mut Pixmap,
        text: &str,
        x: f32,
        baseline: f32,
        width: f32,
        size: f32,
        color: [u8; 4],
    ) -> Result<()> {
        let text = self.supported(&normalize(text)?, &self.body);
        let (line, size) = fit(&self.body, &text, width, size, size);
        draw(canvas, &self.body, &line, size, (x, baseline), color);
        Ok(())
    }

    fn supported(&self, text: &str, face: &Face<'_>) -> String {
        let mut output = String::new();
        for cluster in text.graphemes(true).take(160) {
            let supported = cluster
                .chars()
                .all(|ch| is_joiner(ch) || face.glyph_index(ch).is_some());
            output.push_str(if supported { cluster } else { "?" });
        }
        if text.graphemes(true).count() > 160 {
            output.push('…');
        }
        output
    }
}

/// This normalization affects display text ONLY, never the download bytes.
pub(super) fn normalize(input: &str) -> Result<String> {
    if input.len() > 2048 {
        return Err(Error::DisplayText("text exceeds 2048 bytes"));
    }
    let clean: String = input
        .chars()
        .filter(|ch| {
            (!ch.is_control() || ch.is_whitespace())
                && !matches!(
                    *ch, '\u{00ad}' | '\u{061c}' | '\u{200b}' | '\u{200e}' | '\u{200f}'
                        | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}'
                )
        })
        .collect();
    let compact = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized: String = compact.nfc().collect();
    if !normalized
        .chars()
        .any(|ch| !ch.is_whitespace() && !is_joiner(ch))
    {
        return Err(Error::DisplayText("text must contain a visible character"));
    }
    Ok(normalized)
}

fn is_joiner(ch: char) -> bool {
    matches!(ch, '\u{200c}' | '\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

fn fit(face: &Face<'_>, value: &str, width: f32, maximum: f32, minimum: f32) -> (Line, f32) {
    let line = shape(face, value);
    let units = face.units_per_em() as f32;
    let size = (width * units / line.width.max(1.0)).min(maximum);
    if size >= minimum {
        return (line, size);
    }
    let clusters: Vec<_> = value.graphemes(true).collect();
    for count in (0..clusters.len()).rev() {
        let candidate = format!("{}…", clusters[..count].concat());
        let line = shape(face, &candidate);
        if line.width * minimum / units <= width {
            return (line, minimum);
        }
    }
    let line = shape(face, "…");
    let size = (width * units / line.width.max(1.0)).min(minimum);
    (line, size)
}

fn shape(face: &Face<'_>, value: &str) -> Line {
    let mut input = UnicodeBuffer::new();
    input.push_str(value);
    input.guess_segment_properties();
    // This card uses Manrope's LTR scripts; full bidi/color-emoji fallback is not
    // implemented. Unsupported graphemes are represented by a visible '?'.
    input.set_direction(Direction::LeftToRight);
    let glyphs = rustybuzz::shape(face, &[], input);
    let (mut pen, mut left, mut right) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (info, position) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
        if let Some(bounds) = face.glyph_bounding_box(GlyphId(info.glyph_id as u16)) {
            let origin = pen + position.x_offset as f32;
            left = left.min(origin + f32::from(bounds.x_min));
            right = right.max(origin + f32::from(bounds.x_max));
        }
        pen += position.x_advance as f32;
        right = right.max(pen);
    }
    Line {
        glyphs,
        left,
        width: right - left,
    }
}

fn draw(
    canvas: &mut Pixmap,
    face: &Face<'_>,
    line: &Line,
    size: f32,
    at: (f32, f32),
    color: [u8; 4],
) {
    let scale = size / face.units_per_em() as f32;
    let base_x = at.0 - line.left * scale;
    let ink = drawing::paint(color);
    let (mut pen_x, mut pen_y) = (0.0_f32, 0.0_f32);
    for (info, position) in line
        .glyphs
        .glyph_infos()
        .iter()
        .zip(line.glyphs.glyph_positions())
    {
        let mut outline = GlyphPath(PathBuilder::new());
        if face
            .outline_glyph(GlyphId(info.glyph_id as u16), &mut outline)
            .is_some()
        {
            if let Some(path) = outline.0.finish() {
                let x = base_x + (pen_x + position.x_offset as f32) * scale;
                let y = at.1 - (pen_y + position.y_offset as f32) * scale;
                canvas.fill_path(
                    &path,
                    &ink,
                    FillRule::Winding,
                    Transform::from_row(scale, 0.0, 0.0, -scale, x, y),
                    None,
                );
            }
        }
        pen_x += position.x_advance as f32;
        pen_y += position.y_advance as f32;
    }
}

struct GlyphPath(PathBuilder);
impl OutlineBuilder for GlyphPath {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn display_text_removes_direction_overrides_and_collapses_space() {
        assert_eq!(normalize("  oh\u{202e}neb\nrs  ").unwrap(), "ohneb rs");
        assert_eq!(normalize("Cafe\u{301}").unwrap(), "Café");
        assert!(normalize("\u{200b}\u{200d}\u{fe0f}").is_err());
    }

    #[test]
    fn display_text_has_a_byte_limit() {
        assert!(normalize(&"a".repeat(2049)).is_err());
    }
}
