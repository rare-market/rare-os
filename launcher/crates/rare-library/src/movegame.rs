//! Moving a game between drives (this device and an SD card or USB drive).
//!
//! Every move is copy, check, then switch: the files are copied to the other drive, the copy is
//! read back and compared byte for byte with the original (by hash), the store's own records are
//! pointed at the new place, and only then is the original deleted. A failure or a cancel at any
//! step before the switch removes the partial copy and leaves the game where it was.
//!
//! - Steam: the game's folder, its appmanifest, and its compatdata (Windows saves), shadercache and
//!   workshop content go to a Steam library on the other drive (made and registered when there is
//!   none). Steam must be closed while this runs (the caller closes it quietly and starts it again),
//!   since Steam owns its library files and would rewrite them.
//! - Epic and GOG through Heroic or legendary: the folder moves, and the install path is rewritten
//!   in legendary's and Heroic's installed lists.
//! - Games folders: the folder moves to the other drive's Games folder, which the scan reads.

use crate::model::{Game, LaunchSpec, Source};
use crate::storage::{Drive, DriveKind};
use crate::{steam, vdf, vdfedit};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hasher;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq)]
pub enum MoveKind {
    Steam {
        appid: u32,
        /// The library the game leaves and the one it goes to (folders holding steamapps/).
        from_lib: PathBuf,
        to_lib: PathBuf,
        /// The target library is new: make it and add it to Steam's list.
        register: bool,
    },
    /// Rewrite the install path in these JSON files (legendary, Heroic).
    Heroic { files: Vec<PathBuf> },
    Local,
    /// A Rare Store game: its whole folder moves between Rare folders (rare-store finds it there).
    Rare,
    /// An itch.io game: between itch's apps folder and Games/itch on a drive (Rare scans both).
    Itch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovePlan {
    pub game_id: String,
    pub title: String,
    pub kind: MoveKind,
    /// The game's folder, before and after.
    pub from: PathBuf,
    pub to: PathBuf,
    /// Every file or folder to copy, in order (for Steam the manifest comes last).
    pub parts: Vec<(PathBuf, PathBuf)>,
    pub bytes: u64,
    pub dest: Drive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Copying,
    Checking,
    Finishing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    /// The library id before and after (a Games folder game's id follows its path).
    pub old_id: String,
    pub new_id: String,
    /// Moved, but something after the switch did not go through (the old copy is still there).
    pub warning: Option<String>,
}

/// Room kept free on the target drive on top of the game.
const HEADROOM: u64 = 512 * 1024 * 1024;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn canon(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Where Heroic installs by default on this device (~/Games/Heroic when it does not say).
fn heroic_default_dir() -> PathBuf {
    for c in crate::heroic::config_dirs() {
        let Some(v) = fs::read_to_string(c.join("heroic/config.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
        if let Some(p) = v.pointer("/defaultSettings/defaultInstallPath").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            return PathBuf::from(p.replace('~', &home().to_string_lossy()));
        }
    }
    home().join("Games/Heroic")
}

/// The JSON lists that record where legendary and Heroic installed games.
fn heroic_lists() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for c in crate::heroic::config_dirs() {
        v.push(c.join("legendary/installed.json"));
        v.push(c.join("heroic/legendaryConfig/legendary/installed.json"));
        v.push(c.join("heroic/gog_store/installed.json"));
        v.push(c.join("heroic/sideload_apps/library.json"));
    }
    v.retain(|p| p.is_file());
    v.dedup();
    v
}

/// The Steam library on `dest` to move into: a registered one, one Steam left on the card, or a
/// new SteamLibrary folder (`true`: it has to be made and registered).
fn steam_target(dest: &Drive) -> Result<(PathBuf, bool), String> {
    let roots = steam::find_roots();
    let root = roots.iter().find(|r| r.primary).ok_or("Steam isn't set up on this device yet.")?;
    if dest.kind == DriveKind::Internal {
        return Ok((root.path.clone(), false));
    }
    for l in steam::registered_libraries(&root.path) {
        let c = canon(&l);
        if c.starts_with(&dest.mount) && c.join("steamapps").is_dir() {
            return Ok((c, false));
        }
    }
    for l in steam::libraries_on_mounts() {
        let c = canon(&l);
        if c.starts_with(&dest.mount) {
            return Ok((c, true));
        }
    }
    Ok((dest.mount.join("SteamLibrary"), true))
}

/// What moving `g` to `dest` copies and changes. Checks the game can move and fits.
pub fn plan(g: &Game, dest: &Drive) -> Result<MovePlan, String> {
    if let Some(why) = crate::storage::no_move_reason(g) {
        return Err(why);
    }
    let from = canon(Path::new(g.install_dir.as_deref().ok_or("This game has no folder to move.")?));
    if !from.is_dir() {
        return Err(format!("{} isn't there anymore.", from.display()));
    }
    if from.starts_with(&dest.mount) && dest.kind != DriveKind::Internal {
        return Err(format!("It's already on the {}.", dest.label));
    }
    let name = from.file_name().ok_or("This game's folder has no name.")?.to_os_string();
    let (kind, to, mut parts) = match (&g.launch, g.source) {
        (LaunchSpec::Steam { appid }, Source::Steam) => {
            let appid = *appid;
            let from_lib = from.parent().and_then(Path::parent).and_then(Path::parent).map(Path::to_path_buf).ok_or("This Steam game isn't in a Steam library.")?;
            let manifest = from_lib.join(format!("steamapps/appmanifest_{appid}.acf"));
            if !manifest.is_file() {
                return Err("Steam's record for this game is missing. Open Steam once, then try again.".into());
            }
            if steam::downloads().iter().any(|d| d.id == g.id) {
                return Err("Steam is updating this game. Move it when the update is done.".into());
            }
            let (to_lib, register) = steam_target(dest)?;
            if canon(&to_lib) == from_lib {
                return Err(format!("It's already on the {}.", dest.label));
            }
            let to = to_lib.join("steamapps/common").join(&name);
            let mut parts = vec![(from.clone(), to.clone())];
            for sub in [format!("compatdata/{appid}"), format!("shadercache/{appid}"), format!("workshop/content/{appid}"), format!("workshop/appworkshop_{appid}.acf")] {
                let p = from_lib.join("steamapps").join(&sub);
                if p.exists() {
                    parts.push((p, to_lib.join("steamapps").join(&sub)));
                }
            }
            // The manifest goes last: Steam sees the game in its new library only once it is all there.
            parts.push((manifest, to_lib.join(format!("steamapps/appmanifest_{appid}.acf"))));
            (MoveKind::Steam { appid, from_lib, to_lib, register }, to, parts)
        }
        (LaunchSpec::Heroic { .. } | LaunchSpec::Legendary { .. }, Source::Epic | Source::Gog) => {
            let base = if dest.kind == DriveKind::Internal { heroic_default_dir() } else { dest.mount.join("Games/Heroic") };
            let to = base.join(&name);
            (MoveKind::Heroic { files: heroic_lists() }, to.clone(), vec![(from.clone(), to)])
        }
        (LaunchSpec::Exec { .. }, Source::Local) => {
            let base = if dest.kind == DriveKind::Internal { home().join("Games") } else { dest.mount.join("Games") };
            let to = base.join(&name);
            (MoveKind::Local, to.clone(), vec![(from.clone(), to)])
        }
        (LaunchSpec::Exec { .. }, Source::Rare) => {
            let base = if dest.kind == DriveKind::Internal { home().join("Games/Rare") } else { dest.mount.join("Games/Rare") };
            let to = base.join(&name);
            (MoveKind::Rare, to.clone(), vec![(from.clone(), to)])
        }
        (LaunchSpec::Exec { .. }, Source::Itch) => {
            let base = if dest.kind == DriveKind::Internal { dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("itch/apps") } else { dest.mount.join("Games/itch") };
            let to = base.join(&name);
            (MoveKind::Itch, to.clone(), vec![(from.clone(), to)])
        }
        _ => return Err("This one can't move.".into()),
    };
    for (_, d) in &parts {
        if d.exists() {
            return Err(format!("{} is already there. Remove it in Desktop mode first.", d.display()));
        }
    }
    let bytes: u64 = parts.iter().map(|(s, _)| tree_bytes(s)).sum();
    let free = crate::storage::space(&dest.mount).map(|(_, f)| f).unwrap_or(dest.free);
    if bytes + HEADROOM > free {
        return Err(format!("Not enough space on the {}: it needs {} and has {} free.", dest.label, gb(bytes + HEADROOM), gb(free)));
    }
    parts.retain(|(s, _)| s.exists());
    Ok(MovePlan { game_id: g.id.clone(), title: g.title.clone(), kind, from, to, parts, bytes, dest: dest.clone() })
}

fn gb(b: u64) -> String {
    let g = b as f64 / 1e9;
    if g >= 10.0 {
        format!("{g:.0} GB")
    } else if g >= 1.0 {
        format!("{g:.1} GB")
    } else {
        format!("{:.0} MB", (b as f64 / 1e6).max(1.0))
    }
}

/// Apparent bytes of every file under `p` (what the copy writes).
fn tree_bytes(p: &Path) -> u64 {
    let Ok(m) = fs::symlink_metadata(p) else { return 0 };
    if m.is_dir() {
        fs::read_dir(p).map(|rd| rd.flatten().map(|e| tree_bytes(&e.path())).sum()).unwrap_or(0)
    } else if m.is_file() {
        m.len()
    } else {
        0
    }
}

/// One file's copy, with the hash of what was read so the check can compare.
struct Copied {
    from: PathBuf,
    to: PathBuf,
    hash: u64,
    len: u64,
}

struct Copier<'a> {
    on: &'a mut dyn FnMut(Progress),
    cancel: &'a AtomicBool,
    done: u64,
    total: u64,
    files: Vec<Copied>,
    links: Vec<(PathBuf, PathBuf)>,
    last: u64,
}

const CHUNK: usize = 1 << 20;

impl Copier<'_> {
    fn tick(&mut self, phase: Phase, n: u64) -> Result<(), String> {
        self.done += n;
        if self.cancel.load(Ordering::Relaxed) {
            return Err("Cancelled.".into());
        }
        if self.done - self.last >= 32 * CHUNK as u64 || self.done == self.total {
            self.last = self.done;
            (self.on)(Progress { phase, done: self.done, total: self.total });
        }
        Ok(())
    }

