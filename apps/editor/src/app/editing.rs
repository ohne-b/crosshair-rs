use super::*;
use crate::features::designer::{self, FieldKind};
use anyhow::{bail, ensure};

impl App {
    pub(super) fn open_document(&mut self, id: Id) -> Result<()> {
        ensure!(
            !self.document.conflict,
            "Resolve or export this draft before opening another crosshair"
        );
        if self.group_open || self.inflight.is_some() || self.document.dirty {
            ensure!(
                self.window()?.global::<Store>().get_connected(),
                "Reconnect and save, or export this draft before opening another crosshair"
            );
            defer_ui(
                &mut self.deferred_ui,
                UiEvent::Action("edit-preset".into(), id.to_string()),
            )?;
            self.finish_group = true;
            self.send_document()?;
            self.finish_edit()?;
            return Ok(());
        }
        let preset = self
            .state
            .preset(id)
            .context("This crosshair no longer exists")?
            .clone();
        self.document.open(preset)?;
        self.window()?.global::<Store>().set_page(1);
        self.refresh_document();
        self.render();
        Ok(())
    }

    pub(super) fn begin_edit(&mut self) -> Result<()> {
        ensure!(
            !self.document.conflict,
            "This crosshair changed in the engine. Export the draft or discard it before continuing."
        );
        if !self.window()?.global::<Store>().get_connected() {
            self.document.begin_gesture();
            return Ok(());
        }
        if !self.group_open {
            let document = self
                .document
                .draft
                .as_ref()
                .context("Open a crosshair first")?;
            self.send(
                "begin-edit",
                Command::BeginEdit {
                    preset: document.id,
                    expected_revision: document.revision,
                },
            )?;
            self.group_open = true;
            self.finish_group = false;
            self.cancel_group = false;
            self.document.begin_gesture();
        }
        Ok(())
    }

    pub(super) fn send_document(&mut self) -> Result<()> {
        if self.inflight.is_some()
            || !self.document.dirty
            || self.document.conflict
            || self.cancel_group
            || self.group_ending
            || self.preview_pending
        {
            return Ok(());
        }
        if !self.window()?.global::<Store>().get_connected() {
            return Ok(());
        }
        if !self.group_open {
            let finish = self.finish_group;
            self.begin_edit()?;
            self.finish_group |= finish;
        }
        let document = self
            .document
            .draft
            .as_ref()
            .context("Open a crosshair first")?
            .clone();
        self.client
            .document("document", document.clone(), document.revision)?;
        self.inflight = Some(document);
        Ok(())
    }

    pub(super) fn finish_edit(&mut self) -> Result<()> {
        if !self.window()?.global::<Store>().get_connected()
            && self.finish_group
            && !self.preview_pending
        {
            self.document.gesture = None;
            self.finish_group = false;
            self.cancel_group = false;
            self.draw_base = None;
            return Ok(());
        }
        if self.group_open
            && !self.group_ending
            && self.finish_group
            && self.inflight.is_none()
            && (!self.document.dirty || self.cancel_group)
        {
            self.send(
                if self.cancel_group {
                    "cancel-edit"
                } else {
                    "end-edit"
                },
                if self.cancel_group {
                    Command::CancelEdit
                } else {
                    Command::EndEdit
                },
            )?;
            self.finish_group = false;
            self.group_ending = true;
        }
        Ok(())
    }

    fn edit(&mut self, edit: impl FnOnce(&mut Preset, &BTreeSet<Id>) -> Result<()>) -> Result<()> {
        let before = self
            .document
            .draft
            .as_ref()
            .context("Open a crosshair first")?;
        let mut changed = before.clone();
        edit(&mut changed, &self.document.selected)?;
        validation::validate_preset(&changed, &self.state.assets)?;
        if changed == *before {
            return Ok(());
        }
        self.begin_edit()?;
        self.document.draft = Some(changed);
        self.document.dirty = true;
        self.finish_group = true;
        self.send_document()?;
        self.finish_edit()?;
        self.refresh_document();
        self.render();
        Ok(())
    }

