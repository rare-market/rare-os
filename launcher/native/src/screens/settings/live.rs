//! Settings on the device's real state. The sections draw from a `Snapshot`; on a fixture run they
//! draw their built-in fixtures, so golden images do not change. On the device the first paint uses
//! the values the last run read (kept on disk), never fixtures, until the live reading lands.
//! Readings and actions run on worker threads (they shell out: nmcli, bluetoothctl, rpm-ostree,
//! wpctl) and write the answer here; the app sees `version()` move and rebuilds the open section.
//!
//! Everything goes through rare-system (the same calls the Tauri launcher makes) and the launcher
//! config file (`config.rs`), so the web launcher and this one agree.

use super::about::DeviceInfo;
use super::accounts::ProfileInfo;
use super::controls::PadStatus;
use super::developer::DevGame;
use super::display::{DisplayOutput, DisplayState};
use crate::screens::emulators::{EmuInfo, EmuUpdateInfo};
use super::network::{BluetoothAdapter, BtDev, NetInfo, NetStatus, WifiNet};
use super::power::{BatteryCare, CareState, Perf, PowerInfo};
use super::update::{OsUpdate, SystemInfo};
use rare_system as sys;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Snapshot {
    pub net: NetInfo,
    pub display: (Option<DisplayState>, Option<f32>, Option<bool>),
    pub power: PowerInfo,
    pub system: SystemInfo,
    pub device: DeviceInfo,
    pub pad: Option<PadStatus>,
    /// The developer game and where the Local API listens.
    pub dev: Option<DevGame>,
    pub local_api: Option<String>,
    /// The emulators in ~/Applications (None until read), the updates found, the one at work.
    pub emu: Option<Vec<EmuInfo>>,
    pub emu_found: Vec<(String, EmuUpdateInfo)>,
    pub emu_busy: Option<String>,
    pub profile: ProfileInfo,
    pub sources: Vec<(String, u32)>,
    /// None: no sound output.
    pub volume: Option<f32>,
    /// Sound outputs and the mic (None until read).
    pub audio: Option<sys::audio::Audio>,
    /// A game is running (the restart sheet warns).
    pub running: bool,
    /// Remote access (SSH) and network apps.
    pub ssh: super::remote::SshInfo,
}

#[derive(Default)]
struct Shared {
    snap: Option<Snapshot>,
    toasts: Vec<(String, bool)>,
    asks: Vec<Ask>,
    /// Keys found on a USB stick or GitHub that wait for the owner's yes.
    pending_keys: Vec<String>,
}

/// A confirmation a worker wants the app to show (a sheet with a yes button that runs `act`).
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub title: String,
    pub sub: String,
    pub body: String,
    pub yes: String,
    pub act: String,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared { snap: None, toasts: Vec::new(), asks: Vec::new(), pending_keys: Vec::new() });
static SSH_DIRTY: AtomicBool = AtomicBool::new(false);

/// Confirmations waiting for the app to show.
pub fn take_asks() -> Vec<Ask> {
    SHARED.lock().map(|mut s| std::mem::take(&mut s.asks)).unwrap_or_default()
}

/// SSH was switched on or off since the last call (the top bar icon re-reads).
pub fn take_ssh_changed() -> bool {
    SSH_DIRTY.swap(false, Ordering::Relaxed)
}

/// For the Support rows (support.rs): a toast, a confirmation, a redraw.
pub fn toast_pub(text: impl Into<String>, err: bool) {
    toast(text, err);
}

pub fn ask_pub(a: Ask) {
    ask(a);
}

pub fn bump_pub() {
    bump();
}

fn ask(a: Ask) {
    if let Ok(mut s) = SHARED.lock() {
        s.asks.push(a);
    }
}
static VERSION: AtomicU64 = AtomicU64::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);

/// The device layer's hooks into this launcher: Bluetooth power, for Quick settings' tile and
/// Settings, through BlueZ (`bluetoothctl`).
pub struct NativeHost;

impl rare_system::host::Host for NativeHost {
    fn env(&self, cmd: &mut std::process::Command) {
        rare_launch::launch::host_env(cmd);
    }
    fn log(&self, line: &str) {
        rare_input::log(line);
    }
    // The HUD (volume and brightness bar, toasts over a game) draws only under gamescope, on its
    // base display (src-tauri sysglue.rs).
    fn gamescope_active(&self) -> bool {
        rare_overlay::gamescope::active()
    }
    fn gamescope_focused_app(&self) -> Option<u64> {
        rare_overlay::gamescope::focused_app()
    }
    fn gamescope_base_display(&self) -> String {
        rare_overlay::gamescope::base_display()
    }
    fn launcher_front(&self) -> bool {
        rare_launch::apps::front::launcher_front() && !rare_webgame::visible()
    }
    fn bluetooth_state(&self) -> Option<bool> {
        let s = sys::netctl::bt_state();
        s.present.then_some(s.powered)
    }
    fn bluetooth_set(&self, on: bool) -> Result<(), String> {
        sys::netctl::bt_power(on).map(|_| ())
    }
}

/// Turn the live readings on (the device, not a fixture run). Until the first reading lands the
/// sections show what the last run read (blank on the very first run).
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
    let empty = SHARED.lock().map(|s| s.snap.is_none()).unwrap_or(false);
    if empty {
        let last: Last = crate::data::keep::load(LAST_KEY).unwrap_or_default();
        if let Ok(mut s) = SHARED.lock() {
            s.snap.get_or_insert_with(|| last.snapshot());
        }
    }
}

const LAST_KEY: &str = "settings-last";

/// What Settings showed last time, for the next start's first paint.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct Last {
    device: DeviceInfo,
    update: OsUpdate,
    can_restore: bool,
    net: Option<NetStatus>,
    display: (Option<DisplayState>, Option<f32>, Option<bool>),
    perf: Option<Perf>,
    care: Option<BatteryCare>,
    health: Option<u32>,
    battery: super::power::BatteryExtra,
    pad: Option<PadStatus>,
    local_api: Option<String>,
    emu: Option<Vec<EmuInfo>>,
    profile: ProfileInfo,
    sources: Vec<(String, u32)>,
    volume: Option<f32>,
    ssh: super::remote::SshInfo,
}

impl Last {
    fn of(s: &Snapshot) -> Last {
        Last {
            device: s.device.clone(),
            update: s.system.update.clone(),
            can_restore: s.system.can_restore,
            net: s.net.net.clone(),
            display: s.display.clone(),
            perf: s.power.perf.clone(),
            care: s.power.care.clone(),
            health: s.power.health,
            battery: s.power.battery.clone(),
            pad: s.pad.clone(),
            local_api: s.local_api.clone(),
            emu: s.emu.clone(),
            profile: s.profile.clone(),
            sources: s.sources.clone(),
            volume: s.volume,
            ssh: super::remote::SshInfo { busy: false, activity: vec![], ..s.ssh.clone() },
        }
    }

