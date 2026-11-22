use super::*;
use crate::features::keybinds::{self, Target};

impl App {
    pub(super) fn begin_capture(&mut self, target: Capture, source: InputSource) -> Result<()> {
        self.cancel_capture()?;
        let label = match &target {
            Capture::Existing(Target::Binding(id)) => id.to_string(),
            Capture::Existing(role) => format!("{role:?}").to_lowercase(),
            Capture::New(_) => "new-shortcut".into(),
        };
        let token = Id::new_v4();
        self.send(
            &format!("capture:{token}"),
            Command::BeginCapture { token, source },
        )?;
        self.capture_token = Some(token);
        self.capture = Some((target, source));
        self.capture_profile = Some(self.state.active_profile);
        let ui = self.window()?;
        let store = ui.global::<Store>();
        store.set_capture_controller(source == InputSource::Controller);
        store.set_capture_id(label.into());
        Ok(())
    }
    pub(super) fn cancel_capture(&mut self) -> Result<()> {
        let token = self.capture_token;
        self.clear_capture()?;
        if let Some(token) = token {
            self.send("cancel-capture", Command::CancelCapture { token })?;
        }
        Ok(())
    }
    pub(super) fn clear_capture(&mut self) -> Result<()> {
        self.capture = None;
        self.capture_profile = None;
        self.capture_token = None;
        self.window()?
            .global::<Store>()
            .set_capture_id(SharedString::default());
        Ok(())
    }
    fn pointer_outside_capture(&self) -> Result<bool> {
        let pointer = frame::pointer_position(self.hwnd()?)?;
        let scale = self.window()?.window().scale_factor();
        Ok(self.capture_region.is_none_or(|[x, y, width, height]| {
            pointer[0] / scale < x
                || pointer[1] / scale < y
                || pointer[0] / scale >= x + width
                || pointer[1] / scale >= y + height
        }))
    }

    pub(super) fn complete_capture(&mut self, chord: Option<InputChord>) -> Result<()> {
        if chord.as_ref().is_some_and(|chord| {
            matches!(
                chord.trigger,
                InputKey::Mouse { .. } | InputKey::Wheel { .. }
            )
        }) && self.pointer_outside_capture()?
        {
            return self.clear_capture();
        }
        self.capture_token = None;
        let Some((target, source)) = self.capture.take() else {
            return Ok(());
        };
        self.window()?
            .global::<Store>()
            .set_capture_id(SharedString::default());
        let Some(profile_id) = self
            .capture_profile
            .take()
            .filter(|id| *id == self.state.active_profile)
        else {
            return Ok(());
        };
        if let Some(chord) = &chord {
            anyhow::ensure!(
                chord.trigger.source() == source,
                "Input source changed while recording"
            );
            anyhow::ensure!(
                !matches!(
                    target,
                    Capture::Existing(Target::Aim | Target::Fire | Target::Hide)
                ) || !chord.trigger.is_pulse(),
                "Aim, Fire and Hold to hide require a control with a release"
            );
            anyhow::ensure!(
                !matches!(
                    target,
                    Capture::Existing(
                        Target::Forward | Target::Left | Target::Backward | Target::Right
                    )
                ) || matches!(
                    chord.trigger,
                    InputKey::Keyboard { .. } | InputKey::Gamepad { .. }
                ),
                "Movement requires a keyboard key or controller input"
            );
            let own = capture_target(&target);
            let conflict = self
                .state
                .profiles
                .iter()
                .filter(|profile| {
                    matches!(target, Capture::Existing(Target::Global)) || profile.id == profile_id
                })
                .any(|profile| {
                    !keybinds::role_conflicts(profile, &self.state.settings, own, chord).is_empty()
                });
            if conflict {
                let reactivating = matches!(own, Some(Target::Binding(id)) if self.state.active_profile()
                    .is_some_and(|profile| profile.bindings.iter().any(|b| b.id == id && !b.enabled)));
                let message = if reactivating {
                    "Recording enables this shortcut on both inputs. Replace its overlapping assignments?"
                } else {
                    "Replace the overlapping assignments with this action?"
                };
                self.modal = Modal::Conflict(target, chord.clone(), profile_id);
                self.show_modal(1, "Shortcut already assigned", message, "Replace", "", "");
                return Ok(());
            }
        }
        self.apply_capture(target, source, chord, None)
    }

    pub(super) fn apply_capture(
        &mut self,
        target: Capture,
        source: InputSource,
        chord: Option<InputChord>,
        replace: Option<Id>,
    ) -> Result<()> {
        let profile_id = replace.unwrap_or(self.state.active_profile);
        if profile_id != self.state.active_profile {
            return Ok(());
        }
        let mut profiles = self.state.profiles.clone();
        let mut settings = self.state.settings.clone();
        let mut created = None;
        if replace.is_some()
            && let Some(chord) = &chord
        {
            for profile in profiles.iter_mut().filter(|profile| {
                matches!(target, Capture::Existing(Target::Global)) || profile.id == profile_id
            }) {
                for (conflict, conflicting_source) in
                    keybinds::role_conflicts(profile, &settings, capture_target(&target), chord)
                {
                    conflict.assign(profile, &mut settings, conflicting_source, None)?;
                }
            }
        }
        let profile = profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .context("This profile was deleted")?;
        match target {
            Capture::Existing(existing) => {
                existing.assign(profile, &mut settings, source, chord)?
            }
            Capture::New(preset) => {
                if let Some(chord) = chord {
                    anyhow::ensure!(
                        self.state.preset(preset).is_some(),
                        "This crosshair was deleted"
                    );
                    let binding = Binding::new(chord, BindingAction::SelectPreset { preset });
                    created = Some(binding.id);
                    profile.bindings.insert(0, binding);
                }
            }
        }
        self.send(
            &created.map_or_else(|| "bindings".into(), |id| format!("new-shortcut:{id}")),
            Command::SetBindings {
                profiles: profiles
                    .into_iter()
                    .map(|profile| BindingSet {
                        profile: profile.id,
                        bindings: profile.bindings,
                        actions: profile.actions,
                    })
                    .collect(),
                global_toggle: settings.global_toggle,
                controller_toggle: settings.controller_toggle,
                expected_revision: self.state.revision,
            },
        )?;
        if let Some(id) = created {
            self.window()?
                .global::<Store>()
                .set_pending_shortcut(id.to_string().into());
        }
        Ok(())
    }
}

fn capture_target(target: &Capture) -> Option<Target> {
    match target {
        Capture::Existing(target) => Some(*target),
        Capture::New(_) => None,
    }
}
