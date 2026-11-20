use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use crosshair_core::{Asset, AssetKind, Id};
use crosshair_renderer::{AssetData, inspect_asset};
use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Error, Result, ValidationLimits};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmbeddedAsset {
    pub id: Id,
    pub data: String,
}

pub(super) struct CheckedAssets {
    pub source: BTreeMap<Id, AssetData>,
    pub decoded_bytes: usize,
    pub contains_gif: bool,
}

pub(super) fn validate(
    entries: Vec<EmbeddedAsset>,
    index: &BTreeMap<Id, Asset>,
    limits: &ValidationLimits,
) -> Result<CheckedAssets> {
    if entries.len() != index.len() {
        return Err(Error::Document(
            "embedded assets do not match the asset index".into(),
        ));
    }
    if entries.len() > limits.max_assets {
        return Err(Error::LimitExceeded {
            resource: "asset count",
            limit: limits.max_assets as u64,
        });
    }
    let mut source = BTreeMap::new();
    let mut decoded_bytes = 0usize;
    let mut total_pixels = 0u64;
    let mut contains_gif = false;
    for entry in entries {
        let id = entry.id;
        let metadata = index
            .get(&id)
            .ok_or_else(|| Error::asset(id, "not in the asset index"))?;
        if source.contains_key(&id) {
            return Err(Error::asset(id, "duplicate embedded asset ID"));
        }
        if metadata.source_bytes > limits.max_asset_bytes as u64
            || entry.data.len() > limits.max_asset_bytes.div_ceil(3) * 4
        {
            return Err(Error::asset(
                id,
                "encoded asset exceeds the configured byte limit",
            ));
        }
        let bytes = STANDARD
            .decode(entry.data.as_bytes())
            .map_err(|_| Error::asset(id, "invalid standard padded base64"))?;
        if bytes.len() as u64 != metadata.source_bytes {
            return Err(Error::asset(
                id,
                "decoded byte length does not match source_bytes",
            ));
        }
        if bytes.len() > limits.max_asset_bytes {
            return Err(Error::asset(id, "asset exceeds the configured byte limit"));
        }
        decoded_bytes = decoded_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| Error::asset(id, "asset byte-count overflow"))?;
        if decoded_bytes > limits.max_total_asset_bytes {
            return Err(Error::LimitExceeded {
                resource: "total decoded asset bytes",
                limit: limits.max_total_asset_bytes as u64,
            });
        }
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if digest != metadata.sha256 {
            return Err(Error::asset(
                id,
                "SHA-256 does not match the embedded bytes",
            ));
        }
        if matches!(metadata.kind, AssetKind::Svg) {
            preflight_svg(id, &bytes)?;
        }
        let data = AssetData {
            kind: metadata.kind.clone(),
            bytes: Arc::from(bytes),
        };
        // Upstream enforces its SVG resource policy, verifies dimensions, and
        // accounts for JPEG EXIF orientation. Do not trust uploaded metadata.
        let actual = inspect_asset(&data).map_err(|error| Error::asset(id, error.to_string()))?;
        if (actual.width, actual.height) != (metadata.width, metadata.height) {
            return Err(Error::asset(
                id,
                "dimensions do not match the decoded asset",
            ));
        }
        let pixels = u64::from(actual.width) * u64::from(actual.height);
        if actual.width > limits.max_image_side
            || actual.height > limits.max_image_side
            || pixels > limits.max_image_pixels
        {
            return Err(Error::asset(
                id,
                "decoded image exceeds the configured dimensions/pixel limit",
            ));
        }
        total_pixels = total_pixels
            .checked_add(pixels)
            .ok_or_else(|| Error::asset(id, "image pixel-count overflow"))?;
        if total_pixels > limits.max_total_image_pixels {
            return Err(Error::LimitExceeded {
                resource: "total decoded image pixels",
                limit: limits.max_total_image_pixels,
            });
        }
        if !matches!(metadata.kind, AssetKind::Svg) {
            // Dimensions/pixel budgets are checked BEFORE full decode. Decode
            // hidden layers too: a readable header is not proof of valid data.
            decode_raster(id, &metadata.kind, &data.bytes, limits)?;
        }
        contains_gif |= matches!(metadata.kind, AssetKind::Gif);
        source.insert(id, data);
    }
    Ok(CheckedAssets {
        source,
        decoded_bytes,
        contains_gif,
    })
}

fn decode_raster(id: Id, kind: &AssetKind, bytes: &[u8], limits: &ValidationLimits) -> Result<()> {
    let format = match kind {
        AssetKind::Png => ImageFormat::Png,
        AssetKind::Jpeg => ImageFormat::Jpeg,
        AssetKind::Gif => ImageFormat::Gif,
        AssetKind::Svg => return Err(Error::asset(id, "internal raster/SVG mismatch")),
    };
    let detected =
        image::guess_format(bytes).map_err(|_| Error::asset(id, "unrecognized image header"))?;
    if detected != format {
        return Err(Error::asset(
            id,
            "image header does not match the declared asset kind",
        ));
    }
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format);
    let mut budget = Limits::default();
    budget.max_image_width = Some(limits.max_image_side);
    budget.max_image_height = Some(limits.max_image_side);
    budget.max_alloc = Some(64 * 1024 * 1024);
    decoder.limits(budget);
    let decoded = decoder
        .decode()
        .map_err(|error| Error::asset(id, error.to_string()))?;
    if u64::from(decoded.width()) * u64::from(decoded.height()) > limits.max_image_pixels {
        return Err(Error::asset(id, "decoded raster exceeds the pixel limit"));
    }
    Ok(())
}

fn preflight_svg(id: Id, bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::asset(id, "SVG must use UTF-8"))?;
    let document = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 10_000,
            ..Default::default()
        },
    )
    .map_err(|error| Error::asset(id, error.to_string()))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg"
        || root.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
    {
        return Err(Error::asset(
            id,
            "expected an SVG root in the SVG namespace",
        ));
    }
    // Avoid silently substituting server fonts or loading arbitrary system
    // fonts. Outline SVG text before export. Ordinary SVG paths are supported.
    if document.descendants().any(|node| {
        node.is_element() && matches!(node.tag_name().name(), "text" | "tspan" | "textPath")
    }) {
        return Err(Error::Unsupported(
            "convert SVG text to paths before sharing",
        ));
    }
    // The upstream parser does the rest: no scripts/event handlers, DTDs,
    // foreignObject, animation, external href/url resources or embedded SVGs;
    // references, filters, embedded raster images and surfaces are budgeted.
    Ok(())
}
