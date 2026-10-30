//! Keys: recovery phrases (BIP-39, Sui path m/44'/784'/i'/0'/0' over SLIP-10 Ed25519),
//! imported private keys (`suiprivkey1…` of every scheme, or bare 32-byte hex Ed25519), Sui
//! addresses and signatures. Port of src/wallet/core/keys/{derive,signer}.ts; the output is
//! byte-identical to the TS wallet (see testdata/ts-vectors.json).
//!
//! Secret material lives in types that wipe themselves on drop (`Zeroizing`, and the signing
//! keys of ed25519-dalek, k256 and p256). Nothing here formats a secret into an error.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use bech32::{Bech32, Hrp};
use blake2::digest::consts::{U32, U8};
use blake2::{Blake2b, Digest};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::Sha512;
use sui_sdk_types::{Ed25519PublicKey, Secp256k1PublicKey, Secp256r1PublicKey};
use zeroize::Zeroizing;

/// Key errors, with the TS wallet's messages (the UI shows them as they are).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    InvalidPhrase,
    InvalidIndex,
    InvalidPrivateKey,
    InvalidPublicKey(&'static str),
    Signing,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KeyError::InvalidPhrase => "Invalid recovery phrase",
            KeyError::InvalidIndex => "Invalid account index",
            KeyError::InvalidPrivateKey => "Invalid private key",
            KeyError::InvalidPublicKey(m) => m,
            KeyError::Signing => "Could not sign",
        })
    }
}

impl std::error::Error for KeyError {}

// ---------------------------------------------------------------- phrases

/// Lowercase, single spaces, trimmed: what TS `normalizeMnemonic` does.
pub fn normalize_mnemonic(words: &str) -> Zeroizing<String> {
    Zeroizing::new(words.split_whitespace().map(|w| w.to_lowercase()).collect::<Vec<_>>().join(" "))
}

pub fn is_valid_mnemonic(words: &str) -> bool {
    bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &normalize_mnemonic(words)).is_ok()
}

/// A new phrase: 12 words (128 bits) by default, 24 with 256.
pub fn new_mnemonic(strength: u32) -> Zeroizing<String> {
    let mut entropy = Zeroizing::new(vec![0u8; if strength == 256 { 32 } else { 16 }]);
    OsRng.fill_bytes(&mut entropy);
    let m = bip39::Mnemonic::from_entropy_in(bip39::Language::English, &entropy).expect("16 or 32 bytes of entropy");
    Zeroizing::new(m.to_string())
}

/// The BIP-39 English word list (for the phrase entry keyboard).
pub fn wordlist() -> &'static [&'static str; 2048] {
    bip39::Language::English.word_list()
}

/// Sui standard path for account `index` (Slush, Nightly, Sui CLI and Ledger agree).
pub fn sui_path(index: u32) -> String {
    format!("m/44'/784'/{index}'/0'/0'")
}

/// Account index of a standard Sui path, or None for anything else.
pub fn path_index(path: &str) -> Option<u32> {
    let rest = path.trim().strip_prefix("m/44'/784'/")?.strip_suffix("'/0'/0'")?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u32>().ok().filter(|i| *i < 0x8000_0000)
}

/// Stable id for a phrase without storing it: blake2b-64 of the normalized phrase, hex.
pub fn phrase_id(words: &str) -> String {
    let mut h = Blake2b::<U8>::new();
    h.update(normalize_mnemonic(words).as_bytes());
    hex(&h.finalize())
}

/// SLIP-10 Ed25519 derivation of a hardened path from a BIP-39 seed.
fn slip10_ed25519(seed: &[u8], path: &[u32]) -> Zeroizing<[u8; 32]> {
    type H = Hmac<Sha512>;
    let mut mac = H::new_from_slice(b"ed25519 seed").expect("any key length");
    mac.update(seed);
    let mut i = Zeroizing::new([0u8; 64]);
    i.copy_from_slice(&mac.finalize().into_bytes());
    for &index in path {
        let mut data = Zeroizing::new([0u8; 37]);
        data[1..33].copy_from_slice(&i[..32]);
        data[33..].copy_from_slice(&(index | 0x8000_0000).to_be_bytes());
        let mut mac = H::new_from_slice(&i[32..]).expect("any key length");
        mac.update(&data[..]);
        i.copy_from_slice(&mac.finalize().into_bytes());
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&i[..32]);
    key
}

// ---------------------------------------------------------------- keypairs

