//! Which game a ROM is, exactly, and where its pictures are.
//!
//! In order: the ID or serial the game carries (a GameCube or Wii disc header, a PS1/PS2 disc's
//! SYSTEM.CNF, a PS3 or PSP disc's PARAM.SFO, a DS or 3DS header), then the dump catalogues
//! (Redump, No-Intro) by serial, by hash (cartridges up to 64 MB), by exact size, by exact name,
//! and only then a normalised-title match within the system, kept when it is confident enough.
//! The match gives the catalogue's canonical name (libretro-thumbnails' exact file name) and the
//! IDs GameTDB and xemu's database use. Results are cached per file (path, size, date) in
//! ~/.cache/rare/emu/ident.json. Pictures are links: the launcher loads them through rare.market's
//! image front, which caches them, so devices don't fetch from those sites.

use crate::dat::{self, Entry};
use crate::roms::Rom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Fuzzy matches below this are not used.
pub const MIN_CONFIDENCE: f32 = 0.8;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Ident {
    /// The catalogue's name ("Mortal Kombat - Deception (USA)").
    #[serde(default)]
    pub name: Option<String>,
    /// The game's own ID or serial as read from it ("GQNE5D", "SLUS21005", "BLUS30982").
    #[serde(default)]
    pub id: Option<String>,
    /// The catalogue's serial ("DL-DOL-GQNE-USA", "TQ-031").
    #[serde(default)]
    pub serial: Option<String>,
    /// GameTDB's Switch product ID ("ACBAA").
    #[serde(default)]
    pub tdb: Option<String>,
    /// How it was matched: header, serial, hash, size, name, fuzzy, none.
    pub how: String,
    pub confidence: f32,
}

// ---------------------------------------------------------------- reading the game itself

fn read_at(f: &Path, at: u64, n: usize) -> Option<Vec<u8>> {
    let mut h = std::fs::File::open(f).ok()?;
    h.seek(SeekFrom::Start(at)).ok()?;
    let mut b = vec![0u8; n];
    h.read_exact(&mut b).ok()?;
    Some(b)
}

fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

/// A file in an ISO 9660 image (2048-byte sectors), by path ("PS3_GAME/PARAM.SFO").
pub fn iso_file(img: &Path, path: &str, max: usize) -> Option<Vec<u8>> {
    let pvd = read_at(img, 16 * 2048, 2048)?;
    if &pvd[1..6] != b"CD001" {
        return None;
    }
    let mut lba = le32(&pvd, 156 + 2) as u64;
    let mut size = le32(&pvd, 156 + 10) as usize;
    let parts: Vec<&str> = path.split('/').collect();
    for (pi, part) in parts.iter().enumerate() {
        let dir = read_at(img, lba * 2048, size.min(1 << 20))?;
        let mut i = 0;
        let mut found = None;
        while i < dir.len() {
            let len = dir[i] as usize;
            if len == 0 {
                // Records don't cross sectors: on to the next one.
                i = (i / 2048 + 1) * 2048;
                continue;
            }
            if i + 33 > dir.len() {
                break;
            }
            let nl = dir[i + 32] as usize;
            let name = String::from_utf8_lossy(&dir[i + 33..(i + 33 + nl).min(dir.len())]).to_string();
            let name = name.split(';').next().unwrap_or("").to_string();
            if name.eq_ignore_ascii_case(part) {
                found = Some((le32(&dir, i + 2) as u64, le32(&dir, i + 10) as usize, dir[i + 25] & 2 != 0));
                break;
            }
            i += len;
        }
        let (l, s, is_dir) = found?;
        if pi + 1 < parts.len() && !is_dir {
            return None;
        }
        lba = l;
        size = s;
    }
    read_at(img, lba * 2048, size.min(max))
}

