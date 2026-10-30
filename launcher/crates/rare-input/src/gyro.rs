//! Gyro in Steam games. InputPlumber binds the SuiPlay's IMU, but its default pad target
//! (`xbox-elite`) carries no motion, so Steam sees none. The `deck-uhid` target looks like a
//! Steam Deck to Steam and carries motion. So while a Steam game runs (or has the screen) and the
//! Steam client is up, the composite device's targets are switched to `deck-uhid`, and back to
//! `xbox-elite` otherwise.
//!
//! The Deck pad exists only as a hidraw node (deck.rs: InputPlumber's product id is one
//! hid-steam does not bind), so only Steam and the launcher's own hidraw reader see it. Every app
//! that is not a Steam game therefore needs the Xbox pad, and the switch is only for a real Steam
//! game with the Steam client running: never Steam's own UI, never a program Rare started itself
//! (its app id has the top bit set, overlay/appid.rs), Chrome, an emulator or the launcher. After
//! a switch to the Deck pad the node must show up within VERIFY, or the pad goes straight back
//! to Xbox and the switch backs off until the wanted target changes.
//!
//! `SetTargetDevices` on `org.shadowblip.Input.CompositeDevice` re-creates the virtual pad, so
//! the switch is debounced, rate limited and gives up after repeated failures; the decision is a
//! pure state machine (`Switch`), unit-tested below. D-Bus access needs polkit's
//! `org.shadowblip.Input.CompositeDevice.SetTargetDevices`, which InputPlumber's policy sets to
//! auth_admin; image/system_files/usr/share/polkit-1/rules.d/50-rare-inputplumber.rules allows
//! the active local session.
//!
//! The IMU read rate follows the applied target: InputPlumber reads the bmi323 at 200 Hz through
//! an IIO hrtimer trigger whatever the target, about 6 % of a core on the idle home screen, and
//! only `deck-uhid` carries motion. So the trigger runs at `IMU_FULL_HZ` while the target is the
//! Deck and `IMU_IDLE_HZ` otherwise, set through `pkexec /usr/libexec/rare/imu-rate full|idle`
//! (polkit `org.rare.imu.rate`, active local session). The trigger's rate is world-readable, so
//! the loop checks it every `IMU_CHECK` and puts it back after an InputPlumber restart.
//!
//! The setting ("Gyro in Steam games") is off unless turned on: the hidraw path is unit-tested
//! but not yet proven on the device.

use std::time::{Duration, Instant};

/// IMU trigger rate while the pad target carries motion (InputPlumber's profile rate).
pub const IMU_FULL_HZ: f64 = 200.0;
/// IMU trigger rate otherwise. InputPlumber warns after 2 s without a sample, so not below 1.
pub const IMU_IDLE_HZ: f64 = 2.0;
/// How often the trigger rate is read back.
pub const IMU_CHECK: Duration = Duration::from_secs(10);

/// The helper argument that brings the IMU to the rate for `applied`, or None when `now_hz` (the
/// trigger's sampling_frequency, None when there is no trigger) already matches.
pub fn imu_action(applied: Target, now_hz: Option<f64>) -> Option<&'static str> {
    let (want, arg) = match applied {
        Target::Deck => (IMU_FULL_HZ, "full"),
        Target::Xbox => (IMU_IDLE_HZ, "idle"),
    };
    match now_hz {
        Some(hz) if (hz - want).abs() > 0.5 => Some(arg),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// `xbox-elite`: what every launcher, emulator and game expects; no motion.
    Xbox,
    /// `deck-uhid`: a Steam Deck to Steam, with motion.
    Deck,
}

impl Target {
    /// The InputPlumber target ids for the composite device (pad, then mouse and keyboard).
    pub fn ids(self) -> [&'static str; 3] {
        match self {
            Target::Xbox => ["xbox-elite", "mouse", "keyboard"],
            Target::Deck => ["deck-uhid", "mouse", "keyboard"],
        }
    }
}

