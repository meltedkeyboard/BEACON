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

/// Tauri commands require their `Err` type to implement `Serialize` to cross the IPC boundary,
/// so `CoreError` needs one even though `thiserror` only gives us `Display`/`Error`. Serializes
/// as `{ "kind": ..., "message": ... }` -- `kind` is a stable string a frontend can match on,
/// `message` is the human-readable text already produced by `Display`.
impl serde::Serialize for CoreError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CoreError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl CoreError {
    fn kind(&self) -> &'static str {
        match self {
            CoreError::Http(_) => "http",
            CoreError::Io { .. } => "io",
            CoreError::Json(_) => "json",
            CoreError::VersionNotFound(_) => "version_not_found",
            CoreError::AccountNotFound(_) => "account_not_found",
            CoreError::ChecksumMismatch { .. } => "checksum_mismatch",
            CoreError::Zip(_) => "archive",
            CoreError::Launch(_) => "launch",
            CoreError::Auth(_) => "auth",
            CoreError::SecretStore(_) => "secret_store",
            CoreError::Other(_) => "other",
        }
    }
}

pub(crate) fn io_err(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> CoreError {
    let path = path.into();
    move |source| CoreError::Io { path, source }
}
