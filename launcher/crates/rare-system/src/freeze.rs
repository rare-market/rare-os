//! Quick resume: the game you switch away from is frozen, and comes back the instant you return.
//! A frozen game uses no CPU and no GPU time, so it costs what an idle device costs, and its
//! memory stays where it was, so resuming is immediate.
//!
//! How a game is frozen, in order of preference (`plan`):
//! - **The cgroup v2 freezer on its own scope.** Everything Rare starts (but Steam) runs in a
//!   transient `systemd-run --user --scope` unit (launch.rs), in game mode named
//!   `app-steam-app<id>-<n>.scope` so gamescope gives its windows an app id (overlay/appid.rs).
//!   Freezing the whole scope stops every process in it at once, including a Proton game's
//!   wineserver and the Flatpak scope a Flatpak app moves itself into. `systemctl --user freeze`
//!   keeps systemd's own view right; writing `cgroup.freeze` is the fallback (the user manager's
//!   subtree is delegated, so no privileges are needed).
//! - **Steam games: the reaper's scope, when Steam made one.** Steam starts a game under its
//!   reaper (`reaper SteamLaunch AppId=<id> -- ...`). When that reaper sits in a scope named
//!   `app-steam-app<id>-<n>.scope` for the same id (gamescope reads that name too), the scope is
//!   the game's alone and is frozen. When it sits in Steam's own cgroup (or anywhere a Steam
//!   process lives), only the reaper's descendants are stopped with SIGSTOP; the reaper itself
//!   and Steam are never touched. Proton's whole tree (pressure-vessel, wine, wineserver) is
//!   under the reaper, so it is stopped together.
//! - **SIGSTOP of the process tree** for anything else (Heroic games, programs without a scope).
//!
//! What is never frozen: Steam itself or its UI, a stream (the game runs on the PC; freezing
//! Moonlight drops the session), and web games in Rare's own window (that WebKit process is
//! Rare's, it carries wallet approvals, and every Sui web game holds a live session). The per-game
//! switch "Keep running in the background" (`decide`) covers the rest, with these defaults:
//!
//! | What | Default | Why |
//! |---|---|---|
//! | Steam, native, Proton, Heroic, itch, GOG, Rare Store games, emulators | Paused | Mostly single player; a paused game costs nothing and resumes at once |
//! | A game the UI marks online (a Sui game on rare.market, or a Steam game with anti-cheat) | Keeps running | A network session times out while frozen (servers drop a silent client in 30 to 60 s), and anti-cheat heartbeats kick a stalled client |
//! | Apps (Telegram, Discord, X, YouTube, Netflix, music) | Keeps running | Messaging and calls are network sessions; music and video are what you listen to while doing something else, so freezing them is the audio-critical case |
//! | Web games in Chrome | Keeps running | Live server and chain sessions |
//!
//! At most `MAX_FROZEN` games are frozen; leaving a third asks which one to close (the UI's
//! "Close a paused game" sheet), and until then the third keeps running.
//!
//! Safety: what is frozen is written to `$XDG_RUNTIME_DIR/rare-frozen.json`, so a launcher that
//! crashed or restarted thaws it at start (`thaw_stale`), and the launcher thaws everything when
//! it exits. Quitting a frozen game thaws it first (a frozen process can't handle SIGTERM, so it
//! would never get to save). Audio needs nothing: a frozen client's PipeWire stream underruns
//! into silence and picks up again on thaw.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// The most games frozen at once.
pub const MAX_FROZEN: usize = 2;

/// What the runner knows about a running game (launch.rs `Runner::freeze_target`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A program Rare started, with its systemd scope when it has one.
    Child { pid: u32, unit: Option<String> },
    /// Started through Steam; found by its reaper's `AppId=<id>`.
    Steam { ids: Vec<u64> },
    /// Started through another launcher (Heroic): processes with `dir` in their command line.
    Dir { dir: String },
    /// A web game in Rare's own window.
    Window,
    /// Steam's own UI (Big Picture), opened as an app: Steam itself, never frozen.
    SteamUi,
}

/// How a game is frozen and thawed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "how", rename_all = "lowercase")]
pub enum Plan {
    /// The cgroup v2 freezer on these scopes (cgroup paths, as in /proc/<pid>/cgroup).
    Cgroup { scopes: Vec<String> },
    /// SIGSTOP and SIGCONT to these processes.
    Signal { pids: Vec<u32> },
}

