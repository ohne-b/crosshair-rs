use std::collections::{BTreeMap, BTreeSet};

use crate::*;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {message}")]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

type Result = std::result::Result<(), ValidationError>;

fn require(condition: bool, field: &str, message: &str) -> Result {
    if condition {
        Ok(())
    } else {
        Err(ValidationError::new(field, message))
    }
}

fn number(value: f32, min: f32, max: f32, field: &str) -> Result {
    require(
        value.is_finite() && (min..=max).contains(&value),
        field,
        &format!("must be a finite value from {min} to {max}"),
    )
}

fn name(value: &str, field: &str) -> Result {
    require(
        !value.trim().is_empty()
            && value.chars().count() <= 80
            && !value.chars().any(char::is_control),
        field,
        "must contain 1-80 printable characters",
    )
}

fn unique_id(ids: &mut BTreeSet<Id>, id: Id, field: &str) -> Result {
    require(
        !id.is_nil() && ids.insert(id),
        field,
        "ID must be nonempty and unique",
    )
}

fn point(value: Point, limit: f32, field: &str) -> Result {
    number(value.x, -limit, limit, field)?;
    number(value.y, -limit, limit, field)
}

fn text(value: &Text) -> Result {
    require(
        value.content.chars().count() <= MAX_TEXT_CHARS,
        "text.content",
        "too many characters",
    )?;
    require(
        !value
            .content
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "text.content",
        "unsupported control character",
    )?;
    name(&value.font_family, "text.font_family")?;
    require(
        (1..=999).contains(&value.font_weight),
        "text.font_weight",
        "must be from 1 to 999",
    )?;
    number(value.size, 1.0, 1024.0, "text.size")?;
    number(value.spacing, -100.0, 100.0, "text.spacing")?;
    if let TextLayout::Circle {
        radius,
        start_angle,
        ..
    } = value.layout
    {
        require(
            !value.content.contains(['\n', '\t']),
            "text.content",
            "circular text must use one line",
        )?;
        number(radius, 1.0, 512.0, "text.radius")?;
        number(start_angle, -360.0, 360.0, "text.start_angle")?;
    }
    Ok(())
}

fn image(value: &Image, assets: Option<&BTreeMap<Id, Asset>>) -> Result {
    require(
        !value.asset.is_nil(),
        "image.asset",
        "asset ID cannot be empty",
    )?;
    if let Some(assets) = assets {
        require(
            assets.contains_key(&value.asset),
            "image.asset",
            "referenced asset is missing",
        )?;
    }
    number(value.width, 0.0, 1024.0, "image.width")?;
    number(value.height, 0.0, 1024.0, "image.height")?;
    number(value.crop.x, 0.0, 1.0, "image.crop.x")?;
    number(value.crop.y, 0.0, 1.0, "image.crop.y")?;
    number(value.crop.width, f32::EPSILON, 1.0, "image.crop.width")?;
    number(value.crop.height, f32::EPSILON, 1.0, "image.crop.height")?;
    require(
        value.crop.x + value.crop.width <= 1.0 + f32::EPSILON
            && value.crop.y + value.crop.height <= 1.0 + f32::EPSILON,
        "image.crop",
        "must be inside the source image",
    )
}

pub fn validate_layer(layer: &Layer, assets: &BTreeMap<Id, Asset>) -> Result {
    validate_layer_inner(layer, Some(assets))
}

pub fn validate_layer_geometry(layer: &Layer) -> Result {
    validate_layer_inner(layer, None)
}

