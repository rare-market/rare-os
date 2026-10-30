//! Signature auth for friends (server side: market api/src/friends/auth.ts). No passwords, no email.
//!
//! The launcher keeps one Ed25519 device key per Rare Wallet account. Rare Wallet signs one personal
//! message (the review sheet shows it) that links the key to the account for 90 days; after that every
//! request to /v1/friends and /v1/presence is signed by the device key, so a passkey or Ledger is not
//! asked on every heartbeat. The device key cannot move funds: the server only accepts it here.
//!
//! Signatures are Sui personal-message signatures, byte for byte what `@mysten/sui`'s
//! `Ed25519Keypair.signPersonalMessage` makes, because the server checks them with
//! `verifyPersonalMessageSignature`: the message is BCS `vector<u8>` (ULEB128 length, then the
//! bytes), prefixed with the PersonalMessage intent `[3, 0, 0]`, hashed with BLAKE2b-256, and the
//! digest signed. The result is base64 of `0x00 || signature || public key`. The key's name on the
//! wire is its Sui address: BLAKE2b-256 of `0x00 || public key`, as 0x hex.

use crate::storage::Storage;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub const PROTO: &str = "rare-friends/1";
pub const DAY_MS: i64 = 86_400_000;
/// How long a link lasts.
pub const LINK_MS: i64 = 90 * DAY_MS;
/// A link with less than this left is renewed with the device key itself (no wallet signature).
pub const RENEW_MS: i64 = 30 * DAY_MS;
/// The Ed25519 flag in Sui's signature and key encodings.
const ED25519_FLAG: u8 = 0x00;
/// The PersonalMessage intent: scope 3, version 0, app id Sui.
const PERSONAL_MESSAGE_INTENT: [u8; 3] = [3, 0, 0];
const SUI_PRIVATE_KEY_PREFIX: &str = "suiprivkey";

type Blake2b256 = Blake2b<U32>;

/// The exact text the wallet signs (auth.ts linkText; the server rebuilds it to compare).
pub fn link_text(address: &str, key: &str, issued: &str, expires: &str) -> String {
    [
        "Rare friends",
        "",
        "Let this device handle your friends, parties and presence in Rare.",
        "",
        &format!("Address: {address}"),
        &format!("Device key: {key}"),
        &format!("Issued: {issued}"),
        &format!("Expires: {expires}"),
        "",
        "This does not move funds, send a transaction or cost gas.",
    ]
    .join("\n")
}

/// The text one request is signed over. `path` includes the query.
pub fn request_text(method: &str, path: &str, address: &str, key: &str, ts: i64, nonce: &str, body_hash: &str) -> String {
    [PROTO, &format!("{} {path}", method.to_uppercase()), address, key, &ts.to_string(), nonce, body_hash].join("\n")
}

/// The text the notify stream's `social` token is signed over.
pub fn stream_text(address: &str, key: &str, ts: i64) -> String {
    [PROTO, "STREAM", address, key, &ts.to_string()].join("\n")
}

/// SHA-256 hex of the raw request body, as the server hashes it.
pub fn body_hash(body: &str) -> String {
    hex::encode(Sha256::digest(body.as_bytes()))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("the system random source failed");
    b
}

/// 12 random bytes, base64url without padding (16 characters).
pub fn nonce() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<12>())
}

fn blake2b256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Blake2b256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn uleb128(mut n: usize, out: &mut Vec<u8>) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// A Sui personal-message signature of `message` by `sk` (base64, flag || signature || key).
pub fn sign_personal_message(sk: &SigningKey, message: &[u8]) -> String {
    let mut intent = PERSONAL_MESSAGE_INTENT.to_vec();
    uleb128(message.len(), &mut intent);
    intent.extend_from_slice(message);
    let digest = blake2b256(&[&intent]);
    let sig = sk.sign(&digest);
    let mut out = Vec::with_capacity(97);
    out.push(ED25519_FLAG);
    out.extend_from_slice(&sig.to_bytes());
    out.extend_from_slice(sk.verifying_key().as_bytes());
    STANDARD.encode(out)
}

