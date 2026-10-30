//! Keystone 3 Pro for Rare Wallet: the URs it exchanges over USB (EAPDU CMD_RESOLVE_UR) and the
//! signing layer on top. Port of src/wallet/keystone/{ur,usb,errors,prompt,index}.ts.
//!
//! - accounts: a `qr-hardware-call` KeyDerivation (version 1, ed25519, SLIP-10, chain "SUI") ->
//!   `crypto-multi-accounts` with the master fingerprint and one 32-byte key per path
//! - signing: `sui-sign-hash-request` (the 32-byte intent digest as hex, the path with the master
//!   fingerprint, the address) -> `sui-signature` (64-byte signature and the public key)
//!
//! Transport-agnostic (`KeystoneLink`): `keystone::KeystoneState` (feature `hw`) or the fake.

use crate::keys::{address_of, hex, normalize_address, personal_message_digest, sui_path, tx_digest, unhex, FLAG_ED25519};
use crate::ur::{decode_ur, encode_ur, Cbor, Ur};
use crate::WalletError;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand_core::{OsRng, RngCore};
use std::time::{Duration, Instant};

/// Shown on the Keystone as the app asking.
pub const KEYSTONE_ORIGIN: &str = "Rare Wallet";

/// EAPDU commands (firmware src/webusb_protocol/general/eapdu_protocol_parser.h).
pub mod cmd {
    pub const ECHO: u16 = 1;
    pub const RESOLVE_UR: u16 = 2;
    pub const CHECK_LOCK_STATUS: u16 = 3;
    pub const EXPORT_ADDRESS: u16 = 4;
    pub const GET_DEVICE_INFO: u16 = 5;
}

const TAG_UUID: u64 = 37;
const TAG_HDKEY: u64 = 303;
const TAG_KEYPATH: u64 = 304;
const TAG_KEY_DERIVATION_CALL: u64 = 1301;
const TAG_KEY_DERIVATION_SCHEMA: u64 = 1302;

// ---------------------------------------------------------------- errors

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeystoneCode {
    NoDevice,
    Locked,
    Rejected,
    Timeout,
    UsbOff,
    Udev,
    HomeScreen,
    Busy,
    Disconnected,
    Mismatch,
    Unsupported,
    Cancelled,
    BadResponse,
    Unknown,
}

