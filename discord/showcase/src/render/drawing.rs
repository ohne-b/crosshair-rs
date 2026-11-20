use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::style;
use crate::{Error, Result};

pub(super) fn paint(color: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    paint
}

fn line(image: &mut Pixmap, start: (f32, f32), end: (f32, f32), color: [u8; 4], thickness: f32) {
    let mut builder = PathBuilder::new();
    builder.move_to(start.0, start.1);
    builder.line_to(end.0, end.1);
    if let Some(path) = builder.finish() {
        image.stroke_path(
            &path,
            &paint(color),
            &Stroke {
                width: thickness,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

pub(super) fn background() -> Result<Pixmap> {
    let mut output = Pixmap::new(style::WIDTH, style::HEIGHT)
        .ok_or_else(|| Error::Render("could not allocate showcase canvas".into()))?;

    let [r, g, b, a] = style::BACKGROUND;
    output.fill(Color::from_rgba8(r, g, b, a));

    let x0 = style::PREVIEW_X;
    let y0 = style::PREVIEW_Y;
    let x1 = x0 + style::PREVIEW_W;
    let y1 = y0 + style::PREVIEW_H;
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    let step = style::GRID_STEP;
    let mut dx = step;
    while cx + dx < x1 - 16.0 {
        for x in [cx - dx, cx + dx] {
            line(
                &mut output,
                (x, y0 + 16.0),
                (x, y1 - 16.0),
                style::GRID,
                1.0,
            );
        }
        dx += step;
    }
    let mut dy = step;
    while cy + dy < y1 - 16.0 {
        for y in [cy - dy, cy + dy] {
            line(
                &mut output,
                (x0 + 16.0, y),
                (x1 - 16.0, y),
                style::GRID,
                1.0,
            );
        }
        dy += step;
    }
    // Clearly visible central plus/crosshair alignment guide.
    line(
        &mut output,
        (cx, y0 + 1.0),
        (cx, y1 - 1.0),
        style::AXIS,
        1.6,
    );
    line(
        &mut output,
        (x0 + 1.0, cy),
        (x1 - 1.0, cy),
        style::AXIS,
        1.6,
    );

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn central_axes_are_visible_over_grid() {
        let image = background().unwrap();
        let cx = (style::PREVIEW_X + style::PREVIEW_W / 2.0) as u32;
        let cy = (style::PREVIEW_Y + style::PREVIEW_H / 2.0) as u32;
        assert_ne!(image.pixel(cx, cy - 10), image.pixel(cx + 10, cy - 10));
    }
}
