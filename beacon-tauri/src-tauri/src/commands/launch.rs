use std::sync::Arc;

use beacon_core::downloader::{DownloadProgress, ProgressCallback};
use beacon_core::{
    install_version, launch, offline_account, Account, CoreError, LaunchOptions,
    VersionEntry,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

use crate::state::{AppState, RunningGame};

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LaunchStatusEvent<'a> {
    instance_id: &'a str,
    status: &'a str,
}

fn emit_launch_status(app: &AppHandle, instance_id: &str, status: &str) {
    let _ = app.emit("launch-status", LaunchStatusEvent { instance_id, status });
}

#[tauri::command]
pub async fn list_versions(state: State<'_, AppState>, snapshots: bool) -> Result<Vec<VersionEntry>, CoreError> {
    let manifest = beacon_core::fetch_version_manifest(&state.http).await?;
    Ok(manifest
        .versions
        .into_iter()
        .filter(|entry| snapshots || entry.version_type == "release")
        .collect())
}

#[tauri::command]
pub async fn install_version_cmd(app: AppHandle, state: State<'_, AppState>, version_id: String) -> Result<(), CoreError> {
    let config = state.config.lock().await.clone();
    let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
        let _ = app.emit("install-progress", progress);
    });
    install_version(&state.http, &config, &version_id, Some(on_progress)).await?;
    Ok(())
}

// Outer rename_all doesn't reach fields inside variants, hence the inner one on `Saved`.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AccountSelection {
    Offline { nickname: String },
    #[serde(rename_all = "camelCase")]
    Saved { account_id: String },
}

#[tauri::command]
pub async fn launch_instance_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    account: AccountSelection,
) -> Result<(), CoreError> {
    eprintln!("[beacon] launch_instance_cmd: instance={instance_id}");
    if let Some(running) = state.running.lock().await.as_ref() {
        return Err(log_err(CoreError::Other(format!(
            "an instance is already running (pid {}) -- stop it first",
            running.pid
        ))));
    }
    let config = state.config.lock().await.clone();
    let instance = config
        .find_instance(&instance_id)
        .cloned()
        .ok_or_else(|| log_err(CoreError::Other(format!("no instance '{instance_id}'"))))?;
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

    emit_launch_status(&app, &instance_id, "launching");
    let mut child = launch(&config, &version_data, &account, ms_session.as_ref(), options)
        .await
        .map_err(log_err)?;
    if let Some(stdout) = child.stdout.take() {
        forward_log_lines(app.clone(), stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        forward_log_lines(app.clone(), stderr);
    }

    let pid = child
        .id()
        .ok_or_else(|| log_err(CoreError::Other("launched process has no pid".into())))?;
    *state.running.lock().await = Some(RunningGame {
        instance_id: instance_id.clone(),
        pid,
    });
    emit_launch_status(&app, &instance_id, "running");

    // Reaper: `state` doesn't outlive this command, so re-derive it from `app` ('static) instead.
    let running_instance_id = instance_id.clone();
    tauri::async_runtime::spawn(async move {
        let status = child.wait().await;
        eprintln!("[beacon] launch_instance_cmd: game process exited: {status:?}");
        let app_state = app.state::<AppState>();
        let mut running = app_state.running.lock().await;
        if running.as_ref().map(|r| r.instance_id.as_str()) == Some(running_instance_id.as_str()) {
            *running = None;
        }
        drop(running);
        emit_launch_status(&app, &running_instance_id, "exited");
    });
    Ok(())
}

#[cfg(windows)]
fn kill_pid(pid: u32) -> Result<(), CoreError> {
    let output = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()
        .map_err(|e| CoreError::Other(format!("failed to stop process: {e}")))?;
    if !output.status.success() {
        return Err(CoreError::Other(format!(
            "taskkill failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn kill_pid(pid: u32) -> Result<(), CoreError> {
    let output = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .output()
        .map_err(|e| CoreError::Other(format!("failed to stop process: {e}")))?;
    if !output.status.success() {
        return Err(CoreError::Other(format!(
            "kill failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

/// Only requests the kill; the reaper task in `launch_instance_cmd` clears `AppState::running`.
#[tauri::command]
pub async fn stop_instance_cmd(state: State<'_, AppState>, instance_id: String) -> Result<(), CoreError> {
    let running = state.running.lock().await.clone();
    match running {
        Some(r) if r.instance_id == instance_id => kill_pid(r.pid).map_err(log_err),
        Some(_) => Err(log_err(CoreError::Other("that instance isn't the one running".into()))),
        None => Err(log_err(CoreError::Other("instance is not running".into()))),
    }
}

#[tauri::command]
pub async fn running_instance_cmd(state: State<'_, AppState>) -> Result<Option<String>, CoreError> {
    Ok(state.running.lock().await.as_ref().map(|r| r.instance_id.clone()))
}

/// Command errors cross IPC as JSON only; log here so `cargo tauri dev`'s terminal shows them too.
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