impl KeystoneCode {
    pub const ALL: [KeystoneCode; 14] = [
        KeystoneCode::NoDevice,
        KeystoneCode::Locked,
        KeystoneCode::Rejected,
        KeystoneCode::Timeout,
        KeystoneCode::UsbOff,
        KeystoneCode::Udev,
        KeystoneCode::HomeScreen,
        KeystoneCode::Busy,
        KeystoneCode::Disconnected,
        KeystoneCode::Mismatch,
        KeystoneCode::Unsupported,
        KeystoneCode::Cancelled,
        KeystoneCode::BadResponse,
        KeystoneCode::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            KeystoneCode::NoDevice => "no-device",
            KeystoneCode::Locked => "locked",
            KeystoneCode::Rejected => "rejected",
            KeystoneCode::Timeout => "timeout",
            KeystoneCode::UsbOff => "usb-off",
            KeystoneCode::Udev => "udev",
            KeystoneCode::HomeScreen => "home-screen",
            KeystoneCode::Busy => "busy",
            KeystoneCode::Disconnected => "disconnected",
            KeystoneCode::Mismatch => "mismatch",
            KeystoneCode::Unsupported => "unsupported",
            KeystoneCode::Cancelled => "cancelled",
            KeystoneCode::BadResponse => "bad-response",
            KeystoneCode::Unknown => "unknown",
        }
    }

    /// KEYSTONE_MESSAGES in errors.ts.
    pub fn message(self) -> &'static str {
        match self {
            KeystoneCode::NoDevice => "No Keystone found. Connect it with USB-C and unlock it.",
            KeystoneCode::Locked => "Your Keystone is locked. Unlock it and try again.",
            KeystoneCode::Rejected => "Rejected on your Keystone.",
            KeystoneCode::Timeout => "Your Keystone did not answer in time. Try again.",
            KeystoneCode::UsbOff => "Go to the home screen on your Keystone. If USB is off, turn it on in Settings > Connection.",
            KeystoneCode::Udev => "Rare OS needs permission to use your Keystone. Run this command once in a terminal, then reconnect your Keystone.",
            KeystoneCode::HomeScreen => "Go to the home screen on your Keystone and try again.",
            KeystoneCode::Busy => "Your Keystone is busy with another request. Finish or reject it on the device, then try again.",
            KeystoneCode::Disconnected => "Your Keystone disconnected. Reconnect it and try again.",
            KeystoneCode::Mismatch => "This Keystone does not hold this account. Connect the Keystone you added it with.",
            KeystoneCode::Unsupported => "Your Keystone cannot do this yet. Update it to the latest multi-coin firmware and try again.",
            KeystoneCode::Cancelled => "Cancelled. If your Keystone still shows the request, reject it there.",
            KeystoneCode::BadResponse => "Your Keystone sent an answer Rare Wallet could not read. Update its firmware and try again.",
            KeystoneCode::Unknown => "Your Keystone did not respond. Reconnect it and try again.",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeystoneError {
    pub code: KeystoneCode,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for KeystoneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for KeystoneError {}

fn has(msg: &str, needle: &str) -> bool {
    msg.to_ascii_lowercase().contains(needle)
}

/// Device answers carry text only (firmware service_resolve_ur.c and the Rust errors).
fn specific_text(msg: &str) -> Option<KeystoneCode> {
    let m = msg.to_ascii_lowercase();
    if m.contains("specific pages") {
        return Some(KeystoneCode::HomeScreen);
    }
    if m.contains("waiting for user approval") {
        return Some(KeystoneCode::Busy);
    }
    if m.contains("view type is not supported") || m.contains("not support") || m.contains("notsupport") {
        return Some(KeystoneCode::Unsupported);
    }
    if m.contains("fingerprint") {
        return Some(KeystoneCode::Mismatch);
    }
    None
}

fn from_device_text(msg: &str) -> Option<KeystoneCode> {
    specific_text(msg).or_else(|| {
        if has(msg, "reject") {
            Some(KeystoneCode::Rejected)
        } else if has(msg, "locked") {
            Some(KeystoneCode::Locked)
        } else {
            None
        }
    })
}

impl KeystoneError {
    pub fn new(code: KeystoneCode) -> Self {
        KeystoneError { code, message: code.message().into(), data: None }
    }
    pub fn fix(&self) -> Option<&str> {
        self.data.as_ref()?.get("fix")?.as_str()
    }

    /// A transport error string ("code|message[|{json}]") from `keystone::KeystoneState`.
    pub fn from_native(s: &str) -> Self {
        let e = WalletError::parse(s);
        // the device's text is more precise than the transport's code
        let native = match e.code.as_str() {
            "udev" => Some(KeystoneCode::Udev),
            "notfound" => Some(KeystoneCode::NoDevice),
            "locked" => Some(KeystoneCode::Locked),
            "rejected" => Some(KeystoneCode::Rejected),
            "timeout" => Some(KeystoneCode::Timeout),
            "usb-off" => Some(KeystoneCode::UsbOff),
            "busy" => Some(KeystoneCode::Busy),
            "disconnected" => Some(KeystoneCode::Disconnected),
            "mismatch" => Some(KeystoneCode::Mismatch),
            "unsupported" => Some(KeystoneCode::Unsupported),
            _ => None,
        };
        let code = if e.code == "udev" { KeystoneCode::Udev } else { specific_text(&e.message).or(native).or_else(|| from_device_text(&e.message)).unwrap_or(KeystoneCode::Unknown) };
        let mut out = KeystoneError { data: e.data, ..KeystoneError::new(code) };
        if code == KeystoneCode::Rejected && has(&e.message, "password") {
            out.message = "Wrong password on your Keystone. Try again.".into();
        }
        out
    }
}

type R<T> = Result<T, KeystoneError>;

fn bad() -> KeystoneError {
    KeystoneError::new(KeystoneCode::BadResponse)
}

// ---------------------------------------------------------------- URs

/// "m/44'/784'/0'/0'/0'" -> crypto-keypath (with the source fingerprint when given).
pub fn keypath(path: &str, mfp: Option<&str>) -> Cbor {
    let p = path.strip_prefix("m/").or_else(|| path.strip_prefix("M/")).unwrap_or(path);
    let mut comps = Vec::new();
    for c in p.split('/') {
        let digits: String = c.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        comps.push(Cbor::Uint(digits.parse().unwrap_or(0)));
        comps.push(Cbor::Bool(c.ends_with('\'')));
    }
    let mut map = vec![(1, Cbor::Array(comps))];
    if let Some(fp) = mfp.and_then(unhex).filter(|b| b.len() == 4) {
        map.push((2, Cbor::Uint(u32::from_be_bytes(fp.try_into().unwrap()) as u64)));
    }
    Cbor::Map(map)
}

/// crypto-keypath -> ("m/44'/…", source fingerprint hex).
pub fn read_keypath(c: &Cbor) -> Option<(String, Option<String>)> {
    let comps = c.get(1)?.as_array()?;
    let mut parts = Vec::new();
    for pair in comps.chunks(2) {
        let hard = pair.get(1)?.as_bool()?;
        let idx = match pair[0].untag() {
            Cbor::Uint(n) => n.to_string(),
            Cbor::Array(a) if a.is_empty() => "*".into(),
            _ => return None,
        };
        parts.push(format!("{idx}{}", if hard { "'" } else { "" }));
    }
    if parts.is_empty() {
        return None;
    }
    let fp = c.get(2).and_then(Cbor::as_u64).map(|n| format!("{:08x}", n as u32));
    Some((format!("m/{}", parts.join("/")), fp))
}

/// The key derivation call for `paths`, as the single-part UR string the device takes.
pub fn key_derivation_ur(paths: &[String]) -> String {
    let schemas = paths
        .iter()
        .map(|p| Cbor::tag(TAG_KEY_DERIVATION_SCHEMA, Cbor::Map(vec![(1, Cbor::tag(TAG_KEYPATH, keypath(p, None))), (2, Cbor::Uint(1)), (3, Cbor::Uint(0)), (4, Cbor::Text("SUI".into()))])))
        .collect();
    let call = Cbor::Map(vec![
        (1, Cbor::Uint(0)),
        (2, Cbor::tag(TAG_KEY_DERIVATION_CALL, Cbor::Map(vec![(1, Cbor::Array(schemas))]))),
        (3, Cbor::Text(KEYSTONE_ORIGIN.into())),
        (4, Cbor::Uint(1)),
    ]);
    encode_ur("qr-hardware-call", &call.encode())
}

/// Keys from a crypto-multi-accounts answer.
#[derive(Debug, Clone, PartialEq)]
pub struct KeystoneKeys {
    /// master fingerprint, 8 hex chars
    pub mfp: String,
    pub device: Option<String>,
    /// (path, 32-byte Ed25519 key)
    pub keys: Vec<(String, Vec<u8>)>,
}

pub fn parse_multi_accounts(ur: &Ur) -> R<KeystoneKeys> {
    if ur.kind != "crypto-multi-accounts" {
        return Err(bad());
    }
    let c = Cbor::decode(&ur.cbor).ok_or_else(bad)?;
    let mfp = c.get(1).and_then(Cbor::as_u64).unwrap_or(0) as u32;
    let mut keys = Vec::new();
    for k in c.get(2).and_then(Cbor::as_array).ok_or_else(bad)? {
        let pk = k.get(3).and_then(Cbor::as_bytes).ok_or_else(bad)?;
        // ed25519 keys are 32 bytes; some encoders prefix 0x00
        let pk = if pk.len() == 33 && pk[0] == 0 { &pk[1..] } else { pk };
        let (path, _) = k.get(6).and_then(read_keypath).ok_or_else(bad)?;
        if pk.len() != 32 {
            return Err(bad());
        }
        keys.push((path, pk.to_vec()));
    }
    let device = c.get(3).and_then(Cbor::as_text).filter(|d| !d.is_empty()).map(str::to_string);
    Ok(KeystoneKeys { mfp: format!("{mfp:08x}"), device, keys })
}

/// RFC 4122 v4 uuid (the request id).
pub fn new_request_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    uuid_string(&b)
}

fn uuid_string(b: &[u8]) -> String {
    let h = hex(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// The sui-sign-hash-request UR for a 32-byte digest, and its request id.
pub fn sign_hash_ur(digest: &[u8; 32], path: &str, mfp: &str, address: &str, request_id: Option<&str>) -> R<(String, String)> {
    let id = request_id.map(str::to_string).unwrap_or_else(new_request_id);
    let id_bytes = unhex(&id.replace('-', "")).filter(|b| b.len() == 16).ok_or_else(bad)?;
    let addr = normalize_address(address).and_then(|a| unhex(&a[2..])).ok_or_else(bad)?;
    let req = Cbor::Map(vec![
        (1, Cbor::tag(TAG_UUID, Cbor::Bytes(id_bytes))),
        (2, Cbor::Text(hex(digest))),
        (3, Cbor::Array(vec![Cbor::tag(TAG_KEYPATH, keypath(path, Some(mfp)))])),
        (4, Cbor::Array(vec![Cbor::Bytes(addr)])),
        (5, Cbor::Text(KEYSTONE_ORIGIN.into())),
    ]);
    Ok((encode_ur("sui-sign-hash-request", &req.encode()), id))
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeystoneSignature {
    pub signature: [u8; 64],
    pub public_key: Option<Vec<u8>>,
    pub request_id: String,
}

pub fn parse_sui_signature(ur: &Ur) -> R<KeystoneSignature> {
    if ur.kind != "sui-signature" {
        return Err(bad());
    }
    let c = Cbor::decode(&ur.cbor).ok_or_else(bad)?;
    let signature: [u8; 64] = c.get(2).and_then(Cbor::as_bytes).and_then(|s| s.try_into().ok()).ok_or_else(bad)?;
    // the TS registry reads the request id unconditionally: an answer without one is unreadable
    let id = c.get(1).and_then(Cbor::as_bytes).filter(|b| b.len() == 16).ok_or_else(bad)?;
    Ok(KeystoneSignature { signature, public_key: c.get(3).and_then(Cbor::as_bytes).map(<[u8]>::to_vec), request_id: uuid_string(id) })
}

/// The answer text from the transport: the device's JSON ({"payload": "UR:..."}), a bare JSON
/// string, or the UR itself.
pub fn response_payload(text: &str) -> R<String> {
    let t = text.trim();
    if t.len() >= 3 && t[..3].eq_ignore_ascii_case("ur:") {
        return Ok(t.to_string());
    }
    match serde_json::from_str::<serde_json::Value>(t) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        Ok(v) => v.get("payload").and_then(|p| p.as_str()).map(str::to_string).ok_or_else(bad),
        Err(_) => Err(bad()),
    }
}

fn decode_answer(text: &str) -> R<Ur> {
    decode_ur(&response_payload(text)?).ok_or_else(bad)
}

// ---------------------------------------------------------------- requests (usb.ts)

/// One command to the Keystone; returns the answer text.
pub trait KeystoneLink {
    fn send(&mut self, command: u16, data: &str, timeout_ms: u64) -> R<String>;
}

/// The person approves on the device (and types its password for key export): give them time.
pub const PROMPT_MS: u64 = 5 * 60_000;

#[derive(Debug, Clone, PartialEq)]
pub struct KeystoneAccount {
    pub index: u32,
    pub path: String,
    /// base64 32-byte Ed25519 key
    pub public_key: String,
    pub address: String,
    pub mfp: String,
}

/// Keys for accounts from..from+count-1 in one request: one approval on the device.
pub fn get_accounts(link: &mut dyn KeystoneLink, from: u32, count: u32) -> R<(String, Option<String>, Vec<KeystoneAccount>)> {
    if !(1..=24).contains(&count) {
        return Err(KeystoneError { message: "Ask for 1 to 24 accounts at a time".into(), ..KeystoneError::new(KeystoneCode::Unknown) });
    }
    let paths: Vec<String> = (from..from + count).map(sui_path).collect();
    let answer = link.send(cmd::RESOLVE_UR, &key_derivation_ur(&paths), PROMPT_MS)?;
    let got = parse_multi_accounts(&decode_answer(&answer)?)?;
    if got.mfp.len() != 8 {
        return Err(bad());
    }
    let mut accounts = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let (_, pk) = got.keys.iter().find(|(p, _)| p == path).ok_or_else(bad)?;
        accounts.push(KeystoneAccount { index: from + i as u32, path: path.clone(), public_key: B64.encode(pk), address: address_of(FLAG_ED25519, pk).map_err(|_| bad())?, mfp: got.mfp.clone() });
    }
    Ok((got.mfp, got.device, accounts))
}

fn verify_ed25519(public_key: &[u8], digest: &[u8; 32], sig: &[u8; 64]) -> bool {
    let Ok(pk) = <[u8; 32]>::try_from(public_key) else { return false };
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(&pk) else { return false };
    vk.verify_strict(digest, &ed25519_dalek::Signature::from_bytes(sig)).is_ok()
}

/// Has the Keystone sign a 32-byte digest at `path`. Returns the 64-byte signature after checking
/// it answers our request, comes from `address` (and `public_key` when given) and verifies.
pub fn sign_hash(link: &mut dyn KeystoneLink, path: &str, mfp: &str, address: &str, digest: &[u8; 32], public_key: Option<&[u8]>) -> R<[u8; 64]> {
    let (ur, request_id) = sign_hash_ur(digest, path, mfp, address, None)?;
    let answer = link.send(cmd::RESOLVE_UR, &ur, PROMPT_MS)?;
    let sig = parse_sui_signature(&decode_answer(&answer)?)?;
    if sig.request_id != request_id {
        return Err(bad());
    }
    let key = public_key.map(<[u8]>::to_vec).or_else(|| sig.public_key.clone()).filter(|k| k.len() == 32).ok_or_else(bad)?;
    if address_of(FLAG_ED25519, &key).ok() != normalize_address(address) {
        return Err(KeystoneError::new(KeystoneCode::Mismatch));
    }
    if sig.public_key.as_ref().is_some_and(|p| *p != key) || !verify_ed25519(&key, digest, &sig.signature) {
        return Err(KeystoneError::new(KeystoneCode::Mismatch));
    }
    Ok(sig.signature)
}

/// The 32 bytes a Sui Ed25519 key signs; the Keystone shows them, the review sheet too.
pub fn signing_digest(transaction: bool, bytes: &[u8]) -> [u8; 32] {
    if transaction {
        tx_digest(bytes)
    } else {
        personal_message_digest(bytes)
    }
}

/// Serialized Sui signature (base64) for a Keystone account, over a transaction or a message.
pub fn sign(link: &mut dyn KeystoneLink, account: &KeystoneAccount, transaction: bool, bytes: &[u8]) -> R<String> {
    let pk = B64.decode(&account.public_key).ok().filter(|p| p.len() == 32).ok_or_else(|| KeystoneError {
        message: "This Keystone account has no valid public key. Remove it and add it again.".into(),
        ..KeystoneError::new(KeystoneCode::Unknown)
    })?;
    let digest = signing_digest(transaction, bytes);
    let sig = sign_hash(link, &account.path, &account.mfp, &account.address, &digest, Some(&pk))?;
    let mut out = vec![FLAG_ED25519];
    out.extend_from_slice(&sig);
    out.extend_from_slice(&pk);
    Ok(B64.encode(out))
}

// ---------------------------------------------------------------- the task loop (index.ts)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Connecting,
    Unlock,
    HomeScreen,
    Approve,
    Confirm,
    Done,
}

impl Stage {
    /// KEYSTONE_STAGE_TEXT.
    pub fn text(self) -> &'static str {
        match self {
            Stage::Connecting => "Connecting to your Keystone",
            Stage::Unlock => "Unlock your Keystone",
            Stage::HomeScreen => "Go to the home screen on your Keystone",
            Stage::Approve => "Approve on your Keystone and enter its password",
            Stage::Confirm => "Confirm on your Keystone",
            Stage::Done => "Done",
        }
    }
}

