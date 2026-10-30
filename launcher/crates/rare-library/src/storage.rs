//! Storage: the drives (this device, an SD card, USB drives) with their free space, and
//! everything that takes space on them (games from every store, Flatpak apps, ROMs, Wine
//! prefixes, the image cache), biggest first.
//!
//! Drives come from /proc/mounts and statvfs. One drive per block device: the internal disk is
//! the device that holds the home folder (on Bazzite /var/home and /var are one btrfs volume).

use crate::model::{Game, Library, LaunchSpec, Source};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriveKind {
    Internal,
    Sd,
    Usb,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drive {
    pub kind: DriveKind,
    /// "This device", "SD card", or a USB drive's folder name.
    pub label: String,
    /// Where it is mounted (for the internal disk: the mount holding the home folder).
    pub mount: PathBuf,
    /// The block device ("/dev/mmcblk0p1").
    pub device: String,
    pub fs: String,
    pub total: u64,
    pub free: u64,
}

impl Drive {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
}

/// One line of /proc/mounts.
#[derive(Debug, Clone, PartialEq)]
pub struct Mount {
    pub device: String,
    pub path: PathBuf,
    pub fs: String,
}

/// /proc/mounts escapes spaces and tabs as octal (\040).
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 4 <= b.len() {
            if let Some(n) = std::str::from_utf8(&b[i + 1..i + 4]).ok().and_then(|t| u8::from_str_radix(t, 8).ok()) {
                out.push(n);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (dev, path, fs) = (it.next()?, it.next()?, it.next()?);
            Some(Mount { device: unescape(dev), path: PathBuf::from(unescape(path)), fs: fs.to_string() })
        })
        .collect()
}

pub fn mounts() -> Vec<Mount> {
    fs::read_to_string("/proc/mounts").map(|t| parse_mounts(&t)).unwrap_or_default()
}

/// Total and free bytes of the filesystem holding `p` (free: what an unprivileged user can use).
#[cfg(unix)]
pub fn space(p: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let f = s.f_frsize as u64;
    Some((s.f_blocks as u64 * f, s.f_bavail as u64 * f))
}

#[cfg(not(unix))]
pub fn space(_p: &Path) -> Option<(u64, u64)> {
    None
}

/// The SD card slot's block device: /dev/mmcblk0 (the slot SteamOS's format-sdcard.sh formats).
/// RARE_SD_DEVICE points it at another device for tests on the device (a loop device holding an
/// image file), never at the internal disk.
pub fn sd_device() -> String {
    match std::env::var("RARE_SD_DEVICE") {
        Ok(d) if d.starts_with("/dev/loop") || d.starts_with("/dev/mmcblk") => d,
        _ => "/dev/mmcblk0".into(),
    }
}

fn is_sd(dev: &str) -> bool {
    dev.starts_with("/dev/mmcblk") || dev.starts_with(&sd_device())
}

fn is_usb(dev: &str) -> bool {
    dev.starts_with("/dev/sd")
}

fn removable_place(p: &Path) -> bool {
    ["/run/media", "/media", "/mnt", "/var/mnt"].iter().any(|b| p.starts_with(b))
}

/// The mount holding `p`: the longest mount path that is a prefix of it.
pub fn mount_of<'a>(p: &Path, ms: &'a [Mount]) -> Option<&'a Mount> {
    ms.iter().filter(|m| p.starts_with(&m.path)).max_by_key(|m| m.path.as_os_str().len())
}

/// The drives from a mounts table: the internal disk first, then SD cards, then USB drives.
/// `space_of` reads total and free bytes for a mount path.
pub fn drives_from(ms: &[Mount], home: &Path, space_of: &dyn Fn(&Path) -> Option<(u64, u64)>) -> Vec<Drive> {
    let mut out: Vec<Drive> = Vec::new();
    if let Some(m) = mount_of(home, ms) {
        let (total, free) = space_of(&m.path).unwrap_or((0, 0));
        out.push(Drive { kind: DriveKind::Internal, label: "This device".into(), mount: m.path.clone(), device: m.device.clone(), fs: m.fs.clone(), total, free });
    }
    let mut extra: Vec<Drive> = Vec::new();
    for m in ms {
        let kind = if is_sd(&m.device) {
            DriveKind::Sd
        } else if is_usb(&m.device) && removable_place(&m.path) {
            DriveKind::Usb
        } else {
            continue;
        };
        if out.iter().chain(extra.iter()).any(|d| d.device == m.device) {
            continue;
        }
        let Some((total, free)) = space_of(&m.path) else { continue };
        let label = match kind {
            DriveKind::Sd => "SD card".to_string(),
            _ => m.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "USB drive".into()),
        };
        extra.push(Drive { kind, label, mount: m.path.clone(), device: m.device.clone(), fs: m.fs.clone(), total, free });
    }
    extra.sort_by_key(|d| (d.kind != DriveKind::Sd, d.mount.clone()));
    out.extend(extra);
    out
}

