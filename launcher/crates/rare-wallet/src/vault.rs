//! Encrypted key vault. The payload (the wallet's secret JSON) is sealed with AES-256-GCM under
//! a key from Argon2id(PIN or passphrase). The blob is always encrypted; it lives in the OS
//! keyring when one is usable without a prompt, else in a 0600 file. The derived key stays in
//! memory only while unlocked. Nothing here logs or formats secrets, and errors never echo input.
//!
//! Layout under `<data dir>/rare/wallet/`:
//! - `vault-meta.json`: non-secret: backend, PIN or passphrase, wrong attempts, lockout end.
//! - `vault.json`: the encrypted blob, file backend only.

use crate::{err, err_data};
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

const META_FILE: &str = "vault-meta.json";
const BLOB_FILE: &str = "vault.json";
/// Bound into every ciphertext so a blob can't be replayed into another format version.
const AAD: &[u8] = b"rare-wallet-vault/1";
/// The keyring item that holds the vault blob.
const VAULT_ITEM: &str = "vault";
/// Upper bound on Argon2 memory read from a blob, so a damaged blob can't exhaust RAM.
const MAX_KDF_KIB: u32 = 1 << 20;

/// Argon2id cost. Stored in each blob so it can change later without breaking old vaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kdf {
    /// memory in KiB
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

impl Kdf {
    pub const DEFAULT: Kdf = Kdf { m: 64 * 1024, t: 3, p: 1 };
}

#[derive(Serialize, Deserialize)]
struct KdfBlob {
    alg: String,
    m: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
struct Blob {
    v: u32,
    kdf: KdfBlob,
    nonce: String,
    ct: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Keyring,
    File,
}

#[derive(Clone, Serialize, Deserialize)]
struct Meta {
    backend: Backend,
    kind: String,
    failures: u32,
    /// unix ms until the next attempt is allowed
    until: u64,
}

struct Unlocked {
    key: Zeroizing<[u8; 32]>,
    kdf: Kdf,
    salt: [u8; 16],
    payload: Zeroizing<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub exists: bool,
    pub unlocked: bool,
    pub backend: Option<String>,
    pub kind: Option<String>,
    pub failures: u32,
    pub retry_after_ms: u64,
}

/// Wait before the next attempt after `n` wrong ones in a row: none for the first 4, then
/// 30 s doubling up to 1 h. Same schedule as `throttleMs` in src/wallet/webvault.ts.
pub fn throttle_ms(n: u32) -> u64 {
    if n < 5 {
        return 0;
    }
    let shift = (n - 5).min(20);
    (30_000u64 << shift).min(3_600_000)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn missing() -> String {
    err("missing", "No wallet on this device yet")
}
fn damaged() -> String {
    err("error", "The wallet vault is damaged")
}
fn keyring_locked() -> String {
    err("keyring", "Unlock your system keyring in desktop mode")
}
fn io_err(what: &str) -> impl Fn(std::io::Error) -> String + '_ {
    // io errors carry paths and OS text, never secrets.
    move |e| err("error", format!("Could not {what} the wallet vault: {e}"))
}

pub struct Vault {
    dir: PathBuf,
    /// false in tests: never touch the OS keyring
    use_keyring: bool,
    kdf: Kdf,
    mem: Mutex<Option<Unlocked>>,
    /// Serializes attempts and writes so the throttle can't be raced.
    gate: Mutex<()>,
}

impl Vault {
    pub fn new(dir: PathBuf, use_keyring: bool, kdf: Kdf) -> Self {
        Self { dir, use_keyring, kdf, mem: Mutex::new(None), gate: Mutex::new(()) }
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join(META_FILE)
    }
    fn blob_path(&self) -> PathBuf {
        self.dir.join(BLOB_FILE)
    }

    fn load_meta(&self) -> Result<Option<Meta>, String> {
        match fs::read(self.meta_path()) {
            Ok(b) => serde_json::from_slice(&b).map(Some).map_err(|_| damaged()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err("read")(e)),
        }
    }

    fn save_meta(&self, m: &Meta) -> Result<(), String> {
        let bytes = serde_json::to_vec(m).map_err(|_| damaged())?;
        write_atomic(&self.meta_path(), &bytes).map_err(io_err("write"))
    }

