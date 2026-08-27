use serde::{Deserialize, Serialize};

use crate::error::Result;

const VERSION_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    pub sha1: String,
    #[serde(rename = "complianceLevel")]
    pub compliance_level: u32,
}

pub async fn fetch_version_manifest(client: &reqwest::Client) -> Result<VersionManifest> {
    let manifest = client
        .get(VERSION_MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<VersionManifest>()
        .await?;
    Ok(manifest)
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&VersionEntry> {
        self.versions.iter().find(|v| v.id == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionData {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    pub assets: String,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,
    pub downloads: VersionDownloads,
    pub libraries: Vec<Library>,
    #[serde(rename = "javaVersion", default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(rename = "minecraftArguments", default)]
    pub minecraft_arguments: Option<String>,
}

pub async fn fetch_version_data(client: &reqwest::Client, url: &str) -> Result<VersionData> {
    let data = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<VersionData>()
        .await?;
    Ok(data)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(rename = "totalSize", default)]
    pub total_size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDownloads {
    pub client: DownloadArtifact,
    #[serde(default)]
    pub server: Option<DownloadArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadArtifact {
    #[serde(default)]
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub natives: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Older manifests (pre-1.13) that point at a non-Mojang Maven repo instead of `downloads`.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<DownloadArtifact>,
    #[serde(default)]
    pub classifiers: Option<std::collections::HashMap<String, DownloadArtifact>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<std::collections::HashMap<String, bool>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgumentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentEntry {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Single(String),
    Multiple(Vec<String>),
}

impl ArgumentValue {
    pub fn as_vec(&self) -> Vec<String> {
        match self {
            ArgumentValue::Single(s) => vec![s.clone()],
            ArgumentValue::Multiple(v) => v.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: std::collections::HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// What every mod loader's own JSON provides -- Fabric/Quilt's `.../profile/json` response, or
/// the `version.json` extracted from a Forge/NeoForge installer jar after its processors have
/// run. In every case it's a *partial* version JSON meant to sit on top of the vanilla version
/// named in `inherits_from`: no `downloads`/`assetIndex`/`javaVersion` of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderProfile {
    pub id: String,
    #[serde(rename = "inheritsFrom")]
    pub inherits_from: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(rename = "minecraftArguments", default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
}

/// Merges a loader's partial overlay onto the vanilla `VersionData` it targets, producing a
/// normal, fully-populated `VersionData` -- identical in shape to a plain vanilla one, so nothing
/// downstream (`install_version`, `launch`) needs to know a merge happened. Used identically by
/// all four loaders (Fabric/Quilt profile JSON, Forge/NeoForge's post-processor `version.json`).
///
/// - `id`/`main_class`/`libraries` come from the overlay (libraries are vanilla's ++ overlay's,
///   in that order -- matches the classpath ordering the official installers themselves produce).
/// - `assets`/`asset_index`/`downloads`/`java_version` come from vanilla only -- the overlay
///   never carries these.
/// - `arguments` concatenate vanilla's game/jvm lists with the overlay's own (appended), falling
///   back to synthesizing a minimal `Arguments` from vanilla's legacy `minecraft_arguments` on
///   versions old enough to still use that flat-string form.
pub fn merge_with_vanilla(vanilla: &VersionData, overlay: LoaderProfile) -> VersionData {
    let mut libraries = vanilla.libraries.clone();
    libraries.extend(overlay.libraries);

    let base_arguments = vanilla.arguments.clone().unwrap_or_else(|| Arguments {
        game: vanilla
            .minecraft_arguments
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .map(|s| ArgumentEntry::Plain(s.to_string()))
            .collect(),
        jvm: Vec::new(),
    });
    let overlay_arguments = overlay.arguments.unwrap_or(Arguments { game: Vec::new(), jvm: Vec::new() });
    let arguments = Arguments {
        game: base_arguments.game.into_iter().chain(overlay_arguments.game).collect(),
        jvm: base_arguments.jvm.into_iter().chain(overlay_arguments.jvm).collect(),
    };

    VersionData {
        id: overlay.id,
        version_type: vanilla.version_type.clone(),
        main_class: overlay.main_class,
        assets: vanilla.assets.clone(),
        asset_index: vanilla.asset_index.clone(),
        downloads: vanilla.downloads.clone(),
        libraries,
        java_version: vanilla.java_version.clone(),
        arguments: Some(arguments),
        minecraft_arguments: None,
    }
}

pub async fn fetch_asset_index(
    client: &reqwest::Client,
    asset_index_ref: &AssetIndexRef,
) -> Result<AssetIndex> {
    let index = client
        .get(&asset_index_ref.url)
        .send()
        .await?
        .error_for_status()?
        .json::<AssetIndex>()
        .await?;
    Ok(index)
}
