//! The keystore: accounts (public data, readable while locked) and their secrets (in the vault).
//! Port of src/wallet/core/keys/keystore.ts. The vault payload and the account list keep the TS
//! JSON shapes, so a wallet made by either front end opens in the other.
//!
//! Payload (sealed in the vault): `{"v":2,"phrases":{<phraseId>:<words>},"keys":{<address>:
//! {"kind":"key"|"device","secretKey":"suiprivkey1…"}}}`.
//! Account list (`AccountStore`): `{"accounts":[RareAccount…],"publicKeys":{<address>:<b64>},
//! "selected":<address>|null}`.

use crate::device_seal::{DeviceSeal, Seal};
use crate::pin_device::PinDevice;
use crate::keys::{self, KeyError, Keypair};
use crate::multisig::{self, MultisigDeviceSigner};
use crate::vault::{Kdf, Vault, VaultStatus};
use crate::WalletError;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use zeroize::{Zeroize, Zeroizing};

// ---------------------------------------------------------------- account types

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    /// recovery phrase or imported key in the vault
    Native,
    /// 1-of-2 Sui multisig of the person's passkey and a key kept on this device
    Passkey,
    /// reserved (zkLogin is not offered)
    Social,
    /// view only, no signing
    Watch,
    Ledger,
    Keystone,
}

impl AccountKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Native => "native",
            AccountKind::Passkey => "passkey",
            AccountKind::Social => "social",
            AccountKind::Watch => "watch",
            AccountKind::Ledger => "ledger",
            AccountKind::Keystone => "keystone",
        }
    }
    fn default_label(self) -> &'static str {
        match self {
            AccountKind::Native | AccountKind::Social => "Account",
            AccountKind::Passkey => "Passkey",
            AccountKind::Watch => "Watch",
            AccountKind::Ledger => "Ledger",
            AccountKind::Keystone => "Keystone",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hd {
    pub index: u32,
    pub phrase_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyInfo {
    /// base64 WebAuthn credential id
    pub credential_id: String,
    /// base64 compressed secp256r1 public key (33 bytes)
    pub public_key: String,
    /// base64 Ed25519 public key of the device member
    pub device_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authenticator: Option<String>,
    /// Whether this device holds the device key. Some(false): signed in from elsewhere, so every
    /// signature is the passkey's and never asks for the PIN. None: not recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_key: Option<bool>,
    /// Some(true): a PIN account. The device member is this SuiPlay's PIN device key
    /// (`pin_device`), unlocked by the PIN for each signature, never the PIN vault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_device: Option<bool>,
    /// More passkeys in the account (a security key beside the phone): members 2.. of the
    /// multisig, after the device key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backups: Vec<PasskeyMember>,
}

/// One more passkey of an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyMember {
    pub credential_id: String,
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authenticator: Option<String>,
}

impl PasskeyInfo {
    pub fn is_pin(&self) -> bool {
        self.pin_device == Some(true)
    }

    /// Every passkey of the account in committee order: (credential id, public key, authenticator).
    pub fn passkeys(&self) -> Vec<PasskeyMember> {
        let mut v = vec![PasskeyMember { credential_id: self.credential_id.clone(), public_key: self.public_key.clone(), authenticator: self.authenticator.clone() }];
        v.extend(self.backups.iter().cloned());
        v
    }

    /// The account's multisig committee.
    pub fn committee(&self) -> Result<sui_sdk_types::MultisigCommittee, multisig::MultisigError> {
        let mut keys: Vec<&str> = vec![self.public_key.as_str()];
        keys.extend(self.backups.iter().map(|b| b.public_key.as_str()));
        multisig::rare_committee_b64(&keys, &self.device_key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerInfo {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeystoneInfo {
    pub path: String,
    /// master fingerprint, 8 hex chars, lowercase
    pub mfp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

/// `RareAccount` in src/wallet/core/types.ts. Public data only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RareAccount {
    pub address: String,
    pub label: String,
    pub kind: AccountKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hd: Option<Hd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passkey: Option<PasskeyInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger: Option<LedgerInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keystone: Option<KeystoneInfo>,
}

impl RareAccount {
    fn new(address: String, label: String, kind: AccountKind) -> Self {
        RareAccount { address, label, kind, hd: None, passkey: None, vault_id: None, ledger: None, keystone: None }
    }
    /// True when the account can produce signatures.
    pub fn can_sign(&self) -> bool {
        self.kind != AccountKind::Watch
    }
}

/// The account list, `rare.meta` in the TS keystore.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(default)]
    pub accounts: Vec<RareAccount>,
    #[serde(default)]
    pub public_keys: BTreeMap<String, String>,
    #[serde(default)]
    pub selected: Option<String>,
}

// ---------------------------------------------------------------- the vault payload

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Secret {
    Phrase { mnemonic: String },
    Key {
        #[serde(rename = "secretKey")]
        secret_key: String,
    },
    Device {
        #[serde(rename = "secretKey")]
        secret_key: String,
    },
}

/// Never prints the secret.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Secret::Phrase { .. } => "Secret::Phrase(..)",
            Secret::Key { .. } => "Secret::Key(..)",
            Secret::Device { .. } => "Secret::Device(..)",
        })
    }
}

impl Zeroize for Secret {
    fn zeroize(&mut self) {
        match self {
            Secret::Phrase { mnemonic } => mnemonic.zeroize(),
            Secret::Key { secret_key } | Secret::Device { secret_key } => secret_key.zeroize(),
        }
    }
}

/// The vault's secret JSON. Wiped on drop.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    pub v: u32,
    #[serde(default)]
    pub phrases: BTreeMap<String, String>,
    #[serde(default)]
    pub keys: BTreeMap<String, Secret>,
    /// Other secrets sealed with the wallet (an AI provider's API key for Ask Rare), by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sealed: BTreeMap<String, String>,
}

impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Payload({} phrases, {} keys)", self.phrases.len(), self.keys.len())
    }
}

impl Default for Payload {
    fn default() -> Self {
        Payload { v: 2, phrases: BTreeMap::new(), keys: BTreeMap::new(), sealed: BTreeMap::new() }
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        self.phrases.values_mut().for_each(|s| s.zeroize());
        self.keys.values_mut().for_each(|s| s.zeroize());
        self.sealed.values_mut().for_each(|s| s.zeroize());
    }
}

