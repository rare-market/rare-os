//! The SuiPlay's four extra buttons and InputPlumber's intercept mode.
//!
//! The image's InputPlumber profile maps the extra keys to D-Bus actions, never to a pad or
//! keyboard target (`/etc/inputplumber/capability_maps.d/rare_suiplay.yaml`): Rare (Aya Space) is
//! `ui_guide`, Quick is `ui_quick`, LC is `ui_screenshot` and RC is `ui_osk`. InputPlumber sends a
//! D-Bus action only to its `DBusDevice` target, whatever the intercept mode, so Steam and games
//! never see these buttons while Rare OS runs. This module listens to that target's `InputEvent`
//! signal and turns the actions into the launcher's pad events; LC and RC go to side.rs
//! (which owns what they do) and intercepted volume keys to volume.rs.
//!
//! While a Rare surface (the overlay) is over a game, the SuiPlay composite device is set to
//! intercept everything (`Always`): the virtual pad goes quiet, so the game and Steam get no
//! input, and InputPlumber sends the whole pad here as `ui_*` actions instead. Otherwise it is
//! `Pass`, which only diverts a gamepad Guide (InputPlumber then switches itself to `Always`;
//! the reconcile tick puts `Pass` back once no surface is up).
//!
//! Guest mode (Rare inside someone else's Steam Game Mode) has no such profile: no `ui_*`
//! actions arrive, nothing here changes anything, and hold-View stays the way in.
//!
//! Every intercept mode change is an input reset (reset.rs): what the D-Bus path holds is let go.
//! InputPlumber 0.83.0 sends no release for what its D-Bus target holds when the mode goes back
//! to `Pass`, so a direction held over that switch kept repeating here until it was pressed and
//! let go again over D-Bus (the "stuck stick").

use crate::Pad;
use std::time::{Duration, Instant};

/// Rare held this long: the hold action.
pub const RARE_HOLD: Duration = Duration::from_millis(600);
/// Auto-repeat of a held direction while intercepting.
const REPEAT_DELAY: Duration = Duration::from_millis(320);
const REPEAT_EVERY: Duration = Duration::from_millis(90);

/// What an action asks the launcher to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    Press(Pad, bool),
    Release(Pad),
    /// LC or RC pressed or released: side.rs owns what they do (press, hold, settings).
    Side(crate::side::Key, bool),
    /// A volume key (they arrive here only while intercepting; otherwise volume.rs reads them
    /// from InputPlumber's keyboard): the same steps, Rare + volume brightness and HUD bar.
    Volume(crate::volume::Key),
}

/// The pad button for a `ui_*` navigation action. InputPlumber names X (evdev 307, North)
/// `ui_context` and Y (evdev 308, West) `ui_act_on`.
pub fn nav_pad(action: &str) -> Option<Pad> {
    Some(match action {
        "ui_up" => Pad::Up,
        "ui_down" => Pad::Down,
        "ui_left" => Pad::Left,
        "ui_right" => Pad::Right,
        "ui_accept" => Pad::A,
        "ui_back" => Pad::B,
        "ui_context" => Pad::X,
        "ui_act_on" => Pad::Y,
        "ui_l1" => Pad::Lb,
        "ui_r1" => Pad::Rb,
        "ui_l2" => Pad::Lt,
        "ui_r2" => Pad::Rt,
        "ui_option" => Pad::Start,
        "ui_select" => Pad::Select,
        _ => return None,
    })
}

/// Press and hold state for the actions. Pure: time comes in, outputs go out.
pub struct Keys {
    rare_since: Option<Instant>,
    /// Navigation buttons pressed over D-Bus and not released yet.
    down: Vec<Pad>,
    /// The Rare hold already fired (or a chord used the press): no Rare press on release.
    rare_spent: bool,
    held: Option<(Pad, Instant, Instant)>,
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            rare_since: None,
            down: Vec::new(),
            rare_spent: false,
            held: None,
        }
    }
}

