//! Deterministic secrets from a passkey: the WebAuthn PRF extension, carried over CTAP2 the way
//! Chromium does it, so rare.market in a browser derives the same secret from the same input.
//!
//! Naming:
//! - `prf_input`: the WebAuthn PRF input (`prf.eval.first`). Rare passes SHA-256("rare/" + context).
//! - `hmac_salt`: what the authenticator HMACs, SHA-256("WebAuthn PRF" || 0x00 || prf_input)
//!   (Chromium device/fido/prf_input.cc HashPRFValue). Both CTAP forms below take this salt.
//!
//! Two CTAP forms, picked from the authenticator's getInfo extensions like Chromium does
//! (device/fido/make_credential_request_handler.cc, get_assertion_request_handler.cc):
//! - "prf" (phones over hybrid, CTAP 2.2 keys): makeCredential sends {"prf": {}}, the answer
//!   is unsigned extension output 0x06 {"prf": {"enabled": true}}. getAssertion sends
//!   {"prf": {"eval": {"first": hmac_salt}}}, the output is unsigned extension output 0x08
//!   {"prf": {"results": {"first": 32 bytes}}}, in the clear (the tunnel is encrypted).
//! - "hmac-secret" (USB security keys): makeCredential sends {"hmac-secret": true}, authData
//!   extensions answer {"hmac-secret": true}. getAssertion sends {"hmac-secret": {1: platform
//!   key, 2: saltEnc, 3: saltAuth, 4: pinUvAuthProtocol}} under a fresh key agreement, and
//!   authData extensions carry the encrypted output. Only offered when the authenticator has a
//!   PIN/UV auth protocol, as in Chromium (fido_device_authenticator.cc).

use std::collections::BTreeMap;

use openssl::bn::{BigNum, BigNumContext};
use openssl::derive::Deriver;
use openssl::ec::{EcGroup, EcKey, PointConversionForm};
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::sign::Signer;
use openssl::symm::{Cipher, Crypter, Mode as CipherMode};
use serde_cbor_2::Value;

use super::Fail;

/// SHA-256("WebAuthn PRF" || 0x00 || prf_input): the salt the authenticator HMACs.
pub fn hmac_salt(prf_input: &[u8]) -> [u8; 32] {
    let mut b = b"WebAuthn PRF\0".to_vec();
    b.extend_from_slice(prf_input);
    openssl::sha::sha256(&b)
}

pub fn no_prf() -> Fail {
    Fail::new("no-prf", "This passkey cannot protect saved data. Use a passkey on a recent phone, or a security key that supports it.")
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

fn get<'a>(m: &'a BTreeMap<Value, Value>, k: &str) -> Option<&'a Value> {
    m.get(&text(k))
}

/// Which CTAP form this authenticator gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Prf,
    /// hmac-secret under this PIN/UV auth protocol (1 or 2)
    HmacSecret(u32),
    None,
}

/// Chromium's choice: "prf" when advertised, else "hmac-secret" when advertised and a PIN/UV
/// auth protocol exists (protocol 2 preferred).
pub fn form(extensions: Option<&Vec<String>>, pin_protocols: Option<&Vec<u32>>) -> Form {
    let has = |e: &str| extensions.is_some_and(|x| x.iter().any(|s| s == e));
    if has("prf") {
        return Form::Prf;
    }
    if has("hmac-secret") {
        match pin_protocols {
            Some(p) if p.contains(&2) => return Form::HmacSecret(2),
            Some(p) if p.contains(&1) => return Form::HmacSecret(1),
            _ => {}
        }
    }
    Form::None
}

/// makeCredential extension inputs that turn PRF on for the new credential.
pub fn registration_extensions(f: Form) -> Option<BTreeMap<String, Value>> {
    match f {
        Form::Prf => Some(BTreeMap::from([("prf".into(), Value::Map(BTreeMap::new()))])),
        Form::HmacSecret(_) => Some(BTreeMap::from([("hmac-secret".into(), Value::Bool(true))])),
        Form::None => None,
    }
}

/// Whether makeCredential turned PRF on: unsigned {"prf": {"enabled": true}} or authData
/// extensions {"hmac-secret": true}.
pub fn registration_enabled(unsigned: Option<&Value>, auth_data_ext: Option<&BTreeMap<Value, Value>>) -> bool {
    if let Some(Value::Map(u)) = unsigned {
        if let Some(Value::Map(p)) = get(u, "prf") {
            if get(p, "enabled") == Some(&Value::Bool(true)) {
                return true;
            }
        }
    }
    auth_data_ext.and_then(|m| get(m, "hmac-secret")) == Some(&Value::Bool(true))
}