impl Payload {
    pub fn parse(raw: &str) -> Result<Self, KeystoreError> {
        let mut p: Payload = serde_json::from_str(raw).map_err(|_| KeystoreError::Invalid("The wallet vault is damaged".into()))?;
        p.v = 2;
        Ok(p)
    }
    pub fn to_json(&self) -> Zeroizing<String> {
        Zeroizing::new(serde_json::to_string(self).expect("payload serializes"))
    }
}

// ---------------------------------------------------------------- errors

#[derive(Debug, Clone, PartialEq)]
pub enum KeystoreError {
    /// Adding the first local key to a watch/hardware-only wallet: a PIN must create the vault.
    NeedsPassword,
    Locked,
    WatchOnly,
    /// No signer here for this kind (Ledger, Keystone, a passkey without the device key).
    Unsupported(AccountKind),
    /// A vault error ("wrong", "throttled", "keyring", ...), as the vault reported it.
    Vault(WalletError),
    Key(KeyError),
    /// Anything else, with the message the UI shows.
    Invalid(String),
}

impl KeystoreError {
    pub fn code(&self) -> &str {
        match self {
            KeystoreError::NeedsPassword => "needs-password",
            KeystoreError::Locked => "locked",
            KeystoreError::WatchOnly => "watch-only",
            KeystoreError::Unsupported(_) => "unsupported",
            KeystoreError::Vault(e) => &e.code,
            KeystoreError::Key(_) | KeystoreError::Invalid(_) => "error",
        }
    }
    fn vault(s: String) -> Self {
        KeystoreError::Vault(WalletError::parse(&s))
    }
}

impl std::fmt::Display for KeystoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeystoreError::NeedsPassword => f.write_str("Set a PIN to add this wallet"),
            KeystoreError::Locked => f.write_str("Rare Wallet is locked"),
            KeystoreError::WatchOnly => f.write_str("Watch-only account cannot sign. Import it to sign."),
            KeystoreError::Unsupported(k) => write!(f, "Account kind \"{}\" is not available yet", k.as_str()),
            KeystoreError::Vault(e) => f.write_str(&e.message),
            KeystoreError::Key(e) => write!(f, "{e}"),
            KeystoreError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for KeystoreError {}

impl From<KeyError> for KeystoreError {
    fn from(e: KeyError) -> Self {
        KeystoreError::Key(e)
    }
}

fn invalid(m: &str) -> KeystoreError {
    KeystoreError::Invalid(m.to_string())
}

type R<T> = Result<T, KeystoreError>;

// ---------------------------------------------------------------- account storage

/// Where the account list lives. The Tauri launcher keeps it in the webview's localStorage
/// today; the native launcher in `<data dir>/rare/wallet/accounts.json`.
pub trait AccountStore: Send + Sync {
    fn load(&self) -> Result<Option<Meta>, String>;
    fn save(&self, meta: &Meta) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

#[derive(Default)]
pub struct MemoryStore(Mutex<Option<Meta>>);

impl AccountStore for MemoryStore {
    fn load(&self) -> Result<Option<Meta>, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, meta: &Meta) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(meta.clone());
        Ok(())
    }
    fn clear(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

/// `accounts.json` next to the vault, written 0600 through a temp file and rename.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub const FILE: &'static str = "accounts.json";
    pub fn new(dir: PathBuf) -> Self {
        FileStore { path: dir.join(Self::FILE) }
    }
}

impl AccountStore for FileStore {
    fn load(&self) -> Result<Option<Meta>, String> {
        match std::fs::read(&self.path) {
            Ok(b) => serde_json::from_slice(&b).map(Some).map_err(|_| crate::err("error", "The account list is damaged")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(crate::err("error", format!("Could not read the account list: {e}"))),
        }
    }
    fn save(&self, meta: &Meta) -> Result<(), String> {
        let bytes = serde_json::to_vec(meta).map_err(|_| crate::err("error", "Could not save the account list"))?;
        crate::vault::write_atomic(&self.path, &bytes).map_err(|e| crate::err("error", format!("Could not save the account list: {e}")))
    }
    fn clear(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(crate::err("error", format!("Could not remove the account list: {e}"))),
        }
    }
}

// ---------------------------------------------------------------- signers

/// What can sign for an account.
pub enum Signer {
    /// A phrase or imported key on this device.
    Native(Keypair),
    /// A passkey account through its device member.
    MultisigDevice(MultisigDeviceSigner),
    /// The key is elsewhere (Ledger, Keystone, the passkey on a phone): the caller runs that
    /// device's flow with the account and its public key.
    External { account: RareAccount, public_key: String },
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Signer::Native(k) => write!(f, "Signer::Native({})", k.address()),
            Signer::MultisigDevice(s) => write!(f, "Signer::MultisigDevice({})", s.address()),
            Signer::External { account, .. } => write!(f, "Signer::External({:?}, {})", account.kind, account.address),
        }
    }
}

impl Signer {
    pub fn address(&self) -> String {
        match self {
            Signer::Native(k) => k.address(),
            Signer::MultisigDevice(s) => s.address(),
            Signer::External { account, .. } => account.address.clone(),
        }
    }

    /// Base64 serialized signature over transaction bytes. External signers return
    /// `Unsupported`: their device flow signs.
    pub fn sign_transaction(&self, tx_bytes: &[u8]) -> R<String> {
        match self {
            Signer::Native(k) => Ok(k.sign_transaction(tx_bytes)),
            Signer::MultisigDevice(s) => s.sign_transaction(tx_bytes).map_err(|e| KeystoreError::Invalid(e.0)),
            Signer::External { account, .. } => Err(KeystoreError::Unsupported(account.kind)),
        }
    }

    /// (base64 message, base64 signature), TS `signPersonalMessage`.
    pub fn sign_personal_message(&self, message: &[u8]) -> R<(String, String)> {
        let sig = match self {
            Signer::Native(k) => k.sign_personal_message(message),
            Signer::MultisigDevice(s) => s.sign_personal_message(message).map_err(|e| KeystoreError::Invalid(e.0))?,
            Signer::External { account, .. } => return Err(KeystoreError::Unsupported(account.kind)),
        };
        Ok((B64.encode(message), sig))
    }
}

// ---------------------------------------------------------------- the keystore

/// PIN or passphrase for creating the vault with the first local key.
pub struct PinOptions<'a> {
    pub secret: &'a str,
    /// "pin" or "passphrase"
    pub kind: &'a str,
}

