//! Epic (through legendary) and GOG (through Heroic's gogdl data). Both tools are GPL and
//! are only read from disk or called as external programs; no code is linked. Owned games that
//! are not installed come from the lists rare-stores fetches after sign-in.

use crate::model::{Art, Game, LaunchSpec, Source};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub struct StoreScan {
    pub games: Vec<Game>,
    pub available: bool,
    pub note: String,
}

pub fn config_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(h) = dirs::home_dir() {
        out.push(h.join(".config"));
        out.push(h.join(".var/app/com.heroicgameslauncher.hgl/config"));
    }
    for home in super::roots::other_homes() {
        out.push(home.join(".config"));
        out.push(home.join(".var/app/com.heroicgameslauncher.hgl/config"));
    }
    out
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(p).ok()?).ok()
}

/// Titles of every Epic and GOG game the player owns, installed or not (Heroic's library caches).
pub fn owned_titles() -> Vec<String> {
    let mut out = Vec::new();
    for c in config_dirs() {
        if let Some(v) = read_json(&c.join("heroic/store_cache/legendary_library.json")) {
            out.extend(v["library"].as_array().into_iter().flatten().filter_map(|g| g["title"].as_str().map(str::to_string)));
        }
        if let Some(v) = read_json(&c.join("heroic/store_cache/gog_library.json")) {
            out.extend(v["games"].as_array().into_iter().flatten().filter_map(|g| g["title"].as_str().map(str::to_string)));
        }
        // Signed in through Rare (rare-stores).
        for store in ["epic", "gog"] {
            out.extend(owned(&c, store).iter().filter_map(|g| g["title"].as_str().map(str::to_string)));
        }
    }
    out
}

/// Owned games Rare fetched from the store (rare-stores' `~/.config/rare/stores/<store>-owned.json`,
/// the keys of Heroic's library cache).
fn owned(config: &Path, store: &str) -> Vec<Value> {
    read_json(&config.join(format!("rare/stores/{store}-owned.json"))).and_then(|v| v.get("games").and_then(Value::as_array).cloned()).unwrap_or_default()
}

/// An owned game that is not installed: listed under its store with Install on its page.
fn not_installed(source: Source, id: String, g: &Value, launch: LaunchSpec) -> Game {
    let s = |k: &str| g.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(String::from);
    let mut art = Art::default();
    art.cover.extend(s("art_square"));
    art.hero.extend(s("art_cover"));
    art.logo.extend(s("art_logo"));
    Game {
        title: s("title").unwrap_or_else(|| id.split(':').nth(1).unwrap_or("").to_string()),
        origin: if source == Source::Epic { "Epic Games".into() } else { "GOG".into() },
        id,
        source,
        installed: false,
        install_dir: None,
        size_bytes: None,
        last_played: None,
        playtime_minutes: None,
        art,
        launch,
        foreign: false,
    }
}

/// "4.21 GiB" (Heroic's install_size) to bytes.
fn gib_bytes(s: &str) -> Option<u64> {
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.parse().ok()?;
    let mult = match it.next().unwrap_or("B") {
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "KiB" => 1024.0,
        "GB" => 1e9,
        "MB" => 1e6,
        _ => 1.0,
    };
    Some((n * mult) as u64)
}

/// True when Heroic Games Launcher is installed (Flatpak or native).
pub fn heroic_installed() -> bool {
    let flatpak = dirs::data_dir().map(|d| d.join("flatpak/app/com.heroicgameslauncher.hgl").is_dir()).unwrap_or(false)
        || Path::new("/var/lib/flatpak/app/com.heroicgameslauncher.hgl").is_dir();
    flatpak || super::steam::which("heroic")
}

/// Heroic's play history: `heroic/store/timestamp.json` maps app names to
/// { lastPlayed: ISO time, totalPlayed: minutes }.
fn heroic_played(config: &Path) -> std::collections::HashMap<String, (u64, u64)> {
    let mut out = std::collections::HashMap::new();
    let Some(Value::Object(map)) = read_json(&config.join("heroic/store/timestamp.json")) else { return out };
    for (app, v) in map {
        let last = v.get("lastPlayed").and_then(Value::as_str).and_then(iso_secs).unwrap_or(0);
        let total = v.get("totalPlayed").and_then(Value::as_u64).unwrap_or(0);
        out.insert(app, (last, total));
    }
    out
}