/// Checks a personal-message signature the way the server does: the signature verifies over the
/// message and its public key's Sui address is `address`. For tests and tools.
pub fn verify_personal_message(message: &[u8], signature: &str, address: &str) -> bool {
    let Ok(raw) = STANDARD.decode(signature) else { return false };
    if raw.len() != 97 || raw[0] != ED25519_FLAG {
        return false;
    }
    let Ok(pk) = ed25519_dalek::VerifyingKey::from_bytes(raw[65..97].try_into().unwrap()) else { return false };
    if sui_address(&pk) != address {
        return false;
    }
    let mut intent = PERSONAL_MESSAGE_INTENT.to_vec();
    uleb128(message.len(), &mut intent);
    intent.extend_from_slice(message);
    let digest = blake2b256(&[&intent]);
    let sig = ed25519_dalek::Signature::from_bytes(raw[1..65].try_into().unwrap());
    pk.verify_strict(&digest, &sig).is_ok()
}

/// An Ed25519 public key's Sui address.
pub fn sui_address(pk: &ed25519_dalek::VerifyingKey) -> String {
    format!("0x{}", hex::encode(blake2b256(&[&[ED25519_FLAG], pk.as_bytes()])))
}

/// The secret in Sui's `suiprivkey1...` form (what `Ed25519Keypair.getSecretKey()` returns), so a
/// key moved between the web and native launchers reads the same.
pub fn encode_secret(secret: &[u8; 32]) -> String {
    let mut data = Vec::with_capacity(33);
    data.push(ED25519_FLAG);
    data.extend_from_slice(secret);
    let hrp = bech32::Hrp::parse(SUI_PRIVATE_KEY_PREFIX).expect("valid prefix");
    bech32::encode::<bech32::Bech32>(hrp, &data).expect("33 bytes fit bech32")
}

/// The 32-byte Ed25519 secret from a `suiprivkey1...` string; None for anything else.
pub fn decode_secret(s: &str) -> Option<[u8; 32]> {
    let (hrp, data) = bech32::decode(s).ok()?;
    if hrp.as_str() != SUI_PRIVATE_KEY_PREFIX || data.len() != 33 || data[0] != ED25519_FLAG {
        return None;
    }
    data[1..].try_into().ok()
}

/// A time in ms as JavaScript's `toISOString()` writes it: `2026-10-26T00:00:00.000Z`.
pub fn iso_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let milli = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{milli:03}Z", sod / 3600, sod % 3600 / 60, sod % 60)
}

/// Milliseconds since the Unix epoch (`Date.now()`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---------- the device key, per account ----------

/// What is stored per account (the same JSON the web launcher keeps in localStorage).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    secret: String,
    /// ms; 0 until the wallet linked it
    linked_until: i64,
    /// ms; when the wallet linked it (0: unknown or not linked)
    #[serde(default, skip_serializing_if = "is_zero")]
    linked_at: i64,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

/// One account's rare.market sign-in on this device, read without making a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    /// ms; 0 when not known (linked before this was kept)
    pub linked_at: i64,
    pub linked_until: i64,
}

impl Session {
    pub fn live(&self, now: i64) -> bool {
        self.linked_until > now
    }
}

/// The sign-in kept for `address`, if any (linked or not).
pub fn session_of(store: &dyn Storage, address: &str) -> Option<Session> {
    let s = store.get(&storage_key(address)).and_then(|v| serde_json::from_str::<Stored>(&v).ok())?;
    (s.linked_until > 0).then_some(Session { linked_at: s.linked_at, linked_until: s.linked_until })
}

/// The stored key for `address` when it exists (never makes one).
pub fn stored_key(store: &dyn Storage, address: &str) -> Option<DeviceKey> {
    let s = store.get(&storage_key(address)).and_then(|v| serde_json::from_str::<Stored>(&v).ok())?;
    let secret = decode_secret(&s.secret)?;
    let mut k = DeviceKey::from_secret(address, secret, s.linked_until);
    k.linked_at = s.linked_at;
    Some(k)
}