    fn snapshot(self) -> Snapshot {
        Snapshot {
            net: NetInfo { net: self.net, nets: None, scanning: false, joining: None, error: None, bt: None, bt_devices: vec![], bt_scanning: false, bt_busy: None },
            display: self.display,
            power: PowerInfo { perf: self.perf, care: self.care, health: self.health, battery: self.battery },
            system: SystemInfo { os_name: self.device.os_name.clone(), os_version: self.device.os_version.clone(), update: self.update, checking: false, can_restore: self.can_restore, menu_set: false },
            device: self.device,
            pad: self.pad,
            dev: None,
            local_api: self.local_api,
            emu: self.emu,
            emu_found: vec![],
            emu_busy: None,
            profile: self.profile,
            sources: self.sources,
            volume: self.volume,
            audio: crate::screens::quick::audio::last(),
            running: false,
            ssh: self.ssh,
        }
    }
}

/// Keeps the current snapshot for the next start.
fn save_last() {
    let last = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(Last::of));
    if let Some(l) = last {
        crate::data::keep::save(LAST_KEY, &l);
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Moves whenever the snapshot changed in a way the open section should redraw.
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

fn bump() {
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Something a section shows changed outside the snapshot (the Wi-Fi sign-in state).
pub fn touch() {
    bump();
}

/// The live snapshot, or None on a fixture run or before the first reading.
pub fn get() -> Option<Snapshot> {
    if !enabled() {
        return None;
    }
    SHARED.lock().ok()?.snap.clone()
}

/// Messages from workers for the app's toasts: (text, is_error).
pub fn take_toasts() -> Vec<(String, bool)> {
    SHARED.lock().map(|mut s| std::mem::take(&mut s.toasts)).unwrap_or_default()
}

fn toast(text: impl Into<String>, err: bool) {
    if let Ok(mut s) = SHARED.lock() {
        s.toasts.push((text.into(), err));
    }
}

fn edit(f: impl FnOnce(&mut Snapshot), notify: bool) {
    if let Ok(mut s) = SHARED.lock() {
        if let Some(sn) = s.snap.as_mut() {
            f(sn);
        }
    }
    if notify {
        bump();
    }
}

/// lib/native.ts failed(): the sentence, then the reason when there is one.
pub fn failed(what: &str, e: &str, hint: Option<&str>) -> String {
    let e = e.trim();
    let mut out = what.to_string();
    if !e.is_empty() && e.len() < 140 {
        out.push(' ');
        out.push_str(e.trim_end_matches('.'));
        out.push('.');
    }
    if let Some(h) = hint {
        out.push(' ');
        out.push_str(h);
    }
    out
}

// ---------------------------------------------------------------------------- readings

fn power_cfg() -> sys::power::PowerSettings {
    std::fs::read(rare_core::paths::config_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Merges the power fields into launcher.json, the way the Tauri launcher's Config does.
fn power_save(s: &sys::power::PowerSettings) {
    let path = rare_core::paths::config_file();
    let mut v: serde_json::Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({}));
    if let (Some(obj), Ok(serde_json::Value::Object(p))) = (v.as_object_mut(), serde_json::to_value(s)) {
        for (k, val) in p {
            obj.insert(k, val);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

fn name_of<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn named<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

fn read_perf() -> Option<Perf> {
    let p = sys::system::perf_get(&sys::power::wanted_now(&power_cfg()));
    Some(Perf {
        available: p.available,
        tdp: p.tdp.map(|t| t as f32),
        tdp_min: p.tdp_min as f32,
        tdp_max: p.tdp_max as f32,
        profile: name_of(&p.profile),
        note: p.note,
        cpu_boost: p.cpu_boost,
        fan: p.fan.map(|f| name_of(&f)),
        profile_watts: Some((p.profile_watts.quiet as f32, p.profile_watts.balanced as f32, p.profile_watts.performance as f32)),
        fan_now: sys::system::fan_status().map(|f| crate::features::fancurve::now_line(&f)).filter(|l| l != "Reading the fan"),
    })
}

fn read_care() -> Option<BatteryCare> {
    let c = sys::system::battery_care()?;
    let state = match c.state.as_str() {
        "unavailable" => CareState::Unavailable,
        "unplugged" => CareState::Unplugged,
        "low" => CareState::Low,
        "hot" => CareState::Hot,
        "unknown" => CareState::Unknown,
        "charging" => CareState::Charging,
        "holding" => CareState::Holding,
        _ => CareState::Off,
    };
    Some(BatteryCare { available: c.available, on: c.on, state })
}

fn read_power() -> PowerInfo {
    let h = sys::system::battery_health();
    let battery = super::power::BatteryExtra { cycles: h.as_ref().and_then(|h| h.cycles), counted: h.as_ref().is_some_and(|h| h.counted), limit: sys::system::battery_limit() };
    PowerInfo { perf: read_perf(), care: read_care(), health: sys::system::battery().and_then(|b| b.health).map(u32::from), battery }
}

fn read_display() -> (Option<DisplayState>, Option<f32>, Option<bool>) {
    let d = sys::display::display_state();
    let state = DisplayState {
        outputs: d.outputs.iter().map(|o| DisplayOutput { name: o.name.clone(), internal: o.internal, connected: o.connected }).collect(),
        active: d.active,
        docked: d.docked,
        dock_name: d.dock_name,
        dock_mode: d.dock_mode,
    };
    let info = sys::brightness::brightness_info();
    (Some(state), info.pos.map(f32::from), info.pos.map(|_| info.dim_idle))
}

pub(crate) fn read_update() -> OsUpdate {
    OsUpdate::from_sys(sys::osupdate::status())
}

fn read_bt() -> Option<BluetoothAdapter> {
    let b = sys::netctl::bt_state();
    Some(BluetoothAdapter { present: b.present, powered: b.powered, blocked: b.blocked })
}

fn bt_devs() -> Vec<BtDev> {
    sys::netctl::bt_devices().into_iter().map(|d| BtDev { addr: d.addr, name: d.name, paired: d.paired, connected: d.connected }).collect()
}

fn wifi_nets() -> Result<Vec<WifiNet>, String> {
    sys::netctl::wifi_scan().map(|l| l.into_iter().map(|n| WifiNet { ssid: n.ssid, signal: n.signal as u32, secure: n.secure, active: n.active }).collect())
}

fn read_net_status() -> NetStatus {
    let n = sys::netctl::net_status();
    NetStatus { online: n.online, wifi: n.wifi, managed: n.managed }
}

/// The signed-in Rare account and Steam account, read now (no network).
pub fn read_profile() -> ProfileInfo {
    let (accounts, selected) = crate::screens::wallet::page::load_accounts();
    let cur = selected.as_ref().and_then(|s| accounts.iter().find(|a| &a.address == s)).or(accounts.first());
    ProfileInfo { address: cur.map(|a| a.address.clone()), label: cur.map(|a| a.label.clone()), steam: crate::features::firstrun::steam_account(), ..Default::default() }
}

/// What InputPlumber says about the controller (the same reading the Tauri launcher shows).
fn read_pad() -> Option<PadStatus> {
    let p = rare_input::gyro::input_status();
    Some(PadStatus { available: p.available, name: p.name, motion: p.motion })
}

fn read_dev() -> Option<DevGame> {
    rare_system::devmode::dev_game_info().map(|g| DevGame { url: g.url, slug: g.slug, collections: g.collections.len() })
}

fn read_emulators() -> Vec<EmuInfo> {
    rare_emu::emu_list()
        .into_iter()
        .map(|e| EmuInfo { id: e.id.into(), name: e.name.into(), path: e.path, modified: e.modified, tag: e.tag, power: name_of(&e.power) })
        .collect()
}

fn dmi(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/dmi/id/{name}")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && !matches!(s.as_str(), "Default string" | "To be filled by O.E.M." | "Not Specified" | "0"))
}

/// The serial number (readable by the wheel group on Rare OS).
fn read_serial() -> Option<String> {
    dmi("product_serial").or_else(|| dmi("board_serial"))
}

/// "2.19_S20 (02/19/2025)".
fn read_bios() -> Option<String> {
    let v = dmi("bios_version")?;
    Some(match dmi("bios_date") {
        Some(d) => format!("{v} ({d})"),
        None => v,
    })
}

/// The Rare username and SuiNS name rare.market has for this address.
fn fetch_names(address: &str) -> Option<(Option<String>, Option<String>)> {
    use rare_market::net::Net;
    let net = crate::screens::wallet::live::market_net();
    let key = rare_market::net::market_key(&net.market_origin(), &format!("/v1/accounts/{address}"));
    let body = net.refresh(&key).ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(String::from);
    Some((text("username"), text("suins")))
}

/// Asks rare.market for the account's username (Accounts shows it).
pub fn refresh_profile(waker: Arc<dyn Fn() + Send + Sync>) {
    std::thread::spawn(move || {
        let prof = read_profile();
        let Some(addr) = prof.address.clone() else { return };
        if let Some((username, name)) = fetch_names(&addr) {
            crate::data::account::remember_username(&addr, username.clone());
            edit(|s| { s.profile.address = Some(addr); s.profile.username = username; s.profile.name = name; }, true);
            waker();
        }
    });
}

/// The username Friends knows (after a pick): shown at once, without another request.
pub fn set_username(username: Option<String>) {
    let Some(u) = username else { return };
    if let Some(a) = SHARED.lock().ok().and_then(|s| s.snap.as_ref().and_then(|sn| sn.profile.address.clone())) {
        crate::data::account::remember_username(&a, Some(u.clone()));
    }
    let same = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|sn| sn.profile.username.as_deref() == Some(u.as_str()))).unwrap_or(true);
    if !same {
        edit(|s| s.profile.username = Some(u), true);
    }
}

/// The first full reading. Runs on a worker; the app is woken when it lands.
pub fn load_all(sources: Vec<(String, u32)>, waker: Arc<dyn Fn() + Send + Sync>) {
    // The library's counts are known now; the rest of the first paint is the last run's.
    if !sources.is_empty() {
        let src = sources.clone();
        edit(|s| s.sources = src, true);
    }
    std::thread::spawn(move || {
        let d = sys::system::device_info();
        let update = read_update();
        let levels = sys::system::levels();
        let net_status = read_net_status();
        // The last run's emulator list and username stay until their own reads answer.
        let prev = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|p| (p.emu.clone(), p.profile.clone())));
        let mut profile = read_profile();
        if let Some((_, p)) = &prev {
            if p.address == profile.address {
                profile.username = p.username.clone();
                profile.name = p.name.clone();
            }
        }
        let snap = Snapshot {
            net: NetInfo { net: Some(net_status.clone()), nets: None, scanning: false, joining: None, error: None, bt: None, bt_devices: vec![], bt_scanning: false, bt_busy: None },
            display: read_display(),
            power: read_power(),
            system: SystemInfo {
                os_name: d.os_name.clone(),
                os_version: d.os_version.clone(),
                update,
                checking: false,
                can_restore: sys::osupdate::restore_available(),
                menu_set: false,
            },
            device: DeviceInfo { name: d.name, os_name: d.os_name, os_version: d.os_version, hostname: d.hostname, under_gamescope: d.under_gamescope, serial: read_serial(), bios: read_bios() },
            pad: read_pad(),
            dev: read_dev(),
            local_api: rare_localapi::localapi_info().get("url").and_then(|u| u.as_str()).map(String::from),
            emu: prev.and_then(|p| p.0),
            emu_found: vec![],
            emu_busy: None,
            profile,
            sources,
            volume: levels.volume.map(f32::from),
            audio: Some(read_audio()),
            running: false,
            ssh: read_ssh(),
        };
        if let Ok(mut s) = SHARED.lock() {
            s.snap = Some(snap);
        }
        bump();
        waker();
        // The username and the emulators are quick; Wi-Fi and Bluetooth after them can take seconds.
        let emu = read_emulators();
        edit(|s| s.emu = Some(emu), true);
        waker();
        save_last();
        refresh_profile(waker.clone());
        super::support::load(waker.clone());
        let w2 = waker.clone();
        let bt = std::thread::spawn(move || refresh_bt_devices(&w2));
        if net_status.managed {
            scan_wifi(&waker);
        }
        let _ = bt.join();
    });
}

fn scan_wifi(waker: &Arc<dyn Fn() + Send + Sync>) {
    edit(|s| { s.net.scanning = true; s.net.error = None; }, true);
    waker();
    let r = wifi_nets();
    edit(
        |s| {
            s.net.scanning = false;
            match r {
                Ok(l) => s.net.nets = Some(l),
                Err(e) => {
                    s.net.nets = Some(vec![]);
                    s.net.error = Some(failed("Couldn't look for networks.", &e, None));
                }
            }
        },
        true,
    );
    waker();
}

fn refresh_bt_devices(waker: &Arc<dyn Fn() + Send + Sync>) {
    let bt = read_bt();
    let powered = bt.as_ref().is_some_and(|b| b.powered);
    let devs = if powered { bt_devs() } else { vec![] };
    edit(|s| { s.net.bt = Some(bt); s.net.bt_devices = devs; }, true);
    waker();
}

/// Cheap re-reads while Settings is open (the Wi-Fi status, the update state).
pub fn poll(waker: Arc<dyn Fn() + Send + Sync>) {
    std::thread::spawn(move || {
        let net = read_net_status();
        let update = read_update();
        let changed = SHARED.lock().ok().and_then(|s| s.snap.clone()).map(|s| s.net.net.map(|n| (n.online, n.wifi, n.managed)) != Some((net.online, net.wifi.clone(), net.managed)) || s.system.update != update).unwrap_or(false);
        if changed {
            edit(|s| { s.net.net = Some(net); s.system.update = update; }, true);
            waker();
        }
        // SSH: every third reading (the helper runs through pkexec).
        static POLLS: AtomicU64 = AtomicU64::new(0);
        if POLLS.fetch_add(1, Ordering::Relaxed) % 3 == 2 {
            let info = read_ssh();
            let moved = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|sn| !sn.ssh.busy && sn.ssh != info)).unwrap_or(false);
            if moved {
                edit(|s| s.ssh = info, true);
                SSH_DIRTY.store(true, Ordering::Relaxed);
                waker();
            }
        }
        // Network app activity: cheap, every reading.
        let act = rare_localapi::network_activity();
        let moved = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|sn| sn.ssh.activity != act)).unwrap_or(false);
        if moved {
            edit(|s| s.ssh.activity = act, true);
            SSH_DIRTY.store(true, Ordering::Relaxed);
            waker();
        }
        refresh_audio(&waker, false);
        let pad = read_pad();
        let dev = read_dev();
        let moved = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|s| s.pad.as_ref().map(|p| (p.available, p.motion, p.name.clone())) != pad.as_ref().map(|p| (p.available, p.motion, p.name.clone())) || s.dev.as_ref().map(|d| d.url.clone()) != dev.as_ref().map(|d| d.url.clone()))).unwrap_or(false);
        if moved {
            edit(|s| { s.pad = pad; s.dev = dev; }, true);
            waker();
        }
    });
}

