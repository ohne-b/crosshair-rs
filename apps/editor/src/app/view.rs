use super::*;
use crate::{
    BindingRow, BoundsRow, FieldGroup, FieldRow, LayerRow, PresetGroup, PresetRow,
    features::{
        designer::{self, FieldKind},
        keybinds, library, settings,
    },
};
use slint::{Model, ModelRc, VecModel};

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(rows)).into()
}
fn strings(values: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    model(values.into_iter().map(Into::into).collect())
}

fn inspector_groups(rows: Vec<FieldRow>) -> Vec<FieldGroup> {
    let mut fields = rows.into_iter().peekable();
    let mut groups = Vec::new();
    while let Some(first) = fields.next() {
        let paired = fields.peek().is_some_and(|next| {
            (first.kind == 2
                && next.kind == 2
                && matches!(
                    (first.label.as_str(), next.label.as_str()),
                    ("X", "Y")
                        | ("Scale", "Rotation")
                        | ("Length", "Thickness")
                        | ("Gap", "Rotation")
                        | ("Width", "Height")
                        | ("Bar width", "Bar thickness")
                ))
                || (first.label == "Opacity" && next.key == "/transform/anchor")
        });
        let second = if paired {
            fields.next().unwrap()
        } else {
            FieldRow::default()
        };
        groups.push(FieldGroup {
            first,
            second,
            paired,
        });
    }
    groups
}

fn update_rows<T: Clone + PartialEq + 'static>(
    current: &ModelRc<T>,
    rows: &[T],
    same_identity: impl Fn(&T, &T) -> bool,
) -> bool {
    let Some(current) = current.as_any().downcast_ref::<VecModel<T>>() else {
        return false;
    };
    if current.row_count() != rows.len()
        || rows.iter().enumerate().any(|(index, row)| {
            current
                .row_data(index)
                .is_none_or(|previous| !same_identity(&previous, row))
        })
    {
        return false;
    }
    for (index, row) in rows.iter().enumerate() {
        if current.row_data(index).as_ref() != Some(row) {
            current.set_row_data(index, row.clone());
        }
    }
    true
}

impl App {
    pub(super) fn refresh(&mut self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let store = ui.global::<Store>();
        store.set_profiles(strings(
            self.state
                .profiles
                .iter()
                .map(|profile| profile.name.clone()),
        ));
        store.set_profile_index(
            self.state
                .profiles
                .iter()
                .position(|profile| profile.id == self.state.active_profile)
                .unwrap_or(0) as i32,
        );
        store.set_preset_names(strings(
            self.state.presets.iter().map(|preset| preset.name.clone()),
        ));
        store.set_output_mode(if self.state.settings.output_mode == OutputMode::GameBar {
            1
        } else {
            0
        });
        store.set_startup(self.state.settings.launch_at_login);
        store.set_show_on_launch(self.state.settings.show_on_launch);
        store.set_check_updates(self.state.settings.check_updates);
        if let Some(profile) = self.state.active_profile() {
            store.set_output_scale(profile.scale);
            store.set_output_x(profile.offset.x);
            store.set_output_y(profile.offset.y);
            store.set_positions(strings(
                profile
                    .positions
                    .iter()
                    .map(|position| position.name.clone()),
            ));
        }
        self.refresh_library();
        self.refresh_document();
        self.refresh_bindings();
        self.refresh_status();
    }