/// One process, as `/proc` shows it.
#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub cmdline: String,
    /// The unified cgroup path ("/user.slice/.../app.slice/x.scope").
    pub cgroup: String,
}

/// Steam's own processes: never frozen, and a cgroup holding one is never frozen either.
pub fn is_steam_proc(comm: &str) -> bool {
    matches!(comm, "steam" | "steamwebhelper" | "steam-runtime-l" | "steam-runtime-launcher-service" | "srt-logger" | "gamescope" | "gamescope-wl" | "Xwayland")
}

fn last(cgroup: &str) -> &str {
    cgroup.rsplit('/').next().unwrap_or(cgroup)
}

/// The app id in a Steam-style scope name (`app-steam-app<id>-<n>.scope`), as gamescope reads it.
pub fn steam_scope_id(cgroup: &str) -> Option<u64> {
    let rest = last(cgroup).strip_prefix("app-steam-app")?.strip_suffix(".scope")?;
    let (id, n) = rest.split_once('-')?;
    n.parse::<i64>().ok()?;
    id.parse::<u64>().ok().filter(|v| *v != 0)
}

/// `roots` and everything below them, sorted.
pub fn descendants(procs: &[Proc], roots: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = roots.iter().copied().filter(|r| procs.iter().any(|p| p.pid == *r)).collect();
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        for c in procs.iter().filter(|x| x.ppid == p && x.pid != p) {
            if !out.contains(&c.pid) {
                out.push(c.pid);
            }
        }
        i += 1;
    }
    out.sort();
    out
}

/// A scope that is safe to freeze whole: a `.scope`, holding no Steam process and not the
/// launcher (`me`) or anything above it.
fn scope_ok(procs: &[Proc], cgroup: &str, me: &[u32]) -> bool {
    cgroup.ends_with(".scope")
        && procs.iter().filter(|p| p.cgroup == cgroup).all(|p| !is_steam_proc(&p.comm) && !me.contains(&p.pid))
        && procs.iter().any(|p| p.cgroup == cgroup)
}

/// Why a game can't be frozen right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refuse {
    /// Rare's own window, or nothing to freeze.
    NotFreezable,
    /// Its processes aren't there (Steam still starting, or it ended).
    NotFound,
}

/// How to freeze `target`, given every process (`procs`) and the launcher's own pid and its
/// ancestors (`me`), which are never frozen.
pub fn plan(target: &Target, procs: &[Proc], me: &[u32]) -> Result<Plan, Refuse> {
    let signal = |pids: Vec<u32>| -> Result<Plan, Refuse> {
        let pids: Vec<u32> = pids.into_iter().filter(|p| !me.contains(p) && procs.iter().any(|x| x.pid == *p && !is_steam_proc(&x.comm))).collect();
        if pids.is_empty() {
            Err(Refuse::NotFound)
        } else {
            Ok(Plan::Signal { pids })
        }
    };
    match target {
        Target::Window | Target::SteamUi => Err(Refuse::NotFreezable),
        Target::Steam { ids } => {
            let reapers: Vec<&Proc> = procs.iter().filter(|p| p.cmdline.split(' ').any(|t| ids.iter().any(|i| t == format!("AppId={i}")))).collect();
            if reapers.is_empty() {
                return Err(Refuse::NotFound);
            }
            let mut scopes: Vec<String> = Vec::new();
            for r in &reapers {
                let mine = steam_scope_id(&r.cgroup).is_some_and(|id| ids.contains(&id));
                if mine && scope_ok(procs, &r.cgroup, me) && !scopes.contains(&r.cgroup) {
                    scopes.push(r.cgroup.clone());
                }
            }
            if scopes.len() == reapers.len() {
                return Ok(Plan::Cgroup { scopes });
            }
            // In Steam's cgroup: the reaper's children down, never the reaper or Steam.
            let roots: Vec<u32> = reapers.iter().map(|r| r.pid).collect();
            signal(descendants(procs, &roots).into_iter().filter(|p| !roots.contains(p)).collect())
        }
        Target::Child { pid, unit } => {
            let tree = descendants(procs, &[*pid]);
            let mut scopes: Vec<String> = Vec::new();
            if let Some(u) = unit {
                let want = format!("{u}.scope");
                if let Some(cg) = procs.iter().find(|p| last(&p.cgroup) == want).map(|p| p.cgroup.clone()) {
                    if scope_ok(procs, &cg, me) {
                        scopes.push(cg);
                    }
                }
            }
            if !scopes.is_empty() {
                // A Flatpak moves itself into a scope of its own: freeze that one with it.
                let home = scopes[0].clone();
                for p in procs.iter().filter(|p| tree.contains(&p.pid) || p.cgroup == home) {
                    let cg = &p.cgroup;
                    if last(cg).starts_with("app-flatpak-") && !scopes.contains(cg) && scope_ok(procs, cg, me) {
                        scopes.push(cg.clone());
                    }
                }
                return Ok(Plan::Cgroup { scopes });
            }
            signal(tree)
        }
        Target::Dir { dir } => {
            let roots: Vec<u32> = procs.iter().filter(|p| p.cmdline.contains(dir.as_str())).map(|p| p.pid).collect();
            signal(descendants(procs, &roots))
        }
    }
}

