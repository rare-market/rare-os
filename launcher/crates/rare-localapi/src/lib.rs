//! The Rare Local API: how a game calls Rare.
//!
//! One set of methods, `me`, `inventory`, `sell`, `buy`, `claim`, `open`, `notices`, over two
//! transports:
//! - **Loopback HTTP** for native and Proton games: the launcher binds 127.0.0.1 on a free port
//!   and starts each game with `RARE_API`, `RARE_TOKEN` and `RARE_GAME` in its environment. A web
//!   game gets a token of its own, injected into its window, for engines whose web export speaks
//!   HTTP (CORS answers that game's origin only).
//! - **`rare:platform`**, a Wallet Standard feature of the Rare Wallet injected into web games
//!   (webgame.js). Calls leave through the same blocked `rare-wallet://` navigation as wallet
//!   requests and land in `web_call`.
//!
//! Rust checks the caller (token, origin, host, rate limits, parameters) and raises the overlay
//! for sheets; the launcher UI (src/localapi) does the rest: it scopes everything to the game's
//! collections, builds transactions with the existing sell and buy builders, and every one of them
//! goes through Rare Wallet's review sheet. Nothing here signs or submits anything.
//!
//! Also here: `rare://` deep links (deeplink.rs), routed to the UI or the overlay.

pub mod auth;
pub mod deeplink;
pub mod http;
pub mod host;

pub use host::{install, LocalApiHost};
use host::host;

use auth::{Class, Grants, Kind, Limited, Limits};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};
use rare_core::Event;

/// How long a sheet may stay up before the call answers "cancelled".
pub const SHEET_TIMEOUT: Duration = Duration::from_secs(600);
/// Reads answer from the UI (rare.market and gRPC behind it).
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Open connections at once; more get 503.
const MAX_CONNECTIONS: usize = 32;
/// A game counts as just launched (keeps its token before the runner lists it) for this long.
const LAUNCH_GRACE: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Me,
    Inventory,
    Sell,
    Buy,
    Claim,
    Open,
    Notices,
    // Network apps only (Settings > Developer > Allow network apps).
    Library,
    Launch,
    Close,
    Input,
    Screenshot,
    Capabilities,
    /// MCP (JSON-RPC) for agents: Rare's tools (rare-agent), each behind its own switch.
    Mcp,
    /// One of those tools, as the UI runs it (internal: never a route).
    Tool,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::Me => "me",
            Method::Inventory => "inventory",
            Method::Sell => "sell",
            Method::Buy => "buy",
            Method::Claim => "claim",
            Method::Open => "open",
            Method::Notices => "notices",
            Method::Library => "library",
            Method::Launch => "launch",
            Method::Close => "close",
            Method::Input => "input",
            Method::Screenshot => "screenshot",
            Method::Capabilities => "capabilities",
            Method::Mcp => "mcp",
            Method::Tool => "tool",
        }
    }

    /// Calls only a network app can make (a game has no use for them).
    pub fn network_only(self) -> bool {
        matches!(self, Method::Library | Method::Launch | Method::Close | Method::Input | Method::Screenshot | Method::Capabilities | Method::Mcp | Method::Tool)
    }

    pub fn from_name(s: &str) -> Option<Method> {
        Some(match s {
            "me" => Method::Me,
            "inventory" => Method::Inventory,
            "sell" => Method::Sell,
            "buy" => Method::Buy,
            "claim" => Method::Claim,
            "open" => Method::Open,
            "notices" => Method::Notices,
            _ => return None,
        })
    }

    pub fn class(self) -> Class {
        match self {
            Method::Me | Method::Inventory | Method::Library | Method::Capabilities => Class::Read,
            Method::Notices => Class::Stream,
            Method::Launch | Method::Close | Method::Mcp | Method::Tool => Class::Act,
            Method::Input => Class::Input,
            Method::Screenshot => Class::Shot,
            _ => Class::Sheet,
        }
    }

    /// Reads that show the player's address or holdings: a web game needs the player to have
    /// connected Rare Wallet to it first (as for Wallet Standard `connect`).
    fn private(self) -> bool {
        matches!(self, Method::Me | Method::Inventory | Method::Notices)
    }

    /// The HTTP route: (method, path) to a Method, or (status, message).
    pub fn from_http(method: &str, path: &str) -> Result<Method, (u16, &'static str)> {
        let (m, want) = match path.trim_end_matches('/') {
            "/v1/me" => (Method::Me, "GET"),
            "/v1/inventory" => (Method::Inventory, "GET"),
            "/v1/notices" => (Method::Notices, "GET"),
            "/v1/sell" => (Method::Sell, "POST"),
            "/v1/buy" => (Method::Buy, "POST"),
            "/v1/claim" => (Method::Claim, "POST"),
            "/v1/open" => (Method::Open, "POST"),
            "/v1/library" => (Method::Library, "GET"),
            "/v1/launch" => (Method::Launch, "POST"),
            "/v1/close" => (Method::Close, "POST"),
            "/v1/input" => (Method::Input, "POST"),
            "/v1/screenshot" => (Method::Screenshot, "GET"),
            "/v1/capabilities" => (Method::Capabilities, "GET"),
            "/v1/mcp" => (Method::Mcp, "POST"),
            "/v1/licence" => return Err((404, "licence comes with Rare Store (not live yet)")),
            _ => return Err((404, "no such route")),
        };
        if method != want {
            return Err((405, "method not allowed"));
        }
        Ok(m)
    }
}

/// A call refused or failed: HTTP status, a code games can branch on, and a sentence.
#[derive(Debug, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

fn api_err(status: u16, code: &str, message: impl Into<String>) -> ApiError {
    ApiError { status, code: code.into(), message: message.into() }
}

impl ApiError {
    fn body(&self) -> Value {
        json!({ "error": { "code": self.code, "message": self.message } })
    }
}

fn decimal(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_u64()?.to_string(),
        _ => return None,
    };
    (!s.is_empty() && s.len() <= 20 && s.bytes().all(|c| c.is_ascii_digit()) && s.parse::<u64>().is_ok()).then_some(s)
}

/// A library or run id: printable, no control characters, a sane length.
fn run_id_ok(i: &str) -> bool {
    !i.is_empty() && i.len() <= 300 && i.chars().all(|c| !c.is_control())
}

/// Buttons a network app can press on the virtual pad.
pub const INPUT_BUTTONS: [&str; 17] = ["a", "b", "x", "y", "lb", "rb", "lt", "rt", "up", "down", "left", "right", "start", "select", "guide", "l3", "r3"];
/// Axes it can move: sticks -1..1 (up positive), triggers 0..1.
pub const INPUT_AXES: [&str; 6] = ["left_x", "left_y", "right_x", "right_y", "lt", "rt"];
/// Events in one call, one tap's length, and the time one call may spend on taps.
pub const INPUT_MAX_EVENTS: usize = 16;
pub const TAP_DEFAULT_MS: u64 = 80;
pub const TAP_MAX_MS: u64 = 1000;
pub const TAP_BUDGET_MS: u64 = 3000;

/// `{ "events": [...] }` or one event: buttons (`down`, `up`, `tap`) and axes.
fn validate_input(p: &Value) -> Result<Value, String> {
    let list: Vec<&Value> = match p.get("events") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(_) => return Err("events must be a list".into()),
        None => vec![p],
    };
    if list.is_empty() || (list.len() == 1 && list[0].as_object().is_some_and(|o| o.is_empty())) {
        return Err("give events, for example { \"events\": [{ \"button\": \"a\" }] }".into());
    }
    if list.len() > INPUT_MAX_EVENTS {
        return Err(format!("at most {INPUT_MAX_EVENTS} events in one call"));
    }
    let mut out = Vec::new();
    let mut tap_ms = 0u64;
    for e in list {
        let o = e.as_object().ok_or("each event must be an object")?;
        if let Some(b) = o.get("button") {
            let b = b.as_str().filter(|b| INPUT_BUTTONS.contains(b)).ok_or_else(|| format!("button must be one of {}", INPUT_BUTTONS.join(", ")))?;
            let action = match o.get("action").and_then(Value::as_str) {
                None | Some("tap") => "tap",
                Some(a @ ("down" | "up")) => a,
                Some(_) => return Err("action must be tap, down or up".into()),
            };
            let ms = match o.get("ms") {
                None => TAP_DEFAULT_MS,
                Some(v) => v.as_u64().filter(|m| (10..=TAP_MAX_MS).contains(m)).ok_or_else(|| format!("ms must be 10 to {TAP_MAX_MS}"))?,
            };
            if action == "tap" {
                tap_ms += ms;
            }
            out.push(json!({ "kind": "button", "button": b, "action": action, "ms": ms }));
        } else if let Some(a) = o.get("axis") {
            let a = a.as_str().filter(|a| INPUT_AXES.contains(a)).ok_or_else(|| format!("axis must be one of {}", INPUT_AXES.join(", ")))?;
            let v = o.get("value").and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or("value must be a number")?;
            let (lo, hi) = if a == "lt" || a == "rt" { (0.0, 1.0) } else { (-1.0, 1.0) };
            out.push(json!({ "kind": "axis", "axis": a, "value": v.clamp(lo, hi) }));
        } else {
            return Err("each event needs a button or an axis".into());
        }
    }
    if tap_ms > TAP_BUDGET_MS {
        return Err(format!("taps in one call may add up to {TAP_BUDGET_MS} ms"));
    }
    Ok(json!({ "events": out }))
}