fn validate_layer_inner(layer: &Layer, assets: Option<&BTreeMap<Id, Asset>>) -> Result {
    require(!layer.id.is_nil(), "layer.id", "ID cannot be empty")?;
    name(&layer.name, "layer.name")?;
    point(
        Point {
            x: layer.transform.x,
            y: layer.transform.y,
        },
        4096.0,
        "layer.position",
    )?;
    number(layer.transform.scale, 0.01, 100.0, "layer.scale")?;
    number(layer.transform.rotation, -360.0, 360.0, "layer.rotation")?;
    number(layer.opacity, 0.0, 1.0, "layer.opacity")?;
    number(layer.effects.blur, 0.0, 64.0, "layer.blur")?;
    number(
        layer.effects.outline.thickness,
        0.0,
        64.0,
        "outline.thickness",
    )?;
    number(layer.effects.outline.blur, 0.0, 64.0, "outline.blur")?;
    match &layer.content {
        Content::Crosshair(value) => {
            require(
                (1..=32).contains(&value.arms),
                "crosshair.arms",
                "must be from 1 to 32",
            )?;
            require(
                value.enabled_arms.len() == usize::from(value.arms),
                "crosshair.enabled_arms",
                "one enabled flag is required per arm",
            )?;
            number(value.length, 0.0, 512.0, "crosshair.length")?;
            number(value.thickness, 0.0, 512.0, "crosshair.thickness")?;
            number(value.gap, 0.0, 512.0, "crosshair.gap")?;
            number(value.rotation, -360.0, 360.0, "crosshair.rotation")?;
            number(value.blur, 0.0, 64.0, "crosshair.blur")?;
            number(value.dot.diameter, 0.0, 1024.0, "dot.diameter")?;
            number(value.dot.blur, 0.0, 64.0, "dot.blur")?;
            match &value.shape {
                ArmShape::Rectangle | ArmShape::Triangle => {}
                ArmShape::Chevron { depth, .. } => number(*depth, 0.0, 512.0, "chevron.depth")?,
                ArmShape::Arc { radius, sweep } => {
                    number(*radius, 0.0, 512.0, "arc.radius")?;
                    number(*sweep, 0.0, 360.0, "arc.sweep")?;
                }
                ArmShape::TShape { width, thickness } => {
                    number(*width, 0.0, 1024.0, "t_shape.width")?;
                    number(*thickness, 0.0, 512.0, "t_shape.thickness")?;
                }
                ArmShape::Text(value) => text(value)?,
                ArmShape::Image(value) => image(value, assets)?,
            }
        }
        Content::Rectangle(value) => {
            number(value.width, 0.0, 1024.0, "rectangle.width")?;
            number(value.height, 0.0, 1024.0, "rectangle.height")?;
            number(
                value.radius,
                0.0,
                value.width.min(value.height) / 2.0,
                "rectangle.radius",
            )?;
        }
        Content::Ellipse(value) | Content::Triangle(value) => {
            number(value.width, 0.0, 1024.0, "shape.width")?;
            number(value.height, 0.0, 1024.0, "shape.height")?;
        }
        Content::Text(value) => text(value)?,
        Content::Image(value) => image(value, assets)?,
        Content::Freehand(value) => {
            require(
                value.strokes.len() <= MAX_DRAWING_POINTS,
                "drawing.strokes",
                "too many strokes",
            )?;
            for stroke in &value.strokes {
                require(
                    !stroke.points.is_empty(),
                    "stroke.points",
                    "a stroke needs at least one point",
                )?;
                number(stroke.width, 0.1, 256.0, "stroke.width")?;
                for &position in &stroke.points {
                    point(position, 4096.0, "stroke.point")?;
                }
            }
        }
    }
    Ok(())
}

pub fn validate_preset(preset: &Preset, assets: &BTreeMap<Id, Asset>) -> Result {
    validate_preset_inner(preset, Some(assets))
}

pub fn validate_preset_geometry(preset: &Preset) -> Result {
    validate_preset_inner(preset, None)
}

fn validate_preset_inner(preset: &Preset, assets: Option<&BTreeMap<Id, Asset>>) -> Result {
    require(!preset.id.is_nil(), "preset.id", "ID cannot be empty")?;
    name(&preset.name, "preset.name")?;
    require(
        preset.layers.len() <= MAX_LAYERS,
        "preset.layers",
        "too many layers",
    )?;
    let mut ids = BTreeSet::new();
    let mut points = 0usize;
    for layer in &preset.layers {
        unique_id(&mut ids, layer.id, "layer.id")?;
        validate_layer_inner(layer, assets)?;
        if let Content::Freehand(drawing) = &layer.content {
            for stroke in &drawing.strokes {
                points = points
                    .checked_add(stroke.points.len())
                    .ok_or_else(|| ValidationError::new("drawing", "point count overflow"))?;
            }
        }
    }
    require(
        points <= MAX_DRAWING_POINTS,
        "preset.drawing",
        "too many drawing points",
    )
}

