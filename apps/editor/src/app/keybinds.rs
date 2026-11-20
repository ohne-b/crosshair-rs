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
                        actions: profile.actions.clone(),
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
            Capture::Role(role) => format!("{role:?}").to_lowercase(),
            Capture::Existing(id) => id.to_string(),
            Capture::New(BindingAction::SelectPreset { .. }) => "new-shortcut".into(),
            Capture::New(_) => "new-action".into(),
        };
        let token = Id::new_v4();
        self.send(&format!("capture:{token}"), Command::BeginCapture { token })?;
        self.capture_token = Some(token);
        self.capture = Some(target);
        self.capture_profile = Some(self.state.active_profile);
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
        self.capture_profile = None;
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
        let Some(profile_id) = self
            .capture_profile
            .take()
            .filter(|id| *id == self.state.active_profile)
        else {
            return Ok(());
        };
        if let Some(chord) = &chord {
            anyhow::ensure!(
                !matches!(
                    target,
                    Capture::Role(keybinds::Target::Aim | keybinds::Target::Fire)
                ) || !matches!(chord.trigger, InputKey::Wheel { .. } | InputKey::Pause),
                "Aim and Fire require a keyboard key or mouse button with a release"
            );
            anyhow::ensure!(
                !matches!(
                    target,
                    Capture::Role(
                        keybinds::Target::Forward
                            | keybinds::Target::Left
                            | keybinds::Target::Backward
                            | keybinds::Target::Right
                    )
                ) || matches!(chord.trigger, InputKey::Keyboard { .. }),
                "Movement requires keyboard keys"
            );
            let own = capture_target(&target);
            let conflict = self
                .state
                .profiles
                .iter()
                .filter(|profile| matches!(target, Capture::Global) || profile.id == profile_id)
                .any(|profile| {
                    !keybinds::role_conflicts(
                        &profile.bindings,
                        &profile.actions,
                        self.state.settings.global_toggle.as_ref(),
                        own,
                        chord,
                    )
                    .is_empty()
                });
            if conflict {
                self.modal = Modal::Conflict(target, chord.clone(), profile_id);
                self.show_modal(
                    1,
                    "Shortcut already assigned",
                    "Replace the overlapping assignments with this action?",
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
            for profile in profiles
                .iter_mut()
                .filter(|profile| matches!(target, Capture::Global) || profile.id == profile_id)
            {
                for conflict in keybinds::role_conflicts(
                    &profile.bindings,
                    &profile.actions,
                    settings.global_toggle.as_ref(),
                    capture_target(&target),
                    chord,
                ) {
                    match conflict {
                        keybinds::Target::Global => settings.global_toggle = None,
                        keybinds::Target::Binding(id) => {
                            profile.bindings.retain(|binding| binding.id != id)
                        }
                        role => *role.chord_mut(&mut profile.actions).unwrap() = None,
                    }
                }
            }
        }
        let profile = profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .context("This profile was deleted")?;
        match target {
            Capture::Global => settings.global_toggle = chord,
            Capture::Role(role) => {
                *role
                    .chord_mut(&mut profile.actions)
                    .context("Invalid action role")? = chord
            }
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
                    let binding = Binding::new(chord, action);
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

fn capture_target(target: &Capture) -> Option<keybinds::Target> {
    match target {
        Capture::Global => Some(keybinds::Target::Global),
        Capture::Existing(id) => Some(keybinds::Target::Binding(*id)),
        Capture::Role(role) => Some(*role),
        Capture::New(_) => None,
    }
}