/// Checks and normalizes a call's parameters before anything else sees them.
pub fn validate(m: Method, p: &Value) -> Result<Value, String> {
    let empty = json!({});
    let p = if p.is_null() { &empty } else { p };
    if m == Method::Mcp {
        // A JSON-RPC message or a batch; rare-agent's mcp checks the rest.
        return if p.is_object() || p.as_array().is_some_and(|a| !a.is_empty() && a.len() <= 16) { Ok(p.clone()) } else { Err("send a JSON-RPC message".into()) };
    }
    let obj = p.as_object().ok_or("parameters must be a JSON object")?;
    let s = |k: &str| obj.get(k).filter(|v| !v.is_null());
    match m {
        Method::Me | Method::Inventory | Method::Notices | Method::Library | Method::Screenshot | Method::Capabilities => Ok(json!({})),
        Method::Mcp => Ok(p.clone()),
        Method::Tool => Err("not a route".into()),
        Method::Launch => {
            let id = s("id").and_then(Value::as_str).filter(|i| run_id_ok(i)).ok_or("id must be a library id such as steam:620 or rare:<slug>")?;
            Ok(json!({ "id": id }))
        }
        Method::Close => {
            let id = match s("id") {
                None => None,
                Some(v) => Some(v.as_str().filter(|i| run_id_ok(i)).ok_or("id must be a library id such as steam:620")?.to_string()),
            };
            Ok(json!({ "id": id }))
        }
        Method::Input => validate_input(p),
        Method::Sell => {
            let id = s("itemId").and_then(Value::as_str).and_then(deeplink::object_id).ok_or("itemId must be an object id (0x...)")?;
            let mode = match s("mode").and_then(Value::as_str) {
                None => "best-offer",
                Some(m @ ("best-offer" | "list")) => m,
                Some(_) => return Err("mode must be \"best-offer\" or \"list\"".into()),
            };
            let price = match s("priceMist") {
                None => None,
                Some(v) => Some(decimal(v).ok_or("priceMist must be a whole number of MIST")?),
            };
            if price.is_some() && mode != "list" {
                return Err("priceMist goes with mode \"list\"".into());
            }
            if price.as_deref() == Some("0") {
                return Err("priceMist must be more than 0".into());
            }
            Ok(json!({ "itemId": id, "mode": mode, "priceMist": price }))
        }
        Method::Buy => {
            let id = match s("itemId") {
                None => None,
                Some(v) => Some(v.as_str().and_then(deeplink::object_id).ok_or("itemId must be an object id (0x...)")?),
            };
            let collection = match s("collection") {
                None => None,
                Some(v) => Some(v.as_str().filter(|t| deeplink::move_type(t)).ok_or("collection must be a Move type")?.to_string()),
            };
            let kind = match s("kind") {
                None => None,
                Some(v) => Some(v.as_u64().ok_or("kind must be a whole number")?),
            };
            let max = match s("maxMist") {
                None => None,
                Some(v) => Some(decimal(v).ok_or("maxMist must be a whole number of MIST")?),
            };
            if id.is_none() && collection.is_none() && kind.is_none() {
                return Err("give itemId, collection or kind".into());
            }
            if id.is_some() && (collection.is_some() || kind.is_some()) {
                return Err("itemId goes alone (with maxMist at most)".into());
            }
            Ok(json!({ "itemId": id, "collection": collection, "kind": kind, "maxMist": max }))
        }
        Method::Claim => {
            let v = s("voucher").and_then(Value::as_str).filter(|v| deeplink::voucher(v)).ok_or("voucher must be base64 text")?;
            Ok(json!({ "voucher": v }))
        }
        Method::Open => {
            let view = s("view").and_then(Value::as_str).ok_or("view is required")?;
            let id = s("id").map(|v| v.as_str().map(str::to_string).ok_or("id must be a string")).transpose()?;
            let id = match (view, id) {
                ("item", Some(i)) => Some(deeplink::object_id(&i).ok_or("id must be an object id (0x...)")?),
                ("item", None) => return Err("view \"item\" needs an id".into()),
                ("collection", Some(c)) if deeplink::slug(&c) || deeplink::move_type(&c) => Some(c),
                ("collection", _) => return Err("view \"collection\" needs a slug or Move type id".into()),
                ("trophies" | "store", Some(sl)) if deeplink::slug(&sl) => Some(sl),
                ("trophies" | "store", Some(_)) => return Err("id must be a game slug".into()),
                ("trophies" | "store", None) => None,
                ("overlay", None) => None,
                ("overlay", Some(_)) => return Err("view \"overlay\" takes no id".into()),
                _ => return Err("view must be item, collection, trophies, store or overlay".into()),
            };
            Ok(json!({ "view": view, "id": id }))
        }
    }
}

// ---------------------------------------------------------------- shared state

struct Sub {
    id: u64,
    game_id: String,
    token: String,
    tx: Sender<String>,
}

struct Shared {
    started: OnceLock<()>,
    port: AtomicU16,
    grants: Mutex<Grants>,
    limits: Mutex<Limits>,
    pending: Mutex<HashMap<u64, Sender<(bool, Value)>>>,
    subs: Mutex<Vec<Sub>>,
    /// The UI listens for `rare-api` and `deep-link` (it called `localapi_ready`).
    ui_ready: AtomicBool,
    seq: AtomicU64,
    active: AtomicUsize,
    /// Deep links that came before the UI was listening.
    links: Mutex<Vec<deeplink::Route>>,
    last_link: Mutex<Option<(String, Instant)>>,
}

static S: LazyLock<Shared> = LazyLock::new(|| Shared {
    started: OnceLock::new(),
    port: AtomicU16::new(0),
    grants: Mutex::new(Grants::default()),
    limits: Mutex::new(Limits::new(Instant::now())),
    pending: Mutex::new(HashMap::new()),
    subs: Mutex::new(Vec::new()),
    ui_ready: AtomicBool::new(false),
    seq: AtomicU64::new(0),
    active: AtomicUsize::new(0),
    links: Mutex::new(Vec::new()),
    last_link: Mutex::new(None),
});

fn emit(name: &str, payload: Value) {
    host::emit(Event::Other { name: name.to_string(), payload });
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 256 random bits, hex.
fn new_token() -> Option<String> {
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn base_url() -> Option<String> {
    let port = S.port.load(Ordering::Relaxed);
    (port != 0).then(|| format!("http://127.0.0.1:{port}"))
}

/// `RARE_GAME`: the Rare slug for Store and web games, else the library id (`me` answers the slug).
fn game_env_slug(game_id: &str) -> String {
    game_id.strip_prefix("rare:").or_else(|| game_id.strip_prefix("web:")).unwrap_or(game_id).to_string()
}

/// The environment for a native or Proton game being launched: a fresh token bound to it.
/// Empty when the API isn't running.
pub fn grant_native(game_id: &str) -> Vec<(String, String)> {
    let (Some(url), Some(token)) = (base_url(), new_token()) else { return vec![] };
    lock(&S.grants).issue(token.clone(), game_id, Kind::Native, Instant::now());
    eprintln!("[localapi] token for {game_id}");
    vec![("RARE_API".into(), url), ("RARE_TOKEN".into(), token), ("RARE_GAME".into(), game_env_slug(game_id))]
}

/// What a web game's window gets injected (webgame.js keeps it out of the page's globals and
/// hands it out as `rare:platform`'s `api`).
#[derive(Serialize)]
pub struct WebApi {
    pub url: Option<String>,
    pub token: String,
    pub game: String,
}

/// A token for a web game at its origin.
pub fn grant_web(game_id: &str, origin: &str) -> Option<WebApi> {
    let token = new_token()?;
    lock(&S.grants).issue(token.clone(), game_id, Kind::Web { origin: origin.to_string() }, Instant::now());
    Some(WebApi { url: base_url(), token, game: game_env_slug(game_id) })
}

fn drop_tokens(tokens: &[String]) {
    if tokens.is_empty() {
        return;
    }
    let mut l = lock(&S.limits);
    for t in tokens {
        l.forget(t);
    }
    drop(l);
    // Dropping a stream's sender ends it.
    lock(&S.subs).retain(|s| !tokens.contains(&s.token));
}

// ---------------------------------------------------------------- server

/// Binds the loopback port and starts serving. RARE_API_PORT picks the port (development);
/// otherwise the OS picks a free one.
pub fn start() {
    if S.started.set(()).is_err() {
        return;
    }
    let want: u16 = std::env::var("RARE_API_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = match TcpListener::bind(("127.0.0.1", want)).or_else(|e| if want != 0 { TcpListener::bind(("127.0.0.1", 0)) } else { Err(e) }) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[localapi] could not bind 127.0.0.1: {e}");
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    S.port.store(port, Ordering::Relaxed);
    eprintln!("[localapi] listening on 127.0.0.1:{port}");
    std::thread::Builder::new()
        .name("localapi".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(stream) = conn else { continue };
                if S.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    S.active.fetch_sub(1, Ordering::SeqCst);
                    let mut s = stream;
                    let _ = http::write_all(&mut s, &http::json(503, &[], &json!({ "error": { "code": "busy", "message": "too many connections" } })));
                    continue;
                }
                let _ = std::thread::Builder::new().name("localapi-conn".into()).spawn(move || {
                    serve(stream, false, "loopback");
                    S.active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        })
        .ok();
    // Tokens of games that stopped go away.
    std::thread::Builder::new()
        .name("localapi-sweep".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(3));
            let running = host().running();
            let gone = lock(&S.grants).retain_running(&running, Instant::now(), LAUNCH_GRACE);
            drop_tokens(&gone);
        })
        .ok();
}

