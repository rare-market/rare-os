//! Capture and streaming on the device (Settings > Capture and streaming, Quick settings, the Rare
//! menu): instant replay's settings, Discord (presence through arRPC or the app, mute and deafen),
//! streaming this device with Sunshine, and Rare devices nearby.
//!
//! The settings live in launcher.json (`shareClips`, `replaySeconds`, `clipsCapGb`,
//! `discordPresence`, `streamHost`), read at the moment they are needed. The supervisors (rare-share's
//! recorder, rare-discord, the Sunshine host and the mDNS announcement) start once at launch, never
//! in fixtures or shots. What the settings screen shows is one snapshot (`info`), refreshed on a
//! worker; a change bumps the settings version so the open section rebuilds.

use rare_discord::Status as DiscordStatus;
use rare_stream::host::{Client, HostStatus};
use rare_stream::nearby::Nearby;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub use rare_share::clip::LENGTHS;
/// The space clips may take, in GB (the choices; 5 when unset).
pub const CAPS_GB: [u32; 4] = [2, 5, 10, 20];
const CAP_DEFAULT: u32 = 5;

// ---- the settings ----

fn cfg(key: &str) -> Option<Value> {
    crate::features::config::get(key)
}

/// Instant replay is on (off until turned on).
pub fn replay_on() -> bool {
    cfg("shareClips").and_then(|v| v.as_bool()).unwrap_or(false)
}

pub fn replay_seconds() -> u32 {
    rare_share::clip::sane_seconds(cfg("replaySeconds").and_then(|v| v.as_u64()).map(|s| s as u32).unwrap_or(rare_share::clip::SECONDS))
}

pub fn cap_gb() -> u32 {
    cfg("clipsCapGb").and_then(|v| v.as_u64()).map(|g| g as u32).filter(|g| CAPS_GB.contains(g)).unwrap_or(CAP_DEFAULT)
}

pub fn presence_on() -> bool {
    cfg("discordPresence").and_then(|v| v.as_bool()).unwrap_or(true)
}

pub fn host_on() -> bool {
    cfg("streamHost").and_then(|v| v.as_bool()).unwrap_or(false)
}

/// "30 s", "1 min", "2 min".
pub fn length_label(secs: u32) -> String {
    if secs >= 60 && secs % 60 == 0 {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} s")
    }
}

/// The Save clip button: "Save last 30 s".
pub fn save_label(secs: u32) -> String {
    format!("Save last {}", length_label(secs))
}

/// "the last 30 seconds", "the last 2 minutes".
pub fn last_words(secs: u32) -> String {
    match secs {
        s if s >= 120 && s % 60 == 0 => format!("the last {} minutes", s / 60),
        60 => "the last minute".into(),
        s => format!("the last {s} seconds"),
    }
}

// ---- what the device is called on the network ----

/// The model, never anything about the owner: "SuiPlay" on a SuiPlay0X1, else "Rare device".
pub fn device_name() -> String {
    let product = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
    name_for_product(product.trim())
}

pub fn name_for_product(p: &str) -> String {
    match p {
        "SuiPlay0X1" => "SuiPlay".into(),
        "Jupiter" | "Galileo" => "Steam Deck".into(),
        _ => "Rare device".into(),
    }
}

/// This device's address on the local network (no packet is sent: a UDP socket only picks the route).
pub fn local_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.0.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

fn gamescope() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() || std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.eq_ignore_ascii_case("gamescope"))
}

// ---- the snapshot ----

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Info {
    pub replay_on: bool,
    pub secs: u32,
    pub cap_gb: u32,
    pub used_bytes: u64,
    /// The recorder can run here (gpu-screen-recorder, or GStreamer with VA-API).
    pub replay_available: bool,
    pub recording: bool,
    pub method: Option<String>,
    pub presence_on: bool,
    pub discord: DiscordStatus,
    pub host_on: bool,
    pub host: HostStatus,
    pub clients: Vec<Client>,
    pub name: String,
    pub ip: Option<String>,
    pub nearby: Vec<Nearby>,
    /// A network look finished at least once.
    pub nearby_done: bool,
}