impl Keys {
    /// `chord_used`: the pad loop used the held Rare button for a chord (chords::RARE_CHORD_USED).
    pub fn on(&mut self, action: &str, down: bool, now: Instant, chord_used: bool) -> Vec<Out> {
        let mut out = Vec::new();
        match action {
            "ui_guide" => {
                if down {
                    if self.rare_since.is_none() {
                        self.rare_since = Some(now);
                        self.rare_spent = false;
                    }
                } else if self.rare_since.take().is_some() {
                    if !self.rare_spent && !chord_used {
                        out.push(Out::Press(Pad::Guide, false));
                        out.push(Out::Release(Pad::Guide));
                    }
                    self.rare_spent = false;
                }
            }
            "ui_quick" => {
                if down {
                    out.push(Out::Press(Pad::Qam, false));
                } else {
                    out.push(Out::Release(Pad::Qam));
                }
            }
            "ui_screenshot" => out.push(Out::Side(crate::side::Key::Lc, down)),
            "ui_osk" => out.push(Out::Side(crate::side::Key::Rc, down)),
            "ui_volume_up" if down => out.push(Out::Volume(crate::volume::Key::Up)),
            "ui_volume_down" if down => out.push(Out::Volume(crate::volume::Key::Down)),
            a => {
                if let Some(p) = nav_pad(a) {
                    self.down.retain(|d| *d != p);
                    if down {
                        self.down.push(p);
                        out.push(Out::Press(p, false));
                        if matches!(p, Pad::Up | Pad::Down | Pad::Left | Pad::Right) {
                            self.held = Some((p, now + REPEAT_DELAY, now));
                        }
                    } else {
                        out.push(Out::Release(p));
                        if self.held.is_some_and(|(h, _, _)| h == p) {
                            self.held = None;
                        }
                    }
                }
            }
        }
        out
    }

    /// Input changed hands (an intercept mode change, reset.rs): every navigation button and
    /// direction held over D-Bus is released. The D-Bus path can't be read again, and InputPlumber
    /// 0.83.0 never sends these releases itself. The Rare button is its own action and stays.
    pub fn reset(&mut self) -> Vec<Out> {
        self.held = None;
        self.down.drain(..).map(Out::Release).collect()
    }

    /// Holds and repeats that came due.
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(t) = self.rare_since {
            if !self.rare_spent && now.saturating_duration_since(t) >= RARE_HOLD {
                self.rare_spent = true;
                out.push(Out::Press(Pad::GuideHold, false));
            }
        }
        if let Some((p, due, _)) = self.held {
            if now >= due {
                if crate::debug_input() {
                    crate::log(&format!("[ip] repeat {} (held over D-Bus)", p.name()));
                }
                out.push(Out::Press(p, true));
                self.held = Some((p, now + REPEAT_EVERY, now));
            }
        }
        out
    }

    pub fn rare_held(&self) -> bool {
        self.rare_since.is_some()
    }

    /// Something is timing: the loop should wake soon.
    pub fn busy(&self) -> bool {
        self.rare_since.is_some() || self.held.is_some() || !self.down.is_empty()
    }
}

/// InputPlumber's intercept modes (org.shadowblip.Input.CompositeDevice.InterceptMode).
pub const MODE_PASS: u32 = 1;
pub const MODE_ALWAYS: u32 = 2;

