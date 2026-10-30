//! The device services the Tauri launcher starts once its window is up (src-tauri lib.rs, the
//! background services block), for the native launcher. Only the device starts them (main.rs,
//! next to `ingame::install`), never fixtures or a bench build.
//!
//! - Power upkeep (`system::keep`): the launcher's or the running game's power settings, applied
//!   again after a resume, a charger change or a firmware reset. Per-game power: `game_started`
//!   (launch) and the runner's running set (`webgame::WebWindows::power_running`).
//! - Wake (`sleepwatch`): a `system-resume` event; the app refreshes quietly (`on_resume`).
//! - Gyro in Steam games and the IMU rate (`rare_input::gyro`): the IMU drops to its idle rate
//!   while nothing takes motion.
//! - Wi-Fi power save off while a game or app has the screen (`system::wifi`).
//! - The library watcher: installs, uninstalls and new shortcuts show up without a rescan.
//! - Idle (lib/idleDim.ts, lib/sleep.ts): dim after a minute and sleep after the Power section's
//!   minutes, never while a game, an app, audio or a download runs, nor in first run or sign-in.
//! - The launcher-level watchers (screens/notify/watchers.rs): low battery at 15% and 5%, an OS
//!   update ready to install, and the docked toast.

use crate::screens::notify::backend::BatterySource;
use crate::screens::notify::data::battery::BatteryReading;
use crate::screens::notify::data::LocalNotice;
use crate::screens::notify::watchers::{BatteryNotice, DockWatch, OsUpdateNotice, OS_POLL_SECS};
use crate::screens::settings::config::Config;
use crate::screens::settings::display::DisplayState;
use crate::screens::settings::power::{idle_limit_min, should_sleep, SleepInputs};
use crate::screens::settings::update::OsUpdate;
use rare_system as sys;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Idle this long dims the screen (when Settings > Display > Dim when idle is on).
const DIM_AFTER: Duration = Duration::from_secs(60);
/// Below this the sleep check reads nothing (the shortest sleep choice is 5 minutes).
const SLEEP_CHECK_AFTER: Duration = Duration::from_secs(5 * 60);
/// The watcher's tick.
const TICK: Duration = Duration::from_secs(3);
/// Received bytes per second above which something is downloading (Steam, the Store, an OS update).
const DOWNLOAD_BPS: u64 = 200 * 1024;

static STARTED: AtomicBool = AtomicBool::new(false);
/// ms since `epoch()` of the last input (or of something that keeps the device awake).
static LAST_INPUT: AtomicU64 = AtomicU64::new(0);
static DIMMED: AtomicBool = AtomicBool::new(false);
/// First run or sign-in is on screen (set by the app every update).
static SETUP: AtomicBool = AtomicBool::new(false);

/// What the watcher thread has for the app: notices for the centre and plain toasts.
#[derive(Default)]
struct Out {
    notices: Vec<LocalNotice>,
    toasts: Vec<String>,
}
static OUT: Mutex<Out> = Mutex::new(Out { notices: Vec::new(), toasts: Vec::new() });

fn epoch() -> Instant {
    static E: OnceLock<Instant> = OnceLock::new();
    *E.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

fn idle_for() -> Duration {
    Duration::from_millis(now_ms().saturating_sub(LAST_INPUT.load(Ordering::Relaxed)))
}

fn runner() -> &'static rare_launch::launch::Runner {
    crate::data::backend::runner()
}

/// Input reached the launcher (any pad event, a key, the pointer): the idle clock starts again and
/// a dimmed screen comes back.
pub fn touch() {
    LAST_INPUT.store(now_ms(), Ordering::Relaxed);
    if DIMMED.swap(false, Ordering::Relaxed) {
        std::thread::spawn(|| {
            if let Err(e) = sys::brightness::idle(false) {
                log::warn!("idle dim off: {e}");
            }
        });
    }
}

/// First run or sign-in is up: no idle sleep.
pub fn set_setup(on: bool) {
    SETUP.store(on, Ordering::Relaxed);
}

/// A notice from any thread for the centre (posted on the next update).
pub fn post(n: LocalNotice) {
    if let Ok(mut o) = OUT.lock() {
        o.notices.push(n);
    }
}

/// Notices the watchers raised since the last call (the app posts them through Social).
pub fn take_notices() -> Vec<LocalNotice> {
    OUT.lock().map(|mut o| std::mem::take(&mut o.notices)).unwrap_or_default()
}

