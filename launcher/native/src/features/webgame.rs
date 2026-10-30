//! Web games in the native launcher (the web launcher's webgame.rs and WalletHost.tsx): Play opens
//! the game in its own process (`rare-webgame-host`, Chromium through CEF, launcher/native/webhost) through
//! rare-webgame's `Spawner`; gamescope shows its window over the launcher's (same app id, mapped
//! last). While it is in front the pad goes to the page (`pad_input`, on the input thread). The
//! Rare button over it goes through rare-overlay like a native game's (features/ingame.rs,
//! app/ingame_glue.rs): a still of the page, the launcher raised over it, the Rare menu, Switcher,
//! Quick settings and power menu over the dimmed still; Resume and B show it again, Quit game
//! closes it, Rare again goes Home with it hidden. Webview apps (X, Telegram Web) open in the same
//! window in app mode (no Rare Wallet in the page, rare-webgame `wallet`) and work the same way.
//!
//! The game's wallet requests (Wallet Standard in the page) arrive as `wallet-request` events:
//! connect asks with the connect sheet (or answers at once for a site the player allowed before);
//! transactions and messages go into Rare Wallet's signing queue, so the review sheet shows them over
//! the launcher, which gamescope puts in front of the game until the answer goes back.

use crate::page::{hash, Acts, Btn, Fx, Page};
use crate::screens::wallet::render::{build, Built};
use crate::screens::wallet::service;
use crate::screens::wallet::sign::connect;
use crate::state::{Overlay, State};
use rare_core::{Error, Pad};
use rare_render::input::Input;
use rare_render::view::El;
use rare_wallet::sign::{OriginKind, SignOptions, SignOrigin};
use rare_wallet::webgame::{error_text, map_game_request, norm_origin, reply_value, silent_connect_account, GameCall};
use rare_webgame::engine::WebEngine;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};

/// The overlay kind of the connect sheet.
pub const CONNECT: &str = "webconnect";

// ---------------------------------------------------------------- install

/// The launcher's window, for gamescope's base-layer control while a sheet is over the game.
static MAIN_XID: AtomicU64 = AtomicU64::new(0);

/// The launcher's X11 window (main.rs, once it exists).
pub fn set_main_window(w: &rare_render::winit::window::Window) {
    use rare_render::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let xid = match w.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xlib(h)) => h.window as u64,
        Ok(RawWindowHandle::Xcb(h)) => h.window.get() as u64,
        _ => 0,
    };
    MAIN_XID.store(xid, Ordering::Relaxed);
}

/// What rare-webgame needs from the launcher.
struct Launcher;

impl rare_webgame::WebHost for Launcher {
    fn main_exists(&self) -> bool {
        true
    }
    fn testnet(&self) -> bool {
        crate::features::config::get_str("network").as_deref() == Some("testnet")
    }
}

/// What rare-overlay needs (the sheet over a game holds gamescope's base layer with our window).
struct OverlayHost;

impl rare_overlay::OverlayHost for OverlayHost {
    fn log(&self, msg: &str) {
        log::info!("{msg}");
    }
    fn main_exists(&self) -> bool {
        true
    }
    fn main_xid(&self) -> Option<u64> {
        Some(MAIN_XID.load(Ordering::Relaxed)).filter(|x| *x != 0)
    }
    fn web_visible(&self) -> bool {
        rare_webgame::visible()
    }
    fn web_current(&self) -> Option<String> {
        rare_webgame::current_id()
    }
    fn web_suspend(&self) {
        suspend();
    }
    fn web_resume(&self) {
        rare_webgame::resume();
    }
    fn hide_web_window(&self) {
        rare_webgame::hide();
    }
    fn web_pad(&self, button: Pad, down: bool) {
        rare_webgame::pad(button, down);
    }
}

/// The backends' host hooks for web windows (rare-launch opens, tracks, resumes and quits them).
pub struct WebWindows;

