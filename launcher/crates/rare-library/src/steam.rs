//! Steam: reads libraryfolders.vdf and appmanifest_*.acf from every Steam install and
//! library it can find, including libraries left by an earlier OS on the same disk and
//! SteamOS-style SD cards. Launches go through the Steam client (steam://rungameid).

use super::{roots, vdf};
use crate::model::{Art, Game, LaunchSpec, Source};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Runtimes and tools Steam installs as "apps" that are not games.
const TOOL_APPIDS: &[u32] = &[
    228980,  // Steamworks Common Redistributables
    1070560, // Steam Linux Runtime 1.0 (scout)
    1391110, // Steam Linux Runtime 2.0 (soldier)
    1628350, // Steam Linux Runtime 3.0 (sniper)
    1493710, // Proton Experimental
    2180100, // Proton Hotfix
    1826330, // Proton EasyAntiCheat Runtime
    1161040, // Proton BattlEye Runtime
    2348590, // Proton 8.0
    2805730, // Proton 9.0
    3658110, // Proton 10.0
    961940,  // Proton 3.16
    1054830, // Proton 4.2
    1113280, // Proton 4.11
    1245040, // Proton 5.0
    1420170, // Proton 5.13
    1580130, // Proton 6.3
    1887720, // Proton 7.0
    4183110, // Steam Linux Runtime 4.0
    4628710, // Proton 11.0
];

pub fn is_tool(appid: u32, name: &str) -> bool {
    TOOL_APPIDS.contains(&appid)
        || name.starts_with("Proton ")
        || name.starts_with("Steam Linux Runtime")
        || name.starts_with("Steamworks Common")
        || name == "Proton Experimental"
        || name.starts_with("Proton BattlEye")
        || name.starts_with("Proton EasyAntiCheat")
        || name.starts_with("Steamworks Shared")
        || name.contains("Redistributable")
        || name == "Steam Controller Configs"
        || name.starts_with("SteamVR")
        || name.starts_with("GE-Proton")
}

/// A Steam client install: the directory holding steamapps/ and appcache/.
#[derive(Debug, Clone)]
pub struct SteamRoot {
    pub path: PathBuf,
    /// The install the current user's Steam runs from.
    pub primary: bool,
}

/// Candidate Steam client directories inside one home directory.
fn steam_dirs_in_home(home: &Path) -> Vec<PathBuf> {
    [
        ".local/share/Steam",
        ".steam/steam",
        ".steam/root",
        ".steam/debian-installation",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ".var/app/com.valvesoftware.Steam/data/Steam",
        "Library/Application Support/Steam",
    ]
    .iter()
    .map(|p| home.join(p))
    .collect()
}

pub fn find_roots() -> Vec<SteamRoot> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let me = dirs::home_dir();
    let mut homes: Vec<PathBuf> = me.iter().cloned().collect();
    homes.extend(roots::other_homes());
    for (i, home) in homes.iter().enumerate() {
        for d in steam_dirs_in_home(home) {
            if !d.join("steamapps").is_dir() {
                continue;
            }
            let canon = fs::canonicalize(&d).unwrap_or(d.clone());
            if seen.insert(canon.clone()) {
                let primary = i == 0 && !out.iter().any(|r: &SteamRoot| r.primary);
                out.push(SteamRoot { path: canon, primary });
            }
        }
    }
    out
}

/// Library folders the given Steam root lists in libraryfolders.vdf (plus the root itself).
pub fn registered_libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    for f in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
        if let Ok(text) = fs::read_to_string(root.join(f)) {
            let v = vdf::parse(&text);
            if let Some(lf) = v.get("libraryfolders") {
                for (_, entry) in lf.entries() {
                    let p = match entry {
                        vdf::Vdf::Str(s) => Some(s.clone()), // very old format: "1" "/path"
                        obj => obj.str("path").map(|s| s.to_string()),
                    };
                    if let Some(p) = p {
                        libs.push(PathBuf::from(p));
                    }
                }
            }
        }
    }
    libs
}

/// Library folders sitting on mounted drives and other homes (SD cards, an older OS's disk).
pub fn libraries_on_mounts() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for m in roots::mount_points() {
        for sub in ["", "SteamLibrary", "Steam", "steam", "Games/SteamLibrary", "games/SteamLibrary"] {
            let d = if sub.is_empty() { m.clone() } else { m.join(sub) };
            if d.join("steamapps").is_dir() {
                out.push(d);
            }
        }
    }
    out
}

