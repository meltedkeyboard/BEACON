use std::path::PathBuf;

use beacon_core::config::LauncherConfig;
use beacon_core::instance::{
    add_mods, create_instance, delete_datapack, delete_instance_dir, delete_mod, delete_resource_pack,
    delete_screenshot, delete_shader_pack, delete_world, export_instance, import_instance, list_mods,
    list_resource_packs, list_screenshots, list_shader_packs, list_worlds, rename_instance, toggle_mod,
};
use beacon_core::{CoreError, Instance, ModInfo, ResourcePackInfo, ScreenshotInfo, WorldInfo};
use tauri::{AppHandle, Manager, State};

use crate::state::{join_err, AppState};

/// `Instance` itself deliberately doesn't store resolved paths, so a stale absolute path never
/// ends up baked into config.json if `game_dir` moves.
#[derive(serde::Serialize)]
pub struct InstanceView {
    #[serde(flatten)]
    instance: Instance,
    dir: PathBuf,
    mods_dir: PathBuf,
    saves_dir: PathBuf,
    resource_packs_dir: PathBuf,
    shader_packs_dir: PathBuf,
    screenshots_dir: PathBuf,
}

pub(crate) fn instance_view(config: &LauncherConfig, instance: Instance) -> InstanceView {
    let dir = instance.dir(config);
    let mods_dir = instance.mods_dir(config);
    let saves_dir = instance.saves_dir(config);
    let resource_packs_dir = instance.resource_packs_dir(config);
    let shader_packs_dir = instance.shader_packs_dir(config);
    let screenshots_dir = instance.screenshots_dir(config);
    InstanceView {
        instance,
        dir,
        mods_dir,
        saves_dir,
        resource_packs_dir,
        shader_packs_dir,
        screenshots_dir,
    }
}

fn require_instance<'a>(config: &'a LauncherConfig, instance_id: &str) -> Result<&'a Instance, CoreError> {
    config
        .find_instance(instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))
}

/// Asset protocol scope is in-memory only and resets on every launch, hence re-allowing here.
pub fn allow_existing_icons(app: &AppHandle, config: &LauncherConfig) {
    let scope = app.asset_protocol_scope();
    for instance in &config.instances {
        if let Some(icon_path) = &instance.icon_path {
            let _ = scope.allow_file(icon_path);
        }
    }
}

#[derive(serde::Serialize)]
pub struct InstancesView {
    instances: Vec<InstanceView>,
    selected_id: Option<String>,
}

#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> Result<InstancesView, CoreError> {
    let config = state.config.lock().await;
    Ok(InstancesView {
        instances: config.instances.iter().cloned().map(|i| instance_view(&config, i)).collect(),
        selected_id: config.selected_instance_id.clone(),
    })
}

#[tauri::command]
pub async fn create_instance_cmd(
    state: State<'_, AppState>,
    name: String,
    version_id: String,
) -> Result<InstanceView, CoreError> {
    let mut config = state.config.lock().await;
    let instance = create_instance(&config, name, version_id)?;
    config.upsert_instance(instance.clone());
    config.selected_instance_id = Some(instance.id.clone());
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, instance))
}

#[tauri::command]
pub async fn select_instance_cmd(state: State<'_, AppState>, instance_id: String) -> Result<(), CoreError> {
    let mut config = state.config.lock().await;
    require_instance(&config, &instance_id)?;
    config.selected_instance_id = Some(instance_id);
    config.save(&state.config_path).await
}

/// Id is derived from the name, so renaming replaces the list entry (and on-disk directory).
#[tauri::command]
pub async fn rename_instance_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<InstanceView, CoreError> {
    let mut config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?.clone();
    let renamed = rename_instance(&config, &instance, name)?;

    let was_selected = config.selected_instance_id.as_deref() == Some(instance_id.as_str());
    config.instances.retain(|i| i.id != instance_id);
    config.upsert_instance(renamed.clone());
    if was_selected {
        config.selected_instance_id = Some(renamed.id.clone());
    }
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, renamed))
}

#[tauri::command]
pub async fn set_instance_version_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    version_id: String,
) -> Result<InstanceView, CoreError> {
    let mut config = state.config.lock().await;
    let position = config
        .instances
        .iter()
        .position(|i| i.id == instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    config.instances[position].version_id = version_id;
    config.instances[position].mod_loader = None; // loader build is version-specific, must reinstall
    let instance = config.instances[position].clone();
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, instance))
}

#[tauri::command]
pub async fn set_instance_icon_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    icon_path: Option<String>,
) -> Result<InstanceView, CoreError> {
    let mut config = state.config.lock().await;
    let position = config
        .instances
        .iter()
        .position(|i| i.id == instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    if let Some(path) = &icon_path {
        app.asset_protocol_scope()
            .allow_file(path)
            .map_err(|e| CoreError::Other(format!("couldn't allow icon path: {e}")))?;
    }
    config.instances[position].icon_path = icon_path.map(PathBuf::from);
    let instance = config.instances[position].clone();
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, instance))
}