/// A PARAM.SFO value (PS3 TITLE_ID, PSP DISC_ID).
pub fn sfo_value(sfo: &[u8], key: &str) -> Option<String> {
    if sfo.len() < 20 || &sfo[..4] != b"\0PSF" {
        return None;
    }
    let keys = le32(sfo, 8) as usize;
    let data = le32(sfo, 12) as usize;
    let n = le32(sfo, 16) as usize;
    for e in 0..n.min(256) {
        let at = 20 + e * 16;
        if at + 16 > sfo.len() {
            break;
        }
        let ko = keys + le16(sfo, at) as usize;
        let k: String = sfo.get(ko..)?.iter().take_while(|c| **c != 0).map(|c| *c as char).collect();
        if k == key {
            let len = le32(sfo, at + 4) as usize;
            let off = data + le32(sfo, at + 12) as usize;
            let v: String = sfo.get(off..off + len)?.iter().take_while(|c| **c != 0).map(|c| *c as char).collect();
            return Some(v);
        }
    }
    None
}

/// "BOOT2 = cdrom0:\SLUS_210.05;1" in a PlayStation disc's SYSTEM.CNF: "SLUS_210.05".
pub fn cnf_serial(cnf: &str) -> Option<String> {
    let l = cnf.lines().find(|l| l.trim_start().starts_with("BOOT"))?;
    let v = l.split_once('=')?.1.trim();
    let v = v.rsplit(['\\', '/', ':']).next()?;
    Some(v.split(';').next()?.trim().to_string())
}

fn clean_id(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// The ID a game carries in its own files, when it has one Rare can read.
pub fn header_id(rom: &Rom) -> Option<String> {
    let f = &rom.file;
    let ext = f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match rom.system.id {
        "gc" | "wii" => crate::roms::disc_id(rom),
        "ps2" | "psx" if ext == "iso" => iso_file(f, "SYSTEM.CNF", 4096).and_then(|b| cnf_serial(&String::from_utf8_lossy(&b))).map(|s| clean_id(&s)),
        "ps3" => {
            let sfo = if f.is_dir() { std::fs::read(f.join("PS3_GAME/PARAM.SFO")).ok() } else { iso_file(f, "PS3_GAME/PARAM.SFO", 65536) };
            sfo.and_then(|b| sfo_value(&b, "TITLE_ID")).map(|s| clean_id(&s))
        }
        "psp" if ext == "iso" => iso_file(f, "PSP_GAME/PARAM.SFO", 65536).and_then(|b| sfo_value(&b, "DISC_ID")).map(|s| clean_id(&s)),
        "nds" if ext == "nds" => read_at(f, 0x0C, 4).map(|b| String::from_utf8_lossy(&b).to_string()).filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric())),
        "n3ds" if matches!(ext.as_str(), "3ds" | "cci") => {
            let h = read_at(f, 0x100, 0x30)?;
            if &h[..4] != b"NCSD" {
                return None;
            }
            let part = le32(&h, 0x20) as u64 * 0x200;
            let p = read_at(f, part + 0x150, 16)?;
            let code = String::from_utf8_lossy(&p).trim_end_matches('\0').to_string();
            code.rsplit('-').next().map(String::from).filter(|c| c.len() == 4)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- matching

/// A title for comparing: no tags, no accents, no punctuation, no leading article.
pub fn norm_title(s: &str) -> String {
    let (t, _) = crate::roms::clean_title(s);
    let t: String = t
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            '&' => ' ',
            c => c,
        })
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let words: Vec<&str> = t.split_whitespace().filter(|w| *w != "the" && *w != "and").collect();
    words.join(" ")
}

fn tokens(s: &str) -> std::collections::HashSet<String> {
    s.split(' ').filter(|w| !w.is_empty()).map(String::from).collect()
}

fn region_of(name: &str) -> String {
    let (_, tags) = crate::roms::clean_title(name);
    tags.split(')').next().unwrap_or("").trim_start_matches('(').trim().to_string()
}

fn tags_of(name: &str) -> Vec<String> {
    let (_, tags) = crate::roms::clean_title(name);
    tags.split(|c| c == '(' || c == ')' || c == '[' || c == ']').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
}

