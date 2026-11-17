//! The Epic and GOG download queue: installs, updates and repairs, one at a time, with progress,
//! pause (stops the tool; both tools pick up where they stopped), resume and cancel. Its rows join
//! Steam's in the launcher's downloads list (`downloads`, the same shape as rare-library's).
//!
//! The queue is kept in `~/.config/rare/stores/downloads.json`, so a restart (or a crash) does not
//! lose it: `restore` puts it back at start, and the tools carry on from their own resume data.

use crate::progress::{self, Update};
use crate::tools::{self, GOGDL, LEGENDARY};
use crate::Store;
use std::path::PathBuf;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Into `base` (a drive's Games/Epic or Games/GOG folder).
    Install { base: PathBuf },
    Update,
    /// Check every file and fetch what is missing or broken.
    Repair,
    /// Not queued; listed so "Syncing saves" can show.
    Saves,
    /// GOG: the game's runtimes into its prefix (redist.rs), before its first start.
    Setup,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Queued,
    Running,
    Paused,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    /// "epic:<app>" or "gog:<id>".
    pub id: String,
    pub title: String,
    pub kind: Kind,
    pub phase: Phase,
    /// 0..100
    pub pct: f32,
    pub done: u64,
    pub total: u64,
    /// Bytes a second.
    pub speed: f64,
    pub eta: Option<u64>,
    pub verifying: bool,
    /// Installing the game's runtimes into its prefix (after a GOG install).
    pub setup: bool,
}

impl Job {
    /// "Installing", "Updating", "Checking files", "Queued", "Paused".
    pub fn status(&self) -> String {
        match &self.phase {
            Phase::Queued => "Queued".into(),
            Phase::Paused => "Paused".into(),
            Phase::Failed(e) => e.clone(),
            Phase::Running if self.setup => "Setting up".into(),
            Phase::Running if self.verifying => "Checking files".into(),
            Phase::Running => match self.kind {
                Kind::Install { .. } => "Installing".into(),
                Kind::Update => "Updating".into(),
                Kind::Repair => "Repairing".into(),
                Kind::Saves => "Syncing saves".into(),
                Kind::Setup => "Setting up".into(),
            },
        }
    }
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    /// The running tool's process (group) id.
    pid: Option<u32>,
    /// Stop asked for the running job: true pauses it, false cancels it.
    stop: Option<bool>,
    worker: bool,
}

fn q() -> &'static Mutex<Queue> {
    static Q: OnceLock<Mutex<Queue>> = OnceLock::new();
    Q.get_or_init(Default::default)
}

type Done = Box<dyn Fn(&Job, &Result<(), String>) + Send + Sync>;
static ON_DONE: OnceLock<Done> = OnceLock::new();

/// Called when a job ends (finished or failed, not paused or cancelled), from the worker thread.
pub fn on_done(f: Done) {
    let _ = ON_DONE.set(f);
}

pub fn list() -> Vec<Job> {
    q().lock().unwrap().jobs.clone()
}

pub fn get(id: &str) -> Option<Job> {
    q().lock().unwrap().jobs.iter().find(|j| j.id == id).cloned()
}

/// Queues it (a job already queued for the game is replaced unless it runs). Starts the worker.
pub fn add(id: &str, title: &str, kind: Kind) -> Result<(), String> {
    let (store, _) = Store::of(id).ok_or("Not an Epic or GOG game")?;
    let tool = if store == Store::Epic { LEGENDARY } else { GOGDL };
    if !tools::have(tool) {
        return Err(format!("{tool} is missing. Update Rare and try again."));
    }
    {
        let mut st = q().lock().unwrap();
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            if j.phase == Phase::Running {
                return Ok(());
            }
            *j = Job { kind, phase: Phase::Queued, ..j.clone() };
        } else {
            st.jobs.push(Job { id: id.into(), title: title.into(), kind, phase: Phase::Queued, pct: 0.0, done: 0, total: 0, speed: 0.0, eta: None, verifying: false, setup: false });
        }
        save(&st.jobs);
    }
    crate::changed();
    start_worker();
    Ok(())
}

