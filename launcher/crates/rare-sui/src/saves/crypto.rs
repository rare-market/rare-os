//! Encryption for Rare cloud saves. Ports `src/sui/saves/crypto.ts`, byte for byte: a blob sealed
//! by the web launcher opens here and the other way round.
//!
//! The key comes from the player's own wallet: Rare Wallet's `deriveSecret("cloud-saves")` gives
//! 32 bytes that are the same for an account on any device, and HKDF-SHA256 over them gives the
//! AES-256-GCM key. Blob format: `"RSV1" | 12-byte nonce | ciphertext+tag`, with `"RSV1" + keyId`
//! as additional data so a blob can't be opened with another account's key by accident.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 4] = b"RSV1";

/// The deriveSecret context for cloud saves. Changing it changes every key.
pub const SAVES_CONTEXT: &str = "cloud-saves";
/// Overhead added by [`seal`], for cost estimates.
pub const SEAL_OVERHEAD: usize = 4 + 12 + 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveCryptoError {
    BadSecret,
    NotASave,
    /// "This backup was made with a different account".
    WrongKey,
}

impl std::fmt::Display for SaveCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SaveCryptoError::BadSecret => "Expected a 32-byte secret",
            SaveCryptoError::NotASave => "Not a Rare cloud save",
            SaveCryptoError::WrongKey => "This backup was made with a different account",
        })
    }
}
impl std::error::Error for SaveCryptoError {}

/// An account's save key. `id` (16 hex chars) identifies the key without revealing it and is
/// stored next to each backup.
#[derive(Clone)]
pub struct SaveKey {
    key: [u8; 32],
    pub id: String,
}

impl std::fmt::Debug for SaveKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveKey").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Drop for SaveKey {
    fn drop(&mut self) {
        self.key.fill(0);
    }
}

/// Derives the save key from the account's `deriveSecret(SAVES_CONTEXT)` bytes.
pub fn derive_save_key(secret: &[u8], address: &str) -> Result<SaveKey, SaveCryptoError> {
    if secret.len() != 32 {
        return Err(SaveCryptoError::BadSecret);
    }
    let mut okm = [0u8; 64];
    Hkdf::<Sha256>::new(Some(b"rare-saves-v1"), secret).expand(address.to_lowercase().as_bytes(), &mut okm).map_err(|_| SaveCryptoError::BadSecret)?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&okm[..32]);
    let digest = Sha256::digest(&okm[32..]);
    okm.fill(0);
    Ok(SaveKey { key, id: digest[..8].iter().map(|b| format!("{b:02x}")).collect() })
}

/// The Ed25519 seed of the account's cloud saves writer: an address only Rare's automatic uploads
/// use (the player funds it once), derived from the same `deriveSecret(SAVES_CONTEXT)` bytes, so
/// every device of the account finds the same writer and its backups.
pub fn writer_seed(secret: &[u8], address: &str) -> Result<[u8; 32], SaveCryptoError> {
    if secret.len() != 32 {
        return Err(SaveCryptoError::BadSecret);
    }
    let mut seed = [0u8; 32];
    Hkdf::<Sha256>::new(Some(b"rare-saves-writer-v1"), secret).expand(address.to_lowercase().as_bytes(), &mut seed).map_err(|_| SaveCryptoError::BadSecret)?;
    Ok(seed)
}

fn aad(id: &str) -> Vec<u8> {
    let mut padded: String = id.chars().take(16).collect();
    while padded.len() < 16 {
        padded.push('0');
    }
    [MAGIC.as_slice(), padded.as_bytes()].concat()
}

fn cipher(k: &SaveKey) -> Aes256Gcm {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&k.key))
}

fn seal_with(k: &SaveKey, nonce: &[u8; 12], plain: &[u8]) -> Vec<u8> {
    let ct = cipher(k).encrypt(Nonce::from_slice(nonce), Payload { msg: plain, aad: &aad(&k.id) }).expect("AES-GCM encrypt");
    [MAGIC.as_slice(), nonce.as_slice(), &ct].concat()
}

/// Encrypts with a fresh random nonce.
pub fn seal(k: &SaveKey, plain: &[u8]) -> Vec<u8> {
    let nonce: [u8; 12] = Aes256Gcm::generate_nonce(&mut OsRng).into();
    seal_with(k, &nonce, plain)
}

pub fn open(k: &SaveKey, data: &[u8]) -> Result<Vec<u8>, SaveCryptoError> {
    if data.len() < MAGIC.len() + 12 + 16 || &data[..4] != MAGIC {
        return Err(SaveCryptoError::NotASave);
    }
    let nonce = Nonce::from_slice(&data[4..16]);
    cipher(k).decrypt(nonce, Payload { msg: &data[16..], aad: &aad(&k.id) }).map_err(|_| SaveCryptoError::WrongKey)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
    fn secret(seed: u8) -> Vec<u8> {
        (0..32u8).map(|i| i.wrapping_add(seed)).collect()
    }

    /// Made by crypto.ts (WebCrypto + @noble/hashes) for secret 1..=32, address "0xABCDEF",
    /// nonce 100..=111 and plaintext "memory card".
    #[test]
    fn opens_what_the_web_launcher_sealed() {
        let k = derive_save_key(&secret(1), "0xABCDEF").unwrap();
        assert_eq!(k.id, "e3ce7f1427a78615");
        let sealed = unhex("525356316465666768696a6b6c6d6e6f4356a712f68610cc0f7980c747074cd49390952b8f11ba70bfddfb");
        assert_eq!(open(&k, &sealed).unwrap(), b"memory card");
        let nonce: [u8; 12] = core::array::from_fn(|i| 100 + i as u8);
        assert_eq!(seal_with(&k, &nonce, b"memory card"), sealed);
    }

    // crypto.test.ts "save keys"
    #[test]
    fn the_same_account_derives_the_same_key_on_any_device() {
        let (a, b) = (derive_save_key(&secret(7), "0xAb").unwrap(), derive_save_key(&secret(7), "0xab").unwrap());
        assert_eq!(a.id, b.id);
        assert_eq!(open(&b, &seal(&a, b"memory card")).unwrap(), b"memory card");
    }

    #[test]
    fn another_account_cant_open_it() {
        let a = derive_save_key(&secret(1), "0x1").unwrap();
        let b = derive_save_key(&secret(2), "0x1").unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(open(&b, &seal(&a, &[1, 2, 3])), Err(SaveCryptoError::WrongKey));
        assert_eq!(SaveCryptoError::WrongKey.to_string(), "This backup was made with a different account");
    }

    #[test]
    fn refuses_anything_but_a_32_byte_secret() {
        assert_eq!(derive_save_key(&[0u8; 16], "0x1").unwrap_err().to_string(), "Expected a 32-byte secret");
        let k = derive_save_key(&secret(1), "0x1").unwrap();
        assert_eq!(open(&k, b"RSV2xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"), Err(SaveCryptoError::NotASave));
    }

    #[test]
    fn ciphertext_never_contains_the_plaintext_and_each_seal_is_unique() {
        let k = derive_save_key(&secret(3), "0x1").unwrap();
        let plain = b"SAVEGAME-SAVEGAME-SAVEGAME";
        let (x, y) = (seal(&k, plain), seal(&k, plain));
        assert!(!x.windows(8).any(|w| w == b"SAVEGAME"));
        assert_ne!(x, y);
        assert_eq!(x.len(), plain.len() + SEAL_OVERHEAD);
    }
}
