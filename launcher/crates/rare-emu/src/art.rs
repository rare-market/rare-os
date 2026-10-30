//! Art for ROMs as links, best first, from what `ident` knows: GameTDB and xemu's database by the
//! game's own ID, libretro-thumbnails by the catalogue's exact name, the file's title last. The
//! launcher loads them through rare.market's image front (which caches them) and falls through
//! the list to a text cover. Pictures an earlier version saved in ~/.cache/rare/emu/art still
//! count.

use crate::roms::Rom;
use std::path::{Path, PathBuf};

const HOST: &str = "https://thumbnails.libretro.com";
const GAMETDB: &str = "https://art.gametdb.com/wii/cover";

pub fn dir(home: &Path) -> PathBuf {
    home.join(".cache/rare/emu/art")
}

/// libretro's file name rule: &*/:`<>?\| become underscores.
pub fn thumb_name(name: &str) -> String {
    name.chars().map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c }).collect()
}

/// The game's name as the dump sets write it: no extensions, no disc number.
fn stem(rom: &Rom) -> String {
    let s = crate::roms::game_stem(&rom.file);
    if rom.discs > 1 {
        crate::roms::without_disc(&s)
    } else {
        s
    }
}

fn file_for(home: &Path, rom: &Rom, kind: &str) -> PathBuf {
    let s: String = stem(rom).chars().map(|c| if c == '/' { '_' } else { c }).collect();
    dir(home).join(rom.system.id).join(format!("{s}.{kind}.png"))
}

fn have(f: &Path) -> Option<PathBuf> {
    f.metadata().ok().filter(|m| m.len() > 0).map(|_| f.to_path_buf())
}

/// The cached box art, when Rare has it.
pub fn cached(home: &Path, rom: &Rom) -> Option<PathBuf> {
    have(&file_for(home, rom, "cover"))
}

/// The cached screenshot (the game page's and Home's wide backdrop).
pub fn cached_snap(home: &Path, rom: &Rom) -> Option<PathBuf> {
    have(&file_for(home, rom, "snap"))
}

/// The names to try: the file's own name, then the clean title with the usual regions.
pub fn candidates(rom: &Rom) -> Vec<String> {
    let mut v = vec![stem(rom)];
    let (clean, tags) = crate::roms::clean_title(&v[0]);
    // "(USA) (En,Ja) (Rev 2)": the first region tag alone is the most common set name.
    if let Some(first) = tags.split(") ").next().filter(|t| t.starts_with('(')) {
        let c = format!("{clean} {})", first.trim_end_matches(')'));
        if !v.contains(&c) {
            v.push(c);
        }
    }
    for r in ["(USA)", "(World)", "(Europe)", "(Japan)"] {
        let c = format!("{clean} {r}");
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v
}

/// GameTDB's cover regions for a game ID's region letter, most likely first.
pub fn tdb_regions(id: &str) -> Vec<&'static str> {
    let first = match id.chars().nth(3) {
        Some('E') | Some('N') => "US",
        Some('J') => "JA",
        Some('K') | Some('Q') | Some('T') => "KO",
        Some('D') => "DE",
        Some('F') => "FR",
        Some('S') => "ES",
        Some('I') => "IT",
        Some('H') => "NL",
        Some('U') => "AU",
        _ => "EN",
    };
    let mut v = vec![first];
    for r in ["US", "EN"] {
        if !v.contains(&r) {
            v.push(r);
        }
    }
    v
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~()!,'".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn url_for(repo: &str, kind: &str, name: &str) -> String {
    format!("{HOST}/{}/{kind}/{}.png", enc(repo), enc(&thumb_name(name)))
}

/// The name art is looked up by when nothing better is known.
pub fn stem_of(rom: &Rom) -> String {
    stem(rom)
}

fn tdb_region3(id: &str) -> Vec<&'static str> {
    // PS3 title IDs: BLUS/BCUS/NPUB are American, BLES/BCES European, BLJM/BCJS Japanese.
    let r = match id.get(2..4).unwrap_or("") {
        "US" | "UB" => "US",
        "ES" | "EB" => "EN",
        "JM" | "JS" | "JB" => "JA",
        "AS" => "ZH",
        _ => "US",
    };
    let mut v = vec![r];
    for x in ["US", "EN"] {
        if !v.contains(&x) {
            v.push(x);
        }
    }
    v
}

