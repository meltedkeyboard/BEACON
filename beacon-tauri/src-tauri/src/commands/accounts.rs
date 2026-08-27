use beacon_core::config::LauncherConfig;
use beacon_core::{forget_account, login_with_device_code, offline_account, Account, CoreError};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// Returns saved accounts in the frontend's "current account" order -- `accounts[0]` is always
/// the one Play/launch uses, matching [`move_account_to_front`]/`move_account_cmd` below.
#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, CoreError> {
    Ok(state.config.lock().await.accounts.clone())
}

/// Moves `account_id` to the front of the account list (making it "current") and keeps
/// `selected_account_id` in sync with it.
pub(crate) fn move_account_to_front(config: &mut LauncherConfig, account_id: &str) {
    if let Some(pos) = config.accounts.iter().position(|a| a.id() == account_id) {
        let account = config.accounts.remove(pos);
        config.accounts.insert(0, account);
    }
    config.selected_account_id = Some(account_id.to_string());
}

/// Makes an already-saved account current by moving it to the front of the list.
#[tauri::command]
pub async fn select_account_cmd(state: State<'_, AppState>, account_id: String) -> Result<(), CoreError> {
    let mut config = state.config.lock().await;
    if config.find_account(&account_id).is_none() {
        return Err(CoreError::AccountNotFound(account_id));
    }
    move_account_to_front(&mut config, &account_id);
    config.save(&state.config_path).await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MoveDirection {
    Up,
    Down,
}

/// Swaps `account_id` with its neighbor in the given direction -- a no-op at either end of the
/// list. Moving an account into (or out of) the front slot updates `selected_account_id` to
/// match, same as [`select_account_cmd`].
#[tauri::command]
pub async fn move_account_cmd(
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
pub async fn add_offline_account_cmd(
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
pub async fn rename_offline_account_cmd(
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

/// Runs the device code sign-in flow, emitting a `device-code` event with the code/URL the user
/// needs to complete it on their own device, then blocks until they finish. Saves the resulting
/// account to the config on success.
#[tauri::command]
pub async fn login_microsoft_cmd(app: AppHandle, state: State<'_, AppState>) -> Result<Account, CoreError> {
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
pub async fn logout_cmd(state: State<'_, AppState>, account_id: String) -> Result<(), CoreError> {
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
