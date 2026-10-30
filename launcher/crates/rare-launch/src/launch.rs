//! Starting games and knowing which ones are running.

use rare_library::model::{Game, LaunchSpec};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use rare_core::{Ctx, Event};
use rare_core::events::RunningState;

enum Handle {
    /// A program the launcher started. `unit` is its systemd scope (Linux), `cgroup` that
    /// scope's cgroup once seen: the game counts as running while any process is left in it,
    /// so launchers that fork and exit (shell scripts, AppImages, Flatpak) are still tracked.
    Child { child: Child, unit: Option<String>, cgroup: Option<std::path::PathBuf>, started: Instant, exit: Option<i32> },
    /// A web game in the launcher's own webview window.
    Window { label: String },
    /// Started through another launcher (Heroic): running while a process has `dir` in its
    /// command line.
    Dir { dir: String, seen: bool, started: Instant, grace: Duration },
    /// Started through Steam; tracked by Steam's reaper process (`AppId=<id>` in its command
    /// line, not an install script's). Until the reaper shows up it counts as running for `grace`,
    /// or while Steam waits for the player (steamlog.rs). `seen_at`: when the reaper showed up.
    Steam { ids: Vec<u64>, seen: bool, seen_at: Option<Instant>, started: Instant, grace: Duration },
    /// Steam's own UI ("app:steam"): open while the Steam client runs. Until Steam shows up it
    /// counts as open for `grace` (a cold start, or a client update first).
    SteamUi { seen: bool, started: Instant, grace: Duration },
}

pub use rare_core::events::LaunchFailed;

/// Without window information (outside Rare OS game mode), a program gone this soon failed to start.
pub const QUICK_EXIT: Duration = Duration::from_secs(5);
/// A window should show within this long of a start (for Steam games, of Steam being up).
pub const NO_WINDOW: Duration = Duration::from_secs(45);
/// An end this soon after the first window is a crash, not a quit.
pub const CRASH_WINDOW: Duration = Duration::from_secs(10);

