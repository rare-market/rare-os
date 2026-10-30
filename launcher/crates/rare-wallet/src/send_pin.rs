//! The optional PIN for big sends (Settings > Accounts). Accounts in the device vault sign with A
//! alone; with this set, a transaction that sends more than the threshold asks for it first. It
//! does not encrypt anything (the device key does): it is a second check, Argon2id-hashed, with
//! the vault's wait after wrong tries.
//!
//! `<wallet dir>/send-pin.json`: `{"salt","hash","thresholdUsd","failures","until"}`.

use crate::vault::throttle_ms;
use crate::{err, err_data};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

const FILE: &str = "send-pin.json";
/// The default line: sends worth more than this many US dollars ask for the PIN.
pub const DEFAULT_THRESHOLD_USD: f64 = 50.0;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Stored {
    salt: String,
    hash: String,
    threshold_usd: f64,
    #[serde(default)]
    failures: u32,
    #[serde(default)]
    until: u64,
}

pub struct SendPin {
    dir: PathBuf,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn hash(pin: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
    let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|_| err("error", "Could not check the PIN"))?;
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params).hash_password_into(pin.as_bytes(), salt, out.as_mut()).map_err(|_| err("error", "Could not check the PIN"))?;
    Ok(out)
}

impl SendPin {
    pub fn new(dir: PathBuf) -> Self {
        SendPin { dir }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    fn load(&self) -> Option<Stored> {
        std::fs::read(self.path()).ok().and_then(|b| serde_json::from_slice(&b).ok())
    }

    fn save(&self, s: &Stored) -> Result<(), String> {
        let b = serde_json::to_vec(s).expect("plain JSON");
        crate::vault::write_atomic(&self.path(), &b).map_err(|e| err("error", format!("Could not save the PIN: {}", e.kind())))
    }

    pub fn is_set(&self) -> bool {
        self.load().is_some()
    }

    /// The line in US dollars, when the PIN is on.
    pub fn threshold_usd(&self) -> Option<f64> {
        self.load().map(|s| s.threshold_usd)
    }

    /// Whether a send of `usd` (None: its value is unknown) needs the PIN.
    pub fn needed_for(&self, usd: Option<f64>) -> bool {
        match self.threshold_usd() {
            None => false,
            Some(t) => usd.map_or(true, |v| v > t),
        }
    }

    /// Turns the PIN on (or changes it): 6 or more digits.
    pub fn set(&self, pin: &str, threshold_usd: f64) -> Result<(), String> {
        if pin.len() < 6 || !pin.bytes().all(|b| b.is_ascii_digit()) {
            return Err(err("error", "Use at least 6 digits"));
        }
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        let h = hash(pin, &salt)?;
        self.save(&Stored { salt: B64.encode(salt), hash: B64.encode(&h[..]), threshold_usd: threshold_usd.max(0.0), failures: 0, until: 0 })
    }

    /// Checks the PIN, with the vault's waits after wrong tries.
    pub fn check(&self, pin: &str) -> Result<(), String> {
        let Some(mut s) = self.load() else { return Ok(()) };
        let wait = s.until.saturating_sub(now_ms()).min(3_600_000);
        if wait > 0 {
            return Err(err_data("throttled", "Too many tries. Wait a moment.", json!({ "retryAfterMs": wait })));
        }
        let salt = B64.decode(&s.salt).map_err(|_| err("error", "The PIN record is damaged"))?;
        let want = B64.decode(&s.hash).map_err(|_| err("error", "The PIN record is damaged"))?;
        let got = hash(pin, &salt)?;
        // constant time enough for 32 bytes
        let same = want.len() == 32 && want.iter().zip(got.iter()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
        if same {
            if s.failures > 0 {
                s.failures = 0;
                s.until = 0;
                self.save(&s)?;
            }
            return Ok(());
        }
        s.failures = s.failures.saturating_add(1);
        let retry = throttle_ms(s.failures);
        s.until = if retry > 0 { now_ms() + retry } else { 0 };
        self.save(&s)?;
        Err(err_data("wrong", "Wrong PIN", json!({ "failures": s.failures, "retryAfterMs": retry })))
    }

    /// Turns the PIN off (needs the PIN).
    pub fn clear(&self, pin: &str) -> Result<(), String> {
        self.check(pin)?;
        self.remove();
        Ok(())
    }

    /// Removes the record (wipe).
    pub fn remove(&self) {
        let _ = std::fs::remove_file(self.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_check_clear() {
        let d = tempfile::tempdir().unwrap();
        let p = SendPin::new(d.path().to_path_buf());
        assert!(!p.is_set());
        assert!(!p.needed_for(None));
        assert!(p.set("12", 50.0).is_err());
        p.set("123456", 50.0).unwrap();
        assert!(p.needed_for(Some(80.0)) && p.needed_for(None) && !p.needed_for(Some(10.0)));
        assert!(p.check("123456").is_ok());
        let e = p.check("000000").unwrap_err();
        assert!(e.starts_with("wrong|Wrong PIN|"));
        assert!(!std::fs::read_to_string(d.path().join(FILE)).unwrap().contains("123456"));
        assert!(p.clear("000000").is_err());
        p.clear("123456").unwrap();
        assert!(!p.is_set());
    }
}