/// "2026-10-01T18:22:05.123Z" to Unix seconds (UTC; offsets are ignored).
fn iso_secs(s: &str) -> Option<u64> {
    let d: String = s.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    if d.len() < 14 {
        return None;
    }
    super::esde::parse_time(&format!("{}T{}", &d[..8], &d[8..14]))
}

pub fn scan_epic() -> StoreScan {
    scan_epic_in(&config_dirs(), super::steam::which("legendary"), heroic_installed())
}

pub fn scan_epic_in(configs: &[PathBuf], legendary: bool, heroic: bool) -> StoreScan {
    let mut games = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in configs {
        let played = heroic_played(c);
        for f in ["legendary/installed.json", "heroic/legendaryConfig/legendary/installed.json"] {
            let Some(Value::Object(map)) = read_json(&c.join(f)) else { continue };
            for (app_name, g) in map {
                if g.get("is_dlc").and_then(Value::as_bool).unwrap_or(false) || !seen.insert(app_name.clone()) {
                    continue;
                }
                let title = g.get("title").and_then(Value::as_str).unwrap_or(&app_name).to_string();
                let mut art = epic_art(c, &app_name);
                if art.cover.is_empty() {
                    if let Some(o) = owned(c, "epic").iter().find(|o| o.get("app_name").and_then(Value::as_str) == Some(app_name.as_str())) {
                        art = not_installed(Source::Epic, String::new(), o, LaunchSpec::Legendary { app_name: String::new() }).art;
                    }
                }
                let dir = g.get("install_path").and_then(Value::as_str).map(String::from);
                let in_heroic = f.starts_with("heroic/");
                // Heroic's own installs launch through Heroic (its Wine settings); legendary's through legendary.
                let launch = if (in_heroic || !legendary) && heroic {
                    LaunchSpec::Heroic { runner: "legendary".into(), app_name: app_name.clone(), dir: dir.clone() }
                } else {
                    LaunchSpec::Legendary { app_name: app_name.clone() }
                };
                let p = played.get(&app_name).copied();
                games.push(Game {
                    id: format!("epic:{app_name}"),
                    source: Source::Epic,
                    title,
                    installed: true,
                    install_dir: dir,
                    size_bytes: g.get("install_size").and_then(Value::as_u64),
                    last_played: p.map(|p| p.0).filter(|t| *t > 0),
                    playtime_minutes: p.map(|p| p.1).filter(|m| *m > 0),
                    art,
                    launch,
                    origin: if in_heroic { "Epic Games (Heroic)".into() } else { "Epic Games".into() },
                    foreign: false,
                });
            }
        }
    }
    // Owned and not installed (signed in through Rare).
    for c in configs {
        for o in owned(c, "epic") {
            let Some(app) = o.get("app_name").and_then(Value::as_str).map(String::from) else { continue };
            if seen.insert(app.clone()) {
                games.push(not_installed(Source::Epic, format!("epic:{app}"), &o, LaunchSpec::Legendary { app_name: app }));
            }
        }
    }
    let available = legendary || heroic;
    let note = if heroic { "Heroic".into() } else if legendary { "legendary".into() } else { "Sign in to Epic Games in Settings > Accounts".into() };
    StoreScan { games, available, note }
}

/// Cover art from legendary's metadata cache, when present.
fn epic_art(config: &Path, app_name: &str) -> Art {
    let mut art = Art::default();
    for f in [format!("legendary/metadata/{app_name}.json"), format!("heroic/legendaryConfig/legendary/metadata/{app_name}.json")] {
        if let Some(meta) = read_json(&config.join(f)) {
            if let Some(imgs) = meta.pointer("/metadata/keyImages").and_then(Value::as_array) {
                for img in imgs {
                    let url = img.get("url").and_then(Value::as_str).unwrap_or("").to_string();
                    match img.get("type").and_then(Value::as_str).unwrap_or("") {
                        "DieselGameBoxTall" => art.cover.push(url),
                        "DieselGameBox" => art.hero.push(url),
                        "DieselGameBoxLogo" => art.logo.push(url),
                        _ => {}
                    }
                }
            }
        }
    }
    art
}

