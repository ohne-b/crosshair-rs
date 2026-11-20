mod assets;
mod json;

use std::{collections::BTreeSet, sync::Arc};

use crosshair_core::{AppState, Id, migrate_state};
#[cfg(not(windows))]
use crosshair_core::{ArmShape, Content};
use crosshair_renderer::{AssetData, MAX_DIMENSION, Raster, Renderer};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Error, PresetMetadata, Result, ValidatedPreset, ValidationLimits, Warning};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableDocument {
    format: String,
    version: u32,
    kind: String,
    state: AppState,
    assets: Vec<assets::EmbeddedAsset>,
}

/// Immutable admission policy. A fresh upstream renderer is created for each
/// call: users can reuse UUIDs, so caches must NEVER cross upload boundaries.
#[derive(Clone, Debug, Default)]
pub struct PresetValidator {
    limits: ValidationLimits,
}

impl PresetValidator {
    pub fn new(limits: ValidationLimits) -> Result<Self> {
        Ok(Self {
            limits: limits.check()?,
        })
    }

    pub fn limits(&self) -> &ValidationLimits {
        &self.limits
    }

    /// Checks a standalone `kind: "preset"` export, including embedded assets,
    /// then renders its base state. Never applies settings, bindings, profiles,
    /// monitor positions, startup flags, or any other app state to this process.
    pub fn validate(&self, bytes: &[u8]) -> Result<ValidatedPreset> {
        if bytes.len() > self.limits.max_json_bytes {
            return Err(Error::LimitExceeded {
                resource: "JSON bytes",
                limit: self.limits.max_json_bytes as u64,
            });
        }
        let value = json::parse(bytes, &self.limits)?;
        let index_size = value
            .get("state")
            .and_then(|state| state.get("assets"))
            .and_then(serde_json::Value::as_object)
            .map(|index| index.len());
        let document: PortableDocument = serde_json::from_value(value).map_err(Error::Json)?;
        if document.format != "crosshair-rs" {
            return Err(Error::Format("format must be crosshair-rs"));
        }
        if !matches!(document.version, 1 | 2) {
            return Err(Error::Format(
                "only portable versions 1 and 2 are supported",
            ));
        }
        if document.state.schema_version != document.version {
            return Err(Error::Format(
                "portable version and state.schema_version must match",
            ));
        }
        if document.kind != "preset" {
            return Err(Error::Format(
                "export a single preset, not a profile or full backup",
            ));
        }
        if document.state.presets.len() != 1 || document.state.profiles.len() != 1 {
            return Err(Error::Document(
                "a preset export must contain one preset and one profile".into(),
            ));
        }
        if index_size != Some(document.state.assets.len()) {
            return Err(Error::Document(
                "asset keys resolve to duplicate UUIDs".into(),
            ));
        }
        if document.state.presets[0].layers.len() > self.limits.max_layers {
            return Err(Error::LimitExceeded {
                resource: "layers",
                limit: self.limits.max_layers as u64,
            });
        }
        if document.state.assets.len() > self.limits.max_assets {
            return Err(Error::LimitExceeded {
                resource: "asset count",
                limit: self.limits.max_assets as u64,
            });
        }
        // This is the app's own v1 migration plus state/geometry validation.
        // It rejects unknown enums, invalid ranges, UUIDs and broken references.
        let state =
            migrate_state(document.state).map_err(|error| Error::Document(error.to_string()))?;
        let preset = &state.presets[0];
        let referenced: BTreeSet<Id> = preset
            .layers
            .iter()
            .filter_map(|layer| layer.content.asset_id())
            .collect();
        if referenced != state.assets.keys().copied().collect() {
            return Err(Error::Document(
                "asset index must contain exactly the preset's referenced assets".into(),
            ));
        }
        check_native_text(preset)?;
        let checked = assets::validate(document.assets, &state.assets, &self.limits)?;
        let source = |id: Id| -> anyhow::Result<AssetData> {
            checked
                .source
                .get(&id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("asset {id} is missing"))
        };
        let mut renderer = Renderer::new().map_err(|error| Error::Render(error.to_string()))?;
        let raster = renderer
            .render(preset, 1.0, &source)
            .map_err(|error| Error::Render(error.to_string()))?;
        if !raster.pixels.chunks_exact(4).any(|pixel| pixel[3] != 0) {
            return Err(Error::EmptyPreset);
        }
        // Re-render vector assets and procedural layers at the resolution the
        // card needs, rather than enlarging the native 32px raster. The app's
        // own scale and physical-output caps remain in force.
        let preview_multiplier = preview_scale(&raster);
        let preview_raster = if preview_multiplier > 1.0 {
            renderer
                .render(preset, preview_multiplier, &source)
                .unwrap_or_else(|_| raster.clone())
        } else {
            raster.clone()
        };
        let mut warnings = Vec::new();
        if document.version == 1 {
            warnings.push(Warning::LegacyVersionMigrated);
        }
        if preset.layers.iter().any(|layer| {
            layer
                .animation
                .as_ref()
                .is_some_and(|animation| animation.effective_for(layer))
        }) {
            warnings.push(Warning::AnimationBaseStateOnly);
        }
        if checked.contains_gif {
            warnings.push(Warning::GifFirstFrameOnly);
        }
        let profile = &state.profiles[0];
        if profile.scale != 1.0
            || profile.offset.x != 0.0
            || profile.offset.y != 0.0
            || !profile.positions.is_empty()
            || !state.base_visible
        {
            warnings.push(Warning::ProfilePlacementIgnored);
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        let metadata = PresetMetadata {
            id: preset.id.to_string(),
            name: preset.name.clone(),
            format_version: document.version,
            rendered_schema_version: state.schema_version,
            layers: preset.layers.len(),
            visible_layers: preset
                .layers
                .iter()
                .filter(|layer| layer.visible && layer.opacity > 0.0)
                .count(),
            assets: checked.source.len(),
            source_bytes: bytes.len(),
            decoded_asset_bytes: checked.decoded_bytes,
            sha256: digest,
            native_width: raster.width,
            native_height: raster.height,
            native_origin: raster.origin,
            warnings,
        };
        Ok(ValidatedPreset {
            metadata,
            original: Arc::from(bytes),
            raster,
            preview_raster,
        })
    }
}

fn check_native_text(preset: &crosshair_core::Preset) -> Result<()> {
    #[cfg(not(windows))]
    if preset.layers.iter().any(|layer| {
        matches!(&layer.content, Content::Text(_))
            || matches!(&layer.content, Content::Crosshair(shape) if matches!(&shape.shape, ArmShape::Text(_)))
    }) {
        return Err(Error::Unsupported(
            "native text layers require the app's Windows DirectWrite backend; export them as outlined SVG on other systems",
        ));
    }
    #[cfg(windows)]
    let _ = preset;
    Ok(())
}

/// Try to show roughly a 170px visible crosshair in the card without exceeding
/// the application's 16x scale or 1024px physical raster limit. Transparent
/// padding in SVG assets must NOT count as visible drawing when sizing.
fn preview_scale(raster: &Raster) -> f32 {
    let mut min_x = raster.width;
    let mut min_y = raster.height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for (index, pixel) in raster.pixels.chunks_exact(4).enumerate() {
        if pixel[3] < 5 {
            continue;
        }
        let x = (index as u32) % raster.width;
        let y = (index as u32) / raster.width;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if min_x > max_x || min_y > max_y {
        return 1.0;
    }
    let visible = (max_x - min_x + 1).max(max_y - min_y + 1);
    let max_native = raster.width.max(raster.height).max(1);
    let max_by_dimensions = (MAX_DIMENSION / max_native).clamp(1, 16);
    let desired = (170.0 / visible as f32).ceil() as u32;
    desired.clamp(1, max_by_dimensions) as f32
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    fn preview_is_vector_sized_from_visible_ink_not_transparent_padding() {
        let mut raster = Raster {
            width: 32,
            height: 32,
            origin: [-16, -16],
            pixels: vec![0; 32 * 32 * 4],
        };
        for y in 10..23usize {
            for x in 10..23usize {
                raster.pixels[(y * 32 + x) * 4 + 3] = 255;
            }
        }
        assert!(preview_scale(&raster) >= 12.0);
        let large = Raster {
            width: 1000,
            height: 1000,
            origin: [-500, -500],
            pixels: vec![255; 1000 * 1000 * 4],
        };
        assert_eq!(preview_scale(&large), 1.0);
    }
}
