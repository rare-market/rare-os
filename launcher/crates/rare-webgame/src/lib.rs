//! Browser games as installed web apps: one fullscreen window per game with its own storage,
//! controller-to-keyboard mapping, an on-screen keyboard, and Rare Wallet injected as a Wallet
//! Standard wallet. Pages get no IPC into Rare at all. The window itself is a [`WebEngine`]
//! (the native launcher's Chromium host in its own process, the old Tauri shell's WebKitGTK window);
//! everything else is here.

pub mod engine;
#[cfg(unix)]
pub mod process;

use engine::{EngineEvents, OpenSpec, WebEngine};
use rare_core::{Ctx, Event, Pad};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// What the web game code needs from the launcher itself.
pub trait WebHost: Send + Sync {
    /// The launcher's main window exists (wallet requests are answered there).
    fn main_exists(&self) -> bool {
        false
    }
    /// Shows the main window and focuses it (Guide sends the player back to the launcher).
    fn main_show(&self) {}
    fn main_focus(&self) {}
    /// The launcher is on testnet (the dev banner says which network).
    fn testnet(&self) -> bool {
        false
    }
}

struct Env {
    ctx: Ctx,
    engine: Arc<dyn WebEngine>,
    host: Arc<dyn WebHost>,
}

static ENV: OnceLock<Env> = OnceLock::new();

/// Installs the launcher and the browser engine. The first call wins.
pub fn install(ctx: Ctx, engine: Arc<dyn WebEngine>, host: Arc<dyn WebHost>) {
    let _ = ENV.set(Env { ctx, engine, host });
}

pub fn installed() -> bool {
    ENV.get().is_some()
}

fn env() -> Option<&'static Env> {
    ENV.get()
}

fn engine() -> Option<&'static dyn WebEngine> {
    env().map(|e| e.engine.as_ref())
}

fn host_main_exists() -> bool {
    env().is_some_and(|e| e.host.main_exists())
}

/// Sends an event to the launcher's UI.
fn emit(name: &str, payload: Value) {
    if let Some(e) = env() {
        e.ctx.send(Event::Other { name: name.to_string(), payload });
    }
}

pub const LABEL: &str = "webgame";

/// True while a web game window is open and shown. Kept here so the input thread can check
/// it 60 times a second without asking the window system.
static VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn visible() -> bool {
    VISIBLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Bumped for every window opened, so a closing old window can't hide the new one's state.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn set_visible(v: bool) {
    VISIBLE.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// The open window has Rare Wallet (a web game, or an app whose way asks for it). False in app
/// mode: no wallet in the page and its `rare-wallet:` requests are refused.
static WALLET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether a window opened with `profile` gets Rare Wallet: its `"wallet"` key, else yes for a
/// game and no for an app (`app:<id>`, a webview app from the Apps catalog).
pub fn wants_wallet(game_id: &str, profile: &Value) -> bool {
    profile.get("wallet").and_then(Value::as_bool).unwrap_or(!game_id.starts_with("app:"))
}
const JS: &str = include_str!("webgame.js");

/// The account the launcher's Rare Wallet has selected, pushed from the UI.
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletSession {
    pub address: Option<String>,
    pub label: Option<String>,
    /// "watch", "native", "passkey", "ledger", "social"
    pub kind: Option<String>,
    pub public_key: Option<String>,
}

/// The shared state of the web game window. A process-wide singleton, like the window.
pub fn state() -> &'static WebGames {
    static S: OnceLock<WebGames> = OnceLock::new();
    S.get_or_init(WebGames::default)
}

#[derive(Default)]
pub struct WebGames {
    pub session: Mutex<WalletSession>,
    /// Origins the player approved (set by the launcher UI). Only these may ask for signatures.
    pub approved: Mutex<Vec<String>>,
    pub current: Mutex<Option<(String, String)>>, // (game id, title)
    /// Requests waiting for the launcher UI.
    pub pending: Mutex<HashMap<u64, Pending>>,
    /// The main window's hold on the screen while a sheet is up (see overlay::bring_main_front).
    front: Mutex<Option<rare_overlay::Front>>,
}

#[derive(Clone, Copy)]
pub struct Pending {
    /// we moved focus to the launcher for it
    focus: bool,
    connect: bool,
}

#[derive(Deserialize)]
struct Request {
    id: u64,
    method: String,
    #[serde(default)]
    params: Value,
}

const METHODS: [&str; 5] = ["connect", "signTransaction", "signAndExecuteTransaction", "signPersonalMessage", "platform"];
/// Requests carry a transaction or message; anything larger is not a real one.
const MAX_REQUEST: usize = 4 << 20;

/// A JSON value as a JS expression. JSON is valid JS except U+2028/2029 in older engines.
fn js_literal(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into()).replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")
}

