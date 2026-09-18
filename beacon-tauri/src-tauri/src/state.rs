use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use beacon_core::auth::MinecraftSession;
use beacon_core::config::LauncherConfig;
use beacon_core::{refresh_session, Account, CoreError, MinecraftProfile};
use tokio::sync::Mutex;

pub struct AppState {
    pub http: reqwest::Client,
    pub config: Mutex<LauncherConfig>,
    pub config_path: PathBuf,
    /// Cached to avoid re-running the Microsoft -> Xbox Live -> XSTS -> Minecraft Services
    /// chain on every Play/Skins-tab open, which was triggering 429s.
    mc_sessions: Mutex<HashMap<String, MinecraftSession>>,
    pub skin_profiles: Mutex<HashMap<String, MinecraftProfile>>,
    /// Single slot, not a map: only one game instance can run at a time.
    pub running: Mutex<Option<RunningGame>>,
}

#[derive(Clone)]
pub struct RunningGame {
    pub instance_id: String,
    pub pid: u32,
}

impl AppState {
    pub fn new(http: reqwest::Client, config: LauncherConfig, config_path: PathBuf) -> Self {
        Self {
            http,
            config: Mutex::new(config),
            config_path,
            mc_sessions: Mutex::new(HashMap::new()),
            skin_profiles: Mutex::new(HashMap::new()),
            running: Mutex::new(None),
        }
    }

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

/// Converts a `spawn_blocking` panic into a normal error instead of taking down the worker thread.
pub fn join_err(e: tokio::task::JoinError) -> beacon_core::CoreError {
    beacon_core::CoreError::Other(format!("internal task failed: {e}"))
}
