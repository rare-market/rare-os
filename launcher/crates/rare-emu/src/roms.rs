//! ROM folders: ~/Emulation/roms/<system> on the device and Emulation/roms/<system> on the SD
//! card (EmuDeck's layout, so an existing setup shows at once). Each game becomes a library entry
//! (`emu:<folder>/<path>`, `emu:sd/<folder>/<path>` on the card) that starts in its system's
//! emulator. Multi-disc games show once; their discs go in a playlist Rare keeps in its cache.

use crate::catalog;
use crate::retroarch;
use crate::systems::{self, Runner, System};
use rare_library::model::{Art, Game, LaunchSpec, Source};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// A ROMs folder: the device's or the SD card's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// "" for the device, "sd" for the card.
    pub tag: &'static str,
    /// The `Emulation` folder (roms, bios, saves under it).
    pub base: PathBuf,
}

impl Root {
    pub fn roms(&self) -> PathBuf {
        self.base.join("roms")
    }
    pub fn label(&self) -> &'static str {
        if self.tag == "sd" {
            "SD card"
        } else {
            "This device"
        }
    }
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// The device's Emulation folder, and the card's when one is in.
pub fn roots(home: &Path) -> Vec<Root> {
    let mut v = vec![Root { tag: "", base: home.join("Emulation") }];
    if let rare_library::storage::SdCard::Ready { mount, .. } = rare_library::storage::sd_card() {
        v.push(Root { tag: "sd", base: mount.join("Emulation") });
    }
    v
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rom {
    pub id: String,
    pub system: &'static System,
    /// The file (or folder) the emulator opens: the game, or Rare's playlist of its discs.
    pub path: PathBuf,
    /// The file the player sees in the folder (the first disc for a multi-disc game).
    pub file: PathBuf,
    pub title: String,
    /// "(USA)", "(Rev 1)": the tags taken off the file name.
    pub tags: String,
    pub discs: u32,
    pub bytes: u64,
    pub sd: bool,
}

/// "Legend of Zelda, The - A Link to the Past (USA) (Rev 1)" to ("The Legend of Zelda - A Link to
/// the Past", "(USA) (Rev 1)").
pub fn clean_title(stem: &str) -> (String, String) {
    let mut title = String::new();
    let mut tags = String::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in stem.chars() {
        match c {
            '(' | '[' => {
                if depth == 0 {
                    title.push_str(&cur);
                    cur.clear();
                }
                depth += 1;
                cur.push(c);
            }
            ')' | ']' if depth > 0 => {
                cur.push(c);
                depth -= 1;
                if depth == 0 {
                    if !tags.is_empty() {
                        tags.push(' ');
                    }
                    tags.push_str(cur.trim());
                    cur.clear();
                }
            }
            _ => cur.push(c),
        }
    }
    if depth == 0 {
        title.push_str(&cur);
    }
    let mut t = title.split_whitespace().collect::<Vec<_>>().join(" ").replace('_', " ");
    // "Name, The - Sub" and "Name, The" read as "The Name - Sub".
    for art in ["The", "A", "An"] {
        let pat = format!(", {art}");
        if let Some(i) = t.find(&pat) {
            let after = &t[i + pat.len()..];
            if after.is_empty() || after.starts_with(" -") || after.starts_with(':') {
                t = format!("{art} {}{}", &t[..i], after);
                break;
            }
        }
    }
    let t = t.trim().to_string();
    (if t.is_empty() { stem.to_string() } else { t }, tags)
}

/// Inner extensions a dump tool adds before the real one ("Game (USA).nkit.iso", ".xiso.iso").
const INNER_EXTS: &[&str] = &["nkit", "xiso", "dec", "wud", "ps3"];

/// A game file's name without its extensions: "Luigi's Mansion (USA).nkit.iso" is
/// "Luigi's Mansion (USA)"; a folder keeps its name minus ".ps3" or ".wua".
pub fn game_stem(p: &Path) -> String {
    let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut s = if p.is_dir() {
        name.clone()
    } else {
        p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    };
    loop {
        let Some((head, ext)) = s.rsplit_once('.') else { break };
        if INNER_EXTS.contains(&ext.to_ascii_lowercase().as_str()) || (p.is_dir() && ext.eq_ignore_ascii_case("wua")) {
            s = head.to_string();
        } else {
            break;
        }
    }
    s.trim().to_string()
}

/// A GameCube or Wii disc's 6-character game ID ("GLME01") from its header, in the containers
/// that keep the header readable: plain and NKit ISOs, CISO, WBFS, WIA and RVZ.
pub fn disc_id(rom: &Rom) -> Option<String> {
    if !matches!(rom.system.id, "gc" | "wii") {
        return None;
    }
    let f = &rom.file;
    let name = f.file_name()?.to_string_lossy().to_ascii_lowercase();
    let at: u64 = if name.ends_with(".ciso") {
        0x8000
    } else if name.ends_with(".wbfs") {
        0x200
    } else if name.ends_with(".rvz") || name.ends_with(".wia") {
        0x58
    } else if name.ends_with(".iso") || name.ends_with(".gcm") {
        0
    } else {
        return None;
    };
    use std::io::{Read, Seek, SeekFrom};
    let mut h = std::fs::File::open(f).ok()?;
    h.seek(SeekFrom::Start(at)).ok()?;
    let mut b = [0u8; 6];
    h.read_exact(&mut b).ok()?;
    let id = String::from_utf8(b.to_vec()).ok()?;
    id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()).then_some(id)
}

