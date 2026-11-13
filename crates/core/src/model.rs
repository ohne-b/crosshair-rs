use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Id, SCHEMA_VERSION};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const ORANGE: Self = Self::rgba(249, 115, 22, 255);
    pub const WHITE: Self = Self::rgba(244, 244, 245, 255);
    pub const BLACK: Self = Self::rgba(0, 0, 0, 255);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(value: &str) -> Option<Self> {
        let value = value.strip_prefix('#').unwrap_or(value);
        if !matches!(value.len(), 6 | 8) || !value.is_ascii() {
            return None;
        }
        let byte = |offset| u8::from_str_radix(&value[offset..offset + 2], 16).ok();
        Some(Self::rgba(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            if value.len() == 8 { byte(6)? } else { 255 },
        ))
    }

    pub fn hex_rgb(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::ORANGE
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub fn fractions(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Left => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::Right => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub rotation: f32,
    pub anchor: Anchor,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            rotation: 0.0,
            anchor: Anchor::Center,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outline {
    pub enabled: bool,
    pub color: Color,
    pub thickness: f32,
    pub blur: f32,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            enabled: false,
            color: Color::BLACK,
            thickness: 1.0,
            blur: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    pub blur: f32,
    pub outline: Outline,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub transform: Transform,
    pub opacity: f32,
    pub effects: Effects,
    pub content: Content,
}

impl Layer {
    pub fn new(name: impl Into<String>, content: Content) -> Self {
        Self {
            id: Id::new_v4(),
            name: name.into(),
            visible: true,
            locked: false,
            transform: Transform::default(),
            opacity: 1.0,
            effects: Effects::default(),
            content,
        }
    }

    pub fn duplicate(&self) -> Self {
        let mut layer = self.clone();
        layer.id = Id::new_v4();
        layer
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "properties",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Content {
    Crosshair(Crosshair),
    Rectangle(Rectangle),
    Ellipse(Shape),
    Triangle(Shape),
    Text(Text),
    Image(Image),
    Freehand(Freehand),
}

impl Content {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Crosshair(_) => "Crosshair",
            Self::Rectangle(_) => "Rectangle",
            Self::Ellipse(_) => "Ellipse",
            Self::Triangle(_) => "Triangle",
            Self::Text(_) => "Text",
            Self::Image(_) => "Image",
            Self::Freehand(_) => "Drawing",
        }
    }

    pub fn asset_id(&self) -> Option<Id> {
        match self {
            Self::Image(image) => Some(image.asset),
            Self::Crosshair(Crosshair {
                shape: ArmShape::Image(image),
                ..
            }) => Some(image.asset),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crosshair {
    pub arms: u8,
    pub enabled_arms: Vec<bool>,
    pub length: f32,
    pub thickness: f32,
    pub gap: f32,
    pub rotation: f32,
    pub color: Color,
    pub blur: f32,
    pub shape: ArmShape,
    pub dot: Dot,
}

impl Default for Crosshair {
    fn default() -> Self {
        Self {
            arms: 4,
            enabled_arms: vec![true; 4],
            length: 10.0,
            thickness: 2.0,
            gap: 4.0,
            rotation: 0.0,
            color: Color::ORANGE,
            blur: 0.0,
            shape: ArmShape::Rectangle,
            dot: Dot::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "properties",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ArmShape {
    #[default]
    Rectangle,
    Triangle,
    Chevron {
        inward: bool,
        depth: f32,
    },
    Arc {
        radius: f32,
        sweep: f32,
    },
    TShape {
        width: f32,
        thickness: f32,
    },
    Text(Text),
    Image(Image),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DotShape {
    #[default]
    Circle,
    Square,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dot {
    pub enabled: bool,
    pub shape: DotShape,
    pub diameter: f32,
    pub color: Color,
    pub blur: f32,
}

impl Default for Dot {
    fn default() -> Self {
        Self {
            enabled: false,
            shape: DotShape::Circle,
            diameter: 2.0,
            color: Color::ORANGE,
            blur: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub width: f32,
    pub height: f32,
    pub color: Color,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            width: 20.0,
            height: 20.0,
            color: Color::ORANGE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rectangle {
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub color: Color,
}

impl Default for Rectangle {
    fn default() -> Self {
        Self {
            width: 20.0,
            height: 20.0,
            radius: 0.0,
            color: Color::ORANGE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub content: String,
    pub font_family: String,
    pub font_weight: u16,
    pub italic: bool,
    pub size: f32,
    pub spacing: f32,
    pub color: Color,
    pub layout: TextLayout,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            content: "+".into(),
            font_family: "Manrope".into(),
            font_weight: 600,
            italic: false,
            size: 16.0,
            spacing: 0.0,
            color: Color::ORANGE,
            layout: TextLayout::Straight,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextLayout {
    #[default]
    Straight,
    Circle {
        radius: f32,
        start_angle: f32,
        repeat: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub asset: Id,
    pub width: f32,
    pub height: f32,
    pub crop: Crop,
    pub aspect_locked: bool,
}

impl Image {
    pub fn new(asset: Id, width: f32, height: f32) -> Self {
        Self {
            asset,
            width,
            height,
            crop: Crop::default(),
            aspect_locked: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Freehand {
    pub strokes: Vec<Stroke>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub points: Vec<Point>,
    pub width: f32,
    pub color: Color,
    pub erase: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: Id,
    pub name: String,
    pub revision: u64,
    #[serde(default)]
    pub modified_order: u64,
    pub favorite: bool,
    pub layers: Vec<Layer>,
}

impl Preset {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Id::new_v4(),
            name: name.into(),
            revision: 0,
            modified_order: 0,
            favorite: false,
            layers: vec![Layer::new(
                "Crosshair",
                Content::Crosshair(Crosshair::default()),
            )],
        }
    }

    pub fn duplicate(&self, name: impl Into<String>) -> Self {
        Self {
            id: Id::new_v4(),
            name: name.into(),
            revision: 0,
            modified_order: 0,
            favorite: self.favorite,
            layers: self.layers.iter().map(Layer::duplicate).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    Desktop,
    GameBar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordStyle {
    #[default]
    Simple,
    Quote,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub output_mode: OutputMode,
    pub monitor: Option<String>,
    pub launch_at_login: bool,
    pub show_on_launch: bool,
    pub check_updates: bool,
    #[serde(default = "discord_presence_default")]
    pub discord_presence: bool,
    #[serde(default)]
    pub discord_style: DiscordStyle,
    pub global_toggle: Option<InputChord>,
}

fn discord_presence_default() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_mode: OutputMode::Desktop,
            monitor: None,
            launch_at_login: false,
            show_on_launch: true,
            check_updates: true,
            discord_presence: discord_presence_default(),
            discord_style: DiscordStyle::Simple,
            global_toggle: Some(InputChord {
                trigger: InputKey::Keyboard {
                    scan_code: 0x2c,
                    extended: false,
                },
                modifiers: vec![Modifier::Shift, Modifier::Alt],
            }),
        }
    }
}

#[cfg(test)]
mod discord_settings_tests {
    use super::*;

    #[test]
    fn presence_defaults_on_and_saved_opt_out_round_trips() {
        assert!(Settings::default().discord_presence);
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        old.as_object_mut().unwrap().remove("discord_presence");
        old.as_object_mut().unwrap().remove("discord_style");
        let mut settings: Settings = serde_json::from_value(old).unwrap();
        assert!(settings.discord_presence);
        assert_eq!(settings.discord_style, DiscordStyle::Simple);
        settings.discord_presence = false;
        settings.discord_style = DiscordStyle::Quote;
        assert_eq!(
            serde_json::from_value::<Settings>(serde_json::to_value(&settings).unwrap()).unwrap(),
            settings
        );
        let mut invalid = serde_json::to_value(settings).unwrap();
        invalid["discord_style"] = "unknown".into();
        assert!(serde_json::from_value::<Settings>(invalid).is_err());
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPosition {
    pub id: Id,
    pub name: String,
    pub offset: Point,
    pub scale: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: Id,
    pub name: String,
    pub selected_preset: Id,
    pub scale: f32,
    pub offset: Point,
    pub positions: Vec<SavedPosition>,
    #[serde(deserialize_with = "deserialize_bindings")]
    pub bindings: Vec<Binding>,
}

impl Profile {
    pub fn new(name: impl Into<String>, selected_preset: Id) -> Self {
        Self {
            id: Id::new_v4(),
            name: name.into(),
            selected_preset,
            scale: 1.0,
            offset: Point::default(),
            positions: Vec::new(),
            bindings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Png,
    Jpeg,
    Gif,
    Svg,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub sha256: String,
    pub kind: AssetKind,
    pub source_bytes: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppState {
    pub schema_version: u32,
    pub revision: u64,
    pub settings: Settings,
    pub presets: Vec<Preset>,
    pub profiles: Vec<Profile>,
    pub active_profile: Id,
    pub base_visible: bool,
    pub assets: BTreeMap<Id, Asset>,
}

impl AppState {
    pub fn active_profile(&self) -> Option<&Profile> {
        self.profiles
            .iter()
            .find(|profile| profile.id == self.active_profile)
    }

    pub fn active_profile_mut(&mut self) -> Option<&mut Profile> {
        self.profiles
            .iter_mut()
            .find(|profile| profile.id == self.active_profile)
    }

    pub fn preset(&self, id: Id) -> Option<&Preset> {
        self.presets.iter().find(|p| p.id == id)
    }
    pub fn preset_mut(&mut self, id: Id) -> Option<&mut Preset> {
        self.presets.iter_mut().find(|p| p.id == id)
    }
    pub fn selected_preset(&self) -> Option<&Preset> {
        self.preset(self.active_profile()?.selected_preset)
    }
}

impl Default for AppState {
    fn default() -> Self {
        let mut classic = Preset::new("Classic");
        classic.favorite = true;
        let mut dot = Preset::new("Dot");
        if let Content::Crosshair(shape) = &mut dot.layers[0].content {
            shape.length = 0.0;
            shape.dot.enabled = true;
            shape.dot.color = Color::WHITE;
        }
        let mut wide = Preset::new("Wide");
        if let Content::Crosshair(shape) = &mut wide.layers[0].content {
            shape.gap = 9.0;
            shape.length = 8.0;
            shape.color = Color::rgba(119, 220, 153, 255);
        }
        let profile = Profile::new("Default", classic.id);
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            settings: Settings::default(),
            presets: vec![classic, dot, wide],
            active_profile: profile.id,
            profiles: vec![profile],
            base_visible: true,
            assets: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputKey {
    Keyboard { scan_code: u16, extended: bool },
    Mouse { button: u8 },
    Wheel { up: bool },
    Pause,
}

impl InputKey {
    pub fn is_pulse(self) -> bool {
        matches!(self, Self::Wheel { .. } | Self::Pause)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Shift,
    Control,
    Alt,
    Windows,
    LeftShift,
    RightShift,
    LeftControl,
    RightControl,
    LeftAlt,
    RightAlt,
    LeftWindows,
    RightWindows,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputChord {
    pub trigger: InputKey,
    pub modifiers: Vec<Modifier>,
}

impl InputChord {
    pub fn key(scan_code: u16) -> Self {
        Self {
            trigger: InputKey::Keyboard {
                scan_code,
                extended: false,
            },
            modifiers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BindingAction {
    ToggleVisibility,
    SelectPreset { preset: Id },
}

impl BindingAction {
    pub fn target(&self) -> Option<Id> {
        match *self {
            Self::SelectPreset { preset } => Some(preset),
            Self::ToggleVisibility => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: Id,
    #[serde(default)]
    pub name: String,
    pub enabled: bool,
    pub chord: InputChord,
    pub action: BindingAction,
}

impl Binding {
    pub fn new(chord: InputChord, action: BindingAction) -> Self {
        Self {
            id: Id::new_v4(),
            name: String::new(),
            enabled: true,
            chord,
            action,
        }
    }
}

fn deserialize_bindings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Binding>, D::Error> {
    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
    enum StoredAction {
        ToggleVisibility {},
        SelectPreset { preset: Id },
        HideWhileHeld {},
        ShowWhileHeld {},
        TogglePreset { preset: Id },
        ShowPresetWhileHeld { preset: Id },
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StoredBinding {
        id: Id,
        #[serde(default)]
        name: String,
        enabled: bool,
        chord: InputChord,
        action: StoredAction,
    }
    let bindings = Vec::<StoredBinding>::deserialize(deserializer)?;
    Ok(bindings
        .into_iter()
        .filter_map(|binding| {
            let action = match binding.action {
                StoredAction::ToggleVisibility {} => BindingAction::ToggleVisibility,
                StoredAction::SelectPreset { preset } => BindingAction::SelectPreset { preset },
                StoredAction::HideWhileHeld {} | StoredAction::ShowWhileHeld {} => return None,
                StoredAction::TogglePreset { preset }
                | StoredAction::ShowPresetWhileHeld { preset } => {
                    let _ = preset;
                    return None;
                }
            };
            Some(Binding {
                id: binding.id,
                name: binding.name,
                enabled: binding.enabled,
                chord: binding.chord,
                action,
            })
        })
        .collect())
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_actions_are_removed_without_resetting_profile_or_shortcuts() {
        let mut state = AppState::default();
        let mut binding = Binding::new(
            InputChord::key(0x12),
            BindingAction::SelectPreset {
                preset: state.presets[1].id,
            },
        );
        binding.name = "My shortcut".into();
        state.profiles[0].bindings.push(binding);
        let mut stored = serde_json::to_value(&state).unwrap();
        let bindings = stored["profiles"][0]["bindings"].as_array_mut().unwrap();
        let mut old_shortcut = bindings[0].clone();
        old_shortcut.as_object_mut().unwrap().remove("name");
        old_shortcut["id"] = json!(Id::new_v4());
        bindings.push(old_shortcut);
        for action in [
            json!({"type":"hide_while_held"}),
            json!({"type":"show_while_held"}),
            json!({"type":"toggle_preset","preset":state.presets[0].id}),
            json!({"type":"show_preset_while_held","preset":state.presets[0].id}),
        ] {
            let mut retired = bindings[0].clone();
            retired["id"] = json!(Id::new_v4());
            retired["action"] = action;
            bindings.push(retired);
        }
        let restored: AppState = serde_json::from_value(stored).unwrap();
        assert_eq!(restored.presets, state.presets);
        assert_eq!(restored.active_profile, state.active_profile);
        assert_eq!(restored.profiles[0].id, state.profiles[0].id);
        assert_eq!(
            restored.profiles[0].selected_preset,
            state.profiles[0].selected_preset
        );
        assert_eq!(restored.profiles[0].bindings.len(), 2);
        assert_eq!(
            restored.profiles[0].bindings[0],
            state.profiles[0].bindings[0]
        );
        assert_eq!(restored.profiles[0].bindings[1].name, "");
    }

    #[test]
    fn unknown_and_malformed_retired_actions_still_fail() {
        let mut profile = Profile::new("Synthetic", Id::new_v4());
        profile.bindings.push(Binding::new(
            InputChord::key(0x12),
            BindingAction::ToggleVisibility,
        ));
        let original = serde_json::to_value(profile).unwrap();
        for action in [
            json!({"type":"future_action"}),
            json!({"type":"toggle_preset"}),
            json!({"type":"show_preset_while_held","preset":"invalid"}),
            json!({"type":"hide_while_held","extra":true}),
        ] {
            let mut stored = original.clone();
            stored["bindings"][0]["action"] = action;
            assert!(
                serde_json::from_value::<Profile>(stored.clone()).is_err(),
                "accepted {stored}"
            );
        }
        assert!(
            serde_json::from_value::<BindingAction>(json!({"type":"hide_while_held"})).is_err()
        );
    }
}