fn halt(id: &str, pause: bool) {
    let mut st = q().lock().unwrap();
    let running = st.jobs.iter().any(|j| j.id == id && j.phase == Phase::Running);
    if running {
        st.stop = Some(pause);
        if let Some(pid) = st.pid {
            tools::stop(pid);
        }
    } else if pause {
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            j.phase = Phase::Paused;
        }
    } else {
        st.jobs.retain(|j| j.id != id);
    }
    save(&st.jobs);
    drop(st);
    crate::changed();
}

/// Stops it and keeps what was downloaded; Resume carries on.
pub fn pause(id: &str) {
    halt(id, true);
}

/// Stops it and drops it from the queue (the files fetched so far stay for a later try).
pub fn cancel(id: &str) {
    halt(id, false);
}

pub fn resume(id: &str) {
    let mut st = q().lock().unwrap();
    if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id && matches!(j.phase, Phase::Paused | Phase::Failed(_))) {
        j.phase = Phase::Queued;
    }
    save(&st.jobs);
    drop(st);
    crate::changed();
    start_worker();
}

/// Rows for the launcher's downloads list: (id, title, done, total, paused).
pub fn downloads() -> Vec<(String, String, u64, u64, bool)> {
    list()
        .into_iter()
        .filter(|j| !matches!(j.phase, Phase::Failed(_)))
        .map(|j| {
            // A size before the tool says it: percent of a nominal total.
            let (done, total) = if j.total > 0 { (j.done.min(j.total), j.total) } else { ((j.pct as f64 * 10.0) as u64, 1000) };
            (j.id, j.title, done, total, j.phase == Phase::Paused)
        })
        .collect()
}

/// Runs a job on this thread, listed while it runs (a GOG game's setup before its first start).
pub fn run_now(id: &str, title: &str, kind: Kind) -> Result<(), String> {
    {
        let mut st = q().lock().unwrap();
        if st.jobs.iter().any(|j| j.id == id) {
            return Err("It is busy with a download. Try again when it is done.".into());
        }
        st.jobs.push(Job { id: id.into(), title: title.into(), kind: kind.clone(), phase: Phase::Running, pct: 0.0, done: 0, total: 0, speed: 0.0, eta: None, verifying: false, setup: kind == Kind::Setup });
    }
    crate::changed();
    let job = get(id).unwrap();
    let r = run(&job);
    q().lock().unwrap().jobs.retain(|j| j.id != id);
    crate::changed();
    r
}

/// A job as it would show (fixtures for screenshots: RR_STORE_JOB in the native launcher).
pub fn show(job: Job) {
    FIXTURE.store(true, Ordering::Relaxed);
    let mut st = q().lock().unwrap();
    st.jobs.retain(|j| j.id != job.id);
    st.jobs.push(job);
    drop(st);
    crate::changed();
}

/// The job's line says "Setting up" from now on.
fn mark_setup(id: &str) {
    {
        let mut st = q().lock().unwrap();
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            j.setup = true;
            j.pct = 0.0;
            j.total = 0;
            j.eta = None;
        }
    }
    crate::changed();
}

/// The game's prefix, Proton and umu's runtime (prep.rs), when one is missing.
fn prep(id: &str) -> Result<(), String> {
    if !crate::prep::needed(id) {
        return Ok(());
    }
    mark_setup(id);
    // Not the queue's pid: a first start's setup (run_now) runs beside the download in progress,
    // whose Pause must still stop that download. Setup rows have no Pause or Cancel.
    crate::prep::prepare(id, &mut |_| {}, &mut |_| {})
}

/// GOG: the prefix (prep.rs), then its runtimes, with the job's line saying which one.
fn setup(id: &str, game: &str, deps: Option<Vec<String>>) -> Result<(), String> {
    prep(id)?;
    mark_setup(id);
    crate::redist::ensure(game, deps, &mut |_, i, n| {
        apply(id, Update { pct: Some(i as f32 * 100.0 / n.max(1) as f32), ..Default::default() });
        crate::changed();
    })
    .map(|_| ())
}

