use std::sync::Arc;

use beacon_core::downloader::{DownloadProgress, ProgressCallback};
use beacon_core::instance::ModLoaderKind;
use beacon_core::{modloader, CoreError, ModLoaderInfo};
use tauri::{AppHandle, Emitter, State};

use crate::commands::instances::InstanceView;
use crate::state::AppState;

fn parse_kind(kind: &str) -> Result<ModLoaderKind, CoreError> {
    match kind {
        "Fabric" => Ok(ModLoaderKind::Fabric),
        "Forge" => Ok(ModLoaderKind::Forge),
        "NeoForge" => Ok(ModLoaderKind::NeoForge),
        "Quilt" => Ok(ModLoaderKind::Quilt),
        other => Err(CoreError::Other(format!("unknown mod loader kind '{other}'"))),
    }
}

#[tauri::command]
pub async fn list_loader_versions_cmd(
    state: State<'_, AppState>,
    kind: String,
    mc_version: String,
) -> Result<Vec<modloader::LoaderVersionInfo>, CoreError> {
    let kind = parse_kind(&kind)?;
    modloader::list_versions(&state.http, kind, &mc_version).await
}

/// Installs a mod loader on top of `instance_id`'s current (vanilla) `version_id`, emitting
/// `loader-install-progress` events as it goes -- a separate event name from `install-progress`
/// so this doesn't interfere with the Play button's own progress state while the install-loader
/// modal is open. On success, records the merged effective version id on the instance so
/// `launch_instance_cmd` picks it up on the next Play click.
#[tauri::command]
pub async fn install_loader_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
    loader_version: String,
) -> Result<InstanceView, CoreError> {
    let kind = parse_kind(&kind)?;
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;

    let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
        let _ = app.emit("loader-install-progress", progress);
    });
    let merged = modloader::install(&state.http, &config, kind, &instance.version_id, &loader_version, Some(on_progress)).await?;

    let mut config = state.config.lock().await;
    let position = config
        .instances
        .iter()
        .position(|i| i.id == instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    config.instances[position].mod_loader = Some(ModLoaderInfo {
        kind,
        loader_version,
        effective_version_id: merged.id,
    });
    let instance = config.instances[position].clone();
    config.save(&state.config_path).await?;
    Ok(crate::commands::instances::instance_view(&config, instance))
}

#[tauri::command]
pub async fn remove_loader_cmd(state: State<'_, AppState>, instance_id: String) -> Result<InstanceView, CoreError> {
    let mut config = state.config.lock().await;
    let position = config
        .instances
        .iter()
        .position(|i| i.id == instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    // No file cleanup -- downloaded libraries are shared/content-addressed under `libraries_dir`,
    // same philosophy as vanilla version downloads never being cleaned up either.
    config.instances[position].mod_loader = None;
    let instance = config.instances[position].clone();
    config.save(&state.config_path).await?;
    Ok(crate::commands::instances::instance_view(&config, instance))
}
