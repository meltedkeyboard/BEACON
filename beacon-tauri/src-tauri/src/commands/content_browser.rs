use std::sync::Arc;

use beacon_core::downloader::{DownloadProgress, ProgressCallback};
use beacon_core::instance::{delete_mod, load_content_provenance, record_content_provenance};
use beacon_core::modsource::ContentKind;
use beacon_core::{modsource, secret_store, CoreError, Instance, ModLoaderKind};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// The mod browser, resource-pack browser, and shader-pack browser (instance-detail Mods/Resource
/// Packs/Shader Packs tabs' own "Browse…" buttons) are all the same search/preview/install flow
/// against Modrinth/CurseForge -- these commands are shared across all three, parametrized by
/// `kind`, rather than tripled. Only `Mod` needs (and requires) a mod loader; a resource pack or
/// shader pack search/install is scoped to the instance's Minecraft version alone.
fn parse_kind(kind: &str) -> Result<ContentKind, CoreError> {
    match kind {
        "Mod" => Ok(ContentKind::Mod),
        "ResourcePack" => Ok(ContentKind::ResourcePack),
        "ShaderPack" => Ok(ContentKind::ShaderPack),
        other => Err(CoreError::Other(format!("unknown content kind '{other}'"))),
    }
}

fn parse_source(source: &str) -> Result<modsource::ModSource, CoreError> {
    match source {
        "Modrinth" => Ok(modsource::ModSource::Modrinth),
        "CurseForge" => Ok(modsource::ModSource::CurseForge),
        other => Err(CoreError::Other(format!("unknown mod source '{other}'"))),
    }
}

/// `Mod` needs an installed loader (mods are loader-specific builds); `ResourcePack`/`ShaderPack`
/// don't have a loader concept at all, so `None` for those regardless of whether one's installed.
fn loader_for(instance: &Instance, kind: ContentKind) -> Result<Option<ModLoaderKind>, CoreError> {
    match kind {
        ContentKind::Mod => instance
            .mod_loader
            .as_ref()
            .map(|l| l.kind)
            .ok_or_else(|| CoreError::Other("this instance has no mod loader installed yet".to_string()))
            .map(Some),
        ContentKind::ResourcePack | ContentKind::ShaderPack => Ok(None),
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
pub async fn search_content_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
    source: String,
    query: String,
    offset: u32,
) -> Result<Vec<modsource::ModSearchResult>, CoreError> {
    let kind = parse_kind(&kind)?;
    let source = parse_source(&source)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = loader_for(instance, kind)?;
    let api_key = curseforge_key_if_needed(source).await?;

    modsource::search(&state.http, source, kind, &query, loader, &instance.version_id, offset, api_key.as_deref()).await
}

#[tauri::command]
pub async fn list_content_versions_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
    source: String,
    project_id: String,
) -> Result<Vec<modsource::ModVersionOption>, CoreError> {
    let kind = parse_kind(&kind)?;
    let source = parse_source(&source)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = loader_for(instance, kind)?;
    let api_key = curseforge_key_if_needed(source).await?;

    modsource::list_versions(&state.http, source, &project_id, loader, &instance.version_id, api_key.as_deref()).await
}

/// The project's own README/overview -- raw Markdown (Modrinth) or raw HTML (CurseForge).
/// Rendering and sanitizing (both are third-party rich text) happen frontend-side; this only
/// fetches. Kind-agnostic: both APIs' description endpoints work the same regardless of whether
/// the project is a mod, resource pack, or shader pack.
#[tauri::command]
pub async fn get_content_description_cmd(state: State<'_, AppState>, source: String, project_id: String) -> Result<String, CoreError> {
    let source = parse_source(&source)?;
    let api_key = curseforge_key_if_needed(source).await?;
    modsource::fetch_description(&state.http, source, &project_id, api_key.as_deref()).await
}

#[tauri::command]
pub async fn preview_content_install_cmd(
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
    source: String,
    project_id: String,
    version_id: Option<String>,
) -> Result<Vec<modsource::ModInstallPreviewEntry>, CoreError> {
    let kind = parse_kind(&kind)?;
    let source = parse_source(&source)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = loader_for(instance, kind)?;
    let api_key = curseforge_key_if_needed(source).await?;

    modsource::preview_install(&state.http, source, &project_id, version_id.as_deref(), loader, &instance.version_id, api_key.as_deref()).await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSelection {
    source: String,
    project_id: String,
    version_id: Option<String>,
}

/// Installs every selected item in turn (sequential, so `content-install-progress` events stay
/// meaningful instead of interleaving across items), recording each one's own root file as
/// provenance so it shows as "Installed" (with a Remove option) next time this instance's
/// mods/resource packs/shader packs are browsed.
#[tauri::command]
pub async fn install_selected_content_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
    selections: Vec<ContentSelection>,
) -> Result<(), CoreError> {
    let kind = parse_kind(&kind)?;
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = loader_for(&instance, kind)?;
    let curseforge_key = secret_store::load_curseforge_api_key().await?;
    let dest_dir = kind.dir(&config, &instance);

    for selection in selections {
        let source = parse_source(&selection.source)?;
        let api_key = if source == modsource::ModSource::CurseForge { curseforge_key.as_deref() } else { None };
        let progress_app = app.clone();
        let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
            let _ = progress_app.emit("content-install-progress", progress);
        });
        let filename = modsource::install(
            &state.http,
            &config,
            &instance,
            source,
            kind,
            &selection.project_id,
            selection.version_id.as_deref(),
            loader,
            api_key,
            Some(on_progress),
        )
        .await?;
        record_content_provenance(&dest_dir, &filename, source, &selection.project_id)?;
    }

    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentProvenanceView {
    source: modsource::ModSource,
    project_id: String,
    filename: String,
}