/// The program a GOG install runs: start.sh for Linux builds, the primary play task for Windows builds.
fn gog_launch(dir: &Path, platform: &str) -> Option<LaunchSpec> {
    if platform == "linux" {
        let sh = dir.join("start.sh");
        if sh.is_file() {
            return Some(LaunchSpec::Exec { program: sh.to_string_lossy().into(), args: vec![], cwd: Some(dir.to_string_lossy().into()), windows: false });
        }
    }
    for e in fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("goggame-") && name.ends_with(".info")) {
            continue;
        }
        let info = read_json(&e.path())?;
        let tasks = info.get("playTasks")?.as_array()?;
        let task = tasks.iter().find(|t| t.get("isPrimary").and_then(Value::as_bool).unwrap_or(false)).or_else(|| tasks.first())?;
        let rel = task.get("path")?.as_str()?.replace('\\', "/");
        let args = task.get("arguments").and_then(Value::as_str).map(|a| a.split_whitespace().map(String::from).collect()).unwrap_or_default();
        let work = task.get("workingDir").and_then(Value::as_str).map(|w| dir.join(w.replace('\\', "/")));
        return Some(LaunchSpec::Exec {
            program: dir.join(rel).to_string_lossy().into(),
            args,
            cwd: Some(work.unwrap_or_else(|| dir.to_path_buf()).to_string_lossy().into()),
            windows: true,
        });
    }
    None
}

pub fn scan_gog() -> StoreScan {
    scan_gog_in(&config_dirs(), heroic_installed(), super::steam::which("umu-run"))
}

pub fn scan_gog_in(configs: &[PathBuf], heroic: bool, umu: bool) -> StoreScan {
    let mut games = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in configs {
        let played = heroic_played(c);
        games.extend(sideloaded(c, heroic, &played));
        let Some(installed) = read_json(&c.join("heroic/gog_store/installed.json")) else { continue };
        // Titles and art live in Heroic's library cache.
        let library = read_json(&c.join("heroic/store_cache/gog_library.json")).or_else(|| read_json(&c.join("heroic/gog_store/library.json")));
        let mine = owned(c, "gog");
        let lookup = |id: &str| -> Option<Value> {
            let in_heroic = library.as_ref().and_then(|lib| {
                let arr = lib.get("games").and_then(Value::as_array).or_else(|| lib.as_array())?;
                arr.iter().find(|g| g.get("app_name").and_then(Value::as_str) == Some(id)).cloned()
            });
            in_heroic.or_else(|| mine.iter().find(|g| g.get("app_name").and_then(Value::as_str) == Some(id)).cloned())
        };
        for g in installed.get("installed").and_then(Value::as_array).cloned().unwrap_or_default() {
            let Some(id) = g.get("appName").and_then(Value::as_str).map(String::from) else { continue };
            if !seen.insert(id.clone()) {
                continue;
            }
            let dir = g.get("install_path").and_then(Value::as_str).unwrap_or("").to_string();
            let platform = g.get("platform").and_then(Value::as_str).unwrap_or("windows").to_lowercase();
            let direct = gog_launch(Path::new(&dir), &platform);
            // Rare starts it itself (umu-run in the game's prefix, Heroic's when it came from Heroic);
            // through Heroic only when Rare can't tell what to run.
            let launch = match direct {
                Some(l) => l,
                None if heroic => LaunchSpec::Heroic { runner: "gog".into(), app_name: id.clone(), dir: Some(dir.clone()) },
                None => continue,
            };
            let meta = lookup(&id);
            let title = meta
                .as_ref()
                .and_then(|m| m.get("title").and_then(Value::as_str).map(String::from))
                .unwrap_or_else(|| Path::new(&dir).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(id.clone()));
            let mut art = Art::default();
            if let Some(m) = &meta {
                if let Some(u) = m.get("art_square").and_then(Value::as_str) {
                    art.cover.push(u.to_string());
                }
                if let Some(u) = m.get("art_cover").and_then(Value::as_str) {
                    art.hero.push(u.to_string());
                }
                if let Some(u) = m.get("art_logo").and_then(Value::as_str) {
                    art.logo.push(u.to_string());
                }
            }
            games.push(Game {
                id: format!("gog:{id}"),
                source: Source::Gog,
                title,
                installed: true,
                install_dir: Some(dir),
                size_bytes: g.get("install_size").and_then(Value::as_str).and_then(gib_bytes),
                last_played: None,
                playtime_minutes: None,
                art,
                launch,
                origin: "GOG".into(),
                foreign: false,
            });
            if let Some(g) = games.last_mut() {
                if let Some(p) = played.get(&id) {
                    g.last_played = Some(p.0).filter(|t| *t > 0);
                    g.playtime_minutes = Some(p.1).filter(|m| *m > 0);
                }
            }
        }
    }
    for c in configs {
        for o in owned(c, "gog") {
            let Some(id) = o.get("app_name").and_then(Value::as_str).map(String::from) else { continue };
            if seen.insert(id.clone()) {
                games.push(not_installed(Source::Gog, format!("gog:{id}"), &o, LaunchSpec::Heroic { runner: "gog".into(), app_name: id, dir: None }));
            }
        }
    }
    let available = heroic || umu;
    let note = if heroic { "GOG installs from Heroic".into() } else if umu { "GOG games through umu-run".into() } else { "Sign in to GOG in Settings > Accounts".into() };
    StoreScan { games, available, note }
}