fn cors_for(kind: &Kind) -> Vec<(String, String)> {
    match kind {
        Kind::Web { origin } => vec![("Access-Control-Allow-Origin".into(), origin.clone()), ("Vary".into(), "Origin".into())],
        Kind::Native | Kind::Network => vec![],
    }
}

fn web_approved(origin: &str) -> bool {
    host().web_approved(origin)
}

/// What the gate decided about a request.
#[derive(Debug)]
pub enum Gate {
    /// Checked: run it.
    Pass { grant: auth::Grant, method: Method, params: Value, cors: Vec<(String, String)> },
    /// A CORS preflight for a web game's origin.
    Preflight(Vec<(String, String)>),
    /// Refused: status, extra headers, JSON body.
    Refuse(u16, Vec<(String, String)>, Value),
}

fn refuse(status: u16, extra: Vec<(String, String)>, code: &str, message: &str) -> Gate {
    Gate::Refuse(status, extra, json!({ "error": { "code": code, "message": message } }))
}

/// Every check before a call runs, in order: host, preflight, route, lockout, token, origin,
/// connect (web), body, parameters, rate limits. A Pass holds its rate-limit slot: `release` it.
pub fn gate(req: &http::Request, port: u16, grants: &Mutex<Grants>, limits: &Mutex<Limits>, approved: &dyn Fn(&str) -> bool, now: Instant) -> Gate {
    gate_core(req, auth::host_ok(req.header("host"), port), "Host must be 127.0.0.1 and this port", grants, limits, approved, now)
}

/// The same checks for the network listener: its own tokens and limits (so a guessing stranger on
/// the network can't lock the games out), a Host that is an IP address or the device's own name,
/// and only the calls a network app may make.
pub fn gate_network(req: &http::Request, port: u16, name: &str, grants: &Mutex<Grants>, limits: &Mutex<Limits>, now: Instant) -> Gate {
    gate_core(req, auth::host_ok_net(req.header("host"), port, name), "Host must be this device's address or name and this port", grants, limits, &|_| false, now)
}

/// One switch in Settings > Developer: (id, title, the plain line under it). All off by default.
pub const CAPS: [(&str, &str, &str); 8] = [
    ("profile", "Read profile", "Lets the app see your username, address and balances."),
    ("library", "Read inventory and library", "Lets the app see your items across games, your installed games and what is running."),
    ("launch", "Launch and close games and apps", "Lets the app start and quit games and apps, and open pages in Rare."),
    ("input", "Send controller input", "Lets the app press buttons and move sticks on a virtual controller, to drive Rare and games."),
    ("purchase", "Request purchases and listings", "Lets the app ask to buy or sell. Each request opens a review sheet here, and nothing is signed until you press confirm on this device."),
    ("screenshot", "Screenshots", "Lets the app capture what is on the screen."),
    ("settings", "Change settings", "Lets the app change the power profile, brightness, volume, Wi-Fi and Do not disturb."),
    ("friends", "Friends and messages", "Lets the app see your friends, invite them to a party and send them messages."),
];

/// The switch a network call needs. None: the call is not for network apps.
pub fn network_cap(m: Method, params: &Value) -> Option<&'static str> {
    match m {
        Method::Me => Some("profile"),
        Method::Inventory | Method::Library => Some("library"),
        Method::Open if params["view"] != "overlay" => Some("launch"),
        Method::Launch | Method::Close => Some("launch"),
        Method::Input => Some("input"),
        Method::Sell | Method::Buy => Some("purchase"),
        Method::Screenshot => Some("screenshot"),
        // Always answered: what is on and off. MCP checks each tool's own switch.
        Method::Capabilities | Method::Mcp => Some(""),
        Method::Tool => None,
        Method::Open | Method::Claim | Method::Notices => None,
    }
}

fn caps_mask(id: &str) -> u32 {
    CAPS.iter().position(|c| c.0 == id).map(|i| 1u32 << i).unwrap_or(0)
}

/// Whether the owner switched this capability on.
pub fn network_cap_on(id: &str) -> bool {
    id.is_empty() || NET.caps.load(Ordering::SeqCst) & caps_mask(id) != 0
}

/// The switches that are on.
pub fn network_caps() -> Vec<&'static str> {
    CAPS.iter().map(|c| c.0).filter(|id| network_cap_on(id)).collect()
}

/// Sets the switches to exactly these ids (unknown ids are ignored). Input's virtual pad exists
/// only while its switch and the listener are on.
pub fn network_set_caps(ids: &[String]) {
    let mask = ids.iter().fold(0u32, |m, i| m | caps_mask(i));
    NET.caps.store(mask, Ordering::SeqCst);
    sync_input();
}

fn sync_input() {
    let want = NET.enabled.load(Ordering::SeqCst) && network_cap_on("input");
    if NET.input_ready.swap(want, Ordering::SeqCst) != want {
        host().net_input_ready(want);
    }
}

// ---------------------------------------------------------------- the audit trail

/// One call a network app made (or was refused): the per-capability audit line, also written to the
/// log as `[netapi] cap=<id> ...`. Settings shows the latest as "Recent network activity".
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Activity {
    pub at_ms: u64,
    pub cap: String,
    pub what: String,
    pub ok: bool,
    pub from: String,
}

const KEEP_ACTIVITY: usize = 40;

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Writes the audit line and keeps it for Settings. Input is one line per burst of input, not per call.
fn audit(cap: &str, what: String, ok: bool, from: &str) {
    if cap == "input" {
        let now = now_ms();
        NET.input_at.store(now, Ordering::Relaxed);
        if now.saturating_sub(NET.input_logged.load(Ordering::Relaxed)) < 5000 && ok {
            return;
        }
        NET.input_logged.store(now, Ordering::Relaxed);
    }
    eprintln!("[netapi] cap={cap} {what} {} from {from}", if ok { "ok" } else { "refused" });
    let mut a = lock(&NET.activity);
    a.insert(0, Activity { at_ms: now_ms(), cap: cap.to_string(), what, ok, from: from.to_string() });
    a.truncate(KEEP_ACTIVITY);
}

/// The latest calls, newest first.
pub fn network_activity() -> Vec<Activity> {
    lock(&NET.activity).clone()
}

/// A remote is sending input (a call in the last 3 seconds): Rare shows an icon in the top bar.
pub fn network_input_active() -> bool {
    let at = NET.input_at.load(Ordering::Relaxed);
    at != 0 && now_ms().saturating_sub(at) < 3000
}

/// A short line for the audit trail: what was asked, never a secret.
fn describe(m: Method, params: &Value) -> String {
    let short = |v: &Value| v.as_str().map(|s| s.chars().take(60).collect::<String>()).unwrap_or_default();
    match m {
        Method::Launch => format!("launch {}", short(&params["id"])),
        Method::Close => match params["id"].as_str() {
            Some(_) => format!("close {}", short(&params["id"])),
            None => "close the running app".into(),
        },
        Method::Open => format!("open {} {}", short(&params["view"]), short(&params["id"])).trim_end().to_string(),
        Method::Sell => format!("sell {}", short(&params["itemId"])),
        Method::Buy => format!("buy {}", short(if params["itemId"].is_null() { &params["collection"] } else { &params["itemId"] })),
        Method::Input => format!("input {} events", params["events"].as_array().map(Vec::len).unwrap_or(0)),
        Method::Mcp => format!("mcp {}", short(&params["method"])),
        m => m.name().to_string(),
    }
}

