//! One door to the network for the market, trade, friends and notify crates.
//!
//! - Reads of display data and transaction inputs go through `rare-state`'s cache by key
//!   (`rare:/v1/...` for rare.market, `trade:/api/...` for the Trade API's read-only paths), so a
//!   screen seen before draws at once and the host's freshness policy decides what is refetched.
//! - Writes (POST/PUT/DELETE), signed requests and anything per-user that must not be cached go
//!   straight out with [`Net::call`].
//! - The notice stream (SSE) is a long-lived GET read line by line ([`Net::stream`]).
//!
//! [`StoreNet`] is the real one; [`MockNet`] answers from canned bodies and records every call, for
//! the ported tests.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// rare.market on mainnet. Keys for it are `rare:<path>`.
pub const MARKET: &str = "https://rare.market";
/// The Trade API (the Trade API). Keys for its read-only paths are `trade:<path>`.
pub const TRADE: &str = "https://rare.market/v1/trade";
/// Images the Trade answers point at by path (logos such as /coins/usdc.png).
pub const TRADE_ASSETS: &str = "https://rare.market/v1/trade/asset";
pub const USER_AGENT: &str = "RareLauncher/0.1";
/// The test-run header value for a rare.market URL (`RARE_TEST_TOKEN`), see `rare_state::test_token`.
pub use rare_state::{test_token, trace};
/// Older than this, display data is fetched before it is shown (with the cached copy if offline),
/// as `instant::read` does for the web UI.
pub const MAX_STALE: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// An uncached request.
#[derive(Clone, Debug)]
pub struct Call {
    pub method: Method,
    /// Absolute URL.
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// JSON text (sent with `content-type: application/json`).
    pub body: Option<String>,
    pub timeout: Duration,
}

impl Call {
    pub fn get(url: impl Into<String>) -> Self {
        Call { method: Method::Get, url: url.into(), headers: Vec::new(), body: None, timeout: Duration::from_secs(20) }
    }
    pub fn post(url: impl Into<String>, json: impl Into<String>) -> Self {
        Call { method: Method::Post, url: url.into(), headers: Vec::new(), body: Some(json.into()), timeout: Duration::from_secs(25) }
    }
    pub fn new(method: Method, url: impl Into<String>, body: Option<String>) -> Self {
        Call { method, url: url.into(), headers: Vec::new(), body, timeout: Duration::from_secs(20) }
    }
    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }
}

