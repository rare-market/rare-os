//! Emulators and game frontends from freedesktop desktop entries: EmuDeck launchers in
//! ~/.local/share/applications and Flatpak exports (user and system).

use super::shortcuts::{is_launcher, norm, pretty_name};
use crate::model::{Art, Game, LaunchSpec, Source};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Directories holding desktop entries, user ones first (they override system ones).
pub fn entry_dirs() -> Vec<PathBuf> {
    let h = home();
    vec![
        h.join(".local/share/applications"),
        h.join(".local/share/flatpak/exports/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ]
}

fn icon_dirs() -> Vec<PathBuf> {
    let h = home();
    vec![
        h.join(".local/share/icons"),
        h.join(".local/share/flatpak/exports/share/icons"),
        PathBuf::from("/var/lib/flatpak/exports/share/icons"),
        PathBuf::from("/usr/share/icons"),
    ]
}

/// The [Desktop Entry] group as key/value pairs. Lines may be indented (EmuDeck writes them with a tab).
pub fn parse_entry(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_group = false;
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        if l.starts_with('[') {
            in_group = l.trim_end() == "[Desktop Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            out.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    out
}

/// Splits an Exec line into program and arguments: handles double quotes and backslash
/// escapes, drops field codes (%u %U %f %F %i %c %k) and Flatpak's @@ file-forwarding markers.
pub fn split_exec(exec: &str) -> Option<(String, Vec<String>)> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quoted = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if has || !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        tokens.push(cur);
    }
    let codes = ["%u", "%U", "%f", "%F", "%i", "%c", "%k", "%d", "%D", "%n", "%N", "%v", "%m", "@@", "@@u", "@@U"];
    let mut out: Vec<String> = tokens.into_iter().filter(|t| !codes.contains(&t.as_str())).map(|t| t.replace("%%", "%")).collect();
    if out.is_empty() {
        return None;
    }
    let program = out.remove(0);
    Some((program, out))
}

/// Resolves an Icon= value to a file: an absolute path, or a themed name looked up in hicolor
/// (largest PNG first, then scalable SVG) and pixmaps.
pub fn resolve_icon(icon: &str) -> Option<String> {
    resolve_icon_raw(icon).map(|p| fs::canonicalize(&p).map(|c| c.to_string_lossy().into_owned()).unwrap_or(p))
}

fn resolve_icon_raw(icon: &str) -> Option<String> {
    // Placeholder icons some launchers write instead of the app's own.
    if icon.is_empty() || icon.starts_with("steamdeck-") {
        return None;
    }
    let p = Path::new(icon);
    if p.is_absolute() {
        return p.is_file().then(|| icon.to_string());
    }
    let dirs = icon_dirs();
    for size in [512, 256, 128, 96, 64, 48] {
        for d in &dirs {
            let f = d.join(format!("hicolor/{size}x{size}/apps/{icon}.png"));
            if f.is_file() {
                return Some(f.to_string_lossy().into_owned());
            }
        }
    }
    for d in &dirs {
        let f = d.join(format!("hicolor/scalable/apps/{icon}.svg"));
        if f.is_file() {
            return Some(f.to_string_lossy().into_owned());
        }
    }
    let mut flat: Vec<PathBuf> = dirs.clone();
    flat.push(PathBuf::from("/usr/share/pixmaps"));
    for d in flat {
        for x in ["png", "svg"] {
            let f = d.join(format!("{icon}.{x}"));
            if f.is_file() {
                return Some(f.to_string_lossy().into_owned());
            }
        }
    }
    None
}

const FRONTENDS: &[&str] = &["esde", "emulationstation", "retroarch", "steamrommanager", "emudeck", "scummvm"];

fn entry_to_game(path: &Path, kv: &HashMap<String, String>, flatpak: bool) -> Option<Game> {
    let get = |k: &str| kv.get(k).map(String::as_str).unwrap_or("");
    let yes = |k: &str| get(k).eq_ignore_ascii_case("true");
    if !get("Type").is_empty() && get("Type") != "Application" {
        return None;
    }
    if yes("NoDisplay") || yes("Hidden") {
        return None;
    }
    let file = path.file_name()?.to_string_lossy().to_lowercase();
    let name = get("Name");
    let exec = get("Exec");
    if name.is_empty() || exec.is_empty() || exec.contains("steam://") || file.starts_with("discord") || is_launcher(name, exec) {
        return None;
    }
    let cats: Vec<&str> = get("Categories").split(';').map(str::trim).filter(|c| !c.is_empty()).collect();
    let emudeck = exec.contains("Emulation/tools/launchers/");
    let title = pretty_name(name);
    let frontend = FRONTENDS.contains(&norm(&title).as_str());
    if !(cats.contains(&"Emulator") || emudeck || frontend) {
        return None;
    }
    let (program, args) = split_exec(exec)?;
    let mut art = Art::default();
    art.icon.extend(resolve_icon(get("Icon")));
    let cwd = Some(get("Path")).filter(|p| !p.is_empty()).map(String::from);
    let origin = if emudeck {
        "EmuDeck"
    } else if flatpak {
        "Flatpak"
    } else {
        "Desktop app"
    };
    Some(Game {
        id: super::local::id_for("app", &path.to_string_lossy()),
        source: Source::Apps,
        title,
        installed: true,
        install_dir: None,
        size_bytes: None,
        last_played: None,
        playtime_minutes: None,
        art,
        launch: LaunchSpec::Exec { program, args, cwd, windows: false },
        origin: origin.into(),
        foreign: false,
    })
}

/// Emulators and frontends from desktop entries. A desktop file id seen in a user directory
/// hides the same id in later (system) directories, as the freedesktop spec says.
pub fn scan() -> Vec<Game> {
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for dir in entry_dirs() {
        let flatpak = dir.to_string_lossy().contains("/flatpak/");
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("desktop")).collect();
        files.sort();
        for p in files {
            let id = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !ids.insert(id) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else { continue };
            let Some(g) = entry_to_game(&p, &parse_entry(&text), flatpak) else { continue };
            if names.insert(norm(&g.title)) {
                out.push(g);
            }
        }
    }
    out
}