/// What a run that just ended means for the player, if anything (pure, for the tests).
/// `kind`: child, steam, dir, steamui or window. `seen`: its game showed up (Steam, Heroic).
/// `alive_for`: how long it ran. `code`: exit code (Rare's own programs). `shown_for`: time from
/// its first window to its end, when it had one. `windows_known`: Rare OS game mode, where
/// gamescope says which apps have windows.
pub fn failed_start(kind: &str, seen: bool, alive_for: Duration, code: Option<i32>, shown_for: Option<Duration>, windows_known: bool) -> Option<(&'static str, Option<i32>)> {
    match kind {
        "steam" if !seen => return Some(("steam", None)),
        "dir" if !seen => return Some(("never", None)),
        "steamui" if !seen => return Some(("steamui", None)),
        // Steam closing and a web game window closing are always the player's doing.
        "steamui" | "window" => return None,
        _ => {}
    }
    let error = code.is_some_and(|c| c != 0);
    match shown_for {
        Some(d) if d < CRASH_WINDOW => Some(("crash", code)),
        Some(_) if error => Some(("crash", code)),
        Some(_) => None,
        // No window ever, in game mode: it never started, unless it handed off at once with
        // exit 0 (a browser or AppImage passing to a copy already running).
        None if windows_known => (!(code == Some(0) && alive_for < QUICK_EXIT)).then_some(("quick", code)),
        None if alive_for < QUICK_EXIT && code != Some(0) && kind == "child" => Some(("quick", code)),
        None if error => Some(("crash", code)),
        None => None,
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// The runs whose app id gamescope lists as having a window (`GAMESCOPE_FOCUSABLE_APPS`).
pub fn shown_runs(apps: &[(String, Vec<u64>)], focusable: &[u64]) -> Vec<String> {
    apps.iter().filter(|(_, ids)| ids.iter().any(|i| focusable.contains(i))).map(|(r, _)| r.clone()).collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// Steam's own UI windows carry 769 in gamescope.
pub const STEAM_UI_APPID: u64 = 769;

/// A Steam game whose reaper runs this long without a window gamescope lists under its id gets
/// the screen anyway (a game that shows under another id must never stay hidden behind Rare).
pub const REAPER_SHOW: Duration = Duration::from_secs(15);

/// A process is the Steam game's reaper: `AppId=<id>` in its command line, and not the reaper
/// Steam runs an install script under (`Install=1`, the Proton iscriptevaluator before the game:
/// on the device it ran and ended while Steam still waited on a cloud conflict, which read as a
/// failed start).
pub fn game_reaper(cmd: &str, needles: &[String]) -> bool {
    let mut tokens = cmd.split(' ');
    let hit = tokens.clone().any(|t| needles.iter().any(|n| n == t));
    hit && !tokens.any(|t| t == "Install=1")
}

/// Window information exists: Rare OS game mode, where the base-layer keeper reads gamescope.
fn windows_known() -> bool {
    rare_overlay::gamescope::session_client()
}

#[derive(Default, Clone)]
pub struct Runner {
    inner: Arc<Mutex<HashMap<String, Handle>>>,
    /// Runs the player quit (Close, Quit): their end is never a failed start.
    quitting: Arc<Mutex<std::collections::HashSet<String>>>,
    /// When each run first had a window on screen (Rare OS game mode).
    shown: Arc<Mutex<HashMap<String, Instant>>>,
    /// When each run's wait for a window started (for Steam games: once Steam is up), and
    /// whether "didn't start" was said already.
    waiting: Arc<Mutex<HashMap<String, (Instant, bool)>>>,
    /// Steam games whose start waits for the player in Steam's UI, with what Steam asks.
    asks: Arc<Mutex<HashMap<String, &'static str>>>,
}

fn detached(cmd: &mut Command) -> &mut Command {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
}

/// Variables the AppImage runtime (or the launcher's own session) sets that must not leak
/// into Steam or games.
const APPIMAGE_VARS: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "PYTHONHOME",
    "PYTHONPATH",
    "PYTHONDONTWRITEBYTECODE",
    "PERLLIB",
    "GDK_PIXBUF_MODULE_FILE",
    "GIO_MODULE_DIR",
    "QT_PLUGIN_PATH",
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "RARE_FULLSCREEN",
    "RARE_DEBUG_NAV",
];

/// The environment for programs the launcher starts. When running as an AppImage (APPDIR
/// set), strips the bundle's variables and every path entry that points into the mount.
/// Without APPDIR the environment is returned unchanged.
pub fn clean_env(vars: Vec<(String, String)>) -> Vec<(String, String)> {
    // Rare OS game mode marks the launcher as the session's client (sessions.d/rare); Steam and
    // games are not.
    let vars: Vec<(String, String)> = vars.into_iter().filter(|(k, _)| k != "RARE_SESSION_CLIENT").collect();
    // How the launcher's own WebKit draws (render.rs): GDK_BACKEND=wayland and the rest are for
    // the launcher only, never for Steam or games (SDL and GTK games would leave Xwayland).
    let own: Vec<String> = vars.iter().find(|(k, _)| k == rare_system::render::SET_VAR).map(|(_, v)| v.split(',').map(str::to_string).collect()).unwrap_or_default();
    let vars: Vec<(String, String)> = vars
        .into_iter()
        .filter(|(k, _)| !own.contains(k) && !matches!(k.as_str(), "RARE_WEBKIT" | "RARE_WEBKIT_SET" | "RARE_WEBKIT_SCOPED"))
        .collect();
    // Started by Steam (packaging/AppRun): the launcher dropped Steam's overlay preload for itself;
    // games and apps get it back.
    let vars = match vars.iter().find(|(k, _)| k == "RARE_ORIG_LD_PRELOAD").map(|(_, v)| v.clone()) {
        Some(orig) => {
            let mut out: Vec<(String, String)> = vars.into_iter().filter(|(k, _)| k != "RARE_ORIG_LD_PRELOAD" && k != "LD_PRELOAD").collect();
            if !orig.is_empty() {
                out.push(("LD_PRELOAD".into(), orig));
            }
            out
        }
        None => vars,
    };
    // Started on the system's WebKitGTK (packaging/AppRun): undo the LD_LIBRARY_PATH it set.
    let vars = if vars.iter().any(|(k, _)| k == "RARE_SYSTEM_WEBKIT") {
        let orig = vars.iter().find(|(k, _)| k == "RARE_ORIG_LD_LIBRARY_PATH").map(|(_, v)| v.clone()).unwrap_or_default();
        let mut out: Vec<(String, String)> = vars.into_iter().filter(|(k, _)| !matches!(k.as_str(), "RARE_SYSTEM_WEBKIT" | "RARE_ORIG_LD_LIBRARY_PATH" | "LD_LIBRARY_PATH")).collect();
        if !orig.is_empty() {
            out.push(("LD_LIBRARY_PATH".into(), orig));
        }
        out
    } else {
        vars
    };
    let appdir = vars.iter().find(|(k, _)| k == "APPDIR").map(|(_, v)| v.trim_end_matches('/').to_string()).unwrap_or_default();
    if appdir.is_empty() {
        return vars;
    }
    let inside = |v: &str| v.contains(appdir.as_str());
    vars.into_iter()
        .filter_map(|(k, v)| {
            if APPIMAGE_VARS.contains(&k.as_str()) || k.starts_with("GST_") {
                return None;
            }
            if (k.starts_with("GTK_") || k == "GSETTINGS_SCHEMA_DIR") && inside(&v) {
                return None;
            }
            if !inside(&v) {
                return Some((k, v));
            }
            // Path lists (PATH, LD_LIBRARY_PATH, XDG_DATA_DIRS, ...): keep the host entries.
            let kept: Vec<&str> = v.split(':').filter(|e| !e.is_empty() && !inside(e)).collect();
            (!kept.is_empty()).then(|| (k, kept.join(":")))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn scope_unit(slug: &str) -> Option<String> {
    if std::env::var("RARE_NO_SCOPE").as_deref() == Ok("1") || !rare_library::steam::which("systemd-run") {
        return None;
    }
    let rt = std::env::var_os("XDG_RUNTIME_DIR")?;
    // Only when a user systemd instance is actually running.
    if !std::path::Path::new(&rt).join("systemd").is_dir() {
        return None;
    }
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    // Rare OS game mode: the scope's name is how gamescope gives the program's windows an app id.
    if let Some(id) = rare_overlay::appid::of(slug).filter(|_| game_mode_tagged(slug)) {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 1000;
        return Some(rare_overlay::appid::scope_unit(id, (ms % 1_000_000) as u32 * 1000 + seq));
    }
    let mut s: String = slug.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    s.truncate(40);
    Some(format!("rare-{}-{ms}", if s.is_empty() { "app" } else { s.trim_end_matches('-') }))
}

/// Programs that get an app id of their own: in Rare OS game mode, everything but Steam (which
/// tags its own windows and games; its games must keep their reaper's id). When Steam started
/// Rare as a shortcut, Rare's programs keep the shortcut's id from the reaper above them.
fn game_mode_tagged(slug: &str) -> bool {
    slug != "steam" && rare_overlay::gamescope::session_client()
}

/// A command for an external program, with the host environment (see `clean_env`). On Linux
/// it runs in its own transient systemd scope, so Steam and games do not live in the
/// launcher's unit and survive a launcher restart. `systemd-run --scope` execs the program in
/// place, so the returned Child is still the program. RARE_NO_SCOPE=1 turns the scope off.
pub fn host_command(program: &str, args: &[String], slug: &str) -> Command {
    host_command_unit(program, args, slug).0
}

/// `host_command`, plus the name of the systemd scope the program runs in (Linux only).
pub fn host_command_unit(program: &str, args: &[String], slug: &str) -> (Command, Option<String>) {
    if game_mode_tagged(slug) {
        rare_overlay::appid::assign(slug);
    }
    #[cfg(target_os = "linux")]
    let unit = scope_unit(slug);
    #[cfg(not(target_os = "linux"))]
    let unit: Option<String> = None;
    #[cfg(target_os = "linux")]
    let mut cmd = match unit.clone() {
        Some(unit) => {
            let mut c = Command::new("systemd-run");
            c.args(["--user", "--scope", "--collect", "--quiet"]).arg(format!("--unit={unit}")).arg("--").arg(program).args(args);
            c
        }
        None => {
            let mut c = Command::new(program);
            c.args(args);
            c
        }
    };
    #[cfg(not(target_os = "linux"))]
    let mut cmd = {
        let _ = slug;
        let mut c = Command::new(program);
        c.args(args);
        c
    };
    host_env(&mut cmd);
    if program == "flatpak" && game_mode_tagged(slug) {
        // Flatpak apps move to a scope of their own, so gamescope finds their id only on an X11
        // window Rare tags (overlay/appid.rs); without WAYLAND_DISPLAY they use X11.
        cmd.env_remove("WAYLAND_DISPLAY");
    }
    detached(&mut cmd);
    (cmd, unit)
}

/// Gives a command the host environment instead of the AppImage's (see `clean_env`).
pub fn host_env(cmd: &mut Command) -> &mut Command {
    if std::env::var_os("APPDIR").is_some() {
        let env: Vec<(String, String)> = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
        let clean: HashMap<String, String> = clean_env(env.clone()).into_iter().collect();
        // Applied as a diff, so variables that are not UTF-8 pass through untouched.
        for (k, v) in env {
            match clean.get(&k) {
                None => {
                    cmd.env_remove(&k);
                }
                Some(c) if *c != v => {
                    cmd.env(&k, c);
                }
                _ => {}
            }
        }
    }
    cmd
}

/// Steam's "Switch to Desktop" runs `steamos-session-select plasma` through the PATH Steam was
/// started with (its log: "YldSwitchToDesktop: No SteamOSManager support for SwitchToDesktop,
/// using legacy path"). In Rare OS game mode, Steam started by Rare finds this script first: it
/// asks the launcher to take the screen back (`BACK_TO_RARE`, read by `Runner::watch`), so Steam's
/// menu leads back to Rare, not the desktop. Anything else goes to the system's script.
const SESSION_SELECT: &str = r#"#!/usr/bin/bash
# Written by the Rare launcher (rare-launch launch.rs) for the Steam it starts in Rare OS game mode.
case "${1:-}" in
plasma* | gnome* | desktop)
    if [[ -n ${XDG_RUNTIME_DIR:-} && -d $XDG_RUNTIME_DIR ]]; then
        : >"$XDG_RUNTIME_DIR/rare-back-to-rare"
        exit 0
    fi
    ;;
esac
exec /usr/bin/steamos-session-select "$@"
"#;

/// The file `SESSION_SELECT` creates in $XDG_RUNTIME_DIR.
pub const BACK_TO_RARE: &str = "rare-back-to-rare";

/// Writes the `SESSION_SELECT` script and returns its folder, for Steam's PATH.
#[cfg(target_os = "linux")]
fn session_select_dir() -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = dirs::data_dir()?.join("rare/steam-path");
    std::fs::create_dir_all(&dir).ok()?;
    let f = dir.join("steamos-session-select");
    if std::fs::read_to_string(&f).ok().as_deref() != Some(SESSION_SELECT) {
        std::fs::write(&f, SESSION_SELECT).ok()?;
    }
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(dir)
}

/// A command that runs the Steam client (`steam_url`, `steam_prewarm`): `host_command`, plus the
/// session-select script first in PATH in Rare OS game mode (only a Steam that starts now keeps it).
fn steam_command(prog: &str, args: &[String]) -> Command {
    #[allow(unused_mut)]
    let mut cmd = host_command(prog, args, "steam");
    #[cfg(target_os = "linux")]
    if rare_overlay::gamescope::session_client() && !steam_running() {
        if let Some(dir) = session_select_dir() {
            let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
            // An AppImage's own folders stay out (as in `clean_env`).
            let appdir = std::env::var("APPDIR").ok().filter(|a| !a.is_empty());
            let kept: Vec<&str> = path.split(':').filter(|e| !e.is_empty() && appdir.as_deref().map_or(true, |a| !e.starts_with(a))).collect();
            cmd.env("PATH", format!("{}:{}", dir.display(), kept.join(":")));
        }
    }
    cmd
}

/// Starts a Steam game. Steam already up: the URL goes straight to it. Steam not running (Rare OS):
/// Steam starts quietly first and gets the URL once its UI process is up, so it never starts on
/// its home screen for a Play press; the launcher keeps the screen meanwhile (apps/front.rs).
/// Returns whether Steam had to start.
pub fn steam_game(url: &str) -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    if !steam_running() && rare_library::steam::which("steam") {
        steam_prewarm_now()?;
        let url = url.to_string();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            while !steam_ready() && t0.elapsed() < Duration::from_secs(180) {
                std::thread::sleep(Duration::from_millis(300));
            }
            // steamwebhelper comes up before Steam takes URLs from a second `steam` process.
            std::thread::sleep(Duration::from_secs(2));
            eprintln!("[launch] Steam is up after {} s; sending {url}", t0.elapsed().as_secs());
            if let Err(e) = steam_url(&url) {
                eprintln!("[launch] {e}");
            }
        });
        return Ok(true);
    }
    steam_url(url)?;
    Ok(false)
}