    pub(super) fn property_edit(&mut self, key: &str, input: &str, phase: i32) -> Result<()> {
        ensure!(
            !self.window()?.global::<Store>().get_read_only(),
            "The application is finishing shutdown"
        );
        let mut value = input.to_owned();
        if phase == 3 {
            let field = self
                .fields
                .iter()
                .find(|field| field.key == key)
                .context("The selected field changed")?;
            let FieldKind::Choice(choices) = &field.kind else {
                bail!("The field is not a choice");
            };
            value = choices
                .get(input.parse::<usize>()?)
                .context("Choose a valid value")?
                .0
                .clone();
            if value == field.value {
                return Ok(());
            }
        }
        if key == "/transform/anchor" {
            let anchor: Anchor = serde_json::from_value(serde_json::json!(value))?;
            self.preview.set_anchor(
                self.document
                    .draft
                    .clone()
                    .context("Open a crosshair first")?,
                self.document.selected.iter().copied().collect(),
                anchor,
            );
            return Ok(());
        }
        if key == "@arm_shape" && value == "image" {
            ensure!(
                self.image_target.is_none(),
                "Wait for the current image import to finish"
            );
            let layer = self
                .document
                .selected_layer()
                .context("Select a crosshair layer")?
                .id;
            self.image_target = Some(ImageTarget::Arm {
                document: self.document_id()?,
                layer,
            });
            return self.import_image();
        }
        let before = self
            .document
            .draft
            .clone()
            .context("Open a crosshair first")?;
        let mut changed = before.clone();
        let result = (|| -> Result<()> {
            for layer in changed
                .layers
                .iter_mut()
                .filter(|layer| self.document.selected.contains(&layer.id) && !layer.locked)
            {
                designer::apply_field(layer, key, &value, &self.fonts)?;
            }
            validation::validate_preset(&changed, &self.state.assets)?;
            Ok(())
        })();
        if let Err(error) = result {
            if phase == 0 {
                return Ok(());
            }
            if phase >= 2 {
                self.finish_group = true;
                self.finish_edit()?;
                self.refresh_document();
            }
            return Err(error);
        }
        if changed != before {
            self.begin_edit()?;
            self.document.draft = Some(changed);
            self.document.dirty = true;
            self.send_document()?;
            self.render();
        }
        if phase >= 2 {
            self.finish_group = true;
            self.finish_edit()?;
            self.refresh_document();
        } else {
            self.window()?
                .global::<Store>()
                .set_dirty(self.document.dirty || self.inflight.is_some());
        }
        Ok(())
    }

