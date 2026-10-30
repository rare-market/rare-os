//! The last preset, kept in /var/lib/rare/fan across reboots and updates.
//!
//! Format: the preset name on the first line; a custom curve follows as `temp duty` lines.

use crate::curve::{self, Preset};
use std::fs;
use std::io;
use std::path::Path;

pub const STATE_FILE: &str = "/var/lib/rare/fan";

pub fn encode(p: &Preset) -> String {
    let mut s = format!("{}\n", p.name());
    if let Preset::Custom(points) = p {
        for (t, d) in points {
            s.push_str(&format!("{t} {d}\n"));
        }
    }
    s
}

pub fn decode(text: &str) -> Option<Preset> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
    let name = lines.next()?;
    if name == "custom" {
        let mut points = Vec::new();
        for l in lines {
            let (t, d) = l.split_once(char::is_whitespace)?;
            points.push((t.trim().parse().ok()?, d.trim().parse().ok()?));
        }
        curve::validate(&points).ok()?;
        return Some(Preset::Custom(points));
    }
    Preset::parse(name)
}

/// The saved preset; Default when there is none or it can't be read.
pub fn load(path: &Path) -> Preset {
    match fs::read_to_string(path) {
        Ok(text) => decode(&text).unwrap_or_else(|| {
            eprintln!("rare-fand: {} is not a preset, using default", path.display());
            Preset::Default
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Preset::Default,
        Err(e) => {
            eprintln!("rare-fand: reading {}: {e}, using default", path.display());
            Preset::Default
        }
    }
}

/// Written to a temp file and renamed, so a power cut never leaves half a file.
pub fn save(path: &Path, p: &Preset) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, encode(p))?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for p in [Preset::Default, Preset::Cool, Preset::Quiet, Preset::Custom(vec![(40, 25), (70, 60), (85, 100)])] {
            assert_eq!(decode(&encode(&p)), Some(p));
        }
        assert_eq!(decode("quiet"), Some(Preset::Quiet));
        assert_eq!(decode(""), None);
        assert_eq!(decode("loud\n"), None);
        // A broken custom curve is refused, not half-used.
        assert_eq!(decode("custom\n40 50\n30 20\n"), None);
        assert_eq!(decode("custom\n40 x\n"), None);
    }
}
