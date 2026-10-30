//! rare.market's friends API (/v1/friends, /v1/presence, /v1/usernames, /v1/users). Every private
//! request is signed by this device's key (`sign`). All of it is per user and must not be cached,
//! so it goes out with [`Net::call`].
//!
//! [`FriendsApi`] is what the store talks to: [`Remote`] is rare.market, and with the `mock`
//! feature `mock::MockApi` answers from sample friends (the web launcher's `?friends=mock`).

use crate::sign::{link_request, now_ms, DeviceKey};
use crate::types::*;
use crate::username::{self, Availability, Mine};
use rare_market::net::{Call, Method, Net, NetError};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// Messages rare.market sends per page of a conversation.
pub const DM_PAGE: usize = 100;

/// How long one request may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// rare.market answered with an error status: `{ error: { code, message } }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Api(ApiError),
    /// No answer (offline, DNS, timeout).
    Network(String),
    /// The player cancelled the wallet's review sheet. Not shown as an error.
    Rejected(String),
    /// Anything else, as a message for the player.
    Other(String),
}

impl Error {
    /// The wallet's message when the player cancels (`SignRejected`).
    pub fn rejected() -> Error {
        Error::Rejected("You declined the request".into())
    }
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api(a) => Some(a.status),
            _ => None,
        }
    }
    pub fn is_rejected(&self) -> bool {
        matches!(self, Error::Rejected(_))
    }
    pub fn message(&self) -> &str {
        match self {
            Error::Api(a) => &a.message,
            Error::Network(s) | Error::Rejected(s) | Error::Other(s) => s,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

impl From<Error> for rare_core::Error {
    fn from(e: Error) -> Self {
        rare_core::Error(e.message().to_string())
    }
}

impl From<NetError> for Error {
    fn from(e: NetError) -> Self {
        Error::Network(e.to_string())
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::Other(s.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The wallet's side of a personal-message signature, shown on its review sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignPrompt {
    /// The account that signs.
    pub account: String,
    /// Who is asking, as the sheet names it.
    pub origin: String,
    pub title: String,
}

/// Rare Wallet signing one personal message with the account's own key (passkey, Ledger or
/// software key); implemented by `rare-wallet`. It returns the serialized Sui signature (base64
/// `flag || signature || public key`), or [`Error::rejected`] when the player cancels.
pub trait WalletSigner: Send + Sync {
    fn sign_personal_message(&self, message: &[u8], prompt: &SignPrompt) -> Result<String>;
}

/// No wallet: every signature is refused (watch-only sessions, tests that never link).
pub struct NoWallet;

impl WalletSigner for NoWallet {
    fn sign_personal_message(&self, _: &[u8], _: &SignPrompt) -> Result<String> {
        Err(Error::Other("Add a Rare Wallet account first.".into()))
    }
}

/// Sends one request to rare.market. `key` signs it when given; `body` goes as JSON. Any non-2xx
/// answer becomes [`Error::Api`] with the server's code and message.
pub fn send<T: DeserializeOwned, B: Serialize + ?Sized>(net: &dyn Net, key: Option<&DeviceKey>, method: Method, path: &str, body: Option<&B>, now: i64) -> Result<T> {
    let raw = match body {
        Some(b) => serde_json::to_string(b).map_err(|e| Error::Other(e.to_string()))?,
        None => String::new(),
    };
    let url = format!("{}{path}", net.market_origin().trim_end_matches('/'));
    let mut call = Call::new(method, url, if raw.is_empty() { None } else { Some(raw.clone()) }).timeout(TIMEOUT);
    if !raw.is_empty() {
        call = call.header("content-type", "application/json");
    }
    if let Some(k) = key {
        for (n, v) in k.headers(method.as_str(), path, &raw, now) {
            call = call.header(n, v);
        }
    }
    let r = net.call(&call)?;
    let j = r.json();
    if !r.ok() {
        let e = &j["error"];
        return Err(Error::Api(ApiError {
            status: r.status,
            code: e["code"].as_str().unwrap_or("error").to_string(),
            message: e["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("rare.market {}", r.status)),
        }));
    }
    serde_json::from_value(j).map_err(|e| Error::Other(format!("rare.market {path}: {e}")))
}

/// [`send`] with no body.
pub fn send_empty<T: DeserializeOwned>(net: &dyn Net, key: Option<&DeviceKey>, method: Method, path: &str, now: i64) -> Result<T> {
    send::<T, ()>(net, key, method, path, None, now)
}

/// The friends API as the store uses it.
pub trait FriendsApi: Send + Sync {
    /// Sample data instead of rare.market (the web launcher's `?friends=mock`): linking needs no
    /// wallet and watch-only accounts may use friends.
    fn is_mock(&self) -> bool {
        false
    }
    /// Links this device's key to its account: Rare Wallet's review sheet shows the message and
    /// signs it. Returns when the link expires (ms); the caller marks the key linked until then.
    fn link(&self, k: &DeviceKey, now: i64) -> Result<i64>;
    /// Extends a live link with the device key's own signature (the long-lived sign-in renews
    /// itself; no wallet). Returns the new expiry (ms).
    fn renew(&self, _k: &DeviceKey) -> Result<i64> {
        Err(Error::Other("Renewing is not available here.".into()))
    }
    fn unlink(&self, k: &DeviceKey) -> Result<()>;
    fn overview(&self, k: &DeviceKey) -> Result<Overview>;
    fn graph(&self, k: &DeviceKey, op: GraphOp, address: &str) -> Result<GraphState>;
    fn save_settings(&self, k: &DeviceKey, s: &PrivacyPatch) -> Result<Privacy>;
    fn make_code(&self, k: &DeviceKey) -> Result<FriendCode>;
    fn redeem_code(&self, k: &DeviceKey, code: &str) -> Result<Redeemed>;
    fn heartbeat(&self, k: &DeviceKey, b: &Beat) -> Result<Presence>;
    fn create_party(&self, k: &DeviceKey, game: Option<&GameRef>, invite: &[String]) -> Result<Party>;
    fn invite_to_party(&self, k: &DeviceKey, id: &str, addresses: &[String]) -> Result<Party>;
    fn join_party(&self, k: &DeviceKey, id: &str) -> Result<Party>;
    fn leave_party(&self, k: &DeviceKey, id: &str) -> Result<()>;
    fn set_party_game(&self, k: &DeviceKey, id: &str, game: Option<&GameRef>) -> Result<Party>;
    fn messages(&self, k: &DeviceKey, id: &str) -> Result<Vec<Message>>;
    fn post_message(&self, k: &DeviceKey, id: &str, text: &str) -> Result<Message>;
    /// Saves the profile picture on rare.market (`PUT /v1/profile`, `avatar`: a default id or an
    /// NFT id the account holds). Answers when rare.market stamped the pick (epoch ms).
    fn save_avatar(&self, _k: &DeviceKey, _id: &str) -> Result<Option<i64>> {
        Err(Error::Other("not supported here".into()))
    }

    // ---------- direct messages ----------

    /// The last messages with a friend (a page of up to [`DM_PAGE`]), oldest first; `before` (a
    /// message id) pages back.
    fn dms(&self, _k: &DeviceKey, _peer: &str) -> Result<Vec<DirectMessage>> {
        Ok(Vec::new())
    }
    fn dms_before(&self, _k: &DeviceKey, _peer: &str, _before: &str) -> Result<Vec<DirectMessage>> {
        Ok(Vec::new())
    }
    fn send_dm(&self, _k: &DeviceKey, _peer: &str, _text: &str, _attach: Option<&Attach>) -> Result<DirectMessage> {
        Err(Error::Other("Messages are not available here.".into()))
    }
    /// Marks what the friend sent as read.
    fn read_dms(&self, _k: &DeviceKey, _peer: &str) -> Result<()> {
        Ok(())
    }

    // ---------- usernames (username.rs) ----------

    /// Whether a username is free (`address` marks the player's own name as theirs).
    fn check_username(&self, raw: &str, address: Option<&str>) -> Result<Availability>;
    /// Sets (claims or changes) the username for `address`, with the linked key when given, else
    /// one wallet signature.
    fn set_username(&self, address: &str, raw: &str, key: Option<&DeviceKey>) -> Result<Mine>;
    /// Who has this username (None when nobody).
    fn lookup_username(&self, raw: &str) -> Result<Option<Person>>;
}

/// rare.market over a [`Net`], with Rare Wallet for the signatures only it can make.
#[derive(Clone)]
pub struct Remote {
    pub net: Arc<dyn Net>,
    pub wallet: Arc<dyn WalletSigner>,
    /// The clock for request timestamps (tests fix it).
    pub clock: fn() -> i64,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Linked {
    expires_at: i64,
}

#[derive(serde::Deserialize)]
struct List<T> {
    data: Vec<T>,
}

impl Remote {
    pub fn new(net: Arc<dyn Net>, wallet: Arc<dyn WalletSigner>) -> Remote {
        Remote { net, wallet, clock: now_ms }
    }

    fn signed<T: DeserializeOwned, B: Serialize + ?Sized>(&self, k: &DeviceKey, method: Method, path: &str, body: Option<&B>) -> Result<T> {
        send(&*self.net, Some(k), method, path, body, (self.clock)())
    }

    fn signed_empty<T: DeserializeOwned>(&self, k: &DeviceKey, method: Method, path: &str) -> Result<T> {
        send_empty(&*self.net, Some(k), method, path, (self.clock)())
    }
}

impl FriendsApi for Remote {
    fn link(&self, k: &DeviceKey, now: i64) -> Result<i64> {
        let req = link_request(k, now);
        let prompt = SignPrompt { account: k.address.clone(), origin: "Rare friends".into(), title: "Turn on friends".into() };
        let signature = self.wallet.sign_personal_message(req.message.as_bytes(), &prompt)?;
        let body = serde_json::json!({ "address": k.address, "message": req.message, "signature": signature });
        let r: Linked = send(&*self.net, None, Method::Post, "/v1/friends/link", Some(&body), now)?;
        Ok(req.expires.min(r.expires_at))
    }

    fn unlink(&self, k: &DeviceKey) -> Result<()> {
        self.signed_empty::<serde_json::Value>(k, Method::Delete, "/v1/friends/link").map(|_| ())
    }

    fn renew(&self, k: &DeviceKey) -> Result<i64> {
        let r: Linked = self.signed_empty(k, Method::Post, "/v1/friends/link/renew")?;
        Ok(r.expires_at)
    }

    fn overview(&self, k: &DeviceKey) -> Result<Overview> {
        self.signed_empty(k, Method::Get, "/v1/friends")
    }

    fn graph(&self, k: &DeviceKey, op: GraphOp, address: &str) -> Result<GraphState> {
        self.signed(k, Method::Post, &format!("/v1/friends/{}", op.as_str()), Some(&serde_json::json!({ "address": address })))
    }

    fn save_settings(&self, k: &DeviceKey, s: &PrivacyPatch) -> Result<Privacy> {
        self.signed(k, Method::Put, "/v1/friends/settings", Some(s))
    }

    fn make_code(&self, k: &DeviceKey) -> Result<FriendCode> {
        self.signed_empty(k, Method::Post, "/v1/friends/codes")
    }

    fn redeem_code(&self, k: &DeviceKey, code: &str) -> Result<Redeemed> {
        self.signed(k, Method::Post, "/v1/friends/codes/redeem", Some(&serde_json::json!({ "code": code })))
    }

    fn heartbeat(&self, k: &DeviceKey, b: &Beat) -> Result<Presence> {
        self.signed(k, Method::Post, "/v1/presence", Some(b))
    }

    fn create_party(&self, k: &DeviceKey, game: Option<&GameRef>, invite: &[String]) -> Result<Party> {
        self.signed(k, Method::Post, "/v1/friends/parties", Some(&serde_json::json!({ "game": game, "invite": invite })))
    }

    fn invite_to_party(&self, k: &DeviceKey, id: &str, addresses: &[String]) -> Result<Party> {
        self.signed(k, Method::Post, &format!("/v1/friends/parties/{id}/invite"), Some(&serde_json::json!({ "addresses": addresses })))
    }

    fn join_party(&self, k: &DeviceKey, id: &str) -> Result<Party> {
        self.signed_empty(k, Method::Post, &format!("/v1/friends/parties/{id}/join"))
    }

    fn leave_party(&self, k: &DeviceKey, id: &str) -> Result<()> {
        self.signed_empty::<serde_json::Value>(k, Method::Post, &format!("/v1/friends/parties/{id}/leave")).map(|_| ())
    }

    fn set_party_game(&self, k: &DeviceKey, id: &str, game: Option<&GameRef>) -> Result<Party> {
        self.signed(k, Method::Post, &format!("/v1/friends/parties/{id}/game"), Some(&serde_json::json!({ "game": game })))
    }

    fn messages(&self, k: &DeviceKey, id: &str) -> Result<Vec<Message>> {
        self.signed_empty::<List<Message>>(k, Method::Get, &format!("/v1/friends/parties/{id}/messages")).map(|r| r.data)
    }

    fn save_avatar(&self, k: &DeviceKey, id: &str) -> Result<Option<i64>> {
        let r: serde_json::Value = self.signed(k, Method::Put, "/v1/profile", Some(&serde_json::json!({ "avatar": id })))?;
        Ok(r["avatar"]["at"].as_f64().map(|a| a as i64))
    }

    fn post_message(&self, k: &DeviceKey, id: &str, text: &str) -> Result<Message> {
        self.signed(k, Method::Post, &format!("/v1/friends/parties/{id}/messages"), Some(&serde_json::json!({ "text": text })))
    }

    fn dms(&self, k: &DeviceKey, peer: &str) -> Result<Vec<DirectMessage>> {
        self.signed_empty::<List<DirectMessage>>(k, Method::Get, &format!("/v1/friends/dms/{peer}")).map(|r| r.data)
    }

    fn dms_before(&self, k: &DeviceKey, peer: &str, before: &str) -> Result<Vec<DirectMessage>> {
        if !before.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(Vec::new());
        }
        self.signed_empty::<List<DirectMessage>>(k, Method::Get, &format!("/v1/friends/dms/{peer}?before={before}")).map(|r| r.data)
    }

    fn send_dm(&self, k: &DeviceKey, peer: &str, text: &str, attach: Option<&Attach>) -> Result<DirectMessage> {
        self.signed(k, Method::Post, &format!("/v1/friends/dms/{peer}"), Some(&serde_json::json!({ "text": text, "attach": attach })))
    }

    fn read_dms(&self, k: &DeviceKey, peer: &str) -> Result<()> {
        self.signed_empty::<serde_json::Value>(k, Method::Post, &format!("/v1/friends/dms/{peer}/read")).map(|_| ())
    }

    fn check_username(&self, raw: &str, address: Option<&str>) -> Result<Availability> {
        username::check(&*self.net, raw, address, (self.clock)())
    }

    fn set_username(&self, address: &str, raw: &str, key: Option<&DeviceKey>) -> Result<Mine> {
        username::set(&*self.net, &*self.wallet, address, raw, key, (self.clock)())
    }

    fn lookup_username(&self, raw: &str) -> Result<Option<Person>> {
        username::lookup(&*self.net, raw, (self.clock)())
    }
}

/// The best-effort "I left" heartbeat (also on window close): a ready-made signed call the host
/// fires and forgets.
pub fn goodbye_call(origin: &str, k: &DeviceKey, now: i64) -> Call {
    let body = serde_json::to_string(&Beat { state: PresenceState::Offline, game: None, avatar: None }).expect("plain JSON");
    let mut call = Call::new(Method::Post, format!("{}/v1/presence", origin.trim_end_matches('/')), Some(body.clone())).timeout(TIMEOUT).header("content-type", "application/json");
    for (n, v) in k.headers("POST", "/v1/presence", &body, now) {
        call = call.header(n, v);
    }
    call
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::{self, verify_personal_message};
    use rare_market::net::MockNet;

    const ADDR: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

    fn key() -> DeviceKey {
        DeviceKey::from_secret(ADDR, [7; 32], i64::MAX)
    }

    fn header<'a>(c: &'a Call, k: &str) -> Option<&'a str> {
        c.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn signs_the_exact_body_it_sends() {
        let net = MockNet::new();
        net.on_call(Method::Post, "https://rare.market/v1/friends/request", 200, r#"{"state":"pending"}"#);
        let r = Remote::new(Arc::new(net.clone()), Arc::new(NoWallet));
        let k = key();
        assert_eq!(r.graph(&k, GraphOp::Request, "0x2").unwrap().state.as_deref(), Some("pending"));
        let c = &net.calls()[0];
        let body = c.body.as_deref().unwrap();
        assert_eq!(body, r#"{"address":"0x2"}"#);
        assert_eq!(header(c, "content-type"), Some("application/json"));
        let text = sign::request_text("POST", "/v1/friends/request", ADDR, &k.key, header(c, "x-rare-ts").unwrap().parse().unwrap(), header(c, "x-rare-nonce").unwrap(), &sign::body_hash(body));
        assert!(verify_personal_message(text.as_bytes(), header(c, "x-rare-signature").unwrap(), &k.key));
    }

    #[test]
    fn a_get_has_no_body_and_signs_the_empty_hash() {
        let net = MockNet::new();
        net.on_call(Method::Get, "https://rare.market/v1/friends/parties/p1/messages", 200, r#"{"data":[{"id":"1","partyId":"p1","from":"0x1","text":"hi","at":3}]}"#);
        let r = Remote::new(Arc::new(net.clone()), Arc::new(NoWallet));
        let k = key();
        assert_eq!(r.messages(&k, "p1").unwrap()[0].text, "hi");
        let c = &net.calls()[0];
        assert!(c.body.is_none());
        assert_eq!(header(c, "content-type"), None);
        let text = sign::request_text("GET", "/v1/friends/parties/p1/messages", ADDR, &k.key, header(c, "x-rare-ts").unwrap().parse().unwrap(), header(c, "x-rare-nonce").unwrap(), &sign::body_hash(""));
        assert!(verify_personal_message(text.as_bytes(), header(c, "x-rare-signature").unwrap(), &k.key));
    }

    #[test]
    fn saves_the_picture_signed() {
        let net = MockNet::new();
        net.on_call(Method::Put, "https://rare.market/v1/profile", 200, r#"{"address":"0x1","avatar":{"kind":"default","id":"fox","url":"u","at":42}}"#);
        let r = Remote::new(Arc::new(net.clone()), Arc::new(NoWallet));
        assert_eq!(r.save_avatar(&key(), "fox").unwrap(), Some(42));
        let c = &net.calls()[0];
        assert_eq!(c.body.as_deref(), Some(r#"{"avatar":"fox"}"#));
        assert!(header(c, "x-rare-signature").is_some());
    }

    #[test]
    fn errors_carry_the_servers_code_and_message() {
        let net = MockNet::new();
        net.on_call(Method::Get, "https://rare.market/v1/friends", 401, r#"{"error":{"code":"auth","message":"device key not linked"}}"#);
        net.on_call(Method::Post, "https://rare.market/v1/friends/codes", 502, "<html>");
        let r = Remote::new(Arc::new(net.clone()), Arc::new(NoWallet));
        let e = r.overview(&key()).unwrap_err();
        assert_eq!(e, Error::Api(ApiError { status: 401, code: "auth".into(), message: "device key not linked".into() }));
        let e = r.make_code(&key()).unwrap_err();
        assert_eq!(e.to_string(), "rare.market 502");
        assert_eq!(rare_core::Error::from(e).0, "rare.market 502");
    }

    struct TestWallet(DeviceKey, std::sync::Mutex<Vec<SignPrompt>>);
    impl WalletSigner for TestWallet {
        fn sign_personal_message(&self, message: &[u8], prompt: &SignPrompt) -> Result<String> {
            self.1.lock().unwrap().push(prompt.clone());
            Ok(self.0.sign(message))
        }
    }

    #[test]
    fn links_with_one_wallet_signature_over_the_link_text() {
        let wallet = DeviceKey::from_secret("0x0", [9; 32], 0);
        let w = Arc::new(TestWallet(wallet.clone(), Default::default()));
        let net = MockNet::new();
        let now = 1_792_972_800_000;
        net.on_call(Method::Post, "https://rare.market/v1/friends/link", 200, format!(r#"{{"expiresAt":{}}}"#, now + 1000));
        let r = Remote::new(Arc::new(net.clone()), w.clone());
        let k = DeviceKey::from_secret(&wallet.key, [7; 32], 0);
        assert_eq!(r.link(&k, now).unwrap(), now + 1000);
        assert_eq!(w.1.lock().unwrap()[0], SignPrompt { account: wallet.key.clone(), origin: "Rare friends".into(), title: "Turn on friends".into() });
        let c = &net.calls()[0];
        assert_eq!(header(c, "x-rare-signature"), None);
        let b: serde_json::Value = serde_json::from_str(c.body.as_deref().unwrap()).unwrap();
        assert_eq!(b["message"], link_request(&k, now).message);
        assert!(verify_personal_message(b["message"].as_str().unwrap().as_bytes(), b["signature"].as_str().unwrap(), &wallet.key));
    }

    #[test]
    fn a_cancelled_sheet_sends_nothing() {
        struct No;
        impl WalletSigner for No {
            fn sign_personal_message(&self, _: &[u8], _: &SignPrompt) -> Result<String> {
                Err(Error::rejected())
            }
        }
        let net = MockNet::new();
        let r = Remote::new(Arc::new(net.clone()), Arc::new(No));
        assert!(r.link(&key(), 0).unwrap_err().is_rejected());
        assert!(net.calls().is_empty());
    }

    #[test]
    fn heartbeats_leave_the_avatar_out_unless_it_changed() {
        let b = Beat { state: PresenceState::Online, game: None, avatar: None };
        assert_eq!(serde_json::to_string(&b).unwrap(), r#"{"state":"online","game":null}"#);
        let b = Beat { avatar: Some(None), ..b };
        assert_eq!(serde_json::to_string(&b).unwrap(), r#"{"state":"online","game":null,"avatar":null}"#);
        let c = goodbye_call("https://rare.market", &key(), 5);
        assert_eq!(c.body.as_deref(), Some(r#"{"state":"offline","game":null}"#));
        assert_eq!(header(&c, "x-rare-ts"), Some("5"));
    }
}
