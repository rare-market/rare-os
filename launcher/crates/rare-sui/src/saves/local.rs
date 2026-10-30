//! Cloud saves on this device: find save folders, pack one into a tar.gz, restore one safely.
//! Moved from `src-tauri/src/sui/saves.rs` (whose Tauri commands now call these), so the native
//! UI calls them directly.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_WALK_FILES: u64 = 200_000;
const MAX_PACK_BYTES: u64 = 1 << 30;
const EXCLUDED: &[&str] = &["Cache", "cache", "CacheStorage", "GPUCache", "ShaderCache", "shaders", "logs"];
const WEBGAMES_REL: &str = ".local/share/rare/webgames";
const EMUDECK_REL: &str = "Emulation/saves";

struct Template {
    id: &'static str,
    emulator: &'static str,
    label: &'static str,
    rel: &'static str,
}

const fn t(id: &'static str, emulator: &'static str, label: &'static str, rel: &'static str) -> Template {
    Template { id, emulator, label, rel }
}

static TEMPLATES: &[Template] = &[
    t("retroarch-saves", "RetroArch", "Save files", ".config/retroarch/saves"),
    t("retroarch-states", "RetroArch", "Save states", ".config/retroarch/states"),
    t("retroarch-saves-flatpak", "RetroArch", "Save files (Flatpak)", ".var/app/org.libretro.RetroArch/config/retroarch/saves"),
    t("retroarch-states-flatpak", "RetroArch", "Save states (Flatpak)", ".var/app/org.libretro.RetroArch/config/retroarch/states"),
    t("rpcs3", "RPCS3", "Save files", ".config/rpcs3/dev_hdd0/home/00000001/savedata"),
    t("rpcs3-flatpak", "RPCS3", "Save files (Flatpak)", ".var/app/net.rpcs3.RPCS3/config/rpcs3/dev_hdd0/home/00000001/savedata"),
    t("pcsx2-memcards", "PCSX2", "Memory cards", ".config/PCSX2/memcards"),
    t("pcsx2-states", "PCSX2", "Save states", ".config/PCSX2/sstates"),
    t("pcsx2-memcards-flatpak", "PCSX2", "Memory cards (Flatpak)", ".var/app/net.pcsx2.PCSX2/config/PCSX2/memcards"),
    t("pcsx2-states-flatpak", "PCSX2", "Save states (Flatpak)", ".var/app/net.pcsx2.PCSX2/config/PCSX2/sstates"),
    t("cemu", "Cemu", "Save files", ".local/share/Cemu/mlc01/usr/save"),
    t("cemu-flatpak", "Cemu", "Save files (Flatpak)", ".var/app/info.cemu.Cemu/data/Cemu/mlc01/usr/save"),
    t("duckstation-memcards", "DuckStation", "Memory cards", ".local/share/duckstation/memcards"),
    t("duckstation-states", "DuckStation", "Save states", ".local/share/duckstation/savestates"),
    t("duckstation-memcards-flatpak", "DuckStation", "Memory cards (Flatpak)", ".var/app/org.duckstation.DuckStation/data/duckstation/memcards"),
    t("duckstation-states-flatpak", "DuckStation", "Save states (Flatpak)", ".var/app/org.duckstation.DuckStation/data/duckstation/savestates"),
    t("dolphin-gc", "Dolphin", "GameCube saves", ".local/share/dolphin-emu/GC"),
    t("dolphin-wii", "Dolphin", "Wii saves", ".local/share/dolphin-emu/Wii/title"),
    t("dolphin-gc-flatpak", "Dolphin", "GameCube saves (Flatpak)", ".var/app/org.DolphinEmu.dolphin-emu/data/dolphin-emu/GC"),
    t("dolphin-wii-flatpak", "Dolphin", "Wii saves (Flatpak)", ".var/app/org.DolphinEmu.dolphin-emu/data/dolphin-emu/Wii/title"),
    t("primehack-gc", "PrimeHack", "GameCube saves", ".local/share/primehack/GC"),
    t("primehack-wii", "PrimeHack", "Wii saves", ".local/share/primehack/Wii/title"),
    t("ppsspp", "PPSSPP", "Save files", ".config/ppsspp/PSP/SAVEDATA"),
    t("ppsspp-states", "PPSSPP", "Save states", ".config/ppsspp/PSP/PPSSPP_STATE"),
    t("ppsspp-flatpak", "PPSSPP", "Save files (Flatpak)", ".var/app/org.ppsspp.PPSSPP/config/ppsspp/PSP/SAVEDATA"),
    t("ppsspp-states-flatpak", "PPSSPP", "Save states (Flatpak)", ".var/app/org.ppsspp.PPSSPP/config/ppsspp/PSP/PPSSPP_STATE"),
    t("ryujinx", "Ryujinx", "Save files", ".config/Ryujinx/bis/user/save"),
    t("azahar", "Azahar", "Save files", ".local/share/azahar-emu/sdmc"),
    t("azahar-flatpak", "Azahar", "Save files (Flatpak)", ".var/app/org.azahar_emu.Azahar/data/azahar-emu/sdmc"),
    t("lime3ds", "Lime3DS", "Save files", ".local/share/lime3ds-emu/sdmc"),
    t("citra", "Citra", "Save files", ".local/share/citra-emu/sdmc"),
    t("vita3k", "Vita3K", "Save files", ".local/share/Vita3K/Vita3K/ux0/user/00/savedata"),
    t("flycast", "Flycast", "VMU saves", ".local/share/flycast"),
    t("flycast-flatpak", "Flycast", "VMU saves (Flatpak)", ".var/app/org.flycast.Flycast/data/flycast"),
    t("scummvm", "ScummVM", "Save files", ".local/share/scummvm/saves"),
    t("scummvm-flatpak", "ScummVM", "Save files (Flatpak)", ".var/app/org.scummvm.ScummVM/data/scummvm/saves"),
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaveSet {
    pub id: String,
    pub emulator: String,
    pub label: String,
    /// "emulator" | "webgame"
    pub kind: String,
    pub rel: String,
    pub path: String,
    pub files: u64,
    pub bytes: u64,
    /// Newest file mtime, unix ms.
    pub modified: Option<u64>,
}

pub(crate) fn is_excluded(name: &str, webgame: bool) -> bool {
    EXCLUDED.contains(&name) || (webgame && name == "WebKitCache")
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-')
}

fn valid_emudeck_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

#[derive(Default)]
struct Stat {
    files: u64,
    bytes: u64,
    modified: Option<u64>,
}

fn scan(root: &Path, webgame: bool) -> Stat {
    let mut st = Stat::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(md) = fs::symlink_metadata(e.path()) else { continue };
            let ft = md.file_type();
            if ft.is_dir() {
                if !is_excluded(&e.file_name().to_string_lossy(), webgame) {
                    stack.push(e.path());
                }
            } else if ft.is_file() {
                st.files += 1;
                st.bytes += md.len();
                if let Ok(m) = md.modified() {
                    if let Ok(d) = m.duration_since(UNIX_EPOCH) {
                        let ms = d.as_millis() as u64;
                        st.modified = Some(st.modified.map_or(ms, |o| o.max(ms)));
                    }
                }
                if st.files >= MAX_WALK_FILES {
                    return st;
                }
            }
        }
    }
    st
}