/// Plain toasts (the dock change) since the last call.
pub fn take_toasts() -> Vec<String> {
    OUT.lock().map(|mut o| std::mem::take(&mut o.toasts)).unwrap_or_default()
}

/// Every update: what the watchers raised goes to the notice centre (Social posts it as a toast,
/// over a game through the HUD), or to the toast line in fixtures.
pub fn deliver(st: &mut crate::state::State, social: Option<&crate::social::Social>, now: f64) {
    for n in take_notices() {
        match social {
            Some(s) => s.notify_local(n),
            None => st.toast(n.title, crate::state::Tone::Info, now),
        }
    }
    for t in take_toasts() {
        st.toast(t, crate::state::Tone::Info, now);
    }
}

/// The launcher config, read again only when the file changed.
fn config() -> Config {
    static C: Mutex<Option<(Option<std::time::SystemTime>, Config)>> = Mutex::new(None);
    let path = rare_core::paths::config_file();
    let m = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut c = C.lock().unwrap_or_else(|e| e.into_inner());
    match c.as_ref() {
        Some((at, cfg)) if *at == m => cfg.clone(),
        _ => {
            let cfg = crate::screens::settings::config::load();
            *c = Some((m, cfg.clone()));
            cfg
        }
    }
}

/// Rare started this game (backend.rs launch): its power settings go on now (saved for it, or
/// Balanced), then the game-load CPU boost window when the launch asked for one.
pub fn game_started(id: &str, boost: bool, boost_target: Option<u64>) {
    let then: Box<dyn FnOnce() + Send> = if boost {
        let id = id.to_string();
        Box::new(move || sys::system::load_boost(id, boost_target, |id| runner().running().iter().any(|r| r == id)))
    } else {
        Box::new(|| {})
    };
    sys::power::started(crate::data::backend::power_load, id, then);
}

/// The runner's running set changed: the next game's power settings, or the launcher's own.
pub fn running_changed(now: &[String]) {
    if STARTED.load(Ordering::Relaxed) {
        sys::power::running(crate::data::backend::power_load, now);
    }
}

struct Gyro;

impl rare_input::gyro::Host for Gyro {
    fn enabled(&self) -> bool {
        let c = config();
        c.gyro_in_steam == Some(true) && c.gyro_enabled != Some(false)
    }
    fn running(&self) -> Vec<String> {
        runner().running()
    }
    fn gamescope_active(&self) -> bool {
        rare_overlay::gamescope::active()
    }
    fn focused_app(&self) -> Option<u64> {
        rare_overlay::gamescope::focused_app()
    }
}

/// Starts everything above, a moment after the window is up so the first scan and frame have the
/// disk and the cores to themselves (the Tauri launcher waits 2.5 s too).
pub fn start() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    touch();
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(2500));
        let t = Instant::now();
        // The game whose power applies, for whatever already runs (a launcher restart mid-game).
        sys::power::running(crate::data::backend::power_load, &runner().running());
        sys::system::keep(|| sys::power::wanted_now(&crate::data::backend::power_load()));
        sys::sleepwatch::start(crate::features::bus::ctx().sink);
        rare_input::gyro::start(Gyro);
        sys::system::wifi::start(|| (rare_launch::apps::front::front(), runner().running()));
        rare_library::watch::start(crate::features::bus::ctx(), || config().game_folders);
        std::thread::Builder::new().name("rare-idle".into()).spawn(watch).ok();
        std::thread::Builder::new().name("rare-input-front".into()).spawn(front_watch).ok();
        log::info!("device services started ({} ms)", t.elapsed().as_millis());
    });
}

/// What has the screen changed (a game started or ended, back to Rare, the Switcher): an input
/// reset, so nothing held from before (the game's stick, the last value of a pad that went with
/// it) repeats after. Polled: the hand-over count is one atomic.
fn front_watch() {
    let mut seen = rare_launch::apps::front::generation();
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let now = rare_launch::apps::front::generation();
        if now != seen {
            seen = now;
            rare_input::reset::request("what has the screen changed");
        }
    }
}

/// The device woke up: the idle clock starts again and the screen is back to its level. The
/// power settings come back through `system::keep`, the app refreshes its data.
pub fn on_resume() {
    touch();
    // Nothing held before the sleep may repeat after it.
    rare_input::reset::request("resume");
    // A Rare surface that was up keeps the pad (InputPlumber may have restarted its devices).
    rare_input::ip::surface(crate::features::ingame::surface());
}

