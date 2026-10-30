//! Steam's app info cache (appcache/appinfo.vdf, binary KeyValues): names, types, install folders
//! and depot sizes for the apps the client knows, offline. Read for the owned-but-not-installed
//! games (their names and install sizes). Formats 0x27/0x28 (keys inline) and 0x29 (keys in a
//! string table at the end of the file).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

/// What the library needs from one app's entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppMeta {
    pub appid: u32,
    pub name: String,
    /// common/type, lowercase ("game", "tool", "dlc", "music", "demo", ...).
    pub kind: String,
    pub installdir: String,
    /// Bytes on disk once installed (the public branch's depots for this OS), 0 when unknown.
    pub size: u64,
    /// Bytes to download for that, 0 when unknown.
    pub download: u64,
    /// The licence agreements Steam asks the player to accept on install (common/eulas).
    pub eulas: Vec<Eula>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Eula {
    pub id: String,
    pub name: String,
    pub url: String,
    pub version: u32,
}

/// A binary KeyValues value (only the kinds appinfo uses).
#[derive(Clone, Debug)]
enum Kv {
    Map(Vec<(String, Kv)>),
    Str(String),
    Int(i64),
}

impl Kv {
    fn get(&self, k: &str) -> Option<&Kv> {
        match self {
            Kv::Map(v) => v.iter().find(|(x, _)| x.eq_ignore_ascii_case(k)).map(|(_, v)| v),
            _ => None,
        }
    }
    fn str(&self, k: &str) -> Option<String> {
        match self.get(k)? {
            Kv::Str(s) => Some(s.clone()),
            Kv::Int(i) => Some(i.to_string()),
            Kv::Map(_) => None,
        }
    }
    fn num(&self, k: &str) -> Option<u64> {
        match self.get(k)? {
            Kv::Str(s) => s.trim().parse().ok(),
            Kv::Int(i) => u64::try_from(*i).ok(),
            Kv::Map(_) => None,
        }
    }
    fn entries(&self) -> &[(String, Kv)] {
        match self {
            Kv::Map(v) => v,
            _ => &[],
        }
    }
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
    strings: Option<&'a [String]>,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
    }
    fn cstr(&mut self) -> Option<String> {
        let rest = self.b.get(self.i..)?;
        let n = rest.iter().position(|c| *c == 0)?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.i += n + 1;
        Some(s)
    }
    fn key(&mut self) -> Option<String> {
        match self.strings {
            Some(t) => t.get(self.u32()? as usize).cloned(),
            None => self.cstr(),
        }
    }
    fn map(&mut self, depth: u32) -> Option<Kv> {
        if depth > 32 {
            return None;
        }
        let mut out = Vec::new();
        loop {
            let t = self.u8()?;
            if t == 8 || t == 11 {
                return Some(Kv::Map(out));
            }
            let k = self.key()?;
            let v = match t {
                0 => self.map(depth + 1)?,
                1 => Kv::Str(self.cstr()?),
                2 => Kv::Int(self.u32()? as i32 as i64),
                3 => {
                    self.take(4)?;
                    Kv::Int(0)
                }
                7 | 10 => Kv::Int(self.u64()? as i64),
                _ => return None,
            };
            out.push((k, v));
        }
    }
}

/// Reads the string table of a 0x29 file.
fn string_table(b: &[u8], off: usize) -> Option<Vec<String>> {
    let mut r = Rd { b, i: off, strings: None };
    let n = r.u32()? as usize;
    let mut out = Vec::with_capacity(n.min(1 << 20));
    for _ in 0..n {
        out.push(r.cstr()?);
    }
    Some(out)
}

