//! Automatic cloud saves, the device half: what a game's saves are on this device ([`Target`]), a
//! fingerprint of them, the local version history (every copy Rare replaces is kept here first),
//! the last synced point per save set, and the decision before a launch and after a session
//! ([`plan`]). No network: the launcher pairs this with [`super::walrus`] and [`super::store`].

use super::ludusavi::{matches, may_hold, Root};
use super::local::{self, SaveSet};
use super::meta::Backup;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Local versions kept per save set.
pub const KEEP_LOCAL: usize = 10;
/// Cloud versions kept per save set (older automatic uploads are deleted).
pub const KEEP_CLOUD: usize = 5;
const MAX_FILES: usize = 50_000;
const MAX_BYTES: u64 = 512 << 20;

/// A PC game's saves: one or more places from the Ludusavi manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameSet {
    /// "game:<library id>"
    pub id: String,
    pub title: String,
    pub roots: Vec<Root>,
}

/// Something Rare syncs: an emulator or web game folder (the manual backups' format), or a PC
/// game's save places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Dir(SaveSet),
    Game(GameSet),
}

/// One file of a target: its name in the archive, where it is, size, mtime (s).
#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub mtime: u64,
}

/// What the saves look like now.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scan {
    pub files: u64,
    pub bytes: u64,
    /// newest file, unix ms
    pub modified: Option<i64>,
    /// "" when there are no files
    pub fp: String,
}

fn mtime(md: &fs::Metadata) -> u64 {
    md.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

fn rel_parts(rel: &Path) -> Vec<String> {
    rel.components().filter_map(|c| match c {
        Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
        _ => None,
    }).collect()
}

fn walk_dir(root: &Path, webgame: bool) -> Result<Vec<Item>, String> {
    let mut out = Vec::new();
    let Ok(root) = fs::canonicalize(root) else { return Ok(out) };
    let mut stack = vec![(root.clone(), PathBuf::new())];
    while let Some((dir, rel)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(md) = fs::symlink_metadata(e.path()) else { continue };
            let name = e.file_name();
            let r = rel.join(&name);
            if md.is_dir() {
                if !local::is_excluded(&name.to_string_lossy(), webgame) {
                    stack.push((e.path(), r));
                }
            } else if md.is_file() {
                out.push(Item { name: r.to_string_lossy().into_owned(), path: e.path(), size: md.len(), mtime: mtime(&md) });
                if out.len() > MAX_FILES {
                    return Err("too many save files".into());
                }
            }
        }
    }
    Ok(out)
}

fn walk_root(root: &Root) -> Result<Vec<Item>, String> {
    let mut out = Vec::new();
    if !root.base.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![(root.base.clone(), Vec::<String>::new())];
    while let Some((dir, rel)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(md) = fs::symlink_metadata(e.path()) else { continue };
            let mut r = rel.clone();
            r.push(e.file_name().to_string_lossy().into_owned());
            if md.is_dir() {
                if may_hold(&root.rest, &r) && !(matches(&root.rest, &r) && local::is_excluded(r.last().unwrap(), false)) {
                    stack.push((e.path(), r));
                }
            } else if md.is_file() && matches(&root.rest, &r) {
                out.push(Item { name: format!("{}/{}", root.key, r.join("/")), path: e.path(), size: md.len(), mtime: mtime(&md) });
                if out.len() > MAX_FILES {
                    return Err("too many save files".into());
                }
            }
        }
    }
    Ok(out)
}

impl Target {
    pub fn id(&self) -> &str {
        match self {
            Target::Dir(s) => &s.id,
            Target::Game(g) => &g.id,
        }
    }

    /// "RetroArch save files", "Hollow Knight".
    pub fn label(&self) -> String {
        match self {
            Target::Dir(s) if s.kind == "webgame" => s.label.clone(),
            Target::Dir(s) => format!("{} {}", s.emulator, s.label.to_lowercase()),
            Target::Game(g) => g.title.clone(),
        }
    }

