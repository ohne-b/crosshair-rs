use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use crosshair_core::*;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub enum FieldKind {
    Section,
    Text,
    Number { min: f32, max: f32, integer: bool },
    Toggle,
    Color,
    Choice(Vec<(String, String)>),
    Action,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub value: String,
    pub kind: FieldKind,
}

impl Field {
    fn section(label: &str) -> Self {
        Self {
            key: label.into(),
            label: label.into(),
            value: String::new(),
            kind: FieldKind::Section,
        }
    }
    fn text(key: &str, label: &str, value: &str) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value: value.into(),
            kind: FieldKind::Text,
        }
    }
    fn number(key: &str, label: &str, value: f32, min: f32, max: f32) -> Self {
        let integer = !matches!(key, "/transform/scale" | "/opacity")
            && !key.contains("/crop/")
            && !key.ends_with("/rotation")
            && !key.ends_with("/start_angle")
            && !key.ends_with("/sweep");
        Self {
            key: key.into(),
            label: label.into(),
            value: format_number(value),
            kind: FieldKind::Number {
                min: if integer { min.ceil() } else { min },
                max,
                integer,
            },
        }
    }
    fn integer(key: &str, label: &str, value: u16, min: f32, max: f32) -> Self {
        let mut field = Self::number(key, label, value as f32, min, max);
        field.kind = FieldKind::Number {
            min,
            max,
            integer: true,
        };
        field
    }
    fn toggle(key: &str, label: &str, value: bool) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value: value.to_string(),
            kind: FieldKind::Toggle,
        }
    }
    fn color(key: &str, label: &str, color: Color) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value: format!("#{}{:02X}", color.hex_rgb(), color.a),
            kind: FieldKind::Color,
        }
    }
    fn choice(key: &str, label: &str, value: &str, choices: &[(&str, &str)]) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value: value.into(),
            kind: FieldKind::Choice(
                choices
                    .iter()
                    .map(|(value, label)| ((*value).into(), (*label).into()))
                    .collect(),
            ),
        }
    }
}