fn reply(id: u64, ok: bool, value: &Value) {
    if let Some(e) = engine() {
        if e.is_open() {
            e.eval(&format!("window.__rareWalletReply && window.__rareWalletReply({id}, {ok}, {})", js_literal(value)));
        }
    }
}

/// The game window's real origin. The page's own claim is ignored: a page could lie.
fn window_origin() -> Option<String> {
    let url: url::Url = engine()?.url()?.parse().ok()?;
    Some(url.origin().ascii_serialization())
}

/// A request from the page (`rare-wallet:` navigation), base64 JSON in the path.
fn handle(raw: &str) {
    use base64::Engine;
    if raw.len() > MAX_REQUEST {
        return;
    }
    let decoded = urlencoding_decode(raw);
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(decoded.trim()) else { return };
    let Ok(req) = serde_json::from_slice::<Request>(&bytes) else { return };
    if !METHODS.contains(&req.method.as_str()) {
        reply(req.id, false, &json!("Unsupported request"));
        return;
    }
    // App mode: the page has no wallet, so nothing it sends is a wallet request.
    if !WALLET.load(std::sync::atomic::Ordering::Relaxed) {
        reply(req.id, false, &json!("Unsupported request"));
        return;
    }
    let Some(origin) = window_origin() else { return };
    if req.method == "platform" {
        // rare:platform (the Rare Local API for web games): checked and run by localapi, which
        // raises its own sheets over the game.
        let call = req.params.get("call").and_then(Value::as_str).unwrap_or("").to_string();
        let token = req.params.get("token").and_then(Value::as_str).unwrap_or("").to_string();
        let args = req.params.get("args").cloned().unwrap_or(Value::Null);
        match rare_localapi::web_call(&origin, &token, &call, args) {
            Ok(v) => reply(req.id, true, &v),
            Err(e) => reply(req.id, false, &json!(e)),
        }
        return;
    }
    let state = state();
    let (game_id, title) = state.current.lock().unwrap().clone().unwrap_or_default();
    let approved = state.approved.lock().unwrap().contains(&origin);
    if req.method != "connect" && !approved {
        reply(req.id, false, &json!("Connect Rare Wallet first"));
        return;
    }
    if !host_main_exists() {
        reply(req.id, false, &json!("Rare Wallet is not available"));
        return;
    }
    // A connect from an approved origin may be answered by the UI without showing anything,
    // so the game keeps focus; everything else brings up the launcher's approval sheet.
    let take_focus = !(req.method == "connect" && approved);
    state.pending.lock().unwrap().insert(req.id, Pending { focus: take_focus, connect: req.method == "connect" });
    let payload = json!({ "id": req.id, "method": req.method, "params": req.params, "origin": origin, "game": title, "gameId": game_id });
    emit("wallet-request", payload);
    if take_focus {
        bring_front();
    }
}

/// The launcher's sheet comes to the front, through the overlay's focus helper (gamescope base layer
/// in Game Mode). Held until the last request that needed it is answered.
fn bring_front() {
    let mut front = state().front.lock().unwrap();
    if front.is_some() {
        if let Some(e) = env() {
            e.host.main_focus();
        }
        return;
    }
    *front = rare_overlay::bring_main_front();
}

/// Drops the hold on the screen without touching the game window (a new game is opening).
fn give_back_front() {
    let held = state().front.lock().unwrap().take();
    if let Some(f) = held {
        rare_overlay::release_front(f);
    }
}