/// The depots Steam installs for the public branch on this OS: no redistributables shared from
/// other apps, no DLC depots, English or language-free, and the OS the game runs as (Linux when
/// it ships for Linux, else Windows under Proton).
fn sizes(info: &Kv) -> (u64, u64) {
    let oslist = info.get("common").and_then(|c| c.str("oslist")).unwrap_or_default().to_lowercase();
    let os = if oslist.contains("linux") { "linux" } else { "windows" };
    let Some(depots) = info.get("depots") else { return (0, 0) };
    let (mut size, mut dl) = (0u64, 0u64);
    for (k, d) in depots.entries() {
        if k.parse::<u32>().is_err() || !matches!(d, Kv::Map(_)) {
            continue;
        }
        if d.get("sharedinstall").is_some() || d.get("depotfromapp").is_some() || d.get("dlcappid").is_some() {
            continue;
        }
        if let Some(c) = d.get("config") {
            if let Some(l) = c.str("oslist").map(|s| s.to_lowercase()) {
                if !l.is_empty() && !l.split(',').any(|x| x.trim() == os) {
                    continue;
                }
            }
            if let Some(l) = c.str("language") {
                if !l.is_empty() && l != "english" {
                    continue;
                }
            }
            if c.str("lowviolence").as_deref() == Some("1") {
                continue;
            }
        }
        if let Some(p) = d.get("manifests").and_then(|m| m.get("public")) {
            size += p.num("size").unwrap_or(0);
            dl += p.num("download").unwrap_or(0);
        }
    }
    (size, dl)
}

