#![forbid(unsafe_code)]

pub mod browse;
pub mod history;
pub mod input;
pub mod model;
pub mod protocol;
pub mod validation;

pub use history::{History, HistoryError};
pub use input::{CaptureOutcome, InputEvent, InputOutcome, InputState, Presentation};
pub use model::*;
pub use uuid::Uuid as Id;
pub use validation::{ValidationError, validate_layer, validate_preset, validate_state};

pub const SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 4;
pub const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PORTABLE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_RASTER_SIDE: u32 = 1024;
pub const MAX_LAYERS: usize = 64;
pub const MAX_PRESETS: usize = 500;
pub const MAX_PROFILES: usize = 32;
pub const MAX_BINDINGS: usize = 256;
pub const MAX_TEXT_CHARS: usize = 1024;
pub const MAX_DRAWING_POINTS: usize = 20_000;
