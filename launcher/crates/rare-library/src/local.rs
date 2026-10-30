//! Local games: hand-added manifests, Games folders (on this OS, on other homes and on
//! mounted drives) and itch.io downloads.
//!
//! A manifest is a JSON file in ~/.local/share/rare/games/ (or /usr/share/rare/games/):
//! { "title": "Celeste", "exec": "/home/me/Games/Celeste/Celeste", "args": [], "windows": false, "cover": "/path/cover.jpg" }

use crate::model::{Art, Game, LaunchSpec, Source};
use serde::Deserialize;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Manifest {
    title: String,
    exec: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    windows: bool,
    cover: Option<String>,
    hero: Option<String>,
}

pub(crate) fn id_for(prefix: &str, path: &str) -> String {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    format!("{prefix}:{:016x}", h.finish())
}

pub(crate) fn manifest_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/usr/share/rare/games")];
    if let Some(d) = dirs::data_dir() {
        v.push(d.join("rare/games"));
    }
    v
}

fn read_manifests() -> Vec<Game> {
    let mut out = Vec::new();
    for dir in manifest_dirs() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(m) = fs::read_to_string(e.path()).map_err(|_| ()).and_then(|t| serde_json::from_str::<Manifest>(&t).map_err(|_| ())) else { continue };
            let mut art = Art::default();
            art.cover.extend(m.cover.clone());
            art.hero.extend(m.hero.clone());
            out.push(Game {
                id: id_for("local", &m.exec),
                source: Source::Local,
                title: m.title,
                installed: Path::new(&m.exec).exists(),
                install_dir: Path::new(&m.exec).parent().map(|p| p.to_string_lossy().into_owned()),
                size_bytes: None,
                last_played: None,
                playtime_minutes: None,
                art,
                launch: LaunchSpec::Exec { program: m.exec, args: m.args, cwd: m.cwd, windows: m.windows },
                origin: "Added by you".into(),
                foreign: false,
            });
        }
    }
    out
}

fn is_noise(name: &str) -> bool {
    let n = name.to_lowercase();
    ["unins", "setup", "install", "crash", "redist", "vcredist", "dxsetup", "launcherhelper", "update", "uninstall", "config"].iter().any(|w| n.contains(w))
}

fn is_exec(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Picks the program a game folder most likely starts with. Returns (path, is_windows).
pub fn pick_program(dir: &Path) -> Option<(PathBuf, bool)> {
    let entries: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = |p: &PathBuf| p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ranked = |pred: &dyn Fn(&PathBuf) -> bool| entries.iter().filter(|p| pred(p) && !is_noise(&name(p))).cloned().collect::<Vec<_>>();

    if let Some(p) = entries.iter().find(|p| name(p) == "start.sh") {
        return Some((p.clone(), false));
    }
    for set in [
        ranked(&|p| ext(p) == "appimage"),
        ranked(&|p| matches!(ext(p).as_str(), "x86_64" | "x86-64" | "amd64")),
        ranked(&|p| ext(p) == "sh"),
        ranked(&|p| ext(p).is_empty() && is_exec(p)),
    ] {
        if let Some(p) = set.into_iter().next() {
            return Some((p, false));
        }
    }
    let exes = ranked(&|p| ext(p) == "exe");
    // Prefer the exe named like its folder ("Hollow Knight/hollow_knight.exe").
    let folder = dir.file_name()?.to_string_lossy().to_lowercase().replace([' ', '_', '-'], "");
    let best = exes
        .iter()
        .find(|p| name(p).to_lowercase().replace([' ', '_', '-'], "").starts_with(&folder[..folder.len().min(5)]))
        .or_else(|| exes.first())?;
    Some((best.clone(), true))
}

fn local_art(dir: &Path) -> Art {
    let mut art = Art::default();
    for n in ["cover.jpg", "cover.png", "folder.jpg", "poster.jpg", "boxart.png", "boxart.jpg"] {
        let p = dir.join(n);
        if p.is_file() {
            art.cover.push(p.to_string_lossy().into_owned());
        }
    }
    for n in ["hero.jpg", "hero.png", "background.jpg"] {
        let p = dir.join(n);
        if p.is_file() {
            art.hero.push(p.to_string_lossy().into_owned());
        }
    }
    art
}

fn pretty_title(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().replace(['_', '.'], " ")).unwrap_or_default().trim().to_string()
}

