//! Quilt: `meta.quiltmc.org/v3` is an intentional API/JSON-shape clone of Fabric's meta service
//! (Quilt started as a Fabric fork and kept parity here on purpose) -- see `super::fabric` for
//! the shared implementation this just points at a different base URL.

use crate::config::LauncherConfig;
use crate::error::Result;
use crate::manifest::VersionData;

use super::fabric::{install_from, list_versions_from};
use super::{LoaderVersionInfo, ProgressCallback};

const BASE_URL: &str = "https://meta.quiltmc.org/v3";

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