fn start_worker() {
    {
        let mut st = q().lock().unwrap();
        if st.worker {
            return;
        }
        st.worker = true;
    }
    let _ = std::thread::Builder::new().name("store-downloads".into()).spawn(|| loop {
        let next = {
            let mut st = q().lock().unwrap();
            match st.jobs.iter_mut().find(|j| j.phase == Phase::Queued) {
                Some(j) => {
                    j.phase = Phase::Running;
                    j.verifying = false;
                    j.speed = 0.0;
                    let j = j.clone();
                    save(&st.jobs);
                    Some(j)
                }
                None => {
                    st.worker = false;
                    None
                }
            }
        };
        let Some(job) = next else { break };
        crate::changed();
        let r = run(&job);
        let mut st = q().lock().unwrap();
        let stop = st.stop.take();
        st.pid = None;
        let mut ended = None;
        match (stop, &r) {
            (Some(true), _) => {
                if let Some(j) = st.jobs.iter_mut().find(|j| j.id == job.id) {
                    j.phase = Phase::Paused;
                    j.speed = 0.0;
                }
            }
            (Some(false), _) => st.jobs.retain(|j| j.id != job.id),
            (None, Ok(())) => {
                ended = st.jobs.iter().find(|j| j.id == job.id).cloned();
                st.jobs.retain(|j| j.id != job.id);
            }
            (None, Err(e)) => {
                if let Some(j) = st.jobs.iter_mut().find(|j| j.id == job.id) {
                    j.phase = Phase::Failed(e.clone());
                    j.speed = 0.0;
                    ended = Some(j.clone());
                }
            }
        }
        save(&st.jobs);
        drop(st);
        crate::changed();
        if let (Some(j), Some(f)) = (ended, ON_DONE.get()) {
            f(&j, &r);
        }
    });
}

fn apply(id: &str, u: Update) {
    let mut st = q().lock().unwrap();
    let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) else { return };
    if let Some(p) = u.pct {
        j.pct = p.clamp(0.0, 100.0);
    }
    if let Some(d) = u.done {
        j.done = d;
    }
    if let Some(t) = u.total {
        j.total = t;
    }
    if let Some(s) = u.speed {
        j.speed = s;
    }
    if u.eta.is_some() {
        j.eta = u.eta;
    }
    j.verifying = u.verifying || (j.verifying && u.pct.is_none());
    // legendary counts chunks for the percent and bytes for the size: keep the bar on the percent.
    if j.total > 0 && u.pct.is_some() && u.done.is_none() {
        j.done = (j.total as f64 * j.pct as f64 / 100.0) as u64;
    }
}

/// The device's memory in MiB (/proc/meminfo; 8 GiB when it can't be read).
fn mem_mib() -> u64 {
    std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| t.lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())).map(|kb| kb / 1024).unwrap_or(8192)
}

/// Free space check before an install (both tools check too, with worse messages).
fn check_space(base: &std::path::Path, need: u64) -> Result<(), String> {
    let probe = std::iter::successors(Some(base), |p| p.parent()).find(|p| p.exists()).unwrap_or(base);
    if let Some((free, _)) = crate::drives::space(probe) {
        if need > 0 && free < need {
            return Err(format!("Not enough space. It needs {}, and {} is free.", crate::fmt_bytes(need), crate::fmt_bytes(free)));
        }
    }
    Ok(())
}

