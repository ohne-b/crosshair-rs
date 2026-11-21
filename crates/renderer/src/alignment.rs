use anyhow::{Context, Result};
use crosshair_core::{ArmShape, Crosshair, Effects};
use tiny_skia::{Path, PathBuilder, PathSegment, Transform};

use crate::geometry::{DrawGroup, Element, LayerBounds, effect_padding};

#[derive(Clone, Copy)]
pub(super) struct PixelAxis {
    origin: f32,
    shift: f32,
}

fn grid_shift(start: f32, length: f32) -> f32 {
    if length < 0.9999 || (length - length.round()).abs() >= 0.0001 {
        return 0.0;
    }
    edge_shift(start)
}

fn edge_shift(start: f32) -> f32 {
    let twice = (2.0 * start).round();
    if (2.0 * start - twice).abs() < 0.0001 {
        (twice / 2.0 + 0.5).floor() - start
    } else {
        0.0
    }
}

pub(super) fn crosshair_axes(
    c: &Crosshair,
    rotation: f32,
    transform: Transform,
    scale: f32,
) -> Option<[PixelAxis; 2]> {
    let (first, opposite) = c.enabled_arms.split_at(c.enabled_arms.len() / 2);
    if !matches!(c.shape, ArmShape::Rectangle | ArmShape::TShape { .. }) || first != opposite {
        return None;
    }
    let cardinal = c.enabled_arms.iter().enumerate().any(|(i, enabled)| {
        let turns = (rotation + c.rotation + i as f32 * 360.0 / f32::from(c.arms)) / 90.0;
        *enabled && (turns - turns.round()).abs() < 0.000001
    });
    let width = c.thickness * scale;
    cardinal.then(|| {
        [transform.tx, transform.ty].map(|origin| PixelAxis {
            origin,
            shift: grid_shift(origin - width / 2.0, width),
        })
    })
}

pub(super) fn align_paths(
    groups: &mut [DrawGroup],
    center: Option<[PixelAxis; 2]>,
    effects: &Effects,
    bounds: LayerBounds,
) -> Result<()> {
    let mut replacements = Vec::new();
    for group in groups {
        let padding = crate::composite::blur_padding(group.blur) + effect_padding(effects);
        let crop = LayerBounds {
            left: bounds.left.floor(),
            top: bounds.top.floor(),
            right: bounds.right.ceil(),
            bottom: bounds.bottom.ceil(),
        }
        .outset(-padding);
        for element in &mut group.elements {
            let Element::Path {
                path, antialias, ..
            } = element
            else {
                continue;
            };
            let aligned = if !*antialias {
                let aligned = align_rectangles(path, center, crop);
                if center.is_some() && aligned.is_none() {
                    return Ok(());
                }
                aligned
            } else if let Some([x, y]) = center {
                Some(
                    path.clone()
                        .transform(Transform::from_translate(x.shift, y.shift))
                        .context("Invalid pixel-aligned path")?,
                )
            } else {
                None
            };
            if let Some(aligned) = aligned {
                // A dominant dot or diagonal can already fill the native crop. Keep its exact
                // vector center if shifting the compound shape would clip its opposite edges.
                let ink = aligned.bounds();
                if center.is_some()
                    && (ink.left() < crop.left - 0.0001
                        || ink.top() < crop.top - 0.0001
                        || ink.right() > crop.right + 0.0001
                        || ink.bottom() > crop.bottom + 0.0001)
                {
                    return Ok(());
                }
                replacements.push((path, aligned));
            }
        }
    }
    for (path, aligned) in replacements {
        *path = aligned;
    }
    Ok(())
}

// Snap whole-pixel rectangular spans consistently after all transforms; fractional
// spans and positions retain coverage antialiasing instead of disappearing.
fn align_rectangles(
    path: &Path,
    center: Option<[PixelAxis; 2]>,
    crop: LayerBounds,
) -> Option<Path> {
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
        let near = |a: f32, b: f32| (a - b).abs() < 0.0001;
        let axis_aligned = (near(a.y, b.y) && near(b.x, c.x) && near(c.y, d.y) && near(d.x, a.x))
            || (near(a.x, b.x) && near(b.y, c.y) && near(c.x, d.x) && near(d.y, a.y));
        let dx = span_offset(
            a.x,
            c.x,
            axis_aligned,
            center.map(|axes| axes[0]),
            [crop.left, crop.right],
        )?;
        let dy = span_offset(
            a.y,
            c.y,
            axis_aligned,
            center.map(|axes| axes[1]),
            [crop.top, crop.bottom],
        )?;
        for (index, p) in [a, b, c, d].into_iter().enumerate() {
            let x = p.x + dx;
            let y = p.y + dy;
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

fn span_offset(
    a: f32,
    b: f32,
    cardinal: bool,
    center: Option<PixelAxis>,
    crop: [f32; 2],
) -> Option<f32> {
    if !cardinal {
        return Some(center.map_or(0.0, |axis| axis.shift));
    }
    let low = a.min(b);
    let high = a.max(b);
    let Some(PixelAxis { origin, shift }) = center else {
        return Some(grid_shift(low, high - low));
    };
    let middle = (low + high) / 2.0;
    if (middle - origin).abs() < 0.0001 {
        return Some(shift);
    }
    if (origin * 2.0 - (origin * 2.0).round()).abs() >= 0.0001 {
        return Some(0.0);
    }
    let negative = middle < origin;
    let start = if negative { 2.0 * origin - high } else { low };
    let axis = origin + shift;
    let mut offset = shift - edge_shift(-(start + shift));
    // Resolve half-pixel ties inward except when that would close a positive gap.
    if start > origin + 0.0001 && start + offset <= axis + 0.0001 {
        offset += 1.0;
    }
    // Both fractional tips must fit the native crop after reflecting the positive arm.
    let end = start + (high - low);
    let limit = crop[1].min(2.0 * axis - crop[0]);
    offset = offset.min(limit - end);
    if start > origin + 0.0001 && start + offset <= axis + 0.0001 {
        return None;
    }
    Some(if negative {
        2.0 * shift - offset
    } else {
        offset
    })
}