/// Folders that hold one game per subfolder.
fn game_folders(extra: &[String]) -> Vec<(PathBuf, String)> {
    let mut v: Vec<(PathBuf, String)> = Vec::new();
    let mut homes: Vec<PathBuf> = dirs::home_dir().into_iter().collect();
    homes.extend(super::roots::other_homes());
    for (i, h) in homes.iter().enumerate() {
        let label = if i == 0 { "Games folder".to_string() } else { format!("Games in {}", h.display()) };
        v.push((h.join("Games"), label));
    }
    for m in super::roots::mount_points() {
        for sub in ["Games", "games"] {
            v.push((m.join(sub), format!("Games on {}", m.display())));
        }
    }
    for e in extra {
        v.push((PathBuf::from(e), format!("Folder {e}")));
    }
    v
}

pub fn scan_local(extra: &[String], known_dirs: &[String]) -> Vec<Game> {
    let mut out = read_manifests();
    let known: Vec<PathBuf> = known_dirs.iter().map(|d| fs::canonicalize(d).unwrap_or(PathBuf::from(d))).collect();
    let mut seen = std::collections::HashSet::new();
    for (folder, origin) in game_folders(extra) {
        let Ok(rd) = fs::read_dir(&folder) else { continue };
        for e in rd.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            // Store-managed folders are listed by their store.
            if name.starts_with('.') || ["Heroic", "SteamLibrary", "Steam", "lutris", "itch", "Rare", "Epic", "GOG"].contains(&name.as_str()) {
                continue;
            }
            let canon = fs::canonicalize(&dir).unwrap_or(dir.clone());
            if known.iter().any(|k| canon.starts_with(k) || k.starts_with(&canon)) || !seen.insert(canon.clone()) {
                continue;
            }
            let Some((program, windows)) = pick_program(&dir) else { continue };
            let program = program.to_string_lossy().into_owned();
            out.push(Game {
                id: id_for("local", &program),
                source: Source::Local,
                title: pretty_title(&dir),
                installed: true,
                install_dir: Some(canon.to_string_lossy().into_owned()),
                size_bytes: None,
                last_played: None,
                playtime_minutes: None,
                art: local_art(&dir),
                launch: LaunchSpec::Exec { program, args: vec![], cwd: Some(dir.to_string_lossy().into_owned()), windows },
                origin: origin.clone(),
                foreign: false,
            });
        }
    }
    out
}

/// itch.io app downloads: ~/.config/itch/apps/<game>/
pub fn scan_itch() -> Vec<Game> {
    let mut out = Vec::new();
    // itch's own folder, and Games/itch on drives (where Settings, Storage moves itch games).
    let mut places: Vec<PathBuf> = dirs::config_dir().map(|c| c.join("itch/apps")).into_iter().collect();
    places.extend(super::roots::mount_points().into_iter().map(|m| m.join("Games/itch")));
    for e in places.iter().filter_map(|p| fs::read_dir(p).ok()).flat_map(|rd| rd.flatten()) {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        // itch keeps the actual build one level down sometimes.
        let target = pick_program(&dir).map(|p| (dir.clone(), p)).or_else(|| {
            fs::read_dir(&dir).ok()?.flatten().map(|s| s.path()).filter(|p| p.is_dir() && !p.ends_with(".itch")).find_map(|s| pick_program(&s).map(|p| (s, p)))
        });
        let Some((gdir, (program, windows))) = target else { continue };
        let program = program.to_string_lossy().into_owned();
        out.push(Game {
            id: id_for("itch", &program),
            source: Source::Itch,
            title: pretty_title(&dir),
            installed: true,
            install_dir: Some(dir.to_string_lossy().into_owned()),
            size_bytes: None,
            last_played: None,
            playtime_minutes: None,
            art: local_art(&gdir),
            launch: LaunchSpec::Exec { program, args: vec![], cwd: Some(gdir.to_string_lossy().into_owned()), windows },
            origin: "itch.io".into(),
            foreign: false,
        });
    }
    out
}