/// A private key of any Sui scheme Rare Wallet holds.
pub enum Keypair {
    Ed25519(ed25519_dalek::SigningKey),
    Secp256k1(k256::ecdsa::SigningKey),
    Secp256r1(p256::ecdsa::SigningKey),
}

impl std::fmt::Debug for Keypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keypair({}, {})", self.scheme_name(), self.address())
    }
}

/// Sui signature scheme flags.
pub const FLAG_ED25519: u8 = 0x00;
pub const FLAG_SECP256K1: u8 = 0x01;
pub const FLAG_SECP256R1: u8 = 0x02;
pub const FLAG_MULTISIG: u8 = 0x03;
pub const FLAG_PASSKEY: u8 = 0x06;

impl Keypair {
    /// Account `index` of a recovery phrase (Ed25519, m/44'/784'/index'/0'/0').
    pub fn from_mnemonic(words: &str, index: u32) -> Result<Self, KeyError> {
        let m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &normalize_mnemonic(words)).map_err(|_| KeyError::InvalidPhrase)?;
        if index >= 0x8000_0000 {
            return Err(KeyError::InvalidIndex);
        }
        let seed = Zeroizing::new(m.to_seed_normalized(""));
        let key = slip10_ed25519(&seed[..], &[44, 784, index, 0, 0]);
        Ok(Keypair::Ed25519(ed25519_dalek::SigningKey::from_bytes(&key)))
    }

    /// 32 raw secret bytes of `scheme` ("ED25519", "Secp256k1", "Secp256r1" as TS names them).
    pub fn from_bytes(flag: u8, secret: &[u8]) -> Result<Self, KeyError> {
        let bytes: &[u8; 32] = secret.try_into().map_err(|_| KeyError::InvalidPrivateKey)?;
        Ok(match flag {
            FLAG_ED25519 => Keypair::Ed25519(ed25519_dalek::SigningKey::from_bytes(bytes)),
            FLAG_SECP256K1 => Keypair::Secp256k1(k256::ecdsa::SigningKey::from_slice(bytes).map_err(|_| KeyError::InvalidPrivateKey)?),
            FLAG_SECP256R1 => Keypair::Secp256r1(p256::ecdsa::SigningKey::from_slice(bytes).map_err(|_| KeyError::InvalidPrivateKey)?),
            _ => return Err(KeyError::InvalidPrivateKey),
        })
    }

    /// A private key as Sui wallets export it: bech32 `suiprivkey1…` (any scheme), or a bare
    /// 32-byte hex Ed25519 secret (older Sui Wallet exports).
    pub fn from_secret(key: &str) -> Result<Self, KeyError> {
        let k = key.trim();
        if k.starts_with("suiprivkey") {
            let (flag, secret) = decode_suiprivkey(k)?;
            return Self::from_bytes(flag, &secret[..]);
        }
        let h = k.strip_prefix("0x").or_else(|| k.strip_prefix("0X")).unwrap_or(k);
        if h.len() == 64 {
            let secret = Zeroizing::new(unhex(h).ok_or(KeyError::InvalidPrivateKey)?);
            return Self::from_bytes(FLAG_ED25519, &secret);
        }
        Err(KeyError::InvalidPrivateKey)
    }

    pub fn flag(&self) -> u8 {
        match self {
            Keypair::Ed25519(_) => FLAG_ED25519,
            Keypair::Secp256k1(_) => FLAG_SECP256K1,
            Keypair::Secp256r1(_) => FLAG_SECP256R1,
        }
    }

    /// The scheme as the TS SDK names it.
    pub fn scheme_name(&self) -> &'static str {
        match self {
            Keypair::Ed25519(_) => "ED25519",
            Keypair::Secp256k1(_) => "Secp256k1",
            Keypair::Secp256r1(_) => "Secp256r1",
        }
    }

    /// Raw public key: 32 bytes (Ed25519) or 33 compressed (secp).
    pub fn public_key(&self) -> Vec<u8> {
        match self {
            Keypair::Ed25519(k) => k.verifying_key().to_bytes().to_vec(),
            Keypair::Secp256k1(k) => k.verifying_key().to_encoded_point(true).as_bytes().to_vec(),
            Keypair::Secp256r1(k) => k.verifying_key().to_encoded_point(true).as_bytes().to_vec(),
        }
    }

    pub fn public_key_b64(&self) -> String {
        B64.encode(self.public_key())
    }

    /// The 0x… address: blake2b-256(flag || public key).
    pub fn address(&self) -> String {
        address_of(self.flag(), &self.public_key()).expect("valid public key")
    }

    /// The 32 secret bytes. Callers keep them in the returned `Zeroizing`.
    pub fn secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        let mut out = Zeroizing::new([0u8; 32]);
        match self {
            Keypair::Ed25519(k) => out.copy_from_slice(&k.to_bytes()),
            Keypair::Secp256k1(k) => out.copy_from_slice(&Zeroizing::new(k.to_bytes())[..]),
            Keypair::Secp256r1(k) => out.copy_from_slice(&Zeroizing::new(k.to_bytes())[..]),
        }
        out
    }

    /// `suiprivkey1…`, what TS `getSecretKey()` returns.
    pub fn to_suiprivkey(&self) -> Zeroizing<String> {
        let mut data = Zeroizing::new([0u8; 33]);
        data[0] = self.flag();
        data[1..].copy_from_slice(&self.secret_bytes()[..]);
        Zeroizing::new(bech32::encode::<Bech32>(Hrp::parse_unchecked("suiprivkey"), &data[..]).expect("33 bytes encode"))
    }

    /// Serialized Sui signature over a 32-byte intent digest: flag || signature || public key.
    /// ECDSA signs SHA-256(digest), RFC 6979 nonces and low-S like the TS SDK.
    pub fn sign_digest(&self, digest: &[u8; 32]) -> Vec<u8> {
        use ed25519_dalek::Signer as _;
        let sig: Vec<u8> = match self {
            Keypair::Ed25519(k) => k.sign(digest).to_bytes().to_vec(),
            Keypair::Secp256k1(k) => {
                let s: k256::ecdsa::Signature = k256::ecdsa::signature::Signer::sign(k, digest);
                s.normalize_s().unwrap_or(s).to_bytes().to_vec()
            }
            Keypair::Secp256r1(k) => {
                let s: p256::ecdsa::Signature = p256::ecdsa::signature::Signer::sign(k, digest);
                s.normalize_s().unwrap_or(s).to_bytes().to_vec()
            }
        };
        let mut out = Vec::with_capacity(1 + sig.len() + 33);
        out.push(self.flag());
        out.extend_from_slice(&sig);
        out.extend_from_slice(&self.public_key());
        out
    }

    /// Base64 serialized signature over transaction bytes (TS `signTransaction(...).signature`).
    pub fn sign_transaction(&self, tx_bytes: &[u8]) -> String {
        B64.encode(self.sign_digest(&tx_digest(tx_bytes)))
    }

    /// Base64 serialized signature over a personal message.
    pub fn sign_personal_message(&self, message: &[u8]) -> String {
        B64.encode(self.sign_digest(&personal_message_digest(message)))
    }
}

