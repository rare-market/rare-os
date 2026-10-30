//! Steam Cloud status for one Steam game, read from Steam's own local files. Read-only: nothing
//! under the Steam directory is ever written.
//!
//! Files used (account = the most recent login, `MostRecent` in config/loginusers.vdf, else the
//! newest userdata folder):
//!   userdata/<account>/<appid>/remotecache.vdf      one block per cloud file
//!   userdata/<account>/7/remote/sharedconfig.vdf    apps/<appid>/cloudenabled (per game switch)
//!   userdata/<account>/config/localconfig.vdf       Software/Valve/Steam/CloudEnabled (global switch)
//!
//! remotecache.vdf per-file fields: `size`, `localtime` (mtime of the local file as last seen),
//! `time` (mtime when Steam last synced it), `remotetime` (mtime of the cloud copy), `syncstate`,
//! `persiststate`. Steam does not document `syncstate`; what is known from the client's files:
//!   1  synchronized, local and cloud copies match
//!   anything else (0, 2, 3, 4 ...) is a pending change in some direction: a local edit waiting
//!   to upload, a cloud copy waiting to download, or a deletion not yet pushed.
//! Only 1 counts as synced; every other value reads as OutOfDate, so an unknown value never
//! claims "Synced". Steam does not store a conflict flag (the conflict dialog is transient), so
//! Conflict is inferred: a file changed on this device since the last sync (`localtime > time`)
//! and also changed in the cloud since then (`remotetime > time`) with different times.

