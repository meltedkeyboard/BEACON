use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::config::LauncherConfig;
use crate::error::{io_err, CoreError, Result};
use crate::modsource::{ModProvenanceEntry, ModSource};

/// A self-contained "instance" of a Minecraft version to play, in the same sense Prism Launcher
/// uses the word: its own `saves`/`resourcepacks`/`shaderpacks`/`config` (and, once Beacon
/// supports mod loaders, `mods`), separate from every other instance. The shared, version-keyed
/// download cache (`LauncherConfig::versions_dir`/`libraries_dir`/`assets_dir`) is NOT part of an
/// instance -- it's downloaded once per Minecraft version and reused by every instance that
/// targets that version, exactly like Prism shares its own download cache.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Instance {
    /// Directory name under `instances_dir()`. Doubles as this instance's stable identity.
    /// Derived from `name` (sanitized, de-duplicated against sibling directories) when the
    /// instance is created, and re-derived (with the directory renamed to match) whenever the
    /// instance is renamed -- see [`rename_instance`].
    pub id: String,
    pub name: String,
    pub version_id: String,
    #[serde(default)]
    pub icon_path: Option<PathBuf>,
    /// File name (not a path -- always inside this instance's own `screenshots_dir()`) of the
    /// screenshot pinned as the Play tab's backdrop for this instance, if any. `None` means the
    /// Play tab rotates through every screenshot instead of locking onto one.
    #[serde(default)]
    pub pinned_screenshot: Option<String>,
    /// The mod loader installed on top of `version_id`, if any. Tied to that specific vanilla
    /// version -- changing `version_id` clears this (see `set_instance_version_cmd`), since a
    /// loader build only ever targets one Minecraft version.
    #[serde(default)]
    pub mod_loader: Option<ModLoaderInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ModLoaderKind {
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

impl ModLoaderKind {
    pub fn label(self) -> &'static str {
        match self {
            ModLoaderKind::Fabric => "Fabric",
            ModLoaderKind::Forge => "Forge",
            ModLoaderKind::NeoForge => "NeoForge",
            ModLoaderKind::Quilt => "Quilt",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModLoaderInfo {
    pub kind: ModLoaderKind,
    /// The loader's own version string (e.g. `"0.15.11"`, `"54.1.18"`) -- for display only.
    pub loader_version: String,
    /// The id of the merged (vanilla + loader) version JSON cached under `versions_dir()` --
    /// what `install_version`/`launch` actually use to run this instance. Not user-facing.
    pub effective_version_id: String,
}

impl Instance {
    pub fn dir(&self, config: &LauncherConfig) -> PathBuf {
        config.instances_dir().join(&self.id)
    }

    pub fn saves_dir(&self, config: &LauncherConfig) -> PathBuf {
        self.dir(config).join("saves")
    }

    pub fn resource_packs_dir(&self, config: &LauncherConfig) -> PathBuf {
        self.dir(config).join("resourcepacks")
    }

    pub fn shader_packs_dir(&self, config: &LauncherConfig) -> PathBuf {
        self.dir(config).join("shaderpacks")
    }

    pub fn mods_dir(&self, config: &LauncherConfig) -> PathBuf {
        self.dir(config).join("mods")
    }

    /// Vanilla's own F2 screenshot folder -- Beacon doesn't create anything new here, just lists
    /// and manages what the game already writes.
    pub fn screenshots_dir(&self, config: &LauncherConfig) -> PathBuf {
        self.dir(config).join("screenshots")
    }
}

/// Replaces characters that are invalid (or awkward -- trailing dots/spaces trip up Windows) in
/// a filesystem entry name, and caps the length so deeply nested paths don't hit Windows' old
/// `MAX_PATH` limit. Doesn't try to be clever about it: this only needs to produce *a* valid,
/// recognizable directory name, not preserve every character of the input.
fn sanitize_dir_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).trim();
    if cleaned.is_empty() {
        "Instance".to_string()
    } else {
        cleaned.chars().take(64).collect()
    }
}

/// Appends " (2)", " (3)", ... to `base` until `taken` no longer reports a collision -- the same
/// scheme Windows Explorer uses for duplicate file/folder names.
fn unique_dir_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base} ({n})");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