    fn read_blob(&self, meta: &Meta) -> Result<String, String> {
        match meta.backend {
            Backend::File => match fs::read_to_string(self.blob_path()) {
                Ok(s) => Ok(s),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(missing()),
                Err(e) => Err(io_err("read")(e)),
            },
            Backend::Keyring => {
                if !self.use_keyring || !keyring::available() {
                    return Err(keyring_locked());
                }
                match keyring::get(VAULT_ITEM) {
                    Ok(Some(s)) => Ok(s),
                    Ok(None) => Err(missing()),
                    Err(()) => Err(keyring_locked()),
                }
            }
        }
    }

    fn write_blob(&self, backend: Backend, blob: &str) -> Result<(), String> {
        match backend {
            Backend::File => write_atomic(&self.blob_path(), blob.as_bytes()).map_err(io_err("write")),
            Backend::Keyring => {
                if !self.use_keyring || !keyring::available() || keyring::set(VAULT_ITEM, blob).is_err() {
                    return Err(keyring_locked());
                }
                Ok(())
            }
        }
    }

    /// Keyring when it is usable right now without any prompt and reads back what we wrote,
    /// else the file.
    fn store_new(&self, blob: &str) -> Result<Backend, String> {
        if self.use_keyring && keyring::available() && keyring::set(VAULT_ITEM, blob).is_ok() && keyring::get(VAULT_ITEM) == Ok(Some(blob.to_string())) {
            let _ = fs::remove_file(self.blob_path());
            return Ok(Backend::Keyring);
        }
        self.write_blob(Backend::File, blob)?;
        Ok(Backend::File)
    }

    pub fn status(&self) -> Result<VaultStatus, String> {
        let meta = self.load_meta()?;
        let unlocked = meta.is_some() && self.mem.lock().unwrap().is_some();
        Ok(match meta {
            None => VaultStatus { exists: false, unlocked: false, backend: None, kind: None, failures: 0, retry_after_ms: 0 },
            Some(m) => VaultStatus {
                exists: true,
                unlocked,
                backend: Some(match m.backend {
                    Backend::Keyring => "keyring".into(),
                    Backend::File => "file".into(),
                }),
                kind: Some(m.kind.clone()),
                failures: m.failures,
                retry_after_ms: wait_ms(&m),
            },
        })
    }

    pub fn create(&self, secret: &str, kind: &str, payload: &str) -> Result<VaultStatus, String> {
        check_kind(kind)?;
        check_secret(secret)?;
        {
            let _g = self.gate.lock().unwrap();
            if self.load_meta()?.is_some() || self.blob_path().exists() {
                return Err(err("exists", "A wallet already exists on this device"));
            }
            let salt = random_salt();
            let key = derive(secret, self.kdf, &salt)?;
            let blob = seal(&key, self.kdf, &salt, payload)?;
            let backend = self.store_new(&blob)?;
            self.save_meta(&Meta { backend, kind: kind.into(), failures: 0, until: 0 })?;
            *self.mem.lock().unwrap() = Some(Unlocked { key, kdf: self.kdf, salt, payload: Zeroizing::new(payload.to_string()) });
        }
        self.status()
    }

    /// Checks a secret against the stored blob, applying the throttle. Caller holds `gate`.
    fn attempt(&self, secret: &str) -> Result<(Meta, Unlocked), String> {
        let mut meta = self.load_meta()?.ok_or_else(missing)?;
        let wait = wait_ms(&meta);
        if wait > 0 {
            return Err(err_data("throttled", "Too many tries. Wait a moment.", json!({ "retryAfterMs": wait })));
        }
        let raw = self.read_blob(&meta)?;
        let blob: Blob = serde_json::from_str(&raw).map_err(|_| damaged())?;
        let (kdf, salt) = blob_kdf(&blob)?;
        let key = derive(secret, kdf, &salt)?;
        match open(&key, &blob) {
            Ok(payload) => {
                if meta.failures > 0 || meta.until > 0 {
                    meta.failures = 0;
                    meta.until = 0;
                    self.save_meta(&meta)?;
                }
                Ok((meta, Unlocked { key, kdf, salt, payload }))
            }
            Err(()) => {
                meta.failures = meta.failures.saturating_add(1);
                let retry = throttle_ms(meta.failures);
                meta.until = if retry > 0 { now_ms() + retry } else { 0 };
                self.save_meta(&meta)?;
                let msg = if meta.kind == "pin" { "Wrong PIN" } else { "Wrong passphrase" };
                Err(err_data("wrong", msg, json!({ "failures": meta.failures, "retryAfterMs": retry })))
            }
        }
    }