    pub fn items(&self) -> Result<Vec<Item>, String> {
        let mut v = match self {
            Target::Dir(s) => walk_dir(Path::new(&s.path), s.kind == "webgame")?,
            Target::Game(g) => {
                let mut v = Vec::new();
                for r in &g.roots {
                    v.extend(walk_root(r)?);
                }
                v
            }
        };
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v.dedup_by(|a, b| a.name == b.name);
        Ok(v)
    }

    pub fn scan(&self) -> Result<Scan, String> {
        Ok(scan_items(&self.items()?))
    }

    /// The saves as a tar.gz (the format restore reads), with its scan.
    pub fn pack(&self) -> Result<(Vec<u8>, Scan), String> {
        let items = self.items()?;
        let scan = scan_items(&items);
        if scan.bytes > MAX_BYTES {
            return Err("these saves are larger than 512 MB".into());
        }
        let data = match self {
            Target::Dir(s) => local::pack_dir(Path::new(&s.path))?,
            Target::Game(_) => {
                let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                let mut b = tar::Builder::new(gz);
                b.follow_symlinks(false);
                for it in &items {
                    b.append_path_with_name(&it.path, &it.name).map_err(|e| format!("pack {}: {e}", it.name))?;
                }
                b.into_inner().map_err(|e| e.to_string())?.finish().map_err(|e| e.to_string())?
            }
        };
        Ok((data, scan))
    }

    /// Puts a packed copy in place. The caller keeps the current saves in the history first: the
    /// files this replaces are removed.
    pub fn restore(&self, home: &Path, data: &[u8]) -> Result<(), String> {
        match self {
            Target::Dir(s) => {
                let moved = local::restore_into(home, &s.id, &s.rel, data)?;
                if !moved.is_empty() {
                    let _ = fs::remove_dir_all(&moved);
                }
                Ok(())
            }
            Target::Game(g) => restore_game(g, data),
        }
    }
}

/// The fingerprint and totals of a file list.
pub fn scan_items(items: &[Item]) -> Scan {
    if items.is_empty() {
        return Scan::default();
    }
    let mut h = Sha256::new();
    let (mut bytes, mut newest) = (0u64, 0u64);
    for it in items {
        h.update(it.name.as_bytes());
        h.update(format!("\0{}\0{}\n", it.size, it.mtime).as_bytes());
        bytes += it.size;
        newest = newest.max(it.mtime);
    }
    let d = h.finalize();
    Scan { files: items.len() as u64, bytes, modified: (newest > 0).then_some(newest as i64 * 1000), fp: d[..8].iter().map(|b| format!("{b:02x}")).collect() }
}

