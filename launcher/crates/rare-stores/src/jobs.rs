//! The Epic and GOG download queue: installs, updates and repairs, one at a time, with progress,
//! pause (stops the tool; both tools pick up where they stopped), resume and cancel. Its rows join
//! Steam's in the launcher's downloads list (`downloads`, the same shape as rare-library's).

use crate::progress::{self, Update};
use crate::tools::{self, GOGDL, LEGENDARY};
use crate::Store;
use std::path::PathBuf;
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
    let mut st = q().lock().unwrap();
    st.jobs.retain(|j| j.id != job.id);
    st.jobs.push(job);
    drop(st);
    crate::changed();
}

/// Its runtimes into its prefix, with the job's line saying which one.
fn setup(id: &str, game: &str, deps: Option<Vec<String>>) -> Result<(), String> {
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
                    Some(j.clone())
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
    let id = job.id.clone();
    let mut started = |pid: u32| q().lock().unwrap().pid = Some(pid);
    let mut last = std::time::Instant::now();
    let mut line = |l: &str| {
        if let Some(u) = progress::parse(l) {
            apply(&id, u);
            // At most a few redraws a second.
            if last.elapsed().as_millis() > 400 {
                last = std::time::Instant::now();
                crate::changed();
            }
        }
    };
    match store {
        Store::Epic => {
            if job.kind == Kind::Setup {
                return Ok(());
            }
            if let Kind::Install { base } = &job.kind {
                let _ = std::fs::create_dir_all(base);
                if let Some((_, disk)) = crate::epic::size(sid) {
                    check_space(base, disk)?;
                }
            }
            let args = crate::epic::job_args(&job.kind, sid);
            let out = tools::run_with(LEGENDARY, &args, &[], None, &mut started, &mut line)?;
            if !out.ok {
                return Err(friendly(&out.error()));
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

/// A tool's error as a line for the player.
fn friendly(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("space") {
        "Not enough space on that drive.".into()
    } else if l.contains("login") || l.contains("credentials") || l.contains("auth") || l.contains("sign in") {
        "Sign in again in Settings > Accounts.".into()
    } else if l.contains("connection") || l.contains("timed out") || l.contains("resolve") || l.contains("network") {
        "No connection. Check Wi-Fi and try again.".into()
    } else if e.is_empty() {
        "It stopped with an error. Try again.".into()
    } else {
        log::warn!("store job: {e}");
        "It stopped with an error. Try again.".into()
    }
}