pub fn detect_in(home: &Path) -> Vec<SaveSet> {
    let mut out: Vec<SaveSet> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut consider = |id: String, emulator: String, label: String, kind: &str, rel: String, out: &mut Vec<SaveSet>| {
        let path = home.join(&rel);
        let Ok(canon) = fs::canonicalize(&path) else { return };
        if !canon.is_dir() || seen.contains(&canon) {
            return;
        }
        let st = scan(&canon, kind == "webgame");
        if st.files == 0 {
            return;
        }
        seen.insert(canon);
        out.push(SaveSet {
            id,
            emulator,
            label,
            kind: kind.to_string(),
            rel,
            path: path.to_string_lossy().into_owned(),
            files: st.files,
            bytes: st.bytes,
            modified: st.modified,
        });
    };

    for tpl in TEMPLATES {
        consider(tpl.id.into(), tpl.emulator.into(), tpl.label.into(), "emulator", tpl.rel.into(), &mut out);
    }
    if let Ok(rd) = fs::read_dir(home.join(WEBGAMES_REL)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if valid_name(&name) && e.path().is_dir() {
                consider(format!("webgame:{name}"), "Web game".into(), name.clone(), "webgame", format!("{WEBGAMES_REL}/{name}"), &mut out);
            }
        }
    }
    // EmuDeck aliases last, so a symlink to a real emulator dir is dropped.
    if let Ok(rd) = fs::read_dir(home.join(EMUDECK_REL)) {
        let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| valid_emudeck_name(n)).collect();
        names.sort();
        for name in names {
            consider(format!("emudeck-{name}"), format!("EmuDeck {name}"), "Save files".into(), "emulator", format!("{EMUDECK_REL}/{name}"), &mut out);
        }
    }

    out.sort_by(|a, b| (a.emulator.as_str(), a.label.as_str(), a.id.as_str()).cmp(&(b.emulator.as_str(), b.label.as_str(), b.id.as_str())));
    out
}

