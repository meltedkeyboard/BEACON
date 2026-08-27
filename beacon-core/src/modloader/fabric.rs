//! Fabric: `meta.fabricmc.net/v2` returns a ready-to-merge partial version JSON for any
//! (game_version, loader_version) pair -- no local installer, no patching, just fetch + merge +
//! download the libraries it lists. See `super::quilt`, which is a byte-for-byte structural clone
//! of this file against Quilt's API-compatible `v3` meta service.

use serde::Deserialize;

use crate::config::LauncherConfig;
use crate::error::Result;
use crate::manifest::{merge_with_vanilla, LoaderProfile, VersionData};

use super::{cache_version_data, urlencode, LoaderVersionInfo, ProgressCallback};

const BASE_URL: &str = "https://meta.fabricmc.net/v2";

#[derive(Debug, Deserialize)]
struct LoaderListEntry {
    loader: LoaderBuild,
}

#[derive(Debug, Deserialize)]
struct LoaderBuild {
    version: String,
    stable: bool,
}

pub async fn list_versions(client: &reqwest::Client, mc_version: &str) -> Result<Vec<LoaderVersionInfo>> {
    list_versions_from(client, BASE_URL, mc_version).await
}

pub async fn install(
    client: &reqwest::Client,
    config: &LauncherConfig,
    mc_version: &str,
    loader_version: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<VersionData> {
    install_from(client, config, BASE_URL, mc_version, loader_version, on_progress).await
}

/// Shared by `fabric.rs` and `quilt.rs` -- parameterized by `base_url` since Quilt intentionally
/// kept API/JSON-shape parity with Fabric.
pub(super) async fn list_versions_from(
    client: &reqwest::Client,
    base_url: &str,
    mc_version: &str,
) -> Result<Vec<LoaderVersionInfo>> {
    let url = format!("{base_url}/versions/loader/{}", urlencode(mc_version));
    let entries: Vec<LoaderListEntry> = client.get(&url).send().await?.error_for_status()?.json().await?;
    Ok(entries
        .into_iter()
        .map(|e| LoaderVersionInfo { version: e.loader.version, stable: e.loader.stable, recommended: false })
        .collect())
}

pub(super) async fn install_from(
    client: &reqwest::Client,
    config: &LauncherConfig,
    base_url: &str,
    mc_version: &str,
    loader_version: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<VersionData> {
    let vanilla = crate::launcher::install_version(client, config, mc_version, on_progress.clone()).await?;

    let url = format!(
        "{base_url}/versions/loader/{}/{}/profile/json",
        urlencode(mc_version),
        urlencode(loader_version),
    );
    let profile: LoaderProfile = client.get(&url).send().await?.error_for_status()?.json().await?;
    let merged = merge_with_vanilla(&vanilla, profile);

    cache_version_data(config, &merged).await?;
    // Re-run install_version on the merged id: `load_or_fetch_version_data` now finds the cached
    // JSON we just wrote, so this only downloads the loader's own libraries (fabric-loader +
    // intermediary), nothing already-fetched is redone.
    crate::launcher::install_version(client, config, &merged.id, on_progress).await?;
    Ok(merged)
}
