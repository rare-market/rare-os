//! Rare's own trophies for every game, achievements or not: milestones Rare can see happen. First
//! launch, 1, 10, 50 and 100 hours, a 7-day streak, played on a SuiPlay, and completed (the
//! source's achievements all unlocked, or the player marks it complete). Nothing game-specific is
//! made up: each one is a fact from Rare's play log or the store's own playtime.
//!
//! The play log (`~/.local/share/rare/trophies/plays.json`) gets the seconds and the days each
//! game runs while Rare is up. Hours use the larger of that and the store's playtime (Steam keeps
//! one), so time played before Rare counts too.

use crate::score::Tier;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    First,
    Hours1,
    Hours10,
    Hours50,
    Hours100,
    Streak7,
    SuiPlay,
    Complete,
}

pub const KINDS: [Kind; 8] = [Kind::First, Kind::Hours1, Kind::Hours10, Kind::Hours50, Kind::Hours100, Kind::Streak7, Kind::SuiPlay, Kind::Complete];

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::First => "first",
            Kind::Hours1 => "h1",
            Kind::Hours10 => "h10",
            Kind::Hours50 => "h50",
            Kind::Hours100 => "h100",
            Kind::Streak7 => "streak7",
            Kind::SuiPlay => "suiplay",
            Kind::Complete => "complete",
        }
    }

    pub fn from_key(k: &str) -> Option<Kind> {
        KINDS.into_iter().find(|x| x.key() == k.strip_prefix("rare:").unwrap_or(k))
    }

    /// Fixed tiers: there is no global rarity for these yet.
    pub fn tier(self) -> Tier {
        match self {
            Kind::First | Kind::Hours1 | Kind::SuiPlay => 0,
            Kind::Hours10 | Kind::Streak7 => 1,
            Kind::Hours50 | Kind::Complete => 2,
            Kind::Hours100 => 3,
        }
    }

    pub fn hours(self) -> Option<u64> {
        match self {
            Kind::Hours1 => Some(1),
            Kind::Hours10 => Some(10),
            Kind::Hours50 => Some(50),
            Kind::Hours100 => Some(100),
            _ => None,
        }
    }
}

/// A name and a short line, written for one game.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub name: String,
    pub description: String,
}

/// The plain names, used until rare.market's set for the game arrives (or when it can't).
pub fn template(kind: Kind, title: &str) -> Text {
    let t = if title.trim().is_empty() { "this game" } else { title.trim() };
    let (name, description) = match kind {
        Kind::First => ("First launch".to_string(), format!("Started {t} for the first time.")),
        Kind::Hours1 => ("One hour in".to_string(), format!("Played {t} for an hour.")),
        Kind::Hours10 => ("Ten hours".to_string(), format!("Played {t} for 10 hours.")),
        Kind::Hours50 => ("Fifty hours".to_string(), format!("Played {t} for 50 hours.")),
        Kind::Hours100 => ("One hundred hours".to_string(), format!("Played {t} for 100 hours.")),
        Kind::Streak7 => ("Seven days running".to_string(), format!("Played {t} seven days in a row.")),
        Kind::SuiPlay => ("Played on SuiPlay".to_string(), format!("Played {t} on a SuiPlay0X1.")),
        Kind::Complete => ("Completed".to_string(), format!("Finished {t}.")),
    };
    Text { name, description }
}

/// How a game counts as complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    /// Every achievement the source lists is unlocked.
    Source,
    /// The player marked it complete.
    Marked,
}

/// What Rare knows about one game's play.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub launched: bool,
    /// Unix seconds of the first launch Rare knows of.
    pub first_at: Option<i64>,
    pub minutes: u64,
    pub best_streak: u32,
    pub on_suiplay: bool,
    pub done: Option<Done>,
}