fn meta(appid: u32, root: &Kv) -> Option<AppMeta> {
    let info = root.get("appinfo")?;
    let common = info.get("common")?;
    let (size, download) = sizes(info);
    Some(AppMeta {
        appid,
        name: common.str("name").unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" "),
        kind: common.str("type").unwrap_or_default().to_lowercase(),
        installdir: info.get("config").and_then(|c| c.str("installdir")).unwrap_or_default(),
        size,
        download,
        eulas: common
            .get("eulas")
            .map(|e| {
                e.entries()
                    .iter()
                    .filter_map(|(_, x)| Some(Eula { id: x.str("id")?, name: x.str("name").unwrap_or_default(), url: x.str("url").unwrap_or_default().replace("//eula", "/eula"), version: x.num("version").unwrap_or(0) as u32 }))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Parses the file's bytes, decoding only the apps in `want` (None: all).
pub fn parse(b: &[u8], want: Option<&HashSet<u32>>) -> HashMap<u32, AppMeta> {
    let mut out = HashMap::new();
    let mut r = Rd { b, i: 0, strings: None };
    let Some(magic) = r.u32() else { return out };
    let _universe = r.u32();
    let table;
    let v29 = magic == 0x0756_4429;
    if v29 {
        let Some(off) = r.u64() else { return out };
        table = string_table(b, off as usize);
        if table.is_none() {
            return out;
        }
    } else if magic == 0x0756_4428 || magic == 0x0756_4427 {
        table = None;
    } else {
        return out;
    }
    // Per entry: appid, size, then (in `size` bytes) infostate, last updated, pics token, sha1,
    // change number, (0x28+) binary sha1, then the KeyValues.
    let head: usize = match magic {
        0x0756_4427 => 4 + 4 + 8 + 20 + 4,
        _ => 4 + 4 + 8 + 20 + 4 + 20,
    };
    loop {
        let Some(appid) = r.u32() else { break };
        if appid == 0 {
            break;
        }
        let Some(size) = r.u32() else { break };
        let start = r.i;
        let end = start + size as usize;
        if end > b.len() {
            break;
        }
        if want.map_or(true, |w| w.contains(&appid)) {
            let mut kv = Rd { b: &b[..end], i: start + head, strings: table.as_deref() };
            if let Some(m) = kv.map(0).and_then(|root| meta(appid, &root)) {
                out.insert(appid, m);
            }
        }
        r.i = end;
    }
    out
}

/// The last read, kept while the file and the wanted set stay the same (the file can be tens of
/// MB on a big account and the library rescans often).
static CACHE: Mutex<Option<(SystemTime, u64, HashMap<u32, AppMeta>)>> = Mutex::new(None);

/// The apps in `want` from `<steam root>/appcache/appinfo.vdf`.
pub fn read(root: &Path, want: &HashSet<u32>) -> HashMap<u32, AppMeta> {
    let file = root.join("appcache/appinfo.vdf");
    let Ok(mtime) = fs::metadata(&file).and_then(|m| m.modified()) else { return HashMap::new() };
    let mut ids: Vec<u32> = want.iter().copied().collect();
    ids.sort_unstable();
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&file, &ids).hash(&mut h);
        h.finish()
    };
    if let Some((t, k, m)) = CACHE.lock().unwrap().as_ref() {
        if *t == mtime && *k == key {
            return m.clone();
        }
    }
    let Ok(bytes) = fs::read(&file) else { return HashMap::new() };
    let m = parse(&bytes, Some(want));
    *CACHE.lock().unwrap() = Some((mtime, key, m.clone()));
    m
}

/// The apps the account's licences cover, from `appcache/packageinfo.vdf`: Steam caches the
/// packages it holds a licence for there (package 0, the free-for-everyone package, is left out).
/// Unlock-only DLC (no files of its own) shows up here and nowhere else on disk.
pub fn parse_packages(b: &[u8]) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut r = Rd { b, i: 0, strings: None };
    let Some(magic) = r.u32() else { return out };
    if magic != 0x0656_5527 && magic != 0x0656_5528 {
        return out;
    }
    let _universe = r.u32();
    loop {
        let Some(pid) = r.u32() else { break };
        if pid == 0xFFFF_FFFF {
            break;
        }
        // sha1, change number, (0x28) pics token, then the KeyValues.
        if r.take(20 + 4).is_none() || (magic == 0x0656_5528 && r.take(8).is_none()) {
            break;
        }
        let Some(kv) = r.map(0) else { break };
        if pid == 0 {
            continue;
        }
        for (_, p) in kv.entries() {
            if let Some(apps) = p.get("appids") {
                for (_, a) in apps.entries() {
                    if let Kv::Int(id) = a {
                        if let Ok(id) = u32::try_from(*id) {
                            out.insert(id);
                        }
                    }
                }
            }
        }
    }
    out
}

static PKG_CACHE: Mutex<Option<(std::path::PathBuf, SystemTime, HashSet<u32>)>> = Mutex::new(None);

/// [`parse_packages`] of `<steam root>/appcache/packageinfo.vdf`, kept while the file is unchanged.
pub fn licensed(root: &Path) -> HashSet<u32> {
    let file = root.join("appcache/packageinfo.vdf");
    let Ok(mtime) = fs::metadata(&file).and_then(|m| m.modified()) else { return HashSet::new() };
    if let Some((f, t, s)) = PKG_CACHE.lock().unwrap().as_ref() {
        if *f == file && *t == mtime {
            return s.clone();
        }
    }
    let Ok(bytes) = fs::read(&file) else { return HashSet::new() };
    let s = parse_packages(&bytes);
    *PKG_CACHE.lock().unwrap() = Some((file, mtime, s.clone()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a 0x29 file with one app.
    fn file(appid: u32) -> Vec<u8> {
        let strings = ["appinfo", "common", "name", "type", "oslist", "config", "installdir", "depots", "manifests", "public", "size", "download", "sharedinstall", "language", "101", "102", "103", "104"];
        let k = |s: &str| (strings.iter().position(|x| *x == s).unwrap() as u32).to_le_bytes();
        let mut kv = Vec::new();
        let open = |kv: &mut Vec<u8>, name: &str| {
            kv.push(0);
            kv.extend(k(name));
        };
        let str_ = |kv: &mut Vec<u8>, name: &str, v: &str| {
            kv.push(1);
            kv.extend(k(name));
            kv.extend(v.as_bytes());
            kv.push(0);
        };
        open(&mut kv, "appinfo");
        open(&mut kv, "common");
        str_(&mut kv, "name", "Game  Two");
        str_(&mut kv, "type", "Game");
        str_(&mut kv, "oslist", "windows");
        kv.push(8);
        open(&mut kv, "config");
        str_(&mut kv, "installdir", "GameTwo");
        kv.push(8);
        open(&mut kv, "depots");
        // A Windows depot, an English one, a redistributable and a French one.
        for (id, size, extra) in [("101", "1000", None), ("102", "50", Some(("language", "english"))), ("103", "7", Some(("sharedinstall", "1"))), ("104", "9", Some(("language", "french")))] {
            open(&mut kv, id);
            if let Some((ek, ev)) = extra {
                if ek == "sharedinstall" {
                    str_(&mut kv, "sharedinstall", ev);
                } else {
                    open(&mut kv, "config");
                    str_(&mut kv, "language", ev);
                    kv.push(8);
                }
            }
            open(&mut kv, "manifests");
            open(&mut kv, "public");
            str_(&mut kv, "size", size);
            str_(&mut kv, "download", "1");
            kv.extend([8, 8, 8]);
        }
        kv.extend([8, 8, 8]);
        let mut entry = vec![0u8; 4 + 4 + 8 + 20 + 4 + 20];
        entry.extend(&kv);
        let mut b = Vec::new();
        b.extend(0x0756_4429u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(appid.to_le_bytes());
        b.extend((entry.len() as u32).to_le_bytes());
        b.extend(entry);
        b.extend(0u32.to_le_bytes());
        let off = b.len() as u64;
        b[8..16].copy_from_slice(&off.to_le_bytes());
        b.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            b.extend(s.as_bytes());
            b.push(0);
        }
        b
    }

    #[test]
    fn reads_name_type_dir_and_install_size() {
        let b = file(4242);
        let m = parse(&b, None);
        let a = &m[&4242];
        assert_eq!((a.name.as_str(), a.kind.as_str(), a.installdir.as_str()), ("Game Two", "game", "GameTwo"));
        // The Windows depot and the English one; not the redistributable or the French one.
        assert_eq!((a.size, a.download), (1050, 2));
        assert!(parse(&b, Some(&HashSet::from([1]))).is_empty());
        assert!(parse(b"junk", None).is_empty());
    }

    /// RARE_APPINFO=<appinfo.vdf> RARE_APPIDS=1,2 cargo test -p rare-library appinfo -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_file() {
        let Ok(p) = std::env::var("RARE_APPINFO") else { return };
        let ids: HashSet<u32> = std::env::var("RARE_APPIDS").unwrap_or_default().split(',').filter_map(|x| x.parse().ok()).collect();
        let m = parse(&fs::read(p).unwrap(), Some(&ids));
        let mut v: Vec<_> = m.values().filter(|a| a.kind == "game").collect();
        v.sort_by_key(|a| a.appid);
        for a in v {
            println!("{} {} {} size={} dl={}", a.appid, a.name, a.installdir, a.size, a.download);
        }
    }

    #[test]
    fn packages_list_licensed_apps() {
        // magic 0x28, universe 1, package 7 with apps 10 and 20, package 0 (skipped), end.
        let mut b = vec![];
        b.extend(0x0656_5528u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        for (pid, apps) in [(7u32, vec![10u32, 20]), (0, vec![30])] {
            b.extend(pid.to_le_bytes());
            b.extend([0u8; 20 + 4 + 8]);
            b.push(0);
            b.extend(format!("{pid}\0").as_bytes());
            b.push(0);
            b.extend(b"appids\0");
            for (i, a) in apps.iter().enumerate() {
                b.push(2);
                b.extend(format!("{i}\0").as_bytes());
                b.extend(a.to_le_bytes());
            }
            b.extend([8, 8, 8]);
        }
        b.extend(0xFFFF_FFFFu32.to_le_bytes());
        let s = parse_packages(&b);
        assert!(s.contains(&10) && s.contains(&20) && !s.contains(&30));
    }

    /// RARE_PKGINFO=<packageinfo.vdf> cargo test -- --ignored packages_from_file
    #[test]
    #[ignore]
    fn packages_from_file() {
        let b = fs::read(std::env::var("RARE_PKGINFO").unwrap()).unwrap();
        let mut v: Vec<u32> = parse_packages(&b).into_iter().collect();
        v.sort_unstable();
        println!("{v:?}");
        assert!(!v.is_empty());
    }
}
