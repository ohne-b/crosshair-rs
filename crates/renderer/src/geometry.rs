use std::f32::consts::TAU;

use anyhow::{Context, Result, ensure};
use crosshair_core::{ArmShape, Color, Content, Crosshair, DotShape, Effects, Image, Layer, Text};
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder, Rect, Stroke, Transform};

use crate::text::TextRenderer;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl LayerBounds {
    pub fn width(self) -> f32 {
        self.right - self.left
    }
    pub fn height(self) -> f32 {
        self.bottom - self.top
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
    pub fn outset(self, size: f32) -> Self {
        Self {
            left: self.left - size,
            top: self.top - size,
            right: self.right + size,
            bottom: self.bottom + size,
        }
    }
    fn from_rect(rect: Rect) -> Self {
        Self {
            left: rect.left(),
            top: rect.top(),
            right: rect.right(),
            bottom: rect.bottom(),
        }
    }
}

#[derive(Clone)]
pub(crate) enum Element {
    Path {
        path: Path,
        color: Color,
        erase: bool,
        antialias: bool,
    },
    Image {
        image: Image,
        transform: Transform,
    },
}

impl Element {
    fn bounds(&self) -> Result<LayerBounds> {
        match self {
            Self::Path { path, .. } => Ok(LayerBounds::from_rect(path.bounds())),
            Self::Image { image, transform } => {
                let rect = Rect::from_xywh(
                    -image.width / 2.0,
                    -image.height / 2.0,
                    image.width,
                    image.height,
                )
                .context("Invalid image bounds")?;
                Ok(LayerBounds::from_rect(
                    PathBuilder::from_rect(rect)
                        .transform(*transform)
                        .context("Invalid image transform")?
                        .bounds(),
                ))
            }
        }
    }

    fn transform(self, transform: Transform) -> Result<Self> {
        match self {
            Self::Path {
                path,
                color,
                erase,
                antialias,
            } => Ok(Self::Path {
                path: path
                    .transform(transform)
                    .context("Layer transform is out of range")?,
                color,
                erase,
                antialias: antialias || !is_pixel_aligned(transform),
            }),
            Self::Image {
                image,
                transform: image_transform,
            } => Ok(Self::Image {
                image,
                transform: image_transform.post_concat(transform),
            }),
        }
    }
}

pub(crate) struct DrawGroup {
    pub elements: Vec<Element>,
    pub blur: f32,
}

pub(crate) struct PreparedLayer {
    pub groups: Vec<DrawGroup>,
    pub effects: Effects,
    pub opacity: f32,
    pub bounds: LayerBounds,
}

impl PreparedLayer {
    pub fn is_empty(&self) -> bool {
        self.groups.iter().all(|group| group.elements.is_empty())
    }

    pub fn point_count(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|group| &group.elements)
            .map(|element| match element {
                Element::Path { path, .. } => path.points().len(),
                Element::Image { .. } => 4,
            })
            .sum()
    }
}

pub(crate) fn prepare_layer(
    layer: &Layer,
    scale: f32,
    text: &mut TextRenderer,
) -> Result<PreparedLayer> {
    crosshair_core::validation::validate_layer_geometry(layer)?;
    ensure!(
        scale.is_finite() && scale > 0.0 && scale <= 16.0,
        "Invalid output scale"
    );
    let groups = content(&layer.content, text)?;
    let local = intrinsic_bounds(&groups)?;
    let transform = layer_transform(layer, scale, local);
    let effect_scale = scale * layer.transform.scale;
    let mut effects = layer.effects.clone();
    effects.blur *= effect_scale;
    effects.outline.thickness *= effect_scale;
    effects.outline.blur *= effect_scale;
    let mut final_bounds: Option<LayerBounds> = None;
    let mut transformed = Vec::with_capacity(groups.len());
    for group in groups {
        let blur = group.blur * effect_scale;
        let mut elements = Vec::with_capacity(group.elements.len());
        for element in group.elements {
            let element = element.transform(transform)?;
            let bounds = element
                .bounds()?
                .outset(crate::composite::blur_padding(blur));
            final_bounds = Some(final_bounds.map_or(bounds, |previous| previous.union(bounds)));
            elements.push(element);
        }
        transformed.push(DrawGroup { elements, blur });
    }
    let padding = crate::composite::blur_padding(effects.blur).max(if effects.outline.enabled {
        effects.outline.thickness + crate::composite::blur_padding(effects.outline.blur) + 1.0
    } else {
        0.0
    });
    let bounds = final_bounds.unwrap_or(local).outset(padding);
    ensure!(
        [bounds.left, bounds.top, bounds.right, bounds.bottom]
            .iter()
            .all(|value| value.is_finite()),
        "Layer bounds are not finite"
    );
    ensure!(
        bounds.width() <= 1024.0 && bounds.height() <= 1024.0,
        "Layer exceeds 1024 physical pixels including effects"
    );
    Ok(PreparedLayer {
        groups: transformed,
        effects,
        opacity: layer.opacity,
        bounds,
    })
}

