//! `api.curseforge.com/v1` -- unlike Modrinth this needs an API key (the user's own, pasted in
//! Settings; see `modsource` module doc for why Beacon can't ship one itself). No dependency graph
//! is resolved here the way Modrinth's is: CurseForge's file-list response doesn't expose one the
//! same way, so `preview_install`/`install` always deal with exactly one file. `gameId`/`classId`/
//! `modLoaderType` are the long-standing numeric constants every third-party CurseForge
//! integration uses (Minecraft = 432, Mods category = 6, loader enum below) -- confirmed against
//! CurseForge's own API docs.

use serde::Deserialize;

use crate::config::LauncherConfig;
use crate::downloader::{ensure_files, DownloadTask, ProgressCallback};
use crate::error::{io_err, CoreError, Result};
use crate::instance::{Instance, ModLoaderKind};

use super::{ModInstallPreviewEntry, ModSearchResult, ModSource, ModVersionOption};

const BASE_URL: &str = "https://api.curseforge.com/v1";
const MINECRAFT_GAME_ID: u32 = 432;
const MODS_CLASS_ID: u32 = 6;

fn mod_loader_type(loader: ModLoaderKind) -> u32 {
    match loader {
        ModLoaderKind::Forge => 1,
        ModLoaderKind::Fabric => 4,
        ModLoaderKind::Quilt => 5,
        ModLoaderKind::NeoForge => 6,
    }
}