fn art_for(appid: u32, roots: &[SteamRoot], user_configs: &[PathBuf]) -> Art {
    // Art the user picked in Steam (config/grid/) first, then the local library cache
    // (works offline), then the CDN.
    let mut art = super::shortcuts::grid_art(user_configs, appid);
    for r in roots {
        let cache = r.path.join("appcache/librarycache");
        let per_app = cache.join(appid.to_string());
        let find = |name: &str| -> Option<String> {
            let direct = per_app.join(name);
            if direct.is_file() {
                return Some(direct.to_string_lossy().into_owned());
            }
            // Newer clients nest files one level down in hashed folders.
            if let Ok(rd) = fs::read_dir(&per_app) {
                for e in rd.flatten() {
                    let p = e.path().join(name);
                    if p.is_file() {
                        return Some(p.to_string_lossy().into_owned());
                    }
                }
            }
            let legacy = cache.join(format!("{appid}_{name}"));
            legacy.is_file().then(|| legacy.to_string_lossy().into_owned())
        };
        // Older clients use the flat names, newer ones the library_* names in hashed folders.
        art.cover.extend(["library_600x900.jpg", "library_capsule.jpg"].iter().filter_map(|n| find(n)));
        art.hero.extend(["library_hero.jpg"].iter().filter_map(|n| find(n)));
        art.logo.extend(["logo.png"].iter().filter_map(|n| find(n)));
        art.header.extend(["header.jpg", "library_header.jpg"].iter().filter_map(|n| find(n)));
        // The community icon is the one file named by its hash at the top of the app's folder.
        if let Ok(rd) = fs::read_dir(&per_app) {
            let mut icons: Vec<String> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.file_stem().is_some_and(|s| s.len() == 40 && s.to_string_lossy().chars().all(|c| c.is_ascii_hexdigit())))
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            icons.sort();
            art.icon.extend(icons);
        }
        let legacy_icon = cache.join(format!("{appid}_icon.jpg"));
        if legacy_icon.is_file() {
            art.icon.push(legacy_icon.to_string_lossy().into_owned());
        }
    }
    // Public Steam CDN as fallback.
    let cdn = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
    // 600x900 covers every tile and page cover on a handheld screen; the 2x (1200x1800) is a
    // fallback only, it costs four times the bytes and decode time for nothing.
    art.cover.push(format!("{cdn}/library_600x900.jpg"));
    art.cover.push(format!("{cdn}/library_600x900_2x.jpg"));
    art.hero.push(format!("{cdn}/library_hero.jpg"));
    art.logo.push(format!("{cdn}/logo.png"));
    art.header.push(format!("{cdn}/header.jpg"));
    art
}

/// Playtime (minutes) and last-played time per app, from every Steam user's localconfig.vdf.
fn playtimes(roots: &[SteamRoot]) -> std::collections::HashMap<u32, (u64, u64)> {
    let mut out = std::collections::HashMap::new();
    for r in roots {
        let Ok(users) = fs::read_dir(r.path.join("userdata")) else { continue };
        for u in users.flatten() {
            let Ok(text) = fs::read_to_string(u.path().join("config/localconfig.vdf")) else { continue };
            let v = vdf::parse(&text);
            let apps = v
                .get("UserLocalConfigStore")
                .and_then(|x| x.get("Software"))
                .and_then(|x| x.get("Valve"))
                .and_then(|x| x.get("Steam"))
                .and_then(|x| x.get("apps"));
            let Some(apps) = apps else { continue };
            for (id, app) in apps.entries() {
                let Ok(appid) = id.parse::<u32>() else { continue };
                let minutes = app.str("Playtime").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let last = app.str("LastPlayed").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let e = out.entry(appid).or_insert((0, 0));
                e.0 = e.0.max(minutes);
                e.1 = e.1.max(last);
            }
        }
    }
    out
}

/// Every app any Steam user on this device has played (localconfig.vdf), installed or not: games
/// the player owns, for matching GeForce NOW's catalogue.
pub fn played_appids() -> Vec<u32> {
    playtimes(&find_roots()).into_keys().collect()
}

pub struct SteamScan {
    pub games: Vec<Game>,
    pub installed: bool,
    pub foreign_libraries: Vec<String>,
    /// Steam installs found, for the shortcut scan.
    pub roots: Vec<SteamRoot>,
}