pub fn drives() -> Vec<Drive> {
    let home = dirs::home_dir().map(|h| fs::canonicalize(&h).unwrap_or(h)).unwrap_or_else(|| PathBuf::from("/"));
    drives_from(&mounts(), &home, &|p| space(p))
}

/// The drive `p` is on (by mount path, longest first).
pub fn drive_of<'a>(p: &Path, drives: &'a [Drive]) -> Option<&'a Drive> {
    let c = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let on = |d: &&Drive| c.starts_with(&d.mount);
    drives.iter().filter(on).max_by_key(|d| d.mount.as_os_str().len()).or_else(|| drives.iter().find(|d| d.kind == DriveKind::Internal))
}

// ---- The SD card slot ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SdCard {
    /// The slot is empty (or the device has none).
    None,
    /// A card is in and mounted.
    Ready { mount: PathBuf, device: String },
    /// A card is in but not mounted: no filesystem the system can read, or still mounting.
    NotReady { device: String, size: u64 },
}

/// The card in the slot, from /sys/block (`sys`) and the mounts table. The SuiPlay's slot is
/// mmcblk0, the device SteamOS's format-sdcard.sh formats.
pub fn sd_card_in(sys: &Path, ms: &[Mount]) -> SdCard {
    let dev = sd_device();
    let blk = sys.join(dev.trim_start_matches("/dev/"));
    let size: u64 = fs::read_to_string(blk.join("size")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0) * 512;
    if size == 0 {
        return SdCard::None;
    }
    if let Some(m) = ms.iter().find(|m| m.device.starts_with(&dev)) {
        return SdCard::Ready { mount: m.path.clone(), device: m.device.clone() };
    }
    SdCard::NotReady { device: dev, size }
}

pub fn sd_card() -> SdCard {
    sd_card_in(Path::new("/sys/block"), &mounts())
}

// ---- What takes space ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Game,
    App,
    Roms,
    /// A Wine prefix (Windows saves and settings for a game).
    Prefix,
    Cache,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Unique per row ("steam:620", "flatpak:org.foo.Bar", "prefix:/path", "cache:img").
    pub key: String,
    pub title: String,
    pub kind: ItemKind,
    /// "Steam", "Epic", "Flatpak", "Emulation"...
    pub source: String,
    /// The library entry it belongs to (Uninstall goes through it).
    pub game_id: Option<String>,
    /// The folder it lives in (picks the drive).
    pub path: Option<PathBuf>,
    pub bytes: u64,
    /// Why it cannot move between drives (None: it can).
    pub no_move: Option<String>,
}

/// Bytes on disk under `p` (links not followed).
pub fn du(p: &Path) -> u64 {
    let Ok(m) = fs::symlink_metadata(p) else { return 0 };
    if m.is_dir() {
        fs::read_dir(p).map(|rd| rd.flatten().map(|e| du(&e.path())).sum()).unwrap_or(0)
    } else {
        on_disk(&m)
    }
}

#[cfg(unix)]
fn on_disk(m: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks() * 512
}

#[cfg(not(unix))]
fn on_disk(m: &fs::Metadata) -> u64 {
    m.len()
}

/// Folder sizes measured before, so a second visit shows numbers at once. Keyed by path; the
/// folder's mtime decides whether the number is still good (a game updated in place keeps its
/// mtime only rarely, and Measure again redoes all).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SizeCache {
    pub sizes: HashMap<String, (u64, u64)>,
}

