//! The wallet's account list (public data: addresses, names, public keys) mirrored to
//! `<data dir>/rare/wallet/accounts.json`, where the native launcher's keystore reads it
//! (`rare_wallet::keystore::FileStore`). The web UI keeps its copy in localStorage and writes
//! through here on every change, so both front ends see the same accounts.

use rare_wallet::keystore::{AccountStore, FileStore, Meta};

fn store() -> FileStore {
    FileStore::new(rare_wallet::vault::default_dir())
}

#[tauri::command]
pub async fn wallet_accounts_read() -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(|| store().load().map(|m| m.and_then(|m| serde_json::to_string(&m).ok())))
        .await
        .map_err(|_| rare_wallet::err("error", "Account list task failed"))?
}

#[tauri::command]
pub async fn wallet_accounts_write(json: String) -> Result<(), String> {
    let meta: Meta = serde_json::from_str(&json).map_err(|_| rare_wallet::err("error", "Not an account list"))?;
    tokio::task::spawn_blocking(move || store().save(&meta)).await.map_err(|_| rare_wallet::err("error", "Account list task failed"))?
}

#[tauri::command]
pub async fn wallet_accounts_clear() -> Result<(), String> {
    tokio::task::spawn_blocking(|| store().clear()).await.map_err(|_| rare_wallet::err("error", "Account list task failed"))?
}
