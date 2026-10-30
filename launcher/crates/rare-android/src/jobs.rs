//! The Android jobs, one at a time on their own thread: setting Android up, installing an app from
//! F-Droid, uninstalling one. The UI reads `current()` for the progress line and bar.

use crate::{bump, dir, ensure_files, entries, fdroid, initialized, package_of, runner, Status, STORES};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Setup,
    Install { package: String, name: String },
    Remove { package: String, name: String },
}

impl Kind {
    /// The library id the job is about (none for setup).
    pub fn id(&self) -> Option<String> {
        match self {
            Kind::Setup => None,
            Kind::Install { package, .. } | Kind::Remove { package, .. } => Some(format!("android:{package}")),
        }
    }
}

/// A job as the UI shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub kind: Kind,
    /// "Getting Android", "Downloading 2048"...
    pub line: String,
    /// 0..1 when known.
    pub pct: Option<f32>,
}

/// How a job ended, for one notice.
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    pub kind: Kind,
    pub result: Result<String, String>,
}

#[derive(Default)]
struct Q {
    cur: Option<Job>,
    queue: VecDeque<Kind>,
    ended: Vec<Ended>,
    /// Results for `run_wait` callers (they take theirs).
    results: Vec<Ended>,
    worker: bool,
}

static Q: Mutex<Q> = Mutex::new(Q { cur: None, queue: VecDeque::new(), ended: Vec::new(), results: Vec::new(), worker: false });
static LIBRARY: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

fn q() -> std::sync::MutexGuard<'static, Q> {
    Q.lock().unwrap_or_else(|e| e.into_inner())
}

/// What runs when the installed apps changed (the launcher scans its library again).
pub fn on_library(f: impl Fn() + Send + 'static) {
    *LIBRARY.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
}

fn library_changed() {
    if let Some(f) = LIBRARY.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        f();
    }
}

/// The job running now.
pub fn current() -> Option<Job> {
    match crate::fake().as_deref() {
        Some("setup") => return Some(Job { kind: Kind::Setup, line: "Getting Android \u{b7} 412 of 847 MB".into(), pct: Some(0.4) }),
        Some("install") => return Some(Job { kind: Kind::Install { package: "org.example.longname".into(), name: "Shattered Pixel Dungeon".into() }, line: "Getting Shattered Pixel Dungeon".into(), pct: Some(0.62) }),
        _ => {}
    }
    q().cur.clone()
}

/// The jobs waiting, in order.
pub fn queued() -> Vec<Kind> {
    q().queue.iter().cloned().collect()
}

/// A job for this library id is running or waiting.
pub fn busy_with(id: &str) -> bool {
    let g = q();
    g.cur.as_ref().is_some_and(|j| j.kind.id().as_deref() == Some(id)) || g.queue.iter().any(|k| k.id().as_deref() == Some(id))
}

/// Setup is running or waiting.
pub fn setting_up() -> bool {
    let g = q();
    g.cur.as_ref().is_some_and(|j| j.kind == Kind::Setup) || g.queue.contains(&Kind::Setup)
}

/// Jobs that ended since the last call (for notices).
pub fn take_ended() -> Vec<Ended> {
    std::mem::take(&mut q().ended)
}

/// Adds a job. Err when the same one is already running or waiting.
pub fn start(kind: Kind) -> Result<(), String> {
    {
        let mut g = q();
        if g.cur.as_ref().is_some_and(|j| j.kind == kind) || g.queue.contains(&kind) {
            return Err("Already on it".into());
        }
        g.queue.push_back(kind);
        if g.worker {
            drop(g);
            bump();
            return Ok(());
        }
        g.worker = true;
    }
    std::thread::Builder::new().name("rare-android".into()).spawn(worker).map_err(|e| e.to_string())?;
    bump();
    Ok(())
}