fn collect(root: &Path, webgame: bool) -> Result<Vec<(PathBuf, PathBuf, bool)>, String> {
    // (absolute, relative, is_dir)
    let mut items = Vec::new();
    let mut total = 0u64;
    let mut stack = vec![(root.to_path_buf(), PathBuf::new())];
    while let Some((dir, rel)) = stack.pop() {
        let rd = fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
        for e in rd.flatten() {
            let name = e.file_name();
            let Ok(md) = fs::symlink_metadata(e.path()) else { continue };
            let ft = md.file_type();
            let r = rel.join(&name);
            if ft.is_dir() {
                if is_excluded(&name.to_string_lossy(), webgame) {
                    continue;
                }
                items.push((e.path(), r.clone(), true));
                stack.push((e.path(), r));
            } else if ft.is_file() {
                total += md.len();
                if total > MAX_PACK_BYTES {
                    return Err("save data is larger than 1 GiB".into());
                }
                if items.len() as u64 >= MAX_WALK_FILES {
                    return Err("too many files to pack".into());
                }
                items.push((e.path(), r, false));
            }
        }
    }
    items.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(items)
}

pub fn pack_dir(root: &Path) -> Result<Vec<u8>, String> {
    let root = fs::canonicalize(root).map_err(|e| format!("resolve {}: {e}", root.display()))?;
    let webgame = root.to_string_lossy().contains("/rare/webgames/");
    let items = collect(&root, webgame)?;
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut b = tar::Builder::new(gz);
    b.follow_symlinks(false);
    for (abs, rel, _) in &items {
        b.append_path_with_name(abs, rel).map_err(|e| format!("pack {}: {e}", rel.display()))?;
    }
    let gz = b.into_inner().map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())
}

/// The emulators Rare knows save folders for ("RetroArch", "Dolphin", ...).
pub fn emulator_names() -> Vec<&'static str> {
    let mut v: Vec<&str> = TEMPLATES.iter().map(|t| t.emulator).collect();
    v.dedup();
    v
}

/// A save set as a backup describes it, for restoring onto a device that has no such folder yet.
/// None unless the id and folder are ones Rare knows.
pub fn set_from(id: &str, rel: &str, emulator: &str, label: &str, kind: &str) -> Option<SaveSet> {
    (expected_rel(id).as_deref() == Some(rel)).then(|| SaveSet { id: id.into(), emulator: emulator.into(), label: label.into(), kind: kind.into(), rel: rel.into(), path: home().map(|h| h.join(rel).to_string_lossy().into_owned()).unwrap_or_default(), files: 0, bytes: 0, modified: None })
}

/// The rel path a given id must use, or None for unknown ids.
pub fn expected_rel(id: &str) -> Option<String> {
    if let Some(name) = id.strip_prefix("webgame:") {
        return valid_name(name).then(|| format!("{WEBGAMES_REL}/{name}"));
    }
    if let Some(name) = id.strip_prefix("emudeck-") {
        return valid_emudeck_name(name).then(|| format!("{EMUDECK_REL}/{name}"));
    }
    TEMPLATES.iter().find(|t| t.id == id).map(|t| t.rel.to_string())
}

fn validate_archive(data: &[u8]) -> Result<(), String> {
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(data));
    let mut total = 0u64;
    for entry in ar.entries().map_err(|e| format!("bad archive: {e}"))? {
        let entry = entry.map_err(|e| format!("bad archive: {e}"))?;
        let ty = entry.header().entry_type();
        if !(ty.is_file() || ty.is_dir()) {
            return Err("archive contains a link or special entry".into());
        }
        let p = entry.path().map_err(|e| format!("bad path: {e}"))?;
        for c in p.components() {
            match c {
                Component::Normal(_) | Component::CurDir => {}
                _ => return Err(format!("unsafe path in archive: {}", p.display())),
            }
        }
        total += entry.header().size().unwrap_or(0);
        if total > MAX_PACK_BYTES * 2 {
            return Err("archive is too large".into());
        }
    }
    Ok(())
}

