use serde::{Deserialize, Serialize};

use crate::{AppState, Id, InputChord, InputKey, Profile, ValidationError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AimAction {
    #[default]
    None,
    ToggleVisibility,
    HideWhileHeld,
    ShowWhileHeld,
    TogglePreset {
        preset: Id,
    },
    ShowPresetWhileHeld {
        preset: Id,
    },
}

impl AimAction {
    pub fn target(self) -> Option<Id> {
        match self {
            Self::TogglePreset { preset } | Self::ShowPresetWhileHeld { preset } => Some(preset),
            _ => None,
        }
    }

    pub fn remap(&mut self, mut map: impl FnMut(Id) -> Id) {
        match self {
            Self::TogglePreset { preset } | Self::ShowPresetWhileHeld { preset } => {
                *preset = map(*preset)
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionBindings {
    pub aim: Option<InputChord>,
    pub fire: Option<InputChord>,
    pub reload: Option<InputChord>,
    pub aim_action: AimAction,
}

impl ActionBindings {
    pub fn unbound() -> Self {
        Self {
            aim: None,
            fire: None,
            reload: None,
            aim_action: AimAction::None,
        }
    }

    pub fn chords(&self) -> impl Iterator<Item = &InputChord> {
        [&self.aim, &self.fire, &self.reload]
            .into_iter()
            .filter_map(Option::as_ref)
    }
}

impl Default for ActionBindings {
    fn default() -> Self {
        let mouse = |button| {
            Some(InputChord {
                trigger: InputKey::Mouse { button },
                modifiers: vec![],
            })
        };
        Self {
            aim: mouse(2),
            fire: mouse(1),
            reload: Some(InputChord::key(0x13)),
            aim_action: AimAction::None,
        }
    }
}

impl Profile {
    pub fn referenced_presets(&self) -> impl Iterator<Item = Id> + '_ {
        std::iter::once(self.selected_preset)
            .chain(self.actions.aim_action.target())
            .chain(
                self.bindings
                    .iter()
                    .filter_map(|binding| binding.action.target()),
            )
    }

    pub fn remap_presets(&mut self, mut map: impl FnMut(Id) -> Id) {
        self.selected_preset = map(self.selected_preset);
        self.actions.aim_action.remap(&mut map);
        for binding in &mut self.bindings {
            if let crate::BindingAction::SelectPreset { preset } = &mut binding.action {
                *preset = map(*preset);
            }
        }
    }

    /// Upgrade old profiles without stealing any existing shortcut.
    pub fn install_default_actions(&mut self, global: Option<&InputChord>) {
        self.actions = ActionBindings::unbound();
        let defaults = ActionBindings::default();
        for role in 0..3 {
            let candidate = [
                defaults.aim.clone(),
                defaults.fire.clone(),
                defaults.reload.clone(),
            ][role]
                .clone();
            match role {
                0 => self.actions.aim = candidate,
                1 => self.actions.fire = candidate,
                _ => self.actions.reload = candidate,
            }
            if crate::validation::validate_action_conflicts(&self.bindings, &self.actions, global)
                .is_err()
            {
                match role {
                    0 => self.actions.aim = None,
                    1 => self.actions.fire = None,
                    _ => self.actions.reload = None,
                }
            }
        }
    }
}

/// The caller keeps its original file until the migrated state is saved atomically.
pub fn migrate_state(mut state: AppState) -> Result<AppState, ValidationError> {
    match state.schema_version {
        1 => {
            for profile in &mut state.profiles {
                profile.install_default_actions(state.settings.global_toggle.as_ref());
            }
            state.schema_version = crate::SCHEMA_VERSION;
        }
        crate::SCHEMA_VERSION => {}
        _ => {
            return Err(ValidationError::new(
                "schema_version",
                "unsupported configuration version",
            ));
        }
    }
    crate::validate_state(&state)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Animation, AnimationTarget, Binding, BindingAction, Content, Modifier, validate_state,
    };

    #[test]
    fn v1_migration_preserves_content_and_does_not_steal_existing_keys() {
        let mut state = AppState::default();
        state.profiles[0].bindings.push(Binding::new(
            ActionBindings::default().fire.unwrap(),
            BindingAction::SelectPreset {
                preset: state.presets[1].id,
            },
        ));
        let mut json = serde_json::to_value(&state).unwrap();
        json["schema_version"] = 1.into();
        json["profiles"][0]
            .as_object_mut()
            .unwrap()
            .remove("actions");
        // Retired v1 bindings must not become new Aim actions.
        let mut retired = json["profiles"][0]["bindings"][0].clone();
        retired["action"] = serde_json::json!({"type":"hide_while_held"});
        json["profiles"][0]["bindings"]
            .as_array_mut()
            .unwrap()
            .push(retired);
        let old: AppState = serde_json::from_value(json).unwrap();
        assert_eq!(old.profiles[0].actions, ActionBindings::unbound());
        let migrated = migrate_state(old).unwrap();
        assert_eq!(migrated.schema_version, 2);
        assert_eq!(migrated.presets, state.presets);
        assert_eq!(migrated.profiles[0].bindings, state.profiles[0].bindings);
        assert_eq!(migrated.profiles[0].actions.aim_action, AimAction::None);
        assert_eq!(migrated.profiles[0].actions.fire, None);
        assert_eq!(
            migrated.profiles[0].actions.aim,
            ActionBindings::default().aim
        );
        assert_eq!(migrate_state(migrated.clone()).unwrap(), migrated);
        state.schema_version = 3;
        assert!(migrate_state(state).is_err());
    }

    #[test]
    fn all_profile_references_remap_and_missing_aim_targets_are_rejected() {
        let mut state = AppState::default();
        let target = state.presets[1].id;
        let profile = &mut state.profiles[0];
        profile.actions.aim_action = AimAction::ShowPresetWhileHeld { preset: target };
        profile.bindings.push(Binding::new(
            InputChord::key(0x20),
            BindingAction::SelectPreset { preset: target },
        ));
        assert_eq!(
            profile.referenced_presets().collect::<Vec<_>>(),
            vec![profile.selected_preset, target, target]
        );
        let replacement = state.presets[2].id;
        profile.remap_presets(|id| if id == target { replacement } else { id });
        assert!(profile.referenced_presets().all(|id| id != target));
        validate_state(&state).unwrap();
        state.profiles[0].actions.aim_action = AimAction::TogglePreset {
            preset: Id::new_v4(),
        };
        assert!(validate_state(&state).is_err());
    }

    #[test]
    fn action_conflicts_include_roles_shortcuts_global_and_release_capability() {
        let mut state = AppState::default();
        state.profiles[0].actions.fire = state.profiles[0].actions.aim.clone();
        assert!(validate_state(&state).is_err());
        state.profiles[0].actions = ActionBindings::default();
        state.settings.global_toggle = state.profiles[0].actions.aim.clone();
        assert!(validate_state(&state).is_err());
        state.settings.global_toggle = None;
        state.profiles[0].actions.aim.as_mut().unwrap().trigger = InputKey::Wheel { up: true };
        assert!(validate_state(&state).is_err());
        state.profiles[0].actions.aim.as_mut().unwrap().trigger = InputKey::Pause;
        assert!(validate_state(&state).is_err());
        state.profiles[0].actions.aim = None;
        state.profiles[0].actions.reload.as_mut().unwrap().trigger = InputKey::Wheel { up: true };
        validate_state(&state).unwrap();
        let mut specific = ActionBindings::default().fire.unwrap();
        specific.modifiers.push(Modifier::Shift);
        state.profiles[0].bindings.push(Binding::new(
            specific,
            BindingAction::SelectPreset {
                preset: state.presets[1].id,
            },
        ));
        validate_state(&state).unwrap();
    }

    #[test]
    fn animation_definitions_round_trip_and_reject_invalid_targets() {
        let mut state = AppState::default();
        state.presets[0].layers[0].animation = Some(Animation {
            target: AnimationTarget {
                thickness: Some(3.0),
                x: Some(20.0),
                ..Default::default()
            },
            ..Default::default()
        });
        validate_state(&state).unwrap();
        let encoded = serde_json::to_value(&state).unwrap();
        assert_eq!(
            serde_json::from_value::<AppState>(encoded.clone()).unwrap(),
            state
        );
        let mut unknown = encoded.clone();
        unknown["presets"][0]["layers"][0]["animation"]["target"]["script"] = "bad".into();
        assert!(serde_json::from_value::<AppState>(unknown).is_err());
        let a = state.presets[0].layers[0].animation.as_mut().unwrap();
        a.duration_ms = 0;
        assert!(validate_state(&state).is_err());
        state.presets[0].layers[0]
            .animation
            .as_mut()
            .unwrap()
            .duration_ms = 10001;
        assert!(validate_state(&state).is_err());
        state.presets[0].layers[0]
            .animation
            .as_mut()
            .unwrap()
            .duration_ms = 150;
        state.presets[0].layers[0]
            .animation
            .as_mut()
            .unwrap()
            .target
            .x = Some(f32::NAN);
        assert!(validate_state(&state).is_err());
        state.presets[0].layers[0]
            .animation
            .as_mut()
            .unwrap()
            .target
            .x = Some(0.0);
        state.presets[0].layers[0].content = Content::Rectangle(Default::default());
        assert!(validate_state(&state).is_err());
    }
}
