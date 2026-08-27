use beacon_core::skins::{clear_cape, fetch_profile, reset_skin, set_cape, upload_skin};
use beacon_core::{Account, CoreError, MinecraftProfile};
use tauri::State;

use crate::state::AppState;

/// Resolves `account_id` to a saved `Account::Microsoft` and refreshes it into a live Minecraft
/// access token -- skins/capes are Mojang profile data, meaningless for an offline account, and
/// the token itself is never persisted (same reasoning as `launch_instance_cmd`'s use of
/// `refresh_session`), so every skin command re-derives one instead of caching it.
async fn microsoft_access_token(state: &State<'_, AppState>, account_id: &str) -> Result<String, CoreError> {
    let config = state.config.lock().await.clone();
    let account = config
        .find_account(account_id)
        .cloned()
        .ok_or_else(|| CoreError::AccountNotFound(account_id.to_string()))?;
    if !matches!(account, Account::Microsoft { .. }) {
        return Err(CoreError::Other("skins are only available for Microsoft accounts".into()));
    }
    let session = beacon_core::refresh_session(&state.http, &config.azure_client_id, &account).await?;
    Ok(session.access_token)
}

#[tauri::command]
pub async fn get_skin_profile_cmd(state: State<'_, AppState>, account_id: String) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    fetch_profile(&state.http, &token).await
}

/// Reads `file_path` (picked by the frontend's open-file dialog, no folder restriction) straight
/// off disk -- same "arbitrary dialog-picked path, backend just reads it" treatment as instance
/// icons. `variant` is `"classic"` or `"slim"`.
#[tauri::command]
pub async fn upload_skin_cmd(
    state: State<'_, AppState>,
    account_id: String,
    file_path: String,
    variant: String,
) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| CoreError::Other(format!("couldn't read '{file_path}': {e}")))?;
    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "skin.png".to_string());
    upload_skin(&state.http, &token, bytes, &file_name, &variant).await?;
    fetch_profile(&state.http, &token).await
}

#[tauri::command]
pub async fn reset_skin_cmd(state: State<'_, AppState>, account_id: String) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    reset_skin(&state.http, &token).await?;
    fetch_profile(&state.http, &token).await
}

#[tauri::command]
pub async fn set_cape_cmd(state: State<'_, AppState>, account_id: String, cape_id: String) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    set_cape(&state.http, &token, &cape_id).await?;
    fetch_profile(&state.http, &token).await
}

#[tauri::command]
pub async fn clear_cape_cmd(state: State<'_, AppState>, account_id: String) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    clear_cape(&state.http, &token).await?;
    fetch_profile(&state.http, &token).await
}