/// Adds a job and waits for its result (on a background thread).
pub fn run_wait(kind: Kind) -> Result<String, String> {
    q().results.retain(|e| e.kind != kind);
    start(kind.clone())?;
    loop {
        {
            let mut g = q();
            if let Some(i) = g.results.iter().position(|e| e.kind == kind) {
                return g.results.remove(i).result;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn worker() {
    loop {
        let kind = {
            let mut g = q();
            match g.queue.pop_front() {
                Some(k) => {
                    g.cur = Some(Job { kind: k.clone(), line: String::new(), pct: None });
                    k
                }
                None => {
                    g.cur = None;
                    g.worker = false;
                    drop(g);
                    bump();
                    return;
                }
            }
        };
        let result = match &kind {
            Kind::Setup => setup(),
            Kind::Install { package, name } => install(package, name),
            Kind::Remove { package, name } => remove(package, name),
        };
        if let Err(e) = &result {
            log::warn!("android {kind:?}: {e}");
        }
        {
            let mut g = q();
            // An uninstall answers its sheet (run_wait); the rest become notices.
            if !matches!(kind, Kind::Remove { .. }) {
                g.ended.push(Ended { kind: kind.clone(), result: result.clone() });
            }
            g.results.push(Ended { kind, result });
            let n = g.results.len();
            if n > 16 {
                g.results.drain(..n - 16);
            }
        }
        library_changed();
        bump();
    }
}

static LAST_BUMP: Mutex<Option<Instant>> = Mutex::new(None);

fn say(line: impl Into<String>, pct: Option<f32>) {
    let line = line.into();
    let changed = {
        let mut g = q();
        let Some(j) = g.cur.as_mut() else { return };
        let moved = j.line != line;
        j.line = line;
        j.pct = pct.map(|p| p.clamp(0.0, 1.0));
        moved
    };
    // A new line at once; progress at most ten times a second.
    let mut t = LAST_BUMP.lock().unwrap_or_else(|e| e.into_inner());
    if changed || t.map_or(true, |t| t.elapsed() >= Duration::from_millis(100)) {
        *t = Some(Instant::now());
        drop(t);
        bump();
    }
}

/// Runs the runner script; Err with its last error line.
fn script(args: &[&str]) -> Result<String, String> {
    ensure_files().map_err(|e| format!("Couldn't write Rare's Android helper: {e}"))?;
    let out = Command::new(runner()).args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Android didn't answer").trim_start_matches("[rare-android] ").to_string())
}

/// Android runs (on screen or in the background).
pub fn session_up() -> bool {
    Command::new("timeout").args(["10", "waydroid", "status"]).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.starts_with("Session:") && l.contains("RUNNING"))).unwrap_or(false)
}

/// An Android app runs on screen (a `rare-android run`).
pub fn on_screen() -> bool {
    let me = runner().to_string_lossy().into_owned();
    let Ok(rd) = std::fs::read_dir("/proc") else { return false };
    rd.flatten().any(|e| {
        let Ok(c) = std::fs::read(e.path().join("cmdline")) else { return false };
        let args: Vec<&[u8]> = c.split(|b| *b == 0).collect();
        args.windows(2).any(|w| w[0] == me.as_bytes() && w[1] == b"run")
    })
}

/// The channels in the image's channels.cfg (the D-Bus Init needs no password with exactly these).
fn channels() -> Option<(String, String)> {
    for p in ["/etc/waydroid-extra/channels.cfg", "/usr/share/waydroid-extra/channels.cfg"] {
        let Ok(s) = std::fs::read_to_string(p) else { continue };
        let get = |k: &str| s.lines().find_map(|l| l.split_once('=').filter(|(a, _)| a.trim() == k).map(|(_, v)| v.trim().to_string()));
        if let (Some(a), Some(b)) = (get("system_channel"), get("vendor_channel")) {
            return Some((a, b));
        }
    }
    None
}

/// "[Downloading] 67.31 MB/847.45 MB ..." -> (67.31, 847.45).
fn parse_download(line: &str) -> Option<(f64, f64)> {
    let rest = line.split("[Downloading]").nth(1)?.trim();
    let (done, rest) = rest.split_once("MB/")?;
    let total = rest.trim().split_whitespace().next()?;
    Some((done.trim().parse().ok()?, total.trim_end_matches("MB").parse().ok()?))
}

/// gdbus monitor's signal line -> (member, its string argument).
fn parse_signal(line: &str) -> Option<(&str, String)> {
    let i = line.find("id.waydro.Initializer.")?;
    let rest = &line[i + "id.waydro.Initializer.".len()..];
    let (member, args) = rest.split_once(' ').unwrap_or((rest, ""));
    let arg = args.trim().trim_start_matches("('").trim_end_matches(",)").trim_end_matches('\'').replace("\\r", "").replace("\\n", "");
    Some((member, arg))
}

/// Waydroid's image download and unpack through the container service (no password with the
/// image's channels). Progress from its ProgressChanged signals.
fn init_images() -> Result<(), String> {
    let (sys, ven) = channels().ok_or("This Rare OS can't set up Android yet. Update Rare OS in Settings > System, then try again.")?;
    say("Starting Android's service", None);
    script(&["start"])?;
    // The service's D-Bus name shows once it is up.
    let t = Instant::now();
    loop {
        let ok = Command::new("gdbus").args(["introspect", "--system", "--dest", "id.waydro.Container", "--object-path", "/Initializer"]).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        if ok {
            break;
        }
        if t.elapsed() > Duration::from_secs(60) {
            return Err("Android's service didn't start".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let mut mon = Command::new("gdbus")
        .args(["monitor", "--system", "--dest", "id.waydro.Container", "--object-path", "/Initializer"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("gdbus: {e}"))?;
    let stdout = mon.stdout.take().ok_or("gdbus")?;
    std::thread::sleep(Duration::from_millis(300));
    let params = format!("{{'system_channel': '{sys}', 'vendor_channel': '{ven}', 'system_type': 'VANILLA'}}");
    let call = Command::new("gdbus").args(["call", "--system", "--dest", "id.waydro.Container", "--object-path", "/Initializer", "--method", "id.waydro.Initializer.Init", &params]).output().map_err(|e| e.to_string())?;
    if !call.status.success() {
        let _ = mon.kill();
        let e = String::from_utf8_lossy(&call.stderr);
        return Err(if e.contains("Polkit") { "Android setup needs the next Rare OS update".into() } else { format!("Android setup didn't start: {}", e.trim()) });
    }
    say("Getting Android", Some(0.0));
    // The system image comes first (about 850 MB), then the vendor image (about 150 MB).
    let mut vendor = false;
    let mut result = Err("Android setup stopped".to_string());
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Some((member, arg)) = parse_signal(&line) else { continue };
        match member {
            "ProgressChanged" => {
                if let Some((done, total)) = parse_download(&arg) {
                    let f = if total > 0.0 { (done / total) as f32 } else { 0.0 };
                    let pct = if vendor { 0.85 + f * 0.12 } else { f * 0.82 };
                    say(format!("Getting Android \u{b7} {} of {} MB", done.round(), total.round()), Some(pct));
                } else if arg.starts_with("Validating system") || arg.starts_with("Validating vendor") {
                    say("Checking Android", None);
                } else if arg.starts_with("Extracting") {
                    say("Unpacking Android", None);
                }
                // The system image is checked once it is in; the next download is the vendor's.
                if arg.starts_with("Validating system") {
                    vendor = true;
                }
            }
            "Finished" => {
                result = Ok(());
                break;
            }
            "Interrupted" => {
                result = Err("Android setup stopped. Check the network and try again.".into());
                break;
            }
            _ => {}
        }
    }
    let _ = mon.kill();
    let _ = mon.wait();
    result?;
    if !initialized() {
        return Err("Android's files are not complete. Try again.".into());
    }
    // A fresh start of the container applies the controller and graphics settings.
    say("Starting Android", None);
    script(&["restart"])?;
    Ok(())
}

/// Waits for Android to list an app (its desktop entry appears).
fn wait_listed(pkg: &str, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if entries().iter().any(|e| e.0 == pkg) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    false
}

fn downloads() -> PathBuf {
    dir().join("downloads")
}

/// Downloads an app's APK from F-Droid with progress on the job line.
fn fetch(pkg: &str, name: &str, from: f32, span: f32) -> Result<PathBuf, String> {
    say(format!("Getting {name}"), Some(from));
    let code = fdroid::suggested(pkg)?;
    std::fs::create_dir_all(downloads()).map_err(|e| e.to_string())?;
    let dest = downloads().join(format!("{pkg}.apk"));
    fdroid::download(pkg, code, &dest, |done, total| {
        let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
        say(format!("Getting {name}"), Some(from + f * span));
    })
    .map_err(|e| {
        log::warn!("download {pkg}: {e}");
        format!("Couldn't download {name}. Check the network and try again.")
    })?;
    Ok(dest)
}

/// Puts an APK into Android (which must be up) and waits until Android lists it.
fn put(apk: &std::path::Path, pkg: &str, name: &str) -> Result<(), String> {
    say(format!("Installing {name}"), None);
    script(&["install", &apk.to_string_lossy()])?;
    let ok = wait_listed(pkg, 120);
    let _ = std::fs::remove_file(apk);
    if ok {
        Ok(())
    } else {
        Err(format!("Android couldn't install {name}. It may be made for ARM phones only."))
    }
}

fn setup() -> Result<String, String> {
    if crate::status() == Status::Unavailable {
        return Err("Android isn't available on this system".into());
    }
    if !initialized() {
        // One more try on its own: the image hosts time out now and then.
        let r = init_images().or_else(|e| {
            log::warn!("android init: {e}; trying again");
            say("Trying again", None);
            std::thread::sleep(Duration::from_secs(3));
            init_images()
        });
        if let Err(e) = r {
            let _ = script(&["down"]);
            return Err(e);
        }
    }
    let r = (|| {
        let mut missing: Vec<(&str, &str)> = STORES.iter().copied().filter(|(p, _)| !entries().iter().any(|e| e.0 == *p)).collect();
        say("Starting Android", None);
        script(&["up"])?;
        // Entries for apps installed before (setup again after a reset of Rare's mark).
        missing.retain(|(p, _)| !wait_listed(p, 3));
        let n = missing.len().max(1) as f32;
        for (i, (pkg, name)) in missing.iter().enumerate() {
            let apk = fetch(pkg, name, i as f32 / n, 0.8 / n)?;
            put(&apk, pkg, name)?;
        }
        Ok::<(), String>(())
    })();
    say("Finishing", None);
    let _ = script(&["down"]);
    r?;
    crate::mark_ready();
    Ok("Android is ready. F-Droid and Aurora Store are in Library > Android.".into())
}

fn install(pkg: &str, name: &str) -> Result<String, String> {
    if crate::status() != Status::Ready {
        return Err("Set up Android first".into());
    }
    if entries().iter().any(|e| e.0 == pkg) {
        return Ok(format!("{name} is already installed"));
    }
    let apk = fetch(pkg, name, 0.0, 0.85)?;
    let was_up = session_up();
    if !was_up {
        say("Starting Android", None);
        if let Err(e) = script(&["up"]) {
            let _ = script(&["down"]);
            return Err(e);
        }
    }
    let r = put(&apk, pkg, name);
    if !was_up {
        let _ = script(&["down"]);
    }
    r.map(|_| format!("{name} is in Library > Android"))
}

fn remove(pkg: &str, name: &str) -> Result<String, String> {
    if on_screen() {
        return Err("Close the Android app that's open first".into());
    }
    say(format!("Uninstalling {name}"), None);
    let r = (|| {
        script(&["up"])?;
        // Its data goes now (Android does it); its APK goes once Android has stopped.
        let out = script(&["clear", pkg])?;
        if !out.contains("Success") {
            log::warn!("clear {pkg}: {}", out.trim());
        }
        Ok::<(), String>(())
    })();
    let _ = script(&["down"]);
    r?;
    let dirs = crate::apk_dirs(pkg);
    for d in &dirs {
        std::fs::remove_dir_all(d).map_err(|e| format!("Couldn't remove {name}: {e}"))?;
        // The ~~<random> folder around it, when that was its only content.
        if let Some(parent) = d.parent().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("~~"))) {
            let _ = std::fs::remove_dir(parent);
        }
    }
    crate::forget_entry(pkg);
    crate::touch::forget(pkg);
    Ok(format!("{name} is uninstalled"))
}

/// A library id's display name for a job (the app's name, else its package).
pub fn name_of(id: &str) -> String {
    let pkg = package_of(id).unwrap_or(id);
    entries().into_iter().find(|e| e.0 == pkg).map(|e| e.1).unwrap_or_else(|| pkg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress() {
        assert_eq!(parse_download("\r[Downloading]  67.31 MB/847.45 MB      26.48 MB/s(approx.)"), Some((67.31, 847.45)));
        assert_eq!(parse_download("Validating system image"), None);
        let l = "/Initializer: id.waydro.Initializer.ProgressChanged ('\\r[Downloading] 125.24 MB/847.45 MB      28.96 MB/s(approx.)',)";
        let (m, a) = parse_signal(l).unwrap();
        assert_eq!(m, "ProgressChanged");
        assert_eq!(parse_download(&a), Some((125.24, 847.45)));
        assert_eq!(parse_signal("/Initializer: id.waydro.Initializer.Finished ()").unwrap().0, "Finished");
    }
}
