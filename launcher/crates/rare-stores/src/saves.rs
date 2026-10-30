//! Cloud saves for Epic and GOG games, in the game's Rare prefix.
//!
//! The rule: fetch before a game starts, send after it closes. A game that has never synced on
//! this device only fetches the first time, so a fresh local save can never replace progress made
//! elsewhere. Epic goes through `legendary sync-saves`; GOG through `gogdl save-sync`, once per
//! save location in the game's GOG cloud settings, with the last sync time kept per location.

use crate::{epic, gog, read_json, rare_dir, write_json, Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn marks_path() -> PathBuf {
    rare_dir().join("saves.json")
}

fn marks() -> Value {
    read_json(&marks_path()).unwrap_or_else(|| json!({}))
}

fn set_mark(key: &str, v: Value) {
    let mut m = marks();
    if let Some(o) = m.as_object_mut() {
        o.insert(key.to_string(), v);
    }
    write_json(&marks_path(), &m);
}

/// Whether the game keeps saves in its store's cloud (installed games only for GOG).
pub fn supported(id: &str) -> bool {
    match Store::of(id) {
        Some((Store::Epic, app)) => epic::has_cloud_saves(app),
        Some((Store::Gog, gid)) => gog::installed(gid).is_some_and(|(dir, p, _)| p != "linux" && !gog_locations(gid, Path::new(&dir)).is_empty()),
        None => false,
    }
}

/// When it last synced (Unix seconds), if ever.
pub fn last_sync(id: &str) -> Option<u64> {
    marks().get(id).and_then(|v| v.get("at")).and_then(Value::as_u64)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Fetches newer saves from the cloud (before the game starts).
pub fn before_launch(id: &str) -> Result<(), String> {
    sync(id, true, false)
}

/// Sends the game's saves to the cloud (after it closed). Only fetches when it never synced here.
pub fn after_exit(id: &str) -> Result<(), String> {
    let first = last_sync(id).is_none();
    sync(id, true, !first)
}

/// Both ways (the game page's Sync now).
pub fn sync_now(id: &str) -> Result<(), String> {
    let first = last_sync(id).is_none();
    sync(id, true, !first)
}

fn sync(id: &str, down: bool, up: bool) -> Result<(), String> {
    let Some((store, sid)) = Store::of(id) else { return Ok(()) };
    if !store.signed_in() || !supported(id) {
        return Ok(());
    }
    let r = match store {
        Store::Epic => epic::sync_saves(sid, down, up),
        Store::Gog => gog_sync(sid, down, up),
    };
    if r.is_ok() && crate::prefix(id).join("drive_c").is_dir() {
        set_mark(id, json!({ "at": now() }));
        crate::changed();
    }
    r
}

/// A GOG save location: its cloud name and the folder on this device.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub name: String,
    pub path: PathBuf,
}

/// `<?APPLICATION_DATA_LOCAL?>/Foo` in the game's prefix (Proton's user is steamuser).
pub fn resolve(template: &str, prefix: &Path, install: &Path) -> PathBuf {
    let user = prefix.join("drive_c/users/steamuser");
    let t = template.replace('\\', "/");
    let map: [(&str, PathBuf); 7] = [
        ("<?INSTALL?>", install.to_path_buf()),
        ("<?DOCUMENTS?>", user.join("Documents")),
        ("<?SAVED_GAMES?>", user.join("Saved Games")),
        ("<?APPLICATION_DATA_LOCAL_LOW?>", user.join("AppData/LocalLow")),
        ("<?APPLICATION_DATA_LOCAL?>", user.join("AppData/Local")),
        ("<?APPLICATION_DATA_ROAMING?>", user.join("AppData/Roaming")),
        ("<?USER_HOME?>", user.clone()),
    ];
    for (k, v) in map {
        if let Some(rest) = t.strip_prefix(k) {
            return v.join(rest.trim_start_matches('/'));
        }
    }
    install.join(t.trim_start_matches('/'))
}

/// The game's client id (goggame-<id>.info in its folder).
fn client_id(id: &str, dir: &Path) -> Option<String> {
    let info = read_json(&dir.join(format!("goggame-{id}.info")))?;
    info.get("clientId").and_then(Value::as_str).map(String::from)
}

/// Where the game's saves are, from its GOG cloud settings (cached per client id).
pub fn gog_locations(id: &str, dir: &Path) -> Vec<Location> {
    let Some(client) = client_id(id, dir) else { return vec![] };
    let cache = rare_dir().join("gog-cloud").join(format!("{client}.json"));
    let cfg = read_json(&cache).or_else(|| {
        let url = format!("https://remote-config.gog.com/components/galaxy_client/clients/{client}?component_version=2.0.45");
        let v: Value = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build().get(&url).call().ok()?.into_json().ok()?;
        write_json(&cache, &v);
        Some(v)
    });
    let Some(cfg) = cfg else { return vec![] };
    let cs = cfg.pointer("/content/Windows/cloudStorage").cloned().unwrap_or(Value::Null);
    if !cs.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
        return vec![];
    }
    let prefix = crate::prefix(&format!("gog:{id}"));
    let mut locs: Vec<Location> = cs
        .get("locations")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|l| Some(Location { name: l.get("name")?.as_str()?.to_string(), path: resolve(l.get("location")?.as_str()?, &prefix, dir) }))
                .collect()
        })
        .unwrap_or_default();
    if locs.is_empty() {
        let t = format!("<?APPLICATION_DATA_LOCAL?>/GOG.com/Galaxy/Applications/{client}/Storage/Shared/Files");
        locs.push(Location { name: "__default".into(), path: resolve(&t, &prefix, dir) });
    }
    locs
}

fn gog_sync(id: &str, down: bool, up: bool) -> Result<(), String> {
    let (dir, _, _) = gog::installed(id).ok_or("It isn't installed.")?;
    if !crate::prefix(&format!("gog:{id}")).join("drive_c").is_dir() {
        return Ok(());
    }
    for loc in gog_locations(id, Path::new(&dir)) {
        let key = format!("gog:{id}:{}", loc.name);
        let ts = marks().get(&key).and_then(Value::as_f64).unwrap_or(0.0).to_string();
        let path = loc.path.to_string_lossy().into_owned();
        let mut args = vec!["save-sync", path.as_str(), id, "--os", "windows", "--ts", ts.as_str(), "--name", loc.name.as_str()];
        if !up {
            args.push("--skip-upload");
        }
        if !down {
            args.push("--skip-download");
        }
        let out = gog::run_saves(&args)?;
        // gogdl prints the new sync time last.
        if let Some(t) = out.lines().rev().find_map(|l| l.trim().parse::<f64>().ok()) {
            set_mark(&key, json!(t));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_locations() {
        let p = Path::new("/pfx");
        let i = Path::new("/games/x");
        assert_eq!(resolve("<?APPLICATION_DATA_LOCAL?>/CD Projekt Red/saves", p, i), PathBuf::from("/pfx/drive_c/users/steamuser/AppData/Local/CD Projekt Red/saves"));
        assert_eq!(resolve("<?INSTALL?>\\save", p, i), PathBuf::from("/games/x/save"));
        assert_eq!(resolve("<?APPLICATION_DATA_LOCAL_LOW?>/A", p, i), PathBuf::from("/pfx/drive_c/users/steamuser/AppData/LocalLow/A"));
    }
}
