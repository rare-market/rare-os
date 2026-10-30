//! What the player chose for their trophy case, kept in `~/.local/share/rare/trophies/case.json`:
//! the showcase (up to 3 trophies on the profile), trophies pinned to chain, and rare.market's last
//! answer on which unlocks count (trust) and the score it counted.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub const SHOWCASE_MAX: usize = 3;

/// A trophy on the showcase, with what the profile shows of it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    /// "steam:10180", "ra:1234", "epic:Hades", a library id for Rare's milestones.
    pub game: String,
    pub game_name: String,
    /// The trophy's key in that game ("ACH_WIN", "rare:h10").
    pub key: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub tier: u8,
    /// "steam" | "retroachievements" | "epic" | "gog" | "rare"
    pub source: String,
    /// Unix seconds.
    pub unlocked_at: Option<i64>,
}

/// rare.market's verdict on one unlock.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// "verified" | "unverified" | "flagged"
    pub t: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Case {
    #[serde(default)]
    pub showcase: Vec<Pin>,
    /// "game|key" -> the pin transaction's digest.
    #[serde(default)]
    pub chain: BTreeMap<String, String>,
    /// "game|key" -> verdict, from the last report.
    #[serde(default)]
    pub trust: BTreeMap<String, Verdict>,
    /// "game|key" -> share of Rare players with it (Rare's milestones).
    #[serde(default)]
    pub rarity: BTreeMap<String, f64>,
    /// The score rare.market counted, and when (unix seconds).
    #[serde(default)]
    pub score: Option<u64>,
    #[serde(default)]
    pub synced_at: Option<i64>,
    /// What each game's last report held (a hash), so unchanged games are not sent again.
    #[serde(default)]
    pub sent: BTreeMap<String, u64>,
    /// Why this device's reports are unverified, when rare.market said ("Secure Boot is off").
    #[serde(default)]
    pub device_note: Option<String>,
}

pub fn id(game: &str, key: &str) -> String {
    format!("{game}|{key}")
}

static CASE: Mutex<Option<Case>> = Mutex::new(None);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

fn path() -> PathBuf {
    PATH.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| rare_core::paths::data_dir().join("trophies/case.json"))
}

pub fn set_path(p: PathBuf) {
    *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    *CASE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn with<R>(f: impl FnOnce(&mut Case) -> R) -> R {
    let mut g = CASE.lock().unwrap_or_else(|e| e.into_inner());
    let c = g.get_or_insert_with(|| std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
    f(c)
}

pub fn save() {
    let bytes = with(|c| serde_json::to_vec(c).unwrap_or_default());
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// Adds a trophy to the showcase (the oldest goes when it is full) or takes it off. True when it
/// is on the showcase afterwards.
pub fn toggle_showcase(p: Pin) -> bool {
    let on = with(|c| {
        if let Some(i) = c.showcase.iter().position(|x| x.game == p.game && x.key == p.key) {
            c.showcase.remove(i);
            false
        } else {
            if c.showcase.len() >= SHOWCASE_MAX {
                c.showcase.remove(0);
            }
            c.showcase.push(p);
            true
        }
    });
    save();
    on
}

pub fn showcased(game: &str, key: &str) -> bool {
    with(|c| c.showcase.iter().any(|x| x.game == game && x.key == key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showcase_holds_three() {
        let d = std::env::temp_dir().join(format!("rare-case-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        set_path(d.join("case.json"));
        let pin = |k: &str| Pin { game: "steam:1".into(), key: k.into(), name: k.into(), ..Default::default() };
        for k in ["a", "b", "c", "d"] {
            assert!(toggle_showcase(pin(k)));
        }
        assert_eq!(with(|c| c.showcase.iter().map(|p| p.key.clone()).collect::<Vec<_>>()), ["b", "c", "d"]);
        assert!(!toggle_showcase(pin("c")));
        assert!(showcased("steam:1", "b") && !showcased("steam:1", "c"));
        *CASE.lock().unwrap() = None;
        assert_eq!(with(|c| c.showcase.len()), 2);
    }
}
