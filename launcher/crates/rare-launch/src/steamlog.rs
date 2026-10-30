//! Where Steam is in starting a game, read from its console log (`<Steam>/logs/console_log.txt`).
//!
//! Steam logs every step of a Play press as a GameAction line (seen on the SuiPlay, 2026-10-27):
//!
//! ```text
//! [2026-10-27 23:21:35] GameAction [AppID 1145350, ActionID 1] : LaunchApp changed task to SynchronizingCloud with ""
//! [2026-10-27 23:21:35] GameAction [AppID 1145350, ActionID 1] : LaunchApp waiting for user response to SynchronizingCloud "cloudconflict"
//! [2026-10-27 19:53:49] GameAction [AppID 620, ActionID 2] : LaunchApp continues with user response "ShowInterstitials"
//! [2026-10-27 19:53:50] GameAction [AppID 620, ActionID 2] : LaunchApp changed task to Completed with ""
//! ```
//!
//! Steam answers some waits itself, at once: ShowInterstitials and CreatingProcess come on every
//! launch with an empty reason and continue within the same second. A wait with a reason
//! ("cloudconflict"), or one still open after `SELF_ANSWER`, is a question only Steam's UI can
//! answer: Rare says so and hands Steam the screen for it (launch.rs, apps/front.rs).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A wait with no reason still open after this long is waiting for the player.
pub const SELF_ANSWER: Duration = Duration::from_millis(2500);
/// A question counts this long at most (the player may cancel Steam's dialog, which may log nothing).
pub const QUESTION_MAX: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Task { appid: u64, action: u32, task: String },
    Wait { appid: u64, action: u32, task: String, reason: String },
    Continue { appid: u64, action: u32 },
    End { appid: u64, action: u32 },
}

/// One console_log line, when it is a LaunchApp GameAction line.
pub fn parse(line: &str) -> Option<Line> {
    let rest = &line[line.find("GameAction [AppID ")? + "GameAction [AppID ".len()..];
    let (appid, rest) = rest.split_once(", ActionID ")?;
    let appid: u64 = appid.trim().parse().ok()?;
    let (action, rest) = rest.split_once(']')?;
    let action: u32 = action.trim().parse().ok()?;
    let msg = rest.trim_start_matches([' ', ':']).strip_prefix("LaunchApp ")?;
    if let Some(r) = msg.strip_prefix("changed task to ") {
        let task = r.split(" with ").next().unwrap_or(r).trim().to_string();
        return Some(Line::Task { appid, action, task });
    }
    if let Some(r) = msg.strip_prefix("waiting for user response to ") {
        let (task, reason) = match r.split_once(' ') {
            Some((t, q)) => (t, q.trim().trim_matches('"')),
            None => (r.trim(), ""),
        };
        return Some(Line::Wait { appid, action, task: task.to_string(), reason: reason.to_string() });
    }
    if msg.starts_with("continues with user response") {
        return Some(Line::Continue { appid, action });
    }
    let low = msg.to_ascii_lowercase();
    if ["fail", "cancel", "abort", "error"].iter().any(|w| low.contains(w)) {
        return Some(Line::End { appid, action });
    }
    None
}

#[derive(Debug, Clone)]
struct Wait {
    task: String,
    reason: String,
    since: Instant,
}

#[derive(Debug, Clone)]
struct Action {
    id: u32,
    wait: Option<Wait>,
    done: bool,
}

/// Each app's latest launch, from the lines seen so far.
#[derive(Debug, Default)]
pub struct Tracker {
    apps: HashMap<u64, Action>,
}

impl Tracker {
    pub fn apply(&mut self, line: Line, now: Instant) {
        let (appid, action) = match &line {
            Line::Task { appid, action, .. } | Line::Wait { appid, action, .. } | Line::Continue { appid, action } | Line::End { appid, action } => (*appid, *action),
        };
        let a = self.apps.entry(appid).or_insert(Action { id: action, wait: None, done: false });
        if a.id != action {
            *a = Action { id: action, wait: None, done: false };
        }
        match line {
            Line::Task { task, .. } => {
                // A new step: the last wait was answered.
                a.wait = None;
                a.done = task == "Completed";
            }
            Line::Wait { task, reason, .. } => a.wait = Some(Wait { task, reason, since: now }),
            Line::Continue { .. } => a.wait = None,
            Line::End { .. } => {
                a.wait = None;
                a.done = true;
            }
        }
    }

