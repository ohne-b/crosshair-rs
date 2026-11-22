use super::*;
use crate::features::designer;
use anyhow::{bail, ensure};

impl App {
    pub(super) fn canvas_view(&self) -> CanvasView {
        let v = self
            .ui
            .upgrade()
            .map(|ui| ui.global::<Store>().get_canvas_view())
            .unwrap_or_default();
        CanvasView {
            scale: v.scale.max(0.25),
            visible: LayerBounds {
                left: v.left,
                top: v.top,
                right: v.right,
                bottom: v.bottom,
            },
        }
    }

    pub(super) fn refresh_canvas_view(&mut self) -> Result<()> {
        if self.preview_pending || self.window()?.global::<Store>().get_page() != 1 {
            return Ok(());
        }
        let view = self.canvas_view();
        if self
            .canvas_coverage
            .is_some_and(|(previous, bounds)| previous.covers(view, bounds))
        {
            return Ok(());
        }
        if self.playing {
            self.playback_tick()
        } else if self.animate {
            self.show_animated_target()
        } else if let Some(draft) = self.document.draft.clone() {
            self.request_bitmap(draft)
        } else {
            Ok(())
        }
    }

    pub(super) fn show_canvas_frame(&mut self, frame: CanvasFrame) -> Result<()> {
        let ui = self.window()?;
        let store = ui.global::<Store>();
        let scale = frame.scale;
        self.hits = frame.hits;
        let tiles = frame
            .tiles
            .iter()
            .map(|tile| crate::CanvasTile {
                bitmap: preview::image(tile),
                x: tile.origin[0] as f32 / scale,
                y: tile.origin[1] as f32 / scale,
                width: tile.width as f32 / scale,
                height: tile.height as f32 / scale,
            })
            .collect::<Vec<_>>();
        store.set_canvas_tiles(Rc::new(slint::VecModel::from(tiles)).into());
        self.canvas_coverage = Some((frame.view, frame.painted));
        Ok(())
    }

    pub(super) fn selection_bounds(&self) -> Option<CanvasBounds> {
        let preset = self.document.draft.as_ref()?;
        self.bounds
            .iter()
            .filter(|(id, _)| {
                self.document.selected.contains(id)
                    && preset
                        .layers
                        .iter()
                        .any(|layer| layer.id == *id && !layer.locked)
            })
            .map(|(_, bounds)| *bounds)
            .reduce(CanvasBounds::union)
    }

    fn hit_layer(&self, point: Point, padding: f32) -> Option<Id> {
        let preset = self.document.draft.as_ref()?;
        self.hits.layer_at(point, padding).filter(|id| {
            preset
                .layers
                .iter()
                .any(|layer| layer.id == *id && layer.visible && !layer.locked)
        })
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
        // Panning moves the viewport, so it remains available during read-only playback.
        if button == 3 || matches!(self.drag, Some(Drag::Pan { .. })) {
            match kind {
                0 if button == 3 && self.drag.is_none() => {
                    self.cancel_capture()?;
                    self.drag = Some(Drag::Pan {
                        origin: point,
                        pan: Point {
                            x: store.get_pan_x(),
                            y: store.get_pan_y(),
                        },
                    });
                }
                1 if button == 3 => {
                    if let Some(Drag::Pan { origin, pan }) = self.drag {
                        store.set_pan_x(pan.x + point.x - origin.x);
                        store.set_pan_y(pan.y + point.y - origin.y);
                    }
                }
                2 | 3
                    if (button == 3 || kind == 3)
                        && matches!(self.drag, Some(Drag::Pan { .. })) =>
                {
                    self.drag = None
                }
                _ => {}
            }
            return Ok(());
        }
        if store.get_read_only() || self.playing || self.animate {
            return Ok(());
        }
        match kind {
            0 if self.drag.is_none() => self.press_on_canvas(point, button, shift)?,
            1 => self.move_on_canvas(point, shift)?,
            2 if self.drag.take().is_some() => {
                self.finish_group = true;
                self.finish_edit()?;
                self.refresh_document();
            }
            3 if self.drag.take().is_some() => {
                self.pivot_base = None;
                self.preview_pending = false;
                self.cancel_document_gesture();
                self.cancel_group = true;
                self.finish_group = true;
                self.finish_edit()?;
                self.render();
                self.refresh_document();
            }
            _ => {}
        }
        Ok(())
    }