    pub fn unlock(&self, secret: &str) -> Result<Zeroizing<String>, String> {
        let _g = self.gate.lock().unwrap();
        let (_, u) = self.attempt(secret)?;
        let out = Zeroizing::new(u.payload.to_string());
        *self.mem.lock().unwrap() = Some(u);
        Ok(out)
    }

    /// Checks the secret without changing the lock state and returns the payload.
    pub fn verify(&self, secret: &str) -> Result<Zeroizing<String>, String> {
        let _g = self.gate.lock().unwrap();
        let (_, u) = self.attempt(secret)?;
        Ok(u.payload)
    }

    pub fn read(&self) -> Option<Zeroizing<String>> {
        self.mem.lock().unwrap().as_ref().map(|u| Zeroizing::new(u.payload.to_string()))
    }

    /// Re-encrypts a new payload under the unlocked key with a fresh nonce.
    pub fn save(&self, payload: &str) -> Result<(), String> {
        let _g = self.gate.lock().unwrap();
        let meta = self.load_meta()?.ok_or_else(missing)?;
        let mut mem = self.mem.lock().unwrap();
        let Some(u) = mem.as_mut() else { return Err(err("locked", "Unlock Rare Wallet first")) };
        let blob = seal(&u.key, u.kdf, &u.salt, payload)?;
        self.write_blob(meta.backend, &blob)?;
        u.payload = Zeroizing::new(payload.to_string());
        Ok(())
    }

    pub fn lock(&self) {
        *self.mem.lock().unwrap() = None;
    }

    pub fn change_secret(&self, current: &str, next: &str, kind: &str) -> Result<(), String> {
        check_kind(kind)?;
        check_secret(next)?;
        let _g = self.gate.lock().unwrap();
        let (mut meta, old) = self.attempt(current)?;
        // New salt, and the current default cost (upgrades old vaults).
        let salt = random_salt();
        let key = derive(next, self.kdf, &salt)?;
        let blob = seal(&key, self.kdf, &salt, &old.payload)?;
        self.write_blob(meta.backend, &blob)?;
        meta.kind = kind.into();
        meta.failures = 0;
        meta.until = 0;
        self.save_meta(&meta)?;
        *self.mem.lock().unwrap() = Some(Unlocked { key, kdf: self.kdf, salt, payload: old.payload });
        Ok(())
    }

    /// Deletes the vault for good.
    pub fn reset(&self) -> Result<(), String> {
        let _g = self.gate.lock().unwrap();
        *self.mem.lock().unwrap() = None;
        let was_keyring = matches!(self.load_meta(), Ok(Some(Meta { backend: Backend::Keyring, .. })));
        if was_keyring && self.use_keyring && keyring::available() {
            let _ = keyring::delete(VAULT_ITEM);
        }
        for p in [self.blob_path(), self.meta_path(), self.blob_path().with_extension("json.tmp"), self.meta_path().with_extension("json.tmp")] {
            match fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_err("remove")(e)),
            }
        }
        Ok(())
    }
}

fn wait_ms(m: &Meta) -> u64 {
    // Capped so a clock moved backwards can't lock the wallet for longer than the max.
    m.until.saturating_sub(now_ms()).min(3_600_000)
}

fn check_kind(kind: &str) -> Result<(), String> {
    // "device": a random key sealed to this device (device_seal.rs), no PIN
    if kind == "pin" || kind == "passphrase" || kind == "device" {
        Ok(())
    } else {
        Err(err("error", "Unknown secret kind"))
    }
}