    /// What Steam is asking the player before `appid` can start, if anything.
    pub fn question(&self, appid: u64, now: Instant) -> Option<&'static str> {
        let a = self.apps.get(&appid).filter(|a| !a.done)?;
        let w = a.wait.as_ref()?;
        let age = now.saturating_duration_since(w.since);
        if age >= QUESTION_MAX || (w.reason.is_empty() && age < SELF_ANSWER) {
            return None;
        }
        Some(question_text(&w.task, &w.reason))
    }

    /// Forget an app (its run ended).
    pub fn forget(&mut self, appid: u64) {
        self.apps.remove(&appid);
    }
}

/// The banner for a question Steam is asking (plain words; the dialog itself is Steam's).
pub fn question_text(task: &str, reason: &str) -> &'static str {
    let r = reason.to_ascii_lowercase();
    let t = task.to_ascii_lowercase();
    if r.contains("cloudconflict") || (r.contains("cloud") && r.contains("conflict")) {
        "Steam needs an answer: your cloud save and this device's save differ"
    } else if r.contains("cloud") || t.contains("cloud") {
        "Steam needs an answer: it couldn't sync your cloud save"
    } else if r.contains("eula") || t.contains("eula") {
        "Steam needs an answer: the game's license agreement"
    } else if r.contains("option") || t.contains("option") {
        "Steam needs an answer: how to start the game"
    } else if ["update", "install", "download", "missing", "depot"].iter().any(|w| r.contains(w) || t.contains(w)) {
        "Steam needs an answer: the game needs an update first"
    } else {
        "Steam needs an answer before the game can start"
    }
}

/// Reads what was added to a file since the last look; starts at its end, and from the top again
/// when the file was replaced or cut (Steam rotates the log when it grows).
#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    id: Option<(u64, u64)>,
    pos: u64,
    partial: String,
}

/// At most this much is read per look.
const READ_MAX: u64 = 1 << 20;

fn file_id(m: &std::fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (m.dev(), m.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        (0, 0)
    }
}

impl Tail {
    pub fn new(path: PathBuf) -> Tail {
        let m = std::fs::metadata(&path).ok();
        Tail { id: m.as_ref().map(file_id), pos: m.map(|m| m.len()).unwrap_or(0), path, partial: String::new() }
    }

    pub fn read_new(&mut self) -> Vec<String> {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(m) = std::fs::metadata(&self.path) else { return Vec::new() };
        let id = file_id(&m);
        if self.id != Some(id) || m.len() < self.pos {
            self.id = Some(id);
            self.pos = 0;
            self.partial.clear();
        }
        if m.len() == self.pos {
            return Vec::new();
        }
        let Ok(mut f) = std::fs::File::open(&self.path) else { return Vec::new() };
        if m.len() - self.pos > READ_MAX {
            self.pos = m.len() - READ_MAX;
            self.partial.clear();
        }
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        let Ok(n) = f.take(READ_MAX).read_to_end(&mut buf) else { return Vec::new() };
        self.pos += n as u64;
        self.partial.push_str(&String::from_utf8_lossy(&buf));
        let mut lines: Vec<String> = self.partial.split('\n').map(str::to_string).collect();
        self.partial = lines.pop().unwrap_or_default();
        lines
    }
}

struct Watch {
    tail: Option<Tail>,
    tracker: Tracker,
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

fn log_path() -> Option<PathBuf> {
    Some(crate::steamcfg::primary_root()?.join("logs/console_log.txt"))
}

/// Reads Steam's new log lines (call often; cheap when nothing changed).
pub fn poll() {
    let mut w = WATCH.lock().unwrap_or_else(|e| e.into_inner());
    let w = w.get_or_insert_with(|| Watch { tail: None, tracker: Tracker::default() });
    if w.tail.is_none() {
        w.tail = log_path().map(Tail::new);
    }
    let Some(t) = w.tail.as_mut() else { return };
    let now = Instant::now();
    for l in t.read_new() {
        if let Some(line) = parse(&l) {
            if matches!(line, Line::Wait { .. }) {
                eprintln!("[steam] {}", l.trim());
            }
            w.tracker.apply(line, now);
        }
    }
}

/// What Steam asks the player before `appid` starts (see `Tracker::question`).
pub fn question(appid: u64) -> Option<&'static str> {
    WATCH.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.tracker.question(appid, Instant::now())
}

