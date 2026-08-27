//! NeoForge is a literal fork of Forge's installer code (same `install_profile.json`/processor
//! mechanism, just `net.neoforged.*` coordinates and `maven.neoforged.net` URLs) -- see
//! `super::forge` for the shared processor-execution engine this reuses as-is.

use serde::Deserialize;

use crate::config::LauncherConfig;
use crate::error::Result;
use crate::manifest::VersionData;

use super::forge::install_from_installer;
use super::{version_sort_key, LoaderVersionInfo, ProgressCallback};

/// NeoForge versions are named `{minecraft_minor}.{minecraft_patch}.{build}` with the leading
/// `1.` dropped (e.g. Minecraft `1.21.4` -> NeoForge `21.4.x`) -- confirmed against the live
/// version list, not guessed.
fn version_prefix(mc_version: &str) -> String {
    let rest = mc_version.strip_prefix("1.").unwrap_or(mc_version);
    let mut parts = rest.splitn(2, '.');
    let minor = parts.next().unwrap_or(rest);
    let patch = parts.next().unwrap_or("0");
    format!("{minor}.{patch}.")
}

pub async fn list_versions(client: &reqwest::Client, mc_version: &str) -> Result<Vec<LoaderVersionInfo>> {
    #[derive(Deserialize)]
    struct VersionsResponse {
        versions: Vec<String>,
    }
    let response: VersionsResponse = client
        .get("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let prefix = version_prefix(mc_version);
    let mut versions: Vec<String> = response.versions.into_iter().filter(|v| v.starts_with(&prefix)).collect();
    versions.sort_by_key(|v| std::cmp::Reverse(version_sort_key(v)));

    Ok(versions
        .into_iter()
        .map(|v| {
            let stable = !v.ends_with("-beta");
            LoaderVersionInfo { version: v, stable, recommended: false }
        })
        .collect())
}

pub async fn install(
    client: &reqwest::Client,
    config: &LauncherConfig,
    mc_version: &str,
    loader_version: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<VersionData> {
    let installer_url = format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar"
    );
    let installer_file_name = format!("neoforge-{loader_version}-installer.jar");
    install_from_installer(client, config, "neoforge", mc_version, loader_version, installer_url, installer_file_name, on_progress).await
}
