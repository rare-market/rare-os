//! Discord on Rare.
//!
//! - Rich Presence: friends see "Playing <game>" for whatever runs on Rare. With Discord on the
//!   web in Rare's window, Rare runs arRPC (arrpc.rs) and reports to it like a game would; the
//!   page gets the activity through arRPC's bridge (`BRIDGE_JS`, injected by rare-launch). With
//!   the Discord app (Flatpak), Rare reports to the app's own socket, for games Discord knows
//!   (an activity there shows the application's name, so it needs the game's own id), and links
//!   $XDG_RUNTIME_DIR/discord-ipc-0 to the app's socket so games with Discord's SDK reach it.
//! - Voice: Discord keeps running while a game plays (apps are never paused, rare-system
//!   freeze.rs); mute and deafen from the Rare menu work on its PipeWire streams (voice.rs).
//!
//! Rare has no Discord account or application of its own: activities use the game's id from
//! Discord's list of detectable games, else arRPC's own application.

pub mod arrpc;
pub mod ipc;
pub mod voice;

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The page side of arRPC's bridge, for Discord on the web.
pub const BRIDGE_JS: &str = include_str!("bridge.js");
/// arRPC's own application: the activity's id when Discord has no entry for the game.
pub const ARRPC_APP_ID: &str = "1045800378228281345";
pub const FLATPAK: &str = "com.discordapp.Discord";
/// The run id of the Discord app tile (catalog "discord").
pub const RUN_ID: &str = "app:discord";

/// What Discord needs from the launcher.
pub trait DiscordHost: Send + Sync {
    /// "Show what I'm playing on Discord" is on.
    fn presence_enabled(&self) -> bool;
    /// What runs now, the one in front first: (run id, title).
    fn running(&self) -> Vec<(String, String)>;
    /// The user's session environment for programs Rare starts.
    fn env(&self, cmd: &mut Command);
}

/// Which Discord is open.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    App,
    Web,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub client: Option<Client>,
    /// arRPC is on this device (needed for Discord on the web).
    pub arrpc_available: bool,
    pub arrpc_running: bool,
    /// The game friends see now.
    pub showing: Option<String>,
    pub muted: bool,
    pub deafened: bool,
    /// Discord has a voice stream now (in a call).
    pub in_voice: bool,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---- which Discord ----

/// The Discord app's socket inside its Flatpak runtime dir.
pub fn app_socket(runtime: &Path) -> PathBuf {
    runtime.join("app").join(FLATPAK).join("discord-ipc-0")
}

/// The Discord app runs (its process, by name).
pub fn app_running() -> bool {
    let Ok(rd) = std::fs::read_dir("/proc") else { return false };
    rd.flatten().any(|e| {
        e.file_name().to_str().is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
            && std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| matches!(c.trim(), "Discord" | "DiscordCanary" | "DiscordPTB"))
    })
}

/// Which Discord is open: the app when its process runs, else the web one when the Discord tile
/// runs in Rare's window.
pub fn client_of(app: bool, running: &[(String, String)]) -> Option<Client> {
    if app {
        Some(Client::App)
    } else if running.iter().any(|(id, _)| id == RUN_ID) {
        Some(Client::Web)
    } else {
        None
    }
}

/// What to show: the first running thing that is a game (apps and streams from a PC are not).
pub fn game_of(running: &[(String, String)]) -> Option<(String, String)> {
    running.iter().find(|(id, t)| !id.starts_with("app:") && !id.starts_with("stream:") && !t.trim().is_empty()).cloned()
}

// ---- Discord's detectable games ----

/// "Hades II™" -> "hades ii".
pub fn norm(name: &str) -> String {
    let s: String = name.chars().filter(|c| !matches!(c, '™' | '®' | '©')).flat_map(|c| c.to_lowercase()).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Name to application id from Discord's detectable list (arRPC's detectable.json, or the
/// copy Discord serves).
pub fn parse_detectable(json: &str) -> HashMap<String, String> {
    let Ok(Value::Array(a)) = serde_json::from_str::<Value>(json) else { return HashMap::new() };
    let mut m = HashMap::with_capacity(a.len());
    for e in &a {
        if let (Some(id), Some(name)) = (e["id"].as_str(), e["name"].as_str()) {
            m.entry(norm(name)).or_insert_with(|| id.to_string());
        }
        for alias in e["aliases"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
            if let Some(id) = e["id"].as_str() {
                m.entry(norm(alias)).or_insert_with(|| id.to_string());
            }
        }
    }
    m
}

fn detectable_cache() -> PathBuf {
    rare_core::paths::cache_dir().join("discord-detectable.json")
}

/// The list, loaded once on first use (arRPC's copy, else a cached download).
fn detectable() -> &'static HashMap<String, String> {
    static D: OnceLock<HashMap<String, String>> = OnceLock::new();
    D.get_or_init(|| {
        let text = arrpc::find().and_then(|rt| std::fs::read_to_string(rt.detectable).ok()).or_else(|| std::fs::read_to_string(detectable_cache()).ok()).or_else(|| {
            // Discord's public list (no account): fetched once when the Discord app is in use.
            let p = detectable_cache();
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let ok = Command::new("curl").args(["-fsSL", "--max-time", "30", "-o"]).arg(&p).arg("https://discord.com/api/v9/applications/detectable").status().is_ok_and(|s| s.success());
            ok.then(|| std::fs::read_to_string(&p).ok()).flatten()
        });
        let m = text.map(|t| parse_detectable(&t)).unwrap_or_default();
        eprintln!("[discord] {} detectable games known", m.len());
        m
    })
}

