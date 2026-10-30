//! gamescope's root-window controls, as Steam itself uses them. All through `xprop`, like the
//! FPS limit in system.rs. Checked against gamescope's steamcompmgr.cpp:
//!
//! - gamescope runs one or more Xwayland servers. Server 0 (gamescope's own `DISPLAY`, where Steam
//!   runs) holds the controls; Steam starts games on `STEAM_GAME_DISPLAY_0`, which is server 1
//!   when there are two. Rare, started by Steam as a shortcut, usually sits on that game server.
//!   Each server's root carries `GAMESCOPE_XWAYLAND_SERVER_ID`, so server 0 is found by asking.
//! - Global focus is picked across every server. `GAMESCOPECTRL_BASELAYER_WINDOW` on server 0's
//!   root, when it names a mapped window, wins over everything else; without it gamescope takes
//!   the first app in `GAMESCOPECTRL_BASELAYER_APPID` (Steam's list) and, inside one app, the most
//!   recently mapped window. `GAMESCOPE_FOCUSED_WINDOW` and `GAMESCOPE_FOCUSABLE_WINDOWS` are
//!   gamescope's outputs (what it chose and what it could choose); writing them does nothing.
//! - `GAMESCOPECTRL_REQUEST_SCREENSHOT` = 1 asks for the base plane only (the game, no overlays)
//!   in `/tmp/gamescope.png`; gamescope deletes the property on server 0 once the file is written.
//! - With `--steam`, a window can only get focus when it carries an app id (`STEAM_GAME`, or one
//!   gamescope derives from Steam's reaper), and with any control set and nothing matching it,
//!   gamescope shows nothing at all. Steam tags the windows it starts; nothing tags Rare's when Rare
//!   is the session's own client (Rare OS game mode). See `session`.
//!
//! Rare OS game mode (`session_start`): Rare's windows carry `STEAM_GAME` = `RARE_APPID`, and Rare
//! keeps its id in `GAMESCOPECTRL_BASELAYER_APPID`. Steam runs under Rare (started quietly ahead
//! of Play, or for a game) and rewrites that list with its own choices (the game, its UI as 769)
//! on its own focus changes; gamescope takes the first id that has a window. So Rare puts its id:
//! - first while the launcher has the screen (`apps::front`), Steam running quietly or not: a
//!   Steam window never takes the screen from the launcher;
//! - last once Steam was asked for the screen (Play on a Steam game, Big Picture, a store page):
//!   Steam's choices win while Steam has windows (its login or update dialogs too), and Rare
//!   shows the moment Steam has none (still starting, or quit), never a black screen.
//! A keeper thread watches the list and puts Rare's id back where it belongs after every write
//! by Steam, and when the front changes. The overlay's `raise` (the window control) still wins
//! over both.
//!
//! Programs Rare starts itself (emulators, apps, web games in Chrome, Moonlight) carry ids of
//! their own (overlay/appid.rs): while one has the screen its id goes first, Rare's second (shown
//! until the program maps a window, and the moment it ends), and it leaves the list when the
//! launcher takes the screen back or the program ends.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// The app id of Rare's own windows in Rare OS game mode. Not 769: that is Steam's own UI, which
/// gamescope treats as Steam (window_is_steam: forced fullscreen, frame limiter exempt, Steam's
/// upscaling and cursor rules), and Steam lists 769 in the base layer itself, so with that id Rare
/// and Steam would trade the screen by whichever mapped a window last, and Steam's list would pull
/// Rare over Steam's own UI. "RARE" in ASCII: far above every Steam app id, below the range of
/// non-Steam shortcut ids (top bit set), and positive as an int32.
pub const RARE_APPID: u32 = 0x5241_5245;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const CTRL_APPID: &str = "GAMESCOPECTRL_BASELAYER_APPID";
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const GAME_PROP: &str = "STEAM_GAME";
pub const CTRL_WINDOW: &str = "GAMESCOPECTRL_BASELAYER_WINDOW";
pub const FOCUSED_WINDOW: &str = "GAMESCOPE_FOCUSED_WINDOW";
pub const FOCUSED_APP: &str = "GAMESCOPE_FOCUSED_APP";
/// gamescope's output: the app ids that have a window it could show (a game's first window).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const FOCUSABLE_APPS: &str = "GAMESCOPE_FOCUSABLE_APPS";
pub const SCREENSHOT: &str = "GAMESCOPECTRL_REQUEST_SCREENSHOT";
pub const SERVER_ID: &str = "GAMESCOPE_XWAYLAND_SERVER_ID";
pub const SCREENSHOT_FILE: &str = "/tmp/gamescope.png";
/// gamescope_control's `base_plane_only`: the game, without Steam's overlays or ours.
const BASE_PLANE_ONLY: u32 = 1;