/// The best entry among several (revisions, regions, demos of one serial): the file's own name,
/// then the most tags in common with it (a revision, a version), the same region, then the
/// plainest name.
fn pick<'a>(cands: &[&'a Entry], want_region: &str, stem: &str) -> Option<&'a Entry> {
    let mine = tags_of(stem);
    cands.iter().copied().min_by_key(|e| {
        let r = region_of(&e.name);
        let shared = tags_of(&e.name).iter().filter(|t| mine.contains(t)).count();
        let region_miss = !want_region.is_empty() && !r.contains(want_region) && !want_region.contains(&r);
        (e.name != stem, std::cmp::Reverse(shared), region_miss, !r.contains("USA"), e.name.len())
    })
}

/// Matches a ROM against its system's catalogue. `id` is what `header_id` read.
pub fn match_in(rom: &Rom, id: Option<&str>, entries: &[Entry], stem: &str, size: u64, md5: Option<&str>) -> Ident {
    let mine = Ident { id: id.map(String::from), ..Default::default() };
    let want_region = region_of(stem);
    let found = |e: &Entry, how: &str, c: f32| Ident { name: Some(e.name.clone()), serial: (!e.serial.is_empty()).then(|| e.serial.clone()), how: how.into(), confidence: c, ..mine.clone() };
    // 1. The game's own ID against the catalogue's serials.
    if let Some(id) = id {
        let hits: Vec<&Entry> = entries
            .iter()
            .filter(|e| {
                let s = clean_id(&e.serial);
                match rom.system.id {
                    // "DL-DOL-GQNE-USA" carries the disc ID's first four.
                    "gc" | "wii" => e.serial.split('-').any(|p| p.len() >= 4 && id.len() >= 4 && p[..4].eq_ignore_ascii_case(&id[..4])),
                    "n3ds" => e.serial.rsplit('-').next().is_some_and(|p| p.eq_ignore_ascii_case(id)),
                    _ => !s.is_empty() && (s == id || s.starts_with(id)),
                }
            })
            .collect();
        if let Some(e) = pick(&hits, &want_region, stem) {
            return found(e, "serial", 1.0);
        }
    }
    // 2. The file's hash (cartridges).
    if let Some(h) = md5 {
        if let Some(e) = entries.iter().find(|e| e.md5 == h) {
            return found(e, "hash", 1.0);
        }
    }
    // 3. The exact size of a disc image, when only one dump has it.
    if size > 16 << 20 {
        let hits: Vec<&Entry> = entries.iter().filter(|e| e.size == size).collect();
        if hits.len() == 1 {
            return found(hits[0], "size", 0.98);
        }
    }
    // 4. The catalogue's own name.
    if let Some(e) = entries.iter().find(|e| e.name == stem) {
        return found(e, "name", 1.0);
    }
    // 5. The same title in this system, the region it has.
    let n = norm_title(stem);
    if !n.is_empty() {
        let same: Vec<&Entry> = entries.iter().filter(|e| norm_title(&e.name) == n).collect();
        if let Some(e) = pick(&same, &want_region, stem) {
            let c = if !want_region.is_empty() && region_of(&e.name).contains(&want_region) { 0.95 } else { 0.88 };
            return found(e, "fuzzy", c);
        }
        let t = tokens(&n);
        let best = entries
            .iter()
            .map(|e| {
                let o = tokens(&norm_title(&e.name));
                let inter = t.intersection(&o).count() as f32;
                let uni = t.union(&o).count().max(1) as f32;
                (inter / uni, e)
            })
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((j, e)) = best.filter(|(j, _)| *j * 0.95 >= MIN_CONFIDENCE) {
            return found(e, "fuzzy", j * 0.95);
        }
    }
    Ident { how: if id.is_some() { "header".into() } else { "none".into() }, ..mine }
}

// ---------------------------------------------------------------- GameTDB's Switch list

const SWITCHTDB: &str = "https://www.gametdb.com/switchtdb.zip";