fn restore_game(g: &GameSet, data: &[u8]) -> Result<(), String> {
    // Check every entry first: known place, a path the place's pattern takes, files only.
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(data));
    let mut total = 0u64;
    for e in ar.entries().map_err(|e| format!("bad archive: {e}"))? {
        let e = e.map_err(|e| format!("bad archive: {e}"))?;
        let ty = e.header().entry_type();
        if !(ty.is_file() || ty.is_dir()) {
            return Err("archive contains a link or special entry".into());
        }
        let p = e.path().map_err(|e| e.to_string())?.into_owned();
        if p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
            return Err("unsafe path in archive".into());
        }
        let parts = rel_parts(&p);
        let Some((key, rel)) = parts.split_first() else { continue };
        let root = g.roots.iter().find(|r| &r.key == key);
        if ty.is_file() && !root.is_some_and(|r| matches(&r.rest, rel)) {
            // A place this device does not have (another system's path): skipped, not refused.
            continue;
        }
        total += e.header().size().unwrap_or(0);
        if total > MAX_BYTES * 2 {
            return Err("archive is too large".into());
        }
    }
    for it in Target::Game(g.clone()).items()? {
        fs::remove_file(&it.path).map_err(|e| format!("replace {}: {e}", it.name))?;
    }
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(data));
    ar.set_preserve_mtime(true);
    for e in ar.entries().map_err(|e| e.to_string())? {
        let mut e = e.map_err(|e| e.to_string())?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let parts = rel_parts(&e.path().map_err(|e| e.to_string())?);
        let Some((key, rel)) = parts.split_first() else { continue };
        let Some(root) = g.roots.iter().find(|r| &r.key == key).filter(|r| matches(&r.rest, rel)) else { continue };
        let dest = rel.iter().fold(root.base.clone(), |p, c| p.join(c));
        let parent = dest.parent().ok_or("bad path")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let (cp, cb) = (fs::canonicalize(parent).map_err(|e| e.to_string())?, fs::canonicalize(&root.base).map_err(|e| e.to_string())?);
        if !cp.starts_with(&cb) {
            return Err("unsafe path in archive".into());
        }
        e.unpack(&dest).map_err(|er| format!("unpack {}: {er}", dest.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- the local history

/// One kept copy on this device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// unix ms
    pub created: i64,
    pub files: u64,
    pub bytes: u64,
    pub fp: String,
    /// "played" | "restore" | "download" | "conflict" | "first"
    pub reason: String,
    /// newest save file, unix ms
    #[serde(default)]
    pub modified: Option<i64>,
}

pub struct History {
    root: PathBuf,
}

fn safe(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

impl History {
    pub fn new(root: impl Into<PathBuf>) -> History {
        History { root: root.into() }
    }

    fn dir(&self, set: &str) -> PathBuf {
        self.root.join(safe(set))
    }

    /// Newest first.
    pub fn list(&self, set: &str) -> Vec<Version> {
        let mut v: Vec<Version> = fs::read(self.dir(set).join("index.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        v.sort_by_key(|x| std::cmp::Reverse(x.created));
        v
    }

    fn write_index(&self, set: &str, v: &[Version]) -> Result<(), String> {
        let d = self.dir(set);
        let tmp = d.join("index.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap_or_default()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, d.join("index.json")).map_err(|e| e.to_string())
    }

    /// Keeps a packed copy. The same saves as the newest kept copy are not kept twice.
    pub fn add(&self, set: &str, data: &[u8], scan: &Scan, reason: &str, now_ms: i64) -> Result<Version, String> {
        let mut list = self.list(set);
        if let Some(top) = list.first().filter(|t| t.fp == scan.fp) {
            return Ok(top.clone());
        }
        let d = self.dir(set);
        fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let created = list.first().map_or(now_ms, |t| now_ms.max(t.created + 1));
        let file = d.join(format!("{created}.tar.gz"));
        fs::write(&file, data).map_err(|e| format!("keep a copy: {e}"))?;
        let v = Version { created, files: scan.files, bytes: scan.bytes, fp: scan.fp.clone(), reason: reason.to_string(), modified: scan.modified };
        list.insert(0, v.clone());
        for old in list.split_off(KEEP_LOCAL.min(list.len())) {
            let _ = fs::remove_file(d.join(format!("{}.tar.gz", old.created)));
        }
        self.write_index(set, &list)?;
        Ok(v)
    }

    pub fn read(&self, set: &str, created: i64) -> Result<Vec<u8>, String> {
        fs::read(self.dir(set).join(format!("{created}.tar.gz"))).map_err(|e| format!("read the kept copy: {e}"))
    }
}

// ---------------------------------------------------------------- sync points and decisions

/// The last point this device and the cloud agreed on, per save set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Base {
    pub fp: String,
    /// The Blob object id of the cloud copy at that point.
    pub cloud: Option<String>,
    pub at: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncState {
    pub sets: HashMap<String, Base>,
}

impl SyncState {
    pub fn load(path: &Path) -> SyncState {
        fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Nothing to do (no saves anywhere, or the cloud copy is gone locally on purpose).
    Nothing,
    /// The same on both sides: record the sync point.
    InSync,
    Upload,
    Download,
    /// Both sides changed since they last agreed (or never agreed): the player picks.
    Conflict,
}

/// What to do with one save set, from the saves now, the last sync point and the newest cloud copy.
pub fn plan(local: &Scan, base: Option<&Base>, cloud: Option<&Backup>) -> Plan {
    let have = local.files > 0;
    match (have, cloud) {
        (false, None) => Plan::Nothing,
        (false, Some(c)) => {
            if base.is_some_and(|b| b.cloud.as_deref() == Some(c.blob_object_id.as_str())) {
                Plan::Nothing
            } else {
                Plan::Download
            }
        }
        (true, None) => Plan::Upload,
        (true, Some(c)) => {
            if c.meta.fp.as_deref() == Some(local.fp.as_str()) {
                return Plan::InSync;
            }
            let Some(b) = base else { return Plan::Conflict };
            let local_changed = b.fp != local.fp;
            let cloud_changed = b.cloud.as_deref() != Some(c.blob_object_id.as_str());
            match (local_changed, cloud_changed) {
                (false, false) => Plan::InSync,
                (true, false) => Plan::Upload,
                (false, true) => Plan::Download,
                (true, true) => Plan::Conflict,
            }
        }
    }
}

/// Cloud copies of one set, newest first.
pub fn versions_of<'a>(list: &'a [Backup], set: &str) -> Vec<&'a Backup> {
    let mut v: Vec<&Backup> = list.iter().filter(|b| b.meta.id == set).collect();
    v.sort_by_key(|b| std::cmp::Reverse(b.meta.created));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saves::ludusavi::{self, Ctx, Entry, Rule};
    use crate::saves::meta::BackupMeta;
    use std::time::{Duration, SystemTime};

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rare-sync-{tag}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn put(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
    fn backup(id: &str, fp: Option<&str>, created: i64) -> Backup {
        Backup { blob_object_id: id.into(), blob_id: "b".into(), size: 1, end_epoch: None, meta: BackupMeta { v: 1, id: "s".into(), created, fp: fp.map(str::to_string), ..Default::default() } }
    }
    fn scan(fp: &str) -> Scan {
        Scan { files: if fp.is_empty() { 0 } else { 1 }, bytes: 1, modified: None, fp: fp.into() }
    }

    #[test]
    fn decisions() {
        let base = Base { fp: "a".into(), cloud: Some("0xc1".into()), at: 1 };
        let c1 = backup("0xc1", Some("a"), 1);
        let c2 = backup("0xc2", Some("z"), 2);
        let old = backup("0xold", None, 1);
        assert_eq!(plan(&scan(""), None, None), Plan::Nothing);
        assert_eq!(plan(&scan(""), None, Some(&c2)), Plan::Download);
        assert_eq!(plan(&scan(""), Some(&Base { cloud: Some("0xc2".into()), ..base.clone() }), Some(&c2)), Plan::Nothing);
        assert_eq!(plan(&scan("a"), None, None), Plan::Upload);
        assert_eq!(plan(&scan("a"), Some(&base), Some(&c1)), Plan::InSync);
        assert_eq!(plan(&scan("b"), Some(&base), Some(&c1)), Plan::Upload);
        assert_eq!(plan(&scan("a"), Some(&base), Some(&c2)), Plan::Download);
        assert_eq!(plan(&scan("b"), Some(&base), Some(&c2)), Plan::Conflict);
        assert_eq!(plan(&scan("z"), Some(&base), Some(&c2)), Plan::InSync, "same saves on both sides");
        assert_eq!(plan(&scan("b"), None, Some(&old)), Plan::Conflict, "never synced here, both have saves");
    }

    #[test]
    fn history_keeps_the_last_copies() {
        let root = tmp("hist");
        let h = History::new(&root);
        for i in 0..(KEEP_LOCAL as i64 + 3) {
            h.add("game:x", format!("data{i}").as_bytes(), &scan(&format!("fp{i}")), "played", 1000 + i).unwrap();
        }
        // the same saves twice are kept once
        h.add("game:x", b"again", &scan(&format!("fp{}", KEEP_LOCAL + 2)), "played", 5000).unwrap();
        let l = h.list("game:x");
        assert_eq!(l.len(), KEEP_LOCAL);
        assert_eq!(l[0].fp, format!("fp{}", KEEP_LOCAL + 2));
        assert_eq!(h.read("game:x", l[0].created).unwrap(), format!("data{}", KEEP_LOCAL + 2).into_bytes());
        assert!(h.read("game:x", 1000).is_err(), "the oldest copy is gone");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn game_saves_round_trip() {
        let home = tmp("game");
        let prefix = home.join(".local/share/rare/prefixes/local-x");
        let base = home.join("Games/X");
        let e = Entry { title: "X".into(), files: vec![Rule { path: "<winAppData>/Studio/X".into(), os: vec![] }, Rule { path: "<base>/save/*.sav".into(), os: vec![] }] };
        let cx = Ctx { home: home.clone(), prefix: Some(prefix.clone()), base: Some(base.clone()), windows: true, user: "deck".into() };
        let g = Target::Game(GameSet { id: "game:local:x".into(), title: "X".into(), roots: ludusavi::roots(&e, &cx) });
        let appdata = prefix.join("drive_c/users/steamuser/AppData/Roaming/Studio/X");
        put(&appdata.join("profile/slot1.dat"), "one");
        put(&base.join("save/a.sav"), "aaa");
        put(&base.join("save/readme.txt"), "not a save");
        put(&base.join("game.exe"), "exe");
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options().write(true).open(base.join("save/a.sav")).unwrap().set_modified(when).unwrap();
        let (data, s1) = g.pack().unwrap();
        assert_eq!(s1.files, 2);
        assert_eq!(g.scan().unwrap(), s1);

        // play on: a new file and a change
        put(&base.join("save/b.sav"), "bbb");
        put(&appdata.join("profile/slot1.dat"), "changed");
        assert_ne!(g.scan().unwrap().fp, s1.fp);
        g.restore(&home, &data).unwrap();
        assert_eq!(fs::read_to_string(appdata.join("profile/slot1.dat")).unwrap(), "one");
        assert!(!base.join("save/b.sav").exists());
        assert!(base.join("game.exe").exists() && base.join("save/readme.txt").exists());
        assert_eq!(g.scan().unwrap().fp, s1.fp, "mtimes come back too");

        // a fresh device: nothing there yet
        let _ = fs::remove_dir_all(&prefix);
        g.restore(&home, &data).unwrap();
        assert_eq!(fs::read_to_string(appdata.join("profile/slot1.dat")).unwrap(), "one");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn game_restore_refuses_bad_archives() {
        let home = tmp("bad");
        let g = GameSet { id: "game:x".into(), title: "X".into(), roots: vec![Root { key: "k1".into(), base: home.join("saves"), rest: vec!["*.sav".into()] }] };
        put(&home.join("saves/keep.sav"), "keep");
        let mk = |name: &str, ty: tar::EntryType| {
            let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
            let mut h = tar::Header::new_gnu();
            h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
            h.set_entry_type(ty);
            h.set_size(1);
            h.set_mode(0o644);
            h.set_cksum();
            b.append(&h, &b"x"[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap()
        };
        assert!(restore_game(&g, &mk("k1/../../evil.sav", tar::EntryType::Regular)).is_err());
        assert!(restore_game(&g, &mk("k1/link.sav", tar::EntryType::Symlink)).is_err());
        assert_eq!(fs::read_to_string(home.join("saves/keep.sav")).unwrap(), "keep");
        // an unknown place or a file the pattern does not take is skipped
        restore_game(&g, &mk("zz/a.sav", tar::EntryType::Regular)).unwrap();
        assert!(!home.join("saves/a.sav").exists());
        let _ = fs::remove_dir_all(home);
    }
}