    pub(super) fn refresh_library(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let store = ui.global::<Store>();
        store.set_selector_total(self.state.presets.len() as i32);
        store.set_selector_favorite_count(
            self.state
                .presets
                .iter()
                .filter(|preset| preset.favorite)
                .count() as i32,
        );
        store.set_total_presets(self.state.presets.len() as i32);
        store.set_favorite_presets(
            self.state
                .presets
                .iter()
                .filter(|preset| preset.favorite)
                .count() as i32,
        );
        store.set_preset_thumbnails(model(
            self.state
                .presets
                .iter()
                .map(|preset| {
                    self.thumbnails
                        .get(&preset.id)
                        .map(|(_, image)| image.clone())
                        .unwrap_or_default()
                })
                .collect(),
        ));
        let selected = self
            .status
            .selected_preset
            .or_else(|| self.state.selected_preset().map(|preset| preset.id));
        let preset_row = |preset: &Preset| PresetRow {
            id: preset.id.to_string().into(),
            name: preset.name.clone().into(),
            thumbnail: self
                .thumbnails
                .get(&preset.id)
                .map(|(_, image)| image.clone())
                .unwrap_or_default(),
            selected: selected == Some(preset.id),
            favorite: preset.favorite,
        };
        let selector_rows: Vec<_> = library::filtered(
            &self.state,
            &store.get_selector_search(),
            store.get_selector_favorites(),
            0,
        )
        .into_iter()
        .map(preset_row)
        .collect();
        let palette_rows: Vec<_> = self.palette.iter().map(preset_row).collect();
        if !update_rows(&store.get_layer_presets(), &palette_rows, |a, b| {
            a.id == b.id
        }) {
            store.set_layer_presets(model(palette_rows));
        }
        if !update_rows(&store.get_selector_presets(), &selector_rows, |a, b| {
            a.id == b.id
        }) {
            store.set_selector_presets(model(selector_rows));
        }
        let rows: Vec<_> = library::filtered(
            &self.state,
            &store.get_search(),
            store.get_favorites(),
            store.get_sort(),
        )
        .into_iter()
        .map(preset_row)
        .collect();
        let columns = store.get_library_columns().clamp(2, 20) as usize;
        let groups = store.get_preset_groups();
        let same_groups = groups.row_count() == rows.len().div_ceil(columns)
            && rows.chunks(columns).enumerate().all(|(index, items)| {
                groups.row_data(index).is_some_and(|group| {
                    group.items.row_count() == items.len()
                        && items.iter().enumerate().all(|(index, item)| {
                            group
                                .items
                                .row_data(index)
                                .is_some_and(|previous| previous.id == item.id)
                        })
                })
            });
        if same_groups {
            for (index, items) in rows.chunks(columns).enumerate() {
                if let Some(group) = groups.row_data(index) {
                    update_rows(&group.items, items, |a, b| a.id == b.id);
                }
            }
        } else {
            store.set_preset_groups(model(
                rows.chunks(columns)
                    .map(|items| PresetGroup {
                        items: model(items.to_vec()),
                    })
                    .collect(),
            ));
        }
        if !update_rows(&store.get_presets(), &rows, |a, b| a.id == b.id) {
            store.set_presets(model(rows));
        }
    }

