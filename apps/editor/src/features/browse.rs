use crosshair_core::{Color, Content, Crosshair, Dot, Id, Layer, Preset, Rectangle};

/// Built-in designs use native geometry, so saved copies need no image assets.
pub fn presets() -> Vec<Preset> {
    let red = Color::rgba(255, 0, 0, 255);
    let white = Color::rgba(255, 255, 255, 255);
    let cross = |name: &str, color, length, gap| {
        Layer::new(
            name,
            Content::Crosshair(Crosshair {
                color,
                length,
                gap,
                thickness: 2.0,
                ..Default::default()
            }),
        )
    };
    let outlined = |color| {
        let outline = Layer::new(
            "Outline",
            Content::Crosshair(Crosshair {
                color,
                length: 6.0,
                gap: 0.0,
                thickness: 4.0,
                ..Default::default()
            }),
        );
        vec![outline, cross("Cross", white, 4.0, 1.0)]
    };
    let mut shotgun = vec![
        Layer::new(
            "Center dot",
            Content::Crosshair(Crosshair {
                length: 0.0,
                dot: Dot {
                    enabled: true,
                    diameter: 6.0,
                    color: Color::rgba(5, 255, 0, 255),
                    ..Default::default()
                },
                ..Default::default()
            }),
        ),
        Layer::new(
            "Diagonal marks",
            Content::Crosshair(Crosshair {
                color: red,
                length: 4.0,
                thickness: 10.0,
                gap: 26.0,
                rotation: 45.0,
                ..Default::default()
            }),
        ),
    ];
    for (name, x, y) in [
        ("Upper left outer", -23.0, -14.0),
        ("Upper left inner", -14.0, -23.0),
        ("Upper right outer", 23.0, -14.0),
        ("Upper right inner", 14.0, -23.0),
        ("Lower left outer", -23.0, 14.0),
        ("Lower right outer", 23.0, 14.0),
        ("Lower left inner", -14.0, 23.0),
        ("Lower right inner", 15.0, 23.0),
    ] {
        let mut layer = Layer::new(
            name,
            Content::Rectangle(Rectangle {
                width: 4.0,
                height: 4.0,
                radius: 0.0,
                color: red,
            }),
        );
        layer.transform.x = x;
        layer.transform.y = y;
        shotgun.push(layer);
    }
    let designs = [
        ("Shotgun", shotgun),
        ("Blue outline", outlined(Color::rgba(0, 0, 255, 204))),
        (
            "Small green",
            vec![cross("Cross", Color::rgba(4, 255, 25, 255), 3.0, 0.0)],
        ),
        ("Small red", vec![cross("Cross", red, 4.0, 0.0)]),
        ("Red outline", outlined(Color::rgba(255, 0, 0, 204))),
        ("White cross", vec![cross("Cross", white, 4.0, 2.0)]),
        (
            "Red chevron",
            vec![Layer::new(
                "Chevron",
                Content::Crosshair(Crosshair {
                    color: red,
                    length: 9.0,
                    thickness: 3.0,
                    gap: 0.0,
                    rotation: 45.0,
                    enabled_arms: vec![true, true, false, false],
                    dot: Dot {
                        enabled: true,
                        diameter: 3.0,
                        color: red,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            )],
        ),
    ];
    designs
        .into_iter()
        .enumerate()
        .map(|(index, (name, layers))| Preset {
            id: Id::from_u128(0x43524200000000000000000000000000 + index as u128 + 1),
            name: name.into(),
            layers,
            revision: 0,
            modified_order: 0,
            favorite: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn designs_are_valid_editable_and_copies_are_independent() -> anyhow::Result<()> {
        let designs = presets();
        assert_eq!(designs.len(), 7);
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
