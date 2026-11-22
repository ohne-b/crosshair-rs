use super::{AssetSource, LayerBounds, MAX_DIMENSION, Raster, Renderer, composite};
use anyhow::{Context, Result, ensure};
use crosshair_core::{Id, Point, Preset};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// Physical screen pixels per design pixel and the visible rectangle in design pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasView {
    pub scale: f32,
    pub visible: LayerBounds,
}

#[derive(Clone, Copy)]
pub struct CanvasBounds {
    pub geometry: LayerBounds,
    pub painted: LayerBounds,
    /// Local-axis selection box about its world center, for rotated primitive shapes.
    pub oriented: Option<(LayerBounds, f32)>,
}

impl CanvasBounds {
    pub fn union(self, other: Self) -> Self {
        Self {
            geometry: self.geometry.union(other.geometry),
            painted: self.painted.union(other.painted),
            oriented: None,
        }
    }
}

pub struct CanvasFrame {
    /// The full 100% output bounds, independent of zoom and clipping.
    pub bounds: LayerBounds,
    /// Magnified ink and filter support in design coordinates.
    pub painted: LayerBounds,
    pub view: CanvasView,
    pub scale: f32,
    pub tiles: Vec<Raster>,
    pub hits: CanvasHitMap,
}

/// One byte per displayed pixel identifies the topmost unlocked layer that painted it.
#[derive(Default)]
pub struct CanvasHitMap {
    layers: Vec<Option<Id>>,
    origin: [i32; 2],
    width: usize,
    scale: f32,
    pixels: Vec<u8>,
}

impl CanvasHitMap {
    pub(super) fn new(
        layers: &[super::geometry::PreparedLayer],
        origin: [i32; 2],
        width: u32,
        height: u32,
        scale: f32,
    ) -> Self {
        Self {
            layers: layers.iter().map(|layer| layer.pick_id).collect(),
            origin,
            width: width as usize,
            scale,
            pixels: vec![0; width as usize * height as usize],
        }
    }

    pub(super) fn paint(
        &mut self,
        index: usize,
        raster: &Pixmap,
        source: [usize; 2],
        origin: [i32; 2],
        width: usize,
        height: usize,
    ) {
        if self.layers[index].is_none() {
            return;
        }
        let left = (origin[0] - self.origin[0]) as usize;
        let top = (origin[1] - self.origin[1]) as usize;
        for y in 0..height {
            let src = (source[1] + y) * raster.width() as usize + source[0];
            let dst = (top + y) * self.width + left;
            for (id, pixel) in self.pixels[dst..dst + width]
                .iter_mut()
                .zip(&raster.pixels()[src..src + width])
            {
                if pixel.alpha() > 0 {
                    *id = (index + 1) as u8;
                }
            }
        }
    }

    pub fn layer_at(&self, point: Point, tolerance: f32) -> Option<Id> {
        if self.pixels.is_empty() {
            return None;
        }
        let x = point.x * self.scale - self.origin[0] as f32;
        let y = point.y * self.scale - self.origin[1] as f32;
        let height = self.pixels.len() / self.width;
        if x >= 0.0 && y >= 0.0 && x < self.width as f32 && y < height as f32 {
            let id = self.pixels[y as usize * self.width + x as usize];
            if id != 0 {
                return self.layers[id as usize - 1];
            }
        }
        let radius = tolerance * self.scale;
        let mut closest = radius * radius;
        let mut best = 0;
        for row in
            (y - radius).floor().max(0.0) as usize..((y + radius).ceil() as usize).min(height)
        {
            for col in (x - radius).floor().max(0.0) as usize
                ..((x + radius).ceil() as usize).min(self.width)
            {
                let id = self.pixels[row * self.width + col];
                let distance = (row as f32 + 0.5 - y).powi(2) + (col as f32 + 0.5 - x).powi(2);
                if id != 0 && (distance < closest || (distance == closest && id > best)) {
                    best = id;
                    closest = distance;
                }
            }
        }
        best.checked_sub(1)
            .and_then(|index| self.layers[index as usize])
    }
}