/// What's already installed via the content browser for this instance -- used right after a search
/// to mark matching results "Installed" (with Remove) instead of offering a checkbox for them again.
#[tauri::command]
pub async fn list_content_provenance_cmd(state: State<'_, AppState>, instance_id: String, kind: String) -> Result<Vec<ContentProvenanceView>, CoreError> {
    let kind = parse_kind(&kind)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let dir = kind.dir(&config, instance);
    Ok(load_content_provenance(&dir)
        .into_iter()
        .map(|(filename, entry)| ContentProvenanceView { source: entry.source, project_id: entry.project_id, filename })
        .collect())
}

#[tauri::command]
pub async fn remove_content_source_cmd(state: State<'_, AppState>, instance_id: String, kind: String, filename: String) -> Result<(), CoreError> {
    let kind = parse_kind(&kind)?;
    let config = state.config.lock().await;
    let instance = config
        .find_instance(&instance_id)
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    match kind {
        ContentKind::Mod => delete_mod(&config, instance, &filename)?,
        ContentKind::ResourcePack => beacon_core::instance::delete_resource_pack(&config, instance, &filename)?,
        ContentKind::ShaderPack => beacon_core::instance::delete_shader_pack(&config, instance, &filename)?,
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentUpdateView {
    source: modsource::ModSource,
    project_id: String,
    filename: String,
    latest_version_id: String,
    latest_version_number: String,
    latest_filename: String,
}

/// For every installed item this instance has provenance for (see `list_content_provenance_cmd`),
/// asks the source what its newest compatible build is and reports the ones where that build's
/// filename doesn't match what's actually on disk -- there's no stored "installed version id" to
/// compare against directly, so a filename mismatch is what stands in for "out of date" (the same
/// signal `install`'s own naming already gives every other build). An item whose version lookup
/// fails outright (project pulled from the source, transient network error) is silently skipped
/// rather than failing the whole batch -- one broken project shouldn't hide updates for the rest.
#[tauri::command]
pub async fn check_content_updates_cmd(state: State<'_, AppState>, instance_id: String, kind: String) -> Result<Vec<ContentUpdateView>, CoreError> {
    let kind = parse_kind(&kind)?;
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = loader_for(&instance, kind)?;
    let curseforge_key = secret_store::load_curseforge_api_key().await?;
    let dir = kind.dir(&config, &instance);

    let mut updates = Vec::new();
    for (filename, entry) in load_content_provenance(&dir) {
        let api_key = if entry.source == modsource::ModSource::CurseForge { curseforge_key.as_deref() } else { None };
        let Ok(versions) = modsource::list_versions(&state.http, entry.source, &entry.project_id, loader, &instance.version_id, api_key).await else {
            continue;
        };
        let Some(latest) = versions.iter().find(|v| v.is_stable).or_else(|| versions.first()) else { continue };
        if latest.filename != filename {
            updates.push(ContentUpdateView {
                source: entry.source,
                project_id: entry.project_id,
                filename,
                latest_version_id: latest.id.clone(),
                latest_version_number: latest.version_number.clone(),
                latest_filename: latest.filename.clone(),
            });
        }
    }
    Ok(updates)
}

/// Downloads `version_id` (a `ContentUpdateView.latest_version_id` from `check_content_updates_cmd`)
/// in place of `old_filename`, then re-records provenance under the new filename -- same
/// install-then-record shape as `install_selected_content_cmd`, just for one item, plus the extra
/// step of deleting the superseded file once the new one is safely down.
#[tauri::command]
pub async fn update_content_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
    source: String,
    project_id: String,
    old_filename: String,
    version_id: String,
) -> Result<(), CoreError> {
    let kind = parse_kind(&kind)?;
    let source = parse_source(&source)?;
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| CoreError::Other(format!("no instance '{instance_id}'")))?;
    let loader = loader_for(&instance, kind)?;
    let curseforge_key = secret_store::load_curseforge_api_key().await?;
    let api_key = if source == modsource::ModSource::CurseForge { curseforge_key.as_deref() } else { None };
    let dest_dir = kind.dir(&config, &instance);

    let progress_app = app.clone();
    let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
        let _ = progress_app.emit("content-install-progress", progress);
    });
    let new_filename = modsource::install(
        &state.http,
        &config,
        &instance,
        source,
        kind,
        &project_id,
        Some(version_id.as_str()),
        loader,
        api_key,
        Some(on_progress),
    )
    .await?;

    if new_filename != old_filename {
        match kind {
            ContentKind::Mod => delete_mod(&config, &instance, &old_filename)?,
            ContentKind::ResourcePack => beacon_core::instance::delete_resource_pack(&config, &instance, &old_filename)?,
            ContentKind::ShaderPack => beacon_core::instance::delete_shader_pack(&config, &instance, &old_filename)?,
        }
    }
    record_content_provenance(&dest_dir, &new_filename, source, &project_id)?;
    Ok(())
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