/// True inside a gamescope session (Game Mode).
pub fn active() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some()
}

/// True when Rare is the gamescope session's client (Rare OS game mode, sessions.d/rare sets
/// RARE_SESSION_CLIENT=1 on its command), not a shortcut Steam started and tags itself.
pub fn session_client() -> bool {
    active() && std::env::var("RARE_SESSION_CLIENT").as_deref() == Ok("1")
}

/// The base-layer app list with Rare's id first (`first`) or last, Steam's ids in their order;
/// None when it is that already. The keeper uses `appid::place`, which adds Rare's apps.
#[cfg(test)]
pub fn place_rare(list: &[u64], first: bool) -> Option<Vec<u64>> {
    let rare = RARE_APPID as u64;
    let others: Vec<u64> = list.iter().copied().filter(|v| *v != rare).collect();
    let out = if first { std::iter::once(rare).chain(others).collect::<Vec<_>>() } else { others.into_iter().chain(std::iter::once(rare)).collect() };
    (out != list).then_some(out)
}

/// Set while Rare, as a Wayland client (`RARE_WEBKIT=wayland`), holds the screen for the overlay or
/// a sheet. gamescope's control window only matches X11 windows, so Rare holds the screen by
/// keeping its id first in the base-layer list instead (and no app of Rare's ahead of it).
static HOLD_FRONT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What gamescope shows (GAMESCOPE_FOCUSED_APP), as the watcher last read it every 100 ms; 0 when
/// unknown (no gamescope, the watcher not running).
static SHOWN_APP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// True when gamescope is known to show something other than Rare (a game or app has the screen).
/// Cheap (no X round trip): the pad thread asks on every press.
pub fn other_app_shown() -> bool {
    let a = SHOWN_APP.load(std::sync::atomic::Ordering::Relaxed);
    a != 0 && a != RARE_APPID as u64
}

/// Labels of Rare's own windows (a web game, the in-app browser) that `hold_front` hid, shown
/// again by `release`.
static HIDDEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Set by `session_start`: this process is the session's client, so `hold_front` and `release`
/// have windows to manage.
static STARTED: OnceLock<()> = OnceLock::new();

/// Where Rare's id goes now: first unless Steam was asked for the screen (or Rare holds it).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn rare_first() -> bool {
    HOLD_FRONT.load(std::sync::atomic::Ordering::Relaxed) || !crate::host::host().steam_front()
}

/// The app id gamescope derives from a /proc/<pid>/cgroup text (gamescope's GetAppIdFromCgroup:
/// the last path element is `app-steam-app<appid>-<pid>.scope`).
pub fn appid_from_cgroup(cgroup: &str) -> Option<u32> {
    cgroup.lines().find_map(|line| {
        let path = line.splitn(3, ':').nth(2)?;
        let scope = path.rsplit('/').next()?;
        let rest = scope.strip_prefix("app-steam-app")?.strip_suffix(".scope")?;
        let (id, pid) = rest.split_once('-')?;
        pid.parse::<i32>().ok()?;
        id.parse::<u32>().ok().filter(|v| *v != 0)
    })
}

