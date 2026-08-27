//! `api.modrinth.com/v2` -- fully keyless, confirmed live against the real API (search, the
//! loader category slugs, the version/dependency shape, and the project `body` field are all
//! confirmed against real requests made while building this, not just the docs).

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::config::LauncherConfig;
use crate::downloader::{ensure_files, DownloadTask, ProgressCallback};
use crate::error::{io_err, CoreError, Result};
use crate::instance::{Instance, ModLoaderKind};

use super::{ModInstallPreviewEntry, ModSearchResult, ModSource, ModVersionOption};

const BASE_URL: &str = "https://api.modrinth.com/v2";
// Modrinth's docs ask for a descriptive User-Agent identifying the app (not a generic HTTP-client
// default) -- policy, not enforced, but easy to do right.
const USER_AGENT: &str = "beacon-launcher/0.1 (desktop Minecraft launcher)";

fn loader_slug(loader: ModLoaderKind) -> &'static str {
    match loader {
        ModLoaderKind::Fabric => "fabric",
        ModLoaderKind::Forge => "forge",
        ModLoaderKind::NeoForge => "neoforge",
        ModLoaderKind::Quilt => "quilt",
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    project_id: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    downloads: u64,
}

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    offset: u32,
) -> Result<Vec<ModSearchResult>> {
    // Outer arrays AND together, entries within one inner array OR together -- this is
    // "project_type=mod AND versions=<mc_version> AND categories=<loader>".
    let facets = format!(r#"[["project_type:mod"],["versions:{mc_version}"],["categories:{}"]]"#, loader_slug(loader));

    let response: SearchResponse = client
        .get(format!("{BASE_URL}/search"))
        .header("User-Agent", USER_AGENT)
        .query(&[
            ("query", query),
            ("facets", &facets),
            ("offset", &offset.to_string()),
            ("limit", "20"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(response
        .hits
        .into_iter()
        .map(|h| ModSearchResult {
            id: h.project_id,
            source: ModSource::Modrinth,
            title: h.title,
            author: h.author,
            description: h.description,
            icon_url: h.icon_url,
            downloads: h.downloads,
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
struct VersionFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    hashes: FileHashes,
}

#[derive(Debug, Clone, Deserialize)]
struct FileHashes {
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    #[serde(default)]
    project_id: Option<String>,
    dependency_type: String,
}

#[derive(Debug, Deserialize)]
struct VersionEntry {
    id: String,
    version_number: String,
    /// `"release"` | `"beta"` | `"alpha"` -- confirmed against a real request (Sodium's own
    /// `mc26.2-0.9.2-alpha.4-fabric` genuinely outranks its `mc26.2-0.9.1-fabric` release build by
    /// publish date alone). Auto-pick must not treat these as equally good just because Modrinth
    /// sorts the list newest-first with no regard for stability.
    version_type: String,
    #[serde(default)]
    files: Vec<VersionFile>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

fn pick_file(version: &VersionEntry) -> Option<&VersionFile> {
    version.files.iter().find(|f| f.primary).or_else(|| version.files.first())
}

/// Newest **stable** ("release") build compatible with `loader`/`mc_version` -- falling back to
/// the newest build of any type only if the project has never published a "release"-tagged build
/// at all (some mods only ever ship beta/alpha). Modrinth's own list is newest-first within the
/// filtered set, but mixes stability levels together with no ordering by that axis -- auto-pick
/// (both the root mod when no version is explicitly chosen, and every dependency, which is never
/// user-overridable) has to filter for it explicitly instead of just taking `versions[0]`.
async fn best_version(
    client: &reqwest::Client,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
) -> Result<Option<VersionEntry>> {
    let versions = versions_for(client, project_id, loader, mc_version).await?;
    let mut fallback = None;
    for version in versions {
        if version.version_type == "release" {
            return Ok(Some(version));
        }
        if fallback.is_none() {
            fallback = Some(version);
        }
    }
    Ok(fallback)
}

async fn versions_for(
    client: &reqwest::Client,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
) -> Result<Vec<VersionEntry>> {
    let url = format!(
        "{BASE_URL}/project/{project_id}/version?loaders=[\"{}\"]&game_versions=[\"{mc_version}\"]",
        loader_slug(loader),
    );
    Ok(client.get(&url).header("User-Agent", USER_AGENT).send().await?.error_for_status()?.json().await?)
}

async fn version_by_id(client: &reqwest::Client, version_id: &str) -> Result<VersionEntry> {
    Ok(client
        .get(format!("{BASE_URL}/version/{version_id}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub async fn list_versions(
    client: &reqwest::Client,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
) -> Result<Vec<ModVersionOption>> {
    let versions = versions_for(client, project_id, loader, mc_version).await?;
    Ok(versions
        .into_iter()
        .filter_map(|v| {
            let filename = pick_file(&v)?.filename.clone();
            let is_stable = v.version_type == "release";
            Some(ModVersionOption { id: v.id, version_number: v.version_number, filename, is_stable })
        })
        .collect())
}

pub async fn fetch_description(client: &reqwest::Client, project_id: &str) -> Result<String> {
    #[derive(Debug, Deserialize)]
    struct ProjectDetail {
        #[serde(default)]
        body: String,
    }
    let detail: ProjectDetail = client
        .get(format!("{BASE_URL}/project/{project_id}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(detail.body)
}

struct PlanEntry {
    project_id: String,
    version_number: String,
    filename: String,
    url: String,
    sha1: String,
    is_dependency: bool,
}

/// Resolves `project_id`'s file (`root_version_id` if given, else the newest compatible with
/// `loader`/`mc_version`) plus every `required` dependency's own newest compatible file --
/// `install` turns this into download tasks, `preview_install` turns it into display rows.
/// `seen` guards against a dependency cycle and against resolving/downloading a dependency shared
/// by two mods twice.
async fn resolve_plan(
    client: &reqwest::Client,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    root_version_id: Option<&str>,
) -> Result<Vec<PlanEntry>> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(project_id.to_string());

    let root_version = match root_version_id {
        Some(id) => Some(version_by_id(client, id).await?),
        None => best_version(client, project_id, loader, mc_version).await?,
    };
    let Some(root_version) = root_version else {
        return Ok(entries); // no file compatible with this instance's version/loader
    };

    let mut queue: Vec<String> = root_version
        .dependencies
        .iter()
        .filter(|d| d.dependency_type == "required")
        .filter_map(|d| d.project_id.clone())
        .collect();
    if let Some(file) = pick_file(&root_version) {
        entries.push(PlanEntry {
            project_id: project_id.to_string(),
            version_number: root_version.version_number.clone(),
            filename: file.filename.clone(),
            url: file.url.clone(),
            sha1: file.hashes.sha1.clone(),
            is_dependency: false,
        });
    }

    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(version) = best_version(client, &id, loader, mc_version).await? else {
            continue;
        };
        if let Some(file) = pick_file(&version) {
            entries.push(PlanEntry {
                project_id: id.clone(),
                version_number: version.version_number.clone(),
                filename: file.filename.clone(),
                url: file.url.clone(),
                sha1: file.hashes.sha1.clone(),
                is_dependency: true,
            });
        }
        for dep in version.dependencies {
            if dep.dependency_type == "required" {
                if let Some(dep_id) = dep.project_id {
                    queue.push(dep_id);
                }
            }
        }
    }
    Ok(entries)
}

/// One bulk lookup instead of N+1 requests -- used by `preview_install` to label every entry
/// (including dependencies) with its project's actual title.
async fn fetch_titles(client: &reqwest::Client, project_ids: &[String]) -> Result<HashMap<String, String>> {
    if project_ids.is_empty() {
        return Ok(HashMap::new());
    }
    #[derive(Debug, Deserialize)]
    struct ProjectSummary {
        id: String,
        title: String,
    }
    let ids_json = serde_json::to_string(project_ids)?;
    let projects: Vec<ProjectSummary> = client
        .get(format!("{BASE_URL}/projects"))
        .header("User-Agent", USER_AGENT)
        .query(&[("ids", ids_json.as_str())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(projects.into_iter().map(|p| (p.id, p.title)).collect())
}

pub async fn preview_install(
    client: &reqwest::Client,
    project_id: &str,
    loader: ModLoaderKind,
    mc_version: &str,
    version_id: Option<&str>,
) -> Result<Vec<ModInstallPreviewEntry>> {
    let plan = resolve_plan(client, project_id, loader, mc_version, version_id).await?;
    let ids: Vec<String> = plan.iter().map(|e| e.project_id.clone()).collect();
    let titles = fetch_titles(client, &ids).await?;
    Ok(plan
        .into_iter()
        .map(|e| ModInstallPreviewEntry {
            title: titles.get(&e.project_id).cloned().unwrap_or_else(|| e.project_id.clone()),
            project_id: e.project_id,
            filename: e.filename,
            version_number: e.version_number,
            is_dependency: e.is_dependency,
        })
        .collect())
}

pub async fn install(
    client: &reqwest::Client,
    config: &LauncherConfig,
    instance: &Instance,
    project_id: &str,
    version_id: Option<&str>,
    loader: ModLoaderKind,
    on_progress: Option<ProgressCallback>,
) -> Result<String> {
    let mods_dir = instance.mods_dir(config);
    tokio::fs::create_dir_all(&mods_dir).await.map_err(io_err(&mods_dir))?;

    let plan = resolve_plan(client, project_id, loader, &instance.version_id, version_id).await?;
    let root_filename = plan.iter().find(|e| !e.is_dependency).map(|e| e.filename.clone());
    let tasks = plan
        .into_iter()
        .map(|e| DownloadTask { url: e.url, dest: mods_dir.join(&e.filename), sha1: Some(e.sha1), size: None })
        .collect();
    ensure_files(client, tasks, on_progress).await?;

    root_filename.ok_or_else(|| CoreError::Other(format!("no file for project '{project_id}' compatible with this instance")))
}
