//! The Local API's game sheets (launcher/src/localapi): what a game asks of Rare through
//! `rare-localapi` (loopback HTTP for native games, deep links for `rare://`). Reads (`me`,
//! `inventory`) answer from a worker; `open` moves the launcher; `sell` and `buy` come up as a
//! sheet over the game (`Overlay::Other("apisheet")`) and answer when the player is done. Anything
//! signed goes through Rare Wallet's review sheet (`crate::sign`). B answers "cancelled".
//!
//! Claims (vouchers) decode in `claim.rs`; the sheet signs the one transaction through Rare Wallet,
//! the game's gas budget paying first. When a game's sheet closes the game gets the screen back.

use crate::act::Act;
use crate::page::{Acts, Btn, Fx, Page};
use crate::screens::inventory::game_sheets::{asked_by, Asker};
use crate::screens::wallet::kit::{sheet, SheetSpec};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::render::{build, Built};
use crate::screens::wallet::ui::{button_el, dot, BSize, BVariant, ButtonSpec, Tone as UiTone};
use crate::state::{Overlay, State, Tab, Tone};
use rare_localapi::LocalApiHost;
use rare_market::api::{self, ItemCard, Listing, Offer, Page as MPage};
use rare_market::inventory::notice::SIGN_REJECTED;
use rare_market::inventory::trade::{Quote, RealTrade, TradeApi};
use rare_market::net::Net;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------- state

#[derive(Default)]
pub struct Shared {
    pub sheet: Option<Sheet>,
    /// Bumps when the sheet changes.
    pub ver: u64,
    /// Deep links that arrived before the app was ready to act on them.
    queued: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    /// The run id of the game that asked (it gets the screen back when the sheet closes).
    pub run: Option<String>,
    /// The Local API request to answer, or None for a link the player opened.
    pub req: Option<u64>,
    pub asker: Asker,
    pub game_title: String,
    pub kind: Kind,
    pub phase: Phase,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Sell { item: ItemCard, list: bool, price: Option<u64>, floor: Option<u64>, quote: Option<Quote> },
    Buy { item: ItemCard, listing: Listing },
    Claim { voucher: super::claim::Voucher, chain: Option<super::claim::Chain>, slug: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Working,
    Error(String),
    Done(Value),
}

/// The request whose sheet is up, so a page dropped by the shell still answers it.
static OPEN_REQ: Mutex<Option<u64>> = Mutex::new(None);
static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Library records of the running games (a notice finds its game by title when the id says nothing).
static RUNNING_LIB: Mutex<Vec<crate::data::model::Game>> = Mutex::new(Vec::new());

fn reply_ok(id: u64, v: Value) {
    rare_localapi::localapi_reply(id, true, v);
}

fn reply_err(id: u64, code: &str, message: impl Into<String>) {
    rare_localapi::localapi_reply(id, false, json!({ "code": code, "message": message.into() }));
}

/// The runner's list for the API's token sweep (the shell tells it when it changes).
pub fn note_running(r: &[String], lib: Option<&crate::data::model::Library>) {
    if let Ok(mut g) = RUNNING.lock() {
        *g = r.to_vec();
    }
    if let Ok(mut g) = RUNNING_LIB.lock() {
        *g = lib.map(|l| l.games.iter().filter(|x| r.contains(&x.id)).cloned().collect()).unwrap_or_default();
    }
}

// ---------------------------------------------------------------- notices

/// What a game receives (the shape @rare/sdk's parseNotice reads; notices.ts `gameNotice`).
fn game_notice(n: &rare_notify::types::Notice) -> Value {
    json!({
        "id": n.id,
        "kind": n.kind.as_str(),
        "title": n.title,
        "body": n.body,
        "itemId": n.item.as_ref().map(|i| i.id.clone()),
        "item": n.item.as_ref().map(|i| json!({ "id": i.id, "name": i.name, "imageUrl": i.image_url, "collection": i.collection })),
        "amountMist": n.amount.as_ref().filter(|a| a.coin_type.ends_with("::sui::SUI")).map(|a| a.raw.clone()),
        "digest": n.digest,
        "at": n.created_at,
    })
}

/// A live notice from rare.market's stream: offers, sales and purchases of one of a running game's
/// items go to that game's `/v1/notices` streams. The game's collections are Move types, so the
/// notice's collection slug is matched against their slugs from rare.market (kept after the first
/// ask). Runs on its own thread; the stream's thread never waits for the network.
pub fn forward_notice(n: &rare_notify::types::Notice) {
    if !matches!(n.kind.as_str(), "offer_received" | "sold" | "bought") {
        return;
    }
    let Some(slug) = n.item.as_ref().and_then(|i| i.collection.as_ref()).map(|c| c.slug.clone()) else { return };
    let running = note_running_ids();
    if running.is_empty() {
        return;
    }
    let notice = game_notice(n);
    std::thread::Builder::new()
        .name("rr-notice".into())
        .spawn(move || {
            let games = catalog();
            let lib = RUNNING_LIB.lock().map(|g| g.clone()).unwrap_or_default();
            for id in running {
                let info = resolve_game(Some(&id), lib.iter().find(|g| g.id == id), &games);
                if info.collections.iter().any(|(ty, _)| collection_slug(ty).as_deref() == Some(slug.as_str())) {
                    rare_localapi::localapi_notice(id, notice.clone());
                }
            }
        })
        .ok();
}

fn note_running_ids() -> Vec<String> {
    RUNNING.lock().map(|g| g.clone()).unwrap_or_default()
}

/// A collection's rare.market slug by Move type, asked once.
fn collection_slug(ty: &str) -> Option<String> {
    static CACHE: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());
    if let Some((_, s)) = CACHE.lock().ok()?.iter().find(|(t, _)| t == ty) {
        return s.clone();
    }
    let got = api::get::<Value>(&*net(), &format!("/v1/collections/{}", api::enc(ty))).ok().map(|d| d["slug"].as_str().map(str::to_string));
    // A failed ask is not remembered.
    let s = got.clone().flatten();
    if got.is_some() {
        CACHE.lock().ok()?.push((ty.to_string(), s.clone()));
    }
    s
}

struct NativeHost;

impl LocalApiHost for NativeHost {
    fn running(&self) -> Vec<String> {
        let mut r = RUNNING.lock().map(|g| g.clone()).unwrap_or_default();
        r.extend(dev_game());
        r
    }
    fn main_exists(&self) -> bool {
        true
    }
    fn show_launcher(&self) {
        rare_launch::apps::front::to_launcher();
    }
    fn net_input_ready(&self, on: bool) {
        netpad::ready(on);
    }
    fn net_input(&self, events: &Value) -> Result<Value, String> {
        netpad::send(events)
    }
    fn net_screenshot(&self) -> Result<(String, Vec<u8>), String> {
        let host = crate::side::share_host();
        let cap = rare_share::take_screenshot(&*host)?;
        // The image goes to the caller; the copy in Captures is not kept (an agent may ask often).
        let bytes = rare_share::read(&cap.id);
        let _ = rare_share::delete(&cap.id);
        Ok((cap.mime, bytes?))
    }
}

/// The virtual pad for "Send controller input", and the launcher's own input as a fallback when
/// the device has no /dev/uinput.
pub mod netpad {
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    type Injector = Arc<dyn Fn(rare_core::Pad) + Send + Sync>;
    static INJECT: OnceLock<Injector> = OnceLock::new();
    static NOTE: Mutex<Option<String>> = Mutex::new(None);