fn run(job: &Job) -> Result<(), String> {
    let (store, sid) = Store::of(&job.id).ok_or("Not an Epic or GOG game")?;
    if !matches!(job.kind, Kind::Saves | Kind::Setup) {
        stop_leftovers(std::slice::from_ref(job));
    }
    let id = job.id.clone();
    let mut started = |pid: u32| q().lock().unwrap().pid = Some(pid);
    let mut last = std::time::Instant::now();
    let mut saved = std::time::Instant::now();
    let mut line = |l: &str| {
        if let Some(u) = progress::parse(l) {
            apply(&id, u);
            // At most a few redraws a second.
            if last.elapsed().as_millis() > 400 {
                last = std::time::Instant::now();
                crate::changed();
            }
            // The percent on disk for after a restart, every few seconds.
            if saved.elapsed().as_secs() >= 5 {
                saved = std::time::Instant::now();
                save(&q().lock().unwrap().jobs);
            }
        }
    };
    match store {
        Store::Epic => {
            if job.kind == Kind::Setup {
                return prep(&job.id);
            }
            if let Kind::Install { base } = &job.kind {
                let _ = std::fs::create_dir_all(base);
                if let Some((_, disk)) = crate::epic::size(sid) {
                    check_space(base, disk)?;
                }
            }
            let mut args = crate::epic::job_args(&job.kind, sid);
            let mut out = tools::run_with(LEGENDARY, &args, &[], None, &mut started, &mut line)?;
            // A big game needs more than legendary's 2 GiB of shared memory: once more with room.
            if let Some(more) = (!out.ok).then(|| crate::epic::more_memory_args(&out.error(), mem_mib())).flatten() {
                log::info!("store job {sid}: legendary needs more shared memory, again with {}", more.join(" "));
                args.extend(more);
                out = tools::run_with(LEGENDARY, &args, &[], None, &mut started, &mut line)?;
            }
            if !out.ok {
                return Err(friendly(&out.error()));
            }
            // A fresh install gets its prefix and Proton now, so its first start is a normal one.
            // If this fails it runs again before the first start, which says why.
            if matches!(job.kind, Kind::Install { .. }) {
                if let Err(e) = prep(&job.id) {
                    log::warn!("setup after install {sid}: {e}");
                }
            }
            Ok(())
        }
        Store::Gog => {
            if let Kind::Install { base } = &job.kind {
                let _ = std::fs::create_dir_all(base);
            }
            if job.kind == Kind::Setup {
                return setup(&job.id, sid, None);
            }
            let (args, dir, size) = crate::gog::job_args(&job.kind, sid).map_err(|e| friendly(&e))?;
            if let Kind::Install { base } = &job.kind {
                check_space(base, size)?;
                if size > 0 {
                    apply(&job.id, Update { total: Some(size), ..Default::default() });
                }
            }
            let out = tools::run_with(GOGDL, &args, &[], None, &mut started, &mut line)?;
            if !out.ok {
                return Err(friendly(&out.error()));
            }
            crate::gog::finish(sid, &dir, size);
            // A fresh install gets its runtimes now, so its first start is a normal one. If this
            // fails it runs again before the first start.
            if matches!(job.kind, Kind::Install { .. }) && crate::gog::installed(sid).is_some_and(|i| i.1 != "linux") {
                if let Err(e) = setup(&job.id, sid, None) {
                    log::warn!("setup after install {sid}: {e}");
                }
            }
            Ok(())
        }
    }
}

/// Set by `show`: a screenshot fixture is on screen, so nothing is written to disk.
static FIXTURE: AtomicBool = AtomicBool::new(false);

fn saved_path() -> std::path::PathBuf {
    crate::rare_dir().join("downloads.json")
}

/// Writes the queue to disk. Only installs, updates and repairs are kept: saves and setup run
/// beside a start and are not queued, and a failed job shows its error once.
/// What was last written (or read at start): the same queue is not written again, so a stalled
/// download, or progress that did not move, costs no write.
static SAVED: Mutex<Option<Value>> = Mutex::new(None);

fn save(jobs: &[Job]) {
    if FIXTURE.load(Ordering::Relaxed) || cfg!(test) {
        return;
    }
    let v = to_value(jobs);
    let mut last = SAVED.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref() == Some(&v) {
        return;
    }
    // write_json writes a temporary file and renames it over the old one, so a power cut leaves the
    // old queue or the new one, never half of one.
    crate::write_json(&saved_path(), &v);
    *last = Some(v);
}

fn to_value(jobs: &[Job]) -> Value {
    let rows: Vec<Value> = jobs
        .iter()
        .filter_map(|j| {
            let kind = match &j.kind {
                Kind::Install { base } => json!({ "kind": "install", "base": base }),
                Kind::Update => json!({ "kind": "update" }),
                Kind::Repair => json!({ "kind": "repair" }),
                Kind::Saves | Kind::Setup => return None,
            };
            let phase = match j.phase {
                Phase::Paused => "paused",
                Phase::Queued | Phase::Running => "queued",
                Phase::Failed(_) => return None,
            };
            Some(json!({ "id": j.id, "title": j.title, "job": kind, "phase": phase, "pct": j.pct, "done": j.done, "total": j.total }))
        })
        .collect();
    json!({ "jobs": rows })
}

