use crosshair_core::{ArmShape, Color, Content, Crosshair, Dot, DotShape, Id, Layer, Preset};

/// Original, static tool styles. Order and IDs identify their cached thumbnails.
pub fn presets() -> Vec<Preset> {
    let white = Color::rgba(255, 255, 255, 255);
    let base = Crosshair {
        color: white,
        dot: Dot {
            color: white,
            ..Default::default()
        },
        ..Default::default()
    };
    let styles = [
        ("Classic", base.clone()),
        (
            "Fine",
            Crosshair {
                thickness: 1.0,
                length: 8.0,
                gap: 3.0,
                ..base.clone()
            },
        ),
        (
            "Plus",
            Crosshair {
                length: 9.0,
                gap: 0.0,
                ..base.clone()
            },
        ),
        (
            "Dot",
            Crosshair {
                length: 0.0,
                dot: Dot {
                    enabled: true,
                    diameter: 4.0,
                    color: white,
                    ..Default::default()
                },
                ..base.clone()
            },
        ),
        (
            "Three arms",
            Crosshair {
                arms: 3,
                enabled_arms: vec![true; 3],
                rotation: -90.0,
                length: 8.0,
                ..base.clone()
            },
        ),
        (
            "Triangle",
            Crosshair {
                shape: ArmShape::Triangle,
                arms: 3,
                enabled_arms: vec![true; 3],
                rotation: -90.0,
                thickness: 5.0,
                length: 8.0,
                ..base.clone()
            },
        ),
        (
            "T bars",
            Crosshair {
                shape: ArmShape::TShape {
                    width: 8.0,
                    thickness: 2.0,
                },
                length: 7.0,
                ..base.clone()
            },
        ),
        (
            "Diamond",
            Crosshair {
                rotation: 45.0,
                length: 7.0,
                gap: 3.0,
                ..base.clone()
            },
        ),
        (
            "Chevrons",
            Crosshair {
                shape: ArmShape::Chevron {
                    inward: false,
                    depth: 2.0,
                },
                thickness: 8.0,
                length: 5.0,
                gap: 5.0,
                ..base.clone()
            },
        ),
        (
            "Inward",
            Crosshair {
                shape: ArmShape::Chevron {
                    inward: true,
                    depth: 2.0,
                },
                thickness: 8.0,
                length: 5.0,
                gap: 5.0,
                ..base.clone()
            },
        ),
        (
            "Arcs",
            Crosshair {
                shape: ArmShape::Arc {
                    radius: 9.0,
                    sweep: 70.0,
                },
                gap: 6.0,
                ..base.clone()
            },
        ),
        (
            "Ring",
            Crosshair {
                shape: ArmShape::Arc {
                    radius: 9.0,
                    sweep: 360.0,
                },
                arms: 1,
                enabled_arms: vec![true],
                gap: 0.0,
                ..base.clone()
            },
        ),
        (
            "Corners",
            Crosshair {
                shape: ArmShape::Chevron {
                    inward: false,
                    depth: 2.0,
                },
                rotation: 45.0,
                thickness: 10.0,
                length: 4.0,
                gap: 6.0,
                ..base.clone()
            },
        ),
        (
            "Wide",
            Crosshair {
                thickness: 3.0,
                length: 6.0,
                gap: 8.0,
                ..base.clone()
            },
        ),
        (
            "Square dot",
            Crosshair {
                length: 0.0,
                dot: Dot {
                    enabled: true,
                    shape: DotShape::Square,
                    diameter: 4.0,
                    color: white,
                    ..Default::default()
                },
                ..base
            },
        ),
    ];
    styles
        .into_iter()
        .enumerate()
        .map(|(index, (name, crosshair))| {
            let mut layer = Layer::new(name, Content::Crosshair(crosshair));
            // A full arc's intrinsic circle is centered one radius to the right.
            if name == "Ring" {
                layer.transform.x = -9.0;
            }
            Preset {
                id: Id::from_u128(0x43525000000000000000000000000000 + index as u128 + 1),
                name: name.into(),
                revision: 0,
                modified_order: 0,
                favorite: false,
                layers: vec![layer],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crosshair_renderer::Renderer;
    use std::collections::BTreeSet;

    #[test]
    fn palette_is_stable_valid_white_and_small() -> anyhow::Result<()> {
        let palette = presets();
        assert_eq!(palette.len(), 15);
        assert_eq!(
            palette.iter().map(|p| p.id).collect::<Vec<_>>(),
            presets().iter().map(|p| p.id).collect::<Vec<_>>()
        );
        assert_eq!(
            palette.iter().map(|p| p.id).collect::<BTreeSet<_>>().len(),
            palette.len()
        );
        let mut renderer = Renderer::new()?;
        for preset in palette {
            crosshair_core::validation::validate_preset_geometry(&preset)?;
            assert_eq!(preset.layers.len(), 1);
            let raster = renderer.render(&preset, 1.0, &|_| {
                anyhow::bail!("Palette styles must not load assets")
            })?;
            assert!(raster.width <= 40 && raster.height <= 40);
            assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
            assert!(
                raster
                    .pixels
                    .chunks_exact(4)
                    .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2])
            );
        }
        Ok(())
    }
}