/// The mode wanted for the SuiPlay composite device: `surface_up` is a Rare surface over a game,
/// or the pad diverted to a game's touch controls (`divert`).
pub fn wanted_mode(surface_up: bool) -> u32 {
    if surface_up {
        MODE_ALWAYS
    } else {
        MODE_PASS
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::Connection;
    use dbus::message::MatchRule;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc::channel;
    use crate::chords;

    const DEST: &str = "org.shadowblip.InputPlumber";
    const DEVICE_IFACE: &str = "org.shadowblip.Input.CompositeDevice";
    const TIMEOUT: Duration = Duration::from_secs(3);
    /// How often the intercept mode is read back and put right.
    const RECONCILE: Duration = Duration::from_secs(5);

    static SURFACE: AtomicBool = AtomicBool::new(false);
    /// The pad goes to a game's touch controls (an Android game with touch spots is in front).
    static DIVERT: AtomicBool = AtomicBool::new(false);
    type Sink = Box<dyn Fn(Pad, bool) -> bool + Send>;
    static SINK: std::sync::Mutex<Option<Sink>> = std::sync::Mutex::new(None);

    /// What takes the pad's buttons while diverted (`divert`): (button, down) -> taken. Buttons
    /// it doesn't take do nothing; Rare and Quick always reach the launcher.
    pub fn set_divert_sink(f: impl Fn(Pad, bool) -> bool + Send + 'static) {
        *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
    }

    /// Diverts the pad to the sink (true) or gives it back to the game (false). InputPlumber
    /// intercepts while diverted, so the game gets no pad input of its own.
    pub fn divert(on: bool) {
        if DIVERT.swap(on, Ordering::Relaxed) == on {
            return;
        }
        crate::log(&format!("[ip] divert {on}"));
        if !LIVE.load(Ordering::Relaxed) {
            return;
        }
        std::thread::spawn(|| {
            let want = wanted_mode(SURFACE.load(Ordering::Relaxed) || DIVERT.load(Ordering::Relaxed));
            if let Err(e) = set_mode(want) {
                crate::log(&format!("[ip] {e}"));
            }
        });
    }

    /// An output the sink takes while diverted (no Rare surface up). Repeats are dropped.
    fn diverted(o: &Out) -> bool {
        if !DIVERT.load(Ordering::Relaxed) || SURFACE.load(Ordering::Relaxed) {
            return false;
        }
        let (p, down, repeat) = match *o {
            Out::Press(p, r) => (p, true, r),
            Out::Release(p) => (p, false, false),
            _ => return false,
        };
        if matches!(p, Pad::Guide | Pad::GuideHold | Pad::Qam) {
            return false;
        }
        if !repeat {
            if let Some(f) = SINK.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                f(p, down);
            }
        }
        true
    }
    /// The listener runs (the SuiPlay profile is in place); without it nothing is intercepted.
    static LIVE: AtomicBool = AtomicBool::new(false);

    /// The SuiPlay composite device (named "SuiPlay ..."), looked up on every call: the index
    /// changes when InputPlumber re-creates it. Other devices are never touched.
    fn suiplay_path(conn: &Connection) -> Result<String, String> {
        let mgr = conn.with_proxy(DEST, "/org/shadowblip/InputPlumber/Manager", TIMEOUT);
        let mut paths: Vec<String> = mgr.get("org.shadowblip.InputManager", "GamepadOrder").unwrap_or_default();
        if paths.is_empty() {
            paths = (0..4).map(|i| format!("/org/shadowblip/InputPlumber/CompositeDevice{i}")).collect();
        }
        for p in paths {
            let name: Result<String, _> = conn.with_proxy(DEST, p.as_str(), TIMEOUT).get(DEVICE_IFACE, "Name");
            if name.is_ok_and(|n| n.starts_with("SuiPlay")) {
                return Ok(p);
            }
        }
        Err("no SuiPlay composite device".into())
    }

    /// The intercept mode last read or set (u32::MAX: not known yet).
    static MODE: AtomicU32 = AtomicU32::new(u32::MAX);
    /// One mode change at a time (surface, divert and the reconcile tick run on their own threads).
    static SETTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn set_mode(mode: u32) -> Result<(), String> {
        let _one = SETTING.lock().unwrap_or_else(|e| e.into_inner());
        let conn = Connection::new_system().map_err(|e| format!("system bus: {e}"))?;
        let path = suiplay_path(&conn)?;
        let cur: u32 = conn.with_proxy(DEST, path.as_str(), TIMEOUT).get(DEVICE_IFACE, "InterceptMode").map_err(|e| format!("InterceptMode: {e}"))?;
        // InputPlumber changed it itself (Always on a gamepad Guide, None after a restart): the
        // input changed hands as well.
        let seen = MODE.swap(cur, Ordering::SeqCst);
        if seen != u32::MAX && seen != cur {
            crate::log(&format!("[ip] intercept mode was {seen}, now {cur}"));
            crate::reset::request("intercept mode changed by InputPlumber");
        }
        if cur == mode {
            return Ok(());
        }
        conn.with_proxy(DEST, path.as_str(), TIMEOUT).set(DEVICE_IFACE, "InterceptMode", mode).map_err(|e| format!("set InterceptMode {mode} on {path}: {e}"))?;
        MODE.store(mode, Ordering::SeqCst);
        crate::log(&format!("[ip] intercept mode {cur} -> {mode}"));
        crate::reset::request("intercept mode");
        Ok(())
    }

    /// A Rare surface came up over a game (true) or went away (false). Applied off the caller's
    /// thread; the latest call wins.
    pub fn surface(up: bool) {
        SURFACE.store(up, Ordering::Relaxed);
        if !LIVE.load(Ordering::Relaxed) {
            return;
        }
        std::thread::spawn(|| {
            let want = wanted_mode(SURFACE.load(Ordering::Relaxed) || DIVERT.load(Ordering::Relaxed));
            if let Err(e) = set_mode(want) {
                crate::log(&format!("[ip] {e}"));
            }
        });
    }

    /// Listens to InputPlumber's D-Bus target on its own thread. `apply` gets every output (a
    /// press, a release, LC / RC, a volume key); `surface_up` says whether a Rare surface is over
    /// a game (read every RECONCILE to put the intercept mode right).
    pub fn start(apply: impl Fn(Out) + Send + 'static, surface_up: impl Fn() -> bool + Send + 'static) {
        std::thread::spawn(move || {
            let conn = match Connection::new_system() {
                Ok(c) => c,
                Err(e) => {
                    crate::log(&format!("[ip] no system bus: {e}"));
                    return;
                }
            };
            // Only on a device whose composite carries Rare's capability map (the SuiPlay).
            if suiplay_path(&conn).is_err() {
                return;
            }
            // Each action with the moment it came in, so a reset applies between the ones from
            // before it and the ones after.
            let (tx, rx) = channel::<(String, f64, Instant)>();
            let rule = MatchRule::new_signal("org.shadowblip.Input.DBusDevice", "InputEvent").with_sender(DEST);
            let r = conn.add_match(rule, move |(event, value): (String, f64), _, _| {
                let _ = tx.send((event, value, Instant::now()));
                true
            });
            if let Err(e) = r {
                crate::log(&format!("[ip] cannot listen to InputPlumber: {e}"));
                return;
            }
            LIVE.store(true, Ordering::Relaxed);
            if let Err(e) = set_mode(wanted_mode(SURFACE.load(Ordering::Relaxed) || DIVERT.load(Ordering::Relaxed))) {
                crate::log(&format!("[ip] {e}"));
            }
            let mut keys = Keys::default();
            let mut checked = Instant::now();
            let mut epoch = crate::reset::last().0;
            loop {
                let wait = if keys.busy() { Duration::from_millis(16) } else { Duration::from_secs(1) };
                if let Err(e) = conn.process(wait) {
                    crate::log(&format!("[ip] D-Bus: {e}"));
                    std::thread::sleep(Duration::from_secs(1));
                }
                let now = Instant::now();
                let handle = |keys: &mut Keys, event: &str, value: f64| {
                    let down = value > 0.5;
                    if crate::debug_input() {
                        crate::log(&format!("[ip] {event} {value}"));
                    }
                    if event == "ui_guide" && down {
                        chords::RARE_CHORD_USED.store(false, Ordering::Relaxed);
                    }
                    let used = chords::RARE_CHORD_USED.load(Ordering::Relaxed);
                    for o in keys.on(event, down, now, used) {
                        if !diverted(&o) {
                            apply(o);
                        }
                    }
                    chords::RARE_HELD.store(keys.rare_held(), Ordering::Relaxed);
                };
                let actions: Vec<(String, f64, Instant)> = rx.try_iter().collect();
                let (e, reset_at) = crate::reset::last();
                let mut pending_reset = e != epoch;
                epoch = e;
                for (event, value, at) in actions {
                    // Input changed hands: what the D-Bus path held before the change is let go.
                    if pending_reset && at >= reset_at {
                        pending_reset = false;
                        for o in keys.reset() {
                            if !diverted(&o) {
                                apply(o);
                            }
                        }
                    }
                    handle(&mut keys, &event, value);
                }
                if pending_reset {
                    for o in keys.reset() {
                        if !diverted(&o) {
                            apply(o);
                        }
                    }
                }
                for o in keys.tick(now) {
                    if !diverted(&o) {
                        apply(o);
                    }
                }
                // InputPlumber restarted, or switched itself to Always on a gamepad Guide.
                if now.saturating_duration_since(checked) >= RECONCILE {
                    checked = now;
                    let up = surface_up();
                    SURFACE.store(up, Ordering::Relaxed);
                    if let Err(e) = set_mode(wanted_mode(up || DIVERT.load(Ordering::Relaxed))) {
                        crate::log(&format!("[ip] {e}"));
                    }
                }
            }
        });
    }
}

