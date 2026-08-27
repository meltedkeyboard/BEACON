//! Best-effort mod metadata extraction (version + icon) for the Mods list -- reads whichever of
//! Fabric/Quilt/Forge/NeoForge's own manifest a jar carries, since each mod loader defines its own
//! shape. Never returns an error: a jar this can't make sense of (an unusual/legacy mod, or a
//! `.disabled`-suffixed file that's still a perfectly valid jar underneath) just yields an empty
//! [`ModMetadata`], since this is purely cosmetic and must never break the Mods list itself.
//!
//! Also home to [`encode_icon_data_url`], the shared "read these bytes into a `data:` URL" helper
//! `instance.rs` reuses for world/resource-pack icons -- none of these need a new asset-protocol
//! scope grant the way `Instance::icon_path`'s `convertFileSrc` handling does, since the bytes
//! travel over the IPC call itself instead of a second file read from the frontend.

use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ModMetadata {
    pub version: Option<String>,
    pub icon_data_url: Option<String>,
}

pub fn read_mod_metadata(jar_path: &Path) -> ModMetadata {
    let Ok(file) = std::fs::File::open(jar_path) else { return ModMetadata::default() };
    let Ok(mut archive) = zip::ZipArchive::new(file) else { return ModMetadata::default() };

    read_fabric_or_quilt(&mut archive, "fabric.mod.json", false)
        .or_else(|| read_fabric_or_quilt(&mut archive, "quilt.mod.json", true))
        .or_else(|| read_forge_toml(&mut archive))
        .unwrap_or_default()
}

fn read_zip_entry_bytes(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut entry = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn read_icon_from_zip(archive: &mut zip::ZipArchive<std::fs::File>, icon_path: &str) -> Option<String> {
    let icon_path = icon_path.trim_start_matches('/');
    let bytes = read_zip_entry_bytes(archive, icon_path)?;
    Some(encode_icon_data_url(&bytes, icon_path))
}

/// `fabric.mod.json`'s `version`/`icon` sit at the top level; `quilt.mod.json`'s sit nested under
/// `quilt_loader`/`quilt_loader.metadata` -- otherwise identical shape (Quilt intentionally kept
/// close to Fabric's format here, same as it did for the meta API used by mod-loader install).
fn read_fabric_or_quilt(archive: &mut zip::ZipArchive<std::fs::File>, entry_name: &str, is_quilt: bool) -> Option<ModMetadata> {
    let bytes = read_zip_entry_bytes(archive, entry_name)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;

    let (version, icon) = if is_quilt {
        let loader = json.get("quilt_loader")?;
        let version = loader.get("version").and_then(|v| v.as_str()).map(str::to_string);
        let icon = loader.get("metadata").and_then(|m| m.get("icon")).and_then(icon_field_to_path);
        (version, icon)
    } else {
        let version = json.get("version").and_then(|v| v.as_str()).map(str::to_string);
        let icon = json.get("icon").and_then(icon_field_to_path);
        (version, icon)
    };

    let icon_data_url = icon.and_then(|path| read_icon_from_zip(archive, &path));
    Some(ModMetadata { version, icon_data_url })
}

/// `icon` is usually a plain path string; some mods declare a `{size: path}` map instead (e.g.
/// `{"16": "icon-16.png", "128": "icon-128.png"}`) -- any one entry works fine here.
fn icon_field_to_path(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => map.values().next().and_then(|v| v.as_str()).map(str::to_string),
        _ => None,
    }
}

/// Forge/NeoForge: `META-INF/mods.toml`, a `[[mods]]` array (this only looks at the first entry --
/// a multi-mod jar is rare enough not to worry about here). `version` is very commonly the literal
/// template string `"${file.jarVersion}"`, which Forge's own loader resolves from the jar's own
/// manifest at runtime -- resolved the same way here instead of just displaying the raw template.
fn read_forge_toml(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<ModMetadata> {
    let bytes = read_zip_entry_bytes(archive, "META-INF/mods.toml")?;
    let text = String::from_utf8(bytes).ok()?;
    let value: toml::Value = text.parse().ok()?;

    let mod_table = value.get("mods")?.as_array()?.first()?;
    let mut version = mod_table.get("version").and_then(|v| v.as_str()).map(str::to_string);
    if version.as_deref() == Some("${file.jarVersion}") {
        version = read_manifest_implementation_version(archive);
    }

    let logo_file = mod_table
        .get("logoFile")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("logoFile").and_then(|v| v.as_str()))
        .map(str::to_string);
    let icon_data_url = logo_file.and_then(|path| read_icon_from_zip(archive, &path));

    Some(ModMetadata { version, icon_data_url })
}

fn read_manifest_implementation_version(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<String> {
    let bytes = read_zip_entry_bytes(archive, "META-INF/MANIFEST.MF")?;
    let text = String::from_utf8(bytes).ok()?;
    text.lines().find_map(|line| line.strip_prefix("Implementation-Version:").map(|v| v.trim().to_string()))
}

/// Base64-encodes `bytes` into a `data:` URL, guessing the mime type from `path_hint`'s own
/// extension (defaulting to PNG, the overwhelmingly common case for every icon convention this
/// module deals with). Hand-rolled rather than pulling in a `base64` crate, same call this
/// codebase already made for hex encoding in `downloader.rs`.
pub(crate) fn encode_icon_data_url(bytes: &[u8], path_hint: &str) -> String {
    let lower = path_hint.to_ascii_lowercase();
    let mime = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/png"
    };
    format!("data:{mime};base64,{}", base64_encode(bytes))
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 { ALPHABET[((n >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[(n & 0x3F) as usize] as char } else { '=' });
    }
    out
}
