use std::sync::Arc;

use beacon_core::downloader::{DownloadProgress, ProgressCallback};
use beacon_core::{
    install_version, launch, offline_account, Account, CoreError, LaunchOptions,
    VersionEntry,
};
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

use crate::state::AppState;

#[tauri::command]
pub async fn list_versions(state: State<'_, AppState>, snapshots: bool) -> Result<Vec<VersionEntry>, CoreError> {
    let manifest = beacon_core::fetch_version_manifest(&state.http).await?;
    Ok(manifest
        .versions
        .into_iter()
        .filter(|entry| snapshots || entry.version_type == "release")
        .collect())
}

/// Downloads client jar, libraries, natives and assets for `version_id`, emitting
/// `install-progress` events as it goes. Safe to call again on an already-installed version --
/// `install_version` skips files that already pass their SHA1 check.
#[tauri::command]
pub async fn install_version_cmd(app: AppHandle, state: State<'_, AppState>, version_id: String) -> Result<(), CoreError> {
    let config = state.config.lock().await.clone();
    let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
        let _ = app.emit("install-progress", progress);
    });
    install_version(&state.http, &config, &version_id, Some(on_progress)).await?;
    Ok(())
}

// `rename_all` on the enum only renames the variant tags ("Offline" -> "offline", "Saved" ->
// "saved") -- it does not reach the fields inside each variant, so `account_id` needed its own
// `rename_all` to actually accept the frontend's `accountId`.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AccountSelection {
    Offline { nickname: String },
    #[serde(rename_all = "camelCase")]
    Saved { account_id: String },
}

/// Installs `instance_id`'s target version (if needed, emitting `install-progress` events) into
/// the shared version cache, then launches it with that instance's own directory as the game
/// directory -- so its saves/resourcepacks/shaderpacks/config stay isolated from every other
/// instance, even ones targeting the same Minecraft version. Streams the game process's
/// stdout/stderr back as `game-log` events -- `beacon_core::launch` pipes them rather than
/// inheriting a console, since a GUI window has none to inherit into. Emits `launch-status` with
/// `"launching"` right before the process is spawned and `"exited"` once it quits, so the
/// frontend can drive the Play button's label off two events instead of polling.
#[tauri::command]
pub async fn launch_instance_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    account: AccountSelection,
) -> Result<(), CoreError> {
    eprintln!("[beacon] launch_instance_cmd: instance={instance_id}");
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| log_err(CoreError::Other(format!("no instance '{instance_id}'"))))?;
    // A mod loader's merged version JSON is what's actually installed/launched -- `version_id`
    // itself always stays the plain vanilla id the instance's "Minecraft X.Y" label shows.
    let effective_version_id = instance
        .mod_loader
        .as_ref()
        .map(|l| l.effective_version_id.clone())
        .unwrap_or_else(|| instance.version_id.clone());
    eprintln!("[beacon] launch_instance_cmd: target version={effective_version_id}");

    let progress_app = app.clone();
    let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
        let _ = progress_app.emit("install-progress", progress);
    });
    let version_data = install_version(&state.http, &config, &effective_version_id, Some(on_progress))
        .await
        .map_err(log_err)?;

    let (account, ms_session) = match account {
        AccountSelection::Offline { nickname } => {
            eprintln!("[beacon] launch_instance_cmd: offline account '{nickname}'");
            (offline_account(&config, nickname).map_err(log_err)?, None)
        }
        AccountSelection::Saved { account_id } => {
            let account = config
                .find_account(&account_id)
                .cloned()
                .ok_or_else(|| log_err(CoreError::AccountNotFound(account_id.clone())))?;
            // "Saved" covers both account kinds -- only Microsoft accounts have a session to
            // refresh, offline ones just launch as-is.
            let session = match &account {
                Account::Microsoft { .. } => {
                    eprintln!("[beacon] launch_instance_cmd: session for '{account_id}' (cached if still fresh)");
                    Some(state.minecraft_session(&config, &account).await.map_err(log_err)?)
                }
                Account::Offline { .. } => {
                    eprintln!("[beacon] launch_instance_cmd: saved offline account '{account_id}'");
                    None
                }
            };
            (account, session)
        }
    };

    let options = LaunchOptions {
        game_dir: instance.dir(&config),
        java_path: config.java_path.clone(),
        extra_jvm_args: Vec::new(),
    };

    let _ = app.emit("launch-status", "launching");
    let mut child = launch(&config, &version_data, &account, ms_session.as_ref(), options)
        .await
        .map_err(log_err)?;
    if let Some(stdout) = child.stdout.take() {
        forward_log_lines(app.clone(), stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        forward_log_lines(app.clone(), stderr);
    }
    // The game runs independently of this command's lifetime; just reap the process so it
    // doesn't linger as a zombie once it exits, and let the frontend know it's gone.
    tauri::async_runtime::spawn(async move {
        let status = child.wait().await;
        eprintln!("[beacon] launch_instance_cmd: game process exited: {status:?}");
        let _ = app.emit("launch-status", "exited");
    });
    Ok(())
}

/// Every `CoreError` a Tauri command returns crosses the IPC boundary as JSON and never touches
/// this process's own stdout/stderr -- without this, the terminal running `cargo tauri dev` would
/// show nothing at all for a failed install/launch, only the frontend's generic error modal.
fn log_err(e: CoreError) -> CoreError {
    eprintln!("[beacon] launch_instance_cmd: error: {e}");
    e
}

fn forward_log_lines(app: AppHandle, reader: impl AsyncRead + Unpin + Send + 'static) {
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[game] {line}");
            let _ = app.emit("game-log", line);
        }
    });
}