/// The sheet is done: the game gets the screen and focus back.
fn give_back() {
    give_back_front();
    if let Some(e) = engine() {
        if e.is_open() {
            e.focus();
        }
    }
}

/// The UI needs to show a sheet for a request that Rust expected to answer silently (a connect from an
/// approved origin that needs the player after all: locked wallet, no account). Brings the launcher up.
pub fn wallet_focus(id: u64) {
    {
        let mut p = state().pending.lock().unwrap();
        let Some(req) = p.get_mut(&id) else { return };
        if req.focus {
            return;
        }
        req.focus = true;
    }
    bring_front();
}

/// The launcher UI answers a web game request. `value` is the result JSON, or an error string.
pub fn wallet_reply(id: u64, ok: bool, value: Value) {
    let state = state();
    let (req, any_left) = {
        let mut p = state.pending.lock().unwrap();
        let Some(req) = p.remove(&id) else { return };
        (req, p.values().any(|r| r.focus))
    };
    reply(id, ok, &value);
    if req.connect && ok {
        // The launcher's toast shows which game connected.
        let game = state.current.lock().unwrap().as_ref().map(|c| c.1.clone()).unwrap_or_default();
        emit("wallet-connected", json!({ "game": game }));
    }
    if req.focus && !any_left {
        give_back();
    }
}

/// Origins the player approved for the current session.
pub fn wallet_set_approved(origins: Vec<String>) {
    *state().approved.lock().unwrap() = origins;
}

fn urlencoding_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// https anywhere; plain http only on this machine (localhost, 127.0.0.1, [::1], any port), for developers
/// testing their game. The add dialog applies the same rule (src/lib/webgames.ts).
pub fn url_allowed(u: &url::Url) -> bool {
    match u.scheme() {
        "https" => true,
        "http" => matches!(u.host(), Some(url::Host::Domain("localhost")) | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST)) | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))),
        _ => false,
    }
}

pub fn open(game_id: &str, title: &str, url: &str) -> Result<(), String> {
    open_with(game_id, title, url, None)
}

/// What the engine reports back for the window `open_with` opened.
struct Events {
    /// The window's generation: a closing old window can't hide the new one's state.
    gen: u64,
    on_rare: bool,
}

/// A sign-in window's watcher (`open_sign_in`): sees every navigation of that window first.
struct SignIn {
    gen: u64,
    /// False blocks the navigation (the window got what it was opened for).
    nav: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    closed: Arc<dyn Fn() + Send + Sync>,
}

static SIGN_IN: Mutex<Option<SignIn>> = Mutex::new(None);

impl EngineEvents for Events {
    fn navigation(&self, url: &str) -> bool {
        let watcher = SIGN_IN.lock().unwrap().as_ref().filter(|w| w.gen == self.gen).map(|w| w.nav.clone());
        if let Some(nav) = watcher {
            if !nav(url) {
                return false;
            }
        }
        let Ok(u) = url.parse::<url::Url>() else { return true };
        if u.scheme() == "rare-wallet" {
            let path = u.path().trim_start_matches('/').trim_start_matches("request/").to_string();
            std::thread::spawn(move || handle(&path));
            return false;
        }
        // rare:// links, and rare.market item, collection, game, store, profile and claim links
        // (unless the game itself is on rare.market), open in Rare instead of the game window.
        if rare_localapi::deeplink::is_rare_link(&u) && (u.scheme() == "rare" || !self.on_rare) {
            let link = u.to_string();
            std::thread::spawn(move || rare_localapi::open_link(&link));
            return false;
        }
        true
    }

    fn closed(&self) {
        if GENERATION.load(std::sync::atomic::Ordering::Relaxed) == self.gen {
            set_visible(false);
        }
        let watcher = {
            let mut w = SIGN_IN.lock().unwrap();
            if w.as_ref().is_some_and(|w| w.gen == self.gen) {
                w.take()
            } else {
                None
            }
        };
        if let Some(w) = watcher {
            (w.closed)();
        }
    }
}