/// Outputs and the mic; Quick settings keeps the same reading.
fn read_audio() -> sys::audio::Audio {
    let a = sys::audio::read();
    crate::screens::quick::audio::remember(a.clone());
    a
}

/// Re-reads the sound (a Bluetooth headset that just connected shows up). `force`: redraw even
/// when nothing moved (after an action, so a failed change snaps back).
fn refresh_audio(waker: &Arc<dyn Fn() + Send + Sync>, force: bool) {
    let a = read_audio();
    let moved = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|sn| sn.audio.as_ref() != Some(&a))).unwrap_or(false);
    if moved || force {
        edit(|s| s.audio = Some(a), true);
        waker();
    }
}

fn sound_action(op: &str, rest: String, waker: &Arc<dyn Fn() + Send + Sync>) {
    let w = waker.clone();
    match op {
        "output" => spawn(waker, move || {
            if let Err(e) = sys::audio::set_default_output(&rest) {
                toast(failed("Couldn't switch the output.", &e, None), true);
            }
            refresh_audio(&w, true);
        }),
        "mic-mute" => {
            let muted = rest == "true";
            spawn(waker, move || {
                if let Err(e) = sys::audio::set_mic_muted(muted) {
                    toast(failed(if muted { "Couldn't mute the mic." } else { "Couldn't turn the mic on." }, &e, None), true);
                }
                refresh_audio(&w, true);
            });
        }
        "louder" => {
            let on = rest == "true";
            spawn(waker, move || {
                if let Err(e) = sys::speakers::set_louder(on) {
                    toast(failed("Couldn't change the speakers.", &e, None), true);
                }
                refresh_audio(&w, true);
            });
        }
        "mic-level" => {
            let v: f32 = rest.parse().unwrap_or(0.0);
            spawn(waker, move || {
                if let Err(e) = sys::audio::set_mic_level(v.round().clamp(0.0, 100.0) as u8) {
                    toast(failed("Couldn't change the mic level.", &e, None), true);
                }
                // The slider already shows it: keep the reading without a redraw.
                let a = read_audio();
                edit(|s| s.audio = Some(a), false);
            });
        }
        _ => {}
    }
}

