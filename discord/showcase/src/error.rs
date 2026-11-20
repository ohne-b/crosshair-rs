use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

/// Stable categories for callers. Never parse an English error message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidLimits,
    LimitExceeded,
    InvalidJson,
    UnsupportedFormat,
    InvalidDocument,
    InvalidAsset,
    UnsupportedFeature,
    EmptyPreset,
    InvalidFont,
    InvalidDisplayText,
    RenderFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid limit configuration: {0}")]
    InvalidLimits(&'static str),
    #[error("{resource} exceeds the limit of {limit}")]
    LimitExceeded { resource: &'static str, limit: u64 },
    #[error("invalid JSON: {0}")]
    Json(#[source] serde_json::Error),
    #[error("unsupported document: {0}")]
    Format(&'static str),
    #[error("invalid preset: {0}")]
    Document(String),
    #[error("asset {id}: {message}")]
    Asset { id: String, message: String },
    #[error("unsupported preview feature: {0}")]
    Unsupported(&'static str),
    #[error("the preset has no visible pixels at its base state")]
    EmptyPreset,
    #[error("invalid banner font: {0}")]
    Font(&'static str),
    #[error("invalid display text: {0}")]
    DisplayText(&'static str),
    #[error("rendering failed: {0}")]
    Render(String),
}

impl Error {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidLimits(_) => ErrorCode::InvalidLimits,
            Self::LimitExceeded { .. } => ErrorCode::LimitExceeded,
            Self::Json(_) => ErrorCode::InvalidJson,
            Self::Format(_) => ErrorCode::UnsupportedFormat,
            Self::Document(_) => ErrorCode::InvalidDocument,
            Self::Asset { .. } => ErrorCode::InvalidAsset,
            Self::Unsupported(_) => ErrorCode::UnsupportedFeature,
            Self::EmptyPreset => ErrorCode::EmptyPreset,
            Self::Font(_) => ErrorCode::InvalidFont,
            Self::DisplayText(_) => ErrorCode::InvalidDisplayText,
            Self::Render(_) => ErrorCode::RenderFailed,
        }
    }

    pub(crate) fn asset(id: crosshair_core::Id, message: impl Into<String>) -> Self {
        Self::Asset {
            id: id.to_string(),
            message: message.into(),
        }
    }
}
