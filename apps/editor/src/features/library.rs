use crosshair_core::{AppState, Id, Preset};

pub fn filtered<'a>(
    state: &'a AppState,
    search: &str,
    favorites: bool,
    sort: i32,
) -> Vec<&'a Preset> {
    let search = search.trim().to_lowercase();
    let mut presets: Vec<_> = state
        .presets
        .iter()
        .filter(|preset| {
            (!favorites || preset.favorite) && preset.name.to_lowercase().contains(&search)
        })
        .collect();
    match sort {
        1 => presets.sort_by_key(|preset| preset.name.to_lowercase()),
        2 => presets.sort_by_key(|preset| std::cmp::Reverse(preset.modified_order)),
        _ => {}
    }
    presets
}

pub fn delete_message(state: &AppState, id: Id, replacement: Id) -> String {
    let profiles = state
        .profiles
        .iter()
        .filter(|profile| profile.selected_preset == id)
        .count();
    let bindings = state
        .profiles
        .iter()
        .flat_map(|profile| &profile.bindings)
        .filter(|binding| binding.action.target() == Some(id))
        .count();
    let name = state
        .preset(replacement)
        .map(|preset| preset.name.as_str())
        .unwrap_or("the replacement");
    if profiles + bindings == 0 {
        "Delete this crosshair? You can undo this change.".into()
    } else {
        format!(
            "Delete this crosshair? {profiles} profile(s) and {bindings} shortcut(s) will use {name}. You can undo this change."
        )
    }
}

pub fn moved_order(state: &AppState, id: Id, delta: i32) -> Vec<Id> {
    let mut order: Vec<_> = state.presets.iter().map(|preset| preset.id).collect();
    if let Some(index) = order.iter().position(|candidate| *candidate == id) {
        let target = (index as i32 + delta).clamp(0, order.len() as i32 - 1) as usize;
        let id = order.remove(index);
        order.insert(target, id);
    }
    order
}

pub fn placed_order(state: &AppState, id: Id, target: Id) -> Vec<Id> {
    let mut order: Vec<_> = state.presets.iter().map(|preset| preset.id).collect();
    if let (Some(from), Some(to)) = (
        order.iter().position(|candidate| *candidate == id),
        order.iter().position(|candidate| *candidate == target),
    ) {
        let moved = order.remove(from);
        order.insert(to, moved);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_and_favorites_do_not_mutate_manual_order() {
        let mut state = AppState::default();
        state.presets[0].favorite = true;
        let before = state.presets.clone();
        assert_eq!(filtered(&state, "  CLASS ", true, 1).len(), 1);
        assert_eq!(state.presets, before);
    }
    #[test]
    fn dropping_a_card_moves_it_to_the_target_without_losing_neighbors() {
        let state = AppState::default();
        let [a, b, c] = [
            state.presets[0].id,
            state.presets[1].id,
            state.presets[2].id,
        ];
        assert_eq!(placed_order(&state, a, c), vec![b, c, a]);
        assert_eq!(placed_order(&state, c, a), vec![c, a, b]);
        assert_eq!(placed_order(&state, b, b), vec![a, b, c]);
    }
    #[test]
    fn recent_sort_uses_global_edit_order_instead_of_per_document_revision() {
        let mut state = AppState::default();
        state.presets[0].revision = 100;
        state.presets[0].modified_order = 10;
        state.presets[1].revision = 2;
        state.presets[1].modified_order = 11;
        assert_eq!(filtered(&state, "", false, 2)[0].id, state.presets[1].id);
    }
}
