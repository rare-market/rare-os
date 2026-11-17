//! Adding games to the ROM folders: which system a file is for, where it goes, and the games on
//! a USB drive or an SD card that can be copied in.
//!
//! A file's system comes from its folder when the folder is a system's (`gba/`, `megadrive/`:
//! the layout ES-DE and EmuDeck use, and most collections keep), else from its type when only
//! one system takes that type (`.gba`, `.sfc`, `.nds`). Archives and disc images (`.zip`, `.iso`,
//! `.chd`, `.cue`) fit many systems: those need the player to say which.
//!
//! Games go into the device's `Emulation/roms/<system>` (an existing folder under any of the
//! system's names is used as it is), never over a file that is there.

use crate::systems::{by_folder, System, SYSTEMS};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// A file's type, lowercase, without the dot.
pub fn ext(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default()
}

/// The systems whose ROM folder takes a file of this name (by its type).
pub fn systems_for(name: &str) -> Vec<&'static System> {
    let e = ext(name);
    if e.is_empty() {
        return Vec::new();
    }
    SYSTEMS.iter().filter(|s| s.exts.contains(&e.as_str())).collect()
}

/// The one system a file can be for: its folder's (`hint`, a folder name) when that is a system
/// taking the file, else the only system taking its type.
pub fn system_of(name: &str, hint: Option<&str>) -> Option<&'static System> {
    let fits = systems_for(name);
    if let Some(s) = hint.and_then(by_folder) {
        if fits.iter().any(|f| f.id == s.id) {
            return Some(s);
        }
    }
    (fits.len() == 1).then(|| fits[0])
}

/// The device's ROM folder.
pub fn roms_dir() -> PathBuf {
    crate::roms::home().join("Emulation").join("roms")
}

/// Where a system's games go in `roms`: its folder there under any of its names, else `<id>`.
pub fn folder_in(roms: &Path, sys: &System) -> PathBuf {
    if let Ok(rd) = fs::read_dir(roms) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) && by_folder(&e.file_name().to_string_lossy()).is_some_and(|s| s.id == sys.id) {
                return e.path();
            }
        }
    }
    roms.join(sys.id)
}

/// A file name someone else sent, made safe to write: no folders, no control characters, no
/// leading dot. None when nothing is left.
pub fn safe_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    (!clean.is_empty() && clean.len() <= 255).then_some(clean)
}

/// `dir/name`, or `dir/name (2).ext` and so on when that is taken.
pub fn unique(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let dot = path.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    (2..).map(|n| dir.join(format!("{stem} ({n}){dot}"))).find(|p| !p.exists()).unwrap()
}

/// The files a `.cue` sheet names (its tracks), beside it.
pub fn cue_tracks(cue: &Path) -> Vec<PathBuf> {
    let Ok(f) = fs::File::open(cue) else { return Vec::new() };
    let dir = cue.parent().unwrap_or(Path::new("."));
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("FILE ").or_else(|| l.strip_prefix("file "))?;
            let name = if let Some(q) = rest.strip_prefix('"') { q.split('"').next()? } else { rest.split_whitespace().next()? };
            safe_name(name).map(|n| dir.join(n))
        })
        .collect()
}

/// A game found on a drive.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    /// The game's file and what goes with it (a `.cue` sheet's tracks), in bytes.
    pub bytes: u64,
    /// The system, when the folder or the type says it; None: the player must say.
    pub system: Option<&'static System>,
}