/// What the launcher knows each tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// The "Gyro in Steam games" setting.
    pub enabled: bool,
    /// The Steam client process exists.
    pub steam_client: bool,
    /// A real Steam game Rare started through Steam (`steam:<appid>` in the runner) is running
    /// or starting.
    pub steam_game: bool,
    /// The app id gamescope has on screen (`GAMESCOPE_FOCUSED_APP`), when known.
    pub focused_app: Option<u64>,
    /// An emulator game Rare started (`emu:` run) is running and no Steam game has the screen:
    /// emulators bind the Xbox pad (rare_emu::controls), so the target stays Xbox under them.
    pub emu_front: bool,
}

/// Steam's own UI in gamescope.
const STEAM_UI_APPID: u64 = 769;
/// The launcher's own gamescope app id (`STEAM_GAME` on its window, overlay/gamescope.rs).
pub const RARE_APPID: u32 = 0x5241_5245;

/// A real Steam app id: not 0, not Steam's UI, not the launcher's own id, and not in the
/// non-Steam shortcut range (top bit set), where every program Rare starts itself lives.
pub fn is_steam_game_appid(id: u64) -> bool {
    id != 0 && id != STEAM_UI_APPID && id != RARE_APPID as u64 && id < 0x8000_0000
}

/// The Steam game in a run id: `steam:<appid>` with a real Steam app id (shortcuts are
/// `shortcut:<id>`, apps `app:`, emulators `emu:`, web games `web:`).
pub fn steam_game_run(run_id: &str) -> bool {
    run_id.strip_prefix("steam:").and_then(|n| n.parse::<u64>().ok()).is_some_and(is_steam_game_appid)
}

/// The target wanted for these inputs.
pub fn desired(i: Inputs) -> Target {
    if !i.enabled || !i.steam_client || i.emu_front {
        return Target::Xbox;
    }
    if i.steam_game || i.focused_app.is_some_and(is_steam_game_appid) {
        Target::Deck
    } else {
        Target::Xbox
    }
}

/// A wanted target has to hold this long before the pad is re-created (a game's launch flickers
/// the front, the overlay opens and closes).
pub const SETTLE: Duration = Duration::from_millis(1500);
/// Least time between two switches: each one drops the pad for a moment.
pub const MIN_GAP: Duration = Duration::from_secs(4);
/// Wait after a failed call before trying again.
pub const RETRY: Duration = Duration::from_secs(10);
/// After this many failures in a row for one wanted target the switch gives up until the wanted
/// target changes (no InputPlumber, or no permission: do not hammer D-Bus).
pub const MAX_FAILS: u32 = 3;
/// After a switch to the Deck pad, its hidraw node must exist within this long.
pub const VERIFY: Duration = Duration::from_secs(2);

pub struct Switch {
    /// The target believed to be applied.
    applied: Target,
    /// The wanted target and since when it has been wanted without a change.
    want: Option<(Target, Instant)>,
    last_switch: Option<Instant>,
    retry_at: Option<Instant>,
    fails: u32,
}

impl Switch {
    pub fn new(applied: Target) -> Self {
        Switch { applied, want: None, last_switch: None, retry_at: None, fails: 0 }
    }

    pub fn applied(&self) -> Target {
        self.applied
    }

    /// One tick: returns the target to apply now, if any. Report the result with `done`.
    pub fn step(&mut self, now: Instant, want: Target) -> Option<Target> {
        match self.want {
            Some((w, _)) if w == want => {}
            _ => {
                // A new wish: failures of the old one say nothing about this one.
                self.want = Some((want, now));
                self.fails = 0;
                self.retry_at = None;
            }
        }
        if want == self.applied || self.fails >= MAX_FAILS {
            return None;
        }
        let since = self.want.map(|(_, t)| t).unwrap_or(now);
        if now.saturating_duration_since(since) < SETTLE {
            return None;
        }
        if self.last_switch.is_some_and(|t| now.saturating_duration_since(t) < MIN_GAP) {
            return None;
        }
        if self.retry_at.is_some_and(|t| now < t) {
            return None;
        }
        Some(want)
    }

