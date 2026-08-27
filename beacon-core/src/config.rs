use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::account::Account;
use crate::auth::AZURE_CLIENT_ID;
use crate::error::{io_err, CoreError, Result};
use crate::instance::Instance;

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
    pub instances: Vec<Instance>,
    #[serde(default)]
    pub selected_instance_id: Option<String>,
    /// Overrides `instances_dir()`'s default of `game_dir/instances` -- set once a user
    /// relocates it independently of `game_dir` (see [`relocate_directory`]). `None` means "no
    /// override yet", not "unset the instances dir".
    #[serde(default)]
    pub instances_dir_override: Option<PathBuf>,
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
            instances: Vec::new(),
            selected_instance_id: None,
            instances_dir_override: None,
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

    /// Root of every instance's own directory (see [`crate::instance::Instance::dir`]) --
    /// separate from `versions_dir`/`libraries_dir`/`assets_dir`, which are the *shared*
    /// per-version download cache every instance targeting that version reuses. Defaults to
    /// `game_dir/instances`, independently relocatable via `instances_dir_override`.
    pub fn instances_dir(&self) -> PathBuf {
        self.instances_dir_override
            .clone()
            .unwrap_or_else(|| self.game_dir.join("instances"))
    }

    pub fn find_instance(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    pub fn selected_instance(&self) -> Option<&Instance> {
        self.selected_instance_id.as_deref().and_then(|id| self.find_instance(id))
    }

    /// Inserts or replaces (by id) an instance and returns its id.
    pub fn upsert_instance(&mut self, instance: Instance) -> String {
        let id = instance.id.clone();
        if let Some(existing) = self.instances.iter_mut().find(|i| i.id == id) {
            *existing = instance;
        } else {
            self.instances.push(instance);
        }
        id
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

/// Moves everything at `from` to `to` -- used when the user relocates `game_dir` or the
/// instances directory from Settings, so the switch doesn't orphan already-downloaded versions
/// or (far more importantly) existing worlds. Tries a plain rename first (instant, same volume);
/// if that fails (typically because `to` is on a different drive), falls back to a recursive
/// copy followed by removing the original. Blocking, and potentially slow for a large instances
/// directory -- callers should run this on a blocking thread (see `beacon-tauri`'s
/// `tokio::task::spawn_blocking` usage for `export_instance`/`import_instance`, which have the
/// same shape of problem).
///
/// Refuses to run if `to` is inside `from` (or `from` inside `to`) -- moving a directory into
/// its own subdirectory can't produce a sane result -- or if `to` already exists and has
/// anything in it, so relocating never silently merges into or clobbers unrelated files.
pub fn relocate_directory(from: &Path, to: &Path) -> Result<()> {
    let to_absolute = if to.exists() {
        to.canonicalize().map_err(io_err(to))?
    } else {
        let parent = to
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| CoreError::Other(format!("'{}' is not a valid location", to.display())))?;
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        let parent = parent.canonicalize().map_err(io_err(parent))?;
        let name = to
            .file_name()
            .ok_or_else(|| CoreError::Other(format!("'{}' is not a valid location", to.display())))?;
        parent.join(name)
    };

    if !from.exists() {
        std::fs::create_dir_all(&to_absolute).map_err(io_err(&to_absolute))?;
        return Ok(());
    }
    let from_absolute = from.canonicalize().map_err(io_err(from))?;

    if from_absolute == to_absolute {
        return Ok(());
    }
    if to_absolute.starts_with(&from_absolute) {
        return Err(CoreError::Other(
            "the new location is inside the current one -- pick a location outside it".into(),
        ));
    }
    if from_absolute.starts_with(&to_absolute) {
        return Err(CoreError::Other(
            "the current location is inside the new one -- pick a different location".into(),
        ));
    }

    if to_absolute.exists() {
        let has_entries = std::fs::read_dir(&to_absolute)
            .map_err(io_err(&to_absolute))?
            .next()
            .is_some();
        if has_entries {
            return Err(CoreError::Other(
                "the new location already has files in it -- pick an empty folder".into(),
            ));
        }
        // Empty, but present -- clear it so `rename` (which expects the target to not already
        // exist) and the copy fallback both start from a clean destination.
        std::fs::remove_dir(&to_absolute).map_err(io_err(&to_absolute))?;
    }

    if std::fs::rename(&from_absolute, &to_absolute).is_err() {
        copy_dir_recursive(&from_absolute, &to_absolute)?;
        std::fs::remove_dir_all(&from_absolute).map_err(io_err(&from_absolute))?;
    }
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(io_err(to))?;
    for entry in std::fs::read_dir(from).map_err(io_err(from))? {
        let entry = entry.map_err(io_err(from))?;
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest).map_err(io_err(&dest))?;
        }
    }
    Ok(())
}
