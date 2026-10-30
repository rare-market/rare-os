//! Rare Wallet, native side: one wallet for both front ends (the Tauri launcher through thin
//! command wrappers in src-tauri/src/wallet, and the native Rare Render launcher directly).
//!
//! - `vault`: the encrypted key vault (Argon2id + AES-256-GCM, OS keyring or a 0600 file)
//! - `keys`, `keystore`, `multisig`, `secret`: accounts, phrases, imported keys, passkey
//!   multisig accounts, signing and derived secrets (byte-identical to the TS wallet)
//! - `ledger`, `keystone`, `bluetooth` (feature `hw`): hardware wallet transports
//! - `passkey` (feature `passkey`): WebAuthn/CTAP2 ceremonies to a phone or a security key
//!
//! Secrets never reach logs or error strings. Errors are "code|message" or
//! "code|message|{json}" strings, the contract src/wallet/native.ts parses; `WalletError::parse`
//! reads them back for the native UI.

pub mod amount;
pub mod copy;
pub mod device_seal;
pub mod keys;
pub mod keystone_sui;
pub mod keystore;
pub mod ledger_sui;
pub mod multisig;
pub mod passkey_account;
pub mod pin_device;
pub mod pin_guard;
pub mod passkey_registry;
pub mod prefs;
pub mod recipient;
pub mod review;
pub mod secret;
pub mod send_pin;
#[cfg(feature = "chain")]
pub mod sign;
pub mod sponsor;
pub mod udev;
pub mod ur;
pub mod vault;
#[cfg(feature = "chain")]
pub mod webgame;

#[cfg(feature = "chain")]
pub mod adapters;
#[cfg(feature = "chain")]
pub mod move_all;
#[cfg(feature = "chain")]
pub mod nft;
#[cfg(feature = "chain")]
pub mod rescue;
#[cfg(feature = "hw")]
pub mod bluetooth;
#[cfg(all(feature = "hw", feature = "passkey", feature = "chain"))]
pub mod devices;
#[cfg(feature = "hw")]
pub mod keystone;
#[cfg(feature = "hw")]
pub mod ledger;
#[cfg(feature = "passkey")]
pub mod passkey;

use std::sync::Arc;

/// "code|message" error string.
pub fn err(code: &str, msg: impl std::fmt::Display) -> String {
    format!("{code}|{msg}")
}

/// "code|message|{json}" error string with extra data.
pub fn err_data(code: &str, msg: impl std::fmt::Display, data: serde_json::Value) -> String {
    format!("{code}|{msg}|{data}")
}

/// An error string split into its parts, the same way `parseNativeError` in native.ts does.
#[derive(Debug, Clone, PartialEq)]
pub struct WalletError {
    pub code: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

impl WalletError {
    pub fn parse(s: &str) -> Self {
        let plain = || WalletError { code: "error".into(), message: s.to_string(), data: None };
        let Some(i) = s.find('|') else { return plain() };
        if i == 0 || i > 24 {
            return plain();
        }
        let (code, rest) = (&s[..i], &s[i + 1..]);
        if let Some(j) = rest.find("|{") {
            if let Ok(data) = serde_json::from_str(&rest[j + 1..]) {
                return WalletError { code: code.into(), message: rest[..j].into(), data: Some(data) };
            }
        }
        WalletError { code: code.into(), message: rest.into(), data: None }
    }
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.data {
            Some(d) => write!(f, "{}|{}|{}", self.code, self.message, d),
            None => write!(f, "{}|{}", self.code, self.message),
        }
    }
}

impl std::error::Error for WalletError {}

/// Events the wallet sends while it works (the Tauri shell emits them by `name()`).
#[derive(Debug, Clone, PartialEq)]
pub enum WalletEvent {
    /// The 6-digit code BlueZ shows while pairing a Ledger over Bluetooth.
    LedgerPairing { passkey: String },
    /// Progress of a passkey ceremony.
    #[cfg(feature = "passkey")]
    Passkey(passkey::Event),
}

impl WalletEvent {
    /// The Tauri event name native.ts listens for.
    pub fn name(&self) -> &'static str {
        match self {
            WalletEvent::LedgerPairing { .. } => "ledger-pairing",
            #[cfg(feature = "passkey")]
            WalletEvent::Passkey(_) => "passkey-event",
        }
    }

    /// The event payload as native.ts reads it.
    pub fn payload(&self) -> serde_json::Value {
        match self {
            WalletEvent::LedgerPairing { passkey } => serde_json::json!({ "passkey": passkey }),
            #[cfg(feature = "passkey")]
            WalletEvent::Passkey(e) => serde_json::to_value(e).unwrap_or(serde_json::Value::Null),
        }
    }
}

/// Where events go.
pub type Sink = Arc<dyn Fn(WalletEvent) + Send + Sync>;

/// A sink that drops everything.
pub fn no_events() -> Sink {
    Arc::new(|_| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_strings_round_trip_like_native_ts() {
        let e = WalletError::parse(&err_data("wrong", "Wrong PIN", serde_json::json!({ "failures": 2, "retryAfterMs": 0 })));
        assert_eq!(e.code, "wrong");
        assert_eq!(e.message, "Wrong PIN");
        assert_eq!(e.data.unwrap()["failures"], 2);
        let e = WalletError::parse("locked|Unlock Rare Wallet first");
        assert_eq!((e.code.as_str(), e.message.as_str(), e.data), ("locked", "Unlock Rare Wallet first", None));
        assert_eq!(WalletError::parse("plain text").code, "error");
        assert_eq!(WalletError::parse("|x").code, "error");
        assert_eq!(WalletError::parse("a-very-long-code-that-is-not-one|x").code, "error");
        assert_eq!(WalletError::parse("busy|x").to_string(), "busy|x");
    }

    #[test]
    fn event_names_and_payloads() {
        let e = WalletEvent::LedgerPairing { passkey: "012345".into() };
        assert_eq!(e.name(), "ledger-pairing");
        assert_eq!(e.payload(), serde_json::json!({ "passkey": "012345" }));
    }
}
