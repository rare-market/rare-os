//! Rare Wallet's Tauri commands: thin wrappers over the `rare-wallet` crate
//! (launcher/crates/rare-wallet), which holds the vault, the hardware wallet transports and the
//! passkey ceremonies for both front ends. Secrets never reach logs or error strings.
//!
//! Errors cross to JS as "code|message" (or "code|message|{json}"), see src/wallet/native.ts.

pub mod accounts;
pub mod bluetooth;
pub mod keystone;
pub mod ledger;
pub mod passkey;
pub mod vault;

/// Wallet events to the webview, by the names native.ts listens for.
pub(crate) fn sink<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> rare_wallet::Sink {
    use tauri::Emitter;
    let app = app.clone();
    std::sync::Arc::new(move |e: rare_wallet::WalletEvent| {
        let _ = app.emit(e.name(), e.payload());
    })
}

/// Registers wallet state on the app.
pub fn manage<R: tauri::Runtime>(app: &tauri::App<R>) {
    use tauri::Manager;
    app.manage(vault::VaultState::default());
    app.manage(ledger::LedgerState::default());
    app.manage(keystone::KeystoneState::default());
    app.manage(passkey::PasskeyState::default());
}
