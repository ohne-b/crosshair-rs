use crate::{
    Color, Content, Crosshair, Dot, Freehand, Id, Layer, Point, Preset, Rectangle, Shape, Stroke,
};

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
                length: 2.0,
                thickness: 10.0,
                gap: 26.0,
                rotation: 45.0,
                ..Default::default()
            }),
        ),
    ];
    for (name, x, y, width, height) in [
        ("Upper left outer", -22.0, -14.0, 2.0, 4.0),
        ("Upper left inner", -14.0, -22.0, 4.0, 2.0),
        ("Upper right outer", 22.0, -14.0, 2.0, 4.0),
        ("Upper right inner", 14.0, -22.0, 4.0, 2.0),
        ("Lower left outer", -22.0, 14.0, 2.0, 4.0),
        ("Lower right outer", 22.0, 14.0, 2.0, 4.0),
        ("Lower left inner", -14.0, 22.0, 4.0, 2.0),
        ("Lower right inner", 15.0, 22.0, 4.0, 2.0),
    ] {
        let mut layer = Layer::new(
            name,
            Content::Rectangle(Rectangle {
                width,
                height,
                radius: 0.0,
                color: red,
            }),
        );
        layer.transform.x = x;
        layer.transform.y = y;
        shotgun.push(layer);
    }
    let mut teloqr = Layer::new(
        "Cross",
        Content::Crosshair(Crosshair {
            color: white,
            length: 3.5,
            thickness: 1.0,
            gap: 0.0,
            ..Default::default()
        }),
    );
    teloqr.transform.x = -0.5;
    teloqr.transform.y = 0.5;
    let mut nine_reg = vec![Layer::new(
        "Center dot",
        Content::Crosshair(Crosshair {
            length: 0.0,
            dot: Dot {
                enabled: true,
                color: Color::WHITE,
                ..Default::default()
            },
            ..Default::default()
        }),
    )];
    let arms = [
        ("Top", 0.0, -3.0, 180.0),
        ("Bottom", 0.0, 3.0, 0.0),
        ("Left", -3.0, 0.0, 90.0),
        ("Right", 3.0, 0.0, 270.0),
    ];
    // shortcut: strokes round SVG miter corners; use miter joins if native strokes gain them.
    for (name, x, y, rotation) in arms {
        let mut layer = Layer::new(
            format!("{name} outline"),
            Content::Freehand(Freehand {
                strokes: vec![Stroke {
                    points: vec![
                        Point { x: 0.0, y: -3.0 },
                        Point { x: 1.0, y: 3.0 },
                        Point { x: -1.0, y: 3.0 },
                        Point { x: 0.0, y: -3.0 },
                    ],
                    width: 1.0,
                    color: Color::rgba(0, 0, 0, 61),
                    erase: false,
                }],
            }),
        );
        layer.transform.x = x;
        layer.transform.y = y;
        layer.transform.rotation = rotation;
        nine_reg.push(layer);
    }
    nine_reg.push(Layer::new(
        "Diamond outline",
        Content::Crosshair(Crosshair {
            length: (2.83 - 1.41) / std::f32::consts::SQRT_2,
            thickness: 2.83 * std::f32::consts::SQRT_2,
            gap: 1.41 / std::f32::consts::SQRT_2,
            rotation: 45.0,
            color: Color::rgba(0, 0, 0, 61),
            ..Default::default()
        }),
    ));
    let mut diamond = Layer::new(
        "Center diamond",
        Content::Rectangle(Rectangle {
            width: 1.41 * std::f32::consts::SQRT_2,
            height: 1.41 * std::f32::consts::SQRT_2,
            radius: 0.0,
            color: Color::rgba(255, 255, 255, 184),
        }),
    );
    diamond.transform.rotation = 45.0;
    nine_reg.push(diamond);
    for (name, x, y, rotation) in arms {
        let mut layer = Layer::new(
            name,
            Content::Triangle(Shape {
                width: 2.0,
                height: 6.0,
                color: white,
            }),
        );
        layer.transform.x = x;
        layer.transform.y = y;
        layer.transform.rotation = rotation;
        nine_reg.push(layer);
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
        ("Small white", vec![cross("Cross", white, 4.0, 0.0)]),
        ("TeloqR (CS Pro)", vec![teloqr]),
        ("9reg (Pro Gamer)", nine_reg),
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