fn other_instance_ids<'a>(config: &'a LauncherConfig, except: Option<&str>) -> HashSet<&'a str> {
    config
        .instances
        .iter()
        .map(|i| i.id.as_str())
        .filter(|id| except != Some(*id))
        .collect()
}

/// Creates a new instance: picks a filesystem-safe, unique directory name derived from `name`
/// and creates that directory under `instances_dir()`. Doesn't touch `config.instances` -- like
/// [`crate::account::offline_account`], the caller is responsible for saving the returned value.
pub fn create_instance(
    config: &LauncherConfig,
    name: impl Into<String>,
    version_id: impl Into<String>,
) -> Result<Instance> {
    let name = name.into();
    let base = sanitize_dir_name(&name);
    let taken = other_instance_ids(config, None);
    let id = unique_dir_name(&base, |candidate| taken.contains(candidate));

    let dir = config.instances_dir().join(&id);
    std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;

    Ok(Instance {
        id,
        name,
        version_id: version_id.into(),
        icon_path: None,
        pinned_screenshot: None,
        mod_loader: None,
    })
}

/// Renames an instance. If the new name maps to a different directory-safe id than the current
/// one, the instance's directory is renamed on disk to match, so it stays easy to recognize in a
/// file browser -- same idea as [`crate::account::Account::id`] changing when an offline
/// account's nickname changes. If a rename target is already taken by a *different* instance, a
/// " (2)"-style suffix is appended rather than colliding with it.
pub fn rename_instance(config: &LauncherConfig, instance: &Instance, new_name: impl Into<String>) -> Result<Instance> {
    let new_name = new_name.into();
    let base = sanitize_dir_name(&new_name);
    if base == instance.id {
        return Ok(Instance { name: new_name, ..instance.clone() });
    }

    let taken = other_instance_ids(config, Some(&instance.id));
    let new_id = unique_dir_name(&base, |candidate| taken.contains(candidate));

    let old_dir = config.instances_dir().join(&instance.id);
    let new_dir = config.instances_dir().join(&new_id);
    if old_dir.exists() {
        std::fs::rename(&old_dir, &new_dir).map_err(io_err(&new_dir))?;
    }

    Ok(Instance { id: new_id, name: new_name, ..instance.clone() })
}

/// Deletes an instance's entire directory (saves, resource packs, everything it owns). Doesn't
/// touch `config.instances` -- the caller still needs to remove the entry and save.
pub fn delete_instance_dir(config: &LauncherConfig, instance: &Instance) -> Result<()> {
    let dir = instance.dir(config);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(io_err(&dir))?;
    }
    Ok(())
}

/// Rejects anything that isn't a single, plain path component: no separators, no `.`/`..`. Every
/// world/resource-pack/shader-pack/datapack name that reaches these functions either came from
/// [`list_worlds`]/[`list_resource_packs`]/[`list_shader_packs`] (so it's already a bare
/// directory-entry name) or crossed the Tauri IPC boundary from the frontend -- this keeps a
/// bogus or malicious name from being used to delete something outside the directory it's
/// supposed to be scoped to.
fn safe_child(dir: &Path, name: &str) -> Result<PathBuf> {
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\');
    if !valid {
        return Err(CoreError::Other(format!("'{name}' is not a valid file name")));
    }
    Ok(dir.join(name))
}

/// Removes a file or directory (whichever it is) that must be a direct child of `dir` -- see
/// [`safe_child`]. A no-op if it's already gone.
fn remove_entry(dir: &Path, name: &str) -> Result<()> {
    let path = safe_child(dir, name)?;
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path).map_err(io_err(&path)),
        Ok(_) => std::fs::remove_file(&path).map_err(io_err(&path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(&path)(e)),
    }
}