pub fn scan() -> SteamScan {
    let roots = find_roots();
    let primary = roots.iter().find(|r| r.primary);
    let registered: BTreeSet<PathBuf> = primary
        .map(|r| registered_libraries(&r.path).into_iter().map(|p| fs::canonicalize(&p).unwrap_or(p)).collect())
        .unwrap_or_default();

    let mut libs: Vec<PathBuf> = Vec::new();
    for r in &roots {
        libs.extend(registered_libraries(&r.path));
    }
    libs.extend(libraries_on_mounts());

    let played = playtimes(&roots);
    let user_configs = super::shortcuts::user_config_dirs(&roots);
    let mut seen_libs = HashSet::new();
    let mut seen_apps = HashSet::new();
    let mut games = Vec::new();
    let mut foreign_libraries = Vec::new();

    for lib in libs {
        let canon = fs::canonicalize(&lib).unwrap_or(lib.clone());
        if !seen_libs.insert(canon.clone()) {
            continue;
        }
        let steamapps = canon.join("steamapps");
        let Ok(rd) = fs::read_dir(&steamapps) else { continue };
        let foreign = primary.is_some() && !registered.contains(&canon);
        let mut found_any = false;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else { continue };
            let v = vdf::parse(&text);
            let Some(st) = v.get("AppState") else { continue };
            let Some(appid) = st.str("appid").and_then(|s| s.parse::<u32>().ok()) else { continue };
            let title = st.str("name").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
            if title.is_empty() || is_tool(appid, &title) || !seen_apps.insert(appid) {
                continue;
            }
            // StateFlags bit 4 (value 4) = fully installed.
            let flags = st.str("StateFlags").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
            let install_dir = st.str("installdir").map(|d| steamapps.join("common").join(d).to_string_lossy().into_owned());
            found_any = true;
            games.push(Game {
                id: format!("steam:{appid}"),
                source: Source::Steam,
                title,
                installed: flags & 4 != 0,
                install_dir,
                size_bytes: st.str("SizeOnDisk").and_then(|s| s.parse().ok()),
                last_played: st
                    .str("LastPlayed")
                    .and_then(|s| s.parse().ok())
                    .filter(|t: &u64| *t > 0)
                    .or_else(|| played.get(&appid).map(|p| p.1).filter(|t| *t > 0)),
                playtime_minutes: played.get(&appid).map(|p| p.0).filter(|m| *m > 0),
                art: art_for(appid, &roots, &user_configs),
                launch: LaunchSpec::Steam { appid },
                origin: if foreign {
                    format!("Steam library on {}", canon.display())
                } else {
                    "Steam".into()
                },
                foreign,
            });
        }
        if foreign && found_any {
            foreign_libraries.push(canon.to_string_lossy().into_owned());
        }
    }
    SteamScan { games, installed: !roots.is_empty() || which("steam"), foreign_libraries, roots }
}

/// The signed-in (else most recent) account's userdata folder name under a Steam root.
fn current_account(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join("config/loginusers.vdf")).ok()?;
    let v = vdf::parse(&text);
    let users = v.get("users")?;
    let mut best: Option<(u64, u64, u64)> = None;
    for (id, u) in users.entries() {
        let Ok(id64) = id.parse::<u64>() else { continue };
        let recent = u.str("MostRecent").or(u.str("mostrecent")) == Some("1");
        let ts = u.str("Timestamp").or(u.str("timestamp")).and_then(|t| t.parse::<u64>().ok()).unwrap_or(0);
        let rank = (recent as u64, ts, id64);
        if best.map_or(true, |b| rank > b) {
            best = Some(rank);
        }
    }
    best.map(|(_, _, id64)| (id64.wrapping_sub(76561197960265728) & 0xffff_ffff).to_string())
}

/// App ids in the account's library: Steam keeps one file per app it lists in
/// userdata/<account>/config/librarycache/<appid>.json (owned and free apps, tools included).
fn library_appids(root: &Path) -> HashSet<u32> {
    let users = root.join("userdata");
    let dirs: Vec<PathBuf> = match current_account(root).map(|a| users.join(a)).filter(|d| d.is_dir()) {
        Some(d) => vec![d],
        None => fs::read_dir(&users).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default(),
    };
    let mut out = HashSet::new();
    for d in dirs {
        let Ok(rd) = fs::read_dir(d.join("config/librarycache")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // Non-Steam shortcuts have ids above 2^31.
            if let Some(id) = name.strip_suffix(".json").and_then(|s| s.parse::<u32>().ok()).filter(|id| *id > 0 && *id < 0x8000_0000) {
                out.insert(id);
            }
        }
    }
    out
}