/// Opens a store's sign-in page (Epic, GOG) in the browser window in app mode, with `script` in
/// every page. `nav` sees each navigation first and returns false to stop it (it has the sign-in
/// code); `closed` runs when the window goes (B, or the launcher closed it). Uses the external
/// link window, so B and the Rare button close it like any link.
pub fn open_sign_in(title: &str, url: &str, script: &str, nav: Arc<dyn Fn(&str) -> bool + Send + Sync>, closed: Arc<dyn Fn() + Send + Sync>) -> Result<(), String> {
    let profile = json!({ "input": "focus", "wallet": false, "script": script });
    // Set before the window exists so the first navigation is seen; the generation is the next one.
    let gen = GENERATION.load(std::sync::atomic::Ordering::Relaxed) + 1;
    *SIGN_IN.lock().unwrap() = Some(SignIn { gen, nav, closed });
    let r = open_with(EXTERNAL_ID, title, url, Some(profile));
    if r.is_err() {
        *SIGN_IN.lock().unwrap() = None;
    }
    r
}

/// Id of the window `open_external` uses.
pub const EXTERNAL_ID: &str = "app:external";

/// Opens a link (View in Marketplace, View on Suiscan) in the web window in app mode: no Rare
/// Wallet and no Local API in the page.
pub fn open_external(url: &str) -> Result<(), String> {
    open_with(EXTERNAL_ID, "Browser", url, Some(json!({ "input": "focus", "wallet": false })))
}

/// Id of the window `open_portal` uses.
pub const PORTAL_ID: &str = "app:portal";

/// Opens a Wi-Fi network's sign-in page (a captive portal) in the web window in app mode, like an
/// external link. Portal pages are often plain http and redirect to odd hosts, so this window alone
/// may open http anywhere; its own redirects go ahead as in any window.
pub fn open_portal(url: &str) -> Result<(), String> {
    open_spec(PORTAL_ID, "Wi-Fi sign-in", url, Some(json!({ "input": "focus", "wallet": false })), true)
}

/// The Wi-Fi sign-in window is open (shown or not).
pub fn portal_open() -> bool {
    current_id().as_deref() == Some(PORTAL_ID)
}

/// The window in front is an external link or a Wi-Fi sign-in (B and the Rare button close it).
pub fn external_open() -> bool {
    matches!(current_id().as_deref(), Some(EXTERNAL_ID) | Some(PORTAL_ID))
}

/// Closes the external link window and brings the launcher back.
pub fn close_external() {
    close();
    if let Some(env) = env() {
        if env.host.main_exists() {
            env.host.main_show();
            env.host.main_focus();
        }
    }
}

/// `open`, with a controller profile given by the caller (apps) instead of profiles.json.
pub fn open_with(game_id: &str, title: &str, url: &str, profile: Option<Value>) -> Result<(), String> {
    open_spec(game_id, title, url, profile, false)
}

