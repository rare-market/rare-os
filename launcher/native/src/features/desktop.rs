//! Desktop mode (KDE Plasma): the controller as a mouse and keyboard, the way a Steam Deck's is
//! there, and LC and RC as in Rare. `rare-launcher-native --desktop-controls`, started by Plasma's
//! autostart (/etc/xdg/autostart/rare-desktop-controls.desktop, image/system_files).
//!
//! Before this nothing read the controller in Desktop mode: InputPlumber's Xbox pad is a gamepad,
//! which Plasma never treats as a pointer, and Rare, Quick, LC and RC are D-Bus actions only Rare's
//! session listened to. An intercept mode Rare's session left at Always also kept the pad silent.
//!
//! Two modes, switched on the fly:
//! - **Desktop controls** (the default): every pad is grabbed (EVIOCGRAB), so no app sees it as a
//!   gamepad, and it drives a virtual mouse and keyboard of Rare's own (/dev/uinput):
//!   right stick the pointer, left stick scrolls, A left click, B right click, X Space, Y the
//!   on-screen keyboard, d-pad arrows, LB / RB Shift+Tab / Tab, LT right click, RT left click,
//!   Menu Enter, View Esc, R3 middle click. Buttons are held as long as they are (drag with A).
//! - **Gamepad**: the grab is let go and games and apps get a normal controller. On whenever the
//!   window in front is full screen or a Steam game (`steam_app_*`), which a KWin script reports
//!   over D-Bus (`market.rare.Desktop.Window`). Quick switches by hand; that holds until another
//!   window comes to the front.
//!
//! Rare opens the app menu, Quick switches the mode (with an on-screen note), LC and RC do what
//! Settings > Controls picked (the LC / RC rows with desktop controls, the "in games" rows in
//! gamepad mode): Keyboard toggles Plasma's on-screen keyboard, Capture takes a screenshot with
//! Spectacle, Key shortcut holds its keys down while the button is held (push-to-talk dictation).
//! Return to Rare stays the desktop's Return to Rare.
//!
//! In Rare's Game Mode session the same controls drive a game the player set to the mouse
//! (`start_in_game`).

use crate::screens::settings::config;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, MessageType};
use rare_input::side::{Action, Key};
use rare_input::Pad;
use rare_launch::apps::pointer::{self, code::*, uinput, Carry};
use std::collections::{HashMap, HashSet};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const BUS: &str = "market.rare.Desktop";
const SCRIPT: &str = "rare-desktop-controls";
const TIMEOUT: Duration = Duration::from_secs(2);
/// _IOW('E', 0x90, int)
const EVIOCGRAB: libc::c_ulong = 0x4004_4590;

const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_NORTH: u16 = 0x133;
const BTN_WEST: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_THUMBR: u16 = 0x13e;
const BTN_DPAD_UP: u16 = 0x220;
const BTN_DPAD_DOWN: u16 = 0x221;
const BTN_DPAD_LEFT: u16 = 0x222;
const BTN_DPAD_RIGHT: u16 = 0x223;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

fn log(s: &str) {
    eprintln!("[{}] [desktop] {s}", clock());
}

fn clock() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{:02}:{:02}:{:02}", (t / 3600) % 24, (t / 60) % 60, t % 60)
}

/// What a pad button does with desktop controls on. `xy_swapped`: the pad reports X on BTN_NORTH
///, else X is BTN_WEST.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Does {
    Keys(Vec<u16>),
    Keyboard,
    Nothing,
}