/// "(Disc 2)", "(CD 2)", "(Disk 2 of 3)": the disc number in a file name.
pub fn disc_number(name: &str) -> Option<u32> {
    let l = name.to_ascii_lowercase();
    for key in ["(disc ", "(disk ", "(cd "] {
        if let Some(i) = l.find(key) {
            let rest = &l[i + key.len()..];
            let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = n.parse() {
                return Some(n);
            }
        }
    }
    None
}

/// The name without its disc tag, to group a game's discs.
pub fn without_disc(name: &str) -> String {
    let l = name.to_ascii_lowercase();
    for key in ["(disc ", "(disk ", "(cd "] {
        if let Some(i) = l.find(key) {
            if let Some(j) = l[i..].find(')') {
                return format!("{}{}", &name[..i], &name[i + j + 1..]).split_whitespace().collect::<Vec<_>>().join(" ");
            }
        }
    }
    name.to_string()
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// A folder that is one game (a PS3 game folder, a Wii U one).
fn game_folder(sys: &System, p: &Path) -> bool {
    (!sys.folders.is_empty() && sys.folders.contains(&ext_of(p).as_str())) || (sys.id == "ps3" && (p.join("PS3_GAME").is_dir() || p.join("USRDIR").is_dir()))
}

/// The files an .m3u or .cue names (relative to it), so they don't show as games of their own.
fn listed_by(p: &Path) -> Vec<PathBuf> {
    let Ok(t) = std::fs::read_to_string(p) else { return vec![] };
    let dir = p.parent().unwrap_or(Path::new(""));
    let mut v = Vec::new();
    for l in t.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let name = if ext_of(p) == "cue" {
            match l.strip_prefix("FILE ") {
                Some(r) => r.rsplit_once(' ').map(|(n, _)| n).unwrap_or(r).trim().trim_matches('"').to_string(),
                None => continue,
            }
        } else {
            l.to_string()
        };
        v.push(dir.join(name));
    }
    v
}

/// Where Rare keeps the playlists it makes for multi-disc games.
pub fn playlist_dir(home: &Path) -> PathBuf {
    home.join(".cache/rare/emu/playlists")
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || " ()-.".contains(c) { c } else { '_' }).collect()
}