pub(crate) fn local_point(
    layer: &Layer,
    point: crosshair_core::Point,
    text: &mut TextRenderer,
) -> Result<crosshair_core::Point> {
    crosshair_core::validation::validate_layer_geometry(layer)?;
    ensure!(
        point.x.is_finite() && point.y.is_finite(),
        "Canvas point is not finite"
    );
    let groups = content(&layer.content, text)?;
    let matrix = layer_transform(layer, 1.0, intrinsic_bounds(&groups)?)
        .invert()
        .context("Layer transform cannot be inverted")?;
    let mut point = tiny_skia::Point::from_xy(point.x, point.y);
    matrix.map_point(&mut point);
    Ok(crosshair_core::Point {
        x: point.x,
        y: point.y,
    })
}

pub(crate) fn anchor_point(
    layer: &Layer,
    text: &mut TextRenderer,
) -> Result<crosshair_core::Point> {
    crosshair_core::validation::validate_layer_geometry(layer)?;
    let groups = content(&layer.content, text)?;
    Ok(pivot(layer, intrinsic_bounds(&groups)?))
}

fn intrinsic_bounds(groups: &[DrawGroup]) -> Result<LayerBounds> {
    let mut local_bounds: Option<LayerBounds> = None;
    for element in groups.iter().flat_map(|group| &group.elements) {
        let bounds = element.bounds()?;
        local_bounds = Some(local_bounds.map_or(bounds, |previous| previous.union(bounds)));
    }
    Ok(local_bounds.unwrap_or(LayerBounds {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    }))
}

fn layer_transform(layer: &Layer, scale: f32, local: LayerBounds) -> Transform {
    let pivot = pivot(layer, local);
    let angle = layer.transform.rotation.to_radians();
    let (sin, cos) = (snap_trig(angle.sin()), snap_trig(angle.cos()));
    let local_scale = layer.transform.scale;
    Transform::from_row(
        cos * local_scale * scale,
        sin * local_scale * scale,
        -sin * local_scale * scale,
        cos * local_scale * scale,
        (layer.transform.x + pivot.x - local_scale * (cos * pivot.x - sin * pivot.y)) * scale,
        (layer.transform.y + pivot.y - local_scale * (sin * pivot.x + cos * pivot.y)) * scale,
    )
}

fn pivot(layer: &Layer, local: LayerBounds) -> crosshair_core::Point {
    let (anchor_x, anchor_y) = layer.transform.anchor.fractions();
    crosshair_core::Point {
        x: local.left + local.width() * anchor_x,
        y: local.top + local.height() * anchor_y,
    }
}

fn snap_trig(value: f32) -> f32 {
    if value.abs() < 1e-6 {
        0.0
    } else if (value.abs() - 1.0).abs() < 1e-6 {
        value.signum()
    } else {
        value
    }
}

fn is_pixel_aligned(transform: Transform) -> bool {
    let axis_aligned = (transform.kx == 0.0 && transform.ky == 0.0)
        || (transform.sx == 0.0 && transform.sy == 0.0);
    axis_aligned
        && [
            transform.sx,
            transform.sy,
            transform.kx,
            transform.ky,
            transform.tx,
            transform.ty,
        ]
        .iter()
        .all(|v| v.fract() == 0.0)
}

fn element(path: Path, color: Color, antialias: bool) -> Element {
    Element::Path {
        path,
        color,
        erase: false,
        antialias,
    }
}
fn single(elements: Vec<Element>) -> Vec<DrawGroup> {
    vec![DrawGroup {
        elements,
        blur: 0.0,
    }]
}