/// The application id an activity for `title` uses: the game's own, else arRPC's (`known` false).
pub fn app_id_for(title: &str, list: &HashMap<String, String>) -> (String, bool) {
    match list.get(&norm(title)) {
        Some(id) => (id.clone(), true),
        None => (ARRPC_APP_ID.to_string(), false),
    }
}

/// The activity Rare sets for a game.
pub fn activity(title: &str, start_ms: u64) -> Value {
    json!({ "name": title, "type": 0, "timestamps": { "start": start_ms } })
}

// ---- the supervisor ----

struct Conn {
    client: ipc::Client,
    /// (title, start) last set.
    sent: Option<(String, u64)>,
}

#[derive(Default)]
struct State {
    status: Status,
    /// When each game was first seen running (its activity's start).
    started: HashMap<String, u64>,
    conns: HashMap<PathBuf, Conn>,
    /// The mute switches were used this session (until then Rare leaves Discord's streams alone).
    voice_touched: bool,
    linked: bool,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

pub fn status() -> Status {
    state().lock().unwrap().status.clone()
}

/// Mute or unmute Discord's microphone.
pub fn set_muted(on: bool) {
    let mut s = state().lock().unwrap();
    s.status.muted = on;
    s.voice_touched = true;
    apply_voice(&s.status);
}

/// Deafen or undeafen (Discord's sound and microphone).
pub fn set_deafened(on: bool) {
    let mut s = state().lock().unwrap();
    s.status.deafened = on;
    s.voice_touched = true;
    apply_voice(&s.status);
}

fn apply_voice(st: &Status) {
    let who = match st.client {
        Some(Client::App) => voice::Who::App,
        Some(Client::Web) => voice::Who::Web,
        None => return,
    };
    voice::apply(who, st.muted, st.deafened);
}

/// $XDG_RUNTIME_DIR/discord-ipc-0 points at the Discord app's socket, so games with Discord's SDK
/// reach the app. Only made where nothing else is; only removed when it is that link.
fn link_app_socket(runtime: &Path, on: bool) -> bool {
    let link = runtime.join("discord-ipc-0");
    let target = app_socket(runtime);
    let ours = std::fs::read_link(&link).is_ok_and(|t| t == target);
    if on {
        if ours {
            return true;
        }
        if std::fs::symlink_metadata(&link).is_ok() {
            // A real socket (another Discord, arRPC) or someone else's link: leave it.
            return false;
        }
        std::os::unix::fs::symlink(&target, &link).is_ok()
    } else {
        if ours {
            let _ = std::fs::remove_file(&link);
        }
        false
    }
}

fn report(st: &mut State, path: &Path, client_id: &str, game: Option<&(String, u64)>) {
    let pid = std::process::id();
    let stale = st.conns.get(path).is_some_and(|c| c.client.client_id != client_id);
    if stale || game.is_none() {
        // Dropping the connection clears its activity.
        st.conns.remove(path);
    }
    let Some((title, start)) = game else { return };
    if st.conns.get(path).is_some_and(|c| c.sent.as_ref() == Some(&(title.clone(), *start))) {
        return;
    }
    if !st.conns.contains_key(path) {
        match ipc::Client::connect(path, client_id) {
            Ok(c) => {
                st.conns.insert(path.to_path_buf(), Conn { client: c, sent: None });
            }
            Err(e) => {
                eprintln!("[discord] connect {}: {e}", path.display());
                return;
            }
        }
    }
    let Some(c) = st.conns.get_mut(path) else { return };
    match c.client.set_activity(pid, Some(&activity(title, *start))) {
        Ok(()) => {
            eprintln!("[discord] showing {title} ({client_id})");
            c.sent = Some((title.clone(), *start));
        }
        Err(e) => {
            eprintln!("[discord] set activity: {e}");
            st.conns.remove(path);
        }
    }
}

fn tick(host: &dyn DiscordHost) {
    let on = host.presence_enabled();
    let running = host.running();
    let client = client_of(app_running(), &running);
    arrpc::tick(on && client == Some(Client::Web), &|c| host.env(c));
    let runtime = ipc::runtime_dir();
    // The game and when it started (kept while it runs).
    let game = {
        let mut st = state().lock().unwrap();
        let ids: Vec<String> = running.iter().map(|r| r.0.clone()).collect();
        st.started.retain(|id, _| ids.contains(id));
        game_of(&running).map(|(id, title)| {
            let start = *st.started.entry(id).or_insert_with(now_ms);
            (title, start)
        })
    };
    let want = if on && client.is_some() { game } else { None };
    // Which application the activity uses (the list loads once, outside the lock).
    let (app_id, known) = match &want {
        Some((title, _)) => app_id_for(title, detectable()),
        None => (ARRPC_APP_ID.to_string(), false),
    };
    // The app shows the application's own name, so only games Discord knows go there.
    let want = want.filter(|_| known || client == Some(Client::Web));
    let targets: Vec<PathBuf> = match client {
        Some(Client::Web) => arrpc::socket().into_iter().collect(),
        Some(Client::App) => runtime.as_deref().map(app_socket).into_iter().collect(),
        None => vec![],
    };
    let mut st = state().lock().unwrap();
    st.status.client = client;
    st.status.arrpc_available = arrpc::find().is_some();
    st.status.arrpc_running = arrpc::running();
    if let Some(rt) = runtime.as_deref() {
        let link = on && client == Some(Client::App);
        if link != st.linked {
            st.linked = link_app_socket(rt, link);
        }
    }
    // Connections to sockets that are no longer the target go (clearing what they set).
    st.conns.retain(|p, _| targets.contains(p));
    let mut showing = None;
    for t in &targets {
        report(&mut st, t, &app_id, want.as_ref());
        if want.is_some() && st.conns.get(t).is_some_and(|c| c.sent.is_some()) {
            showing = want.as_ref().map(|g| g.0.clone());
        }
    }
    st.status.showing = showing;
    // Voice: Discord's streams follow the switches once they were used.
    match client {
        Some(c) => {
            let who = if c == Client::App { voice::Who::App } else { voice::Who::Web };
            let (_, mic) = voice::streams(who);
            st.status.in_voice = !mic.is_empty();
            if st.voice_touched {
                voice::apply(who, st.status.muted, st.status.deafened);
            }
        }
        None => st.status.in_voice = false,
    }
}

/// Starts the thread that keeps presence, arRPC and the voice switches in step (every 2 s).
pub fn start(host: Arc<dyn DiscordHost>) {
    std::thread::spawn(move || loop {
        tick(&*host);
        std::thread::sleep(Duration::from_secs(2));
    });
}

/// Launcher exit: clear what Rare set and stop arRPC.
pub fn shutdown() {
    if let Ok(mut s) = state().lock() {
        s.conns.clear();
        if s.linked {
            if let Some(rt) = ipc::runtime_dir() {
                link_app_socket(&rt, false);
            }
        }
    }
    arrpc::shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, t: &str) -> (String, String) {
        (id.into(), t.into())
    }