pub fn validate_chord(chord: &InputChord) -> Result {
    match chord.trigger {
        InputKey::Keyboard { scan_code, .. } => require(
            (1..=0x7f).contains(&scan_code),
            "chord.key",
            "invalid keyboard scan code",
        )?,
        InputKey::Mouse { button } => require(
            (1..=5).contains(&button),
            "chord.mouse",
            "mouse button must be from 1 to 5",
        )?,
        InputKey::Wheel { .. } | InputKey::Pause => {}
    }
    require(
        chord.modifiers.len() <= 8,
        "chord.modifiers",
        "too many modifiers",
    )?;
    let modifiers: BTreeSet<_> = chord.modifiers.iter().copied().collect();
    require(
        modifiers.len() == chord.modifiers.len(),
        "chord.modifiers",
        "duplicate modifier",
    )?;
    if let Some(trigger) = crate::input::modifier_bit(chord.trigger) {
        require(
            !chord
                .modifiers
                .iter()
                .any(|modifier| modifiers_match(&[*modifier], trigger)),
            "chord.modifiers",
            "trigger cannot also be a required modifier",
        )?;
    }
    for (generic, left, right) in [
        (Modifier::Shift, Modifier::LeftShift, Modifier::RightShift),
        (
            Modifier::Control,
            Modifier::LeftControl,
            Modifier::RightControl,
        ),
        (Modifier::Alt, Modifier::LeftAlt, Modifier::RightAlt),
        (
            Modifier::Windows,
            Modifier::LeftWindows,
            Modifier::RightWindows,
        ),
    ] {
        require(
            !modifiers.contains(&generic)
                || (!modifiers.contains(&left) && !modifiers.contains(&right)),
            "chord.modifiers",
            "generic and sided modifiers cannot be combined",
        )?;
    }
    Ok(())
}

pub fn validate_state(state: &AppState) -> Result {
    require(
        state.schema_version == SCHEMA_VERSION,
        "schema_version",
        "unsupported configuration version",
    )?;
    require(
        (1..=MAX_PRESETS).contains(&state.presets.len()),
        "presets",
        "must contain 1-500 presets",
    )?;
    require(
        (1..=MAX_PROFILES).contains(&state.profiles.len()),
        "profiles",
        "must contain 1-32 profiles",
    )?;
    let mut ids = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    let mut encoded_assets = 0u64;
    for (&id, asset) in &state.assets {
        unique_id(&mut ids, id, "asset.id")?;
        require(
            asset.sha256.len() == 64
                && asset
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "asset.sha256",
            "expected lowercase SHA-256",
        )?;
        require(
            hashes.insert(&asset.sha256),
            "asset.sha256",
            "duplicate content must reuse the existing asset ID",
        )?;
        require(
            (1..=MAX_ASSET_BYTES).contains(&asset.source_bytes),
            "asset.bytes",
            "asset exceeds source size limit",
        )?;
        require(
            asset.width > 0
                && asset.height > 0
                && u64::from(asset.width) * u64::from(asset.height) <= 16 * 1024 * 1024,
            "asset.dimensions",
            "decoded image exceeds 16 megapixels",
        )?;
        encoded_assets = encoded_assets
            .checked_add(asset.source_bytes.div_ceil(3) * 4 + 256)
            .ok_or_else(|| ValidationError::new("assets", "size overflow"))?;
    }
    for preset in &state.presets {
        unique_id(&mut ids, preset.id, "preset.id")?;
        validate_preset(preset, &state.assets)?;
        for layer in &preset.layers {
            unique_id(&mut ids, layer.id, "layer.id")?;
        }
    }
    for profile in &state.profiles {
        unique_id(&mut ids, profile.id, "profile.id")?;
        name(&profile.name, "profile.name")?;
        require(
            state.preset(profile.selected_preset).is_some(),
            "profile.selected_preset",
            "referenced preset is missing",
        )?;
        number(profile.scale, 0.05, 10.0, "profile.scale")?;
        point(profile.offset, 32_768.0, "profile.offset")?;
        require(
            profile.positions.len() <= 64,
            "profile.positions",
            "too many saved positions",
        )?;
        for position in &profile.positions {
            unique_id(&mut ids, position.id, "position.id")?;
            name(&position.name, "position.name")?;
            number(position.scale, 0.05, 10.0, "position.scale")?;
            point(position.offset, 32_768.0, "position.offset")?;
        }
        require(
            profile.bindings.len() <= MAX_BINDINGS,
            "profile.bindings",
            "too many bindings",
        )?;
        for binding in &profile.bindings {
            unique_id(&mut ids, binding.id, "binding.id")?;
            validate_chord(&binding.chord)?;
            if !binding.name.is_empty() {
                name(&binding.name, "binding.name")?;
            }
            if let Some(target) = binding.action.target() {
                require(
                    state.preset(target).is_some(),
                    "binding.target",
                    "referenced preset is missing",
                )?;
            }
        }
        validate_binding_conflicts(&profile.bindings, state.settings.global_toggle.as_ref())?;
    }
    require(
        state.active_profile().is_some(),
        "active_profile",
        "referenced profile is missing",
    )?;
    if let Some(chord) = &state.settings.global_toggle {
        validate_chord(chord)?;
    }
    if let Some(monitor) = &state.settings.monitor {
        require(
            monitor.len() <= 512 && !monitor.chars().any(char::is_control),
            "settings.monitor",
            "invalid display identifier",
        )?;
    }
    let bytes = serde_json::to_vec(state)
        .map_err(|e| ValidationError::new("state", e.to_string()))?
        .len();
    require(
        bytes <= MAX_CONFIG_BYTES,
        "state",
        "configuration exceeds 8 MiB",
    )?;
    require(
        bytes as u64 + encoded_assets + 1024 <= MAX_PORTABLE_BYTES,
        "state",
        "portable backup would exceed 512 MiB",
    )
}