    /// main.rs gives the launcher's press queue, for when there is no virtual pad.
    pub fn set_injector(f: Injector) {
        let _ = INJECT.set(f);
    }

    pub fn ready(on: bool) {
        match rare_input::remote::ready(on) {
            Ok(()) => *NOTE.lock().unwrap() = None,
            Err(e) => {
                eprintln!("[netapi] {e}; remote input goes to the launcher only");
                *NOTE.lock().unwrap() = Some(e);
            }
        }
    }

    fn pad_of(name: &str) -> Option<rare_core::Pad> {
        use rare_core::Pad;
        Some(match name {
            "a" => Pad::A,
            "b" => Pad::B,
            "x" => Pad::X,
            "y" => Pad::Y,
            "lb" => Pad::Lb,
            "rb" => Pad::Rb,
            "lt" => Pad::Lt,
            "rt" => Pad::Rt,
            "up" => Pad::Up,
            "down" => Pad::Down,
            "left" => Pad::Left,
            "right" => Pad::Right,
            "start" => Pad::Start,
            "select" => Pad::Select,
            "guide" => Pad::Guide,
            _ => return None,
        })
    }

    /// Applies the checked events (rare_localapi::validate) in order; taps wait their length.
    pub fn send(events: &Value) -> Result<Value, String> {
        let virt = rare_input::remote::available();
        if !virt && INJECT.get().is_none() {
            return Err("Controller input isn't available on this device".into());
        }
        let mut sent = 0;
        for e in events["events"].as_array().cloned().unwrap_or_default() {
            match e["kind"].as_str().unwrap_or("") {
                "button" => {
                    let name = e["button"].as_str().unwrap_or("");
                    let ms = e["ms"].as_u64().unwrap_or(80);
                    match e["action"].as_str().unwrap_or("tap") {
                        "down" => press(virt, name, true)?,
                        "up" => press(virt, name, false)?,
                        _ => {
                            press(virt, name, true)?;
                            std::thread::sleep(Duration::from_millis(ms));
                            press(virt, name, false)?;
                        }
                    }
                }
                "axis" if virt => rare_input::remote::axis(e["axis"].as_str().unwrap_or(""), e["value"].as_f64().unwrap_or(0.0) as f32)?,
                // The launcher alone has no analog sticks to move.
                "axis" => {}
                _ => {}
            }
            sent += 1;
        }
        Ok(json!({ "ok": true, "events": sent, "via": if virt { "virtual-pad" } else { "launcher" } }))
    }

    fn press(virt: bool, name: &str, down: bool) -> Result<(), String> {
        if virt {
            return rare_input::remote::button(name, down);
        }
        // Launcher only: a press is a press (releases are implied).
        if down {
            if let (Some(inj), Some(p)) = (INJECT.get(), pad_of(name)) {
                inj(p);
            }
        }
        Ok(())
    }
}

/// Development: `RARE_LOCALAPI_DEV=<run id>` lets a test client act as that game. `boot` writes its
/// RARE_API, RARE_TOKEN and RARE_GAME to `RARE_LOCALAPI_DEV_OUT` (default /tmp/rare-localapi-dev.env).
fn dev_game() -> Option<String> {
    std::env::var("RARE_LOCALAPI_DEV").ok().filter(|s| !s.is_empty())
}

/// Once at start: installs the API, starts the loopback server and takes the links that came first.
pub fn boot(st: &mut State) {
    rare_localapi::install(super::bus::ctx(), Arc::new(NativeHost));
    rare_localapi::start();
    crate::screens::settings::live::start_network_apps();
    let links = rare_localapi::localapi_ready();
    for l in links {
        if let Ok(v) = serde_json::to_value(l) {
            st.feat.localapi.queued.push(v);
        }
    }
    if let Some(id) = dev_game() {
        let out = std::env::var("RARE_LOCALAPI_DEV_OUT").unwrap_or_else(|_| "/tmp/rare-localapi-dev.env".into());
        let env: String = rare_localapi::grant_native(&id).into_iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let _ = std::fs::write(out, env);
    }
    for a in rare_localapi::links_in(std::env::args().skip(1)) {
        rare_localapi::open_link(&a);
    }
}

// ---------------------------------------------------------------- game and items

/// Who asked: the run id and its library record (resolved to a catalog game on a worker).
#[derive(Clone)]
struct Who {
    id: Option<String>,
    lib: Option<crate::data::model::Game>,
}

/// The Sui games catalog with collections: rare.market live, the bundled copy otherwise.
fn catalog() -> Vec<crate::screens::sui::catalog::SuiGame> {
    use crate::screens::sui::catalog as c;
    let live = api::get::<Value>(&*net(), "/v1/games").ok().map(|v| c::clean(v.get("data").unwrap_or(&v))).filter(|g| !g.is_empty());
    live.unwrap_or_else(c::seed)
}

/// The id the Local API gives a network app (rare-localapi `device:network`).
const NET_ID: &str = "device:network";
/// Ask Rare asking (features/agent).
const AGENT_ID: &str = "device:agent";

fn game_of(w: &Who) -> GameInfo {
    if w.id.as_deref() == Some(NET_ID) {
        return GameInfo { any: true, title: "A network app".into(), ..Default::default() };
    }
    if w.id.as_deref() == Some(AGENT_ID) {
        return GameInfo { any: true, title: "Ask Rare".into(), ..Default::default() };
    }
    resolve_game(w.id.as_deref(), w.lib.as_ref(), &catalog())
}

