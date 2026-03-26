use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("missing header: first line of trace file must be a header object")]
    MissingHeader,

    #[error("invalid header: {0}")]
    InvalidHeader(String),

    #[error("profile error: {0}")]
    Profile(String),

    #[error("diff error: {0}")]
    Diff(String),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
}

pub type Result<T> = std::result::Result<T, Error>;