/// Bytes received on every interface but loopback.
fn rx_bytes() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/net/dev").ok()?;
    Some(
        s.lines()
            .skip(2)
            .filter_map(|l| {
                let (name, rest) = l.split_once(':')?;
                (name.trim() != "lo").then(|| rest.split_whitespace().next()?.parse::<u64>().ok()).flatten()
            })
            .sum(),
    )
}

struct SysBattery;

impl BatterySource for SysBattery {
    fn reading(&mut self) -> Option<BatteryReading> {
        sys::system::battery().map(|b| BatteryReading { percent: b.percent as f64, charging: b.charging })
    }
}

fn display_state() -> DisplayState {
    let d = sys::display::display_state();
    DisplayState {
        outputs: d.outputs.iter().map(|o| crate::screens::settings::display::DisplayOutput { name: o.name.clone(), internal: o.internal, connected: o.connected }).collect(),
        active: d.active,
        docked: d.docked,
        dock_name: d.dock_name,
        dock_mode: d.dock_mode,
    }
}

fn os_update() -> OsUpdate {
    OsUpdate::from_sys(sys::osupdate::status())
}

fn push(f: impl FnOnce(&mut Out)) {
    if let Ok(mut o) = OUT.lock() {
        f(&mut o);
    }
    crate::features::bus::wake();
}

/// The idle and watcher loop.
fn watch() {
    let mut battery = BatteryNotice::default();
    let mut dock = DockWatch::default();
    let mut os = OsUpdateNotice::default();
    let mut next_os = Instant::now() + Duration::from_secs(30);
    let mut rx = rx_bytes();
    let mut rx_at = Instant::now();
    loop {
        std::thread::sleep(TICK);
        // Low battery, a TV plugged in or out.
        if let Some(n) = battery.poll(&mut SysBattery) {
            push(|o| o.notices.push(n));
        }
        if let Some(t) = dock.on_state(display_state()) {
            push(|o| o.toasts.push(t.text));
        }
        let launcher = rare_launch::apps::front::launcher_front() && !rare_webgame::visible();
        let running = !runner().running().is_empty();
        // An OS update ready: read while the launcher has the screen (rpm-ostree is not free).
        if launcher && !running && Instant::now() >= next_os {
            next_os = Instant::now() + Duration::from_secs_f64(OS_POLL_SECS);
            if let Some(n) = os.on_update(&os_update()) {
                push(|o| o.notices.push(n));
            }
        }
        // Idle: a game, an app or Steam on screen, a download, or setup keeps the device awake.
        let now_rx = rx_bytes();
        let secs = rx_at.elapsed().as_secs_f64().max(0.5);
        let downloading = matches!((rx, now_rx), (Some(a), Some(b)) if (b.saturating_sub(a) as f64 / secs) as u64 > DOWNLOAD_BPS);
        rx = now_rx;
        rx_at = Instant::now();
        if running || !launcher || downloading || SETUP.load(Ordering::Relaxed) {
            touch();
            continue;
        }
        let idle = idle_for();
        if idle < DIM_AFTER {
            continue;
        }
        let state = sys::sleepwatch::state();
        if state.audio_playing {
            touch();
            continue;
        }
        if !DIMMED.load(Ordering::Relaxed) && sys::brightness::brightness_info().dim_idle {
            match sys::brightness::idle(true) {
                Ok(()) => DIMMED.store(true, Ordering::Relaxed),
                Err(e) => log::warn!("idle dim: {e}"),
            }
        }
        if idle < SLEEP_CHECK_AFTER {
            continue;
        }
        let limit_min = idle_limit_min(&config(), state.on_battery);
        let i = SleepInputs { idle_ms: idle.as_millis() as f64, limit_min, running: 0, audio_playing: false, setup: false };
        if should_sleep(&i) {
            log::info!("idle {} min (limit {limit_min}): sleeping", idle.as_secs() / 60);
            LAST_INPUT.store(now_ms(), Ordering::Relaxed);
            if let Err(e) = sys::system::power("suspend") {
                log::warn!("idle sleep: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_resets_the_idle_clock() {
        LAST_INPUT.store(0, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(5));
        touch();
        assert!(idle_for() < Duration::from_millis(5));
    }
}
