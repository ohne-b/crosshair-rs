use std::sync::Arc;

use crosshair_renderer::Raster;
use serde::Serialize;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Warning {
    /// Migration is in memory only. The attached JSON remains at version 1.
    LegacyVersionMigrated,
    /// Animation definitions are checked but only the base/rest pose is drawn.
    AnimationBaseStateOnly,
    /// The app's static image renderer uses the first GIF frame.
    GifFirstFrameOnly,
    /// A preset preview uses scale 1; profile positioning/settings are not applied.
    ProfilePlacementIgnored,
}

/// Safe-to-display metadata. The author is intentionally NOT inferred from JSON.
#[derive(Clone, Debug, Serialize)]
pub struct PresetMetadata {
    pub id: String,
    pub name: String,
    pub format_version: u32,
    pub rendered_schema_version: u32,
    pub layers: usize,
    pub visible_layers: usize,
    pub assets: usize,
    pub source_bytes: usize,
    pub decoded_asset_bytes: usize,
    pub sha256: String,
    pub native_width: u32,
    pub native_height: u32,
    /// Native top-left relative to the app's design origin. A card centers the
    /// image for presentation; this coordinate is NOT an on-screen position.
    pub native_origin: [i32; 2],
    pub warnings: Vec<Warning>,
}

/// Only returned after structural checks, asset checks, and a successful native
/// render. Private fields prevent bypassing the validator with raw JSON.
/// Keep this bounded-lifetime object only as long as necessary: it owns the
/// original document and the native raster, not a persistent gallery cache.
pub struct ValidatedPreset {
    pub(crate) metadata: PresetMetadata,
    pub(crate) original: Arc<[u8]>,
    pub(crate) raster: Arc<Raster>,
    /// High-resolution preview rendered by the same application renderer.
    pub(crate) preview_raster: Arc<Raster>,
}

impl std::fmt::Debug for ValidatedPreset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedPreset")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl ValidatedPreset {
    pub fn metadata(&self) -> &PresetMetadata {
        &self.metadata
    }

    pub fn original_json(&self) -> &[u8] {
        &self.original
    }

    pub fn download_filename(&self, username: &str) -> String {
        format!(
            "{}-shared-{}.json",
            slug(username, "user"),
            slug(&self.metadata.name, "preset")
        )
    }

    /// Pixel-accurate transparent PNG from the app's renderer at scale 1.
    /// Unlike the showcase card, this contains no background, text, or logo.
    pub fn crosshair_png(&self) -> Result<Vec<u8>> {
        self.raster
            .to_png()
            .map_err(|error| Error::Render(error.to_string()))
    }
}

fn slug(name: &str, fallback: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for ch in name.chars() {
        if slug.len() >= 48 {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() && slug.len() < 47 {
                slug.push('-');
            }
            slug.push(ch.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        slug.push_str(fallback);
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn filename_is_never_a_path_or_windows_device() {
        for name in [
            "../../CON",
            "C:\\users\\x",
            "日本語",
            "...",
            "NUL",
            "Training Grid",
        ] {
            let value = slug(name, "preset");
            assert!(!value.is_empty());
            assert!(!value.contains(['/', '\\', ':']));
        }
    }
}