impl rare_core::Host for WebWindows {
    // The runner's running set changed: power follows the next game (features/services.rs).
    fn power_running(&self, now: &[String]) {
        super::services::running_changed(now);
    }
    fn window_open(&self, label: &str) -> bool {
        label == rare_webgame::LABEL && rare_webgame::current_id().is_some()
    }
    fn close_window(&self, label: &str) {
        if label == rare_webgame::LABEL {
            rare_webgame::close();
        }
    }
    fn open_web_window(&self, run: &str, title: &str, url: &str, profile: Option<Value>) -> Result<(), Error> {
        install();
        rare_webgame::open_with(run, title, url, profile).map_err(Error::new)
    }
    fn current_web_id(&self) -> Option<String> {
        rare_webgame::current_id()
    }
    fn resume_web(&self) -> bool {
        rare_webgame::resume()
    }
}

/// The one way to open an external link (View in Marketplace, View on Suiscan, Launchpad): the
/// web window in app mode, no wallet in the page. B or the Rare button closes it.
pub fn open_external(url: &str) -> Result<(), String> {
    install();
    rare_webgame::open_external(url)
}

/// A Wi-Fi network's sign-in page (features/portal.rs), in the same window. B or the Rare button
/// closes it.
pub fn open_portal(url: &str) -> Result<(), String> {
    install();
    rare_webgame::open_portal(url)
}

/// lib/webgames.ts normaliseUrl: https anywhere, http only on localhost, 127.0.0.1 and [::1]. A bare
/// local address ("localhost:3000") is read as http, anything else without a scheme as https.
pub fn normalise_url(input: &str) -> Result<String, String> {
    const BAD: &str = "Web games need an https address (http works only for localhost)";
    let u = input.trim();
    let lower = u.to_ascii_lowercase();
    let local = |rest: &str| ["localhost", "127.0.0.1", "[::1]"].iter().any(|h| rest.strip_prefix(h).is_some_and(|t| t.is_empty() || t.starts_with([':', '/', '?', '#'])));
    let full = if let Some(rest) = lower.strip_prefix("https://") {
        format!("https://{}", &u[u.len() - rest.len()..])
    } else if let Some(rest) = lower.strip_prefix("http://") {
        if !local(rest) {
            return Err(BAD.into());
        }
        format!("http://{}", &u[u.len() - rest.len()..])
    } else if lower.contains("://") {
        return Err(BAD.into());
    } else if local(&lower) {
        format!("http://{u}")
    } else {
        format!("https://{u}")
    };
    let host = full.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("");
    let name = host.rsplit_once(':').filter(|(h, p)| !h.ends_with(']') || p.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(host);
    if name.is_empty() || name.contains(' ') || !(name.contains('.') || local(name)) {
        return Err("That isn't a web address. Check it and try again.".into());
    }
    Ok(if full.split("://").nth(1).is_some_and(|r| r.contains('/')) { full } else { format!("{full}/") })
}

/// lib/webgames.ts slug: lowercase words joined by '-', at most 40 characters.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(40).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "game".into()
    } else {
        out
    }
}

/// lib/webgames.ts addWebGame: puts the game in launcher.json's webGames (replacing one with the
/// same id) and brings back a built-in one the player removed. The library shows it after a scan.
pub fn add(id: Option<&str>, title: &str, url: &str, cover: Option<&str>, hero: Option<&str>) -> Result<String, String> {
    let url = normalise_url(url)?;
    let id = id.map(String::from).unwrap_or_else(|| slug(title));
    let game = serde_json::json!({ "id": id, "title": title.trim(), "url": url, "cover": cover.filter(|c| !c.is_empty()), "hero": hero.filter(|h| !h.is_empty()) });
    let mut web = super::config::get("webGames").and_then(|v| v.as_array().cloned()).unwrap_or_default();
    web.retain(|w| w.get("id").and_then(|v| v.as_str()) != Some(id.as_str()));
    web.push(game);
    let mut removed: Vec<String> = super::config::get("removedBuiltins").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    removed.retain(|r| *r != id);
    super::config::set(&[("webGames", serde_json::Value::Array(web)), ("removedBuiltins", serde_json::json!(removed))]);
    Ok(format!("web:{id}"))
}

