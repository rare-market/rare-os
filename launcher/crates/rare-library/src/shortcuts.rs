//! Non-Steam shortcuts: userdata/<user>/config/shortcuts.vdf of every Steam install,
//! in Valve's binary KeyValues format. They launch through Steam (steam://rungameid/<gameid>)
//! so Steam Input and the overlay work as they do from Steam itself.

use super::steam::SteamRoot;
use crate::model::{Art, Game, LaunchSpec, Source};
use std::fs;
use std::path::{Path, PathBuf};

/// A binary KeyValues value.
#[derive(Debug, Clone, PartialEq)]
pub enum Bin {
    Obj(Vec<(String, Bin)>),
    Str(String),
    Int(i64),
    Float(f32),
}

impl Bin {
    pub fn get(&self, key: &str) -> Option<&Bin> {
        match self {
            Bin::Obj(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Bin::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(Bin::Int(i)) => Some(*i),
            Some(Bin::Str(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn entries(&self) -> &[(String, Bin)] {
        match self {
            Bin::Obj(items) => items,
            _ => &[],
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let v = *self.b.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn cstr(&mut self) -> Option<String> {
        let rest = self.b.get(self.i..)?;
        let end = rest.iter().position(|&c| c == 0)?;
        let s = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.i += end + 1;
        Some(s)
    }
    fn wstr(&mut self) -> Option<String> {
        let mut units = Vec::new();
        loop {
            let u = u16::from_le_bytes(self.take::<2>()?);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        Some(String::from_utf16_lossy(&units))
    }
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s: [u8; N] = self.b.get(self.i..self.i + N)?.try_into().ok()?;
        self.i += N;
        Some(s)
    }
    /// Reads key/value pairs until the end marker. Stops quietly on malformed input.
    fn obj(&mut self, depth: usize) -> Vec<(String, Bin)> {
        let mut items = Vec::new();
        while let Some(t) = self.byte() {
            if t == 0x08 || t == 0x0b {
                break;
            }
            let Some(key) = self.cstr() else { break };
            let v = match t {
                0x00 if depth < 32 => Bin::Obj(self.obj(depth + 1)),
                0x01 => match self.cstr() {
                    Some(s) => Bin::Str(s),
                    None => break,
                },
                0x02 | 0x04 | 0x06 => match self.take::<4>() {
                    Some(b) => Bin::Int(i32::from_le_bytes(b) as i64),
                    None => break,
                },
                0x03 => match self.take::<4>() {
                    Some(b) => Bin::Float(f32::from_le_bytes(b)),
                    None => break,
                },
                // Wide string (UTF-16LE up to a 0x0000 unit): rare, but schema files may carry one.
                0x05 => match self.wstr() {
                    Some(s) => Bin::Str(s),
                    None => break,
                },
                0x07 | 0x0a => match self.take::<8>() {
                    Some(b) => Bin::Int(i64::from_le_bytes(b)),
                    None => break,
                },
                _ => break,
            };
            items.push((key, v));
        }
        items
    }
}

/// Parses a binary KeyValues file. Never fails: malformed input yields whatever parsed cleanly.
pub fn parse(bytes: &[u8]) -> Bin {
    Bin::Obj(Reader { b: bytes, i: 0 }.obj(0))
}

/// Every userdata/<user>/config directory of the given Steam installs.
pub fn user_config_dirs(roots: &[SteamRoot]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for r in roots {
        let Ok(users) = fs::read_dir(r.path.join("userdata")) else { continue };
        for u in users.flatten() {
            let c = u.path().join("config");
            if c.is_dir() {
                out.push(c);
            }
        }
    }
    out
}

/// The first real image for `stem` in the grid folder. Tiny files are placeholders some tools
/// write (blank 1x1 PNGs), so they don't count.
fn first_existing(dir: &Path, stem: &str) -> Option<String> {
    ["png", "jpg", "jpeg", "webp"]
        .iter()
        .map(|x| dir.join(format!("{stem}.{x}")))
        .find(|p| fs::metadata(p).map(|m| m.is_file() && m.len() > 1024).unwrap_or(false))
        .map(|p| fs::canonicalize(&p).unwrap_or(p).to_string_lossy().into_owned())
}

/// Custom art the user set in Steam (config/grid/), for a Steam appid or a shortcut's 32-bit id.
pub fn grid_art(config_dirs: &[PathBuf], appid: u32) -> Art {
    let mut art = Art::default();
    for c in config_dirs {
        let grid = c.join("grid");
        if !grid.is_dir() {
            continue;
        }
        art.cover.extend(first_existing(&grid, &format!("{appid}p")));
        art.hero.extend(first_existing(&grid, &format!("{appid}_hero")));
        art.logo.extend(first_existing(&grid, &format!("{appid}_logo")));
        art.header.extend(first_existing(&grid, &format!("{appid}")));
        art.icon.extend(first_existing(&grid, &format!("{appid}_icon")));
    }
    art
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t).to_string()
}

/// True for Rare Launcher's own shortcut (Steam users add it to launch Rare from Game Mode).
pub fn is_launcher(name: &str, exec: &str) -> bool {
    let n = name.trim().to_lowercase();
    let e = exec.to_lowercase();
    n == "rare" || n == "rare launcher" || e.contains("rare-launcher")
}

fn is_version_or_arch(seg: &str) -> bool {
    let s = seg.to_lowercase();
    let v = s.strip_prefix('v').unwrap_or(&s);
    let version = v.contains('.') && v.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    version
        || [
            "x86_64", "x86-64", "x64", "amd64", "aarch64", "arm64", "i386", "i686", "linux", "linux64", "anylinux", "appimage", "portable",
        ]
        .contains(&s.as_str())
}

/// Turns a file-ish shortcut name into a title: "Citron-0.12.25-anylinux-x86_64_v3.AppImage" -> "Citron".
pub fn pretty_name(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for suffix in [".appimage", " appimage"] {
        if s.to_lowercase().ends_with(suffix) {
            s.truncate(s.len() - suffix.len());
        }
    }
    // Cut at the first "-<version>" or "-<arch>" segment.
    let mut cut = s.len();
    let mut pos = 0;
    for (i, seg) in s.split('-').enumerate() {
        if i > 0 {
            let first = seg.split('_').next().unwrap_or(seg);
            if is_version_or_arch(seg) || is_version_or_arch(first) {
                cut = pos - 1;
                break;
            }
        }
        pos += seg.len() + 1;
    }
    s.truncate(cut);
    let mut s = s.trim().to_string();
    for suffix in [" binary", "-qt", " qt"] {
        if s.to_lowercase().ends_with(suffix) && s.len() > suffix.len() {
            s.truncate(s.len() - suffix.len());
        }
    }
    // "Steam-ROM-Manager" and "Model-2-Emulator" read as words; short tags like "ES-DE" stay.
    let wordy = |t: &str| t.matches('-').count() >= 2 && t.split('-').any(|w| w.len() >= 5);
    if !s.contains(' ') && wordy(&s) {
        s = s.replace('-', " ");
    } else if let Some((a, rest)) = s.split_once(" (") {
        if wordy(a) {
            s = format!("{} ({}", a.replace('-', " "), rest);
        }
    }
    match norm(&s).as_str() {
        "esde" => "ES-DE".into(),
        _ if s.is_empty() => raw.trim().to_string(),
        _ => s,
    }
}

/// Name key for matching the same app across shortcuts and desktop entries.
pub fn norm(name: &str) -> String {
    let n: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    let n = n.strip_suffix("appimage").map(String::from).unwrap_or(n);
    match n.as_str() {
        "emulationstationde" | "esde" | "emulationstationdesktopedition" => "esde".into(),
        _ => n,
    }
}

/// The flatpak app id in a shortcut's launch options ("run" ... "com.google.Chrome" ...).
fn flatpak_id(opts: &str) -> Option<String> {
    opts.split_whitespace()
        .map(|t| t.trim_matches('"'))
        .find(|t| !t.starts_with('-') && t.split('.').count() >= 3 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'))
        .map(String::from)
}

/// Emulators, frontends and their tools, by normalised name. Matched as a prefix so
/// "Dolphin Emulator", "RPCS3 (Proton)" and "Xenia Canary" all count.
const EMULATORS: &[&str] = &[
    "esde", "emulationstation", "retroarch", "citron", "ryujinx", "yuzu", "eden", "sudachi", "torzu", "suyu", "xemu", "xenia", "dolphin", "primehack",
    "cemu", "rpcs3", "pcsx2", "duckstation", "ppsspp", "azahar", "citra", "lime3ds", "melonds", "shadps4", "vita3k", "scummvm", "mame", "flycast",
    "redream", "model2", "supermodel", "steamrommanager", "emudeck", "mgba", "snes9x", "bsnes", "mednafen", "ares", "bigpemu", "rmg", "mupen64",
    "simple64", "desmume", "dosbox", "pegasus", "gzdoom",
];

/// Browsers and tools people add to Steam to use in Game Mode: apps, not games.
const APPS: &[&str] = &["googlechrome", "chrome", "chromium", "firefox", "brave", "microsoftedge", "librewolf", "vivaldi", "opera", "discord", "spotify", "kodi", "moonlight", "chiaki", "greenlight", "heroic", "lutris", "bottles", "protonplus", "protonup", "decky", "youtube", "netflix", "twitch", "konsole", "dolphinfilemanager"];

/// True when a shortcut starts an emulator, a frontend or an app rather than a game.
pub fn is_app(title: &str, exe: &str) -> bool {
    let n = norm(title);
    let file = norm(Path::new(exe.trim_matches('"')).file_stem().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default().as_str());
    EMULATORS.iter().any(|e| n.starts_with(e) || (!file.is_empty() && file.starts_with(e))) || APPS.iter().any(|a| n == *a || n.starts_with(a))
}

/// Steam's 64-bit game id for a non-Steam shortcut.
pub fn shortcut_gameid(appid: u32) -> u64 {
    ((appid as u64) << 32) | 0x0200_0000
}

pub fn scan(roots: &[SteamRoot]) -> Vec<Game> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let configs = user_config_dirs(roots);
    for c in &configs {
        let Ok(bytes) = fs::read(c.join("shortcuts.vdf")) else { continue };
        let root = parse(&bytes);
        let Some(list) = root.get("shortcuts") else { continue };
        for (_, s) in list.entries() {
            let name = s.str("AppName").or_else(|| s.str("appname")).unwrap_or("").to_string();
            let exe = unquote(s.str("Exe").unwrap_or(""));
            let opts = s.str("LaunchOptions").unwrap_or("");
            if name.trim().is_empty() || s.int("IsHidden").unwrap_or(0) != 0 {
                continue;
            }
            if is_launcher(&name, &format!("{exe} {opts}")) {
                continue;
            }
            let Some(appid) = s.int("appid").map(|a| a as u32) else { continue };
            if !seen.insert(appid) {
                continue;
            }
            // Art from the grid folder of the user who owns this shortcut first.
            let mut dirs = vec![c.clone()];
            dirs.extend(configs.iter().filter(|d| *d != c).cloned());
            let mut art = grid_art(&dirs, appid);
            let icon = unquote(s.str("icon").unwrap_or(""));
            if !icon.is_empty() && Path::new(&icon).is_file() {
                art.icon.push(icon);
            } else if let Some(id) = flatpak_id(opts) {
                // Flatpak shortcuts: the app's own icon.
                art.icon.extend(super::apps::resolve_icon(&id));
            }
            let start = unquote(s.str("StartDir").unwrap_or(""));
            let title = pretty_name(&name);
            let source = if is_app(&title, &exe) { Source::Apps } else { Source::Local };
            out.push(Game {
                id: format!("shortcut:{appid}"),
                source,
                title,
                installed: true,
                install_dir: (!start.is_empty()).then_some(start),
                size_bytes: None,
                last_played: s.int("LastPlayTime").map(|t| t as u32 as u64).filter(|t| *t > 0),
                playtime_minutes: None,
                art,
                launch: LaunchSpec::SteamShortcut { gameid: shortcut_gameid(appid) },
                origin: "Steam shortcut".into(),
                foreign: false,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(buf: &mut Vec<u8>, idx: &str, appid: i32, name: &str, exe: &str, hidden: i32) {
        buf.push(0);
        buf.extend(idx.as_bytes());
        buf.push(0);
        buf.push(2);
        buf.extend(b"appid\0");
        buf.extend(appid.to_le_bytes());
        buf.push(1);
        buf.extend(b"AppName\0");
        buf.extend(name.as_bytes());
        buf.push(0);
        buf.push(1);
        buf.extend(b"Exe\0");
        buf.extend(exe.as_bytes());
        buf.push(0);
        buf.push(2);
        buf.extend(b"IsHidden\0");
        buf.extend(hidden.to_le_bytes());
        buf.push(0);
        buf.extend(b"tags\0");
        buf.push(1);
        buf.extend(b"0\0favorite\0");
        buf.push(8);
        buf.push(8);
    }

    #[test]
    fn parses_binary_shortcuts() {
        let mut b = vec![0];
        b.extend(b"shortcuts\0");
        entry(&mut b, "0", -1_000_000, "xemu", "\"/usr/bin/flatpak\"", 0);
        entry(&mut b, "1", 5, "Hidden", "x", 1);
        b.extend([8, 8]);
        let v = parse(&b);
        let list = v.get("Shortcuts").unwrap().entries();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].1.str("appname"), Some("xemu"));
        assert_eq!(list[0].1.int("APPID").map(|a| a as u32), Some((-1_000_000i32) as u32));
        assert_eq!(list[1].1.int("IsHidden"), Some(1));
        assert_eq!(unquote(list[0].1.str("exe").unwrap()), "/usr/bin/flatpak");
    }

    #[test]
    fn prettifies_names() {
        assert_eq!(pretty_name("Citron-0.12.25-anylinux-x86_64_v3.AppImage"), "Citron");
        assert_eq!(pretty_name("EmulationStationDE"), "ES-DE");
        assert_eq!(pretty_name("ES-DE AppImage"), "ES-DE");
        assert_eq!(pretty_name("Yu-Gi-Oh!"), "Yu-Gi-Oh!");
        assert_eq!(pretty_name("Google Chrome"), "Google Chrome");
        assert_eq!(pretty_name("RPCS3-v0.0.32-x86_64.AppImage"), "RPCS3");
        assert_eq!(pretty_name("Steam-ROM-Manager"), "Steam ROM Manager");
        assert_eq!(pretty_name("Model-2-Emulator (Proton)"), "Model 2 Emulator (Proton)");
        assert_eq!(pretty_name("Ryujinx Binary"), "Ryujinx");
        assert_eq!(pretty_name("PCSX2-QT"), "PCSX2");
        assert_eq!(flatpak_id(r#""run" "--branch=stable" "--arch=x86_64" "--command=/app/bin/chrome" "--file-forwarding" "com.google.Chrome" "@@u" "@@" --window-size=1024,640"#).as_deref(), Some("com.google.Chrome"));
        assert_eq!(norm("EmulationStationDE"), norm("ES-DE"));
    }

    #[test]
    fn gameid() {
        assert_eq!(shortcut_gameid(4072284071), (4072284071u64 << 32) | 0x0200_0000);
    }
}
