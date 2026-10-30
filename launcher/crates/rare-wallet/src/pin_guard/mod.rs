//! The PIN guard: who checks the PIN of a PIN account's device key, counts wrong tries, locks
//! after five and lets a phone passkey unlock it (`rules` has them).
//!
//! Two places, best first:
//! - `Service`: the root service rare-pin-guard (Rare OS image, /run/rare-pin-guard.sock). Its
//!   record is sealed with systemd-creds (host key + TPM2) under /var/lib/rare-pin-guard, so the
//!   launcher's user can't read, edit, delete or roll back the count, and reinstalling the
//!   launcher changes nothing.
//! - `Local`: in this process, the record sealed by `systemd-creds --user` (TPM2) in the wallet
//!   folder (a 0600 file where that is missing: tests, a dev box). Editing it breaks the seal and
//!   deleting it deletes the device key with it (fail closed), but the user could put back an
//!   older copy; only the service stops that.
//!
//! Errors are the crate's "code|message|{json}" strings; secrets never reach them or the logs.

pub mod rules;

use self::rules::{Guard, Record, Request, Store};
use crate::{err, err_data};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

pub const SOCKET: &str = "/run/rare-pin-guard.sock";
const LOCAL_CRED: &str = "guard.cred";
const LOCAL_PLAIN: &str = "guard.json";
const CRED_NAME: &str = "rare-pin-guard";

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// What the guard says about this device's PIN.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub exists: bool,
    pub locked: bool,
    pub failures: u32,
    pub tries_left: u32,
    pub unlockers: u32,
}

/// Where the guard runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Service,
    Sealed,
    File,
}

impl Where {
    pub fn describe(self) -> &'static str {
        match self {
            Where::Service => "Checked by this SuiPlay's security chip",
            Where::Sealed => "Locked to this SuiPlay's security chip",
            Where::File => "Kept in a private file on this device",
        }
    }
}

/// The in-process record: sealed with systemd-creds --user, else a 0600 file.
pub struct LocalStore {
    dir: PathBuf,
    system: bool,
}

impl LocalStore {
    fn cred(&self) -> PathBuf {
        self.dir.join(LOCAL_CRED)
    }
    fn plain(&self) -> PathBuf {
        self.dir.join(LOCAL_PLAIN)
    }
}

impl Store for LocalStore {
    fn load(&self) -> Result<Option<Record>, String> {
        let raw = if self.cred().exists() {
            crate::device_seal::open_with(CRED_NAME, &self.cred())?
        } else if self.plain().exists() {
            Zeroizing::new(std::fs::read_to_string(self.plain()).map_err(|e| e.kind().to_string())?)
        } else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map(Some).map_err(|_| "damaged".to_string())
    }
    fn save(&self, r: &Record) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.kind().to_string())?;
        let raw = Zeroizing::new(serde_json::to_string(r).expect("plain JSON"));
        // a sealed record stays sealed; a new one is sealed when the system can
        if self.system && (self.cred().exists() || !self.plain().exists()) {
            let tmp = self.dir.join(".guard.cred.tmp");
            if crate::device_seal::seal_with(CRED_NAME, &raw, &tmp).is_ok() && std::fs::rename(&tmp, self.cred()).is_ok() {
                return Ok(());
            }
            let _ = std::fs::remove_file(&tmp);
            if self.cred().exists() {
                return Err("seal failed".into());
            }
        }
        crate::vault::write_atomic(&self.plain(), raw.as_bytes()).map_err(|e| e.kind().to_string())
    }
    fn remove(&self) -> Result<(), String> {
        let _ = std::fs::remove_file(self.cred());
        let _ = std::fs::remove_file(self.plain());
        Ok(())
    }
}

enum Backend {
    Service(PathBuf),
    Local(Mutex<Guard<LocalStore>>),
}

pub struct PinGuard {
    backend: Backend,
}