/// Tests all physical modifier combinations so sided and generic chords cannot race.
pub fn validate_binding_conflicts(bindings: &[Binding], global: Option<&InputChord>) -> Result {
    let mut chords: Vec<&InputChord> = bindings
        .iter()
        .filter(|b| b.enabled)
        .map(|b| &b.chord)
        .collect();
    if let Some(global) = global {
        chords.push(global);
    }
    for (index, chord) in chords.iter().enumerate() {
        for other in chords.iter().skip(index + 1) {
            if chord.trigger != other.trigger {
                continue;
            }
            if chord_specificity(chord) != chord_specificity(other) {
                continue;
            }
            if (0..=u8::MAX).any(|bits| {
                modifiers_match(&chord.modifiers, bits) && modifiers_match(&other.modifiers, bits)
            }) {
                return Err(ValidationError::new(
                    "bindings",
                    "equally specific chords overlap",
                ));
            }
        }
    }
    Ok(())
}

pub fn chord_specificity(chord: &InputChord) -> usize {
    chord.modifiers.len() * 16
        + chord
            .modifiers
            .iter()
            .map(|m| match m {
                Modifier::Shift | Modifier::Control | Modifier::Alt | Modifier::Windows => 0,
                _ => 1,
            })
            .sum::<usize>()
}