fn gate_core(req: &http::Request, host_is_ok: bool, host_msg: &str, grants: &Mutex<Grants>, limits: &Mutex<Limits>, approved: &dyn Fn(&str) -> bool, now: Instant) -> Gate {
    if !host_is_ok {
        return refuse(421, vec![], "wrong_host", host_msg);
    }
    let origin = req.header("origin");
    if req.method == "OPTIONS" {
        return match origin {
            Some(o) if lock(grants).web_origin(o) => Gate::Preflight(vec![
                ("Access-Control-Allow-Origin".into(), o.to_string()),
                ("Access-Control-Allow-Methods".into(), "GET, POST".into()),
                ("Access-Control-Allow-Headers".into(), "Authorization, Content-Type".into()),
                ("Access-Control-Max-Age".into(), "600".into()),
                ("Vary".into(), "Origin".into()),
            ]),
            _ => refuse(403, vec![], "origin", "origin not allowed"),
        };
    }
    let method = match Method::from_http(&req.method, &req.path) {
        Ok(m) => m,
        Err((status, msg)) => return refuse(status, vec![], if status == 404 { "not_found" } else { "method" }, msg),
    };
    if let Some(wait) = lock(limits).auth_locked(now) {
        return refuse(429, vec![("Retry-After".into(), wait.to_string())], "rate_limited", "too many bad tokens");
    }
    let grant = auth::bearer(req.header("authorization")).and_then(|t| lock(grants).find(t).cloned());
    let Some(grant) = grant else {
        lock(limits).auth_failed(now);
        return refuse(401, vec![("WWW-Authenticate".into(), "Bearer".into())], "unauthorized", "missing or unknown RARE_TOKEN");
    };
    if let Err(e) = auth::origin_ok(&grant, origin) {
        let msg = match e {
            auth::OriginError::NativeFromWeb => "a game's token can't be used from a web page",
            auth::OriginError::WrongOrigin => "this token belongs to another origin",
        };
        return refuse(403, vec![], "origin", msg);
    }
    let cors = cors_for(&grant.kind);
    if let Kind::Web { origin: o } = &grant.kind {
        if method.private() && !approved(o) {
            return refuse(403, cors, "not_connected", "Connect Rare Wallet to this game first");
        }
    }
    let params = if req.body.is_empty() {
        Value::Null
    } else {
        if !req.header("content-type").is_some_and(|c| c.to_ascii_lowercase().starts_with("application/json")) {
            return refuse(415, cors, "bad_request", "send JSON (Content-Type: application/json)");
        }
        match serde_json::from_slice::<Value>(&req.body) {
            Ok(v) => v,
            Err(_) => return refuse(400, cors, "bad_request", "body is not JSON"),
        }
    };
    let params = match validate(method, &params) {
        Ok(p) => p,
        Err(e) => return refuse(400, cors, "bad_request", &e),
    };
    if grant.kind != Kind::Network && method.network_only() {
        return refuse(403, cors, "network_scope", "this call is for network apps (Settings > Developer > Allow network apps)");
    }
    if grant.kind == Kind::Network {
        let Some(cap) = network_cap(method, &params) else {
            return refuse(403, cors, "network_scope", "network apps can't call this (claim, notices and the overlay stay with games on the device)");
        };
        if !network_cap_on(cap) {
            let title = CAPS.iter().find(|c| c.0 == cap).map(|c| c.1).unwrap_or(cap);
            return Gate::Refuse(
                403,
                cors,
                json!({ "error": { "code": "capability_off", "capability": cap, "message": format!("The owner has not switched on \"{title}\" in Settings > Developer") } }),
            );
        }
    }
    let cost = if method == Method::Input { params["events"].as_array().map(Vec::len).unwrap_or(1) } else { 1 };
    if let Err(l) = lock(limits).admit_cost(&grant.token, method.class(), cost, now) {
        return match l {
            Limited::Rate(s) => refuse(429, [cors, vec![("Retry-After".into(), s.to_string())]].concat(), "rate_limited", "too many calls; wait and try again"),
            Limited::SheetOpen => refuse(409, cors, "busy", "a Rare sheet from this game is already open"),
            Limited::Streams => refuse(429, cors, "rate_limited", "two notice streams at most"),
        };
    }
    Gate::Pass { grant, method, params, cors }
}

fn serve(stream: TcpStream, net: bool, peer: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let Ok(read_half) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read_half);
    let send = |out: &mut TcpStream, status: u16, extra: &[(String, String)], body: Value| {
        let _ = http::write_all(out, &http::json(status, extra, &body));
    };
    let req = match http::read_request(&mut reader) {
        Ok(r) => r,
        Err(e) => return send(&mut out, e.status, &[], json!({ "error": { "code": "bad_request", "message": e.message } })),
    };
    if S.started.get().is_none() {
        return;
    }
    let approved = |o: &str| web_approved(o);
    let (grants, limits) = if net { (&NET.grants, &NET.limits) } else { (&S.grants, &S.limits) };
    let checked = if net {
        let name = lock(&NET.name).clone();
        gate_network(&req, NET.port.load(Ordering::Relaxed), &name, grants, limits, Instant::now())
    } else {
        gate(&req, S.port.load(Ordering::Relaxed), grants, limits, &approved, Instant::now())
    };
    let (grant, method, params, cors) = match checked {
        Gate::Pass { grant, method, params, cors } => (grant, method, params, cors),
        Gate::Preflight(h) => {
            let _ = http::write_all(&mut out, &http::response(204, &h, "text/plain", b""));
            return;
        }
        Gate::Refuse(status, h, body) => {
            // A switch that is off is worth a line: someone asked for something the owner didn't allow.
            if net && body["error"]["code"] == "capability_off" {
                let cap = body["error"]["capability"].as_str().unwrap_or("").to_string();
                let what = format!("{} (switch is off)", req.path.trim_start_matches("/v1/"));
                audit(&cap, what, false, peer);
            }
            return send(&mut out, status, &h, body);
        }
    };
    if method == Method::Notices {
        stream_notices(&mut out, &grant, &cors);
    } else {
        let transport = match grant.kind {
            Kind::Web { .. } => "web-http",
            Kind::Network => "network",
            Kind::Native => "native",
        };
        let what = describe(method, &params);
        let cap = if net { network_cap(method, &params).unwrap_or("") } else { "" };
        if method == Method::Mcp {
            match mcp(&grant.game_id, &params, peer) {
                Some(v) => send(&mut out, 200, &cors, v),
                None => {
                    let _ = http::write_all(&mut out, &http::response(202, &cors, "application/json", b""));
                }
            }
        } else if method == Method::Screenshot {
            match host().net_screenshot() {
                Ok((ctype, bytes)) => {
                    audit(cap, what, true, peer);
                    let _ = http::write_all(&mut out, &http::response(200, &cors, &ctype, &bytes));
                }
                Err(e) => {
                    audit(cap, format!("{what}: {e}"), false, peer);
                    send(&mut out, 503, &cors, api_err(503, "unavailable", e).body());
                }
            }
        } else {
            let r = dispatch(&grant.game_id, transport, req.header("origin"), method, params);
            if net && !cap.is_empty() {
                audit(cap, if let Err(e) = &r { format!("{what}: {}", e.code) } else { what }, r.is_ok(), peer);
            }
            match r {
                Ok(v) => send(&mut out, 200, &cors, v),
                Err(e) => send(&mut out, e.status, &cors, e.body()),
            }
        }
    }
    lock(limits).release(&grant.token, method.class());
}

// ---------------------------------------------------------------- the network listener

/// The Local API for tools on the local network. Off until the owner turns on "Allow network
/// apps"; then one extra listener on all interfaces, with its own token, limits and lockout.
struct NetShared {
    enabled: AtomicBool,
    /// Bumps on every enable, so an older listener thread ends.
    gen: AtomicU64,
    port: AtomicU16,
    name: Mutex<String>,
    grants: Mutex<Grants>,
    limits: Mutex<Limits>,
    active: AtomicUsize,
    /// Bit i is `CAPS[i]` switched on.
    caps: AtomicU32,
    input_ready: AtomicBool,
    /// Wall clock ms of the last input call, and of the last input audit line.
    input_at: AtomicU64,
    input_logged: AtomicU64,
    activity: Mutex<Vec<Activity>>,
}

static NET: LazyLock<NetShared> = LazyLock::new(|| NetShared {
    enabled: AtomicBool::new(false),
    gen: AtomicU64::new(0),
    port: AtomicU16::new(0),
    name: Mutex::new(String::new()),
    grants: Mutex::new(Grants::default()),
    limits: Mutex::new(Limits::new(Instant::now())),
    active: AtomicUsize::new(0),
    caps: AtomicU32::new(0),
    input_ready: AtomicBool::new(false),
    input_at: AtomicU64::new(0),
    input_logged: AtomicU64::new(0),
    activity: Mutex::new(Vec::new()),
});

