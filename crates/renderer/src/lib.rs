//! Deterministic premultiplied raster output shared by every application surface.

mod assets;
mod composite;
mod export;
mod geometry;
mod svg;
mod text;

pub use assets::{AssetData, AssetInfo, AssetSource, inspect_asset};
pub use geometry::LayerBounds;

use std::{collections::VecDeque, sync::Arc};

use anyhow::{Result, ensure};
use crosshair_core::Preset;
use sha2::{Digest, Sha256};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

const RASTER_CACHE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 1024;

#[derive(Clone, Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// Top-left pixel relative to the physical design origin.
    pub origin: [i32; 2],
    pub pixels: Vec<u8>,
}

impl Raster {
    /// A filtered UI thumbnail, independent of the overlay's physical origin.
    pub fn thumbnail(&self, limit: u32, dpi: f32) -> Result<Self> {
        // Prefilter before Slint's nearest-neighbor scaling can shift thin crosshair stems.
        ensure!(
            limit > 0 && limit <= MAX_DIMENSION,
            "Invalid thumbnail limit"
        );
        ensure!(
            dpi.is_finite() && dpi > 0.0 && limit as f32 * dpi <= MAX_DIMENSION as f32,
            "Invalid thumbnail scale"
        );
        let source = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
            self.width,
            self.height,
            self.pixels.as_slice(),
        )
        .ok_or_else(|| anyhow::anyhow!("Invalid raster buffer"))?;
        ensure!(self.width > 0 && self.height > 0, "Empty raster");
        let scale = (limit as f32 / self.width.max(self.height) as f32).min(1.0) * dpi;
        let width = (self.width as f32 * scale).round().max(1.0) as u32;
        let height = (self.height as f32 * scale).round().max(1.0) as u32;
        let pixels = image::imageops::resize(
            &source,
            width,
            height,
            image::imageops::FilterType::Triangle,
        );
        Ok(Self {
            width,
            height,
            origin: [0, 0],
            pixels: pixels.into_raw(),
        })
    }

    pub fn to_png(&self) -> Result<Vec<u8>> {
        let size = tiny_skia::IntSize::from_wh(self.width, self.height)
            .ok_or_else(|| anyhow::anyhow!("Invalid raster dimensions"))?;
        let pixmap = Pixmap::from_vec(self.pixels.clone(), size)
            .ok_or_else(|| anyhow::anyhow!("Invalid raster buffer"))?;
        Ok(pixmap.encode_png()?)
    }

    pub fn to_bgra(&self) -> Vec<u8> {
        let mut pixels = self.pixels.clone();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        pixels
    }
}

