//! A ROM's serial, the id the disc or the dump carries: what tells that "Borderlands 2 (USA).iso"
//! and an extracted "Borderlands2" folder are one game (rare-library dupes.rs). Reads a few KB:
//! PS3 PS3_DISC.SFB (in the folder or the ISO's root), PARAM.SFO; PS1 and PS2 SYSTEM.CNF; PSP
//! UMD_DATA.BIN; GameCube and Wii the header's game ID; Switch the title id in the file name.

use crate::roms::Rom;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The identity key for the library ("ps3:BLUS30982"), when the ROM says.
pub fn key(rom: &Rom) -> Option<String> {
    rare_library::dupes::serial_key(rom.system.id, &serial(rom)?)
}

pub fn serial(rom: &Rom) -> Option<String> {
    let f = &rom.file;
    match rom.system.id {
        "gc" | "wii" => crate::roms::disc_id(rom),
        "ps3" if f.is_dir() => std::fs::read(f.join("PS3_DISC.SFB")).ok().and_then(|b| sfb_title(&b)).or_else(|| std::fs::read(f.join("PS3_GAME/PARAM.SFO")).ok().and_then(|b| sfo_title(&b))),
        "ps3" => iso_file(f, "PS3_DISC.SFB", 2048, 0).and_then(|b| sfb_title(&b)),
        "ps2" | "psx" => disc_file(f, "SYSTEM.CNF").and_then(|b| cnf_boot(&String::from_utf8_lossy(&b))),
        "psp" => iso_file(f, "UMD_DATA.BIN", 2048, 0).and_then(|b| String::from_utf8_lossy(&b).split('|').next().map(|s| s.trim().to_string())).filter(|s| s.len() >= 9),
        "switch" => switch_title(&f.file_name()?.to_string_lossy()),
        _ => None,
    }
}

/// A file in the root of a disc image: plain 2048-byte sectors, or raw 2352-byte ones (a .bin,
/// mode 1 or 2). A .cue reads its first track.
fn disc_file(f: &Path, name: &str) -> Option<Vec<u8>> {
    let ext = f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let f = if ext == "cue" {
        let t = std::fs::read_to_string(f).ok()?;
        let line = t.lines().map(str::trim).find(|l| l.starts_with("FILE "))?;
        let n = line.strip_prefix("FILE ")?.rsplit_once(' ').map(|(n, _)| n)?.trim().trim_matches('"').to_string();
        f.parent()?.join(n)
    } else {
        f.to_path_buf()
    };
    iso_file(&f, name, 2048, 0).or_else(|| iso_file(&f, name, 2352, 24)).or_else(|| iso_file(&f, name, 2352, 16))
}

/// Reads `name` from the root directory of an ISO 9660 image (sector size, data offset in a sector).
fn iso_file(f: &Path, name: &str, sector: u64, skip: u64) -> Option<Vec<u8>> {
    if !f.is_file() {
        return None;
    }
    let mut h = std::fs::File::open(f).ok()?;
    let mut read_at = |lba: u64, len: usize| -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        let mut lba = lba;
        while out.len() < len {
            h.seek(SeekFrom::Start(lba * sector + skip)).ok()?;
            let mut b = vec![0u8; 2048.min(len - out.len())];
            h.read_exact(&mut b).ok()?;
            out.extend(b);
            lba += 1;
        }
        Some(out)
    };
    let pvd = read_at(16, 2048)?;
    if &pvd[1..6] != b"CD001" || pvd[0] != 1 {
        return None;
    }
    let root = &pvd[156..190];
    let lba = u32::from_le_bytes(root[2..6].try_into().ok()?) as u64;
    let size = (u32::from_le_bytes(root[10..14].try_into().ok()?) as usize).min(64 << 10);
    let dir = read_at(lba, size)?;
    let mut i = 0;
    while i < dir.len() {
        let l = dir[i] as usize;
        if l == 0 {
            // Records never cross a sector: the rest of this one is padding.
            i = (i / 2048 + 1) * 2048;
            continue;
        }
        let r = dir.get(i..i + l)?;
        let nl = *r.get(32)? as usize;
        let n = String::from_utf8_lossy(r.get(33..33 + nl)?);
        let n = n.split(';').next().unwrap_or("");
        if n.eq_ignore_ascii_case(name) {
            let at = u32::from_le_bytes(r[2..6].try_into().ok()?) as u64;
            let len = (u32::from_le_bytes(r[10..14].try_into().ok()?) as usize).min(64 << 10);
            return read_at(at, len);
        }
        i += l;
    }
    None
}