/// Any status comes back; only network failures are errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// The body text ("" for 204 or an empty body).
    pub body: String,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    /// The body as JSON (`null` when empty or not JSON), as the web UI's `{ status, body }`.
    pub fn json(&self) -> serde_json::Value {
        if self.body.is_empty() {
            return serde_json::Value::Null;
        }
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// No answer (DNS, connect, timeout, reset).
    Network(String),
    /// A cached read whose fetch answered with an error status, or a body that is not JSON.
    Status(String),
    /// The key or URL is not one this crate may ask for.
    NotAllowed(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Network(s) | NetError::Status(s) | NetError::NotAllowed(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for NetError {}

impl From<NetError> for rare_core::Error {
    fn from(e: NetError) -> Self {
        rare_core::Error(e.to_string())
    }
}

/// A line reader over a streaming response (SSE).
pub type LineStream = Box<dyn Iterator<Item = std::io::Result<String>> + Send>;

pub trait Net: Send + Sync {
    /// A cached read by key (`rare:/v1/...`, `rare:https://...` for testnet, `trade:/api/...`), served
    /// by the host's freshness policy. The JSON text exactly as fetched.
    fn read(&self, key: &str) -> Result<Arc<str>, NetError>;
    /// Fetches a key now, whatever is cached.
    fn refresh(&self, key: &str) -> Result<Arc<str>, NetError> {
        self.read(key)
    }
    /// What is cached for a key, without fetching.
    fn peek(&self, key: &str) -> Option<Arc<str>>;
    /// Keeps state the launcher computes itself (`ui:` keys) for the next start, so the first
    /// frame after a restart draws it (the web UI's `persist` / `kept`).
    fn put(&self, key: &str, _json: &str) -> Result<(), NetError> {
        Err(NetError::NotAllowed(format!("{key}: this net keeps no local state")))
    }
    /// An uncached request.
    fn call(&self, call: &Call) -> Result<Reply, NetError>;
    /// A long-lived GET read line by line (the notice stream). Errors with the status when the
    /// server answers anything but 2xx.
    fn stream(&self, call: &Call) -> Result<LineStream, NetError>;
    /// rare.market's origin for the current network (`https://rare.market` on mainnet).
    fn market_origin(&self) -> String {
        MARKET.to_string()
    }
}

/// The cache key for a rare.market path under `origin` (mainnet keys are short, others carry the URL),
/// matching the Tauri host's `instant::market_key`.
pub fn market_key(origin: &str, path: &str) -> String {
    if origin.trim_end_matches('/') == MARKET {
        format!("rare:{path}")
    } else {
        format!("rare:{}{path}", origin.trim_end_matches('/'))
    }
}

/// Paths the cache may read from rare.market (as the host's `market_path_ok`).
pub fn market_path_ok(path: &str) -> bool {
    (path.starts_with("/v1/") || path.starts_with("/games/") || path == "/launchpad/launches.json") && !path.contains("..")
}

/// Reads a rare.market path through the cache under the net's network.
pub fn read_market(net: &dyn Net, path: &str) -> Result<Arc<str>, NetError> {
    if !market_path_ok(path) {
        return Err(NetError::NotAllowed(format!("path not allowed: {path}")));
    }
    net.read(&market_key(&net.market_origin(), path))
}

/// [`read_market`] parsed into `T`.
pub fn get_market<T: serde::de::DeserializeOwned>(net: &dyn Net, path: &str) -> Result<T, NetError> {
    let body = read_market(net, path)?;
    serde_json::from_str(&body).map_err(|e| NetError::Status(format!("{path}: {e}")))
}

/// A key's cached body parsed into `T`, if any.
pub fn peek_market<T: serde::de::DeserializeOwned>(net: &dyn Net, path: &str) -> Option<T> {
    let body = net.peek(&market_key(&net.market_origin(), path))?;
    serde_json::from_str(&body).ok()
}

// ---------- StoreNet ----------

/// The real net: cached reads through a `rare_state::Store` whose resolver knows `rare:` and `trade:`
/// keys (the host's), everything else over one ureq agent.
#[derive(Clone)]
pub struct StoreNet {
    pub store: rare_state::Store,
    agent: ureq::Agent,
    origin: Arc<Mutex<String>>,
}

impl StoreNet {
    pub fn new(store: rare_state::Store) -> Self {
        let agent = ureq::AgentBuilder::new().user_agent(USER_AGENT).max_idle_connections_per_host(4).build();
        StoreNet { store, agent, origin: Arc::new(Mutex::new(MARKET.to_string())) }
    }
    /// Points market reads at another rare.market (testnet).
    pub fn set_market_origin(&self, origin: &str) {
        *self.origin.lock().unwrap() = origin.trim_end_matches('/').to_string();
    }

    fn call_inner(&self, call: &Call) -> Result<Reply, NetError> {
        let req = self.request(call);
        let res = match &call.body {
            Some(b) => req.set("content-type", "application/json").send_string(b),
            None if call.method == Method::Get || call.method == Method::Delete => req.call(),
            None => req.send_string(""),
        };
        let resp = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(NetError::Network(e.to_string())),
        };
        let status = resp.status();
        let mut body = String::new();
        if status != 204 {
            resp.into_reader().take(32 << 20).read_to_string(&mut body).map_err(|e| NetError::Network(e.to_string()))?;
        }
        Ok(Reply { status, body })
    }

    fn stream_inner(&self, call: &Call) -> Result<LineStream, NetError> {
        let req = self.request(call).set("accept", "text/event-stream");
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return Err(NetError::Status(format!("{}: status code {code}", call.url))),
            Err(e) => return Err(NetError::Network(e.to_string())),
        };
        Ok(Box::new(BufReader::new(resp.into_reader()).lines()))
    }

    fn request(&self, call: &Call) -> ureq::Request {
        let mut r = self.agent.request(call.method.as_str(), &call.url).timeout(call.timeout);
        for (k, v) in &call.headers {
            r = r.set(k, v);
        }
        if let Some(t) = rare_state::test_token(&call.url) {
            r = r.set("x-rare-test", &t);
        }
        r
    }
}

impl Net for StoreNet {
    fn read(&self, key: &str) -> Result<Arc<str>, NetError> {
        use rare_state::Freshness;
        let f = self.store.policy_for(key);
        let f = match (f, self.store.peek(key)) {
            (Freshness::Swr { .. }, Some(h)) if h.age() > MAX_STALE => Freshness::Fresh { ttl: Duration::ZERO, offline_ok: true },
            _ => f,
        };
        self.store.get_with(key, f).map(|h| h.body).map_err(classify)
    }

    fn refresh(&self, key: &str) -> Result<Arc<str>, NetError> {
        self.store.refresh(key).map(|h| h.body).map_err(classify)
    }

    fn peek(&self, key: &str) -> Option<Arc<str>> {
        self.store.peek(key).map(|h| h.body)
    }

    fn put(&self, key: &str, json: &str) -> Result<(), NetError> {
        self.store.put(key, json).map_err(NetError::Status)
    }

    fn call(&self, call: &Call) -> Result<Reply, NetError> {
        let t0 = std::time::Instant::now();
        let r = self.call_inner(call);
        rare_state::trace(t0, r.as_ref().map(|x| x.status).unwrap_or(0), r.as_ref().map(|x| x.body.len()).unwrap_or(0), &call.url);
        r
    }

    fn stream(&self, call: &Call) -> Result<LineStream, NetError> {
        self.stream_inner(call)
    }

    fn market_origin(&self) -> String {
        self.origin.lock().unwrap().clone()
    }
}

/// rare-state reports errors as text: "<url>: status code N" for HTTP errors, "not JSON", else the
/// transport's message.
fn classify(e: String) -> NetError {
    if e.contains("status code") || e.contains("not JSON") || e.contains("not cached") {
        NetError::Status(e)
    } else {
        NetError::Network(e)
    }
}

// ---------- MockNet ----------

/// Canned answers for tests. Keys answer `read`; `(METHOD, url prefix)` pairs answer `call` (the
/// longest matching prefix wins; a queue answers in order, its last answer repeating).
#[derive(Clone, Default)]
pub struct MockNet {
    inner: Arc<Mutex<MockInner>>,
}

#[derive(Default)]
struct MockInner {
    keys: HashMap<String, Result<String, NetError>>,
    calls: Vec<(String, String, Vec<Reply>)>,
    streams: Vec<(String, Vec<String>)>,
    log: Vec<Call>,
    reads: Vec<String>,
    origin: Option<String>,
}

impl MockNet {
    pub fn new() -> Self {
        Self::default()
    }
    /// Answers a cached read of `rare:<path>` (or any full key containing ':').
    pub fn on_read(&self, key_or_path: &str, json: impl Into<String>) -> &Self {
        let key = if key_or_path.starts_with('/') { format!("rare:{key_or_path}") } else { key_or_path.to_string() };
        self.inner.lock().unwrap().keys.insert(key, Ok(json.into()));
        self
    }
    /// [`MockNet::on_read`] with a JSON value.
    pub fn on_read_json(&self, key_or_path: &str, v: serde_json::Value) -> &Self {
        self.on_read(key_or_path, v.to_string())
    }
    pub fn fail_read(&self, key_or_path: &str, e: NetError) -> &Self {
        let key = if key_or_path.starts_with('/') { format!("rare:{key_or_path}") } else { key_or_path.to_string() };
        self.inner.lock().unwrap().keys.insert(key, Err(e));
        self
    }
    /// Answers calls whose method matches and whose URL starts with `url_prefix`.
    pub fn on_call(&self, method: Method, url_prefix: &str, status: u16, body: impl Into<String>) -> &Self {
        let mut g = self.inner.lock().unwrap();
        let m = method.as_str().to_string();
        let reply = Reply { status, body: body.into() };
        if let Some(e) = g.calls.iter_mut().find(|(mm, p, _)| *mm == m && p == url_prefix) {
            e.2.push(reply);
        } else {
            g.calls.push((m, url_prefix.to_string(), vec![reply]));
        }
        self
    }
    /// Lines a stream to `url_prefix` yields before it ends.
    pub fn on_stream(&self, url_prefix: &str, lines: &[&str]) -> &Self {
        self.inner.lock().unwrap().streams.push((url_prefix.to_string(), lines.iter().map(|s| s.to_string()).collect()));
        self
    }
    pub fn set_origin(&self, origin: &str) {
        self.inner.lock().unwrap().origin = Some(origin.to_string());
    }
    /// Every uncached call made, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.inner.lock().unwrap().log.clone()
    }
    /// Every key read, in order.
    pub fn reads(&self) -> Vec<String> {
        self.inner.lock().unwrap().reads.clone()
    }
}