/// The games in one system folder.
pub fn scan_system(root: &Root, folder: &str, sys: &'static System, home: &Path) -> Vec<Rom> {
    let dir = root.roms().join(folder);
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![(dir.clone(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                if game_folder(sys, &p) {
                    files.push(p);
                } else if depth < 2 && !matches!(name.as_str(), "media" | "images" | "videos" | "manuals" | "downloaded_media") {
                    stack.push((p, depth + 1));
                }
            } else if sys.exts.contains(&ext_of(&p).as_str()) {
                files.push(p);
            }
        }
    }
    files.sort();
    // Discs an .m3u or .cue already names are part of that game.
    let mut hidden: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for f in files.iter().filter(|f| matches!(ext_of(f).as_str(), "m3u" | "cue")) {
        for l in listed_by(f) {
            if l != *f {
                hidden.insert(l);
            }
        }
    }
    let mut groups: BTreeMap<(PathBuf, String), Vec<(u32, PathBuf)>> = BTreeMap::new();
    for f in files.into_iter().filter(|f| !hidden.contains(f)) {
        let name = game_stem(&f);
        let parent = f.parent().map(Path::to_path_buf).unwrap_or_default();
        match disc_number(&name).filter(|_| ext_of(&f) != "m3u") {
            Some(n) => groups.entry((parent, format!("{}.{}", without_disc(&name), ext_of(&f)))).or_default().push((n, f)),
            None => groups.entry((parent, f.file_name().unwrap_or_default().to_string_lossy().into_owned())).or_default().push((0, f)),
        }
    }
    let mut out = Vec::new();
    for ((_, key), mut discs) in groups {
        discs.sort();
        let first = discs[0].1.clone();
        let rel = first.strip_prefix(&dir).unwrap_or(&first).to_string_lossy().into_owned();
        let id = if root.tag == "sd" { format!("emu:sd/{folder}/{rel}") } else { format!("emu:{folder}/{rel}") };
        let stem = if discs.len() > 1 { game_stem(Path::new(&key)) } else { game_stem(&first) };
        let (title, tags) = clean_title(&stem);
        let bytes: u64 = discs.iter().map(|(_, p)| p.metadata().map(|m| m.len()).unwrap_or(0)).sum();
        let path = if discs.len() > 1 { playlist_dir(home).join(format!("{}.m3u", safe(&format!("{folder}-{stem}")))) } else { first.clone() };
        out.push(Rom { id, system: sys, path, file: first, title, tags, discs: discs.len() as u32, bytes, sd: root.tag == "sd" });
        // The playlist itself: written when missing or stale (absolute paths, so it sits outside the ROM folder).
        if discs.len() > 1 {
            let body: String = discs.iter().map(|(_, p)| format!("{}\n", p.display())).collect();
            if std::fs::read_to_string(out.last().map(|r| r.path.clone()).unwrap_or_default()).ok().as_deref() != Some(body.as_str()) {
                let p = &out.last().unwrap().path;
                let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new("/")));
                let _ = std::fs::write(p, body);
            }
        }
    }
    // Two copies with the same name (USA and Europe) keep their tags so they read apart.
    let mut count: HashMap<String, u32> = HashMap::new();
    for r in &out {
        *count.entry(r.title.to_lowercase()).or_default() += 1;
    }
    for r in out.iter_mut() {
        if count[&r.title.to_lowercase()] > 1 && !r.tags.is_empty() {
            r.title = format!("{} {}", r.title, r.tags);
        }
    }
    out
}

/// Every game in every ROM folder.
pub fn scan_in(home: &Path, roots: &[Root]) -> Vec<Rom> {
    let mut out = Vec::new();
    // EmuDeck links alias folders to each other (gamecube -> gc): each real folder once.
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root.roms()) else { continue };
        let mut folders: Vec<String> = rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        // The system's own name first, so a link's target keeps the id.
        folders.sort_by_key(|f| (systems::by_folder(f).map(|s| s.id != f.as_str()).unwrap_or(true), f.clone()));
        for f in folders {
            let Some(sys) = systems::by_folder(&f) else { continue };
            let real = std::fs::canonicalize(root.roms().join(&f)).unwrap_or_else(|_| root.roms().join(&f));
            if seen.insert(real) {
                out.extend(scan_system(root, &f, sys, home));
            }
        }
    }
    out
}

pub fn scan() -> Vec<Rom> {
    let h = home();
    scan_in(&h, &roots(&h))
}

/// The emulator a system's games start in.
pub fn emulator_of(sys: &System) -> &'static catalog::Emulator {
    match sys.runner {
        Runner::Core(_) => catalog::by_id("retroarch").expect("retroarch"),
        Runner::App(a) => catalog::by_id(a).expect("emulator"),
    }
}

/// The emulator that will start a system's games on this device: its own, else an alternative
/// the player already has (Citron's AppImage for the Switch).
pub fn runner_of(sys: &System, home: &Path) -> (&'static catalog::Emulator, Option<catalog::Install>) {
    let emu = emulator_of(sys);
    if let Some(i) = catalog::installed(emu, home) {
        return (emu, Some(i));
    }
    for alt in catalog::ALTERNATES.iter().filter(|a| a.0 == emu.id) {
        if let Some(i) = catalog::installed(&alt.1, home) {
            return (&alt.1, Some(i));
        }
    }
    (emu, None)
}

