use rustybuzz::{
    ttf_parser::{GlyphId, OutlineBuilder, Tag},
    Direction, Face, GlyphBuffer, UnicodeBuffer, Variation,
};
use tiny_skia::{FillRule, PathBuilder, Pixmap, Transform};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::{drawing, style, Error, Result, MAX_FONT_BYTES};

const MAX_NAME_BYTES: usize = 1024;
const MAX_NAME_GRAPHEMES: usize = 64;

/// Parsed font faces, with the original design's variable-font weights applied.
/// Borrowing the font avoids self-referential structs and repeated font parsing.
pub(crate) struct Typography<'a> {
    title: Face<'a>,
    subtitle: Face<'a>,
}

struct Line {
    glyphs: GlyphBuffer,
    left: f32,
    width: f32,
}

impl<'a> Typography<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(Error::InputTooLarge {
                kind: "font",
                limit: MAX_FONT_BYTES,
            });
        }
        let mut title = Face::from_slice(bytes, 0)
            .ok_or(Error::InvalidFont("could not parse the first font face"))?;
        let weight = Tag::from_bytes(b"wght");
        let supports_weights = title.variation_axes().into_iter().any(|axis| {
            axis.tag == weight
                && axis.min_value <= style::SUBTITLE_WEIGHT
                && axis.max_value >= style::TITLE_WEIGHT
        });
        if !supports_weights {
            return Err(Error::InvalidFont("use the bundled variable Manrope.ttf"));
        }
        for character in "Welcome, Member #0123456789?…".chars() {
            if title.glyph_index(character).is_none() {
                return Err(Error::InvalidFont("font lacks a required banner character"));
            }
        }
        title.set_variations(&[Variation {
            tag: weight,
            value: style::TITLE_WEIGHT,
        }]);
        let mut subtitle = title.clone();
        subtitle.set_variations(&[Variation {
            tag: weight,
            value: style::SUBTITLE_WEIGHT,
        }]);
        Ok(Self { title, subtitle })
    }

    pub(crate) fn draw(&self, canvas: &mut Pixmap, name: &str, member: u64) {
        let name = self.supported_name(name);
        let (line, size) = self.fit_title(&name);
        draw_line(
            canvas,
            &self.title,
            &line,
            size,
            (style::TEXT_X, style::TITLE_BASELINE),
            Some(style::PREFIX.len() as u32),
            style::INK,
        );
        let subtitle = format!("Member #{}", format_member(member));
        let line = shape(&self.subtitle, &subtitle);
        draw_line(
            canvas,
            &self.subtitle,
            &line,
            style::SUBTITLE_SIZE,
            (style::TEXT_X + 2.0, style::SUBTITLE_BASELINE),
            None,
            style::MUTED,
        );
    }

    fn supported_name(&self, normalized: &str) -> String {
        let mut name = String::new();
        let mut graphemes = normalized.graphemes(true);
        for cluster in graphemes.by_ref().take(MAX_NAME_GRAPHEMES) {
            // Replace an unsupported grapheme as a unit; do not display raw
            // missing-glyph boxes or break an emoji/combining sequence in half.
            let covered = cluster
                .chars()
                .all(|ch| is_joiner_or_selector(ch) || self.title.glyph_index(ch).is_some());
            if covered {
                name.push_str(cluster);
            } else {
                name.push('?');
            }
        }
        if graphemes.next().is_some() {
            name.push('…');
        }
        name
    }

    fn fit_title(&self, name: &str) -> (Line, f32) {
        let available = style::TEXT_RIGHT - style::TEXT_X;
        let units_per_em = self.title.units_per_em() as f32;
        let line = shape(&self.title, &format!("{}{name}", style::PREFIX));
        let fitted_size =
            (available * units_per_em / line.width.max(1.0)).min(style::TITLE_MAX_SIZE);
        if fitted_size >= style::TITLE_MIN_SIZE {
            return (line, fitted_size);
        }

        // Once the minimum readable size is reached, ellipsize at grapheme
        // boundaries rather than shrinking the welcome text indefinitely.
        let clusters: Vec<&str> = name.graphemes(true).collect();
        for count in (0..clusters.len()).rev() {
            let candidate = format!("{}{}…", style::PREFIX, clusters[..count].concat());
            let line = shape(&self.title, &candidate);
            if line.width * style::TITLE_MIN_SIZE / units_per_em <= available {
                return (line, style::TITLE_MIN_SIZE);
            }
        }
        // Also safe for an unusual, very wide compatible font.
        let line = shape(&self.title, &format!("{}…", style::PREFIX));
        let size = (available * units_per_em / line.width.max(1.0)).min(style::TITLE_MIN_SIZE);
        (line, size)
    }
}