fn check_secret(secret: &str) -> Result<(), String> {
    if secret.is_empty() {
        Err(err("error", "Enter a PIN or passphrase"))
    } else {
        Ok(())
    }
}

fn random_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    OsRng.fill_bytes(&mut s);
    s
}

fn derive(secret: &str, kdf: Kdf, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
    if kdf.m > MAX_KDF_KIB {
        return Err(damaged());
    }
    let params = Params::new(kdf.m, kdf.t, kdf.p, Some(32)).map_err(|_| damaged())?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(secret.as_bytes(), salt, key.as_mut())
        .map_err(|_| damaged())?;
    Ok(key)
}

fn seal(key: &[u8; 32], kdf: Kdf, salt: &[u8; 16], payload: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| damaged())?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: payload.as_bytes(), aad: AAD })
        .map_err(|_| err("error", "Could not encrypt the wallet vault"))?;
    let blob = Blob {
        v: 1,
        kdf: KdfBlob { alg: "argon2id".into(), m: kdf.m, t: kdf.t, p: kdf.p, salt: B64.encode(salt) },
        nonce: B64.encode(nonce),
        ct: B64.encode(ct),
    };
    serde_json::to_string(&blob).map_err(|_| damaged())
}

fn blob_kdf(b: &Blob) -> Result<(Kdf, [u8; 16]), String> {
    if b.v != 1 || b.kdf.alg != "argon2id" {
        return Err(damaged());
    }
    let salt: [u8; 16] = B64.decode(&b.kdf.salt).ok().and_then(|s| s.try_into().ok()).ok_or_else(damaged)?;
    Ok((Kdf { m: b.kdf.m, t: b.kdf.t, p: b.kdf.p }, salt))
}

/// Decrypts; any failure (wrong key, tampered or truncated data) is the same `Err(())`.
fn open(key: &[u8; 32], b: &Blob) -> Result<Zeroizing<String>, ()> {
    let nonce: [u8; 12] = B64.decode(&b.nonce).ok().and_then(|n| n.try_into().ok()).ok_or(())?;
    let ct = B64.decode(&b.ct).map_err(|_| ())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ())?;
    let pt = Zeroizing::new(cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: AAD }).map_err(|_| ())?);
    let s = std::str::from_utf8(&pt).map_err(|_| ())?;
    Ok(Zeroizing::new(s.to_string()))
}

#[cfg(unix)]
fn make_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}
#[cfg(not(unix))]
fn make_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

/// Writes via a 0600 temp file and rename, so a crash never leaves a half-written vault.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    make_dir(dir)?;
    let tmp = path.with_extension("json.tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        let mut f = opts.open(&tmp)?;
        #[cfg(unix)]
        {
            // mode() only applies on creation; a leftover temp file may have other bits.
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

// ---------------------------------------------------------------- OS keyring

/// Linux: the Secret Service (KWallet on KDE) through dbus-secret-service, the backend the
/// keyring crate uses, but scoped to the default collection and only when it is already
/// unlocked, with a zero prompt timeout: a KWallet password dialog in the gamescope session
/// must never appear. Every call runs on a helper thread with a deadline.
#[cfg(target_os = "linux")]
pub(crate) mod keyring {
    use dbus_secret_service::{Collection, EncryptionType, SecretService};
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::Duration;

    const PROBE: Duration = Duration::from_secs(2);
    const OP: Duration = Duration::from_secs(5);

    fn run<T: Send + 'static>(limit: Duration, f: impl FnOnce() -> Option<T> + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(limit).ok().flatten()
    }

    fn connect() -> Option<SecretService> {
        SecretService::connect_with_max_prompt_timeout(EncryptionType::Dh, 0).ok()
    }

    /// The default collection, only if it exists and is unlocked. Reading the alias and the
    /// Locked property never prompts.
    fn default_unlocked(ss: &SecretService) -> Option<Collection<'_>> {
        let c = ss.get_default_collection().ok()?;
        if c.is_locked().ok()? {
            return None;
        }
        Some(c)
    }

    fn attrs(user: &str) -> HashMap<&'static str, &str> {
        HashMap::from([("service", "rare-wallet"), ("username", user), ("application", "rare-launcher")])
    }

    pub fn available() -> bool {
        run(PROBE, || {
            let ss = connect()?;
            default_unlocked(&ss).map(|_| ())
        })
        .is_some()
    }

    pub fn get(user: &str) -> Result<Option<String>, ()> {
        let user = user.to_string();
        run(OP, move || {
            let ss = connect()?;
            let c = default_unlocked(&ss)?;
            let items = c.search_items(attrs(&user)).ok()?;
            match items.first() {
                None => Some(None),
                Some(i) => {
                    if i.is_locked().ok()? {
                        return None;
                    }
                    i.get_secret().ok().and_then(|b| String::from_utf8(b).ok()).map(Some)
                }
            }
        })
        .ok_or(())
    }

    pub fn set(user: &str, blob: &str) -> Result<(), ()> {
        let (user, blob) = (user.to_string(), zeroize::Zeroizing::new(blob.to_string()));
        run(OP, move || {
            let ss = connect()?;
            let c = default_unlocked(&ss)?;
            let label = if user == "vault" { "Rare Wallet vault (encrypted)" } else { "Rare Wallet device key" };
            c.create_item(label, attrs(&user), blob.as_bytes(), true, "text/plain").ok().map(|_| ())
        })
        .ok_or(())
    }

    pub fn delete(user: &str) -> Result<(), ()> {
        let user = user.to_string();
        run(OP, move || {
            let ss = connect()?;
            let c = default_unlocked(&ss)?;
            for i in c.search_items(attrs(&user)).ok()? {
                i.delete().ok()?;
            }
            Some(())
        })
        .ok_or(())
    }
}

