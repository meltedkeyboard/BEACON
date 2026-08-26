use std::path::PathBuf;
use std::sync::Arc;

use beacon_core::config::{default_config_path, LauncherConfig};
use beacon_core::downloader::{DownloadProgress, ProgressCallback};
use beacon_core::{
    forget_account, install_version, launch, login_with_device_code, offline_account,
    refresh_session, Account, CoreError, LaunchOptions, VersionEntry,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::Mutex;

/// Everything a command needs to talk to `beacon-core`. `config` is kept in memory and only
/// written back to disk on the commands that actually change it, mirroring what `beacon-cli`
/// does per-invocation.
struct AppState {
    http: reqwest::Client,
    config: Mutex<LauncherConfig>,
    config_path: PathBuf,
}

#[tauri::command]
async fn list_versions(
    state: State<'_, AppState>,
    snapshots: bool,
) -> Result<Vec<VersionEntry>, CoreError> {
    let manifest = beacon_core::fetch_version_manifest(&state.http).await?;
    Ok(manifest
        .versions
        .into_iter()
        .filter(|entry| snapshots || entry.version_type == "release")
        .collect())
}

/// Returns saved accounts in the frontend's "current account" order -- `accounts[0]` is always
/// the one Play/launch uses, matching [`move_account_to_front`]/`move_account_cmd` below.
#[tauri::command]
async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, CoreError> {
    Ok(state.config.lock().await.accounts.clone())
}

/// Moves `account_id` to the front of the account list (making it "current") and keeps
/// `selected_account_id` in sync with it.
fn move_account_to_front(config: &mut LauncherConfig, account_id: &str) {
    if let Some(pos) = config.accounts.iter().position(|a| a.id() == account_id) {
        let account = config.accounts.remove(pos);
        config.accounts.insert(0, account);
    }
    config.selected_account_id = Some(account_id.to_string());
}

/// Makes an already-saved account current by moving it to the front of the list.
#[tauri::command]
async fn select_account_cmd(state: State<'_, AppState>, account_id: String) -> Result<(), CoreError> {
    let mut config = state.config.lock().await;
    if config.find_account(&account_id).is_none() {
        return Err(CoreError::AccountNotFound(account_id));
    }
    move_account_to_front(&mut config, &account_id);
    config.save(&state.config_path).await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum MoveDirection {
    Up,
    Down,
}

/// Swaps `account_id` with its neighbor in the given direction -- a no-op at either end of the
/// list. Moving an account into (or out of) the front slot updates `selected_account_id` to
/// match, same as [`select_account_cmd`].
#[tauri::command]
async fn move_account_cmd(
    state: State<'_, AppState>,
    account_id: String,
    direction: MoveDirection,
) -> Result<(), CoreError> {
    let mut config = state.config.lock().await;
    let pos = config
        .accounts
        .iter()
        .position(|a| a.id() == account_id)
        .ok_or_else(|| CoreError::AccountNotFound(account_id.clone()))?;
    let neighbor = match direction {
        MoveDirection::Up => pos.checked_sub(1),
        MoveDirection::Down if pos + 1 < config.accounts.len() => Some(pos + 1),
        MoveDirection::Down => None,
    };
    if let Some(neighbor) = neighbor {
        config.accounts.swap(pos, neighbor);
    }
    config.selected_account_id = config.accounts.first().map(|a| a.id());
    config.save(&state.config_path).await
}

/// Adds a new offline account and appends it to the end of the list -- it does not become
/// current automatically (except trivially, if it's the only account) so that switching who's
/// "current" always goes through the explicit move/select commands above.
#[tauri::command]
async fn add_offline_account_cmd(
    state: State<'_, AppState>,
    nickname: String,
) -> Result<Account, CoreError> {
    let mut config = state.config.lock().await;
    let account = offline_account(&config, nickname)?;
    config.upsert_account(account.clone());
    if config.selected_account_id.is_none() {
        config.selected_account_id = Some(account.id());
    }
    config.save(&state.config_path).await?;
    Ok(account)
}

/// Renames an offline account. Its id is derived from the nickname (see
/// [`beacon_core::Account::id`]), so renaming replaces the list entry rather than mutating it in
/// place, and updates `selected_account_id` if the renamed account was current.
#[tauri::command]
async fn rename_offline_account_cmd(
    state: State<'_, AppState>,
    account_id: String,
    nickname: String,
) -> Result<Account, CoreError> {
    let mut config = state.config.lock().await;
    let position = config
        .accounts
        .iter()
        .position(|a| a.id() == account_id)
        .ok_or_else(|| CoreError::AccountNotFound(account_id.clone()))?;
    if !matches!(config.accounts[position], Account::Offline { .. }) {
        return Err(CoreError::Other("only offline accounts can be renamed".into()));
    }

    let new_account = offline_account(&config, nickname)?;
    let was_current = config.selected_account_id.as_deref() == Some(account_id.as_str());
    config.accounts[position] = new_account.clone();
    if was_current {
        config.selected_account_id = Some(new_account.id());
    }
    config.save(&state.config_path).await?;
    Ok(new_account)
}

/// Downloads client jar, libraries, natives and assets for `version_id`, emitting
/// `install-progress` events as it goes. Safe to call again on an already-installed version --
/// `install_version` skips files that already pass their SHA1 check.
#[tauri::command]
async fn install_version_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    version_id: String,
) -> Result<(), CoreError> {
    let config = state.config.lock().await.clone();
    let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
        let _ = app.emit("install-progress", progress);
    });
    install_version(&state.http, &config, &version_id, Some(on_progress)).await?;
    Ok(())
}