fn content(shape: &Content, text: &mut TextRenderer) -> Result<Vec<DrawGroup>> {
    Ok(match shape {
        Content::Crosshair(crosshair) => crosshair_groups(crosshair, text)?,
        Content::Rectangle(rect) => single(
            rounded_rect(rect.width, rect.height, rect.radius)
                .map(|p| element(p, rect.color, rect.radius > 0.0))
                .into_iter()
                .collect(),
        ),
        Content::Ellipse(shape) => single(
            ellipse(shape.width, shape.height)
                .map(|p| element(p, shape.color, true))
                .into_iter()
                .collect(),
        ),
        Content::Triangle(shape) => single(
            triangle(shape.width, shape.height)
                .map(|p| element(p, shape.color, true))
                .into_iter()
                .collect(),
        ),
        Content::Text(value) => single(text_elements(value, text)?),
        Content::Image(image) => single(if image.width > 0.0 && image.height > 0.0 {
            vec![Element::Image {
                image: image.clone(),
                transform: Transform::identity(),
            }]
        } else {
            Vec::new()
        }),
        Content::Freehand(drawing) => {
            let mut elements = Vec::with_capacity(drawing.strokes.len());
            for stroke in &drawing.strokes {
                if stroke.width <= 0.0 || stroke.points.is_empty() {
                    continue;
                }
                let path = if stroke.points.len() == 1 {
                    let point = stroke.points[0];
                    PathBuilder::from_circle(point.x, point.y, stroke.width / 2.0)
                } else {
                    let mut path = PathBuilder::new();
                    path.move_to(stroke.points[0].x, stroke.points[0].y);
                    for point in &stroke.points[1..] {
                        path.line_to(point.x, point.y);
                    }
                    path.finish().and_then(|path| {
                        path.stroke(
                            &Stroke {
                                width: stroke.width,
                                line_cap: LineCap::Round,
                                line_join: LineJoin::Round,
                                ..Default::default()
                            },
                            1.0,
                        )
                    })
                };
                if let Some(path) = path {
                    elements.push(Element::Path {
                        path,
                        color: stroke.color,
                        erase: stroke.erase,
                        antialias: true,
                    });
                }
            }
            single(elements)
        }
    })
}

fn text_elements(value: &Text, renderer: &mut TextRenderer) -> Result<Vec<Element>> {
    Ok(renderer
        .paths(value)?
        .into_iter()
        .map(|path| element(path, value.color, true))
        .collect())
}