/// macOS (development): the login keychain.
#[cfg(target_os = "macos")]
pub(crate) mod keyring {
    use ::keyring::{Entry, Error};

    fn entry(user: &str) -> Option<Entry> {
        Entry::new("rare-wallet", user).ok()
    }
    pub fn available() -> bool {
        entry("vault").is_some()
    }
    pub fn get(user: &str) -> Result<Option<String>, ()> {
        match entry(user).ok_or(())?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(Error::NoEntry) => Ok(None),
            Err(_) => Err(()),
        }
    }
    pub fn set(user: &str, blob: &str) -> Result<(), ()> {
        entry(user).ok_or(())?.set_password(blob).map_err(|_| ())
    }
    pub fn delete(user: &str) -> Result<(), ()> {
        match entry(user).ok_or(())?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(_) => Err(()),
        }
    }
}

/// Elsewhere: file only.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) mod keyring {
    pub fn available() -> bool {
        false
    }
    pub fn get(_: &str) -> Result<Option<String>, ()> {
        Err(())
    }
    pub fn set(_: &str, _: &str) -> Result<(), ()> {
        Err(())
    }
    pub fn delete(_: &str) -> Result<(), ()> {
        Err(())
    }
}

// ---------------------------------------------------------------- shared state

/// The device's vault, shared by both front ends (Tauri commands and the native UI).
#[derive(Clone)]
pub struct VaultState(pub Arc<Vault>);

impl Default for VaultState {
    fn default() -> Self {
        Self(Arc::new(Vault::new(default_dir(), true, Kdf::DEFAULT)))
    }
}

/// `<data dir>/rare/wallet`, where the vault, its meta and the account list live.
pub fn default_dir() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rare").join("wallet")
}