/// Games the Steam account owns that are not on this device (`installed` holds the app ids that
/// have a manifest in any library). Names, sizes and install folders come from Steam's app info
/// cache, so this works offline; an app the cache doesn't know is left out.
pub fn owned(roots: &[SteamRoot], installed: &HashSet<u32>) -> Vec<Game> {
    let Some(root) = roots.iter().find(|r| r.primary).or(roots.first()) else { return Vec::new() };
    let want: HashSet<u32> = library_appids(&root.path).into_iter().filter(|id| !installed.contains(id) && !TOOL_APPIDS.contains(id)).collect();
    if want.is_empty() {
        return Vec::new();
    }
    let meta = super::appinfo::read(&root.path, &want);
    let played = playtimes(roots);
    let user_configs = super::shortcuts::user_config_dirs(roots);
    let mut out: Vec<Game> = meta
        .values()
        .filter(|m| m.kind == "game" && !m.name.is_empty() && !is_tool(m.appid, &m.name))
        .map(|m| Game {
            id: format!("steam:{}", m.appid),
            source: Source::Steam,
            title: m.name.clone(),
            installed: false,
            install_dir: None,
            size_bytes: (m.size > 0).then_some(m.size),
            last_played: played.get(&m.appid).map(|p| p.1).filter(|t| *t > 0),
            playtime_minutes: played.get(&m.appid).map(|p| p.0).filter(|m| *m > 0),
            art: art_for(m.appid, roots, &user_configs),
            launch: LaunchSpec::Steam { appid: m.appid },
            origin: "Steam".into(),
            foreign: false,
        })
        .collect();
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    out
}

/// A library folder the primary Steam lists, in libraryfolders.vdf order (`index` is its key
/// there, the folder index Steam's install dialog uses).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InstallFolder {
    pub index: u32,
    /// As Steam wrote it.
    pub path: String,
    pub label: String,
}

pub fn install_folders() -> Vec<InstallFolder> {
    let roots = find_roots();
    let Some(root) = roots.iter().find(|r| r.primary).or(roots.first()) else { return Vec::new() };
    let mut out = Vec::new();
    for f in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
        let Ok(text) = fs::read_to_string(root.path.join(f)) else { continue };
        let v = vdf::parse(&text);
        let Some(lf) = v.get("libraryfolders") else { continue };
        for (k, entry) in lf.entries() {
            let (Ok(index), Some(path)) = (k.parse::<u32>(), entry.str("path")) else { continue };
            out.push(InstallFolder { index, path: path.to_string(), label: entry.str("label").unwrap_or("").to_string() });
        }
        if !out.is_empty() {
            break;
        }
    }
    if out.is_empty() {
        out.push(InstallFolder { index: 0, path: root.path.to_string_lossy().into_owned(), label: String::new() });
    }
    out
}

/// A Steam download or update in progress (appmanifest StateFlags + BytesDownloaded/BytesToDownload).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    /// "steam:<appid>", as in the library.
    pub id: String,
    pub title: String,
    pub done: u64,
    pub total: u64,
    pub paused: bool,
    /// An update of an installed game (not a first install).
    pub update: bool,
}

/// StateFlags bits: 2 update required, 256 update running, 512 update paused, 1024 update started.
/// Returns the download an appmanifest describes, or None when nothing is being fetched.
pub fn parse_download(st: &vdf::Vdf) -> Option<Download> {
    let appid = st.str("appid")?.parse::<u32>().ok()?;
    let title = st.str("name").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() || is_tool(appid, &title) {
        return None;
    }
    let flags = st.str("StateFlags").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let total = st.str("BytesToDownload").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let done = st.str("BytesDownloaded").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let running = flags & (256 | 1024) != 0;
    let paused = flags & 512 != 0;
    if total == 0 || !(running || paused) {
        return None;
    }
    Some(Download { id: format!("steam:{appid}"), title, done: done.min(total), total, paused, update: flags & 4 != 0 })
}

/// Updates of installed games Steam knows about but has not started (StateFlags 2 with 4, not
/// running or paused): Steam's "unscheduled" list, for when its own list can't be read.
pub fn pending_updates() -> Vec<Download> {
    let mut out = Vec::new();
    for_each_manifest(|st| {
        let Some(appid) = st.str("appid").and_then(|s| s.parse::<u32>().ok()) else { return };
        let title = st.str("name").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
        let flags = st.str("StateFlags").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        if title.is_empty() || is_tool(appid, &title) || flags & 6 != 6 || flags & (256 | 512 | 1024) != 0 {
            return;
        }
        let total = st.str("BytesToDownload").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let done = st.str("BytesDownloaded").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        let id = format!("steam:{appid}");
        if !out.iter().any(|d: &Download| d.id == id) {
            out.push(Download { id, title, done: done.min(total), total, paused: false, update: true });
        }
    });
    out
}

