use std::path::PathBuf;

use beacon_core::config::relocate_directory;
use beacon_core::{forget_account, CoreError};
use tauri::{AppHandle, State};

use crate::state::{join_err, AppState};

// ---------------------------------------------------------------------------------------------
// Directory settings -- `game_dir` (the shared version/library/asset cache) and the instances
// directory (every instance's own saves/resourcepacks/shaderpacks/config) are independently
// relocatable. Both "move" commands actually move the files (see
// `beacon_core::config::relocate_directory`), not just repoint the config and orphan whatever
// was already there -- relocating the instances directory in particular can be moving someone's
// actual worlds, not disposable cache.
// ---------------------------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct DirectorySettings {
    game_dir: PathBuf,
    instances_dir: PathBuf,
    /// Where `config.json` (accounts, instance list, these very paths) lives. Shown read-only --
    /// unlike `game_dir`/`instances_dir`, this one can't be relocated through the app: it has to
    /// be known *before* config.json is read, so it can't be a setting stored inside config.json
    /// itself. Moving it would need a separate pointer file (or an env var/launch flag) outside
    /// config.json, which is its own bit of work -- not attempted here.
    config_dir: PathBuf,
    /// The shared, cross-instance library cache (`game_dir/libraries`) -- not independently
    /// relocatable like `game_dir`/`instances_dir` (it's just a subfolder of `game_dir`), only
    /// exposed here so the instance screen's "Open libraries" button has a path to open.
    libraries_dir: PathBuf,
}

/// Also makes sure the directories actually exist on disk (a fresh setup that hasn't installed
/// anything, or created an instance, yet otherwise wouldn't have them) -- Settings' "Open"
/// buttons point the OS file explorer at these paths, and a folder that doesn't exist yet would
/// make that fail for no reason a user watching the app would understand.
#[tauri::command]
pub async fn get_directory_settings(state: State<'_, AppState>) -> Result<DirectorySettings, CoreError> {
    let config = state.config.lock().await;
    let game_dir = config.game_dir.clone();
    let instances_dir = config.instances_dir();
    let libraries_dir = config.libraries_dir();
    let config_dir = state
        .config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| state.config_path.clone());

    tokio::fs::create_dir_all(&game_dir)
        .await
        .map_err(|e| CoreError::Other(format!("couldn't create '{}': {e}", game_dir.display())))?;
    tokio::fs::create_dir_all(&instances_dir)
        .await
        .map_err(|e| CoreError::Other(format!("couldn't create '{}': {e}", instances_dir.display())))?;
    // config_dir should already exist (config.save() creates it), but a truly fresh install that
    // hasn't saved yet -- e.g. opened Settings before ever signing in or picking a version --
    // otherwise wouldn't have it yet either.
    tokio::fs::create_dir_all(&config_dir)
        .await
        .map_err(|e| CoreError::Other(format!("couldn't create '{}': {e}", config_dir.display())))?;

    Ok(DirectorySettings { game_dir, instances_dir, config_dir, libraries_dir })
}

/// Moves `game_dir` (and therefore every shared version/library/asset it contains) to
/// `new_path`. Runs the actual move on a blocking thread -- this can be a large amount of data,
/// and holding the async config lock for the whole move would freeze every other command (Play,
/// account switching, anything) until it finished; instead the lock is only held briefly before
/// and after to read the old path and persist the new one.
#[tauri::command]
pub async fn set_game_dir_cmd(state: State<'_, AppState>, new_path: String) -> Result<PathBuf, CoreError> {
    let old_path = state.config.lock().await.game_dir.clone();
    let new_path = PathBuf::from(new_path);
    let moved_to = new_path.clone();
    tokio::task::spawn_blocking(move || relocate_directory(&old_path, &new_path))
        .await
        .map_err(join_err)??;

    let mut config = state.config.lock().await;
    config.game_dir = moved_to.clone();
    config.save(&state.config_path).await?;
    Ok(moved_to)
}

/// Same idea as `set_game_dir_cmd`, but for the instances directory (see
/// `LauncherConfig::instances_dir_override`).
#[tauri::command]
pub async fn set_instances_dir_cmd(state: State<'_, AppState>, new_path: String) -> Result<PathBuf, CoreError> {
    let old_path = state.config.lock().await.instances_dir();
    let new_path = PathBuf::from(new_path);
    let moved_to = new_path.clone();
    tokio::task::spawn_blocking(move || relocate_directory(&old_path, &new_path))
        .await
        .map_err(join_err)??;

    let mut config = state.config.lock().await;
    config.instances_dir_override = Some(moved_to.clone());
    config.save(&state.config_path).await?;
    Ok(moved_to)
}

/// Deletes every account (including their Windows Credential Manager entries -- not just the
/// config record, which alone would leave orphaned refresh tokens neither visible nor reachable
/// from the UI), the shared download cache, every instance, and `config.json` itself, then
/// closes the app so the next launch starts completely fresh. Irreversible; the frontend gates
/// this behind a typed confirmation, not just a click, before ever calling it.
///
/// `game_dir` and `instances_dir` are deleted independently rather than assuming one nests
/// inside the other -- `set_instances_dir_cmd` above lets a user relocate them apart, so by the
/// time this runs they may not share anything.
#[tauri::command]
pub async fn wipe_all_data_cmd(app: AppHandle, state: State<'_, AppState>) -> Result<(), CoreError> {
    let config = state.config.lock().await.clone();

    for account in &config.accounts {
        // Best-effort -- an already-missing credential-store entry (or an offline account, which
        // never had one) shouldn't stop the rest of the wipe.
        let _ = forget_account(account).await;
    }

    let game_dir = config.game_dir.clone();
    let instances_dir = config.instances_dir();
    let config_path = state.config_path.clone();

    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        if game_dir.exists() {
            std::fs::remove_dir_all(&game_dir)?;
        }
        if instances_dir.exists() {
            std::fs::remove_dir_all(&instances_dir)?;
        }
        if config_path.exists() {
            std::fs::remove_file(&config_path)?;
        }
        Ok(())
    })
    .await
    .map_err(join_err)?
    .map_err(|e| {
        CoreError::Other(format!(
            "wipe failed partway through: {e} -- if Beacon (or the game) is still running, close it and try again"
        ))
    })?;

    app.exit(0);
    Ok(())
}
