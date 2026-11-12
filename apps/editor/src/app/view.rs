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
                || (first.kind == 4 && next.key.starts_with("@opacity:"))
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

fn update_rows<T: Clone + 'static>(
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
        current.set_row_data(index, row.clone());
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
        let rows: Vec<_> = library::filtered(
            &self.state,
            &store.get_search(),
            store.get_favorites(),
            store.get_sort(),
        )
        .into_iter()
        .map(|preset| PresetRow {
            id: preset.id.to_string().into(),
            name: preset.name.clone().into(),
            thumbnail: self
                .thumbnails
                .get(&preset.id)
                .map(|(_, image)| image.clone())
                .unwrap_or_default(),
            selected: selected == Some(preset.id),
            favorite: preset.favorite,
        })
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
        store.set_can_undo(self.status.can_undo);
        store.set_can_redo(self.status.can_redo);
        let active = self
            .status
            .selected_preset
            .or_else(|| self.state.selected_preset().map(|preset| preset.id));
        if let Some(active) = active {
            store.set_active_thumbnail(
                self.thumbnails
                    .get(&active)
                    .map(|(_, image)| image.clone())
                    .unwrap_or_default(),
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
        ui.global::<Store>().set_bounds(model(
            self.bounds
                .iter()
                .map(|(id, bounds)| BoundsRow {
                    id: id.to_string().into(),
                    x: bounds.left,
                    y: bounds.top,
                    width: bounds.width(),
                    height: bounds.height(),
                    selected: self.document.selected.contains(id),
                })
                .collect(),
        ));
    }

    pub(super) fn refresh_bindings(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let mut rows = vec![BindingRow {
            id: "global".into(),
            action: "Toggle visibility".into(),
            target: "All profiles".into(),
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
                    target: binding
                        .action
                        .target()
                        .and_then(|id| self.state.preset(id))
                        .map(|preset| preset.name.clone())
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