#[cfg(test)]
mod add_tests {
    use super::*;

    #[test]
    fn urls_like_the_web_launcher() {
        assert_eq!(normalise_url("game.example").unwrap(), "https://game.example/");
        assert_eq!(normalise_url(" https://Game.example/play?x=1 ").unwrap(), "https://Game.example/play?x=1");
        assert_eq!(normalise_url("localhost:3000").unwrap(), "http://localhost:3000/");
        assert_eq!(normalise_url("http://127.0.0.1:8080/x").unwrap(), "http://127.0.0.1:8080/x");
        assert!(normalise_url("http://game.example").is_err());
        assert!(normalise_url("ftp://game.example").is_err());
        assert!(normalise_url("not a url").is_err());
        assert!(normalise_url("hello").is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Word Game!"), "word-game");
        assert_eq!(slug("***"), "game");
    }
}

/// The host program: `RARE_WEBGAME_HOST`, else next to this binary, else on PATH.
fn host_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("RARE_WEBGAME_HOST") {
        return PathBuf::from(p);
    }
    let here = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("rare-webgame-host")));
    here.filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("rare-webgame-host"))
}

/// The host's environment beyond the launcher's own: none. It runs Chromium (CEF) as an X11 client
/// of gamescope's Xwayland and finds its runtime next to itself (`cef/`) or in /usr/lib/rare/cef.
fn host_env() -> Vec<(String, String)> {
    Vec::new()
}

/// Installs rare-webgame (and the overlay hooks it uses) once.
pub fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let ctx = crate::features::bus::ctx();
        if !rare_overlay::installed() {
            rare_overlay::install(ctx.clone(), Arc::new(OverlayHost));
        }
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let bin = host_bin();
        log::info!("web games: host {}", bin.display());
        let engine: Arc<dyn WebEngine> = Arc::new(rare_webgame::process::Spawner::new(bin, host_env(), dir));
        rare_webgame::install(ctx, engine, Arc::new(Launcher));
        rare_webgame::wallet_set_approved(approved());
    });
}

// ---------------------------------------------------------------- the pad

/// The pad as the page's virtual gamepad sees it (standard mapping): 17 buttons (the triggers
/// analog), the left and right sticks with down positive.
static PAD: Mutex<([f32; 17], [f32; 4])> = Mutex::new(([0.0; 17], [0.0; 4]));
/// The state moved since the page last got it.
static DIRTY: AtomicBool = AtomicBool::new(false);
/// The longest the page waits for analog changes: sticks report hundreds of events a second, and
/// each one sent would be a script run in the page.
const PAD_FRAME: std::time::Duration = std::time::Duration::from_millis(8);

fn std_index(p: Pad) -> Option<usize> {
    Some(match p {
        Pad::A => 0,
        Pad::B => 1,
        Pad::X => 2,
        Pad::Y => 3,
        Pad::Lb => 4,
        Pad::Rb => 5,
        Pad::Lt => 6,
        Pad::Rt => 7,
        Pad::Select => 8,
        Pad::Start => 9,
        Pad::Up => 12,
        Pad::Down => 13,
        Pad::Left => 14,
        Pad::Right => 15,
        Pad::Guide => 16,
        _ => return None,
    })
}

fn send_state(f: impl FnOnce(&mut ([f32; 17], [f32; 4]))) {
    let (b, a) = {
        let mut s = PAD.lock().unwrap();
        f(&mut s);
        *s
    };
    DIRTY.store(false, Ordering::Relaxed);
    rare_webgame::pad_state(&b, &a);
}

/// An analog change: kept, and sent with the next pad frame (`pad_frames`).
fn analog(f: impl FnOnce(&mut ([f32; 17], [f32; 4]))) {
    f(&mut PAD.lock().unwrap());
    DIRTY.store(true, Ordering::Relaxed);
}

