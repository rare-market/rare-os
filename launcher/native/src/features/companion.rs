//! Phone control: rare.market on the owner's phone (rare.market/devices) controls this device.
//!
//! Pairing: Settings > Devices asks rare.market for a code (`POST /v1/devices/self/pair`, signed with this
//! device's friends key) and shows it as a QR code; the phone, signed in to the same account, confirms it. Only
//! a key linked to that account can be paired, so commands reach only the owner's devices.
//!
//! Commands arrive live as `device` events on the notify stream (social.rs hands them here) and are also
//! picked up every 30 s (`GET /v1/devices/self/commands`), each run once. They run on the UI thread in
//! [`poll`] and are answered with an ack (done or failed, with a line the phone shows). The status (battery,
//! what's playing, storage, downloads, finished downloads) goes up every 30 s, sooner when it changes; the
//! server turns finished downloads and low battery into pushes to the phone.
//!
//! A background thread does all network work; the UI thread never waits on it.

use crate::act::Act;
use crate::features::downloads::Ctl;
use crate::page::Fx;
use crate::state::{State, Tone};
use rare_friends::sign::DeviceKey;
use rare_market::net::Method;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// What Settings > Devices shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// The background thread runs (a device run, not a fixture).
    pub live: bool,
    /// A Rare Wallet account is in use.
    pub signed_in: bool,
    /// This device has a linked friends key for it (phone control signs with it).
    pub key: bool,
    /// None until rare.market answered.
    pub paired: Option<bool>,
    /// The code on screen: (shown code, URL for the QR, expires at ms).
    pub code: Option<(String, String, i64)>,
    pub busy: bool,
    pub error: Option<String>,
    /// The latest actions from the phone: (ms, what, done).
    pub recent: Vec<(i64, String, bool)>,
    /// The account, for the "Paired with" line.
    pub account: Option<String>,
}

static VIEW: Mutex<Option<View>> = Mutex::new(None);
static VER: AtomicU64 = AtomicU64::new(0);
static QUEUE: Mutex<VecDeque<Cmd>> = Mutex::new(VecDeque::new());
static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static SNAP: Mutex<Option<Snap>> = Mutex::new(None);
static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

/// Moves when [`view`] changes (Settings rebuilds the open section).
pub fn version() -> u64 {
    VER.load(Ordering::Relaxed)
}

pub fn view() -> View {
    VIEW.lock().unwrap().clone().unwrap_or_default()
}

fn edit(f: impl FnOnce(&mut View)) {
    {
        let mut g = VIEW.lock().unwrap();
        let v = g.get_or_insert_with(View::default);
        let before = v.clone();
        f(v);
        if *v == before {
            return;
        }
    }
    VER.fetch_add(1, Ordering::Relaxed);
    super::bus::wake();
}