/// The saved queue as jobs: a job that was running comes back queued, a paused one stays paused.
fn from_value(v: &Value) -> Vec<Job> {
    let Some(rows) = v["jobs"].as_array() else { return vec![] };
    rows.iter()
        .filter_map(|r| {
            let id = r["id"].as_str()?;
            Store::of(id)?;
            let kind = match r["job"]["kind"].as_str()? {
                "install" => Kind::Install { base: r["job"]["base"].as_str()?.into() },
                "update" => Kind::Update,
                "repair" => Kind::Repair,
                _ => return None,
            };
            let phase = if r["phase"] == "paused" { Phase::Paused } else { Phase::Queued };
            let pct = r["pct"].as_f64().unwrap_or(0.0) as f32;
            let done = r["done"].as_u64().unwrap_or(0);
            let total = r["total"].as_u64().unwrap_or(0);
            Some(Job { id: id.into(), title: r["title"].as_str().unwrap_or(id).into(), kind, phase, pct, done, total, speed: 0.0, eta: None, verifying: false, setup: false })
        })
        .collect()
}

/// Once at start: the queue from before a restart, and the worker for what was not paused.
/// legendary and gogdl skip what they already fetched, so a job carries on rather than restarts.
pub fn restore() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::Relaxed) || FIXTURE.load(Ordering::Relaxed) {
        return;
    }
    let Some(v) = crate::read_json(&saved_path()) else { return };
    let saved = from_value(&v);
    *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(to_value(&saved));
    if saved.is_empty() {
        return;
    }
    {
        let mut st = q().lock().unwrap();
        for j in saved {
            if !st.jobs.iter().any(|k| k.id == j.id) {
                st.jobs.push(j);
            }
        }
    }
    crate::changed();
    start_worker();
}