    pub(super) fn refresh_status(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let store = ui.global::<Store>();
        store.set_visible(self.status.visible);
        store.set_visibility_shortcut(
            self.state
                .settings
                .global_toggle
                .as_ref()
                .map(keybinds::chord_label)
                .unwrap_or_else(|| "Unbound".into())
                .into(),
        );
        let local_history = store.get_page() == 1 && self.document.draft.is_some();
        store.set_can_undo(if local_history {
            self.document.can_undo()
        } else {
            self.status.can_undo
        });
        store.set_can_redo(if local_history {
            self.document.can_redo()
        } else {
            self.status.can_redo
        });
        let active = self
            .status
            .selected_preset
            .or_else(|| self.state.selected_preset().map(|preset| preset.id));
        if let Some(active) = active {
            store.set_active_thumbnail(
                if self
                    .last_valid
                    .as_ref()
                    .is_some_and(|preset| preset.id == active)
                    && self
                        .document
                        .draft
                        .as_ref()
                        .is_some_and(|preset| preset.id == active)
                    && store.get_preview().size().width > 0
                {
                    store.get_preview()
                } else {
                    self.thumbnails
                        .get(&active)
                        .map(|(_, image)| image.clone())
                        .unwrap_or_default()
                },
            );
            store.set_active_name(
                self.state
                    .preset(active)
                    .map(|preset| preset.name.clone())
                    .unwrap_or_default()
                    .into(),
            );
        }
        store.set_document_active(self.document.draft.as_ref().map(|preset| preset.id) == active);
        store.set_monitors(strings(
            std::iter::once("Primary monitor".into()).chain(
                self.status
                    .monitors
                    .iter()
                    .map(|monitor| monitor.name.clone()),
            ),
        ));
        store.set_monitor_index(
            self.status
                .monitors
                .iter()
                .position(|monitor| self.state.settings.monitor.as_ref() == Some(&monitor.id))
                .map(|index| index + 1)
                .unwrap_or(0) as i32,
        );
        let widget = &self.status.widget;
        store.set_widget_status(
            if !widget.installed {
                "Not installed"
            } else if !widget.connected {
                "Not connected"
            } else if !widget.pinned {
                "Connected · not pinned"
            } else if !widget.click_through {
                "Pinned · enable click-through"
            } else if !widget.positioned {
                "Pinned · position required"
            } else {
                "Pinned · connected"
            }
            .into(),
        );
        store.set_update_phase(match self.status.update.phase {
            UpdatePhase::Idle => 0,
            UpdatePhase::Checking => 1,
            UpdatePhase::Current => 2,
            UpdatePhase::Available => 3,
            UpdatePhase::Downloading => 4,
            UpdatePhase::Ready => 5,
            UpdatePhase::Installing => 6,
            UpdatePhase::Failed => 7,
        });
        store.set_update_progress(
            self.status
                .update
                .total
                .filter(|total| *total > 0)
                .map(|total| (self.status.update.downloaded as f64 / total as f64).min(1.0) as f32)
                .unwrap_or(0.0),
        );
        store.set_update_total(self.status.update.total.is_some_and(|total| total > 0));
        store.set_update_status(settings::update_label(&self.status.update).into());
        store.set_update_version(
            self.status
                .update
                .version
                .clone()
                .unwrap_or_default()
                .into(),
        );
        store.set_update_bytes(settings::download_progress(&self.status.update).into());
        store.set_update_error(self.status.update.error.clone().unwrap_or_default().into());
        store.set_version(self.status.update.current_version.clone().into());
        if let Some(error) = self
            .status
            .save_error
            .as_ref()
            .or(self.status.render_error.as_ref())
        {
            store.set_error(error.into());
        }
    }