// ---------- the per-game decision ----------

/// What kind of thing a run id is, for the defaults.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Game,
    /// A web game in Rare's own window.
    WebWindow,
    /// A web game Rare opened in Chrome.
    WebChrome,
    /// An app from the apps catalog (app:<id>).
    App,
    /// A game streamed from a PC (stream:<pc>:<app>).
    Stream,
    /// Steam's own UI.
    Steam,
}

pub fn kind(run: &str, target: Option<&Target>) -> Kind {
    if matches!(target, Some(Target::SteamUi)) || run == "app:steam" {
        Kind::Steam
    } else if run.starts_with("stream:") {
        Kind::Stream
    } else if run.starts_with("app:") {
        Kind::App
    } else if run.starts_with("web:") {
        if matches!(target, Some(Target::Window) | None) {
            Kind::WebWindow
        } else {
            Kind::WebChrome
        }
    } else {
        Kind::Game
    }
}

/// Why a game pauses or keeps running.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Why {
    /// The default for games: paused.
    Default,
    /// The UI marked it online (a Sui game, anti-cheat).
    Online,
    /// An app: messaging, calls, music or video.
    App,
    /// A web game.
    WebGame,
    /// A stream from a PC.
    Stream,
    /// Steam itself.
    Steam,
    /// The player set it on the game page.
    You,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// Frozen when you switch away.
    pub pause: bool,
    /// The game page offers "Keep running in the background".
    pub can_change: bool,
    pub why: Why,
}

/// The decision for one game: `online` from the UI's note, `keep` the player's own switch
/// ("Keep running in the background": Some(true) keeps it running, Some(false) pauses it).
pub fn decide(kind: Kind, online: bool, keep: Option<bool>) -> Decision {
    let fixed = |why| Decision { pause: false, can_change: false, why };
    let default = match kind {
        Kind::Stream => return fixed(Why::Stream),
        Kind::Steam => return fixed(Why::Steam),
        Kind::WebWindow => return fixed(Why::WebGame),
        Kind::WebChrome => Decision { pause: false, can_change: true, why: Why::WebGame },
        Kind::App => Decision { pause: false, can_change: true, why: Why::App },
        Kind::Game if online => Decision { pause: false, can_change: true, why: Why::Online },
        Kind::Game => Decision { pause: true, can_change: true, why: Why::Default },
    };
    match keep {
        Some(k) => Decision { pause: !k, can_change: true, why: Why::You },
        None => default,
    }
}

// ---------- the frozen set ----------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Frozen {
    pub run: String,
    pub plan: Plan,
    /// Unix ms.
    pub at: u64,
}

/// What `Set::admit` says about freezing one more game.
#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    Yes,
    /// Frozen already.
    Already,
    /// Full: these are frozen, the player picks one to close.
    Full(Vec<String>),
}

/// The frozen games, oldest first.
#[derive(Default, Debug, Clone)]
pub struct Set {
    pub list: Vec<Frozen>,
}

impl Set {
    pub fn admit(&self, run: &str) -> Admit {
        if self.is_frozen(run) {
            Admit::Already
        } else if self.list.len() >= MAX_FROZEN {
            Admit::Full(self.list.iter().map(|f| f.run.clone()).collect())
        } else {
            Admit::Yes
        }
    }
    pub fn is_frozen(&self, run: &str) -> bool {
        self.list.iter().any(|f| f.run == run)
    }
    pub fn add(&mut self, f: Frozen) {
        self.list.retain(|x| x.run != f.run);
        self.list.push(f);
    }
    pub fn take(&mut self, run: &str) -> Option<Frozen> {
        let i = self.list.iter().position(|f| f.run == run)?;
        Some(self.list.remove(i))
    }
    /// Drops games that are no longer running; returns them.
    pub fn retain_running(&mut self, running: &[String]) -> Vec<Frozen> {
        let (keep, gone): (Vec<Frozen>, Vec<Frozen>) = self.list.drain(..).partition(|f| running.contains(&f.run));
        self.list = keep;
        gone
    }
    pub fn runs(&self) -> Vec<String> {
        self.list.iter().map(|f| f.run.clone()).collect()
    }
}

