use anyhow::Context;
use crosshair_core::{
    BindingAction, GamepadControl, Id, InputChord, InputKey, InputSource, Modifier, Profile,
    Settings, validation::chords_conflict,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Global,
    Binding(Id),
    Aim,
    Fire,
    Reload,
    Hide,
    Forward,
    Left,
    Backward,
    Right,
}

impl Target {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(match value {
            "global" => Self::Global,
            "aim" => Self::Aim,
            "fire" => Self::Fire,
            "reload" => Self::Reload,
            "hide" => Self::Hide,
            "forward" => Self::Forward,
            "left" => Self::Left,
            "backward" => Self::Backward,
            "right" => Self::Right,
            _ => Self::Binding(value.parse()?),
        })
    }
    /// Change one input slot without removing its saved crosshair shortcut.
    pub fn assign(
        self,
        profile: &mut Profile,
        settings: &mut Settings,
        source: InputSource,
        chord: Option<InputChord>,
    ) -> anyhow::Result<()> {
        let actions = &mut profile.actions;
        let pad = &mut actions.controller;
        let (keyboard, controller) = match self {
            Self::Global => (&mut settings.global_toggle, &mut settings.controller_toggle),
            Self::Binding(id) => {
                let binding = profile
                    .bindings
                    .iter_mut()
                    .find(|b| b.id == id)
                    .context("This shortcut was deleted")?;
                if chord.is_some() {
                    binding.enabled = true;
                }
                (&mut binding.chord, &mut binding.controller)
            }
            Self::Aim => (&mut actions.aim, &mut pad.aim),
            Self::Fire => (&mut actions.fire, &mut pad.fire),
            Self::Reload => (&mut actions.reload, &mut pad.reload),
            Self::Hide => (&mut actions.hold_to_hide, &mut pad.hold_to_hide),
            Self::Forward => (&mut actions.movement[0], &mut pad.movement[0]),
            Self::Left => (&mut actions.movement[1], &mut pad.movement[1]),
            Self::Backward => (&mut actions.movement[2], &mut pad.movement[2]),
            Self::Right => (&mut actions.movement[3], &mut pad.movement[3]),
        };
        match source {
            InputSource::KeyboardMouse => *keyboard = chord,
            InputSource::Controller => {
                *controller = chord
                    .map(|chord| match chord.trigger {
                        InputKey::Gamepad { control } => Ok(control),
                        _ => anyhow::bail!("Expected a controller input"),
                    })
                    .transpose()?;
            }
        }
        if let Self::Binding(id) = self
            && keyboard.is_none()
            && controller.is_none()
        {
            profile
                .bindings
                .retain(|binding| binding.id != id || binding.action.target().is_some());
        }
        Ok(())
    }
}

pub fn role_conflicts(
    profile: &Profile,
    settings: &Settings,
    own: Option<Target>,
    chord: &InputChord,
) -> Vec<(Target, InputSource)> {
    // Recording a disabled preset shortcut also re-enables its other assignment.
    let disabled = match own {
        Some(Target::Binding(id)) => profile.bindings.iter().find(|b| b.id == id && !b.enabled),
        _ => None,
    };
    let (key, controller) = match chord.trigger {
        InputKey::Gamepad { control } => (disabled.and_then(|b| b.chord.as_ref()), Some(control)),
        _ => (Some(chord), disabled.and_then(|b| b.controller)),
    };
    let a = &profile.actions;
    let pad = &a.controller;
    let roles = [
        (
            Target::Global,
            &settings.global_toggle,
            settings.controller_toggle,
        ),
        (Target::Aim, &a.aim, pad.aim),
        (Target::Fire, &a.fire, pad.fire),
        (Target::Reload, &a.reload, pad.reload),
        (Target::Hide, &a.hold_to_hide, pad.hold_to_hide),
        (Target::Forward, &a.movement[0], pad.movement[0]),
        (Target::Left, &a.movement[1], pad.movement[1]),
        (Target::Backward, &a.movement[2], pad.movement[2]),
        (Target::Right, &a.movement[3], pad.movement[3]),
    ];
    let presets = profile
        .bindings
        .iter()
        .filter(|binding| binding.enabled)
        .map(|binding| {
            (
                Target::Binding(binding.id),
                &binding.chord,
                binding.controller,
            )
        });
    let mut conflicts = Vec::new();
    for (target, other_key, other_controller) in roles.into_iter().chain(presets) {
        if Some(target) == own {
            continue;
        }
        if key
            .zip(other_key.as_ref())
            .is_some_and(|(a, b)| chords_conflict(a, b))
        {
            conflicts.push((target, InputSource::KeyboardMouse));
        }
        if controller.is_some() && controller == other_controller {
            conflicts.push((target, InputSource::Controller));
        }
    }
    conflicts
}