/// getAssertion extension input for the "prf" form.
pub fn prf_assertion_extensions(salt: &[u8; 32]) -> BTreeMap<String, Value> {
    let eval = Value::Map(BTreeMap::from([(text("first"), Value::Bytes(salt.to_vec()))]));
    BTreeMap::from([("prf".into(), Value::Map(BTreeMap::from([(text("eval"), eval)])))])
}

/// The "prf" form's output: unsigned {"prf": {"results": {"first": 32 bytes}}}.
pub fn prf_assertion_output(unsigned: Option<&Value>) -> Option<[u8; 32]> {
    let Some(Value::Map(u)) = unsigned else { return None };
    let Some(Value::Map(p)) = get(u, "prf") else { return None };
    let Some(Value::Map(r)) = get(p, "results") else { return None };
    match get(r, "first") {
        Some(Value::Bytes(b)) => b.as_slice().try_into().ok(),
        _ => None,
    }
}

/// Extensions map from authenticatorData (flag ED), after the attested credential if any.
pub fn auth_data_extensions(auth_data: &[u8]) -> Option<BTreeMap<Value, Value>> {
    if auth_data.len() < 37 || auth_data[32] & 0x80 == 0 {
        return None;
    }
    let mut rest = &auth_data[37..];
    if auth_data[32] & 0x40 != 0 {
        if rest.len() < 18 {
            return None;
        }
        let len = u16::from_be_bytes([rest[16], rest[17]]) as usize;
        rest = rest.get(18 + len..)?;
        let mut de = serde_cbor_2::Deserializer::from_slice(rest);
        let _cose: Value = serde::Deserialize::deserialize(&mut de).ok()?;
        rest = &rest[de.byte_offset()..];
    }
    match serde_cbor_2::from_slice::<Value>(rest).ok()? {
        Value::Map(m) => Some(m),
        _ => None,
    }
}

// ------------------------------------------------------------------ hmac-secret (CTAP 2.1 §12.5)

fn group() -> EcGroup {
    EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).expect("P-256")
}

fn hkdf32(ikm: &[u8], info: &[u8]) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    // HKDF-SHA-256, salt = 32 zero bytes, one 32-byte block: T1 = HMAC(PRK, info || 0x01)
    let prk = hmac_sha256(&[0u8; 32], ikm)?;
    let mut msg = info.to_vec();
    msg.push(1);
    hmac_sha256(&prk, &msg)
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    let k = PKey::hmac(key)?;
    let mut s = Signer::new(openssl::hash::MessageDigest::sha256(), &k)?;
    s.update(msg)?;
    s.sign_to_vec()
}

fn aes_cbc(key: &[u8], iv: &[u8], data: &[u8], mode: CipherMode) -> Result<Vec<u8>, openssl::error::ErrorStack> {
    let mut c = Crypter::new(Cipher::aes_256_cbc(), mode, key, Some(iv))?;
    c.pad(false);
    let mut out = vec![0; data.len() + 16];
    let n = c.update(data, &mut out)?;
    let m = c.finalize(&mut out[n..])?;
    out.truncate(n + m);
    Ok(out)
}

/// One hmac-secret exchange: our ephemeral P-256 key against the authenticator's key agreement
/// key, and the shared secret both sides derive.
pub struct HmacSecret {
    protocol: u32,
    platform_x: Vec<u8>,
    platform_y: Vec<u8>,
    shared: Vec<u8>,
}

impl HmacSecret {
    pub fn new(protocol: u32, peer_x: &[u8], peer_y: &[u8]) -> Result<Self, Fail> {
        let key = EcKey::generate(&group()).map_err(|_| Fail::new("error", "Key agreement failed"))?;
        Self::with_key(protocol, key, peer_x, peer_y)
    }

    pub fn with_key(protocol: u32, key: EcKey<Private>, peer_x: &[u8], peer_y: &[u8]) -> Result<Self, Fail> {
        let bad = || Fail::new("error", "The security key sent a bad key agreement");
        if protocol != 1 && protocol != 2 {
            return Err(bad());
        }
        let g = group();
        let mut ctx = BigNumContext::new().map_err(|_| bad())?;
        let (px, py) = (BigNum::from_slice(peer_x).map_err(|_| bad())?, BigNum::from_slice(peer_y).map_err(|_| bad())?);
        let peer = EcKey::from_public_key_affine_coordinates(&g, &px, &py).map_err(|_| bad())?;
        peer.check_key().map_err(|_| bad())?;
        let mine = PKey::from_ec_key(key.clone()).map_err(|_| bad())?;
        let theirs = PKey::from_ec_key(peer).map_err(|_| bad())?;
        let mut d = Deriver::new(&mine).map_err(|_| bad())?;
        d.set_peer(&theirs).map_err(|_| bad())?;
        let z = d.derive_to_vec().map_err(|_| bad())?; // x coordinate of the shared point
        let shared = if protocol == 1 {
            openssl::sha::sha256(&z).to_vec()
        } else {
            let mut s = hkdf32(&z, b"CTAP2 HMAC key").map_err(|_| bad())?;
            s.extend(hkdf32(&z, b"CTAP2 AES key").map_err(|_| bad())?);
            s
        };
        let pt = key.public_key().to_bytes(&g, PointConversionForm::UNCOMPRESSED, &mut ctx).map_err(|_| bad())?;
        Ok(HmacSecret { protocol, platform_x: pt[1..33].to_vec(), platform_y: pt[33..65].to_vec(), shared })
    }