/// Bits are L/R Shift, L/R Control, L/R Alt, L/R Windows, in that order.
pub fn modifiers_match(modifiers: &[Modifier], bits: u8) -> bool {
    modifiers.iter().all(|modifier| match modifier {
        Modifier::Shift => bits & 0b00000011 != 0,
        Modifier::Control => bits & 0b00001100 != 0,
        Modifier::Alt => bits & 0b00110000 != 0,
        Modifier::Windows => bits & 0b11000000 != 0,
        Modifier::LeftShift => bits & 1 != 0,
        Modifier::RightShift => bits & 2 != 0,
        Modifier::LeftControl => bits & 4 != 0,
        Modifier::RightControl => bits & 8 != 0,
        Modifier::LeftAlt => bits & 16 != 0,
        Modifier::RightAlt => bits & 32 != 0,
        Modifier::LeftWindows => bits & 64 != 0,
        Modifier::RightWindows => bits & 128 != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_and_validate() {
        let state = AppState::default();
        let restored: AppState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(state, restored);
        validate_state(&restored).unwrap();
    }

    #[test]
    fn nonfinite_geometry_and_missing_references_are_rejected() {
        let mut state = AppState::default();
        state.presets[0].layers[0].transform.scale = f32::NAN;
        assert!(validate_state(&state).is_err());
        state.presets[0].layers[0].transform.scale = 1.0;
        state.profiles[0].selected_preset = Id::new_v4();
        assert!(validate_state(&state).is_err());
    }

    #[test]
    fn duplicate_ids_unknown_schema_and_missing_assets_are_rejected() {
        let mut state = AppState::default();
        state.presets[1].layers[0].id = state.presets[0].layers[0].id;
        assert!(validate_state(&state).is_err());
        state = AppState::default();
        state.schema_version += 1;
        assert!(validate_state(&state).is_err());
        state = AppState::default();
        state.presets[0].layers[0].content = Content::Image(Image::new(Id::new_v4(), 20.0, 20.0));
        assert!(validate_state(&state).is_err());
    }

    #[test]
    fn bound_is_aggregate_across_drawing_layers() {
        let mut preset = Preset::new("Drawing");
        let drawing = Freehand {
            strokes: vec![Stroke {
                points: vec![Point::default(); 10_001],
                width: 1.0,
                color: Color::WHITE,
                erase: false,
            }],
        };
        preset.layers = vec![
            Layer::new("A", Content::Freehand(drawing.clone())),
            Layer::new("B", Content::Freehand(drawing)),
        ];
        assert!(validate_preset(&preset, &BTreeMap::new()).is_err());
    }

    #[test]
    fn modifier_overlap_is_checked_beyond_exact_equality() {
        let a = Binding::new(
            InputChord {
                trigger: InputChord::key(0x12).trigger,
                modifiers: vec![Modifier::Shift],
            },
            BindingAction::ToggleVisibility,
        );
        let b = Binding::new(
            InputChord {
                trigger: a.chord.trigger,
                modifiers: vec![Modifier::Control],
            },
            BindingAction::ToggleVisibility,
        );
        assert!(validate_binding_conflicts(&[a.clone(), b], None).is_err());
        let c = Binding::new(InputChord::key(0x12), BindingAction::ToggleVisibility);
        assert!(validate_binding_conflicts(&[a, c], None).is_ok());
    }

    #[test]
    fn wheel_shortcuts_and_bounded_names_and_colors() {
        let mut state = AppState::default();
        state.profiles[0].bindings.push(Binding::new(
            InputChord {
                trigger: InputKey::Wheel { up: true },
                modifiers: vec![],
            },
            BindingAction::SelectPreset {
                preset: state.presets[0].id,
            },
        ));
        assert!(validate_state(&state).is_ok());
        state.profiles[0].bindings[0].name = "x".repeat(81);
        assert!(validate_state(&state).is_err());
        state.profiles[0].bindings[0].name = "invalid\nname".into();
        assert!(validate_state(&state).is_err());
        assert_eq!(
            Color::from_hex("#F9731680"),
            Some(Color::rgba(249, 115, 22, 128))
        );
        assert!(Color::from_hex("éééééé").is_none());
    }

    #[test]
    fn portable_limit_counts_base64_and_all_library_assets() {
        let mut state = AppState::default();
        for index in 0..47 {
            state.assets.insert(
                Id::new_v4(),
                Asset {
                    sha256: format!("{index:064x}"),
                    kind: AssetKind::Png,
                    source_bytes: MAX_ASSET_BYTES,
                    width: 1,
                    height: 1,
                },
            );
        }
        validate_state(&state).unwrap();
        state.assets.insert(
            Id::new_v4(),
            Asset {
                sha256: format!("{:064x}", 47),
                kind: AssetKind::Png,
                source_bytes: MAX_ASSET_BYTES,
                width: 1,
                height: 1,
            },
        );
        assert!(
            validate_state(&state)
                .unwrap_err()
                .message
                .contains("portable")
        );
    }

    #[test]
    fn unknown_fields_are_not_silently_discarded() {
        let mut json = serde_json::to_value(AppState::default()).unwrap();
        json["presets"][0]["layers"][0]["content"]["future_effect"] = true.into();
        assert!(serde_json::from_value::<AppState>(json).is_err());
    }
}