    /// The result of applying `t`.
    pub fn done(&mut self, now: Instant, t: Target, ok: bool) {
        if ok {
            self.applied = t;
            self.last_switch = Some(now);
            self.fails = 0;
            self.retry_at = None;
        } else {
            self.fails += 1;
            self.retry_at = Some(now + RETRY);
        }
    }

    pub fn gave_up(&self) -> bool {
        self.fails >= MAX_FAILS
    }

    /// The Deck pad never showed up (or went away) and the target was put back to Xbox at
    /// `now`: no Deck again until the wanted target changes.
    pub fn reverted(&mut self, now: Instant) {
        self.applied = Target::Xbox;
        self.last_switch = Some(now);
        self.fails = MAX_FAILS;
        self.retry_at = None;
    }
}

/// A "Steam Deck" evdev pad (hid-steam bound to the Deck pad: a real Deck, or an InputPlumber
/// that uses the Deck's own product id).
pub fn deck_pad_present(proc_input_devices: &str) -> bool {
    proc_input_devices.lines().any(|l| l.starts_with("N: Name=") && l.to_lowercase().contains("steam deck") && !l.to_lowercase().contains("motion"))
}

/// The Deck pad exists: InputPlumber's hidraw node (deck.rs) or a hid-steam evdev pad. How a
/// restarted launcher finds the target was left on the Deck pad.
#[cfg(target_os = "linux")]
fn deck_pad_exists() -> bool {
    crate::deck::find_node().is_some() || std::fs::read_to_string("/proc/bus/input/devices").is_ok_and(|t| deck_pad_present(&t))
}

/// A pad the launcher really has on the Deck target: its hidraw reader is reading it
/// (deck.rs), or hid-steam made an evdev pad gilrs sees. A node that merely exists is not
/// enough: that is the state that left Home dead on the device.
#[cfg(target_os = "linux")]
fn deck_pad_working() -> bool {
    crate::deck::active() || std::fs::read_to_string("/proc/bus/input/devices").is_ok_and(|t| deck_pad_present(&t))
}

#[cfg(target_os = "linux")]
mod bus {
    use super::Target;
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::Connection;
    use std::time::Duration;

    const DEST: &str = "org.shadowblip.InputPlumber";
    const MANAGER: &str = "/org/shadowblip/InputPlumber/Manager";
    const MANAGER_IFACE: &str = "org.shadowblip.InputManager";
    const DEVICE_IFACE: &str = "org.shadowblip.Input.CompositeDevice";
    const TIMEOUT: Duration = Duration::from_secs(5);

    /// The composite device to switch. Looked up on every call (the index changes when the
    /// device is re-created): the one named "SuiPlay ..." among the gamepad composites, else the
    /// first of them.
    fn composite_path(conn: &Connection) -> Result<String, String> {
        let mgr = conn.with_proxy(DEST, MANAGER, TIMEOUT);
        let mut paths: Vec<String> = mgr.get(MANAGER_IFACE, "GamepadOrder").map_err(|e| format!("GamepadOrder: {e}"))?;
        if paths.is_empty() {
            paths = (0..4).map(|i| format!("/org/shadowblip/InputPlumber/CompositeDevice{i}")).collect();
        }
        for p in &paths {
            let name: Result<String, _> = conn.with_proxy(DEST, p.as_str(), TIMEOUT).get(DEVICE_IFACE, "Name");
            if name.is_ok_and(|n| n.starts_with("SuiPlay")) {
                return Ok(p.clone());
            }
        }
        paths.into_iter().next().ok_or_else(|| "no composite device".to_string())
    }

    /// The composite device's name and current target ids, for Settings > Controls.
    pub fn status() -> Result<(String, Vec<String>), String> {
        let conn = Connection::new_system().map_err(|e| format!("system bus: {e}"))?;
        let path = composite_path(&conn)?;
        let p = conn.with_proxy(DEST, path.as_str(), TIMEOUT);
        let name: String = p.get(DEVICE_IFACE, "Name").map_err(|e| format!("Name: {e}"))?;
        let targets: Vec<String> = p.get(DEVICE_IFACE, "TargetDevices").map_err(|e| format!("TargetDevices: {e}"))?;
        Ok((name, targets))
    }

