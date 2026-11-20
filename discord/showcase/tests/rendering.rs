use crosshair_showcase::{HEIGHT, PresetValidator, ShowcaseCard, ShowcaseRenderer, WIDTH};

const FONT: &[u8] = include_bytes!("../../assets/Manrope.ttf");

#[test]
fn invalid_banner_font_is_rejected() {
    assert!(ShowcaseRenderer::new(b"not a font").is_err());
    assert!(ShowcaseRenderer::new(&[]).is_err());
}

#[test]
fn card_has_a_solid_background_to_all_four_edges() {
    let preset = PresetValidator::default()
        .validate(include_bytes!("fixtures/preset-v1.json"))
        .unwrap();
    let renderer = ShowcaseRenderer::new(FONT).unwrap();
    let png = renderer
        .render_png(ShowcaseCard {
            preset: &preset,
            author: Some("Player"),
        })
        .unwrap();
    let image = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (WIDTH, HEIGHT));
    let background = image::Rgba([0x10, 0x11, 0x12, 255]);
    for (x, y) in [
        (0, 0),
        (WIDTH - 1, 0),
        (0, HEIGHT - 1),
        (WIDTH - 1, HEIGHT - 1),
        (24, 138),
        (48, 150),
    ] {
        assert_eq!(*image.get_pixel(x, y), background);
    }
}

#[test]
fn repeated_cards_are_deterministic_and_do_not_change_the_download() {
    let original = include_bytes!("fixtures/preset-v1.json");
    let preset = PresetValidator::default().validate(original).unwrap();
    let renderer = ShowcaseRenderer::new(FONT).unwrap();
    let a = renderer
        .render_png(ShowcaseCard {
            preset: &preset,
            author: Some("Player"),
        })
        .unwrap();
    let b = renderer
        .render_png(ShowcaseCard {
            preset: &preset,
            author: Some("Player"),
        })
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(preset.original_json(), original);
}

#[test]
fn long_names_and_optional_attribution_are_handled_without_overflow() {
    let mut source: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/preset-v1.json")).unwrap();
    source["state"]["presets"][0]["name"] = "W".repeat(80).into();
    let preset = PresetValidator::default()
        .validate(&serde_json::to_vec(&source).unwrap())
        .unwrap();
    let renderer = ShowcaseRenderer::new(FONT).unwrap();
    for author in [
        None,
        Some("Player with a very long display name"),
        Some("A 🦀 B"),
    ] {
        let png = renderer
            .render_png(ShowcaseCard {
                preset: &preset,
                author,
            })
            .unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (WIDTH, HEIGHT));
        assert_eq!(image.get_pixel(WIDTH - 1, 0)[3], 255);
    }
    assert!(
        renderer
            .render_png(ShowcaseCard {
                preset: &preset,
                author: Some("   ")
            })
            .is_err()
    );
}
