//! Bluetooth adapter commands over `rare_wallet::bluetooth` (phone passkeys, Ledger BLE).

pub use rare_wallet::bluetooth::{set_powered, state_now, BtState};

#[tauri::command]
pub async fn bluetooth_state() -> Result<BtState, String> {
    rare_wallet::bluetooth::state().await
}

/// Powers the adapter on. Error codes: "no-adapter", "bt-blocked", "bt-denied", "bt-failed".
#[tauri::command]
pub async fn bluetooth_power_on() -> Result<BtState, String> {
    rare_wallet::bluetooth::power_on().await
}