/// Opens a steam:// URL with the Steam client, starting Steam if needed.
pub fn steam_url(url: &str) -> Result<(), String> {
    let prog = if cfg!(target_os = "macos") {
        "open"
    } else if rare_library::steam::which("steam") {
        "steam"
    } else {
        "xdg-open"
    };
    #[cfg(target_os = "linux")]
    crate::steamcfg::prepare_start();
    let mut child = steam_command(prog, &[url.to_string()]).spawn().map_err(|e| format!("Could not start Steam: {e}"))?;
    // Reap it when it exits (at once when Steam is already running) so it leaves no zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// True once Steam's UI process is up. A cold start reaches it in about 6 s (median of 94 starts
/// on the SuiPlay0X1) but a client update first makes it 25 to 160 s.
pub fn steam_ready() -> bool {
    #[cfg(target_os = "linux")]
    {
        rare_core::proc::cmdlines().iter().any(|(_, comm, _)| comm == "steamwebhelper")
    }
    #[cfg(not(target_os = "linux"))]
    {
        steam_running()
    }
}

#[cfg(target_os = "linux")]
static PREWARM: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Starts Steam with no window (`-silent`: no UI, nothing takes the screen from the launcher)
/// so a later Play press only hands over the game. Does nothing when Steam is running or was
/// started in the last minute.
pub fn steam_prewarm() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if steam_running() || !rare_library::steam::which("steam") {
            return Ok(());
        }
        steam_prewarm_now()?;
    }
    Ok(())
}

/// `steam -silent` now, unless one was started in the last minute (Steam still coming up).
#[cfg(target_os = "linux")]
fn steam_prewarm_now() -> Result<(), String> {
    let mut last = PREWARM.lock().unwrap();
    if last.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
        return Ok(());
    }
    *last = Some(Instant::now());
    drop(last);
    eprintln!("[launch] starting Steam quietly");
    crate::steamcfg::prepare_start();
    let mut child = steam_command("steam", &["-silent".to_string()]).spawn().map_err(|e| format!("Could not start Steam: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// `roots` and every process below them.
#[cfg(target_os = "linux")]
pub fn with_descendants(roots: &[u32]) -> Vec<u32> {
    let parents = rare_core::proc::parents();
    let mut out: Vec<u32> = roots.to_vec();
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        out.extend(parents.iter().filter(|(_, pp)| *pp == p).map(|(c, _)| *c).filter(|c| !roots.contains(c)));
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

/// TERM now, KILL whatever is left after a few seconds.
#[cfg(target_os = "linux")]
fn terminate(pids: Vec<u32>) {
    if pids.is_empty() {
        return;
    }
    let list: Vec<String> = pids.iter().map(|p| p.to_string()).collect();
    let _ = Command::new("kill").arg("-TERM").args(&list).stderr(Stdio::null()).status();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(6));
        let left: Vec<String> = list.into_iter().filter(|p| std::path::Path::new("/proc").join(p).exists()).collect();
        if !left.is_empty() {
            let _ = Command::new("kill").arg("-KILL").args(&left).stderr(Stdio::null()).status();
        }
    });
}

/// The cgroup directory a process lives in (cgroup v2).
#[cfg(target_os = "linux")]
fn cgroup_of(pid: u32) -> Option<std::path::PathBuf> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let rel = text.lines().find_map(|l| l.strip_prefix("0::"))?;
    Some(std::path::Path::new("/sys/fs/cgroup").join(rel.trim_start_matches('/')))
}