pub fn action_name(action: &BindingAction) -> &'static str {
    match action {
        BindingAction::ToggleVisibility => "Toggle visibility",
        BindingAction::SelectPreset { .. } => "Select crosshair",
    }
}

pub fn controller_label(control: GamepadControl) -> &'static str {
    use GamepadControl::*;
    match control {
        North => "Top face button",
        East => "Right face button",
        South => "Bottom face button",
        West => "Left face button",
        Start => "Start",
        Guide => "Guide",
        Back => "Back",
        DPadUp => "D-pad up",
        DPadDown => "D-pad down",
        DPadLeft => "D-pad left",
        DPadRight => "D-pad right",
        LeftStick => "Left stick click",
        RightStick => "Right stick click",
        LeftTrigger => "Left trigger",
        RightTrigger => "Right trigger",
        LeftShoulder => "Left shoulder",
        RightShoulder => "Right shoulder",
        LeftStickUp => "Left stick up",
        LeftStickDown => "Left stick down",
        LeftStickLeft => "Left stick left",
        LeftStickRight => "Left stick right",
        RightStickUp => "Right stick up",
        RightStickDown => "Right stick down",
        RightStickLeft => "Right stick left",
        RightStickRight => "Right stick right",
        LeftPaddle1 => "Left paddle 1",
        LeftPaddle2 => "Left paddle 2",
        RightPaddle1 => "Right paddle 1",
        RightPaddle2 => "Right paddle 2",
        Touchpad => "Touchpad click",
        Misc1 => "Extra button 1",
        Misc2 => "Extra button 2",
        Misc3 => "Extra button 3",
        Misc4 => "Extra button 4",
        Misc5 => "Extra button 5",
        Misc6 => "Extra button 6",
    }
}

pub fn chord_label(chord: &InputChord) -> String {
    let mut names: Vec<String> = chord
        .modifiers
        .iter()
        .map(|modifier| {
            match modifier {
                Modifier::Shift => "Shift",
                Modifier::Control => "Ctrl",
                Modifier::Alt => "Alt",
                Modifier::Windows => "Win",
                Modifier::LeftShift => "Left Shift",
                Modifier::RightShift => "Right Shift",
                Modifier::LeftControl => "Left Ctrl",
                Modifier::RightControl => "Right Ctrl",
                Modifier::LeftAlt => "Left Alt",
                Modifier::RightAlt => "Right Alt",
                Modifier::LeftWindows => "Left Win",
                Modifier::RightWindows => "Right Win",
            }
            .into()
        })
        .collect();
    names.push(match chord.trigger {
        InputKey::Mouse { button } => format!("Mouse {button}"),
        InputKey::Wheel { up } => if up { "Wheel up" } else { "Wheel down" }.into(),
        InputKey::Gamepad { control } => controller_label(control).into(),
        InputKey::Pause => "Pause".into(),
        InputKey::Keyboard {
            scan_code,
            extended,
        } => keyboard_name(scan_code, extended),
    });
    names.join(" + ")
}

fn keyboard_name(scan_code: u16, extended: bool) -> String {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyNameTextW;
        let mut buffer = [0u16; 128];
        let code = (i32::from(scan_code) << 16) | if extended { 1 << 24 } else { 0 };
        let length = unsafe { GetKeyNameTextW(code, &mut buffer) };
        if length > 0 {
            return String::from_utf16_lossy(&buffer[..length as usize]);
        }
    }
    format!(
        "Key {scan_code:02X}{}",
        if extended { " extended" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crosshair_core::{ActionBindings, AppState, Binding};

    #[test]
    fn role_conflicts_exclude_own_and_disabled_but_include_inactive_actions() {
        let chord = InputChord::key(0x12);
        let mut state = AppState::default();
        let profile = &mut state.profiles[0];
        profile.actions = ActionBindings::unbound();
        profile.actions.aim = Some(chord.clone());
        profile
            .bindings
            .push(Binding::new(chord.clone(), BindingAction::ToggleVisibility));
        state.settings.global_toggle = Some(chord.clone());
        assert_eq!(
            role_conflicts(profile, &state.settings, None, &chord),
            vec![
                (Target::Global, InputSource::KeyboardMouse),
                (Target::Aim, InputSource::KeyboardMouse),
                (
                    Target::Binding(profile.bindings[0].id),
                    InputSource::KeyboardMouse
                ),
            ]
        );
        profile.bindings[0].enabled = false;
        state.settings.global_toggle = None;
        assert!(role_conflicts(profile, &state.settings, Some(Target::Aim), &chord).is_empty());
    }
}