/// Runs the device code sign-in flow, emitting a `device-code` event with the code/URL the user
/// needs to complete it on their own device, then blocks until they finish. Saves the resulting
/// account to the config on success.
#[tauri::command]
async fn login_microsoft_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Account, CoreError> {
    let client_id = state.config.lock().await.azure_client_id.clone();
    let (account, _session) =
        login_with_device_code(&state.http, &client_id, |authorization| {
            let _ = app.emit("device-code", authorization);
        })
        .await?;

    let mut config = state.config.lock().await;
    config.upsert_account(account.clone());
    move_account_to_front(&mut config, &account.id());
    config.save(&state.config_path).await?;
    Ok(account)
}

/// Removes a saved account (Microsoft or offline). If it was current, the next account (now
/// first in the list) becomes current instead.
#[tauri::command]
async fn logout_cmd(state: State<'_, AppState>, account_id: String) -> Result<(), CoreError> {
    let mut config = state.config.lock().await;
    let account = config
        .find_account(&account_id)
        .cloned()
        .ok_or_else(|| CoreError::AccountNotFound(account_id.clone()))?;
    forget_account(&account).await?;
    config.accounts.retain(|a| a.id() != account_id);
    if config.selected_account_id.as_deref() == Some(account_id.as_str()) {
        config.selected_account_id = config.accounts.first().map(|a| a.id());
    }
    config.save(&state.config_path).await
}

// `rename_all` on the enum only renames the variant tags ("Offline" -> "offline", "Saved" ->
// "saved") -- it does not reach the fields inside each variant, so `account_id` needed its own
// `rename_all` to actually accept the frontend's `accountId`.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum AccountSelection {
    Offline { nickname: String },
    #[serde(rename_all = "camelCase")]
    Saved { account_id: String },
}

/// Installs (if needed, emitting `install-progress` events) and launches `version_id`, then
/// streams the game process's stdout/stderr back as `game-log` events -- `beacon_core::launch`
/// pipes them rather than inheriting a console, since a GUI window has none to inherit into.
/// Emits `launch-status` with `"launching"` right before the process is spawned and `"exited"`
/// once it quits, so the frontend can drive the Play button's label off two events instead of
/// polling.
#[tauri::command]
async fn launch_version_cmd(
    app: AppHandle,
    state: State<'_, AppState>,
    version_id: String,
    account: AccountSelection,
) -> Result<(), CoreError> {
    let config = state.config.lock().await.clone();
    let progress_app = app.clone();
    let on_progress: ProgressCallback = Arc::new(move |progress: DownloadProgress| {
        let _ = progress_app.emit("install-progress", progress);
    });
    let version_data = install_version(&state.http, &config, &version_id, Some(on_progress)).await?;

    let (account, ms_session) = match account {
        AccountSelection::Offline { nickname } => (offline_account(&config, nickname)?, None),
        AccountSelection::Saved { account_id } => {
            let account = config
                .find_account(&account_id)
                .cloned()
                .ok_or_else(|| CoreError::AccountNotFound(account_id.clone()))?;
            // "Saved" covers both account kinds -- only Microsoft accounts have a session to
            // refresh, offline ones just launch as-is.
            let session = match &account {
                Account::Microsoft { .. } => {
                    Some(refresh_session(&state.http, &config.azure_client_id, &account).await?)
                }
                Account::Offline { .. } => None,
            };
            (account, session)
        }
    };

    let options = LaunchOptions {
        game_dir: config.game_dir.clone(),
        java_path: config.java_path.clone(),
        extra_jvm_args: Vec::new(),
    };

    let _ = app.emit("launch-status", "launching");
    let mut child = launch(&config, &version_data, &account, ms_session.as_ref(), options).await?;
    if let Some(stdout) = child.stdout.take() {
        forward_log_lines(app.clone(), stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        forward_log_lines(app.clone(), stderr);
    }
    // The game runs independently of this command's lifetime; just reap the process so it
    // doesn't linger as a zombie once it exits, and let the frontend know it's gone.
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        let _ = app.emit("launch-status", "exited");
    });
    Ok(())
}

fn forward_log_lines(app: AppHandle, reader: impl AsyncRead + Unpin + Send + 'static) {
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app.emit("game-log", line);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_path = default_config_path();
            let config = tauri::async_runtime::block_on(LauncherConfig::load_or_default(&config_path))?;
            app.manage(AppState {
                http: beacon_core::http_client(),
                config: Mutex::new(config),
                config_path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_versions,
            list_accounts,
            select_account_cmd,
            move_account_cmd,
            add_offline_account_cmd,
            rename_offline_account_cmd,
            install_version_cmd,
            login_microsoft_cmd,
            logout_cmd,
            launch_version_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