/// 0x… address of a raw public key for a scheme flag.
pub fn address_of(flag: u8, public_key: &[u8]) -> Result<String, KeyError> {
    let addr = match flag {
        FLAG_ED25519 => Ed25519PublicKey::new(public_key.try_into().map_err(|_| KeyError::InvalidPublicKey("Expected a 32-byte Ed25519 public key"))?).derive_address(),
        FLAG_SECP256K1 => Secp256k1PublicKey::new(public_key.try_into().map_err(|_| KeyError::InvalidPublicKey("Invalid public key"))?).derive_address(),
        FLAG_SECP256R1 => Secp256r1PublicKey::new(public_key.try_into().map_err(|_| KeyError::InvalidPublicKey("Invalid public key"))?).derive_address(),
        _ => return Err(KeyError::InvalidPublicKey("Invalid public key")),
    };
    Ok(addr.to_string())
}

/// Ed25519 public key (raw or base64) -> address; Ledger and Keystone accounts.
pub fn ed25519_address(public_key_b64: &str) -> Result<String, KeyError> {
    let raw = B64.decode(public_key_b64.trim()).map_err(|_| KeyError::InvalidPublicKey("Expected a 32-byte Ed25519 public key"))?;
    address_of(FLAG_ED25519, &raw)
}

/// `suiprivkey1…` -> (scheme flag, 32 secret bytes).
pub fn decode_suiprivkey(s: &str) -> Result<(u8, Zeroizing<Vec<u8>>), KeyError> {
    let (hrp, data) = bech32::decode(s.trim()).map_err(|_| KeyError::InvalidPrivateKey)?;
    let data = Zeroizing::new(data);
    if hrp.as_str() != "suiprivkey" || data.len() != 33 {
        return Err(KeyError::InvalidPrivateKey);
    }
    Ok((data[0], Zeroizing::new(data[1..].to_vec())))
}

