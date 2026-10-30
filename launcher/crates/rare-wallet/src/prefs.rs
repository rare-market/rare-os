//! The wallet's small per-device lists: recent recipients (ui/sign/recents.ts) and the web games
//! the player approved (ui/home/origins.ts). The web launcher keeps them in localStorage; the
//! native launcher in `<data dir>/rare/wallet/prefs.json` (same JSON shapes, written 0600).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suins: Option<String>,
    /// ms epoch
    pub at: u64,
}

/// Newest first, 20 at most, one entry per address (lowercased).
pub fn note_sent(list: &[Recent], address: &str, suins: Option<&str>, now_ms: u64) -> Vec<Recent> {
    let a = address.to_lowercase();
    std::iter::once(Recent { address: a.clone(), suins: suins.map(str::to_string), at: now_ms })
        .chain(list.iter().filter(|r| r.address != a).cloned())
        .take(20)
        .collect()
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "Today", "3d ago", "Mar 4" (UTC date).
pub fn since(ts_ms: u64, now_ms: u64) -> String {
    let days = now_ms.saturating_sub(ts_ms) / 86_400_000;
    if days < 1 {
        return "Today".into();
    }
    if days < 7 {
        return format!("{days}d ago");
    }
    // civil date from days since the epoch (Howard Hinnant's algorithm)
    let z = (ts_ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{} {d}", MONTHS[(m - 1) as usize])
}

/// Approved origins from the stored value: a list of strings or `{origin}` objects, or an object
/// keyed by origin (older builds).
pub fn approved_origins(raw: &Value) -> Vec<String> {
    let items: Vec<Value> = match raw {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.keys().map(|k| Value::String(k.clone())).collect(),
        _ => vec![],
    };
    items
        .iter()
        .filter_map(|x| match x {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => o.get("origin").and_then(Value::as_str).map(str::to_string),
            _ => None,
        })
        .collect()
}

/// "https://game.example" -> "game.example".
pub fn host_of(origin: &str) -> &str {
    origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://")).unwrap_or(origin)
}

/// The file both lists live in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    #[serde(default)]
    pub recents: Vec<Recent>,
    #[serde(default)]
    pub approved_origins: Value,
    /// auto-lock minutes (`copy::AutoLock`)
    #[serde(default)]
    pub autolock_min: Option<u32>,
    /// the old SuiPlay wallet last entered on the move page (MoveAssets remembers it)
    #[serde(default)]
    pub playtron_from: Option<String>,
}

pub struct PrefsFile(PathBuf);

impl PrefsFile {
    pub fn new(wallet_dir: PathBuf) -> Self {
        PrefsFile(wallet_dir.join("prefs.json"))
    }
    /// Missing or unreadable: empty (these are conveniences).
    pub fn load(&self) -> Prefs {
        std::fs::read(&self.0).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    pub fn save(&self, p: &Prefs) -> std::io::Result<()> {
        crate::vault::write_atomic(&self.0, &serde_json::to_vec(p).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recents_newest_first_unique_capped() {
        let mut l = vec![];
        for i in 0..25u64 {
            l = note_sent(&l, &format!("0xA{i}"), None, i);
        }
        assert_eq!(l.len(), 20);
        assert_eq!(l[0].address, "0xa24");
        l = note_sent(&l, "0xA10", Some("bob.sui"), 100);
        assert_eq!((l[0].address.as_str(), l[0].suins.as_deref(), l.len()), ("0xa10", Some("bob.sui"), 20));
        assert_eq!(l.iter().filter(|r| r.address == "0xa10").count(), 1);
    }

    #[test]
    fn since_words() {
        let day = 86_400_000;
        assert_eq!(since(10 * day, 10 * day + 5), "Today");
        assert_eq!(since(10 * day, 13 * day), "3d ago");
        // 2026-03-04
        assert_eq!(since(1_772_582_400_000, 1_772_582_400_000 + 30 * day), "Mar 4");
        assert_eq!(since(0, 8 * day), "Jan 1");
    }

    #[test]
    fn origins_in_every_stored_shape() {
        assert_eq!(approved_origins(&json!(["https://a.example", { "origin": "https://b.example" }, 5])), vec!["https://a.example", "https://b.example"]);
        assert_eq!(approved_origins(&json!({ "https://c.example": true })), vec!["https://c.example"]);
        assert!(approved_origins(&json!("x")).is_empty());
        assert_eq!(host_of("https://game.example"), "game.example");
    }

    #[test]
    fn prefs_file_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let f = PrefsFile::new(d.path().to_path_buf());
        assert_eq!(f.load(), Prefs::default());
        let p = Prefs { recents: note_sent(&[], "0xAB", None, 1), approved_origins: json!(["https://g.example"]), autolock_min: Some(5), playtron_from: None };
        f.save(&p).unwrap();
        assert_eq!(f.load(), p);
    }
}