impl Net for MockNet {
    fn read(&self, key: &str) -> Result<Arc<str>, NetError> {
        let mut g = self.inner.lock().unwrap();
        g.reads.push(key.to_string());
        match g.keys.get(key) {
            Some(Ok(b)) => Ok(Arc::from(b.as_str())),
            Some(Err(e)) => Err(e.clone()),
            None => Err(NetError::Status(format!("{key}: status code 404"))),
        }
    }
    fn peek(&self, key: &str) -> Option<Arc<str>> {
        match self.inner.lock().unwrap().keys.get(key) {
            Some(Ok(b)) => Some(Arc::from(b.as_str())),
            _ => None,
        }
    }
    fn put(&self, key: &str, json: &str) -> Result<(), NetError> {
        if serde_json::from_str::<serde::de::IgnoredAny>(json).is_err() {
            return Err(NetError::Status("not JSON".into()));
        }
        self.inner.lock().unwrap().keys.insert(key.to_string(), Ok(json.to_string()));
        Ok(())
    }
    fn call(&self, call: &Call) -> Result<Reply, NetError> {
        let mut g = self.inner.lock().unwrap();
        g.log.push(call.clone());
        let m = call.method.as_str();
        let hit = g.calls.iter_mut().filter(|(mm, p, _)| mm == m && call.url.starts_with(p.as_str())).max_by_key(|(_, p, _)| p.len());
        match hit {
            Some((_, _, q)) => Ok(if q.len() > 1 { q.remove(0) } else { q[0].clone() }),
            None => Err(NetError::Network(format!("no mock for {m} {}", call.url))),
        }
    }
    fn stream(&self, call: &Call) -> Result<LineStream, NetError> {
        let mut g = self.inner.lock().unwrap();
        g.log.push(call.clone());
        let i = g.streams.iter().position(|(p, _)| call.url.starts_with(p.as_str())).ok_or_else(|| NetError::Network(format!("no mock stream for {}", call.url)))?;
        let (_, lines) = g.streams.remove(i);
        Ok(Box::new(lines.into_iter().map(Ok)))
    }
    fn market_origin(&self) -> String {
        self.inner.lock().unwrap().origin.clone().unwrap_or_else(|| MARKET.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_the_host() {
        assert_eq!(market_key(MARKET, "/v1/games"), "rare:/v1/games");
        assert_eq!(market_key("https://testnet.rare.market/", "/v1/games"), "rare:https://testnet.rare.market/v1/games");
        assert!(market_path_ok("/v1/items/0x1"));
        assert!(!market_path_ok("/v1/../admin"));
        assert!(!market_path_ok("/admin"));
    }

    #[test]
    fn mock_answers_reads_and_calls() {
        let n = MockNet::new();
        n.on_read("/v1/status", r#"{"suiUsd":3.5}"#);
        n.on_call(Method::Post, "https://rare.market/v1/sponsor", 200, "{}").on_call(Method::Post, "https://rare.market/v1/sponsor", 503, "");
        let v: serde_json::Value = get_market(&n, "/v1/status").unwrap();
        assert_eq!(v["suiUsd"], 3.5);
        assert!(read_market(&n, "/v1/nope").is_err());
        let c = Call::post("https://rare.market/v1/sponsor", "{}");
        assert_eq!(n.call(&c).unwrap().status, 200);
        assert_eq!(n.call(&c).unwrap().status, 503);
        assert_eq!(n.call(&c).unwrap().status, 503);
        assert_eq!(n.calls().len(), 3);
    }
}