/// The guard's answer as Ok(json) or the crate's error string.
fn answer(v: Value) -> Result<Value, String> {
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        return Ok(v);
    }
    let code = v.get("code").and_then(|c| c.as_str()).unwrap_or("error").to_string();
    let message = v.get("message").and_then(|c| c.as_str()).unwrap_or("The PIN check failed").to_string();
    match code.as_str() {
        "wrong" => Err(err_data("wrong", &message, json!({ "failures": v.get("failures"), "triesLeft": v.get("triesLeft") }))),
        _ => Err(err(&code, message)),
    }
}

impl PinGuard {
    /// The service when it runs, else in-process in `dir` (`<wallet dir>/pin`).
    pub fn system(dir: PathBuf) -> Self {
        let sock = std::env::var_os("RARE_PIN_GUARD_SOCKET").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(SOCKET));
        if sock.exists() {
            return PinGuard { backend: Backend::Service(sock) };
        }
        PinGuard::local(dir, true)
    }

    /// In-process (`system`: seal with systemd-creds when it can; false in tests).
    pub fn local(dir: PathBuf, system: bool) -> Self {
        PinGuard { backend: Backend::Local(Mutex::new(Guard::new(LocalStore { dir, system }))) }
    }

    /// Tests: a cheap Argon2 cost.
    pub fn local_for_tests(dir: PathBuf) -> Self {
        let mut g = Guard::new(LocalStore { dir, system: false });
        g.kdf = rules::KDF_TEST;
        PinGuard { backend: Backend::Local(Mutex::new(g)) }
    }

    pub fn place(&self) -> Where {
        match &self.backend {
            Backend::Service(_) => Where::Service,
            Backend::Local(g) => {
                let g = g.lock().unwrap();
                if g.store.cred().exists() || (g.store.system && !g.store.plain().exists() && which_creds()) {
                    Where::Sealed
                } else {
                    Where::File
                }
            }
        }
    }

    fn call(&self, req: Value) -> Result<Value, String> {
        match &self.backend {
            Backend::Local(g) => {
                let r: Request = serde_json::from_value(req).map_err(|_| err("error", "Bad PIN request"))?;
                answer(g.lock().unwrap().handle(r, now_ms()))
            }
            Backend::Service(path) => answer(service_call(path, &req)?),
        }
    }

    pub fn status(&self) -> Result<Status, String> {
        let v = self.call(json!({ "op": "status" }))?;
        let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        Ok(Status { exists: v["exists"].as_bool() == Some(true), locked: v["locked"].as_bool() == Some(true), failures: n("failures"), tries_left: n("triesLeft"), unlockers: n("unlockers") })
    }

    fn key_of(v: &Value) -> Result<Zeroizing<[u8; 32]>, String> {
        use base64::{engine::general_purpose::STANDARD as B64, Engine};
        let b = Zeroizing::new(B64.decode(v["key"].as_str().unwrap_or_default()).map_err(|_| err("error", "The PIN check answered badly"))?);
        let mut k = Zeroizing::new([0u8; 32]);
        if b.len() != 32 {
            return Err(err("error", "The PIN check answered badly"));
        }
        k.copy_from_slice(&b);
        Ok(k)
    }

    /// A new PIN for this device: the device key file's AES key.
    pub fn create(&self, pin: &str) -> Result<Zeroizing<[u8; 32]>, String> {
        let v = Zeroizing::new(self.call(json!({ "op": "create", "pin": pin }))?.to_string());
        Self::key_of(&serde_json::from_str(&v).unwrap_or_default())
    }

    /// The right PIN gives the key; a wrong one counts ("wrong" with triesLeft, "pin-locked").
    pub fn open(&self, pin: &str) -> Result<Zeroizing<[u8; 32]>, String> {
        let v = Zeroizing::new(self.call(json!({ "op": "open", "pin": pin }))?.to_string());
        Self::key_of(&serde_json::from_str(&v).unwrap_or_default())
    }

    pub fn change(&self, pin: &str, new_pin: &str) -> Result<(), String> {
        self.call(json!({ "op": "change", "pin": pin, "newPin": new_pin })).map(|_| ())
    }

    /// Lets this phone passkey (compressed P-256, base64) unlock the device after a lock.
    pub fn add_unlocker(&self, pin: &str, public_key: &str) -> Result<(), String> {
        self.call(json!({ "op": "addUnlocker", "pin": pin, "publicKey": public_key })).map(|_| ())
    }

    /// A fresh 32-byte challenge for the phone to sign (base64).
    pub fn challenge(&self) -> Result<String, String> {
        Ok(self.call(json!({ "op": "challenge" }))?["challenge"].as_str().unwrap_or_default().to_string())
    }

    /// The phone's assertion over `challenge`: resets the count; answers a token for `reset_pin`.
    pub fn unlock(&self, challenge: &str, authenticator_data: &str, client_data_json: &str, signature: &str) -> Result<String, String> {
        let v = self.call(json!({ "op": "unlock", "challenge": challenge, "authenticatorData": authenticator_data, "clientDataJson": client_data_json, "signature": signature }))?;
        Ok(v["token"].as_str().unwrap_or_default().to_string())
    }

    /// The new PIN after a phone unlock; gives the key.
    pub fn reset_pin(&self, token: &str, new_pin: &str) -> Result<Zeroizing<[u8; 32]>, String> {
        let v = Zeroizing::new(self.call(json!({ "op": "resetPin", "token": token, "newPin": new_pin }))?.to_string());
        Self::key_of(&serde_json::from_str(&v).unwrap_or_default())
    }

    /// Forgets the record (and with it the device key for good).
    pub fn wipe(&self) -> Result<(), String> {
        self.call(json!({ "op": "wipe" })).map(|_| ())
    }
}

