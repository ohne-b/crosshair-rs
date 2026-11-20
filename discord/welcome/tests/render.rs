use crosshair_welcome::{Error, WelcomeCard, WelcomeRenderer, HEIGHT, WIDTH};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

fn font() -> &'static [u8] {
    include_bytes!("../../assets/Manrope.ttf")
}

fn card() -> WelcomeCard<'static> {
    WelcomeCard {
        display_name: "ohneb",
        member_number: 81,
        avatar: None,
    }
}

fn png(image: RgbaImage) -> Vec<u8> {
    let mut output = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

#[test]
fn outputs_a_decodable_png_with_the_approved_dimensions_and_colors() {
    let renderer = WelcomeRenderer::new(font()).unwrap();
    let bytes = renderer.render_png(card()).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert_eq!(image.dimensions(), (WIDTH, HEIGHT));
    for (x, y) in [(0, 0), (1199, 0), (0, 399), (1199, 399), (600, 0)] {
        assert_eq!(*image.get_pixel(x, y), Rgba([0x10, 0x11, 0x12, 255]));
    }
    assert_eq!(*image.get_pixel(600, 320), Rgba([0x10, 0x11, 0x12, 255]));
    let orange_in_title = (480..1142)
        .flat_map(|x| (110..210).map(move |y| (x, y)))
        .filter(|&(x, y)| *image.get_pixel(x, y) == Rgba([0xf9, 0x73, 0x16, 255]))
        .count();
    assert!(
        orange_in_title > 100,
        "the name must be drawn in the brand orange"
    );
}

#[test]
fn a_long_name_stays_inside_the_text_column() {
    let renderer = WelcomeRenderer::new(font()).unwrap();
    let name = "W".repeat(200);
    let image = renderer
        .render(WelcomeCard {
            display_name: &name,
            ..card()
        })
        .unwrap();
    // Leave 3 pixels for downsampling fringes past the nominal right edge.
    for x in 1145..1180 {
        for y in 90..280 {
            assert_eq!(*image.get_pixel(x, y), Rgba([0x10, 0x11, 0x12, 255]));
        }
    }
}

#[test]
fn missing_glyphs_and_combining_sequences_do_not_break_the_card() {
    let renderer = WelcomeRenderer::new(font()).unwrap();
    for name in ["Café", "Cafe\u{301}", "Женя", "player 🎯", "玩家", "👨‍👩‍👧‍👦"]
    {
        let image = renderer
            .render(WelcomeCard {
                display_name: name,
                ..card()
            })
            .unwrap();
        assert_eq!(image.dimensions(), (WIDTH, HEIGHT));
    }
}

#[test]
fn invalid_input_is_reported() {
    let renderer = WelcomeRenderer::new(font()).unwrap();
    assert!(matches!(
        renderer.render(WelcomeCard {
            member_number: 0,
            ..card()
        }),
        Err(Error::InvalidMemberNumber)
    ));
    assert!(matches!(
        renderer.render(WelcomeCard {
            member_number: 1_000_000_000,
            ..card()
        }),
        Err(Error::InvalidMemberNumber)
    ));
    assert!(matches!(
        renderer.render(WelcomeCard {
            display_name: " \n ",
            ..card()
        }),
        Err(Error::InvalidName(_))
    ));
    assert!(renderer
        .render(WelcomeCard {
            avatar: Some(b"bad image"),
            ..card()
        })
        .is_err());
}

#[test]
fn transparent_and_nonsquare_avatars_are_supported() {
    let renderer = WelcomeRenderer::new(font()).unwrap();
    let mut avatar = RgbaImage::from_pixel(90, 30, Rgba([255, 0, 0, 0]));
    for x in 30..60 {
        for y in 0..30 {
            avatar.put_pixel(x, y, Rgba([0, 255, 0, 255]));
        }
    }
    let bytes = png(avatar);
    let image = renderer
        .render(WelcomeCard {
            avatar: Some(&bytes),
            ..card()
        })
        .unwrap();
    // Sample inside the circular crop but away from the crosshair and bolt holes.
    assert_eq!(*image.get_pixel(165, 155), Rgba([0, 255, 0, 255]));
    assert_eq!(*image.get_pixel(600, 320), Rgba([0x10, 0x11, 0x12, 255]));
}

#[test]
fn reusing_the_renderer_is_deterministic_and_does_not_retain_a_previous_avatar() {
    let renderer = WelcomeRenderer::new(font()).unwrap();
    let first = renderer.render_png(card()).unwrap();
    let avatar = png(RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255])));
    renderer
        .render_png(WelcomeCard {
            avatar: Some(&avatar),
            ..card()
        })
        .unwrap();
    let again = renderer.render_png(card()).unwrap();
    assert_eq!(first, again);
}
