pub use crosshair_core::browse::presets;

#[cfg(test)]
mod tests {
    use super::*;
    use crosshair_core::{Content, Id, Preset};

    #[test]
    fn updated_designs_match_the_supplied_svg_geometry() -> anyhow::Result<()> {
        use crosshair_core::{AssetKind, Image};
        use crosshair_renderer::{AssetData, Raster, Renderer};
        let designs = presets();
        let mut renderer = Renderer::new()?;
        let pixel = |raster: &Raster, x: i32, y: i32| -> [u8; 4] {
            let x = x - raster.origin[0];
            let y = y - raster.origin[1];
            if x < 0 || y < 0 || x >= raster.width as i32 || y >= raster.height as i32 {
                return [0; 4];
            }
            let offset = (y as usize * raster.width as usize + x as usize) * 4;
            raster.pixels[offset..offset + 4].try_into().unwrap()
        };
        for (index, bytes, size) in [
            (0, include_bytes!("fixtures/shotgun.svg").as_slice(), 288.0),
            (
                7,
                include_bytes!("fixtures/small-white.svg").as_slice(),
                28.0,
            ),
            (8, include_bytes!("fixtures/teloqr.svg").as_slice(), 28.0),
        ] {
            let asset = Id::new_v4();
            let data = AssetData {
                kind: AssetKind::Svg,
                bytes: bytes.into(),
            };
            let mut reference = Preset::new("SVG reference");
            reference.layers[0].content = Content::Image(Image::new(asset, size, size));
            let expected = renderer.render(&reference, 1.0, &|_| Ok(data.clone()))?;
            let actual = renderer.render(&designs[index], 1.0, &|_| {
                anyhow::bail!("Native layers need no assets")
            })?;
            for y in -144..144 {
                for x in -144..144 {
                    let expected = pixel(&expected, x, y);
                    let actual = pixel(&actual, x, y);
                    // SVG edges are antialiased; native rectangular arms deliberately stay sharp.
                    if expected[3] != 0 && expected[3] != 255 {
                        continue;
                    }
                    assert!(
                        expected.iter().zip(actual).all(|(a, b)| a.abs_diff(b) <= 2),
                        "{} differs at ({x}, {y}): {expected:?} != {actual:?}",
                        designs[index].name
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn designs_are_valid_editable_and_copies_are_independent() -> anyhow::Result<()> {
        let designs = presets();
        assert_eq!(designs.len(), 9);
        let mut renderer = crosshair_renderer::Renderer::new()?;
        for preset in &designs {
            crosshair_core::validation::validate_preset_geometry(preset)?;
            assert!(
                preset
                    .layers
                    .iter()
                    .all(|layer| layer.content.asset_id().is_none() && !layer.locked)
            );
            let copy = preset.duplicate(&preset.name);
            assert_ne!(copy.id, preset.id);
            for (copied, original) in copy.layers.iter().zip(&preset.layers) {
                assert_ne!(copied.id, original.id);
                assert_eq!(copied.content, original.content);
            }
            let raster = renderer.render(preset, 1.0, &|_| anyhow::bail!("No assets expected"))?;
            assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        }
        assert_eq!(
            designs.iter().map(|p| p.id).collect::<Vec<_>>(),
            presets().iter().map(|p| p.id).collect::<Vec<_>>()
        );
        Ok(())
    }
}
