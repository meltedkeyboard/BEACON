use std::path::PathBuf;

use beacon_core::config::LauncherConfig;
use tokio::sync::Mutex;

/// Everything a command needs to talk to `beacon-core`. `config` is kept in memory and only
/// written back to disk on the commands that actually change it, mirroring what `beacon-cli`
/// does per-invocation.
pub struct AppState {
    pub http: reqwest::Client,
    pub config: Mutex<LauncherConfig>,
    pub config_path: PathBuf,
}

/// Turns a `tokio::task::JoinError` from a `spawn_blocking` task into a `CoreError` instead of
/// panicking the calling command -- a panic inside the blocking task (e.g. the `zip` crate
/// choking on a corrupt archive during import) would otherwise take down the async worker thread
/// via `.expect(..)` rather than surfacing as an ordinary error on the frontend.
pub fn join_err(e: tokio::task::JoinError) -> beacon_core::CoreError {
    beacon_core::CoreError::Other(format!("internal task failed: {e}"))
}
