use base64::{Engine as _, engine::general_purpose::STANDARD};
use crosshair_showcase::{ErrorCode, PresetValidator, ValidatedPreset, ValidationLimits, Warning};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SAMPLE: &[u8] = include_bytes!("fixtures/preset-v1.json");
const ASSET_ID: &str = "00000000-0000-4000-b000-000000000005";

fn document() -> Value {
    serde_json::from_slice(SAMPLE).unwrap()
}
fn validate(value: &Value) -> crosshair_showcase::Result<ValidatedPreset> {
    PresetValidator::default().validate(&serde_json::to_vec(value).unwrap())
}
fn asset(value: &mut Value, kind: &str, bytes: &[u8]) {
    value["assets"][0]["data"] = STANDARD.encode(bytes).into();
    value["state"]["assets"][ASSET_ID]["source_bytes"] = (bytes.len() as u64).into();
    value["state"]["assets"][ASSET_ID]["sha256"] = format!("{:x}", Sha256::digest(bytes)).into();
    value["state"]["assets"][ASSET_ID]["kind"] = kind.into();
}
fn svg(body: &str) -> Vec<u8> {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">{body}</svg>"#)
        .into_bytes()
}

#[test]
fn synthetic_export_validates_and_original_bytes_are_not_rewritten() {
    let preset = PresetValidator::default().validate(SAMPLE).unwrap();
    assert_eq!(preset.original_json(), SAMPLE);
    assert_eq!(preset.metadata().name, "Training Grid");
    assert_eq!(preset.metadata().layers, 2);
    assert_eq!(preset.metadata().assets, 1);
    assert_eq!(preset.metadata().decoded_asset_bytes, 115);
    assert_eq!(preset.metadata().format_version, 1);
    assert_eq!(preset.metadata().rendered_schema_version, 2);
    assert!(
        preset
            .metadata()
            .warnings
            .contains(&Warning::LegacyVersionMigrated)
    );
    assert!(
        preset
            .crosshair_png()
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
}

#[test]
fn version_two_is_supported_without_migration_warning() {
    let mut value = document();
    value["version"] = 2.into();
    value["state"]["schema_version"] = 2.into();
    let preset = validate(&value).unwrap();
    assert!(
        !preset
            .metadata()
            .warnings
            .contains(&Warning::LegacyVersionMigrated)
    );
}

#[test]
fn wrong_format_version_kind_and_mismatched_schema_are_rejected() {
    for (key, replacement) in [
        ("format", json!("something-else")),
        ("version", json!(99)),
        ("kind", json!("all")),
        ("kind", json!("profile")),
    ] {
        let mut value = document();
        value[key] = replacement;
        assert_eq!(
            validate(&value).unwrap_err().code(),
            ErrorCode::UnsupportedFormat
        );
    }
    let mut value = document();
    value["version"] = 2.into();
    assert!(validate(&value).is_err());
}

#[test]
fn unknown_fields_at_each_level_are_rejected() {
    let mut value = document();
    value["extra"] = true.into();
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["extra"] = true.into();
    assert!(validate(&value).is_err());
    let mut value = document();
    value["assets"][0]["extra"] = true.into();
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["presets"][0]["layers"][0]["extra"] = true.into();
    assert!(validate(&value).is_err());
}

#[test]
fn raw_duplicate_keys_and_trailing_json_are_rejected() {
    let original = std::str::from_utf8(SAMPLE).unwrap();
    let duplicate = original.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    assert!(
        PresetValidator::default()
            .validate(duplicate.as_bytes())
            .is_err()
    );
    let duplicate = original.replacen("\"opacity\":1.0", "\"opacity\":1.0,\"opacity\":1.0", 1);
    assert!(
        PresetValidator::default()
            .validate(duplicate.as_bytes())
            .is_err()
    );
    assert!(
        PresetValidator::default()
            .validate(format!("{original} {{}}").as_bytes())
            .is_err()
    );
}

#[test]
fn exactly_one_preset_and_profile_are_required() {
    let mut value = document();
    let extra = value["state"]["presets"][0].clone();
    value["state"]["presets"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["profiles"] = json!([]);
    assert!(validate(&value).is_err());
}

#[test]
fn duplicate_asset_ids_and_uuid_key_aliases_are_rejected() {
    let mut value = document();
    let extra = value["assets"][0].clone();
    value["assets"].as_array_mut().unwrap().push(extra);
    assert!(validate(&value).is_err());
    let mut value = document();
    let extra = value["state"]["assets"][ASSET_ID].clone();
    value["state"]["assets"]
        .as_object_mut()
        .unwrap()
        .insert(ASSET_ID.to_uppercase(), extra);
    assert!(validate(&value).is_err());
}

#[test]
fn missing_assets_metadata_and_broken_references_are_rejected() {
    let mut value = document();
    value["assets"] = json!([]);
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["assets"] = json!({});
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["presets"][0]["layers"][1]["content"]["properties"]["asset"] =
        "d7519425-1f28-441b-94ca-12933ce19b71".into();
    assert!(validate(&value).is_err());
}

#[test]
fn unreferenced_assets_are_rejected_not_repackaged() {
    let mut value = document();
    value["state"]["presets"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(validate(&value).is_err());
}

#[test]
fn invalid_base64_length_hash_and_dimensions_are_rejected() {
    let mut value = document();
    value["assets"][0]["data"] = "!!!".into();
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["assets"][ASSET_ID]["source_bytes"] = 7.into();
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["assets"][ASSET_ID]["sha256"] = "0".repeat(64).into();
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["assets"][ASSET_ID]["width"] = 64.into();
    assert!(validate(&value).is_err());
}

#[test]
fn nil_ids_duplicate_layers_bad_ranges_and_unknown_variants_are_rejected() {
    let mut value = document();
    value["state"]["presets"][0]["id"] = "00000000-0000-0000-0000-000000000000".into();
    assert!(validate(&value).is_err());
    let mut value = document();
    value["state"]["presets"][0]["layers"][1]["id"] =
        value["state"]["presets"][0]["layers"][0]["id"].clone();
    assert!(validate(&value).is_err());
    for opacity in [json!(-1), json!(2)] {
        let mut value = document();
        value["state"]["presets"][0]["layers"][0]["opacity"] = opacity;
        assert!(validate(&value).is_err());
    }
    let mut value = document();
    value["state"]["presets"][0]["layers"][0]["content"]["type"] = "shell_command".into();
    assert!(validate(&value).is_err());
}

#[test]
fn geometry_that_exceeds_the_native_renderer_canvas_is_rejected() {
    let mut value = document();
    value["state"]["presets"][0]["layers"][1]["transform"]["scale"] = 100.into();
    assert!(validate(&value).is_err());
}

#[test]
fn no_visible_pixels_is_not_a_valid_showcase() {
    let mut value = document();
    for layer in value["state"]["presets"][0]["layers"]
        .as_array_mut()
        .unwrap()
    {
        layer["visible"] = false.into();
    }
    assert_eq!(validate(&value).unwrap_err().code(), ErrorCode::EmptyPreset);
}

#[test]
fn png_decoder_errors_are_not_hidden_by_invisible_layers() {
    let mut value = document();
    asset(&mut value, "png", b"\x89PNG\r\n\x1a\ntruncated");
    value["state"]["presets"][0]["layers"][1]["visible"] = false.into();
    assert!(validate(&value).is_err());
}

#[test]
fn declared_raster_kind_must_match_the_signature() {
    let mut value = document();
    value["state"]["assets"][ASSET_ID]["kind"] = "png".into();
    assert!(validate(&value).is_err());
}

#[test]
fn svg_active_content_external_resources_and_recursive_use_are_rejected() {
    for body in [
        "<script>alert(1)</script>",
        "<foreignObject><p>bad</p></foreignObject>",
        "<animate attributeName=\"opacity\" from=\"0\" to=\"1\"/>",
        "<rect width=\"10\" height=\"10\" onclick=\"bad()\"/>",
        "<image href=\"file:///etc/passwd\" width=\"32\" height=\"32\"/>",
        "<image href=\"https://example.invalid/x.png\" width=\"32\" height=\"32\"/>",
        "<style>@import url(https://example.invalid/a.css);</style>",
        "<g id=\"cycle\"><use href=\"#cycle\"/></g><use href=\"#cycle\"/>",
    ] {
        let mut value = document();
        asset(&mut value, "svg", &svg(body));
        assert!(validate(&value).is_err(), "unexpectedly accepted: {body}");
    }
}

#[test]
fn svg_text_requires_outlines_instead_of_server_font_substitution() {
    let mut value = document();
    asset(&mut value, "svg", &svg("<text x=\"0\" y=\"16\">A</text>"));
    assert_eq!(
        validate(&value).unwrap_err().code(),
        ErrorCode::UnsupportedFeature
    );
}

#[test]
fn svg_dtd_is_rejected() {
    let mut value = document();
    let data = b"<!DOCTYPE svg [<!ENTITY x 'expanded'>]><svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'>&x;</svg>";
    asset(&mut value, "svg", data);
    assert!(validate(&value).is_err());
}

#[test]
fn lower_input_asset_and_layer_budgets_are_enforced() {
    let limits = ValidationLimits {
        max_json_bytes: SAMPLE.len() - 1,
        ..Default::default()
    };
    assert_eq!(
        PresetValidator::new(limits)
            .unwrap()
            .validate(SAMPLE)
            .unwrap_err()
            .code(),
        ErrorCode::LimitExceeded
    );
    let limits = ValidationLimits {
        max_layers: 1,
        ..Default::default()
    };
    assert!(
        PresetValidator::new(limits)
            .unwrap()
            .validate(SAMPLE)
            .is_err()
    );
    let limits = ValidationLimits {
        max_total_asset_bytes: 100,
        ..Default::default()
    };
    assert!(
        PresetValidator::new(limits)
            .unwrap()
            .validate(SAMPLE)
            .is_err()
    );
    let limits = ValidationLimits {
        max_image_pixels: 100,
        ..Default::default()
    };
    assert!(
        PresetValidator::new(limits)
            .unwrap()
            .validate(SAMPLE)
            .is_err()
    );
}

#[test]
fn limits_cannot_disable_the_hard_caps() {
    let limits = ValidationLimits {
        max_json_bytes: usize::MAX,
        ..Default::default()
    };
    assert!(PresetValidator::new(limits).is_err());
    let limits = ValidationLimits {
        max_json_depth: 0,
        ..Default::default()
    };
    assert!(PresetValidator::new(limits).is_err());
}

#[test]
fn profile_positions_and_runtime_settings_never_control_the_card() {
    let normal = PresetValidator::default().validate(SAMPLE).unwrap();
    let mut value = document();
    value["state"]["profiles"][0]["scale"] = 2.into();
    value["state"]["profiles"][0]["offset"] = json!({"x":1000,"y":-400});
    value["state"]["settings"]["launch_at_login"] = true.into();
    value["state"]["base_visible"] = false.into();
    let modified = validate(&value).unwrap();
    assert_eq!(
        normal.crosshair_png().unwrap(),
        modified.crosshair_png().unwrap()
    );
    assert!(
        modified
            .metadata()
            .warnings
            .contains(&Warning::ProfilePlacementIgnored)
    );
}

#[test]
fn animation_is_explicitly_a_base_state_preview() {
    let mut value = document();
    value["version"] = 2.into();
    value["state"]["schema_version"] = 2.into();
    value["state"]["presets"][0]["layers"][0]["animation"] =
        json!({"enabled":true,"target":{"opacity":0.5}});
    let preset = validate(&value).unwrap();
    assert!(
        preset
            .metadata()
            .warnings
            .contains(&Warning::AnimationBaseStateOnly)
    );
}

#[test]
fn reused_ids_with_different_contents_cannot_reuse_a_previous_upload_cache() {
    let validator = PresetValidator::default();
    let first = validator.validate(SAMPLE).unwrap();
    let mut value = document();
    let decoded = STANDARD
        .decode(value["assets"][0]["data"].as_str().unwrap())
        .unwrap();
    let red = String::from_utf8(decoded)
        .unwrap()
        .replace("#FFFFFF", "#FF0000");
    asset(&mut value, "svg", red.as_bytes());
    let second = validator
        .validate(&serde_json::to_vec(&value).unwrap())
        .unwrap();
    assert_ne!(
        first.crosshair_png().unwrap(),
        second.crosshair_png().unwrap()
    );
    assert_eq!(first.metadata().id, second.metadata().id);
}

#[test]
fn same_input_produces_the_same_native_output_and_digest() {
    let validator = PresetValidator::default();
    let a = validator.validate(SAMPLE).unwrap();
    let b = validator.validate(SAMPLE).unwrap();
    assert_eq!(a.crosshair_png().unwrap(), b.crosshair_png().unwrap());
    assert_eq!(a.metadata().sha256, b.metadata().sha256);
    assert_eq!(a.download_filename("nova"), b.download_filename("nova"));
}