    pub(super) fn refresh_document(&mut self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let store = ui.global::<Store>();
        store.set_document_open(self.document.draft.is_some());
        store.set_dirty(self.document.dirty || self.inflight.is_some());
        store.set_save_paused(self.document.save_paused || self.document.conflict);
        store.set_selection_editable(self.document.draft.as_ref().is_some_and(|preset| {
            preset
                .layers
                .iter()
                .any(|layer| self.document.selected.contains(&layer.id) && !layer.locked)
        }));
        if let Some(preset) = &self.document.draft {
            store.set_document_id(preset.id.to_string().into());
            store.set_document_name(preset.name.clone().into());
            let layers: Vec<_> = preset
                .layers
                .iter()
                .rev()
                .map(|layer| LayerRow {
                    id: layer.id.to_string().into(),
                    name: layer.name.clone().into(),
                    kind: layer.content.kind_name().into(),
                    selected: self.document.selected.contains(&layer.id),
                    shown: layer.visible,
                    locked: layer.locked,
                })
                .collect();
            if !update_rows(&store.get_layers(), &layers, |a, b| a.id == b.id) {
                store.set_layers(model(layers));
            }
        }
        if !self.group_open || self.finish_group {
            self.fields = self
                .document
                .selected_layer()
                .map(|layer| designer::fields(layer, &self.fonts, self.document.selected.len() > 1))
                .unwrap_or_default();
            let mut collapsed = false;
            let selection: SharedString = self.selection_token().into();
            let rows: Vec<FieldRow> = self
                .fields
                .iter()
                .filter_map(|field| {
                    if matches!(field.kind, FieldKind::Section) {
                        collapsed = self.collapsed.contains(&field.key);
                    } else if collapsed {
                        return None;
                    }
                    let mut row = FieldRow {
                        selection: selection.clone(),
                        key: field.key.clone().into(),
                        label: field.label.clone().into(),
                        value: field.value.clone().into(),
                        ..Default::default()
                    };
                    match &field.kind {
                        FieldKind::Section => {
                            row.kind = 0;
                            row.checked = collapsed;
                        }
                        FieldKind::Text => row.kind = 1,
                        FieldKind::Number { min, max, .. } => {
                            row.kind = 2;
                            row.minimum = *min;
                            row.maximum = *max;
                            row.step =
                                if matches!(field.key.as_str(), "/transform/scale" | "/opacity")
                                    || field.key.contains("/crop/")
                                {
                                    0.01
                                } else {
                                    1.0
                                };
                            row.numeric = field.value.parse().unwrap_or_default();
                        }
                        FieldKind::Toggle => {
                            row.kind = 3;
                            row.checked = field.value == "true";
                        }
                        FieldKind::Color => {
                            row.kind = 4;
                            if let Some(color) = Color::from_hex(&field.value) {
                                row.swatch =
                                    slint::Color::from_argb_u8(color.a, color.r, color.g, color.b);
                            }
                        }
                        FieldKind::Choice(choices) => {
                            row.kind = 5;
                            row.selected = choices
                                .iter()
                                .position(|(value, _)| value == &field.value)
                                .unwrap_or_default()
                                as i32;
                            row.choices = strings(choices.iter().map(|(_, label)| label.clone()));
                        }
                        FieldKind::Action => row.kind = 6,
                    }
                    Some(row)
                })
                .collect();
            let rows = inspector_groups(rows);
            if !update_rows(&store.get_fields(), &rows, |a, b| {
                a.first.key == b.first.key
                    && a.first.selection == b.first.selection
                    && a.first.kind == b.first.kind
                    && a.paired == b.paired
                    && a.second.key == b.second.key
            }) {
                store.set_fields(model(rows));
            }
        }
        self.refresh_bounds();
        self.refresh_status();
    }
    pub(super) fn selection_token(&self) -> String {
        self.document
            .draft
            .iter()
            .map(|document| document.id)
            .chain(self.document.selected.iter().copied())
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(":")
    }

    pub(super) fn refresh_bounds(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let rows: Vec<_> = self
            .bounds
            .iter()
            .map(|(id, bounds)| BoundsRow {
                id: id.to_string().into(),
                x: bounds.left,
                y: bounds.top,
                width: bounds.width(),
                height: bounds.height(),
                selected: self.document.selected.contains(id),
            })
            .collect();
        let store = ui.global::<Store>();
        if !update_rows(&store.get_bounds(), &rows, |a, b| a.id == b.id) {
            store.set_bounds(model(rows));
        }
    }

    pub(super) fn refresh_bindings(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let mut rows = vec![BindingRow {
            id: "global".into(),
            name: SharedString::default(),
            action: "Toggle visibility".into(),
            target: SharedString::default(),
            target_index: -1,
            chord: self
                .state
                .settings
                .global_toggle
                .as_ref()
                .map(keybinds::chord_label)
                .unwrap_or_else(|| "Set shortcut".into())
                .into(),
            enabled: self.state.settings.global_toggle.is_some(),
            shortcut: false,
            global: true,
        }];
        if let Some(profile) = self.state.active_profile() {
            rows.extend(profile.bindings.iter().map(|binding| {
                BindingRow {
                    id: binding.id.to_string().into(),
                    action: keybinds::action_name(&binding.action).into(),
                    name: if binding.name.is_empty() {
                        binding
                            .action
                            .target()
                            .and_then(|id| self.state.preset(id))
                            .map(|preset| preset.name.clone())
                            .unwrap_or_default()
                    } else {
                        binding.name.clone()
                    }
                    .into(),
                    target: binding
                        .action
                        .target()
                        .map(|id| id.to_string())
                        .unwrap_or_default()
                        .into(),
                    chord: keybinds::chord_label(&binding.chord).into(),
                    enabled: binding.enabled,
                    target_index: binding
                        .action
                        .target()
                        .and_then(|id| self.state.presets.iter().position(|preset| preset.id == id))
                        .map(|index| index as i32)
                        .unwrap_or(-1),
                    shortcut: matches!(binding.action, BindingAction::SelectPreset { .. }),
                    global: false,
                }
            }));
        }
        let store = ui.global::<Store>();
        if !update_rows(&store.get_bindings(), &rows, |a, b| a.id == b.id) {
            store.set_bindings(model(rows));
        }
    }

