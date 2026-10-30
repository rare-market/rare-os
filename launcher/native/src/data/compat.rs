//! How well a Steam game runs on a handheld: Steam Deck Verified from Steam's own app cache
//! (appcache/appinfo.vdf, `common/steam_deck_compatibility/category`), and the ProtonDB tier from
//! its public summary API, kept in ~/.cache/rare/protondb.json for a few days. Blocking: call
//! from a worker.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub struct Compat {
    /// Steam Deck: 1 unsupported, 2 playable, 3 verified (None: unknown or not tested).
    pub deck: Option<u8>,
    /// ProtonDB: "platinum", "gold", "silver", "bronze", "borked".
    pub protondb: Option<String>,
}

impl Compat {
    /// "Steam Deck Verified", None when unknown.
    pub fn deck_text(&self) -> Option<&'static str> {
        match self.deck? {
            3 => Some("Steam Deck Verified"),
            2 => Some("Steam Deck Playable"),
            1 => Some("Not supported on Steam Deck"),
            _ => None,
        }
    }

    /// "ProtonDB Gold", None when unknown.
    pub fn protondb_text(&self) -> Option<String> {
        let t = self.protondb.as_deref()?;
        let name = match t {
            "platinum" => "Platinum",
            "gold" => "Gold",
            "silver" => "Silver",
            "bronze" => "Bronze",
            "borked" => "Borked",
            _ => return None,
        };
        Some(format!("ProtonDB {name}"))
    }
}

pub fn lookup(appid: u64) -> Compat {
    let deck = rare_launch::steamcfg::primary_root().and_then(|r| deck_category(&r.join("appcache/appinfo.vdf"), appid));
    Compat { deck, protondb: protondb(appid) }
}

// ---- appinfo.vdf -------------------------------------------------------------------------------

/// Read per app once a run (the file only changes when Steam refreshes its app info).
static DECK: Mutex<Option<HashMap<u64, Option<u8>>>> = Mutex::new(None);

fn deck_category(path: &Path, appid: u64) -> Option<u8> {
    if let Some(v) = DECK.lock().ok()?.as_ref().and_then(|m| m.get(&appid).copied()) {
        return v;
    }
    let v = read_appinfo(path, appid).ok().flatten().and_then(|kv| kv.find(&["appinfo", "common", "steam_deck_compatibility", "category"]).and_then(Kv::int)).and_then(|c| u8::try_from(c).ok());
    DECK.lock().ok()?.get_or_insert_with(HashMap::new).insert(appid, v);
    v
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    Map(Vec<(String, Kv)>),
    Str(String),
    Int(i64),
    Float(f32),
}

impl Kv {
    fn get(&self, k: &str) -> Option<&Kv> {
        match self {
            Kv::Map(v) => v.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, x)| x),
            _ => None,
        }
    }
    pub fn find(&self, path: &[&str]) -> Option<&Kv> {
        path.iter().try_fold(self, |kv, k| kv.get(k))
    }
    fn int(&self) -> Option<i64> {
        match self {
            Kv::Int(i) => Some(*i),
            Kv::Str(s) => s.parse().ok(),
            _ => None,
        }
    }
}

fn u32le(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn cstr(b: &[u8], at: &mut usize) -> Option<String> {
    let end = b[*at..].iter().position(|c| *c == 0)? + *at;
    let s = String::from_utf8_lossy(&b[*at..end]).into_owned();
    *at = end + 1;
    Some(s)
}

/// Steam's binary KeyValues (one app's blob). `strings`: v29's key table.
fn parse_kv(b: &[u8], at: &mut usize, strings: Option<&[String]>, depth: u32) -> Option<Kv> {
    if depth > 32 {
        return None;
    }
    let mut out = Vec::new();
    loop {
        let t = *b.get(*at)?;
        *at += 1;
        if t == 0x08 || t == 0x0b {
            return Some(Kv::Map(out));
        }
        let key = match strings {
            Some(tab) => {
                let i = u32::from_le_bytes(b.get(*at..*at + 4)?.try_into().ok()?) as usize;
                *at += 4;
                tab.get(i)?.clone()
            }
            None => cstr(b, at)?,
        };
        let n = |at: &mut usize, len: usize| -> Option<&[u8]> {
            let s = b.get(*at..*at + len)?;
            *at += len;
            Some(s)
        };
        let v = match t {
            0x00 => parse_kv(b, at, strings, depth + 1)?,
            0x01 => Kv::Str(cstr(b, at)?),
            0x02 | 0x04 | 0x06 => Kv::Int(i32::from_le_bytes(n(at, 4)?.try_into().ok()?) as i64),
            0x03 => Kv::Float(f32::from_le_bytes(n(at, 4)?.try_into().ok()?)),
            0x07 => Kv::Int(u64::from_le_bytes(n(at, 8)?.try_into().ok()?) as i64),
            0x0a => Kv::Int(i64::from_le_bytes(n(at, 8)?.try_into().ok()?)),
            _ => return None,
        };
        out.push((key, v));
    }
}

/// One app's KeyValues from appinfo.vdf (v27, v28, v29), skipping the others by their size.
pub fn read_appinfo(path: &Path, appid: u64) -> std::io::Result<Option<Kv>> {
    let mut f = BufReader::new(std::fs::File::open(path)?);
    let magic = u32le(&mut f)?;
    let _universe = u32le(&mut f)?;
    let (head, strings) = match magic {
        0x0756_4427 => (40usize, None),
        0x0756_4428 => (60, None),
        0x0756_4429 => {
            let mut b = [0u8; 8];
            f.read_exact(&mut b)?;
            let off = i64::from_le_bytes(b) as u64;
            let here = f.stream_position()?;
            f.seek(SeekFrom::Start(off))?;
            let count = u32le(&mut f)? as usize;
            let mut rest = Vec::new();
            f.read_to_end(&mut rest)?;
            let mut at = 0;
            let mut tab = Vec::with_capacity(count.min(1 << 20));
            for _ in 0..count {
                match cstr(&rest, &mut at) {
                    Some(s) => tab.push(s),
                    None => break,
                }
            }
            f.seek(SeekFrom::Start(here))?;
            (60, Some(tab))
        }
        _ => return Ok(None),
    };
    loop {
        let id = u32le(&mut f)? as u64;
        if id == 0 {
            return Ok(None);
        }
        let size = u32le(&mut f)? as usize;
        if id != appid {
            f.seek_relative(size as i64)?;
            continue;
        }
        let mut b = vec![0u8; size];
        f.read_exact(&mut b)?;
        let mut at = head.min(size);
        return Ok(parse_kv(&b, &mut at, strings.as_deref(), 0));
    }
}

// ---- ProtonDB ----------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Clone)]
struct Cached {
    tier: Option<String>,
    at: i64,
}

fn cache_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"));
    base.join("rare/protondb.json")
}