// The capability switches for network apps: one id per line, next to the token.
fn net_caps_path() -> std::path::PathBuf {
    rare_core::paths::config_file().with_file_name("network-api-caps")
}

fn net_caps_read() -> Vec<String> {
    std::fs::read_to_string(net_caps_path()).ok().map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| rare_localapi::CAPS.iter().any(|c| c.0 == l)).collect()).unwrap_or_default()
}

fn net_caps_write(ids: &[String]) -> Result<(), String> {
    let path = net_caps_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, ids.join("\n")).map_err(|e| e.to_string())
}

fn read_ssh() -> super::remote::SshInfo {
    let prev = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|sn| (sn.ssh.netapi, sn.ssh.net_token.clone())));
    let cfg = super::config::load();
    let (netapi, net_token) = match prev {
        Some(p) => p,
        None => (cfg.allow_network_apps == Some(true), net_token_read()),
    };
    super::remote::SshInfo { status: sys::ssh::status().ok(), host: sys::ssh::hostname(), ip: sys::ssh::lan_ip(), busy: false, netapi, net_token, caps: net_caps_read(), activity: rare_localapi::network_activity() }
}

/// Refreshes the SSH reading and wakes the app. Keeps `busy` off.
fn refresh_ssh(waker: &Arc<dyn Fn() + Send + Sync>) {
    let info = read_ssh();
    edit(|s| s.ssh = info, true);
    SSH_DIRTY.store(true, Ordering::Relaxed);
    waker();
}

// The network apps token: a file next to launcher.json, readable by the owner only.
fn net_token_path() -> std::path::PathBuf {
    rare_core::paths::config_file().with_file_name("network-api-token")
}

fn net_token_read() -> Option<String> {
    std::fs::read_to_string(net_token_path()).ok().map(|t| t.trim().to_string()).filter(|t| t.len() >= 32)
}

fn net_token_new() -> Result<String, String> {
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| e.to_string())?;
    let t: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let path = net_token_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path).map_err(|e| e.to_string())?;
        f.write_all(t.as_bytes()).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, &t).map_err(|e| e.to_string())?;
    Ok(t)
}

/// At launch: if the owner allowed network apps, the listener comes back up.
pub fn start_network_apps() {
    if super::config::load().allow_network_apps != Some(true) {
        return;
    }
    let Some(t) = net_token_read() else { return };
    // The switches first, so the virtual pad exists the moment the listener does.
    rare_localapi::network_set_caps(&net_caps_read());
    if let Err(e) = rare_localapi::network_enable(&t, sys::ssh::NETAPI_PORT, &sys::ssh::hostname()) {
        eprintln!("[settings] network apps: {e}");
    }
}

