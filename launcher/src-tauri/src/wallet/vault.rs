//! Vault commands over `rare_wallet::vault`. Secrets arrive as Strings and are zeroized here.

pub use rare_wallet::vault::{VaultState, VaultStatus};
use zeroize::Zeroizing;

type St<'a> = tauri::State<'a, VaultState>;

#[tauri::command]
pub async fn vault_status(state: St<'_>) -> Result<VaultStatus, String> {
    state.blocking(|v| v.status()).await
}

#[tauri::command]
pub async fn vault_create(state: St<'_>, secret: String, kind: String, payload: String) -> Result<VaultStatus, String> {
    let (secret, payload) = (Zeroizing::new(secret), Zeroizing::new(payload));
    state.blocking(move |v| v.create(&secret, &kind, &payload)).await
}

#[tauri::command]
pub async fn vault_unlock(state: St<'_>, secret: String) -> Result<String, String> {
    let secret = Zeroizing::new(secret);
    state.blocking(move |v| v.unlock(&secret).map(|p| p.to_string())).await
}

#[tauri::command]
pub async fn vault_read(state: St<'_>) -> Result<Option<String>, String> {
    Ok(state.0.read().map(|p| p.to_string()))
}

#[tauri::command]
pub async fn vault_save(state: St<'_>, payload: String) -> Result<(), String> {
    let payload = Zeroizing::new(payload);
    state.blocking(move |v| v.save(&payload)).await
}

#[tauri::command]
pub async fn vault_lock(state: St<'_>) -> Result<(), String> {
    state.0.lock();
    Ok(())
}

#[tauri::command]
pub async fn vault_verify(state: St<'_>, secret: String) -> Result<String, String> {
    let secret = Zeroizing::new(secret);
    state.blocking(move |v| v.verify(&secret).map(|p| p.to_string())).await
}

#[tauri::command]
pub async fn vault_change_secret(state: St<'_>, current: String, next: String, kind: String) -> Result<(), String> {
    let (current, next) = (Zeroizing::new(current), Zeroizing::new(next));
    state.blocking(move |v| v.change_secret(&current, &next, &kind)).await
}

#[tauri::command]
pub async fn vault_reset(state: St<'_>) -> Result<(), String> {
    state.blocking(|v| v.reset()).await
}