/// (normalised title, product ID) for every Switch game GameTDB knows, cached a month.
fn switch_index(home: &Path) -> Vec<(String, String)> {
    let f = home.join(".cache/rare/emu/dat/switchtdb.json");
    let fresh = f.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() < 30 * 86400);
    let cached = || std::fs::read(&f).ok().and_then(|b| serde_json::from_slice::<Vec<(String, String)>>(&b).ok());
    if fresh {
        if let Some(v) = cached() {
            return v;
        }
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(10)).timeout_read(std::time::Duration::from_secs(60)).build();
    let zip = agent.get(SWITCHTDB).set("User-Agent", "rare-launcher").call().ok().and_then(|r| {
        let mut b = Vec::new();
        r.into_reader().take(64 << 20).read_to_end(&mut b).ok().map(|_| b)
    });
    let Some(xml) = zip.and_then(|z| crate::unzip::entry(&z, ".xml").ok()) else { return cached().unwrap_or_default() };
    let text = String::from_utf8_lossy(&xml);
    let mut v = Vec::new();
    for block in text.split("<game name=\"").skip(1) {
        let Some((name, rest)) = block.split_once('"') else { continue };
        let Some(id) = rest.split("<id>").nth(1).and_then(|r| r.split('<').next()) else { continue };
        let name = name.replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\"");
        // "Animal Crossing: New Horizons (EN,FR,...)": the language list is not part of the title.
        let title = name.rsplit_once(" (").map(|(a, _)| a.to_string()).unwrap_or(name);
        v.push((norm_title(&title), id.to_string()));
    }
    if !v.is_empty() {
        let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new("/")));
        let _ = std::fs::write(&f, serde_json::to_vec(&v).unwrap_or_default());
    }
    v
}

/// The Switch game's GameTDB ID by its title: the plain release before bundles and packs.
pub fn switch_id(index: &[(String, String)], title: &str) -> Option<String> {
    let n = norm_title(title);
    index.iter().find(|(t, _)| *t == n).map(|(_, id)| id.clone())
}

// ---------------------------------------------------------------- the cache

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    /// Key: path, size, modified seconds.
    by_file: HashMap<String, Ident>,
}

fn cache_path(home: &Path) -> PathBuf {
    home.join(".cache/rare/emu/ident.json")
}

fn key_of(rom: &Rom) -> String {
    let m = rom.file.metadata().ok();
    let secs = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}|{}|{secs}", rom.file.display(), m.map(|m| m.len()).unwrap_or(0))
}

static CACHE: Mutex<Option<(PathBuf, Cache)>> = Mutex::new(None);