pub fn forget(appid: u64) {
    if let Some(w) = WATCH.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        w.tracker.forget(appid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HADES: &str = "[2026-10-27 23:21:35] GameAction [AppID 1145350, ActionID 1] : LaunchApp waiting for user response to SynchronizingCloud \"cloudconflict\"";

    #[test]
    fn reads_steams_launch_lines() {
        assert_eq!(parse(HADES), Some(Line::Wait { appid: 1145350, action: 1, task: "SynchronizingCloud".into(), reason: "cloudconflict".into() }));
        assert_eq!(
            parse("[2026-10-27 23:21:28] GameAction [AppID 1145350, ActionID 1] : LaunchApp changed task to ProcessingInstallScript with \"\""),
            Some(Line::Task { appid: 1145350, action: 1, task: "ProcessingInstallScript".into() })
        );
        assert_eq!(parse("[x] GameAction [AppID 620, ActionID 2] : LaunchApp continues with user response \"ShowInterstitials\""), Some(Line::Continue { appid: 620, action: 2 }));
        assert_eq!(parse("[x] GameAction [AppID 620, ActionID 2] : LaunchApp waiting for user response to CreatingProcess \"\""), Some(Line::Wait { appid: 620, action: 2, task: "CreatingProcess".into(), reason: "".into() }));
        assert_eq!(parse("[x] ExecuteSteamURL: \"steam://rungameid/1145350\""), None);
    }

    #[test]
    fn only_real_questions_count() {
        let t0 = Instant::now();
        let mut t = Tracker::default();
        // Steam answers these itself within the second, on every launch.
        t.apply(parse("GameAction [AppID 620, ActionID 1] : LaunchApp waiting for user response to ShowInterstitials \"\"").unwrap(), t0);
        assert_eq!(t.question(620, t0 + Duration::from_millis(500)), None);
        t.apply(parse("GameAction [AppID 620, ActionID 1] : LaunchApp continues with user response \"ShowInterstitials\"").unwrap(), t0);
        assert_eq!(t.question(620, t0 + Duration::from_secs(5)), None);
        // An empty-reason wait left open is the player's to answer.
        t.apply(parse("GameAction [AppID 620, ActionID 1] : LaunchApp waiting for user response to ShowEula \"\"").unwrap(), t0);
        assert_eq!(t.question(620, t0 + Duration::from_secs(3)), Some("Steam needs an answer: the game's license agreement"));
        // The cloud conflict is a question at once, until the next step.
        t.apply(parse(HADES).unwrap(), t0);
        assert_eq!(t.question(1145350, t0), Some("Steam needs an answer: your cloud save and this device's save differ"));
        t.apply(parse("GameAction [AppID 1145350, ActionID 1] : LaunchApp changed task to SynchronizingStats with \"\"").unwrap(), t0);
        assert_eq!(t.question(1145350, t0), None);
        // A new launch starts clean; a question past QUESTION_MAX is dropped.
        t.apply(parse(HADES).unwrap(), t0);
        assert!(t.question(1145350, t0 + QUESTION_MAX).is_none());
        t.apply(parse("GameAction [AppID 1145350, ActionID 2] : LaunchApp changed task to CheckShaderDepotManifest with \"\"").unwrap(), t0);
        assert_eq!(t.question(1145350, t0), None);
    }

    #[test]
    fn tail_reads_appended_lines_and_follows_rotation() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("console_log.txt");
        std::fs::write(&p, "old line\n").unwrap();
        let mut t = Tail::new(p.clone());
        assert!(t.read_new().is_empty());
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        write!(f, "{HADES}\nhalf").unwrap();
        assert_eq!(t.read_new(), vec![HADES.to_string()]);
        writeln!(f, " done").unwrap();
        assert_eq!(t.read_new(), vec!["half done".to_string()]);
        // Steam moved the log aside and started a new one.
        std::fs::rename(&p, dir.path().join("console_log.previous.txt")).unwrap();
        std::fs::write(&p, "fresh\n").unwrap();
        assert_eq!(t.read_new(), vec!["fresh".to_string()]);
    }
}