/// Starts (or moves to a new token) the network listener on `port`. `token` is 32 to 128 hex or
/// base64url characters; `name` is the device's network name (for the Host check).
pub fn network_enable(token: &str, port: u16, name: &str) -> Result<(), String> {
    if token.len() < 32 || token.len() > 128 || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("The network token is not valid.".into());
    }
    if port < 1024 {
        return Err("Pick a port of 1024 or higher.".into());
    }
    *lock(&NET.name) = name.to_string();
    let same = NET.enabled.load(Ordering::SeqCst) && NET.port.load(Ordering::SeqCst) == port;
    if !same {
        // Stopping forgets the token, so it is set after.
        network_disable();
    }
    {
        let mut g = lock(&NET.grants);
        *g = Grants::default();
        g.issue(token.to_string(), "device:network", Kind::Network, Instant::now());
    }
    if same {
        // A new token on the running listener: the old one stops working at once.
        return Ok(());
    }
    let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("Couldn't listen on port {port}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    NET.port.store(port, Ordering::SeqCst);
    let gen = NET.gen.fetch_add(1, Ordering::SeqCst) + 1;
    NET.enabled.store(true, Ordering::SeqCst);
    sync_input();
    eprintln!("[localapi] network apps allowed on port {port}");
    std::thread::Builder::new()
        .name("localapi-net".into())
        .spawn(move || {
            while NET.enabled.load(Ordering::SeqCst) && NET.gen.load(Ordering::SeqCst) == gen {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        let _ = stream.set_nonblocking(false);
                        let peer = addr.ip().to_string();
                        if NET.active.fetch_add(1, Ordering::SeqCst) >= 8 {
                            NET.active.fetch_sub(1, Ordering::SeqCst);
                            continue;
                        }
                        let _ = std::thread::Builder::new().name("localapi-net-conn".into()).spawn(move || {
                            serve(stream, true, &peer);
                            NET.active.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                    Err(_) => std::thread::sleep(Duration::from_millis(500)),
                }
            }
            eprintln!("[localapi] network listener closed");
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Stops the network listener and forgets its token.
pub fn network_disable() {
    NET.enabled.store(false, Ordering::SeqCst);
    NET.gen.fetch_add(1, Ordering::SeqCst);
    *lock(&NET.grants) = Grants::default();
    NET.port.store(0, Ordering::SeqCst);
    sync_input();
}

/// The port the network listener is on, if it is.
pub fn network_port() -> Option<u16> {
    NET.enabled.load(Ordering::SeqCst).then(|| NET.port.load(Ordering::SeqCst)).filter(|p| *p != 0)
}

fn stream_notices(out: &mut TcpStream, grant: &auth::Grant, cors: &[(String, String)]) {
    let (tx, rx) = mpsc::channel::<String>();
    let id = S.seq.fetch_add(1, Ordering::Relaxed) + 1;
    lock(&S.subs).push(Sub { id, game_id: grant.game_id.clone(), token: grant.token.clone(), tx });
    let _ = out.set_write_timeout(Some(Duration::from_secs(5)));
    let mut ok = http::write_all(out, &http::sse_head(cors)) && http::write_all(out, &http::event("ready", &json!({ "game": game_env_slug(&grant.game_id) }).to_string()));
    while ok {
        ok = match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(data) => http::write_all(out, &http::event("notice", &data)),
            Err(RecvTimeoutError::Timeout) => http::write_all(out, b": ping\n\n"),
            Err(RecvTimeoutError::Disconnected) => false,
        };
    }
    lock(&S.subs).retain(|s| s.id != id);
}

// ---------------------------------------------------------------- dispatch to the UI

/// Brings the launcher in front of the game, the way Guide does (the game keeps running).
fn show_launcher() {
    host().show_launcher();
}

/// Asks the UI to answer a call; waits up to `timeout`.
/// `before_emit` runs first (the overlay comes up), so the UI never answers before the screen is ours.
#[allow(clippy::too_many_arguments)]
fn ui_call(game_id: &str, transport: &str, origin: Option<&str>, method: Method, params: Value, close_after: bool, timeout: Duration, before_emit: impl FnOnce()) -> Result<Value, ApiError> {
    if !S.ui_ready.load(Ordering::Relaxed) {
        return Err(api_err(503, "starting", "Rare is still starting; try again in a moment"));
    }
    before_emit();
    let id = S.seq.fetch_add(1, Ordering::Relaxed) + 1;
    let (tx, rx) = mpsc::channel();
    lock(&S.pending).insert(id, tx);
    let payload = json!({ "id": id, "gameId": game_id, "transport": transport, "origin": origin, "method": method.name(), "params": params, "closeAfter": close_after });
    if !host().main_exists() {
        lock(&S.pending).remove(&id);
        if close_after {
            let _ = rare_overlay::close();
        }
        return Err(api_err(503, "unavailable", "Rare is not available"));
    }
    emit("rare-api", payload);
    match rx.recv_timeout(timeout) {
        Ok((true, v)) => Ok(v),
        Ok((false, v)) => {
            let code = v.get("code").and_then(Value::as_str).unwrap_or("failed").to_string();
            let message = v.get("message").and_then(Value::as_str).map(str::to_string).or_else(|| v.as_str().map(str::to_string)).unwrap_or_else(|| "Something went wrong".into());
            let status = match code.as_str() {
                "bad_request" => 400,
                "forbidden" | "not_this_game" => 403,
                "not_found" => 404,
                "no_account" | "watch_only" | "busy" => 409,
                "unavailable" | "offline" => 503,
                _ => 422,
            };
            Err(ApiError { status, code, message })
        }
        Err(_) => {
            lock(&S.pending).remove(&id);
            emit("rare-api-cancel", json!({ "id": id }));
            if method.class() == Class::Sheet {
                // A sheet nobody finished counts as cancelled.
                Ok(json!({ "status": "cancelled" }))
            } else {
                Err(api_err(504, "timeout", "Rare did not answer in time"))
            }
        }
    }
}

/// Runs a checked call for `game_id`. Shared by both transports.
pub fn dispatch(game_id: &str, transport: &str, origin: Option<&str>, method: Method, params: Value) -> Result<Value, ApiError> {
    match method {
        Method::Me | Method::Inventory | Method::Library | Method::Launch | Method::Close => ui_call(game_id, transport, origin, method, params, false, READ_TIMEOUT, || {}),
        Method::Capabilities => Ok(capabilities_json()),
        Method::Input => host().net_input(&params).map_err(|e| api_err(503, "unavailable", e)),
        Method::Screenshot => Err(api_err(400, "bad_request", "screenshot answers an image")),
        Method::Open if params["view"] == "overlay" => {
            rare_overlay::open(Some(game_id.to_string())).map_err(|e| api_err(503, "unavailable", e))?;
            Ok(json!({ "ok": true }))
        }
        Method::Open => {
            let r = ui_call(game_id, transport, origin, method, params, false, READ_TIMEOUT, || {})?;
            show_launcher();
            Ok(r)
        }
        Method::Sell | Method::Buy | Method::Claim if transport == "network" => {
            // A tool on the network asked: Rare comes to the front and the sheet waits for the
            // owner. Nothing is signed without a press on this device.
            ui_call(game_id, transport, origin, method, params, false, SHEET_TIMEOUT, show_launcher)
        }
        Method::Sell | Method::Buy | Method::Claim => {
            // The sheet comes up over the game the way the overlay does (gamescope's base layer in
            // Game Mode), and the overlay closes again when it is done unless the player had it open.
            let was_open = rare_overlay::is_open();
            let gid = game_id.to_string();
            ui_call(game_id, transport, origin, method, params, !was_open, SHEET_TIMEOUT, move || {
                if !was_open {
                    if let Err(e) = rare_overlay::open(Some(gid)) {
                        eprintln!("[localapi] overlay: {e}");
                    }
                }
            })
        }
        Method::Notices => Err(api_err(400, "bad_request", "notices is a stream")),
        Method::Mcp | Method::Tool => Err(api_err(400, "bad_request", "mcp answers JSON-RPC")),
    }
}

/// How long a tool may take in the UI (a sheet it opens answers at once; reads take a moment).
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(90);

/// `POST /v1/mcp`: Rare's tools for agents (rare-agent's catalog), each run by the UI exactly as
/// Ask Rare runs it. Every call is in Settings' recent network activity under its tool's switch.
fn mcp(game_id: &str, msg: &Value, peer: &str) -> Option<Value> {
    let call = |name: &str, args: &Value| -> Result<Value, String> {
        let cap = rare_agent::tools::find(name).map(|t| t.cap).unwrap_or("");
        let r = ui_call(game_id, "network", None, Method::Tool, json!({ "name": name, "args": args }), false, TOOL_TIMEOUT, || {}).map_err(|e| e.message);
        audit(if cap.is_empty() { "library" } else { cap }, format!("mcp {name}"), r.is_ok(), peer);
        r
    };
    rare_agent::mcp::handle(msg, &|cap: &str| network_cap_on(cap), &call)
}

/// What `GET /v1/capabilities` answers: which switches are on, and the routes each one opens.
fn capabilities_json() -> Value {
    let routes = |id: &str| -> Vec<&'static str> {
        match id {
            "profile" => vec!["GET /v1/me"],
            "library" => vec!["GET /v1/inventory", "GET /v1/library"],
            "launch" => vec!["POST /v1/launch", "POST /v1/close", "POST /v1/open"],
            "input" => vec!["POST /v1/input"],
            "purchase" => vec!["POST /v1/sell", "POST /v1/buy"],
            "screenshot" => vec!["GET /v1/screenshot"],
            "settings" | "friends" => vec!["POST /v1/mcp"],
            _ => vec![],
        }
    };
    let caps: Vec<Value> = CAPS.iter().map(|(id, title, about)| json!({ "id": id, "title": title, "about": about, "enabled": network_cap_on(id), "routes": routes(id) })).collect();
    json!({ "capabilities": caps })
}

/// `rare:platform` from a web game window: `origin` is the window's real origin, `token` what the
/// page sent (the one injected into it). Answers the value, or an error sentence for the page.
pub fn web_call(origin: &str, token: &str, call: &str, params: Value) -> Result<Value, String> {
    let game_id = host().web_current_id().ok_or("No web game is open")?;
    let grant = lock(&S.grants).web(&game_id, origin).cloned().ok_or("Rare is not available to this page")?;
    if !auth::same(token.as_bytes(), grant.token.as_bytes()) {
        return Err("Rare is not available to this page".into());
    }
    let method = Method::from_name(call).ok_or("Unknown rare:platform method")?;
    if method.private() && !web_approved(origin) {
        return Err("Connect Rare Wallet to this game first".into());
    }
    let params = validate(method, &params)?;
    if method == Method::Notices {
        // The page gets notices through window.__rareNotice (webgame.js turns them into events).
        lock(&S.limits).admit(&grant.token, Class::Read, Instant::now()).map_err(|_| "Too many calls; wait and try again".to_string())?;
        return Ok(json!({ "ok": true }));
    }
    let class = method.class();
    lock(&S.limits).admit(&grant.token, class, Instant::now()).map_err(|l| match l {
        Limited::SheetOpen => "A Rare sheet from this game is already open".to_string(),
        _ => "Too many calls; wait and try again".to_string(),
    })?;
    let r = dispatch(&game_id, "web", Some(origin), method, params);
    lock(&S.limits).release(&grant.token, class);
    r.map_err(|e| e.message)
}

// ---------------------------------------------------------------- deep links

/// `rare://` and https://rare.market links in a program's arguments.
pub fn links_in(args: impl IntoIterator<Item = String>) -> Vec<String> {
    args.into_iter().filter(|a| a.starts_with("rare://") || a.starts_with("https://rare.market/") || a.starts_with("https://www.rare.market/")).collect()
}

/// Opens a link: the overlay over the game, or a page or sheet in the launcher.
pub fn open_link(link: &str) {
    {
        // The same link twice in a row (the deep-link plugin and the arguments) opens once.
        let mut last = lock(&S.last_link);
        if last.as_ref().is_some_and(|(l, t)| l == link && t.elapsed() < Duration::from_millis(1500)) {
            return;
        }
        *last = Some((link.to_string(), Instant::now()));
    }
    let route = match deeplink::parse(link) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[deeplink] ignored {}: {e}", link.chars().take(120).collect::<String>());
            return;
        }
    };
    eprintln!("[deeplink] {route:?}");
    let overlay = matches!(route, deeplink::Route::Overlay { .. });
    deliver(route);
    if overlay {
        std::thread::spawn(move || {
            if let Err(e) = rare_overlay::open(None) {
                eprintln!("[deeplink] overlay: {e}");
            }
        });
    } else {
        show_launcher();
    }
}