    fn press_on_canvas(&mut self, point: Point, button: i32, shift: bool) -> Result<()> {
        self.cancel_capture()?;
        let ui = self.window()?;
        let store = ui.global::<Store>();
        store.set_moving_layer(false);
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
            let inverse = *self
                .inverses
                .get(&layer.id)
                .context("Wait for the drawing preview")?;
            let stroke = Stroke {
                points: Vec::new(),
                width: store.get_brush_size(),
                color: Color::from_hex(&store.get_brush_color()).context("Invalid brush color")?,
                erase: button == 2,
            };
            let base = layer.clone();
            self.begin_edit()?;
            self.drag = Some(Drag::Draw {
                layer: base.id,
                inverse,
                stroke,
            });
            self.pivot_base = Some(base);
            return self.move_on_canvas(point, shift);
        }
        let zoom = store.get_zoom() / ui.window().scale_factor();
        let tolerance = 7.0 / zoom;
        let hit = self.hit_layer(point, tolerance);
        if button == 2 {
            if let Some(id) = hit
                && !self.document.selected.contains(&id)
            {
                self.document.select(id, false);
                self.refresh_document();
            }
            return self.action("layer-menu", if hit.is_some() { "selected" } else { "new" });
        }
        if let Some(selection) = self.selection_bounds() {
            let (bounds, rotation) = selection.oriented.unwrap_or((selection.painted, 0.0));
            let center = selection
                .oriented
                .map_or(selection.geometry.center(), |(bounds, _)| bounds.center());
            let handle_center = bounds.center();
            let handle = store.invoke_canvas_handle_at(
                bounds.width() * zoom,
                bounds.height() * zoom,
                rotation,
                (point.x - handle_center.x) * zoom,
                (point.y - handle_center.y) * zoom,
            );
            if handle.rotate {
                self.begin_edit()?;
                self.drag = Some(Drag::Rotate {
                    angle: (point.y - center.y).atan2(point.x - center.x),
                    center,
                });
                return Ok(());
            }
            if handle.sx != 0.0 {
                self.begin_edit()?;
                self.drag = Some(Drag::Resize {
                    origin: point,
                    center,
                    handle_center,
                    corner: Point {
                        x: bounds.width() / 2.0 * handle.sx,
                        y: bounds.height() / 2.0 * handle.sy,
                    },
                    span: selection.geometry.width().max(selection.geometry.height()),
                });
                return Ok(());
            }
        }
        if let Some(id) = hit {
            if shift || !self.document.selected.contains(&id) {
                self.document.select(id, shift);
            }
            if self.document.selected.contains(&id) {
                self.begin_edit()?;
                self.drag = Some(Drag::Move { origin: point });
            }
        } else if !shift {
            self.document.selected.clear();
        }
        self.refresh_document();

        Ok(())
    }

    fn move_on_canvas(&mut self, point: Point, shift: bool) -> Result<()> {
        let Some(drag) = self.drag.as_mut() else {
            return Ok(());
        };
        let Some(mut changed) = self.document.gesture.clone() else {
            return Ok(());
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
            Drag::Resize {
                origin,
                center,
                span,
                handle_center,
                corner,
            } => {
                let mut selected = changed
                    .layers
                    .iter_mut()
                    .filter(|l| !l.locked && self.document.selected.contains(&l.id));
                let single = selected.next().filter(|_| selected.next().is_none());
                let delta = Point {
                    x: point.x - origin.x,
                    y: point.y - origin.y,
                };
                let resized = single.is_some_and(|layer| {
                    if matches!(layer.content, Content::Crosshair(_))
                        && (layer.transform.scale != 1.0 || layer.transform.rotation != 0.0)
                    {
                        self.pivot_base.get_or_insert_with(|| layer.clone());
                    }
                    designer::resize_shape(layer, *corner, delta, shift)
                });
                if !resized {
                    let scale =
                        designer::resize_scale(*origin, point, *handle_center, *span, shift);
                    designer::transform_around(
                        &mut changed,
                        &self.document.selected,
                        &self.anchors,
                        *center,
                        scale,
                        0.0,
                    )?;
                }
            }
            Drag::Rotate { angle, center, .. } => {
                let mut delta =
                    ((point.y - center.y).atan2(point.x - center.x) - *angle).to_degrees();
                if shift {
                    delta = (delta / 15.0).round() * 15.0;
                }
                designer::transform_around(
                    &mut changed,
                    &self.document.selected,
                    &self.anchors,
                    *center,
                    1.0,
                    delta,
                )?;
            }
            Drag::Draw {
                layer,
                inverse: [a, b, c],
                stroke,
            } => {
                let local = Point {
                    x: a.x + point.x * (b.x - a.x) + point.y * (c.x - a.x),
                    y: a.y + point.x * (b.y - a.y) + point.y * (c.y - a.y),
                };
                if stroke
                    .points
                    .last()
                    .is_some_and(|last| (last.x - local.x).hypot(last.y - local.y) <= 0.15)
                {
                    return Ok(());
                }
                ensure!(
                    stroke.points.len() < MAX_DRAWING_POINTS,
                    "This drawing has reached its point limit"
                );
                stroke.points.push(local);
                let target = changed
                    .layers
                    .iter_mut()
                    .find(|target| target.id == *layer)
                    .context("The drawing layer was deleted")?;
                let Content::Freehand(drawing) = &mut target.content else {
                    bail!("Select a drawing layer");
                };
                drawing.strokes.push(stroke.clone());
            }
            Drag::Pan { .. } => unreachable!(),
        }
        if self
            .document
            .draft
            .as_ref()
            .is_none_or(|current| !designer::same_design(current, &changed))
            && validation::validate_preset(&changed, &self.state.assets).is_ok()
        {
            if let Some(current) = &self.document.draft {
                changed.revision = current.revision;
            }
            self.document.draft = Some(changed);
            self.document.dirty = true;
            let ui = self.window()?;
            let store = ui.global::<Store>();
            store.set_dirty(true);
            store.set_moving_layer(matches!(self.drag, Some(Drag::Move { .. })));
            self.render();
        }

        Ok(())
    }
}