pub fn button(code: u16, xy_swapped: bool) -> Does {
    let (x, y) = if xy_swapped { (BTN_NORTH, BTN_WEST) } else { (BTN_WEST, BTN_NORTH) };
    let k = |c: &[u16]| Does::Keys(c.to_vec());
    match code {
        BTN_SOUTH => k(&[BTN_LEFT]),
        BTN_EAST => k(&[BTN_RIGHT]),
        c if c == x => k(&[KEY_SPACE]),
        c if c == y => Does::Keyboard,
        BTN_TL => k(&[KEY_LEFTSHIFT, KEY_TAB]),
        BTN_TR => k(&[KEY_TAB]),
        BTN_START => k(&[KEY_ENTER]),
        BTN_SELECT => k(&[KEY_ESC]),
        BTN_THUMBR => k(&[BTN_MIDDLE]),
        BTN_DPAD_UP => k(&[KEY_UP]),
        BTN_DPAD_DOWN => k(&[KEY_DOWN]),
        BTN_DPAD_LEFT => k(&[KEY_LEFT]),
        BTN_DPAD_RIGHT => k(&[KEY_RIGHT]),
        _ => Does::Nothing,
    }
}

/// Which mode the window in front asks for: a normal controller for a full screen window or a
/// Steam game, desktop controls for everything else.
pub fn wants_gamepad(fullscreen: bool, class: &str) -> bool {
    fullscreen || class.starts_with("steam_app_")
}

/// The mode now and why: the window's wish, unless Quick switched it for this window.
#[derive(Debug, Default)]
pub struct Modes {
    window: String,
    auto: bool,
    manual: Option<(bool, String)>,
}

impl Modes {
    /// The window in front changed (or changed full screen): a hand switch ends with its window.
    pub fn window(&mut self, fullscreen: bool, class: &str) {
        let key = format!("{}:{class}", fullscreen as u8);
        if self.manual.as_ref().is_some_and(|(_, w)| *w != key) {
            self.manual = None;
        }
        self.window = key;
        self.auto = wants_gamepad(fullscreen, class);
    }

    /// Quick: the other mode, for this window.
    pub fn toggle(&mut self) {
        let next = !self.gamepad();
        self.manual = Some((next, self.window.clone()));
    }

    pub fn gamepad(&self) -> bool {
        self.manual.as_ref().map(|(g, _)| *g).unwrap_or(self.auto)
    }
}

static MODES: Mutex<Modes> = Mutex::new(Modes { window: String::new(), auto: false, manual: None });
/// The mode applied: true while games get the controller.
static GAMEPAD: AtomicBool = AtomicBool::new(false);

struct PadFd {
    path: PathBuf,
    fd: i32,
}

/// The pads being read (their fds, for grabbing and letting go from the control thread).
static PADS: Mutex<Vec<PadFd>> = Mutex::new(Vec::new());
/// Each pad's sticks (left x, left y, right x, right y, down positive), by pad number.
static AXES: Mutex<Option<HashMap<u64, [f32; 4]>>> = Mutex::new(None);
static NEXT_PAD: AtomicU64 = AtomicU64::new(1);

struct Out {
    dev: Option<uinput::Device>,
    failed: bool,
    /// How many pad buttons hold each output key (A and RT both hold the left button).
    held: std::collections::BTreeMap<u16, u32>,
}

static OUT: Mutex<Out> = Mutex::new(Out { dev: None, failed: false, held: std::collections::BTreeMap::new() });

fn with_out<F: FnOnce(&mut Out)>(f: F) {
    let mut o = OUT.lock().unwrap_or_else(|e| e.into_inner());
    if o.dev.is_none() && !o.failed {
        match uinput::Device::open("Rare desktop controller") {
            Ok(d) => o.dev = Some(d),
            Err(e) => {
                log(&format!("no virtual mouse ({e}); is /dev/uinput writable?"));
                o.failed = true;
            }
        }
    }
    f(&mut *o);
}

/// Output keys down (`down`) or up, counted per key so two buttons on one key work together.
fn keys(codes: &[u16], down: bool) {
    with_out(|o| {
        let Out { dev, held, .. } = o;
        let Some(d) = dev.as_mut() else { return };
        let list: Vec<u16> = if down { codes.to_vec() } else { codes.iter().rev().copied().collect() };
        for c in list {
            let n = held.entry(c).or_insert(0);
            if down {
                *n += 1;
                if *n == 1 {
                    d.key(c, true);
                    d.syn();
                }
            } else if *n > 0 {
                *n -= 1;
                if *n == 0 {
                    d.key(c, false);
                    d.syn();
                }
            }
        }
    });
}