static INFO: Mutex<Option<Info>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(1);
static STARTED: AtomicBool = AtomicBool::new(false);
static FIXTURE: Mutex<Option<Info>> = Mutex::new(None);

pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

fn bump() {
    VERSION.fetch_add(1, Ordering::Relaxed);
    crate::screens::settings::live::bump_pub();
    if let Some(w) = WAKER.get() {
        w();
    }
}

static WAKER: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();

/// The values the settings section draws (a fixture's when one is set, else the last reading;
/// the settings themselves are read now).
pub fn info() -> Info {
    if let Some(f) = FIXTURE.lock().unwrap().clone() {
        return f;
    }
    let mut i = INFO.lock().unwrap().clone().unwrap_or_else(|| Info { name: device_name(), ..Default::default() });
    i.replay_on = replay_on();
    i.secs = replay_seconds();
    i.cap_gb = cap_gb();
    i.presence_on = presence_on();
    i.host_on = host_on();
    i
}

/// Reads everything again on a worker (the clip folder's size, the recorder, Sunshine's paired
/// devices, the network when `look`).
pub fn refresh(look: bool) {
    if FIXTURE.lock().unwrap().is_some() {
        return;
    }
    std::thread::spawn(move || {
        let st = rare_share::status(&*crate::side::share_host());
        let mut i = info();
        i.used_bytes = rare_share::clips_bytes();
        i.replay_available = st.clips.available;
        i.recording = st.clips.recording;
        i.method = st.clips.method;
        i.discord = rare_discord::status();
        i.host = rare_stream::host::status();
        i.clients = if i.host.running && i.host.up_s >= 2 { rare_stream::host::clients().unwrap_or_else(|_| i.clients.clone()) } else { vec![] };
        i.name = device_name();
        i.ip = local_ip();
        *INFO.lock().unwrap() = Some(i.clone());
        bump();
        if look {
            let found = rare_stream::nearby::browse(Duration::from_millis(2500));
            if let Some(cur) = INFO.lock().unwrap().as_mut() {
                cur.nearby = found;
                cur.nearby_done = true;
            }
            bump();
        }
    });
}

// ---- the supervisors ----

struct Discord;

impl rare_discord::DiscordHost for Discord {
    fn presence_enabled(&self) -> bool {
        presence_on()
    }
    fn running(&self) -> Vec<(String, String)> {
        crate::side::running_titles()
    }
    fn env(&self, cmd: &mut std::process::Command) {
        rare_system::host::env(cmd);
    }
}