/// Whether a Settings > Controls value takes effect, and why not when it doesn't. Read by the
/// Controls page (`input_controls`), so it can say so instead of showing a dead slider.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlSupport {
    /// The config key (camelCase, as in launcher.json).
    pub key: &'static str,
    pub applies: bool,
    pub why: &'static str,
}

/// What InputPlumber 0.83 can do with each Controls setting. Checked against its source
/// (v0.83.1): device profiles (LoadProfileFromYaml) carry `invert` and `quadratic_scaling` for
/// stick to stick, but `invert` flips both axes together and `deadzone` is only read when a
/// stick becomes a button (event/value.rs); the IMU's gyro scale is device config
/// (`imu.gyro_correction`), read once when the source starts, and profiles have no gyro to gyro
/// translation. LC and RC are Rare's own (this module), so those apply.
pub fn controls_support() -> Vec<ControlSupport> {
    vec![
        ControlSupport { key: "lcAction", applies: true, why: "" },
        ControlSupport { key: "rcAction", applies: true, why: "" },
        ControlSupport { key: "deadzoneLeft", applies: false, why: "InputPlumber 0.83 has no stick dead zone for games yet" },
        ControlSupport { key: "deadzoneRight", applies: false, why: "InputPlumber 0.83 has no stick dead zone for games yet" },
        ControlSupport { key: "invertLeftY", applies: false, why: "InputPlumber 0.83 can only invert both axes of a stick" },
        ControlSupport { key: "invertRightY", applies: false, why: "InputPlumber 0.83 can only invert both axes of a stick" },
        ControlSupport { key: "gyroSensitivity", applies: false, why: "Motion only reaches Steam games; set it in the game's Steam controller settings" },
    ]
}