impl CanvasView {
    pub fn covers(self, view: CanvasView, b: LayerBounds) -> bool {
        let old = self.visible;
        let new = view.visible;
        self.scale == view.scale
            && new.left.max(b.left) >= old.left
            && new.top.max(b.top) >= old.top
            && new.right.min(b.right) <= old.right
            && new.bottom.min(b.bottom) <= old.bottom
    }
}

impl Renderer {
    pub fn canvas_layer_bounds(&mut self, layer: &crosshair_core::Layer) -> Result<CanvasBounds> {
        let prepared = super::geometry::prepare_layer(layer, 1.0, &mut self.text, false)?;
        let geometry = prepared.bounds;
        let painted = if prepared.is_empty() {
            geometry
        } else {
            super::geometry::group_bounds(&prepared.groups, &prepared.effects, geometry)?
        };
        let t = layer.transform;
        let oriented = layer
            .content
            .shape_size()
            .filter(|&(w, h)| w > 0.0 && h > 0.0 && t.rotation.rem_euclid(360.0) != 0.0)
            .map(|(w, h)| {
                // At quarter turns, use the actual snapped edge of a one-pixel rectangle.
                let (center, width, height) = if t.rotation.rem_euclid(90.0) == 0.0 {
                    if t.rotation.rem_euclid(180.0) == 0.0 {
                        (painted.center(), painted.width(), painted.height())
                    } else {
                        (painted.center(), painted.height(), painted.width())
                    }
                } else {
                    let local = LayerBounds {
                        left: -w / 2.0,
                        right: w / 2.0,
                        top: -h / 2.0,
                        bottom: h / 2.0,
                    };
                    let matrix = super::geometry::layer_transform(layer, 1.0, local);
                    let padding = 2.0 * super::geometry::effect_padding(&prepared.effects);
                    (
                        crosshair_core::Point {
                            x: matrix.tx,
                            y: matrix.ty,
                        },
                        t.scale * w + padding,
                        t.scale * h + padding,
                    )
                };
                (
                    LayerBounds {
                        left: center.x - width / 2.0,
                        right: center.x + width / 2.0,
                        top: center.y - height / 2.0,
                        bottom: center.y + height / 2.0,
                    },
                    t.rotation,
                )
            });
        Ok(CanvasBounds {
            geometry,
            painted,
            oriented,
        })
    }

    /// Magnify validated output geometry and draw only the visible part. Tile gutters
    /// include every filter's support so strokes and blurs cross seams continuously.
    pub fn render_canvas(
        &mut self,
        preset: &Preset,
        view: CanvasView,
        assets: &impl AssetSource,
        cancelled: impl Fn() -> bool,
    ) -> Result<CanvasFrame> {
        self.render_canvas_frame(preset, view, assets, false, cancelled)
    }