/// Rare OS game mode as a Wayland client: the screen is held by app id (see `HOLD_FRONT`). Returns
/// false when not a Wayland session client (use `raise` with the X11 id then).
///
/// gamescope 3.17 has no defined order between two xdg windows of one app: they tie in
/// `is_focus_priority_greater` (it returns true for any xdg window once the earlier checks tie),
/// which with its stable sort leaves the newest xdg surface first. So Rare's other windows (a web
/// game, the in-app browser) are hidden while the main window holds the screen: GTK destroys a
/// hidden window's xdg surface, the main window is then Rare's only one, and showing them again
/// (`release`, or the overlay's resume) makes them the newest, in front, as on X11.
pub fn hold_front() -> bool {
    if !(session_client() && crate::host::host().wayland_client()) {
        return false;
    }
    HOLD_FRONT.store(true, std::sync::atomic::Ordering::Relaxed);
    #[cfg(target_os = "linux")]
    if STARTED.get().is_some() {
        let host = crate::host::host();
        let mut hidden = HIDDEN.lock().unwrap_or_else(|e| e.into_inner());
        for label in host.visible_other_windows() {
            if host.hide_window(&label) {
                crate::host::log(&format!("[gamescope] {label} window hidden while the launcher holds the screen"));
                hidden.push(label);
            }
        }
        // Rare first now, not on the keeper's next look.
        if let Err(e) = write_rare_first() {
            crate::host::log(&format!("[gamescope] holding the screen by app id: {e}"));
        }
    }
    true
}

/// Puts Rare's id first in the base-layer list (and Rare's apps out of it) right away.
#[cfg(target_os = "linux")]
fn write_rare_first() -> Result<(), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    let (conn, screen) = x11rb::connect(Some(&base_display())).map_err(|e| format!("x11: {e}"))?;
    let root = conn.setup().roots[screen].root;
    let atom = intern(&conn, CTRL_APPID)?;
    let reply = conn.get_property(false, root, atom, AtomEnum::ANY, 0, 256).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    let list: Vec<u64> = reply.value32().map(|v| v.map(u64::from).collect()).unwrap_or_default();
    if let Some(next) = super::appid::place(&list, true, None, &super::appid::all()) {
        let values: Vec<u32> = next.iter().map(|v| *v as u32).collect();
        conn.change_property32(PropMode::REPLACE, root, atom, AtomEnum::CARDINAL, &values).map_err(|e| e.to_string())?;
        conn.sync().map_err(|e| e.to_string())?;
        crate::host::log(&format!("[gamescope] base-layer apps {list:?} -> {next:?} (held)"));
    }
    Ok(())
}

