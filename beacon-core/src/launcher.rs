use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tokio::process::{Child, Command};
use uuid::Uuid;

use crate::account::Account;
use crate::assets::install_assets;
use crate::config::LauncherConfig;
use crate::downloader::{ensure_files, DownloadProgress, DownloadTask, ProgressCallback};
use crate::error::{CoreError, Result};
use crate::libraries::{self, extract_natives};
use crate::manifest::{
    fetch_version_data, fetch_version_manifest, ArgumentEntry, VersionData,
};
use crate::rules::{rules_allow, FeatureFlags};

/// Wraps `on_progress` so every event it forwards is stamped with `phase` -- `ensure_files` has
/// no idea whether a batch of tasks is libraries or assets, so the caller (here) is the only place
/// that knows, and the UI wants that label (real launchers show "Downloading Libraries" /
/// "Downloading Assets" rather than a bare percentage).
fn with_phase(phase: &'static str, on_progress: Option<ProgressCallback>) -> Option<ProgressCallback> {
    on_progress.map(|cb| -> ProgressCallback {
        Arc::new(move |mut progress: DownloadProgress| {
            progress.phase = phase.to_string();
            cb(progress);
        })
    })
}

/// Downloads everything required to run `version_id`: client jar, libraries, natives and assets.
/// Already-valid files (verified by SHA1) are skipped, so this is safe to call on every launch.
pub async fn install_version(
    client: &reqwest::Client,
    config: &LauncherConfig,
    version_id: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<VersionData> {
    eprintln!("[beacon] install_version: {version_id}");
    let version_data = load_or_fetch_version_data(client, config, version_id).await?;
    eprintln!(
        "[beacon] install_version: version data ready (type={}, main_class={}, {} libraries, asset index={})",
        version_data.version_type,
        version_data.main_class,
        version_data.libraries.len(),
        version_data.asset_index.id,
    );

    let version_dir = config.version_dir(version_id);
    let client_jar_path = version_dir.join(format!("{version_id}.jar"));
    let mut tasks = vec![DownloadTask {
        url: version_data.downloads.client.url.clone(),
        dest: client_jar_path,
        sha1: Some(version_data.downloads.client.sha1.clone()),
        size: Some(version_data.downloads.client.size),
    }];

    let resolved = libraries::resolve(&version_data.libraries, &config.libraries_dir(), &FeatureFlags);
    eprintln!(
        "[beacon] install_version: {} library download tasks, {} native archives to extract",
        resolved.download_tasks.len(),
        resolved.native_archives.len(),
    );
    tasks.extend(resolved.download_tasks);

    eprintln!("[beacon] install_version: downloading client jar + libraries...");
    ensure_files(client, tasks, with_phase("Libraries", on_progress.clone())).await?;
    eprintln!("[beacon] install_version: extracting natives...");
    extract_natives(&resolved.native_archives, &config.natives_dir(version_id))?;
    eprintln!("[beacon] install_version: downloading assets...");
    install_assets(client, &config.assets_dir(), &version_data.asset_index, with_phase("Assets", on_progress)).await?;
    eprintln!("[beacon] install_version: {version_id} fully installed");

    Ok(version_data)
}

async fn load_or_fetch_version_data(
    client: &reqwest::Client,
    config: &LauncherConfig,
    version_id: &str,
) -> Result<VersionData> {
    let cache_path = config.version_dir(version_id).join(format!("{version_id}.json"));
    if let Ok(bytes) = tokio::fs::read(&cache_path).await {
        if let Ok(data) = serde_json::from_slice::<VersionData>(&bytes) {
            return Ok(data);
        }
    }

    let manifest = fetch_version_manifest(client).await?;
    let entry = manifest
        .find(version_id)
        .ok_or_else(|| CoreError::VersionNotFound(version_id.to_string()))?;
    let version_data = fetch_version_data(client, &entry.url).await?;

    if let Some(parent) = cache_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(crate::error::io_err(parent))?;
    }
    tokio::fs::write(&cache_path, serde_json::to_vec_pretty(&version_data)?)
        .await
        .map_err(crate::error::io_err(&cache_path))?;

    Ok(version_data)
}

fn build_classpath(classpath_jars: &[PathBuf], client_jar: &PathBuf) -> String {
    let separator = if cfg!(target_os = "windows") { ";" } else { ":" };
    classpath_jars
        .iter()
        .chain(std::iter::once(client_jar))
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(separator)
}

fn substitute(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("${{{key}}}"), value);
    }
    result
}

fn default_jvm_args() -> Vec<String> {
    vec![
        "-Djava.library.path=${natives_directory}".to_string(),
        "-cp".to_string(),
        "${classpath}".to_string(),
    ]
}