#[cfg(target_os = "linux")]
fn cgroup_busy(dir: &std::path::Path) -> bool {
    std::fs::read_to_string(dir.join("cgroup.procs")).map(|t| !t.trim().is_empty()).unwrap_or(false)
}

/// Opens a URL in a Chromium-based browser as a fullscreen app window (web games that need
/// Chrome-only features). Returns the browser's name.
pub fn browser_command(id: &str, url: &str) -> Result<(Command, Option<String>, String), String> {
    let b = rare_library::browser::find().ok_or("No Chromium-based browser is installed. Install Google Chrome from Discover in Desktop mode.")?;
    let slug: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let profile = dirs::data_dir().unwrap_or_default().join("rare/chrome").join(&slug);
    let _ = std::fs::create_dir_all(&profile);
    let args = rare_library::browser::args(&b, url, &profile);
    let (cmd, unit) = host_command_unit(&b.program, &args, id);
    Ok((cmd, unit, b.name))
}

pub use rare_core::proc::steam_running;

/// Extra arguments and environment for one game (the launcher's per-game settings).
pub type LaunchExtra = (Vec<String>, Vec<(String, String)>);

type ExtraSource = Box<dyn Fn(&str) -> Option<LaunchExtra> + Send + Sync>;
static EXTRA: std::sync::OnceLock<ExtraSource> = std::sync::OnceLock::new();

/// Where the per-game launch arguments and environment come from (set once by the launcher).
/// Used for programs Rare starts itself; Steam games keep theirs in Steam's own launch options.
pub fn set_launch_extra(f: impl Fn(&str) -> Option<LaunchExtra> + Send + Sync + 'static) {
    let _ = EXTRA.set(Box::new(f));
}

fn launch_extra(id: &str) -> LaunchExtra {
    EXTRA.get().and_then(|f| f(id)).unwrap_or_default()
}

impl Runner {
    pub fn launch(&self, game: &Game) -> Result<(), String> {
        rare_system::upscale::apply_saved();
        let handle = match &game.launch {
            LaunchSpec::Steam { appid } => {
                let cold = steam_game(&format!("steam://rungameid/{appid}"))?;
                // Steam may need a while to update or start; give it two minutes to show up (three
                // when it had to start first).
                let grace = Duration::from_secs(if cold { 180 } else { 120 });
                crate::steamlog::forget(*appid as u64);
                Handle::Steam { ids: vec![*appid as u64], seen: false, seen_at: None, started: Instant::now(), grace }
            }
            LaunchSpec::SteamShortcut { gameid } => {
                let cold = steam_game(&format!("steam://rungameid/{gameid}"))?;
                // Shortcuts may not get a reaper we can match; never hold "running" for long without one.
                let grace = Duration::from_secs(if cold { 80 } else { 20 });
                Handle::Steam { ids: vec![*gameid >> 32, *gameid], seen: false, seen_at: None, started: Instant::now(), grace }
            }
            LaunchSpec::Web { .. } | LaunchSpec::Cloud { .. } => return Err("Web games open in their own window".into()),
            LaunchSpec::Legendary { app_name } => {
                if !rare_library::steam::which("legendary") {
                    return Err("legendary is missing. Update Rare and try again.".into());
                }
                // Proton through umu-run, in Rare's prefix for the game (the one cloud saves and
                // Uninstall use). legendary starts the game and exits; the scope's cgroup keeps it tracked.
                let mut args = vec!["launch".to_string(), app_name.clone(), "--skip-version-check".to_string()];
                if rare_library::steam::which("umu-run") {
                    args.extend(["--no-wine".to_string(), "--wrapper".to_string(), "umu-run".to_string()]);
                }
                let (mut cmd, unit) = host_command_unit("legendary", &args, &game.id);
                // The game's prefix (Heroic's when it came from Heroic) and its Proton (rare-library umu).
                let _ = std::fs::create_dir_all(rare_library::umu::prefix(&game.id));
                cmd.envs(rare_library::umu::env(&game.id));
                // Rare Local API: RARE_API, RARE_TOKEN, RARE_GAME (Wine passes them to the game).
                cmd.envs(rare_localapi::grant_native(&game.id));
                cmd.envs(launch_extra(&game.id).1);
                Handle::Child { child: cmd.spawn().map_err(|e| e.to_string())?, unit, cgroup: None, started: Instant::now(), exit: None }
            }
            LaunchSpec::Heroic { runner, app_name, dir } => {
                let url = format!("heroic://launch?appName={app_name}&runner={runner}");
                let flatpak = dirs::data_dir().map(|d| d.join("flatpak/app/com.heroicgameslauncher.hgl").is_dir()).unwrap_or(false)
                    || std::path::Path::new("/var/lib/flatpak/app/com.heroicgameslauncher.hgl").is_dir();
                let (prog, args): (&str, Vec<String>) = if rare_library::steam::which("heroic") && !flatpak {
                    ("heroic", vec!["--no-gui".into(), url])
                } else {
                    ("flatpak", vec!["run".into(), "com.heroicgameslauncher.hgl".into(), "--no-gui".into(), url])
                };
                // Heroic stays open after the game; track the game by its folder instead.
                let mut child = host_command(prog, &args, "heroic").spawn().map_err(|e| format!("Could not start Heroic: {e}"))?;
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                // Heroic starts the game in its own scope: the game shows under Heroic's id.
                if let Some(id) = rare_overlay::appid::of("heroic") {
                    rare_overlay::appid::alias(&game.id, id);
                }
                match dir {
                    Some(d) if !d.is_empty() => Handle::Dir { dir: d.clone(), seen: false, started: Instant::now(), grace: Duration::from_secs(90) },
                    _ => return Ok(()),
                }
            }
            LaunchSpec::Exec { program, args, cwd, windows } => {
                let (more, env) = launch_extra(&game.id);
                let args: &Vec<String> = &args.iter().cloned().chain(more).collect();
                let (mut cmd, unit) = if *windows {
                    if !rare_library::steam::which("umu-run") {
                        return Err("Windows games need umu-run (Proton)".into());
                    }
                    let mut all = vec![program.clone()];
                    all.extend(args.iter().cloned());
                    let (mut c, unit) = host_command_unit("umu-run", &all, &game.id);
                    if game.id.starts_with("gog:") {
                        // GOG: the game's prefix (Heroic's when it came from Heroic) and its Proton.
                        let _ = std::fs::create_dir_all(rare_library::umu::prefix(&game.id));
                        c.envs(rare_library::umu::env(&game.id));
                    } else {
                        let prefix = dirs::data_dir().unwrap_or_default().join("rare/prefixes").join(game.id.replace(':', "-"));
                        let _ = std::fs::create_dir_all(&prefix);
                        c.env("WINEPREFIX", prefix).env("GAMEID", "umu-default").env("STORE", "none");
                    }
                    (c, unit)
                } else {
                    host_command_unit(program, args, &game.id)
                };
                // Rare Local API: RARE_API, RARE_TOKEN, RARE_GAME, for native and Proton builds alike.
                cmd.envs(rare_localapi::grant_native(&game.id));
                cmd.envs(env);
                if let Some(d) = cwd.as_deref().filter(|d| std::path::Path::new(d).is_dir()) {
                    cmd.current_dir(d);
                } else if let Some(parent) = std::path::Path::new(program).parent().filter(|p| p.is_absolute() && p.is_dir()) {
                    // Games often load files relative to their own folder.
                    cmd.current_dir(parent);
                }
                Handle::Child { child: cmd.spawn().map_err(|e| format!("Could not start {}: {e}", game.title))?, unit, cgroup: None, started: Instant::now(), exit: None }
            }
        };
        self.quitting.lock().unwrap().remove(game.id.as_str());
        self.inner.lock().unwrap().insert(game.id.clone(), handle);
        Ok(())
    }