#[derive(Clone, Debug, Default)]
struct GameInfo {
    /// A network app: every game's items, no game's name.
    any: bool,
    slug: Option<String>,
    title: String,
    collections: Vec<(String, String)>,
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Which catalog game a library id belongs to (overlay/game.ts `catalogFor`).
fn resolve_game(game_id: Option<&str>, lib: Option<&crate::data::model::Game>, games: &[crate::screens::sui::catalog::SuiGame]) -> GameInfo {
    let by_slug = |s: &str| games.iter().find(|g| g.slug == s);
    let mut hit = None;
    if let Some(id) = game_id {
        if let Some(s) = id.strip_prefix("web:").or_else(|| id.strip_prefix("rare:")) {
            hit = by_slug(s);
        }
        if hit.is_none() {
            let steam = id.strip_prefix("steam:").and_then(|n| n.parse::<u64>().ok());
            if let Some(n) = steam {
                hit = games.iter().find(|g| crate::screens::sui::catalog::profile(&g.slug).and_then(|p| p.native).is_some_and(|nv| nv.steam_app_ids.contains(&n)));
            }
        }
        if hit.is_none() {
            if let Some(t) = lib.map(|l| norm(&l.title)) {
                hit = games.iter().find(|g| norm(&g.name) == t || crate::screens::sui::catalog::profile(&g.slug).and_then(|p| p.native).is_some_and(|nv| nv.titles.iter().any(|x| norm(x) == t)));
            }
        }
    }
    GameInfo {
        any: false,
        slug: hit.map(|g| g.slug.clone()),
        title: hit.map(|g| g.name.clone()).or_else(|| lib.map(|l| l.title.clone())).unwrap_or_else(|| "Your game".into()),
        collections: hit.map(|g| g.collections.iter().map(|c| (canon_type(&c.nft_type), c.name.clone())).collect()).unwrap_or_default(),
    }
}

/// `0x2::a::B<0x00ab::c::D>` with the leading zeros of addresses dropped and no spaces.
fn canon_type(t: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |w: &mut String, out: &mut String| {
        if let Some(h) = w.strip_prefix("0x") {
            let h = h.trim_start_matches('0');
            out.push_str("0x");
            out.push_str(if h.is_empty() { "0" } else { h });
        } else {
            out.push_str(w);
        }
        w.clear();
    };
    for c in t.chars().filter(|c| !c.is_whitespace()) {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c.to_ascii_lowercase());
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn type_matches(ty: &str, nft: &str) -> bool {
    let (a, b) = (canon_type(ty), canon_type(nft));
    a == b || a.starts_with(&format!("{b}<")) || b.starts_with(&format!("{a}<"))
}

fn owns(g: &GameInfo, ty: &str) -> bool {
    g.any || g.collections.iter().any(|(c, _)| type_matches(ty, c))
}

/// `normalizeSuiAddress`: 0x and 64 hex digits.
fn norm_addr(a: &str) -> String {
    let h = a.trim().trim_start_matches("0x").to_lowercase();
    format!("0x{h:0>64}")
}

fn net() -> Arc<dyn Net> {
    crate::screens::market::feed::net()
}

fn mist(v: &Option<String>) -> Option<u64> {
    v.as_deref().and_then(|s| s.parse().ok())
}

/// The player's items in the game's collections, each with its collection's floor (overlay/data.ts `loadItems`).
fn load_items(n: &dyn Net, address: &str, g: &GameInfo) -> Vec<(ItemCard, Option<String>)> {
    let mut out = Vec::new();
    if g.any {
        // A network app sees every item, newest pages first, up to 180.
        let mut cursor: Option<String> = None;
        for _ in 0..3 {
            let c = cursor.as_ref().map(|c| format!("&cursor={}", api::enc(c))).unwrap_or_default();
            let Ok(page) = api::get::<MPage<ItemCard>>(n, &format!("/v1/accounts/{}/items?limit=60{c}", api::enc(address))) else { break };
            out.extend(page.data.into_iter().map(|i| (i, None)));
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        return out;
    }
    for (ty, _) in &g.collections {
        let page = api::get::<MPage<ItemCard>>(n, &format!("/v1/accounts/{}/items?collection={}&limit=60", api::enc(address), api::enc(ty))).map(|p| p.data).unwrap_or_default();
        let floor = api::get::<Value>(n, &format!("/v1/collections/{}", api::enc(ty))).ok().and_then(|d| d.pointer("/stats/floorMist").and_then(|f| f.as_str().map(str::to_string)));
        out.extend(page.into_iter().map(|i| (i, floor.clone())));
    }
    // Most sellable first: an offer, then listed, then the rest.
    let score = |i: &ItemCard| if i.top_offer_mist.is_some() { 2 } else if i.listing.is_some() { 1 } else { 0 };
    out.sort_by(|a, b| score(&b.0).cmp(&score(&a.0)).then(mist(&b.0.top_offer_mist).cmp(&mist(&a.0.top_offer_mist))).then(a.0.name.cmp(&b.0.name)));
    out
}

struct Fail(&'static str, String);

fn fail(code: &'static str, m: impl Into<String>) -> Fail {
    Fail(code, m.into())
}

fn account() -> Result<crate::sign::Account, Fail> {
    crate::sign::account().ok_or_else(|| fail("no_account", "Add a Rare Wallet account first"))
}

// ---------------------------------------------------------------- requests

/// `rare-api`, `rare-api-cancel` and `deep-link` from the API.
pub fn on_event(st: &mut State, fx: &mut Fx, name: &str, payload: &Value, now: f64) {
    match name {
        "rare-api" => request(st, fx, payload, now),
        "rare-api-cancel" => {
            let id = payload["id"].as_u64();
            if id.is_some() && st.feat.localapi.sheet.as_ref().and_then(|s| s.req) == id {
                drop_sheet(st, fx);
            }
        }
        "deep-link" => route(st, fx, payload, now),
        _ => {}
    }
}

/// Every update: a remote sending input shows an icon in the top bar for as long as it does.
pub fn tick(st: &mut State) {
    let on = rare_localapi::network_input_active();
    if on != st.remote_input {
        st.remote_input = on;
        st.ver.device += 1;
    }
}

/// Every update: links that came before the app was ready.
pub fn poll(st: &mut State, fx: &mut Fx, now: f64) {
    if !st.feat.localapi.queued.is_empty() {
        for r in std::mem::take(&mut st.feat.localapi.queued) {
            route(st, fx, &r, now);
        }
    }
}

fn request(st: &mut State, fx: &mut Fx, p: &Value, now: f64) {
    let Some(id) = p["id"].as_u64() else { return };
    let game_id = p["gameId"].as_str().map(str::to_string);
    let params = p["params"].clone();
    let lib = game_id.as_deref().and_then(|g| st.library.as_ref().and_then(|l| l.games.iter().find(|x| x.id == g).cloned()));
    let who = Who { id: game_id.clone(), lib };
    let seen = st.feat.localapi.sheet.is_some();
    let m = p["method"].as_str().unwrap_or("");
    match m {
        "me" => fx.spawn(move |_| {
            match me(&game_of(&who)) {
                Ok(v) => reply_ok(id, v),
                Err(Fail(c, m)) => reply_err(id, c, m),
            }
            Box::new(|_, _| {})
        }),
        "inventory" => fx.spawn(move |_| {
            match inventory(&game_of(&who)) {
                Ok(v) => reply_ok(id, v),
                Err(Fail(c, m)) => reply_err(id, c, m),
            }
            Box::new(|_, _| {})
        }),
        "open" => {
            let game = resolve_game(who.id.as_deref(), who.lib.as_ref(), &crate::screens::sui::catalog::seed());
            match open_view(st, fx, &params, &game) {
                Ok(()) => reply_ok(id, json!({ "ok": true })),
                Err(Fail(c, m)) => reply_err(id, c, m),
            }
        }
        "library" => reply_ok(id, library(st)),
        "tool" => crate::features::agent::api_tool(st, fx, id, &params),
        "launch" => {
            let target = params["id"].as_str().unwrap_or("").to_string();
            match launch_target(st, fx, &target) {
                Ok(title) => reply_ok(id, json!({ "ok": true, "id": target, "title": title })),
                Err(Fail(c, m)) => reply_err(id, c, m),
            }
        }
        "close" => {
            let target = params["id"].as_str().map(str::to_string).or_else(|| st.running.first().cloned());
            match target.filter(|t| st.is_running(t)) {
                None => reply_err(id, "not_found", if params["id"].is_null() { "Nothing is running" } else { "That isn't running" }),
                Some(t) => {
                    let title = st.run_name(&t);
                    fx.spawn(move |b| {
                        let r = b.quit(&t);
                        match r {
                            Ok(()) => reply_ok(id, json!({ "ok": true, "id": t, "title": title })),
                            Err(e) => reply_err(id, "failed", e),
                        }
                        let running = b.running();
                        Box::new(move |st: &mut State, _| st.set_running(running))
                    });
                }
            }
        }
        "sell" | "buy" | "claim" if seen => reply_err(id, "busy", "Another Rare sheet is open"),
        "sell" => prepare_sheet(fx, Some(id), who, move |g| prepare_sell(g, &params)),
        "buy" => prepare_sheet(fx, Some(id), who, move |g| prepare_buy(g, &params)),
        "claim" => {
            let text = params["voucher"].as_str().unwrap_or("").to_string();
            prepare_sheet(fx, Some(id), who, move |g| prepare_claim(Some(g), &text))
        }
        _ => reply_err(id, "bad_request", "Unknown method"),
    }
}

/// `GET /v1/library`: installed games, apps and what is running.
fn library(st: &State) -> Value {
    let games: Vec<Value> = st
        .games()
        .iter()
        .filter(|g| g.installed)
        .take(500)
        .map(|g| json!({ "id": g.id, "title": g.title, "source": g.source, "lastPlayed": g.last_played, "playtimeMinutes": g.playtime_minutes, "running": st.is_running(&g.id) }))
        .collect();
    let apps: Vec<Value> = crate::screens::apps::catalog::apps().iter().map(|a| json!({ "id": crate::screens::apps::catalog::run_id(&a.id), "name": a.name, "running": st.is_running(&crate::screens::apps::catalog::run_id(&a.id)) })).collect();
    let running: Vec<Value> = st.running.iter().map(|r| json!({ "id": r, "title": st.run_name(r) })).collect();
    json!({ "games": games, "apps": apps, "running": running })
}

/// `POST /v1/launch`: a game by library id, an app as `app:<id>`. Rare shows its usual toast.
fn launch_target(st: &mut State, fx: &mut Fx, id: &str) -> Result<String, Fail> {
    if let Some(app) = id.strip_prefix("app:") {
        let entry = crate::screens::apps::catalog::app_by_id(app).ok_or_else(|| fail("not_found", "No such app"))?;
        fx.run(Act::OpenApp(app.to_string()));
        return Ok(entry.name.clone());
    }
    let g = st.game(id).ok_or_else(|| fail("not_found", "That isn't in the library"))?;
    if !g.installed {
        return Err(fail("not_found", "That game isn't installed"));
    }
    let title = g.title.clone();
    fx.run(Act::Launch(id.to_string()));
    Ok(title)
}

fn me(game: &GameInfo) -> Result<Value, Fail> {
    let a = account()?;
    let network = super::config::get_str("network").filter(|n| n == "testnet").unwrap_or_else(|| "mainnet".into());
    let mut v = json!({
        "address": norm_addr(&a.address),
        "username": username(&a.address),
        "network": network,
        "canSign": a.signer,
        "game": { "id": null, "slug": game.slug, "title": game.title },
    });
    if game.any {
        // A network app has no game; it gets the balances instead.
        v["game"] = Value::Null;
        v["balances"] = balances(&a.address);
    }
    Ok(v)
}

/// Non-zero coin balances (SUI first), or null when the chain can't be reached.
fn balances(address: &str) -> Value {
    let Some(sui) = crate::screens::wallet::live::sui() else { return Value::Null };
    match crate::screens::wallet::live::rt().block_on(rare_sui::balances::balances(sui, &norm_addr(address))) {
        Ok(list) => Value::Array(
            list.into_iter().map(|b| json!({ "coinType": b.coin_type, "symbol": b.symbol, "name": b.name, "decimals": b.decimals, "raw": b.raw.to_string(), "amount": b.amount })).collect(),
        ),
        Err(_) => Value::Null,
    }
}

/// The player's Rare username (rare.market `/v1/players/<address>`), None when they have none.
fn username(address: &str) -> Value {
    let path = format!("/v1/players/{}", api::enc(&norm_addr(address)));
    api::get::<Value>(&*net(), &path).ok().and_then(|v| v["username"].as_str().filter(|s| !s.is_empty()).map(str::to_string)).map(Value::String).unwrap_or(Value::Null)
}

fn inventory(game: &GameInfo) -> Result<Value, Fail> {
    let a = account()?;
    if game.collections.is_empty() && !game.any {
        return Ok(json!({ "items": [] }));
    }
    let n = net();
    let items: Vec<Value> = load_items(&*n, &a.address, game)
        .into_iter()
        .map(|(i, floor)| {
            let mut v = serde_json::to_value(&i).unwrap_or(Value::Null);
            v["floorMist"] = floor.map(Value::String).unwrap_or(Value::Null);
            v
        })
        .collect();
    Ok(json!({ "items": items }))
}

/// `open`: the page in the launcher. An item opens its Marketplace page; a collection its page
/// (a Move type is resolved to rare.market's slug on a worker first).
fn open_view(st: &mut State, fx: &mut Fx, p: &Value, game: &GameInfo) -> Result<(), Fail> {
    match p["view"].as_str().unwrap_or("") {
        "item" => {
            let id = p["id"].as_str().ok_or_else(|| fail("bad_request", "view \"item\" needs an id"))?;
            st.set_tab(Tab::Market, false);
            crate::screens::market::live::open_on(format!("item:{}", norm_addr(id)));
        }
        "collection" => {
            let id = p["id"].as_str().ok_or_else(|| fail("bad_request", "view \"collection\" needs an id"))?.to_string();
            st.set_tab(Tab::Market, false);
            open_collection(fx, id);
        }
        "trophies" => st.open(Some(Overlay::Other("trophies"))),
        "store" => {
            let slug = p["id"].as_str().map(str::to_string).or_else(|| game.slug.clone()).ok_or_else(|| fail("not_found", "This game has no Rare page"))?;
            st.open(Some(Overlay::Game(format!("rare:{slug}"))));
        }
        _ => return Err(fail("bad_request", "Unknown view")),
    }
    Ok(())
}

/// A collection by slug, or by Move type (rare.market resolves the slug).
fn open_collection(fx: &mut Fx, id: String) {
    if !id.contains("::") {
        crate::screens::market::live::open_on(format!("collection:{id}"));
        return;
    }
    fx.spawn(move |_| {
        let slug = api::get::<Value>(&*net(), &format!("/v1/collections/{}", api::enc(&canon_type(&id)))).ok().and_then(|d| d["slug"].as_str().map(str::to_string));
        Box::new(move |st: &mut State, now: f64| match slug {
            Some(slug) => {
                st.set_tab(Tab::Market, false);
                crate::screens::market::live::open_on(format!("collection:{slug}"));
            }
            None => st.toast("That collection isn't on Rare.", Tone::Info, now),
        })
    });
}

fn route(st: &mut State, fx: &mut Fx, r: &Value, now: f64) {
    match r["kind"].as_str().unwrap_or("") {
        "item" => {
            let Some(id) = r["id"].as_str().map(str::to_string) else { return };
            st.set_tab(Tab::Market, false);
            crate::screens::market::live::open_on(format!("item:{}", norm_addr(&id)));
            match r["sheet"].as_str() {
                Some("sell") => {
                    let params = json!({ "itemId": id, "mode": "best-offer" });
                    prepare_sheet(fx, None, Who { id: None, lib: None }, move |g| prepare_sell(g, &params).map_err(|f| f));
                }
                Some("buy") => {
                    let params = json!({ "itemId": id });
                    prepare_sheet(fx, None, Who { id: None, lib: None }, move |g| prepare_buy(g, &params));
                }
                _ => {}
            }
        }
        "collection" => {
            if let Some(id) = r["id"].as_str() {
                st.set_tab(Tab::Market, false);
                open_collection(fx, id.to_string());
            }
        }
        "game" | "store" => {
            if let Some(s) = r["slug"].as_str() {
                st.open(Some(Overlay::Game(format!("rare:{s}"))));
            }
        }
        "trophies" => st.open(Some(Overlay::Other("trophies"))),
        "profile" => st.open(Some(Overlay::Other("profile"))),
        "claim" => {
            let text = r["voucher"].as_str().unwrap_or("").to_string();
            prepare_sheet(fx, None, Who { id: None, lib: None }, move |_| prepare_claim(None, &text));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- preparing the sheets

enum Prepared {
    Sheet(Kind),
    /// The answer for the game without a sheet.
    Answer(Value),
}

/// Runs `f` on a worker; a sheet comes up when it returns one, else the game gets the answer.
fn prepare_sheet(fx: &mut Fx, req: Option<u64>, who: Who, f: impl FnOnce(&GameInfo) -> Result<Prepared, Fail> + Send + 'static) {
    fx.spawn(move |_| {
        let run = who.id.clone().filter(|_| req.is_some());
        let game = game_of(&who);
        let r = f(&game);
        Box::new(move |st: &mut State, now: f64| match r {
            Ok(Prepared::Sheet(kind)) => {
                if st.feat.localapi.sheet.is_some() {
                    if let Some(id) = req {
                        reply_err(id, "busy", "Another Rare sheet is open");
                    }
                    return;
                }
                show(st, req, run, &game, kind);
            }
            Ok(Prepared::Answer(v)) => match req {
                Some(id) => reply_ok(id, v),
                None => st.toast(v["reason"].as_str().unwrap_or("That isn't available").to_string(), Tone::Info, now),
            },
            Err(Fail(c, m)) => match req {
                Some(id) => reply_err(id, c, m),
                None => st.toast(m, Tone::Error, now),
            },
        })
    });
}

/// Ask Rare's market_sell and market_list: the same sell sheet a game gets (sell into the best
/// offer, or list at `price` MIST), for any of the player's items. `done` gets a line for the model
/// once the sheet is up, or why it can't be.
pub fn agent_sell(fx: &mut Fx, item_id: &str, list: bool, price: Option<u64>, done: Box<dyn FnOnce(Result<String, String>) + Send>) {
    let mut params = json!({ "itemId": item_id, "mode": if list { "list" } else { "best-offer" } });
    if let Some(p) = price {
        params["priceMist"] = Value::from(p.to_string());
    }
    let who = Who { id: Some(AGENT_ID.into()), lib: None };
    fx.spawn(move |_| {
        let game = game_of(&who);
        let r = prepare_sell(&game, &params);
        Box::new(move |st: &mut State, _now: f64| match r {
            Ok(Prepared::Sheet(kind)) => {
                if st.feat.localapi.sheet.is_some() {
                    return done(Err("Another Rare sheet is open.".into()));
                }
                let line = match &kind {
                    Kind::Sell { item, list: true, .. } => format!("The listing sheet for {} is open; the player confirms it.", item.name),
                    Kind::Sell { item, quote: Some(_), .. } => format!("The sell sheet for {} is open with the best offer; the player confirms it.", item.name),
                    Kind::Sell { item, .. } => format!("The sell sheet for {} is open. There is no offer right now, so the player can list it instead.", item.name),
                    _ => "The sheet is open.".into(),
                };
                show(st, None, None, &game, kind);
                done(Ok(line));
            }
            Ok(Prepared::Answer(v)) => done(Err(v["reason"].as_str().unwrap_or("That item isn't available.").to_string())),
            Err(Fail(_, m)) => done(Err(m)),
        })
    });
}

fn show(st: &mut State, req: Option<u64>, run: Option<String>, game: &GameInfo, kind: Kind) {
    if let Ok(mut o) = OPEN_REQ.lock() {
        *o = req;
    }
    if req.is_some() {
        rare_localapi_front();
    }
    st.feat.localapi.sheet = Some(Sheet { run, req, asker: Asker { req_id: req, game_title: Some(game.title.clone()) }, game_title: game.title.clone(), kind, phase: Phase::Idle });
    st.feat.localapi.ver += 1;
    st.open(Some(Overlay::Other("apisheet")));
}

/// The launcher in front of the game, the way Guide does it.
fn rare_localapi_front() {
    rare_launch::apps::front::to_launcher();
}

fn prepare_sell(game: &GameInfo, p: &Value) -> Result<Prepared, Fail> {
    let a = account()?;
    if !a.signer {
        return Err(fail("watch_only", "This Rare Wallet account is watch only"));
    }
    let n = net();
    let want = norm_addr(p["itemId"].as_str().unwrap_or(""));
    let mine = load_items(&*n, &a.address, game);
    let Some((item, floor)) = mine.into_iter().find(|(i, _)| norm_addr(&i.id) == want) else {
        let card = api::get::<ItemCard>(&*n, &api::paths::item(&want)).ok();
        if let Some(c) = &card {
            if !game.collections.is_empty() && !owns(game, &c.kind) {
                return Err(fail("not_this_game", "That item isn't from this game"));
            }
        }
        let reason = if card.is_some() { "You don't hold this item" } else { "That item doesn't exist" };
        return Ok(Prepared::Answer(json!({ "status": "unavailable", "reason": reason })));
    };
    let list = p["mode"].as_str() == Some("list");
    let price = p["priceMist"].as_str().and_then(|s| s.parse().ok());
    let quote = if list { None } else { trade(&a.address).quote(&item, &a.address).ok().flatten() };
    Ok(Prepared::Sheet(Kind::Sell { item, list, price, floor: floor.and_then(|f| f.parse().ok()), quote }))
}

fn prepare_buy(game: &GameInfo, p: &Value) -> Result<Prepared, Fail> {
    let n = net();
    let me = crate::sign::account().map(|a| norm_addr(&a.address));
    let max: Option<u64> = p["maxMist"].as_str().and_then(|s| s.parse().ok());
    let all_in = |l: &Listing| mist(&l.all_in_mist).or_else(|| l.price_mist.parse().ok()).unwrap_or(0);
    let why = |c: &ItemCard| -> Option<&'static str> {
        let l = c.listing.as_ref()?;
        if me.as_ref().is_some_and(|m| l.seller.as_deref().is_some_and(|s| norm_addr(s) == *m)) {
            return Some("That's your own listing");
        }
        if max.is_some_and(|m| all_in(l) > m) {
            return Some("It costs more than maxMist");
        }
        None
    };
    let pick = if let Some(id) = p["itemId"].as_str() {
        let card = api::get::<ItemCard>(&*n, &api::paths::item(&norm_addr(id))).map_err(|_| fail("not_found", "That item doesn't exist"));
        let Ok(card) = card else { return Ok(Prepared::Answer(json!({ "status": "unavailable", "reason": "That item doesn't exist" }))) };
        if !game.collections.is_empty() && !owns(game, &card.kind) {
            return Err(fail("not_this_game", "That item isn't from this game"));
        }
        if card.listing.is_none() {
            return Ok(Prepared::Answer(json!({ "status": "unavailable", "reason": "That item isn't for sale" })));
        }
        if let Some(w) = why(&card) {
            return Ok(Prepared::Answer(json!({ "status": "unavailable", "reason": w })));
        }
        card
    } else {
        let mut cols: Vec<String> = game.collections.iter().map(|c| c.0.clone()).collect();
        if game.any {
            cols = p["collection"].as_str().map(|c| vec![canon_type(c)]).ok_or_else(|| fail("bad_request", "give itemId or collection"))?;
        } else if let Some(c) = p["collection"].as_str() {
            if !owns(game, c) {
                return Err(fail("not_this_game", "That collection isn't from this game"));
            }
            cols.retain(|x| type_matches(x, c));
        }
        if cols.is_empty() {
            return Ok(Prepared::Answer(json!({ "status": "unavailable", "reason": "This game has no items on Rare yet" })));
        }
        let mut cards: Vec<ItemCard> = Vec::new();
        for ty in &cols {
            let slug = api::get::<Value>(&*n, &format!("/v1/collections/{}", api::enc(ty))).ok().and_then(|d| d["slug"].as_str().map(str::to_string));
            if let Some(s) = slug {
                if let Ok(pg) = api::get::<MPage<ItemCard>>(&*n, &format!("/v1/collections/{}/items?sort=price_asc&limit=24", api::enc(&s))) {
                    cards.extend(pg.data);
                }
            }
        }
        cards.retain(|c| c.listing.is_some() && why(c).is_none());
        cards.sort_by_key(|c| all_in(c.listing.as_ref().unwrap()));
        let kind = p["kind"].as_u64();
        let found = match kind {
            None => cards.into_iter().next(),
            Some(k) => cards.into_iter().take(12).find(|c| {
                api::get::<api::ItemDetail>(&*n, &api::paths::item(&c.id)).ok().is_some_and(|d| d.attributes.iter().flatten().any(|t| t.kind.eq_ignore_ascii_case("kind") && t.value == k.to_string()))
            }),
        };
        match found {
            Some(c) => c,
            None => {
                let reason = if max.is_some() { "Nothing listed at or under maxMist" } else { "Nothing is listed right now" };
                return Ok(Prepared::Answer(json!({ "status": "unavailable", "reason": reason })));
            }
        }
    };
    let listing = pick.listing.clone().unwrap();
    Ok(Prepared::Sheet(Kind::Buy { item: pick, listing }))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// methods.ts `prepareClaim`: reads the voucher and scopes it. It must be for the player's account
/// and not expired; an item or state voucher must be for one of the game's item types (`Item<C>`,
/// read from the Class or Item object on chain); the package it needs must be published. `game`
/// None: a claim link the player opened, not a game.
fn prepare_claim(game: Option<&GameInfo>, text: &str) -> Result<Prepared, Fail> {
    use super::claim as c;
    let answer = |status: &str, reason: &str| Ok(Prepared::Answer(json!({ "status": status, "reason": reason })));
    let v = c::decode(text).map_err(|m| fail("bad_request", m))?;
    let a = account()?;
    if !a.signer {
        return Err(fail("watch_only", "This Rare Wallet account is watch only"));
    }
    if let Some(why) = c::reject_reason(&v, &a.address, now_ms()) {
        return answer("rejected", &why);
    }
    let slug = game.and_then(|g| g.slug.clone());
    if let c::Voucher::Trophy { registry, .. } = &v {
        let cfg = rare_sui::network::trophy_config().filter(|c| c.network == rare_sui::Network::Mainnet);
        let Some(cfg) = cfg else { return answer("unavailable", "Rare Trophies isn't live yet") };
        if norm_addr(&cfg.registry) != *registry {
            return answer("rejected", "The voucher is for a different trophy registry");
        }
        return Ok(Prepared::Sheet(Kind::Claim { voucher: v, chain: None, slug }));
    }
    let Some(pkg) = rare_sui::network::items_package(rare_sui::Network::Mainnet) else { return answer("unavailable", "Rare items aren't live yet") };
    let sui = crate::screens::wallet::live::sui().ok_or_else(|| fail("offline", "Couldn't reach the Sui network"))?;
    let read = crate::screens::wallet::live::rt().block_on(async {
        match &v {
            c::Voucher::Item { class_id, .. } => c::read_class(sui, &pkg, class_id).await.map(|ch| (ch, None)),
            c::Voucher::State { item_id, .. } => c::read_item(sui, &pkg, item_id).await.map(|(ch, rev)| (ch, Some(rev))),
            c::Voucher::Trophy { .. } => unreachable!(),
        }
    });
    let (chain, rev) = match read {
        Ok(r) => r,
        Err(why) => return answer("rejected", &why),
    };
    if let (c::Voucher::State { rev: want, .. }, Some(have)) = (&v, rev) {
        if *want != have {
            return answer("rejected", "This item changed since the voucher was made. Ask the game for a new one.");
        }
    }
    if let Some(g) = game {
        if !owns(g, &c::item_type(&pkg, &chain.marker)) {
            return Err(fail("not_this_game", "That item class isn't from this game"));
        }
    }
    Ok(Prepared::Sheet(Kind::Claim { voucher: v, chain: Some(chain), slug }))
}

/// claim.ts `runClaim`: the one transaction for a voucher, reviewed and signed in Rare Wallet. A
/// game's claim is paid by its gas budget first, then Rare's trophy cover (trophies), then the player.
fn run_claim(voucher: &super::claim::Voucher, chain: &Option<super::claim::Chain>, slug: Option<&str>, who: &str, player: &str) -> Result<Value, String> {
    use super::claim as c;
    let unavailable = |r: &str| Ok(json!({ "status": "unavailable", "reason": r }));
    let digest = |tx: rare_sui::ptb::Tx| -> Result<Value, String> {
        let d = crate::sign::sign_for_game(tx, player, slug, who, voucher.title(), matches!(voucher, c::Voucher::Trophy { .. }))?;
        Ok(json!({ "status": "claimed", "digest": d }))
    };
    match voucher {
        c::Voucher::Trophy { game, trophy_id, nonce, expires_ms, sig, .. } => {
            let Some(cfg) = rare_sui::network::trophy_config() else { return unavailable("Rare Trophies isn't live yet") };
            let profile = super::trophies::profile_id_of(player)?;
            let att = super::trophies::Attestation { trophy_id: *trophy_id, nonce: *nonce, expires_ms: *expires_ms, signature: sig.clone() };
            digest(super::trophies::build_claim_tx(&cfg.package, &cfg.registry, game, player, profile.as_deref(), &[att]))
        }
        c::Voucher::Item { .. } | c::Voucher::State { .. } => {
            let (Some(pkg), Some(chain)) = (rare_sui::network::items_package(rare_sui::Network::Mainnet), chain) else { return unavailable("Rare items aren't live yet") };
            let sui = crate::screens::wallet::live::sui().ok_or("Couldn't reach the Sui network")?;
            let owned = crate::screens::wallet::live::rt().block_on(rare_sui::kiosk::owned_kiosks(sui, player)).map_err(|e| e.to_string())?;
            let plain = owned.kiosks.iter().find(|k| !k.is_personal).map(|k| (k.kiosk_id.clone(), k.owner_cap_id.clone()));
            let tx = match voucher {
                c::Voucher::Item { .. } => c::build_item_claim(&pkg, voucher, &chain.marker, chain.policy_id.as_deref().unwrap_or_default(), player, plain.as_ref()),
                c::Voucher::State { item_id, .. } => {
                    // Where the item is now: in one of the player's kiosks, or in the wallet.
                    let card = api::get::<ItemCard>(&*net(), &api::paths::item(item_id)).ok();
                    let kid = card.and_then(|c| c.kiosk_id).map(|k| norm_addr(&k));
                    let kiosk = match &kid {
                        Some(kid) => match owned.kiosks.iter().find(|k| norm_addr(&k.kiosk_id) == *kid) {
                            None => return unavailable("That item isn't in one of your kiosks"),
                            Some(k) if k.is_personal => return unavailable("Items in a personal kiosk can't be updated yet"),
                            Some(k) => Some((k.kiosk_id.clone(), k.owner_cap_id.clone())),
                        },
                        None => None,
                    };
                    c::build_state_claim(&pkg, voucher, &chain.marker, chain.class_id.as_deref().unwrap_or_default(), player, kiosk.as_ref())
                }
                c::Voucher::Trophy { .. } => None,
            };
            digest(tx.ok_or("The voucher can't be turned into a transaction")?)
        }
    }
}

fn trade(_addr: &str) -> RealTrade {
    RealTrade { net: net(), cfg: Default::default(), chain: None, exec: crate::sign::executor() }
}

// ---------------------------------------------------------------- finishing

/// The player is done (or the request ended): answers the game and closes the sheet.
fn finish(st: &mut State, result: Value) -> Option<String> {
    let s = st.feat.localapi.sheet.take();
    st.feat.localapi.ver += 1;
    let mut run = None;
    if let Some(s) = s {
        if let Some(id) = s.req {
            if let Ok(mut o) = OPEN_REQ.lock() {
                *o = None;
            }
            reply_ok(id, result);
            run = s.run;
        }
    }
    if st.overlay == Some(Overlay::Other("apisheet")) {
        st.close();
    }
    run
}

/// `finish`, then the game that asked gets the screen and the pad back (gamescope puts the
/// resumed game's window in front, the way Resume does).
fn finish_to_game(st: &mut State, fx: &mut Fx, result: Value) {
    let run = finish(st, result);
    hand_back(st, fx, run);
}

fn hand_back(st: &State, fx: &mut Fx, run: Option<String>) {
    let Some(id) = run.filter(|id| st.is_running(id)) else { return };
    fx.spawn(move |b| {
        let _ = b.launch(&id);
        Box::new(|_: &mut State, _: f64| {})
    });
}

/// The API gave up on the request: the sheet goes without an answer.
fn drop_sheet(st: &mut State, fx: &mut Fx) {
    let run = st.feat.localapi.sheet.take().and_then(|s| s.run);
    hand_back(st, fx, run);
    st.feat.localapi.ver += 1;
    if let Ok(mut o) = OPEN_REQ.lock() {
        *o = None;
    }
    if st.overlay == Some(Overlay::Other("apisheet")) {
        st.close();
    }
}

fn close_answer(s: &Sheet) -> Value {
    match &s.phase {
        Phase::Done(v) => v.clone(),
        _ => json!({ "status": "cancelled" }),
    }
}

// ---------------------------------------------------------------- the page

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// Sell now, List, or Buy.
    Go,
    /// Sell sheet: switch between selling into the offer and listing.
    Other,
    Close,
    Swallow,
}

fn sui(m: Option<u128>) -> String {
    format!("{} SUI", crate::kit::format::fmt_sui(m, 4, false, true))
}

pub fn view(s: &Sheet) -> El<Msg> {
    let busy = s.phase == Phase::Working;
    let done = matches!(s.phase, Phase::Done(_));
    let mut body = div("flex flex-col gap-[0.8rem] py-2 text-small");
    let (title, mut sub, go, other): (String, Option<String>, String, Option<String>);
    let mut go_off = false;
    match &s.kind {
        Kind::Claim { voucher, .. } => {
            title = if matches!(&s.phase, Phase::Done(v) if v["status"].as_str() == Some("claimed")) { "Claimed".to_string() } else { voucher.title().to_string() };
            sub = Some("From a Rare link".to_string());
            body = body.kid(p("text-body").text(voucher.what()));
            body = body.kid(p("text-secondary").text(if matches!(voucher, super::claim::Voucher::Trophy { .. }) {
                "Goes to your Rare profile. Rare covers the network fee when your wallet is short."
            } else {
                "Goes to your Rare Wallet. You review it before anything is signed."
            }));
            go = "Claim".into();
            other = None;
        }
        Kind::Sell { item, list, price, floor, quote } => {
            title = format!("Sell {}", item.name);
            sub = Some(item.collection.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| s.game_title.clone()));
            let list_price = price.or(*floor);
            if *list {
                body = body.kid(p("text-body").text(match list_price {
                    Some(m) => format!("List for {}", sui(Some(m as u128))),
                    None => "Set a price in the marketplace to list this item.".into(),
                }));
                go_off = list_price.is_none();
                go = "List".into();
                other = quote_label(quote).map(|_| "Sell to the best offer".to_string());
            } else {
                match quote {
                    Some(q) => {
                        body = body.kid(p("text-body").text(format!("Best offer {}", sui(mist(&Some(q.offer.price_mist.clone())).map(u128::from)))));
                        body = body.kid(p("text-secondary").text(format!("You receive {}", sui(Some(q.net_mist.max(0) as u128)))));
                        if let Some(b) = &q.blocked {
                            body = body.kid(p("text-negative").text(b.clone()));
                            go_off = true;
                        } else if q.sponsored {
                            body = body.kid(p("text-secondary").text("Rare covers the network fee."));
                        }
                    }
                    None => {
                        body = body.kid(p("text-body").text("No offers on this item right now."));
                        go_off = true;
                    }
                }
                go = match quote {
                    Some(q) => format!("Sell for {}", sui(Some(q.net_mist.max(0) as u128))),
                    None => "Sell".into(),
                };
                other = list_price.map(|m| format!("List for {}", sui(Some(m as u128))));
            }
        }
        Kind::Buy { item, listing } => {
            title = format!("Buy {}", item.name);
            sub = Some(item.collection.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| s.game_title.clone()));
            let price = mist(&listing.all_in_mist).or_else(|| listing.price_mist.parse().ok());
            body = body.kid(p("text-body").text(format!("Price {}", sui(price.map(u128::from)))));
            body = body.kid(p("text-secondary").text("You review it before anything is signed."));
            go = "Buy".into();
            other = None;
        }
    }
    if let Some(a) = asked_by(&s.asker) {
        sub = Some(a);
    }
    match &s.phase {
        Phase::Error(m) => body = body.kid(p("text-negative").text(m.clone())),
        Phase::Done(v) if v["status"].as_str() == Some("unavailable") => body = body.kid(p("text-negative").text(v["reason"].as_str().unwrap_or("That isn't available").to_string())),
        Phase::Done(v) => {
            let word = match v["status"].as_str() {
                Some("sold") => "Sold",
                Some("listed") => "Listed",
                Some("claimed") => "Done",
                _ => "Bought",
            };
            body = body.kid(p("flex items-center gap-[0.6rem]").kid(dot(UiTone::Positive, false, "")).kid(text(format!(" {word}. It can take a minute to show."))));
        }
        _ => {}
    }
    let mut footer = Vec::new();
    if !done {
        let label = if busy { "Waiting for approval".to_string() } else if matches!(s.phase, Phase::Error(_)) { "Try again".to_string() } else { go };
        footer.push(button_el(ButtonSpec::new(&label).v(BVariant::Primary).size(BSize::Md).def(true).class("flex-1").disabled(busy || go_off).hint("Confirm"), Some(Msg::Go)));
        if let Some(o) = other {
            footer.push(button_el(ButtonSpec::new(&o).size(BSize::Md).disabled(busy), Some(Msg::Other)));
        }
        footer.push(button_el(ButtonSpec::new("Not now").size(BSize::Md), Some(Msg::Close)));
    }
    let mut el = sheet(SheetSpec { title, subtitle: sub, icon: None, footer: (!footer.is_empty()).then_some(footer), close: Some(Msg::Close), width: "34rem" }, vec![body]);
    for b in ["lb", "rb", "lt", "rt", "x", "y", "select", "start", "qam"] {
        el = el.on_pad(b, Msg::Swallow);
    }
    el
}

fn quote_label(q: &Option<Quote>) -> Option<()> {
    q.as_ref().map(|_| ())
}

pub fn fixture_sheet() -> Sheet {
    let item = ItemCard { id: "0x1204".into(), name: "Pawtato Hero #1204".into(), kind: "0xabc::hero::Hero".into(), ..Default::default() };
    Sheet { run: None, req: None, asker: Asker { req_id: Some(1), game_title: Some("Pawtato".into()) }, game_title: "Pawtato".into(), kind: Kind::Sell { item, list: false, price: None, floor: Some(2_500_000_000), quote: None }, phase: Phase::Idle }
}

#[derive(Default)]
pub struct SheetPage {
    built: Built<Msg>,
    /// When a finished sheet hands the screen back (1.6 s after Done, as in the web).
    done_at: Option<f64>,
}

impl Drop for SheetPage {
    fn drop(&mut self) {
        // The shell closed the overlay (Guide, another page): a request nobody answered is cancelled.
        if let Some(id) = OPEN_REQ.lock().ok().and_then(|mut o| o.take()) {
            reply_ok(id, json!({ "status": "cancelled" }));
        }
    }
}

impl Page for SheetPage {
    fn stamp(&self, st: &State) -> u64 {
        crate::page::hash(st.feat.localapi.ver)
    }

    fn view(&mut self, ui: &mut crate::kit::Ui, st: &State, acts: &mut Acts) -> rare_render::view::El {
        let fixture = fixture_sheet();
        let s = st.feat.localapi.sheet.as_ref().unwrap_or(&fixture);
        let mut m = view(s);
        m.class = m.class.replace("fixed inset-x-0 bottom-[3rem] top-0 z-40", "h-full w-full");
        let (el, built) = build(ui, acts, &m);
        self.built = built;
        el
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&rare_render::focus::Focusable, Option<&Act>)>) -> bool {
        match b {
            Btn::B => {
                let ans = st.feat.localapi.sheet.as_ref().map(close_answer).unwrap_or_else(|| json!({ "status": "cancelled" }));
                if st.feat.localapi.sheet.as_ref().is_some_and(|s| s.phase == Phase::Working) {
                    return true;
                }
                finish_to_game(st, fx, ans);
                true
            }
            Btn::Lb | Btn::Rb | Btn::Lt | Btn::Rt | Btn::X | Btn::Y | Btn::Select | Btn::Start | Btn::Qam => true,
            _ => false,
        }
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        let Some(m) = self.built.msg(code) else { return };
        match m {
            Msg::Swallow => {}
            Msg::Close => {
                let ans = st.feat.localapi.sheet.as_ref().map(close_answer).unwrap_or_else(|| json!({ "status": "cancelled" }));
                finish_to_game(st, fx, ans);
            }
            Msg::Other => {
                if let Some(Sheet { kind: Kind::Sell { list, .. }, phase: Phase::Idle | Phase::Error(_), .. }) = st.feat.localapi.sheet.as_mut() {
                    *list = !*list;
                    st.feat.localapi.ver += 1;
                }
                // Selling into the offer needs the quote: fetch it when switching back.
                refresh_quote(st, fx);
            }
            Msg::Go => go(st, fx),
        }
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, now: f64) -> Option<f64> {
        let done = st.feat.localapi.sheet.as_ref().is_some_and(|s| matches!(s.phase, Phase::Done(_)));
        if !done {
            self.done_at = None;
            return None;
        }
        let at = *self.done_at.get_or_insert(now + 1.6);
        if now >= at {
            let ans = st.feat.localapi.sheet.as_ref().map(close_answer).unwrap_or(Value::Null);
            finish_to_game(st, fx, ans);
            return None;
        }
        Some(at)
    }
}

fn refresh_quote(st: &mut State, fx: &mut Fx) {
    let Some(Sheet { req, kind: Kind::Sell { item, list: false, quote: None, .. }, .. }) = st.feat.localapi.sheet.clone() else { return };
    let Some(a) = crate::sign::account() else { return };
    fx.spawn(move |_| {
        let q = trade(&a.address).quote(&item, &a.address).ok().flatten();
        Box::new(move |st: &mut State, _| {
            if let Some(Sheet { req: r, kind: Kind::Sell { quote, .. }, .. }) = st.feat.localapi.sheet.as_mut() {
                if *r == req {
                    *quote = q;
                    st.feat.localapi.ver += 1;
                }
            }
        })
    });
}

/// Confirm pressed: builds the transaction and hands it to Rare Wallet's review on a worker.
fn go(st: &mut State, fx: &mut Fx) {
    let Some(sh) = st.feat.localapi.sheet.as_mut() else { return };
    if matches!(sh.phase, Phase::Working | Phase::Done(_)) {
        return;
    }
    let Some(acct) = crate::sign::account().filter(|a| a.signer) else {
        sh.phase = Phase::Error(crate::sign::NO_SIGNER.into());
        st.feat.localapi.ver += 1;
        return;
    };
    sh.phase = Phase::Working;
    let (req, kind, who) = (sh.req, sh.kind.clone(), sh.game_title.clone());
    st.feat.localapi.ver += 1;
    fx.spawn(move |_| {
        let result: Result<Value, String> = match kind {
            Kind::Sell { item, list, price, floor, quote } => {
                let t = trade(&acct.address);
                if list {
                    match price.or(floor) {
                        Some(pm) => t.list(&item, pm, &acct.address).map(|_| json!({ "status": "listed", "digest": "" })).map_err(|e| e.0),
                        None => Err("Set a price first".into()),
                    }
                } else {
                    match quote {
                        Some(q) => {
                            let received = q.net_mist.max(0).to_string();
                            let offer: Offer = q.offer;
                            t.sell(&item, &offer, &acct.address).map(|_| json!({ "status": "sold", "digest": "", "receivedMist": received })).map_err(|e| e.0)
                        }
                        None => Err("No offer to sell into".into()),
                    }
                }
            }
            Kind::Claim { voucher, chain, slug } => run_claim(&voucher, &chain, slug.as_deref(), &who, &acct.address),
            Kind::Buy { item, listing } => {
                let n = net();
                rare_market::buy::build_buy_transaction(&*n, &Default::default(), &item, &listing, &acct.address, None, None).map_err(|e| if e.0.is_empty() { "This item can't be bought here.".into() } else { e.0 }).and_then(|tx| {
                    crate::sign::sign_and_execute(tx, &acct.address, &format!("Buy {}", item.name)).map(|d| json!({ "status": "bought", "digest": d, "itemId": item.id }))
                })
            }
        };
        Box::new(move |st: &mut State, _| {
            let Some(sh) = st.feat.localapi.sheet.as_mut() else { return };
            if sh.req != req {
                return;
            }
            sh.phase = match result {
                Ok(v) => Phase::Done(v),
                Err(e) if e == SIGN_REJECTED => Phase::Idle,
                Err(e) => Phase::Error(e),
            };
            st.feat.localapi.ver += 1;
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_and_addresses() {
        assert!(type_matches("0x0002::a::B<0x2::sui::SUI>", "0x2::a::B"));
        assert!(!type_matches("0x2::a::C", "0x2::a::B"));
        assert_eq!(norm_addr("0x1").len(), 66);
    }

    #[test]
    fn sell_sheet_text() {
        let s = fixture_sheet();
        let v = view(&s);
        let t = v.texts();
        assert_eq!(t[0], "Sell Pawtato Hero #1204");
        assert_eq!(t[1], "Pawtato asked Rare");
        assert!(t.iter().any(|x| x == "No offers on this item right now."));
        assert!(t.iter().any(|x| x.starts_with("List for ")));
        assert!(t.iter().any(|x| x == "Not now"));
    }

    #[test]
    fn api_event_opens_sheet_and_cancel_closes_it() {
        let mut st = State::default();
        show(&mut st, None, None, &GameInfo { title: "Pawtato".into(), ..Default::default() }, fixture_sheet().kind);
        assert_eq!(st.overlay, Some(Overlay::Other("apisheet")));
        finish(&mut st, json!({ "status": "cancelled" }));
        assert!(st.overlay.is_none());
    }
}
