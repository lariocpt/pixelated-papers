use std::io;
use std::path::PathBuf;

/// Everything that can go wrong in pixcore. Thin and typed; the binary layer
/// renders these to users.
#[derive(Debug, thiserror::Error)]
pub enum PixError {
    #[error("io error for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },

    #[error("could not decode image {path}: {reason}")]
    Decode { path: PathBuf, reason: String },

    #[error("unsupported image {path}: {reason}")]
    Unsupported { path: PathBuf, reason: String },

    #[error("invalid value: {0}")]
    Invalid(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad config at {}: {reason}", path.display())]
    Config { path: PathBuf, reason: String },

    #[error("bad manifest at {}: {reason}", path.display())]
    Manifest { path: PathBuf, reason: String },

    #[error("{0}")]
    Other(String),
}

impl PixError {
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, PixError>;