/// A tool runs in a process group of its own, so a launcher that quits or restarts (not a reboot)
/// leaves it running. A job for the same game (restored, or Install pressed again) would run a
/// second copy beside it; seen with legendary, the second one ended at once with nothing done and
/// the job counted as finished. So `run` stops the one left over first (both tools keep what they
/// fetched), at most a few seconds' wait.
fn stop_leftovers(jobs: &[Job]) {
    let Ok(dir) = std::fs::read_dir("/proc") else { return };
    let mut left = Vec::new();
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(raw) = std::fs::read(e.path().join("cmdline")) else { continue };
        let args: Vec<String> = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        if leftover_for(&args, jobs) {
            left.push(pid);
        }
    }
    for pid in &left {
        // legendary's download workers are forks with the same arguments: the group they share
        // goes with its leader. A tool that is not its group's leader was started by something
        // else (Heroic runs legendary in its own group): only that process is stopped, never the
        // program that started it.
        match pgid(*pid) {
            Some(g) if g == *pid => {
                log::info!("store job: stopping a download the last launcher left running ({pid})");
                tools::stop(*pid);
            }
            Some(g) if left.contains(&g) => {}
            _ => {
                log::info!("store job: stopping a download left running ({pid})");
                #[cfg(unix)]
                unsafe {
                    libc::kill(*pid as i32, libc::SIGTERM);
                }
            }
        }
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < until && left.iter().any(|p| std::path::Path::new(&format!("/proc/{p}")).exists()) {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// A process's group id (/proc/<pid>/stat, the fifth field; the name in field two may hold spaces).
fn pgid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(2)?.parse().ok()
}

/// A process's arguments are the tool itself (`legendary` or `gogdl`, run directly or by its
/// interpreter) downloading one of `jobs`' games: an install, update or repair, never a quick
/// query on the same game (`legendary info`, which the install sheet runs for the size).
fn leftover_for(args: &[String], jobs: &[Job]) -> bool {
    let is_tool = |a: &String| [LEGENDARY, GOGDL].iter().any(|t| a == t || a.ends_with(&format!("/{t}")));
    let downloads = args.iter().any(|a| matches!(a.as_str(), "install" | "repair" | "update" | "download"));
    args.iter().take(2).any(is_tool) && downloads && jobs.iter().any(|j| Store::of(&j.id).is_some_and(|(_, sid)| args.iter().any(|a| a == sid)))
}

/// A tool's error as a line for the player.
fn friendly(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("space") {
        ERR_SPACE.into()
    } else if l.contains("login") || l.contains("credentials") || l.contains("auth") || l.contains("sign in") {
        ERR_SIGN_IN.into()
    } else if l.contains("connection") || l.contains("timed out") || l.contains("resolve") || l.contains("network") {
        ERR_OFFLINE.into()
    } else if e.is_empty() {
        ERR_GENERIC.into()
    } else {
        log::warn!("store job: {e}");
        ERR_GENERIC.into()
    }
}

/// The failures `friendly` words (in English: the launcher shows them in the UI language,
/// features::stores::status_text, by these values).
pub const ERR_SPACE: &str = "Not enough space on that drive.";
pub const ERR_SIGN_IN: &str = "Sign in again in Settings > Accounts.";
pub const ERR_OFFLINE: &str = "No connection. Check Wi-Fi and try again.";
pub const ERR_GENERIC: &str = "It stopped with an error. Try again.";

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, kind: Kind, phase: Phase) -> Job {
        Job { id: id.into(), title: format!("T {id}"), kind, phase, pct: 42.5, done: 4_250, total: 10_000, speed: 1e6, eta: Some(9), verifying: true, setup: false }
    }

    #[test]
    fn the_queue_survives_a_restart() {
        let jobs = vec![
            job("epic:a", Kind::Install { base: "/g/Epic".into() }, Phase::Running),
            job("gog:1", Kind::Update, Phase::Queued),
            job("epic:b", Kind::Repair, Phase::Paused),
        ];
        let back = from_value(&to_value(&jobs));
        assert_eq!(back.len(), 3);
        // Running comes back queued so the worker picks it up again; paused stays paused.
        assert_eq!(back[0].phase, Phase::Queued);
        assert_eq!(back[0].kind, Kind::Install { base: "/g/Epic".into() });
        assert_eq!(back[1].phase, Phase::Queued);
        assert_eq!(back[2].phase, Phase::Paused);
        // Where it got to shows at once; the live numbers start fresh.
        assert_eq!((back[0].pct, back[0].done, back[0].total), (42.5, 4_250, 10_000));
        assert_eq!((back[0].speed, back[0].eta, back[0].verifying), (0.0, None, false));
        assert_eq!(back[0].title, "T epic:a");
    }

    #[test]
    fn failed_saves_and_setup_jobs_are_not_kept() {
        let jobs = vec![
            job("epic:a", Kind::Update, Phase::Failed("x".into())),
            job("epic:b", Kind::Saves, Phase::Running),
            job("gog:2", Kind::Setup, Phase::Queued),
        ];
        assert_eq!(to_value(&jobs), json!({ "jobs": [] }));
    }

    #[test]
    fn a_leftover_tool_is_found_by_its_game() {
        let jobs = vec![job("epic:abc", Kind::Update, Phase::Queued), job("gog:42", Kind::Repair, Phase::Paused)];
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(leftover_for(&a(&["python3", "/usr/libexec/rare/stores/legendary", "-y", "install", "abc", "--base-path", "/g"]), &jobs));
        assert!(leftover_for(&a(&["/x/bin/gogdl", "download", "42"]), &jobs));
        // Another game, another program, or the tool's helpers (the game id only inside -c code).
        assert!(!leftover_for(&a(&["python3", "/usr/libexec/rare/stores/legendary", "install", "other"]), &jobs));
        assert!(!leftover_for(&a(&["vim", "abc"]), &jobs));
        assert!(!leftover_for(&a(&["python3", "/usr/libexec/rare/stores/legendary", "info", "abc", "--json"]), &jobs));
        assert!(!leftover_for(&a(&["/usr/bin/python3", "-c", "main()", "/usr/libexec/rare/stores/legendary", "install", "abc"]), &jobs));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_group_is_read() {
        let me = std::process::id();
        assert_eq!(pgid(me), Some(unsafe { libc::getpgid(0) } as u32));
        assert_eq!(pgid(u32::MAX), None);
    }

    #[test]
    fn a_bad_saved_file_gives_an_empty_queue() {
        assert!(from_value(&json!(null)).is_empty());
        assert!(from_value(&json!({ "jobs": "x" })).is_empty());
        let v = json!({ "jobs": [
            { "id": "steam:1", "job": { "kind": "update" } },
            { "id": "epic:a", "job": { "kind": "warp" } },
            { "id": "epic:b", "job": { "kind": "install" } },
            { "id": "epic:c", "job": { "kind": "update" } }
        ] });
        let back = from_value(&v);
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].id.as_str(), back[0].title.as_str(), back[0].phase.clone()), ("epic:c", "epic:c", Phase::Queued));
    }
}