fn storage_key(address: &str) -> String {
    format!("rare.friends.key.{}", address.to_lowercase())
}

#[derive(Clone)]
pub struct DeviceKey {
    /// The account, lowercase.
    pub address: String,
    sk: SigningKey,
    /// The device key's own Sui address (its name on the wire).
    pub key: String,
    pub linked_until: i64,
    /// ms; when the wallet linked it (0: unknown)
    pub linked_at: i64,
}

impl std::fmt::Debug for DeviceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceKey").field("address", &self.address).field("key", &self.key).field("linked_until", &self.linked_until).finish()
    }
}

impl PartialEq for DeviceKey {
    fn eq(&self, o: &Self) -> bool {
        self.address == o.address && self.key == o.key && self.linked_until == o.linked_until
    }
}

/// Headers for one signed request, in the order the web launcher sends them.
pub type Headers = Vec<(String, String)>;

#[derive(Serialize)]
struct StreamToken<'a> {
    a: &'a str,
    k: &'a str,
    t: i64,
    s: &'a str,
}

impl DeviceKey {
    pub fn from_secret(address: &str, secret: [u8; 32], linked_until: i64) -> DeviceKey {
        let sk = SigningKey::from_bytes(&secret);
        let key = sui_address(&sk.verifying_key());
        DeviceKey { address: address.to_lowercase(), sk, key, linked_until, linked_at: 0 }
    }

    /// A new random key for `address`, not linked.
    pub fn generate(address: &str) -> DeviceKey {
        Self::from_secret(address, random_bytes::<32>(), 0)
    }

    /// Linked and more than a day left.
    pub fn linked(&self, now: i64) -> bool {
        self.linked_until > now + DAY_MS
    }

    /// The secret in `suiprivkey1...` form.
    pub fn secret(&self) -> String {
        encode_secret(&self.sk.to_bytes())
    }

    /// A personal-message signature by this device key.
    pub fn sign(&self, message: &[u8]) -> String {
        sign_personal_message(&self.sk, message)
    }

    /// Headers for one signed request. `path` includes the query.
    pub fn headers(&self, method: &str, path: &str, body: &str, now: i64) -> Headers {
        self.headers_with_nonce(method, path, body, now, &nonce())
    }

    /// [`DeviceKey::headers`] with a given nonce (tests compare against the TypeScript output).
    pub fn headers_with_nonce(&self, method: &str, path: &str, body: &str, now: i64, nonce: &str) -> Headers {
        let text = request_text(method, path, &self.address, &self.key, now, nonce, &body_hash(body));
        let signature = self.sign(text.as_bytes());
        vec![
            ("x-rare-address".into(), self.address.clone()),
            ("x-rare-key".into(), self.key.clone()),
            ("x-rare-ts".into(), now.to_string()),
            ("x-rare-nonce".into(), nonce.to_string()),
            ("x-rare-signature".into(), signature),
        ]
    }

    /// The notify stream's `social` token (valid 12 hours on the server).
    pub fn stream_token(&self, now: i64) -> String {
        let s = self.sign(stream_text(&self.address, &self.key, now).as_bytes());
        let json = serde_json::to_string(&StreamToken { a: &self.address, k: &self.key, t: now, s: &s }).expect("plain JSON");
        URL_SAFE_NO_PAD.encode(json)
    }
}

/// This device's key for the account, made (and stored) on first use.
pub fn device_key(store: &dyn Storage, address: &str) -> DeviceKey {
    if let Some(s) = store.get(&storage_key(address)).and_then(|v| serde_json::from_str::<Stored>(&v).ok()) {
        if let Some(secret) = decode_secret(&s.secret) {
            let mut k = DeviceKey::from_secret(address, secret, s.linked_until);
            k.linked_at = s.linked_at;
            return k;
        }
        // corrupt: make a new one
    }
    let k = DeviceKey::generate(address);
    write(store, &k);
    k
}