    pub fn set_target(t: Target) -> Result<(), String> {
        let conn = Connection::new_system().map_err(|e| format!("system bus: {e}"))?;
        let path = composite_path(&conn)?;
        let ids: Vec<String> = t.ids().iter().map(|s| s.to_string()).collect();
        let r: Result<(), dbus::Error> = conn.with_proxy(DEST, path.as_str(), TIMEOUT).method_call(DEVICE_IFACE, "SetTargetDevices", (ids,));
        r.map_err(|e| format!("SetTargetDevices on {path}: {e}"))
    }
}

/// The sampling_frequency of the trigger InputPlumber reads the bmi323 with, if any.
#[cfg(target_os = "linux")]
fn imu_trigger_hz() -> Option<f64> {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    for dev in std::fs::read_dir("/sys/bus/iio/devices").ok()?.flatten() {
        let d = dev.path();
        if read(d.join("name")).as_deref() != Some("bmi323-imu") {
            continue;
        }
        let cur = read(d.join("trigger/current_trigger")).filter(|c| !c.is_empty())?;
        for t in std::fs::read_dir("/sys/bus/iio/devices").ok()?.flatten() {
            if read(t.path().join("name")).as_deref() == Some(cur.as_str()) {
                return read(t.path().join("sampling_frequency"))?.parse().ok();
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn imu_rate(arg: &str) -> Result<(), String> {
    rare_system::system::run("pkexec", &["--disable-internal-agent", "/usr/libexec/rare/imu-rate", arg]).map(|_| ())
}

/// What the watcher asks the launcher every tick.
pub trait Host: Send + 'static {
    /// The "Gyro in Steam games" setting is on.
    fn enabled(&self) -> bool;
    /// The run ids Rare's runner holds.
    fn running(&self) -> Vec<String>;
    /// gamescope is the compositor.
    fn gamescope_active(&self) -> bool;
    /// The app id gamescope has on screen.
    fn focused_app(&self) -> Option<u64>;
}

/// Starts the watcher thread (Linux only; nothing elsewhere).
#[cfg(target_os = "linux")]
pub fn start(host: impl Host) {
    std::thread::spawn(move || {
        let initial = if deck_pad_exists() { Target::Deck } else { Target::Xbox };
        let mut sw = Switch::new(initial);
        let mut said_giving_up = false;
        let imu_helper = std::path::Path::new("/usr/libexec/rare/imu-rate").exists();
        let mut imu_checked: Option<Instant> = None;
        let mut imu_fails = 0u32;
        // Steam and gamescope are asked once a second, not every tick.
        let mut steam_client = false;
        let mut focused_app = None;
        let mut looked: Option<Instant> = None;
        let mut missing_since: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let enabled = host.enabled();
            let running = host.running();
            let steam_game = running.iter().any(|id| steam_game_run(id));
            let emu_running = running.iter().any(|id| id.starts_with("emu:"));
            if !looked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
                looked = Some(Instant::now());
                steam_client = (enabled || sw.applied() == Target::Deck) && rare_core::proc::steam_running();
                focused_app = if enabled && steam_client && host.gamescope_active() { host.focused_app() } else { None };
            }
            let emu_front = emu_running && !focused_app.is_some_and(is_steam_game_appid);
            let inputs = Inputs { enabled, steam_client, steam_game, focused_app, emu_front };
            let now = Instant::now();
            if let Some(t) = sw.step(now, desired(inputs)) {
                let r = bus::set_target(t);
                match &r {
                    Ok(()) => {
                        crate::log(&format!("[gyro] input target now {:?} ({}) for {inputs:?}", t, t.ids()[0]));
                        // The pad is re-created: nothing held on the old one may repeat.
                        crate::reset::request("pad target");
                    }
                    Err(e) => crate::log(&format!("[gyro] switching to {:?} failed: {e}", t)),
                }
                sw.done(Instant::now(), t, r.is_ok());
                said_giving_up = false;
                imu_checked = None;
                missing_since = None;
                if r.is_ok() && t == Target::Deck {
                    // The Deck pad has to be there, readable, within VERIFY, or the launcher
                    // and every app are left without a controller.
                    let t0 = Instant::now();
                    while !deck_pad_working() && t0.elapsed() < VERIFY {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    if !deck_pad_working() {
                        revert(&mut sw, "the launcher has no Deck pad within 2 s");
                        imu_checked = None;
                    }
                }
            }
            // On the Deck pad, it must stay there (InputPlumber restarted, the node went away).
            if sw.applied() == Target::Deck {
                if deck_pad_working() {
                    missing_since = None;
                } else if missing_since.get_or_insert(now).elapsed() >= VERIFY {
                    missing_since = None;
                    revert(&mut sw, "the Deck pad went away");
                    imu_checked = None;
                }
            }
            // IMU rate follows the applied target (checked now after a switch, else every 10 s).
            if imu_helper && imu_fails < MAX_FAILS && imu_checked.is_none_or(|c| now.saturating_duration_since(c) >= IMU_CHECK) {
                imu_checked = Some(now);
                if let Some(arg) = imu_action(sw.applied(), imu_trigger_hz()) {
                    match imu_rate(arg) {
                        Ok(()) => {
                            imu_fails = 0;
                            crate::log(&format!("[gyro] imu rate {arg}"));
                        }
                        Err(e) => {
                            imu_fails += 1;
                            crate::log(&format!("[gyro] imu rate {arg} failed: {e}"));
                        }
                    }
                }
            }
            if sw.gave_up() && !said_giving_up {
                said_giving_up = true;
                crate::log("[gyro] giving up until the wanted target changes");
            }
        }
    });
}

/// Puts the Xbox pad back at once and backs off (`Switch::reverted`). When that call fails the
/// target stays Deck, so the presence check tries again.
#[cfg(target_os = "linux")]
fn revert(sw: &mut Switch, why: &str) {
    match bus::set_target(Target::Xbox) {
        Ok(()) => {
            crate::log(&format!("[gyro] {why}: input target back to Xbox (xbox-elite), no Deck again until the game changes"));
            crate::reset::request("pad target");
            sw.reverted(Instant::now());
        }
        Err(e) => crate::log(&format!("[gyro] {why}: switching back to Xbox failed: {e}")),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn start(_host: impl Host) {}

#[cfg(test)]
mod tests {
    use super::*;


    /// Setting on, Steam client up, nothing running, nothing known on screen.
    fn base() -> Inputs {
        Inputs { enabled: true, steam_client: true, steam_game: false, focused_app: None, emu_front: false }
    }

    #[test]
    fn an_emulator_in_front_keeps_the_xbox_pad() {
        // A Steam game still running behind a ROM game: the emulator has the screen, Xbox it is.
        assert_eq!(desired(Inputs { steam_game: true, emu_front: true, ..base() }), Target::Xbox);
        assert_eq!(desired(Inputs { steam_game: true, emu_front: false, ..base() }), Target::Deck);
    }

    /// What Rare's runner holds for these run ids.
    fn game_in(running: &[&str]) -> bool {
        running.iter().any(|r| steam_game_run(r))
    }

    #[test]
    fn steam_game_with_steam_running_gets_the_deck_pad() {
        // Started from Rare (the runner has it), on screen or behind the launcher or overlay.
        assert_eq!(desired(Inputs { steam_game: game_in(&["steam:620"]), focused_app: Some(620), ..base() }), Target::Deck);
        assert_eq!(desired(Inputs { steam_game: game_in(&["steam:620"]), focused_app: Some(RARE_APPID as u64), ..base() }), Target::Deck);
        // Started from Steam's own UI: only gamescope knows, by its app id on screen.
        assert_eq!(desired(Inputs { focused_app: Some(1245620), ..base() }), Target::Deck);
        // Setting off: never.
        assert_eq!(desired(Inputs { enabled: false, steam_game: true, focused_app: Some(620), ..base() }), Target::Xbox);
    }

    #[test]
    fn steam_big_picture_is_not_a_game() {
        assert_eq!(desired(Inputs { focused_app: Some(769), ..base() }), Target::Xbox);
        assert_eq!(desired(Inputs { focused_app: Some(0), ..base() }), Target::Xbox);
        assert_eq!(desired(base()), Target::Xbox);
    }

    #[test]
    fn no_steam_process_means_no_deck_pad() {
        // A stale steam: run (Steam still starting, or it quit) or a stale focused id.
        let i = Inputs { steam_client: false, steam_game: game_in(&["steam:620"]), focused_app: Some(620), ..base() };
        assert_eq!(desired(i), Target::Xbox);
    }

    #[test]
    fn steam_run_ids() {
        assert!(steam_game_run("steam:620"));
        assert!(!steam_game_run("steam:769"));
        assert!(!steam_game_run("steam:0"));
        assert!(!steam_game_run(&format!("steam:{}", 0x8000_0001u64)));
        assert!(!steam_game_run("shortcut:620"));
        assert!(!steam_game_run("app:steam"));
        assert!(!steam_game_run("steam:abc"));
    }

    #[test]
    fn a_missing_deck_pad_reverts_and_backs_off() {
        let t0 = Instant::now();
        let mut s = Switch::new(Target::Xbox);
        s.step(t0, Target::Deck);
        let t1 = t0 + SETTLE;
        assert_eq!(s.step(t1, Target::Deck), Some(Target::Deck));
        s.done(t1, Target::Deck, true);
        // No pad within VERIFY: back to Xbox, and no new try while the same game is wanted.
        s.reverted(t1 + VERIFY);
        assert_eq!(s.applied(), Target::Xbox);
        assert!(s.gave_up());
        assert_eq!(s.step(t1 + Duration::from_secs(600), Target::Deck), None);
        // The game ends, a new one starts: one more try.
        assert_eq!(s.step(t1 + Duration::from_secs(601), Target::Xbox), None);
        s.step(t1 + Duration::from_secs(602), Target::Deck);
        assert_eq!(s.step(t1 + Duration::from_secs(602) + SETTLE, Target::Deck), Some(Target::Deck));
    }

    #[test]
    fn settles_before_switching_and_back() {
        let t0 = Instant::now();
        let mut s = Switch::new(Target::Xbox);
        assert_eq!(s.step(t0, Target::Xbox), None);
        assert_eq!(s.step(t0, Target::Deck), None); // just wanted
        assert_eq!(s.step(t0 + SETTLE - Duration::from_millis(1), Target::Deck), None);
        let t1 = t0 + SETTLE;
        assert_eq!(s.step(t1, Target::Deck), Some(Target::Deck));
        s.done(t1, Target::Deck, true);
        assert_eq!(s.applied(), Target::Deck);
        assert_eq!(s.step(t1 + Duration::from_secs(1), Target::Deck), None); // already there
        // The game ends: back to Xbox after settling and the minimum gap.
        let t2 = t1 + Duration::from_secs(10);
        assert_eq!(s.step(t2, Target::Xbox), None);
        assert_eq!(s.step(t2 + SETTLE, Target::Xbox), Some(Target::Xbox));
    }

    #[test]
    fn a_flicker_never_switches() {
        let t0 = Instant::now();
        let mut s = Switch::new(Target::Deck);
        // The overlay closes and reopens within the settle time: the wish flips back to Deck.
        assert_eq!(s.step(t0, Target::Xbox), None);
        assert_eq!(s.step(t0 + Duration::from_millis(1000), Target::Deck), None);
        assert_eq!(s.step(t0 + Duration::from_millis(2000), Target::Xbox), None); // timer restarted
        assert_eq!(s.step(t0 + Duration::from_millis(3000), Target::Xbox), None);
        assert_eq!(s.step(t0 + Duration::from_millis(3600), Target::Xbox), Some(Target::Xbox));
    }

    #[test]
    fn minimum_gap_between_switches() {
        let t0 = Instant::now();
        let mut s = Switch::new(Target::Xbox);
        s.step(t0, Target::Deck);
        let t1 = t0 + SETTLE;
        assert_eq!(s.step(t1, Target::Deck), Some(Target::Deck));
        s.done(t1, Target::Deck, true);
        // Wanted back at once and held: waits for the gap, not just the settle time.
        s.step(t1 + Duration::from_millis(100), Target::Xbox);
        assert_eq!(s.step(t1 + Duration::from_millis(100) + SETTLE, Target::Xbox), None);
        assert_eq!(s.step(t1 + MIN_GAP, Target::Xbox), Some(Target::Xbox));
    }

    #[test]
    fn failures_retry_slowly_then_give_up_until_the_wish_changes() {
        let t0 = Instant::now();
        let mut s = Switch::new(Target::Xbox);
        s.step(t0, Target::Deck);
        let mut now = t0 + SETTLE;
        for n in 1..=MAX_FAILS {
            assert_eq!(s.step(now, Target::Deck), Some(Target::Deck), "attempt {n}");
            s.done(now, Target::Deck, false);
            assert_eq!(s.applied(), Target::Xbox);
            assert_eq!(s.step(now + Duration::from_secs(1), Target::Deck), None); // backing off
            now += RETRY;
        }
        assert!(s.gave_up());
        assert_eq!(s.step(now + Duration::from_secs(600), Target::Deck), None);
        // The wish changes (and changes again): a fresh start.
        s.step(now + Duration::from_secs(601), Target::Xbox);
        s.step(now + Duration::from_secs(602), Target::Deck);
        assert!(!s.gave_up());
        assert_eq!(s.step(now + Duration::from_secs(602) + SETTLE, Target::Deck), Some(Target::Deck));
    }

    #[test]
    fn imu_rate_follows_the_target() {
        assert_eq!(imu_action(Target::Xbox, Some(200.0)), Some("idle"));
        assert_eq!(imu_action(Target::Xbox, Some(2.0)), None);
        assert_eq!(imu_action(Target::Deck, Some(2.0)), Some("full"));
        assert_eq!(imu_action(Target::Deck, Some(200.0)), None);
        // After suspend the sleep hook may leave 1 Hz for a moment; the loop corrects it.
        assert_eq!(imu_action(Target::Deck, Some(1.0)), Some("full"));
        // No IMU trigger (no InputPlumber, another device): nothing to do.
        assert_eq!(imu_action(Target::Xbox, None), None);
    }

    #[test]
    fn target_ids() {
        assert_eq!(Target::Deck.ids(), ["deck-uhid", "mouse", "keyboard"]);
        assert_eq!(Target::Xbox.ids(), ["xbox-elite", "mouse", "keyboard"]);
    }

    #[test]
    fn finds_a_deck_pad_after_a_restart() {
        let xbox = "N: Name=\"Microsoft X-Box One Elite 2 pad\"\nN: Name=\"InputPlumber Mouse\"\n";
        assert!(!deck_pad_present(xbox));
        let deck = "N: Name=\"InputPlumber Mouse\"\nN: Name=\"Steam Deck\"\nN: Name=\"Steam Deck Motion Sensors\"\n";
        assert!(deck_pad_present(deck));
        assert!(!deck_pad_present("N: Name=\"Steam Deck Motion Sensors\"\n"));
    }
}

/// What InputPlumber says about the pad, for the Controls page. `available` is false off Linux
/// or when InputPlumber is not running; the page then explains instead of failing.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadStatus {
    pub available: bool,
    pub name: Option<String>,
    pub targets: Vec<String>,
    /// The pad currently carries motion (`deck-uhid`).
    pub motion: bool,
}

pub fn input_status() -> PadStatus {
    #[cfg(target_os = "linux")]
    if let Ok((name, targets)) = bus::status() {
        let motion = targets.iter().any(|t| t.contains("deck"));
        return PadStatus { available: true, name: Some(name), targets, motion };
    }
    PadStatus { available: false, name: None, targets: vec![], motion: false }
}
