//! What the player changed about a game: its name and its cover, hero and logo art (kept per
//! game in ~/.config/rare/game-custom.json and put over every scan), and games they added
//! themselves (a manifest in ~/.local/share/rare/games/, read by `local::scan_local`).

use crate::model::{Game, Library};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One game's changes. Art is a URL or a local file path.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", default)]
pub struct Custom {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

impl Custom {
    pub fn is_empty(&self) -> bool {
        *self == Custom::default()
    }
}

/// Which picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArtKind {
    Cover,
    Hero,
    Logo,
}

impl ArtKind {
    pub fn slot(self, c: &mut Custom) -> &mut Option<String> {
        match self {
            ArtKind::Cover => &mut c.cover,
            ArtKind::Hero => &mut c.hero,
            ArtKind::Logo => &mut c.logo,
        }
    }
}

pub fn path() -> PathBuf {
    rare_core::paths::config_dir().join("game-custom.json")
}

pub fn load_from(p: &Path) -> BTreeMap<String, Custom> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn load() -> BTreeMap<String, Custom> {
    load_from(&path())
}

pub fn save_to(p: &Path, m: &BTreeMap<String, Custom>) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

/// Changes one game's entry (an empty entry is dropped) and saves the file.
pub fn update(id: &str, f: impl FnOnce(&mut Custom)) -> Result<Custom, String> {
    let p = path();
    let mut m = load_from(&p);
    let mut c = m.remove(id).unwrap_or_default();
    f(&mut c);
    // Blank values mean "the original".
    for v in [&mut c.title, &mut c.cover, &mut c.hero, &mut c.logo] {
        if v.as_deref().is_some_and(|s| s.trim().is_empty()) {
            *v = None;
        }
    }
    if !c.is_empty() {
        m.insert(id.to_string(), c.clone());
    }
    save_to(&p, &m)?;
    Ok(c)
}

/// Puts one game's changes over it: the name, and each picture first in its list (the original
/// stays behind it, so a picture that fails to load falls back to it).
pub fn apply_one(g: &mut Game, c: &Custom) {
    if let Some(t) = c.title.as_ref().filter(|t| !t.trim().is_empty()) {
        g.title = t.trim().to_string();
    }
    for (pick, list) in [(&c.cover, &mut g.art.cover), (&c.hero, &mut g.art.hero), (&c.logo, &mut g.art.logo)] {
        if let Some(p) = pick.as_ref().filter(|p| !p.is_empty()) {
            list.retain(|x| x != p);
            list.insert(0, p.clone());
        }
    }
    // The game page falls back to the header when there is no hero: a custom hero wins there too.
    if let Some(h) = c.hero.as_ref().filter(|p| !p.is_empty()) {
        g.art.header.retain(|x| x != h);
        g.art.header.insert(0, h.clone());
    }
}

pub fn apply(lib: &mut Library, m: &BTreeMap<String, Custom>) {
    if m.is_empty() {
        return;
    }
    for g in lib.games.iter_mut() {
        if let Some(c) = m.get(&g.id) {
            apply_one(g, c);
        }
    }
}

// ---------------------------------------------------------------- games the player added

/// What the player picked: a Windows .exe, an AppImage, a shell script or a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    Exe,
    AppImage,
    Script,
    Folder,
    Program,
}

/// What a file is, by its name (None: not something that starts a game).
pub fn kind_of(p: &Path) -> Option<PickKind> {
    if p.is_dir() {
        return Some(PickKind::Folder);
    }
    let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "exe" => Some(PickKind::Exe),
        "appimage" => Some(PickKind::AppImage),
        "sh" => Some(PickKind::Script),
        "x86_64" | "x86-64" | "amd64" => Some(PickKind::Program),
        _ => None,
    }
}

/// "hollow_knight-v1.5.exe" -> "Hollow Knight", "Celeste.x86_64" -> "Celeste".
pub fn title_from(p: &Path) -> String {
    let raw = if p.is_dir() { p.file_name() } else { p.file_stem() }.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let spaced: String = raw.chars().map(|c| if matches!(c, '_' | '.' | '-' | '+') { ' ' } else { c }).collect();
    let noise = ["x86", "x64", "amd64", "linux", "win64", "win32", "windows", "setup", "launcher", "shipping", "portable", "appimage"];
    let toks: Vec<&str> = spaced.split_whitespace().collect();
    let digits = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit());
    let mut words: Vec<String> = Vec::new();
    let mut in_version = false;
    for (i, w) in toks.iter().enumerate() {
        let lw = w.to_lowercase();
        let tagged = lw.starts_with('v') && lw[1..].chars().next().is_some_and(|c| c.is_ascii_digit());
        // "1 4" from "1.4" is a version; a lone "2" ("Hades 2") is part of the name.
        let run = digits(w) && (in_version || toks.get(i + 1).is_some_and(|n| digits(n)) || i > 0 && digits(toks[i - 1]));
        let arch = lw == "64" && i > 0 && toks[i - 1].eq_ignore_ascii_case("x86");
        in_version = tagged || run;
        if noise.contains(&lw.as_str()) || tagged || run || arch {
            continue;
        }
        words.push(w.to_string());
    }
    // A single lowercase run of words gets capitals; names that already have some keep theirs.
    let any_upper = words.iter().any(|w| w.chars().any(|c| c.is_uppercase()));
    let words: Vec<String> = if any_upper {
        words
    } else {
        words.iter().map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }).collect()
    };
    let t = words.join(" ");
    if t.is_empty() { raw } else { t }
}