fn with_cache<R>(home: &Path, f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().map(|(p, _)| p != &cache_path(home)).unwrap_or(true) {
        let c = std::fs::read(cache_path(home)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        *g = Some((cache_path(home), c));
    }
    f(&mut g.as_mut().unwrap().1)
}

/// What Rare already knows about a ROM (no reading, no network).
pub fn known(home: &Path, rom: &Rom) -> Option<Ident> {
    with_cache(home, |c| c.by_file.get(&key_of(rom)).cloned())
}

/// Identifies the ROMs Rare doesn't know yet (reads headers, fetches catalogues). Calls `done`
/// after each one. Returns how many were identified.
pub fn identify_missing(home: &Path, roms: &[Rom], done: &(dyn Fn() + Sync)) -> usize {
    let todo: Vec<&Rom> = roms.iter().filter(|r| known(home, r).is_none()).collect();
    if todo.is_empty() {
        return 0;
    }
    let mut dats: HashMap<&'static str, Vec<Entry>> = HashMap::new();
    let mut sw: Option<Vec<(String, String)>> = None;
    let mut n = 0;
    for rom in todo {
        let id = header_id(rom);
        let entries = dats.entry(rom.system.id).or_insert_with(|| dat::load(home, rom.system).unwrap_or_default());
        let stem = crate::art::stem_of(rom);
        let size = rom.file.metadata().map(|m| m.len()).unwrap_or(0);
        let md5 = (rom.file.is_file() && size > 0 && size <= 64 << 20 && !entries.is_empty())
            .then(|| {
                let skip = if rom.system.id == "nes" && read_at(&rom.file, 0, 4).as_deref() == Some(b"NES\x1a") { 16 } else { 0 };
                crate::md5::file(&rom.file, skip, u64::MAX).ok()
            })
            .flatten();
        let mut ident = match_in(rom, id.as_deref(), entries, &stem, size, md5.as_deref());
        if rom.system.id == "switch" {
            let idx = sw.get_or_insert_with(|| switch_index(home));
            if let Some(t) = switch_id(idx, &rom.title) {
                ident.tdb = Some(t);
                if ident.how == "none" {
                    ident.how = "gametdb title".into();
                    ident.confidence = 0.95;
                }
            }
        }
        with_cache(home, |c| {
            c.by_file.insert(key_of(rom), ident);
        });
        n += 1;
        done();
    }
    with_cache(home, |c| {
        let _ = std::fs::create_dir_all(cache_path(home).parent().unwrap_or(Path::new("/")));
        let _ = std::fs::write(cache_path(home), serde_json::to_vec(c).unwrap_or_default());
    });
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom(sys: &str, file: &str) -> Rom {
        Rom { id: format!("emu:{sys}/{file}"), system: crate::systems::by_id(sys).unwrap(), path: format!("/r/{file}").into(), file: format!("/r/{file}").into(), title: crate::roms::clean_title(file).0, tags: String::new(), discs: 1, bytes: 1, sd: false }
    }
    fn e(name: &str, serial: &str, size: u64, md5: &str) -> Entry {
        Entry { name: name.into(), serial: serial.into(), size, crc: String::new(), md5: md5.into() }
    }

    #[test]
    fn matches_in_order() {
        let gc = vec![e("Mortal Kombat - Deception (USA)", "DL-DOL-GQNE-USA", 1459978240, ""), e("Mortal Kombat - Deception (Europe)", "DL-DOL-GQNP-EUR", 1459978240, ""), e("Mortal Kombat - Deadly Alliance (USA)", "DL-DOL-GMKE-USA", 1, "")];
        let r = rom("gc", "Mortal Kombat - Deception (USA).nkit.iso");
        let m = match_in(&r, Some("GQNE5D"), &gc, "Mortal Kombat - Deception (USA)", 123, None);
        assert_eq!((m.name.as_deref(), m.how.as_str()), (Some("Mortal Kombat - Deception (USA)"), "serial"));
        let ps2 = vec![e("Kingdom Hearts II (USA)", "SLUS-21005", 3966500864, ""), e("Kingdom Hearts II (USA) (Demo)", "SLUS-21005GH", 1, "")];
        let pm = vec![e("Pac-Man World 2 (USA) (v1.00)", "SLUS-20224", 1, ""), e("Pac-Man World 2 (USA) (v2.00)", "SLUS-20224", 2, "")];
        assert_eq!(match_in(&rom("ps2", "p.iso"), Some("SLUS20224"), &pm, "Pac-Man World 2 (USA) (v2.00)", 5, None).name.as_deref(), Some("Pac-Man World 2 (USA) (v2.00)"));
        let m = match_in(&rom("ps2", "KH2.iso"), Some("SLUS21005"), &ps2, "KH2", 5, None);
        assert_eq!(m.name.as_deref(), Some("Kingdom Hearts II (USA)"));
        // Hash, size, name, then a confident title.
        let gba = vec![e("Anguna - Warriors of Virtue (USA)", "", 4, "abc")];
        assert_eq!(match_in(&rom("gba", "x.gba"), None, &gba, "x", 4, Some("abc")).how, "hash");
        assert_eq!(match_in(&rom("ps2", "y.iso"), None, &ps2, "y", 3966500864, None).how, "size");
        let m = match_in(&rom("ps2", "Kingdom Hearts II.iso"), None, &ps2, "Kingdom Hearts II", 5, None);
        assert_eq!((m.name.as_deref(), m.how.as_str()), (Some("Kingdom Hearts II (USA)"), "fuzzy"));
        assert!(m.confidence >= MIN_CONFIDENCE);
        let xb = vec![e("Jet Set Radio Future (USA)", "TQ-031", 7825162240, "")];
        let m = match_in(&rom("xbox", "a.iso"), None, &xb, "Jet Set Radio Future (USA)", 9, None);
        assert_eq!((m.serial.as_deref(), m.how.as_str()), (Some("TQ-031"), "name"));
        // Nothing near enough: no guess.
        let m = match_in(&rom("ps2", "Totally Different.iso"), None, &ps2, "Totally Different", 5, None);
        assert_eq!((m.name, m.how.as_str()), (None, "none"));
        // "The Simpsons - Hit & Run" against Redump's "Simpsons, The - Hit & Run (USA)".
        let s = vec![e("Simpsons, The - Hit & Run (USA)", "DL-DOL-GHQE-USA", 1, "")];
        assert_eq!(match_in(&rom("gc", "a.iso"), None, &s, "The Simpsons - Hit and Run", 5, None).how, "fuzzy");
    }

    #[test]
    fn reads_serials() {
        assert_eq!(cnf_serial("BOOT2 = cdrom0:\\SLUS_210.05;1\r\nVER = 1.00\r\n").as_deref(), Some("SLUS_210.05"));
        assert_eq!(cnf_serial("BOOT = cdrom:\\SCUS_944.26;1").as_deref(), Some("SCUS_944.26"));
        // A tiny PARAM.SFO with TITLE_ID.
        let mut sfo = b"\0PSF\x01\x01\0\0".to_vec();
        let keys = b"TITLE_ID\0\0\0\0";
        let data = b"BLUS30982\0\0\0";
        sfo.extend(&(20u32 + 16).to_le_bytes());
        sfo.extend(&(20u32 + 16 + keys.len() as u32).to_le_bytes());
        sfo.extend(&1u32.to_le_bytes());
        sfo.extend(&0u16.to_le_bytes());
        sfo.extend(&0x0204u16.to_le_bytes());
        sfo.extend(&10u32.to_le_bytes());
        sfo.extend(&12u32.to_le_bytes());
        sfo.extend(&0u32.to_le_bytes());
        sfo.extend(keys);
        sfo.extend(data);
        assert_eq!(sfo_value(&sfo, "TITLE_ID").as_deref(), Some("BLUS30982"));
        assert_eq!(switch_id(&[("animal crossing new horizons".into(), "ACBAA".into())], "Animal Crossing New Horizons").as_deref(), Some("ACBAA"));
        assert_eq!(norm_title("Pokémon Violet (EN,FR)"), norm_title("Pokemon Violet"));
    }

    #[test]
    fn iso_reader() {
        // A minimal ISO: PVD at sector 16, root at sector 18 with SYSTEM.CNF at sector 20.
        let mut img = vec![0u8; 21 * 2048];
        let pvd = 16 * 2048;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        let rec = |lba: u32, size: u32, flags: u8, name: &[u8]| {
            let mut r = vec![0u8; 33 + name.len() + (name.len() + 1) % 2];
            r[0] = r.len() as u8;
            r[2..6].copy_from_slice(&lba.to_le_bytes());
            r[10..14].copy_from_slice(&size.to_le_bytes());
            r[25] = flags;
            r[32] = name.len() as u8;
            r[33..33 + name.len()].copy_from_slice(name);
            r
        };
        let root = rec(18, 2048, 2, b"\0");
        img[pvd + 156..pvd + 156 + root.len()].copy_from_slice(&root);
        let body = b"BOOT2 = cdrom0:\\SLUS_210.05;1\n";
        let f = rec(20, body.len() as u32, 0, b"SYSTEM.CNF;1");
        img[18 * 2048..18 * 2048 + f.len()].copy_from_slice(&f);
        img[20 * 2048..20 * 2048 + body.len()].copy_from_slice(body);
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("g.iso");
        std::fs::write(&p, &img).unwrap();
        let r = Rom { file: p.clone(), path: p, ..rom("ps2", "g.iso") };
        assert_eq!(header_id(&r).as_deref(), Some("SLUS21005"));
    }
}
