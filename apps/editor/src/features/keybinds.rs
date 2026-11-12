use crosshair_core::{Binding, BindingAction, Id, InputChord, InputKey, Modifier};

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

pub fn conflict(
    bindings: &[Binding],
    global: Option<&InputChord>,
    own: Option<Id>,
    chord: &InputChord,
) -> Option<Option<Id>> {
    if let Some(global) = global
        && chords_conflict(global, chord)
    {
        return Some(None);
    }
    bindings
        .iter()
        .find(|binding| {
            binding.enabled && Some(binding.id) != own && chords_conflict(&binding.chord, chord)
        })
        .map(|binding| Some(binding.id))
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
    fn conflict_detection_excludes_edited_and_disabled_bindings() {
        let chord = InputChord::key(0x12);
        let mut binding = Binding::new(chord.clone(), BindingAction::ToggleVisibility);
        assert_eq!(
            conflict(&[binding.clone()], None, None, &chord),
            Some(Some(binding.id))
        );
        assert_eq!(
            conflict(&[binding.clone()], None, Some(binding.id), &chord),
            None
        );
        binding.enabled = false;
        assert_eq!(conflict(&[binding], None, None, &chord), None);
        assert_eq!(conflict(&[], Some(&chord), None, &chord), Some(None));
    }
}
