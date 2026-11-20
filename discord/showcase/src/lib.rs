//! Offline preset validation and showcase PNG rendering.

#![forbid(unsafe_code)]

mod error;
mod limits;
mod preset;
mod render;
mod validation;

pub use error::{Error, ErrorCode, Result};
pub use limits::ValidationLimits;
pub use preset::{PresetMetadata, ValidatedPreset, Warning};
pub use render::{HEIGHT, ShowcaseCard, ShowcaseRenderer, WIDTH};
pub use validation::PresetValidator;