    pub(super) fn layer_action(&mut self, action: &str, value: &str) -> Result<()> {
        match action {
            "add-layer" => {
                let mut id = Id::nil();
                self.edit(|preset, _| {
                    id = designer::add_layer(preset, value)?;
                    Ok(())
                })?;
                self.document.select(id, false);
                self.refresh_document();
            }
            "layer-visible" | "layer-lock" => {
                let id: Id = value.parse()?;
                self.edit(|preset, _| {
                    let layer = preset
                        .layers
                        .iter_mut()
                        .find(|layer| layer.id == id)
                        .context("Layer was deleted")?;
                    if action == "layer-lock" {
                        layer.locked = !layer.locked;
                    } else {
                        ensure!(!layer.locked, "Unlock this layer to change its visibility");
                        layer.visible = !layer.visible;
                    }
                    Ok(())
                })?;
            }
            "unlock-layers" => self.edit(|preset, selected| {
                for layer in &mut preset.layers {
                    if selected.contains(&layer.id) {
                        layer.locked = false;
                    }
                }
                Ok(())
            })?,
            "delete-layer" => {
                let id: Id = value.parse()?;
                self.edit(|preset, _| {
                    ensure!(
                        !preset
                            .layers
                            .iter()
                            .any(|layer| layer.id == id && layer.locked),
                        "Unlock this layer from its context menu before deleting it"
                    );
                    preset.layers.retain(|layer| layer.id != id);
                    Ok(())
                })?;
            }
            "delete-layers" => self.edit(|preset, selected| {
                preset
                    .layers
                    .retain(|layer| !selected.contains(&layer.id) || layer.locked);
                Ok(())
            })?,
            "duplicate-layers" | "paste-layers" => {
                let source: Vec<Layer> = if action == "paste-layers" {
                    designer::parse_clipboard(&dialogs::paste(self.hwnd()?)?, &self.state.assets)?
                } else {
                    self.document
                        .draft
                        .as_ref()
                        .context("Open a crosshair first")?
                        .layers
                        .iter()
                        .filter(|layer| self.document.selected.contains(&layer.id))
                        .cloned()
                        .collect()
                };
                let mut selected = BTreeSet::new();
                self.edit(|preset, _| {
                    ensure!(
                        preset.layers.len() + source.len() <= MAX_LAYERS,
                        "A crosshair can contain at most {MAX_LAYERS} layers"
                    );
                    for layer in source {
                        let mut duplicate = layer.duplicate();
                        duplicate.locked = false;
                        selected.insert(duplicate.id);
                        preset.layers.push(duplicate);
                    }
                    Ok(())
                })?;
                self.document.selected = selected;
                self.refresh_document();
            }
            "copy-layers" | "cut-layers" => {
                self.document.copy_selection();
                dialogs::copy(
                    self.hwnd()?,
                    &serde_json::to_string(
                        &serde_json::json!({"format":"crosshair-rs-layers-1","layers":self.document.clipboard}),
                    )?,
                )?;
                if action == "cut-layers" {
                    self.layer_action("delete-layers", "")?;
                }
            }
            "layer-order" => self.edit(|preset, selected| {
                designer::reorder(preset, selected, value);
                Ok(())
            })?,
            "layer-drop" => {
                let (id, target) = value
                    .split_once(':')
                    .context("Invalid layer reorder target")?;
                let id: Id = id.parse()?;
                let target: Id = target.parse()?;
                self.edit(|preset, _| {
                    let from = preset
                        .layers
                        .iter()
                        .position(|layer| layer.id == id)
                        .context("The layer was deleted")?;
                    let to = preset
                        .layers
                        .iter()
                        .position(|layer| layer.id == target)
                        .context("The target layer was deleted")?;
                    ensure!(
                        !preset.layers[from].locked,
                        "Unlock this layer to change its order"
                    );
                    let layer = preset.layers.remove(from);
                    preset.layers.insert(to, layer);
                    Ok(())
                })?;
            }
            "rename-document" => self.edit(|preset, _| {
                preset.name = value.trim().into();
                Ok(())
            })?,
            "draw" => {
                if self.window()?.global::<Store>().get_drawing() {
                    self.window()?.global::<Store>().set_drawing(false);
                } else {
                    if !matches!(
                        self.document.selected_layer().map(|layer| &layer.content),
                        Some(Content::Freehand(_))
                    ) {
                        self.layer_action("add-layer", "draw")?;
                    }
                    self.window()?.global::<Store>().set_drawing(true);
                }
            }
            "draw-finish" => {
                self.window()?.global::<Store>().set_drawing(false);
            }
            "brush-size" => {
                let number: f32 = value.parse()?;
                ensure!(
                    number.is_finite() && (0.1..=256.0).contains(&number),
                    "Brush size must be between 0.1 and 256"
                );
                self.window()?.global::<Store>().set_brush_size(number);
            }
            "brush-color" => {
                Color::from_hex(value).context("Use six or eight hexadecimal digits")?;
                self.window()?
                    .global::<Store>()
                    .set_brush_color(value.into());
            }
            _ => bail!("Unknown layer action"),
        }
        Ok(())
    }

    pub(super) fn import_image(&mut self) -> Result<()> {
        let result = (|| -> Result<bool> {
            if let Some(path) = dialogs::choose(self.hwnd()?, false, dialogs::FileKind::Image, "")?
            {
                self.client
                    .stage_file(path, StagingPurpose::Image, "image")?;
                Ok(true)
            } else {
                Ok(false)
            }
        })();
        if !matches!(result, Ok(true)) {
            self.image_target = None;
        }
        result.map(|_| ())
    }