impl VaultState {
    /// Runs vault work (Argon2, keyring D-Bus) on a blocking thread, off the async runtime.
    pub async fn blocking<T: Send + 'static>(&self, f: impl FnOnce(&Vault) -> Result<T, String> + Send + 'static) -> Result<T, String> {
        let v = self.0.clone();
        tokio::task::spawn_blocking(move || f(&v)).await.map_err(|_| err("error", "Vault task failed"))?
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: Kdf = Kdf { m: 1024, t: 1, p: 1 };

    fn vault(dir: &tempfile::TempDir) -> Vault {
        Vault::new(dir.path().join("rare/wallet"), false, FAST)
    }
    fn code(e: &str) -> &str {
        e.split('|').next().unwrap()
    }
    fn data(e: &str) -> serde_json::Value {
        serde_json::from_str(&e[e.find("|{").unwrap() + 1..]).unwrap()
    }
    fn blob(v: &Vault) -> Blob {
        serde_json::from_str(&fs::read_to_string(v.blob_path()).unwrap()).unwrap()
    }

    #[test]
    fn roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        assert!(!v.status().unwrap().exists);
        let s = v.create("1234", "pin", r#"{"p":"secret"}"#).unwrap();
        assert!(s.exists && s.unlocked);
        assert_eq!(s.backend.as_deref(), Some("file"));
        assert_eq!(s.kind.as_deref(), Some("pin"));
        assert_eq!(v.read().unwrap().as_str(), r#"{"p":"secret"}"#);
        v.lock();
        assert!(v.read().is_none());
        assert!(!v.status().unwrap().unlocked);
        assert_eq!(v.unlock("1234").unwrap().as_str(), r#"{"p":"secret"}"#);
        assert!(v.status().unwrap().unlocked);
        // a fresh process reads the same vault
        let v2 = vault(&d);
        assert_eq!(v2.unlock("1234").unwrap().as_str(), r#"{"p":"secret"}"#);
    }

    #[test]
    fn plaintext_not_on_disk() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "word1 word2 word3").unwrap();
        let raw = fs::read_to_string(v.blob_path()).unwrap();
        assert!(!raw.contains("word1"));
        assert!(!fs::read_to_string(v.meta_path()).unwrap().contains("word1"));
    }

    #[test]
    fn wrong_secret() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "x").unwrap();
        v.lock();
        let e = v.unlock("9999").unwrap_err();
        assert_eq!(code(&e), "wrong");
        assert!(e.starts_with("wrong|Wrong PIN|"));
        assert!(!e.contains("9999"));
        assert_eq!(data(&e), json!({ "failures": 1, "retryAfterMs": 0 }));
        assert!(!v.status().unwrap().unlocked);
        assert_eq!(v.status().unwrap().failures, 1);
        v.unlock("1234").unwrap();
        assert_eq!(v.status().unwrap().failures, 0);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "payload").unwrap();
        v.lock();
        let mut b = blob(&v);
        let mut ct = B64.decode(&b.ct).unwrap();
        ct[0] ^= 1;
        b.ct = B64.encode(ct);
        fs::write(v.blob_path(), serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(code(&v.unlock("1234").unwrap_err()), "wrong");
    }

    #[test]
    fn nonce_differs_between_saves() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        let b1 = blob(&v);
        v.save("a").unwrap();
        let b2 = blob(&v);
        v.save("b").unwrap();
        let b3 = blob(&v);
        assert_ne!(b1.nonce, b2.nonce);
        assert_ne!(b2.nonce, b3.nonce);
        assert_ne!(b1.ct, b2.ct);
        assert_eq!(b1.kdf.salt, b3.kdf.salt);
        v.lock();
        assert_eq!(v.unlock("1234").unwrap().as_str(), "b");
    }

    #[test]
    fn save_needs_unlock() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        v.lock();
        assert_eq!(code(&v.save("b").unwrap_err()), "locked");
    }

    #[test]
    fn create_twice_and_missing() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        assert_eq!(code(&v.unlock("1234").unwrap_err()), "missing");
        v.create("1234", "pin", "a").unwrap();
        assert_eq!(code(&v.create("1234", "pin", "b").unwrap_err()), "exists");
    }

    #[test]
    fn verify_keeps_lock_state() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        v.lock();
        assert_eq!(v.verify("1234").unwrap().as_str(), "a");
        assert!(!v.status().unwrap().unlocked);
        assert_eq!(code(&v.verify("0000").unwrap_err()), "wrong");
    }

    #[test]
    fn change_secret() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "keep me").unwrap();
        let salt1 = blob(&v).kdf.salt;
        assert_eq!(code(&v.change_secret("0000", "long passphrase", "passphrase").unwrap_err()), "wrong");
        v.change_secret("1234", "long passphrase", "passphrase").unwrap();
        assert_ne!(blob(&v).kdf.salt, salt1);
        assert_eq!(v.status().unwrap().kind.as_deref(), Some("passphrase"));
        v.lock();
        let e = v.unlock("1234").unwrap_err();
        assert!(e.starts_with("wrong|Wrong passphrase|"));
        assert_eq!(v.unlock("long passphrase").unwrap().as_str(), "keep me");
    }

    #[test]
    fn throttle_schedule() {
        let got: Vec<u64> = (0..=13).map(throttle_ms).collect();
        assert_eq!(got, vec![0, 0, 0, 0, 0, 30_000, 60_000, 120_000, 240_000, 480_000, 960_000, 1_920_000, 3_600_000, 3_600_000]);
        assert_eq!(throttle_ms(u32::MAX), 3_600_000);
    }

    #[test]
    fn throttle_applies_and_resets() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        v.lock();
        for i in 1..=4 {
            assert_eq!(data(&v.unlock("0000").unwrap_err()), json!({ "failures": i, "retryAfterMs": 0 }));
        }
        let e = v.unlock("0000").unwrap_err();
        assert_eq!(data(&e), json!({ "failures": 5, "retryAfterMs": 30_000 }));
        // even the right PIN waits
        let e = v.unlock("1234").unwrap_err();
        assert_eq!(code(&e), "throttled");
        assert!(data(&e)["retryAfterMs"].as_u64().unwrap() > 25_000);
        let st = v.status().unwrap();
        assert_eq!(st.failures, 5);
        assert!(st.retry_after_ms > 25_000);
        // time passes
        let mut m = v.load_meta().unwrap().unwrap();
        m.until = now_ms() - 1;
        v.save_meta(&m).unwrap();
        assert_eq!(data(&v.unlock("0000").unwrap_err())["retryAfterMs"], 60_000);
        let mut m = v.load_meta().unwrap().unwrap();
        m.until = 0;
        v.save_meta(&m).unwrap();
        v.unlock("1234").unwrap();
        let st = v.status().unwrap();
        assert_eq!((st.failures, st.retry_after_ms), (0, 0));
    }

    #[test]
    fn kdf_params_live_in_blob() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        assert_eq!(blob(&v).kdf.m, FAST.m);
        // A build with different defaults still opens the old vault.
        let v2 = Vault::new(d.path().join("rare/wallet"), false, Kdf { m: 2048, t: 2, p: 1 });
        assert_eq!(v2.unlock("1234").unwrap().as_str(), "a");
        // and change_secret moves it to the new defaults
        v2.change_secret("1234", "5678", "pin").unwrap();
        assert_eq!((blob(&v2).kdf.m, blob(&v2).kdf.t), (2048, 2));
    }

    #[test]
    fn default_kdf_works() {
        assert_eq!(Kdf::DEFAULT, Kdf { m: 65536, t: 3, p: 1 });
        let d = tempfile::tempdir().unwrap();
        let v = Vault::new(d.path().to_path_buf(), false, Kdf::DEFAULT);
        v.create("1234", "pin", "a").unwrap();
        v.lock();
        assert_eq!(v.unlock("1234").unwrap().as_str(), "a");
    }

    #[test]
    fn reset_removes_everything() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        v.reset().unwrap();
        assert!(!v.blob_path().exists() && !v.meta_path().exists());
        assert!(!v.status().unwrap().exists);
        assert!(v.read().is_none());
        v.create("5678", "pin", "b").unwrap();
    }

    #[test]
    fn keyring_meta_without_keyring_reports_keyring() {
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        let mut m = v.load_meta().unwrap().unwrap();
        m.backend = Backend::Keyring;
        v.save_meta(&m).unwrap();
        let e = v.unlock("1234").unwrap_err();
        assert_eq!(e, "keyring|Unlock your system keyring in desktop mode");
    }

    #[cfg(unix)]
    #[test]
    fn files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let v = vault(&d);
        v.create("1234", "pin", "a").unwrap();
        v.save("b").unwrap();
        for p in [v.blob_path(), v.meta_path()] {
            assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600, "{p:?}");
        }
        assert_eq!(fs::metadata(&v.dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(!v.blob_path().with_extension("json.tmp").exists());
    }
}
