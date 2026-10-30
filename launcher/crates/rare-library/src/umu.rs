//! Games Rare runs through umu-run (Epic through legendary, GOG and other Windows builds): their
//! Wine prefix and their Proton.
//!
//! Prefix: Rare's own (`~/.local/share/rare/prefixes/<id>`), unless the game came from Heroic and
//! only Heroic's prefix exists (its GamesConfig `winePrefix`): then that one, so the saves made
//! under Heroic carry on.
//!
//! Proton: the player's per-game pick (`~/.config/rare/umu-proton.json`, by game id and Steam's
//! tool name, the names the game page's Proton row shows), else Valve's newest Proton when Steam
//! has one, else umu's own default. `protonpath` is what goes in PROTONPATH.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(p).ok()?).ok()
}

/// Rare's own prefix for a game.
pub fn rare_prefix(id: &str) -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share")).join("rare/prefixes").join(id.replace(':', "-"))
}

/// The prefix Heroic keeps for a game (`heroic/GamesConfig/<app>.json` winePrefix), when it exists.
pub fn heroic_prefix(app: &str) -> Option<PathBuf> {
    for c in crate::heroic::config_dirs() {
        let Some(v) = read_json(&c.join(format!("heroic/GamesConfig/{app}.json"))) else { continue };
        if let Some(p) = v.get(app).and_then(|g| g.get("winePrefix")).and_then(Value::as_str).filter(|p| !p.is_empty()) {
            let p = PathBuf::from(p.replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy()));
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

/// The prefix a game runs in (see the module notes).
pub fn prefix(id: &str) -> PathBuf {
    let own = rare_prefix(id);
    if own.join("drive_c").is_dir() || own.join("pfx/drive_c").is_dir() {
        return own;
    }
    let app = id.split_once(':').map(|(_, a)| a).unwrap_or(id);
    heroic_prefix(app).unwrap_or(own)
}

/// Where drive_c is: the prefix itself (umu, Wine) or its `pfx` (a prefix Proton made directly).
pub fn wine_root(prefix: &Path) -> PathBuf {
    if !prefix.join("drive_c").is_dir() && prefix.join("pfx/drive_c").is_dir() {
        prefix.join("pfx")
    } else {
        prefix.to_path_buf()
    }
}

fn choices_path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("rare/umu-proton.json")
}

/// The Proton the player picked for this game (Steam's tool name), if any.
pub fn choice(id: &str) -> Option<String> {
    read_json(&choices_path())?.get(id)?.as_str().map(String::from)
}

/// Picks one (None: back to the default).
pub fn set_choice(id: &str, tool: Option<&str>) -> Result<(), String> {
    if let Some(t) = tool {
        if tool_path(t).is_none() {
            return Err(format!("{t} is not installed."));
        }
    }
    let path = choices_path();
    let mut v = read_json(&path).unwrap_or_else(|| json!({}));
    if let Some(o) = v.as_object_mut() {
        match tool {
            Some(t) => {
                o.insert(id.to_string(), json!(t));
            }
            None => {
                o.remove(id);
            }
        }
    }
    if let Some(d) = path.parent() {
        let _ = fs::create_dir_all(d);
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn steam_root() -> Option<PathBuf> {
    let roots = crate::steam::find_roots();
    roots.iter().find(|r| r.primary).or(roots.first()).map(|r| r.path.clone())
}

/// Valve's Protons in the Steam libraries: (major, minor, folder).
fn valve(libs: &[PathBuf]) -> Vec<(u32, u32, PathBuf)> {
    let mut out = Vec::new();
    for lib in libs {
        let Ok(rd) = fs::read_dir(lib.join("steamapps/common")) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let Some((a, b)) = n.strip_prefix("Proton ").and_then(|v| v.split_once('.')) else { continue };
            let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else { continue };
            if e.path().join("proton").is_file() {
                out.push((a, b, e.path()));
            }
        }
    }
    out.sort();
    out
}

/// The folder of a Proton by Steam's tool name: `proton_N` (Valve's newest N.x),
/// `proton_experimental`, or a tool in compatibilitytools.d (by its compatibilitytool.vdf name).
pub fn tool_path(name: &str) -> Option<PathBuf> {
    let root = steam_root()?;
    let libs = crate::steam::registered_libraries(&root);
    if let Some(major) = name.strip_prefix("proton_").and_then(|m| m.parse::<u32>().ok()) {
        return valve(&libs).into_iter().rev().find(|(a, _, _)| *a == major).map(|(_, _, p)| p);
    }
    if name == "proton_experimental" {
        return libs.iter().map(|l| l.join("steamapps/common/Proton - Experimental")).find(|p| p.join("proton").is_file());
    }
    let rd = fs::read_dir(root.join("compatibilitytools.d")).ok()?;
    for e in rd.flatten() {
        let dir = e.path();
        let Ok(text) = fs::read_to_string(dir.join("compatibilitytool.vdf")) else { continue };
        if (e.file_name().to_string_lossy() == name || text.contains(&format!("\"{name}\""))) && dir.join("proton").is_file() {
            return Some(dir);
        }
    }
    None
}

/// The default: Valve's newest Proton (Steam's tool name), when Steam has one.
pub fn default_tool() -> Option<String> {
    let root = steam_root()?;
    valve(&crate::steam::registered_libraries(&root)).last().map(|(a, _, _)| format!("proton_{a}"))
}

/// PROTONPATH for a game: its pick, else the default; None leaves umu to its own default.
pub fn protonpath(id: &str) -> Option<PathBuf> {
    choice(id).and_then(|t| tool_path(&t)).or_else(|| default_tool().and_then(|t| tool_path(&t)))
}

/// The environment umu-run needs for a game (prefix, Proton, store).
pub fn env(id: &str) -> Vec<(String, String)> {
    let mut v = vec![("WINEPREFIX".to_string(), prefix(id).to_string_lossy().into_owned()), ("GAMEID".to_string(), "umu-default".to_string())];
    let store = if id.starts_with("gog:") {
        "gog"
    } else if id.starts_with("epic:") {
        "egs"
    } else {
        "none"
    };
    v.push(("STORE".to_string(), store.to_string()));
    if let Some(p) = protonpath(id) {
        v.push(("PROTONPATH".to_string(), p.to_string_lossy().into_owned()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wine_root_follows_pfx() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a/pfx/drive_c")).unwrap();
        fs::create_dir_all(t.path().join("b/drive_c")).unwrap();
        assert_eq!(wine_root(&t.path().join("a")), t.path().join("a/pfx"));
        assert_eq!(wine_root(&t.path().join("b")), t.path().join("b"));
    }
}