    pub(super) fn add_imported_image(&mut self, id: Id, width: u32, height: u32) -> Result<()> {
        let target = self
            .image_target
            .take()
            .context("Image import no longer has a target")?;
        let factor = (128.0 / width.max(height).max(1) as f32).min(1.0);
        let image = crosshair_core::Image::new(id, width as f32 * factor, height as f32 * factor);
        let mut select = None;
        self.edit(|preset, _| {
            match target {
                ImageTarget::New { document } => { ensure!(preset.id == document, "The image's crosshair is no longer open. Reopen it and import the image again."); let layer = Layer::new("Image", Content::Image(image)); select = Some(layer.id); preset.layers.push(layer); }
                ImageTarget::Arm { document, layer } => {
                    ensure!(preset.id == document, "The image's crosshair is no longer open");
                    let layer = preset.layers.iter_mut().find(|candidate| candidate.id == layer).context("Image layer was deleted")?;
                    ensure!(!layer.locked, "Unlock this layer before replacing its image");
                    let Content::Crosshair(crosshair) = &mut layer.content else { bail!("Select a crosshair layer"); };
                    crosshair.shape = ArmShape::Image(image);
                }
                ImageTarget::Layer { document, layer, prefix } => {
                    ensure!(preset.id == document, "The image's crosshair is no longer open");
                    let layer = preset.layers.iter_mut().find(|candidate| candidate.id == layer).context("Image layer was deleted")?;
                    ensure!(!layer.locked, "Unlock this layer before replacing its image");
                    let mut encoded = serde_json::to_value(&*layer)?;
                    *encoded.pointer_mut(&prefix).context("The image field changed")? = serde_json::to_value(image)?;
                    *layer = serde_json::from_value(encoded)?;
                }
            }
            Ok(())
        })?;
        if let Some(id) = select {
            self.document.select(id, false);
            self.refresh_document();
        }
        Ok(())
    }

