use super::*;
use crate::features::keybinds;

impl App {
    pub(super) fn set_global_toggle(&self, global_toggle: Option<InputChord>) -> Result<()> {
        self.send(
            "bindings",
            Command::SetBindings {
                profiles: self
                    .state
                    .profiles
                    .iter()
                    .map(|profile| BindingSet {
                        profile: profile.id,
                        bindings: profile.bindings.clone(),
                    })
                    .collect(),
                global_toggle,
                expected_revision: self.state.revision,
            },
        )
    }
    pub(super) fn begin_capture(&mut self, target: Capture) -> Result<()> {
        self.cancel_capture()?;
        let label = match &target {
            Capture::Global => "global".into(),
            Capture::Existing(id) => id.to_string(),
            Capture::New(BindingAction::SelectPreset { .. }) => "new-shortcut".into(),
            Capture::New(_) => "new-action".into(),
        };
        let token = Id::new_v4();
        self.send(&format!("capture:{token}"), Command::BeginCapture { token })?;
        self.capture_token = Some(token);
        self.capture = Some(target);
        self.window()?
            .global::<Store>()
            .set_capture_id(label.into());
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
        self.capture_token = None;
        self.window()?
            .global::<Store>()
            .set_capture_id(SharedString::default());
        Ok(())
    }
    pub(super) fn complete_capture(&mut self, chord: Option<InputChord>) -> Result<()> {
        if chord.as_ref().is_some_and(|chord| {
            matches!(
                chord.trigger,
                InputKey::Mouse { .. } | InputKey::Wheel { .. }
            )
        }) {
            let pointer = frame::pointer_position(self.hwnd()?)?;
            let scale = self.window()?.window().scale_factor();
            if self.capture_region.is_none_or(|[x, y, width, height]| {
                pointer[0] / scale < x
                    || pointer[1] / scale < y
                    || pointer[0] / scale >= x + width
                    || pointer[1] / scale >= y + height
            }) {
                return self.clear_capture();
            }
        }
        self.capture_token = None;
        let Some(target) = self.capture.take() else {
            return Ok(());
        };
        self.window()?
            .global::<Store>()
            .set_capture_id(SharedString::default());
        if let Some(chord) = &chord {
            let profile = self.active_profile()?;
            let own = match target {
                Capture::Existing(id) => Some(id),
                _ => None,
            };
            let global = if matches!(target, Capture::Global) {
                None
            } else {
                self.state.settings.global_toggle.as_ref()
            };
            let conflict = if matches!(target, Capture::Global) {
                self.state
                    .profiles
                    .iter()
                    .find_map(|profile| keybinds::conflict(&profile.bindings, None, None, chord))
            } else {
                keybinds::conflict(&profile.bindings, global, own, chord)
            };
            if let Some(conflict) = conflict {
                self.modal = Modal::Conflict(target, chord.clone(), conflict);
                self.show_modal(
                    1,
                    "Shortcut already assigned",
                    "Replace the overlapping shortcuts with this action? The change can be undone.",
                    "Replace",
                    "",
                    "",
                );
                return Ok(());
            }
        }
        self.apply_capture(target, chord, None)
    }
    pub(super) fn apply_capture(
        &mut self,
        target: Capture,
        chord: Option<InputChord>,
        replace: Option<Option<Id>>,
    ) -> Result<()> {
        let mut profiles = self.state.profiles.clone();
        if replace.is_some()
            && matches!(target, Capture::Global)
            && let Some(chord) = &chord
        {
            for profile in &mut profiles {
                while let Some(Some(id)) = keybinds::conflict(&profile.bindings, None, None, chord)
                {
                    profile.bindings.retain(|binding| binding.id != id);
                }
            }
        }
        let profile = profiles
            .iter_mut()
            .find(|profile| profile.id == self.state.active_profile)
            .context("No active profile")?;
        let mut settings = self.state.settings.clone();
        if let Some(conflict) = replace {
            if let Some(id) = conflict {
                profile.bindings.retain(|binding| binding.id != id);
            } else {
                settings.global_toggle = None;
            }
        }
        match target {
            Capture::Global => settings.global_toggle = chord,
            Capture::Existing(id) => {
                if let Some(chord) = chord {
                    let binding = profile
                        .bindings
                        .iter_mut()
                        .find(|binding| binding.id == id)
                        .context("This shortcut was deleted")?;
                    binding.chord = chord;
                    binding.enabled = true;
                } else {
                    profile.bindings.retain(|binding| binding.id != id);
                }
            }
            Capture::New(action) => {
                if let Some(chord) = chord {
                    profile.bindings.push(Binding::new(chord, action));
                }
            }
        }
        validation::validate_binding_conflicts(&profile.bindings, settings.global_toggle.as_ref())?;
        self.send(
            "bindings",
            Command::SetBindings {
                profiles: profiles
                    .into_iter()
                    .map(|profile| BindingSet {
                        profile: profile.id,
                        bindings: profile.bindings,
                    })
                    .collect(),
                global_toggle: settings.global_toggle,
                expected_revision: self.state.revision,
            },
        )
    }
}
