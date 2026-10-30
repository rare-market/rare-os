//! One door to the Trade API (token lists, charts, holders, trades, perps, swap quotes and unsigned
//! builds). Read-only paths go through rare-state's cache as `trade:<path>` keys; everything else
//! (swap auth and builds, perps builds, trader pages) is an uncached call. The same allow-list the
//! Tauri `trade_api` command enforces is enforced here. Nothing here signs or submits.

use rare_market::net::{Call, Method, Net, NetError, TRADE};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

/// Paths the launcher may ask the Trade API for (the Tauri host's `TRADE_PATHS`).
pub const TRADE_PATHS: [&str; 10] = [
    "/ext/",
    "/search?",
    "/perps?l=1",
    "/token/",
    "/tokens?",
    "/auth/",
    "/swap/",
    // Top traders and trader pages (the trader ledger): boards, profile, stats, P/L, trades, links.
    "/leaderboard?",
    "/leaderboard/extras?",
    "/users/",
];
/// Read-only prefixes: only GET goes through (no follows, no profile edits from the launcher).
pub const TRADE_GET_ONLY: [&str; 3] = ["/leaderboard?", "/leaderboard/extras?", "/users/"];
/// Display data read through the cache (the host's `instant::TRADE_READS`).
pub const TRADE_READS: [&str; 7] = ["/ext/", "/token/", "/tokens?", "/perps?l=1", "/search?", "/leaderboard?", "/leaderboard/extras?"];

fn clean(path: &str) -> bool {
    !path.contains("..") && !path.contains('#')
}

/// Whether the launcher may call this path at all.
pub fn trade_path_ok(path: &str) -> bool {
    TRADE_PATHS.iter().any(|p| path.starts_with(p)) && clean(path)
}

/// Whether a GET of this path is read through the cache.
pub fn trade_read(path: &str) -> bool {
    TRADE_READS.iter().any(|p| path.starts_with(p)) && clean(path)
}

/// The cache key for a cached Trade read.
pub fn trade_key(path: &str) -> String {
    format!("trade:{path}")
}

/// An error from the Trade API (or the way to it) in plain words. `status` 0 means no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl TradeApiError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        TradeApiError { status, code: code.into(), message: message.into() }
    }
    /// No answer at all (DNS, connect, timeout), or a request this crate refuses to send.
    pub fn network(message: impl Into<String>) -> Self {
        TradeApiError::new(0, "network", message)
    }
}

impl std::fmt::Display for TradeApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TradeApiError {}

impl From<TradeApiError> for rare_core::Error {
    fn from(e: TradeApiError) -> Self {
        rare_core::Error(e.message)
    }
}

impl From<NetError> for TradeApiError {
    fn from(e: NetError) -> Self {
        match e {
            NetError::Status(m) => match status_in(&m) {
                Some(s) => error_of(s, &Value::Null),
                None => TradeApiError::new(502, "invalid_answer", "Could not load"),
            },
            NetError::Network(m) | NetError::NotAllowed(m) => TradeApiError::network(m),
        }
    }
}