fn mtime(p: &Path) -> u64 {
    fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

impl SizeCache {
    fn path() -> PathBuf {
        rare_core::paths::cache_dir().join("storage-sizes.json")
    }
    pub fn load() -> SizeCache {
        fs::read(Self::path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        if let Ok(b) = serde_json::to_vec(self) {
            let tmp = p.with_extension("json.tmp");
            if fs::write(&tmp, b).is_ok() {
                let _ = fs::rename(tmp, p);
            }
        }
    }
    /// The size of `p`, from the cache when its mtime matches, else measured (and kept).
    pub fn size(&mut self, p: &Path, fresh: bool) -> u64 {
        let k = p.to_string_lossy().into_owned();
        let mt = mtime(p);
        if !fresh {
            if let Some((m, b)) = self.sizes.get(&k) {
                if *m == mt {
                    return *b;
                }
            }
        }
        let b = du(p);
        self.sizes.insert(k, (mt, b));
        b
    }
}

fn source_name(s: Source) -> &'static str {
    match s {
        Source::Steam => "Steam",
        Source::Epic => "Epic",
        Source::Gog => "GOG",
        Source::Itch => "itch.io",
        Source::Local => "Games folder",
        Source::Web => "Web game",
        Source::Apps => "App",
        Source::Rare => "Rare Store",
        Source::Cloud => "Cloud game",
        Source::Android => "Android",
        Source::Emu => "Emulation",
    }
}

/// Why a library entry cannot move (None: it can).
pub fn no_move_reason(g: &Game) -> Option<String> {
    let r = match (g.source, &g.launch) {
        (Source::Steam, LaunchSpec::Steam { .. }) => return None,
        (Source::Epic | Source::Gog, LaunchSpec::Heroic { .. } | LaunchSpec::Legendary { .. }) => return None,
        (Source::Local, _) if g.origin == "Added by you" => "Games you added by hand stay where they are.",
        (Source::Local, LaunchSpec::Exec { .. }) if g.install_dir.is_some() => return None,
        (Source::Rare | Source::Itch, LaunchSpec::Exec { .. }) if g.install_dir.is_some() => return None,
        (Source::Web, _) => "Web games are small and stay on this device.",
        (Source::Cloud, _) => "Cloud games stream and take no space here.",
        (Source::Android, _) => "Android apps stay inside Android on this device.",
        (Source::Emu, _) => "ROMs stay in the emulation folder.",
        _ => "This one can't move.",
    };
    Some(r.to_string())
}

/// A Flatpak app as `flatpak list` reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatpakApp {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    pub user: bool,
}

/// "286.1 MB", "1.4 GB", "38.0 kB" (flatpak prints SI units; the space may be a no-break space).
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().replace('\u{a0}', " ");
    let (n, unit) = s.split_once(' ').unwrap_or((s.as_str(), "B"));
    let n: f64 = n.trim().replace(',', ".").parse().ok()?;
    let mul = match unit.trim() {
        "B" | "bytes" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some((n * mul) as u64)
}

pub fn parse_flatpak_list(text: &str) -> Vec<FlatpakApp> {
    text.lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() < 4 || c[0].is_empty() {
                return None;
            }
            Some(FlatpakApp { id: c[0].to_string(), name: if c[1].is_empty() { c[0].to_string() } else { c[1].to_string() }, bytes: parse_size(c[2]).unwrap_or(0), user: c[3].trim() == "user" })
        })
        .collect()
}