pub(crate) fn normalize_name(input: &str) -> Result<String> {
    if input.len() > MAX_NAME_BYTES {
        return Err(Error::InputTooLarge {
            kind: "display name",
            limit: MAX_NAME_BYTES,
        });
    }
    let cleaned: String = input
        .chars()
        .filter(|ch| {
            // Whitespace becomes a single space below. Explicit direction overrides
            // and invisible formatting must not reorder or disguise the heading.
            (!ch.is_control() || ch.is_whitespace())
                && !matches!(
                    *ch,
                    '\u{00ad}' | '\u{061c}' | '\u{200b}' | '\u{200e}' | '\u{200f}'
                        | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}'
                )
        })
        .collect();
    let compact = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized: String = compact.nfc().collect();
    if !normalized
        .chars()
        .any(|ch| !ch.is_whitespace() && !is_joiner_or_selector(ch))
    {
        return Err(Error::InvalidName("a visible, nonempty name is required"));
    }
    Ok(normalized)
}

fn is_joiner_or_selector(ch: char) -> bool {
    matches!(ch, '\u{200c}' | '\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

fn shape(face: &Face<'_>, text: &str) -> Line {
    let mut input = UnicodeBuffer::new();
    input.push_str(text);
    input.guess_segment_properties();
    // This design uses Manrope's LTR scripts. Full bidirectional layout and
    // color-emoji fallback are intentionally not claimed or implemented.
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

fn draw_line(
    canvas: &mut Pixmap,
    face: &Face<'_>,
    line: &Line,
    size: f32,
    baseline: (f32, f32),
    accent_start: Option<u32>,
    color: [u8; 4],
) {
    let scale = style::scaled(size) / face.units_per_em() as f32;
    let base_x = style::scaled(baseline.0) - line.left * scale;
    let base_y = style::scaled(baseline.1);
    let normal = drawing::paint(color);
    let accent = drawing::paint(style::ACCENT);
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
                let y = base_y - (pen_y + position.y_offset as f32) * scale;
                let ink = if accent_start.is_some_and(|start| info.cluster >= start) {
                    &accent
                } else {
                    &normal
                };
                canvas.fill_path(
                    &path,
                    ink,
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

fn format_member(number: u64) -> String {
    let digits = number.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_and_direction_overrides_are_removed() {
        assert_eq!(
            normalize_name("  oh\u{202e}neb \n\t rs  ").unwrap(),
            "ohneb rs"
        );
        assert!(normalize_name("\u{200b}\u{feff} \n").is_err());
        assert!(normalize_name("\u{200d}\u{fe0f}").is_err());
    }

    #[test]
    fn combining_accents_are_normalized_without_losing_the_name() {
        assert_eq!(normalize_name("Cafe\u{301}").unwrap(), "Café");
    }

    #[test]
    fn member_numbers_have_consistent_separators() {
        assert_eq!(format_member(81), "81");
        assert_eq!(format_member(1234), "1,234");
        assert_eq!(format_member(999_999_999), "999,999,999");
    }

    #[test]
    fn oversized_names_are_rejected() {
        assert!(matches!(
            normalize_name(&"a".repeat(1025)),
            Err(Error::InputTooLarge { .. })
        ));
    }
}