/// PS3_DISC.SFB: a ".SFB" header, then 32-byte entries (16-byte key, offset and length, big end).
fn sfb_title(b: &[u8]) -> Option<String> {
    if b.get(..4)? != b".SFB" {
        return None;
    }
    let mut at = 0x20;
    while at + 0x20 <= b.len().min(0x200) {
        let key = String::from_utf8_lossy(&b[at..at + 16]);
        let key = key.trim_end_matches('\0');
        if key.is_empty() {
            break;
        }
        if key == "TITLE_ID" {
            let off = u32::from_be_bytes(b[at + 16..at + 20].try_into().ok()?) as usize;
            let len = u32::from_be_bytes(b[at + 20..at + 24].try_into().ok()?) as usize;
            let v = String::from_utf8_lossy(b.get(off..off + len)?).trim_end_matches('\0').trim().to_string();
            return (!v.is_empty()).then_some(v);
        }
        at += 0x20;
    }
    None
}

/// PARAM.SFO's TITLE_ID.
fn sfo_title(b: &[u8]) -> Option<String> {
    if b.get(..4)? != b"\0PSF" {
        return None;
    }
    let u32le = |o: usize| -> Option<usize> { Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize) };
    let (keys, data, n) = (u32le(8)?, u32le(12)?, u32le(16)?);
    for e in 0..n.min(256) {
        let at = 20 + e * 16;
        let ko = u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize;
        let len = u32le(at + 4)?;
        let off = u32le(at + 12)?;
        let k = b.get(keys + ko..)?.split(|c| *c == 0).next()?;
        if k == b"TITLE_ID" {
            let v = String::from_utf8_lossy(b.get(data + off..data + off + len)?).trim_end_matches('\0').to_string();
            return (!v.is_empty()).then_some(v);
        }
    }
    None
}

/// SYSTEM.CNF's boot file: "BOOT2 = cdrom0:\SLUS_213.62;1" is SLUS_213.62.
fn cnf_boot(t: &str) -> Option<String> {
    let line = t.lines().find(|l| l.trim_start().to_ascii_uppercase().starts_with("BOOT"))?;
    let v = line.split_once('=')?.1.trim();
    let v = v.rsplit(['\\', ':', '/']).next()?.split(';').next()?.trim();
    (v.len() >= 9 && v.as_bytes()[4] == b'_').then(|| v.to_string())
}

/// "[01006F8002326000]" in a Switch file name; updates (..800) and DLC go by the base game's id.
fn switch_title(name: &str) -> Option<String> {
    let i = name.find('[')?;
    let id: String = name[i + 1..].chars().take_while(|c| *c != ']').collect();
    (id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit()) && id.starts_with("01")).then(|| format!("{}000", &id[..13].to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        assert_eq!(cnf_boot("BOOT2 = cdrom0:\\SLUS_213.62;1\nVER = 1.00\n").as_deref(), Some("SLUS_213.62"));
        assert_eq!(cnf_boot("BOOT = cdrom:\\SCUS_944.26;1").as_deref(), Some("SCUS_944.26"));
        assert_eq!(switch_title("Pokemon Violet [01008F6008C5E800][v0][US].nsp").as_deref(), Some("01008F6008C5E000"));
        let mut sfb = vec![0u8; 0x300];
        sfb[..4].copy_from_slice(b".SFB");
        sfb[0x20..0x2b].copy_from_slice(b"HYBRID_FLAG");
        sfb[0x40..0x48].copy_from_slice(b"TITLE_ID");
        sfb[0x50..0x54].copy_from_slice(&0x220u32.to_be_bytes());
        sfb[0x54..0x58].copy_from_slice(&0x10u32.to_be_bytes());
        sfb[0x220..0x22a].copy_from_slice(b"BLUS-30982");
        assert_eq!(sfb_title(&sfb).as_deref(), Some("BLUS-30982"));
    }

    #[test]
    fn reads_a_file_from_an_iso() {
        // A minimal image: the PVD at sector 16, the root directory at 18, the file at 19.
        let mut img = vec![0u8; 2048 * 20];
        let pvd = 16 * 2048;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        let root = pvd + 156;
        img[root + 2..root + 6].copy_from_slice(&18u32.to_le_bytes());
        img[root + 10..root + 14].copy_from_slice(&2048u32.to_le_bytes());
        let d = 18 * 2048;
        let name = b"SYSTEM.CNF;1";
        let len = 33 + name.len() + 1;
        img[d] = len as u8;
        img[d + 2..d + 6].copy_from_slice(&19u32.to_le_bytes());
        let body = b"BOOT2 = cdrom0:\\SLES_123.45;1\n";
        img[d + 10..d + 14].copy_from_slice(&(body.len() as u32).to_le_bytes());
        img[d + 32] = name.len() as u8;
        img[d + 33..d + 33 + name.len()].copy_from_slice(name);
        img[19 * 2048..19 * 2048 + body.len()].copy_from_slice(body);
        let dir = tempfile_dir();
        let p = dir.join("g.iso");
        std::fs::write(&p, &img).unwrap();
        let b = disc_file(&p, "system.cnf").unwrap();
        assert_eq!(cnf_boot(&String::from_utf8_lossy(&b)).as_deref(), Some("SLES_123.45"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rare-serial-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