/// The status in rare-state's "<url>: status code N".
fn status_in(m: &str) -> Option<u16> {
    let i = m.rfind("status code ")?;
    m[i + 12..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

pub type Result<T, E = TradeApiError> = std::result::Result<T, E>;

/// The error message an API answer carries, in plain words.
pub fn error_of(status: u16, body: &Value) -> TradeApiError {
    let err = body.get("error");
    let (code, msg) = match err {
        Some(Value::Object(o)) => (o.get("code").and_then(Value::as_str), o.get("message").and_then(Value::as_str)),
        Some(Value::String(s)) => (None, Some(s.as_str())),
        _ => (None, None),
    };
    let message = match status {
        429 => "Too many requests. Try again in a moment.".to_string(),
        204 => "No route for this pair right now.".to_string(),
        _ => match msg {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => format!("Trade service error ({status})"),
        },
    };
    TradeApiError::new(status, code.unwrap_or("http_error"), message)
}

/// Any status comes back; only network failures are errors. `body` is the JSON (`null` when empty
/// or not JSON).
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Raw uncached call: GET or POST (JSON) with an optional API key.
pub fn trade_call(net: &dyn Net, method: Method, path: &str, body: Option<&Value>, auth: Option<&str>) -> Result<Reply> {
    if !trade_path_ok(path) {
        return Err(TradeApiError::network("path not allowed"));
    }
    if method != Method::Get && method != Method::Post {
        return Err(TradeApiError::network("method not allowed"));
    }
    if method != Method::Get && TRADE_GET_ONLY.iter().any(|p| path.starts_with(p)) {
        return Err(TradeApiError::network("method not allowed"));
    }
    let url = format!("{TRADE}{path}");
    let json = match (method, body) {
        (Method::Post, Some(b)) => Some(b.to_string()),
        // The host sends an empty object for a body-less POST.
        (Method::Post, None) => Some("{}".to_string()),
        _ => None,
    };
    let mut call = Call::new(method, url, json).timeout(Duration::from_secs(25));
    if let Some(a) = auth {
        call = call.header("authorization", format!("Bearer {a}"));
    }
    let r = net.call(&call)?;
    let body = if r.status == 204 || r.body.is_empty() { Value::Null } else { serde_json::from_str(&r.body).unwrap_or(Value::Null) };
    Ok(Reply { status: r.status, body })
}

/// A GET: cached paths through rare-state, the rest as a call. Errors carry the API's words.
pub fn trade_get(net: &dyn Net, path: &str) -> Result<Value> {
    if trade_read(path) {
        let body = net.read(&trade_key(path))?;
        return serde_json::from_str(&body).map_err(|_| TradeApiError::new(502, "invalid_answer", "Could not load"));
    }
    let r = trade_call(net, Method::Get, path, None, None)?;
    if !r.ok() {
        return Err(error_of(r.status, &r.body));
    }
    Ok(r.body)
}

/// A POST; non-2xx answers become errors.
pub fn trade_post(net: &dyn Net, path: &str, body: &Value, auth: Option<&str>) -> Result<Value> {
    let r = trade_call(net, Method::Post, path, Some(body), auth)?;
    if !r.ok() {
        return Err(error_of(r.status, &r.body));
    }
    Ok(r.body)
}

/// [`trade_get`] parsed into `T`.
pub fn get<T: DeserializeOwned>(net: &dyn Net, path: &str) -> Result<T> {
    parse(trade_get(net, path)?)
}

/// A cached read's body now, without fetching (instant first paint).
pub fn peek<T: DeserializeOwned>(net: &dyn Net, path: &str) -> Option<T> {
    if !trade_read(path) {
        return None;
    }
    parse(serde_json::from_str(&net.peek(&trade_key(path))?).ok()?).ok()
}

/// Fetches a cached path now whatever the cache holds (after a trade, the portfolio and the perps
/// account are read again).
pub fn refresh<T: DeserializeOwned>(net: &dyn Net, path: &str) -> Result<T> {
    if !trade_read(path) {
        return get(net, path);
    }
    let body = net.refresh(&trade_key(path))?;
    parse(serde_json::from_str(&body).map_err(|_| TradeApiError::new(502, "invalid_answer", "Could not load"))?)
}

/// Parses an answer leniently: a `null` field reads as missing, so it takes the field's default
/// instead of failing the whole answer (the web launcher simply carried the null along).
pub(crate) fn parse<T: DeserializeOwned>(mut v: Value) -> Result<T> {
    strip_nulls(&mut v);
    serde_json::from_value(v).map_err(|_| TradeApiError::new(502, "invalid_answer", "Could not load"))
}

fn strip_nulls(v: &mut Value) {
    match v {
        Value::Object(o) => {
            o.retain(|_, x| !x.is_null());
            o.values_mut().for_each(strip_nulls);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

/// `encodeURIComponent`.
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `new URLSearchParams(pairs).toString()`.
pub fn query(pairs: &[(&str, &str)]) -> String {
    let form = |s: &str| {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b"*-._".contains(&b) {
                out.push(b as char);
            } else if b == b' ' {
                out.push('+');
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    };
    pairs.iter().map(|(k, v)| format!("{}={}", form(k), form(v))).collect::<Vec<_>>().join("&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_market::net::MockNet;
    use serde_json::json;

    #[test]
    fn error_words() {
        assert_eq!(error_of(429, &json!({"error": {"message": "slow down"}})).message, "Too many requests. Try again in a moment.");
        assert_eq!(error_of(204, &Value::Null).message, "No route for this pair right now.");
        let e = error_of(422, &json!({"error": {"code": "build_declined", "message": "Request a new quote."}}));
        assert_eq!((e.code.as_str(), e.message.as_str()), ("build_declined", "Request a new quote."));
        assert_eq!(error_of(422, &json!({"error": "Set up your perps account first."})).message, "Set up your perps account first.");
        assert_eq!(error_of(500, &Value::Null).message, "Trade service error (500)");
        assert_eq!(error_of(500, &Value::Null).code, "http_error");
    }

    #[test]
    fn allow_list_matches_the_host() {
        let n = MockNet::new();
        assert_eq!(trade_call(&n, Method::Get, "/admin", None, None).unwrap_err().message, "path not allowed");
        assert_eq!(trade_call(&n, Method::Get, "/ext/../admin", None, None).unwrap_err().message, "path not allowed");
        assert_eq!(trade_call(&n, Method::Post, "/users/0x1/follow", None, None).unwrap_err().message, "method not allowed");
        assert_eq!(trade_call(&n, Method::Delete, "/swap/x", None, None).unwrap_err().message, "method not allowed");
        assert!(n.calls().is_empty());
    }

    #[test]
    fn reads_go_through_the_cache_and_the_rest_are_calls() {
        let n = MockNet::new();
        n.on_read("trade:/tokens?list=trending&limit=20", r#"{"items":[]}"#);
        n.on_call(Method::Get, "https://rare.market/v1/trade/users/0x1", 404, r#"{"error":{"message":"Unknown trader"}}"#);
        n.on_call(Method::Post, "https://rare.market/v1/trade/swap/quote", 200, r#"{"data":{}}"#);
        assert_eq!(trade_get(&n, "/tokens?list=trending&limit=20").unwrap(), json!({"items": []}));
        assert_eq!(n.reads(), vec!["trade:/tokens?list=trending&limit=20".to_string()]);
        assert_eq!(trade_get(&n, "/users/0x1").unwrap_err().message, "Unknown trader");
        assert_eq!(trade_get(&n, "/tokens?list=new&limit=6").unwrap_err().message, "Trade service error (404)");
        trade_post(&n, "/swap/quote", &json!({"a": 1}), Some("yk")).unwrap();
        let c = n.calls().pop().unwrap();
        assert_eq!(c.body.as_deref(), Some(r#"{"a":1}"#));
        assert!(c.headers.contains(&("authorization".to_string(), "Bearer yk".to_string())));
    }

    #[test]
    fn encodes_like_the_browser() {
        assert_eq!(enc("0x2::sui::SUI"), "0x2%3A%3Asui%3A%3ASUI");
        assert_eq!(enc("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(query(&[("market", "0xab"), ("q", "a b:c")]), "market=0xab&q=a+b%3Ac");
    }
}