    #[test]
    fn which_discord_and_which_game() {
        assert_eq!(client_of(true, &[]), Some(Client::App));
        assert_eq!(client_of(false, &[r("app:discord", "Discord")]), Some(Client::Web));
        assert_eq!(client_of(false, &[r("steam:1", "X")]), None);
        let running = vec![r("app:discord", "Discord"), r("stream:abc:1", "Desktop"), r("steam:1145350", "Hades II"), r("emu:snes:1", "Chrono Trigger")];
        assert_eq!(game_of(&running), Some(r("steam:1145350", "Hades II")));
        assert_eq!(game_of(&running[..2]), None);
    }

    #[test]
    fn detectable_names() {
        let list = parse_detectable(r#"[{"id":"1","name":"Hades II"},{"id":"2","name":"Celeste™","aliases":["Celeste Classic"]},{"id":"3","name":"hades ii"},{"name":"no id"}]"#);
        assert_eq!(app_id_for("Hades  II", &list), ("1".into(), true));
        assert_eq!(app_id_for("celeste", &list), ("2".into(), true));
        assert_eq!(app_id_for("Celeste Classic", &list), ("2".into(), true));
        assert_eq!(app_id_for("Chrono Trigger", &list), (ARRPC_APP_ID.into(), false));
        assert!(parse_detectable("{}").is_empty());
        assert_eq!(activity("Hades II", 5)["timestamps"]["start"], 5);
        assert_eq!(activity("Hades II", 5)["name"], "Hades II");
    }

    #[test]
    fn links_only_its_own() {
        let d = std::env::temp_dir().join(format!("rare-discord-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(link_app_socket(&d, true));
        assert_eq!(std::fs::read_link(d.join("discord-ipc-0")).unwrap(), app_socket(&d));
        assert!(link_app_socket(&d, true));
        assert!(!link_app_socket(&d, false));
        assert!(std::fs::symlink_metadata(d.join("discord-ipc-0")).is_err());
        // Someone else's file stays.
        std::fs::write(d.join("discord-ipc-0"), "").unwrap();
        assert!(!link_app_socket(&d, true));
        link_app_socket(&d, false);
        assert!(d.join("discord-ipc-0").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_bridge_is_arrpcs_with_rares_start() {
        assert!(BRIDGE_JS.contains("ws://127.0.0.1:1337"));
        assert!(BRIDGE_JS.contains("LOCAL_ACTIVITY_UPDATE"));
        assert!(BRIDGE_JS.contains("MIT License"));
        assert!(BRIDGE_JS.contains("rareWait();"));
    }
}