/// Sends the coalesced analog state every PAD_FRAME while it moves.
fn pad_frames() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::thread::Builder::new()
            .name("webgame-pad".into())
            .spawn(|| loop {
                std::thread::sleep(PAD_FRAME);
                if DIRTY.swap(false, Ordering::Relaxed) && rare_webgame::visible() {
                    let (b, a) = *PAD.lock().unwrap();
                    rare_webgame::pad_state(&b, &a);
                }
            })
            .ok();
    });
}

/// The overlay is coming up over the web game: it stops getting the pad, and the page sees every
/// button let go (the releases while Rare is up are the launcher's).
pub fn suspend() {
    *PAD.lock().unwrap() = ([0.0; 17], [0.0; 4]);
    rare_webgame::pad_state(&[0.0; 17], &[0.0; 4]);
    rare_webgame::suspend();
}

/// The input thread's hand-off: while a web game has the screen its presses go to the page and
/// None comes back; anything for the launcher comes back (the Rare button, presses while a sheet is
/// over the game, and everything while no game is shown).
pub fn pad_input(i: Input) -> Option<Input> {
    if !rare_webgame::visible() {
        return Some(i);
    }
    match i {
        // An external link (View on Suiscan): B and the Rare button close it and the launcher is back.
        Input::Press(e) if rare_webgame::external_open() && matches!(e.pad, Pad::B | Pad::Guide | Pad::GuideHold) => {
            rare_webgame::close_external();
            None
        }
        Input::Exit if rare_webgame::external_open() => {
            rare_webgame::close_external();
            None
        }
        Input::Release(Pad::B | Pad::Guide | Pad::GuideHold) if rare_webgame::external_open() => None,
        // The Rare button (pressed or held) and View + Menu: rare-overlay's, as over a native game
        // (features/ingame.rs). Without the overlay (a bench build) the window hides and the
        // launcher has the screen; the game keeps running behind it.
        Input::Press(e) if matches!(e.pad, Pad::Guide | Pad::GuideHold) => {
            if !crate::features::ingame::on() {
                rare_webgame::pad(Pad::Guide, true);
            }
            Some(i)
        }
        Input::Exit => {
            if !crate::features::ingame::on() {
                rare_webgame::pad(Pad::Guide, true);
            }
            Some(i)
        }
        Input::Press(e) => {
            // A sheet for this game is up: the launcher drives it.
            if !rare_webgame::pad(e.pad, true) {
                return Some(i);
            }
            if let Some(i) = std_index(e.pad) {
                send_state(|s| s.0[i] = 1.0);
            }
            None
        }
        Input::Release(p) => {
            rare_webgame::pad(p, false);
            if let Some(i) = std_index(p) {
                send_state(|s| s.0[i] = 0.0);
            }
            None
        }
        // Sticks and triggers as read (the launcher's Stick is the right stick's pointer view).
        Input::Analog(axis, v) => {
            use rare_input::pad::Axis;
            pad_frames();
            analog(|s| match axis {
                Axis::LeftStickX => s.1[0] = v,
                Axis::LeftStickY => s.1[1] = -v,
                Axis::RightStickX => s.1[2] = v,
                Axis::RightStickY => s.1[3] = -v,
                Axis::LeftZ => s.0[6] = v,
                Axis::RightZ => s.0[7] = v,
            });
            None
        }
        Input::Stick(..) => None,
        other => Some(other),
    }
}

// ---------------------------------------------------------------- wallet requests

/// A request from the game (rare-webgame's `wallet-request` payload).
#[derive(Clone, Debug)]
struct Req {
    id: u64,
    method: String,
    params: Value,
    origin: String,
    game: String,
    game_id: String,
}

impl Req {
    fn parse(v: &Value) -> Option<Req> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Some(Req { id: v.get("id")?.as_u64()?, method: s("method"), params: v.get("params").cloned().unwrap_or(Value::Null), origin: s("origin"), game: s("game"), game_id: s("gameId") })
    }
}

/// Connect requests waiting for the sheet, oldest first.
static CONNECTS: Mutex<Vec<Req>> = Mutex::new(Vec::new());

const APPROVED_KEY: &str = "webgameApprovedOrigins";