    pub fn render_canvas_frame(
        &mut self,
        preset: &Preset,
        view: CanvasView,
        assets: &impl AssetSource,
        pick_layers: bool,
        cancelled: impl Fn() -> bool,
    ) -> Result<CanvasFrame> {
        let (mut layers, bounds) = self.prepare(preset, 1.0, view.scale > 1.0, &cancelled)?;
        let v = view.visible;
        ensure!(
            view.scale.is_finite()
                && view.scale > 0.0
                && [v.left, v.top, v.right, v.bottom]
                    .into_iter()
                    .all(f32::is_finite),
            "Invalid canvas viewport"
        );
        let left = v.left.max(bounds.left);
        let top = v.top.max(bounds.top);
        let right = v.right.min(bounds.right);
        let bottom = v.bottom.min(bounds.bottom);
        let mut scale = view.scale;
        // Bound simultaneous tile storage on very large displays and filter scratch on large blurs.
        let area = ((right - left).max(0.0) * (bottom - top).max(0.0)).max(1.0);
        scale = scale.min((8_388_608.0 / area).sqrt());
        let (gutter, side) = loop {
            let gutter = layers
                .iter()
                .map(|layer| {
                    let group = layer
                        .groups
                        .iter()
                        .map(|g| composite::blur_padding(g.blur * scale))
                        .fold(0.0, f32::max);
                    let fill = group + composite::blur_padding(layer.effects.blur * scale);
                    let outline = &layer.effects.outline;
                    let outline =
                        if outline.enabled && outline.thickness > 0.0 && outline.color.a > 0 {
                            outline.thickness * scale
                                + composite::blur_padding(outline.blur * scale)
                                + 1.0
                        } else {
                            0.0
                        };
                    fill.max(outline).ceil() as u32
                })
                .max()
                .unwrap_or(0);
            if gutter <= MAX_DIMENSION / 4 {
                break (gutter, MAX_DIMENSION - 2 * gutter);
            }
            scale *= 0.5;
        };
        let mut ink: Option<LayerBounds> = None;
        for layer in &mut layers {
            ensure!(!cancelled(), "Rendering cancelled");
            layer.magnify(scale)?;
            let b = super::geometry::group_bounds(&layer.groups, &layer.effects, bounds)?;
            ink = Some(ink.map_or(b, |current| current.union(b)));
        }
        let ink = ink.unwrap_or(LayerBounds {
            left: 0.0,
            top: 0.0,
            right: scale,
            bottom: scale,
        });
        // A fixed tile grid keeps a curved path's tessellation stable while panning.
        let doc_x = ink.left.floor() as i32;
        let doc_y = ink.top.floor() as i32;
        let doc_right = ink.right.ceil() as i32;
        let doc_bottom = ink.bottom.ceil() as i32;
        let mut frame = CanvasFrame {
            bounds,
            view,
            scale,
            tiles: Vec::new(),
            hits: CanvasHitMap::default(),
            painted: LayerBounds {
                left: doc_x as f32 / scale,
                top: doc_y as f32 / scale,
                right: doc_right as f32 / scale,
                bottom: doc_bottom as f32 / scale,
            },
        };
        let x0 = ((v.left * scale).floor() as i32).max(doc_x);
        let y0 = ((v.top * scale).floor() as i32).max(doc_y);
        let x1 = ((v.right * scale).ceil() as i32).min(doc_right);
        let y1 = ((v.bottom * scale).ceil() as i32).min(doc_bottom);
        if x1 <= x0 || y1 <= y0 {
            return Ok(frame);
        }
        ensure!(
            i64::from(x1 - x0) * i64::from(y1 - y0) <= 8_388_608,
            "Magnified canvas exceeds its pixel budget"
        );
        let side = side as i32;
        if pick_layers {
            frame.hits =
                CanvasHitMap::new(&layers, [x0, y0], (x1 - x0) as u32, (y1 - y0) as u32, scale);
        }
        let tile_x = doc_x + (x0 - doc_x).div_euclid(side) * side;
        let tile_y = doc_y + (y0 - doc_y).div_euclid(side) * side;
        for y in (tile_y..y1).step_by(side as usize) {
            for x in (tile_x..x1).step_by(side as usize) {
                ensure!(!cancelled(), "Rendering cancelled");
                let origin = [x.max(x0), y.max(y0)];
                let width = ((x + side).min(x1) - origin[0]) as u32;
                let height = ((y + side).min(y1) - origin[1]) as u32;
                let mut output =
                    Pixmap::new(width, height).context("Could not allocate canvas tile")?;
                let padded_origin = [x - gutter as i32, y - gutter as i32];
                let padded_size = [
                    (doc_right - x).min(side) as u32 + 2 * gutter,
                    (doc_bottom - y).min(side) as u32 + 2 * gutter,
                ];
                for (index, layer) in layers.iter().enumerate() {
                    ensure!(!cancelled(), "Rendering cancelled");
                    let raster = composite::render_layer(
                        layer,
                        padded_origin,
                        padded_size,
                        &mut self.assets,
                        assets,
                        &cancelled,
                    )?;
                    if pick_layers {
                        frame.hits.paint(
                            index,
                            &raster,
                            [
                                (origin[0] - padded_origin[0]) as usize,
                                (origin[1] - padded_origin[1]) as usize,
                            ],
                            origin,
                            width as usize,
                            height as usize,
                        );
                    }
                    output.draw_pixmap(
                        padded_origin[0] - origin[0],
                        padded_origin[1] - origin[1],
                        raster.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                }
                frame.tiles.push(Raster {
                    width,
                    height,
                    origin,
                    pixels: output.take(),
                });
            }
        }
        Ok(frame)
    }
}