/// The manifest a pick becomes: (program, args, working folder, through Proton).
pub fn launch_for(p: &Path) -> Result<(String, Vec<String>, Option<String>, bool), String> {
    let kind = kind_of(p).ok_or("That file can't start a game. Pick an .exe, an AppImage, a .sh script or a folder.")?;
    let parent = |x: &Path| x.parent().map(|d| d.to_string_lossy().into_owned());
    match kind {
        PickKind::Folder => {
            let (prog, windows) = crate::local::pick_program(p).ok_or("There's no game program in that folder. Pick the .exe, AppImage or script inside it.")?;
            if !windows {
                make_executable(&prog);
            }
            Ok((prog.to_string_lossy().into_owned(), vec![], Some(p.to_string_lossy().into_owned()), windows))
        }
        PickKind::Exe => Ok((p.to_string_lossy().into_owned(), vec![], parent(p), true)),
        _ => {
            make_executable(p);
            Ok((p.to_string_lossy().into_owned(), vec![], parent(p), false))
        }
    }
}

fn make_executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(p) {
            let mode = m.permissions().mode();
            if mode & 0o111 == 0 {
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | 0o755));
            }
        }
    }
}

#[derive(Serialize)]
struct Manifest<'a> {
    title: &'a str,
    exec: &'a str,
    args: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
    windows: bool,
}

/// Where added games' manifests go.
pub fn manifest_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| rare_core::paths::home().join(".local/share")).join("rare/games")
}

/// Adds a game from a picked file or folder. Returns its library id and title.
pub fn add_game(picked: &Path) -> Result<(String, String), String> {
    add_game_in(&manifest_dir(), picked)
}

pub fn add_game_in(dir: &Path, picked: &Path) -> Result<(String, String), String> {
    let (exec, args, cwd, windows) = launch_for(picked)?;
    let title = title_from(picked);
    let id = crate::local::id_for("local", &exec);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let slug: String = title.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let file = dir.join(format!("{}-{}.json", slug.trim_matches('-'), id.trim_start_matches("local:")));
    let m = Manifest { title: &title, exec: &exec, args: &args, cwd: cwd.as_deref(), windows };
    std::fs::write(&file, serde_json::to_vec_pretty(&m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok((id, title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Art, LaunchSpec, Source};

    fn game() -> Game {
        Game {
            id: "steam:1".into(),
            source: Source::Steam,
            title: "Old".into(),
            installed: true,
            install_dir: None,
            size_bytes: None,
            last_played: None,
            playtime_minutes: None,
            art: Art { cover: vec!["c0".into()], hero: vec!["h0".into()], header: vec!["hd".into()], ..Default::default() },
            launch: LaunchSpec::Steam { appid: 1 },
            origin: String::new(),
            foreign: false,
        }
    }

    #[test]
    fn overrides_go_first_and_keep_the_original_behind() {
        let mut g = game();
        apply_one(&mut g, &Custom { title: Some(" New ".into()), cover: Some("c1".into()), hero: Some("h1".into()), logo: Some("l1".into()) });
        assert_eq!(g.title, "New");
        assert_eq!(g.art.cover, vec!["c1", "c0"]);
        assert_eq!(g.art.hero, vec!["h1", "h0"]);
        assert_eq!(g.art.header, vec!["h1", "hd"]);
        assert_eq!(g.art.logo, vec!["l1"]);
        // Applying twice changes nothing more.
        let before = g.art.cover.clone();
        apply_one(&mut g, &Custom { cover: Some("c1".into()), ..Default::default() });
        assert_eq!(g.art.cover, before);
    }

    #[test]
    fn titles_from_file_names() {
        assert_eq!(title_from(Path::new("/g/hollow_knight-v1.5.exe")), "Hollow Knight");
        assert_eq!(title_from(Path::new("/g/Celeste.x86_64")), "Celeste");
        assert_eq!(title_from(Path::new("/g/SuperTuxKart-1.4-linux-x86_64.AppImage")), "SuperTuxKart");
        assert_eq!(title_from(Path::new("/g/Hades 2")), "Hades 2");
        assert_eq!(title_from(Path::new("/g/start.sh")), "Start");
    }

    #[test]
    fn picks_become_manifests() {
        let d = tempfile::tempdir().unwrap();
        let games = d.path().join("Games/Cave Story");
        std::fs::create_dir_all(&games).unwrap();
        let exe = games.join("Doukutsu.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let sh = games.join("run.sh");
        std::fs::write(&sh, b"#!/bin/sh\n").unwrap();
        let out = d.path().join("manifests");
        let (id, title) = add_game_in(&out, &exe).unwrap();
        assert_eq!(title, "Doukutsu");
        assert!(id.starts_with("local:"));
        let (_, prog, _, windows) = {
            let (p, a, c, w) = launch_for(&exe).unwrap();
            (a, p, c, w)
        };
        assert!(windows && prog.ends_with("Doukutsu.exe"));
        // A folder picks its program; a script becomes executable.
        let (prog, _, cwd, windows) = launch_for(&games).unwrap();
        assert!(prog.ends_with("run.sh") && !windows);
        assert_eq!(cwd.as_deref(), Some(games.to_string_lossy().as_ref()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert!(std::fs::metadata(&sh).unwrap().permissions().mode() & 0o111 != 0);
        }
        assert!(launch_for(&games.join("readme.txt")).is_err());
        let written: serde_json::Value = serde_json::from_slice(&std::fs::read(std::fs::read_dir(&out).unwrap().next().unwrap().unwrap().path()).unwrap()).unwrap();
        assert_eq!(written["windows"], true);
        assert_eq!(written["title"], "Doukutsu");
    }

    #[test]
    fn stored_per_game() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.json");
        let mut m = BTreeMap::new();
        m.insert("steam:1".to_string(), Custom { logo: Some("/x.png".into()), ..Default::default() });
        save_to(&p, &m).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap().contains("\"logo\": \"/x.png\""), true);
        assert_eq!(load_from(&p), m);
    }
}
