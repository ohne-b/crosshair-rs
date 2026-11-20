//! All design values are in final-output pixels, before supersampling.
//! This is deliberately one fixed banner design, not a theme framework.

pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 400;
pub(crate) const SCALE: u32 = 3;

pub(crate) const BACKGROUND: [u8; 4] = [0x10, 0x11, 0x12, 255];
pub(crate) const INK: [u8; 4] = [0xf1, 0xf1, 0xef, 255];
pub(crate) const MUTED: [u8; 4] = [0xa3, 0xa5, 0xa8, 255];
pub(crate) const ACCENT: [u8; 4] = [0xf9, 0x73, 0x16, 255];

pub(crate) const AVATAR_CENTER: (f32, f32) = (210.0, 200.0);
pub(crate) const LOGO_SIZE: f32 = 310.0;
// Calibrated to the supplied logo, after cropping its transparent padding.
pub(crate) const AVATAR_RADIUS: f32 = 0.373 * LOGO_SIZE;
pub(crate) const BOLT_RING_RADIUS: f32 = 40.0 * LOGO_SIZE / 106.0;
pub(crate) const BOLT_RADIUS: f32 = 3.3 * LOGO_SIZE / 106.0;

pub(crate) const TEXT_X: f32 = 480.0;
pub(crate) const TEXT_RIGHT: f32 = WIDTH as f32 - 58.0;
pub(crate) const TITLE_BASELINE: f32 = 198.0;
pub(crate) const TITLE_MAX_SIZE: f32 = 72.0;
pub(crate) const TITLE_MIN_SIZE: f32 = 36.0;
pub(crate) const TITLE_WEIGHT: f32 = 680.0;
pub(crate) const SUBTITLE_BASELINE: f32 = 258.0;
pub(crate) const SUBTITLE_SIZE: f32 = 33.0;
pub(crate) const SUBTITLE_WEIGHT: f32 = 460.0;
pub(crate) const PREFIX: &str = "Welcome, ";

pub(crate) fn scaled(value: f32) -> f32 {
    value * SCALE as f32
}

pub(crate) fn pixels(value: f32) -> u32 {
    scaled(value).round() as u32
}
