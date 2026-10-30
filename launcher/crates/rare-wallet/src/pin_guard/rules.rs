//! The PIN guard's rules, shared by the root service (image/rare-pin-guard, which includes this
//! file) and rare-wallet's in-process fallback (`pin_guard::Local`). No IO: the caller loads and
//! saves the sealed record through `Store`.
//!
//! One record per user: a random 256-bit secret S, the PIN verifier (HMAC-SHA256(S,
//! Argon2id(PIN, salt))), the wrong-try count, the lock, and the phone passkeys allowed to unlock
//! it. The device key file is AES-256-GCM under K = HKDF-SHA256(S, "rare-pin-guard/device-key/v1"),
//! and K leaves the guard only for the right PIN or a phone unlock. The PIN is never stored, only
//! its verifier; S never leaves the guard.
//!
//! Rules (owner, 2026-10-28): 5 wrong PINs in a row lock the device key. From the 3rd wrong one
//! the answer says how many tries are left. Locked: no PIN is checked at all until a fresh
//! signature from a registered phone passkey over a guard-issued challenge, which resets the count
//! and allows a new PIN.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_TRIES: u32 = 5;
/// From this many wrong PINs on, the answer counts the tries left.
pub const WARN_FROM: u32 = 3;
pub const RP_ID: &str = "rare.market";
const KEY_INFO: &[u8] = b"rare-pin-guard/device-key/v1";
/// Unlock challenges and reset tokens live this long (ms).
pub const TICKET_MS: u64 = 5 * 60_000;

pub const LOCKED_MSG: &str = "Locked. Use your phone to unlock";

/// Argon2id cost of the PIN verifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kdf {
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

/// 64 MiB, 3 passes: about 0.3 s on the SuiPlay's 7840U.
pub const KDF_DEFAULT: Kdf = Kdf { m: 64 * 1024, t: 3, p: 1 };
/// Tests only.
pub const KDF_TEST: Kdf = Kdf { m: 256, t: 1, p: 1 };

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub v: u32,
    secret: String,
    salt: String,
    verifier: String,
    kdf: Kdf,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub locked: bool,
    /// compressed P-256 passkey keys (base64, 33 bytes) that may unlock
    #[serde(default)]
    pub unlockers: Vec<String>,
}

impl Drop for Record {
    fn drop(&mut self) {
        self.secret.zeroize();
        self.verifier.zeroize();
    }
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Record(failures {}, locked {}, {} unlockers)", self.failures, self.locked, self.unlockers.len())
    }
}

/// The wrong-PIN answer's words for `failures` wrong PINs in a row (owner's copy).
pub fn wrong_text(failures: u32) -> String {
    let left = MAX_TRIES.saturating_sub(failures);
    if failures >= MAX_TRIES {
        return LOCKED_MSG.into();
    }
    if failures < WARN_FROM {
        return "Wrong PIN".into();
    }
    format!("{left} {} left. After {MAX_TRIES} wrong PINs this device locks and you'll need your phone to unlock it.", if left == 1 { "try" } else { "tries" })
}

pub fn valid_pin(pin: &str) -> bool {
    (6..=12).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

fn stretch(pin: &str, salt: &[u8], kdf: Kdf) -> Result<Zeroizing<[u8; 32]>, String> {
    let params = Params::new(kdf.m, kdf.t, kdf.p, Some(32)).map_err(|_| "bad kdf".to_string())?;
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params).hash_password_into(pin.as_bytes(), salt, out.as_mut()).map_err(|_| "kdf failed".to_string())?;
    Ok(out)
}