/// Construct on the rendering worker; its font state remains on that thread.
pub struct Renderer {
    text: text::TextRenderer,
    assets: assets::AssetCache,
    rasters: VecDeque<([u8; 32], Arc<Raster>)>,
    raster_bytes: usize,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            text: text::TextRenderer::new()?,
            assets: assets::AssetCache::default(),
            rasters: VecDeque::new(),
            raster_bytes: 0,
        })
    }

    pub fn clear_cache(&mut self) {
        self.clear_raster_cache();
        self.assets.clear();
    }

    pub fn clear_raster_cache(&mut self) {
        self.rasters.clear();
        self.raster_bytes = 0;
    }

    pub fn font_families(&mut self) -> Result<Vec<String>> {
        self.text.font_families()
    }

    pub fn layer_bounds(
        &mut self,
        layer: &crosshair_core::Layer,
        scale: f32,
    ) -> Result<LayerBounds> {
        geometry::prepare_layer(layer, scale, &mut self.text).map(|layer| layer.bounds)
    }

    /// Changes the transform pivot without moving the currently visible shape.
    /// A rejected edit leaves the caller's layer untouched.
    pub fn set_anchor(
        &mut self,
        layer: &mut crosshair_core::Layer,
        anchor: crosshair_core::Anchor,
    ) -> Result<()> {
        if layer.transform.anchor == anchor {
            return Ok(());
        }
        let before = self.layer_bounds(layer, 1.0)?;
        let mut edited = layer.clone();
        edited.transform.anchor = anchor;
        let after = self.layer_bounds(&edited, 1.0)?;
        edited.transform.x += before.left - after.left;
        edited.transform.y += before.top - after.top;
        crosshair_core::validation::validate_layer_geometry(&edited)?;
        *layer = edited;
        Ok(())
    }

    /// Converts an unscaled canvas point into the layer's editable coordinates.
    pub fn local_point(
        &mut self,
        layer: &crosshair_core::Layer,
        point: crosshair_core::Point,
    ) -> Result<crosshair_core::Point> {
        geometry::local_point(layer, point, &mut self.text)
    }

    pub fn anchor_point(&mut self, layer: &crosshair_core::Layer) -> Result<crosshair_core::Point> {
        geometry::anchor_point(layer, &mut self.text)
    }

    /// Pixel-rounded output bounds, including all effects, without allocating pixels.
    pub fn measure(&mut self, preset: &Preset, scale: f32) -> Result<LayerBounds> {
        self.prepare(preset, scale, &|| false)
            .map(|(_, bounds)| bounds)
    }

    /// Self-contained SVG with outlined text and embedded image assets.
    pub fn to_svg(
        &mut self,
        preset: &Preset,
        scale: f32,
        assets: &impl AssetSource,
    ) -> Result<Vec<u8>> {
        let (layers, bounds) = self.prepare(preset, scale, &|| false)?;
        export::svg(&layers, bounds, &mut self.assets, assets)
    }

    fn prepare(
        &mut self,
        preset: &Preset,
        scale: f32,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(Vec<geometry::PreparedLayer>, LayerBounds)> {
        ensure!(
            scale.is_finite() && scale > 0.0 && scale <= 16.0,
            "Output scale is outside the supported range"
        );
        crosshair_core::validation::validate_preset_geometry(preset)?;
        let mut layers = Vec::new();
        let mut point_count = 0usize;
        let mut bounds: Option<LayerBounds> = None;
        for layer in preset
            .layers
            .iter()
            .filter(|layer| layer.visible && layer.opacity > 0.0)
        {
            ensure!(!cancelled(), "Rendering cancelled");
            let prepared = geometry::prepare_layer(layer, scale, &mut self.text)?;
            point_count = point_count.saturating_add(prepared.point_count());
            ensure!(
                point_count <= 1_000_000,
                "Crosshair exceeds one million vector points"
            );
            if prepared.is_empty() {
                continue;
            }
            bounds = Some(bounds.map_or(prepared.bounds, |current| current.union(prepared.bounds)));
            layers.push(prepared);
        }
        let bounds = bounds.unwrap_or(LayerBounds {
            left: 0.0,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        });
        let bounds = LayerBounds {
            left: bounds.left.floor(),
            top: bounds.top.floor(),
            right: bounds.right.ceil(),
            bottom: bounds.bottom.ceil(),
        };
        ensure!(
            bounds.width() <= MAX_DIMENSION as f32 && bounds.height() <= MAX_DIMENSION as f32,
            "Crosshair exceeds 1024 physical pixels including effects"
        );
        Ok((layers, bounds))
    }

    pub fn render(
        &mut self,
        preset: &Preset,
        scale: f32,
        assets: &impl AssetSource,
    ) -> Result<Arc<Raster>> {
        self.render_cancellable(preset, scale, assets, || false)
    }

    pub fn render_cancellable(
        &mut self,
        preset: &Preset,
        scale: f32,
        assets: &impl AssetSource,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<Raster>> {
        ensure!(!cancelled(), "Rendering cancelled");
        ensure!(
            scale.is_finite() && scale > 0.0 && scale <= 16.0,
            "Output scale is outside the supported range"
        );
        ensure!(
            preset.layers.len() <= 64,
            "A crosshair can contain at most 64 layers"
        );
        crosshair_core::validation::validate_preset_geometry(preset)?;
        let mut hash = Sha256::new();
        hash.update(serde_json::to_vec(preset)?);
        hash.update(scale.to_bits().to_le_bytes());
        // Immutable asset IDs are part of the serialized document cache key.
        let key: [u8; 32] = hash.finalize().into();
        if let Some(index) = self.rasters.iter().position(|(stored, _)| *stored == key) {
            let item = self.rasters.remove(index).expect("cache entry exists");
            let raster = item.1.clone();
            self.rasters.push_back(item);
            return Ok(raster);
        }
        let (layers, bounds) = self.prepare(preset, scale, &cancelled)?;
        let origin = [bounds.left as i32, bounds.top as i32];
        let width = bounds.width() as u32;
        let height = bounds.height() as u32;
        let mut output = Pixmap::new(width, height)
            .ok_or_else(|| anyhow::anyhow!("Could not allocate crosshair pixels"))?;
        for layer in layers {
            ensure!(!cancelled(), "Rendering cancelled");
            let raster = composite::render_layer(
                &layer,
                origin,
                [width, height],
                &mut self.assets,
                assets,
                &cancelled,
            )?;
            output.draw_pixmap(
                0,
                0,
                raster.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        ensure!(!cancelled(), "Rendering cancelled");
        let raster = Arc::new(Raster {
            width,
            height,
            origin,
            pixels: output.take(),
        });
        self.raster_bytes += raster.pixels.len();
        self.rasters.push_back((key, raster.clone()));
        while self.raster_bytes > RASTER_CACHE_BYTES {
            if let Some((_, old)) = self.rasters.pop_front() {
                self.raster_bytes -= old.pixels.len();
            }
        }
        Ok(raster)
    }
}