    /// A web game window the launcher opened, running until the window closes.
    pub fn track_window(&self, id: &str, label: &str) {
        let mut map = self.inner.lock().unwrap();
        map.retain(|_, h| !matches!(h, Handle::Window { label: l } if l == label));
        map.insert(id.to_string(), Handle::Window { label: label.to_string() });
    }

    /// A program started elsewhere (apps), tracked like a game: its scope's cgroup, or its tree.
    pub fn track_child(&self, id: &str, child: Child, unit: Option<String>) {
        self.quitting.lock().unwrap().remove(id);
        self.inner.lock().unwrap().insert(id.to_string(), Handle::Child { child, unit, cgroup: None, started: Instant::now(), exit: None });
    }

    /// Steam's own UI, open from now until the Steam client quits. Opening it again keeps the
    /// entry it has.
    pub fn track_steam_ui(&self, id: &str) {
        self.quitting.lock().unwrap().remove(id);
        self.inner.lock().unwrap().entry(id.to_string()).or_insert(Handle::SteamUi { seen: false, started: Instant::now(), grace: Duration::from_secs(180) });
    }

    /// Run ids of the programs the launcher started itself, with the pid it started.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn child_pids(&self) -> Vec<(String, u32)> {
        self.inner.lock().unwrap().iter().filter_map(|(id, h)| if let Handle::Child { child, .. } = h { Some((id.clone(), child.id())) } else { None }).collect()
    }

    /// A web game opened in a Chromium browser.
    pub fn launch_browser(&self, id: &str, url: &str) -> Result<String, String> {
        let (mut cmd, unit, name) = browser_command(id, url)?;
        let child = cmd.spawn().map_err(|e| format!("Could not start the browser: {e}"))?;
        self.quitting.lock().unwrap().remove(id);
        self.inner.lock().unwrap().insert(id.to_string(), Handle::Child { child, unit, cgroup: None, started: Instant::now(), exit: None });
        Ok(name)
    }

    /// Ends a running game: its scope or process tree, Steam's reaper and everything under it,
    /// or the web game window. Saves are the game's job; this is what "Quit game" does on a Deck.
    pub fn quit(&self, ctx: &Ctx, id: &str) -> Result<(), String> {
        // A frozen game can't act on SIGTERM (and so never saves): thaw it first.
        rare_system::freeze::thaw(id);
        let mut map = self.inner.lock().unwrap();
        let Some(h) = map.get_mut(id) else { return Err("That game is not running".into()) };
        self.quitting.lock().unwrap().insert(id.to_string());
        match h {
            Handle::Window { label } => {
                ctx.host.close_window(label);
            }
            #[cfg(target_os = "linux")]
            Handle::Child { child, unit, .. } => {
                // TERM to everything in its scope (an emulator saves and exits on it), KILL what is
                // left after 6 s. `systemctl stop` waited out a 90 s timeout on a game that ignored TERM.
                let stopped = unit.as_ref().is_some_and(|u| {
                    let scope = format!("{u}.scope");
                    let ok = Command::new("systemctl").args(["--user", "kill", "--signal=SIGTERM", &scope]).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
                    if ok {
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_secs(6));
                            let active = Command::new("systemctl").args(["--user", "is-active", "--quiet", &scope]).status().map(|s| s.success()).unwrap_or(false);
                            if active {
                                let _ = Command::new("systemctl").args(["--user", "kill", "--signal=SIGKILL", &scope]).stdout(Stdio::null()).stderr(Stdio::null()).status();
                            }
                        });
                    }
                    ok
                });
                if !stopped {
                    terminate(with_descendants(&[child.id()]));
                }
            }
            #[cfg(target_os = "linux")]
            Handle::Steam { ids, .. } => {
                let needles: Vec<String> = ids.iter().map(|i| format!("AppId={i}")).collect();
                let reapers: Vec<u32> = rare_core::proc::cmdlines().into_iter().filter(|(_, _, cmd)| game_reaper(cmd, &needles)).map(|(p, _, _)| p).collect();
                if reapers.is_empty() {
                    return Err("Steam has not started the game yet".into());
                }
                // Children first, so the reaper sees them go and Steam marks the game stopped.
                let all = with_descendants(&reapers);
                terminate(all.into_iter().filter(|p| !reapers.contains(p)).collect());
            }
            Handle::SteamUi { .. } => {
                // Steam closes itself and everything it runs, the way its own Exit does.
                if steam_running() {
                    let mut c = host_command("steam", &["-shutdown".to_string()], "steam").spawn().map_err(|e| format!("Could not close Steam: {e}"))?;
                    std::thread::spawn(move || {
                        let _ = c.wait();
                    });
                } else {
                    map.remove(id);
                }
            }
            #[cfg(target_os = "linux")]
            Handle::Dir { dir, .. } => {
                let pids: Vec<u32> = rare_core::proc::cmdlines().into_iter().filter(|(_, _, cmd)| cmd.contains(dir.as_str())).map(|(p, _, _)| p).collect();
                terminate(with_descendants(&pids));
            }
            #[cfg(not(target_os = "linux"))]
            Handle::Child { child, .. } => {
                let _ = child.kill();
            }
            #[cfg(not(target_os = "linux"))]
            _ => return Err("Quit is only available on Rare OS".into()),
        }
        Ok(())
    }

    pub fn running(&self) -> Vec<String> {
        self.inner.lock().unwrap().keys().cloned().collect()
    }

    /// How long a run has been going (the Rare menu's "Playing for"); None for a run the launcher
    /// does not time (a web game's window) or one that is not running.
    pub fn elapsed(&self, id: &str) -> Option<Duration> {
        let inner = self.inner.lock().unwrap();
        let h = inner.get(id)?;
        // Rare OS game mode knows when its first window showed: that is when play began.
        if let Some(t) = self.shown.lock().unwrap().get(id) {
            return Some(t.elapsed());
        }
        match h {
            Handle::Child { started, .. } | Handle::Dir { started, .. } | Handle::Steam { started, .. } | Handle::SteamUi { started, .. } => Some(started.elapsed()),
            Handle::Window { .. } => None,
        }
    }

    /// What quick resume (freeze.rs) needs to find a running game's processes.
    pub fn freeze_target(&self, id: &str) -> Option<rare_system::freeze::Target> {
        use rare_system::freeze::Target;
        Some(match self.inner.lock().unwrap().get(id)? {
            Handle::Child { child, unit, .. } => Target::Child { pid: child.id(), unit: unit.clone() },
            Handle::Steam { ids, .. } => Target::Steam { ids: ids.clone() },
            Handle::Dir { dir, .. } => Target::Dir { dir: dir.clone() },
            Handle::Window { .. } => Target::Window,
            Handle::SteamUi { .. } => Target::SteamUi,
        })
    }

    /// What Steam asks the player before this game can start (with `phase` "answer").
    pub fn question(&self, id: &str) -> Option<&'static str> {
        self.asks.lock().unwrap().get(id).copied()
    }

    /// Where a launched game is: "steam" (Steam is still starting), "game" (Steam is up, the game
    /// has not started), "answer" (Steam waits for the player, `question`), "playing" or "none".
    /// Drives the progress bar on the game page.
    pub fn phase(&self, id: &str) -> &'static str {
        if windows_known() {
            let shown = self.shown.lock().unwrap().contains_key(id);
            let asked = self.asks.lock().unwrap().contains_key(id);
            return match self.inner.lock().unwrap().get(id) {
                None => "none",
                Some(Handle::Window { .. }) => "playing",
                Some(_) if shown => "playing",
                Some(_) if asked => "answer",
                Some(Handle::Steam { .. } | Handle::SteamUi { .. }) if !steam_ready() => "steam",
                Some(_) => "game",
            };
        }
        match self.inner.lock().unwrap().get(id) {
            Some(Handle::Steam { seen: true, .. }) => "playing",
            Some(Handle::SteamUi { seen: false, .. }) if !steam_ready() => "steam",
            Some(Handle::Steam { .. }) => {
                if steam_ready() {
                    "game"
                } else {
                    "steam"
                }
            }
            Some(_) => "playing",
            None => "none",
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// The app ids each run's windows carry in gamescope: a Steam game's (and a shortcut's) id,
    /// Steam's UI (769), and the id Rare gave a program it started (overlay/appid.rs).
    pub fn app_ids(&self) -> Vec<(String, Vec<u64>)> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, h)| {
                let ids = match h {
                    Handle::Steam { ids, .. } => ids.clone(),
                    Handle::SteamUi { .. } => vec![STEAM_UI_APPID],
                    Handle::Window { .. } => return None,
                    _ => rare_overlay::appid::of(id).map(|v| vec![v as u64]).unwrap_or_default(),
                };
                Some((id.clone(), ids))
            })
            .collect()
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    /// gamescope listed these runs' windows (the base-layer keeper calls this).
    pub fn mark_shown(&self, ids: &[String]) {
        let mut s = self.shown.lock().unwrap();
        for id in ids {
            s.entry(id.clone()).or_insert_with(|| {
                eprintln!("[launch] {id} has a window");
                crate::apps::front::steam_shown(id);
                Instant::now()
            });
        }
    }

    /// Started but no window yet (Rare OS game mode only; elsewhere nothing is known).
    pub fn starting(&self, id: &str) -> bool {
        windows_known() && self.phase(id) != "playing" && self.phase(id) != "none"
    }

    pub fn is_any_running(&self) -> bool {
        !self.inner.lock().unwrap().is_empty()
    }

    /// Steam's "Switch to Desktop" in Rare OS game mode (`SESSION_SELECT`): the launcher takes the
    /// screen and Steam's UI leaves the Running list; Steam keeps running for games.
    fn back_to_rare(&self) {
        let Some(f) = std::env::var_os("XDG_RUNTIME_DIR").map(|d| std::path::PathBuf::from(d).join(BACK_TO_RARE)) else { return };
        if std::fs::remove_file(&f).is_err() {
            return;
        }
        eprintln!("[launch] Steam's Switch to Desktop: back to Rare");
        self.untrack_steam_ui();
        crate::apps::front::to_launcher();
    }

    /// Steam's UI leaves the Running list (Steam itself keeps running).
    pub fn untrack_steam_ui(&self) {
        self.inner.lock().unwrap().retain(|_, h| !matches!(h, Handle::SteamUi { .. }));
    }

    /// Reads Steam's log for questions it asks the player before a game starts (steamlog.rs) and
    /// tells apps/front.rs, which hands Steam the screen for them.
    fn steam_questions(&self) {
        let steam_runs: Vec<(String, Vec<u64>)> =
            self.inner.lock().unwrap().iter().filter_map(|(id, h)| if let Handle::Steam { ids, .. } = h { Some((id.clone(), ids.clone())) } else { None }).collect();
        if steam_runs.is_empty() {
            self.asks.lock().unwrap().clear();
            return;
        }
        crate::steamlog::poll();
        let mut asks = self.asks.lock().unwrap();
        for (id, ids) in steam_runs {
            let q = ids.iter().find_map(|a| crate::steamlog::question(*a));
            match q {
                Some(q) => {
                    if asks.insert(id.clone(), q).is_none() {
                        eprintln!("[launch] {id}: {q}");
                    }
                }
                None => {
                    if asks.remove(&id).is_some() {
                        eprintln!("[launch] {id}: answered in Steam");
                    }
                }
            }
            crate::apps::front::steam_ask(&id, q.is_some());
        }
    }

    /// Polls children and Steam reapers; emits `game-state` when the set changes.
    pub fn watch(&self, ctx: Ctx) {
        let me = self.clone();
        std::thread::spawn(move || {
            let mut last: Vec<String> = Vec::new();
            loop {
                std::thread::sleep(Duration::from_millis(1500));
                let mut failed: Vec<LaunchFailed> = Vec::new();
                me.back_to_rare();
                me.steam_questions();
                {
                    #[cfg(target_os = "linux")]
                    let procs = rare_core::proc::cmdlines();
                    #[cfg(not(target_os = "linux"))]
                    let procs: Vec<(u32, String, String)> = Vec::new();
                    let mut map = me.inner.lock().unwrap();
                    // A start with no window after NO_WINDOW: said once, while it still runs.
                    if windows_known() {
                        let shown = me.shown.lock().unwrap();
                        let mut waiting = me.waiting.lock().unwrap();
                        let ready = steam_ready();
                        for (id, h) in map.iter() {
                            if shown.contains_key(id) || matches!(h, Handle::Window { .. }) {
                                continue;
                            }
                            let steam_wait = matches!(h, Handle::Steam { .. } | Handle::SteamUi { .. });
                            if steam_wait && !ready && !waiting.contains_key(id) {
                                continue;
                            }
                            // Steam waiting for the player is not stuck: the wait starts again after.
                            if me.asks.lock().unwrap().contains_key(id) {
                                waiting.remove(id);
                                continue;
                            }
                            // A Steam game's reaper runs but gamescope lists no window under its
                            // id: Steam's choices get the screen (the game may show under another).
                            if let Handle::Steam { seen_at: Some(t), .. } = h {
                                if t.elapsed() >= REAPER_SHOW {
                                    crate::apps::front::steam_shown(id);
                                }
                            }
                            let w = waiting.entry(id.clone()).or_insert((Instant::now(), false));
                            if !w.1 && w.0.elapsed() >= NO_WINDOW {
                                w.1 = true;
                                // Nothing to show after all this time: let Steam show what it has.
                                crate::apps::front::steam_shown(id);
                                failed.push(LaunchFailed { id: id.clone(), reason: "stuck", code: None });
                            }
                        }
                    }
                    let asks = me.asks.lock().unwrap().clone();
                    map.retain(|id, h| {
                        let keep = alive(&ctx, h, &procs, asks.contains_key(id));
                        if !keep {
                            let (kind, seen, alive_for, code) = match h {
                                Handle::Child { started, exit, .. } => ("child", true, started.elapsed(), *exit),
                                Handle::Steam { seen, .. } => ("steam", *seen, Duration::ZERO, None),
                                Handle::Dir { seen, .. } => ("dir", *seen, Duration::ZERO, None),
                                Handle::SteamUi { seen, .. } => ("steamui", *seen, Duration::ZERO, None),
                                Handle::Window { .. } => ("window", true, Duration::ZERO, None),
                            };
                            let quit = me.quitting.lock().unwrap().remove(id.as_str());
                            let shown_for = me.shown.lock().unwrap().remove(id.as_str()).map(|t| t.elapsed());
                            let stuck_said = me.waiting.lock().unwrap().remove(id.as_str()).is_some_and(|(_, said)| said);
                            me.asks.lock().unwrap().remove(id.as_str());
                            crate::apps::front::steam_ask(id, false);
                            if let Some((reason, code)) = failed_start(kind, seen, alive_for, code, shown_for, windows_known()).filter(|_| !quit && !stuck_said) {
                                failed.push(LaunchFailed { id: id.clone(), reason, code });
                            }
                        }
                        keep
                    });
                }
                for f in failed {
                    eprintln!("[launch] {}: {}{}", f.id, f.reason, f.code.map(|c| format!(", exit code {c}")).unwrap_or_default());
                    ctx.send(Event::LaunchFailed(f));
                }
                let mut now = me.running();
                now.sort();
                if now != last {
                    // Power follows: the next game's settings, or the launcher's own.
                    ctx.host.power_running(&now);
                    ctx.send(Event::GameState(RunningState { running: now.clone() }));
                    last = now;
                }
            }
        });
    }
}

