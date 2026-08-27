//! Forge (and, via `install_from_installer`, NeoForge -- a literal fork of Forge's installer
//! code, same mechanism, different Maven coordinates) install a "installer" jar containing:
//!
//! - `version.json`: the same partial/`inheritsFrom` overlay shape Fabric/Quilt's API returns
//!   directly -- parsed as [`crate::manifest::LoaderProfile`] and merged the same way.
//! - `install_profile.json`: a `libraries` array (the installer's own dependencies: processor
//!   tool jars and Forge's client/universal jars) plus a `processors` array and a `data` map.
//!
//! Each processor is a small external tool that must actually be *run* -- `java -cp <classpath>
//! <MainClass from its own jar manifest> <args>` -- in listed order, to binary-patch the vanilla
//! client jar into the real Forge/NeoForge client (extract mappings, rename to official names,
//! apply a binary diff). This isn't simulable; it's exactly what the official installer GUI does,
//! and reimplementing the processor loop generically (no Forge-specific special-casing beyond
//! this loop) is the standard approach other third-party launchers use.
//!
//! `data` map entries are per-side (`{"client": "...", "server": "..."}` -- only `.client` is
//! read here) and each resolves one of three ways: `[group:artifact:version]` is a Maven
//! coordinate resolved to a local path under `libraries_dir` (via the same layout
//! `libraries::resolve` uses); a bare string is a path *inside the installer jar*, extracted to a
//! scratch directory; a `'quoted'` string is a literal value with the quotes stripped.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use tokio::process::Command;

use crate::config::LauncherConfig;
use crate::downloader::{ensure_file, ensure_files, DownloadTask};
use crate::error::{io_err, CoreError, Result};
use crate::libraries::{self, maven_path};
use crate::manifest::{merge_with_vanilla, LoaderProfile, VersionData};
use crate::rules::FeatureFlags;

use super::{cache_version_data, extract_xml_tag_values, version_sort_key, LoaderVersionInfo, ProgressCallback};

#[derive(Debug, Deserialize)]
struct InstallProfile {
    #[serde(default)]
    libraries: Vec<crate::manifest::Library>,
    #[serde(default)]
    processors: Vec<ProcessorSpec>,
    #[serde(default)]
    data: HashMap<String, DataSides>,
}

#[derive(Debug, Deserialize)]
struct DataSides {
    client: String,
    #[allow(dead_code)] // only client installs are supported today; kept for schema fidelity
    server: String,
}

#[derive(Debug, Deserialize)]
struct ProcessorSpec {
    #[serde(default)]
    sides: Option<Vec<String>>,
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
}