#[cfg(target_os = "linux")]
pub use imp::{divert, set_divert_sink, start, surface};

#[cfg(not(target_os = "linux"))]
pub fn divert(_on: bool) {}

#[cfg(not(target_os = "linux"))]
pub fn set_divert_sink(_f: impl Fn(Pad, bool) -> bool + Send + 'static) {}

#[cfg(not(target_os = "linux"))]
pub fn start(_apply: impl Fn(Out) + Send + 'static, _surface_up: impl Fn() -> bool + Send + 'static) {}

#[cfg(not(target_os = "linux"))]
pub fn surface(_up: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::side::Key as Side;

    #[test]
    fn rare_press_and_hold() {
        let t = Instant::now();
        let mut k = Keys::default();
        assert!(k.on("ui_guide", true, t, false).is_empty());
        assert!(k.rare_held());
        assert!(k.tick(t + Duration::from_millis(100)).is_empty());
        assert_eq!(k.on("ui_guide", false, t + Duration::from_millis(150), false), vec![Out::Press(Pad::Guide, false), Out::Release(Pad::Guide)]);
        // Held: the hold fires once, the release sends nothing.
        k.on("ui_guide", true, t + Duration::from_secs(1), false);
        assert_eq!(k.tick(t + Duration::from_secs(1) + RARE_HOLD), vec![Out::Press(Pad::GuideHold, false)]);
        assert!(k.tick(t + Duration::from_secs(3)).is_empty());
        assert!(k.on("ui_guide", false, t + Duration::from_secs(3), false).is_empty());
        // A chord (Rare + Y) used the press: no Rare press on release.
        k.on("ui_guide", true, t + Duration::from_secs(5), false);
        assert!(k.on("ui_guide", false, t + Duration::from_secs(5) + Duration::from_millis(200), true).is_empty());
        assert!(!k.rare_held());
    }

    #[test]
    fn quick_and_lc_and_rc() {
        let t = Instant::now();
        let mut k = Keys::default();
        assert_eq!(k.on("ui_quick", true, t, false), vec![Out::Press(Pad::Qam, false)]);
        assert_eq!(k.on("ui_quick", false, t, false), vec![Out::Release(Pad::Qam)]);
        // LC and RC go to side.rs as they are (it times presses and holds).
        assert_eq!(k.on("ui_screenshot", true, t, false), vec![Out::Side(Side::Lc, true)]);
        assert_eq!(k.on("ui_screenshot", false, t, false), vec![Out::Side(Side::Lc, false)]);
        assert_eq!(k.on("ui_osk", true, t, false), vec![Out::Side(Side::Rc, true)]);
        assert_eq!(k.on("ui_osk", false, t, false), vec![Out::Side(Side::Rc, false)]);
        assert!(k.tick(t + Duration::from_secs(3)).is_empty());
    }

    #[test]
    fn whole_pad_while_intercepting() {
        let t = Instant::now();
        let mut k = Keys::default();
        assert_eq!(k.on("ui_accept", true, t, false), vec![Out::Press(Pad::A, false)]);
        assert_eq!(k.on("ui_accept", false, t, false), vec![Out::Release(Pad::A)]);
        assert_eq!(k.on("ui_context", true, t, false), vec![Out::Press(Pad::X, false)]);
        assert_eq!(k.on("ui_act_on", true, t, false), vec![Out::Press(Pad::Y, false)]);
        assert_eq!(k.on("ui_volume_up", true, t, false), vec![Out::Volume(crate::volume::Key::Up)]);
        assert!(k.on("ui_volume_up", false, t, false).is_empty());
        assert!(k.on("ui_unknown", true, t, false).is_empty());
        // A held direction repeats after the delay, then steadily.
        k.on("ui_down", true, t, false);
        assert!(k.tick(t + Duration::from_millis(100)).is_empty());
        assert_eq!(k.tick(t + REPEAT_DELAY), vec![Out::Press(Pad::Down, true)]);
        assert_eq!(k.tick(t + REPEAT_DELAY + REPEAT_EVERY), vec![Out::Press(Pad::Down, true)]);
        assert_eq!(k.on("ui_down", false, t + Duration::from_secs(1), false), vec![Out::Release(Pad::Down)]);
        assert!(k.tick(t + Duration::from_secs(2)).is_empty());
        k.on("ui_context", false, t, false);
        k.on("ui_act_on", false, t, false);
        assert!(!k.busy());
    }

    #[test]
    fn a_switch_back_to_pass_mid_hold_releases_what_dbus_held() {
        let t = Instant::now();
        let mut k = Keys::default();
        k.on("ui_left", true, t, false);
        k.on("ui_accept", true, t, false);
        k.on("ui_guide", true, t, false);
        assert_eq!(k.tick(t + REPEAT_DELAY), vec![Out::Press(Pad::Left, true)]);
        // Mode 2 -> 1 while held. InputPlumber 0.83.0 sends no ui_left release after this, so
        // without the reset Left repeated every 90 ms for good.
        assert_eq!(k.reset(), vec![Out::Release(Pad::Left), Out::Release(Pad::A)]);
        for n in 0..40 {
            let out = k.tick(t + REPEAT_DELAY + REPEAT_EVERY * n);
            assert!(!out.iter().any(|o| matches!(o, Out::Press(Pad::Left, _))), "Left repeated after the switch");
        }
        // The Rare button is its own action and still works.
        assert!(k.rare_held());
        // A release that does come later is harmless.
        assert_eq!(k.on("ui_left", false, t + Duration::from_secs(5), false), vec![Out::Release(Pad::Left)]);
        assert!(k.reset().is_empty());
    }

    #[test]
    fn controls_support_lists_every_setting() {
        let keys: Vec<_> = controls_support().iter().map(|c| c.key).collect();
        for k in ["lcAction", "rcAction", "deadzoneLeft", "deadzoneRight", "invertLeftY", "invertRightY", "gyroSensitivity"] {
            assert!(keys.contains(&k), "{k}");
        }
        assert!(controls_support().iter().all(|c| c.applies || !c.why.is_empty()));
    }

    #[test]
    fn modes() {
        assert_eq!(wanted_mode(true), MODE_ALWAYS);
        assert_eq!(wanted_mode(false), MODE_PASS);
    }
}