/// Origins the player allowed (launcher.json, so they survive a restart).
fn approved() -> Vec<String> {
    crate::features::config::get(APPROVED_KEY).and_then(|v| serde_json::from_value::<Vec<String>>(v).ok()).unwrap_or_default()
}

fn approve(origin: &str) {
    let mut list = approved();
    if !list.iter().any(|o| o == origin) {
        list.push(origin.to_string());
        crate::features::config::set(&[(APPROVED_KEY, json!(list))]);
    }
    rare_webgame::wallet_set_approved(list);
}

fn reply(id: u64, ok: bool, value: Value) {
    rare_webgame::wallet_reply(id, ok, value);
}

fn fail(id: u64, msg: &str) {
    reply(id, false, json!(msg));
}

/// The connect answer: the account the game sees.
fn connect_reply(w: &service::Wallet, address: &str, label: &str) -> Value {
    let pk = w.keystore.public_key(address).ok().flatten();
    json!({ "accounts": [{ "address": address, "label": label, "publicKey": pk }] })
}

/// `https://www.game.example:8080/x?y` -> `game.example:8080` (scheme and `www.` dropped, lowercase).
fn site_of(u: &str) -> Option<String> {
    let rest = u.trim().split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    let host = host.rsplit_once('@').map(|(_, h)| h.to_string()).unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    (!host.is_empty()).then_some(host)
}

/// The game's own budget pays when it covers the fee (payingGame): the catalog slug of a Sui game,
/// by the run id (`web:<slug>`), else by the site it plays on (a web game the player added by
/// address, or one whose library id is not its slug), else by the profile's hosts.
fn paying_game(st: &State, r: &Req) -> (Option<String>, String) {
    let fallback = if r.game.is_empty() { "This game".to_string() } else { r.game.clone() };
    let found = paying_slug(&st.sui_games, &r.game_id, &r.origin);
    match found {
        Some(slug) => {
            let name = st.sui_games.iter().find(|g| g.slug == slug).map(|g| g.name.clone()).unwrap_or(fallback);
            (Some(slug), name)
        }
        None => (None, fallback),
    }
}

fn paying_slug(games: &[crate::data::catalog::SuiGame], game_id: &str, origin: &str) -> Option<String> {
    let id = game_id.strip_prefix("web:").unwrap_or(game_id);
    if let Some(g) = games.iter().find(|g| g.slug == id) {
        return Some(g.slug.clone());
    }
    let site = site_of(origin)?;
    let plays_here = |g: &&crate::data::catalog::SuiGame| {
        [&g.play_url, &g.website].iter().any(|u| site_of(u).as_deref() == Some(site.as_str()) && (u.as_str() == g.play_url || g.platforms.iter().any(|p| p == "web")))
    };
    if let Some(g) = games.iter().find(plays_here) {
        return Some(g.slug.clone());
    }
    // The bundled profiles list each game's hosts (rare-library profiles.json).
    rare_library::profiles::for_game(game_id, origin).map(|(slug, _)| slug).filter(|s| games.iter().any(|g| &g.slug == s))
}

/// `wallet-request` and `wallet-connected` (features::on_event).
pub fn on_event(st: &mut State, name: &str, payload: &Value, now: f64) {
    match name {
        "wallet-connected" => {
            let game = payload.get("game").and_then(Value::as_str).unwrap_or("");
            if !game.is_empty() {
                st.toast(format!("{game} connected to Rare Wallet"), crate::state::Tone::Info, now);
            }
        }
        "wallet-request" => {
            if let Some(r) = Req::parse(payload) {
                request(st, r);
            }
        }
        _ => {}
    }
}