/// `open_with`; `any_http` lets plain http through anywhere (the Wi-Fi sign-in window only).
fn open_spec(game_id: &str, title: &str, url: &str, profile: Option<Value>, any_http: bool) -> Result<(), String> {
    let (Some(engine), Some(env)) = (engine(), env()) else { return Err("No web game window available".into()) };
    let parsed: url::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;
    if !(url_allowed(&parsed) || (any_http && parsed.scheme() == "http")) {
        return Err("Web games must use https (http only for localhost)".into());
    }
    if engine.is_open() {
        engine.close();
    }
    let state = state();
    *state.current.lock().unwrap() = Some((game_id.to_string(), title.to_string()));
    // Requests from the previous game can't be answered any more.
    state.pending.lock().unwrap().clear();
    give_back_front();
    // Storage: the game's own, or the one the profile names (a cloud game shares its app's sign-in).
    let data_id = profile.as_ref().and_then(|p| p.get("dataId")).and_then(Value::as_str).unwrap_or(game_id).to_string();
    let safe_id: String = data_id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    // The game's controller profile goes in first, then the runtime that reads it.
    let mut profile = profile.or_else(|| rare_library::profiles::for_game(game_id, url).map(|(_, p)| p)).unwrap_or_else(|| json!({}));
    // App mode (X, Telegram Web): no Rare Wallet and no Local API in the page; webgame.js reads
    // the profile's `wallet`.
    let wallet = wants_wallet(game_id, &profile);
    if let Some(o) = profile.as_object_mut() {
        o.insert("wallet".into(), json!(wallet));
    }
    WALLET.store(wallet, std::sync::atomic::Ordering::Relaxed);
    // This launch's Local API token, for this game at this origin (webgame.js takes it off window).
    let origin = parsed.origin().ascii_serialization();
    let api = if wallet { rare_localapi::grant_web(game_id, &origin).map(|a| serde_json::to_value(a).unwrap_or(Value::Null)).unwrap_or(Value::Null) } else { Value::Null };
    let network = if env.host.testnet() { "testnet" } else { "mainnet" };
    // A sign-in window's own script (open_sign_in), after the runtime.
    let extra = profile.as_object_mut().and_then(|o| o.remove("script")).and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let script = format!("window.__rareProfile = {profile};\nwindow.__rareApi = {};\n{JS}\n{}\n{extra}", js_literal(&api), rare_system::devmode::script_for(game_id, network));
    let on_rare = matches!(parsed.host_str(), Some("rare.market") | Some("www.rare.market"));
    let gen = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let spec = OpenSpec {
        label: LABEL,
        title: title.to_string(),
        url: parsed.to_string(),
        init_script: script,
        data_dir: dirs::data_dir().map(|d| d.join("rare/webgames").join(&safe_id)),
        wallet,
        user_agent: profile.get("userAgent").and_then(Value::as_str).map(str::to_string),
        stream: profile.get("stream").and_then(Value::as_bool).unwrap_or(false),
    };
    engine.open(&spec, Arc::new(Events { gen, on_rare }))?;
    rare_overlay::gamescope::claim(LABEL);
    set_visible(true);
    Ok(())
}

/// True when the game's profile asks for a Chromium browser.
pub fn wants_chrome(game_id: &str, url: &str) -> bool {
    rare_library::profiles::for_game(game_id, url).is_some_and(|(_, p)| rare_library::profiles::wants_chrome(&p))
}

/// Full analog controller state for the open web game (virtual Gamepad, pointer stick).
/// Standard mapping: 17 buttons, 4 axes with down positive.
pub fn pad_state(buttons: &[f32; 17], axes: &[f32; 4]) {
    let Some(e) = engine() else { return };
    if !e.is_open() {
        return;
    }
    let r = |v: &f32| (v * 1000.0).round() / 1000.0;
    let b: Vec<f32> = buttons.iter().map(r).collect();
    let a: Vec<f32> = axes.iter().map(r).collect();
    e.eval(&format!("window.__rarePadState && window.__rarePadState({}, {})", json!(b), json!(a)));
}

