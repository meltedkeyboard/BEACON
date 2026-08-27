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

// ---------------------------------------------------------------------------------------------
// Instances -- each one is its own sandbox (saves/resourcepacks/shaderpacks/config), isolated
// from every other instance, the same way Prism Launcher's instances are. Unlike accounts,
// there's no "move to front" scheme: `selected_instance_id` is just set directly, and the
// frontend's instance picker shows every instance in whatever order the backend returns them.
// ---------------------------------------------------------------------------------------------

/// An [`Instance`] plus its resolved absolute directory (and every content subfolder's own
/// resolved path, for the instance screen's "open folder" buttons) -- the frontend needs these to
/// open them in the file explorer, but `Instance` itself deliberately doesn't store any of them,
/// so a stale absolute path never ends up baked into `config.json` if `game_dir` ever moves.
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

/// Grants the asset protocol scope read access to every instance icon already on disk. The scope
/// (`tauri.conf.json`'s `assetProtocol.scope`) starts empty -- icons live at arbitrary,
/// user-chosen paths outside any directory Beacon controls, so instead of statically allowing the
/// whole filesystem (`["**"]`), each icon path is allowed individually, once at startup for
/// existing icons and again in [`set_instance_icon_cmd`] whenever a new one is picked. The scope
/// itself is in-memory only and resets on every launch, hence re-allowing here.
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
    // A freshly created instance becomes current -- you just made it, Play should use it.
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

/// Renames an instance. Its id is derived from the name (see [`beacon_core::Instance::id`]), so
/// like [`crate::commands::accounts::rename_offline_account_cmd`] this replaces the list entry
/// (and, here, the instance's on-disk directory) rather than mutating it in place.
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
    // A loader build targets one specific Minecraft version -- changing it invalidates whatever
    // was installed, same as every other launcher's behavior here. The user just reinstalls it
    // for the new version from the instance screen.
    config.instances[position].mod_loader = None;
    let instance = config.instances[position].clone();
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, instance))
}

/// `icon_path` is `None` to clear the icon back to the fallback glyph, `Some(path)` to set it --
/// the path itself comes from the frontend's native file-picker dialog (`@tauri-apps/plugin-dialog`).
/// Since the asset protocol scope starts empty (see [`allow_existing_icons`]), a newly picked path
/// is granted read access here -- otherwise the webview couldn't load it via `convertFileSrc()`.
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

/// Also grants the asset protocol scope read access to the instance's whole `screenshots/` folder
/// before returning -- like `set_instance_icon_cmd`, the scope starts empty, but unlike a single
/// icon path a folder that can hold dozens of screenshots is worth granting once as a directory
/// (non-recursive; screenshots live directly in it, no subfolders) rather than file-by-file.
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

/// Deletes a screenshot, clearing it as the pinned Play-tab backdrop first if it was pinned --
/// otherwise the instance would be left pointing at a pin that no longer exists on disk.
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

/// Pins a screenshot as this instance's Play-tab backdrop (`Some(name)`), or unpins back to
/// rotating through all of them (`None`).
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

/// Copies picked mod jars (from the frontend's open-file dialog) into the instance's `mods/`
/// folder. Unlike `export_instance_cmd`/`import_instance_cmd` below, this doesn't need
/// `spawn_blocking` -- a handful of mod jars copy fast enough not to stall this command's worker
/// thread the way zipping/unzipping a whole instance directory can.
#[tauri::command]
pub async fn add_mods_cmd(state: State<'_, AppState>, instance_id: String, source_paths: Vec<String>) -> Result<(), CoreError> {
    let config = state.config.lock().await;
    let instance = require_instance(&config, &instance_id)?;
    let paths: Vec<PathBuf> = source_paths.into_iter().map(PathBuf::from).collect();
    add_mods(&config, instance, &paths)
}

/// Zips the instance's whole directory to `dest_path` (chosen by the frontend's save-file
/// dialog). Runs on a blocking thread -- unlike the small list/delete commands above, a heavily-
/// played instance's directory can be large enough that walking and compressing it would
/// otherwise stall this command's async worker thread for a noticeable moment.
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

/// Unpacks an instance archive (chosen by the frontend's open-file dialog) into a new instance
/// and adds it to the config. See [`export_instance_cmd`] for why this runs on a blocking thread.
#[tauri::command]
pub async fn import_instance_cmd(state: State<'_, AppState>, source_path: String) -> Result<InstanceView, CoreError> {
    let config_snapshot = state.config.lock().await.clone();
    let source = PathBuf::from(source_path);
    let instance = tokio::task::spawn_blocking(move || import_instance(&config_snapshot, &source))
        .await
        .map_err(join_err)??;

    let mut config = state.config.lock().await;
    config.upsert_instance(instance.clone());
    // Same as `create_instance_cmd` -- an imported instance becomes current immediately.
    config.selected_instance_id = Some(instance.id.clone());
    config.save(&state.config_path).await?;
    Ok(instance_view(&config, instance))
}
