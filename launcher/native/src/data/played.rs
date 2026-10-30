//! Rare's own play record: every game Rare launches gets its sessions counted here, so local,
//! itch, web, shortcut and Rare Store games have a playtime and a last played time like Steam and
//! Heroic games do. Kept in ~/.local/state/rare-native/played.json.
//!
//! A session starts when a run id shows up in `running` and ends when it goes. While one is open
//! the file is touched once a minute (`seen`), so a session the launcher never saw end (it was
//! restarted, the device lost power) is closed at the last minute it was seen.

use super::model::Library;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How often an open session is written down.
const HEARTBEAT: i64 = 60;

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Rec {
    /// Seconds played in finished sessions.
    #[serde(default)]
    pub secs: i64,
    /// When the last session started (unix seconds).
    #[serde(default)]
    pub last: i64,
    #[serde(default)]
    pub sessions: u32,
    /// The open session's start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<i64>,
    /// The open session was last seen running at.
    #[serde(default)]
    pub seen: i64,
}

impl Rec {
    /// Minutes, the open session included up to `now`.
    pub fn minutes(&self, now: i64) -> i64 {
        let open = self.open.map(|s| (now.max(self.seen) - s).max(0)).unwrap_or(0);
        (self.secs + open) / 60
    }
}

/// One finished session (features/stats.rs): the game and when it ran (unix secs).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    pub s: i64,
    pub e: i64,
}

fn log_path() -> std::path::PathBuf {
    super::userlib::state_dir().join("sessions.jsonl")
}

/// Every finished session Rare saw (a line each in sessions.jsonl), oldest first.
pub fn sessions() -> Vec<Session> {
    if std::env::var("RR_FIXTURE").is_ok() {
        return Vec::new();
    }
    std::fs::read_to_string(log_path()).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[derive(Default)]
pub struct Played {
    pub recs: BTreeMap<String, Rec>,
    persist: bool,
    /// The first `note_running` after boot closes sessions left open at their `seen`.
    booted: bool,
    beat: i64,
}

/// Run ids that are not library games.
fn counted(id: &str) -> bool {
    !(id.starts_with("app:") || id.starts_with("stream:") || id.is_empty())
}

fn path() -> std::path::PathBuf {
    super::userlib::state_dir().join("played.json")
}

impl Played {
    pub fn load() -> Played {
        if std::env::var("RR_FIXTURE").is_ok() {
            return Played::default();
        }
        let recs = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Played { recs, persist: true, booted: false, beat: 0 }
    }

    fn save(&self) {
        if self.persist {
            super::userlib::write_json(&path(), &self.recs);
        }
    }

    /// The run ids changed: open sessions for new ones, close the ones that went. True when a
    /// session closed (the library's numbers moved).
    pub fn note_running(&mut self, running: &[String], now: i64) -> bool {
        let boot = !self.booted;
        self.booted = true;
        let mut changed = false;
        let mut closed: Vec<Session> = Vec::new();
        for (id, r) in self.recs.iter_mut() {
            let Some(start) = r.open else { continue };
            if running.iter().any(|x| x == id) {
                continue;
            }
            // Gone. At boot it went while the launcher was away: count up to when it was last seen.
            let end = if boot { r.seen.max(start) } else { now };
            r.secs += (end - start).max(0);
            r.open = None;
            r.seen = end;
            changed = true;
            if end - start >= 60 {
                closed.push(Session { id: id.clone(), s: start, e: end });
            }
        }
        if self.persist && !closed.is_empty() {
            use std::io::Write;
            let p = log_path();
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                for c in &closed {
                    let _ = writeln!(f, "{}", serde_json::to_string(c).unwrap_or_default());
                }
            }
        }
        for id in running.iter().filter(|id| counted(id)) {
            let r = self.recs.entry(id.clone()).or_default();
            if r.open.is_none() {
                r.open = Some(now);
                r.seen = now;
                r.last = now;
                r.sessions += 1;
                changed = true;
            }
        }
        if changed {
            self.beat = now;
            self.save();
        }
        changed
    }

    /// Once a minute while something runs: remember it was still running.
    pub fn heartbeat(&mut self, now: i64) {
        if now - self.beat < HEARTBEAT {
            return;
        }
        self.beat = now;
        let mut any = false;
        for r in self.recs.values_mut().filter(|r| r.open.is_some()) {
            r.seen = now;
            any = true;
        }
        if any {
            self.save();
        }
    }

    /// Sessions still running, up to `now` (for the stats).
    pub fn open_sessions(&self, now: i64) -> Vec<Session> {
        self.recs.iter().filter_map(|(id, r)| r.open.map(|s| Session { id: id.clone(), s, e: now.max(s) })).collect()
    }

    /// Puts Rare's numbers into the scanned library: the later last played, and the larger
    /// playtime (Steam and Heroic count Rare's sessions themselves, so the two never add up).
    pub fn apply(&self, lib: &mut Library, now: i64) {
        for g in lib.games.iter_mut().chain(lib.cloud.iter_mut()) {
            let Some(r) = self.recs.get(&g.id) else { continue };
            if r.last > 0 {
                g.last_played = Some(g.last_played.unwrap_or(0).max(r.last));
            }
            let m = r.minutes(now);
            if m > 0 {
                g.playtime_minutes = Some(g.playtime_minutes.unwrap_or(0).max(m));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::model::Game;

    fn game(id: &str, last: Option<i64>, mins: Option<i64>) -> Game {
        serde_json::from_value(serde_json::json!({"id": id, "source": "local", "title": id, "lastPlayed": last, "playtimeMinutes": mins})).unwrap()
    }

    #[test]
    fn sessions_count_and_merge() {
        let mut p = Played::default();
        p.note_running(&[], 0);
        assert!(p.note_running(&["local:a".into(), "app:chrome".into()], 1000));
        assert!(!p.recs.contains_key("app:chrome"));
        assert!(p.note_running(&[], 1000 + 3600));
        let r = &p.recs["local:a"];
        assert_eq!((r.secs, r.last, r.sessions, r.open), (3600, 1000, 1, None));
        let mut lib = Library { games: vec![game("local:a", None, None), game("steam:1", Some(5), Some(600))], ..Default::default() };
        p.note_running(&["steam:1".into()], 9000);
        p.note_running(&[], 9000 + 120);
        p.apply(&mut lib, 10_000);
        assert_eq!((lib.games[0].last_played, lib.games[0].playtime_minutes), (Some(1000), Some(60)));
        // Steam's own count already holds Rare's two minutes.
        assert_eq!((lib.games[1].last_played, lib.games[1].playtime_minutes), (Some(9000), Some(600)));
    }

    #[test]
    fn a_session_left_open_closes_at_its_last_heartbeat() {
        let mut p = Played::default();
        p.recs.insert("local:a".into(), Rec { secs: 0, last: 100, sessions: 1, open: Some(100), seen: 400 });
        p.note_running(&[], 99_999);
        assert_eq!(p.recs["local:a"].secs, 300);
        // Still running after a launcher restart: the session goes on.
        let mut q = Played::default();
        q.recs.insert("local:b".into(), Rec { secs: 0, last: 100, sessions: 1, open: Some(100), seen: 400 });
        q.note_running(&["local:b".into()], 500);
        assert_eq!(q.recs["local:b"].open, Some(100));
        assert_eq!(q.recs["local:b"].minutes(700), 10);
    }
}