fn request(st: &mut State, r: Req) {
    let origin = norm_origin(&r.origin);
    let call = match map_game_request(&r.method, &r.params) {
        Ok(c) => c,
        Err(e) => return fail(r.id, &e),
    };
    let Some(w) = service::wallet() else { return fail(r.id, "Rare Wallet is not available") };
    let (accounts, selected) = w.accounts();
    if accounts.is_empty() {
        return fail(r.id, "Add an account to Rare Wallet first");
    }
    let current = selected.as_deref().and_then(|s| accounts.iter().find(|a| a.address == s));
    if call == GameCall::Connect {
        // A site the player allowed before connects without a sheet: the game keeps the screen.
        if let Some(a) = silent_connect_account(&approved(), &origin, &accounts, current) {
            let (id, address, label, select) = (r.id, a.address.clone(), a.label.clone(), current.map(|c| c.address != a.address).unwrap_or(true));
            std::thread::spawn(move || {
                if select {
                    let _ = w.keystore.select(&address);
                    w.bump();
                }
                reply(id, true, connect_reply(w, &address, &label));
            });
            return;
        }
        rare_webgame::wallet_focus(r.id);
        CONNECTS.lock().unwrap().push(r);
        st.open(Some(Overlay::Other(CONNECT)));
        return;
    }
    if !approved().contains(&origin) {
        return fail(r.id, "Connect this game to Rare Wallet first");
    }
    let Some(acct) = current.map(|a| a.address.clone()) else { return fail(r.id, "Add an account to Rare Wallet first") };
    // The sender the game wrote (Transaction JSON, or decoded from built bytes) must be this account.
    if let GameCall::Transaction { tx, .. } = &call {
        if let Err(e) = rare_wallet::webgame::check_sender(tx, &acct) {
            return fail(r.id, &e);
        }
    }
    let (slug, name) = paying_game(st, &r);
    let from = SignOrigin { name: if r.game.is_empty() { name.clone() } else { r.game.clone() }, url: Some(r.origin.clone()), icon: None, kind: OriginKind::Game };
    let id = r.id;
    // Blocks until the review sheet answers: never on the UI thread.
    std::thread::Builder::new()
        .name("rare-webgame-sign".into())
        .spawn(move || {
            let o = SignOptions { account: Some(acct), origin: Some(from), ..Default::default() };
            let s = &w.signing;
            let ticket = match &call {
                GameCall::Transaction { tx, execute: true } => s.sign_and_execute_for_game(tx.clone(), slug.as_deref(), &name, o, None),
                // The game submits it itself, so only the player can pay its fee.
                GameCall::Transaction { tx, execute: false } => s.sign_transaction(tx.clone(), o),
                GameCall::Message(m) => s.sign_personal_message(m.clone(), o),
                GameCall::Connect => return,
            };
            w.bump();
            match ticket.and_then(|t| t.wait()) {
                Ok(res) => reply(id, true, reply_value(&call, &res)),
                Err(e) => fail(id, &error_text(&e)),
            }
        })
        .ok();
}

// ---------------------------------------------------------------- the connect sheet

/// "<game> wants to see your address" over the launcher, for the oldest connect request.
#[derive(Default)]
pub struct ConnectPage {
    shown: Option<(u64, connect::State)>,
    accounts: Vec<crate::screens::wallet::types::RareAccount>,
    built: Built<connect::Msg>,
    ver: u64,
}

impl ConnectPage {
    fn head() -> Option<Req> {
        CONNECTS.lock().unwrap().first().cloned()
    }

    fn sync(&mut self) -> Option<Req> {
        let head = Self::head()?;
        if self.shown.as_ref().map(|s| s.0) != Some(head.id) {
            let (accts, selected) = service::wallet().map(|w| w.accounts()).unwrap_or_default();
            self.accounts = accts.iter().map(crate::screens::wallet::types::RareAccount::from_wallet).collect();
            let pr = connect::Props { game: &head.game, origin: &head.origin, accounts: &self.accounts, initial: selected.as_deref() };
            self.shown = Some((head.id, connect::State::new(&pr)));
            self.ver += 1;
        }
        Some(head)
    }