/// One milestone for one game.
#[derive(Clone, Debug, PartialEq)]
pub struct Milestone {
    pub kind: Kind,
    /// "rare:h10"
    pub key: String,
    pub text: Text,
    pub tier: Tier,
    pub unlocked: bool,
    /// Unix seconds.
    pub unlocked_at: Option<i64>,
    /// (have, need), for the ones that count up.
    pub progress: Option<(u64, u64)>,
    pub done: Option<Done>,
}

fn met(kind: Kind, f: &Facts) -> bool {
    match kind {
        Kind::First => f.launched || f.minutes > 0,
        Kind::Streak7 => f.best_streak >= 7,
        Kind::SuiPlay => f.on_suiplay,
        Kind::Complete => f.done.is_some(),
        k => f.minutes >= k.hours().unwrap_or(0) * 60,
    }
}

fn progress(kind: Kind, f: &Facts) -> Option<(u64, u64)> {
    match kind {
        Kind::Streak7 => Some((f.best_streak.min(7) as u64, 7)),
        k => k.hours().map(|h| ((f.minutes / 60).min(h), h)),
    }
}

/// The longest run of consecutive days in `days` (day numbers).
pub fn best_streak(days: &BTreeSet<i64>) -> u32 {
    let (mut best, mut run, mut prev) = (0u32, 0u32, None::<i64>);
    for d in days {
        run = if prev == Some(d - 1) { run + 1 } else { 1 };
        best = best.max(run);
        prev = Some(*d);
    }
    best
}