pub const WAIT: Duration = Duration::from_secs(180);
pub const POLL: Duration = Duration::from_millis(1500);

/// Runs a request; while the device is locked or away from its home screen it reports that,
/// waits `poll` and retries until `wait` runs out or `cancelled()`.
pub fn task<T>(stage: Stage, wait: Duration, poll: Duration, on_stage: &mut dyn FnMut(Stage), cancelled: &dyn Fn() -> bool, mut f: impl FnMut() -> R<T>) -> R<T> {
    let deadline = Instant::now() + wait;
    on_stage(stage);
    loop {
        if cancelled() {
            return Err(KeystoneError::new(KeystoneCode::Cancelled));
        }
        match f() {
            Ok(v) => {
                on_stage(Stage::Done);
                return Ok(v);
            }
            Err(e) => {
                // "usb-off" is how the Rust side reports a request refused away from the home screen
                let w = match e.code {
                    KeystoneCode::Locked => Stage::Unlock,
                    KeystoneCode::HomeScreen | KeystoneCode::UsbOff => Stage::HomeScreen,
                    _ => return Err(e),
                };
                if Instant::now() > deadline {
                    return Err(e);
                }
                on_stage(w);
                let until = Instant::now() + poll;
                while Instant::now() < until {
                    if cancelled() {
                        return Err(KeystoneError::new(KeystoneCode::Cancelled));
                    }
                    std::thread::sleep(Duration::from_millis(20).min(poll));
                }
                on_stage(stage);
            }
        }
    }
}