pub fn format_number(value: f32) -> String {
    let formatted = format!("{value:.2}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

pub fn same_design(a: &Preset, b: &Preset) -> bool {
    a.id == b.id && a.name == b.name && a.layers == b.layers
}

const ANCHORS: &[(&str, &str)] = &[
    ("top_left", "Top left"),
    ("top", "Top"),
    ("top_right", "Top right"),
    ("left", "Left"),
    ("center", "Center"),
    ("right", "Right"),
    ("bottom_left", "Bottom left"),
    ("bottom", "Bottom"),
    ("bottom_right", "Bottom right"),
];
const ARM_SHAPES: &[(&str, &str)] = &[
    ("rectangle", "Rectangle"),
    ("triangle", "Triangle"),
    ("chevron_in", "Chevron in"),
    ("chevron_out", "Chevron out"),
    ("arc", "Arc"),
    ("t_shape", "T-shape"),
    ("image", "Image"),
];

pub fn fields(layer: &Layer, fonts: &[String], common_only: bool) -> Vec<Field> {
    let t = layer.transform;
    let anchor = serde_json::to_value(t.anchor).unwrap_or_default();
    let mut rows = vec![
        Field::text("/name", "Name", &layer.name),
        Field::section("Transform"),
        Field::number("/transform/x", "X", t.x, -4096.0, 4096.0),
        Field::number("/transform/y", "Y", t.y, -4096.0, 4096.0),
        Field::number("/transform/scale", "Scale", t.scale, 0.01, 100.0),
        Field::number("/transform/rotation", "Rotation", t.rotation, -360.0, 360.0),
        Field::number("/opacity", "Opacity", layer.opacity, 0.0, 1.0),
        Field::choice(
            "/transform/anchor",
            "Anchor",
            anchor.as_str().unwrap_or("center"),
            ANCHORS,
        ),
    ];
    if !common_only {
        content_fields(&layer.content, fonts, &mut rows);
    }
    rows.extend([
        Field::section("Effects"),
        Field::number("/effects/blur", "Blur", layer.effects.blur, 0.0, 64.0),
        Field::toggle(
            "/effects/outline/enabled",
            "Outline",
            layer.effects.outline.enabled,
        ),
    ]);
    if layer.effects.outline.enabled {
        rows.extend([
            Field::color(
                "/effects/outline/color",
                "Color",
                layer.effects.outline.color,
            ),
            Field::number(
                "/effects/outline/thickness",
                "Thickness",
                layer.effects.outline.thickness,
                0.0,
                64.0,
            ),
            Field::number(
                "/effects/outline/blur",
                "Outline blur",
                layer.effects.outline.blur,
                0.0,
                64.0,
            ),
        ]);
    }
    rows
}

fn content_fields(content: &Content, fonts: &[String], rows: &mut Vec<Field>) {
    let p = "/content/properties";
    match content {
        Content::Crosshair(crosshair) => {
            rows.push(Field::section("Lines"));
            let shape = match &crosshair.shape {
                ArmShape::Rectangle => "rectangle",
                ArmShape::Triangle => "triangle",
                ArmShape::Chevron { inward: true, .. } => "chevron_in",
                ArmShape::Chevron { inward: false, .. } => "chevron_out",
                ArmShape::Arc { .. } => "arc",
                ArmShape::TShape { .. } => "t_shape",
                ArmShape::Text(_) => "text",
                ArmShape::Image(_) => "image",
            };
            rows.extend([
                Field::choice(
                    "@arm_shape",
                    "Style",
                    shape,
                    &if shape == "text" {
                        let mut choices = ARM_SHAPES.to_vec();
                        choices.push(("text", "Existing text"));
                        choices
                    } else {
                        ARM_SHAPES.to_vec()
                    },
                ),
                Field::integer(
                    &format!("{p}/arms"),
                    "Arms",
                    crosshair.arms.into(),
                    1.0,
                    32.0,
                ),
                Field::number(
                    &format!("{p}/length"),
                    "Length",
                    crosshair.length,
                    0.0,
                    512.0,
                ),
                Field::number(
                    &format!("{p}/thickness"),
                    "Thickness",
                    crosshair.thickness,
                    0.0,
                    512.0,
                ),
                Field::number(&format!("{p}/gap"), "Gap", crosshair.gap, 0.0, 512.0),
                Field::number(
                    &format!("{p}/rotation"),
                    "Rotation",
                    crosshair.rotation,
                    -360.0,
                    360.0,
                ),
                Field::color(&format!("{p}/color"), "Color", crosshair.color),
                Field::number(&format!("{p}/blur"), "Line blur", crosshair.blur, 0.0, 64.0),
            ]);
            for (index, enabled) in crosshair.enabled_arms.iter().enumerate() {
                rows.push(Field::toggle(
                    &format!("{p}/enabled_arms/{index}"),
                    &format!("Arm {}", index + 1),
                    *enabled,
                ));
            }
            match &crosshair.shape {
                ArmShape::Chevron { depth, .. } => rows.push(Field::number(
                    &format!("{p}/shape/properties/depth"),
                    "Depth",
                    *depth,
                    0.0,
                    512.0,
                )),
                ArmShape::Arc { radius, sweep } => rows.extend([
                    Field::number(
                        &format!("{p}/shape/properties/radius"),
                        "Radius",
                        *radius,
                        0.0,
                        512.0,
                    ),
                    Field::number(
                        &format!("{p}/shape/properties/sweep"),
                        "Sweep",
                        *sweep,
                        0.0,
                        360.0,
                    ),
                ]),
                ArmShape::TShape { width, thickness } => rows.extend([
                    Field::number(
                        &format!("{p}/shape/properties/width"),
                        "Bar width",
                        *width,
                        0.0,
                        1024.0,
                    ),
                    Field::number(
                        &format!("{p}/shape/properties/thickness"),
                        "Bar thickness",
                        *thickness,
                        0.0,
                        512.0,
                    ),
                ]),
                ArmShape::Text(text) => {
                    text_fields(text, &format!("{p}/shape/properties"), fonts, rows)
                }
                ArmShape::Image(image) => {
                    image_fields(image, &format!("{p}/shape/properties"), rows)
                }
                _ => {}
            }
            rows.extend([
                Field::section("Center dot"),
                Field::toggle(&format!("{p}/dot/enabled"), "Dot", crosshair.dot.enabled),
            ]);
            if crosshair.dot.enabled {
                rows.extend([
                    Field::choice(
                        &format!("{p}/dot/shape"),
                        "Shape",
                        if crosshair.dot.shape == DotShape::Circle {
                            "circle"
                        } else {
                            "square"
                        },
                        &[("circle", "Circle"), ("square", "Square")],
                    ),
                    Field::number(
                        &format!("{p}/dot/diameter"),
                        "Diameter",
                        crosshair.dot.diameter,
                        0.0,
                        1024.0,
                    ),
                    Field::color(&format!("{p}/dot/color"), "Color", crosshair.dot.color),
                    Field::number(
                        &format!("{p}/dot/blur"),
                        "Dot blur",
                        crosshair.dot.blur,
                        0.0,
                        64.0,
                    ),
                ]);
            }
        }
        Content::Rectangle(rectangle) => {
            rows.push(Field::section("Rectangle"));
            size_fields(rectangle.width, rectangle.height, rectangle.color, p, rows);
            rows.push(Field::number(
                &format!("{p}/radius"),
                "Corner radius",
                rectangle.radius,
                0.0,
                512.0,
            ));
        }
        Content::Ellipse(shape) | Content::Triangle(shape) => {
            rows.push(Field::section(content.kind_name()));
            size_fields(shape.width, shape.height, shape.color, p, rows);
        }
        Content::Text(text) => {
            rows.push(Field::section("Text"));
            text_fields(text, p, fonts, rows);
        }
        Content::Image(image) => {
            rows.push(Field::section("Image"));
            image_fields(image, p, rows);
        }
        Content::Freehand(_) => {}
    }
}

fn size_fields(width: f32, height: f32, color: Color, prefix: &str, rows: &mut Vec<Field>) {
    rows.extend([
        Field::number(&format!("{prefix}/width"), "Width", width, 0.0, 1024.0),
        Field::number(&format!("{prefix}/height"), "Height", height, 0.0, 1024.0),
        Field::color(&format!("{prefix}/color"), "Color", color),
    ]);
}

fn text_fields(text: &Text, prefix: &str, fonts: &[String], rows: &mut Vec<Field>) {
    rows.push(Field::text(
        &format!("{prefix}/content"),
        "Text",
        &text.content,
    ));
    let mut font = Field::text(&format!("{prefix}/font_family"), "Font", &text.font_family);
    if !fonts.is_empty() {
        font.kind = FieldKind::Choice(
            fonts
                .iter()
                .map(|name| (name.clone(), name.clone()))
                .collect(),
        );
    }
    rows.extend([
        font,
        Field::integer(
            &format!("{prefix}/font_weight"),
            "Weight",
            text.font_weight,
            1.0,
            999.0,
        ),
        Field::toggle(&format!("{prefix}/italic"), "Italic", text.italic),
        Field::number(&format!("{prefix}/size"), "Size", text.size, 1.0, 1024.0),
        Field::number(
            &format!("{prefix}/spacing"),
            "Spacing",
            text.spacing,
            -100.0,
            100.0,
        ),
        Field::color(&format!("{prefix}/color"), "Color", text.color),
        Field::choice(
            &format!("@layout:{prefix}"),
            "Layout",
            if matches!(text.layout, TextLayout::Straight) {
                "straight"
            } else {
                "circle"
            },
            &[("straight", "Straight"), ("circle", "Circular")],
        ),
    ]);
    if let TextLayout::Circle {
        radius,
        start_angle,
        repeat,
    } = text.layout
    {
        rows.extend([
            Field::number(
                &format!("{prefix}/layout/radius"),
                "Radius",
                radius,
                1.0,
                512.0,
            ),
            Field::number(
                &format!("{prefix}/layout/start_angle"),
                "Start angle",
                start_angle,
                -360.0,
                360.0,
            ),
            Field::toggle(&format!("{prefix}/layout/repeat"), "Repeat to fill", repeat),
        ]);
    }
}

fn image_fields(image: &Image, prefix: &str, rows: &mut Vec<Field>) {
    rows.extend([
        Field {
            key: format!("@image:{prefix}"),
            label: "Replace image".into(),
            value: String::new(),
            kind: FieldKind::Action,
        },
        Field::number(
            &format!("{prefix}/width"),
            "Width",
            image.width,
            0.0,
            1024.0,
        ),
        Field::number(
            &format!("{prefix}/height"),
            "Height",
            image.height,
            0.0,
            1024.0,
        ),
        Field::toggle(
            &format!("{prefix}/aspect_locked"),
            "Lock aspect",
            image.aspect_locked,
        ),
        Field::number(
            &format!("{prefix}/crop/x"),
            "Crop left",
            image.crop.x,
            0.0,
            1.0,
        ),
        Field::number(
            &format!("{prefix}/crop/y"),
            "Crop top",
            image.crop.y,
            0.0,
            1.0,
        ),
        Field::number(
            &format!("{prefix}/crop/width"),
            "Crop width",
            image.crop.width,
            0.01,
            1.0,
        ),
        Field::number(
            &format!("{prefix}/crop/height"),
            "Crop height",
            image.crop.height,
            0.01,
            1.0,
        ),
    ]);
}

/// The editable ID stays pinned even when global input selects another preset.
#[derive(Default)]
pub struct Document {
    pub draft: Option<Preset>,
    pub selected: BTreeSet<Id>,
    pub dirty: bool,
    pub save_paused: bool,
    pub conflict: bool,
    pub gesture: Option<Preset>,
    gesture_dirty: bool,
    pub clipboard: Vec<Layer>,
    pub saved: Option<Preset>,
    undo: Vec<(Preset, usize)>,
    redo: Vec<(Preset, usize)>,
    history_bytes: usize,
}

impl Document {
    pub fn reconnected(&mut self) {
        self.save_paused = self.dirty;
    }

    pub fn open(&mut self, preset: Preset) -> Result<()> {
        ensure!(
            !self.dirty,
            "Resolve the unsent edit before opening another crosshair"
        );
        self.selected.clear();
        if let Some(layer) = preset.layers.last() {
            self.selected.insert(layer.id);
        }
        self.saved = Some(preset.clone());
        self.draft = Some(preset);
        self.undo.clear();
        self.redo.clear();
        self.history_bytes = 0;
        self.gesture = None;
        self.save_paused = false;
        self.conflict = false;
        Ok(())
    }
    pub fn receive(&mut self, preset: &Preset) {
        if let Some(draft) = self.draft.as_mut().filter(|draft| draft.id == preset.id)
            && !self.dirty
            && self.gesture.is_none()
        {
            *draft = preset.clone();
            self.saved = Some(preset.clone());
            self.selected
                .retain(|id| preset.layers.iter().any(|layer| layer.id == *id));
        }
    }
    pub fn acknowledge(&mut self, sent: &Preset, revision: u64) {
        let mut saved = sent.clone();
        saved.revision = revision;
        self.saved = Some(saved);
        if let Some(draft) = self.draft.as_mut().filter(|draft| draft.id == sent.id) {
            self.dirty = draft.layers != sent.layers || draft.name != sent.name;
            draft.revision = revision;
            self.conflict = false;
        }
    }
    pub fn begin_gesture(&mut self) {
        if self.gesture.is_none() {
            self.gesture = self.draft.clone();
            self.gesture_dirty = self.dirty;
        }
    }
    pub fn finish_gesture(&mut self) {
        if let Some(before) = self.gesture.take()
            && self
                .draft
                .as_ref()
                .is_some_and(|draft| !same_design(draft, &before))
        {
            let bytes = serde_json::to_vec(&before).map_or(MAX_CONFIG_BYTES, |data| data.len());
            self.history_bytes += bytes;
            self.history_bytes = self
                .history_bytes
                .saturating_sub(self.redo.iter().map(|(_, bytes)| bytes).sum());
            self.undo.push((before, bytes));
            self.redo.clear();
            while self.undo.len() > 1
                && (self.undo.len() > 100 || self.history_bytes > 32 * 1024 * 1024)
            {
                let (_, bytes) = self.undo.remove(0);
                self.history_bytes = self.history_bytes.saturating_sub(bytes);
            }
        }
        self.update_dirty();
    }
    pub fn update_dirty(&mut self) {
        self.dirty = match (&self.draft, &self.saved) {
            (Some(draft), Some(saved)) => !same_design(draft, saved),
            (Some(_), None) => true,
            _ => false,
        };
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self, redo: bool) {
        self.finish_gesture();
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if let Some((mut previous, bytes)) = from.pop() {
            self.history_bytes = self.history_bytes.saturating_sub(bytes);
            if let Some(current) = self.draft.take() {
                previous.revision = current.revision;
                previous.favorite = current.favorite;
                previous.modified_order = current.modified_order;
                let bytes =
                    serde_json::to_vec(&current).map_or(MAX_CONFIG_BYTES, |data| data.len());
                self.history_bytes += bytes;
                to.push((current, bytes));
            }
            self.selected
                .retain(|id| previous.layers.iter().any(|layer| layer.id == *id));
            self.draft = Some(previous);
            self.update_dirty();
        }
    }
    pub fn asset_ids(&self) -> BTreeSet<Id> {
        self.draft
            .iter()
            .chain(self.undo.iter().map(|(preset, _)| preset))
            .chain(self.redo.iter().map(|(preset, _)| preset))
            .chain(self.gesture.iter())
            .flat_map(|preset| &preset.layers)
            .chain(self.clipboard.iter())
            .filter_map(|layer| layer.content.asset_id())
            .collect()
    }
    pub fn cancel_gesture(&mut self, state: &AppState) {
        if let Some(before) = self.gesture.take() {
            self.draft = Some(before);
            self.dirty = self.gesture_dirty;
            self.save_paused &= self.dirty;
            if !self.dirty && self.conflict {
                self.draft = self
                    .draft
                    .as_ref()
                    .and_then(|draft| state.preset(draft.id))
                    .cloned();
                self.selected.retain(|id| {
                    self.draft
                        .as_ref()
                        .is_some_and(|draft| draft.layers.iter().any(|layer| layer.id == *id))
                });
                self.conflict = false;
            }
        }
    }
    pub fn selected_layer(&self) -> Option<&Layer> {
        self.draft
            .as_ref()?
            .layers
            .iter()
            .find(|layer| self.selected.contains(&layer.id))
    }
    pub fn select(&mut self, id: Id, extend: bool) {
        if !extend {
            self.selected.clear();
        }
        if !self.selected.insert(id) && extend {
            self.selected.remove(&id);
        }
    }
    pub fn copy_selection(&mut self) {
        self.clipboard = self
            .draft
            .as_ref()
            .map(|preset| {
                preset
                    .layers
                    .iter()
                    .filter(|layer| self.selected.contains(&layer.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
    }
}

pub fn apply_field(layer: &mut Layer, key: &str, input: &str, fonts: &[String]) -> Result<()> {
    ensure!(!layer.locked, "Unlock this layer to edit it");
    let field = fields(layer, fonts, false)
        .into_iter()
        .find(|field| field.key == key)
        .context("This field does not belong to the selected layer")?;
    if field.value == input && (key == "@arm_shape" || key.starts_with("@layout:")) {
        return Ok(());
    }
    if key == "@arm_shape" {
        let Content::Crosshair(crosshair) = &mut layer.content else {
            bail!("Select a crosshair layer");
        };
        crosshair.shape = match input {
            "rectangle" => ArmShape::Rectangle,
            "triangle" => ArmShape::Triangle,
            "chevron_in" => ArmShape::Chevron {
                inward: true,
                depth: 3.0,
            },
            "chevron_out" => ArmShape::Chevron {
                inward: false,
                depth: 3.0,
            },
            "arc" => ArmShape::Arc {
                radius: 10.0,
                sweep: 90.0,
            },
            "t_shape" => ArmShape::TShape {
                width: 8.0,
                thickness: 2.0,
            },
            "image" => bail!("Choose an image file for this arm shape"),
            _ => bail!("Unknown arm shape"),
        };
        return Ok(());
    }
    let value = match field.kind {
        FieldKind::Number { min, max, integer } => {
            let number: f32 = input.trim().parse().context("Enter a number")?;
            ensure!(
                number.is_finite() && (min..=max).contains(&number),
                "Enter a value from {min} to {max}"
            );
            if integer {
                ensure!(number.fract() == 0.0, "Enter a whole number");
                json!(number as i32)
            } else {
                json!(number)
            }
        }
        FieldKind::Toggle => json!(input.parse::<bool>().context("Invalid switch value")?),
        FieldKind::Color => serde_json::to_value(
            Color::from_hex(input).context("Use six or eight hexadecimal digits")?,
        )?,
        FieldKind::Choice(choices) => {
            ensure!(
                choices.iter().any(|(value, _)| value == input),
                "Choose a valid option"
            );
            json!(input)
        }
        FieldKind::Text => json!(input),
        _ => bail!("This field is not editable"),
    };
    let mut encoded = serde_json::to_value(&*layer)?;
    if let Some(prefix) = key.strip_prefix("@opacity:") {
        *encoded
            .pointer_mut(&format!("{prefix}/a"))
            .context("Color opacity is missing")? =
            json!((input.parse::<f32>()? * 255.0 / 100.0).round() as u8);
    } else if let Some(prefix) = key.strip_prefix("@layout:") {
        *encoded
            .pointer_mut(&format!("{prefix}/layout"))
            .context("Text layout is missing")? = if input == "circle" {
            json!({"type":"circle","radius":24.0,"start_angle":0.0,"repeat":false})
        } else {
            json!({"type":"straight"})
        };
    } else {
        *encoded
            .pointer_mut(key)
            .context("The field is unavailable")? = value;
    }
    let mut changed: Layer = serde_json::from_value(encoded)?;
    if let Content::Crosshair(crosshair) = &mut changed.content {
        crosshair.enabled_arms.resize(crosshair.arms as usize, true);
    }
    preserve_image_aspect(&layer.content, &mut changed.content, key);
    validation::validate_layer_geometry(&changed)?;
    *layer = changed;
    Ok(())
}

fn preserve_image_aspect(before: &Content, after: &mut Content, key: &str) {
    let pair = match (before, after) {
        (Content::Image(before), Content::Image(after)) => Some((before, after)),
        (
            Content::Crosshair(Crosshair {
                shape: ArmShape::Image(before),
                ..
            }),
            Content::Crosshair(Crosshair {
                shape: ArmShape::Image(after),
                ..
            }),
        ) => Some((before, after)),
        _ => None,
    };
    if let Some((before, after)) =
        pair.filter(|(before, _)| before.aspect_locked && before.width > 0.0 && before.height > 0.0)
    {
        if key.ends_with("/width") && !key.contains("/crop/") {
            after.height = before.height * after.width / before.width;
        }
        if key.ends_with("/height") && !key.contains("/crop/") {
            after.width = before.width * after.height / before.height;
        }
    }
}

pub fn add_layer(preset: &mut Preset, kind: &str) -> Result<Id> {
    let content = match kind {
        "crosshair" => Content::Crosshair(Crosshair::default()),
        "rectangle" => Content::Rectangle(Rectangle::default()),
        "ellipse" => Content::Ellipse(Shape::default()),
        "triangle" => Content::Triangle(Shape::default()),
        "draw" => Content::Freehand(Freehand::default()),
        _ => bail!("Choose a layer type"),
    };
    ensure!(
        preset.layers.len() < MAX_LAYERS,
        "A crosshair can contain at most {MAX_LAYERS} layers"
    );
    let layer = Layer::new(content.kind_name(), content);
    let id = layer.id;
    preset.layers.push(layer);
    Ok(id)
}

pub fn reorder(preset: &mut Preset, selected: &BTreeSet<Id>, operation: &str) {
    match operation {
        "front" | "back" => preset.layers.sort_by_key(|layer| {
            let selected = selected.contains(&layer.id) && !layer.locked;
            if operation == "front" {
                selected
            } else {
                !selected
            }
        }),
        "up" => {
            for index in (0..preset.layers.len().saturating_sub(1)).rev() {
                if selected.contains(&preset.layers[index].id)
                    && !preset.layers[index].locked
                    && !selected.contains(&preset.layers[index + 1].id)
                {
                    preset.layers.swap(index, index + 1);
                }
            }
        }
        "down" => {
            for index in 1..preset.layers.len() {
                if selected.contains(&preset.layers[index].id)
                    && !preset.layers[index].locked
                    && !selected.contains(&preset.layers[index - 1].id)
                {
                    preset.layers.swap(index, index - 1);
                }
            }
        }
        _ => {}
    }
}

pub fn nudge(preset: &mut Preset, selected: &BTreeSet<Id>, x: f32, y: f32) {
    for layer in preset
        .layers
        .iter_mut()
        .filter(|layer| selected.contains(&layer.id) && !layer.locked)
    {
        layer.transform.x += x;
        layer.transform.y += y;
    }
}

pub fn transform_around(
    preset: &mut Preset,
    selected: &BTreeSet<Id>,
    anchors: &BTreeMap<Id, Point>,
    center: Point,
    scale: f32,
    rotation: f32,
) -> Result<()> {
    let (sin, cos) = rotation.to_radians().sin_cos();
    for layer in preset
        .layers
        .iter_mut()
        .filter(|layer| selected.contains(&layer.id) && !layer.locked)
    {
        let pivot = anchors
            .get(&layer.id)
            .context("Wait for the layer preview before transforming it")?;
        let x = layer.transform.x + pivot.x - center.x;
        let y = layer.transform.y + pivot.y - center.y;
        layer.transform.x = center.x + scale * (x * cos - y * sin) - pivot.x;
        layer.transform.y = center.y + scale * (x * sin + y * cos) - pivot.y;
        layer.transform.scale *= scale;
        layer.transform.rotation =
            (layer.transform.rotation + rotation + 360.0).rem_euclid(720.0) - 360.0;
    }
    Ok(())
}

pub fn resize_scale(origin: Point, point: Point, center: Point, span: f32, coarse: bool) -> f32 {
    let x = origin.x - center.x;
    let y = origin.y - center.y;
    // Signed projection keeps a handle dragged past the center from growing again.
    let mut ratio =
        1.0 + ((point.x - origin.x) * x + (point.y - origin.y) * y) / (x * x + y * y).max(0.0001);
    if coarse {
        ratio = (ratio * 10.0).round() / 10.0;
    }
    let span = span.max(1.0);
    (span + (span * (ratio - 1.0)).round()).clamp(1.0, crosshair_renderer::MAX_DIMENSION as f32)
        / span
}

pub fn parse_clipboard(text: &str, assets: &BTreeMap<Id, Asset>) -> Result<Vec<Layer>> {
    ensure!(
        text.len() <= MAX_CONFIG_BYTES,
        "Clipboard exceeds the document limit"
    );
    let value: Value =
        serde_json::from_str(text).context("The clipboard does not contain crosshair layers")?;
    ensure!(
        value["format"] == "crosshair-rs-layers-1",
        "The clipboard does not contain crosshair layers"
    );
    let layers: Vec<Layer> = serde_json::from_value(value["layers"].clone())?;
    ensure!(
        !layers.is_empty() && layers.len() <= MAX_LAYERS,
        "Invalid layer count"
    );
    for layer in &layers {
        validation::validate_layer(layer, assets)?;
    }
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_hide_float_noise_without_trailing_zeroes() {
        assert_eq!(format_number(0.78999996), "0.79");
        assert_eq!(format_number(28.0), "28");
        assert_eq!(format_number(-1.25), "-1.25");
    }

    #[test]
    fn manual_edits_keep_saved_design_and_undo_preserves_save_revision() {
        let original = Preset::new("Manual");
        let mut document = Document::default();
        document.open(original.clone()).unwrap();
        for x in [5.0, 12.0] {
            document.begin_gesture();
            document.draft.as_mut().unwrap().layers[0].transform.x = x;
            document.finish_gesture();
        }
        assert_eq!(document.saved.as_ref(), Some(&original));
        assert!(document.dirty);
        document.undo(false);
        assert_eq!(document.draft.as_ref().unwrap().layers[0].transform.x, 5.0);
        document.undo(true);
        let saved = document.draft.clone().unwrap();
        document.acknowledge(&saved, 7);
        assert!(!document.dirty);
        document.undo(false);
        assert!(document.dirty);
        assert_eq!(document.draft.as_ref().unwrap().revision, 7);
        document.undo(true);
        assert!(!document.dirty);
        assert_eq!(document.draft.as_ref().unwrap().revision, 7);
    }

    #[test]
    fn local_history_is_bounded_and_new_edit_discards_redo() {
        let mut document = Document::default();
        document.open(Preset::new("History")).unwrap();
        for x in 1..=105 {
            document.begin_gesture();
            document.draft.as_mut().unwrap().layers[0].transform.x = x as f32;
            document.finish_gesture();
        }
        assert_eq!(document.undo.len(), 100);
        assert_eq!(
            document.history_bytes,
            document.undo.iter().map(|(_, bytes)| bytes).sum::<usize>()
        );
        document.undo(false);
        assert!(document.can_redo());
        document.begin_gesture();
        document.draft.as_mut().unwrap().name = "Changed".into();
        document.finish_gesture();
        assert!(!document.can_redo());
        assert_eq!(
            document.history_bytes,
            document.undo.iter().map(|(_, bytes)| bytes).sum::<usize>()
        );
    }

    #[test]
    fn pixel_fields_reject_fractions_and_allow_negative_integer_positions() {
        let mut layer = Preset::new("Pixels").layers.remove(0);
        apply_field(&mut layer, "/transform/x", "-7", &[]).unwrap();
        assert_eq!(layer.transform.x, -7.0);
        assert!(apply_field(&mut layer, "/transform/x", "-7.5", &[]).is_err());
        apply_field(&mut layer, "/transform/scale", "0.25", &[]).unwrap();
        let fields = fields(&layer, &[], false);
        assert!(
            !fields
                .iter()
                .any(|field| field.key.starts_with("@opacity:"))
        );
    }
    #[test]
    fn switching_active_preset_cannot_retarget_dirty_draft() {
        let state = AppState::default();
        let mut document = Document::default();
        document.open(state.presets[0].clone()).unwrap();
        document.dirty = true;
        assert!(document.open(state.presets[1].clone()).is_err());
        document.receive(&state.presets[1]);
        assert_eq!(document.draft.unwrap().id, state.presets[0].id);
    }
    #[test]
    fn invalid_field_keeps_document_and_arm_count_keeps_flags_consistent() {
        let mut layer = Layer::new("Lines", Content::Crosshair(Crosshair::default()));
        let original = layer.clone();
        assert!(apply_field(&mut layer, "/content/properties/length", "NaN", &[]).is_err());
        assert_eq!(layer, original);
        apply_field(&mut layer, "/content/properties/arms", "7", &[]).unwrap();
        let Content::Crosshair(crosshair) = layer.content else {
            panic!()
        };
        assert_eq!(crosshair.enabled_arms.len(), 7);
    }
    #[test]
    fn cancelling_a_gesture_preserves_an_older_unsent_draft() {
        let mut document = Document::default();
        document
            .open(AppState::default().presets.remove(0))
            .unwrap();
        document.draft.as_mut().unwrap().name = "Unsent name".into();
        document.dirty = true;
        document.save_paused = true;
        document.begin_gesture();
        document.draft.as_mut().unwrap().layers[0].transform.x = 20.0;
        document.cancel_gesture(&AppState::default());
        assert!(document.dirty);
        assert!(document.save_paused);
        let draft = document.draft.as_ref().unwrap();
        assert_eq!(draft.name, "Unsent name");
        assert_eq!(draft.layers[0].transform.x, 0.0);
        let mut saved = draft.clone();
        saved.name = "A newer saved name".into();
        saved.revision += 1;
        document.receive(&saved);
        assert_eq!(document.draft.as_ref().unwrap().name, "Unsent name");
        document.reconnected();
        assert!(document.save_paused);
    }

    #[test]
    fn reconnecting_after_cancelling_a_clean_offline_gesture_resumes_saving() {
        let mut document = Document::default();
        document
            .open(AppState::default().presets.remove(0))
            .unwrap();
        document.save_paused = true;
        document.begin_gesture();
        document.draft.as_mut().unwrap().layers[0].transform.x = 20.0;
        document.dirty = true;
        document.cancel_gesture(&AppState::default());
        assert!(!document.dirty);
        document.reconnected();
        assert!(!document.save_paused);

        document.begin_gesture();
        document.draft.as_mut().unwrap().layers[0].transform.x = 20.0;
        document.dirty = true;
        document.reconnected();
        assert!(document.save_paused);
        document.cancel_gesture(&AppState::default());
        assert!(!document.dirty && !document.save_paused);
    }

    #[test]
    fn acknowledgement_preserves_newer_unsent_edit() {
        let mut document = Document::default();
        document
            .open(AppState::default().presets.remove(0))
            .unwrap();
        let sent = document.draft.clone().unwrap();
        document.draft.as_mut().unwrap().name = "Newer".into();
        document.acknowledge(&sent, 4);
        assert!(document.dirty);
        assert_eq!(document.draft.unwrap().revision, 4);
    }

    #[test]
    fn cancelling_a_reconnected_conflict_reconciles_only_a_clean_starting_draft() {
        for earlier_unsent in [false, true] {
            let mut state = AppState::default();
            let mut document = Document::default();
            document.open(state.presets[0].clone()).unwrap();
            if earlier_unsent {
                document.draft.as_mut().unwrap().name = "Earlier unsent edit".into();
                document.dirty = true;
            }
            document.begin_gesture();
            document.draft.as_mut().unwrap().layers[0].transform.x = 20.0;
            document.dirty = true;
            document.reconnected();
            state.presets[0].revision += 1;
            state.presets[0].layers[0].transform.x = 10.0;
            document.conflict = true;
            document.receive(&state.presets[0]);
            document.cancel_gesture(&state);
            if earlier_unsent {
                assert!(document.dirty && document.save_paused && document.conflict);
                assert_eq!(document.draft.as_ref().unwrap().name, "Earlier unsent edit");
                assert_eq!(document.draft.as_ref().unwrap().layers[0].transform.x, 0.0);
            } else {
                assert!(!document.dirty && !document.save_paused && !document.conflict);
                assert_eq!(document.draft.as_ref(), Some(&state.presets[0]));
            }
        }
    }
    #[test]
    fn image_aspect_is_preserved_without_changing_crop() {
        let mut layer = Layer::new(
            "Image",
            Content::Image(Image::new(Id::new_v4(), 20.0, 10.0)),
        );
        apply_field(&mut layer, "/content/properties/width", "60", &[]).unwrap();
        let Content::Image(image) = layer.content else {
            panic!()
        };
        assert_eq!(image.height, 30.0);
        assert_eq!(image.crop, Crop::default());
    }
    #[test]
    fn normalized_crop_remains_fractional() {
        let mut layer = Layer::new(
            "Image",
            Content::Image(Image::new(Id::new_v4(), 20.0, 10.0)),
        );
        apply_field(&mut layer, "/content/properties/crop/width", "0.5", &[]).unwrap();
        apply_field(&mut layer, "/content/properties/crop/x", "0.25", &[]).unwrap();
        let Content::Image(image) = layer.content else {
            panic!()
        };
        assert_eq!(image.crop.width, 0.5);
        assert_eq!(image.crop.x, 0.25);
    }
    #[test]
    fn every_static_inspector_field_round_trips_through_the_shared_schema() {
        let image = Image::new(Id::new_v4(), 20.0, 10.0);
        let text = Text {
            layout: TextLayout::Circle {
                radius: 24.0,
                start_angle: 12.0,
                repeat: true,
            },
            ..Default::default()
        };
        let mut contents = vec![
            Content::Rectangle(Rectangle::default()),
            Content::Ellipse(Shape::default()),
            Content::Triangle(Shape::default()),
            Content::Text(text.clone()),
            Content::Image(image.clone()),
            Content::Freehand(Freehand::default()),
        ];
        for shape in [
            ArmShape::Rectangle,
            ArmShape::Triangle,
            ArmShape::Chevron {
                inward: true,
                depth: 4.0,
            },
            ArmShape::Chevron {
                inward: false,
                depth: 4.0,
            },
            ArmShape::Arc {
                radius: 16.0,
                sweep: 90.0,
            },
            ArmShape::TShape {
                width: 4.0,
                thickness: 2.0,
            },
            ArmShape::Text(text),
            ArmShape::Image(image),
        ] {
            contents.push(Content::Crosshair(Crosshair {
                shape,
                dot: Dot {
                    enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            }));
        }
        for content in contents {
            let mut layer = Layer::new(content.kind_name(), content);
            layer.effects.outline.enabled = true;
            for field in fields(&layer, &[], false) {
                if matches!(field.kind, FieldKind::Section | FieldKind::Action)
                    || field.key == "@arm_shape"
                {
                    continue;
                }
                let mut changed = layer.clone();
                apply_field(&mut changed, &field.key, &field.value, &[]).unwrap_or_else(|error| {
                    panic!("{} {}: {error}", layer.content.kind_name(), field.key)
                });
                validation::validate_layer_geometry(&changed).unwrap();
            }
        }
    }
    #[test]
    fn locked_layers_and_unknown_clipboard_assets_are_never_mutated() {
        let mut layer = Layer::new("Locked", Content::Rectangle(Rectangle::default()));
        layer.locked = true;
        let before = layer.clone();
        assert!(apply_field(&mut layer, "/transform/x", "10", &[]).is_err());
        assert_eq!(layer, before);
        let image = Layer::new(
            "Missing",
            Content::Image(Image::new(Id::new_v4(), 10.0, 10.0)),
        );
        let clipboard =
            serde_json::to_string(&json!({"format":"crosshair-rs-layers-1","layers":[image]}))
                .unwrap();
        assert!(parse_clipboard(&clipboard, &BTreeMap::new()).is_err());
    }
    #[test]
    fn cancelling_a_gesture_restores_the_complete_document() {
        let mut document = Document::default();
        document
            .open(AppState::default().presets.remove(0))
            .unwrap();
        let before = document.draft.clone();
        document.begin_gesture();
        document.draft.as_mut().unwrap().layers[0]
            .transform
            .rotation = 90.0;
        document.dirty = true;
        document.cancel_gesture(&AppState::default());
        assert_eq!(document.draft, before);
        assert!(!document.dirty);
    }
}

/// Preserve the design point beneath the pointer when changing magnification.
pub fn zoom_pan(pan: [f32; 2], pointer: [f32; 2], old: f32, new: f32) -> [f32; 2] {
    std::array::from_fn(|axis| pointer[axis] - (pointer[axis] - pan[axis]) * new / old)
}

#[cfg(test)]
mod viewport_tests {
    use super::*;
    #[test]
    fn pixel_nudges_preserve_fractional_origins_and_the_untouched_axis() {
        let mut preset = Preset::new("Imported");
        preset.layers[0].transform.x = 0.25;
        preset.layers[0].transform.y = -0.5;
        let selected = BTreeSet::from([preset.layers[0].id]);
        let before = preset.clone();
        nudge(&mut preset, &selected, 0.0, 0.0);
        assert_eq!(preset, before);
        nudge(&mut preset, &selected, 1.0, 0.0);
        assert_eq!(preset.layers[0].transform.x, 1.25);
        assert_eq!(preset.layers[0].transform.y, -0.5);
        nudge(&mut preset, &selected, -1.0, 0.0);
        assert_eq!(preset, before);
    }

    #[test]
    fn transformations_preserve_the_center_with_a_noncentral_anchor() -> Result<()> {
        let mut preset = Preset::new("Off-center pivot");
        preset.layers[0].content = Content::Rectangle(Rectangle {
            width: 10.0,
            height: 6.0,
            ..Default::default()
        });
        preset.layers[0].transform.x = 0.25;
        preset.layers[0].transform.y = -0.5;
        preset.layers[0].transform.anchor = Anchor::TopLeft;
        let mut renderer = crosshair_renderer::Renderer::new()?;
        let layer = &preset.layers[0];
        let bounds = renderer.layer_bounds(layer, 1.0)?;
        let center = Point {
            x: (bounds.left + bounds.right) / 2.0,
            y: (bounds.top + bounds.bottom) / 2.0,
        };
        let selected = BTreeSet::from([layer.id]);
        let anchors = BTreeMap::from([(layer.id, renderer.anchor_point(layer)?)]);
        let original = preset.clone();
        transform_around(&mut preset, &selected, &anchors, center, 1.0, 0.0)?;
        assert_eq!(preset, original);
        transform_around(&mut preset, &selected, &anchors, center, 1.1, 90.0)?;
        let changed = renderer.layer_bounds(&preset.layers[0], 1.0)?;
        assert!(((changed.left + changed.right) / 2.0 - center.x).abs() < 0.001);
        assert!(((changed.top + changed.bottom) / 2.0 - center.y).abs() < 0.001);
        assert!((changed.height() - 11.0).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn resize_ignores_tangential_motion_and_preserves_fractional_start_sizes() {
        let origin = Point { x: 14.25, y: 14.25 };
        assert_eq!(
            resize_scale(origin, origin, Point::default(), 28.5, false),
            1.0
        );
        assert_eq!(
            resize_scale(
                origin,
                Point { x: 15.25, y: 13.25 },
                Point::default(),
                28.5,
                false
            ),
            1.0
        );
        let scale = resize_scale(
            origin,
            Point { x: 14.75, y: 14.75 },
            Point::default(),
            28.5,
            false,
        );
        assert!((28.5 * scale - 29.5).abs() < 0.001);
    }

    #[test]
    fn coarse_resize_snaps_to_ten_percent_without_jumping_on_press() {
        let origin = Point { x: 50.0, y: 50.0 };
        assert_eq!(
            resize_scale(origin, origin, Point::default(), 100.0, true),
            1.0
        );
        assert_eq!(
            resize_scale(
                origin,
                Point { x: 53.0, y: 53.0 },
                Point::default(),
                100.0,
                true
            ),
            1.1
        );
    }
    #[test]
    fn zoom_keeps_the_point_under_the_pointer_after_panning() {
        let pan = [127.0, -81.0];
        let pointer = [-93.0, 205.0];
        let after = zoom_pan(pan, pointer, 8.0, 10.0);
        for axis in 0..2 {
            assert!(
                ((pointer[axis] - pan[axis]) / 8.0 - (pointer[axis] - after[axis]) / 10.0).abs()
                    < 0.001
            );
        }
        assert_eq!(zoom_pan(after, pointer, 10.0, 8.0), pan);
    }
    #[test]
    fn text_cannot_be_created_by_the_editor() {
        assert!(add_layer(&mut Preset::new("test"), "text").is_err());
        assert!(!ARM_SHAPES.iter().any(|(value, _)| *value == "text"));
    }
}