/// Merges Steam shortcuts into desktop-entry apps: when both exist for the same app, the
/// desktop entry stays (direct launch, no Steam needed) and takes the shortcut's art.
pub fn merge(mut apps: Vec<Game>, shortcuts: Vec<Game>) -> Vec<Game> {
    for s in shortcuts {
        let key = norm(&s.title);
        if let Some(a) = apps.iter_mut().find(|a| norm(&a.title) == key) {
            let mut art = s.art;
            art.icon.splice(0..0, a.art.icon.drain(..));
            // Shortcut grid art goes first for cover/hero/logo/header: desktop entries have none.
            art.cover.extend(a.art.cover.drain(..));
            art.hero.extend(a.art.hero.drain(..));
            art.logo.extend(a.art.logo.drain(..));
            art.header.extend(a.art.header.drain(..));
            a.art = art;
            a.last_played = a.last_played.max(s.last_played);
        } else {
            apps.push(s);
        }
    }
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_indented_entry() {
        let t = "\t[Desktop Entry]\n\tName=ES-DE AppImage\n\tExec=/home/b/Emulation/tools/launchers/es-de/es-de.sh\n\tIcon=/nope.png\n\tCategories=Game;\n\t[Desktop Action x]\n\tName=Other\n";
        let kv = parse_entry(t);
        assert_eq!(kv.get("Name").map(String::as_str), Some("ES-DE AppImage"));
        let g = entry_to_game(Path::new("/x/es-de.desktop"), &kv, false).unwrap();
        assert_eq!(g.title, "ES-DE");
        assert_eq!(g.origin, "EmuDeck");
    }

    #[test]
    fn filters_entries() {
        let mk = |body: &str| parse_entry(&format!("[Desktop Entry]\nType=Application\n{body}"));
        let p = Path::new("/x/a.desktop");
        assert!(entry_to_game(p, &mk("Name=ProtonPlus\nExec=protonplus\nCategories=Game;Utility;"), true).is_none());
        assert!(entry_to_game(p, &mk("Name=Hades\nExec=steam steam://rungameid/1\nCategories=Game;"), false).is_none());
        assert!(entry_to_game(p, &mk("Name=Dolphin\nExec=dolphin\nNoDisplay=true\nCategories=Game;Emulator;"), true).is_none());
        assert!(entry_to_game(p, &mk("Name=Dolphin\nExec=dolphin\nCategories=Game;Emulator;"), true).is_some());
        assert!(entry_to_game(p, &mk("Name=RetroArch\nExec=retroarch\nCategories=Game;"), true).is_some());
        assert!(entry_to_game(Path::new("/x/discord-1.desktop"), &mk("Name=X\nExec=x\nCategories=Emulator;"), false).is_none());
    }

    #[test]
    fn splits_exec() {
        let (p, a) = split_exec(r#"/usr/bin/flatpak run --branch=stable --command=xemu app.xemu.xemu @@u %U @@"#).unwrap();
        assert_eq!(p, "/usr/bin/flatpak");
        assert_eq!(a, vec!["run", "--branch=stable", "--command=xemu", "app.xemu.xemu"]);
        let (p, a) = split_exec(r#""/home/b/My Apps/run.sh" "a b" 100%% %f"#).unwrap();
        assert_eq!(p, "/home/b/My Apps/run.sh");
        assert_eq!(a, vec!["a b", "100%"]);
    }
}