/// Shows Rare's windows `hold_front` hid (they are the newest xdg surfaces again, so in front).
fn show_hidden() {
    let labels = std::mem::take(&mut *HIDDEN.lock().unwrap_or_else(|e| e.into_inner()));
    #[cfg(target_os = "linux")]
    if STARTED.get().is_some() {
        for label in labels {
            crate::host::host().show_window(&label);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = labels;
}

/// The app id of the program Rare started that has the screen (an emulator, an app, a web game
/// in Chrome), when there is one: it goes first in the list (overlay/appid.rs).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn front_app() -> Option<u32> {
    if HOLD_FRONT.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    crate::host::host().front_run().and_then(|r| super::appid::of(&r))
}

/// gamescope's choice in `--steam` mode (pick_primary_focus_and_override, Steam-controlled), for
/// the tests: the control window when it names a focusable window, else the first app id in the
/// list that has one, else nothing (no fallback: the screen stays black). `windows` are the
/// focusable ones (they carry an app id), newest map first, as gamescope sorts them.
#[cfg(test)]
fn gamescope_pick(ctrl_window: Option<u64>, ctrl_appids: &[u64], windows: &[(u64, u64)]) -> Option<u64> {
    let focusable: Vec<&(u64, u64)> = windows.iter().filter(|(_, app)| *app != 0).collect();
    if let Some(w) = ctrl_window.filter(|w| *w != 0) {
        if focusable.iter().any(|(x, _)| *x == w) {
            return Some(w);
        }
    }
    ctrl_appids.iter().find_map(|id| focusable.iter().find(|(_, app)| app == id).map(|(x, _)| *x))
}

/// Rare OS game mode, at start: tags the main window and keeps Rare's id in the base-layer list.
/// Other Rare windows (web games, the in-app browser) are tagged with `claim` when they open.
pub fn session_start() {
    if !session_client() {
        return;
    }
    crate::host::log(&format!("[gamescope] session client: windows carry app id {RARE_APPID}{}", if crate::host::host().wayland_client() { " (Wayland, from the scope)" } else { "" }));
    let _ = STARTED.set(());
    claim("main");
    #[cfg(target_os = "linux")]
    {
        super::appid::start_tagger();
        std::thread::spawn(move || {
            let base = base_display();
            loop {
                if let Err(e) = keep_rare_listed(&base) {
                    crate::host::log(&format!("[gamescope] base-layer app list on {base}: {e}"));
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }
}

/// Tags a Rare window with `RARE_APPID` so gamescope can focus it (Rare OS game mode only; Steam
/// tags a shortcut's windows itself). Off the calling thread: the X id needs the main thread.
pub fn claim(label: &str) {
    // As a Wayland client every window has Rare's id already, from the scope (render.rs).
    if !session_client() || crate::host::host().wayland_client() {
        return;
    }
    let label = label.to_string();
    std::thread::spawn(move || {
        // The X window exists once GTK realized it; look often, the screen is black until then.
        for _ in 0..200 {
            if let Some(x) = crate::host::host().window_xid(&label) {
                match tag_window(x) {
                    Ok(()) => crate::host::log(&format!("[gamescope] {} window 0x{x:x} tagged", label)),
                    Err(e) => crate::host::log(&format!("[gamescope] tagging {} window 0x{x:x} failed: {e}", label)),
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        crate::host::log(&format!("[gamescope] no X11 id for the {} window", label));
    });
}

#[cfg(target_os = "linux")]
pub(super) fn intern(conn: &impl x11rb::connection::Connection, name: &str) -> Result<u32, String> {
    use x11rb::protocol::xproto::ConnectionExt as _;
    Ok(conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom)
}

/// `STEAM_GAME` on a window of our own Xwayland server (DISPLAY).
#[cfg(target_os = "linux")]
fn tag_window(xid: u64) -> Result<(), String> {
    use x11rb::protocol::xproto::{AtomEnum, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    let (conn, _) = x11rb::connect(None).map_err(|e| format!("x11: {e}"))?;
    let atom = intern(&conn, GAME_PROP)?;
    conn.change_property32(PropMode::REPLACE, xid as u32, atom, AtomEnum::CARDINAL, &[RARE_APPID]).map_err(|e| e.to_string())?;
    conn.sync().map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
fn tag_window(_xid: u64) -> Result<(), String> {
    Ok(())
}

/// Watches server 0's root and puts Rare's id back where `rare_first` says after every write by
/// Steam and every change of what has the screen. Returns on a lost connection.
#[cfg(target_os = "linux")]
fn keep_rare_listed(display: &str) -> Result<(), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, PropMode};
    use x11rb::protocol::Event;
    use x11rb::wrapper::ConnectionExt as _;
    let (conn, screen) = x11rb::connect(Some(display)).map_err(|e| format!("x11: {e}"))?;
    let root = conn.setup().roots[screen].root;
    let atom = intern(&conn, CTRL_APPID)?;
    let focusable = intern(&conn, FOCUSABLE_APPS)?;
    let shown = intern(&conn, FOCUSED_APP)?;
    // Unknown again when this watcher ends (a lost connection).
    struct Unknown;
    impl Drop for Unknown {
        fn drop(&mut self) {
            SHOWN_APP.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let _unknown = Unknown;
    conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE)).map_err(|e| e.to_string())?;
    let ensure = |(first, front): (bool, Option<u32>)| -> Result<(), String> {
        let reply = conn.get_property(false, root, atom, AtomEnum::ANY, 0, 256).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let list: Vec<u64> = reply.value32().map(|v| v.map(u64::from).collect()).unwrap_or_default();
        if let Some(next) = super::appid::place(&list, first, front, &super::appid::all()) {
            let values: Vec<u32> = next.iter().map(|v| *v as u32).collect();
            conn.change_property32(PropMode::REPLACE, root, atom, AtomEnum::CARDINAL, &values).map_err(|e| e.to_string())?;
            conn.flush().map_err(|e| e.to_string())?;
            crate::host::log(&format!("[gamescope] base-layer apps {list:?} -> {next:?}"));
        }
        Ok(())
    };
    let want = || (rare_first(), front_app());
    let mut last = want();
    ensure(last)?;
    let mut ticks = 0u32;
    loop {
        // Steam's writes arrive as events; the front changes in-process, so look every 100 ms.
        let mut dirty = false;
        while let Some(ev) = conn.poll_for_event().map_err(|e| e.to_string())? {
            if let Event::PropertyNotify(ev) = ev {
                dirty |= ev.window == root && ev.atom == atom;
            }
        }
        ticks = ticks.wrapping_add(1);
        // What has the screen, for the pad thread (a game's presses are not the launcher's).
        if let Ok(c) = conn.get_property(false, root, shown, AtomEnum::CARDINAL, 0, 1) {
            if let Ok(r) = c.reply() {
                let v = r.value32().and_then(|mut v| v.next()).map(u64::from).unwrap_or(0);
                SHOWN_APP.store(v, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if ticks % 5 == 0 {
            // A game that ended hands the screen back without waiting for a button press, and the
            // started programs that have a window now count as shown (the launch progress, and "didn't start").
            let reply = conn.get_property(false, root, focusable, AtomEnum::CARDINAL, 0, 256).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
            let apps: Vec<u64> = reply.value32().map(|v| v.map(u64::from).collect()).unwrap_or_default();
            crate::host::host().focusable_apps_seen(&apps);
        }
        let now = want();
        if dirty || now != last {
            last = now;
            ensure(last)?;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn xprop(display: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("xprop").arg("-display").arg(display).args(args).output().map_err(|e| format!("xprop: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `NAME(CARDINAL) = 1, 2, 3` (or `NAME = 1, 2` with -notype) to its numbers; empty when the
/// property is missing (`NAME:  not found.`).
pub fn parse_cardinals(out: &str) -> Vec<u64> {
    let Some((_, values)) = out.split_once('=') else { return vec![] };
    values.split(',').filter_map(|v| v.trim().parse::<u64>().ok()).collect()
}

/// A root property on a display, as numbers.
pub fn get(display: &str, name: &str) -> Vec<u64> {
    xprop(display, &["-root", "-notype", name]).map(|o| parse_cardinals(&o)).unwrap_or_default()
}

pub fn set(display: &str, name: &str, value: u64) -> Result<(), String> {
    xprop(display, &["-root", "-f", name, "32c", "-set", name, &value.to_string()]).map(|_| ())
}

pub fn remove(display: &str, name: &str) -> Result<(), String> {
    xprop(display, &["-root", "-remove", name]).map(|_| ())
}

/// The display of gamescope's server 0, where the focus controls live. Found once: the first of
/// our own DISPLAY, :0, :1, :2 whose root says server id 0; :0 when none answers.
pub fn base_display() -> String {
    static BASE: OnceLock<String> = OnceLock::new();
    BASE.get_or_init(|| {
        let own = std::env::var("DISPLAY").unwrap_or_default();
        let mut tried = Vec::new();
        for d in [own.as_str(), ":0", ":1", ":2"] {
            if d.is_empty() || tried.contains(&d) {
                continue;
            }
            tried.push(d);
            if get(d, SERVER_ID).first() == Some(&0) {
                return d.to_string();
            }
        }
        ":0".to_string()
    })
    .clone()
}

/// Marker file: present while Rare holds the base layer, so a crash or restart can let go.
fn marker() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("rare-overlay-baselayer"))
}

/// The window Rare holds in front, while it holds the base layer.
///
/// Steam writes `GAMESCOPECTRL_BASELAYER_WINDOW` = 0 on its own focus changes (seen on the SuiPlay
/// in Game Mode: a controller connecting or going away, a second or two after Rare raised its
/// window), which drops Rare's window back behind the game. While Rare holds the base layer a
/// keeper thread puts it back. The lock is held across each check and write, so a release never
/// races a late re-raise.
static HELD: Mutex<Option<u64>> = Mutex::new(None);

fn keeper() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(200));
            let held = HELD.lock().unwrap_or_else(|e| e.into_inner());
            let Some(xid) = *held else { continue };
            let base = base_display();
            if get(&base, CTRL_WINDOW).first() != Some(&xid) {
                match set(&base, CTRL_WINDOW, xid) {
                    Ok(()) => crate::host::log(&format!("[overlay] base layer was reset (Steam); 0x{xid:x} in front again")),
                    Err(e) => crate::host::log(&format!("[overlay] re-raising 0x{xid:x} failed: {e}")),
                }
            }
        });
    });
}

/// Puts the window `xid` in front of whatever has the screen, and keeps it there until `release`.
/// Returns the previous control value (Steam's, if it had set one) so `release` can put it back.
pub fn raise(xid: u64) -> Result<Option<u64>, String> {
    let base = base_display();
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    let prev = get(&base, CTRL_WINDOW).first().copied().filter(|v| *v != 0 && *v != xid);
    set(&base, CTRL_WINDOW, xid)?;
    *held = Some(xid);
    drop(held);
    keeper();
    if let Some(m) = marker() {
        let _ = std::fs::write(m, prev.map(|p| p.to_string()).unwrap_or_default());
    }
    Ok(prev)
}

/// Lets go of the base layer: back to Steam's choice (the game).
pub fn release(prev: Option<u64>) -> Result<(), String> {
    if HOLD_FRONT.swap(false, std::sync::atomic::Ordering::Relaxed) {
        // Held by app id: the keeper moves Rare's id back on its next look, and Rare's other
        // windows come back in front of the main window.
        show_hidden();
        return Ok(());
    }
    let base = base_display();
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    *held = None;
    if let Some(m) = marker() {
        let _ = std::fs::remove_file(m);
    }
    match prev {
        Some(p) => set(&base, CTRL_WINDOW, p),
        None => remove(&base, CTRL_WINDOW),
    }
}

/// At start: if a previous run still held the base layer (crash, restart), let go of it.
pub fn release_stale() {
    let Some(m) = marker() else { return };
    let Ok(t) = std::fs::read_to_string(&m) else { return };
    let prev = t.trim().parse::<u64>().ok();
    crate::host::log("[overlay] releasing the base layer a previous run held");
    let _ = release(prev);
}

/// What gamescope has on screen now (0 when unknown).
pub fn focused_window() -> u64 {
    get(&base_display(), FOCUSED_WINDOW).first().copied().unwrap_or(0)
}

/// The app id gamescope has on screen (a Steam app id for Steam games).
pub fn focused_app() -> Option<u64> {
    get(&base_display(), FOCUSED_APP).first().copied().filter(|v| *v != 0)
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Asks gamescope for a frame of the game and copies it to `dest`. Blocks up to `timeout`.
/// None when gamescope didn't write one in time (older builds, no game frame).
pub fn screenshot(dest: &Path, timeout: Duration) -> Option<PathBuf> {
    let file = Path::new(SCREENSHOT_FILE);
    let before = mtime(file);
    let base = base_display();
    let asked = SystemTime::now();
    if let Err(e) = set(&base, SCREENSHOT, BASE_PLANE_ONLY as u64) {
        crate::host::log(&format!("[overlay] screenshot request failed: {e}"));
        return None;
    }
    let t0 = Instant::now();
    let mut last_len = None;
    while t0.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(25));
        let Some(m) = mtime(file) else { continue };
        if Some(m) == before || m < asked - Duration::from_millis(50) {
            continue;
        }
        // Written once gamescope removed the request, or once the size stops changing.
        let len = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        let done = get(&base, SCREENSHOT).is_empty() || (len > 0 && last_len == Some(len));
        last_len = Some(len);
        if done && len > 0 {
            if let Some(dir) = dest.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            return std::fs::copy(file, dest).ok().map(|_| dest.to_path_buf());
        }
    }
    crate::host::log(&format!("[overlay] no game frame from gamescope within {} ms", timeout.as_millis()));
    // Don't leave the request behind for Steam to find.
    let _ = remove(&base, SCREENSHOT);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_xprop_cardinals() {
        assert_eq!(parse_cardinals("GAMESCOPE_FOCUSED_WINDOW = 4194311\n"), vec![4194311]);
        assert_eq!(parse_cardinals("GAMESCOPE_FOCUSABLE_WINDOWS(CARDINAL) = 4194311, 769, 1201, 6291462, 3001, 1300\n"), vec![4194311, 769, 1201, 6291462, 3001, 1300]);
        assert!(parse_cardinals("GAMESCOPECTRL_BASELAYER_WINDOW:  not found.\n").is_empty());
        assert_eq!(parse_cardinals("GAMESCOPE_XWAYLAND_SERVER_ID = 0"), vec![0]);
    }

    #[test]
    fn cgroup_app_id_as_gamescope_reads_it() {
        assert_eq!(appid_from_cgroup("0::/user.slice/user@1000.service/app.slice/app-steam-app1380012613-77.scope\n"), Some(RARE_APPID));
        assert_eq!(appid_from_cgroup("0::/user.slice/user@1000.service/app.slice/gamescope-session-plus@rare.service\n"), None);
        assert_eq!(appid_from_cgroup("0::/app.slice/app-steam-app0-5.scope\n"), None);
        assert_eq!(appid_from_cgroup("0::/app.slice/app-steam-app769.scope\n"), None);
    }

    const RARE: u64 = RARE_APPID as u64;
    const STEAM: u64 = 769;
    const GAME: u64 = 1245620;
    const MAIN: u64 = 0x400007;
    const WEBGAME: u64 = 0x400019;
    const STEAM_WIN: u64 = 0x600003;
    const GAME_WIN: u64 = 0x800002;

    /// What the keeper leaves in the list after Steam wrote `steam_wrote`.
    fn kept(steam_wrote: &[u64], first: bool) -> Vec<u64> {
        place_rare(steam_wrote, first).unwrap_or_else(|| steam_wrote.to_vec())
    }

    #[test]
    fn rare_id_placement() {
        assert_eq!(place_rare(&[], true), Some(vec![RARE]));
        assert_eq!(place_rare(&[], false), Some(vec![RARE]));
        assert_eq!(place_rare(&[STEAM], true), Some(vec![RARE, STEAM]));
        assert_eq!(place_rare(&[STEAM], false), Some(vec![STEAM, RARE]));
        assert_eq!(place_rare(&[RARE, STEAM], true), None);
        assert_eq!(place_rare(&[GAME, STEAM, RARE], false), None);
        // Front changes move it, Steam's order stays.
        assert_eq!(place_rare(&[RARE, GAME, STEAM], false), Some(vec![GAME, STEAM, RARE]));
        assert_eq!(place_rare(&[GAME, STEAM, RARE], true), Some(vec![RARE, GAME, STEAM]));
        // Duplicates collapse.
        assert_eq!(place_rare(&[RARE, STEAM, RARE], true), Some(vec![RARE, STEAM]));
    }

    #[test]
    fn rare_id_is_not_steams() {
        assert_ne!(RARE, STEAM);
        assert!(RARE_APPID < 0x8000_0000, "shortcut ids have the top bit set");
        assert!(RARE_APPID > 10_000_000, "well above real Steam app ids");
    }

    /// The boot bug: no Steam, an untagged window, no control. gamescope shows nothing.
    #[test]
    fn untagged_window_is_never_shown() {
        assert_eq!(gamescope_pick(None, &[], &[(MAIN, 0)]), None);
        assert_eq!(gamescope_pick(None, &[STEAM], &[(MAIN, 0)]), None);
    }

    #[test]
    fn rare_shows_at_boot() {
        assert_eq!(gamescope_pick(None, &kept(&[], true), &[(MAIN, RARE)]), Some(MAIN));
        // A web game opens: Rare's newest window.
        assert_eq!(gamescope_pick(None, &kept(&[], true), &[(WEBGAME, RARE), (MAIN, RARE)]), Some(WEBGAME));
    }

    /// (a) Steam started quietly (-silent) while the launcher has the screen writes its own list,
    /// with or without a window of its own mapped: the launcher stays in front.
    #[test]
    fn quiet_steam_never_takes_the_screen() {
        for wrote in [vec![], vec![STEAM], vec![STEAM, GAME]] {
            let list = kept(&wrote, true);
            assert_eq!(gamescope_pick(None, &list, &[(STEAM_WIN, STEAM), (MAIN, RARE)]), Some(MAIN), "{wrote:?}");
            assert_eq!(gamescope_pick(None, &list, &[(MAIN, RARE), (STEAM_WIN, STEAM)]), Some(MAIN), "{wrote:?}");
            assert_eq!(gamescope_pick(None, &list, &[(MAIN, RARE)]), Some(MAIN), "{wrote:?}");
        }
    }

    #[test]
    fn steam_has_the_screen_once_asked() {
        // Play: Steam still starting, no window yet. The launcher (and its progress bar) stays.
        assert_eq!(gamescope_pick(None, &kept(&[STEAM], false), &[(MAIN, RARE)]), Some(MAIN));
        // Steam puts up its UI (a login, an update), then the game.
        assert_eq!(gamescope_pick(None, &kept(&[STEAM], false), &[(STEAM_WIN, STEAM), (MAIN, RARE)]), Some(STEAM_WIN));
        let game = kept(&[GAME, STEAM], false);
        let wins = [(GAME_WIN, GAME), (STEAM_WIN, STEAM), (MAIN, RARE)];
        assert_eq!(gamescope_pick(None, &game, &wins), Some(GAME_WIN));
        // The overlay raises Rare over the game, and lets go.
        assert_eq!(gamescope_pick(Some(MAIN), &game, &wins), Some(MAIN));
        assert_eq!(gamescope_pick(None, &game, &wins), Some(GAME_WIN));
        // Steam's "no window" control (0) changes nothing.
        assert_eq!(gamescope_pick(Some(0), &game, &wins), Some(GAME_WIN));
        // The game ends: the front goes back to the launcher, Rare first over Steam's UI.
        assert_eq!(gamescope_pick(None, &kept(&[STEAM], true), &[(STEAM_WIN, STEAM), (MAIN, RARE)]), Some(MAIN));
        // Steam quits and leaves its last list behind: Rare, not a black screen.
        assert_eq!(gamescope_pick(None, &kept(&[GAME, STEAM], false), &[(MAIN, RARE)]), Some(MAIN));
    }

    /// (b) The device hot fix tagged Rare 769. Once Steam runs, Steam's own list ([769]) matches
    /// Rare's window and Steam's alike and the newest mapped one wins: Rare's web game or browser
    /// window covers Steam's UI, or Steam's window covers the launcher, by map order alone. With
    /// its own id, Rare's place in the list decides.
    #[test]
    fn steams_id_would_collide() {
        assert_eq!(gamescope_pick(None, &[STEAM], &[(MAIN, STEAM), (STEAM_WIN, STEAM)]), Some(MAIN));
        assert_eq!(gamescope_pick(None, &[STEAM], &[(STEAM_WIN, STEAM), (MAIN, STEAM)]), Some(STEAM_WIN));
        for wins in [[(MAIN, RARE), (STEAM_WIN, STEAM)], [(STEAM_WIN, STEAM), (MAIN, RARE)]] {
            assert_eq!(gamescope_pick(None, &kept(&[STEAM], false), &wins), Some(STEAM_WIN));
            assert_eq!(gamescope_pick(None, &kept(&[STEAM], true), &wins), Some(MAIN));
        }
    }
}