/// How to start a ROM now, or the sentence that says what is missing.
pub fn launch_spec(rom: &Rom, home: &Path, rewind_ok: bool) -> Result<LaunchSpec, String> {
    let (emu, inst) = runner_of(rom.system, home);
    let inst = inst.ok_or_else(|| format!("{} isn't installed. Set up emulation in Library, Emulation first.", emu.name))?;
    let (program, mut args) = inst.command();
    // RPCS3's Flatpak asks "use this build anyway?" at every start (FLATPAK_ID set), with No as the
    // default a pad press picks, which quits: start it with that variable unset.
    if let (catalog::Install::Flatpak(app), "rpcs3") = (&inst, emu.id) {
        args = vec!["run".into(), "--command=env".into(), (*app).into(), "-u".into(), "FLATPAK_ID".into(), "rpcs3".into()];
    }
    // The pad environment (controls::launch_env) named on the command line too, so the Flatpak's
    // SDL gets it whatever the sandbox passes through.
    if matches!(inst, catalog::Install::Flatpak(_)) {
        let env: Vec<String> = crate::controls::launch_env().into_iter().map(|(k, v)| format!("--env={k}={v}")).collect();
        for (i, e) in env.into_iter().enumerate() {
            args.insert(1 + i, e);
        }
    }
    if let Runner::Core(core) = rom.system.runner {
        let p = retroarch::paths(home);
        let so = retroarch::core_path(&p, core);
        if !so.is_file() {
            return Err(format!("RetroArch's {core} core isn't installed. Set up emulation in Library, Emulation first."));
        }
        let append = retroarch::append_arg(home, rom.system.rewind && rewind_ok);
        args.extend(["-L".into(), so.to_string_lossy().into_owned(), "--appendconfig".into(), append]);
    }
    args.extend(emu.args.iter().map(|a| a.to_string()));
    args.push(rom.path.to_string_lossy().into_owned());
    args.extend(emu.tail.iter().map(|a| a.to_string()));
    Ok(LaunchSpec::Exec { program, args, cwd: None, windows: false })
}

/// Before the game starts: Rare's RetroArch settings, the handheld shader, the achievements
/// retro trophies sign-in for the standalone emulators. Never fails the launch.
pub fn prepare(rom: &Rom, home: &Path) {
    let ra = crate::retro::sign();
    if matches!(rom.system.runner, Runner::Core(_)) {
        if let Err(e) = retroarch::write_append(home, ra.as_ref(), rom.system.rewind) {
            eprintln!("[emu] rare.cfg: {e}");
        }
    }
    crate::setup::shader_preset(home, rom.system);
    let (emu, inst) = runner_of(rom.system, home);
    if let Some(a) = ra.as_ref() {
        crate::retro::write_standalone(home, emu.id, a);
    }
    // One pad, every button (never over the player's own pad setup), then the Rare menu's keys.
    let w = crate::controls::ensure(emu.id, home);
    if !w.is_empty() {
        eprintln!("[emu] {}: controller profile written: {}", emu.name, w.join(", "));
    }
    if emu.id != "retroarch" {
        let env = crate::defaults::Env { home: home.to_path_buf(), pad_guid: None, pad_name: None, xdg: None };
        let env = match inst {
            Some(catalog::Install::Flatpak(app)) => env.flatpak(app),
            _ => env,
        };
        if let Err(e) = crate::actions::ensure_bindings(&env, emu.id) {
            eprintln!("[emu] {} bindings: {e}", emu.name);
        }
    }
}

/// The library entry for a ROM. Art is the cached box art when Rare has it.
pub fn game(rom: &Rom, home: &Path) -> Game {
    // Pictures as links (rare.market's image front loads and caches them), best first; one an
    // earlier version saved on this device goes before them.
    let ident = crate::ident::known(home, rom);
    let local = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().into_owned());
    let cover: Vec<String> = local(crate::art::cached(home, rom)).into_iter().chain(crate::art::cover_urls(rom, ident.as_ref())).collect();
    let snap: Vec<String> = local(crate::art::cached_snap(home, rom)).into_iter().chain(crate::art::snap_urls(rom, ident.as_ref())).collect();
    // The real command is made at launch (the emulator may be installed after the scan).
    let launch = LaunchSpec::Exec { program: emulator_of(rom.system).id.to_string(), args: vec![rom.path.to_string_lossy().into_owned()], cwd: None, windows: false };
    Game {
        id: rom.id.clone(),
        source: Source::Emu,
        title: rom.title.clone(),
        installed: true,
        // Never the folder: the system's folder holds other games.
        install_dir: None,
        size_bytes: Some(rom.bytes),
        last_played: None,
        playtime_minutes: None,
        // The screenshot is the wide backdrop (game page, Home's stage) and the header.
        art: Art { cover, hero: snap.clone(), header: snap, ..Default::default() },
        launch,
        origin: format!("{} \u{b7} {}", rom.system.name, if rom.sd { "SD card" } else { "This device" }),
        foreign: false,
    }
}

