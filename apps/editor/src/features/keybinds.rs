use crosshair_core::{ActionBindings, Binding, BindingAction, Id, InputChord, InputKey, Modifier};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Global,
    Binding(Id),
    Aim,
    Fire,
    Reload,
}

impl Target {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        Ok(match value {
            "global" => Self::Global,
            "aim" => Self::Aim,
            "fire" => Self::Fire,
            "reload" => Self::Reload,
            _ => Self::Binding(value.parse()?),
        })
    }
    pub fn chord_mut(self, actions: &mut ActionBindings) -> Option<&mut Option<InputChord>> {
        match self {
            Self::Aim => Some(&mut actions.aim),
            Self::Fire => Some(&mut actions.fire),
            Self::Reload => Some(&mut actions.reload),
            _ => None,
        }
    }
}

pub fn role_conflicts(
    bindings: &[Binding],
    actions: &ActionBindings,
    global: Option<&InputChord>,
    own: Option<Target>,
    chord: &InputChord,
) -> Vec<Target> {
    let mut conflicts = Vec::new();
    for (target, assigned) in [
        (Target::Global, global),
        (Target::Aim, actions.aim.as_ref()),
        (Target::Fire, actions.fire.as_ref()),
        (Target::Reload, actions.reload.as_ref()),
    ] {
        if Some(target) != own && assigned.is_some_and(|assigned| chords_conflict(assigned, chord))
        {
            conflicts.push(target);
        }
    }
    conflicts.extend(
        bindings
            .iter()
            .filter(|binding| {
                binding.enabled
                    && own != Some(Target::Binding(binding.id))
                    && chords_conflict(&binding.chord, chord)
            })
            .map(|binding| Target::Binding(binding.id)),
    );
    conflicts
}

pub fn action_name(action: &BindingAction) -> &'static str {
    match action {
        BindingAction::ToggleVisibility => "Toggle visibility",
        BindingAction::SelectPreset { .. } => "Select crosshair",
    }
}

pub fn action(index: i32, preset: Id) -> BindingAction {
    if index == 5 {
        BindingAction::SelectPreset { preset }
    } else {
        BindingAction::ToggleVisibility
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

fn chords_conflict(a: &InputChord, b: &InputChord) -> bool {
    let bindings = [
        Binding::new(a.clone(), BindingAction::ToggleVisibility),
        Binding::new(b.clone(), BindingAction::ToggleVisibility),
    ];
    crosshair_core::validation::validate_binding_conflicts(&bindings, None).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn role_conflicts_exclude_own_and_disabled_but_include_inactive_actions() {
        let chord = InputChord::key(0x12);
        let mut binding = Binding::new(chord.clone(), BindingAction::ToggleVisibility);
        let mut actions = ActionBindings::unbound();
        actions.aim = Some(chord.clone());
        let conflicts = role_conflicts(&[binding.clone()], &actions, Some(&chord), None, &chord);
        assert_eq!(
            conflicts,
            vec![Target::Global, Target::Aim, Target::Binding(binding.id)]
        );
        binding.enabled = false;
        assert!(role_conflicts(&[binding], &actions, None, Some(Target::Aim), &chord).is_empty());
    }
}
