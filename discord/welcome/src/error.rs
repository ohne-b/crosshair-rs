use std::{error::Error as StdError, fmt};

/// Errors are returned to the caller; the library never logs or exits.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    InvalidName(&'static str),
    InvalidMemberNumber,
    InvalidFont(&'static str),
    InputTooLarge { kind: &'static str, limit: usize },
    UnsupportedAvatarFormat,
    InvalidImageDimensions,
    TransparentLogo,
    Image(image::ImageError),
    Io(std::io::Error),
    Internal(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(reason) => write!(f, "invalid display name: {reason}"),
            Self::InvalidMemberNumber => {
                write!(f, "member number must be between 1 and 999,999,999")
            }
            Self::InvalidFont(reason) => write!(f, "invalid font: {reason}"),
            Self::InputTooLarge { kind, limit } => {
                write!(f, "{kind} exceeds the {limit}-byte input limit")
            }
            Self::UnsupportedAvatarFormat => write!(f, "avatar must be PNG, JPEG, or WebP"),
            Self::InvalidImageDimensions => {
                write!(
                    f,
                    "image dimensions must be between 1 and 4096 on each axis"
                )
            }
            Self::TransparentLogo => write!(f, "bundled logo has no visible pixels"),
            Self::Image(error) => write!(f, "image error: {error}"),
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Internal(reason) => write!(f, "rendering error: {reason}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Image(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<image::ImageError> for Error {
    fn from(value: image::ImageError) -> Self {
        Self::Image(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