/// Starts Discord's supervisor, the Sunshine host and the network announcement (once).
pub fn start(waker: Arc<dyn Fn() + Send + Sync>) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = WAKER.set(waker);
    rare_discord::start(Arc::new(Discord));
    std::thread::spawn(|| {
        rare_stream::host::cleanup_stale();
        let name = device_name();
        let gs = gamescope();
        let env = |c: &mut std::process::Command| {
            rare_system::host::env(c);
        };
        let mut last = (false, false, false);
        loop {
            let want = host_on();
            rare_stream::host::tick(want, &name, gs, &env);
            rare_stream::host::poll_pending();
            let st = rare_stream::host::status();
            rare_stream::nearby::announce(&name, st.running);
            // A device asked to pair: say where to type its PIN (once per request).
            if st.pin_wanted && !last.2 {
                crate::side::toast("A device wants to pair. Type its PIN in Settings, Capture and streaming.", false);
            }
            let now = (want, st.running, st.pin_wanted);
            if now != last {
                last = now;
                refresh(false);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// Launcher exit.
pub fn shutdown() {
    rare_discord::shutdown();
    rare_stream::host::shutdown();
    rare_stream::nearby::stop();
}

// ---- actions (Settings, Quick settings, the Rare menu) ----

fn set(key: &str, v: Value) {
    crate::features::config::set(&[(key, v)]);
}

/// Turns instant replay on or off. Returns the message to show.
pub fn set_replay(on: bool) -> String {
    set("shareClips", on.into());
    refresh(false);
    if on {
        format!("Instant replay is on. It keeps {} while a game runs.", last_words(replay_seconds()))
    } else {
        "Instant replay is off".into()
    }
}

/// What an action string from these screens does: None when it is not ours, else the message
/// for a toast (empty: none) and whether it is an error.
pub fn act(a: &str, arg: Option<String>) -> Option<(String, bool)> {
    let ok = |s: &str| Some((s.to_string(), false));
    if let Some(v) = a.strip_prefix("capture:replay:") {
        return Some((set_replay(v == "true"), false));
    }
    if let Some(v) = a.strip_prefix("capture:length:") {
        let secs = rare_share::clip::sane_seconds(v.parse().unwrap_or(30));
        set("replaySeconds", secs.into());
        refresh(false);
        return ok("");
    }
    if let Some(v) = a.strip_prefix("capture:cap:") {
        let gb = v.parse::<u32>().ok().filter(|g| CAPS_GB.contains(g)).unwrap_or(CAP_DEFAULT);
        set("clipsCapGb", gb.into());
        std::thread::spawn(move || {
            let gone = rare_share::prune_clips((gb as u64) << 30, "");
            if !gone.is_empty() {
                crate::side::toast(&format!("Deleted {} older clip{} to fit {gb} GB", gone.len(), if gone.len() == 1 { "" } else { "s" }), false);
            }
            refresh(false);
        });
        return ok("");
    }
    if let Some(v) = a.strip_prefix("discord:presence:") {
        set("discordPresence", (v == "true").into());
        refresh(false);
        return ok("");
    }
    match a {
        "discord:mute" | "discord:deafen" => {
            let st = rare_discord::status();
            let msg = if a == "discord:mute" {
                rare_discord::set_muted(!st.muted);
                if st.muted { "Discord microphone on" } else { "Discord microphone muted" }
            } else {
                rare_discord::set_deafened(!st.deafened);
                if st.deafened { "Discord sound on" } else { "Discord deafened" }
            };
            refresh(false);
            return ok(msg);
        }
        "nearby:scan" => {
            refresh(true);
            return ok("Looking for Rare devices");
        }
        _ => {}
    }
    if let Some(v) = a.strip_prefix("streamhost:toggle:") {
        let on = v == "true";
        if on && rare_stream::host::find().is_none() {
            // Draws the switch off again.
            refresh(false);
            return Some(("Streaming this device needs Sunshine, which comes with the next Rare OS update.".into(), true));
        }
        set("streamHost", on.into());
        refresh(false);
        return ok(if on { "Starting streaming" } else { "Streaming is off" });
    }
    if a == "streamhost:pin" {
        let pin = arg.unwrap_or_default();
        std::thread::spawn(move || {
            match rare_stream::host::submit_pin(&pin, "Moonlight") {
                Ok(()) => crate::side::toast("Paired. In Moonlight, pick Rare to play.", false),
                Err(e) => crate::side::toast(&e, true),
            }
            std::thread::sleep(Duration::from_millis(800));
            refresh(false);
        });
        return ok("");
    }
    if let Some(uuid) = a.strip_prefix("streamhost:unpair:") {
        let uuid = uuid.to_string();
        std::thread::spawn(move || {
            match rare_stream::host::unpair(&uuid) {
                Ok(()) => crate::side::toast("Removed. It has to pair again to stream.", false),
                Err(e) => crate::side::toast(&e, true),
            }
            refresh(false);
        });
        return ok("");
    }
    None
}

// ---- fixtures (shots) ----

/// What a shot shows: RR_FIXTURE_CAPTURE = off | on | many | unavailable (default "on").
pub fn fixture(kind: &str) -> Info {
    let n = |name: &str, id: &str, ip: &str, stream: bool| Nearby { id: id.into(), name: name.into(), address: ip.into(), stream };
    let base = Info {
        replay_on: true,
        secs: 30,
        cap_gb: 5,
        used_bytes: 1_340_000_000,
        replay_available: true,
        recording: false,
        method: Some("GStreamer".into()),
        presence_on: true,
        discord: DiscordStatus { client: Some(rare_discord::Client::App), arrpc_available: true, showing: Some("Hades II".into()), in_voice: true, ..Default::default() },
        host_on: true,
        host: HostStatus { available: true, running: true, up_s: 40, ..Default::default() },
        clients: vec![Client { name: "Living room TV".into(), uuid: "A1".into() }],
        name: "SuiPlay".into(),
        ip: Some("192.168.1.92".into()),
        nearby: vec![n("SuiPlay", "0a1b2c3d4e5f", "192.168.0.41", true)],
        nearby_done: true,
    };
    match kind {
        "off" => Info {
            replay_on: false,
            used_bytes: 0,
            presence_on: true,
            discord: DiscordStatus::default(),
            host_on: false,
            host: HostStatus { available: true, ..Default::default() },
            clients: vec![],
            nearby: vec![],
            ..base
        },
        "unavailable" => Info {
            replay_on: false,
            replay_available: false,
            method: None,
            discord: DiscordStatus { client: Some(rare_discord::Client::Web), arrpc_available: false, ..Default::default() },
            host_on: false,
            host: HostStatus::default(),
            clients: vec![],
            nearby: vec![],
            ..base
        },
        "many" => Info {
            secs: 120,
            cap_gb: 20,
            used_bytes: 19_800_000_000,
            recording: true,
            discord: DiscordStatus { client: Some(rare_discord::Client::Web), arrpc_available: true, arrpc_running: true, showing: Some("The Elder Scrolls Anthology: Definitive Collection of Every Remaster Edition".into()), muted: true, deafened: true, in_voice: true },
            host: HostStatus { available: true, running: true, up_s: 400, pin_wanted: true, ..Default::default() },
            clients: vec![
                Client { name: "Living room TV".into(), uuid: "A1".into() },
                Client { name: "Pixel 9 Pro".into(), uuid: "A2".into() },
                Client { name: "A very long device name that someone typed into Moonlight".into(), uuid: "A3".into() },
                Client { name: String::new(), uuid: "A4".into() },
            ],
            nearby: vec![n("SuiPlay", "0a1b2c3d4e5f", "192.168.0.41", true), n("Steam Deck", "1a1b2c3d4e5f", "192.168.0.57", false), n("Rare device", "2a1b2c3d4e5f", "10.20.30.140", false)],
            ..base
        },
        _ => base,
    }
}

/// Shots: draw the fixture instead of the device.
pub fn use_fixture(kind: &str) {
    *FIXTURE.lock().unwrap() = Some(fixture(kind));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(length_label(15), "15 s");
        assert_eq!(length_label(60), "1 min");
        assert_eq!(length_label(120), "2 min");
        assert_eq!(save_label(30), "Save last 30 s");
        assert_eq!(last_words(30), "the last 30 seconds");
        assert_eq!(last_words(60), "the last minute");
        assert_eq!(last_words(120), "the last 2 minutes");
        assert_eq!(name_for_product("SuiPlay0X1"), "SuiPlay");
        assert_eq!(name_for_product("ROG Ally"), "Rare device");
    }

    #[test]
    fn fixtures_are_distinct() {
        assert!(!fixture("off").replay_on && fixture("on").replay_on);
        assert!(!fixture("unavailable").host.available);
        assert_eq!(fixture("many").clients.len(), 4);
    }
}