// ---------------------------------------------------------------- intent digests

fn blake2b256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Blake2b::<U32>::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// blake2b-256(intent [0,0,0] || TransactionData bytes): what a transaction signature covers.
pub fn tx_digest(tx_bytes: &[u8]) -> [u8; 32] {
    blake2b256(&[&[0, 0, 0], tx_bytes])
}

/// blake2b-256(intent [3,0,0] || BCS vector<u8> of the message).
pub fn personal_message_digest(message: &[u8]) -> [u8; 32] {
    // BCS length prefix: ULEB128 of the length.
    let mut prefix = Vec::with_capacity(5);
    let mut n = message.len();
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            prefix.push(b);
            break;
        }
        prefix.push(b | 0x80);
    }
    blake2b256(&[&[3, 0, 0], &prefix, message])
}

// ---------------------------------------------------------------- small helpers

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// Normalized 0x + 64 hex, lowercase (TS normalizeSuiAddress).
pub fn normalize_address(a: &str) -> Option<String> {
    let t = a.trim();
    let h = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if h.is_empty() || h.len() > 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{:0>64}", h.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn same_phrase_same_address_index_changes_it() {
        let a = Keypair::from_mnemonic(PHRASE, 0).unwrap().address();
        assert_eq!(Keypair::from_mnemonic(&format!("  {} ", PHRASE.to_uppercase()), 0).unwrap().address(), a);
        // Sui CLI / Slush address for the all-abandon phrase, index 0
        assert_eq!(a, "0x5e93a736d04fbb25737aa40bee40171ef79f65fae833749e3c089fe7cc2161f1");
        assert_ne!(Keypair::from_mnemonic(PHRASE, 1).unwrap().address(), a);
        assert_eq!(Keypair::from_mnemonic("not a real phrase", 0).unwrap_err(), KeyError::InvalidPhrase);
        assert_eq!(Keypair::from_mnemonic(PHRASE, 0x8000_0000).unwrap_err(), KeyError::InvalidIndex);
    }

    #[test]
    fn new_phrases_are_valid_12_and_24_words() {
        let m = new_mnemonic(128);
        assert_eq!(m.split(' ').count(), 12);
        assert!(is_valid_mnemonic(&m));
        assert_eq!(new_mnemonic(256).split(' ').count(), 24);
        assert_ne!(*new_mnemonic(128), *m);
        assert_eq!(wordlist().len(), 2048);
    }

    #[test]
    fn paths() {
        assert_eq!(sui_path(3), "m/44'/784'/3'/0'/0'");
        assert_eq!(path_index(" m/44'/784'/3'/0'/0' "), Some(3));
        assert_eq!(path_index("m/44'/784'/2147483648'/0'/0'"), None);
        assert_eq!(path_index("m/44'/784'/x'/0'/0'"), None);
        assert_eq!(path_index("m/44'/784'/0'/0/0"), None);
        assert_eq!(path_index("m/44'/784'/'/0'/0'"), None);
    }

    #[test]
    fn private_key_import_rejects_junk() {
        assert_eq!(Keypair::from_secret("suiprivkey1nope").unwrap_err(), KeyError::InvalidPrivateKey);
        assert_eq!(Keypair::from_secret("0x1234").unwrap_err(), KeyError::InvalidPrivateKey);
        assert_eq!(Keypair::from_secret(&"zz".repeat(32)).unwrap_err(), KeyError::InvalidPrivateKey);
        let k = Keypair::from_mnemonic(PHRASE, 0).unwrap();
        let back = Keypair::from_secret(&k.to_suiprivkey()).unwrap();
        assert_eq!(back.address(), k.address());
        assert!(!format!("{k:?}").contains(&*k.to_suiprivkey()), "Debug never prints the secret");
    }

    #[test]
    fn addresses_normalize() {
        assert_eq!(normalize_address("0xAB").unwrap(), format!("0x{}ab", "0".repeat(62)));
        assert!(normalize_address("0x").is_none());
        assert!(normalize_address(&format!("0x{}", "1".repeat(65))).is_none());
        assert!(normalize_address("0xzz").is_none());
    }
}
