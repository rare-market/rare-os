//! Ledger transport commands over `rare_wallet::ledger`. JS runs the Sui app protocol on top.

pub use rare_wallet::ledger::{LedgerDevice, LedgerState};

#[tauri::command]
pub async fn ledger_list(ble: bool, scan_ms: u64) -> Result<Vec<LedgerDevice>, String> {
    LedgerState::list(ble, scan_ms).await
}

#[tauri::command]
pub async fn ledger_open(app: tauri::AppHandle, state: tauri::State<'_, LedgerState>, id: String) -> Result<LedgerDevice, String> {
    state.open(&super::sink(&app), &id).await
}

#[tauri::command]
pub async fn ledger_exchange(state: tauri::State<'_, LedgerState>, apdu: String) -> Result<String, String> {
    state.exchange_b64(&apdu).await
}

#[tauri::command]
pub async fn ledger_close(state: tauri::State<'_, LedgerState>) -> Result<(), String> {
    state.close().await;
    Ok(())
}