/// Runs `f` on every appmanifest's AppState in every library, once per library folder.
fn for_each_manifest(mut f: impl FnMut(&vdf::Vdf)) {
    let roots = find_roots();
    let mut libs: Vec<PathBuf> = Vec::new();
    for r in &roots {
        libs.extend(registered_libraries(&r.path));
    }
    libs.extend(libraries_on_mounts());
    let mut seen = HashSet::new();
    for lib in libs {
        let canon = fs::canonicalize(&lib).unwrap_or(lib);
        if !seen.insert(canon.clone()) {
            continue;
        }
        let Ok(rd) = fs::read_dir(canon.join("steamapps")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else { continue };
            let v = vdf::parse(&text);
            if let Some(st) = v.get("AppState") {
                f(st);
            }
        }
    }
}

/// Every download in progress across the libraries. Cheap: reads the manifests only.
pub fn downloads() -> Vec<Download> {
    let roots = find_roots();
    let mut libs: Vec<PathBuf> = Vec::new();
    for r in &roots {
        libs.extend(registered_libraries(&r.path));
    }
    libs.extend(libraries_on_mounts());
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for lib in libs {
        let canon = fs::canonicalize(&lib).unwrap_or(lib);
        if !seen.insert(canon.clone()) {
            continue;
        }
        let Ok(rd) = fs::read_dir(canon.join("steamapps")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else { continue };
            let v = vdf::parse(&text);
            if let Some(d) = v.get("AppState").and_then(parse_download) {
                if !out.iter().any(|x: &Download| x.id == d.id) {
                    out.push(d);
                }
            }
        }
    }
    out
}

pub use rare_core::proc::which;

/// Adds a library folder to the primary Steam's libraryfolders.vdf so Steam picks the
/// games up on its next start. Refuses while Steam is running, since Steam rewrites the file.
pub fn add_library(path: &str) -> Result<(), String> {
    if rare_core::proc::steam_running() {
        return Err("Close Steam first, then try again".into());
    }
    let roots = find_roots();
    let root = roots.iter().find(|r| r.primary).ok_or("Steam is not set up for this user yet")?;
    let file = root.path.join("steamapps/libraryfolders.vdf");
    let text = fs::read_to_string(&file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?;
    if text.contains(&format!("\"{}\"", path)) {
        return Ok(());
    }
    let v = vdf::parse(&text);
    let next = v.get("libraryfolders").map(|lf| lf.entries().len()).unwrap_or(0);
    let close = text.rfind('}').ok_or("libraryfolders.vdf looks malformed")?;
    let entry = format!("\t\"{next}\"\n\t{{\n\t\t\"path\"\t\t\"{path}\"\n\t\t\"label\"\t\t\"\"\n\t\t\"contentid\"\t\t\"0\"\n\t\t\"totalsize\"\t\t\"0\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n");
    let backup = file.with_extension("vdf.rare-backup");
    fs::copy(&file, &backup).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(text.len() + entry.len());
    out.push_str(&text[..close]);
    out.push_str(&entry);
    out.push_str(&text[close..]);
    fs::write(&file, out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod download_tests {
    use super::*;

    fn dl(flags: u32, done: u64, total: u64) -> Option<Download> {
        let text = format!(
            "\"AppState\"\n{{\n\"appid\" \"620\"\n\"name\" \"Portal 2\"\n\"StateFlags\" \"{flags}\"\n\"BytesToDownload\" \"{total}\"\n\"BytesDownloaded\" \"{done}\"\n}}\n"
        );
        let v = vdf::parse(&text);
        parse_download(v.get("AppState").unwrap())
    }

    #[test]
    fn running_update_is_a_download() {
        let d = dl(1026, 500, 1000).unwrap();
        assert_eq!((d.id.as_str(), d.done, d.total, d.paused), ("steam:620", 500, 1000, false));
    }
    #[test]
    fn paused_is_kept_and_marked() {
        assert!(dl(512 | 2, 10, 100).unwrap().paused);
    }
    #[test]
    fn installed_games_and_empty_queues_are_not_downloads() {
        assert!(dl(4, 0, 0).is_none());
        assert!(dl(1028, 0, 0).is_none());
    }
}