    pub(super) fn canvas_event(
        &mut self,
        kind: i32,
        point: Point,
        button: i32,
        shift: bool,
    ) -> Result<()> {
        let ui = self.window()?;
        let store = ui.global::<Store>();
        if store.get_read_only() {
            return Ok(());
        }
        if kind == 0 {
            self.cancel_capture()?;
            if button == 3 {
                self.drag = Some(Drag::Pan {
                    origin: point,
                    pan: Point {
                        x: store.get_pan_x(),
                        y: store.get_pan_y(),
                    },
                });
                return Ok(());
            }
            let before = self
                .document
                .draft
                .clone()
                .context("Open a crosshair first")?;
            if store.get_drawing() {
                let layer = self
                    .document
                    .selected_layer()
                    .context("Select a drawing layer")?;
                ensure!(!layer.locked, "Unlock this drawing first");
                ensure!(
                    matches!(layer.content, Content::Freehand(_)),
                    "Select a drawing layer"
                );
                let local = self.local_point(layer.id, point)?;
                let stroke = Stroke {
                    points: vec![local],
                    width: store.get_brush_size(),
                    color: Color::from_hex(&store.get_brush_color())
                        .context("Invalid brush color")?,
                    erase: button == 2,
                };
                self.draw_base = Some(layer.clone());
                self.drag = Some(Drag::Draw {
                    layer: layer.id,
                    before,
                    stroke,
                });
                self.begin_edit()?;
                return self.canvas_event(1, point, button, shift);
            }
            if button == 2 {
                self.action("layer-menu", "selected")?;
                return Ok(());
            }
            let tolerance = 7.0 / store.get_zoom();
            let selected_bounds = self
                .bounds
                .iter()
                .filter(|(id, _)| self.document.selected.contains(id))
                .map(|(_, bounds)| *bounds)
                .reduce(LayerBounds::union);
            if let Some(bounds) = selected_bounds {
                let center = Point {
                    x: (bounds.left + bounds.right) / 2.0,
                    y: (bounds.top + bounds.bottom) / 2.0,
                };
                if (point.x - center.x).abs() < tolerance
                    && (point.y - (bounds.top - 21.0 / store.get_zoom())).abs() < tolerance
                {
                    self.begin_edit()?;
                    self.drag = Some(Drag::Rotate {
                        angle: (point.y - center.y).atan2(point.x - center.x),
                        center,
                        before,
                    });
                    return Ok(());
                }
                if [bounds.left, bounds.right]
                    .iter()
                    .any(|x| (point.x - x).abs() < tolerance)
                    && [bounds.top, bounds.bottom]
                        .iter()
                        .any(|y| (point.y - y).abs() < tolerance)
                {
                    self.begin_edit()?;
                    self.drag = Some(Drag::Resize {
                        origin: point,
                        center,
                        before,
                    });
                    return Ok(());
                }
            }
            let hit = before.layers.iter().rev().find(|layer| {
                layer.visible
                    && self.bounds.iter().any(|(id, bounds)| {
                        *id == layer.id
                            && point.x >= bounds.left - tolerance
                            && point.x <= bounds.right + tolerance
                            && point.y >= bounds.top - tolerance
                            && point.y <= bounds.bottom + tolerance
                    })
            });
            if let Some(layer) = hit {
                if !self.document.selected.contains(&layer.id) || shift {
                    self.document.select(layer.id, shift);
                }
                if !layer.locked {
                    self.begin_edit()?;
                    self.drag = Some(Drag::Move {
                        origin: point,
                        before,
                    });
                }
            } else if !shift {
                self.document.selected.clear();
            }
            self.refresh_document();
        } else if kind == 1 {
            let Some(drag) = self.drag.clone() else {
                return Ok(());
            };
            if let Drag::Pan { origin, pan } = drag {
                store.set_pan_x(pan.x + point.x - origin.x);
                store.set_pan_y(pan.y + point.y - origin.y);
                return Ok(());
            }
            let mut changed = match &drag {
                Drag::Move { before, .. }
                | Drag::Resize { before, .. }
                | Drag::Rotate { before, .. }
                | Drag::Draw { before, .. } => before.clone(),
                _ => unreachable!(),
            };
            match drag {
                Drag::Move { origin, .. } => {
                    let mut x = point.x - origin.x;
                    let mut y = point.y - origin.y;
                    if shift {
                        if x.abs() > y.abs() {
                            y = 0.0;
                        } else {
                            x = 0.0;
                        }
                    }
                    designer::nudge(&mut changed, &self.document.selected, x.round(), y.round());
                }
                Drag::Resize { origin, center, .. } => {
                    let before = (origin.x - center.x).hypot(origin.y - center.y).max(0.01);
                    let mut scale = ((point.x - center.x).hypot(point.y - center.y) / before)
                        .clamp(0.01, 100.0);
                    if shift {
                        scale = (scale * 10.0).round().max(0.1) / 10.0;
                    }
                    designer::transform_around(
                        &mut changed,
                        &self.document.selected,
                        &self.anchors,
                        center,
                        scale,
                        0.0,
                    )?;
                }
                Drag::Rotate { angle, center, .. } => {
                    let mut delta =
                        ((point.y - center.y).atan2(point.x - center.x) - angle).to_degrees();
                    if shift {
                        delta = (delta / 15.0).round() * 15.0;
                    }
                    designer::transform_around(
                        &mut changed,
                        &self.document.selected,
                        &self.anchors,
                        center,
                        1.0,
                        delta,
                    )?;
                }
                Drag::Draw {
                    layer,
                    before,
                    mut stroke,
                } => {
                    let local = self.local_point(layer, point)?;
                    if stroke
                        .points
                        .last()
                        .is_none_or(|last| (last.x - local.x).hypot(last.y - local.y) > 0.15)
                    {
                        stroke.points.push(local);
                    }
                    ensure!(
                        stroke.points.len() <= MAX_DRAWING_POINTS,
                        "This drawing has reached its point limit"
                    );
                    let target = changed
                        .layers
                        .iter_mut()
                        .find(|target| target.id == layer)
                        .context("The drawing layer was deleted")?;
                    let Content::Freehand(drawing) = &mut target.content else {
                        bail!("Select a drawing layer");
                    };
                    drawing.strokes.push(stroke.clone());
                    self.drag = Some(Drag::Draw {
                        layer,
                        before,
                        stroke,
                    });
                }
                Drag::Pan { .. } => unreachable!(),
            }
            if validation::validate_preset(&changed, &self.state.assets).is_ok() {
                if let Some(current) = &self.document.draft {
                    changed.revision = current.revision;
                }
                self.document.draft = Some(changed);
                self.document.dirty = true;
                self.preview_pending = self.draw_base.is_some();
                if !self.preview_pending {
                    self.send_document()?;
                }
                self.render();
            }
        } else if kind == 2 && self.drag.take().is_some() {
            self.finish_group = true;
            self.finish_edit()?;
            self.refresh_document();
        } else if kind == 3
            && let Some(drag) = self.drag.take()
            && !matches!(drag, Drag::Pan { .. })
        {
            self.draw_base = None;
            self.preview_pending = false;
            self.document.cancel_gesture();
            self.cancel_group = true;
            self.finish_group = true;
            self.finish_edit()?;
            self.render();
            self.refresh_document();
        }
        Ok(())
    }