fn flatpak_apps() -> Vec<FlatpakApp> {
    let mut c = std::process::Command::new("flatpak");
    c.args(["list", "--app", "--columns=application,name,size,installation"]);
    match c.output() {
        Ok(o) if o.status.success() => parse_flatpak_list(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// The Flatpak app a library entry opens, if it is one (`flatpak run <id>`).
fn flatpak_of(g: &Game) -> Option<String> {
    match &g.launch {
        LaunchSpec::Exec { program, args, .. } if program.ends_with("flatpak") => {
            let i = args.iter().position(|a| a == "run")?;
            args[i + 1..].iter().find(|a| !a.starts_with('-')).cloned()
        }
        _ => None,
    }
}

fn data() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share"))
}

/// A web game's storage (rare-webgame's data_dir).
fn web_data(id: &str) -> PathBuf {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    data().join("rare/webgames").join(safe)
}

/// Everything that takes space, biggest first. Slow (it walks folders): call it off the UI thread.
/// `fresh` measures every folder again instead of trusting the size cache.
pub fn items(lib: &Library, cache: &mut SizeCache, fresh: bool) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let mut by_flatpak: HashMap<String, String> = HashMap::new();
    let titles: HashMap<&str, &str> = lib.games.iter().map(|g| (g.id.as_str(), g.title.as_str())).collect();
    for g in &lib.games {
        if let Some(f) = flatpak_of(g) {
            by_flatpak.insert(f, g.id.clone());
            continue;
        }
        if !g.installed {
            continue;
        }
        let (path, bytes) = match g.source {
            Source::Web => {
                let p = web_data(&g.id);
                let b = if p.is_dir() { cache.size(&p, fresh) } else { 0 };
                (Some(p), b)
            }
            _ => match g.install_dir.as_deref().map(PathBuf::from) {
                Some(p) if p.is_dir() => {
                    let b = match g.size_bytes {
                        Some(b) if b > 0 && g.source == Source::Steam => b,
                        _ => cache.size(&p, fresh),
                    };
                    (Some(p), b)
                }
                _ => continue,
            },
        };
        if g.source == Source::Web && bytes == 0 {
            continue;
        }
        let kind = if g.source == Source::Apps { ItemKind::App } else { ItemKind::Game };
        out.push(Item { key: g.id.clone(), title: g.title.clone(), kind, source: source_name(g.source).into(), game_id: Some(g.id.clone()), path, bytes, no_move: no_move_reason(g) });
    }
    for a in flatpak_apps() {
        let home_data = dirs::home_dir().unwrap_or_default().join(".var/app").join(&a.id);
        let data_bytes = if home_data.is_dir() { cache.size(&home_data, fresh) } else { 0 };
        let base = if a.user { data().join("flatpak/app").join(&a.id) } else { PathBuf::from("/var/lib/flatpak/app").join(&a.id) };
        out.push(Item {
            key: format!("flatpak:{}", a.id),
            title: a.name.clone(),
            kind: ItemKind::App,
            source: "Flatpak".into(),
            game_id: by_flatpak.get(&a.id).cloned(),
            path: Some(base),
            bytes: a.bytes + data_bytes,
            no_move: Some("Apps stay on this device.".into()),
        });
    }
    if let Some(emu) = lib.emulation.as_ref().filter(|e| !e.rom_dir.is_empty()) {
        for s in &emu.systems {
            let p = Path::new(&emu.rom_dir).join(&s.id);
            let b = cache.size(&p, fresh);
            if b == 0 {
                continue;
            }
            out.push(Item {
                key: format!("roms:{}", s.id),
                title: format!("{} games", s.name),
                kind: ItemKind::Roms,
                source: "Emulation".into(),
                game_id: None,
                path: Some(p),
                bytes: b,
                no_move: Some("ROMs stay in the emulation folder.".into()),
            });
        }
    }
    // Wine prefixes: Rare's own (umu-run), Heroic's, and Steam's compatdata in every library.
    let mut prefixes: Vec<(PathBuf, Option<String>)> = Vec::new();
    if let Ok(rd) = fs::read_dir(data().join("rare/prefixes")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.replacen('-', ":", 1);
            prefixes.push((e.path(), titles.get(id.as_str()).map(|t| t.to_string())));
        }
    }
    for c in crate::heroic::config_dirs() {
        let Some(v) = fs::read_to_string(c.join("heroic/config.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
        if let Some(p) = v.pointer("/defaultSettings/defaultWinePrefix").and_then(|v| v.as_str()).filter(|p| !p.is_empty()) {
            let p = PathBuf::from(p.replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy()));
            if let Ok(rd) = fs::read_dir(&p) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    let t = lib.games.iter().find(|g| matches!(g.source, Source::Epic | Source::Gog) && g.title == n).map(|g| g.title.clone()).or(Some(n));
                    prefixes.push((e.path(), t));
                }
            }
        }
    }
    for root in crate::steam::find_roots().iter().filter(|r| r.primary) {
        for l in crate::steam::registered_libraries(&root.path) {
            let Ok(rd) = fs::read_dir(l.join("steamapps/compatdata")) else { continue };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n == "0" || !e.path().is_dir() {
                    continue;
                }
                prefixes.push((e.path(), titles.get(format!("steam:{n}").as_str()).map(|t| t.to_string())));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (p, title) in prefixes {
        let c = fs::canonicalize(&p).unwrap_or(p.clone());
        if !seen.insert(c.clone()) || !c.is_dir() {
            continue;
        }
        let b = cache.size(&c, fresh);
        if b < 1_000_000 {
            continue;
        }
        let orphan = title.is_none();
        let title = match title {
            Some(t) => format!("{t}: saves and Windows files"),
            None => match c.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| n.chars().all(|ch| ch.is_ascii_digit())) {
                Some(appid) => format!("Leftover saves, Steam game {appid}"),
                None => format!("Leftover saves, {}", c.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            },
        };
        let key = if orphan { format!("orphan:{}", c.display()) } else { format!("prefix:{}", c.display()) };
        out.push(Item { key, title, kind: ItemKind::Prefix, source: if orphan { "Left over".into() } else { "Wine".into() }, game_id: None, path: Some(c), bytes: b, no_move: Some(if orphan { "It belongs to no game now. Delete it or leave it." } else { "Moves with its game." }.into()) });
    }
    for (key, title, dir) in cache_dirs() {
        if !dir.is_dir() {
            continue;
        }
        let b = cache.size(&dir, true);
        out.push(Item { key: format!("cache:{key}"), title: title.into(), kind: ItemKind::Cache, source: "Rare".into(), game_id: None, path: Some(dir), bytes: b, no_move: Some("The cache stays on this device.".into()) });
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    out
}

/// Where Wine prefixes live: Rare's own, Heroic's default, and every Steam library's compatdata.
/// A prefix is only ever deleted when it is one folder directly inside one of these.
pub fn prefix_roots() -> Vec<PathBuf> {
    let mut v = vec![data().join("rare/prefixes")];
    for c in crate::heroic::config_dirs() {
        let Some(j) = fs::read_to_string(c.join("heroic/config.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
        if let Some(p) = j.pointer("/defaultSettings/defaultWinePrefix").and_then(|v| v.as_str()).filter(|p| !p.is_empty()) {
            v.push(PathBuf::from(p.replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy())));
        }
    }
    for root in crate::steam::find_roots().iter().filter(|r| r.primary) {
        for l in crate::steam::registered_libraries(&root.path) {
            v.push(l.join("steamapps/compatdata"));
        }
    }
    v.into_iter().filter_map(|p| fs::canonicalize(&p).ok()).collect()
}

/// `p` when it is safe to delete as a leftover prefix: absolute, no `.` or `..`, not a link, and
/// exactly one folder inside a prefix root (never a root itself or anything deeper).
pub fn prefix_guard(p: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    use std::path::Component;
    let shown = p.display();
    if !p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{shown} isn't a prefix folder."));
    }
    let m = fs::symlink_metadata(p).map_err(|_| format!("{shown} isn't there anymore."))?;
    if m.file_type().is_symlink() || !m.is_dir() {
        return Err(format!("{shown} is a link, not a folder. Remove it in Desktop mode."));
    }
    let c = fs::canonicalize(p).map_err(|e| format!("Couldn't read {shown}: {e}"))?;
    match c.parent() {
        Some(parent) if roots.iter().any(|r| r == parent) => Ok(c),
        _ => Err(format!("Rare only deletes prefixes in Steam's, Heroic's or its own prefix folders, and {shown} isn't one.")),
    }
}

/// Deletes a prefix left behind by a removed game (the row's `path`). Returns the bytes freed.
pub fn delete_prefix(p: &Path) -> Result<u64, String> {
    let c = prefix_guard(p, &prefix_roots())?;
    let b = du(&c);
    fs::remove_dir_all(&c).map_err(|e| format!("Couldn't delete {}: {e}", c.display()))?;
    Ok(b)
}

/// Rare's caches that can be cleared: they fill again as they are needed.
pub fn cache_dirs() -> Vec<(&'static str, &'static str, PathBuf)> {
    vec![("img", "Image cache", rare_core::paths::cache_dir().join("img"))]
}

/// Empties a cache folder: every file goes, the folders stay (the image loader keeps writing
/// into them). Only the folders `cache_dirs` names. Returns the bytes freed.
pub fn clear_cache(key: &str) -> Result<u64, String> {
    let (_, _, dir) = cache_dirs().into_iter().find(|(k, _, _)| *k == key).ok_or("Unknown cache")?;
    let before = du(&dir);
    fn files(p: &Path) -> Result<(), String> {
        let rd = fs::read_dir(p).map_err(|e| format!("Couldn't open {}: {e}", p.display()))?;
        for e in rd.flatten() {
            let q = e.path();
            match fs::symlink_metadata(&q) {
                Ok(m) if m.is_dir() => files(&q)?,
                Ok(_) => fs::remove_file(&q).map_err(|e| format!("Couldn't clear {}: {e}", q.display()))?,
                Err(_) => {}
            }
        }
        Ok(())
    }
    files(&dir)?;
    Ok(before.saturating_sub(du(&dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTS: &str = "composefs / overlay ro 0 0\n/dev/nvme0n1p3 /var btrfs rw 0 0\n/dev/nvme0n1p3 /var/home btrfs rw 0 0\n/dev/nvme0n1p2 /boot ext4 rw 0 0\n/dev/mmcblk0p1 /run/media/bazzite/My\\040Card ext4 rw 0 0\n/dev/sda1 /run/media/bazzite/STICK exfat rw 0 0\n/dev/sdb1 /srv ext4 rw 0 0\n";

    #[test]
    fn drives_from_mounts() {
        let ms = parse_mounts(MOUNTS);
        assert_eq!(ms[4].path, PathBuf::from("/run/media/bazzite/My Card"));
        let d = drives_from(&ms, Path::new("/var/home/bazzite"), &|p| Some((100, if p.starts_with("/var") { 60 } else { 10 })));
        let kinds: Vec<_> = d.iter().map(|d| (d.kind, d.label.as_str(), d.mount.to_string_lossy().into_owned())).collect();
        assert_eq!(
            kinds,
            vec![
                (DriveKind::Internal, "This device", "/var/home".to_string()),
                (DriveKind::Sd, "SD card", "/run/media/bazzite/My Card".to_string()),
                (DriveKind::Usb, "STICK", "/run/media/bazzite/STICK".to_string()),
            ]
        );
        assert_eq!(d[0].used(), 40);
    }

    #[test]
    fn sd_card_states() {
        let t = tempfile::tempdir().unwrap();
        let ms = parse_mounts(MOUNTS);
        assert_eq!(sd_card_in(t.path(), &ms), SdCard::None);
        fs::create_dir_all(t.path().join("mmcblk0")).unwrap();
        fs::write(t.path().join("mmcblk0/size"), "0\n").unwrap();
        assert_eq!(sd_card_in(t.path(), &ms), SdCard::None);
        fs::write(t.path().join("mmcblk0/size"), "1000\n").unwrap();
        assert!(matches!(sd_card_in(t.path(), &ms), SdCard::Ready { .. }));
        assert_eq!(sd_card_in(t.path(), &[]), SdCard::NotReady { device: "/dev/mmcblk0".into(), size: 512_000 });
    }

    #[test]
    fn flatpak_sizes() {
        assert_eq!(parse_size("286.1 MB"), Some(286_100_000));
        assert_eq!(parse_size("1.4\u{a0}GB"), Some(1_400_000_000));
        assert_eq!(parse_size("38.0 kB"), Some(38_000));
        let l = parse_flatpak_list("app.xemu.xemu\txemu\t38.0 MB\tuser\ncom.discordapp.Discord\tDiscord\t286.1 MB\tsystem\n");
        assert_eq!(l.len(), 2);
        assert!(l[0].user && !l[1].user);
        assert_eq!(l[1].bytes, 286_100_000);
    }

    #[test]
    fn prefix_guard_takes_one_folder_inside_a_root_only() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("compatdata");
        fs::create_dir_all(root.join("620/pfx/drive_c")).unwrap();
        let roots = vec![fs::canonicalize(&root).unwrap()];
        let r = fs::canonicalize(&root).unwrap();
        assert!(prefix_guard(&r.join("620"), &roots).is_ok());
        assert!(prefix_guard(&r, &roots).is_err(), "never the root");
        assert!(prefix_guard(&r.join("620/pfx"), &roots).is_err(), "never deeper");
        assert!(prefix_guard(&r.join("620/../620"), &roots).is_err());
        assert!(prefix_guard(Path::new("relative/620"), &roots).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(t.path(), r.join("link")).unwrap();
            assert!(prefix_guard(&r.join("link"), &roots).is_err(), "never a link");
        }
        let elsewhere = t.path().join("other/1");
        fs::create_dir_all(&elsewhere).unwrap();
        assert!(prefix_guard(&fs::canonicalize(&elsewhere).unwrap(), &roots).is_err());
    }

    #[test]
    fn du_counts_files() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a/b")).unwrap();
        fs::write(t.path().join("a/b/f"), vec![1u8; 10_000]).unwrap();
        assert!(du(t.path()) >= 10_000);
    }
}