    pub(super) fn show_modal(
        &self,
        kind: i32,
        title: &str,
        message: &str,
        confirm: &str,
        alternate: &str,
        text: &str,
    ) {
        if let Some(ui) = self.ui.upgrade() {
            let store = ui.global::<Store>();
            store.set_modal_kind(kind);
            store.set_modal_title(title.into());
            store.set_modal_message(message.into());
            store.set_modal_confirm(confirm.into());
            store.set_modal_alternate(alternate.into());
            store.set_modal_text(text.into());
        }
    }
    pub(super) fn hide_modal(&mut self) {
        self.modal = Modal::None;
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Store>().set_modal_kind(0);
        }
    }
    pub(super) fn menu(&mut self, title: &str, choices: Vec<(&str, String, String)>) {
        let labels = strings(choices.iter().map(|(label, _, _)| (*label).to_owned()));
        let actions = strings(choices.iter().map(|(_, action, _)| action.clone()));
        self.modal = Modal::Menu(
            choices
                .into_iter()
                .map(|(_, action, argument)| (action, argument))
                .collect(),
        );
        self.show_modal(3, title, "", "", "", "");
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Store>().set_modal_options(labels);
            ui.global::<Store>().set_modal_actions(actions);
        }
    }
}

#[cfg(test)]
mod inspector_tests {
    use super::*;

    #[test]
    fn refreshing_models_notifies_only_changed_rows_and_preserves_identity() {
        let rows = vec![
            LayerRow {
                id: "a".into(),
                name: "First".into(),
                ..Default::default()
            },
            LayerRow {
                id: "b".into(),
                name: "Second".into(),
                ..Default::default()
            },
        ];
        let current = model(rows.clone());
        let notifications = Rc::new(std::cell::Cell::new(0));
        let counter = notifications.clone();
        let observed = slint::FilterModel::new(current.clone(), move |_| {
            counter.set(counter.get() + 1);
            true
        });
        assert_eq!(observed.row_count(), 2);
        notifications.set(0);
        assert!(update_rows(&current, &rows, |a, b| a.id == b.id));
        assert_eq!(notifications.get(), 0);
        let mut changed = rows;
        changed[1].selected = true;
        assert!(update_rows(&current, &changed, |a, b| a.id == b.id));
        assert_eq!(notifications.get(), 1);
        assert!(current.row_data(1).unwrap().selected);
        changed.swap(0, 1);
        assert!(!update_rows(&current, &changed, |a, b| a.id == b.id));
        assert_eq!(current.row_data(0).unwrap().id, "a");
        assert_eq!(notifications.get(), 1);
    }