    fn encrypt(&self, plain: &[u8], iv: [u8; 16]) -> Result<Vec<u8>, Fail> {
        let e = |_| Fail::new("error", "Encryption failed");
        if self.protocol == 1 {
            aes_cbc(&self.shared, &[0; 16], plain, CipherMode::Encrypt).map_err(e)
        } else {
            let mut out = iv.to_vec();
            out.extend(aes_cbc(&self.shared[32..], &iv, plain, CipherMode::Encrypt).map_err(e)?);
            Ok(out)
        }
    }

    fn authenticate(&self, msg: &[u8]) -> Result<Vec<u8>, Fail> {
        let e = |_| Fail::new("error", "Encryption failed");
        if self.protocol == 1 {
            let mut m = hmac_sha256(&self.shared, msg).map_err(e)?;
            m.truncate(16);
            Ok(m)
        } else {
            hmac_sha256(&self.shared[..32], msg).map_err(e)
        }
    }

    /// The getAssertion "hmac-secret" input for one salt.
    pub fn extension_input(&self, salt: &[u8; 32]) -> Result<Value, Fail> {
        let mut iv = [0u8; 16];
        openssl::rand::rand_bytes(&mut iv).map_err(|_| Fail::new("error", "Random failed"))?;
        self.extension_input_with_iv(salt, iv)
    }

    pub fn extension_input_with_iv(&self, salt: &[u8; 32], iv: [u8; 16]) -> Result<Value, Fail> {
        let salt_enc = self.encrypt(salt, iv)?;
        let salt_auth = self.authenticate(&salt_enc)?;
        let key = Value::Map(BTreeMap::from([
            (Value::Integer(1), Value::Integer(2)),
            (Value::Integer(3), Value::Integer(-25)), // ECDH-ES+HKDF-256, as the CTAP spec says to send
            (Value::Integer(-1), Value::Integer(1)),
            (Value::Integer(-2), Value::Bytes(self.platform_x.clone())),
            (Value::Integer(-3), Value::Bytes(self.platform_y.clone())),
        ]));
        let mut m = BTreeMap::from([(Value::Integer(1), key), (Value::Integer(2), Value::Bytes(salt_enc)), (Value::Integer(3), Value::Bytes(salt_auth))]);
        // CTAP 2.0 keys only know protocol 1 and may reject an unknown key 4
        if self.protocol != 1 {
            m.insert(Value::Integer(4), Value::Integer(self.protocol as i128));
        }
        Ok(Value::Map(m))
    }

    /// The decrypted first output from authData extensions {"hmac-secret": encrypted}.
    pub fn decrypt_output(&self, enc: &[u8]) -> Result<[u8; 32], Fail> {
        let bad = || Fail::new("error", "The security key sent a bad PRF output");
        let plain = if self.protocol == 1 {
            aes_cbc(&self.shared, &[0; 16], enc, CipherMode::Decrypt).map_err(|_| bad())?
        } else {
            if enc.len() < 16 {
                return Err(bad());
            }
            aes_cbc(&self.shared[32..], &enc[..16], &enc[16..], CipherMode::Decrypt).map_err(|_| bad())?
        };
        plain.get(..32).and_then(|s| s.try_into().ok()).ok_or_else(bad)
    }

    #[cfg(test)]
    pub fn shared(&self) -> &[u8] {
        &self.shared
    }
    #[cfg(test)]
    pub fn platform_xy(&self) -> (&[u8], &[u8]) {
        (&self.platform_x, &self.platform_y)
    }
}

/// The encrypted hmac-secret output in authData extensions.
pub fn hmac_secret_output(auth_data: &[u8]) -> Option<Vec<u8>> {
    match auth_data_extensions(auth_data)?.get(&text("hmac-secret")) {
        Some(Value::Bytes(b)) => Some(b.clone()),
        _ => None,
    }
}
