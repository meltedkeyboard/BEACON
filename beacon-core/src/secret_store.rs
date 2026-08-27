use keyring::Entry;

use crate::error::Result;

const SERVICE: &str = "beacon";

/// Persists a Microsoft account's OAuth2 refresh token in the OS credential store
/// (Windows Credential Manager / macOS Keychain / Linux Secret Service, depending on
/// platform and enabled `keyring` backend features) instead of writing it to disk in
/// plaintext alongside the rest of the config.
pub async fn save_refresh_token(account_id: &str, refresh_token: &str) -> Result<()> {
    let account_id = account_id.to_string();
    let refresh_token = refresh_token.to_string();
    tokio::task::spawn_blocking(move || {
        Entry::new(SERVICE, &account_id)?.set_password(&refresh_token)?;
        Ok(())
    })
    .await
    .expect("blocking task panicked")
}

pub async fn load_refresh_token(account_id: &str) -> Result<Option<String>> {
    let account_id = account_id.to_string();
    tokio::task::spawn_blocking(move || match Entry::new(SERVICE, &account_id)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    })
    .await
    .expect("blocking task panicked")
}

pub async fn delete_refresh_token(account_id: &str) -> Result<()> {
    let account_id = account_id.to_string();
    tokio::task::spawn_blocking(move || match Entry::new(SERVICE, &account_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    })
    .await
    .expect("blocking task panicked")
}

/// Fixed credential-store key name for the user's own personal CurseForge API key -- unlike
/// accounts there's only ever one, so no per-entity id is needed. Stored here (not plaintext in
/// `config.json`) because it's the user's own credential, not something Beacon owns: CurseForge's
/// own Terms forbid a key being embedded in a distributed app binary, so this only ever holds a
/// key the user pasted in themselves, in Settings.
const CURSEFORGE_API_KEY_ENTRY: &str = "curseforge-api-key";

pub async fn save_curseforge_api_key(api_key: &str) -> Result<()> {
    let api_key = api_key.to_string();
    tokio::task::spawn_blocking(move || {
        Entry::new(SERVICE, CURSEFORGE_API_KEY_ENTRY)?.set_password(&api_key)?;
        Ok(())
    })
    .await
    .expect("blocking task panicked")
}

pub async fn load_curseforge_api_key() -> Result<Option<String>> {
    tokio::task::spawn_blocking(|| match Entry::new(SERVICE, CURSEFORGE_API_KEY_ENTRY)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    })
    .await
    .expect("blocking task panicked")
}

pub async fn delete_curseforge_api_key() -> Result<()> {
    tokio::task::spawn_blocking(|| match Entry::new(SERVICE, CURSEFORGE_API_KEY_ENTRY)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    })
    .await
    .expect("blocking task panicked")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_through_credential_manager() {
        let account_id = "test:roundtrip-9f3a";
        save_refresh_token(account_id, "sekrit-token").await.unwrap();
        assert_eq!(
            load_refresh_token(account_id).await.unwrap(),
            Some("sekrit-token".to_string())
        );
        delete_refresh_token(account_id).await.unwrap();
        assert_eq!(load_refresh_token(account_id).await.unwrap(), None);
    }
}