fn set_allow_network_apps(on: bool) -> Result<(), String> {
    let old = super::config::load();
    let mut new = old.clone();
    new.allow_network_apps = if on { Some(true) } else { None };
    super::config::save_diff(&old, &new)
}

fn github_keys(user: &str) -> Result<Vec<String>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(10))).http_status_as_error(false).build().into();
    let mut resp = agent.get(&sys::ssh::github_keys_url(user)).header("User-Agent", "RareLauncher/0.1").call().map_err(|_| "Couldn't reach GitHub. Check your connection.".to_string())?;
    if resp.status().as_u16() == 404 {
        return Err(format!("GitHub has no user called {user}."));
    }
    if !resp.status().is_success() {
        return Err("GitHub didn't answer. Try again in a moment.".into());
    }
    let text = resp.body_mut().with_config().limit(64 * 1024).read_to_string().map_err(|_| "GitHub's answer was too large.".to_string())?;
    let keys = sys::ssh::parse_keys(&text);
    if keys.is_empty() {
        return Err(format!("{user} has no public keys on GitHub."));
    }
    Ok(keys)
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn add_keys_now(lines: Vec<String>, waker: &Arc<dyn Fn() + Send + Sync>) {
    match sys::ssh::add_keys(&lines) {
        Ok(m) => {
            let added = m.split_whitespace().find_map(|p| p.strip_prefix("added=")).and_then(|n| n.parse::<usize>().ok()).unwrap_or(lines.len());
            toast(if added == 0 { "Those keys were already added".to_string() } else { format!("Added {added} key{}. Password sign-in turns off once a key is added.", plural(added)) }, false);
        }
        Err(e) => toast(failed("Couldn't add the key.", &e, None), true),
    }
    refresh_ssh(waker);
}

fn ssh_action(op: &str, rest: String, arg: Option<String>, waker: &Arc<dyn Fn() + Send + Sync>) {
    let w = waker.clone();
    match op {
        "toggle" => {
            let on = rest == "true";
            edit(|s| s.ssh.busy = true, true);
            spawn(waker, move || {
                match sys::ssh::set_enabled(on) {
                    Ok(()) => {
                        let st = sys::ssh::status().ok();
                        let none = st.as_ref().is_some_and(|s| !s.has_password && s.keys.is_empty());
                        toast(if on { if none { "Remote access is on. Set a password or add a key to sign in.".to_string() } else { "Remote access is on".to_string() } } else { "Remote access is off".to_string() }, false);
                    }
                    Err(e) => toast(failed(if on { "Couldn't turn on remote access." } else { "Couldn't turn off remote access." }, &e, None), true),
                }
                refresh_ssh(&w);
            });
        }
        "password" => {
            let pw = arg.unwrap_or_default();
            spawn(waker, move || {
                match sys::ssh::set_password(&pw) {
                    Ok(()) => toast("Password set", false),
                    Err(e) => toast(failed("Couldn't set the password.", &e, None), true),
                }
                refresh_ssh(&w);
            });
        }
        "keys" => {
            let text = arg.unwrap_or_default();
            spawn(waker, move || {
                let keys = sys::ssh::parse_keys(&text);
                if keys.is_empty() {
                    toast("That isn't a public key. It starts with ssh-ed25519 or ssh-rsa.", true);
                    return;
                }
                add_keys_now(keys, &w);
            });
        }
        "usb" => spawn(waker, move || {
            let files = sys::ssh::usb_key_files();
            let mut keys: Vec<String> = Vec::new();
            for (_, t) in &files {
                for k in sys::ssh::parse_keys(t) {
                    if !keys.contains(&k) {
                        keys.push(k);
                    }
                }
            }
            if keys.is_empty() {
                toast("No keys found. Put a file called authorized_keys on a USB stick and plug it in.", true);
                return;
            }
            let drive = files.first().map(|(d, _)| d.clone()).unwrap_or_default();
            let n = keys.len();
            if let Ok(mut s) = SHARED.lock() {
                s.pending_keys = keys;
            }
            ask(Ask {
                title: format!("Add {n} key{} from the USB stick?", plural(n)),
                sub: drive,
                body: "Anyone with a matching private key will be able to sign in to this device.".into(),
                yes: "Add keys".into(),
                act: "ssh:add-pending".into(),
            });
        }),
        "github" => {
            let user = arg.unwrap_or_default().trim().trim_start_matches('@').to_string();
            spawn(waker, move || {
                if !sys::ssh::github_user_ok(&user) {
                    toast("That isn't a GitHub user name.", true);
                    return;
                }
                match github_keys(&user) {
                    Ok(keys) => {
                        let n = keys.len();
                        if let Ok(mut s) = SHARED.lock() {
                            s.pending_keys = keys;
                        }
                        let (title, sub, body) = super::remote::github_confirm(&user, n);
                        ask(Ask { title, sub, body, yes: "Add keys".into(), act: "ssh:add-pending".into() });
                    }
                    Err(e) => toast(e, true),
                }
            });
        }
        "add-pending" => spawn(waker, move || {
            let keys = SHARED.lock().map(|mut s| std::mem::take(&mut s.pending_keys)).unwrap_or_default();
            add_keys_now(keys, &w);
        }),
        "remove-ask" => {
            let kind = SHARED.lock().ok().and_then(|s| s.snap.as_ref().and_then(|sn| sn.ssh.status.as_ref().and_then(|st| st.keys.iter().find(|k| k.fingerprint == rest).cloned())));
            let label = kind.map(|k| if k.comment.is_empty() { format!("{} key", k.kind) } else { k.comment }).unwrap_or_else(|| "this key".into());
            ask(Ask {
                title: "Remove this key?".into(),
                sub: label,
                body: format!("The computer that uses it can no longer sign in.\n{rest}"),
                yes: "Remove".into(),
                act: format!("ssh:remove:{rest}"),
            });
        }
        "remove" => spawn(waker, move || {
            match sys::ssh::remove_key(&rest) {
                Ok(()) => toast("Key removed", false),
                Err(e) => toast(failed("Couldn't remove the key.", &e, None), true),
            }
            refresh_ssh(&w);
        }),
        "keep-password" => {
            let on = rest == "true";
            spawn(waker, move || {
                if let Err(e) = sys::ssh::keep_password(on) {
                    toast(failed("Couldn't change that.", &e, None), true);
                }
                refresh_ssh(&w);
            });
        }
        "auto-off" => {
            let on = rest == "true";
            spawn(waker, move || {
                match sys::ssh::auto_off(on) {
                    Ok(()) if on => toast("Remote access turns off in an hour", false),
                    Ok(()) => {}
                    Err(e) => toast(failed("Couldn't set the timer.", &e, None), true),
                }
                refresh_ssh(&w);
            });
        }
        "copy" => {
            let cmd = SHARED.lock().ok().and_then(|s| s.snap.as_ref().map(|sn| sn.ssh.command())).unwrap_or_default();
            spawn(waker, move || {
                crate::screens::wallet::flows::manage::copy_text(&cmd);
                toast("Copied", false);
            });
        }
        _ => {}
    }
}

