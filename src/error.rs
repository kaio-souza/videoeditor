use std::path::PathBuf;

use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum EditorError {
    #[error("media not found: {0:?}")]
    MediaNotFound(PathBuf),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("decoder error: {0}")]
    DecoderError(String),
    #[error("encoder error: {0}")]
    EncoderError(String),
    #[error("invalid timeline: {0}")]
    InvalidTimeline(String),
    #[error("export error: {0}")]
    ExportError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, EditorError>;
