use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::account::Account;
use crate::auth::AZURE_CLIENT_ID;
use crate::error::{io_err, Result};

/// Layout mirrors the vanilla `.minecraft` directory so existing tooling (and the assets/
/// libraries the vanilla launcher already downloaded) can be reused.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    pub game_dir: PathBuf,
    #[serde(default = "default_java_path")]
    pub java_path: String,
    #[serde(default = "default_azure_client_id")]
    pub azure_client_id: String,
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub selected_account_id: Option<String>,
    #[serde(default)]
    pub selected_version: Option<String>,
}

fn default_java_path() -> String {
    "java".to_string()
}

fn default_azure_client_id() -> String {
    AZURE_CLIENT_ID.to_string()
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            game_dir: default_game_dir(),
            java_path: default_java_path(),
            azure_client_id: default_azure_client_id(),
            accounts: Vec::new(),
            selected_account_id: None,
            selected_version: None,
        }
    }
}

fn default_game_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("beacon"))
        .unwrap_or_else(|| PathBuf::from(".beacon"))
}

/// Where the launcher's own config file (accounts, selected version) lives, separate from
/// `game_dir` which only holds Minecraft assets/libraries/versions.
pub fn default_config_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.config_dir().join("beacon").join("config.json"))
        .unwrap_or_else(|| PathBuf::from("beacon-config.json"))
}

impl LauncherConfig {
    pub async fn load(path: &Path) -> Result<Self> {
        let bytes = tokio::fs::read(path).await.map_err(io_err(path))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn load_or_default(path: &Path) -> Result<Self> {
        match Self::load(path).await {
            Ok(config) => Ok(config),
            Err(crate::error::CoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(Self::default())
            }
            Err(e) => Err(e),
        }
    }

    pub async fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_err(parent))?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        tokio::fs::write(path, json).await.map_err(io_err(path))?;
        Ok(())
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.game_dir.join("versions")
    }

    pub fn version_dir(&self, version_id: &str) -> PathBuf {
        self.versions_dir().join(version_id)
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.game_dir.join("libraries")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.game_dir.join("assets")
    }

    pub fn natives_dir(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join("natives")
    }

    pub fn find_account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id() == id)
    }

    /// Whether a Microsoft account has been signed in at least once. Offline accounts are only
    /// meant as a convenience for players who already own the game -- gating them behind one
    /// successful Microsoft sign-in confirms that ownership instead of handing out an
    /// unauthenticated profile to anyone.
    pub fn has_verified_microsoft_account(&self) -> bool {
        self.accounts.iter().any(|a| matches!(a, Account::Microsoft { .. }))
    }

    pub fn selected_account(&self) -> Option<&Account> {
        self.selected_account_id
            .as_deref()
            .and_then(|id| self.find_account(id))
    }

    /// Inserts or replaces (by id) an account and returns its id.
    pub fn upsert_account(&mut self, account: Account) -> String {
        let id = account.id();
        if let Some(existing) = self.accounts.iter_mut().find(|a| a.id() == id) {
            *existing = account;
        } else {
            self.accounts.push(account);
        }
        id
    }
}