    fn local_point(&self, layer: Id, point: Point) -> Result<Point> {
        let [a, b, c] = self
            .inverses
            .get(&layer)
            .context("Wait for the drawing preview")?;
        Ok(Point {
            x: a.x + point.x * (b.x - a.x) + point.y * (c.x - a.x),
            y: a.y + point.x * (b.y - a.y) + point.y * (c.y - a.y),
        })
    }

    pub(super) fn shortcut(
        &mut self,
        text: &str,
        control: bool,
        shift: bool,
        alt: bool,
    ) -> Result<bool> {
        use slint::platform::Key;
        if self.window()?.global::<Store>().get_read_only() {
            return Ok(false);
        }
        if self.group_ending && !alt {
            let recognized = if control {
                matches!(
                    text.to_ascii_lowercase().as_str(),
                    "z" | "y" | "d" | "c" | "x" | "v" | "s"
                )
            } else {
                [
                    Key::Escape,
                    Key::Delete,
                    Key::Backspace,
                    Key::LeftArrow,
                    Key::RightArrow,
                    Key::UpArrow,
                    Key::DownArrow,
                ]
                .iter()
                .any(|key| SharedString::from(*key).as_str() == text)
                    || matches!(text, "[" | "]")
            };
            if recognized {
                defer_ui(
                    &mut self.deferred_ui,
                    UiEvent::Shortcut(text.into(), control, shift, alt),
                )?;
                return Ok(true);
            }
        }
        if text == SharedString::from(Key::Escape).as_str() {
            if self.capture.is_some() {
                self.cancel_capture()?;
            } else if !matches!(self.modal, Modal::None)
                || self.window()?.global::<Store>().get_modal_kind() != 0
            {
                self.action("modal-cancel", "")?;
            } else if self.drag.is_some() || self.group_open {
                self.drag = None;
                self.draw_base = None;
                self.preview_pending = false;
                self.document.cancel_gesture();
                self.cancel_group = true;
                self.finish_group = true;
                self.finish_edit()?;
                self.render();
                self.refresh_document();
            } else {
                self.window()?.global::<Store>().set_drawing(false);
            }
            return Ok(true);
        }
        if self.capture.is_some() {
            return Ok(true);
        }
        if !matches!(self.modal, Modal::None)
            || self.window()?.global::<Store>().get_modal_kind() != 0
            || alt
        {
            return Ok(false);
        }
        if control {
            let action = match text.to_lowercase().as_str() {
                "z" if shift => "redo",
                "z" => "undo",
                "y" => "redo",
                "d" => "duplicate-layers",
                "c" => "copy-layers",
                "x" => "cut-layers",
                "v" => "paste-layers",
                "s" => "flush",
                _ => return Ok(false),
            };
            if !matches!(action, "undo" | "redo" | "flush")
                && self.window()?.global::<Store>().get_page() != 1
            {
                return Ok(false);
            }
            self.action(action, "")?;
            return Ok(true);
        }
        if self.window()?.global::<Store>().get_page() != 1 {
            return Ok(false);
        }
        if text == SharedString::from(Key::Delete).as_str()
            || text == SharedString::from(Key::Backspace).as_str()
        {
            self.layer_action("delete-layers", "")?;
            return Ok(true);
        }
        if text == "[" || text == "]" {
            self.layer_action(
                "layer-order",
                if text == "[" {
                    if shift { "back" } else { "down" }
                } else if shift {
                    "front"
                } else {
                    "up"
                },
            )?;
            return Ok(true);
        }
        let amount = if shift { 10.0 } else { 1.0 };
        let delta = if text == SharedString::from(Key::LeftArrow).as_str() {
            Some((-amount, 0.0))
        } else if text == SharedString::from(Key::RightArrow).as_str() {
            Some((amount, 0.0))
        } else if text == SharedString::from(Key::UpArrow).as_str() {
            Some((0.0, -amount))
        } else if text == SharedString::from(Key::DownArrow).as_str() {
            Some((0.0, amount))
        } else {
            None
        };
        if let Some((x, y)) = delta {
            self.edit(|preset, selected| {
                designer::nudge(preset, selected, x, y);
                Ok(())
            })?;
            return Ok(true);
        }
        Ok(false)
    }
}