fn list_dir_entries(dir: &Path) -> Result<Vec<String>> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_err(dir)(e)),
    };
    let mut names = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(io_err(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldInfo {
    pub name: String,
    pub datapacks: Vec<String>,
    /// Vanilla writes `icon.png` at the save's root the first time it's loaded into -- `None` for
    /// a world that's never been opened yet, not an error.
    pub icon_data_url: Option<String>,
}

/// Reads a plain (non-zipped) `icon.png` at `dir`'s root, if present -- used for both world saves
/// and unpacked resource pack folders, the two places this convention shows up as a loose file
/// rather than inside a zip.
fn read_loose_icon(dir: &Path, file_name: &str) -> Option<String> {
    let bytes = std::fs::read(dir.join(file_name)).ok()?;
    Some(crate::mod_metadata::encode_icon_data_url(&bytes, file_name))
}

/// Lists this instance's saved worlds (each a subdirectory of `saves/`), and, per world, the
/// datapacks in its `datapacks/` folder -- datapacks are per-world in vanilla Minecraft, not
/// instance-wide, so they're nested here rather than listed separately.
pub fn list_worlds(config: &LauncherConfig, instance: &Instance) -> Result<Vec<WorldInfo>> {
    let saves_dir = instance.saves_dir(config);
    let mut worlds = Vec::new();
    for name in list_dir_entries(&saves_dir)? {
        let world_dir = saves_dir.join(&name);
        if !world_dir.is_dir() {
            continue;
        }
        let datapacks = list_dir_entries(&world_dir.join("datapacks"))?;
        let icon_data_url = read_loose_icon(&world_dir, "icon.png");
        worlds.push(WorldInfo { name, datapacks, icon_data_url });
    }
    Ok(worlds)
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourcePackInfo {
    pub name: String,
    /// The pack's own `pack.png`, if it has one -- read from inside the zip for a `.zip` pack, or
    /// as a loose file for an unpacked folder. `None` isn't an error; plenty of packs skip it.
    pub icon_data_url: Option<String>,
}

/// A resource pack is either a `.zip` (read `pack.png` from inside it) or an unpacked folder (read
/// `pack.png` as a loose file) -- both are valid ways Minecraft itself accepts a pack.
fn read_pack_icon(path: &Path) -> Option<String> {
    if path.is_dir() {
        return read_loose_icon(path, "pack.png");
    }
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("pack.png").ok()?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes).ok()?;
    Some(crate::mod_metadata::encode_icon_data_url(&bytes, "pack.png"))
}

pub fn list_resource_packs(config: &LauncherConfig, instance: &Instance) -> Result<Vec<ResourcePackInfo>> {
    let dir = instance.resource_packs_dir(config);
    Ok(list_dir_entries(&dir)?
        .into_iter()
        .map(|name| {
            let icon_data_url = read_pack_icon(&dir.join(&name));
            ResourcePackInfo { name, icon_data_url }
        })
        .collect())
}

pub fn list_shader_packs(config: &LauncherConfig, instance: &Instance) -> Result<Vec<String>> {
    list_dir_entries(&instance.shader_packs_dir(config))
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotInfo {
    pub name: String,
    /// Resolved absolute path -- `Instance` itself never stores one (same reasoning as
    /// `beacon-tauri`'s `InstanceView` not storing `dir`: a stale path shouldn't survive
    /// `game_dir` moving), so the frontend needs it here to load the image via `convertFileSrc()`.
    pub path: PathBuf,
}

/// Lists this instance's screenshots (vanilla only ever writes `.png` there via F2).
pub fn list_screenshots(config: &LauncherConfig, instance: &Instance) -> Result<Vec<ScreenshotInfo>> {
    let screenshots_dir = instance.screenshots_dir(config);
    Ok(list_dir_entries(&screenshots_dir)?
        .into_iter()
        .filter(|name| name.to_ascii_lowercase().ends_with(".png"))
        .map(|name| {
            let path = screenshots_dir.join(&name);
            ScreenshotInfo { name, path }
        })
        .collect())
}

pub fn delete_screenshot(config: &LauncherConfig, instance: &Instance, name: &str) -> Result<()> {
    remove_entry(&instance.screenshots_dir(config), name)
}

/// Suffix used to disable a mod without deleting it -- the same convention Prism/MultiMC and the
/// vanilla launcher's own mod ecosystem already use, so a mod disabled in Beacon stays disabled if
/// the instance is later opened elsewhere, and vice versa.
const DISABLED_SUFFIX: &str = ".disabled";

#[derive(Debug, Clone, Serialize)]
pub struct ModInfo {
    /// The mod's real file name, e.g. `"sodium.jar"` -- with any `.disabled` suffix already
    /// stripped, so the frontend doesn't need to know about the convention to display it.
    pub name: String,
    pub enabled: bool,
    /// Read from the jar's own Fabric/Quilt/Forge/NeoForge manifest -- `None` for a mod loader
    /// Beacon doesn't recognize the metadata shape of, not an error.
    pub version: Option<String>,
    pub icon_data_url: Option<String>,
}

/// Lists this instance's mods (each a `.jar` or disabled `.jar.disabled` file directly in
/// `mods/`), reading each one's own version/icon out of its Fabric/Quilt/Forge/NeoForge manifest.
pub fn list_mods(config: &LauncherConfig, instance: &Instance) -> Result<Vec<ModInfo>> {
    let mods_dir = instance.mods_dir(config);
    let mut mods = Vec::new();
    for entry in list_dir_entries(&mods_dir)? {
        let (name, enabled, jar_file_name) = if let Some(name) = entry.strip_suffix(DISABLED_SUFFIX) {
            if !name.ends_with(".jar") {
                continue;
            }
            (name.to_string(), false, entry.clone())
        } else if entry.ends_with(".jar") {
            (entry.clone(), true, entry.clone())
        } else {
            continue;
        };
        let metadata = crate::mod_metadata::read_mod_metadata(&mods_dir.join(&jar_file_name));
        mods.push(ModInfo { name, enabled, version: metadata.version, icon_data_url: metadata.icon_data_url });
    }
    Ok(mods)
}

/// Enables or disables a mod by renaming it to/from its `.disabled` form. `name` is always the
/// real (non-disabled) file name, as returned by [`list_mods`] -- this figures out which of the
/// two on-disk forms currently exists.
pub fn toggle_mod(config: &LauncherConfig, instance: &Instance, name: &str, enable: bool) -> Result<()> {
    let mods_dir = instance.mods_dir(config);
    let enabled_path = safe_child(&mods_dir, name)?;
    let disabled_name = format!("{name}{DISABLED_SUFFIX}");
    let disabled_path = safe_child(&mods_dir, &disabled_name)?;

    let (from, to) = if enable { (&disabled_path, &enabled_path) } else { (&enabled_path, &disabled_path) };
    if from.exists() {
        std::fs::rename(from, to).map_err(io_err(to))?;
    }
    Ok(())
}

/// Deletes a mod, whichever of its enabled/disabled on-disk forms currently exists.
pub fn delete_mod(config: &LauncherConfig, instance: &Instance, name: &str) -> Result<()> {
    let mods_dir = instance.mods_dir(config);
    remove_entry(&mods_dir, name)?;
    remove_entry(&mods_dir, &format!("{name}{DISABLED_SUFFIX}"))
}

/// Copies picked mod jars into `mods/` (created if missing), keeping each source file's own name.
/// Like [`crate::instance::Instance`]'s icon path, these come from an open-file dialog with no
/// restriction on where they live, so this only needs to read them, not validate them.
pub fn add_mods(config: &LauncherConfig, instance: &Instance, source_paths: &[PathBuf]) -> Result<()> {
    let mods_dir = instance.mods_dir(config);
    std::fs::create_dir_all(&mods_dir).map_err(io_err(&mods_dir))?;
    for source in source_paths {
        let Some(file_name) = source.file_name() else { continue };
        let dest = mods_dir.join(file_name);
        std::fs::copy(source, &dest).map_err(io_err(&dest))?;
    }
    Ok(())
}

/// Sidecar file recording which mod-browser install a jar came from -- deliberately not a
/// `config.json`/`Instance` field, since it's a growing collection tied 1:1 to files in `mods/`,
/// same spirit as the `.disabled` suffix convention already used there. Hidden (dot-prefixed), so
/// `list_dir_entries`'s existing `!name.starts_with('.')` filter already keeps it out of
/// `list_mods`/`list_dir_entries` callers without any extra filtering here.
const MOD_SOURCES_FILE: &str = ".beacon-mod-sources.json";

fn read_mod_provenance_file(mods_dir: &Path) -> HashMap<String, ModProvenanceEntry> {
    std::fs::read(mods_dir.join(MOD_SOURCES_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Records that `filename` (already installed in `mods/`) came from `source`'s `project_id` --
/// called once per root mod right after the mod-browser's install succeeds. Dependencies pulled in
/// alongside it are deliberately left untracked (see module-level plan notes): removing a mod only
/// ever removes its own file, never a shared dependency another mod might also need.
pub fn record_mod_provenance(config: &LauncherConfig, instance: &Instance, filename: &str, source: ModSource, project_id: &str) -> Result<()> {
    let mods_dir = instance.mods_dir(config);
    std::fs::create_dir_all(&mods_dir).map_err(io_err(&mods_dir))?;
    let mut map = read_mod_provenance_file(&mods_dir);
    map.insert(filename.to_string(), ModProvenanceEntry { source, project_id: project_id.to_string() });
    let path = mods_dir.join(MOD_SOURCES_FILE);
    std::fs::write(&path, serde_json::to_vec_pretty(&map)?).map_err(io_err(&path))?;
    Ok(())
}

/// Reads back what `record_mod_provenance` wrote, filtered to files that still actually exist --
/// self-healing against a jar deleted some other way (the ordinary Mods list's own Remove button,
/// or by hand) without needing to hook every deletion path to also prune this file.
pub fn load_mod_provenance(config: &LauncherConfig, instance: &Instance) -> HashMap<String, ModProvenanceEntry> {
    let mods_dir = instance.mods_dir(config);
    read_mod_provenance_file(&mods_dir).into_iter().filter(|(filename, _)| mods_dir.join(filename).is_file()).collect()
}

pub fn delete_world(config: &LauncherConfig, instance: &Instance, world_name: &str) -> Result<()> {
    remove_entry(&instance.saves_dir(config), world_name)
}

pub fn delete_datapack(config: &LauncherConfig, instance: &Instance, world_name: &str, datapack_name: &str) -> Result<()> {
    let world_dir = safe_child(&instance.saves_dir(config), world_name)?;
    remove_entry(&world_dir.join("datapacks"), datapack_name)
}

pub fn delete_resource_pack(config: &LauncherConfig, instance: &Instance, file_name: &str) -> Result<()> {
    remove_entry(&instance.resource_packs_dir(config), file_name)
}

pub fn delete_shader_pack(config: &LauncherConfig, instance: &Instance, file_name: &str) -> Result<()> {
    remove_entry(&instance.shader_packs_dir(config), file_name)
}

/// What identifies an exported instance archive as one of ours, and lets [`import_instance`]
/// recreate a proper `Instance` (name, target version) instead of just unpacking files with no
/// idea what they were.
#[derive(Debug, Serialize, Deserialize)]
struct ExportManifest {
    name: String,
    version_id: String,
    #[serde(default)]
    mod_loader: Option<ModLoaderInfo>,
}

const MANIFEST_ENTRY_NAME: &str = "beacon-instance.json";

/// Zips this instance's entire directory (saves, resource packs, shader packs, config, screenshots
/// -- everything under it) to `dest_zip`, plus a small `beacon-instance.json` manifest at the
/// archive root recording its name and target version, so [`import_instance`] can recreate the
/// instance itself rather than just its files.
pub fn export_instance(config: &LauncherConfig, instance: &Instance, dest_zip: &Path) -> Result<()> {
    let source_dir = instance.dir(config);
    let file = std::fs::File::create(dest_zip).map_err(io_err(dest_zip))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let manifest = ExportManifest {
        name: instance.name.clone(),
        version_id: instance.version_id.clone(),
        mod_loader: instance.mod_loader.clone(),
    };
    zip.start_file(MANIFEST_ENTRY_NAME, options)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?).map_err(io_err(dest_zip))?;

    if source_dir.is_dir() {
        add_dir_to_zip(&mut zip, &source_dir, &source_dir, options)?;
    }

    zip.finish()?;
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    dir: &Path,
    options: SimpleFileOptions,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        let relative = path.strip_prefix(root).expect("walked path is under root").to_string_lossy().replace('\\', "/");

        if path.is_dir() {
            zip.add_directory(format!("{relative}/"), options)?;
            add_dir_to_zip(zip, root, &path, options)?;
        } else {
            zip.start_file(relative, options)?;
            let bytes = std::fs::read(&path).map_err(io_err(&path))?;
            zip.write_all(&bytes).map_err(io_err(&path))?;
        }
    }
    Ok(())
}

/// Unpacks an instance archive created by [`export_instance`] into a new instance directory
/// (name/id picked the same way [`create_instance`] does) and returns the resulting `Instance`.
/// Falls back to the zip's file stem for the name if it has no `beacon-instance.json` manifest
/// (still imports the files -- just without knowing the original target version, defaulted to
/// an empty string the caller/UI should prompt to fill in).
pub fn import_instance(config: &LauncherConfig, source_zip: &Path) -> Result<Instance> {
    let file = std::fs::File::open(source_zip).map_err(io_err(source_zip))?;
    let mut archive = zip::ZipArchive::new(file)?;

    let manifest: Option<ExportManifest> = match archive.by_name(MANIFEST_ENTRY_NAME) {
        Ok(mut entry) => {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(io_err(source_zip))?;
            serde_json::from_slice(&bytes).ok()
        }
        Err(_) => None,
    };

    let fallback_name = source_zip
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported instance".to_string());
    // A carried-over `mod_loader` names a merged version JSON cached under this machine's own
    // `versions_dir()` -- which won't exist here (or on a fresh machine) until it's reinstalled.
    // Rather than have `launch` fail confusingly on a missing cache entry, drop it on import and
    // let the instance detail screen show "no mod loader installed", same as any other instance.
    let (name, version_id) = match manifest {
        Some(m) => (m.name, m.version_id),
        None => (fallback_name, String::new()),
    };

    let base = sanitize_dir_name(&name);
    let taken = other_instance_ids(config, None);
    let id = unique_dir_name(&base, |candidate| taken.contains(candidate));
    let dest_dir = config.instances_dir().join(&id);
    std::fs::create_dir_all(&dest_dir).map_err(io_err(&dest_dir))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(entry_path) = entry.enclosed_name() else { continue };
        if entry_path == Path::new(MANIFEST_ENTRY_NAME) {
            continue;
        }
        let out_path = dest_dir.join(entry_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(io_err(&out_path))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut out_file = std::fs::File::create(&out_path).map_err(io_err(&out_path))?;
        std::io::copy(&mut entry, &mut out_file).map_err(io_err(&out_path))?;
    }

    Ok(Instance { id, name, version_id, icon_path: None, pinned_screenshot: None, mod_loader: None })
}