#[tauri::command]
pub async fn delete_instance_cmd(state: State<'_, AppState>, instance_id: String) -> Result<(), CoreError> {
    let mut config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?.clone();
    delete_instance_dir(&config, &instance)?;
    config.instances.retain(|i| i.id != instance_id);
    if config.selected_instance_id.as_deref() == Some(instance_id.as_str()) {
        config.selected_instance_id = config.instances.first().map(|i| i.id.clone());
    }
    config.save(&state.config_path).await
}

#[tauri::command]
pub async fn list_worlds_cmd(state: State<'_, AppState>, instance_id: String) -> Result<Vec<WorldInfo>, CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    list_worlds(&config, instance)
}

#[tauri::command]
pub async fn list_resource_packs_cmd(state: State<'_, AppState>, instance_id: String) -> Result<Vec<ResourcePackInfo>, CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    list_resource_packs(&config, instance)
}

#[tauri::command]
pub async fn list_shader_packs_cmd(state: State<'_, AppState>, instance_id: String) -> Result<Vec<String>, CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    list_shader_packs(&config, instance)
}

#[tauri::command]
pub async fn delete_world_cmd(state: State<'_, AppState>, instance_id: String, world_name: String) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    delete_world(&config, instance, &world_name)
}

#[tauri::command]
pub async fn delete_datapack_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    world_name: String,
    datapack_name: String,
) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    delete_datapack(&config, instance, &world_name, &datapack_name)
}

#[tauri::command]
pub async fn delete_resource_pack_cmd(state: State<'_, AppState>, instance_id: String, file_name: String) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    delete_resource_pack(&config, instance, &file_name)
}

#[tauri::command]
pub async fn delete_shader_pack_cmd(state: State<'_, AppState>, instance_id: String, file_name: String) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    delete_shader_pack(&config, instance, &file_name)
}

#[tauri::command]
pub async fn list_screenshots_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ScreenshotInfo>, CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    app.asset_protocol_scope()
        .allow_directory(instance.screenshots_dir(&config), false)
        .map_err(|e| CoreError::Other(format!("couldn't allow screenshots folder: {e}")))?;
    list_screenshots(&config, instance)
}

#[tauri::command]
pub async fn delete_screenshot_cmd(state: State<'_, AppState>, instance_id: String, name: String) -> Result<(), CoreError> {
    let mut config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?.clone();
    delete_screenshot(&config, &instance, &name)?;

    if instance.pinned_screenshot.as_deref() == Some(name.as_str()) {
        let position = config.instances.iter().position(|i| i.id == instance_id).expect("just read above");
        config.instances[position].pinned_screenshot = None;
        config.save(&state.config_path).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_pinned_screenshot_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    name: Option<String>,
) -> Result<InstanceView, CoreError> {
    let mut config = state.config.lock().await;
    let position = config
        .instances
        .iter()
        .position(|i| i.id == instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    config.instances[position].pinned_screenshot = name;
    let instance = config.instances[position].clone();
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, instance))
}

#[tauri::command]
pub async fn list_mods_cmd(state: State<'_, AppState>, instance_id: String) -> Result<Vec<ModInfo>, CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    list_mods(&config, instance)
}

#[tauri::command]
pub async fn toggle_mod_cmd(state: State<'_, AppState>, instance_id: String, name: String, enable: bool) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    toggle_mod(&config, instance, &name, enable)
}

#[tauri::command]
pub async fn delete_mod_cmd(state: State<'_, AppState>, instance_id: String, name: String) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    delete_mod(&config, instance, &name)
}

#[tauri::command]
pub async fn add_mods_cmd(state: State<'_, AppState>, instance_id: String, source_paths: Vec<String>) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    let paths: Vec<PathBuf> = source_paths.into_iter().map(PathBuf::from).collect();
    add_mods(&config, instance, &paths)
}

/// Runs on a blocking thread: zipping a large instance directory would otherwise stall the worker.
#[tauri::command]
pub async fn export_instance_cmd(state: State<'_, AppState>, instance_id: String, dest_path: String) -> Result<(), CoreError> {
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let dest = PathBuf::from(dest_path);
    tokio::task::spawn_blocking(move || export_instance(&config, &instance, &dest))
        .await
        .map_err(join_err)?
}

#[tauri::command]
pub async fn import_instance_cmd(state: State<'_, AppState>, source_path: String) -> Result<InstanceView, CoreError> {
    let config_snapshot = state.config.lock().await.clone();
    let source = PathBuf::from(source_path);
    let instance = tokio::task::spawn_blocking(move || import_instance(&config_snapshot, &source))
        .await
        .map_err(join_err)??;

    let mut config = state.config.lock().await;
    config.upsert_instance(instance.clone());
    config.selected_instance_id = Some(instance.id.clone());
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, instance))
}