/// Every ROM as a library entry.
pub fn library_games() -> Vec<Game> {
    let h = home();
    scan().iter().map(|r| game(r, &h)).collect()
}

/// Where a library id points: its root, its system folder and the path inside it.
pub fn locate(id: &str) -> Option<(Root, String, String)> {
    let rest = id.strip_prefix("emu:")?;
    let (sd, rest) = match rest.strip_prefix("sd/") {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let (folder, rel) = rest.split_once('/')?;
    let root = roots(&home()).into_iter().find(|r| (r.tag == "sd") == sd)?;
    Some((root, folder.to_string(), rel.to_string()))
}

/// The ROM behind a library id.
pub fn find(id: &str) -> Option<Rom> {
    let (root, folder, _) = locate(id)?;
    let sys = systems::by_folder(&folder)?;
    scan_system(&root, &folder, sys, &home()).into_iter().find(|r| r.id == id)
}

/// Every file of a ROM: the game, or each disc of a multi-disc game (from Rare's playlist).
pub fn files_of(rom: &Rom) -> Vec<PathBuf> {
    if rom.discs > 1 {
        if let Ok(t) = std::fs::read_to_string(&rom.path) {
            return t.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect();
        }
    }
    vec![rom.file.clone()]
}

/// The system id of a library id ("emu:megadrive/x.md" is the Genesis).
pub fn system_of(id: &str) -> Option<&'static System> {
    let rest = id.strip_prefix("emu:")?;
    let rest = rest.strip_prefix("sd/").unwrap_or(rest);
    systems::by_folder(rest.split_once('/')?.0)
}