/// Games on a drive mounted at `dir`: files a system takes, a few folders down. A `.cue` brings
/// its tracks and they are not listed again; the drive's own `Emulation` folder (a ROM root Rare
/// already reads) is skipped. Stops after `limit` entries looked at.
pub fn find_on(dir: &Path, limit: usize) -> Vec<Found> {
    let mut out = Vec::new();
    let mut tracks: Vec<PathBuf> = Vec::new();
    let mut left = limit;
    let mut stack = vec![(dir.to_path_buf(), 0u8)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        let hint = d.file_name().map(|n| n.to_string_lossy().into_owned());
        let mut files = Vec::new();
        for e in rd.flatten() {
            if left == 0 {
                break;
            }
            left -= 1;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                if depth < 5 && !(depth == 0 && name == "Emulation") {
                    stack.push((e.path(), depth + 1));
                }
            } else if t.is_file() && !systems_for(&name).is_empty() {
                files.push(e.path());
            }
        }
        // Sheets first, so their tracks are known before the folder's other files.
        files.sort_by_key(|p| ext(&p.to_string_lossy()) != "cue");
        for p in files {
            if tracks.contains(&p) {
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let mut bytes = p.metadata().map(|m| m.len()).unwrap_or(0);
            if ext(&name) == "cue" {
                for t in cue_tracks(&p) {
                    bytes += t.metadata().map(|m| m.len()).unwrap_or(0);
                    tracks.push(t);
                }
            }
            out.push(Found { system: system_of(&name, hint.as_deref()), path: p, bytes });
        }
    }
    out.retain(|f| !tracks.contains(&f.path));
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Copies a found game (and a `.cue` sheet's tracks) into `sys`'s folder under `roms`, through
/// a `.part` file per file, so a half copy never shows as a game. `progress(bytes)` after each
/// chunk. Returns the game's new path.
pub fn copy_in(f: &Found, sys: &System, roms: &Path, progress: &mut dyn FnMut(u64)) -> Result<PathBuf, String> {
    let dir = folder_in(roms, sys);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut files = vec![f.path.clone()];
    if ext(&f.path.to_string_lossy()) == "cue" {
        files.extend(cue_tracks(&f.path));
    }
    let mut first = None;
    for src in files {
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("No file name")?;
        // A sheet's tracks keep their names: the sheet points at them.
        let dst = if first.is_none() { unique(&dir, &name) } else { dir.join(&name) };
        copy_file(&src, &dst, progress)?;
        first.get_or_insert(dst);
    }
    first.ok_or_else(|| "Nothing to copy".into())
}

fn copy_file(src: &Path, dst: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), String> {
    use std::io::{Read, Write};
    let part = dst.with_file_name(format!("{}.part", dst.file_name().unwrap().to_string_lossy()));
    let r = (|| -> std::io::Result<()> {
        let mut i = fs::File::open(src)?;
        let mut o = fs::File::create(&part)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = i.read(&mut buf)?;
            if n == 0 {
                break;
            }
            o.write_all(&buf[..n])?;
            progress(n as u64);
        }
        o.sync_all()?;
        fs::rename(&part, dst)
    })();
    if r.is_err() {
        let _ = fs::remove_file(&part);
    }
    r.map_err(|e| format!("Couldn't copy {}: {e}", src.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_s_system() {
        assert_eq!(system_of("Game.gba", None).map(|s| s.id), Some("gba"));
        assert_eq!(system_of("Game.SFC", None).map(|s| s.id), Some("snes"));
        // Archives and discs fit many: the folder decides, else nobody guesses.
        assert_eq!(system_of("Game.zip", None), None);
        assert_eq!(system_of("Game.zip", Some("megadrive")).map(|s| s.id), Some("genesis"));
        assert_eq!(system_of("Game.iso", Some("PS2")).map(|s| s.id), Some("ps2"));
        // A folder that doesn't take the type is no hint.
        assert_eq!(system_of("Game.gba", Some("snes")).map(|s| s.id), Some("gba"));
        assert_eq!(system_of("notes.txt", Some("gba")), None);
    }

    #[test]
    fn names_from_elsewhere_are_made_safe() {
        assert_eq!(safe_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(safe_name("C:\\games\\Zelda.gba").as_deref(), Some("Zelda.gba"));
        assert_eq!(safe_name(".hidden").as_deref(), Some("hidden"));
        assert_eq!(safe_name("a\u{0}b.nes").as_deref(), Some("ab.nes"));
        assert_eq!(safe_name("/"), None);
    }

    #[test]
    fn finds_games_on_a_drive_and_copies_one_in() {
        let t = tempfile::tempdir().unwrap();
        let drive = t.path().join("drive");
        fs::create_dir_all(drive.join("roms/gba")).unwrap();
        fs::create_dir_all(drive.join("PS1/Game")).unwrap();
        fs::create_dir_all(drive.join("Emulation/roms/nes")).unwrap();
        fs::write(drive.join("roms/gba/A.zip"), b"zip").unwrap();
        fs::write(drive.join("B.nds"), b"nds").unwrap();
        fs::write(drive.join("C.iso"), b"iso").unwrap();
        fs::write(drive.join("PS1/Game/G.cue"), "FILE \"G (Track 1).bin\" BINARY\n").unwrap();
        fs::write(drive.join("PS1/Game/G (Track 1).bin"), b"0123456789").unwrap();
        fs::write(drive.join("Emulation/roms/nes/skip.nes"), b"x").unwrap();
        fs::write(drive.join("readme.txt"), b"x").unwrap();
        let found = find_on(&drive, 1000);
        let got: Vec<(String, Option<&str>)> = found.iter().map(|f| (f.path.file_name().unwrap().to_string_lossy().into_owned(), f.system.map(|s| s.id))).collect();
        assert_eq!(got, vec![("B.nds".into(), Some("nds")), ("C.iso".into(), None), ("G.cue".into(), None), ("A.zip".into(), Some("gba"))]);
        let cue = found.iter().find(|f| f.path.ends_with("G.cue")).unwrap();
        let sheet = fs::metadata(drive.join("PS1/Game/G.cue")).unwrap().len();
        assert_eq!(cue.bytes, sheet + 10);

        // Into an existing folder under another name, the sheet with its track, never over a file.
        let roms = t.path().join("roms");
        fs::create_dir_all(roms.join("PSX")).unwrap();
        let psx = crate::systems::by_id("psx").unwrap();
        let mut n = 0;
        let p = copy_in(cue, psx, &roms, &mut |b| n += b).unwrap();
        assert_eq!(p, roms.join("PSX/G.cue"));
        assert!(roms.join("PSX/G (Track 1).bin").exists());
        assert_eq!(n, sheet + 10);
        let again = copy_in(&found[0], crate::systems::by_id("nds").unwrap(), &roms, &mut |_| {}).unwrap();
        assert_eq!(again, roms.join("nds/B.nds"));
        let twice = copy_in(&found[0], crate::systems::by_id("nds").unwrap(), &roms, &mut |_| {}).unwrap();
        assert_eq!(twice, roms.join("nds/B (2).nds"));
    }
}
