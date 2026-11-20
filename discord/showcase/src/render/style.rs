//! Fixed Discord showcase card geometry, using the website's dark color system.
pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 512;

pub(super) const BACKGROUND: [u8; 4] = [0x10, 0x11, 0x12, 255];
pub(super) const INK: [u8; 4] = [0xf1, 0xf1, 0xef, 255];
pub(super) const MUTED: [u8; 4] = [0xa3, 0xa5, 0xa8, 255];
pub(super) const GRID: [u8; 4] = [0x56, 0x5d, 0x66, 45];
pub(super) const AXIS: [u8; 4] = [0x9c, 0xa2, 0xab, 120];

pub(super) const LEFT: f32 = 32.0;
pub(super) const TITLE_BASELINE: f32 = 72.0;
pub(super) const SUBTITLE_BASELINE: f32 = 111.0;
pub(super) const TITLE_WIDTH: f32 = 730.0;
pub(super) const TITLE_MAX_SIZE: f32 = 47.0;
pub(super) const TITLE_MIN_SIZE: f32 = 25.0;
pub(super) const TEXT_WEIGHT: f32 = 400.0;
pub(super) const TITLE_WEIGHT: f32 = 500.0;

pub(super) const LOGO_SIZE: f32 = 45.0;
pub(super) const LOGO_X: f32 = 808.0;
pub(super) const LOGO_Y: f32 = 34.0;
pub(super) const BRAND_X: f32 = 865.0;
pub(super) const BRAND_BASELINE: f32 = 65.0;
pub(super) const BRAND_WIDTH: f32 = 146.0;

pub(super) const PREVIEW_X: f32 = 24.0;
pub(super) const PREVIEW_Y: f32 = 138.0;
pub(super) const PREVIEW_W: f32 = 976.0;
pub(super) const PREVIEW_H: f32 = 350.0;
pub(super) const GRID_STEP: f32 = 28.0;

pub(super) const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