use crate::steam::SteamRoot;
use crate::vdf::{self, Vdf};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudStatus {
    /// Every cloud file matches the cloud.
    Synced,
    /// Local changes not uploaded yet, or the cloud is newer.
    OutOfDate,
    /// Changed both here and in the cloud.
    Conflict,
    /// Steam Cloud is switched off, globally or for this game.
    Off,
    /// The game has no cloud files (show nothing).
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudInfo {
    pub status: CloudStatus,
    /// Newest sync time among the files (unix seconds).
    pub synced_at: Option<i64>,
    pub files: u32,
    pub bytes: u64,
}

impl CloudInfo {
    fn bare(status: CloudStatus) -> CloudInfo {
        CloudInfo { status, synced_at: None, files: 0, bytes: 0 }
    }
}

/// One game's cloud status.
pub fn status(steam_root: &Path, appid: u32) -> CloudStatus {
    info(steam_root, appid).status
}

pub fn info(steam_root: &Path, appid: u32) -> CloudInfo {
    let Some(acc) = account_dir(steam_root) else { return CloudInfo::bare(CloudStatus::Unsupported) };
    info_in(&acc, appid)
}

fn info_in(acc: &Path, appid: u32) -> CloudInfo {
    let Ok(text) = fs::read_to_string(acc.join(appid.to_string()).join("remotecache.vdf")) else {
        return CloudInfo::bare(CloudStatus::Unsupported);
    };
    let mut i = parse_cache(&text);
    if i.files == 0 {
        return CloudInfo::bare(CloudStatus::Unsupported);
    }
    if !global_enabled_in(acc) || !app_enabled_in(acc, appid) {
        i.status = CloudStatus::Off;
    }
    i
}

/// The cloud switch in Steam's settings (Settings > Cloud). Missing means on.
pub fn global_enabled(steam_root: &Path) -> bool {
    account_dir(steam_root).map(|a| global_enabled_in(&a)).unwrap_or(true)
}

fn global_enabled_in(acc: &Path) -> bool {
    let Ok(text) = fs::read_to_string(acc.join("config/localconfig.vdf")) else { return true };
    let v = vdf::parse(&text);
    let steam = v.get("UserLocalConfigStore").and_then(|s| s.get("Software")).and_then(|s| s.get("Valve")).and_then(|s| s.get("Steam"));
    steam.and_then(|s| s.str("CloudEnabled")).map(|s| s != "0").unwrap_or(true)
}

fn app_enabled_in(acc: &Path, appid: u32) -> bool {
    let Ok(text) = fs::read_to_string(acc.join("7/remote/sharedconfig.vdf")) else { return true };
    let v = vdf::parse(&text);
    v.entries()
        .first()
        .and_then(|(_, root)| root.get("apps"))
        .and_then(|a| a.get(&appid.to_string()))
        .and_then(|a| a.str("cloudenabled"))
        .map(|s| s != "0")
        .unwrap_or(true)
}

fn parse_cache(text: &str) -> CloudInfo {
    let v = vdf::parse(text);
    let Some((_, app)) = v.entries().first() else { return CloudInfo::bare(CloudStatus::Unsupported) };
    let mut info = CloudInfo { status: CloudStatus::Synced, synced_at: None, files: 0, bytes: 0 };
    let n = |f: &Vdf, k: &str| f.str(k).and_then(|s| s.trim().parse::<i64>().ok());
    for (_, f) in app.entries() {
        // File blocks are objects; plain values (ChangeNumber, OSType) are skipped.
        if !matches!(f, Vdf::Obj(_)) {
            continue;
        }
        info.files += 1;
        info.bytes += n(f, "size").unwrap_or(0).max(0) as u64;
        let (local, time, remote) = (n(f, "localtime").unwrap_or(0), n(f, "time").unwrap_or(0), n(f, "remotetime").unwrap_or(0));
        if time > 0 {
            info.synced_at = Some(info.synced_at.map_or(time, |t| t.max(time)));
        }
        let state = n(f, "syncstate");
        if local > time && remote > time && local != remote {
            info.status = CloudStatus::Conflict;
        } else if info.status == CloudStatus::Synced && (state != Some(1) || local > time.max(remote) || remote > time.max(local)) {
            info.status = CloudStatus::OutOfDate;
        }
    }
    info
}

/// userdata/<account> for the most recent login.
fn account_dir(root: &Path) -> Option<PathBuf> {
    let ud = root.join("userdata");
    if let Ok(text) = fs::read_to_string(root.join("config/loginusers.vdf")) {
        let v = vdf::parse(&text);
        if let Some(users) = v.get("users") {
            for (id64, u) in users.entries() {
                if u.str("MostRecent") == Some("1") {
                    if let Ok(id) = id64.parse::<u64>() {
                        let d = ud.join((id & 0xFFFF_FFFF).to_string());
                        if d.is_dir() {
                            return Some(d);
                        }
                    }
                }
            }
        }
    }
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in fs::read_dir(&ud).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.parse::<u64>().map_or(true, |n| n == 0) || !e.path().is_dir() {
            continue;
        }
        let m = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
        if best.as_ref().map_or(true, |(t, _)| m > *t) {
            best = Some((m, e.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// The first Steam install with cloud data for `appid`: its root.
pub fn root_for(roots: &[SteamRoot], appid: u32) -> Option<&Path> {
    roots.iter().map(|r| r.path.as_path()).find(|r| account_dir(r).is_some_and(|a| a.join(appid.to_string()).join("remotecache.vdf").is_file()))
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Summary {
    pub synced: u32,
    pub out_of_date: u32,
    pub conflict: u32,
    /// Cloud switched off globally.
    pub off: bool,
}

/// Every game with cloud files under the most recent account of the primary install.
pub fn summary(roots: &[SteamRoot]) -> Option<Summary> {
    let root = roots.iter().find(|r| r.primary).or(roots.first())?;
    let acc = account_dir(&root.path)?;
    let mut s = Summary { off: !global_enabled_in(&acc), ..Default::default() };
    for e in fs::read_dir(&acc).ok()?.flatten() {
        let Ok(appid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        if appid == 7 || !e.path().join("remotecache.vdf").is_file() {
            continue;
        }
        match info_in(&acc, appid).status {
            CloudStatus::Synced => s.synced += 1,
            CloudStatus::OutOfDate => s.out_of_date += 1,
            CloudStatus::Conflict => s.conflict += 1,
            _ => {}
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(a: &str) -> String {
        format!("\"440\"\n{{\n\t\"ChangeNumber\"\t\"5\"\n{a}\n}}\n")
    }
    fn file(name: &str, local: i64, time: i64, remote: i64, state: i64) -> String {
        format!("\t\"{name}\"\n\t{{\n\t\t\"root\"\t\"0\"\n\t\t\"size\"\t\"100\"\n\t\t\"localtime\"\t\"{local}\"\n\t\t\"time\"\t\"{time}\"\n\t\t\"remotetime\"\t\"{remote}\"\n\t\t\"syncstate\"\t\"{state}\"\n\t\t\"persiststate\"\t\"0\"\n\t}}\n")
    }

    #[test]
    fn synced_outdated_conflict() {
        let i = parse_cache(&cache(&(file("a", 10, 10, 10, 1) + &file("b", 20, 20, 20, 1))));
        assert_eq!((i.status, i.files, i.bytes, i.synced_at), (CloudStatus::Synced, 2, 200, Some(20)));
        assert_eq!(parse_cache(&cache(&file("a", 10, 10, 10, 4))).status, CloudStatus::OutOfDate);
        assert_eq!(parse_cache(&cache(&file("a", 30, 10, 10, 1))).status, CloudStatus::OutOfDate);
        assert_eq!(parse_cache(&cache(&file("a", 30, 10, 40, 1))).status, CloudStatus::Conflict);
        assert_eq!(parse_cache("").status, CloudStatus::Unsupported);
    }

    #[test]
    fn switches_and_account() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let acc = root.join("userdata/42");
        fs::create_dir_all(acc.join("440")).unwrap();
        fs::create_dir_all(acc.join("config")).unwrap();
        fs::create_dir_all(acc.join("7/remote")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(acc.join("440/remotecache.vdf"), cache(&file("a", 10, 10, 10, 1))).unwrap();
        fs::write(root.join("config/loginusers.vdf"), format!("\"users\"\n{{\n\"{}\"\n{{\n\"MostRecent\"\t\"1\"\n}}\n}}", 76561197960265728u64 + 42)).unwrap();
        assert_eq!(status(root, 440), CloudStatus::Synced);
        assert_eq!(status(root, 999), CloudStatus::Unsupported);
        fs::write(acc.join("7/remote/sharedconfig.vdf"), "\"UserRoamingConfigStore\"\n{\n\"Software\"\n{\n}\n\"apps\"\n{\n\"440\"\n{\n\"cloudenabled\"\t\"0\"\n}\n}\n}").unwrap();
        assert_eq!(status(root, 440), CloudStatus::Off);
        fs::remove_file(acc.join("7/remote/sharedconfig.vdf")).unwrap();
        fs::write(acc.join("config/localconfig.vdf"), "\"UserLocalConfigStore\"\n{\n\"Software\"\n{\n\"Valve\"\n{\n\"Steam\"\n{\n\"CloudEnabled\"\t\"0\"\n}\n}\n}\n}").unwrap();
        assert_eq!(status(root, 440), CloudStatus::Off);
        assert!(!global_enabled(root));
    }
}
