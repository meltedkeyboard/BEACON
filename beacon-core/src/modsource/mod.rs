//! Browse/install mods from Modrinth (keyless, always available) or CurseForge (needs the user's
//! own personal API key, pasted in Settings -- see `secret_store::{save,load}_curseforge_api_key`
//! for why Beacon can't ship its own key: CurseForge's Terms forbid embedding one in a distributed
//! app binary).
//!
//! Both sources converge on the same operations: `search`, `list_versions` (every file compatible
//! with the instance's target loader/Minecraft version, for a version-picker dropdown),
//! `preview_install` (what `install` would actually write to disk, without writing it -- Modrinth
//! only ever includes `required` dependencies here; CurseForge has no dependency graph exposed the
//! same way, so its preview is always exactly one entry), `install` itself, and `fetch_description`
//! (the mod's own README/overview, Markdown for Modrinth or HTML for CurseForge -- rendering and
//! sanitizing happens frontend-side, this just fetches the raw text).

use serde::{Deserialize, Serialize};

use crate::config::LauncherConfig;
use crate::downloader::ProgressCallback;
use crate::error::{CoreError, Result};
use crate::instance::{Instance, ModLoaderKind};

pub mod curseforge;
pub mod modrinth;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ModSource {
    Modrinth,
    CurseForge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSearchResult {
    pub id: String,
    pub source: ModSource,
    pub title: String,
    pub author: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
}

/// One selectable build in the version-picker dropdown (Modrinth: a version; CurseForge: a file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModVersionOption {
    pub id: String,
    pub version_number: String,
    pub filename: String,
    /// `false` for a beta/alpha build -- auto-pick (both the root mod when nothing's explicitly
    /// chosen, and every dependency) always prefers the newest stable build over a newer-but-less-
    /// stable one; this only affects what the version-picker dropdown displays/defaults to.
    pub is_stable: bool,
}

/// One row of "what `install` would actually write to disk" -- shown in the review table before
/// the user commits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInstallPreviewEntry {
    pub project_id: String,
    pub title: String,
    pub filename: String,
    pub version_number: String,
    pub is_dependency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModProvenanceEntry {
    pub source: ModSource,
    pub project_id: String,
}

pub async fn search(
    client: &reqwest::Client,
    source: ModSource,
    query: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    offset: u32,
    curseforge_api_key: Option<&str>,
) -> Result<Vec<ModSearchResult>> {
    match source {
        ModSource::Modrinth => modrinth::search(client, query, loader, mc_version, offset).await,
        ModSource::CurseForge => {
            let key = curseforge_api_key.ok_or(CoreError::CurseForgeKeyMissing)?;
            curseforge::search(client, key, query, loader, mc_version, offset).await
        }
    }
}

pub async fn list_versions(
    client: &reqwest::Client,
    source: ModSource,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    curseforge_api_key: Option<&str>,
) -> Result<Vec<ModVersionOption>> {
    match source {
        ModSource::Modrinth => modrinth::list_versions(client, project_id, loader, mc_version).await,
        ModSource::CurseForge => {
            let key = curseforge_api_key.ok_or(CoreError::CurseForgeKeyMissing)?;
            curseforge::list_versions(client, key, project_id, loader, mc_version).await
        }
    }
}

pub async fn preview_install(
    client: &reqwest::Client,
    source: ModSource,
    project_id: &str,
    version_id: Option<&str>,
    loader: ModLoaderKind,
    mc_version: &str,
    curseforge_api_key: Option<&str>,
) -> Result<Vec<ModInstallPreviewEntry>> {
    match source {
        ModSource::Modrinth => modrinth::preview_install(client, project_id, loader, mc_version, version_id).await,
        ModSource::CurseForge => {
            let key = curseforge_api_key.ok_or(CoreError::CurseForgeKeyMissing)?;
            curseforge::preview_install(client, key, project_id, loader, mc_version, version_id).await
        }
    }
}

/// Downloads `project_id`'s file -- `version_id` if given, else the newest compatible with
/// `instance`'s current `version_id` + `loader` (Modrinth: plus every `required` dependency,
/// resolved the same way) -- into `instance.mods_dir()`. Returns the root project's own installed
/// filename (not any dependency's), for the caller to record as this install's provenance.
pub async fn install(
    client: &reqwest::Client,
    config: &LauncherConfig,
    instance: &Instance,
    source: ModSource,
    project_id: &str,
    version_id: Option<&str>,
    loader: ModLoaderKind,
    curseforge_api_key: Option<&str>,
    on_progress: Option<ProgressCallback>,
) -> Result<String> {
    match source {
        ModSource::Modrinth => modrinth::install(client, config, instance, project_id, version_id, loader, on_progress).await,
        ModSource::CurseForge => {
            let key = curseforge_api_key.ok_or(CoreError::CurseForgeKeyMissing)?;
            curseforge::install(client, config, instance, project_id, version_id, loader, key, on_progress).await
        }
    }
}

pub async fn fetch_description(
    client: &reqwest::Client,
    source: ModSource,
    project_id: &str,
    curseforge_api_key: Option<&str>,
) -> Result<String> {
    match source {
        ModSource::Modrinth => modrinth::fetch_description(client, project_id).await,
        ModSource::CurseForge => {
            let key = curseforge_api_key.ok_or(CoreError::CurseForgeKeyMissing)?;
            curseforge::fetch_description(client, key, project_id).await
        }
    }
}