fn netapi_action(op: &str, rest: String, waker: &Arc<dyn Fn() + Send + Sync>) {
    let w = waker.clone();
    match op {
        "toggle" => {
            let on = rest == "true";
            spawn(waker, move || {
                let r: Result<Option<String>, String> = (|| {
                    if on {
                        let t = match net_token_read() {
                            Some(t) => t,
                            None => net_token_new()?,
                        };
                        // The firewall first: the listener is useless behind a closed port, and a
                        // device without the helper (not Rare OS) still works on the loopback side.
                        let _ = sys::ssh::netapi_firewall(true);
                        rare_localapi::network_set_caps(&net_caps_read());
                        rare_localapi::network_enable(&t, sys::ssh::NETAPI_PORT, &sys::ssh::hostname())?;
                        set_allow_network_apps(true)?;
                        Ok(Some(t))
                    } else {
                        rare_localapi::network_disable();
                        let _ = sys::ssh::netapi_firewall(false);
                        set_allow_network_apps(false)?;
                        Ok(None)
                    }
                })();
                match r {
                    Ok(t) => {
                        edit(|s| { s.ssh.netapi = on; s.ssh.net_token = t; }, true);
                        toast(if on { "Network apps are allowed" } else { "Network apps are off" }, false);
                    }
                    Err(e) => toast(failed("Couldn't change network apps.", &e, None), true),
                }
                w();
            });
        }
        "cap" => {
            // "<id>:<on>"
            let (id, on) = match rest.rsplit_once(':') {
                Some((i, v)) => (i.to_string(), v == "true"),
                None => return,
            };
            spawn(waker, move || {
                let mut ids = net_caps_read();
                ids.retain(|i| *i != id);
                if on {
                    ids.push(id.clone());
                }
                match net_caps_write(&ids) {
                    Ok(()) => {
                        rare_localapi::network_set_caps(&ids);
                        edit(|s| s.ssh.caps = ids.clone(), true);
                        let title = rare_localapi::CAPS.iter().find(|c| c.0 == id).map(|c| c.1).unwrap_or("That");
                        toast(&format!("{title} is {}", if on { "on" } else { "off" }), false);
                    }
                    Err(e) => toast(failed("Couldn't save that.", &e, None), true),
                }
                w();
            });
        }
        "newtoken" => spawn(waker, move || {
            match net_token_new().and_then(|t| rare_localapi::network_enable(&t, sys::ssh::NETAPI_PORT, &sys::ssh::hostname()).map(|_| t)) {
                Ok(t) => {
                    edit(|s| s.ssh.net_token = Some(t), true);
                    toast("New token made. The old one stopped working.", false);
                }
                Err(e) => toast(failed("Couldn't make a new token.", &e, None), true),
            }
            w();
        }),
        "copy" => spawn(waker, move || {
            if let Some(t) = SHARED.lock().ok().and_then(|s| s.snap.as_ref().and_then(|sn| sn.ssh.net_token.clone())) {
                crate::screens::wallet::flows::manage::copy_text(&t);
                toast("Copied", false);
            }
        }),
        _ => {}
    }
}

// ----------------------------------------------------------------------------- actions

fn emu_name(id: &str) -> String {
    rare_emu::by_id(id).map(|e| e.name.to_string()).unwrap_or_else(|| id.to_string())
}

/// What running an action asks of the app.
#[derive(Debug, PartialEq)]
pub enum Ran {
    /// Handled by a worker (or not worth a worker).
    Started,
    /// Not one of ours.
    NotMine,
}

fn spawn(waker: &Arc<dyn Fn() + Send + Sync>, f: impl FnOnce() + Send + 'static) {
    let w = waker.clone();
    std::thread::spawn(move || {
        f();
        w();
    });
}

/// True for the actions `run` handles (the device's own: system commands and services).
pub fn owns(act: &str) -> bool {
    let mut p = act.splitn(3, ':');
    let (a, b) = (p.next().unwrap_or(""), p.next().unwrap_or(""));
    matches!(
        (a, b),
        ("sound", "volume" | "output" | "mic-mute" | "mic-level" | "louder")
            | ("display", "brightness" | "night-on" | "night-warmth" | "night-schedule" | "tv-size" | "tv-refresh" | "dim-idle")
            | ("power", "profile" | "tdp" | "cpu-boost" | "fan" | "charge-limit" | "battery-care" | "desktop" | "reboot" | "poweroff" | "suspend")
            | ("os", "check" | "rollback-ask" | "rollback" | "boot-menu-next" | "restore")
            | ("emu", "check" | "update")
            | ("network", "scan" | "join" | "forget" | "bluetooth-on" | "bt-scan" | "bt-pair" | "bt-connect" | "bt-disconnect" | "bt-forget")
            | ("ssh" | "netapi" | "time" | "logs" | "reset", _)
    )
}