fn note_recent(what: String, ok: bool) {
    edit(|v| {
        v.recent.insert(0, (now_ms(), what, ok));
        v.recent.truncate(6);
    });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

enum Msg {
    Wake,
    Pair,
    Unpair,
    Ack { id: String, ok: bool, message: String },
    File { id: String, file: String, name: String, mime: String },
    Type { id: String, text: String, enter: bool },
}

fn send(m: Msg) {
    if let Some(tx) = TX.get() {
        let _ = tx.lock().unwrap().send(m);
    }
}

/// Settings > Devices' buttons.
pub fn pair() {
    if TX.get().is_none() {
        // No phone control here (fixtures, a bench build): say so instead of nothing.
        crate::screens::settings::live::toast_pub("Phone control runs on the device (fixture).", false);
        return;
    }
    edit(|v| {
        v.busy = true;
        v.error = None;
    });
    send(Msg::Pair);
}

pub fn unpair() {
    edit(|v| v.busy = true);
    send(Msg::Unpair);
}

// ---------------------------------------------------------------- commands

#[derive(Clone, Debug, PartialEq)]
pub struct Cmd {
    pub id: String,
    pub kind: String,
    pub data: Value,
}

/// A `device` event from the notify stream (any thread).
pub fn on_stream(data: &Value) {
    match data["kind"].as_str() {
        Some("command") => {
            if let Some(c) = parse_cmd(&data["command"]) {
                enqueue(c);
            }
        }
        Some("paired") | Some("unpaired") => send(Msg::Wake),
        _ => {}
    }
}

/// `{ id, kind, args, at }`: the command's own id and kind, what it carries in `args`.
fn parse_cmd(v: &Value) -> Option<Cmd> {
    let id = v["id"].as_str()?.to_string();
    let kind = v["kind"].as_str()?.to_string();
    let data = v.get("args").filter(|a| a.is_object()).cloned().unwrap_or_else(|| v.clone());
    Some(Cmd { id, kind, data })
}

fn enqueue(c: Cmd) {
    {
        let mut g = SEEN.lock().unwrap();
        let seen = g.get_or_insert_with(HashSet::new);
        if !seen.insert(c.id.clone()) {
            return;
        }
    }
    QUEUE.lock().unwrap().push_back(c);
    super::bus::wake();
}

fn ack(id: &str, ok: bool, message: impl Into<String>) {
    let message = message.into();
    note_recent(message.clone(), ok);
    send(Msg::Ack { id: id.to_string(), ok, message });
}

/// Every update on the UI thread: run what the phone sent, and keep the status snapshot fresh.
pub fn poll(st: &mut State, fx: &mut Fx, now: f64) {
    if TX.get().is_none() {
        return;
    }
    let cmds: Vec<Cmd> = QUEUE.lock().unwrap().drain(..).collect();
    for c in cmds {
        run(st, fx, c, now);
    }
    snapshot(st, now);
}

fn run(st: &mut State, fx: &mut Fx, c: Cmd, now: f64) {
    let d = &c.data;
    log::info!("phone: {} {}", c.kind, c.id);
    match c.kind.as_str() {
        "text" => {
            let text = d["text"].as_str().unwrap_or("").to_string();
            let enter = d["enter"].as_bool().unwrap_or(false);
            if let Some(o) = st.osk.as_mut() {
                o.value.push_str(&text.replace('\n', " "));
                let title = o.title.clone();
                if enter {
                    if let Some(req) = st.osk.take() {
                        let v = req.value.clone();
                        crate::screens::osk::finish(st, req, Some(v));
                    }
                }
                ack(&c.id, true, format!("Typed into {title}"));
            } else if !st.running.is_empty() && !rare_launch::apps::front::launcher_front() {
                send(Msg::Type { id: c.id, text, enter });
            } else {
                ack(&c.id, false, "Nothing on the device is waiting for text. Open a text field first.");
            }
        }
        "open" => {
            let url = d["url"].as_str().unwrap_or("").to_string();
            if !url.starts_with("https://") {
                return ack(&c.id, false, "Links need to start with https://.");
            }
            st.toast("Opening a link from your phone", Tone::Info, now);
            fx.run(Act::OpenWeb(url.clone()));
            ack(&c.id, true, format!("Opened {}", short_url(&url)));
        }
        "install" => install(st, fx, &c, now),
        "downloads" => downloads(st, fx, &c, now),
        "file" => {
            let file = d["fileId"].as_str().unwrap_or("").to_string();
            let name = d["name"].as_str().unwrap_or("file").to_string();
            let mime = d["mime"].as_str().unwrap_or("").to_string();
            send(Msg::File { id: c.id, file, name, mime });
        }
        _ => ack(&c.id, false, "This version of Rare does not know that action yet. Update Rare."),
    }
}

fn short_url(u: &str) -> String {
    let h = u.split("://").nth(1).unwrap_or(u);
    let h = h.split(['/', '?', '#']).next().unwrap_or(h);
    h.trim_start_matches("www.").to_string()
}

fn install(st: &mut State, fx: &mut Fx, c: &Cmd, now: f64) {
    let d = &c.data;
    let store = d["store"].as_str().unwrap_or("");
    let id = d["id"].as_str().unwrap_or("").to_string();
    let title = d["title"].as_str().map(str::to_string);
    match store {
        "steam" => match id.parse::<u32>() {
            Ok(appid) => steam_install(st, fx, c.id.clone(), appid, title, now),
            Err(_) => ack(&c.id, false, "That Steam app number is not valid."),
        },
        "epic" | "gog" => store_install(st, &c.id, &format!("{store}:{id}"), title, now),
        "rare" => {
            // A rare.market game: its Steam build when it has one, else a web game into the library, else its page.
            let game = crate::screens::sui::catalog::seed().into_iter().find(|g| g.slug == id);
            let prof = crate::screens::sui::catalog::profile(&id);
            let name = title.or_else(|| game.as_ref().map(|g| g.name.clone())).or_else(|| prof.as_ref().and_then(|p| p.title.clone())).unwrap_or_else(|| id.clone());
            let steam = prof.as_ref().and_then(|p| p.native.as_ref()).and_then(|n| n.steam_app_ids.first().copied());
            let play = game.as_ref().and_then(|g| g.play_url.clone()).or_else(|| prof.as_ref().and_then(|p| p.play_url.clone().flatten()));
            if let Some(appid) = steam {
                steam_install(st, fx, c.id.clone(), appid as u32, Some(name), now);
            } else if let Some(url) = play {
                if st.game(&id).is_some() {
                    fx.run(Act::OpenGame(id.clone()));
                    ack(&c.id, true, format!("{name} is in the library"));
                } else {
                    let (cover, hero) = game.as_ref().map(|g| (Some(g.logo_url.clone()).filter(|s| !s.is_empty()), Some(g.banner_url.clone()).filter(|s| !s.is_empty()))).unwrap_or((None, None));
                    fx.run(Act::AddWebGame { id: Some(id.clone()), title: name.clone(), url, cover, hero });
                    st.toast(format!("{name} added from your phone"), Tone::Info, now);
                    ack(&c.id, true, format!("Added {name} to the library"));
                }
            } else if game.is_some() || prof.is_some() {
                fx.run(Act::SuiGame(id.clone()));
                ack(&c.id, true, format!("Opened {name}. It has no install on this device yet."));
            } else {
                ack(&c.id, false, format!("{name} is not in Rare's catalog on this device yet."));
            }
        }
        _ => ack(&c.id, false, "Unknown store."),
    }
}

/// A Steam install into the library folder with the most free space (the drive sheet's pick, without asking).
fn steam_install(st: &mut State, fx: &mut Fx, cmd: String, appid: u32, title: Option<String>, now: f64) {
    let gid = format!("steam:{appid}");
    let name = st.game(&gid).map(|g| g.title.clone()).or(title).unwrap_or_else(|| format!("Steam app {appid}"));
    if st.game(&gid).is_some_and(|g| g.installed) {
        return ack(&cmd, true, format!("{name} is already installed"));
    }
    st.feat.downloads.asked.retain(|(a, _)| *a != gid);
    st.feat.downloads.asked.push((gid.clone(), now));
    st.feat.downloads.ver += 1;
    st.toast(format!("Installing {name}, sent from your phone"), Tone::Info, now);
    fx.spawn(move |b| {
        let folders = b.steam_folders();
        let best = folders.iter().max_by_key(|f| f.free.unwrap_or(0)).map(|f| f.path.clone());
        let r = match best {
            None => Err("Steam has no library folder yet. Open Steam once, then try again.".to_string()),
            Some(path) => b.steam_install(appid, &path),
        };
        Box::new(move |st: &mut State, now| match r {
            Ok(rare_launch::steamdl::Started::Queued) => ack(&cmd, true, format!("{name} is in the downloads")),
            Ok(rare_launch::steamdl::Started::SteamShown) => ack(&cmd, true, format!("Steam opened {name}. Finish the install on the device.")),
            Ok(rare_launch::steamdl::Started::NeedsEula(_)) => {
                st.toast(format!("{name} has a licence agreement. Accept it on its page to install."), Tone::Info, now);
                ack(&cmd, true, format!("{name} has a licence agreement. Accept it on the device."))
            }
            Err(e) => {
                st.feat.downloads.asked.retain(|(a, _)| *a != gid);
                st.feat.downloads.ver += 1;
                st.toast(e.clone(), Tone::Error, now);
                ack(&cmd, false, e);
            }
        })
    });
}

fn store_install(st: &mut State, cmd: &str, gid: &str, title: Option<String>, now: f64) {
    let Some((store, _)) = rare_stores::Store::of(gid) else { return ack(cmd, false, "That game id is not valid.") };
    let name = st.game(gid).map(|g| g.title.clone()).or(title).unwrap_or_else(|| gid.to_string());
    if st.game(gid).is_some_and(|g| g.installed) {
        return ack(cmd, true, format!("{name} is already installed"));
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let dir = rare_stores::drives::games_dir(&home, store);
    match rare_stores::jobs::add(gid, &name, rare_stores::jobs::Kind::Install { base: dir }) {
        Ok(()) => {
            st.toast(format!("Installing {name}, sent from your phone"), Tone::Info, now);
            ack(cmd, true, format!("{name} is in the downloads"));
        }
        Err(e) => ack(cmd, false, e),
    }
}

fn downloads(st: &mut State, fx: &mut Fx, c: &Cmd, now: f64) {
    let pause = c.data["action"].as_str() == Some("pause");
    let only = c.data["id"].as_str().map(str::to_string);
    let mut n = 0;
    let steam: Vec<(u32, bool)> = st.feat.downloads.snap.items.iter().map(|d| (d.appid, d.paused)).collect();
    for (appid, paused) in steam {
        let id = format!("steam:{appid}");
        if only.as_ref().is_some_and(|o| *o != id) || paused == pause {
            continue;
        }
        fx.run(Act::Download(appid, if pause { Ctl::Pause } else { Ctl::Resume }));
        n += 1;
    }
    for (id, _, _, _, paused) in crate::features::stores::downloads() {
        if only.as_ref().is_some_and(|o| *o != id) || paused == pause {
            continue;
        }
        crate::features::stores::act(st, &format!("stores:{}:{id}", if pause { "pause" } else { "resume" }), now);
        n += 1;
    }
    let verb = if pause { "Paused" } else { "Resumed" };
    if n == 0 {
        ack(&c.id, true, if pause { "Nothing to pause" } else { "Nothing to resume" });
    } else {
        ack(&c.id, true, format!("{verb} {n} download{}", if n == 1 { "" } else { "s" }));
    }
}

// ---------------------------------------------------------------- status

#[derive(Clone, Debug, Default, PartialEq)]
struct Snap {
    playing: Option<(String, String)>,
    /// (id, title, done, total, paused, rate)
    downloads: Vec<(String, String, u64, u64, bool, u64)>,
    finished: Vec<(String, String, i64)>,
}

struct Tracker {
    next: f64,
    last: HashMap<String, (String, u64, u64)>,
    finished: Vec<(String, String, i64)>,
}

static TRACK: Mutex<Option<Tracker>> = Mutex::new(None);

fn snapshot(st: &State, now: f64) {
    let mut g = TRACK.lock().unwrap();
    let t = g.get_or_insert_with(|| Tracker { next: 0.0, last: HashMap::new(), finished: Vec::new() });
    if now < t.next {
        return;
    }
    t.next = now + 2.0;
    let playing = st.running.first().map(|id| (id.clone(), st.run_name(id)));
    let mut dls: Vec<(String, String, u64, u64, bool, u64)> = st
        .feat
        .downloads
        .snap
        .items
        .iter()
        .map(|d| (format!("steam:{}", d.appid), crate::features::downloads::title_of(st, d), d.done, d.total, d.paused || !d.active, d.rate))
        .collect();
    for (id, title, done, total, paused) in crate::features::stores::downloads() {
        dls.push((id, title, done, total, paused, 0));
    }
    // A download that left the queue near its end finished (Steam drops it when done).
    let ids: HashSet<&str> = dls.iter().map(|d| d.0.as_str()).collect();
    let ms = now_ms();
    for (id, (title, done, total)) in &t.last {
        if !ids.contains(id.as_str()) && *total > 0 && *done * 100 >= *total * 95 {
            t.finished.push((id.clone(), title.clone(), ms));
        }
    }
    t.finished.retain(|f| ms - f.2 < 15 * 60_000);
    t.last = dls.iter().map(|d| (d.0.clone(), (d.1.clone(), d.2, d.3))).collect();
    let snap = Snap { playing, downloads: dls, finished: t.finished.clone() };
    let mut s = SNAP.lock().unwrap();
    if s.as_ref() != Some(&snap) {
        *s = Some(snap);
        drop(s);
        send(Msg::Wake);
    }
}

fn status_body(snap: &Snap, name: &str, model: &str) -> Value {
    let bat = rare_system::system::battery();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let space = rare_library::storage::space(&home);
    json!({
        "name": name,
        "model": model,
        "version": env!("CARGO_PKG_VERSION"),
        "battery": bat.map(|b| json!({ "percent": b.percent, "charging": b.charging })),
        "playing": snap.playing.as_ref().map(|(id, t)| json!({ "id": id, "title": t })),
        "storage": space.map(|(total, free)| json!({ "free": free, "total": total })),
        "downloads": snap.downloads.iter().take(20).map(|d| json!({ "id": d.0, "title": d.1, "done": d.2, "total": d.3, "paused": d.4, "rate": d.5 })).collect::<Vec<_>>(),
        "finished": snap.finished.iter().map(|f| json!({ "id": f.0, "title": f.1, "at": f.2 })).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------- the background thread

/// Starts phone control on a device run (never on fixtures; a live RR_SHOT run has it, for end to end checks).
pub fn boot() {
    if std::env::var_os("RR_FIXTURE").is_some() || std::env::var_os("RARE_NO_COMPANION").is_some() {
        return;
    }
    if !cfg!(target_os = "linux") && std::env::var_os("RR_DEVICE").is_none() {
        return;
    }
    let (tx, rx) = channel();
    if TX.set(Mutex::new(tx)).is_err() {
        return;
    }
    edit(|v| v.live = true);
    let _ = std::thread::Builder::new().name("rr-companion".into()).spawn(move || worker(rx));
}

type R<T> = Result<T, String>;

fn call(k: &DeviceKey, method: Method, path: &str, body: Option<&Value>) -> R<Value> {
    let net = crate::screens::wallet::live::market_net();
    rare_friends::api::send::<Value, Value>(net, Some(k), method, path, body, now_ms()).map_err(|e| e.to_string())
}

/// A file from the phone, raw (the shared net reads text).
fn fetch_file(k: &DeviceKey, file: &str) -> R<Vec<u8>> {
    let net = crate::screens::wallet::live::market_net();
    let origin = rare_market::net::Net::market_origin(net);
    let path = format!("/v1/devices/self/files/{file}");
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(120))).build().into();
    let mut q = agent.get(format!("{}{path}", origin.trim_end_matches('/')));
    for (n, v) in k.headers("GET", &path, "", now_ms()) {
        q = q.header(n, v);
    }
    q = q.header("user-agent", concat!("RareLauncher/", env!("CARGO_PKG_VERSION")));
    let mut r = q.call().map_err(|e| e.to_string())?;
    if r.status().as_u16() != 200 {
        let t = r.body_mut().read_to_string().unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&t).ok().and_then(|j| j["error"]["message"].as_str().map(str::to_string));
        return Err(msg.unwrap_or_else(|| format!("rare.market {}", r.status())));
    }
    r.body_mut().with_config().limit(21 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())
}

/// Saves a file from the phone: pictures into the Share gallery, anything else into ~/Downloads.
fn save_file(bytes: &[u8], name: &str, mime: &str) -> R<String> {
    let picture = mime.starts_with("image/") && !mime.contains("svg");
    if picture {
        if let Ok(c) = rare_share::import_picture(bytes) {
            let _ = c;
            return Ok(format!("Saved {name} to Share"));
        }
    }
    let dir = std::env::var_os("XDG_DOWNLOAD_DIR").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Downloads"))).ok_or("No Downloads folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = name.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let safe = safe.trim_start_matches('.').to_string();
    let safe = if safe.is_empty() { "file".to_string() } else { safe };
    let (stem, ext) = match safe.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.len() <= 10 => (s.to_string(), format!(".{e}")),
        _ => (safe.clone(), String::new()),
    };
    let mut path = dir.join(&safe);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(format!("Saved {} to Downloads", path.file_name().and_then(|f| f.to_str()).unwrap_or(name)))
}

fn worker(rx: Receiver<Msg>) {
    let info = rare_system::system::device_info();
    let host = info.hostname.trim().to_string();
    let name = if host.is_empty() || host.eq_ignore_ascii_case("localhost") { info.name.clone() } else { host };
    let name = if name.trim().is_empty() { "Rare device".to_string() } else { name };
    let model = info.name.clone();
    let mut key: Option<DeviceKey> = None;
    let mut paired = false;
    let mut last_self = Instant::now() - Duration::from_secs(3600);
    let mut last_status = Instant::now() - Duration::from_secs(3600);
    let mut last_sent: Option<Value> = None;
    let mut last_pull = Instant::now() - Duration::from_secs(3600);
    loop {
        let msg = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // The key in use: a change (sign-in, another account) asks rare.market again.
        let k = crate::social::device_key();
        let signed_in = crate::data::account::current().is_some();
        if k.as_ref().map(|k| (&k.key, &k.address)) != key.as_ref().map(|k| (&k.key, &k.address)) {
            key = k.clone();
            paired = false;
            last_self = Instant::now() - Duration::from_secs(3600);
            edit(|v| {
                v.paired = None;
                v.code = None;
                v.account = k.as_ref().map(|k| k.address.clone());
            });
        }
        edit(|v| {
            v.signed_in = signed_in;
            v.key = key.is_some();
        });
        let showing_code = view().code.is_some_and(|c| c.2 > now_ms());
        match msg {
            Some(Msg::Pair) => {
                let mut why = None;
                if key.is_none() && signed_in {
                    match crate::social::link_device_key() {
                        Ok(()) => {
                            key = crate::social::device_key();
                            // the stream needs the key's token to carry commands
                            crate::social::refresh_stream();
                        }
                        Err(e) => why = Some(e),
                    }
                }
                match &key {
                    None => edit(|v| {
                        v.busy = false;
                        v.error = Some(if signed_in { format!("This device could not sign in to rare.market: {}", why.unwrap_or_default()) } else { "Sign in with Rare Wallet first.".into() });
                    }),
                    Some(k) => match call(k, Method::Post, "/v1/devices/self/pair", Some(&json!({ "name": name, "model": model }))) {
                        Ok(j) => {
                            let code = (j["code"].as_str().unwrap_or("").to_string(), j["url"].as_str().unwrap_or("").to_string(), j["expiresAt"].as_i64().unwrap_or(0));
                            edit(|v| {
                                v.busy = false;
                                v.error = None;
                                v.code = Some(code);
                                v.key = true;
                            });
                        }
                        Err(e) => edit(|v| {
                            v.busy = false;
                            v.error = Some(e);
                        }),
                    },
                }
                continue;
            }
            Some(Msg::Unpair) => {
                if let Some(k) = &key {
                    let r = call(k, Method::Delete, "/v1/devices/self", None);
                    if r.is_ok() {
                        paired = false;
                    }
                    edit(|v| {
                        v.busy = false;
                        v.code = None;
                        v.error = r.err();
                        if v.error.is_none() {
                            v.paired = Some(false);
                        }
                    });
                }
                continue;
            }
            Some(Msg::Ack { id, ok, message }) => {
                if let Some(k) = &key {
                    if let Err(e) = call(k, Method::Post, &format!("/v1/devices/self/commands/{id}/ack"), Some(&json!({ "ok": ok, "message": message }))) {
                        log::warn!("phone ack {id}: {e}");
                    }
                }
            }
            Some(Msg::File { id, file, name, mime }) => {
                let r = key.as_ref().ok_or_else(|| "Not signed in".to_string()).and_then(|k| fetch_file(k, &file)).and_then(|b| save_file(&b, &name, &mime));
                let (ok, line) = match r {
                    Ok(l) => (true, l),
                    Err(e) => (false, format!("Could not save {name}: {e}")),
                };
                crate::social::toast_any(&line, !ok);
                ack(&id, ok, line);
            }
            Some(Msg::Type { id, text, enter }) => {
                let (ok, line) = match typing::type_text(&text, enter) {
                    Ok(skipped) if skipped > 0 => (true, format!("Typed it ({skipped} characters this keyboard cannot type were left out)")),
                    Ok(_) => (true, "Typed it into the game".to_string()),
                    Err(e) => (false, e),
                };
                ack(&id, ok, line);
            }
            Some(Msg::Wake) => last_self = Instant::now() - Duration::from_secs(3600),
            None => {}
        }
        let Some(k) = key.clone() else { continue };
        // Paired yet? Asked at start, after a stream event, and every 3 s while a code is on screen.
        let ask_every = if showing_code { Duration::from_secs(3) } else if paired { Duration::from_secs(600) } else { Duration::from_secs(120) };
        if last_self.elapsed() >= ask_every {
            last_self = Instant::now();
            match call(&k, Method::Get, "/v1/devices/self", None) {
                Ok(j) => {
                    let now_paired = j["paired"].as_bool().unwrap_or(false);
                    if now_paired && !paired {
                        note_recent("Paired with your phone".into(), true);
                        last_pull = Instant::now() - Duration::from_secs(3600);
                        last_status = Instant::now() - Duration::from_secs(3600);
                    }
                    paired = now_paired;
                    edit(|v| {
                        v.paired = Some(now_paired);
                        if now_paired {
                            v.code = None;
                        }
                    });
                }
                Err(e) => log::warn!("phone: {e}"),
            }
        }
        if !paired {
            continue;
        }
        // What waited for this device.
        if last_pull.elapsed() >= Duration::from_secs(30) {
            last_pull = Instant::now();
            match call(&k, Method::Get, "/v1/devices/self/commands", None) {
                Ok(j) => {
                    for c in j["data"].as_array().cloned().unwrap_or_default() {
                        if let Some(c) = parse_cmd(&c) {
                            enqueue(c);
                        }
                    }
                }
                Err(e) if e.contains("not paired") => {
                    paired = false;
                    edit(|v| v.paired = Some(false));
                }
                Err(e) => log::warn!("phone commands: {e}"),
            }
        }
        // The status: every 30 s, or 5 s after a change.
        let snap = SNAP.lock().unwrap().clone().unwrap_or_default();
        let body = status_body(&snap, &name, &model);
        let changed = last_sent.as_ref().map(|l| l["downloads"] != body["downloads"] || l["playing"] != body["playing"] || l["finished"] != body["finished"]).unwrap_or(true);
        let due = last_status.elapsed() >= Duration::from_secs(30) || (changed && last_status.elapsed() >= Duration::from_secs(5));
        if due {
            last_status = Instant::now();
            match call(&k, Method::Post, "/v1/devices/self/status", Some(&body)) {
                Ok(_) => last_sent = Some(body),
                Err(e) if e.contains("not paired") => {
                    paired = false;
                    edit(|v| v.paired = Some(false));
                }
                Err(e) => log::warn!("phone status: {e}"),
            }
        }
    }
}

/// Typing into a game from the phone: a virtual keyboard of Rare's own on /dev/uinput, US layout.
mod typing {
    use std::time::Duration;

    const SHIFT: u16 = 42;
    const ENTER: u16 = 28;

    /// The key and whether Shift is held, for one character.
    pub fn key_of(c: char) -> Option<(u16, bool)> {
        const LOWER: &str = "qwertyuiop";
        const MID: &str = "asdfghjkl";
        const LOW: &str = "zxcvbnm";
        let l = c.to_ascii_lowercase();
        if c.is_ascii_alphabetic() {
            let code = LOWER.find(l).map(|i| 16 + i as u16).or_else(|| MID.find(l).map(|i| 30 + i as u16)).or_else(|| LOW.find(l).map(|i| 44 + i as u16))?;
            return Some((code, c.is_ascii_uppercase()));
        }
        if let Some(d) = c.to_digit(10) {
            return Some((if d == 0 { 11 } else { 1 + d as u16 }, false));
        }
        let plain = [(' ', 57), ('-', 12), ('=', 13), ('[', 26), (']', 27), (';', 39), ('\'', 40), ('`', 41), ('\\', 43), (',', 51), ('.', 52), ('/', 53), ('\n', 28), ('\t', 15)];
        if let Some((_, k)) = plain.iter().find(|(ch, _)| *ch == c) {
            return Some((*k, false));
        }
        let shifted = [('!', 2), ('@', 3), ('#', 4), ('$', 5), ('%', 6), ('^', 7), ('&', 8), ('*', 9), ('(', 10), (')', 11), ('_', 12), ('+', 13), ('{', 26), ('}', 27), (':', 39), ('"', 40), ('~', 41), ('|', 43), ('<', 51), ('>', 52), ('?', 53)];
        shifted.iter().find(|(ch, _)| *ch == c).map(|(_, k)| (*k, true))
    }

    /// Types `text` (and Enter). Returns how many characters had no key.
    pub fn type_text(text: &str, enter: bool) -> Result<usize, String> {
        let mut keys: Vec<u16> = (1..=57).collect();
        keys.push(SHIFT);
        keys.dedup();
        let mut d = rare_launch::apps::pointer::uinput::Device::open_keys("Rare phone keyboard", &keys).map_err(|e| format!("No virtual keyboard on this device ({e})"))?;
        // The compositor needs a moment to see a new input device.
        std::thread::sleep(Duration::from_millis(350));
        let mut skipped = 0;
        let mut tap = |code: u16, shift: bool| {
            if shift {
                d.key(SHIFT, true);
                d.syn();
            }
            d.key(code, true);
            d.syn();
            std::thread::sleep(Duration::from_millis(6));
            d.key(code, false);
            d.syn();
            if shift {
                d.key(SHIFT, false);
                d.syn();
            }
            std::thread::sleep(Duration::from_millis(6));
        };
        for c in text.chars() {
            match key_of(c) {
                Some((k, s)) => tap(k, s),
                None => skipped += 1,
            }
        }
        if enter {
            tap(ENTER, false);
        }
        std::thread::sleep(Duration::from_millis(100));
        Ok(skipped)
    }

    #[cfg(test)]
    mod tests {
        use super::key_of;

        #[test]
        fn us_layout_keys() {
            assert_eq!(key_of('a'), Some((30, false)));
            assert_eq!(key_of('Q'), Some((16, true)));
            assert_eq!(key_of('m'), Some((50, false)));
            assert_eq!(key_of('1'), Some((2, false)));
            assert_eq!(key_of('0'), Some((11, false)));
            assert_eq!(key_of('!'), Some((2, true)));
            assert_eq!(key_of(' '), Some((57, false)));
            assert_eq!(key_of('?'), Some((53, true)));
            assert_eq!(key_of('é'), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_commands_run_once() {
        let c = json!({ "kind": "command", "command": { "id": "t-77", "kind": "text", "args": { "text": "hi" }, "at": 1 } });
        on_stream(&c);
        on_stream(&c);
        let q: Vec<Cmd> = QUEUE.lock().unwrap().drain(..).filter(|c| c.id == "t-77").collect();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].data["text"], "hi");
        let i = parse_cmd(&json!({ "id": "9", "kind": "install", "args": { "store": "rare", "id": "act" } })).unwrap();
        assert_eq!((i.id.as_str(), i.data["id"].as_str()), ("9", Some("act")));
    }

    #[test]
    fn short_urls() {
        assert_eq!(short_url("https://www.example.com/a?b"), "example.com");
        assert_eq!(short_url("https://play.actuniverse.com"), "play.actuniverse.com");
    }
}
