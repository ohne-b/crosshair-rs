use std::{collections::VecDeque, io::Cursor, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use crosshair_core::{AssetKind, Crop, Id};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use tiny_skia::{Pixmap, Transform};

const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
const CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct AssetData {
    pub kind: AssetKind,
    pub bytes: Arc<[u8]>,
}

pub trait AssetSource {
    /// IDs must identify immutable bytes for the lifetime of a renderer cache.
    fn load(&self, id: Id) -> Result<AssetData>;
}

impl<F: Fn(Id) -> Result<AssetData>> AssetSource for F {
    fn load(&self, id: Id) -> Result<AssetData> {
        self(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetInfo {
    pub width: u32,
    pub height: u32,
}

pub fn inspect_asset(data: &AssetData) -> Result<AssetInfo> {
    ensure!(data.bytes.len() <= MAX_SOURCE_BYTES, "Image exceeds 8 MiB");
    match data.kind {
        AssetKind::Svg => {
            let tree = crate::svg::parse(&data.bytes)?;
            let size = tree.size().to_int_size();
            check_dimensions(size.width(), size.height())?;
            Ok(AssetInfo {
                width: size.width(),
                height: size.height(),
            })
        }
        _ => {
            let mut decoder = reader(data)?.into_decoder()?;
            let (mut width, mut height) = decoder.dimensions();
            check_dimensions(width, height)?;
            if orientation_swaps_dimensions(decoder.orientation()?) {
                std::mem::swap(&mut width, &mut height);
            }
            Ok(AssetInfo { width, height })
        }
    }
}

fn orientation_swaps_dimensions(orientation: image::metadata::Orientation) -> bool {
    use image::metadata::Orientation::*;
    matches!(
        orientation,
        Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH
    )
}

fn check_dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "Image exceeds 16 megapixels or has empty dimensions"
    );
    Ok(())
}

fn reader(data: &AssetData) -> Result<ImageReader<Cursor<&[u8]>>> {
    ensure!(data.bytes.len() <= MAX_SOURCE_BYTES, "Image exceeds 8 MiB");
    let format = match data.kind {
        AssetKind::Png => ImageFormat::Png,
        AssetKind::Jpeg => ImageFormat::Jpeg,
        AssetKind::Gif => ImageFormat::Gif,
        AssetKind::Svg => bail!("SVG is not a raster image"),
    };
    let mut reader = ImageReader::with_format(Cursor::new(data.bytes.as_ref()), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

type CachedImage = ((Id, [u32; 2], [u32; 4]), Arc<Pixmap>);

#[derive(Default)]
pub(crate) struct AssetCache {
    images: VecDeque<CachedImage>,
    bytes: usize,
}

impl AssetCache {
    fn reserve(&mut self, scratch_bytes: u64) {
        while self.bytes as u64 + scratch_bytes > 96 * 1024 * 1024 {
            if let Some((_, old)) = self.images.pop_front() {
                self.bytes -= old.data().len();
            } else {
                break;
            }
        }
    }
    pub fn clear(&mut self) {
        self.images.clear();
        self.bytes = 0;
    }

    pub fn image(
        &mut self,
        id: Id,
        requested_size: [u32; 2],
        crop: Crop,
        source: &impl AssetSource,
    ) -> Result<(Arc<Pixmap>, Crop)> {
        let crop_key = [
            crop.x.to_bits(),
            crop.y.to_bits(),
            crop.width.to_bits(),
            crop.height.to_bits(),
        ];
        if let Some(index) = self
            .images
            .iter()
            .position(|((stored, size, stored_crop), _)| {
                *stored == id
                    && (*size == [0, 0] || (*size == requested_size && *stored_crop == crop_key))
            })
        {
            let entry = self.images.remove(index).expect("cache entry exists");
            let image = entry.1.clone();
            let effective_crop = if entry.0.1 == [0, 0] {
                crop
            } else {
                Crop::default()
            };
            self.images.push_back(entry);
            return Ok((image, effective_crop));
        }
        let data = source.load(id).context("Could not read image asset")?;
        ensure!(data.bytes.len() <= MAX_SOURCE_BYTES, "Image exceeds 8 MiB");
        let (image, key_size) = if data.kind == AssetKind::Svg {
            ensure!(
                requested_size.iter().all(|size| *size > 0 && *size <= 1024),
                "SVG output exceeds raster bounds"
            );
            let tree = crate::svg::parse(&data.bytes)?;
            let mut pixmap = Pixmap::new(requested_size[0], requested_size[1])
                .context("Could not allocate SVG raster")?;
            let sx = requested_size[0] as f32 / (tree.size().width() * crop.width);
            let sy = requested_size[1] as f32 / (tree.size().height() * crop.height);
            let scale = Transform::from_row(
                sx,
                0.0,
                0.0,
                sy,
                -crop.x * tree.size().width() * sx,
                -crop.y * tree.size().height() * sy,
            );
            crate::svg::check_render_budget(&tree, scale)?;
            self.reserve(64 * 1024 * 1024);
            resvg::render(&tree, scale, &mut pixmap.as_mut());
            (pixmap, requested_size)
        } else {
            let mut decoder = reader(&data)?.into_decoder()?;
            let (width, height) = decoder.dimensions();
            check_dimensions(width, height)?;
            let conversion_bytes = if decoder.color_type() == image::ColorType::Rgba8 {
                0
            } else {
                u64::from(width) * u64::from(height) * 4
            };
            let orientation = decoder.orientation()?;
            let scratch_bytes = (decoder.total_bytes() + conversion_bytes).max(
                decoder.total_bytes()
                    * if orientation_swaps_dimensions(orientation) {
                        2
                    } else {
                        1
                    },
            );
            ensure!(
                scratch_bytes <= 96 * 1024 * 1024,
                "Image conversion exceeds the 128 MiB working-memory limit"
            );
            // Decoding happens with enough cache space reserved for conversion and layer buffers.
            self.reserve(scratch_bytes);
            let mut image = DynamicImage::from_decoder(decoder)?;
            image.apply_orientation(orientation);
            let rgba = image.into_rgba8();
            let size =
                tiny_skia::IntSize::from_wh(rgba.width(), rgba.height()).context("Empty image")?;
            let mut bytes = rgba.into_raw();
            for pixel in bytes.chunks_exact_mut(4) {
                let alpha = u16::from(pixel[3]);
                for channel in &mut pixel[..3] {
                    *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
                }
            }
            (
                Pixmap::from_vec(bytes, size).context("Invalid decoded pixels")?,
                [0, 0],
            )
        };
        let image = Arc::new(image);
        let length = image.data().len();
        while self.bytes + length > CACHE_BYTES {
            if let Some((_, old)) = self.images.pop_front() {
                self.bytes -= old.data().len();
            } else {
                break;
            }
        }
        if length <= CACHE_BYTES {
            self.bytes += length;
            self.images.push_back((
                (
                    id,
                    key_size,
                    if key_size == [0, 0] { [0; 4] } else { crop_key },
                ),
                image.clone(),
            ));
        }
        Ok((
            image,
            if key_size == [0, 0] {
                crop
            } else {
                Crop::default()
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(text: &str) -> AssetData {
        AssetData {
            kind: AssetKind::Svg,
            bytes: Arc::from(text.as_bytes()),
        }
    }

    #[test]
    fn svg_rejects_active_external_and_unbounded_content() {
        for content in [
            "<script>alert(1)</script>",
            "<use href='file:///secret'/>",
            "<image href='https://example.com/a.png'/>",
        ] {
            let data = svg(&format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>{content}</svg>"
            ));
            assert!(inspect_asset(&data).is_err());
        }
        assert!(inspect_asset(&svg("<!DOCTYPE svg [<!ENTITY x 'a'>]><svg/>")).is_err());
    }

    #[test]
    fn svg_supports_bounded_local_vector_references() {
        let data = svg(
            "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='14'><defs><path id='p' d='M0 0L10 0L0 10Z'/></defs><use href='#p'/></svg>",
        );
        assert_eq!(
            inspect_asset(&data).unwrap(),
            AssetInfo {
                width: 12,
                height: 14
            }
        );
    }
}
