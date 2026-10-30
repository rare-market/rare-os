//! The dump sets' catalogues (Redump for discs, No-Intro for cartridges) from libretro-database:
//! every known dump's canonical name, serial, size and hashes. libretro-thumbnails names its
//! pictures after these names, so an exact catalogue match is an exact picture name. Cached per
//! system in ~/.cache/rare/emu/dat for a month.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const BASE: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat";
const KEEP_SECS: u64 = 30 * 86400;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub serial: String,
    /// The first file's size, crc and md5 (lowercase).
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub crc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub md5: String,
}

/// Which catalogue a system's dumps are in: (set, file name).
pub fn source(sys: &crate::systems::System) -> Option<(&'static str, &'static str)> {
    let set = match sys.id {
        "psx" | "ps2" | "ps3" | "psp" | "gc" | "wii" | "segacd" | "saturn" | "dreamcast" | "pcenginecd" | "3do" | "xbox" => "redump",
        "arcade" | "mame" | "neogeo" | "dos" | "switch" | "wiiu" => return None,
        _ => "no-intro",
    };
    Some((set, sys.thumbs))
}

/// Reads a clrmamepro DAT: one entry per `game ( ... )`.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut toks = Tokens { s: text.as_bytes(), i: 0 };
    while let Some(t) = toks.next() {
        if t != "game" {
            continue;
        }
        if toks.next().as_deref() != Some("(") {
            continue;
        }
        let mut e = Entry::default();
        let mut depth = 1;
        let mut in_rom = false;
        let mut first_rom = true;
        let mut key: Option<String> = None;
        while let Some(t) = toks.next() {
            match t.as_str() {
                "(" => {
                    depth += 1;
                    if key.as_deref() == Some("rom") {
                        in_rom = true;
                    }
                    key = None;
                }
                ")" => {
                    depth -= 1;
                    if in_rom && depth == 1 {
                        in_rom = false;
                        first_rom = false;
                    }
                    if depth == 0 {
                        break;
                    }
                    key = None;
                }
                _ => match key.take() {
                    None => key = Some(t),
                    Some(k) => match (k.as_str(), in_rom) {
                        ("name", false) if e.name.is_empty() => e.name = t,
                        ("serial", _) if e.serial.is_empty() => e.serial = t,
                        ("size", true) if first_rom => e.size = t.parse().unwrap_or(0),
                        ("crc", true) if first_rom => e.crc = t.to_ascii_lowercase(),
                        ("md5", true) if first_rom => e.md5 = t.to_ascii_lowercase(),
                        _ => {}
                    },
                },
            }
        }
        if !e.name.is_empty() {
            out.push(e);
        }
    }
    out
}

struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl Tokens<'_> {
    fn next(&mut self) -> Option<String> {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
        if self.i >= self.s.len() {
            return None;
        }
        let c = self.s[self.i];
        if c == b'(' || c == b')' {
            self.i += 1;
            return Some((c as char).to_string());
        }
        if c == b'"' {
            let start = self.i + 1;
            let mut j = start;
            while j < self.s.len() && self.s[j] != b'"' {
                j += 1;
            }
            self.i = (j + 1).min(self.s.len());
            return Some(String::from_utf8_lossy(&self.s[start..j]).into_owned());
        }
        let start = self.i;
        while self.i < self.s.len() && !self.s[self.i].is_ascii_whitespace() && self.s[self.i] != b'(' && self.s[self.i] != b')' {
            self.i += 1;
        }
        Some(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }
}

fn cache_file(home: &Path, sys: &str) -> PathBuf {
    home.join(".cache/rare/emu/dat").join(format!("{sys}.json"))
}

fn fresh(p: &Path, secs: u64) -> bool {
    p.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() < secs)
}

/// A system's catalogue: the cached copy, else downloaded and cached (blocks).
pub fn load(home: &Path, sys: &crate::systems::System) -> Option<Vec<Entry>> {
    let (set, file) = source(sys)?;
    let f = cache_file(home, sys.id);
    let cached = || std::fs::read(&f).ok().and_then(|b| serde_json::from_slice::<Vec<Entry>>(&b).ok());
    if fresh(&f, KEEP_SECS) {
        if let Some(v) = cached() {
            return Some(v);
        }
    }
    let url = format!("{BASE}/{set}/{}.dat", file.replace(' ', "%20"));
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(60)).build();
    let text = agent.get(&url).set("User-Agent", "rare-launcher").call().ok().and_then(|r| r.into_string().ok());
    match text.map(|t| parse(&t)).filter(|v| !v.is_empty()) {
        Some(v) => {
            let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new("/")));
            let _ = std::fs::write(&f, serde_json::to_vec(&v).unwrap_or_default());
            Some(v)
        }
        // Offline: a stale copy beats none.
        None => cached(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_a_dat() {
        let t = r#"clrmamepro ( name "x" )
game (
	name "Mortal Kombat - Deception (USA)"
	region "USA"
	serial "DL-DOL-GQNE-USA"
	rom ( name "Mortal Kombat - Deception (USA).iso" size 1459978240 crc AA45D0B6 md5 BDEF32ED1DCB85BC6DA370FA5C2D949D sha1 489C serial "DL-DOL-GQNE-USA" )
)
game (
	name "Big (Disc 1) (USA)"
	rom ( name "a.bin" size 10 crc 01 md5 AA )
	rom ( name "b.bin" size 20 crc 02 md5 BB )
)
"#;
        let v = super::parse(t);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "Mortal Kombat - Deception (USA)");
        assert_eq!(v[0].serial, "DL-DOL-GQNE-USA");
        assert_eq!(v[0].size, 1459978240);
        assert_eq!(v[0].md5, "bdef32ed1dcb85bc6da370fa5c2d949d");
        assert_eq!((v[1].size, v[1].md5.as_str()), (10, "aa"));
    }
}