/// Whether a tracked run is still going; updates what it learns on the way (seen, exit code).
/// `procs` is the process list (empty outside Linux).
/// `asked`: Steam waits for the player before it starts this game (the run stays while it does).
fn alive(ctx: &Ctx, h: &mut Handle, procs: &[(u32, String, String)], asked: bool) -> bool {
    match h {
        Handle::Window { label } => ctx.host.window_open(label),
        Handle::Child { child, unit, cgroup, exit, .. } => {
            let alive = match child.try_wait() {
                Ok(None) => true,
                Ok(Some(st)) => {
                    exit.get_or_insert(st.code().unwrap_or(-1));
                    false
                }
                Err(_) => false,
            };
            #[cfg(target_os = "linux")]
            {
                // Learn the scope's cgroup while the first process is alive.
                if cgroup.is_none() && alive {
                    if let (Some(u), Some(cg)) = (unit.as_ref(), cgroup_of(child.id())) {
                        if cg.to_string_lossy().contains(u.as_str()) {
                            *cgroup = Some(cg);
                        }
                    }
                }
                alive || cgroup.as_deref().is_some_and(cgroup_busy)
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (unit, cgroup);
                alive
            }
        }
        Handle::Dir { dir, seen, started, grace } => {
            let alive = procs.iter().any(|(_, _, cmd)| cmd.contains(dir.as_str()));
            if alive {
                *seen = true;
            }
            alive || (!*seen && started.elapsed() < *grace)
        }
        Handle::Steam { ids, seen, seen_at, started, grace } => {
            let alive = if cfg!(target_os = "linux") {
                let needles: Vec<String> = ids.iter().map(|i| format!("AppId={i}")).collect();
                procs.iter().any(|(_, _, cmd)| game_reaper(cmd, &needles))
            } else {
                started.elapsed() < Duration::from_secs(20)
            };
            if alive {
                *seen = true;
                seen_at.get_or_insert_with(Instant::now);
            }
            alive || (!*seen && (asked || started.elapsed() < *grace))
        }
        Handle::SteamUi { seen, started, grace } => {
            let alive = if cfg!(target_os = "linux") { procs.iter().any(|(_, comm, _)| comm == "steam" || comm == "steamwebhelper") } else { steam_running() };
            if alive {
                *seen = true;
            }
            alive || (!*seen && started.elapsed() < *grace)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_starts_and_crashes() {
        let s = Duration::from_secs;
        let f = failed_start;
        // Steam never ran the game, Heroic never started it, Steam itself never came up.
        assert_eq!(f("steam", false, s(0), None, None, true), Some(("steam", None)));
        assert_eq!(f("dir", false, s(0), None, None, false), Some(("never", None)));
        assert_eq!(f("steamui", false, s(0), None, None, true), Some(("steamui", None)));
        // Steam closing, a web game window closing: the player's doing.
        assert_eq!(f("steamui", true, s(9), None, Some(s(1)), true), None);
        assert_eq!(f("window", true, s(1), None, None, true), None);
        // Game mode: gone before any window is a start that failed, with its code.
        assert_eq!(f("child", true, s(2), Some(1), None, true), Some(("quick", Some(1))));
        assert_eq!(f("child", true, s(30), None, None, true), Some(("quick", None)));
        assert_eq!(f("steam", true, s(20), None, None, true), Some(("quick", None)));
        // ...unless it handed off at once with exit 0.
        assert_eq!(f("child", true, s(1), Some(0), None, true), None);
        // A window, then gone within 10 s: a crash. A non-zero exit later: a crash too.
        assert_eq!(f("steam", true, s(30), None, Some(s(3)), true), Some(("crash", None)));
        assert_eq!(f("child", true, s(900), Some(-1), Some(s(880)), true), Some(("crash", Some(-1))));
        // A long run that ends cleanly: a quit.
        assert_eq!(f("child", true, s(900), Some(0), Some(s(880)), true), None);
        assert_eq!(f("steam", true, s(900), None, Some(s(880)), true), None);
        // Outside game mode (no window information): quick errors and error exits only.
        assert_eq!(f("child", true, s(1), Some(1), None, false), Some(("quick", Some(1))));
        assert_eq!(f("child", true, s(1), Some(0), None, false), None);
        assert_eq!(f("child", true, s(600), Some(2), None, false), Some(("crash", Some(2))));
        assert_eq!(f("child", true, s(600), Some(0), None, false), None);
        assert_eq!(f("steam", true, s(600), None, None, false), None);
    }

    #[test]
    fn install_script_reaper_is_not_the_game() {
        let n = vec!["AppId=1145350".to_string()];
        // The device, 23:21:28: Steam ran Hades II's install script before the cloud conflict.
        assert!(!game_reaper("/s/steam-launch-wrapper -- /s/reaper SteamLaunch AppId=1145350 Install=1 -- /s/proton run iscriptevaluator.exe", &n));
        assert!(game_reaper("/s/steam-launch-wrapper -- /s/reaper SteamLaunch AppId=1145350 -- /s/proton waitforexitandrun Hades2.exe", &n));
        assert!(!game_reaper("/s/reaper SteamLaunch AppId=11453500 -- x", &n));
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn windows_come_from_gamescopes_focusable_apps() {
        let apps = vec![("steam:620".to_string(), vec![620]), ("app:steam".to_string(), vec![STEAM_UI_APPID]), ("emu:x".to_string(), vec![0x8123_4567]), ("web:act".to_string(), vec![])];
        // The device read: GAMESCOPE_FOCUSABLE_APPS = 1380012613 (only Rare).
        assert!(shown_runs(&apps, &[1380012613]).is_empty());
        let mut got = shown_runs(&apps, &[1380012613, 620, 769]);
        got.sort();
        assert_eq!(got, vec!["app:steam".to_string(), "steam:620".to_string()]);
        assert_eq!(shown_runs(&apps, &[0x8123_4567]), vec!["emu:x".to_string()]);
    }

    #[test]
    fn system_webkit_env_is_undone() {
        let e = vec![
            ("RARE_SYSTEM_WEBKIT".to_string(), "1".to_string()),
            ("RARE_ORIG_LD_LIBRARY_PATH".to_string(), "".to_string()),
            ("LD_LIBRARY_PATH".to_string(), "/usr/lib64".to_string()),
            ("HOME".to_string(), "/home/x".to_string()),
        ];
        let out = clean_env(e);
        assert!(out.iter().all(|(k, _)| k != "LD_LIBRARY_PATH" && !k.starts_with("RARE_")));
        assert!(out.iter().any(|(k, _)| k == "HOME"));
    }

    #[test]
    fn steam_overlay_preload_comes_back_for_games() {
        let e = env(&[("RARE_ORIG_LD_PRELOAD", "/s/ubuntu12_32/gameoverlayrenderer.so:/s/ubuntu12_64/gameoverlayrenderer.so"), ("HOME", "/home/x")]);
        let out = clean_env(e);
        assert!(out.iter().all(|(k, _)| k != "RARE_ORIG_LD_PRELOAD"));
        assert_eq!(out.iter().find(|(k, _)| k == "LD_PRELOAD").map(|(_, v)| v.as_str()), Some("/s/ubuntu12_32/gameoverlayrenderer.so:/s/ubuntu12_64/gameoverlayrenderer.so"));
    }

    #[test]
    fn session_client_mark_stays_with_the_launcher() {
        let e = env(&[("RARE_SESSION_CLIENT", "1"), ("HOME", "/home/x")]);
        assert_eq!(clean_env(e), env(&[("HOME", "/home/x")]));
    }

    #[test]
    fn launcher_render_env_stays_with_the_launcher() {
        let e = env(&[
            ("GDK_BACKEND", "wayland"),
            ("WAYLAND_DISPLAY", "gamescope-0"),
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "0"),
            ("RARE_WEBKIT", "wayland"),
            ("RARE_WEBKIT_SCOPED", "1"),
            ("RARE_WEBKIT_SET", "WEBKIT_DISABLE_DMABUF_RENDERER,GDK_BACKEND,WAYLAND_DISPLAY"),
            ("DISPLAY", ":0"),
            ("HOME", "/home/x"),
        ]);
        assert_eq!(clean_env(e), env(&[("DISPLAY", ":0"), ("HOME", "/home/x")]));
        // A WAYLAND_DISPLAY the launcher did not set (desktop mode) is the user's and stays.
        let e = env(&[("WAYLAND_DISPLAY", "wayland-0"), ("HOME", "/home/x")]);
        assert_eq!(clean_env(e.clone()), e);
    }

    #[test]
    fn unchanged_without_appdir() {
        let e = env(&[("PATH", "/usr/bin"), ("PYTHONHOME", "/x")]);
        assert_eq!(clean_env(e.clone()), e);
    }

    #[test]
    fn strips_appimage_env() {
        let d = "/tmp/.mount_Rare-abc";
        let e = env(&[
            ("APPDIR", "/tmp/.mount_Rare-abc/"),
            ("APPIMAGE", "/home/b/Rare.AppImage"),
            ("HOME", "/home/b"),
            ("PATH", &format!("{d}/usr/bin:/usr/local/bin:/usr/bin")),
            ("LD_LIBRARY_PATH", &format!("{d}/usr/lib:{d}/usr/lib64")),
            ("XDG_DATA_DIRS", &format!("{d}/usr/share:/usr/share")),
            ("PYTHONHOME", &format!("{d}/usr")),
            ("PYTHONPATH", "/whatever"),
            ("GTK_PATH", &format!("{d}/usr/lib/gtk-3.0")),
            ("GTK_THEME", "Adwaita:dark"),
            ("GST_PLUGIN_SYSTEM_PATH", "/usr/lib/gstreamer-1.0"),
            ("GSETTINGS_SCHEMA_DIR", &format!("{d}/usr/share/glib-2.0/schemas")),
            ("QT_PLUGIN_PATH", "/q"),
            ("RARE_FULLSCREEN", "1"),
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("SOME_FILE", &format!("{d}/etc/thing")),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        let out = clean_env(e);
        let get = |k: &str| out.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
        assert_eq!(get("PATH"), Some("/usr/local/bin:/usr/bin"));
        assert_eq!(get("XDG_DATA_DIRS"), Some("/usr/share"));
        assert_eq!(get("GTK_THEME"), Some("Adwaita:dark"));
        assert_eq!(get("HOME"), Some("/home/b"));
        assert_eq!(get("XDG_RUNTIME_DIR"), Some("/run/user/1000"));
        for gone in [
            "APPDIR",
            "APPIMAGE",
            "LD_LIBRARY_PATH",
            "PYTHONHOME",
            "PYTHONPATH",
            "GTK_PATH",
            "GST_PLUGIN_SYSTEM_PATH",
            "GSETTINGS_SCHEMA_DIR",
            "QT_PLUGIN_PATH",
            "RARE_FULLSCREEN",
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "SOME_FILE",
        ] {
            assert_eq!(get(gone), None, "{gone} should be removed");
        }
    }
}