/// Games added to Heroic by hand ("Add game"): heroic/sideload_apps/library.json.
fn sideloaded(config: &Path, heroic: bool, played: &std::collections::HashMap<String, (u64, u64)>) -> Vec<Game> {
    let Some(lib) = read_json(&config.join("heroic/sideload_apps/library.json")) else { return vec![] };
    let list = lib.get("games").and_then(Value::as_array).cloned().unwrap_or_default();
    list.iter()
        .filter_map(|g| {
            let app = g.get("app_name")?.as_str()?.to_string();
            if g.get("is_installed").and_then(Value::as_bool) == Some(false) {
                return None;
            }
            let title = g.get("title").and_then(Value::as_str).unwrap_or(&app).to_string();
            let exe = g.pointer("/install/executable").and_then(Value::as_str).unwrap_or("").to_string();
            let platform = g.pointer("/install/platform").and_then(Value::as_str).unwrap_or("windows").to_lowercase();
            let dir = Path::new(&exe).parent().map(|p| p.to_string_lossy().into_owned());
            let launch = if heroic {
                LaunchSpec::Heroic { runner: "sideload".into(), app_name: app.clone(), dir: dir.clone() }
            } else if !exe.is_empty() {
                LaunchSpec::Exec { program: exe.clone(), args: vec![], cwd: dir.clone(), windows: platform != "linux" }
            } else {
                return None;
            };
            let mut art = Art::default();
            art.cover.extend(g.get("art_square").and_then(Value::as_str).filter(|u| !u.is_empty()).map(String::from));
            art.hero.extend(g.get("art_cover").and_then(Value::as_str).filter(|u| !u.is_empty()).map(String::from));
            let p = played.get(&app).copied();
            Some(Game {
                id: format!("heroic:{app}"),
                source: Source::Local,
                title,
                installed: true,
                install_dir: dir,
                size_bytes: None,
                last_played: p.map(|p| p.0).filter(|t| *t > 0),
                playtime_minutes: p.map(|p| p.1).filter(|m| *m > 0),
                art,
                launch,
                origin: "Heroic".into(),
                foreign: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod owned_tests {
    use super::*;

    #[test]
    fn owned_games_are_listed_as_not_installed() {
        let t = tempfile::tempdir().unwrap();
        let c = t.path().to_path_buf();
        fs::create_dir_all(c.join("rare/stores")).unwrap();
        fs::create_dir_all(c.join("legendary")).unwrap();
        fs::write(c.join("rare/stores/epic-owned.json"), r#"{"games":[{"app_name":"A","title":"Alpha","art_square":"https://x/a.jpg"},{"app_name":"B","title":"Beta"}]}"#).unwrap();
        fs::write(c.join("legendary/installed.json"), r#"{"B":{"title":"Beta","install_path":"/g/B","install_size":5}}"#).unwrap();
        fs::write(c.join("rare/stores/gog-owned.json"), r#"{"games":[{"app_name":"7","title":"Seven","platform":"windows"}]}"#).unwrap();
        let e = scan_epic_in(&[c.clone()], true, false);
        assert_eq!(e.games.len(), 2);
        let a = e.games.iter().find(|g| g.id == "epic:A").unwrap();
        assert!(!a.installed && a.art.cover == vec!["https://x/a.jpg".to_string()]);
        assert!(e.games.iter().find(|g| g.id == "epic:B").unwrap().installed);
        let g = scan_gog_in(&[c], false, true);
        assert_eq!((g.games.len(), g.games[0].installed, g.games[0].title.as_str()), (1, false, "Seven"));
        assert_eq!(gib_bytes("2.00 GiB"), Some(2 * 1024 * 1024 * 1024));
    }
}