fn which_creds() -> bool {
    std::process::Command::new("systemd-creds").arg("--version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

/// One request line to the service, one answer line back.
#[cfg(unix)]
fn service_call(path: &Path, req: &Value) -> Result<Value, String> {
    let mut s = std::os::unix::net::UnixStream::connect(path).map_err(|_| err("error", "This SuiPlay's PIN check is not running"))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let line = Zeroizing::new(format!("{req}\n"));
    s.write_all(line.as_bytes()).map_err(|_| err("error", "Could not reach this SuiPlay's PIN check"))?;
    let mut out = Zeroizing::new(String::new());
    BufReader::new(&s).read_line(&mut out).map_err(|_| err("error", "This SuiPlay's PIN check did not answer"))?;
    serde_json::from_str(&out).map_err(|_| err("error", "This SuiPlay's PIN check answered badly"))
}

#[cfg(not(unix))]
fn service_call(_: &Path, _: &Value) -> Result<Value, String> {
    Err(err("error", "No PIN check here"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_guard_round_trip_and_lock() {
        let d = tempfile::tempdir().unwrap();
        let g = PinGuard::local_for_tests(d.path().join("pin"));
        assert!(!g.status().unwrap().exists);
        let k = g.create("123456").unwrap();
        assert_eq!(*g.open("123456").unwrap(), *k);
        for _ in 0..2 {
            assert!(g.open("000000").unwrap_err().starts_with("wrong|Wrong PIN|"));
        }
        let e = crate::WalletError::parse(&g.open("000000").unwrap_err());
        assert_eq!(e.message, "2 tries left. After 5 wrong PINs this device locks and you'll need your phone to unlock it.");
        assert_eq!(e.data.unwrap()["triesLeft"], 2);
        g.open("000000").unwrap_err();
        assert_eq!(g.open("000000").unwrap_err(), "pin-locked|Locked. Use your phone to unlock");
        assert!(g.status().unwrap().locked);
        // the count is on disk: a new guard (a restarted launcher) is still locked
        let g2 = PinGuard::local_for_tests(d.path().join("pin"));
        assert_eq!(g2.open("123456").unwrap_err(), "pin-locked|Locked. Use your phone to unlock");
        // the file never holds the PIN
        let raw = std::fs::read_to_string(d.path().join("pin").join(LOCAL_PLAIN)).unwrap();
        assert!(!raw.contains("123456"));
        g.wipe().unwrap();
        assert!(!g.status().unwrap().exists);
        assert_eq!(g.place(), Where::File);
    }
}
