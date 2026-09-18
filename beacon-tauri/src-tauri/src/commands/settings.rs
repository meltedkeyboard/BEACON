use std::path::PathBuf;

use beacon_core::config::relocate_directory;
use beacon_core::{forget_account, CoreError};
use tauri::{AppHandle, State};

use crate::state::{join_err, AppState};

#[derive(serde::Serialize)]
pub struct DirectorySettings {
    game_dir: PathBuf,
    instances_dir: PathBuf,
    /// Read-only: has to be known before config.json is read, so it can't live inside config.json.
    config_dir: PathBuf,
    libraries_dir: PathBuf,
}

/// Also creates the directories if missing, so Settings' "Open" buttons have something to open.
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
    tokio::fs::create_dir_all(&config_dir)
        .await
        .map_err(|e| CoreError::Other(format!("couldn't create '{}': {e}", config_dir.display())))?;

    Ok(DirectorySettings { game_dir, instances_dir, config_dir, libraries_dir })
}

/// Moves on a blocking thread; holding the config lock for the whole move would freeze every
/// other command until a potentially large copy finished.
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

/// Irreversible: deletes accounts, credential-store entries, all instances, and config.json.
/// Frontend gates this behind a typed confirmation before ever calling it.
#[tauri::command]
pub async fn wipe_all_data_cmd(app: AppHandle, state: State<'_, AppState>) -> Result<(), CoreError> {
    let config = state.config.lock().await.clone();

    for account in &config.accounts {
        let _ = forget_account(account).await; // best-effort
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