static PDB: Mutex<()> = Mutex::new(());

fn protondb(appid: u64) -> Option<String> {
    let _one = PDB.lock().ok()?;
    let path = cache_path();
    let mut cache: HashMap<String, Cached> = super::userlib::read_json(&path);
    let now = super::model::now_secs();
    let key = appid.to_string();
    if let Some(c) = cache.get(&key) {
        // Three days for a tier, one for "no reports yet".
        let ttl = if c.tier.is_some() { 3 * 86400 } else { 86400 };
        if now - c.at < ttl {
            return c.tier.clone();
        }
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(8))).http_status_as_error(false).build().into();
    let fetched = (|| -> Option<Option<String>> {
        let mut r = agent.get(&format!("https://www.protondb.com/api/v1/reports/summaries/{appid}.json")).header("User-Agent", "RareLauncher/0.1").call().ok()?;
        match r.status().as_u16() {
            200 => {
                let v: serde_json::Value = r.body_mut().read_json().ok()?;
                Some(v.get("tier").and_then(|t| t.as_str()).filter(|t| *t != "pending").map(String::from))
            }
            404 => Some(None),
            _ => None,
        }
    })();
    match fetched {
        Some(tier) => {
            cache.insert(key, Cached { tier: tier.clone(), at: now });
            super::userlib::write_json(&path, &cache);
            tier
        }
        // Offline: the last answer, however old.
        None => cache.get(&key).and_then(|c| c.tier.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv_bytes(strings: bool) -> Vec<u8> {
        // appinfo { common { name "X" steam_deck_compatibility { category 3 } } }
        let mut b = Vec::new();
        let key = |b: &mut Vec<u8>, s: &str, i: u32| {
            if strings {
                b.extend(i.to_le_bytes());
            } else {
                b.extend(s.as_bytes());
                b.push(0);
            }
        };
        b.push(0);
        key(&mut b, "appinfo", 0);
        b.push(0);
        key(&mut b, "common", 1);
        b.push(1);
        key(&mut b, "name", 2);
        b.extend(b"X\0");
        b.push(0);
        key(&mut b, "steam_deck_compatibility", 3);
        b.push(2);
        key(&mut b, "category", 4);
        b.extend(3i32.to_le_bytes());
        b.extend([8, 8, 8, 8]);
        b
    }

    fn file(v: u32) -> Vec<u8> {
        let strings = v == 0x29;
        let mut f = Vec::new();
        f.extend((0x0756_4400u32 + v).to_le_bytes());
        f.extend(1u32.to_le_bytes());
        let off_at = f.len();
        if strings {
            f.extend(0i64.to_le_bytes());
        }
        let head = if v == 0x27 { 40 } else { 60 };
        for (id, body) in [(10u32, vec![8u8]), (440, kv_bytes(strings))] {
            f.extend(id.to_le_bytes());
            f.extend(((head + body.len()) as u32).to_le_bytes());
            f.extend(vec![0u8; head]);
            f.extend(body);
        }
        f.extend(0u32.to_le_bytes());
        if strings {
            let off = f.len() as i64;
            f[off_at..off_at + 8].copy_from_slice(&off.to_le_bytes());
            let tab = ["appinfo", "common", "name", "steam_deck_compatibility", "category"];
            f.extend((tab.len() as u32).to_le_bytes());
            for s in tab {
                f.extend(s.as_bytes());
                f.push(0);
            }
        }
        f
    }

    #[test]
    fn deck_category_from_appinfo() {
        let dir = std::env::temp_dir().join(format!("rare-appinfo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for v in [0x27, 0x28, 0x29] {
            let p = dir.join(format!("appinfo-{v:x}.vdf"));
            std::fs::write(&p, file(v)).unwrap();
            let kv = read_appinfo(&p, 440).unwrap().unwrap();
            assert_eq!(kv.find(&["appinfo", "common", "steam_deck_compatibility", "category"]).and_then(Kv::int), Some(3), "v{v:x}");
            assert_eq!(kv.find(&["appinfo", "common", "name"]), Some(&Kv::Str("X".into())));
            assert!(read_appinfo(&p, 7).unwrap().is_none());
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Compat { deck: Some(3), protondb: Some("gold".into()) }.protondb_text().as_deref(), Some("ProtonDB Gold"));
    }
}