/// Lets go of every output key (the mode changed while something was held).
fn release_all() {
    with_out(|o| {
        let Out { dev, held, .. } = o;
        if let Some(d) = dev.as_mut() {
            for (c, n) in held.iter() {
                if *n > 0 {
                    d.key(*c, false);
                }
            }
            d.syn();
        }
        held.clear();
    });
}

fn grab(fd: i32, on: bool) -> bool {
    // SAFETY: EVIOCGRAB on an evdev fd we hold open; the argument is an int by value.
    unsafe { libc::ioctl(fd, EVIOCGRAB as _, on as libc::c_int) >= 0 }
}

/// Applies the mode the window and Quick ask for. `note`: show it on screen (a hand switch).
fn apply(note: bool) {
    let want = MODES.lock().unwrap_or_else(|e| e.into_inner()).gamepad();
    switch(want, note);
}

/// Gamepad (`want`) or desktop controls: the pads let go or grabbed, nothing left held.
fn switch(want: bool, note: bool) {
    if GAMEPAD.swap(want, Ordering::SeqCst) == want {
        if note {
            osd(want);
        }
        return;
    }
    release_all();
    for p in PADS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        if !grab(p.fd, !want) {
            log(&format!("{} {}: {}", if want { "release" } else { "grab" }, p.path.display(), std::io::Error::last_os_error()));
        }
    }
    log(if want { "gamepad: games get the controller" } else { "desktop controls" });
    if note && !IN_GAME.load(Ordering::Relaxed) {
        osd(want);
    }
}

fn osd(gamepad: bool) {
    let (icon, text) = if gamepad { ("input-gaming", rare_i18n::tr().features.desktop_osd_controller) } else { ("input-mouse", rare_i18n::tr().features.desktop_osd_mouse) };
    let r = Connection::new_session().map_err(|e| e.to_string()).and_then(|c| {
        c.with_proxy("org.kde.plasmashell", "/org/kde/osdService", TIMEOUT).method_call::<(), _, _, _>("org.kde.osdService", "showText", (icon, text)).map_err(|e| e.to_string())
    });
    if let Err(e) = r {
        log(&format!("osd: {e}"));
    }
}

/// Plasma's app menu (the Rare button).
fn app_menu() {
    let r = Connection::new_session().map_err(|e| e.to_string()).and_then(|c| {
        c.with_proxy("org.kde.plasmashell", "/PlasmaShell", TIMEOUT).method_call::<(), _, _, _>("org.kde.PlasmaShell", "activateLauncherMenu", ()).map_err(|e| e.to_string())
    });
    if let Err(e) = r {
        log(&format!("app menu: {e}"));
    }
}

/// The input methods KWin can show as its on-screen keyboard, in the order tried.
const IMS: [&str; 2] = ["/usr/share/applications/com.github.maliit.keyboard.desktop", "/usr/share/applications/org.kde.plasma.keyboard.desktop"];

