//! Mod loader install/list for the four loaders Beacon supports. Fabric and Quilt
//! (`fabric.rs`/`quilt.rs`) share one implementation: their API returns a ready-to-merge partial
//! version JSON, no local installer involved. Forge and NeoForge (`forge.rs`/`neoforge.rs`) share
//! a different implementation: their "installer" is a jar whose `install_profile.json`
//! describes a sequence of external tools ("processors") that must actually be run to
//! binary-patch the vanilla client jar -- see `forge.rs`'s module doc for the details.
//!
//! Both paths converge on the same thing: a [`crate::manifest::LoaderProfile`] merged onto the
//! vanilla `VersionData` via [`crate::manifest::merge_with_vanilla`], cached to disk under the
//! merged profile's own `id` exactly where [`crate::launcher::load_or_fetch_version_data`]
//! expects to find a version's cache -- so `install_version`/`launch` need no changes at all to
//! run a modded instance.

use serde::{Deserialize, Serialize};

use crate::config::LauncherConfig;
use crate::error::io_err;
use crate::error::Result;
use crate::instance::ModLoaderKind;
use crate::manifest::VersionData;

pub mod fabric;
pub mod forge;
pub mod neoforge;
pub mod quilt;

/// One selectable loader build, as shown in the install-loader picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderVersionInfo {
    pub version: String,
    /// Fabric/Quilt: the loader's own "stable" flag. Forge: always `true` (no beta channel in
    /// the version list we scrape). NeoForge: `false` for a `-beta` suffixed build.
    pub stable: bool,
    /// Only ever `true` for the Forge build `promotions_slim.json` names as `{mc}-recommended`.
    pub recommended: bool,
}

pub type ProgressCallback = crate::downloader::ProgressCallback;

pub async fn list_versions(
    client: &reqwest::Client,
    kind: ModLoaderKind,
    mc_version: &str,
) -> Result<Vec<LoaderVersionInfo>> {
    match kind {
        ModLoaderKind::Fabric => fabric::list_versions(client, mc_version).await,
        ModLoaderKind::Quilt => quilt::list_versions(client, mc_version).await,
        ModLoaderKind::Forge => forge::list_versions(client, mc_version).await,
        ModLoaderKind::NeoForge => neoforge::list_versions(client, mc_version).await,
    }
}

/// Installs `loader_version` of `kind` on top of `mc_version` -- fully downloaded and ready to
/// launch by the time this returns. The returned `VersionData.id` is what
/// `ModLoaderInfo::effective_version_id` should be set to.
pub async fn install(
    client: &reqwest::Client,
    config: &LauncherConfig,
    kind: ModLoaderKind,
    mc_version: &str,
    loader_version: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<VersionData> {
    match kind {
        ModLoaderKind::Fabric => fabric::install(client, config, mc_version, loader_version, on_progress).await,
        ModLoaderKind::Quilt => quilt::install(client, config, mc_version, loader_version, on_progress).await,
        ModLoaderKind::Forge => forge::install(client, config, mc_version, loader_version, on_progress).await,
        ModLoaderKind::NeoForge => neoforge::install(client, config, mc_version, loader_version, on_progress).await,
    }
}

/// Writes `data` to the exact cache path `load_or_fetch_version_data` reads from -- so a later
/// `install_version(client, config, &data.id, ..)` call sees it as already-cached instead of
/// trying (and failing) to look `data.id` up in Mojang's own manifest.
pub(super) async fn cache_version_data(config: &LauncherConfig, data: &VersionData) -> Result<()> {
    let cache_path = config.version_dir(&data.id).join(format!("{}.json", data.id));
    if let Some(parent) = cache_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_err(parent))?;
    }
    tokio::fs::write(&cache_path, serde_json::to_vec_pretty(data)?)
        .await
        .map_err(io_err(&cache_path))?;
    Ok(())
}

/// Percent-encodes a path segment (Fabric/Quilt version strings can contain spaces on ancient
/// pre-release Minecraft versions, e.g. `"1.14 Pre-Release 5"`).
pub(super) fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Extracts the text content of every `<tag>...</tag>` occurrence in `xml` -- used for Forge's
/// `maven-metadata.xml`, which is flat enough (a `<versions><version>x</version>...</versions>`
/// list, no nesting of the same tag name) that a real XML parser would be pure overhead.
pub(super) fn extract_xml_tag_values(xml: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        let Some(end) = rest.find(&close) else { break };
        out.push(rest[..end].to_string());
        rest = &rest[end + close.len()..];
    }
    out
}

/// Splits a dot-separated version string into numeric parts for a proper newest-first sort
/// (`"54.1.9" < "54.1.18"`, unlike a plain string sort). Non-numeric segments (a trailing
/// `-beta`, say) sort as `0` -- fine here since it's only ever used to order builds that share a
/// numbering scheme, not to compare across schemes.
pub(super) fn version_sort_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap_or(0))
        .collect()
}