fn hmac32(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key");
    m.update(msg);
    m.finalize().into_bytes().into()
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Record {
    pub fn new(pin: &str, kdf: Kdf) -> Result<Record, String> {
        if !valid_pin(pin) {
            return Err("Use 6 to 12 digits".into());
        }
        let secret = Zeroizing::new(random::<32>());
        let mut r = Record { v: 1, secret: B64.encode(&secret[..]), salt: String::new(), verifier: String::new(), kdf, failures: 0, locked: false, unlockers: Vec::new() };
        r.set_pin(pin)?;
        Ok(r)
    }

    fn secret(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        let s = Zeroizing::new(B64.decode(&self.secret).map_err(|_| "damaged".to_string())?);
        if s.len() != 32 {
            return Err("damaged".into());
        }
        Ok(s)
    }

    fn set_pin(&mut self, pin: &str) -> Result<(), String> {
        if !valid_pin(pin) {
            return Err("Use 6 to 12 digits".into());
        }
        let salt = random::<16>();
        let st = stretch(pin, &salt, self.kdf)?;
        let v = hmac32(&self.secret()?, &st[..]);
        self.salt = B64.encode(salt);
        self.verifier = B64.encode(v);
        Ok(())
    }

    fn pin_ok(&self, pin: &str) -> Result<bool, String> {
        let salt = B64.decode(&self.salt).map_err(|_| "damaged".to_string())?;
        let want = B64.decode(&self.verifier).map_err(|_| "damaged".to_string())?;
        let st = stretch(pin, &salt, self.kdf)?;
        Ok(ct_eq(&want, &hmac32(&self.secret()?, &st[..])))
    }

    /// K, the device key file's AES key.
    pub fn device_key(&self) -> Result<Zeroizing<[u8; 32]>, String> {
        let s = self.secret()?;
        let mut k = Zeroizing::new([0u8; 32]);
        hkdf::Hkdf::<Sha256>::new(Some(b"rare-pin-guard"), &s).expand(KEY_INFO, &mut k[..]).map_err(|_| "kdf failed".to_string())?;
        Ok(k)
    }

    pub fn tries_left(&self) -> u32 {
        if self.locked {
            0
        } else {
            MAX_TRIES.saturating_sub(self.failures)
        }
    }
}

/// Checks a WebAuthn assertion over `challenge` (a get, rare.market, user present) by one of
/// `unlockers`. Returns the key that signed.
pub fn verify_assertion(unlockers: &[String], challenge: &[u8], authenticator_data: &[u8], client_data_json: &[u8], signature_der: &[u8]) -> Option<String> {
    let cdj: Value = serde_json::from_slice(client_data_json).ok()?;
    if cdj.get("type")?.as_str()? != "webauthn.get" || cdj.get("challenge")?.as_str()? != B64URL.encode(challenge) {
        return None;
    }
    if authenticator_data.len() < 37 || authenticator_data[..32] != Sha256::digest(RP_ID.as_bytes())[..] || authenticator_data[32] & 0x01 == 0 {
        return None;
    }
    let sig = p256::ecdsa::Signature::from_der(signature_der).ok()?;
    let sig = sig.normalize_s().unwrap_or(sig);
    let mut msg = authenticator_data.to_vec();
    msg.extend_from_slice(&Sha256::digest(client_data_json));
    use p256::ecdsa::signature::Verifier as _;
    unlockers.iter().find(|pk| {
        B64.decode(pk).ok().and_then(|b| p256::ecdsa::VerifyingKey::from_sec1_bytes(&b).ok()).is_some_and(|vk| vk.verify(&msg, &sig).is_ok())
    }).cloned()
}

/// Where the sealed record lives.
pub trait Store {
    /// Ok(None): no record. Err: there is one but it can't be opened (fail closed).
    fn load(&self) -> Result<Option<Record>, String>;
    fn save(&self, r: &Record) -> Result<(), String>;
    fn remove(&self) -> Result<(), String>;
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    Status,
    #[serde(rename_all = "camelCase")]
    Create {
        pin: String,
        #[serde(default)]
        kdf: Option<Kdf>,
    },
    Open {
        pin: String,
    },
    #[serde(rename_all = "camelCase")]
    Change {
        pin: String,
        new_pin: String,
    },
    #[serde(rename_all = "camelCase")]
    AddUnlocker {
        pin: String,
        public_key: String,
    },
    Challenge,
    #[serde(rename_all = "camelCase")]
    Unlock {
        challenge: String,
        authenticator_data: String,
        client_data_json: String,
        signature: String,
    },
    #[serde(rename_all = "camelCase")]
    ResetPin {
        token: String,
        new_pin: String,
    },
    Wipe,
}

impl Drop for Request {
    fn drop(&mut self) {
        match self {
            Request::Create { pin, .. } | Request::Open { pin } | Request::AddUnlocker { pin, .. } => pin.zeroize(),
            Request::Change { pin, new_pin } => {
                pin.zeroize();
                new_pin.zeroize();
            }
            Request::ResetPin { token, new_pin } => {
                token.zeroize();
                new_pin.zeroize();
            }
            _ => {}
        }
    }
}

fn fail(code: &str, message: &str) -> Value {
    json!({ "ok": false, "code": code, "message": message })
}

fn key_answer(r: &Record) -> Value {
    match r.device_key() {
        Ok(k) => json!({ "ok": true, "key": B64.encode(&k[..]) }),
        Err(_) => fail("damaged", "This device's key record is damaged"),
    }
}

/// One user's guard: the store plus the challenges and reset tokens it handed out (memory only).
pub struct Guard<S: Store> {
    pub store: S,
    challenges: Vec<([u8; 32], u64)>,
    tokens: Vec<([u8; 32], u64)>,
    /// Argon2 cost for new records (tests use KDF_TEST).
    pub kdf: Kdf,
    /// whether a client may pick the cost (in-process fallback tests only, never the service)
    pub client_kdf: bool,
}

impl<S: Store> Guard<S> {
    pub fn new(store: S) -> Self {
        Guard { store, challenges: Vec::new(), tokens: Vec::new(), kdf: KDF_DEFAULT, client_kdf: false }
    }

    fn load(&self) -> Result<Record, Value> {
        match self.store.load() {
            Ok(Some(r)) => Ok(r),
            Ok(None) => Err(fail("missing", "No PIN is set on this device")),
            Err(_) => Err(fail("damaged", "This device's key record can't be opened")),
        }
    }

    fn save(&self, r: &Record) -> Result<(), Value> {
        self.store.save(r).map_err(|_| fail("error", "Could not save the PIN record"))
    }

    /// Checks the PIN, counting a wrong one (saved before the answer goes out).
    fn check(&self, r: &mut Record, pin: &str) -> Result<(), Value> {
        if r.locked || r.failures >= MAX_TRIES {
            return Err(fail("pin-locked", LOCKED_MSG));
        }
        let ok = r.pin_ok(pin).map_err(|_| fail("damaged", "This device's key record is damaged"))?;
        if ok {
            if r.failures > 0 {
                r.failures = 0;
                self.save(r)?;
            }
            return Ok(());
        }
        r.failures += 1;
        if r.failures >= MAX_TRIES {
            r.locked = true;
        }
        self.save(r)?;
        if r.locked {
            return Err(fail("pin-locked", LOCKED_MSG));
        }
        Err(json!({ "ok": false, "code": "wrong", "message": wrong_text(r.failures), "failures": r.failures, "triesLeft": r.tries_left() }))
    }

    fn take(list: &mut Vec<([u8; 32], u64)>, b64: &str, now: u64) -> bool {
        list.retain(|(_, exp)| *exp > now);
        let Ok(b) = B64.decode(b64) else { return false };
        match list.iter().position(|(c, _)| ct_eq(c, &b)) {
            Some(i) => {
                list.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn handle(&mut self, req: Request, now_ms: u64) -> Value {
        match self.run(&req, now_ms) {
            Ok(v) | Err(v) => v,
        }
    }

    fn run(&mut self, req: &Request, now: u64) -> Result<Value, Value> {
        match req {
            Request::Status => match self.store.load() {
                Ok(None) => Ok(json!({ "ok": true, "exists": false })),
                Ok(Some(r)) => Ok(json!({ "ok": true, "exists": true, "locked": r.locked, "failures": r.failures, "triesLeft": r.tries_left(), "unlockers": r.unlockers.len() })),
                Err(_) => Ok(json!({ "ok": true, "exists": true, "damaged": true, "locked": true, "triesLeft": 0 })),
            },
            Request::Create { pin, kdf } => {
                if !matches!(self.store.load(), Ok(None)) {
                    return Err(fail("exists", "This device already has a PIN"));
                }
                let kdf = kdf.filter(|_| self.client_kdf).unwrap_or(self.kdf);
                let r = Record::new(pin, kdf).map_err(|m| fail("bad-pin", &m))?;
                self.save(&r)?;
                Ok(key_answer(&r))
            }
            Request::Open { pin } => {
                let mut r = self.load()?;
                self.check(&mut r, pin)?;
                Ok(key_answer(&r))
            }
            Request::Change { pin, new_pin } => {
                if !valid_pin(new_pin) {
                    return Err(fail("bad-pin", "Use 6 to 12 digits"));
                }
                let mut r = self.load()?;
                self.check(&mut r, pin)?;
                r.set_pin(new_pin).map_err(|m| fail("bad-pin", &m))?;
                self.save(&r)?;
                Ok(json!({ "ok": true }))
            }
            Request::AddUnlocker { pin, public_key } => {
                let pk = B64.decode(public_key).ok().filter(|b| b.len() == 33 && p256::PublicKey::from_sec1_bytes(b).is_ok()).ok_or_else(|| fail("bad-key", "That is not a passkey key"))?;
                let mut r = self.load()?;
                self.check(&mut r, pin)?;
                let pk = B64.encode(pk);
                if !r.unlockers.contains(&pk) {
                    r.unlockers.push(pk);
                    self.save(&r)?;
                }
                Ok(json!({ "ok": true, "unlockers": r.unlockers.len() }))
            }
            Request::Challenge => {
                let r = self.load()?;
                if r.unlockers.is_empty() {
                    return Err(fail("no-phone", "No phone can unlock this device"));
                }
                self.challenges.retain(|(_, e)| *e > now);
                if self.challenges.len() >= 8 {
                    self.challenges.remove(0);
                }
                let c = random::<32>();
                self.challenges.push((c, now + TICKET_MS));
                Ok(json!({ "ok": true, "challenge": B64.encode(c) }))
            }
            Request::Unlock { challenge, authenticator_data, client_data_json, signature } => {
                let mut r = self.load()?;
                if !Self::take(&mut self.challenges, challenge, now) {
                    return Err(fail("expired", "That unlock request expired. Try again."));
                }
                let dec = |s: &str| B64.decode(s).map_err(|_| fail("bad-assertion", "Could not read the phone's signature"));
                let (c, ad, cdj, sig) = (dec(challenge)?, dec(authenticator_data)?, dec(client_data_json)?, dec(signature)?);
                if verify_assertion(&r.unlockers, &c, &ad, &cdj, &sig).is_none() {
                    return Err(fail("bad-assertion", "That passkey can't unlock this device"));
                }
                r.failures = 0;
                r.locked = false;
                self.save(&r)?;
                let t = random::<32>();
                self.tokens.retain(|(_, e)| *e > now);
                self.tokens.push((t, now + TICKET_MS));
                Ok(json!({ "ok": true, "token": B64.encode(t) }))
            }
            Request::ResetPin { token, new_pin } => {
                if !valid_pin(new_pin) {
                    return Err(fail("bad-pin", "Use 6 to 12 digits"));
                }
                let mut r = self.load()?;
                if !Self::take(&mut self.tokens, token, now) {
                    return Err(fail("expired", "Unlock with your phone again"));
                }
                r.set_pin(new_pin).map_err(|m| fail("bad-pin", &m))?;
                r.failures = 0;
                r.locked = false;
                self.save(&r)?;
                Ok(key_answer(&r))
            }
            Request::Wipe => {
                self.store.remove().map_err(|_| fail("error", "Could not remove the PIN record"))?;
                self.challenges.clear();
                self.tokens.clear();
                Ok(json!({ "ok": true }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Mem(RefCell<Option<String>>);
    impl Store for Mem {
        fn load(&self) -> Result<Option<Record>, String> {
            self.0.borrow().as_ref().map(|s| serde_json::from_str(s).map_err(|_| "damaged".to_string())).transpose()
        }
        fn save(&self, r: &Record) -> Result<(), String> {
            *self.0.borrow_mut() = Some(serde_json::to_string(r).unwrap());
            Ok(())
        }
        fn remove(&self) -> Result<(), String> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }

    fn req(v: Value) -> Request {
        serde_json::from_value(v).unwrap()
    }

    fn assertion(sk: &p256::ecdsa::SigningKey, challenge: &[u8]) -> Value {
        let mut ad = Sha256::digest(RP_ID.as_bytes()).to_vec();
        ad.push(0x05);
        ad.extend_from_slice(&1u32.to_be_bytes());
        let cdj = format!(r#"{{"type":"webauthn.get","challenge":"{}","origin":"https://rare.market"}}"#, B64URL.encode(challenge));
        let mut msg = ad.clone();
        msg.extend_from_slice(&Sha256::digest(cdj.as_bytes()));
        let sig: p256::ecdsa::Signature = p256::ecdsa::signature::Signer::sign(sk, &msg);
        json!({ "op": "unlock", "challenge": B64.encode(challenge), "authenticatorData": B64.encode(ad), "clientDataJson": B64.encode(cdj), "signature": B64.encode(sig.to_der().as_bytes()) })
    }

    #[test]
    fn five_wrong_lock_and_the_phone_unlocks() {
        let mut g = Guard::new(Mem::default());
        g.kdf = KDF_TEST;
        assert_eq!(g.handle(req(json!({"op":"status"})), 0)["exists"], false);
        let k = g.handle(req(json!({"op":"create","pin":"123456"})), 0)["key"].as_str().unwrap().to_string();
        assert!(!g.store.0.borrow().as_ref().unwrap().contains("123456"));
        assert_eq!(g.handle(req(json!({"op":"create","pin":"111111"})), 0)["code"], "exists");
        assert_eq!(g.handle(req(json!({"op":"open","pin":"123456"})), 0)["key"], k);
        let sk = p256::ecdsa::SigningKey::random(&mut OsRng);
        let pk = B64.encode(sk.verifying_key().to_encoded_point(true).as_bytes());
        assert_eq!(g.handle(req(json!({"op":"addUnlocker","pin":"123456","publicKey":pk})), 0)["ok"], true);

        let texts: Vec<Value> = (0..5).map(|_| g.handle(req(json!({"op":"open","pin":"000000"})), 0)).collect();
        assert_eq!(texts[0]["message"], "Wrong PIN");
        assert_eq!(texts[1]["message"], "Wrong PIN");
        assert_eq!(texts[2]["message"], "2 tries left. After 5 wrong PINs this device locks and you'll need your phone to unlock it.");
        assert_eq!(texts[3]["message"], "1 try left. After 5 wrong PINs this device locks and you'll need your phone to unlock it.");
        assert_eq!(texts[4]["code"], "pin-locked");
        assert_eq!(texts[4]["message"], LOCKED_MSG);
        // locked: even the right PIN is not checked
        assert_eq!(g.handle(req(json!({"op":"open","pin":"123456"})), 0)["code"], "pin-locked");
        // a guard started again (reboot, reinstalled launcher) reads the same lock
        let mut g2 = Guard::new(Mem(RefCell::new(g.store.0.borrow().clone())));
        assert_eq!(g2.handle(req(json!({"op":"open","pin":"123456"})), 0)["code"], "pin-locked");

        // the phone: a stranger's key and a stale challenge are refused
        let c = B64.decode(g.handle(req(json!({"op":"challenge"})), 0)["challenge"].as_str().unwrap()).unwrap();
        let other = p256::ecdsa::SigningKey::random(&mut OsRng);
        assert_eq!(g.handle(req(assertion(&other, &c)), 1)["code"], "bad-assertion");
        assert_eq!(g.handle(req(assertion(&sk, &c)), 2)["code"], "expired", "a challenge is used once");
        let c = B64.decode(g.handle(req(json!({"op":"challenge"})), 0)["challenge"].as_str().unwrap()).unwrap();
        assert_eq!(g.handle(req(assertion(&sk, &c)), TICKET_MS + 1)["code"], "expired");
        let c = B64.decode(g.handle(req(json!({"op":"challenge"})), 10)["challenge"].as_str().unwrap()).unwrap();
        let t = g.handle(req(assertion(&sk, &c)), 20);
        assert_eq!(t["ok"], true);
        let token = t["token"].as_str().unwrap();
        let r = g.handle(req(json!({"op":"resetPin","token":token,"newPin":"654321"})), 30);
        assert_eq!(r["key"], k, "the same device key, a new PIN");
        assert_eq!(g.handle(req(json!({"op":"resetPin","token":token,"newPin":"999999"})), 31)["code"], "expired");
        assert_eq!(g.handle(req(json!({"op":"open","pin":"123456"})), 40)["message"], "Wrong PIN");
        assert_eq!(g.handle(req(json!({"op":"open","pin":"654321"})), 40)["key"], k);
        assert_eq!(g.handle(req(json!({"op":"status"})), 0)["failures"], 0);
    }

    #[test]
    fn damaged_or_tampered_record_fails_closed() {
        let mut g = Guard::new(Mem::default());
        g.kdf = KDF_TEST;
        g.handle(req(json!({"op":"create","pin":"123456"})), 0);
        let mut v: Value = serde_json::from_str(g.store.0.borrow().as_ref().unwrap()).unwrap();
        v["secret"] = json!(B64.encode([7u8; 32]));
        *g.store.0.borrow_mut() = Some(v.to_string());
        assert_eq!(g.handle(req(json!({"op":"open","pin":"123456"})), 0)["message"], "Wrong PIN");
        *g.store.0.borrow_mut() = Some("{".into());
        assert_eq!(g.handle(req(json!({"op":"open","pin":"123456"})), 0)["code"], "damaged");
        assert_eq!(g.handle(req(json!({"op":"status"})), 0)["locked"], true);
        assert!(!valid_pin("12345") && !valid_pin("12345a") && valid_pin("123456") && !valid_pin("1234567890123"));
    }
}