/// KWin has no input method set (a device from before the image set one): sets the first one
/// installed for this user, as System Settings > Virtual Keyboard does, and has KWin reload.
fn setup_im(c: &Connection) -> Result<(), String> {
    let Some(im) = IMS.iter().find(|p| std::path::Path::new(p).exists()) else {
        return Err("no on-screen keyboard installed (maliit-keyboard)".into());
    };
    let cur = std::process::Command::new("kreadconfig6").args(["--file", "kwinrc", "--group", "Wayland", "--key", "InputMethod"]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if !cur.is_empty() {
        return Err(format!("KWin's input method {cur} is not available"));
    }
    let ok = std::process::Command::new("kwriteconfig6").args(["--file", "kwinrc", "--group", "Wayland", "--key", "InputMethod", im]).status().is_ok_and(|s| s.success());
    if !ok {
        return Err("kwriteconfig6 failed".into());
    }
    c.with_proxy("org.kde.KWin", "/KWin", TIMEOUT).method_call::<(), _, _, _>("org.kde.KWin", "reconfigure", ()).map_err(|e| format!("reconfigure: {e}"))?;
    log(&format!("on-screen keyboard set to {im}"));
    std::thread::sleep(Duration::from_millis(800));
    Ok(())
}

/// Plasma's on-screen keyboard (KWin's input method), up or down. Turned on first if it is off.
fn keyboard() {
    let r = Connection::new_session().map_err(|e| e.to_string()).and_then(|c| {
        let p = c.with_proxy("org.kde.KWin", "/VirtualKeyboard", TIMEOUT);
        const IF: &str = "org.kde.kwin.VirtualKeyboard";
        let mut available: bool = p.get(IF, "available").unwrap_or(false);
        if !available {
            setup_im(&c)?;
            available = p.get(IF, "available").unwrap_or(false);
        }
        if !available {
            return Err("KWin has no on-screen keyboard".into());
        }
        // Plasma up to 6.4 has an `enabled` switch (newer KWin has `mode` instead, and
        // forceActivate shows the keyboard whatever the mode).
        let enabled: Result<bool, _> = p.get(IF, "enabled");
        if enabled.is_ok_and(|e| !e) {
            if let Err(e) = p.set(IF, "enabled", true) {
                log(&format!("keyboard: enable: {e}"));
            }
        }
        let visible: bool = p.get(IF, "visible").unwrap_or(false);
        if visible {
            p.set(IF, "active", false).map_err(|e| format!("hide: {e}"))
        } else {
            // forceActivate (Plasma 6.1+) shows it even over a field that never asked for it.
            let forced: Result<(), dbus::Error> = p.method_call(IF, "forceActivate", ());
            forced.or_else(|_| p.set(IF, "active", true)).map_err(|e| format!("show: {e}"))
        }
    });
    if let Err(e) = r {
        log(&format!("keyboard: {e}"));
    }
}

fn screenshot() {
    if let Err(e) = std::process::Command::new("spectacle").args(["--background", "--fullscreen"]).spawn() {
        log(&format!("screenshot: {e}"));
    }
}

/// The keys LC and RC hold right now (released with the button, whatever the mode is by then).
static SIDE_HELD: Mutex<[Option<Vec<u16>>; 2]> = Mutex::new([None, None]);

/// The LC or RC setting for the mode: the button's row with desktop controls, its "in games" row
/// (the same default as Rare's session) while games get the controller.
fn side_action(k: Key, gamepad: bool) -> Action {
    let c = config::load();
    let a = match (k, gamepad) {
        (Key::Lc, false) => c.lc_action,
        (Key::Rc, false) => c.rc_action,
        (Key::Lc, true) => Some(c.lc_game_action.unwrap_or_else(|| crate::side::game_default(Key::Lc, c.lc_action.as_deref()).into())),
        (Key::Rc, true) => Some(c.rc_game_action.unwrap_or_else(|| crate::side::game_default(Key::Rc, c.rc_action.as_deref()).into())),
    };
    rare_input::side::action(k, a.as_deref())
}

fn side(k: Key, down: bool) {
    let i = if k == Key::Lc { 0 } else { 1 };
    if !down {
        if let Some(keys) = SIDE_HELD.lock().ok().and_then(|mut h| h[i].take()) {
            pointer::hold_keys(&keys, false);
        }
        return;
    }
    match side_action(k, GAMEPAD.load(Ordering::SeqCst)) {
        Action::Shortcut => {
            if let Some(s) = crate::side::shortcut_of(k) {
                let keys = s.keys().to_vec();
                pointer::hold_keys(&keys, true);
                if let Ok(mut h) = SIDE_HELD.lock() {
                    h[i] = Some(keys);
                }
            }
        }
        Action::Keyboard => keyboard(),
        Action::Capture => screenshot(),
        // No performance overlay in Desktop mode.
        Action::Perf | Action::Nothing => {}
    }
}

/// An action from InputPlumber's D-Bus target (Rare, Quick, LC, RC).
fn on_ip(o: rare_input::ip::Out) {
    use rare_input::ip::Out as O;
    match o {
        O::Press(Pad::Guide, false) => app_menu(),
        O::Press(Pad::Qam, false) => {
            MODES.lock().unwrap_or_else(|e| e.into_inner()).toggle();
            apply(true);
        }
        O::Side(k, down) => side(k, down),
        _ => {}
    }
}

/// A pad's events. Runs on its own thread until the pad goes away.
fn read_pad(mut dev: evdev::Device, n: u64, swapped: bool) {
    let abs = dev.get_abs_state().ok();
    let range = |code: u16| -> (i32, i32) { abs.as_ref().map(|a| (a[code as usize].minimum, a[code as usize].maximum)).filter(|(lo, hi)| hi > lo).unwrap_or((-32768, 32767)) };
    let ranges: HashMap<u16, (i32, i32)> = [ABS_X, ABS_Y, ABS_RX, ABS_RY, ABS_Z, ABS_RZ].iter().map(|c| (*c, range(*c))).collect();
    let mut trig = [false; 2];
    let mut hat = [0i32; 2];
    loop {
        let Ok(events) = dev.fetch_events() else { return };
        let desk = !GAMEPAD.load(Ordering::SeqCst);
        for e in events {
            let (code, v) = (e.code(), e.value());
            match e.event_type() {
                evdev::EventType::KEY if desk && v != 2 => match button(code, swapped) {
                    Does::Keys(k) => keys(&k, v == 1),
                    Does::Keyboard if v == 1 => {
                        if IN_GAME.load(Ordering::Relaxed) {
                            crate::side::app_keyboard(false);
                        } else {
                            keyboard();
                        }
                    }
                    _ => {}
                },
                evdev::EventType::ABSOLUTE => match code {
                    ABS_X | ABS_Y | ABS_RX | ABS_RY => {
                        let (lo, hi) = ranges[&code];
                        let x = rare_input::evdev::norm(v, lo, hi, false);
                        let i = match code {
                            ABS_X => 0,
                            ABS_Y => 1,
                            ABS_RX => 2,
                            _ => 3,
                        };
                        if let Ok(mut a) = AXES.lock() {
                            a.get_or_insert_with(HashMap::new).entry(n).or_insert([0.0; 4])[i] = x;
                        }
                    }
                    ABS_Z | ABS_RZ => {
                        let (lo, hi) = ranges[&code];
                        let x = rare_input::evdev::norm(v, lo, hi, true);
                        let i = (code == ABS_RZ) as usize;
                        // LT right click, RT left click, with some hysteresis.
                        let now = if trig[i] { x > 0.35 } else { x > 0.55 };
                        if now != trig[i] {
                            trig[i] = now;
                            if desk || !now {
                                keys(&[if i == 0 { BTN_RIGHT } else { BTN_LEFT }], now);
                            }
                        }
                    }
                    ABS_HAT0X | ABS_HAT0Y => {
                        let i = (code == ABS_HAT0Y) as usize;
                        let (neg, pos) = if i == 0 { (KEY_LEFT, KEY_RIGHT) } else { (KEY_UP, KEY_DOWN) };
                        let old = hat[i];
                        let new = v.signum();
                        if old != new {
                            if old != 0 {
                                keys(&[if old < 0 { neg } else { pos }], false);
                            }
                            if new != 0 && desk {
                                keys(&[if new < 0 { neg } else { pos }], true);
                            }
                            hat[i] = new;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

/// Finds pads (every 2 s) and reads each. A pad someone else holds (InputPlumber's hidden source
/// pad) can't be grabbed and is left alone.
fn scan() {
    // Nodes that are not pads, or pads something else holds: looked at once while they exist.
    let mut skipped: HashSet<PathBuf> = HashSet::new();
    loop {
        let live: HashSet<PathBuf> = PADS.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|p| p.path.clone()).collect();
        skipped.retain(|p| p.exists());
        for (path, dev) in evdev::enumerate() {
            if live.contains(&path) || skipped.contains(&path) {
                continue;
            }
            if !dev.supported_keys().is_some_and(|k| k.contains(evdev::KeyCode::BTN_SOUTH)) {
                skipped.insert(path);
                continue;
            }
            let name = dev.name().unwrap_or("?").to_string();
            let fd = dev.as_raw_fd();
            if !grab(fd, true) {
                log(&format!("{} ({name}) is held by something else, left alone", path.display()));
                skipped.insert(path);
                continue;
            }
            if GAMEPAD.load(Ordering::SeqCst) {
                grab(fd, false);
            }
            let swapped = rare_input::pad::xy_swapped(&name, Some(dev.input_id().vendor()));
            log(&format!("pad {} ({name})", path.display()));
            PADS.lock().unwrap_or_else(|e| e.into_inner()).push(PadFd { path: path.clone(), fd });
            let n = NEXT_PAD.fetch_add(1, Ordering::Relaxed);
            std::thread::Builder::new()
                .name("rare-desktop-pad".into())
                .spawn(move || {
                    read_pad(dev, n, swapped);
                    PADS.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| p.path != path);
                    if let Ok(mut a) = AXES.lock() {
                        a.get_or_insert_with(HashMap::new).remove(&n);
                    }
                    log(&format!("pad {} gone", path.display()));
                })
                .ok();
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// The pointer and the scroll wheel from the sticks, while desktop controls are on.
fn motion() {
    let mut carry = Carry::default();
    let mut last = Instant::now();
    loop {
        let axes = {
            let a = AXES.lock().unwrap_or_else(|e| e.into_inner());
            // The stick pushed furthest, per axis, of every pad.
            let mut m = [0.0f32; 4];
            for v in a.iter().flat_map(|h| h.values()) {
                for i in 0..4 {
                    if v[i].abs() > m[i].abs() {
                        m[i] = v[i];
                    }
                }
            }
            m
        };
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.05);
        last = now;
        if GAMEPAD.load(Ordering::SeqCst) {
            carry = Carry::default();
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let evs = pointer::motion(&axes, dt, &mut carry);
        if !evs.is_empty() {
            with_out(|o| {
                if let Some(d) = o.dev.as_mut() {
                    for (c, v) in &evs {
                        d.rel(*c, *v);
                    }
                    d.syn();
                }
            });
        }
        let moving = axes.iter().any(|v| v.abs() > 0.1);
        std::thread::sleep(Duration::from_millis(if moving { 8 } else { 30 }));
    }
}

/// The KWin script that reports the window in front (KWin 6 scripting API).
const KWIN_JS: &str = r#"// Rare OS: tells Rare's desktop controller which window is in front, so a game in full
// screen gets a normal controller (launcher/native/src/features/desktop.rs).
function report(w) {
    var full = !!(w && w.fullScreen);
    var cls = w ? String(w.resourceClass || "") : "";
    callDBus("market.rare.Desktop", "/market/rare/Desktop", "market.rare.Desktop", "Window", full, cls);
}
var watched = [];
function watch(w) {
    if (!w || watched.indexOf(w) >= 0) return;
    watched.push(w);
    w.fullScreenChanged.connect(function () {
        if (workspace.activeWindow === w) report(w);
    });
}
workspace.windowActivated.connect(function (w) { watch(w); report(w); });
workspace.windowRemoved.connect(function (w) {
    var i = watched.indexOf(w);
    if (i >= 0) watched.splice(i, 1);
});
watch(workspace.activeWindow);
report(workspace.activeWindow);
"#;

fn load_kwin_script(conn: &Connection) -> Result<(), String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("rare");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("desktop-controls.js");
    std::fs::write(&path, KWIN_JS).map_err(|e| e.to_string())?;
    let p = conn.with_proxy("org.kde.KWin", "/Scripting", TIMEOUT);
    const IF: &str = "org.kde.kwin.Scripting";
    let _: Result<(bool,), _> = p.method_call(IF, "unloadScript", (SCRIPT,));
    let (id,): (i32,) = p.method_call(IF, "loadScript", (path.to_string_lossy().to_string(), SCRIPT)).map_err(|e| format!("loadScript: {e}"))?;
    p.method_call::<(), _, _, _>(IF, "start", ()).map_err(|e| format!("start: {e}"))?;
    log(&format!("window script loaded ({id})"));
    Ok(())
}

/// InputPlumber as Desktop mode needs it: the Xbox pad (a Deck target left from Rare's session has
/// no evdev pad) and intercept mode Pass. Retried while InputPlumber starts.
fn inputplumber() {
    for attempt in 0..20 {
        let targets = rare_input::gyro::targets_now();
        let ok_targets = match &targets {
            Ok(t) if t.iter().any(|x| x == "xbox-elite") => true,
            Ok(t) => {
                log(&format!("pad targets {t:?}: back to the Xbox pad"));
                rare_input::gyro::set_target_now(rare_input::gyro::Target::Xbox).map_err(|e| log(&e)).is_ok()
            }
            Err(_) => false,
        };
        let ok_mode = rare_input::ip::pass_now().map_err(|e| if attempt == 0 { log(&e) }).is_ok();
        if ok_targets && ok_mode {
            return;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

static STARTED: OnceLock<()> = OnceLock::new();
/// Running inside Rare's Game Mode session (`start_in_game`), not Plasma's.
static IN_GAME: AtomicBool = AtomicBool::new(false);

/// Game Mode: the same controller-as-mouse for a game the player set to the mouse (the Controls
/// row on its page; `mouse` says when one has the screen and no Rare surface is over it).
///
/// The pads are found as in Desktop mode, InputPlumber's virtual pad among them (its source pads
/// are held by InputPlumber and left alone), but left to the games until then. While such a game
/// has the screen they are grabbed, so the game sees only Rare's mouse and keyboard; Rare and
/// Quick are InputPlumber D-Bus actions, not pad buttons, so they still bring the launcher back,
/// and a Rare surface over the game gets the pad as always (InputPlumber intercepts).
pub fn start_in_game(mouse: fn() -> bool) {
    if STARTED.set(()).is_err() {
        return;
    }
    IN_GAME.store(true, Ordering::Relaxed);
    GAMEPAD.store(true, Ordering::SeqCst);
    std::thread::Builder::new().name("rare-game-mouse-scan".into()).spawn(scan).ok();
    std::thread::Builder::new().name("rare-game-mouse-motion".into()).spawn(motion).ok();
    std::thread::Builder::new()
        .name("rare-game-mouse".into())
        .spawn(move || loop {
            let on = mouse();
            if on {
                // The virtual mouse exists before the first move.
                with_out(|_| {});
            }
            switch(!on, false);
            std::thread::sleep(Duration::from_millis(150));
        })
        .ok();
}

/// `--desktop-controls`: runs until the desktop session ends. Exit code for the wrapper.
pub fn run() -> i32 {
    if STARTED.set(()).is_err() {
        return 0;
    }
    let conn = match Connection::new_session() {
        Ok(c) => c,
        Err(e) => {
            log(&format!("no session bus: {e}"));
            return 1;
        }
    };
    match conn.request_name(BUS, false, false, true) {
        Ok(dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply::PrimaryOwner) => {}
        Ok(_) => {
            log("already running");
            return 0;
        }
        Err(e) => {
            log(&format!("bus name: {e}"));
            return 1;
        }
    }
    log("start");
    conn.start_receive(
        MatchRule::new_method_call(),
        Box::new(|msg, c| {
            if msg.msg_type() == MessageType::MethodCall {
                if msg.member().as_deref() == Some("Window") {
                    if let Ok((full, class)) = msg.read2::<bool, String>() {
                        MODES.lock().unwrap_or_else(|e| e.into_inner()).window(full, &class);
                        apply(false);
                    }
                }
                let _ = c.send(msg.method_return());
            }
            true
        }),
    );
    std::thread::spawn(inputplumber);
    // The virtual devices exist before the first press, so Plasma has picked them up by then.
    with_out(|_| {});
    if crate::side::uses_shortcut() {
        pointer::prepare_keys();
    }
    std::thread::Builder::new().name("rare-desktop-scan".into()).spawn(scan).ok();
    std::thread::Builder::new().name("rare-desktop-motion".into()).spawn(motion).ok();
    rare_input::ip::start(on_ip, || false);
    if let Err(e) = load_kwin_script(&conn) {
        log(&format!("window script: {e} (games get desktop controls until Quick switches)"));
    }
    // Until the desktop session ends: KWin gone for 10 s ends it.
    let mut gone = 0;
    let mut checked = Instant::now();
    loop {
        if let Err(e) = conn.process(Duration::from_millis(500)) {
            log(&format!("D-Bus: {e}"));
            return 1;
        }
        if checked.elapsed() >= Duration::from_secs(5) {
            checked = Instant::now();
            let has: Result<(bool,), _> = conn.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", TIMEOUT).method_call("org.freedesktop.DBus", "NameHasOwner", ("org.kde.KWin",));
            gone = if has.is_ok_and(|(h,)| h) { 0 } else { gone + 1 };
            if gone >= 2 {
                log("KWin is gone: the desktop session ended");
                release_all();
                return 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_layout() {
        // An Xbox-named pad (InputPlumber's): X on BTN_NORTH, Y on BTN_WEST.
        assert_eq!(button(BTN_SOUTH, true), Does::Keys(vec![BTN_LEFT]));
        assert_eq!(button(BTN_EAST, true), Does::Keys(vec![BTN_RIGHT]));
        assert_eq!(button(BTN_NORTH, true), Does::Keys(vec![KEY_SPACE]));
        assert_eq!(button(BTN_WEST, true), Does::Keyboard);
        // A pad with positional codes: X is West, Y is North.
        assert_eq!(button(BTN_WEST, false), Does::Keys(vec![KEY_SPACE]));
        assert_eq!(button(BTN_NORTH, false), Does::Keyboard);
        assert_eq!(button(BTN_DPAD_LEFT, true), Does::Keys(vec![KEY_LEFT]));
        assert_eq!(button(BTN_TL, true), Does::Keys(vec![KEY_LEFTSHIFT, KEY_TAB]));
        assert_eq!(button(BTN_START, true), Does::Keys(vec![KEY_ENTER]));
        assert_eq!(button(BTN_SELECT, true), Does::Keys(vec![KEY_ESC]));
        // Guide is not a desktop button (Rare is InputPlumber's D-Bus action).
        assert_eq!(button(0x13c, true), Does::Nothing);
        // Every key it sends is on the virtual device.
        for c in [BTN_SOUTH, BTN_EAST, BTN_NORTH, BTN_TL, BTN_TR, BTN_START, BTN_SELECT, BTN_THUMBR, BTN_DPAD_UP, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT] {
            if let Does::Keys(ks) = button(c, true) {
                assert!(ks.iter().all(|k| KEYS.contains(k)), "{c:#x}");
            }
        }
    }

    #[test]
    fn games_get_the_controller() {
        assert!(wants_gamepad(true, "org.kde.dolphin"));
        assert!(wants_gamepad(false, "steam_app_1091500"));
        assert!(!wants_gamepad(false, "steam"));
        assert!(!wants_gamepad(false, ""));
    }

    #[test]
    fn quick_switches_until_another_window_comes_up() {
        let mut m = Modes::default();
        m.window(false, "org.kde.dolphin");
        assert!(!m.gamepad());
        m.toggle();
        assert!(m.gamepad());
        // The same window reported again keeps the hand switch.
        m.window(false, "org.kde.dolphin");
        assert!(m.gamepad());
        // Another window: its own mode.
        m.window(true, "retroarch");
        assert!(m.gamepad());
        m.toggle();
        assert!(!m.gamepad());
        m.window(false, "firefox");
        assert!(!m.gamepad());
        m.window(true, "firefox");
        assert!(m.gamepad());
    }
}