struct State {
    set: Set,
    /// The player's "Keep running in the background" switches, loaded once.
    keep: Option<BTreeMap<String, bool>>,
    /// Games the UI marked online.
    online: HashMap<String, bool>,
    /// A game that would have been frozen when the set was full, waiting for the player to close one.
    pending: Option<String>,
}

static STATE: std::sync::OnceLock<Mutex<State>> = std::sync::OnceLock::new();

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.get_or_init(|| Mutex::new(State { set: Set::default(), keep: None, online: HashMap::new(), pending: None })).lock().unwrap_or_else(|e| e.into_inner())
}

fn keep_file() -> std::path::PathBuf {
    dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("rare/background.json")
}

fn keep_map(s: &mut State) -> &mut BTreeMap<String, bool> {
    s.keep.get_or_insert_with(|| std::fs::read_to_string(keep_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
}

fn runtime_file() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| std::path::PathBuf::from(d).join("rare-frozen.json"))
}

fn persist(set: &Set) {
    let Some(f) = runtime_file() else { return };
    if set.list.is_empty() {
        let _ = std::fs::remove_file(f);
    } else if let Ok(t) = serde_json::to_string(&set.list) {
        let _ = std::fs::write(f, t);
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn is_frozen(run: &str) -> bool {
    state().set.is_frozen(run)
}

pub fn frozen() -> Vec<String> {
    state().set.runs()
}

/// The decision for `run` now (its kind, the UI's note, the player's switch).
pub fn decision(run: &str, target: Option<&Target>) -> Decision {
    let mut s = state();
    let online = s.online.get(run).copied().unwrap_or(false);
    let keep = keep_map(&mut s).get(run).copied();
    decide(kind(run, target), online, keep)
}

/// What happened when Rare tried to freeze a game.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum Outcome {
    Frozen,
    /// It keeps running (its decision says so, or there is nothing to freeze).
    Running { why: String },
    /// Two are frozen already: the player picks one to close; this one keeps running until then.
    Full { pending: String, frozen: Vec<String> },
    Failed { error: String },
}

/// Freezes `run` if its decision says so and there is room.
pub fn freeze(run: &str, target: Option<Target>) -> Outcome {
    let d = decision(run, target.as_ref());
    if !d.pause {
        return Outcome::Running { why: format!("{:?}", d.why) };
    }
    let Some(target) = target else { return Outcome::Running { why: "not running".into() } };
    match state().set.admit(run) {
        Admit::Already => return Outcome::Frozen,
        Admit::Full(frozen) => {
            state().pending = Some(run.to_string());
            return Outcome::Full { pending: run.to_string(), frozen };
        }
        Admit::Yes => {}
    }
    let procs = snapshot();
    let me = own_lineage(&procs);
    let plan = match plan(&target, &procs, &me) {
        Ok(p) => p,
        Err(e) => return Outcome::Running { why: format!("{e:?}") },
    };
    if let Err(e) = apply(&plan, true) {
        // Half frozen is worse than running: put back whatever did stop.
        let _ = apply(&plan, false);
        return Outcome::Failed { error: e };
    }
    let mut s = state();
    s.set.add(Frozen { run: run.to_string(), plan: plan.clone(), at: now_ms() });
    persist(&s.set);
    crate::host::log(&format!("[freeze] {run} frozen ({plan:?})"));
    Outcome::Frozen
}

/// Thaws `run` if it is frozen. Fast when it isn't (a lookup). Called before anything puts a game
/// back in front (apps::front::to_front), and before a quit.
pub fn thaw(run: &str) {
    let f = {
        let mut s = state();
        let f = s.set.take(run);
        if f.is_some() {
            persist(&s.set);
        }
        f
    };
    if let Some(f) = f {
        match apply(&f.plan, false) {
            Ok(()) => crate::host::log(&format!("[freeze] {run} thawed after {} ms", now_ms().saturating_sub(f.at))),
            Err(e) => crate::host::log(&format!("[freeze] thawing {run}: {e}")),
        }
    }
}

/// Everything frozen goes (the launcher is exiting).
pub fn thaw_all() {
    for r in frozen() {
        thaw(&r);
    }
}

/// At start: games a previous launcher run froze (it crashed or restarted) are thawed.
pub fn thaw_stale() {
    let Some(f) = runtime_file() else { return };
    let Ok(t) = std::fs::read_to_string(&f) else { return };
    let list: Vec<Frozen> = serde_json::from_str(&t).unwrap_or_default();
    for fr in list {
        crate::host::log(&format!("[freeze] thawing {} left frozen by a previous run", fr.run));
        let _ = apply(&fr.plan, false);
    }
    let _ = std::fs::remove_file(f);
}

/// Forgets games that ended; returns the pending one when it can be frozen now (room was made).
pub fn sync(running: &[String]) -> Option<String> {
    let mut s = state();
    let gone = s.set.retain_running(running);
    if !gone.is_empty() {
        persist(&s.set);
    }
    if s.pending.as_ref().is_some_and(|p| !running.contains(p)) {
        s.pending = None;
    }
    if s.pending.is_some() && s.set.list.len() < MAX_FROZEN {
        return s.pending.take();
    }
    None
}

/// The player kept the pending game running instead of closing a paused one.
pub fn drop_pending() {
    state().pending = None;
}

pub fn note_online(run: &str, online: bool) {
    state().online.insert(run.to_string(), online);
}

/// The player's switch: Some(true) keeps `run` running in the background, Some(false) pauses it,
/// None goes back to the default.
pub fn set_keep(run: &str, keep: Option<bool>) -> Result<(), String> {
    let mut s = state();
    let m = keep_map(&mut s);
    match keep {
        Some(k) => {
            m.insert(run.to_string(), k);
        }
        None => {
            m.remove(run);
        }
    }
    let text = serde_json::to_string_pretty(m).map_err(|e| e.to_string())?;
    let f = keep_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(f, text).map_err(|e| e.to_string())
}

// ---------- the system side ----------

/// Every process, from /proc (Linux); empty elsewhere.
#[cfg(target_os = "linux")]
pub fn snapshot() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return out };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let stat = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
        let Some((ppid, comm)) = parse_stat(&stat) else { continue };
        let cmdline = std::fs::read(e.path().join("cmdline")).map(|b| String::from_utf8_lossy(&b).replace('\0', " ").trim_end().to_string()).unwrap_or_default();
        let cgroup = std::fs::read_to_string(e.path().join("cgroup")).ok().and_then(|t| parse_cgroup(&t)).unwrap_or_default();
        out.push(Proc { pid, ppid, comm, cmdline, cgroup });
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn snapshot() -> Vec<Proc> {
    Vec::new()
}

