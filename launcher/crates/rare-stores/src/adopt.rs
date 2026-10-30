//! Games Heroic installed become Rare's: their entries move into the lists legendary and gogdl
//! use here, so they get Update, Check files, cloud saves and Uninstall like any other, and start
//! through legendary or umu-run instead of Heroic. The game folders stay where they are, and so
//! does Heroic's prefix (rare-library umu::prefix keeps using it, so saves carry on). A Heroic
//! sign-in is copied when Rare has none.
//!
//! Epic: `heroic/legendaryConfig/legendary/{installed.json, metadata, manifests}` into legendary's
//! own folder. GOG: a Flatpak Heroic's `gog_store/installed.json` into `~/.config/heroic/gog_store`
//! (the native Heroic's is already the one Rare uses). Only this user's folders are touched.

use crate::{read_json, write_json};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Heroic's config folders in this home (native, Flatpak).
fn heroic_configs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    rare_library::heroic::config_dirs().into_iter().filter(|c| c.starts_with(&home)).map(|c| c.join("heroic")).filter(|c| c.is_dir()).collect()
}

fn copy_if_missing(from: &Path, to: &Path) {
    if from.is_file() && !to.exists() {
        if let Some(d) = to.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::copy(from, to);
    }
}

/// Moves Heroic's Epic installs into `legendary` (legendary's folder). Returns the apps moved.
pub fn epic_into(heroic: &Path, legendary: &Path) -> Vec<String> {
    let src_dir = heroic.join("legendaryConfig/legendary");
    let src_file = src_dir.join("installed.json");
    let Some(Value::Object(mut src)) = read_json(&src_file) else { return vec![] };
    if src.is_empty() {
        return vec![];
    }
    let dst_file = legendary.join("installed.json");
    let mut dst: Map<String, Value> = match read_json(&dst_file) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let mut moved = Vec::new();
    for (app, entry) in src.clone() {
        let dir = entry.get("install_path").and_then(Value::as_str).unwrap_or("");
        if dir.is_empty() || !Path::new(dir).is_dir() {
            continue;
        }
        copy_if_missing(&src_dir.join(format!("metadata/{app}.json")), &legendary.join(format!("metadata/{app}.json")));
        copy_if_missing(&src_dir.join(format!("manifests/{app}.manifest")), &legendary.join(format!("manifests/{app}.manifest")));
        dst.entry(app.clone()).or_insert(entry);
        src.remove(&app);
        moved.push(app);
    }
    if moved.is_empty() {
        return moved;
    }
    // Rare's list first, then Heroic's without them: a crash in between lists a game twice, never zero times.
    write_json(&dst_file, &Value::Object(dst));
    write_json(&src_file, &Value::Object(src));
    copy_if_missing(&src_dir.join("user.json"), &legendary.join("user.json"));
    moved
}

/// Moves a Heroic's GOG installs into `store` (Rare's gog_store). Returns the ids moved.
pub fn gog_into(heroic: &Path, store: &Path) -> Vec<String> {
    let src_store = heroic.join("gog_store");
    if src_store == store {
        return vec![];
    }
    let src_file = src_store.join("installed.json");
    let Some(src) = read_json(&src_file) else { return vec![] };
    let mut list: Vec<Value> = src.get("installed").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        return vec![];
    }
    let dst_file = store.join("installed.json");
    let mut dst: Vec<Value> = read_json(&dst_file).and_then(|v| v.get("installed").and_then(Value::as_array).cloned()).unwrap_or_default();
    let mut moved = Vec::new();
    list.retain(|g| {
        let id = g.get("appName").and_then(Value::as_str).unwrap_or("").to_string();
        let dir = g.get("install_path").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() || dir.is_empty() || !Path::new(dir).is_dir() {
            return true;
        }
        if !dst.iter().any(|d| d.get("appName").and_then(Value::as_str) == Some(id.as_str())) {
            dst.push(g.clone());
        }
        moved.push(id);
        false
    });
    if moved.is_empty() {
        return moved;
    }
    write_json(&dst_file, &json!({ "installed": dst }));
    write_json(&src_file, &json!({ "installed": list }));
    copy_if_missing(&src_store.join("auth.json"), &store.join("auth.json"));
    moved
}

/// Adopts whatever Heroic installed. Cheap when there is nothing (two file reads per Heroic).
/// Returns the library ids adopted.
pub fn run() -> Vec<String> {
    let mut out = Vec::new();
    let store = crate::gog::store_dir();
    for h in heroic_configs() {
        out.extend(epic_into(&h, &crate::epic::config_dir()).into_iter().map(|a| format!("epic:{a}")));
        out.extend(gog_into(&h, &store).into_iter().map(|a| format!("gog:{a}")));
    }
    if !out.is_empty() {
        log::info!("adopted from Heroic: {}", out.join(", "));
        crate::accounts_changed();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heroic_installs_move_over() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path().join("flatpak/heroic");
        let game = t.path().join("Games/Hades");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(h.join("legendaryConfig/legendary/metadata")).unwrap();
        fs::create_dir_all(h.join("gog_store")).unwrap();
        let g = game.to_string_lossy();
        fs::write(h.join("legendaryConfig/legendary/installed.json"), format!(r#"{{"Hades":{{"install_path":"{g}","title":"Hades"}},"Gone":{{"install_path":"/nope"}}}}"#)).unwrap();
        fs::write(h.join("legendaryConfig/legendary/metadata/Hades.json"), "{}").unwrap();
        fs::write(h.join("legendaryConfig/legendary/user.json"), r#"{"displayName":"t"}"#).unwrap();
        fs::write(h.join("gog_store/installed.json"), format!(r#"{{"installed":[{{"appName":"7","install_path":"{g}"}}]}}"#)).unwrap();
        let leg = t.path().join("legendary");
        let store = t.path().join("native/heroic/gog_store");
        assert_eq!(epic_into(&h, &leg), vec!["Hades".to_string()]);
        assert!(leg.join("metadata/Hades.json").is_file() && leg.join("user.json").is_file());
        let left = read_json(&h.join("legendaryConfig/legendary/installed.json")).unwrap();
        assert!(left.get("Hades").is_none() && left.get("Gone").is_some());
        assert!(epic_into(&h, &leg).is_empty(), "nothing twice");
        assert_eq!(gog_into(&h, &store), vec!["7".to_string()]);
        assert_eq!(read_json(&store.join("installed.json")).unwrap()["installed"][0]["appName"], "7");
    }
}