/// The lowercase name the page's `__rarePad` takes.
fn pad_name(button: Pad) -> String {
    serde_json::to_value(button).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// Routes a controller event to the open web game. Guide sends the player back to the
/// launcher with the game still running (Resume brings it back, Quit game closes it).
pub fn pad(button: Pad, down: bool) -> bool {
    if !visible() {
        return false;
    }
    let (Some(e), Some(env)) = (engine(), env()) else { return false };
    if !e.is_open() {
        return false;
    }
    // While the launcher shows an approval sheet for this game, the controller drives the sheet.
    if state().pending.lock().unwrap().values().any(|r| r.focus) {
        return false;
    }
    if button == Pad::Guide {
        if down {
            e.hide();
            set_visible(false);
            if env.host.main_exists() {
                env.host.main_show();
                env.host.main_focus();
            }
        }
        return true;
    }
    e.eval(&format!("window.__rarePad && window.__rarePad('{}', {down})", pad_name(button)));
    true
}

/// Id of the game or app in the web window, if one is open.
pub fn current_id() -> Option<String> {
    if !engine()?.is_open() {
        return None;
    }
    state().current.lock().unwrap().as_ref().map(|c| c.0.clone())
}

/// The in-game overlay is up: the web game stays mapped under it but stops getting the
/// controller (presses and analog state) until `resume`.
pub fn suspend() {
    set_visible(false);
}

/// Title of the game or app in the web window, if one is open.
pub fn current_title() -> Option<String> {
    if !engine()?.is_open() {
        return None;
    }
    state().current.lock().unwrap().as_ref().map(|c| c.1.clone())
}

/// Closes the web game window (Quit game). Requests still waiting for the player are dropped.
pub fn close() {
    let Some(e) = engine() else { return };
    state().pending.lock().unwrap().clear();
    give_back_front();
    if e.is_open() {
        e.close();
    }
    set_visible(false);
}

/// Hides the window, the game still running (the overlay went Home over it; `resume` shows it).
pub fn hide() {
    if let Some(e) = engine() {
        if e.is_open() {
            e.hide();
        }
    }
    set_visible(false);
}

/// Brings a running web game back to the screen.
pub fn resume() -> bool {
    let Some(e) = engine() else { return false };
    if !e.is_open() {
        return false;
    }
    e.show();
    set_visible(true);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(u: &str) -> bool {
        url_allowed(&u.parse::<url::Url>().unwrap())
    }

    #[test]
    fn https_anywhere_and_http_only_on_this_machine() {
        assert!(ok("https://play.example.com/game"));
        assert!(ok("http://localhost:3000/"));
        assert!(ok("http://localhost"));
        assert!(ok("http://127.0.0.1:8080/x"));
        assert!(ok("http://[::1]:5173/"));
        assert!(!ok("http://example.com"));
        assert!(!ok("http://localhost.example.com"));
        assert!(!ok("http://localhost@example.com"));
        assert!(!ok("http://192.168.1.5:3000"));
        assert!(!ok("http://127.0.0.2"));
        assert!(!ok("http://0.0.0.0"));
        assert!(!ok("ftp://localhost"));
        assert!(!ok("file:///etc/passwd"));
    }

    #[test]
    fn pad_names_match_the_page() {
        assert_eq!(pad_name(Pad::A), "a");
        assert_eq!(pad_name(Pad::GuideHold), "guidehold");
        assert_eq!(pad_name(Pad::Lb), "lb");
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(urlencoding_decode("a%2Bb%3D"), "a+b=");
        assert_eq!(urlencoding_decode("100%"), "100%");
    }

    use crate::engine::{EngineEvents, OpenSpec, WebEngine};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Fake {
        open: Mutex<Option<(OpenSpec, Arc<dyn EngineEvents>)>>,
        log: Mutex<Vec<String>>,
    }

    impl WebEngine for Fake {
        fn open(&self, spec: &OpenSpec, events: Arc<dyn EngineEvents>) -> Result<(), String> {
            *self.open.lock().unwrap() = Some((spec.clone(), events));
            Ok(())
        }
        fn is_open(&self) -> bool {
            self.open.lock().unwrap().is_some()
        }
        fn url(&self) -> Option<String> {
            self.open.lock().unwrap().as_ref().map(|o| o.0.url.clone())
        }
        fn eval(&self, js: &str) {
            self.log.lock().unwrap().push(format!("eval {js}"));
        }
        fn show(&self) {
            self.log.lock().unwrap().push("show".into());
        }
        fn hide(&self) {
            self.log.lock().unwrap().push("hide".into());
        }
        fn focus(&self) {
            self.log.lock().unwrap().push("focus".into());
        }
        fn close(&self) {
            *self.open.lock().unwrap() = None;
        }
    }

    struct Launcher;
    impl WebHost for Launcher {
        fn main_exists(&self) -> bool {
            true
        }
    }

    /// One test drives the whole flow: the engine and the state are process-wide.
    #[test]
    fn open_route_pad_suspend_resume_and_close() {
        let fake = Arc::new(Fake::default());
        install(Ctx::headless(), fake.clone(), Arc::new(Launcher));
        assert!(open("web:x", "X", "http://example.com").is_err(), "plain http is refused off this machine");
        assert!(!visible() && !fake.is_open());

        open("web:x", "X", "https://play.example.com/g").unwrap();
        assert!(visible());
        assert_eq!(current_id().as_deref(), Some("web:x"));
        let (spec, events) = fake.open.lock().unwrap().clone().unwrap();
        assert_eq!(spec.label, LABEL);
        assert!(spec.init_script.starts_with("window.__rareProfile = "));
        assert!(spec.data_dir.unwrap().ends_with("rare/webgames/web-x"));

        // The page's own navigation goes on; a wallet request is taken, not loaded.
        assert!(events.navigation("https://play.example.com/level2"));
        assert!(!events.navigation("rare-wallet://request/e30%3D"));

        // Buttons reach the page; Guide hides the window and gives the screen back.
        assert!(pad(Pad::A, true));
        assert!(fake.log.lock().unwrap().last().unwrap().contains("__rarePad('a', true)"));
        assert!(pad(Pad::Guide, true));
        assert!(!visible());
        assert!(fake.log.lock().unwrap().contains(&"hide".to_string()));
        assert!(!pad(Pad::A, true), "nothing goes to a hidden game");

        // The overlay suspends and resumes it.
        assert!(resume());
        assert!(visible());
        suspend();
        assert!(!visible() && !pad(Pad::B, true));

        // A newer window's close is not the older one's.
        assert!(resume());
        events.closed();
        assert!(!visible());
        assert!(spec.wallet, "a web game has Rare Wallet");

        // A webview app opens in the same window in app mode: no wallet, no Local API token.
        open_with("app:x", "X", "https://x.com/home", Some(json!({ "input": "focus" }))).unwrap();
        let (spec, _) = fake.open.lock().unwrap().clone().unwrap();
        assert!(!spec.wallet);
        assert!(spec.init_script.starts_with("window.__rareProfile = {\"input\":\"focus\",\"wallet\":false};\nwindow.__rareApi = null;"), "{}", &spec.init_script[..80]);
        assert!(spec.init_script.contains("if (P.wallet === false) return;"));
        assert_eq!(current_id().as_deref(), Some("app:x"));
        hide();
        assert!(!visible() && fake.log.lock().unwrap().last().map(String::as_str) == Some("hide"));
        assert!(resume() && visible());
        close();

        // A Wi-Fi sign-in may be plain http anywhere, in app mode; other windows still may not.
        open_portal("http://neverssl.com/").unwrap();
        let (spec, events) = fake.open.lock().unwrap().clone().unwrap();
        assert!(!spec.wallet && portal_open() && external_open());
        assert!(events.navigation("http://login.hotel.example/portal?x=1"));
        close_external();
        assert!(!portal_open());
        assert!(open_with("app:x", "X", "http://neverssl.com/", None).is_err());

        // A cloud game: the app's storage, its user agent, the stream engine, the real pad.
        let profile = json!({ "input": "native", "padHosts": ["www.xbox.com"], "wallet": false, "dataId": "app:xcloud", "userAgent": "UA Edg/1", "stream": true });
        open_with("cloud:xbox:9X", "Halo", "https://www.xbox.com/en-US/play/launch/halo/9X", Some(profile)).unwrap();
        let (spec, _) = fake.open.lock().unwrap().clone().unwrap();
        assert!(spec.data_dir.unwrap().ends_with("rare/webgames/app-xcloud"));
        assert_eq!(spec.user_agent.as_deref(), Some("UA Edg/1"));
        assert!(spec.stream && !spec.wallet);
        assert!(spec.init_script.contains("\"input\":\"native\""));
        close();
    }

    #[test]
    fn apps_get_the_wallet_only_when_asked() {
        assert!(wants_wallet("web:wordgame", &json!({})));
        assert!(!wants_wallet("app:x", &json!({ "input": "focus" })));
        assert!(wants_wallet("app:dapp", &json!({ "wallet": true })));
        assert!(!wants_wallet("web:x", &json!({ "wallet": false })));
    }
}