/// The save sets a ROM's saves are in, by the names cloud saves use: the emulator's name, and
/// EmuDeck's folder under ~/Emulation/saves.
pub fn save_owner(id: &str) -> Option<(&'static str, &'static str)> {
    let e = emulator_of(system_of(id)?);
    Some((e.name, e.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_read_like_names() {
        assert_eq!(clean_title("Legend of Zelda, The - A Link to the Past (USA) (Rev 1)"), ("The Legend of Zelda - A Link to the Past".into(), "(USA) (Rev 1)".into()));
        assert_eq!(clean_title("Tetris (World) [!]").0, "Tetris");
        assert_eq!(clean_title("Final_Fantasy_VII").0, "Final Fantasy VII");
        assert_eq!(clean_title("(Unl)").0, "(Unl)");
        assert_eq!(disc_number("Final Fantasy VII (USA) (Disc 2)"), Some(2));
        assert_eq!(disc_number("Game (CD 3)"), Some(3));
        assert_eq!(disc_number("Discworld (USA)"), None);
        assert_eq!(game_stem(Path::new("/r/Luigi's Mansion (USA).nkit.iso")), "Luigi's Mansion (USA)");
        assert_eq!(game_stem(Path::new("/r/Jet Set Radio Future (USA).xiso.iso")), "Jet Set Radio Future (USA)");
        assert_eq!(game_stem(Path::new("/r/Super Smash Bros. Melee (USA) (En,Ja) (Rev 2).nkit.iso")), "Super Smash Bros. Melee (USA) (En,Ja) (Rev 2)");
        assert_eq!(clean_title(&game_stem(Path::new("/r/Simpsons, The - Hit & Run (USA).nkit.iso"))).0, "The Simpsons - Hit & Run");
        assert_eq!(clean_title("Animal Crossing New Horizons [01006F8002326000][v0]").0, "Animal Crossing New Horizons");
    }

    #[test]
    fn scans_folders_discs_and_ids() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path();
        let base = h.join("Emulation");
        let w = |p: &str, b: &[u8]| {
            let p = base.join("roms").join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b).unwrap();
        };
        w("nes/Alpha (USA).nes", b"x");
        w("nes/Alpha (Europe).nes", b"x");
        w("nes/systeminfo.txt", b"x");
        w("nes/.hidden.nes", b"x");
        w("megadrive/Sonic.md", b"xx");
        w("psx/Big Game (USA) (Disc 1).chd", b"1");
        w("psx/Big Game (USA) (Disc 2).chd", b"22");
        w("psx/Other/Other.cue", b"FILE \"Other.bin\" BINARY\n");
        w("psx/Other/Other.bin", b"b");
        w("unknown/x.bin", b"x");
        std::fs::create_dir_all(base.join("roms/ps3/Cool Game.ps3/PS3_GAME")).unwrap();
        let roots = vec![Root { tag: "", base }];
        let roms = scan_in(h, &roots);
        let ids: Vec<&str> = roms.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"emu:nes/Alpha (USA).nes"), "{ids:?}");
        assert!(ids.contains(&"emu:megadrive/Sonic.md"));
        assert!(!ids.iter().any(|i| i.contains("systeminfo") || i.contains("hidden") || i.contains("unknown")));
        let titles: Vec<&str> = roms.iter().map(|r| r.title.as_str()).collect();
        assert!(titles.contains(&"Alpha (USA)") && titles.contains(&"Alpha (Europe)"), "{titles:?}");
        let big = roms.iter().find(|r| r.title == "Big Game").unwrap();
        assert_eq!(big.discs, 2);
        assert!(big.path.extension().unwrap() == "m3u");
        let pl = std::fs::read_to_string(&big.path).unwrap();
        // A linked alias folder (EmuDeck's gamecube -> gc) doesn't list its games twice.
        std::fs::create_dir_all(roots[0].roms().join("gc")).unwrap();
        std::fs::write(roots[0].roms().join("gc/Cube.rvz"), b"c").unwrap();
        std::os::unix::fs::symlink(roots[0].roms().join("gc"), roots[0].roms().join("gamecube")).unwrap();
        let again = scan_in(h, &roots);
        assert_eq!(again.iter().filter(|r| r.title == "Cube").map(|r| r.id.as_str()).collect::<Vec<_>>(), ["emu:gc/Cube.rvz"]);
        assert!(pl.lines().count() == 2 && pl.contains("(Disc 2).chd"));
        // The .bin a .cue names is not a game of its own (and .bin isn't a PS1 type anyway).
        assert_eq!(roms.iter().filter(|r| r.system.id == "psx").count(), 2);
        let ps3 = roms.iter().find(|r| r.system.id == "ps3").unwrap();
        assert_eq!(ps3.title, "Cool Game");
        // A GameCube disc's ID from its header.
        let gcf = h.join("Emulation/roms/gc/Mansion (USA).nkit.iso");
        std::fs::create_dir_all(gcf.parent().unwrap()).unwrap();
        std::fs::write(&gcf, b"GLME01\0\0rest").unwrap();
        let gc = scan_in(h, &roots).into_iter().find(|r| r.system.id == "gc" && r.title == "Mansion").unwrap();
        assert_eq!(disc_id(&gc).as_deref(), Some("GLME01"));
        assert_eq!(system_of("emu:megadrive/Sonic.md").unwrap().id, "genesis");
        assert_eq!(system_of("emu:sd/gba/x.gba").unwrap().id, "gba");
        assert_eq!(save_owner("emu:snes/a.sfc"), Some(("RetroArch", "retroarch")));
        // No emulator installed: the launch says what to do.
        let sonic = roms.iter().find(|r| r.title == "Sonic").unwrap();
        assert!(launch_spec(sonic, h, true).unwrap_err().contains("Set up emulation"));
    }

    #[test]
    fn retroarch_launch_line() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path();
        std::fs::create_dir_all(h.join(".local/share/flatpak/app/org.libretro.RetroArch/x86_64")).unwrap();
        let cores = h.join(".var/app/org.libretro.RetroArch/config/retroarch/cores");
        std::fs::create_dir_all(&cores).unwrap();
        std::fs::write(cores.join("gambatte_libretro.so"), b"x").unwrap();
        let rom = Rom { id: "emu:gb/T.gb".into(), system: systems::by_id("gb").unwrap(), path: "/r/T.gb".into(), file: "/r/T.gb".into(), title: "T".into(), tags: String::new(), discs: 1, bytes: 1, sd: false };
        let LaunchSpec::Exec { program, args, .. } = launch_spec(&rom, h, true).unwrap() else { panic!() };
        assert_eq!(program, "flatpak");
        assert_eq!(&args[..2], ["run", "org.libretro.RetroArch"]);
        assert!(args.contains(&"-L".to_string()) && args.iter().any(|a| a.ends_with("gambatte_libretro.so")));
        assert!(args.iter().any(|a| a.contains("rare.cfg|") && a.ends_with("rare-rewind.cfg")));
        assert_eq!(args.last().unwrap(), "/r/T.gb");
    }
}
