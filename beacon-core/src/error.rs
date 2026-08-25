use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("version '{0}' not found in manifest")]
    VersionNotFound(String),

    #[error("account '{0}' not found")]
    AccountNotFound(String),

    #[error("checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("failed to launch JVM: {0}")]
    Launch(std::io::Error),

    #[error("Microsoft authentication failed: {0}")]
    Auth(String),

    #[error("secret storage error: {0}")]
    SecretStore(#[from] keyring::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

pub(crate) fn io_err(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> CoreError {
    let path = path.into();
    move |source| CoreError::Io { path, source }
}