/// "pid (comm) S ppid ..." to (ppid, comm); comm may hold spaces and parentheses.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_stat(stat: &str) -> Option<(u32, String)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_string();
    let ppid = stat.get(close + 1..)?.split_whitespace().nth(1)?.parse().ok()?;
    Some((ppid, comm))
}

/// The unified (cgroup v2) path in /proc/<pid>/cgroup.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_cgroup(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix("0::")).map(|p| p.trim().to_string())
}

/// The launcher's pid and every ancestor: never frozen.
fn own_lineage(procs: &[Proc]) -> Vec<u32> {
    let mut out = vec![std::process::id()];
    let mut cur = std::process::id();
    for _ in 0..64 {
        let Some(p) = procs.iter().find(|p| p.pid == cur) else { break };
        if p.ppid == 0 || out.contains(&p.ppid) {
            break;
        }
        out.push(p.ppid);
        cur = p.ppid;
    }
    out
}

#[cfg(target_os = "linux")]
fn apply(plan: &Plan, freeze: bool) -> Result<(), String> {
    use std::process::{Command, Stdio};
    match plan {
        Plan::Cgroup { scopes } => {
            let mut err = None;
            for cg in scopes {
                let unit = last(cg).to_string();
                let file = std::path::Path::new("/sys/fs/cgroup").join(cg.trim_start_matches('/')).join("cgroup.freeze");
                let systemctl = |verb: &str| Command::new("systemctl").args(["--user", verb, &unit]).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
                if freeze {
                    // systemd's own freeze first, so it knows; the file when it can't.
                    if !systemctl("freeze") {
                        if let Err(e) = std::fs::write(&file, "1") {
                            err = Some(format!("{unit}: {e}"));
                        }
                    }
                } else {
                    // The file first: a thaw has to be instant. systemd catches up behind it.
                    match std::fs::write(&file, "0") {
                        Ok(()) => {
                            let u = unit.clone();
                            std::thread::spawn(move || {
                                let _ = Command::new("systemctl").args(["--user", "thaw", &u]).stdout(Stdio::null()).stderr(Stdio::null()).status();
                            });
                        }
                        Err(e) => {
                            if !systemctl("thaw") {
                                err = Some(format!("{unit}: {e}"));
                            }
                        }
                    }
                }
            }
            err.map_or(Ok(()), Err)
        }
        Plan::Signal { pids } => {
            let sig = if freeze { libc::SIGSTOP } else { libc::SIGCONT };
            let mut failed = 0;
            for p in pids {
                // SAFETY: kill(2) with a plain pid and signal number.
                if unsafe { libc::kill(*p as libc::pid_t, sig) } != 0 {
                    failed += 1;
                }
            }
            if freeze && failed == pids.len() {
                return Err("no process could be stopped".into());
            }
            Ok(())
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn apply(_plan: &Plan, _freeze: bool) -> Result<(), String> {
    Ok(())
}

// ---------- commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    pub run: String,
    pub kind: Kind,
    pub pause: bool,
    pub can_change: bool,
    pub why: Why,
    pub frozen: bool,
}

/// The game page's switch: how `id` behaves when you switch away from it.
/// `target` is what the runner knows about the run (`Runner::freeze_target`).
pub fn freeze_policy(target: Option<Target>, id: String) -> Background {
    let d = decision(&id, target.as_ref());
    Background { kind: kind(&id, target.as_ref()), pause: d.pause, can_change: d.can_change, why: d.why, frozen: is_frozen(&id), run: id }
}

/// "Keep running in the background": true keeps it running, false pauses it, null is the default.
pub fn freeze_policy_set(id: String, keep: Option<bool>) -> Result<(), String> {
    set_keep(&id, keep)
}

/// The UI knows what Rust doesn't: whether a game is online (a Sui game, anti-cheat).
pub fn freeze_note(id: String, online: bool) {
    note_online(&id, online);
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice";
    const STEAM_CG: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/rare-steam-1761540000000.scope";
    const SESSION: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/gamescope-session-plus@rare.service";

    fn p(pid: u32, ppid: u32, comm: &str, cmdline: &str, cgroup: &str) -> Proc {
        Proc { pid, ppid, comm: comm.into(), cmdline: cmdline.into(), cgroup: cgroup.into() }
    }

    /// The launcher (10) under the session (2), Steam (20) in its own scope.
    fn base() -> Vec<Proc> {
        vec![
            p(1, 0, "systemd", "/usr/lib/systemd/systemd --user", "/init.scope"),
            p(2, 1, "gamescope", "gamescope --steam", SESSION),
            p(10, 2, "rare-launcher", "rare-launcher", SESSION),
            p(20, 1, "steam", "steam -silent", STEAM_CG),
            p(21, 20, "steamwebhelper", "steamwebhelper", STEAM_CG),
        ]
    }

    const ME: &[u32] = &[10, 2, 1];

    #[test]
    fn reads_proc() {
        assert_eq!(parse_stat("4242 (wine64-preloader) S 4200 4242 ..."), Some((4200, "wine64-preloader".into())));
        assert_eq!(parse_stat("77 (Main (thread) x) R 12 77"), Some((12, "Main (thread) x".into())));
        assert_eq!(parse_cgroup("0::/user.slice/app.slice/a.scope\n"), Some("/user.slice/app.slice/a.scope".into()));
        assert_eq!(parse_cgroup("1:name=systemd:/x\n"), None);
        assert_eq!(steam_scope_id(&format!("{APP}/app-steam-app1245620-4321.scope")), Some(1245620));
        assert_eq!(steam_scope_id(&format!("{APP}/app-steam-app0-1.scope")), None);
        assert_eq!(steam_scope_id(&format!("{APP}/app-steam-app12-word.scope")), None);
        assert_eq!(steam_scope_id(STEAM_CG), None);
    }

    #[test]
    fn a_proton_game_in_its_own_steam_scope_is_frozen_whole() {
        let scope = format!("{APP}/app-steam-app1245620-300.scope");
        let mut ps = base();
        ps.extend([
            p(300, 20, "reaper", "/home/u/.steam/ubuntu12_32/reaper SteamLaunch AppId=1245620 -- ...", &scope),
            p(301, 300, "pv-adverb", "pv-adverb", &scope),
            p(302, 301, "python3", "proton waitforexitandrun game.exe", &scope),
            p(303, 302, "game.exe", "Z:\\game.exe", &scope),
            p(304, 300, "wineserver", "wineserver", &scope),
        ]);
        assert_eq!(plan(&Target::Steam { ids: vec![1245620] }, &ps, ME), Ok(Plan::Cgroup { scopes: vec![scope] }));
    }

    #[test]
    fn a_steam_game_in_steams_cgroup_stops_below_the_reaper_only() {
        let mut ps = base();
        ps.extend([
            p(300, 20, "reaper", "reaper SteamLaunch AppId=620 -- ./portal2.sh", STEAM_CG),
            p(301, 300, "portal2.sh", "/bin/sh ./portal2.sh", STEAM_CG),
            p(302, 301, "portal2_linux", "./portal2_linux", STEAM_CG),
            // Another game Steam runs: never touched.
            p(400, 20, "reaper", "reaper SteamLaunch AppId=570 -- dota", STEAM_CG),
            p(401, 400, "dota2", "dota2", STEAM_CG),
        ]);
        assert_eq!(plan(&Target::Steam { ids: vec![620] }, &ps, ME), Ok(Plan::Signal { pids: vec![301, 302] }));
        // A scope named for another id is not this game's.
        let other = format!("{APP}/app-steam-app570-400.scope");
        ps.retain(|x| x.pid < 400);
        ps.extend([p(400, 20, "reaper", "reaper SteamLaunch AppId=620 --", &other), p(401, 400, "x", "x", &other)]);
        assert!(matches!(plan(&Target::Steam { ids: vec![620] }, &ps, ME), Ok(Plan::Signal { .. })));
    }

    #[test]
    fn steam_and_the_launcher_are_never_frozen() {
        // A scope that holds a Steam process (Steam started the game's scope oddly) is refused
        // whole; only the reaper's children are stopped, Steam's processes left out.
        let scope = format!("{APP}/app-steam-app620-300.scope");
        let mut ps = base();
        ps.extend([p(300, 20, "reaper", "reaper SteamLaunch AppId=620 --", &scope), p(301, 300, "steamwebhelper", "steamwebhelper", &scope), p(302, 300, "hl2", "hl2", &scope)]);
        assert_eq!(plan(&Target::Steam { ids: vec![620] }, &ps, ME), Ok(Plan::Signal { pids: vec![302] }));
        // A program whose tree reaches the launcher's own pid: the launcher is left out.
        let ps = base();
        assert_eq!(plan(&Target::Child { pid: 2, unit: None }, &ps, ME), Err(Refuse::NotFound));
        // Rare's own web game window.
        assert_eq!(plan(&Target::Window, &ps, ME), Err(Refuse::NotFreezable));
        assert_eq!(plan(&Target::SteamUi, &ps, ME), Err(Refuse::NotFreezable));
        // Steam has not started the game yet.
        assert_eq!(plan(&Target::Steam { ids: vec![620] }, &ps, ME), Err(Refuse::NotFound));
    }

    #[test]
    fn programs_rare_started_freeze_by_scope_with_their_flatpak() {
        let unit = "app-steam-app2876543210-5000123";
        let scope = format!("{APP}/{unit}.scope");
        let fp = format!("{APP}/app-flatpak-org.duckstation.DuckStation-777.scope");
        let mut ps = base();
        ps.extend([
            p(500, 10, "flatpak", "flatpak run org.duckstation.DuckStation", &scope),
            p(501, 500, "bwrap", "bwrap --args 41 duckstation", &fp),
            p(502, 501, "duckstation-qt", "duckstation-qt", &fp),
        ]);
        let t = Target::Child { pid: 500, unit: Some(unit.into()) };
        assert_eq!(plan(&t, &ps, ME), Ok(Plan::Cgroup { scopes: vec![scope.clone(), fp] }));
        // No scope (RARE_NO_SCOPE, desktop): the tree by signal.
        let t = Target::Child { pid: 500, unit: None };
        assert_eq!(plan(&t, &ps, ME), Ok(Plan::Signal { pids: vec![500, 501, 502] }));
        // A scope that also holds the launcher (never: it would freeze Rare) falls back to signals.
        let mut ps2 = base();
        ps2.extend([p(500, 10, "retroarch", "retroarch", &scope), p(11, 2, "rare-launcher", "rare-launcher", &scope)]);
        assert_eq!(plan(&Target::Child { pid: 500, unit: Some(unit.into()) }, &ps2, &[11, 2, 1]), Ok(Plan::Signal { pids: vec![500] }));
    }

    #[test]
    fn heroic_games_stop_by_their_folder() {
        let mut ps = base();
        ps.extend([p(600, 1, "heroic", "heroic --no-gui", SESSION), p(601, 600, "wine", "wine /games/Hades/Hades.exe", SESSION), p(602, 601, "Hades.exe", "/games/Hades/Hades.exe", SESSION)]);
        assert_eq!(plan(&Target::Dir { dir: "/games/Hades".into() }, &ps, ME), Ok(Plan::Signal { pids: vec![601, 602] }));
    }

    #[test]
    fn defaults() {
        // Games pause; online ones, apps and Chrome web games keep running; the player can flip those.
        assert_eq!(decide(Kind::Game, false, None), Decision { pause: true, can_change: true, why: Why::Default });
        assert_eq!(decide(Kind::Game, true, None), Decision { pause: false, can_change: true, why: Why::Online });
        assert_eq!(decide(Kind::App, false, None), Decision { pause: false, can_change: true, why: Why::App });
        assert_eq!(decide(Kind::WebChrome, false, None), Decision { pause: false, can_change: true, why: Why::WebGame });
        // The player's switch wins where there is one.
        assert_eq!(decide(Kind::Game, false, Some(true)), Decision { pause: false, can_change: true, why: Why::You });
        assert_eq!(decide(Kind::App, false, Some(false)), Decision { pause: true, can_change: true, why: Why::You });
        assert_eq!(decide(Kind::Game, true, Some(false)).pause, true);
        // Streams and Rare's own web game window never freeze, whatever was saved.
        for k in [Kind::Stream, Kind::WebWindow, Kind::Steam] {
            assert!(!decide(k, false, Some(false)).pause);
            assert!(!decide(k, false, None).can_change);
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("stream:abc:1", None), Kind::Stream);
        assert_eq!(kind("app:steam", Some(&Target::SteamUi)), Kind::Steam);
        assert_eq!(kind("app:steam", None), Kind::Steam);
        assert_eq!(kind("app:telegram", Some(&Target::Child { pid: 1, unit: None })), Kind::App);
        assert_eq!(kind("web:act", Some(&Target::Window)), Kind::WebWindow);
        assert_eq!(kind("web:act", None), Kind::WebWindow);
        assert_eq!(kind("web:act", Some(&Target::Child { pid: 1, unit: None })), Kind::WebChrome);
        for g in ["steam:620", "shortcut:123", "epic:x", "gog:1", "local:a", "rare:b", "emu:duck:x"] {
            assert_eq!(kind(g, None), Kind::Game, "{g}");
        }
    }

    #[test]
    fn at_most_two() {
        let f = |r: &str| Frozen { run: r.into(), plan: Plan::Signal { pids: vec![1] }, at: 0 };
        let mut s = Set::default();
        assert_eq!(s.admit("a"), Admit::Yes);
        s.add(f("a"));
        assert_eq!(s.admit("a"), Admit::Already);
        s.add(f("b"));
        assert_eq!(s.admit("c"), Admit::Full(vec!["a".into(), "b".into()]));
        // Thawing one makes room; a game that ended leaves the set.
        assert!(s.take("a").is_some());
        assert_eq!(s.admit("c"), Admit::Yes);
        s.add(f("c"));
        let gone = s.retain_running(&["c".into()]);
        assert_eq!(gone.len(), 1);
        assert_eq!(s.runs(), vec!["c".to_string()]);
        assert!(s.take("zzz").is_none());
    }

    #[test]
    fn frozen_list_survives_a_restart_as_json() {
        let list = vec![
            Frozen { run: "steam:620".into(), plan: Plan::Cgroup { scopes: vec!["/a/app-steam-app620-1.scope".into()] }, at: 5 },
            Frozen { run: "local:x".into(), plan: Plan::Signal { pids: vec![42, 43] }, at: 6 },
        ];
        let t = serde_json::to_string(&list).unwrap();
        assert!(t.contains("\"how\":\"cgroup\"") && t.contains("\"how\":\"signal\""));
        let back: Vec<Frozen> = serde_json::from_str(&t).unwrap();
        assert_eq!(back, list);
    }
}