pub async fn list_versions(client: &reqwest::Client, mc_version: &str) -> Result<Vec<LoaderVersionInfo>> {
    #[derive(Deserialize)]
    struct Promotions {
        promos: HashMap<String, String>,
    }
    let promos: Promotions = client
        .get("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let recommended = promos.promos.get(&format!("{mc_version}-recommended")).cloned();

    let xml = client
        .get("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let prefix = format!("{mc_version}-");
    let mut versions: Vec<String> = extract_xml_tag_values(&xml, "version")
        .into_iter()
        .filter_map(|v| v.strip_prefix(&prefix).map(|s| s.to_string()))
        .collect();
    versions.sort_by_key(|v| std::cmp::Reverse(version_sort_key(v)));
    versions.dedup();

    Ok(versions
        .into_iter()
        .map(|v| {
            let recommended = recommended.as_deref() == Some(v.as_str());
            LoaderVersionInfo { version: v, stable: true, recommended }
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
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc_version}-{loader_version}/forge-{mc_version}-{loader_version}-installer.jar"
    );
    let installer_file_name = format!("forge-{mc_version}-{loader_version}-installer.jar");
    install_from_installer(client, config, "forge", mc_version, loader_version, installer_url, installer_file_name, on_progress).await
}

/// Shared by `forge::install` and `neoforge::install`. `flavor`/`mc_version`/`loader_version`
/// are only used to name this install's own scratch directory distinctly from any other
/// in-progress or previous loader install.
pub(super) async fn install_from_installer(
    client: &reqwest::Client,
    config: &LauncherConfig,
    flavor: &str,
    mc_version: &str,
    loader_version: &str,
    installer_url: String,
    installer_file_name: String,
    on_progress: Option<ProgressCallback>,
) -> Result<VersionData> {
    let vanilla = crate::launcher::install_version(client, config, mc_version, on_progress.clone()).await?;

    let installer_path = config.loader_installers_dir().join(&installer_file_name);
    ensure_file(
        client,
        &DownloadTask { url: installer_url, dest: installer_path.clone(), sha1: None, size: None },
    )
    .await?;

    let file = std::fs::File::open(&installer_path).map_err(io_err(&installer_path))?;
    let mut archive = zip::ZipArchive::new(file)?;

    let install_profile: InstallProfile = read_json_entry(&mut archive, "install_profile.json")?;
    let loader_profile: LoaderProfile = read_json_entry(&mut archive, "version.json")?;

    eprintln!(
        "[beacon] {flavor} installer: {} processors, {} installer libraries",
        install_profile.processors.len(),
        install_profile.libraries.len(),
    );

    // The installer's own dependencies: processor tool jars + their classpath deps + Forge's own
    // client/universal jars. Standard `downloads.artifact` shape -- the generic resolver/
    // downloader handle it with no changes.
    let resolved = libraries::resolve(&install_profile.libraries, &config.libraries_dir(), &FeatureFlags);
    ensure_files(client, resolved.download_tasks, on_progress.clone()).await?;

    let scratch_dir = config.loader_installers_dir().join(format!("{flavor}-{mc_version}-{loader_version}"));
    let extracted_dir = scratch_dir.join("extracted");
    std::fs::create_dir_all(&extracted_dir).map_err(io_err(&extracted_dir))?;

    let vanilla_client_jar = config.version_dir(mc_version).join(format!("{mc_version}.jar"));
    let mut values: HashMap<String, String> = HashMap::new();
    for (key, sides) in &install_profile.data {
        let value = resolve_data_value(&sides.client, config, &mut archive, &extracted_dir)?;
        values.insert(key.clone(), value);
    }
    values.insert("SIDE".to_string(), "client".to_string());
    values.insert("MINECRAFT_VERSION".to_string(), mc_version.to_string());
    values.entry("MINECRAFT_JAR".to_string()).or_insert_with(|| path_string(&vanilla_client_jar));
    values.entry("ROOT".to_string()).or_insert_with(|| path_string(&scratch_dir));
    values.entry("INSTALLER".to_string()).or_insert_with(|| path_string(&installer_path));
    values.entry("LIBRARY_DIR".to_string()).or_insert_with(|| path_string(&config.libraries_dir()));

    for processor in &install_profile.processors {
        if let Some(sides) = &processor.sides {
            if !sides.iter().any(|s| s == "client") {
                continue;
            }
        }
        run_processor(config, processor, &values).await?;
    }

    let merged = merge_with_vanilla(&vanilla, loader_profile);
    cache_version_data(config, &merged).await?;
    // Re-run install_version on the merged id: downloads any of `version.json`'s own libraries
    // that weren't already covered by the installer's library list (e.g. Forge's bootstrap
    // launcher libraries) -- the patched client jar a processor just wrote above is already
    // valid on disk under its expected library path, so this verifies and skips it rather than
    // trying (and failing) to download it, since modern Forge/NeoForge client library entries
    // carry no real download URL for that artifact.
    crate::launcher::install_version(client, config, &merged.id, on_progress).await?;
    Ok(merged)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn resolve_data_value(
    raw: &str,
    config: &LauncherConfig,
    archive: &mut zip::ZipArchive<std::fs::File>,
    extracted_dir: &Path,
) -> Result<String> {
    if let Some(coord) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let rel_path = maven_path(coord, None);
        Ok(path_string(&config.libraries_dir().join(rel_path)))
    } else if let Some(literal) = raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        Ok(literal.to_string())
    } else {
        let entry_name = raw.trim_start_matches('/');
        let out_path = extracted_dir.join(entry_name);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut entry = archive
            .by_name(entry_name)
            .map_err(|_| CoreError::LoaderInstall(format!("installer jar has no entry '{entry_name}'")))?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(io_err(&out_path))?;
        std::fs::write(&out_path, &bytes).map_err(io_err(&out_path))?;
        Ok(path_string(&out_path))
    }
}

fn read_json_entry<T: for<'de> Deserialize<'de>>(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<T> {
    let mut entry = archive
        .by_name(name)
        .map_err(|_| CoreError::LoaderInstall(format!("installer jar is missing '{name}'")))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|e| CoreError::LoaderInstall(format!("reading '{name}' from installer jar: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| CoreError::LoaderInstall(format!("parsing installer's '{name}': {e}")))
}

/// Reads the `Main-Class:` attribute out of a jar's own `META-INF/MANIFEST.MF` -- every processor
/// tool jar declares one, so this is how its entry point is found without hardcoding a single
/// tool's class name anywhere in here.
fn read_main_class(jar_path: &Path) -> Result<String> {
    let file = std::fs::File::open(jar_path).map_err(io_err(jar_path))?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut manifest = archive
        .by_name("META-INF/MANIFEST.MF")
        .map_err(|_| CoreError::LoaderInstall(format!("{} has no manifest", jar_path.display())))?;
    let mut text = String::new();
    manifest.read_to_string(&mut text).map_err(io_err(jar_path))?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Main-Class:") {
            return Ok(rest.trim().to_string());
        }
    }
    Err(CoreError::LoaderInstall(format!("{} has no Main-Class in its manifest", jar_path.display())))
}

/// Some processor `args` embed a `[group:artifact:version]` Maven reference directly, not routed
/// through the `data` map at all (confirmed against a real NeoForge install_profile.json --
/// its `MCP_DATA` processor's `--input [net.neoforged:neoform:...@zip]` arg is exactly this).
/// Resolves every such reference in `arg` to its local `libraries_dir` path, same as a `data`
/// entry's `[...]` form.
fn resolve_inline_maven_refs(arg: &str, libraries_dir: &Path) -> String {
    let mut result = String::new();
    let mut rest = arg;
    loop {
        let Some(start) = rest.find('[') else {
            result.push_str(rest);
            break;
        };
        let Some(end) = rest[start..].find(']') else {
            result.push_str(rest);
            break;
        };
        let end = start + end;
        result.push_str(&rest[..start]);
        let coord = &rest[start + 1..end];
        result.push_str(&path_string(&libraries_dir.join(maven_path(coord, None))));
        rest = &rest[end + 1..];
    }
    result
}

fn substitute_args(args: &[String], values: &HashMap<String, String>, libraries_dir: &Path) -> Vec<String> {
    args.iter()
        .map(|arg| {
            let mut out = arg.clone();
            for (key, value) in values {
                out = out.replace(&format!("{{{key}}}"), value);
            }
            resolve_inline_maven_refs(&out, libraries_dir)
        })
        .collect()
}

async fn run_processor(config: &LauncherConfig, processor: &ProcessorSpec, values: &HashMap<String, String>) -> Result<()> {
    let libraries_dir = config.libraries_dir();
    let jar_path = libraries_dir.join(maven_path(&processor.jar, None));
    let main_class = read_main_class(&jar_path)?;

    let separator = if cfg!(target_os = "windows") { ";" } else { ":" };
    let mut classpath_parts: Vec<String> = processor
        .classpath
        .iter()
        .map(|coord| path_string(&libraries_dir.join(maven_path(coord, None))))
        .collect();
    classpath_parts.push(path_string(&jar_path));
    let classpath = classpath_parts.join(separator);

    let args = substitute_args(&processor.args, values, &libraries_dir);
    eprintln!("[beacon] running loader processor: {main_class} {args:?}");

    let output = Command::new(&config.java_path)
        .arg("-cp")
        .arg(&classpath)
        .arg(&main_class)
        .args(&args)
        .output()
        .await
        .map_err(CoreError::Launch)?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("[beacon] processor {main_class} failed:\nstdout:\n{stdout}\nstderr:\n{stderr}");
        return Err(CoreError::LoaderInstall(format!(
            "{main_class} exited with {:?}: {}",
            output.status.code(),
            stderr.lines().last().unwrap_or("(no output)"),
        )));
    }
    Ok(())
}