fn write(store: &dyn Storage, k: &DeviceKey) {
    let s = serde_json::to_string(&Stored { secret: k.secret(), linked_until: k.linked_until, linked_at: k.linked_at }).expect("plain JSON");
    store.set(&storage_key(&k.address), Some(&s));
}

pub fn mark_linked(store: &dyn Storage, k: &mut DeviceKey, until: i64) {
    // a fresh link (or one from before this was kept) starts now; a renewal keeps its start
    if k.linked_at == 0 || k.linked_until <= now_ms() {
        k.linked_at = now_ms();
    }
    k.linked_until = until;
    write(store, k);
}

/// Forget the key (sign out of friends on this device).
pub fn forget_key(store: &dyn Storage, address: &str) {
    store.set(&storage_key(address), None);
}

/// The link request for the wallet to sign: the message and when it expires (ms).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkRequest {
    pub message: String,
    pub expires: i64,
}

/// The link request for the wallet to sign: 90 days from now.
pub fn link_request(k: &DeviceKey, now: i64) -> LinkRequest {
    let expires = now + LINK_MS;
    LinkRequest { message: link_text(&k.address, &k.key, &iso_time(now), &iso_time(expires)), expires }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    const ADDR: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
    const TS: i64 = 1_800_000_000_000;

    fn header<'a>(h: &'a Headers, k: &str) -> &'a str {
        &h.iter().find(|(n, _)| n == k).unwrap().1
    }

    #[test]
    fn signs_each_request_over_method_path_address_key_time_nonce_and_body_hash() {
        let store = MemoryStorage::new();
        let k = device_key(&store, ADDR);
        let body = r#"{"address":"0x2"}"#;
        let h = k.headers("POST", "/v1/friends/request", body, TS);
        assert_eq!(header(&h, "x-rare-address"), ADDR);
        assert_eq!(header(&h, "x-rare-key"), k.key);
        let n = header(&h, "x-rare-nonce");
        assert!((12..=64).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "{n}");
        let text = request_text("POST", "/v1/friends/request", ADDR, &k.key, TS, n, &body_hash(body));
        // The server's check: the device key's own address signed exactly this text.
        assert!(verify_personal_message(text.as_bytes(), header(&h, "x-rare-signature"), &k.key));
        assert!(!verify_personal_message(text.replace("request", "block").as_bytes(), header(&h, "x-rare-signature"), &k.key));
    }

    #[test]
    fn uses_a_fresh_nonce_every_time() {
        let store = MemoryStorage::new();
        let k = device_key(&store, ADDR);
        let a = k.headers("GET", "/v1/friends", "", now_ms());
        let b = k.headers("GET", "/v1/friends", "", now_ms());
        assert_ne!(header(&a, "x-rare-nonce"), header(&b, "x-rare-nonce"));
    }

    #[test]
    fn hashes_the_body_as_the_server_does() {
        assert_eq!(body_hash(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn builds_a_stream_token_the_server_can_read() {
        let store = MemoryStorage::new();
        let k = device_key(&store, ADDR);
        let t = k.stream_token(TS);
        let j: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&t).unwrap()).unwrap();
        assert_eq!(j["a"], ADDR);
        assert_eq!(j["k"], k.key.as_str());
        assert_eq!(j["t"], TS);
        assert!(verify_personal_message(stream_text(ADDR, &k.key, TS).as_bytes(), j["s"].as_str().unwrap(), &k.key));
    }

    #[test]
    fn asks_the_wallet_to_sign_the_exact_link_text_the_server_rebuilds() {
        let store = MemoryStorage::new();
        let k = device_key(&store, ADDR);
        // Date.UTC(2026, 9, 26)
        let r = link_request(&k, 1_792_972_800_000);
        let want = [
            "Rare friends".to_string(),
            "".into(),
            "Let this device handle your friends, parties and presence in Rare.".into(),
            "".into(),
            format!("Address: {ADDR}"),
            format!("Device key: {}", k.key),
            "Issued: 2026-10-26T00:00:00.000Z".into(),
            "Expires: 2027-01-24T00:00:00.000Z".into(),
            "".into(),
            "This does not move funds, send a transaction or cost gas.".into(),
        ]
        .join("\n");
        assert_eq!(r.message, want);
        assert_ne!(k.key, ADDR);
    }

    // Byte-for-byte against @mysten/sui 2.38 (Ed25519Keypair.fromSecretKey(bytes 1..=32), run once
    // with node): the key's address, its suiprivkey form, a request signature and a stream token.
    #[test]
    fn matches_the_typescript_signatures_byte_for_byte() {
        let secret: [u8; 32] = std::array::from_fn(|i| i as u8 + 1);
        let k = DeviceKey::from_secret(ADDR, secret, 0);
        assert_eq!(k.key, "0x7573c697fa68450f04fa0dee2d39dcdc8a5ccf5db547f3e47638a6f8eeeec110");
        assert_eq!(k.secret(), "suiprivkey1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0jqa4ffsr");
        assert_eq!(decode_secret(&k.secret()), Some(secret));
        let h = k.headers_with_nonce("post", "/v1/friends/request", "", TS, "AAECAwQFBgcICQoL");
        assert_eq!(
            header(&h, "x-rare-signature"),
            "AG0EM0Fhyr5PMP/aeZzsHyRD+0SwEeSYyMt9WNWmG+eswfmv62RLntI0lrdmDZgys7+MRKHkKOKOIcc+Gp3WHgh5tVYuj+ZU+UB4sRLoqYunkB+FOuaVvtfg45ELrQSWZA=="
        );
        assert_eq!(
            k.stream_token(TS),
            "eyJhIjoiMHhhMDQ5MGY2ZmUwOTE4MzcwMGUyYWY2NDg0YjdmYjJhMDlhMTJmMGY2OTFmNWI4ZTU1ODJjNDc4MmQ2ZGU4ODdlIiwiayI6IjB4NzU3M2M2OTdmYTY4NDUwZjA0ZmEwZGVlMmQzOWRjZGM4YTVjY2Y1ZGI1NDdmM2U0NzYzOGE2ZjhlZWVlYzExMCIsInQiOjE4MDAwMDAwMDAwMDAsInMiOiJBRVcwN0p5aWhObUZYengxalZ3MXN2SlFuLzZRS1Z2RFZxUXphT3dqeUNMdGd3bmV1QVFZenJYRU9oM09YWjFMVXlFS2xMYUcyMit4SUFWcWEvdFVTd2w1dFZZdWorWlUrVUI0c1JMb3FZdW5rQitGT3VhVnZ0Zmc0NUVMclFTV1pBPT0ifQ"
        );
    }

    #[test]
    fn keeps_one_key_per_account_until_forgotten() {
        let store = MemoryStorage::new();
        let mut a = device_key(&store, ADDR);
        assert!(!a.linked(TS));
        mark_linked(&store, &mut a, TS + LINK_MS);
        let b = device_key(&store, &ADDR.to_uppercase().replace("0X", "0x"));
        assert_eq!(a, b);
        assert!(b.linked(TS));
        assert!(!b.linked(TS + LINK_MS - DAY_MS));
        forget_key(&store, ADDR);
        assert_ne!(device_key(&store, ADDR).key, a.key);
        store.set(&storage_key(ADDR), Some("{\"secret\":\"junk\",\"linkedUntil\":5}"));
        assert_eq!(device_key(&store, ADDR).linked_until, 0);
    }

    #[test]
    fn writes_times_as_javascript_does() {
        assert_eq!(iso_time(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(951_782_400_123), "2000-02-29T00:00:00.123Z");
        assert_eq!(iso_time(1_800_000_000_000), "2027-01-15T08:00:00.000Z");
    }
}