/// The box art links for a ROM, best first: the game's own ID on GameTDB or xemu's database, the
/// catalogue's exact name on libretro-thumbnails, then (only with no catalogue match) the names a
/// file's title suggests.
pub fn cover_urls(rom: &Rom, id: Option<&crate::ident::Ident>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let own = id.and_then(|i| i.id.clone()).or_else(|| crate::roms::disc_id(rom));
    match (rom.system.id, own.as_deref()) {
        ("gc" | "wii", Some(d)) => v.extend(tdb_regions(d).iter().map(|r| format!("{GAMETDB}/{r}/{d}.png"))),
        ("ps3", Some(d)) => v.extend(tdb_region3(d).iter().map(|r| format!("https://art.gametdb.com/ps3/cover/{r}/{d}.jpg"))),
        ("nds", Some(d)) => v.extend(tdb_regions(d).iter().map(|r| format!("https://art.gametdb.com/ds/cover/{r}/{d}.jpg"))),
        ("n3ds", Some(d)) => v.extend(tdb_regions(d).iter().map(|r| format!("https://art.gametdb.com/3ds/cover/{r}/{d}.jpg"))),
        _ => {}
    }
    if let Some(t) = id.and_then(|i| i.tdb.as_deref()) {
        v.extend(["US", "EN"].iter().map(|r| format!("https://art.gametdb.com/switch/cover/{r}/{t}.jpg")));
    }
    // xemu's title database by the Redump serial ("TQ-031": titles/TQ/031).
    if rom.system.id == "xbox" {
        if let Some((a, b)) = id.and_then(|i| i.serial.as_deref()).and_then(|s| s.split_once('-')) {
            if a.len() == 2 && b.len() == 3 && b.chars().all(|c| c.is_ascii_digit()) {
                v.push(format!("https://raw.githubusercontent.com/xemu-project/xdb/master/titles/{a}/{b}/cover_front.jpg"));
            }
        }
    }
    match id.and_then(|i| i.name.as_deref()) {
        Some(n) => v.push(url_for(rom.system.thumbs, "Named_Boxarts", n)),
        None => v.extend(candidates(rom).iter().take(3).map(|n| url_for(rom.system.thumbs, "Named_Boxarts", n))),
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|u| seen.insert(u.clone()));
    v
}

/// A screenshot for the wide backdrop.
pub fn snap_urls(rom: &Rom, id: Option<&crate::ident::Ident>) -> Vec<String> {
    let name = id.and_then(|i| i.name.clone()).unwrap_or_else(|| stem(rom));
    vec![url_for(rom.system.thumbs, "Named_Snaps", &name)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ident::Ident;

    fn rom(sys: &str, file: &str) -> Rom {
        Rom { id: format!("emu:{sys}/{file}"), system: crate::systems::by_id(sys).unwrap(), path: format!("/r/{file}").into(), file: format!("/r/{file}").into(), title: String::new(), tags: String::new(), discs: 1, bytes: 1, sd: false }
    }

    #[test]
    fn names_and_urls() {
        assert_eq!(thumb_name("Mario & Luigi: Superstar Saga (USA)"), "Mario _ Luigi_ Superstar Saga (USA)");
        let u = url_for("Nintendo - Game Boy", "Named_Boxarts", "Tetris (World) (Rev 1)");
        assert_eq!(u, "https://thumbnails.libretro.com/Nintendo%20-%20Game%20Boy/Named_Boxarts/Tetris%20(World)%20(Rev%201).png");
        let r = rom("gb", "Tetris (World) (Rev 1).gb");
        let c = candidates(&r);
        assert_eq!(c[0], "Tetris (World) (Rev 1)");
        assert!(c.contains(&"Tetris (World)".to_string()));
        let t = tempfile::tempdir().unwrap();
        assert!(cached(t.path(), &r).is_none());
        let f = file_for(t.path(), &r, "cover");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, b"png").unwrap();
        assert_eq!(cached(t.path(), &r), Some(f));
    }

    #[test]
    fn links_from_the_match() {
        let id = |name: &str, own: Option<&str>, serial: Option<&str>, tdb: Option<&str>| Ident { name: Some(name.into()), id: own.map(String::from), serial: serial.map(String::from), tdb: tdb.map(String::from), how: "serial".into(), confidence: 1.0 };
        let gc = rom("gc", "Mortal Kombat - Deception (USA).nkit.iso");
        let u = cover_urls(&gc, Some(&id("Mortal Kombat - Deception (USA)", Some("GQNE5D"), None, None)));
        assert_eq!(u[0], "https://art.gametdb.com/wii/cover/US/GQNE5D.png");
        assert!(u.last().unwrap().ends_with("/Nintendo%20-%20GameCube/Named_Boxarts/Mortal%20Kombat%20-%20Deception%20(USA).png"));
        let ps3 = cover_urls(&rom("ps3", "Borderlands2"), Some(&id("Borderlands 2 (USA) (En,Fr,De,Es,It)", Some("BLUS30982"), None, None)));
        assert_eq!(ps3[0], "https://art.gametdb.com/ps3/cover/US/BLUS30982.jpg");
        let xb = cover_urls(&rom("xbox", "a.iso"), Some(&id("Jet Set Radio Future (USA)", None, Some("TQ-031"), None)));
        assert_eq!(xb[0], "https://raw.githubusercontent.com/xemu-project/xdb/master/titles/TQ/031/cover_front.jpg");
        let sw = cover_urls(&rom("switch", "a.nsp"), Some(&Ident { tdb: Some("ACBAA".into()), how: "gametdb title".into(), ..Default::default() }));
        assert_eq!(sw[0], "https://art.gametdb.com/switch/cover/US/ACBAA.jpg");
        assert_eq!(cover_urls(&rom("nds", "a.nds"), Some(&id("A", Some("BKAJ"), None, None)))[0], "https://art.gametdb.com/ds/cover/JA/BKAJ.jpg");
        // No match: the file's names, never more than three.
        assert_eq!(cover_urls(&rom("snes", "Odd (USA).sfc"), None).len(), 3);
        assert!(snap_urls(&gc, None)[0].contains("/Named_Snaps/Mortal%20Kombat%20-%20Deception%20(USA).png"));
    }
}
