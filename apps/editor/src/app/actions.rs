use super::*;
use crate::features::{designer, keybinds, library, profiles, settings};
use anyhow::{bail, ensure};

impl App {
    pub(super) fn action(&mut self, action: &str, value: &str) -> Result<()> {
        let ui = self.window()?;
        let store = ui.global::<Store>();
        ensure!(
            !store.get_read_only() || matches!(action, "modal-cancel" | "dismiss-error"),
            "The application is finishing shutdown. Wait for it to complete."
        );
        let resolves_draft = matches!(action, "modal-confirm" | "modal-alternate")
            && matches!(self.modal, Modal::Close | Modal::DraftConflict);
        if self.group_open && requires_finished_edit(action) && !resolves_draft {
            ensure!(
                !self.document.conflict,
                "Resolve or export the unsent draft before changing application state"
            );
            defer_ui(
                &mut self.deferred_ui,
                UiEvent::Action(action.into(), value.into()),
            )?;
            self.finish_group = true;
            self.send_document()?;
            self.finish_edit()?;
            return Ok(());
        }
        if self.capture.is_some()
            && !matches!(
                action,
                "capture" | "capture-cancel" | "modal-confirm" | "modal-cancel"
            )
        {
            self.cancel_capture()?;
        }
        match action {
            "request-close" => self.request_close()?,
            "selector-filter" => {
                store.set_selector_search(value.into());
                self.refresh_library();
            }
            "selector-favorites" => {
                store.set_selector_favorites(value.parse()?);
                self.refresh_library();
            }
            "binding-name" => {
                let (id, name) = value.split_once(':').context("Invalid shortcut name")?;
                let mut profile = self.active_profile()?;
                profile
                    .bindings
                    .iter_mut()
                    .find(|binding| binding.id.to_string() == id)
                    .context("The shortcut was deleted")?
                    .name = name.trim().into();
                self.profile_edit(profile)?;
            }
            "binding-target-id" => {
                let (id, target) = value.split_once(':').context("Invalid shortcut target")?;
                let preset = self
                    .state
                    .preset(target.parse()?)
                    .context("The crosshair was deleted")?
                    .id;
                let mut profile = self.active_profile()?;
                profile
                    .bindings
                    .iter_mut()
                    .find(|binding| binding.id.to_string() == id)
                    .context("The shortcut was deleted")?
                    .action = BindingAction::SelectPreset { preset };
                self.profile_edit(profile)?;
            }
            "add-crosshair-preset" => self.layer_action(action, value)?,
            "drop-image" => {
                ensure!(
                    store.get_page() == 1 && self.document.draft.is_some(),
                    "Open the designer to add an image"
                );
                ensure!(
                    self.image_target.is_none(),
                    "Wait for the current image import to finish"
                );
                self.image_target = Some(ImageTarget::New {
                    document: self.document_id()?,
                });
                if let Err(error) =
                    self.client
                        .stage_file(value.into(), StagingPurpose::Image, "image")
                {
                    self.image_target = None;
                    return Err(error);
                }
            }
            "window-minimize" | "window-maximize" | "window-close" => {
                frame::caption_action(self.hwnd()?, action)?
            }
            "navigate" => {
                store.set_page(value.parse()?);
                if store.get_page() != 2 {
                    store.set_request_shortcut(false);
                    store.set_pending_shortcut("".into());
                }
                if store.get_page() == 1 && self.document.draft.is_none() {
                    let id = self
                        .state
                        .selected_preset()
                        .context("Create a crosshair first")?
                        .id;
                    self.open_document(id)?;
                }
            }
            "github" => dialogs::open_url(self.hwnd()?, "https://github.com/ohne-b/crosshair-rs")?,
            "license" => dialogs::open_url(
                self.hwnd()?,
                "https://github.com/ohne-b/crosshair-rs/blob/main/LICENSE.md",
            )?,
            "release-notes" => {
                let url = self
                    .status
                    .update
                    .version
                    .as_ref()
                    .map(|version| {
                        format!("https://github.com/ohne-b/crosshair-rs/releases/tag/v{version}")
                    })
                    .unwrap_or_else(|| "https://github.com/ohne-b/crosshair-rs/releases".into());
                dialogs::open_url(self.hwnd()?, &url)?;
            }
            "update-help" => {
                self.modal = Modal::None;
                let message = self.status.update.error.clone().unwrap_or_else(|| "Downloads are verified with the project's updater key. Windows 11 requires administrator approval to install this unsigned package. The editor and overlay close during installation, then reopen with your settings.".into());
                self.show_modal(5, "Application updates", &message, "", "", "");
            }
            "notices" => {
                self.modal = Modal::None;
                self.show_modal(4, "Third-party notices", "Slint — Royalty-free Desktop License\nTabler Icons — MIT License\nManrope — SIL Open Font License 1.1\nRust logo — CC BY 4.0, modified for crosshair-rs. Rust Foundation trademarks belong to the Rust Foundation.\n\nRendering uses tiny-skia, resvg and Windows DirectWrite. Dependency licenses are included with the application package.", "", "", "");
            }
            "input-help" => {
                self.modal = Modal::None;
                self.show_modal(5, "Global shortcuts", settings::INPUT_HELP, "", "", "");
            }
            "gamebar-help" => {
                self.modal = Modal::None;
                self.show_modal(5, "Game Bar", settings::GAME_BAR_HELP, "", "", "");
            }
            "interface-help" => {
                self.modal = Modal::None;
                self.show_modal(5, "Interface", "The editor follows Windows display and text scaling, supports keyboard navigation, and avoids decorative animations. Windows minimize keeps the editor open; Close exits the editor while the crosshair engine stays running.", "", "", "");
            }
            "dismiss-error" => store.set_error(SharedString::default()),
            "reconnect" => self.client.reconnect()?,
            "toggle-visible" => self.send("visibility", Command::ToggleVisibility)?,
            "select-profile" => {
                store.set_pending_shortcut("".into());
                store.set_request_shortcut(false);
                let id = self
                    .state
                    .profiles
                    .get(value.parse::<usize>()?)
                    .context("This profile no longer exists")?
                    .id;
                self.send("select-profile", Command::SelectProfile { profile: id })?;
            }
            "select-preset" => self.send(
                "select-preset",
                Command::SelectPreset {
                    preset: value.parse()?,
                },
            )?,
            "use-document" => self.send(
                "select-preset",
                Command::SelectPreset {
                    preset: self.document_id()?,
                },
            )?,
            "edit-preset" => self.open_document(value.parse()?)?,
            "new-preset" => self.send(
                "new-preset",
                Command::NewPreset {
                    name: "New crosshair".into(),
                },
            )?,
            "duplicate-preset" => self.send(
                "duplicate-open",
                Command::DuplicatePreset {
                    preset: value.parse()?,
                },
            )?,
            "favorite" => {
                let id: Id = value.parse()?;
                let preset = self
                    .state
                    .preset(id)
                    .context("This crosshair no longer exists")?;
                self.send(
                    "favorite",
                    Command::SetFavorite {
                        preset: id,
                        favorite: !preset.favorite,
                    },
                )?;
            }
            "library-tab" => {
                store.set_browsing(value == "browse");
                store.set_search("".into());
                store.set_library_notice("".into());
                store.set_dragged_preset("".into());
                store.set_preset_drop_index(-1);
                self.refresh_library();
            }
            "save-browse" => {
                ensure!(
                    !store.get_browse_saving(),
                    "A crosshair is already being saved"
                );
                let id: Id = value.parse()?;
                let preset = self
                    .browse
                    .iter()
                    .find(|p| p.id == id)
                    .context("Unknown built-in crosshair")?;
                self.send(
                    "save-browse",
                    Command::AddPreset {
                        preset: preset.clone(),
                    },
                )?;
                store.set_browse_saving(true);
                store.set_library_notice("".into());
            }
            "search" => {
                store.set_search(value.into());
                self.refresh_library();
            }
            "favorites" => {
                store.set_favorites(value.parse()?);
                self.refresh_library();
            }
            "sort" => {
                store.set_sort(value.parse()?);
                self.refresh_library();
            }
            "library-columns" => {
                store.set_library_columns(value.parse::<i32>()?.clamp(2, 20));
                self.refresh_library();
            }
            "preset-drop" => {
                let (id, target) = value.split_once(':').context("Invalid reorder target")?;
                ensure!(
                    store.get_sort() == 0,
                    "Choose Manual order to rearrange crosshairs"
                );
                self.send(
                    "order",
                    Command::ReorderPresets {
                        order: library::placed_order(&self.state, id.parse()?, target.parse()?),
                    },
                )?;
            }
            "preset-order" => {
                let (id, delta) = value.split_once(':').context("Invalid reorder request")?;
                self.send(
                    "order",
                    Command::ReorderPresets {
                        order: library::moved_order(&self.state, id.parse()?, delta.parse()?),
                    },
                )?;
                store.set_sort(0);
                self.refresh_library();
            }
            "preset-menu" => {
                let name = self
                    .state
                    .preset(value.parse()?)
                    .context("This crosshair no longer exists")?
                    .name
                    .clone();
                self.menu(
                    &name,
                    vec![
                        ("Edit", "edit-preset".into(), value.into()),
                        ("Rename", "rename-preset".into(), value.into()),
                        ("Duplicate", "duplicate-preset".into(), value.into()),
                        ("Export", "export-preset".into(), value.into()),
                        ("Create keybind", "preset-keybind".into(), value.into()),
                        ("Move earlier", "preset-order".into(), format!("{value}:-1")),
                        ("Move later", "preset-order".into(), format!("{value}:1")),
                        ("Delete", "delete-preset".into(), value.into()),
                    ],
                );
            }
            "rename-preset" => {
                let preset = self
                    .state
                    .preset(value.parse()?)
                    .context("This crosshair no longer exists")?;
                self.modal = Modal::RenamePreset(preset.id);
                self.show_modal(2, "Rename crosshair", "", "Rename", "", &preset.name);
            }
            "commit-preset-name" => {
                let (id, name) = value.split_once(':').context("Invalid crosshair name")?;
                let mut preset = self
                    .state
                    .preset(id.parse()?)
                    .context("This crosshair no longer exists")?
                    .clone();
                let name = name.trim();
                if preset.name != name {
                    preset.name = name.into();
                    let revision = preset.revision;
                    self.client.document("rename", preset, revision)?;
                }
            }
            "delete-preset" => {
                let id = value.parse()?;
                let replacement = self
                    .state
                    .presets
                    .iter()
                    .find(|preset| preset.id != id)
                    .context("Keep at least one crosshair")?
                    .id;
                self.modal = Modal::DeletePreset(id, replacement);
                self.show_modal(
                    1,
                    "Delete crosshair",
                    &library::delete_message(&self.state, id, replacement),
                    "Delete",
                    "",
                    "",
                );
            }
            "export-preset" => self.export(ExportKind::Preset {
                preset: value.parse()?,
            })?,
            "export-png" => self.export_draft(DraftFormat::Png)?,
            "export-svg" => self.export_draft(DraftFormat::Svg)?,
            "export-draft" => self.export_draft(DraftFormat::Preset)?,
            "export-all" => self.export(ExportKind::All)?,
            "import" => {
                if let Some(path) =
                    dialogs::choose(self.hwnd()?, false, dialogs::FileKind::Document, "")?
                {
                    self.client.stage_file(
                        path,
                        StagingPurpose::Import { replace: false },
                        "import",
                    )?;
                }
            }
            "replace-data" => {
                ensure!(
                    !self.document.dirty && self.inflight.is_none(),
                    "Save or export the unsent draft before replacing all data"
                );
                if let Some(path) =
                    dialogs::choose(self.hwnd()?, false, dialogs::FileKind::Document, "")?
                {
                    self.modal = Modal::ReplaceData(path);
                    self.show_modal(1, "Replace all data", "Replace profiles, crosshairs and settings from this all-data export? The engine preserves a recovery backup before applying the replacement.", "Replace all", "", "");
                }
            }
            "file-menu" => {
                let id = self.document_id()?.to_string();
                self.menu(
                    "File",
                    vec![
                        ("New crosshair", "new-preset".into(), "".into()),
                        ("Import", "import".into(), "".into()),
                        ("Export crosshair", "export-draft".into(), id.clone()),
                        ("Export PNG", "export-png".into(), "".into()),
                        ("Duplicate crosshair", "duplicate-preset".into(), id),
                    ],
                );
            }
            "layer-menu" => {
                if value == "new" {
                    self.menu(
                        "New layer",
                        vec![
                            ("Crosshair", "add-layer".into(), "crosshair".into()),
                            ("Rectangle", "add-layer".into(), "rectangle".into()),
                            ("Ellipse", "add-layer".into(), "ellipse".into()),
                            ("Triangle", "add-layer".into(), "triangle".into()),
                            ("Image", "add-image".into(), "".into()),
                            ("Drawing", "draw".into(), "".into()),
                        ],
                    );
                } else {
                    self.menu(
                        "Layers",
                        vec![
                            ("Copy", "copy-layers".into(), "".into()),
                            ("Cut", "cut-layers".into(), "".into()),
                            ("Paste", "paste-layers".into(), "".into()),
                            ("Duplicate", "duplicate-layers".into(), "".into()),
                            ("Bring to front", "layer-order".into(), "front".into()),
                            ("Move up", "layer-order".into(), "up".into()),
                            ("Move down", "layer-order".into(), "down".into()),
                            ("Send to back", "layer-order".into(), "back".into()),
                            ("Unlock selection", "unlock-layers".into(), "".into()),
                            ("Delete", "delete-layers".into(), "".into()),
                        ],
                    );
                }
            }
            "shape-menu" => self.menu(
                "Shape",
                vec![
                    ("Rectangle", "add-layer".into(), "rectangle".into()),
                    ("Ellipse", "add-layer".into(), "ellipse".into()),
                    ("Triangle", "add-layer".into(), "triangle".into()),
                ],
            ),
            "rename-layer" => {
                let layer = self
                    .document
                    .draft
                    .as_ref()
                    .context("Open a crosshair first")?
                    .layers
                    .iter()
                    .find(|layer| layer.id.to_string() == value)
                    .context("This layer no longer exists")?;
                self.modal = Modal::RenameLayer(layer.id);
                self.show_modal(2, "Rename layer", "", "Rename", "", &layer.name);
            }
            "add-image" => {
                ensure!(
                    self.image_target.is_none(),
                    "Wait for the current image import to finish"
                );
                self.image_target = Some(ImageTarget::New {
                    document: self.document_id()?,
                });
                self.import_image()?;
            }
            "property-action" => {
                ensure!(
                    self.image_target.is_none(),
                    "Wait for the current image import to finish"
                );
                let prefix = value
                    .strip_prefix("@image:")
                    .context("Unknown image action")?;
                let layer = self
                    .document
                    .selected_layer()
                    .context("Select an image layer")?
                    .id;
                self.image_target = Some(ImageTarget::Layer {
                    document: self.document_id()?,
                    layer,
                    prefix: prefix.into(),
                });
                self.import_image()?;
            }
            "collapse-section" => {
                if !self.collapsed.insert(value.into()) {
                    self.collapsed.remove(value);
                }
                self.refresh_document();
            }
            "unlock-layers" | "delete-layer" | "add-layer" | "layer-visible" | "layer-lock"
            | "delete-layers" | "duplicate-layers" | "paste-layers" | "copy-layers"
            | "cut-layers" | "layer-order" | "layer-drop" | "rename-document" | "draw"
            | "draw-finish" | "brush-size" | "brush-color" => self.layer_action(action, value)?,
            "save" | "flush" if store.get_page() == 1 && self.document.draft.is_some() => {
                self.save_document()?
            }
            "undo" | "redo" if store.get_page() == 1 && self.document.draft.is_some() => {
                self.drag = None;
                self.draw_base = None;
                self.document.undo(action == "redo");
                self.preview_sent = None;
                self.render();
                self.refresh_document();
            }
            "undo" | "redo" | "flush" => {
                ensure!(
                    !self.document.conflict,
                    "Resolve or export the unsent draft before continuing"
                );
                let command = match action {
                    "undo" => Command::Undo,
                    "redo" => Command::Redo,
                    _ => Command::Flush,
                };
                if self.group_open || self.inflight.is_some() {
                    self.pending_command = Some(command);
                    self.finish_group = true;
                    self.send_document()?;
                    self.finish_edit()?;
                } else {
                    self.send(action, command)?;
                }
            }
            "retry-edit" => {
                ensure!(
                    store.get_connected(),
                    "Reconnect before retrying this draft"
                );
                if self.document.conflict {
                    self.modal = Modal::DraftConflict;
                    self.show_modal(1, "The saved crosshair changed", "Keep the editor open, export this draft, or replace the engine's current design with this draft.", "Replace saved design", "Export draft", "");
                } else {
                    store.set_error(SharedString::default());
                    self.save_document()?;
                }
            }
            "zoom-step" => {
                let values: Vec<f32> = value
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()?;
                ensure!(
                    values.len() == 3 && values.iter().all(|value| value.is_finite()),
                    "Invalid zoom position"
                );
                let old = store.get_zoom();
                let zoom = (old * if values[0] > 0.0 { 1.25 } else { 0.8 }).clamp(0.25, 64.0);
                let pan = designer::zoom_pan(
                    [store.get_pan_x(), store.get_pan_y()],
                    [values[1], values[2]],
                    old,
                    zoom,
                );
                store.set_pan_x(pan[0]);
                store.set_pan_y(pan[1]);
                store.set_zoom(zoom);
                self.render();
            }
            "zoom-reset" => {
                store.set_zoom(1.0);
                self.render();
                store.set_pan_x(0.0);
                store.set_pan_y(0.0);
            }
            "zoom-fit" => {
                let (width, height) = value.split_once(',').context("Invalid canvas size")?;
                let width: f32 = width.parse()?;
                let height: f32 = height.parse()?;
                store.set_zoom(
                    ((width - 100.0) / store.get_preview_width().max(1.0))
                        .min((height - 160.0) / store.get_preview_height().max(1.0))
                        .clamp(0.25, 32.0)
                        .floor()
                        .max(0.25),
                );
                store.set_pan_x(
                    -(store.get_preview_x() + store.get_preview_width() / 2.0) * store.get_zoom(),
                );
                store.set_pan_y(
                    -(store.get_preview_y() + store.get_preview_height() / 2.0) * store.get_zoom(),
                );
                self.render();
            }
            "reset-layout" => {
                store.set_zoom(8.0);
                store.set_pan_x(0.0);
                store.set_pan_y(0.0);
                self.collapsed.clear();
                self.refresh_document();
                self.render();
            }
            "profiles" => {
                let id = self.state.active_profile.to_string();
                self.menu(
                    "Profiles",
                    vec![
                        ("Create profile", "new-profile".into(), "".into()),
                        ("Rename", "rename-profile".into(), id.clone()),
                        ("Duplicate", "duplicate-profile".into(), id.clone()),
                        ("Import", "import".into(), "".into()),
                        ("Export", "export-profile".into(), id.clone()),
                        ("Delete", "delete-profile".into(), id),
                    ],
                );
            }
            "new-profile" => {
                self.modal = Modal::NewProfile;
                self.show_modal(
                    2,
                    "Create profile",
                    "",
                    "Create",
                    "",
                    &profiles::new_name(&self.state, "Profile"),
                );
            }
            "rename-profile" => {
                let profile = self
                    .state
                    .profiles
                    .iter()
                    .find(|profile| profile.id.to_string() == value)
                    .context("This profile no longer exists")?;
                self.modal = Modal::RenameProfile(profile.id);
                self.show_modal(2, "Rename profile", "", "Rename", "", &profile.name);
            }
            "duplicate-profile" => {
                let profile = self
                    .state
                    .profiles
                    .iter()
                    .find(|profile| profile.id.to_string() == value)
                    .context("This profile no longer exists")?;
                self.profile_edit(profiles::duplicate(profile))?;
            }
            "delete-profile" => {
                let id = value.parse()?;
                let replacement = self
                    .state
                    .profiles
                    .iter()
                    .find(|profile| profile.id != id)
                    .context("Keep at least one profile")?
                    .id;
                self.modal = Modal::DeleteProfile(id, replacement);
                self.show_modal(1, "Delete profile", "Its shortcuts and saved positions will be removed. The shared crosshair library is kept.", "Delete", "", "");
            }
            "export-profile" => self.export(ExportKind::Profile {
                profile: value.parse()?,
            })?,
            "output-mode"
            | "monitor"
            | "startup"
            | "show-on-launch"
            | "check-updates-setting"
            | "discord-presence"
            | "discord-style" => {
                let mut settings = self.state.settings.clone();
                match action {
                    "output-mode" => {
                        settings.output_mode = if value == "1" {
                            OutputMode::GameBar
                        } else {
                            OutputMode::Desktop
                        }
                    }
                    "monitor" => {
                        settings.monitor = value
                            .parse::<usize>()?
                            .checked_sub(1)
                            .and_then(|index| self.status.monitors.get(index))
                            .map(|monitor| monitor.id.clone())
                    }
                    "startup" => settings.launch_at_login = value.parse()?,
                    "show-on-launch" => settings.show_on_launch = value.parse()?,
                    "discord-presence" => settings.discord_presence = value.parse()?,
                    "discord-style" => {
                        settings.discord_style = match value {
                            "0" => crosshair_core::DiscordStyle::Simple,
                            "1" => crosshair_core::DiscordStyle::Quote,
                            _ => bail!("Choose Simple or Quote"),
                        };
                    }
                    _ => settings.check_updates = value.parse()?,
                }
                self.settings_edit(settings)?;
            }
            "output-scale" | "output-x" | "output-y" | "reset-position" => {
                let mut profile = self.active_profile()?;
                if action == "reset-position" {
                    profile.offset = Point::default();
                    profile.scale = 1.0;
                } else {
                    let number: f32 = value.parse()?;
                    ensure!(number.is_finite(), "Enter a finite number");
                    match action {
                        "output-scale" => profile.scale = number,
                        "output-x" => profile.offset.x = number,
                        _ => profile.offset.y = number,
                    }
                }
                self.profile_edit(profile)?;
            }
            "position-save" => {
                self.modal = Modal::SavePosition;
                self.show_modal(2, "Save position", "", "Save", "", "Position");
            }
            "position-use" | "position-delete" => {
                let mut profile = self.active_profile()?;
                let index: usize = value.parse()?;
                let position = profile
                    .positions
                    .get(index)
                    .context("Choose a saved position")?
                    .clone();
                if action == "position-use" {
                    profile.scale = position.scale;
                    profile.offset = position.offset;
                } else {
                    profile.positions.remove(index);
                }
                self.profile_edit(profile)?;
            }
            "open-gamebar" => self.send("gamebar", Command::OpenGameBar)?,
            "open-data" => self.send("open-data", Command::OpenDataDirectory)?,
            "reset-settings" => {
                self.modal = Modal::Reset;
                self.show_modal(
                    1,
                    "Reset settings",
                    "A recovery copy will be created before settings are reset.",
                    "Reset",
                    "",
                    "",
                );
            }
            "check-update" => self.send("update", Command::CheckUpdate)?,
            "download-update" => self.send("update", Command::DownloadUpdate)?,
            "cancel-update" => self.send("update", Command::CancelUpdate)?,
            "install-update" => {
                ensure!(
                    !self.document.conflict && !self.document.save_paused,
                    "Save or discard the unsent draft before installing"
                );
                self.install_requested = true;
                if self.group_open || self.document.dirty || self.inflight.is_some() {
                    self.modal = Modal::Install;
                    self.show_modal(
                        1,
                        "Save before installing?",
                        "Save this crosshair before restarting, or discard the edits.",
                        "Save",
                        "Discard",
                        "",
                    );
                } else {
                    self.send("install-flush", Command::Flush)?;
                }
            }
            "capture" => self.begin_capture(if value == "global" {
                Capture::Global
            } else {
                Capture::Existing(value.parse()?)
            })?,
            "capture-cancel" => self.cancel_capture()?,
            "discard-shortcut" => {
                self.cancel_capture()?;
                store.set_pending_shortcut("".into());
                store.set_request_shortcut(false);
            }
            "new-binding" => {
                ensure!(
                    store.get_pending_shortcut() == "new-shortcut",
                    "Wait for the shortcut to finish saving"
                );
                let index: usize = store.get_target_index().try_into()?;
                let preset = self
                    .state
                    .presets
                    .get(index)
                    .context("Select a target crosshair")?
                    .id;
                self.begin_capture(Capture::New(keybinds::action(value.parse()?, preset)))?;
            }
            "preset-keybind" => {
                ensure!(
                    store.get_pending_shortcut().is_empty()
                        || store.get_pending_shortcut() == "new-shortcut",
                    "Wait for the shortcut to finish saving"
                );
                let preset: Id = value.parse()?;
                store.set_target_index(
                    self.state
                        .presets
                        .iter()
                        .position(|candidate| candidate.id == preset)
                        .context("This crosshair was deleted")? as i32,
                );
                store.set_page(2);
                store.set_pending_shortcut("new-shortcut".into());
                store.set_request_shortcut(true);
            }
            "binding-remove" => {
                if value == "global" {
                    self.set_global_toggle(None)?;
                } else {
                    let mut profile = self.active_profile()?;
                    profile
                        .bindings
                        .retain(|binding| binding.id.to_string() != value);
                    self.profile_edit(profile)?;
                }
            }
            "binding-target" => {
                let (id, target) = value.split_once(':').context("Invalid shortcut target")?;
                let preset = self
                    .state
                    .presets
                    .get(target.parse::<usize>()?)
                    .context("The crosshair was deleted")?
                    .id;
                let mut profile = self.active_profile()?;
                let binding = profile
                    .bindings
                    .iter_mut()
                    .find(|binding| binding.id.to_string() == id)
                    .context("The shortcut was deleted")?;
                match &mut binding.action {
                    BindingAction::SelectPreset { preset: target } => *target = preset,
                    _ => bail!("This action has no crosshair target"),
                }
                self.profile_edit(profile)?;
            }
            "modal-cancel" => {
                self.install_requested = false;
                self.open_after_save = None;
                self.hide_modal();
                self.closing = false;
                if self.shutdown_requested.take().is_some() {
                    store.set_read_only(false);
                    self.send("cancel-quit", Command::CancelQuit)?;
                }
            }
            "modal-option" => {
                let index: usize = value.parse()?;
                let Modal::Menu(options) = std::mem::replace(&mut self.modal, Modal::None) else {
                    bail!("The menu has closed");
                };
                let (action, value) = options
                    .get(index)
                    .context("This menu item no longer exists")?
                    .clone();
                self.hide_modal();
                self.action(&action, &value)?;
            }
            "modal-confirm" => self.confirm_modal(value, false)?,
            "modal-alternate" => self.confirm_modal(value, true)?,
            _ => bail!("Unknown editor action: {action}"),
        }
        Ok(())
    }
}

fn requires_finished_edit(action: &str) -> bool {
    matches!(
        action,
        "new-preset"
            | "save-browse"
            | "duplicate-preset"
            | "favorite"
            | "commit-preset-name"
            | "preset-order"
            | "preset-drop"
            | "export-preset"
            | "export-all"
            | "import"
            | "replace-data"
            | "new-profile"
            | "duplicate-profile"
            | "export-profile"
            | "modal-confirm"
            | "modal-alternate"
            | "output-mode"
            | "monitor"
            | "output-scale"
            | "output-x"
            | "output-y"
            | "reset-position"
            | "startup"
            | "show-on-launch"
            | "check-updates-setting"
            | "discord-presence"
            | "discord-style"
            | "position-use"
            | "position-delete"
            | "binding-remove"
            | "binding-target"
            | "install-update"
    )
}