/// Filters a mixed plain/conditional argument list down to the flat list of strings that
/// actually apply on this platform, with `${...}` placeholders left unsubstituted.
fn expand_argument_entries(entries: &[ArgumentEntry]) -> Vec<String> {
    let features = FeatureFlags;
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            ArgumentEntry::Plain(s) => out.push(s.clone()),
            ArgumentEntry::Conditional { rules, value } => {
                if rules_allow(rules, &features) {
                    out.extend(value.as_vec());
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LaunchOptions {
    pub game_dir: PathBuf,
    pub java_path: String,
    pub extra_jvm_args: Vec<String>,
}

/// Builds and spawns the JVM process for an already-installed version. Callers are expected to
/// have run [`install_version`] first (or otherwise guarantee the files are present).
///
/// `ms_session` must be `Some` when `account` is [`Account::Microsoft`] -- obtain one via
/// [`crate::login::login_with_device_code`] or [`crate::login::refresh_session`] right before
/// calling this, since the token it carries is short-lived and not persisted.
///
/// The returned child's stdout/stderr are piped, not inherited -- a GUI frontend has no console
/// to inherit into, so callers (CLI included) are expected to read and forward those streams
/// themselves.
pub async fn launch(
    config: &LauncherConfig,
    version_data: &VersionData,
    account: &Account,
    ms_session: Option<&crate::auth::MinecraftSession>,
    options: LaunchOptions,
) -> Result<Child> {
    eprintln!("[beacon] launch: {} (account: {})", version_data.id, account.username());
    let resolved = libraries::resolve(&version_data.libraries, &config.libraries_dir(), &FeatureFlags);
    let client_jar = config
        .version_dir(&version_data.id)
        .join(format!("{}.jar", version_data.id));
    let classpath = build_classpath(&resolved.classpath, &client_jar);
    let natives_dir = config.natives_dir(&version_data.id);
    eprintln!(
        "[beacon] launch: classpath has {} entries, natives_dir={}, game_dir={}",
        resolved.classpath.len() + 1,
        natives_dir.display(),
        options.game_dir.display(),
    );

    let (auth_access_token, user_type, client_id, auth_xuid) = match account {
        Account::Offline { .. } => (String::new(), "legacy".to_string(), String::new(), String::new()),
        Account::Microsoft { .. } => {
            let session = ms_session.ok_or_else(|| {
                CoreError::Other("launching a Microsoft account requires a ms_session".into())
            })?;
            (
                session.access_token.clone(),
                "msa".to_string(),
                config.azure_client_id.clone(),
                session.xuid.clone(),
            )
        }
    };

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", account.username().to_string());
    vars.insert("version_name", version_data.id.clone());
    vars.insert("game_directory", options.game_dir.to_string_lossy().into_owned());
    vars.insert("assets_root", config.assets_dir().to_string_lossy().into_owned());
    vars.insert("assets_index_name", version_data.assets.clone());
    vars.insert("auth_uuid", account.uuid().hyphenated().to_string());
    vars.insert("auth_access_token", auth_access_token);
    vars.insert("clientid", client_id);
    vars.insert("auth_xuid", auth_xuid);
    vars.insert("user_type", user_type);
    vars.insert("version_type", version_data.version_type.clone());
    vars.insert("natives_directory", natives_dir.to_string_lossy().into_owned());
    vars.insert("launcher_name", "beacon".to_string());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").to_string());
    vars.insert("classpath", classpath);

    let (jvm_template, game_template): (Vec<String>, Vec<String>) = match &version_data.arguments {
        Some(arguments) => (
            expand_argument_entries(&arguments.jvm),
            expand_argument_entries(&arguments.game),
        ),
        None => (
            default_jvm_args(),
            version_data
                .minecraft_arguments
                .as_deref()
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        ),
    };

    let jvm_args: Vec<String> = jvm_template.iter().map(|arg| substitute(arg, &vars)).collect();
    let game_args: Vec<String> = game_template.iter().map(|arg| substitute(arg, &vars)).collect();

    // The access token would otherwise show up verbatim in `--accessToken <token>` -- redact it
    // before this ever reaches a log line.
    let redact = |s: &str| -> String {
        let token = vars.get("auth_access_token").map(String::as_str).unwrap_or("");
        if token.is_empty() { s.to_string() } else { s.replace(token, "<redacted>") }
    };
    eprintln!(
        "[beacon] launch: {} {}",
        options.java_path,
        options
            .extra_jvm_args
            .iter()
            .chain(jvm_args.iter())
            .map(|s| redact(s))
            .chain(std::iter::once(version_data.main_class.clone()))
            .chain(game_args.iter().map(|s| redact(s)))
            .collect::<Vec<_>>()
            .join(" "),
    );

    let mut command = Command::new(&options.java_path);
    command
        .args(options.extra_jvm_args)
        .args(jvm_args)
        .arg(&version_data.main_class)
        .args(game_args)
        .current_dir(&options.game_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    tokio::fs::create_dir_all(&options.game_dir)
        .await
        .map_err(crate::error::io_err(&options.game_dir))?;

    match command.spawn() {
        Ok(child) => {
            eprintln!("[beacon] launch: JVM spawned (pid={:?})", child.id());
            Ok(child)
        }
        Err(e) => {
            eprintln!("[beacon] launch: failed to spawn JVM: {e}");
            Err(CoreError::Launch(e))
        }
    }
}

/// Convenience: some code paths only care about the offline UUID, kept here so downstream
/// crates don't need to depend on `uuid` directly just to format one.
pub fn format_uuid(uuid: Uuid) -> String {
    uuid.hyphenated().to_string()
}