fn crosshair_groups(crosshair: &Crosshair, text: &mut TextRenderer) -> Result<Vec<DrawGroup>> {
    let mut arms = Vec::new();
    let mut point_count = 0usize;
    let text_arm = match &crosshair.shape {
        ArmShape::Text(value) => Some(text_elements(value, text)?),
        _ => None,
    };
    if crosshair.length > 0.0 && crosshair.thickness > 0.0 {
        let length = crosshair.length;
        let thickness = crosshair.thickness;
        let gap = crosshair.gap;
        for index in 0..crosshair.arms as usize {
            if !crosshair.enabled_arms.get(index).copied().unwrap_or(false) {
                continue;
            }
            let angle =
                crosshair.rotation.to_radians() + index as f32 * TAU / f32::from(crosshair.arms);
            let (sin, cos) = (snap_trig(angle.sin()), snap_trig(angle.cos()));
            let rotation = Transform::from_row(cos, sin, -sin, cos, 0.0, 0.0);
            let mut primitives = match &crosshair.shape {
                ArmShape::Rectangle => vec![element(
                    PathBuilder::from_rect(
                        Rect::from_xywh(gap, -thickness / 2.0, length, thickness)
                            .context("Invalid arm rectangle")?,
                    ),
                    crosshair.color,
                    false,
                )],
                ArmShape::Triangle => {
                    let mut path = PathBuilder::new();
                    path.move_to(gap, -thickness / 2.0);
                    path.line_to(gap + length, 0.0);
                    path.line_to(gap, thickness / 2.0);
                    path.close();
                    path.finish()
                        .map(|p| element(p, crosshair.color, true))
                        .into_iter()
                        .collect()
                }
                ArmShape::Chevron { inward, depth } => {
                    let depth = depth.min(length);
                    let mut path = PathBuilder::new();
                    let points = [
                        [gap, -thickness / 2.0],
                        [gap + length - depth, -thickness / 2.0],
                        [gap + length, 0.0],
                        [gap + length - depth, thickness / 2.0],
                        [gap, thickness / 2.0],
                        [gap + depth, 0.0],
                    ];
                    for (i, point) in points.iter().enumerate() {
                        let x = if *inward {
                            2.0 * gap + length - point[0]
                        } else {
                            point[0]
                        };
                        if i == 0 {
                            path.move_to(x, point[1]);
                        } else {
                            path.line_to(x, point[1]);
                        }
                    }
                    path.close();
                    path.finish()
                        .map(|p| element(p, crosshair.color, true))
                        .into_iter()
                        .collect()
                }
                ArmShape::Arc { radius, sweep } => {
                    let sweep = sweep.to_radians();
                    let segments = ((radius * sweep.abs() / 2.0).ceil() as usize).clamp(8, 256);
                    let mut path = PathBuilder::new();
                    for n in 0..=segments {
                        let angle = -sweep / 2.0 + sweep * n as f32 / segments as f32;
                        let x = gap + radius * (1.0 - angle.cos());
                        let y = radius * angle.sin();
                        if n == 0 {
                            path.move_to(x, y);
                        } else {
                            path.line_to(x, y);
                        }
                    }
                    path.finish()
                        .and_then(|path| {
                            path.stroke(
                                &Stroke {
                                    width: thickness,
                                    line_cap: LineCap::Butt,
                                    ..Default::default()
                                },
                                1.0,
                            )
                        })
                        .map(|p| element(p, crosshair.color, true))
                        .into_iter()
                        .collect()
                }
                ArmShape::TShape {
                    width,
                    thickness: bar,
                } => {
                    let mut paths = Vec::new();
                    if let Some(rect) = Rect::from_xywh(gap, -thickness / 2.0, length, thickness) {
                        paths.push(element(
                            PathBuilder::from_rect(rect),
                            crosshair.color,
                            false,
                        ));
                    }
                    if let Some(rect) =
                        Rect::from_xywh(gap + length - bar, -width / 2.0, *bar, *width)
                    {
                        paths.push(element(
                            PathBuilder::from_rect(rect),
                            crosshair.color,
                            false,
                        ));
                    }
                    paths
                }
                ArmShape::Text(_) => text_arm
                    .as_ref()
                    .expect("text arm prepared")
                    .clone()
                    .into_iter()
                    .map(|p| p.transform(Transform::from_translate(gap + length / 2.0, 0.0)))
                    .collect::<Result<Vec<_>>>()?,
                ArmShape::Image(image) => {
                    if image.width > 0.0 && image.height > 0.0 {
                        vec![Element::Image {
                            image: image.clone(),
                            transform: Transform::from_translate(gap + image.width / 2.0, 0.0),
                        }]
                    } else {
                        Vec::new()
                    }
                }
            };
            for primitive in primitives.drain(..) {
                if let Element::Path { path, .. } = &primitive {
                    point_count = point_count.saturating_add(path.points().len());
                    ensure!(
                        point_count <= 1_000_000,
                        "Crosshair exceeds one million vector points"
                    );
                }
                arms.push(primitive.transform(rotation)?);
            }
        }
    }
    // A crosshair is one fill. Merging its contours prevents darker seams where
    // semitransparent arms or the two rectangles of a T overlap.
    if let Some(Element::Path { color, .. }) = arms.first() {
        let color = *color;
        let mut path = PathBuilder::new();
        let mut antialias = false;
        for arm in &arms {
            if let Element::Path {
                path: contour,
                antialias: aa,
                ..
            } = arm
            {
                path.push_path(contour);
                antialias |= aa;
            }
        }
        arms = path
            .finish()
            .map(|path| element(path, color, antialias))
            .into_iter()
            .collect();
    }
    let mut groups = vec![DrawGroup {
        elements: arms,
        blur: crosshair.blur,
    }];
    if crosshair.dot.enabled && crosshair.dot.diameter > 0.0 {
        let diameter = crosshair.dot.diameter;
        let (path, antialias) = match crosshair.dot.shape {
            DotShape::Circle => (ellipse(diameter, diameter), true),
            DotShape::Square => (rounded_rect(diameter, diameter, 0.0), false),
        };
        if let Some(path) = path {
            groups.push(DrawGroup {
                elements: vec![element(path, crosshair.dot.color, antialias)],
                blur: crosshair.dot.blur,
            });
        }
    }
    Ok(groups)
}

fn ellipse(width: f32, height: f32) -> Option<Path> {
    let rect = Rect::from_xywh(-width / 2.0, -height / 2.0, width, height)?;
    PathBuilder::from_oval(rect)
}

fn triangle(width: f32, height: f32) -> Option<Path> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let mut path = PathBuilder::new();
    path.move_to(0.0, -height / 2.0);
    path.line_to(width / 2.0, height / 2.0);
    path.line_to(-width / 2.0, height / 2.0);
    path.close();
    path.finish()
}

fn rounded_rect(width: f32, height: f32, radius: f32) -> Option<Path> {
    let rect = Rect::from_xywh(-width / 2.0, -height / 2.0, width, height)?;
    let radius = radius.min(width / 2.0).min(height / 2.0);
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    let k = 0.552_284_8 * radius;
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let mut p = PathBuilder::new();
    p.move_to(l + radius, t);
    p.line_to(r - radius, t);
    p.cubic_to(r - radius + k, t, r, t + radius - k, r, t + radius);
    p.line_to(r, b - radius);
    p.cubic_to(r, b - radius + k, r - radius + k, b, r - radius, b);
    p.line_to(l + radius, b);
    p.cubic_to(l + radius - k, b, l, b - radius + k, l, b - radius);
    p.line_to(l, t + radius);
    p.cubic_to(l, t + radius - k, l + radius - k, t, l + radius, t);
    p.close();
    p.finish()
}