// ---------------------------------------------------------------- the play log

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Play {
    /// Unix seconds of the first session Rare saw.
    #[serde(default)]
    pub first: i64,
    #[serde(default)]
    pub secs: u64,
    /// UTC day numbers it ran on (the last 400).
    #[serde(default)]
    pub days: BTreeSet<i64>,
    #[serde(default)]
    pub suiplay: bool,
    /// Play sessions as (start, end) unix seconds, the last 300 (rare.market checks unlock times
    /// against them).
    #[serde(default)]
    pub sessions: Vec<(i64, i64)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Log {
    #[serde(default)]
    pub games: BTreeMap<String, Play>,
    /// game id -> milestone key -> unix seconds unlocked. Once there, a milestone stays.
    #[serde(default)]
    pub unlocked: BTreeMap<String, BTreeMap<String, i64>>,
    /// Games the player marked complete (unix seconds).
    #[serde(default)]
    pub marked: BTreeMap<String, i64>,
    /// Games whose milestones were looked at once (later unlocks are new and pop up).
    #[serde(default)]
    pub seen: BTreeSet<String>,
    /// rare.market's names per game, kept once fetched: game id -> milestone key -> text.
    #[serde(default)]
    pub texts: BTreeMap<String, BTreeMap<String, Text>>,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

fn path() -> PathBuf {
    PATH.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| rare_core::paths::data_dir().join("trophies/plays.json"))
}

/// Tests and fixtures keep their log elsewhere.
pub fn set_path(p: PathBuf) {
    *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn with_log<R>(f: impl FnOnce(&mut Log) -> R) -> R {
    let mut g = LOG.lock().unwrap_or_else(|e| e.into_inner());
    let log = g.get_or_insert_with(|| std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
    f(log)
}

pub fn save() {
    let bytes = with_log(|l| serde_json::to_vec(l).unwrap_or_default());
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

pub fn on_suiplay() -> bool {
    std::fs::read_to_string("/sys/class/dmi/id/product_name").map(|s| s.trim() == "SuiPlay0X1").unwrap_or(false)
}

/// Adds `secs` of play to each running game at `now` (unix seconds).
pub fn credit(ids: &[String], secs: u64, now: i64, suiplay: bool) {
    if ids.is_empty() || secs == 0 {
        return;
    }
    with_log(|l| {
        for id in ids {
            let p = l.games.entry(id.clone()).or_default();
            if p.first == 0 {
                p.first = now - secs as i64;
            }
            p.secs += secs;
            p.days.insert(now.div_euclid(86_400));
            while p.days.len() > 400 {
                let first = *p.days.iter().next().unwrap();
                p.days.remove(&first);
            }
            p.suiplay |= suiplay;
            let start = now - secs as i64;
            match p.sessions.last_mut() {
                Some(last) if last.1 >= start - 90 => last.1 = now,
                _ => p.sessions.push((start, now)),
            }
            if p.sessions.len() > 300 {
                p.sessions.remove(0);
            }
        }
    });
}

pub fn set_marked(id: &str, on: bool, now: i64) {
    with_log(|l| {
        if on {
            l.marked.insert(id.to_string(), now);
        } else {
            l.marked.remove(id);
            if let Some(u) = l.unlocked.get_mut(id) {
                u.remove(Kind::Complete.key());
            }
        }
    });
    save();
}

pub fn marked(id: &str) -> bool {
    with_log(|l| l.marked.contains_key(id))
}

pub fn set_texts(id: &str, texts: BTreeMap<String, Text>) {
    with_log(|l| {
        l.texts.insert(id.to_string(), texts);
    });
    save();
}

pub fn has_texts(id: &str) -> bool {
    with_log(|l| l.texts.contains_key(id))
}

/// What Rare knows of a game: its log, the store's playtime (minutes) and last played (unix
/// seconds), and whether the source has every achievement unlocked.
pub fn facts(id: &str, store_minutes: Option<u64>, last_played: Option<u64>, source_complete: bool) -> Facts {
    with_log(|l| {
        let p = l.games.get(id);
        let lp = last_played.filter(|t| *t > 0).map(|t| t as i64);
        let first = p.map(|p| p.first).filter(|t| *t > 0);
        Facts {
            launched: p.is_some_and(|p| p.secs > 0) || lp.is_some() || store_minutes.unwrap_or(0) > 0,
            first_at: first.or(lp),
            minutes: (p.map(|p| p.secs / 60).unwrap_or(0)).max(store_minutes.unwrap_or(0)),
            best_streak: p.map(|p| best_streak(&p.days)).unwrap_or(0),
            on_suiplay: p.is_some_and(|p| p.suiplay && p.secs > 0),
            done: if source_complete {
                Some(Done::Source)
            } else if l.marked.contains_key(id) {
                Some(Done::Marked)
            } else {
                None
            },
        }
    })
}

/// The game's milestones at `now`, and the ones that unlocked just now (empty the first time a
/// game is looked at: what is already true is learned, not announced).
pub fn evaluate(id: &str, title: &str, f: &Facts, now: i64) -> (Vec<Milestone>, Vec<Milestone>) {
    let (list, fresh, dirty) = with_log(|l| {
        let first_look = !l.seen.contains(id);
        let mut dirty = first_look;
        l.seen.insert(id.to_string());
        let texts = l.texts.get(id).cloned().unwrap_or_default();
        let got = l.unlocked.entry(id.to_string()).or_default();
        let mut list = Vec::new();
        let mut fresh = Vec::new();
        for kind in KINDS {
            let k = kind.key();
            let is = met(kind, f);
            if is && !got.contains_key(k) {
                let at = match kind {
                    Kind::First => f.first_at.unwrap_or(now),
                    _ => now,
                };
                got.insert(k.to_string(), at.min(now));
                dirty = true;
                if !first_look {
                    fresh.push(kind);
                }
            }
            // Un-marking takes Completed back; everything else stays once earned.
            if kind == Kind::Complete && !is && got.remove(k).is_some() {
                dirty = true;
            }
            let at = got.get(k).copied();
            let text = texts.get(k).cloned().filter(|t| !t.name.is_empty()).unwrap_or_else(|| template(kind, title));
            list.push(Milestone { kind, key: format!("rare:{k}"), text, tier: kind.tier(), unlocked: at.is_some(), unlocked_at: at, progress: progress(kind, f), done: if kind == Kind::Complete { f.done } else { None } });
        }
        let fresh = list.iter().filter(|m| fresh.contains(&m.kind)).cloned().collect::<Vec<_>>();
        (list, fresh, dirty)
    });
    if dirty {
        save();
    }
    (list, fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_log(name: &str) {
        let d = std::env::temp_dir().join(format!("rare-trophies-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        set_path(d.join("plays.json"));
    }

    #[test]
    fn streaks() {
        let s: BTreeSet<i64> = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 20].into_iter().collect();
        assert_eq!(best_streak(&s), 7);
        assert_eq!(best_streak(&BTreeSet::new()), 0);
    }

    // One test owns the process-wide log (set_path), so the steps run in order.
    #[test]
    fn log_and_milestones() {
        fresh_log("ms");
        let id = "steam:10180";
        // Steam says 12 hours before Rare ever saw it: learned quietly.
        let f = facts(id, Some(12 * 60), Some(1_700_000_000), false);
        assert!(f.launched && f.minutes == 720 && f.done.is_none());
        let (list, fresh) = evaluate(id, "Modern Warfare 2", &f, 1_800_000_000);
        assert!(fresh.is_empty());
        let on: Vec<&str> = list.iter().filter(|m| m.unlocked).map(|m| m.key.as_str()).collect();
        assert_eq!(on, ["rare:first", "rare:h1", "rare:h10"]);
        assert_eq!(list[0].unlocked_at, Some(1_700_000_000));
        assert_eq!(list[3].progress, Some((12, 50)));
        assert_eq!(list[1].text.description, "Played Modern Warfare 2 for an hour.");
        // Seven days of play on the SuiPlay, 40 more hours: three new ones pop up.
        for d in 0..7 {
            credit(&[id.to_string()], 6 * 3600, 1_800_000_000 + d * 86_400, true);
        }
        let f = facts(id, Some(12 * 60), None, false);
        assert_eq!((f.best_streak, f.on_suiplay, f.minutes), (7, true, 42 * 60));
        assert_eq!(with_log(|l| l.games[id].sessions.clone()), (0..7).map(|d| (1_800_000_000 + d * 86_400 - 6 * 3600, 1_800_000_000 + d * 86_400)).collect::<Vec<_>>());
        credit(&[id.to_string()], 30, 1_800_000_000 + 6 * 86_400 + 30, true);
        assert_eq!(with_log(|l| l.games[id].sessions.len()), 7);
        let (_, fresh) = evaluate(id, "Modern Warfare 2", &f, 1_800_700_000);
        assert_eq!(fresh.iter().map(|m| m.kind).collect::<Vec<_>>(), [Kind::Streak7, Kind::SuiPlay]);
        // Marked complete, then not: Completed comes and goes.
        set_marked(id, true, 1_800_800_000);
        let (_, fresh) = evaluate(id, "Modern Warfare 2", &facts(id, None, None, false), 1_800_800_000);
        assert_eq!(fresh[0].kind, Kind::Complete);
        assert_eq!(fresh[0].done, Some(Done::Marked));
        set_marked(id, false, 0);
        let (list, _) = evaluate(id, "Modern Warfare 2", &facts(id, None, None, false), 1_800_900_000);
        assert!(!list.iter().find(|m| m.kind == Kind::Complete).unwrap().unlocked);
        // rare.market's names replace the template.
        let mut t = BTreeMap::new();
        t.insert("h1".to_string(), Text { name: "Boots on the ground".into(), description: "An hour in Modern Warfare 2.".into() });
        set_texts(id, t);
        let (list, _) = evaluate(id, "Modern Warfare 2", &facts(id, None, None, false), 1_800_900_000);
        assert_eq!(list[1].text.name, "Boots on the ground");
        assert_eq!(list[0].text.name, "First launch");
        // Persisted: a new process reads the same log.
        *LOG.lock().unwrap() = None;
        assert!(with_log(|l| l.unlocked[id].contains_key("streak7")));
        // A game Rare never saw has nothing.
        let (list, fresh) = evaluate("epic:fn", "Fortnite", &facts("epic:fn", None, None, false), 1);
        assert!(fresh.is_empty() && list.iter().all(|m| !m.unlocked));
    }
}
