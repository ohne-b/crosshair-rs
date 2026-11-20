//! Render 1200 × 400 welcome banners from a display name, member count and avatar.

mod drawing;
mod error;
mod images;
mod renderer;
mod style;
mod text;

pub use error::{Error, Result};
pub use renderer::{WelcomeCard, WelcomeRenderer};
pub use style::{HEIGHT, WIDTH};

/// Maximum encoded avatar size accepted by the library: 8 MiB.
pub const MAX_AVATAR_BYTES: usize = 8 * 1024 * 1024;
/// Maximum font size accepted by the constructor: 4 MiB.
pub const MAX_FONT_BYTES: usize = 4 * 1024 * 1024;