    fn copy(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        let m = fs::symlink_metadata(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?;
        let ft = m.file_type();
        if ft.is_symlink() {
            let target = fs::read_link(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?;
            symlink(&target, to).map_err(|e| format!("Couldn't copy the link {}: {e}. Cards formatted by Rare can hold links.", from.display()))?;
            self.links.push((from.to_path_buf(), to.to_path_buf()));
        } else if ft.is_dir() {
            fs::create_dir_all(to).map_err(|e| format!("Couldn't make {}: {e}", to.display()))?;
            let mut kids: Vec<_> = fs::read_dir(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?.flatten().collect();
            kids.sort_by_key(|e| e.file_name());
            for e in kids {
                self.copy(&e.path(), &to.join(e.file_name()))?;
            }
            let _ = fs::set_permissions(to, m.permissions());
        } else if ft.is_file() {
            let mut r = fs::File::open(from).map_err(|e| format!("Couldn't open {}: {e}", from.display()))?;
            let mut w = fs::File::create(to).map_err(|e| format!("Couldn't write {}: {e}", to.display()))?;
            let mut h = DefaultHasher::new();
            let mut buf = vec![0u8; CHUNK];
            let mut len = 0u64;
            loop {
                let n = r.read(&mut buf).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?;
                if n == 0 {
                    break;
                }
                h.write(&buf[..n]);
                w.write_all(&buf[..n]).map_err(|e| format!("Couldn't write {}: {e}", to.display()))?;
                len += n as u64;
                self.tick(Phase::Copying, n as u64)?;
            }
            w.sync_all().map_err(|e| format!("Couldn't write {}: {e}", to.display()))?;
            drop_cache(&w);
            let _ = w.set_permissions(m.permissions());
            if let Ok(t) = m.modified() {
                let _ = w.set_modified(t);
            }
            self.files.push(Copied { from: from.to_path_buf(), to: to.to_path_buf(), hash: h.finish(), len });
        }
        Ok(())
    }

    /// Reads every copied file back from the target drive and compares it with the original.
    fn check(&mut self) -> Result<(), String> {
        self.done = 0;
        self.last = 0;
        let files = std::mem::take(&mut self.files);
        for f in &files {
            let mut r = fs::File::open(&f.to).map_err(|e| format!("The copy of {} can't be read: {e}", f.from.display()))?;
            drop_cache(&r);
            let mut h = DefaultHasher::new();
            let mut buf = vec![0u8; CHUNK];
            let mut len = 0u64;
            loop {
                let n = r.read(&mut buf).map_err(|e| format!("The copy of {} can't be read: {e}", f.from.display()))?;
                if n == 0 {
                    break;
                }
                h.write(&buf[..n]);
                len += n as u64;
                self.tick(Phase::Checking, n as u64)?;
            }
            if len != f.len || h.finish() != f.hash {
                return Err(format!("The copy of {} doesn't match the original.", f.from.display()));
            }
        }
        for (from, to) in &self.links {
            if fs::read_link(from).ok() != fs::read_link(to).ok() {
                return Err(format!("The copy of the link {} doesn't match.", from.display()));
            }
        }
        self.files = files;
        Ok(())
    }
}

#[cfg(unix)]
fn symlink(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(not(unix))]
fn symlink(_target: &Path, _at: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("links are not supported"))
}

/// Drops a file's pages from memory, so reading it back comes from the drive, not the cache.
#[cfg(target_os = "linux")]
fn drop_cache(f: &fs::File) {
    use std::os::unix::io::AsRawFd;
    unsafe {
        libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

#[cfg(not(target_os = "linux"))]
fn drop_cache(_f: &fs::File) {}

fn remove(p: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Rewrites every string in a JSON file that is `old` or a path under it to sit under `new`.
/// Returns whether anything changed. The file is replaced in one rename.
pub fn rewrite_paths(file: &Path, old: &Path, new: &Path) -> Result<bool, String> {
    let text = fs::read_to_string(file).map_err(|e| format!("Couldn't read {}: {e}", file.display()))?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("Couldn't read {}: {e}", file.display()))?;
    let (o, n) = (old.to_string_lossy().into_owned(), new.to_string_lossy().into_owned());
    fn walk(v: &mut Value, o: &str, n: &str) -> bool {
        match v {
            Value::String(s) => {
                let t = s.trim_end_matches('/');
                if t == o || t.starts_with(&format!("{o}/")) {
                    *s = format!("{n}{}", &s[o.len()..]);
                    true
                } else {
                    false
                }
            }
            Value::Array(a) => a.iter_mut().fold(false, |c, x| walk(x, o, n) | c),
            Value::Object(m) => m.values_mut().fold(false, |c, x| walk(x, o, n) | c),
            _ => false,
        }
    }
    if !walk(&mut v, &o, &n) {
        return Ok(false);
    }
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    let tmp = file.with_extension("json.rare-tmp");
    fs::write(&tmp, out).map_err(|e| format!("Couldn't write {}: {e}", file.display()))?;
    fs::rename(&tmp, file).map_err(|e| format!("Couldn't write {}: {e}", file.display()))?;
    Ok(true)
}

/// A new Steam library folder: steamapps/ and the libraryfolder.vdf Steam keeps in each library.
pub fn make_steam_library(lib: &Path) -> Result<(), String> {
    fs::create_dir_all(lib.join("steamapps")).map_err(|e| format!("Couldn't make {}: {e}", lib.display()))?;
    let marker = lib.join("libraryfolder.vdf");
    if !marker.exists() {
        let id: u64 = {
            let mut h = DefaultHasher::new();
            h.write(lib.to_string_lossy().as_bytes());
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
            h.finish()
        };
        let text = format!("\"libraryfolder\"\n{{\n\t\"contentid\"\t\t\"{id}\"\n\t\"label\"\t\t\"\"\n}}\n");
        fs::write(&marker, text).map_err(|e| format!("Couldn't write {}: {e}", marker.display()))?;
    }
    Ok(())
}

/// Moves the appid between the "apps" lists of the two libraries in libraryfolders.vdf, so Steam
/// shows the right sizes before it rescans. Best effort: Steam rebuilds these lists itself.
fn steam_apps_lists(appid: u32, from_lib: &Path, to_lib: &Path) {
    let Some(root) = steam::find_roots().into_iter().find(|r| r.primary) else { return };
    let file = root.path.join("steamapps/libraryfolders.vdf");
    let Ok(text) = fs::read_to_string(&file) else { return };
    let v = vdf::parse(&text);
    let Some(lf) = v.get("libraryfolders") else { return };
    let mut from_key = None;
    let mut to_key = None;
    for (k, e) in lf.entries() {
        let Some(p) = e.str("path") else { continue };
        let c = canon(Path::new(p));
        if c == canon(from_lib) {
            from_key = Some(k.clone());
        }
        if c == canon(to_lib) {
            to_key = Some(k.clone());
        }
    }
    let id = appid.to_string();
    let mut out = text.clone();
    let mut size = "0".to_string();
    if let Some(k) = &from_key {
        if let Ok(Some(s)) = vdfedit::get(&out, &["libraryfolders", k, "apps", &id]) {
            size = s;
        }
        if let Ok(t) = vdfedit::remove(&out, &["libraryfolders", k, "apps", &id]) {
            out = t;
        }
    }
    if let Some(k) = &to_key {
        if let Ok(t) = vdfedit::set(&out, &["libraryfolders", k, "apps", &id], &size) {
            out = t;
        }
    }
    if out != text {
        let tmp = file.with_extension("vdf.rare-tmp");
        if fs::write(&tmp, out).is_ok() {
            let _ = fs::rename(tmp, &file);
        }
    }
}

/// The library id a Games folder game gets at its new place (local.rs ids follow the program path).
fn moved_id(plan: &MovePlan, g: &Game) -> String {
    let prefix = match plan.kind {
        MoveKind::Local => "local",
        MoveKind::Itch => "itch",
        _ => return g.id.clone(),
    };
    if let LaunchSpec::Exec { program, .. } = &g.launch {
        let p = Path::new(program);
        let rel = p.strip_prefix(&plan.from).ok().or_else(|| g.install_dir.as_deref().and_then(|d| p.strip_prefix(d).ok()));
        if let Some(rel) = rel {
            return crate::local::id_for(prefix, &plan.to.join(rel).to_string_lossy());
        }
    }
    g.id.clone()
}

/// Runs the move: copy, check, switch, delete the original. Blocks for as long as the copy takes.
/// For Steam the caller has closed Steam first. `cancel` stops it before the switch.
pub fn run(plan: &MovePlan, g: &Game, on: &mut dyn FnMut(Progress), cancel: &AtomicBool) -> Result<Moved, String> {
    if let MoveKind::Steam { .. } = plan.kind {
        if rare_core::proc::steam_running() {
            return Err("Steam is still open.".into());
        }
    }
    if let MoveKind::Heroic { .. } = plan.kind {
        if rare_core::proc::cmdlines().iter().any(|(_, comm, _)| comm.to_lowercase().starts_with("heroic")) {
            return Err("Close Heroic first, then try again.".into());
        }
    }
    let total: u64 = plan.parts.iter().map(|(s, _)| tree_bytes(s)).sum();
    let made: Vec<PathBuf> = plan.parts.iter().map(|(_, d)| d.clone()).collect();
    let cleanup = || {
        for d in &made {
            let _ = remove(d);
        }
    };
    let mut c = Copier { on, cancel, done: 0, total, files: Vec::new(), links: Vec::new(), last: 0 };
    let copied = (|| {
        for (s, d) in &plan.parts {
            if let Some(p) = d.parent() {
                fs::create_dir_all(p).map_err(|e| format!("Couldn't make {}: {e}", p.display()))?;
            }
            c.copy(s, d)?;
        }
        c.check()
    })();
    if let Err(e) = copied {
        cleanup();
        return Err(e);
    }
    (c.on)(Progress { phase: Phase::Finishing, done: total, total });
    // The switch: the store's records point at the copy.
    let switched: Result<(), String> = match &plan.kind {
        MoveKind::Steam { appid, from_lib, to_lib, register } => (|| {
            make_steam_library(to_lib)?;
            if *register {
                steam::add_library(&to_lib.to_string_lossy())?;
            }
            steam_apps_lists(*appid, from_lib, to_lib);
            Ok(())
        })(),
        MoveKind::Heroic { files } => (|| {
            let mut any = false;
            for f in files {
                any |= rewrite_paths(f, &plan.from, &plan.to)?;
            }
            if any {
                Ok(())
            } else {
                Err("Couldn't find this game in Heroic's list.".into())
            }
        })(),
        MoveKind::Local | MoveKind::Rare | MoveKind::Itch => Ok(()),
    };
    if let Err(e) = switched {
        cleanup();
        return Err(e);
    }
    // Only now the original goes. Steam's manifest first, so Steam never sees the game twice.
    let mut warning = None;
    for (s, _) in plan.parts.iter().rev() {
        if let Err(e) = remove(s) {
            warning = Some(format!("Moved, but the old copy at {} couldn't be removed: {e}", s.display()));
        }
    }
    Ok(Moved { old_id: g.id.clone(), new_id: moved_id(plan, g), warning })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Art;

    fn drive(mount: &Path, kind: DriveKind) -> Drive {
        Drive { kind, label: "SD card".into(), mount: mount.to_path_buf(), device: "/dev/mmcblk0p1".into(), fs: "ext4".into(), total: 1 << 40, free: 1 << 40 }
    }

    fn local_game(dir: &Path) -> Game {
        let program = dir.join("start.sh").to_string_lossy().into_owned();
        Game {
            id: crate::local::id_for("local", &program),
            source: Source::Local,
            title: "Test".into(),
            installed: true,
            install_dir: Some(dir.to_string_lossy().into_owned()),
            size_bytes: None,
            last_played: None,
            playtime_minutes: None,
            art: Art::default(),
            launch: LaunchSpec::Exec { program, args: vec![], cwd: None, windows: false },
            origin: String::new(),
            foreign: false,
        }
    }

    #[test]
    fn local_game_moves_and_original_goes_after_the_check() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("home/Games/Cool Game");
        fs::create_dir_all(src.join("data")).unwrap();
        fs::write(src.join("start.sh"), "#!/bin/sh\n").unwrap();
        fs::write(src.join("data/big.bin"), vec![7u8; 3 * CHUNK + 17]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("big.bin", src.join("data/link")).unwrap();
        let card = t.path().join("card");
        fs::create_dir_all(&card).unwrap();
        let g = local_game(&canon(&src));
        let p = plan(&g, &drive(&canon(&card), DriveKind::Sd)).unwrap();
        assert_eq!(p.to, canon(&card).join("Games/Cool Game"));
        let mut seen = Vec::new();
        let m = run(&p, &g, &mut |pr| seen.push(pr.phase), &AtomicBool::new(false)).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read(p.to.join("data/big.bin")).unwrap().len(), 3 * CHUNK + 17);
        #[cfg(unix)]
        assert_eq!(fs::read_link(p.to.join("data/link")).unwrap(), PathBuf::from("big.bin"));
        assert!(seen.contains(&Phase::Checking) && seen.contains(&Phase::Finishing));
        assert_ne!(m.old_id, m.new_id);
        assert_eq!(m.new_id, crate::local::id_for("local", &p.to.join("start.sh").to_string_lossy()));
    }

    #[test]
    fn a_cancelled_move_keeps_the_original_and_removes_the_copy() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("Games/G");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("start.sh"), "x").unwrap();
        fs::write(src.join("a.bin"), vec![1u8; 2 * CHUNK]).unwrap();
        let card = t.path().join("card");
        fs::create_dir_all(&card).unwrap();
        let g = local_game(&canon(&src));
        let p = plan(&g, &drive(&canon(&card), DriveKind::Sd)).unwrap();
        let cancel = AtomicBool::new(true);
        assert!(run(&p, &g, &mut |_| {}, &cancel).is_err());
        assert!(src.join("a.bin").exists());
        assert!(!p.to.exists());
    }

    #[test]
    fn a_rare_store_game_moves_whole_with_its_current_link() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("home/Games/Rare/act");
        fs::create_dir_all(src.join("3")).unwrap();
        fs::write(src.join("3/act.x86_64"), "bin").unwrap();
        fs::write(src.join("install.json"), "{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("3", src.join("current")).unwrap();
        let card = t.path().join("card");
        fs::create_dir_all(&card).unwrap();
        let src = canon(&src);
        let mut g = local_game(&src);
        g.source = Source::Rare;
        g.id = "rare:act".into();
        let p = plan(&g, &drive(&canon(&card), DriveKind::Sd)).unwrap();
        assert_eq!(p.to, canon(&card).join("Games/Rare/act"));
        let m = run(&p, &g, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(m.new_id, "rare:act");
        #[cfg(unix)]
        assert_eq!(fs::read_link(p.to.join("current")).unwrap(), PathBuf::from("3"));
        assert!(!src.exists());
    }

    #[test]
    fn heroic_paths_are_rewritten() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("installed.json");
        fs::write(&f, r#"{"Fortnite":{"install_path":"/home/a/Games/Heroic/Fortnite","exe":"x.exe"},"other":{"install_path":"/home/a/Games/Heroic/Fortnite2"}}"#).unwrap();
        assert!(rewrite_paths(&f, Path::new("/home/a/Games/Heroic/Fortnite"), Path::new("/run/media/a/SD/Games/Heroic/Fortnite")).unwrap());
        let v: Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["Fortnite"]["install_path"], "/run/media/a/SD/Games/Heroic/Fortnite");
        assert_eq!(v["other"]["install_path"], "/home/a/Games/Heroic/Fortnite2");
    }

    #[test]
    fn not_enough_space_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("Games/G");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("start.sh"), "x").unwrap();
        let mut d = drive(&canon(t.path()).join("nowhere"), DriveKind::Sd);
        d.free = 10;
        // space() fails on a path that does not exist, so the drive's own free count is used.
        assert!(plan(&local_game(&canon(&src)), &d).unwrap_err().starts_with("Not enough space"));
    }
}
