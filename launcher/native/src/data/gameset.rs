//! Per-game settings the game page sets: launch arguments and environment for games Rare starts
//! itself (Steam games keep theirs in Steam's own launch options), and the LC and RC buttons for
//! that game on top of Settings > Controls. Kept in ~/.local/state/rare-native/games.json.
//! Shared by the UI thread, the launcher (rare-launch's launch hook) and the side buttons.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct GameSet {
    /// Extra arguments, as typed (split like a shell does).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub args: String,
    /// Environment, as typed: `NAME=value` pairs separated by spaces.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub env: String,
    /// LC and RC for this game (Settings > Controls' action keys); None follows Settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rc: Option<String>,
}

impl GameSet {
    fn is_default(&self) -> bool {
        *self == GameSet::default()
    }
}

static STORE: Mutex<Option<BTreeMap<String, GameSet>>> = Mutex::new(None);
static VER: AtomicU64 = AtomicU64::new(0);

fn path() -> std::path::PathBuf {
    super::userlib::state_dir().join("games.json")
}

fn persist() -> bool {
    !cfg!(test) && std::env::var("RR_FIXTURE").is_err()
}

fn with<R>(f: impl FnOnce(&mut BTreeMap<String, GameSet>) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(|| if persist() { super::userlib::read_json(&path()) } else { BTreeMap::new() });
    f(m)
}

/// Moves whenever a game's settings change (the game page's stamp).
pub fn version() -> u64 {
    VER.load(Ordering::Relaxed)
}

pub fn get(id: &str) -> GameSet {
    with(|m| m.get(id).cloned().unwrap_or_default())
}

/// Changes one game's settings and saves them.
pub fn update(id: &str, f: impl FnOnce(&mut GameSet)) {
    with(|m| {
        let mut s = m.get(id).cloned().unwrap_or_default();
        f(&mut s);
        if s.is_default() {
            m.remove(id);
        } else {
            m.insert(id.to_string(), s);
        }
        if persist() {
            super::userlib::write_json(&path(), &*m);
        }
    });
    VER.fetch_add(1, Ordering::Relaxed);
}

/// Splits like a shell: spaces separate, quotes group, a backslash escapes the next character.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut any = false;
    let mut quote: Option<char> = None;
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (_, '\\') => {
                if let Some(n) = it.next() {
                    cur.push(n);
                    any = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (_, c) => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// `NAME=value NAME2="two words"` into pairs; Err names the first piece that is not one.
pub fn parse_env(s: &str) -> Result<Vec<(String, String)>, String> {
    split_args(s)
        .into_iter()
        .map(|w| match w.split_once('=') {
            Some((k, v)) if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.starts_with(|c: char| c.is_ascii_digit()) => Ok((k.to_string(), v.to_string())),
            _ => Err(w),
        })
        .collect()
}

/// rare-launch's hook: the arguments and environment for a game Rare starts.
pub fn launch_extra(id: &str) -> Option<(Vec<String>, Vec<(String, String)>)> {
    let s = get(id);
    if s.args.trim().is_empty() && s.env.trim().is_empty() {
        return None;
    }
    Some((split_args(&s.args), parse_env(&s.env).unwrap_or_default()))
}

/// LC or RC for the game running now, when one of the running games sets it.
pub fn side_action(running: &[String], lc: bool) -> Option<String> {
    running.iter().find_map(|id| {
        let s = get(id);
        if lc {
            s.lc
        } else {
            s.rc
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_and_env() {
        assert_eq!(split_args(r#"-dx11  --name "Rare Player" it\'s ''"#), vec!["-dx11", "--name", "Rare Player", "it's", ""]);
        assert_eq!(parse_env("DXVK_HUD=1 WINEDLLOVERRIDES=\"dinput8=n,b\"").unwrap(), vec![("DXVK_HUD".into(), "1".into()), ("WINEDLLOVERRIDES".into(), "dinput8=n,b".into())]);
        assert_eq!(parse_env("1X=2"), Err("1X=2".into()));
        assert_eq!(parse_env("-dx11"), Err("-dx11".into()));
        update("local:t", |s| s.args = "-windowed".into());
        assert_eq!(launch_extra("local:t"), Some((vec!["-windowed".into()], vec![])));
        update("local:t", |s| s.lc = Some("keyboard".into()));
        assert_eq!(side_action(&["steam:9".into(), "local:t".into()], true).as_deref(), Some("keyboard"));
        assert_eq!(side_action(&["local:t".into()], false), None);
        update("local:t", |s| *s = GameSet::default());
        assert_eq!(get("local:t"), GameSet::default());
    }
}
