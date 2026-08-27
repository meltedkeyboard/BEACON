use std::sync::Arc;

use beacon_core::downloader::{DownloadProgress, ProgressCallback};
use beacon_core::instance::{delete_mod, list_mods, load_mod_provenance, record_mod_provenance};
use beacon_core::{modsource, secret_store, CoreError, Instance, ModInfo, ModLoaderKind};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

fn require_loader(instance: &Instance) -> Result<ModLoaderKind, CoreError> {
    instance
        .mod_loader
        .as_ref()
        .map(|l| l.kind)
        .ok_or_else(|| CoreError::Other("this instance has no mod loader installed yet".to_string()))
}

fn parse_source(source: &str) -> Result<modsource::ModSource, CoreError> {
    match source {
        "Modrinth" => Ok(modsource::ModSource::Modrinth),
        "CurseForge" => Ok(modsource::ModSource::CurseForge),
        other => Err(CoreError::Other(format!("unknown mod source '{other}'"))),
    }
}

async fn curseforge_key_if_needed(source: modsource::ModSource) -> Result<Option<String>, CoreError> {
    if source == modsource::ModSource::CurseForge {
        secret_store::load_curseforge_api_key().await
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn search_mods_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    source: String,
    query: String,
    offset: u32,
) -> Result<Vec<modsource::ModSearchResult>, CoreError> {
    let source = parse_source(&source)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = require_loader(instance)?;
    let api_key = curseforge_key_if_needed(source).await?;

    modsource::search(&state.http, source, &query, loader, &instance.version_id, offset, api_key.as_deref()).await
}

#[tauri::command]
pub async fn list_mod_versions_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    source: String,
    project_id: String,
) -> Result<Vec<modsource::ModVersionOption>, CoreError> {
    let source = parse_source(&source)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = require_loader(instance)?;
    let api_key = curseforge_key_if_needed(source).await?;

    modsource::list_versions(&state.http, source, &project_id, loader, &instance.version_id, api_key.as_deref()).await
}

/// The mod's own README/overview -- raw Markdown (Modrinth) or raw HTML (CurseForge). Rendering
/// and sanitizing (both are third-party rich text) happen frontend-side; this only fetches.
#[tauri::command]
pub async fn get_mod_description_cmd(state: State<'_, AppState>, source: String, project_id: String) -> Result<String, CoreError> {
    let source = parse_source(&source)?;
    let api_key = curseforge_key_if_needed(source).await?;
    modsource::fetch_description(&state.http, source, &project_id, api_key.as_deref()).await
}

#[tauri::command]
pub async fn preview_mod_install_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    source: String,
    project_id: String,
    version_id: Option<String>,
) -> Result<Vec<modsource::ModInstallPreviewEntry>, CoreError> {
    let source = parse_source(&source)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = require_loader(instance)?;
    let api_key = curseforge_key_if_needed(source).await?;

    modsource::preview_install(&state.http, source, &project_id, version_id.as_deref(), loader, &instance.version_id, api_key.as_deref()).await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSelection {
    source: String,
    project_id: String,
    version_id: Option<String>,
}

/// Installs every selected mod in turn (sequential, so `mod-install-progress` events stay
/// meaningful instead of interleaving across mods), recording each one's own root file as
/// provenance so it shows as "Installed" (with a Remove option) next time this instance's mods are
/// browsed. Returns the instance's refreshed mod list once everything's done.
#[tauri::command]
pub async fn install_selected_mods_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    selections: Vec<ModSelection>,
) -> Result<Vec<ModInfo>, CoreError> {
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = require_loader(&instance)?;
    let curseforge_key = secret_store::load_curseforge_api_key().await?;

    for selection in selections {
        let source = parse_source(&selection.source)?;
        let api_key = if source == modsource::ModSource::CurseForge { curseforge_key.as_deref() } else { None };
        let progress_app = app.clone();
        let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
            let _ = progress_app.emit("mod-install-progress", progress);
        });
        let filename = modsource::install(
            &state.http,
            &config,
            &instance,
            source,
            &selection.project_id,
            selection.version_id.as_deref(),
            loader,
            api_key,
            Some(on_progress),
        )
        .await?;
        record_mod_provenance(&config, &instance, &filename, source, &selection.project_id)?;
    }

    list_mods(&config, &instance)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModProvenanceView {
    source: modsource::ModSource,
    project_id: String,
    filename: String,
}

/// What's already installed via the mod browser for this instance -- used right after a search to
/// mark matching results "Installed" (with Remove) instead of offering a checkbox for them again.
#[tauri::command]
pub async fn list_mod_provenance_cmd(state: State<'_, AppState>, instance_id: String) -> Result<Vec<ModProvenanceView>, CoreError> {
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    Ok(load_mod_provenance(&config, instance)
        .into_iter()
        .map(|(filename, entry)| ModProvenanceView { source: entry.source, project_id: entry.project_id, filename })
        .collect())
}

#[tauri::command]
pub async fn remove_mod_source_cmd(state: State<'_, AppState>, instance_id: String, filename: String) -> Result<Vec<ModInfo>, CoreError> {
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    delete_mod(&config, instance, &filename)?;
    list_mods(&config, instance)
}

#[tauri::command]
pub async fn set_curseforge_api_key_cmd(key: Option<String>) -> Result<(), CoreError> {
    match key.filter(|k| !k.trim().is_empty()) {
        Some(key) => secret_store::save_curseforge_api_key(key.trim()).await,
        None => secret_store::delete_curseforge_api_key().await,
    }
}

#[tauri::command]
pub async fn has_curseforge_api_key_cmd() -> Result<bool, CoreError> {
    Ok(secret_store::load_curseforge_api_key().await?.is_some())
}