fn deliver(route: deeplink::Route) {
    if S.ui_ready.load(Ordering::Relaxed) {
        emit("deep-link", serde_json::to_value(route).unwrap_or(Value::Null));
    } else {
        let mut q = lock(&S.links);
        q.push(route);
        let n = q.len();
        if n > 8 {
            q.drain(..n - 8);
        }
    }
}

// ---------------------------------------------------------------- commands for the UI

/// The UI listens now: answers the deep links that came before.
pub fn localapi_ready() -> Vec<deeplink::Route> {
    S.ui_ready.store(true, Ordering::Relaxed);
    std::mem::take(&mut *lock(&S.links))
}

/// The UI's answer to a `rare-api` request. `value` is the result, or `{ code, message }`.
pub fn localapi_reply(id: u64, ok: bool, value: Value) {
    if let Some(tx) = lock(&S.pending).remove(&id) {
        let _ = tx.send((ok, value));
    }
}

/// A notice about one of this game's items (the UI filters by the game's collections).
pub fn localapi_notice(game_id: String, notice: Value) {
    let data = notice.to_string();
    if data.len() > 16 * 1024 {
        return;
    }
    lock(&S.subs).retain(|s| s.game_id != game_id || s.tx.send(data.clone()).is_ok());
    // The web game in Rare's window, when it is this game and the player connected it.
    if host().web_current_id().as_deref() == Some(game_id.as_str()) {
        let origin = host().web_origin();
        if origin.is_some_and(|o| web_approved(&o)) {
            host().web_eval(&format!("window.__rareNotice && window.__rareNotice({})", data.replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")));
        }
    }
}

/// Where the API listens, for Settings > Developer and the docs' curl examples.
pub fn localapi_info() -> Value {
    json!({ "url": base_url() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_routes() {
        assert_eq!(Method::from_http("GET", "/v1/me"), Ok(Method::Me));
        assert_eq!(Method::from_http("GET", "/v1/inventory/"), Ok(Method::Inventory));
        assert_eq!(Method::from_http("GET", "/v1/notices"), Ok(Method::Notices));
        assert_eq!(Method::from_http("POST", "/v1/sell"), Ok(Method::Sell));
        assert_eq!(Method::from_http("POST", "/v1/buy"), Ok(Method::Buy));
        assert_eq!(Method::from_http("POST", "/v1/claim"), Ok(Method::Claim));
        assert_eq!(Method::from_http("POST", "/v1/open"), Ok(Method::Open));
        assert_eq!(Method::from_http("GET", "/v1/sell"), Err((405, "method not allowed")));
        assert_eq!(Method::from_http("POST", "/v1/me"), Err((405, "method not allowed")));
        assert_eq!(Method::from_http("GET", "/v1/licence").unwrap_err().0, 404);
        assert_eq!(Method::from_http("GET", "/"), Err((404, "no such route")));
    }

    #[test]
    fn input_parameters() {
        let ok = validate(Method::Input, &json!({ "button": "a" })).unwrap();
        assert_eq!(ok, json!({ "events": [{ "kind": "button", "button": "a", "action": "tap", "ms": 80 }] }));
        let two = validate(Method::Input, &json!({ "events": [{ "button": "down", "action": "down" }, { "axis": "rt", "value": 3 }, { "axis": "left_y", "value": -0.5 }] })).unwrap();
        assert_eq!(two["events"][1]["value"], 1.0);
        assert_eq!(two["events"][2]["value"], -0.5);
        assert!(validate(Method::Input, &json!({})).is_err());
        assert!(validate(Method::Input, &json!({ "events": [] })).is_err());
        assert!(validate(Method::Input, &json!({ "button": "a", "action": "mash" })).is_err());
        assert!(validate(Method::Input, &json!({ "button": "a", "ms": 5 })).is_err());
        assert!(validate(Method::Input, &json!({ "axis": "left_x" })).is_err());
        assert!(validate(Method::Input, &json!({ "events": [1] })).is_err());
        let many: Vec<Value> = (0..17).map(|_| json!({ "button": "a" })).collect();
        assert!(validate(Method::Input, &json!({ "events": many })).is_err());
        // Taps add up: four seconds of them in one call is too much.
        let long: Vec<Value> = (0..5).map(|_| json!({ "button": "a", "ms": 800 })).collect();
        assert!(validate(Method::Input, &json!({ "events": long })).is_err());
    }

    #[test]
    fn launch_close_parameters() {
        assert_eq!(validate(Method::Launch, &json!({ "id": "steam:620" })).unwrap(), json!({ "id": "steam:620" }));
        assert!(validate(Method::Launch, &json!({})).is_err());
        assert!(validate(Method::Launch, &json!({ "id": "a\nb" })).is_err());
        assert_eq!(validate(Method::Close, &json!({})).unwrap(), json!({ "id": null }));
        assert_eq!(validate(Method::Close, &json!({ "id": "steam:620" })).unwrap()["id"], "steam:620");
    }

    #[test]
    fn network_routes_are_not_for_games() {
        assert_eq!(Method::from_http("POST", "/v1/launch"), Ok(Method::Launch));
        assert_eq!(Method::from_http("GET", "/v1/screenshot"), Ok(Method::Screenshot));
        assert_eq!(Method::from_http("GET", "/v1/capabilities"), Ok(Method::Capabilities));
        assert!(Method::Launch.network_only());
        assert!(!Method::Sell.network_only());
        // A game's token is refused with a plain reason, not a 404.
        let w = world();
        let r = req("POST", "/v1/launch", &[("Authorization", &bearer_of(NATIVE)), ("Content-Type", "application/json")], r#"{"id":"steam:620"}"#);
        assert_eq!(status(&run(&w, &r, true)), 403);
    }

    #[test]
    fn sell_parameters() {
        let id = "0x00000000000000000000000000000000000000000000000000000000000000ab";
        assert_eq!(validate(Method::Sell, &json!({ "itemId": "0xAB" })).unwrap(), json!({ "itemId": id, "mode": "best-offer", "priceMist": null }));
        assert_eq!(validate(Method::Sell, &json!({ "itemId": "0xab", "mode": "list", "priceMist": "1500000000" })).unwrap()["priceMist"], "1500000000");
        assert_eq!(validate(Method::Sell, &json!({ "itemId": "0xab", "mode": "list", "priceMist": 7 })).unwrap()["priceMist"], "7");
        assert!(validate(Method::Sell, &json!({})).is_err());
        assert!(validate(Method::Sell, &json!({ "itemId": "ab" })).is_err());
        assert!(validate(Method::Sell, &json!({ "itemId": "0xab", "mode": "auction" })).is_err());
        assert!(validate(Method::Sell, &json!({ "itemId": "0xab", "priceMist": "5" })).is_err());
        assert!(validate(Method::Sell, &json!({ "itemId": "0xab", "mode": "list", "priceMist": "-5" })).is_err());
        assert!(validate(Method::Sell, &json!({ "itemId": "0xab", "mode": "list", "priceMist": "0" })).is_err());
        assert!(validate(Method::Sell, &json!({ "itemId": "0xab", "mode": "list", "priceMist": "99999999999999999999" })).is_err());
        assert!(validate(Method::Sell, &json!([1])).is_err());
    }

    #[test]
    fn buy_parameters() {
        assert!(validate(Method::Buy, &json!({ "itemId": "0x1" })).is_ok());
        assert_eq!(validate(Method::Buy, &json!({ "kind": 3, "maxMist": 2000000000u64 })).unwrap(), json!({ "itemId": null, "collection": null, "kind": 3, "maxMist": "2000000000" }));
        assert!(validate(Method::Buy, &json!({ "collection": "0x2::popkins::Popkin", "maxMist": "5" })).is_ok());
        assert!(validate(Method::Buy, &json!({})).is_err());
        assert!(validate(Method::Buy, &json!({ "maxMist": "5" })).is_err());
        assert!(validate(Method::Buy, &json!({ "itemId": "0x1", "kind": 3 })).is_err());
        assert!(validate(Method::Buy, &json!({ "collection": "popkins" })).is_err());
        assert!(validate(Method::Buy, &json!({ "kind": -1 })).is_err());
    }

    #[test]
    fn claim_and_open_parameters() {
        assert!(validate(Method::Claim, &json!({ "voucher": "eyJ2IjoxfQ" })).is_ok());
        assert!(validate(Method::Claim, &json!({ "voucher": "" })).is_err());
        assert!(validate(Method::Claim, &json!({ "voucher": "a b" })).is_err());
        assert!(validate(Method::Open, &json!({ "view": "overlay" })).is_ok());
        assert!(validate(Method::Open, &json!({ "view": "trophies" })).is_ok());
        assert!(validate(Method::Open, &json!({ "view": "store", "id": "my-game" })).is_ok());
        assert!(validate(Method::Open, &json!({ "view": "item", "id": "0xabc" })).is_ok());
        assert!(validate(Method::Open, &json!({ "view": "collection", "id": "0x2::popkins::Popkin" })).is_ok());
        assert!(validate(Method::Open, &json!({ "view": "item" })).is_err());
        assert!(validate(Method::Open, &json!({ "view": "settings" })).is_err());
        assert!(validate(Method::Open, &json!({ "view": "overlay", "id": "x" })).is_err());
        assert!(validate(Method::Open, &json!({ "view": "store", "id": "../x" })).is_err());
    }

    #[test]
    fn reads_take_no_parameters() {
        assert_eq!(validate(Method::Me, &Value::Null).unwrap(), json!({}));
        assert_eq!(validate(Method::Inventory, &json!({ "anything": 1 })).unwrap(), json!({}));
    }

    #[test]
    fn env_slug_and_link_arguments() {
        assert_eq!(game_env_slug("rare:my-game"), "my-game");
        assert_eq!(game_env_slug("web:act"), "act");
        assert_eq!(game_env_slug("local:abc"), "local:abc");
        let args = ["/usr/bin/rare-launcher", "--x", "rare://item/0x1", "https://rare.market/u/0x1", "https://evil.example/rare://"].map(String::from);
        assert_eq!(links_in(args), vec!["rare://item/0x1".to_string(), "https://rare.market/u/0x1".to_string()]);
    }

    /// The network switches and listener are process-wide: tests that use them take turns.
    static NET_TEST: Mutex<()> = Mutex::new(());

    const PORT: u16 = 4100;
    const NATIVE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const WEB: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const GAME_ORIGIN: &str = "https://act.example";

    struct World {
        grants: Mutex<Grants>,
        limits: Mutex<Limits>,
        now: Instant,
    }

    fn world() -> World {
        let now = Instant::now();
        let mut g = Grants::default();
        g.issue(NATIVE.into(), "local:game", Kind::Native, now);
        g.issue(WEB.into(), "web:act", Kind::Web { origin: GAME_ORIGIN.into() }, now);
        World { grants: Mutex::new(g), limits: Mutex::new(Limits::new(now)), now }
    }

    fn req(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> http::Request {
        let mut s = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\n");
        for (k, v) in headers {
            s.push_str(&format!("{k}: {v}\r\n"));
        }
        if !body.is_empty() {
            s.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
        }
        s.push_str("\r\n");
        s.push_str(body);
        http::read_request(&mut std::io::Cursor::new(s.into_bytes())).unwrap()
    }

    fn run(w: &World, r: &http::Request, connected: bool) -> Gate {
        gate(r, PORT, &w.grants, &w.limits, &|_o: &str| connected, w.now)
    }

    fn status(g: &Gate) -> u16 {
        match g {
            Gate::Pass { .. } => 200,
            Gate::Preflight(_) => 204,
            Gate::Refuse(s, _, _) => *s,
        }
    }

    fn bearer_of(t: &str) -> String {
        format!("Bearer {t}")
    }

    #[test]
    fn gate_needs_a_known_token() {
        let w = world();
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[], ""), false)), 401);
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", "Bearer nope")], ""), false)), 401);
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", &format!("Basic {NATIVE}"))], ""), false)), 401);
        match run(&w, &req("GET", "/v1/me", &[("Authorization", &bearer_of(NATIVE))], ""), false) {
            Gate::Pass { grant, method, .. } => {
                assert_eq!(grant.game_id, "local:game");
                assert_eq!(method, Method::Me);
            }
            g => panic!("{g:?}"),
        }
    }

    #[test]
    fn network_calls_need_their_switch() {
        let _one = NET_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let net_tok = "n".repeat(64);
        let mut g = Grants::default();
        g.issue(net_tok.clone(), "device:network", Kind::Network, now);
        let (grants, limits) = (Mutex::new(g), Mutex::new(Limits::new(now)));
        let call = |method: &str, path: &str, body: &str, host: &str, origin: Option<&str>| {
            let mut s = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {net_tok}\r\n");
            if let Some(o) = origin {
                s.push_str(&format!("Origin: {o}\r\n"));
            }
            if !body.is_empty() {
                s.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
            }
            s.push_str("\r\n");
            s.push_str(body);
            let r = http::read_request(&mut std::io::Cursor::new(s.into_bytes())).unwrap();
            status(&gate_network(&r, 47100, "suiplay", &grants, &limits, now))
        };
        let h = "192.168.0.50:47100";
        // Every switch starts off.
        network_set_caps(&[]);
        assert_eq!(network_caps(), Vec::<&str>::new());
        for (m, path, body) in [
            ("GET", "/v1/me", ""),
            ("GET", "/v1/inventory", ""),
            ("GET", "/v1/library", ""),
            ("POST", "/v1/open", r#"{"view":"trophies"}"#),
            ("POST", "/v1/launch", r#"{"id":"steam:620"}"#),
            ("POST", "/v1/input", r#"{"button":"a"}"#),
            ("POST", "/v1/sell", r#"{"itemId":"0x1"}"#),
            ("GET", "/v1/screenshot", ""),
        ] {
            assert_eq!(call(m, path, body, h, None), 403, "{path} with every switch off");
        }
        // What is on and off is always answered.
        assert_eq!(call("GET", "/v1/capabilities", "", h, None), 200);
        // One switch opens only its own routes.
        network_set_caps(&["profile".to_string()]);
        assert_eq!(call("GET", "/v1/me", "", h, None), 200);
        assert_eq!(call("GET", "/v1/me", "", "suiplay.local:47100", None), 200);
        assert_eq!(call("GET", "/v1/inventory", "", h, None), 403);
        assert_eq!(call("POST", "/v1/launch", r#"{"id":"steam:620"}"#, h, None), 403);
        network_set_caps(&["library".to_string(), "launch".to_string(), "input".to_string(), "purchase".to_string(), "screenshot".to_string()]);
        assert_eq!(call("GET", "/v1/me", "", h, None), 403);
        assert_eq!(call("GET", "/v1/inventory", "", h, None), 200);
        assert_eq!(call("GET", "/v1/library", "", h, None), 200);
        assert_eq!(call("POST", "/v1/open", r#"{"view":"trophies"}"#, h, None), 200);
        // (serve releases a sheet's slot when the call is done)
        lock(&limits).release(&net_tok, Class::Sheet);
        assert_eq!(call("POST", "/v1/launch", r#"{"id":"steam:620"}"#, h, None), 200);
        assert_eq!(call("POST", "/v1/launch", r#"{}"#, h, None), 400);
        assert_eq!(call("POST", "/v1/close", "", h, None), 200);
        assert_eq!(call("POST", "/v1/input", r#"{"events":[{"button":"a"},{"axis":"left_x","value":1}]}"#, h, None), 200);
        assert_eq!(call("POST", "/v1/input", r#"{"button":"turbo"}"#, h, None), 400);
        assert_eq!(call("POST", "/v1/sell", r#"{"itemId":"0x1"}"#, h, None), 200);
        assert_eq!(call("GET", "/v1/screenshot", "", h, None), 200);
        // Never for network apps: the overlay, claims, notices.
        assert_eq!(call("POST", "/v1/open", r#"{"view":"overlay"}"#, h, None), 403);
        assert_eq!(call("POST", "/v1/claim", r#"{"voucher":"AAAA"}"#, h, None), 403);
        assert_eq!(call("GET", "/v1/notices", "", h, None), 403);
        network_set_caps(&[]);
        // A web page (a rebind or a cross-site call) is refused, and so is a name that isn't ours.
        assert_eq!(call("GET", "/v1/me", "", h, Some("https://evil.example")), 403);
        assert_eq!(call("GET", "/v1/me", "", "evil.example:47100", None), 421);
        // A game's token means nothing here.
        let r = req("GET", "/v1/me", &[("Authorization", &bearer_of(NATIVE))], "");
        assert_eq!(status(&gate_network(&r, PORT, "suiplay", &grants, &limits, now)), 401);
    }

    /// The whole path over a real socket: token, host, switches, the audit line.
    #[test]
    fn network_listener_end_to_end() {
        let _one = NET_TEST.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::{Read, Write};
        start();
        let port = 47391;
        let token = "e".repeat(64);
        network_enable(&token, port, "suiplay").unwrap();
        let call = |path: &str, method: &str, body: &str, tok: &str| -> (u16, Value) {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let host = format!("192.168.0.9:{port}");
            let head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            c.write_all(head.as_bytes()).unwrap();
            let mut out = String::new();
            c.read_to_string(&mut out).unwrap();
            let status: u16 = out.split(' ').nth(1).unwrap().parse().unwrap();
            let json = out.split("\r\n\r\n").nth(1).and_then(|b| serde_json::from_str(b).ok()).unwrap_or(Value::Null);
            (status, json)
        };
        network_set_caps(&[]);
        let (st, v) = call("/v1/launch", "POST", r#"{"id":"steam:620"}"#, &token);
        assert_eq!((st, v["error"]["code"].as_str(), v["error"]["capability"].as_str()), (403, Some("capability_off"), Some("launch")));
        let (st, v) = call("/v1/capabilities", "GET", "", &token);
        assert_eq!(st, 200);
        assert_eq!(v["capabilities"].as_array().unwrap().len(), 8);
        assert!(v["capabilities"].as_array().unwrap().iter().all(|c| c["enabled"] == false));
        assert_eq!(call("/v1/capabilities", "GET", "", "wrong").0, 401);
        // The refusal is in the audit trail, with the switch it was about.
        let a = network_activity();
        assert!(a.iter().any(|x| x.cap == "launch" && !x.ok && x.from == "127.0.0.1"), "{a:?}");
        // With the switch on, input reaches the host (the inert test host says it can't).
        network_set_caps(&["input".to_string()]);
        let (st, v) = call("/v1/input", "POST", r#"{"button":"a"}"#, &token);
        assert_eq!((st, v["error"]["code"].as_str()), (503, Some("unavailable")));
        // MCP: the handshake, then only the tools whose switch is on (and public ones).
        let (st, v) = call("/v1/mcp", "POST", r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#, &token);
        assert_eq!((st, v["result"]["serverInfo"]["name"].as_str()), (200, Some("rare")));
        assert_eq!(call("/v1/mcp", "POST", r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &token).0, 202);
        network_set_caps(&["settings".to_string()]);
        let (_, v) = call("/v1/mcp", "POST", r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, &token);
        let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"change_setting") && !names.contains(&"wallet_send"), "{names:?}");
        let (_, v) = call("/v1/mcp", "POST", r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"wallet_send","arguments":{"to":"0x1","amount":"1"}}}"#, &token);
        assert_eq!(v["result"]["isError"], true);
        assert_eq!(call("/v1/mcp", "POST", r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#, "wrong").0, 401);
        network_set_caps(&[]);
        network_disable();
    }

    #[test]
    fn gate_checks_the_host() {
        let w = world();
        let mut r = req("GET", "/v1/me", &[("Authorization", &bearer_of(NATIVE))], "");
        r.headers.retain(|(n, _)| n != "host");
        r.headers.push(("host".into(), "rebind.evil.example:4100".into()));
        assert_eq!(status(&run(&w, &r, false)), 421);
    }

    #[test]
    fn gate_origin_rules() {
        let w = world();
        // A web page can't use a native game's token.
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", &bearer_of(NATIVE)), ("Origin", "https://evil.example")], ""), false)), 403);
        // A web game's token only from its own origin, and never without one.
        assert_eq!(status(&run(&w, &req("POST", "/v1/open", &[("Authorization", &bearer_of(WEB)), ("Origin", "https://evil.example")], r#"{"view":"overlay"}"#), true)), 403);
        assert_eq!(status(&run(&w, &req("POST", "/v1/open", &[("Authorization", &bearer_of(WEB))], r#"{"view":"overlay"}"#), true)), 403);
        match run(&w, &req("POST", "/v1/open", &[("Authorization", &bearer_of(WEB)), ("Origin", GAME_ORIGIN)], r#"{"view":"overlay"}"#), true) {
            Gate::Pass { cors, .. } => assert!(cors.contains(&("Access-Control-Allow-Origin".to_string(), GAME_ORIGIN.to_string()))),
            g => panic!("{g:?}"),
        }
        // The address and holdings only once the player connected Rare Wallet to the web game.
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", &bearer_of(WEB)), ("Origin", GAME_ORIGIN)], ""), false)), 403);
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", &bearer_of(WEB)), ("Origin", GAME_ORIGIN)], ""), true)), 200);
    }

    #[test]
    fn gate_preflight_only_for_web_game_origins() {
        let w = world();
        assert_eq!(status(&run(&w, &req("OPTIONS", "/v1/sell", &[("Origin", GAME_ORIGIN)], ""), false)), 204);
        assert_eq!(status(&run(&w, &req("OPTIONS", "/v1/sell", &[("Origin", "https://evil.example")], ""), false)), 403);
        assert_eq!(status(&run(&w, &req("OPTIONS", "/v1/sell", &[], ""), false)), 403);
    }

    #[test]
    fn gate_routes_bodies_and_parameters() {
        let w = world();
        let auth = bearer_of(NATIVE);
        assert_eq!(status(&run(&w, &req("GET", "/v1/nope", &[("Authorization", &auth)], ""), false)), 404);
        assert_eq!(status(&run(&w, &req("GET", "/v1/sell", &[("Authorization", &auth)], ""), false)), 405);
        assert_eq!(status(&run(&w, &req("POST", "/v1/sell", &[("Authorization", &auth)], "{not json"), false)), 400);
        assert_eq!(status(&run(&w, &req("POST", "/v1/sell", &[("Authorization", &auth)], r#"{"itemId":"nope"}"#), false)), 400);
        let mut text = req("POST", "/v1/sell", &[("Authorization", &auth)], r#"{"itemId":"0x1"}"#);
        text.headers.retain(|(n, _)| n != "content-type");
        text.headers.push(("content-type".into(), "text/plain".into()));
        assert_eq!(status(&run(&w, &text, false)), 415);
        match run(&w, &req("POST", "/v1/sell", &[("Authorization", &auth)], r#"{"itemId":"0x1"}"#), false) {
            Gate::Pass { method, params, .. } => {
                assert_eq!(method, Method::Sell);
                assert_eq!(params["mode"], "best-offer");
            }
            g => panic!("{g:?}"),
        }
    }

    #[test]
    fn gate_rate_limits() {
        let w = world();
        let auth = bearer_of(NATIVE);
        let sell = || req("POST", "/v1/sell", &[("Authorization", &auth)], r#"{"itemId":"0x1"}"#);
        // One sheet at a time: a second sell while the first is up is busy.
        assert_eq!(status(&run(&w, &sell(), false)), 200);
        assert_eq!(status(&run(&w, &sell(), false)), 409);
        lock(&w.limits).release(NATIVE, Class::Sheet);
        // Reads: the burst, then 429 with Retry-After.
        for _ in 0..auth::READ_BURST as usize {
            assert_eq!(status(&run(&w, &req("GET", "/v1/inventory", &[("Authorization", &auth)], ""), false)), 200);
        }
        match run(&w, &req("GET", "/v1/inventory", &[("Authorization", &auth)], ""), false) {
            Gate::Refuse(429, h, _) => assert!(h.iter().any(|(k, _)| k == "Retry-After")),
            g => panic!("{g:?}"),
        }
        // The web game's token has its own budget.
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", &bearer_of(WEB)), ("Origin", GAME_ORIGIN)], ""), true)), 200);
    }

    #[test]
    fn gate_locks_out_token_guessing() {
        let w = world();
        for _ in 0..auth::MAX_AUTH_FAILURES {
            assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", "Bearer guess")], ""), false)), 401);
        }
        // Even the right token waits out the lockout: guessing gains nothing.
        assert_eq!(status(&run(&w, &req("GET", "/v1/me", &[("Authorization", &bearer_of(NATIVE))], ""), false)), 429);
    }

    #[test]
    fn tokens_are_random_and_long() {
        let a = new_token().unwrap();
        let b = new_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