fn check_new_secret(secret: &str, kind: &str) -> R<()> {
    if kind == "pin" && !(secret.len() >= 6 && secret.bytes().all(|b| b.is_ascii_digit())) {
        return Err(invalid("Use at least 6 digits"));
    }
    if kind == "passphrase" && secret.encode_utf16().count() < 8 {
        return Err(invalid("Use at least 8 characters"));
    }
    Ok(())
}

/// Options for a passkey account.
pub struct PasskeyAccount<'a> {
    pub credential_id: &'a str,
    pub public_key: &'a str,
    pub device_key: &'a str,
    /// the device member's private key (`suiprivkey1…`) when this device holds it
    pub device_secret: Option<&'a str>,
    pub label: Option<&'a str>,
    pub authenticator: Option<&'a str>,
    pub pin: Option<PinOptions<'a>>,
    /// more passkeys of the account (members 2..), from rare.market's registry at sign in
    pub backups: &'a [PasskeyMember],
}

/// `vault_id` of accounts whose secret is in the device vault.
pub const DEVICE_VAULT: &str = "device";
/// The device vault's key is 256 random bits, so its KDF only has to be a formality.
const DEVICE_KDF: Kdf = Kdf { m: 8 * 1024, t: 1, p: 1 };

/// The second vault: recovery-phrase and imported-key accounts made with "Create account", sealed
/// by a key bound to this device (`device_seal`), so they open at login with no PIN.
pub struct DeviceVault {
    pub vault: Arc<Vault>,
    pub seal: DeviceSeal,
}

impl DeviceVault {
    /// `<wallet dir>/device`. `system`: use the TPM and the keyring (false in tests).
    pub fn at(wallet_dir: &std::path::Path, system: bool) -> Self {
        let dir = wallet_dir.join(DEVICE_VAULT);
        DeviceVault { vault: Arc::new(Vault::new(dir.clone(), false, DEVICE_KDF)), seal: DeviceSeal::new(dir, system) }
    }
}

/// A PIN account's device key, opened by the PIN for the next signature only.
struct Armed {
    address: String,
    key: Keypair,
    at: std::time::Instant,
}

/// An armed key is used within this long or dropped.
const ARMED_FOR: std::time::Duration = std::time::Duration::from_secs(120);

pub struct Keystore<S: AccountStore> {
    store: S,
    vault: Arc<Vault>,
    device: Option<DeviceVault>,
    pin: Option<PinDevice>,
    armed: Mutex<Option<Armed>>,
    /// Serializes read-modify-write of the account list and the payload.
    gate: Mutex<()>,
}

/// Whether the account's secret is in the device vault.
pub fn in_device_vault(a: &RareAccount) -> bool {
    a.vault_id.as_deref() == Some(DEVICE_VAULT)
}

impl<S: AccountStore> Keystore<S> {
    pub fn new(store: S, vault: Arc<Vault>) -> Self {
        Keystore { store, vault, device: None, pin: None, armed: Mutex::new(None), gate: Mutex::new(()) }
    }

    /// With the device vault (phrase accounts that need no PIN).
    pub fn with_device(mut self, device: DeviceVault) -> Self {
        self.device = Some(device);
        self
    }

    /// With this SuiPlay's PIN device key (PIN accounts).
    pub fn with_pin(mut self, pin: PinDevice) -> Self {
        self.pin = Some(pin);
        self
    }

    /// This SuiPlay's PIN device key, when PIN accounts are available here.
    pub fn pin_device(&self) -> Option<&PinDevice> {
        self.pin.as_ref()
    }

    fn pd(&self) -> R<&PinDevice> {
        self.pin.as_ref().ok_or_else(|| invalid("PIN accounts are not available here"))
    }

    /// Whether `address` is a PIN account whose device key is on this SuiPlay.
    pub fn is_pin_account(&self, address: &str) -> bool {
        let Some(pd) = &self.pin else { return false };
        let Ok(m) = self.meta() else { return false };
        m.accounts.iter().find(|a| a.address == address).and_then(|a| a.passkey.as_ref()).is_some_and(|p| p.is_pin() && pd.public_key().as_deref() == Some(p.device_key.as_str()))
    }

    /// PIN accounts that use this SuiPlay's device key.
    pub fn pin_accounts(&self) -> R<Vec<RareAccount>> {
        let pk = self.pin.as_ref().and_then(|p| p.public_key());
        Ok(self.meta()?.accounts.into_iter().filter(|a| a.passkey.as_ref().is_some_and(|p| p.is_pin() && Some(&p.device_key) == pk.as_ref())).collect())
    }