fn unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn restore_into(home: &Path, id: &str, rel: &str, data: &[u8]) -> Result<String, String> {
    let want = expected_rel(id).ok_or("unknown save set")?;
    if rel != want {
        return Err("path does not match the save set".into());
    }
    validate_archive(data)?;

    let target = home.join(&want);
    // Resolve a symlinked root so we write into (and move aside) the real dir.
    let real = match fs::symlink_metadata(&target) {
        Ok(m) if m.file_type().is_symlink() => fs::canonicalize(&target).map_err(|e| format!("resolve: {e}"))?,
        _ => target.clone(),
    };
    let mut moved = String::new();
    if real.is_dir() && fs::read_dir(&real).map_err(|e| e.to_string())?.next().is_some() {
        let mut name = real.file_name().ok_or("bad target")?.to_os_string();
        name.push(format!(".rare-before-restore-{}", unix_secs()));
        let dest = real.with_file_name(name);
        fs::rename(&real, &dest).map_err(|e| format!("move old saves aside: {e}"))?;
        moved = dest.to_string_lossy().into_owned();
    } else if real.exists() && !real.is_dir() {
        return Err("target is not a directory".into());
    }
    fs::create_dir_all(&real).map_err(|e| format!("create {}: {e}", real.display()))?;

    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(data));
    ar.set_preserve_mtime(true);
    for entry in ar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path().map_err(|e| e.to_string())?.into_owned();
        let dest = real.join(&p);
        if !dest.starts_with(&real) {
            return Err("unsafe path in archive".into());
        }
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            entry.unpack(&dest).map_err(|e| format!("unpack {}: {e}", p.display()))?;
        }
    }
    Ok(moved)
}

/// The home folder (save sets are relative to it, the same on every Rare OS device).
pub fn home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "no home directory".to_string())
}

/// Every save set in the player's home folder.
pub fn detect() -> Result<Vec<SaveSet>, String> {
    Ok(detect_in(&home()?))
}

/// tar.gz of a detected save set, by id only: never an arbitrary path.
pub fn pack(id: &str) -> Result<Vec<u8>, String> {
    let home = home()?;
    let set = detect_in(&home).into_iter().find(|s| s.id == id).ok_or("save set not found")?;
    pack_dir(Path::new(&set.path))
}

/// Unpacks into the save set's folder; an existing folder is moved aside first. Returns where it
/// went ("" if nothing was there).
pub fn restore(id: &str, rel: &str, data: &[u8]) -> Result<String, String> {
    restore_into(&home()?, id, rel, data)
}

/// base64url (no padding) of a UTF-8 string, the header encoding `saves_restore` takes
/// (`b64url` in sui/native.ts).
pub fn b64url(s: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
}

