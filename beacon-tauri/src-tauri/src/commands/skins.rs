use beacon_core::skins::{clear_cape, fetch_profile, reset_skin, set_cape, upload_skin};
use beacon_core::{Account, CoreError, MinecraftProfile};
use tauri::State;

use crate::state::AppState;

/// Resolves `account_id` to a saved `Account::Microsoft` and gets a live Minecraft access token
/// for it -- skins/capes are Mojang profile data, meaningless for an offline account. Goes through
/// `AppState::minecraft_session`'s cache rather than calling `refresh_session` directly: opening
/// the Skins tab repeatedly (it reloads on every tab switch and every account-list change) used to
/// re-run the whole Microsoft -> Xbox Live -> XSTS -> Minecraft Services chain each time, which is
/// exactly what was tripping Microsoft's rate limit (429s) when done in quick succession.
async fn microsoft_access_token(state: &State<'_, AppState>, account_id: &str) -> Result<String, CoreError> {
    let config = state.config.lock().await.clone();
    let account = config
        .find_account(account_id)
        .cloned()
        .ok_or_else(|| CoreError::AccountNotFound(account_id.to_string()))?;
    if !matches!(account, Account::Microsoft { .. }) {
        return Err(CoreError::Other("skins are only available for Microsoft accounts".into()));
    }
    let session = state.minecraft_session(&config, &account).await?;
    Ok(session.access_token)
}

/// Fetches a fresh profile and updates the cache with it -- used by every command that just
/// mutated the account's skin/cape (upload/reset/set/clear), since they already need the fresh
/// state to return to the frontend, and by `get_skin_profile_cmd` on a cache miss or explicit
/// refresh.
async fn fetch_and_cache_profile(state: &State<'_, AppState>, account_id: &str, token: &str) -> Result<MinecraftProfile, CoreError> {
    let profile = fetch_profile(&state.http, token).await?;
    state.skin_profiles.lock().await.insert(account_id.to_string(), profile.clone());
    Ok(profile)
}

/// Returns the cached profile unless `force_refresh` is set or nothing's cached yet -- see
/// `AppState::skin_profiles`'s doc comment for why a live fetch isn't the default on every tab
/// open. The frontend passes `force_refresh: true` from its own Refresh button.
#[tauri::command]
pub async fn get_skin_profile_cmd(state: State<'_, AppState>, account_id: String, force_refresh: bool) -> Result<MinecraftProfile, CoreError> {
    if !force_refresh {
        if let Some(cached) = state.skin_profiles.lock().await.get(&account_id).cloned() {
            return Ok(cached);
        }
    }
    let token = microsoft_access_token(&state, &account_id).await?;
    fetch_and_cache_profile(&state, &account_id, &token).await
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
    fetch_and_cache_profile(&state, &account_id, &token).await
}

#[tauri::command]
pub async fn reset_skin_cmd(state: State<'_, AppState>, account_id: String) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    reset_skin(&state.http, &token).await?;
    fetch_and_cache_profile(&state, &account_id, &token).await
}

#[tauri::command]
pub async fn set_cape_cmd(state: State<'_, AppState>, account_id: String, cape_id: String) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    set_cape(&state.http, &token, &cape_id).await?;
    fetch_and_cache_profile(&state, &account_id, &token).await
}

#[tauri::command]
pub async fn clear_cape_cmd(state: State<'_, AppState>, account_id: String) -> Result<MinecraftProfile, CoreError> {
    let token = microsoft_access_token(&state, &account_id).await?;
    clear_cape(&state.http, &token).await?;
    fetch_and_cache_profile(&state, &account_id, &token).await
}