    /// A Rare passkey account signed in here from its backup becomes a PIN account again: its
    /// device key is this SuiPlay's PIN device key now (restored from the 12 words). Same address.
    pub fn adopt_pin(&self, address: &str) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let dk = self.pd()?.public_key().ok_or_else(|| invalid("Set a PIN first"))?;
        let mut m = self.meta()?;
        let acc = m.accounts.iter_mut().find(|a| a.address == address).ok_or_else(|| invalid("Unknown account"))?;
        let info = acc.passkey.as_mut().ok_or_else(|| invalid("Not a passkey account"))?;
        if info.device_key != dk {
            return Err(invalid("Those words belong to another account"));
        }
        info.local_key = Some(true);
        info.pin_device = Some(true);
        let out = acc.clone();
        self.set_meta(&m)?;
        Ok(out)
    }

    /// The 12 recovery words of this SuiPlay's device key, behind the PIN (counted like any PIN).
    pub fn pin_words(&self, pin: &str) -> R<Zeroizing<String>> {
        self.pd()?.words(pin).map_err(KeystoreError::vault)
    }

    /// Changes this SuiPlay's PIN (the current one is checked and counted).
    pub fn change_pin(&self, pin: &str, new_pin: &str) -> R<()> {
        self.pd()?.change_pin(pin, new_pin).map_err(KeystoreError::vault)
    }

    /// Opens the device key with the PIN for the next signature of `address` (the review sheet
    /// asks for the PIN, then approves). Wrong PINs count: "wrong" (triesLeft), "pin-locked".
    pub fn arm_pin(&self, address: &str, pin: &str) -> R<()> {
        let kp = self.pd()?.open(pin).map_err(KeystoreError::vault)?;
        self.arm_key(address, kp)
    }

    /// Arms a device key opened another way (a new PIN after the phone unlocked the device).
    pub fn arm_key(&self, address: &str, kp: Keypair) -> R<()> {
        let acc = self.meta()?.accounts.into_iter().find(|a| a.address == address).ok_or_else(|| invalid("Unknown account"))?;
        let info = acc.passkey.filter(|p| p.is_pin()).ok_or_else(|| invalid("Not a PIN account"))?;
        if kp.public_key_b64() != info.device_key {
            return Err(invalid("Device key does not match"));
        }
        *self.armed.lock().unwrap() = Some(Armed { address: address.to_string(), key: kp, at: std::time::Instant::now() });
        Ok(())
    }

    /// Drops an armed key (the sheet was cancelled).
    pub fn disarm(&self) {
        *self.armed.lock().unwrap() = None;
    }

    fn take_armed(&self, address: &str) -> Option<Keypair> {
        let mut a = self.armed.lock().unwrap();
        match a.take() {
            Some(x) if x.address == address && x.at.elapsed() < ARMED_FOR => Some(x.key),
            _ => None,
        }
    }

    /// Whether this keystore can make accounts that need no PIN.
    pub fn has_device_vault(&self) -> bool {
        self.device.is_some()
    }

    fn dv(&self) -> R<&DeviceVault> {
        self.device.as_ref().ok_or_else(|| invalid("Accounts without a PIN are not available here"))
    }

    /// How the device vault's key is kept (None: no device vault yet).
    pub fn device_seal(&self) -> Option<Seal> {
        self.device.as_ref().and_then(|d| d.seal.seal())
    }

    /// Opens the device vault with its sealed key: no prompt. Ok(false) when there is none.
    /// Slow on a TPM (about a second): call it off the UI thread, once at start.
    pub fn open_device(&self) -> R<bool> {
        let Some(d) = self.device.as_ref() else { return Ok(false) };
        if d.vault.read().is_some() {
            return Ok(true);
        }
        if !d.vault.status().map_err(KeystoreError::vault)?.exists {
            return Ok(false);
        }
        let key = d.seal.load().map_err(KeystoreError::vault)?.ok_or_else(|| invalid("This device's wallet key is missing"))?;
        let raw = d.vault.unlock(&key).map_err(KeystoreError::vault)?;
        Payload::parse(&raw).map(|_| true)
    }

    /// The device vault's payload, opened now if needed (and made when `create`).
    fn device_payload(&self, create: bool) -> R<Payload> {
        let d = self.dv()?;
        if let Some(p) = d.vault.read().and_then(|raw| Payload::parse(&raw).ok()) {
            return Ok(p);
        }
        if self.open_device()? {
            return d.vault.read().and_then(|raw| Payload::parse(&raw).ok()).ok_or(KeystoreError::Locked);
        }
        if !create {
            return Err(invalid("This device's wallet is missing"));
        }
        // a vault without its key (or the other way round) is a leftover: start clean
        let _ = d.vault.reset();
        let (key, _) = d.seal.create().map_err(KeystoreError::vault)?;
        let p = Payload::default();
        d.vault.create(&key, "device", &p.to_json()).map_err(KeystoreError::vault)?;
        Ok(p)
    }

    fn persist_device(&self, p: &Payload) -> R<()> {
        self.dv()?.vault.save(&p.to_json()).map_err(KeystoreError::vault)
    }

    /// The payload that holds this account's secret.
    fn payload_for(&self, account: &RareAccount) -> R<Payload> {
        if in_device_vault(account) {
            self.device_payload(false)
        } else {
            self.load().ok_or(KeystoreError::Locked)
        }
    }

    pub fn vault(&self) -> &Arc<Vault> {
        &self.vault
    }

    // ---- state
    pub fn status(&self) -> R<VaultStatus> {
        self.vault.status().map_err(KeystoreError::vault)
    }
    pub fn has_vault(&self) -> R<bool> {
        Ok(self.status()?.exists)
    }
    pub fn is_initialized(&self) -> R<bool> {
        Ok(self.has_vault()? || !self.meta()?.accounts.is_empty() || self.device_seal().is_some())
    }
    /// No vault (watch/hardware only) means nothing to unlock.
    pub fn is_unlocked(&self) -> R<bool> {
        Ok(!self.has_vault()? || self.load().is_some())
    }

    // ---- lifecycle
    pub fn unlock(&self, secret: &str) -> R<()> {
        let raw = self.vault.unlock(secret).map_err(KeystoreError::vault)?;
        Payload::parse(&raw).map(|_| ())
    }
    pub fn lock(&self) {
        self.vault.lock();
    }
    pub fn change_secret(&self, current: &str, next: &str, kind: &str) -> R<()> {
        check_new_secret(next, kind)?;
        self.vault.change_secret(current, next, kind).map_err(KeystoreError::vault)
    }
    /// Removes every account and the vault from this device.
    pub fn wipe(&self) -> R<()> {
        let _g = self.gate.lock().unwrap();
        self.vault.reset().map_err(KeystoreError::vault)?;
        if let Some(d) = &self.device {
            d.vault.reset().map_err(KeystoreError::vault)?;
            d.seal.remove();
        }
        if let Some(p) = &self.pin {
            p.wipe();
        }
        self.disarm();
        self.store.clear().map_err(KeystoreError::vault)
    }

    // ---- accounts (public data: readable while locked)
    pub fn meta(&self) -> R<Meta> {
        Ok(self.store.load().map_err(KeystoreError::vault)?.unwrap_or_default())
    }
    fn set_meta(&self, m: &Meta) -> R<()> {
        self.store.save(m).map_err(KeystoreError::vault)
    }
    pub fn list_accounts(&self) -> R<Vec<RareAccount>> {
        Ok(self.meta()?.accounts)
    }
    pub fn current(&self) -> R<Option<RareAccount>> {
        let m = self.meta()?;
        Ok(m.accounts.iter().find(|a| Some(&a.address) == m.selected.as_ref()).or(m.accounts.first()).cloned())
    }
    /// The selected account when it can sign, else None (market.ts currentSigningAccount: the
    /// Marketplace shows its QR fallback then).
    pub fn current_signing_account(&self) -> R<Option<RareAccount>> {
        Ok(self.current()?.filter(RareAccount::can_sign))
    }

    pub fn select(&self, address: &str) -> R<()> {
        let _g = self.gate.lock().unwrap();
        let mut m = self.meta()?;
        if !m.accounts.iter().any(|a| a.address == address) {
            return Err(invalid("Unknown account"));
        }
        m.selected = Some(address.to_string());
        self.set_meta(&m)
    }
    pub fn public_key(&self, address: &str) -> R<Option<String>> {
        Ok(self.meta()?.public_keys.get(address).cloned())
    }

    fn add_account(&self, account: RareAccount, public_key: Option<String>) -> R<RareAccount> {
        let mut m = self.meta()?;
        if m.accounts.iter().any(|a| a.address == account.address) {
            return Err(invalid("Account already exists"));
        }
        if let Some(pk) = public_key {
            m.public_keys.insert(account.address.clone(), pk);
        }
        if m.selected.is_none() {
            m.selected = Some(account.address.clone());
        }
        m.accounts.push(account.clone());
        self.set_meta(&m)?;
        Ok(account)
    }

    fn label(m: &Meta, kind: AccountKind, given: Option<&str>) -> String {
        match given.map(str::trim).filter(|s| !s.is_empty()) {
            Some(l) => l.to_string(),
            None => format!("{} {}", kind.default_label(), m.accounts.iter().filter(|a| a.kind == kind).count() + 1),
        }
    }

    fn exists(&self, address: &str) -> R<bool> {
        Ok(self.meta()?.accounts.iter().any(|a| a.address == address))
    }

    /// The unlocked payload for adding a local key. A wallet without a vault creates one now,
    /// which needs `pin`.
    fn vault_for(&self, pin: Option<&PinOptions<'_>>) -> R<Payload> {
        if self.has_vault()? {
            return self.load().ok_or(KeystoreError::Locked);
        }
        let pin = pin.ok_or(KeystoreError::NeedsPassword)?;
        check_new_secret(pin.secret, pin.kind)?;
        let p = Payload::default();
        self.vault.create(pin.secret, pin.kind, &p.to_json()).map_err(KeystoreError::vault)?;
        Ok(p)
    }

    // ---- phrase and key accounts (need unlock, or a PIN for the first one)
    /// First run or another phrase: account `index` (0 by default) of `words` (generated when None).
    pub fn add_phrase(&self, words: Option<&str>, label: Option<&str>, pin: Option<PinOptions<'_>>, index: Option<u32>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        self.add_phrase_locked(words, label, pin, index)
    }

    fn add_phrase_locked(&self, words: Option<&str>, label: Option<&str>, pin: Option<PinOptions<'_>>, index: Option<u32>) -> R<RareAccount> {
        let mnemonic = match words {
            Some(w) => keys::normalize_mnemonic(w),
            None => keys::new_mnemonic(128),
        };
        if !keys::is_valid_mnemonic(&mnemonic) {
            return Err(KeyError::InvalidPhrase.into());
        }
        let index = index.unwrap_or(0);
        let kp = Keypair::from_mnemonic(&mnemonic, index)?;
        let address = kp.address();
        if self.exists(&address)? {
            return Err(invalid("Account already exists"));
        }
        let mut p = self.vault_for(pin.as_ref())?;
        let id = keys::phrase_id(&mnemonic);
        p.phrases.insert(id.clone(), mnemonic.to_string());
        self.persist(&p)?;
        let m = self.meta()?;
        let mut acc = RareAccount::new(address, Self::label(&m, AccountKind::Native, label), AccountKind::Native);
        acc.hd = Some(Hd { index, phrase_id: id });
        self.add_account(acc, Some(kp.public_key_b64()))
    }

    /// "Add account": the next unused index of an existing phrase (the first phrase when None).
    pub fn derive_next(&self, phrase_id: Option<&str>, label: Option<&str>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        // A phrase in the device vault first: the next account needs no PIN either.
        if self.device.is_some() {
            if let Ok(dp) = self.device_payload(false) {
                let id = match phrase_id {
                    Some(id) => dp.phrases.contains_key(id).then(|| id.to_string()),
                    None => dp.phrases.keys().next().cloned(),
                };
                if let Some(id) = id {
                    let mnemonic = Zeroizing::new(dp.phrases[&id].clone());
                    let used: Vec<u32> = self.meta()?.accounts.iter().filter_map(|a| a.hd.as_ref().filter(|h| h.phrase_id == id).map(|h| h.index)).collect();
                    let mut index = 0;
                    while used.contains(&index) {
                        index += 1;
                    }
                    return self.add_phrase_device_locked(Some(&mnemonic), label, Some(index));
                }
            }
        }
        let p = self.load().ok_or(KeystoreError::Locked)?;
        let id = match phrase_id {
            Some(id) => Some(id.to_string()),
            None => p.phrases.keys().next().cloned(),
        };
        let mnemonic = id.as_ref().and_then(|id| p.phrases.get(id)).map(|w| Zeroizing::new(w.clone()));
        let (Some(id), Some(mnemonic)) = (id, mnemonic) else {
            return Err(invalid("No recovery phrase on this device. Create or import one first."));
        };
        let used: Vec<u32> = self.meta()?.accounts.iter().filter_map(|a| a.hd.as_ref().filter(|h| h.phrase_id == id).map(|h| h.index)).collect();
        let mut index = 0;
        while used.contains(&index) {
            index += 1;
        }
        self.add_phrase_locked(Some(&mnemonic), label, None, Some(index))
    }

    /// Imported private key (`suiprivkey1…` of any scheme, or 32-byte hex Ed25519).
    pub fn add_private_key(&self, key: &str, label: Option<&str>, pin: Option<PinOptions<'_>>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let kp = Keypair::from_secret(key)?;
        let address = kp.address();
        if self.exists(&address)? {
            return Err(invalid("Account already exists"));
        }
        let mut p = self.vault_for(pin.as_ref())?;
        p.keys.insert(address.clone(), Secret::Key { secret_key: kp.to_suiprivkey().to_string() });
        self.persist(&p)?;
        let m = self.meta()?;
        self.add_account(RareAccount::new(address, Self::label(&m, AccountKind::Native, label), AccountKind::Native), Some(kp.public_key_b64()))
    }

    /// "Create account" and "Import" on this device: a phrase account in the device vault, so it
    /// signs with no PIN (account `index`, 0 by default, of `words`, generated when None).
    pub fn add_phrase_device(&self, words: Option<&str>, label: Option<&str>, index: Option<u32>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        self.add_phrase_device_locked(words, label, index)
    }

    fn add_phrase_device_locked(&self, words: Option<&str>, label: Option<&str>, index: Option<u32>) -> R<RareAccount> {
        let mnemonic = match words {
            Some(w) => keys::normalize_mnemonic(w),
            None => keys::new_mnemonic(128),
        };
        if !keys::is_valid_mnemonic(&mnemonic) {
            return Err(KeyError::InvalidPhrase.into());
        }
        let index = index.unwrap_or(0);
        let kp = Keypair::from_mnemonic(&mnemonic, index)?;
        let address = kp.address();
        if self.exists(&address)? {
            return Err(invalid("Account already exists"));
        }
        let mut p = self.device_payload(true)?;
        let id = keys::phrase_id(&mnemonic);
        p.phrases.insert(id.clone(), mnemonic.to_string());
        self.persist_device(&p)?;
        let m = self.meta()?;
        let mut acc = RareAccount::new(address, Self::label(&m, AccountKind::Native, label), AccountKind::Native);
        acc.hd = Some(Hd { index, phrase_id: id });
        acc.vault_id = Some(DEVICE_VAULT.into());
        self.add_account(acc, Some(kp.public_key_b64()))
    }

    /// An imported private key in the device vault (no PIN).
    pub fn add_private_key_device(&self, key: &str, label: Option<&str>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let kp = Keypair::from_secret(key)?;
        let address = kp.address();
        if self.exists(&address)? {
            return Err(invalid("Account already exists"));
        }
        let mut p = self.device_payload(true)?;
        p.keys.insert(address.clone(), Secret::Key { secret_key: kp.to_suiprivkey().to_string() });
        self.persist_device(&p)?;
        let m = self.meta()?;
        let mut acc = RareAccount::new(address, Self::label(&m, AccountKind::Native, label), AccountKind::Native);
        acc.vault_id = Some(DEVICE_VAULT.into());
        self.add_account(acc, Some(kp.public_key_b64()))
    }

    /// Whether some account's phrase is in the device vault (Add account: the next one needs no PIN).
    pub fn has_device_phrase(&self) -> R<bool> {
        Ok(self.meta()?.accounts.iter().any(|a| in_device_vault(a) && a.hd.is_some()))
    }

    /// The recovery phrase of a device-vault account (Show recovery phrase). No PIN: the caller asks
    /// for the send PIN first when one is set.
    pub fn reveal_device_mnemonic(&self, address: &str) -> R<Zeroizing<String>> {
        let acc = self.meta()?.accounts.into_iter().find(|a| a.address == address && in_device_vault(a)).ok_or_else(|| invalid("No recovery phrase for this account"))?;
        let p = self.device_payload(false)?;
        acc.hd.and_then(|hd| p.phrases.get(&hd.phrase_id).map(|w| Zeroizing::new(w.clone()))).ok_or_else(|| invalid("No recovery phrase for this account"))
    }

    /// The private key of a device-vault account, as `suiprivkey1…`. No PIN (see above).
    pub fn reveal_device_private_key(&self, address: &str) -> R<Zeroizing<String>> {
        let acc = self.meta()?.accounts.into_iter().find(|a| a.address == address && in_device_vault(a)).ok_or_else(|| invalid("No private key for this account"))?;
        let p = self.device_payload(false)?;
        Ok(Self::keypair(&p, &acc)?.to_suiprivkey())
    }

    /// View-only account: the address and nothing else. Needs no unlock.
    pub fn add_watch(&self, address: &str, label: Option<&str>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let raw = address.trim();
        let ok = raw.len() > 2 && raw.len() <= 66 && raw.starts_with("0x") && raw[2..].bytes().all(|b| b.is_ascii_hexdigit());
        let address = keys::normalize_address(raw).filter(|_| ok).ok_or_else(|| invalid("Invalid Sui address"))?;
        let m = self.meta()?;
        self.add_account(RareAccount::new(address, Self::label(&m, AccountKind::Watch, label), AccountKind::Watch), None)
    }

    /// Ledger account: the device's Ed25519 public key (base64) and path. The key never leaves the device.
    pub fn add_ledger(&self, public_key: &str, path: &str, label: Option<&str>, device: Option<&str>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let address = keys::ed25519_address(public_key).map_err(|_| invalid("Invalid Ledger public key"))?;
        if keys::path_index(path).is_none() {
            return Err(invalid("Unsupported Ledger path"));
        }
        let m = self.meta()?;
        let mut acc = RareAccount::new(address, Self::label(&m, AccountKind::Ledger, label), AccountKind::Ledger);
        acc.ledger = Some(LedgerInfo { path: path.trim().to_string(), device: device.filter(|d| !d.is_empty()).map(|d| clip(d, 40)) });
        self.add_account(acc, Some(public_key.to_string()))
    }

    /// Keystone account (over USB): Ed25519 public key, path and master fingerprint.
    pub fn add_keystone(&self, public_key: &str, path: &str, mfp: &str, label: Option<&str>, device: Option<&str>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let address = keys::ed25519_address(public_key).map_err(|_| invalid("Invalid Keystone public key"))?;
        if keys::path_index(path).is_none() {
            return Err(invalid("Unsupported path"));
        }
        if mfp.len() != 8 || !mfp.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("Invalid master fingerprint"));
        }
        let m = self.meta()?;
        let mut acc = RareAccount::new(address, Self::label(&m, AccountKind::Keystone, label), AccountKind::Keystone);
        acc.keystone = Some(KeystoneInfo { path: path.trim().to_string(), mfp: mfp.to_ascii_lowercase(), device: device.filter(|d| !d.is_empty()).map(|d| clip(d, 40)) });
        self.add_account(acc, Some(public_key.to_string()))
    }

    /// Passkey account: a 1-of-2 multisig of the passkey and a device key. With the device
    /// secret this device signs behind the PIN; without it, every signature is a passkey tap.
    pub fn add_passkey(&self, o: PasskeyAccount<'_>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let mut keys = vec![o.public_key];
        keys.extend(o.backups.iter().map(|b| b.public_key.as_str()));
        let committee = multisig::rare_committee_b64(&keys, o.device_key).map_err(|e| KeystoreError::Invalid(e.0))?;
        let address = multisig::multisig_address(&committee);
        if self.exists(&address)? {
            return Err(invalid("Account already exists"));
        }
        if let Some(secret) = o.device_secret {
            let kp = Keypair::from_secret(secret)?;
            if kp.flag() != keys::FLAG_ED25519 || kp.public_key_b64() != o.device_key {
                return Err(invalid("Device key does not match"));
            }
            let mut p = self.vault_for(o.pin.as_ref())?;
            p.keys.insert(address.clone(), Secret::Device { secret_key: kp.to_suiprivkey().to_string() });
            self.persist(&p)?;
        }
        let m = self.meta()?;
        let mut acc = RareAccount::new(address, Self::label(&m, AccountKind::Passkey, o.label), AccountKind::Passkey);
        acc.passkey = Some(PasskeyInfo {
            credential_id: o.credential_id.to_string(),
            public_key: o.public_key.to_string(),
            device_key: o.device_key.to_string(),
            authenticator: o.authenticator.filter(|a| !a.is_empty()).map(str::to_string),
            local_key: Some(o.device_secret.is_some()),
            pin_device: None,
            backups: o.backups.to_vec(),
        });
        self.add_account(acc, Some(B64.encode(multisig::multisig_public_key_bytes(&committee))))
    }

    /// A PIN account: the 1-of-2 multisig of the phone passkey and this SuiPlay's PIN device key
    /// (made first with `pin_device().create`). Same address rule as every Rare passkey account.
    pub fn add_pin_account(&self, passkeys: &[PasskeyMember], label: Option<&str>) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let device_key = self.pd()?.public_key().ok_or_else(|| invalid("Set a PIN first"))?;
        let first = passkeys.first().ok_or_else(|| invalid("Add a phone or a security key"))?;
        let info = PasskeyInfo {
            credential_id: first.credential_id.clone(),
            public_key: first.public_key.clone(),
            device_key: device_key.clone(),
            authenticator: first.authenticator.clone().filter(|a| !a.is_empty()),
            local_key: Some(true),
            pin_device: Some(true),
            backups: passkeys[1..].to_vec(),
        };
        let committee = info.committee().map_err(|e| KeystoreError::Invalid(e.0))?;
        let address = multisig::multisig_address(&committee);
        let m = self.meta()?;
        if let Some(i) = m.accounts.iter().position(|a| a.address == address) {
            // the same account signed in from the phone before: it signs with the PIN now
            let mut m = m;
            let acc = &mut m.accounts[i];
            if let Some(p) = acc.passkey.as_mut() {
                p.local_key = Some(true);
                p.pin_device = Some(true);
            }
            let out = acc.clone();
            self.set_meta(&m)?;
            return Ok(out);
        }
        let mut acc = RareAccount::new(address, Self::label(&m, AccountKind::Passkey, label.or(Some("Rare account"))), AccountKind::Passkey);
        acc.passkey = Some(info);
        self.add_account(acc, Some(B64.encode(multisig::multisig_public_key_bytes(&committee))))
    }

    /// Whether this device holds the passkey account's device key (quick signing with the PIN).
    pub fn has_device_key(&self, address: &str) -> bool {
        if self.is_pin_account(address) {
            return true;
        }
        matches!(self.load().as_ref().and_then(|p| p.keys.get(address)), Some(Secret::Device { .. }))
    }

    pub fn rename(&self, address: &str, label: &str) -> R<RareAccount> {
        let _g = self.gate.lock().unwrap();
        let name = label.trim();
        if name.is_empty() {
            return Err(invalid("Name cannot be empty"));
        }
        let mut m = self.meta()?;
        let acc = m.accounts.iter_mut().find(|a| a.address == address).ok_or_else(|| invalid("Unknown account"))?;
        acc.label = clip(name, 40);
        let out = acc.clone();
        self.set_meta(&m)?;
        Ok(out)
    }

    /// Removes one account. Its secret goes too, unless other accounts share the phrase.
    pub fn remove(&self, address: &str) -> R<()> {
        let _g = self.gate.lock().unwrap();
        let m = self.meta()?;
        let Some(acc) = m.accounts.iter().find(|a| a.address == address).cloned() else { return Ok(()) };
        let rest: Vec<RareAccount> = m.accounts.iter().filter(|a| a.address != address).cloned().collect();
        let pin_acc = acc.passkey.as_ref().is_some_and(|p| p.is_pin());
        if matches!(acc.kind, AccountKind::Native | AccountKind::Passkey) && !pin_acc {
            let device = in_device_vault(&acc);
            let mut p = if device { self.device_payload(false)? } else { self.load().ok_or(KeystoreError::Locked)? };
            if let Some(mut s) = p.keys.remove(address) {
                s.zeroize();
            }
            if let Some(hd) = &acc.hd {
                if !rest.iter().any(|a| a.hd.as_ref().map(|h| &h.phrase_id) == Some(&hd.phrase_id)) {
                    if let Some(mut w) = p.phrases.remove(&hd.phrase_id) {
                        w.zeroize();
                    }
                }
            }
            if device {
                self.persist_device(&p)?;
            } else {
                self.persist(&p)?;
            }
        }
        let mut public_keys = m.public_keys.clone();
        public_keys.remove(address);
        let selected = if m.selected.as_deref() == Some(address) { rest.first().map(|a| a.address.clone()) } else { m.selected.clone() };
        let device_left = rest.iter().any(in_device_vault);
        let pin_left = rest.iter().any(|a| a.passkey.as_ref().is_some_and(|p| p.is_pin()));
        self.set_meta(&Meta { accounts: rest, public_keys, selected })?;
        // the last PIN account went: the device key and its PIN record go too
        if pin_acc && !pin_left {
            if let Some(p) = &self.pin {
                p.wipe();
            }
            self.disarm();
        }
        // the last device-vault account went: its vault and sealed key go too
        if in_device_vault(&acc) && !device_left {
            if let Some(d) = &self.device {
                let _ = d.vault.reset();
                d.seal.remove();
            }
        }
        Ok(())
    }

    // ---- signing and secrets
    /// Whether signing with this account needs the vault unlocked first.
    pub fn needs_unlock(&self, address: &str) -> R<bool> {
        let Some(acc) = self.meta()?.accounts.into_iter().find(|a| a.address == address) else { return Ok(false) };
        Ok(match acc.kind {
            // the device vault opens itself with the device key
            AccountKind::Native if in_device_vault(&acc) => false,
            AccountKind::Native => self.load().is_none(),
            // an account signed in from elsewhere has no device key here: the passkey signs, no PIN
            // a PIN account asks for its PIN on the review sheet for every signature, never the vault
            AccountKind::Passkey if acc.passkey.as_ref().is_some_and(|p| p.is_pin()) => false,
            AccountKind::Passkey => acc.passkey.as_ref().and_then(|p| p.local_key) != Some(false) && self.has_vault()? && self.load().is_none(),
            _ => false,
        })
    }

    pub fn get_signer(&self, address: &str) -> R<Signer> {
        let m = self.meta()?;
        let account = m.accounts.iter().find(|a| a.address == address).cloned().ok_or_else(|| invalid("Unknown account"))?;
        let public_key = m.public_keys.get(address).cloned().unwrap_or_default();
        match account.kind {
            AccountKind::Watch => Err(KeystoreError::WatchOnly),
            AccountKind::Native => {
                let p = self.payload_for(&account)?;
                Ok(Signer::Native(Self::keypair(&p, &account)?))
            }
            AccountKind::Passkey if account.passkey.as_ref().is_some_and(|p| p.is_pin()) => {
                // the key the PIN opened for this signature, else the phone passkey signs
                let info = account.passkey.as_ref().expect("pin account");
                if let Some(kp) = self.take_armed(address) {
                    let committee = info.committee().map_err(|e| KeystoreError::Invalid(e.0))?;
                    let dev = MultisigDeviceSigner::new(committee, kp).map_err(|e| KeystoreError::Invalid(e.0))?;
                    return Ok(Signer::MultisigDevice(dev));
                }
                Ok(Signer::External { account, public_key })
            }
            AccountKind::Passkey => {
                let p = self.load();
                if let Some(Secret::Device { secret_key }) = p.as_ref().and_then(|p| p.keys.get(address)) {
                    let info = account.passkey.as_ref().ok_or_else(|| invalid("Not a passkey account"))?;
                    let committee = info.committee().map_err(|e| KeystoreError::Invalid(e.0))?;
                    let dev = MultisigDeviceSigner::new(committee, Keypair::from_secret(secret_key)?).map_err(|e| KeystoreError::Invalid(e.0))?;
                    return Ok(Signer::MultisigDevice(dev));
                }
                // no device key here (or locked): sign with the passkey itself
                Ok(Signer::External { account, public_key })
            }
            AccountKind::Ledger | AccountKind::Keystone => Ok(Signer::External { account, public_key }),
            AccountKind::Social => Err(KeystoreError::Unsupported(AccountKind::Social)),
        }
    }

    fn keypair(p: &Payload, account: &RareAccount) -> R<Keypair> {
        if let Some(hd) = &account.hd {
            let words = p.phrases.get(&hd.phrase_id).ok_or_else(|| invalid("Missing key material"))?;
            return Ok(Keypair::from_mnemonic(words, hd.index)?);
        }
        match p.keys.get(&account.address) {
            Some(Secret::Key { secret_key }) => Ok(Keypair::from_secret(secret_key)?),
            _ => Err(invalid("Missing key material")),
        }
    }

    /// Asks for the PIN again; the only path that returns a phrase.
    pub fn reveal_mnemonic(&self, secret: &str, address: &str) -> R<Zeroizing<String>> {
        let p = Payload::parse(&self.vault.verify(secret).map_err(KeystoreError::vault)?)?;
        let acc = self.meta()?.accounts.into_iter().find(|a| a.address == address);
        acc.and_then(|a| a.hd).and_then(|hd| p.phrases.get(&hd.phrase_id).map(|w| Zeroizing::new(w.clone()))).ok_or_else(|| invalid("No recovery phrase for this account"))
    }

    /// Asks for the PIN again; the account's private key as `suiprivkey1…`.
    pub fn reveal_private_key(&self, secret: &str, address: &str) -> R<Zeroizing<String>> {
        let p = Payload::parse(&self.vault.verify(secret).map_err(KeystoreError::vault)?)?;
        let acc = self.meta()?.accounts.into_iter().find(|a| a.address == address && a.kind == AccountKind::Native).ok_or_else(|| invalid("No private key for this account"))?;
        Ok(Self::keypair(&p, &acc)?.to_suiprivkey())
    }

    /// 32 secret bytes for `info`, deterministic per account: HKDF-SHA256 over the account's
    /// private key. The key never leaves the keystore. Phrase and imported-key accounts only.
    pub fn derive_from_key(&self, address: &str, salt: &[u8], info: &[u8]) -> R<Zeroizing<[u8; 32]>> {
        let acc = self.meta()?.accounts.into_iter().find(|a| a.address == address && a.kind == AccountKind::Native).ok_or_else(|| invalid("No local key for this account"))?;
        let p = self.payload_for(&acc)?;
        let kp = Self::keypair(&p, &acc)?;
        let mut out = Zeroizing::new([0u8; 32]);
        hkdf::Hkdf::<sha2::Sha256>::new(Some(salt), &kp.secret_bytes()[..]).expand(info, &mut out[..]).expect("32 bytes");
        Ok(out)
    }

    /// Whether this account came from a recovery phrase (public metadata, works while locked).
    pub fn has_phrase(&self, address: &str) -> R<bool> {
        Ok(self.meta()?.accounts.iter().any(|a| a.address == address && a.hd.is_some()))
    }

    // ---- other sealed secrets (not keys: an API key Ask Rare uses)
    /// A secret sealed in the vault under `name`. Err(Locked) while the vault is locked; None when
    /// there is no vault or nothing is saved under that name.
    pub fn sealed(&self, name: &str) -> R<Option<Zeroizing<String>>> {
        if !self.has_vault()? {
            return Ok(None);
        }
        let p = self.load().ok_or(KeystoreError::Locked)?;
        Ok(p.sealed.get(name).map(|v| Zeroizing::new(v.clone())))
    }

    /// Seals `value` under `name` (None removes it). Needs the vault, unlocked.
    pub fn set_sealed(&self, name: &str, value: Option<&str>) -> R<()> {
        let _g = self.gate.lock().unwrap();
        if !self.has_vault()? {
            return Err(KeystoreError::NeedsPassword);
        }
        let mut p = self.load().ok_or(KeystoreError::Locked)?;
        match value {
            Some(v) => {
                p.sealed.insert(name.to_string(), v.to_string());
            }
            None => {
                p.sealed.remove(name);
            }
        }
        self.persist(&p)
    }

    // ---- internals
    /// The payload while the vault is unlocked (Rust keeps it, also across UI reloads).
    fn load(&self) -> Option<Payload> {
        self.vault.read().and_then(|raw| Payload::parse(&raw).ok())
    }

    fn persist(&self, p: &Payload) -> R<()> {
        self.vault.save(&p.to_json()).map_err(KeystoreError::vault)
    }
}

/// First `n` UTF-16 units, like JS `slice(0, n)` (never splitting a character).
fn clip(s: &str, n: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in s.chars() {
        units += c.len_utf16();
        if units > n {
            break;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests;