/// Runs a Settings action on the device. `arg` is the text an on-screen keyboard returned (a Wi-Fi
/// password). Actions the app owns (overlays, the library scan, keyboards) are not here.
pub fn run(act: &str, arg: Option<String>, waker: Arc<dyn Fn() + Send + Sync>) -> Ran {
    let parts: Vec<&str> = act.splitn(3, ':').collect();
    let (a, b, rest) = (parts.first().copied().unwrap_or(""), parts.get(1).copied().unwrap_or(""), parts.get(2).copied().unwrap_or(""));
    let rest = rest.to_string();
    let w = waker.clone();
    match (a, b) {
        ("sound", "volume") => {
            let v: f32 = rest.parse().unwrap_or(0.0);
            spawn(&w, move || {
                if let Err(e) = sys::system::set_volume(v.round().clamp(0.0, 100.0) as u8) {
                    toast(failed("Couldn't change the volume.", &e, None), true);
                }
            });
        }
        ("display", "brightness") => {
            let v: f32 = rest.parse().unwrap_or(0.0);
            spawn(&w, move || {
                if let Err(e) = sys::system::set_brightness(v.round().clamp(0.0, 100.0) as u8) {
                    toast(failed("Couldn't change the brightness.", &e, None), true);
                }
            });
        }
        ("display", op @ ("night-on" | "night-warmth" | "night-schedule")) => {
            let op = op.to_string();
            spawn(&w, move || {
                let mut n = sys::screen::saved().night_light;
                match op.as_str() {
                    "night-on" => n.on = rest == "true",
                    "night-schedule" => n.scheduled = rest == "true",
                    _ => n.warmth = rest.parse::<f32>().unwrap_or(50.0).round().clamp(10.0, 100.0) as u8,
                }
                if let Err(e) = sys::screen::night_set(n) {
                    toast(failed("Couldn't change Night light.", &e, None), true);
                }
            });
        }
        ("display", op @ ("tv-size" | "tv-refresh")) => {
            let op = op.to_string();
            spawn(&w, move || {
                let mut t = sys::screen::saved().tv;
                if op == "tv-size" {
                    let (wd, ht) = rest.split_once('x').map(|(a, b)| (a.parse().unwrap_or(0), b.parse().unwrap_or(0))).unwrap_or((0, 0));
                    t.width = wd;
                    t.height = ht;
                } else {
                    t.refresh = rest.parse().unwrap_or(0);
                }
                if let Err(e) = sys::screen::tv_set(t) {
                    toast(failed("Couldn't change the TV's picture.", &e, None), true);
                }
            });
        }
        ("display", "dim-idle") => {
            let on = rest == "true";
            spawn(&w, move || {
                sys::brightness::brightness_dim_idle(on);
                edit(|s| s.display.2 = Some(on), false);
            });
        }
        ("power", "profile") | ("power", "tdp") => {
            let (profile, tdp) = if b == "tdp" { ("custom".to_string(), rest.parse::<f64>().ok()) } else { (rest.clone(), None) };
            let quiet = b == "tdp";
            spawn(&w, move || {
                let r = (|| {
                    let p: rare_core::Profile = named(&profile).ok_or("Unknown profile")?;
                    let mut s = power_cfg();
                    let wanted = sys::power::perf_set_config(&mut s, p, tdp, None);
                    power_save(&s);
                    sys::power::perf_set_apply(tdp, wanted, None)
                })();
                if let Err(e) = r {
                    toast(failed("Couldn't change the performance profile.", &e, None), true);
                }
                let perf = read_perf();
                edit(|s| s.power.perf = perf, !quiet);
            });
        }
        ("power", "cpu-boost") => {
            let on = rest == "true";
            spawn(&w, move || {
                let mut s = power_cfg();
                sys::power::perf_boost_config(&mut s, on);
                power_save(&s);
                if let Err(e) = sys::system::set_cpu_boost(on) {
                    toast(failed("Couldn't change CPU boost.", &e, None), true);
                }
                let perf = read_perf();
                edit(|s| s.power.perf = perf, true);
            });
        }
        ("power", "fan") => {
            let mode = rest.clone();
            spawn(&w, move || {
                let r = named::<sys::system::FanMode>(&mode).ok_or("Unknown fan mode".to_string()).and_then(sys::system::set_fan);
                if let Err(e) = r {
                    toast(failed("Couldn't change the fan.", &e, None), true);
                }
                let perf = read_perf();
                edit(|s| s.power.perf = perf, true);
            });
        }
        ("power", "charge-limit") => {
            let pct = rest.parse::<u32>().unwrap_or(80);
            spawn(&w, move || {
                if let Err(e) = sys::system::set_battery_limit(pct) {
                    toast(failed("Couldn't change the charge limit.", &e, None), true);
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
                let care = read_care();
                let limit = sys::system::battery_limit();
                edit(|s| {
                    s.power.care = care;
                    s.power.battery.limit = limit;
                }, true);
            });
        }
        ("power", "battery-care") => {
            let on = rest == "true";
            spawn(&w, move || {
                if let Err(e) = sys::system::set_battery_care(on) {
                    toast(failed("Couldn't change battery care.", &e, None), true);
                }
                // The driver takes a moment to report the new state (PowerSection.tsx waits 6 s).
                std::thread::sleep(std::time::Duration::from_secs(2));
                let care = read_care();
                edit(|s| s.power.care = care, true);
            });
        }
        ("power", action) if matches!(action, "desktop" | "reboot" | "poweroff" | "suspend") => {
            let action = action.to_string();
            spawn(&w, move || {
                if let Err(e) = sys::system::power(&action) {
                    toast(failed("That didn't work.", &e, None), true);
                }
            });
        }
        ("os", "check") => {
            let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            edit(|s| s.system.checking = true, true);
            spawn(&w, move || {
                if let Err(e) = sys::osupdate::check_now() {
                    edit(|s| s.system.checking = false, true);
                    toast(failed("Couldn't check for updates.", &e, Some("Check your Wi-Fi and try again.")), true);
                    return;
                }
                // Follow the updater for as long as it works (an 8 GB download takes a while): its
                // heartbeat says it is alive, its result says how it ended. No fixed time limit.
                let mut seen_busy = false;
                let mut idle_polls = 0u32;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let u = read_update();
                    let busy = u.busy.is_some();
                    seen_busy |= busy;
                    let done = u.staged || (!busy && (super::update::check_done(&u, started) || u.stopped && seen_busy));
                    if done {
                        edit(|s| { s.system.update = u; s.system.checking = false; }, true);
                        return;
                    }
                    edit(|s| s.system.update = u, true);
                    if busy {
                        idle_polls = 0;
                    } else {
                        idle_polls += 1;
                        // Nothing started within a minute: say so instead of spinning.
                        if idle_polls >= 20 {
                            edit(|s| s.system.checking = false, true);
                            toast("The update check did not start. Try again in a moment.".to_string(), true);
                            return;
                        }
                    }
                }
            });
        }
        ("os", "rollback-ask") => {
            let snap = SHARED.lock().ok().and_then(|s| s.snap.clone());
            let prev = snap.as_ref().and_then(|s| s.system.update.previous_version.clone()).unwrap_or_else(|| "the previous version".into());
            let running = snap.as_ref().is_some_and(|s| s.running);
            let mut body = format!("The device restarts into Rare OS {prev}. Your games and files stay as they are.\nThis version stays on the device but does not install again by itself. Update now in Settings installs it again.");
            if running {
                body.push_str("\nA game is running. Close it first so nothing is lost.");
            }
            ask(Ask { title: "Go back to the previous version".into(), sub: format!("Rare OS {prev}"), body, yes: "Go back and restart".into(), act: "os:rollback:reboot".into() });
        }
        ("os", "rollback") => {
            let reboot = rest == "reboot";
            spawn(&w, move || match sys::osupdate::rollback(reboot) {
                Ok(m) => toast(m, false),
                Err(e) => toast(failed("Couldn't go back to the previous version.", &e, None), true),
            });
        }
        ("os", "boot-menu-next") => {
            spawn(&w, move || match sys::osupdate::boot_menu_next() {
                Ok(()) => {
                    edit(|s| s.system.menu_set = true, true);
                    toast("The boot menu shows on the next start", false);
                }
                Err(e) => toast(failed("Couldn't set the boot menu.", &e, None), true),
            });
        }
        ("os", "restore") => {
            let reboot = rest == "reboot";
            spawn(&w, move || match sys::osupdate::restore(reboot) {
                Ok(m) => toast(m, false),
                Err(e) => toast(failed("Couldn't restore the previous system.", &e, None), true),
            });
        }
        ("emu", "check") => {
            let id = rest.clone();
            edit(|s| s.emu_busy = Some(id.clone()), true);
            spawn(&w, move || {
                match rare_emu::emu_check(&id) {
                    Ok(u) => {
                        let info = EmuUpdateInfo { available: u.available, tag: u.tag.clone(), verify: u.verify.into() };
                        if !u.available {
                            toast(format!("{} is up to date", emu_name(&id)), false);
                        }
                        edit(|s| { s.emu_found.retain(|(i, _)| *i != id); s.emu_found.push((id.clone(), info)); }, false);
                    }
                    Err(e) => toast(failed(&format!("Couldn't check {}.", emu_name(&id)), &e.to_string(), None), true),
                }
                edit(|s| s.emu_busy = None, true);
            });
        }
        ("emu", "update") => {
            let id = rest.clone();
            edit(|s| s.emu_busy = Some(id.clone()), true);
            spawn(&w, move || {
                match rare_emu::emu_update(&crate::features::bus::ctx(), id.clone()) {
                    Ok(d) => {
                        toast(format!("{} updated to {}", emu_name(&id), d.tag), false);
                        let list = read_emulators();
                        edit(|s| { s.emu = Some(list); s.emu_found.retain(|(i, _)| *i != id); }, false);
                    }
                    Err(e) => toast(failed(&format!("Couldn't update {}.", emu_name(&id)), &e.to_string(), None), true),
                }
                edit(|s| s.emu_busy = None, true);
            });
        }
        ("network", "scan") => {
            spawn(&w, {
                let w2 = waker.clone();
                move || scan_wifi(&w2)
            });
        }
        ("network", "join") => {
            // rest = "<secure 0|1>:<ssid>"; the password arrives in `arg` for a secure network.
            let (secure, ssid) = rest.split_once(':').map(|(a, b)| (a == "1", b.to_string())).unwrap_or((false, rest.clone()));
            let _ = secure;
            let w2 = waker.clone();
            edit(|s| { s.net.joining = Some(ssid.clone()); s.net.error = None; }, true);
            spawn(&w, move || {
                let r = sys::netctl::wifi_connect(&ssid, arg.as_deref());
                edit(|s| s.net.joining = None, false);
                match r {
                    Ok(()) => {
                        edit(|s| s.net.net = Some(read_net_status()), true);
                        crate::features::portal::nudge();
                    }
                    Err(m) => {
                        let low = m.to_lowercase();
                        let msg = if ["secret", "password", "802-11-wireless-security"].iter().any(|k| low.contains(k)) { format!("Couldn't join {ssid}. Check the password and try again.") } else { failed(&format!("Couldn't join {ssid}."), &m, None) };
                        edit(|s| s.net.error = Some(msg), true);
                    }
                }
                scan_wifi(&w2);
            });
        }
        ("network", "forget") => {
            let ssid = if rest.is_empty() { b.to_string() } else { rest.clone() };
            let w2 = waker.clone();
            spawn(&w, move || {
                if let Err(e) = sys::netctl::wifi_forget(&ssid) {
                    edit(|s| s.net.error = Some(failed("Couldn't forget that network.", &e, None)), true);
                }
                edit(|s| s.net.net = Some(read_net_status()), true);
                scan_wifi(&w2);
            });
        }
        ("network", "bluetooth-on") => {
            let w2 = waker.clone();
            spawn(&w, move || {
                if let Err(e) = sys::netctl::bt_power(true) {
                    toast(failed("Couldn't turn Bluetooth on.", &e, None), true);
                }
                refresh_bt_devices(&w2);
            });
        }
        ("network", "bt-scan") => {
            edit(|s| s.net.bt_scanning = true, true);
            let w2 = waker.clone();
            spawn(&w, move || {
                let devs: Vec<BtDev> = sys::netctl::bt_scan(8).into_iter().map(|d| BtDev { addr: d.addr, name: d.name, paired: d.paired, connected: d.connected }).collect();
                edit(|s| { s.net.bt_scanning = false; s.net.bt_devices = devs; }, true);
                w2();
            });
        }
        ("network", op @ ("bt-pair" | "bt-connect" | "bt-disconnect" | "bt-forget")) => {
            let addr = rest.clone();
            let op = op.to_string();
            let w2 = waker.clone();
            edit(|s| s.net.bt_busy = Some(addr.clone()), true);
            spawn(&w, move || {
                let name = SHARED.lock().ok().and_then(|s| s.snap.clone()).and_then(|s| s.net.bt_devices.iter().find(|d| d.addr == addr).map(|d| d.name.clone())).unwrap_or_else(|| "the device".into());
                let r = match op.as_str() {
                    "bt-disconnect" => sys::netctl::bt_disconnect(&addr),
                    "bt-forget" => sys::netctl::bt_forget(&addr),
                    _ => sys::netctl::bt_pair(&addr),
                };
                match (&r, op.as_str()) {
                    (Ok(()), "bt-forget") => toast(format!("Forgot {name}"), false),
                    (Ok(()), "bt-disconnect") => toast(format!("Disconnected {name}"), false),
                    (Ok(()), _) => toast(format!("Connected to {name}"), false),
                    (Err(e), "bt-forget") => toast(failed("Couldn't forget that device.", e, None), true),
                    (Err(e), _) => toast(failed(&format!("Couldn't connect to {name}."), e, Some("Put it in pairing mode and try again.")), true),
                }
                edit(|s| s.net.bt_busy = None, false);
                refresh_bt_devices(&w2);
            });
        }
        ("sound", op @ ("output" | "mic-mute" | "mic-level" | "louder")) => sound_action(op, rest.clone(), &waker),
        ("ssh", op) => ssh_action(op, rest.clone(), arg, &waker),
        ("netapi", op) => netapi_action(op, rest.clone(), &waker),
        ("time" | "logs" | "reset", _) => return super::support::run(act, waker),
        _ => return Ran::NotMine,
    }
    Ran::Started
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_adds_the_reason_and_hint() {
        assert_eq!(failed("Couldn't do it.", "offline.", Some("Try again.")), "Couldn't do it. offline. Try again.");
        assert_eq!(failed("Couldn't do it.", "", None), "Couldn't do it.");
    }

    #[test]
    fn not_enabled_means_fixtures() {
        assert!(get().is_none());
    }
}
