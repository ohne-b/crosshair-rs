use crosshair_core::{AppState, Id, Profile};

pub fn duplicate(profile: &Profile) -> Profile {
    let mut copy = profile.clone();
    copy.id = Id::new_v4();
    copy.name = format!("{} copy", profile.name.chars().take(74).collect::<String>());
    for binding in &mut copy.bindings {
        binding.id = Id::new_v4();
    }
    for position in &mut copy.positions {
        position.id = Id::new_v4();
    }
    copy
}

pub fn new_name(state: &AppState, base: &str) -> String {
    if state.profiles.iter().all(|profile| profile.name != base) {
        return base.into();
    }
    (2..)
        .map(|number| format!("{base} {number}"))
        .find(|name| state.profiles.iter().all(|profile| &profile.name != name))
        .expect("finite profile list")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_copy_has_independent_ids_but_same_preset() {
        let state = AppState::default();
        let original = state.active_profile().unwrap();
        let copy = duplicate(original);
        assert_ne!(copy.id, original.id);
        assert_eq!(copy.selected_preset, original.selected_preset);
    }
}
