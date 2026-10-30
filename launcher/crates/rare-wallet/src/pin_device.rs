//! A PIN account's device key: the Ed25519 member of the account's 1-of-2 multisig (the phone
//! passkey is the other), made from 12 recovery words so the same key can come back on a new
//! SuiPlay (same address). It is kept in `<wallet dir>/pin/device.json`, AES-256-GCM under the key
//! the PIN guard (`pin_guard`) releases only for the right PIN or a phone unlock; the public key is
//! the additional data, so a file can't be swapped for another device's.
//!
//! One device key per SuiPlay and one PIN for it: every PIN account here uses the same key (a
//! phone passkey replaced later makes a new account with the same device key).

use crate::keys::{self, Keypair, FLAG_ED25519};
use crate::pin_guard::{PinGuard, Status, Where};
use crate::err;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const FILE: &str = "device.json";
/// `<wallet dir>/pin`
pub const DIR: &str = "pin";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sealed {
    v: u32,
    /// base64 Ed25519 public key
    public_key: String,
    nonce: String,
    ct: String,
}

#[derive(Serialize, Deserialize)]
struct Inner {
    mnemonic: String,
}

impl Drop for Inner {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.mnemonic);
    }
}

pub struct PinDevice {
    dir: PathBuf,
    pub guard: PinGuard,
}

impl PinDevice {
    /// `<wallet dir>/pin` with the system's guard (the service, else in-process sealed).
    pub fn at(wallet_dir: &Path) -> Self {
        let dir = wallet_dir.join(DIR);
        PinDevice { guard: PinGuard::system(dir.clone()), dir }
    }

    pub fn new(dir: PathBuf, guard: PinGuard) -> Self {
        PinDevice { dir, guard }
    }

    fn sealed(&self) -> Option<Sealed> {
        std::fs::read(self.dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok())
    }

    /// The device member's public key (base64), when this SuiPlay has one.
    pub fn public_key(&self) -> Option<String> {
        self.sealed().map(|s| s.public_key)
    }

    pub fn status(&self) -> Result<Status, String> {
        self.guard.status()
    }

    pub fn place(&self) -> Where {
        self.guard.place()
    }