/// A test device (port of keystone/mock.ts): answers key derivation calls and Sui hash requests
/// from a phrase with real Ed25519 signatures, checks the master fingerprint like the firmware,
/// and fails with the transport's "code|message" errors.
pub mod fake {
    use super::*;
    use crate::keys::Keypair;

    pub const TEST_PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    /// BIP32 master fingerprint of the test phrase (BIP84 test vectors)
    pub const TEST_MFP: &str = "73c5da0a";

    #[derive(Debug, Clone)]
    pub struct FakeKeystone {
        pub phrase: String,
        pub mfp: String,
        pub locked: bool,
        pub reject: bool,
        /// not on the home screen: key export is refused like the firmware does
        pub away: bool,
        pub requests: Vec<String>,
        pub hashes: Vec<String>,
    }

    impl Default for FakeKeystone {
        fn default() -> Self {
            FakeKeystone { phrase: TEST_PHRASE.into(), mfp: TEST_MFP.into(), locked: false, reject: false, away: false, requests: vec![], hashes: vec![] }
        }
    }

    fn answer(ur: String) -> String {
        serde_json::json!({ "payload": ur }).to_string()
    }

    impl FakeKeystone {
        /// The transport's answer or its "code|message" error string.
        pub fn handle(&mut self, command: u16, data: &str) -> Result<String, String> {
            match command {
                1 => return Ok(serde_json::json!({ "payload": data }).to_string()),
                3 => return Ok(serde_json::json!({ "payload": if self.locked { "true" } else { "false" } }).to_string()),
                5 => return Ok(serde_json::json!({ "payload": serde_json::json!({ "firmwareVersion": "2.1.4", "walletMFP": self.mfp }).to_string() }).to_string()),
                2 => {}
                _ => return Err(crate::err("error", "unknown command")),
            }
            if self.locked {
                return Err(crate::err("locked", "Unlock your Keystone"));
            }
            let ur = decode_ur(data).ok_or_else(|| crate::err("error", "bad UR"))?;
            self.requests.push(ur.kind.clone());
            let c = Cbor::decode(&ur.cbor).ok_or_else(|| crate::err("error", "bad CBOR"))?;
            match ur.kind.as_str() {
                "qr-hardware-call" => {
                    if self.away {
                        return Err(crate::err("usb-off", "Open the Keystone home screen and turn on USB in Settings > Connection"));
                    }
                    if self.reject {
                        return Err(crate::err("rejected", "Rejected on your Keystone"));
                    }
                    let schemas = c.get(2).and_then(|p| p.get(1)).and_then(Cbor::as_array).ok_or_else(|| crate::err("error", "bad call"))?;
                    let mut keys = Vec::new();
                    for s in schemas {
                        let (path, _) = s.get(1).and_then(read_keypath).ok_or_else(|| crate::err("error", "bad path"))?;
                        let pk = Keypair::from_mnemonic(&self.phrase, crate::keys::path_index(&path).unwrap()).unwrap().public_key();
                        keys.push(Cbor::tag(TAG_HDKEY, Cbor::Map(vec![(3, Cbor::Bytes(pk)), (6, Cbor::tag(TAG_KEYPATH, keypath(&path, None)))])));
                    }
                    let mfp = u32::from_be_bytes(unhex(&self.mfp).unwrap().try_into().unwrap());
                    let accounts = Cbor::Map(vec![(1, Cbor::Uint(mfp as u64)), (2, Cbor::Array(keys)), (3, Cbor::Text("Keystone 3 Pro".into())), (5, Cbor::Text("2.1.4".into()))]);
                    Ok(answer(encode_ur("crypto-multi-accounts", &accounts.encode())))
                }
                "sui-sign-hash-request" => {
                    let kp = c.get(3).and_then(Cbor::as_array).and_then(|a| a.first()).and_then(read_keypath);
                    let Some((path, Some(fp))) = kp else { return Err(crate::err("mismatch", "This Keystone holds a different wallet")) };
                    if fp != self.mfp {
                        return Err(crate::err("mismatch", "This Keystone holds a different wallet"));
                    }
                    let hash = c.get(2).and_then(Cbor::as_text).unwrap_or_default().to_string();
                    self.hashes.push(hash.clone());
                    if self.reject {
                        return Err(crate::err("rejected", "Rejected on your Keystone"));
                    }
                    let Keypair::Ed25519(k) = Keypair::from_mnemonic(&self.phrase, crate::keys::path_index(&path).unwrap()).unwrap() else { unreachable!() };
                    use ed25519_dalek::Signer as _;
                    let sig = k.sign(&unhex(&hash).unwrap()).to_bytes();
                    let mut map = vec![(2, Cbor::Bytes(sig.to_vec())), (3, Cbor::Bytes(k.verifying_key().to_bytes().to_vec()))];
                    if let Some(id) = c.get(1) {
                        map.push((1, id.clone()));
                    }
                    Ok(answer(encode_ur("sui-signature", &Cbor::Map(map).encode())))
                }
                _ => Err(crate::err("error", "Keystone: this view type is not supported")),
            }
        }
    }

    impl KeystoneLink for FakeKeystone {
        fn send(&mut self, command: u16, data: &str, _timeout_ms: u64) -> R<String> {
            self.handle(command, data).map_err(|e| KeystoneError::from_native(&e))
        }
    }
}

#[cfg(test)]
mod tests;
