use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

use crate::{style, Error, Result};

pub(crate) fn paint(color: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    paint
}

pub(crate) fn background() -> Result<Pixmap> {
    let mut canvas = Pixmap::new(style::WIDTH * style::SCALE, style::HEIGHT * style::SCALE)
        .ok_or(Error::Internal("could not allocate the canvas"))?;
    let [r, g, b, a] = style::BACKGROUND;
    canvas.fill(Color::from_rgba8(r, g, b, a));
    Ok(canvas)
}

pub(crate) fn over_at(canvas: &mut Pixmap, image: &Pixmap, center: (f32, f32)) {
    let x = style::pixels(center.0) as i32 - image.width() as i32 / 2;
    let y = style::pixels(center.1) as i32 - image.height() as i32 / 2;
    canvas.draw_pixmap(
        x,
        y,
        image.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

/// The logo's bolt holes show the panel, not the avatar behind the ring.
pub(crate) fn cover_bolt_holes(canvas: &mut Pixmap) -> Result<()> {
    let mut builder = PathBuilder::new();
    for index in 0..5 {
        let angle = index as f32 * std::f32::consts::TAU / 5.0;
        let x = style::AVATAR_CENTER.0 + style::BOLT_RING_RADIUS * angle.sin();
        let y = style::AVATAR_CENTER.1 - style::BOLT_RING_RADIUS * angle.cos();
        builder.push_circle(x, y, style::BOLT_RADIUS);
    }
    let holes = builder
        .finish()
        .ok_or(Error::Internal("invalid bolt-hole geometry"))?;
    canvas.fill_path(
        &holes,
        &paint(style::BACKGROUND),
        FillRule::Winding,
        Transform::from_scale(style::SCALE as f32, style::SCALE as f32),
        None,
    );
    Ok(())
}
