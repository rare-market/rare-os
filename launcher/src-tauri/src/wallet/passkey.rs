//! Passkey ceremony commands over `rare_wallet::passkey` (CTAP2 to a phone or a security key).
//! Progress goes to the webview as "passkey-event".

use rare_wallet::passkey::webauthn::{Assertion, Registration};
pub use rare_wallet::passkey::PasskeyState;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn passkey_register<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, PasskeyState>,
    user_id: String,
    user_name: String,
    display_name: String,
    challenge: String,
    via: String,
) -> Result<Registration, String> {
    state.register(&super::sink(&app), &user_id, user_name, display_name, &challenge, &via).await
}

#[tauri::command]
pub async fn passkey_assert<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, PasskeyState>,
    challenge: String,
    credential_ids: Vec<String>,
    via: String,
    prf_salt: Option<String>,
) -> Result<Assertion, String> {
    state.assert(&super::sink(&app), &challenge, &credential_ids, &via, prf_salt.as_deref()).await
}

#[tauri::command]
pub async fn passkey_prf<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, PasskeyState>,
    credential_ids: Vec<String>,
    salt: String,
    via: String,
) -> Result<String, String> {
    state.prf(&super::sink(&app), &credential_ids, &salt, &via).await
}

#[tauri::command]
pub async fn passkey_pin(state: State<'_, PasskeyState>, pin: Option<String>) -> Result<(), String> {
    state.pin(pin);
    Ok(())
}

#[tauri::command]
pub async fn passkey_cancel(state: State<'_, PasskeyState>) -> Result<(), String> {
    state.cancel();
    Ok(())
}
