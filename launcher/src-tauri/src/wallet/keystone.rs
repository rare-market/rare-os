//! Keystone 3 Pro USB commands over `rare_wallet::keystone`.

pub use rare_wallet::keystone::{KeystoneDevice, KeystoneState};

#[tauri::command]
pub async fn keystone_list() -> Result<Vec<KeystoneDevice>, String> {
    KeystoneState::list().await
}

#[tauri::command]
pub async fn keystone_send(state: tauri::State<'_, KeystoneState>, command: u16, data: String, timeout_ms: u64) -> Result<String, String> {
    state.send(command, &data, timeout_ms).await
}