    #[test]
    #[ignore = "manual software-renderer timing benchmark; run with --nocapture"]
    fn editor_frame_timings() {
        use slint::platform::{Platform, WindowAdapter, WindowEvent, software_renderer::*};
        use std::time::Instant;
        struct Headless(Rc<MinimalSoftwareWindow>);
        impl Platform for Headless {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
                Ok(self.0.clone())
            }
        }
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
        let store = ui.global::<Store>();
        store.set_connected(true);
        store.set_page(1);
        store.set_document_name("Timing fixture".into());
        store.set_selection_editable(true);
        store.on_format_number(|value| designer::format_number(value).into());
        let rows = inspector_groups(
            (0..40)
                .map(|index| FieldRow {
                    selection: "fixture".into(),
                    key: format!("field-{index}").into(),
                    label: ["Length", "Thickness", "Gap", "Rotation"][index % 4].into(),
                    kind: 2,
                    numeric: 10.0,
                    maximum: 512.0,
                    step: 1.0,
                    ..Default::default()
                })
                .collect(),
        );
        store.set_fields(model(rows.clone()));
        ui.show().unwrap();
        let mut buffer = vec![slint::Rgb8Pixel::default(); 1280 * 800];
        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut buffer, 1280);
        });
        for scenario in [
            "scroll",
            "inspector-update",
            "preview-resize",
            "large-preview-resize",
        ] {
            ui.window().dispatch_event(WindowEvent::PointerScrolled {
                position: slint::LogicalPosition::new(1150.0, 450.0),
                delta_x: 0.0,
                delta_y: 10000.0,
            });
            slint::platform::update_timers_and_animations();
            window.draw_if_needed(|renderer| {
                renderer.render(&mut buffer, 1280);
            });
            let mut samples = Vec::new();
            let mut draws = 0;
            for frame in 0..70 {
                let start = Instant::now();
                match scenario {
                    "scroll" => ui.window().dispatch_event(WindowEvent::PointerScrolled {
                        position: slint::LogicalPosition::new(1150.0, 450.0),
                        delta_x: 0.0,
                        delta_y: if frame % 20 < 10 { -24.0 } else { 24.0 },
                    }),
                    "inspector-update" => {
                        let mut changed = rows.clone();
                        changed[0].first.numeric = 10.0 + frame as f32;
                        update_rows(&store.get_fields(), &changed, |a, b| {
                            a.first.key == b.first.key
                        });
                    }
                    _ => {
                        let size = (if scenario == "large-preview-resize" {
                            768
                        } else {
                            224
                        }) + frame % 32;
                        let raster = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(size, size);
                        store.set_preview(slint::Image::from_rgba8_premultiplied(raster));
                        store.set_preview_width(size as f32 / 8.0);
                        store.set_preview_height(size as f32 / 8.0);
                    }
                }
                slint::platform::update_timers_and_animations();
                let drew = window.draw_if_needed(|renderer| {
                    renderer.render(&mut buffer, 1280);
                });
                draws += usize::from(drew);
                if frame >= 10 {
                    samples.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            assert!(
                draws >= 60,
                "{scenario} must exercise actual paints: {draws}"
            );
            samples.sort_by(f64::total_cmp);
            println!(
                "{scenario}: p50={:.2}ms p95={:.2}ms worst={:.2}ms",
                samples[30], samples[57], samples[59]
            );
        }
        ui.hide().unwrap();
    }

    #[test]
    fn colors_do_not_pair_with_separate_opacity_fields() {
        let groups = inspector_groups(vec![
            FieldRow {
                kind: 4,
                key: "/content/color".into(),
                ..Default::default()
            },
            FieldRow {
                kind: 2,
                key: "@opacity:/content/color".into(),
                ..Default::default()
            },
        ]);
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| !group.paired));
    }

    #[test]
    fn pairing_preserves_edit_identity_and_never_crosses_sections() {
        let rows = [
            ("X", 2),
            ("Y", 2),
            ("Lines", 0),
            ("Length", 2),
            ("Thickness", 2),
            ("Gap", 2),
        ]
        .into_iter()
        .map(|(label, kind)| FieldRow {
            key: label.into(),
            label: label.into(),
            selection: "document:layer".into(),
            kind,
            ..Default::default()
        })
        .collect();
        let groups = inspector_groups(rows);
        assert_eq!(groups.len(), 4);
        assert!(groups[0].paired);
        assert_eq!(groups[0].second.key, "Y");
        assert_eq!(groups[0].second.selection, "document:layer");
        assert!(!groups[1].paired);
        assert!(groups[2].paired);
        assert_eq!(groups[2].second.key, "Thickness");
        assert!(!groups[3].paired);
    }
}