/// The inverse of [`b64url`] (padding tolerated).
pub fn from_b64url(v: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(v.trim_end_matches('=')).ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fake_home(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rare-saves-test-{tag}-{}-{}", std::process::id(), unix_secs()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn put(p: &Path, content: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }
    fn gz_tar(build: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>)) -> Vec<u8> {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        build(&mut b);
        b.into_inner().unwrap().finish().unwrap()
    }
    fn raw_entry(b: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>, name: &str, ty: tar::EntryType, link: Option<&str>, body: &[u8]) {
        let mut h = tar::Header::new_gnu();
        h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        h.set_entry_type(ty);
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        if let Some(l) = link {
            h.as_old_mut().linkname[..l.len()].copy_from_slice(l.as_bytes());
        }
        h.set_cksum();
        b.append(&h, body).unwrap();
    }

    #[test]
    fn header_encoding_round_trips() {
        assert_eq!(b64url("webgame:act"), "d2ViZ2FtZTphY3Q");
        assert_eq!(from_b64url("d2ViZ2FtZTphY3Q=").as_deref(), Some("webgame:act"));
        assert_eq!(from_b64url("%%"), None);
    }

    #[test]
    fn detect_basics() {
        let home = fake_home("detect");
        put(&home.join(".config/retroarch/saves/a.srm"), "abc");
        put(&home.join(".config/retroarch/saves/Cache/junk"), "zzzz");
        put(&home.join(".config/rpcs3/dev_hdd0/home/00000001/savedata/G/SAVE.DAT"), "x");
        fs::create_dir_all(home.join(".config/PCSX2/memcards")).unwrap();
        put(&home.join(".config/ppsspp/PSP/SAVEDATA/logs/only.txt"), "log");
        put(&home.join(".local/share/rare/webgames/cool-game/data.json"), "{}");
        put(&home.join(".local/share/rare/webgames/cool-game/WebKitCache/c"), "c");
        fs::create_dir_all(home.join(".local/share/rare/webgames/empty")).unwrap();
        fs::create_dir_all(home.join("Emulation/saves")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(home.join(".config/retroarch/saves"), home.join("Emulation/saves/retroarch")).unwrap();
        put(&home.join("Emulation/saves/other/s.sav"), "s");

        let sets = detect_in(&home);
        let ids: Vec<&str> = sets.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"retroarch-saves"));
        assert!(ids.contains(&"rpcs3"));
        assert!(ids.contains(&"webgame:cool-game"));
        assert!(ids.contains(&"emudeck-other"));
        assert!(!ids.contains(&"emudeck-retroarch"), "{ids:?}");
        assert!(!ids.contains(&"pcsx2-memcards"));
        assert!(!ids.contains(&"ppsspp"));
        assert!(!ids.contains(&"webgame:empty"));
        let ra = sets.iter().find(|s| s.id == "retroarch-saves").unwrap();
        assert_eq!((ra.files, ra.bytes), (1, 3));
        assert_eq!(sets.iter().find(|s| s.id == "webgame:cool-game").unwrap().files, 1);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn roundtrip_and_move_aside() {
        let home = fake_home("rt");
        let src = home.join(".config/retroarch/saves");
        put(&src.join("sub/a.srm"), "hello");
        put(&src.join("b.srm"), "world");
        put(&src.join("shaders/skip"), "no");
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        fs::File::options().write(true).open(src.join("b.srm")).unwrap().set_modified(when).unwrap();

        let data = pack_dir(&src).unwrap();
        put(&src.join("new-local.srm"), "stale");
        let rel = ".config/retroarch/saves";
        let moved = restore_into(&home, "retroarch-saves", rel, &data).unwrap();
        assert!(moved.contains(".rare-before-restore-"));
        assert!(Path::new(&moved).join("new-local.srm").exists());
        assert_eq!(fs::read_to_string(src.join("sub/a.srm")).unwrap(), "hello");
        assert_eq!(fs::read_to_string(src.join("b.srm")).unwrap(), "world");
        assert!(!src.join("shaders").exists());
        assert!(!src.join("new-local.srm").exists());
        let m = fs::metadata(src.join("b.srm")).unwrap().modified().unwrap();
        assert_eq!(m.duration_since(UNIX_EPOCH).unwrap().as_secs(), 1_600_000_000);

        // Restoring into an absent target moves nothing.
        let home2 = fake_home("rt2");
        assert_eq!(restore_into(&home2, "retroarch-saves", rel, &data).unwrap(), "");
        assert_eq!(fs::read_to_string(home2.join(rel).join("b.srm")).unwrap(), "world");
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&home2);
    }

    #[test]
    fn rejects_bad_requests() {
        let home = fake_home("bad");
        let ok = gz_tar(|b| raw_entry(b, "a.sav", tar::EntryType::Regular, None, b"x"));
        let rel = ".config/retroarch/saves";
        assert!(restore_into(&home, "retroarch-saves", ".config/rpcs3", &ok).is_err());
        assert!(restore_into(&home, "retroarch-saves", "../x", &ok).is_err());
        assert!(restore_into(&home, "nope", rel, &ok).is_err());
        assert!(restore_into(&home, "webgame:../x", ".local/share/rare/webgames/../x", &ok).is_err());
        assert!(restore_into(&home, "webgame:Bad", ".local/share/rare/webgames/Bad", &ok).is_err());
        assert!(restore_into(&home, "webgame:ok-1", ".local/share/rare/webgames/ok-1", &ok).is_ok());
        assert!(restore_into(&home, "emudeck-foo", "Emulation/saves/foo", &ok).is_ok());

        put(&home.join(rel).join("keep.srm"), "keep");
        let evil = gz_tar(|b| raw_entry(b, "../evil", tar::EntryType::Regular, None, b"x"));
        let abs = gz_tar(|b| raw_entry(b, "/etc/evil", tar::EntryType::Regular, None, b"x"));
        let sym = gz_tar(|b| raw_entry(b, "ln", tar::EntryType::Symlink, Some("/etc"), b""));
        let hard = gz_tar(|b| raw_entry(b, "hl", tar::EntryType::Link, Some("keep.srm"), b""));
        for bad in [&evil, &abs, &sym, &hard] {
            assert!(restore_into(&home, "retroarch-saves", rel, bad).is_err());
        }
        // Nothing was moved aside or written by the rejected archives.
        assert_eq!(fs::read_to_string(home.join(rel).join("keep.srm")).unwrap(), "keep");
        assert!(!home.join(".config/retroarch/evil").exists());
        assert!(!home.join(".config/evil").exists());
        let _ = fs::remove_dir_all(&home);
    }
}
