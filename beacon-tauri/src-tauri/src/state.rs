use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use beacon_core::auth::MinecraftSession;
use beacon_core::config::LauncherConfig;
use beacon_core::{refresh_session, Account, CoreError, MinecraftProfile};
use tokio::sync::Mutex;

/// Everything a command needs to talk to `beacon-core`. `config` is kept in memory and only
/// written back to disk on the commands that actually change it, mirroring what `beacon-cli`
/// does per-invocation.
pub struct AppState {
    pub http: reqwest::Client,
    pub config: Mutex<LauncherConfig>,
    pub config_path: PathBuf,
    /// In-memory only, keyed by account id -- never written to disk, same non-persistence
    /// guarantee the Microsoft refresh token's own handling already has. Exists purely so opening
    /// the Skins tab (or clicking Play) repeatedly in a short span doesn't re-run the full
    /// Microsoft -> Xbox Live -> XSTS -> Minecraft Services chain every single time; that chain
    /// hammering Microsoft's endpoints in quick succession is what was triggering 429s.
    mc_sessions: Mutex<HashMap<String, MinecraftSession>>,
    /// In-memory only, keyed by account id -- the Skins tab reloads (a live `GET
    /// /minecraft/profile` call) on every tab switch and every account-list refresh, which alone
    /// (session caching above notwithstanding) was still enough live traffic to Minecraft
    /// Services to trip 429s under heavy tab-switching. Skins/capes essentially never change
    /// except through Beacon's own upload/reset/cape commands (which already refetch and update
    /// this cache themselves) or a manual refresh -- there's no reason to treat every tab open as
    /// a reason to ask Mojang again.
    pub skin_profiles: Mutex<HashMap<String, MinecraftProfile>>,
}

impl AppState {
    pub fn new(http: reqwest::Client, config: LauncherConfig, config_path: PathBuf) -> Self {
        Self {
            http,
            config: Mutex::new(config),
            config_path,
            mc_sessions: Mutex::new(HashMap::new()),
            skin_profiles: Mutex::new(HashMap::new()),
        }
    }

    /// Reuses a cached session for `account` if it isn't about to expire, refreshing (and
    /// re-caching) only when it's missing or close to it -- see the `mc_sessions` field doc for
    /// why this exists.
    pub async fn minecraft_session(&self, config: &LauncherConfig, account: &Account) -> Result<MinecraftSession, CoreError> {
        const EXPIRY_BUFFER: Duration = Duration::from_secs(60);
        let account_id = account.id();

        {
            let cache = self.mc_sessions.lock().await;
            if let Some(session) = cache.get(&account_id) {
                if session.expires_at > SystemTime::now() + EXPIRY_BUFFER {
                    return Ok(session.clone());
                }
            }
        }

        let session = refresh_session(&self.http, &config.azure_client_id, account).await?;
        self.mc_sessions.lock().await.insert(account_id, session.clone());
        Ok(session)
    }
}

/// Turns a `tokio::task::JoinError` from a `spawn_blocking` task into a `CoreError` instead of
/// panicking the calling command -- a panic inside the blocking task (e.g. the `zip` crate
/// choking on a corrupt archive during import) would otherwise take down the async worker thread
/// via `.expect(..)` rather than surfacing as an ordinary error on the frontend.
pub fn join_err(e: tokio::task::JoinError) -> beacon_core::CoreError {
    beacon_core::CoreError::Other(format!("internal task failed: {e}"))
}