/// A 401/403 almost always means a missing/revoked/mistyped key -- worth a distinct, actionable
/// error instead of a generic "request failed" the user can't act on.
fn status_to_error(status: reqwest::StatusCode) -> CoreError {
    if status.as_u16() == 401 || status.as_u16() == 403 {
        CoreError::CurseForgeAuth
    } else {
        CoreError::Other(format!("CurseForge request failed: HTTP {status}"))
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    id: u32,
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    logo: Option<Logo>,
    #[serde(rename = "downloadCount", default)]
    download_count: f64,
}

#[derive(Debug, Deserialize)]
struct Author {
    name: String,
}

#[derive(Debug, Deserialize)]
struct Logo {
    #[serde(rename = "thumbnailUrl", default)]
    thumbnail_url: Option<String>,
}

pub async fn search(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    offset: u32,
) -> Result<Vec<ModSearchResult>> {
    let response = client
        .get(format!("{BASE_URL}/mods/search"))
        .header("x-api-key", api_key)
        .query(&[
            ("gameId", MINECRAFT_GAME_ID.to_string()),
            ("classId", MODS_CLASS_ID.to_string()),
            ("modLoaderType", mod_loader_type(loader).to_string()),
            ("gameVersion", mc_version.to_string()),
            ("searchFilter", query.to_string()),
            ("index", offset.to_string()),
            ("pageSize", "20".to_string()),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(status_to_error(response.status()));
    }
    let parsed: SearchResponse = response.json().await?;

    Ok(parsed
        .data
        .into_iter()
        .map(|h| ModSearchResult {
            id: h.id.to_string(),
            source: ModSource::CurseForge,
            title: h.name,
            author: h.authors.into_iter().next().map(|a| a.name).unwrap_or_default(),
            description: h.summary,
            icon_url: h.logo.and_then(|l| l.thumbnail_url),
            downloads: h.download_count as u64,
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
struct FileEntry {
    id: u32,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "downloadUrl", default)]
    download_url: Option<String>,
    /// 1 = Release, 2 = Beta, 3 = Alpha, per CurseForge's own `FileReleaseType` enum -- same
    /// stability-vs-recency bug as Modrinth's `version_type` (see that module's comment): the
    /// files list is newest-first with no regard for this, so auto-pick must filter for it
    /// explicitly instead of just taking the first entry with a download URL.
    #[serde(rename = "releaseType", default)]
    release_type: u8,
}

#[derive(Debug, Deserialize)]
struct FilesResponse {
    data: Vec<FileEntry>,
}

#[derive(Debug, Deserialize)]
struct SingleFileResponse {
    data: FileEntry,
}

#[derive(Debug, Deserialize)]
struct ModDetailResponse {
    data: ModDetail,
}

#[derive(Debug, Deserialize)]
struct ModDetail {
    name: String,
}

#[derive(Debug, Deserialize)]
struct DescriptionResponse {
    data: String,
}

async fn fetch_files(
    client: &reqwest::Client,
    api_key: &str,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
) -> Result<Vec<FileEntry>> {
    let response = client
        .get(format!("{BASE_URL}/mods/{project_id}/files"))
        .header("x-api-key", api_key)
        .query(&[("gameVersion", mc_version.to_string()), ("modLoaderType", mod_loader_type(loader).to_string())])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(status_to_error(response.status()));
    }
    Ok(response.json::<FilesResponse>().await?.data)
}

async fn fetch_file(client: &reqwest::Client, api_key: &str, project_id: &str, file_id: &str) -> Result<FileEntry> {
    let response = client.get(format!("{BASE_URL}/mods/{project_id}/files/{file_id}")).header("x-api-key", api_key).send().await?;
    if !response.status().is_success() {
        return Err(status_to_error(response.status()));
    }
    Ok(response.json::<SingleFileResponse>().await?.data)
}

/// Picks `file_id` if given, else the newest file that actually has a download URL -- a `null`
/// URL means the mod author opted out of third-party distribution (a real, documented CurseForge
/// behavior, not a bug here).
async fn resolve_file(
    client: &reqwest::Client,
    api_key: &str,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    file_id: Option<&str>,
) -> Result<Option<FileEntry>> {
    if let Some(file_id) = file_id {
        return Ok(Some(fetch_file(client, api_key, project_id, file_id).await?));
    }
    let files = fetch_files(client, api_key, project_id, loader, mc_version).await?;
    let downloadable: Vec<FileEntry> = files.into_iter().filter(|f| f.download_url.is_some()).collect();
    // Newest **release** file first (see `FileEntry::release_type` doc); fall back to newest of
    // any type only if the mod has never published a Release-tagged file for this version/loader.
    Ok(downloadable
        .iter()
        .find(|f| f.release_type == 1)
        .or_else(|| downloadable.first())
        .cloned())
}

pub async fn list_versions(
    client: &reqwest::Client,
    api_key: &str,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
) -> Result<Vec<ModVersionOption>> {
    let files = fetch_files(client, api_key, project_id, loader, mc_version).await?;
    Ok(files
        .into_iter()
        .filter(|f| f.download_url.is_some())
        .map(|f| ModVersionOption {
            id: f.id.to_string(),
            version_number: f.display_name,
            filename: f.file_name,
            is_stable: f.release_type == 1,
        })
        .collect())
}

pub async fn fetch_description(client: &reqwest::Client, api_key: &str, project_id: &str) -> Result<String> {
    let response = client.get(format!("{BASE_URL}/mods/{project_id}/description")).header("x-api-key", api_key).send().await?;
    if !response.status().is_success() {
        return Err(status_to_error(response.status()));
    }
    Ok(response.json::<DescriptionResponse>().await?.data)
}

async fn fetch_mod_name(client: &reqwest::Client, api_key: &str, project_id: &str) -> Result<String> {
    let response = client.get(format!("{BASE_URL}/mods/{project_id}")).header("x-api-key", api_key).send().await?;
    if !response.status().is_success() {
        return Err(status_to_error(response.status()));
    }
    Ok(response.json::<ModDetailResponse>().await?.data.name)
}

pub async fn preview_install(
    client: &reqwest::Client,
    api_key: &str,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    file_id: Option<&str>,
) -> Result<Vec<ModInstallPreviewEntry>> {
    let Some(file) = resolve_file(client, api_key, project_id, loader, mc_version, file_id).await? else {
        return Ok(Vec::new());
    };
    let title = fetch_mod_name(client, api_key, project_id).await?;
    Ok(vec![ModInstallPreviewEntry {
        project_id: project_id.to_string(),
        title,
        filename: file.file_name,
        version_number: file.display_name,
        is_dependency: false,
    }])
}

pub async fn install(
    client: &reqwest::Client,
    config: &LauncherConfig,
    instance: &Instance,
    project_id: &str,
    file_id: Option<&str>,
    loader: ModLoaderKind,
    api_key: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<String> {
    let Some(file) = resolve_file(client, api_key, project_id, loader, &instance.version_id, file_id).await? else {
        return Err(CoreError::Other(
            "no downloadable file for this Minecraft version/loader (the author may have disabled third-party downloads)"
                .to_string(),
        ));
    };
    let Some(download_url) = file.download_url else {
        return Err(CoreError::Other(
            "no downloadable file for this Minecraft version/loader (the author may have disabled third-party downloads)"
                .to_string(),
        ));
    };

    let mods_dir = instance.mods_dir(config);
    tokio::fs::create_dir_all(&mods_dir).await.map_err(io_err(&mods_dir))?;
    let task = DownloadTask { url: download_url, dest: mods_dir.join(&file.file_name), sha1: None, size: None };
    ensure_files(client, vec![task], on_progress).await?;
    Ok(file.file_name)
}