    /// The player answered (Some(address) to allow).
    fn answer(&mut self, st: &mut State, allow: Option<String>) {
        let Some(r) = Self::head() else { return };
        CONNECTS.lock().unwrap().retain(|x| x.id != r.id);
        self.shown = None;
        self.ver += 1;
        match (allow, service::wallet()) {
            (Some(address), Some(w)) => {
                let Some(label) = self.accounts.iter().find(|a| a.address == address).map(|a| a.label.clone()) else { return fail(r.id, "Account not found") };
                approve(&norm_origin(&r.origin));
                std::thread::spawn(move || {
                    let _ = w.keystore.select(&address);
                    w.bump();
                    reply(r.id, true, connect_reply(w, &address, &label));
                });
            }
            _ => fail(r.id, "User rejected the request"),
        }
        if Self::head().is_none() {
            st.open(None);
        }
    }

    fn handle(&mut self, st: &mut State, m: connect::Msg) {
        let Some((_, cs)) = self.shown.as_mut() else { return };
        match connect::update(cs, m) {
            Some(Ok(a)) => self.answer(st, Some(a)),
            Some(Err(())) => self.answer(st, None),
            None => self.ver += 1,
        }
    }
}

impl Page for ConnectPage {
    fn stamp(&self, _st: &State) -> u64 {
        hash((self.ver, Self::head().map(|r| r.id)))
    }

    fn view(&mut self, ui: &mut crate::kit::Ui, _st: &State, acts: &mut Acts) -> El {
        let Some(head) = self.sync() else {
            self.built = Built::default();
            return ui.bx(crate::kit::s());
        };
        let cs = &self.shown.as_ref().unwrap().1;
        let pr = connect::Props { game: &head.game, origin: &head.origin, accounts: &self.accounts, initial: None };
        let (el, built) = build(ui, acts, &connect::view(&pr, cs));
        self.built = built;
        el
    }

    fn pad(&mut self, st: &mut State, _fx: &mut Fx, b: Btn, _cur: Option<(&rare_render::focus::Focusable, Option<&crate::act::Act>)>) -> bool {
        if self.shown.is_none() {
            return false;
        }
        if b == Btn::B {
            self.handle(st, connect::Msg::Deny);
            return true;
        }
        matches!(b, Btn::Lb | Btn::Rb | Btn::Y | Btn::Start | Btn::Select)
    }

    fn act(&mut self, st: &mut State, _fx: &mut Fx, code: u32) {
        if let Some(m) = self.built.msg(code) {
            self.handle(st, m);
        }
    }

    fn tick(&mut self, st: &mut State, _fx: &mut Fx, _now: f64) -> Option<f64> {
        // The game went away with its request (closed, another game opened).
        if rare_webgame::current_id().is_none() {
            CONNECTS.lock().unwrap().clear();
        }
        if Self::head().is_none() && st.overlay == Some(Overlay::Other(CONNECT)) {
            st.open(None);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::catalog::SuiGame;

    fn game(slug: &str, play: &str, site: &str, web: bool) -> SuiGame {
        SuiGame { slug: slug.into(), name: slug.to_uppercase(), play_url: play.into(), website: site.into(), platforms: if web { vec!["web".into()] } else { vec![] }, ..Default::default() }
    }

    #[test]
    fn budgets_follow_the_slug_then_the_site() {
        let games = vec![game("aresrpg", "https://aresrpg.world/", "", true), game("other", "", "https://other.example", false), game("wordgame", "", "https://www.wordgame.example.com", true)];
        assert_eq!(paying_slug(&games, "web:aresrpg", "https://x.example").as_deref(), Some("aresrpg"));
        // Added by address: the library id is not the slug, the origin is the play URL's.
        assert_eq!(paying_slug(&games, "web:my-ares", "https://aresrpg.world").as_deref(), Some("aresrpg"));
        assert_eq!(paying_slug(&games, "web:w1", "https://wordgame.example.com").as_deref(), Some("wordgame"));
        // A site that is not where the game plays (not a web game) pays nothing.
        assert_eq!(paying_slug(&games, "web:x", "https://other.example"), None);
        assert_eq!(paying_slug(&games, "web:x", "https://unknown.example"), None);
        assert_eq!(site_of("https://www.Game.example:8080/p?q#h").as_deref(), Some("game.example:8080"));
        assert_eq!(site_of("not a url"), None);
    }
}