    /// A new device key behind a new PIN (`words`: restore an existing one; None: new words).
    /// A leftover record or file from an earlier key is replaced.
    pub fn create(&self, pin: &str, words: Option<&str>) -> Result<Keypair, String> {
        let mnemonic = match words {
            Some(w) => keys::normalize_mnemonic(w),
            None => keys::new_mnemonic(128),
        };
        if !keys::is_valid_mnemonic(&mnemonic) {
            return Err(err("error", "Those words are not a recovery phrase"));
        }
        let kp = Keypair::from_mnemonic(&mnemonic, 0).map_err(|e| err("error", e))?;
        if self.guard.status()?.exists {
            self.guard.wipe()?;
        }
        let key = self.guard.create(pin)?;
        let pk = kp.public_key_b64();
        let inner = Inner { mnemonic: mnemonic.to_string() };
        let plain = Zeroizing::new(serde_json::to_vec(&inner).expect("plain JSON"));
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let cipher = Aes256Gcm::new_from_slice(&key[..]).map_err(|_| err("error", "Could not seal the device key"))?;
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: &plain, aad: pk.as_bytes() }).map_err(|_| err("error", "Could not seal the device key"))?;
        let s = Sealed { v: 1, public_key: pk, nonce: B64.encode(nonce), ct: B64.encode(ct) };
        std::fs::create_dir_all(&self.dir).map_err(|e| err("error", format!("Could not make the wallet folder: {}", e.kind())))?;
        crate::vault::write_atomic(&self.dir.join(FILE), &serde_json::to_vec(&s).expect("plain JSON")).map_err(|e| err("error", format!("Could not save the device key: {}", e.kind())))?;
        log::info!("PIN device key made ({})", self.guard.place().describe());
        Ok(kp)
    }

    fn decrypt(&self, key: &[u8; 32]) -> Result<Zeroizing<String>, String> {
        let s = self.sealed().ok_or_else(|| err("missing", "This SuiPlay has no device key"))?;
        let nonce = B64.decode(&s.nonce).ok().filter(|n| n.len() == 12).ok_or_else(|| err("error", "The device key file is damaged"))?;
        let ct = B64.decode(&s.ct).map_err(|_| err("error", "The device key file is damaged"))?;
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| err("error", "The device key file is damaged"))?;
        let plain = Zeroizing::new(cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: s.public_key.as_bytes() }).map_err(|_| err("error", "This device key belongs to another PIN"))?);
        let inner: Inner = serde_json::from_slice(&plain).map_err(|_| err("error", "The device key file is damaged"))?;
        Ok(Zeroizing::new(inner.mnemonic.clone()))
    }

    fn keypair(&self, key: &[u8; 32]) -> Result<Keypair, String> {
        let words = self.decrypt(key)?;
        let kp = Keypair::from_mnemonic(&words, 0).map_err(|e| err("error", e))?;
        if kp.flag() != FLAG_ED25519 || Some(kp.public_key_b64()) != self.public_key() {
            return Err(err("error", "The device key file is damaged"));
        }
        Ok(kp)
    }

    /// The device key for one signature: the PIN, checked and counted by the guard.
    pub fn open(&self, pin: &str) -> Result<Keypair, String> {
        let key = self.guard.open(pin)?;
        self.keypair(&key)
    }

    /// The 12 recovery words (Settings, with the PIN).
    pub fn words(&self, pin: &str) -> Result<Zeroizing<String>, String> {
        let key = self.guard.open(pin)?;
        self.decrypt(&key)
    }

    pub fn change_pin(&self, pin: &str, new_pin: &str) -> Result<(), String> {
        self.guard.change(pin, new_pin)
    }

    /// The phone passkey (compressed P-256, base64) that may unlock this device after a lock.
    pub fn add_unlocker(&self, pin: &str, passkey_public_key: &str) -> Result<(), String> {
        self.guard.add_unlocker(pin, passkey_public_key)
    }

    /// After the phone unlocked the guard (`guard.unlock`): the new PIN, and the key for a signature.
    pub fn reset_pin(&self, token: &str, new_pin: &str) -> Result<Keypair, String> {
        let key = self.guard.reset_pin(token, new_pin)?;
        self.keypair(&key)
    }

    /// Forgets the device key and its PIN record.
    pub fn wipe(&self) {
        let _ = self.guard.wipe();
        let _ = std::fs::remove_file(self.dir.join(FILE));
        let _ = std::fs::remove_dir(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_open_words_and_wrong_file() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("pin");
        let dev = PinDevice::new(dir.clone(), PinGuard::local_for_tests(dir.clone()));
        assert!(dev.public_key().is_none());
        let kp = dev.create("123456", None).unwrap();
        assert_eq!(dev.public_key(), Some(kp.public_key_b64()));
        assert_eq!(dev.open("123456").unwrap().public_key(), kp.public_key());
        let words = dev.words("123456").unwrap();
        assert_eq!(words.split(' ').count(), 12);
        assert!(!std::fs::read_to_string(dir.join(FILE)).unwrap().contains(words.as_str()));
        assert!(dev.open("000000").unwrap_err().starts_with("wrong|"));
        // the same words on another device give the same key
        let d2 = tempfile::tempdir().unwrap();
        let dir2 = d2.path().join("pin");
        let dev2 = PinDevice::new(dir2.clone(), PinGuard::local_for_tests(dir2));
        assert_eq!(dev2.create("222222", Some(&words)).unwrap().public_key(), kp.public_key());
        // another device's file under this PIN does not open
        std::fs::copy(d2.path().join("pin").join(FILE), dir.join(FILE)).unwrap();
        assert!(dev.open("123456").is_err());
        dev.wipe();
        assert!(dev.public_key().is_none() && !dev.status().unwrap().exists);
    }
}
