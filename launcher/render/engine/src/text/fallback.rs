//! Lazy fallback fonts. The launcher starts with the bundled faces only (a full system font scan
//! parses ~800 faces on the device). When shaping leaves a glyph missing:
//!
//! 1. the font directories are listed once (names only, no parsing);
//! 2. files whose names fit the missing character's class (emoji: "*Emoji*"; CJK: Noto Sans CJK,
//!    Source Han, ...; anything else: Noto Sans, Noto Sans Symbols, DejaVu Sans) are checked for
//!    the character in their cmap, best name first, and the first that covers it is loaded into
//!    the font database; the text is shaped again at once;
//! 3. characters still missing go to a background thread that checks every other file's cmap;
//!    `TextSys::poll_fonts` loads what it found and says the text must be set again.
//!
//! A character nothing covers is remembered and never looked up again.

use cosmic_text::{fontdb, Buffer, FontSystem};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Characters the buffer shaped to .notdef (glyph 0), skipping ones that never draw.
pub fn missing_chars(buf: &Buffer) -> Vec<char> {
    let mut out = Vec::new();
    for line in &buf.lines {
        let Some(shape) = line.shape_opt() else { continue };
        let text = line.text();
        for span in &shape.spans {
            for word in &span.words {
                for g in &word.glyphs {
                    if g.glyph_id != 0 {
                        continue;
                    }
                    for c in text.get(g.start..g.end).unwrap_or("").chars() {
                        if !ignorable(c) && !out.contains(&c) {
                            out.push(c);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Controls, spaces and default-ignorable code points (joiners, variation selectors, tags).
fn ignorable(c: char) -> bool {
    let u = c as u32;
    c.is_control()
        || c.is_whitespace()
        || matches!(u, 0xAD | 0x34F | 0x61C | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x180B..=0x180F | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0x3164 | 0xFE00..=0xFE0F | 0xFEFF | 0xFFA0 | 0xFFF0..=0xFFF8 | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Emoji,
    Cjk,
    Other,
}

pub fn class(c: char) -> Class {
    match c as u32 {
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2300..=0x23FF => Class::Emoji,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x2FF0..=0x31FF | 0x3200..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF => Class::Cjk,
        _ => Class::Other,
    }
}

/// How well a font file's name fits a class: lower is better, None: not a candidate.
pub fn name_rank(cls: Class, file: &str) -> Option<u8> {
    let n = file.to_ascii_lowercase().replace([' ', '-', '_'], "");
    if !(n.ends_with(".ttf") || n.ends_with(".otf") || n.ends_with(".ttc") || n.ends_with(".otc")) {
        return None;
    }
    match cls {
        Class::Emoji => {
            if n.contains("emoji") {
                Some(if n.contains("color") { 0 } else { 1 })
            } else {
                None
            }
        }
        Class::Cjk => [
            ("notosanscjk", 0),
            ("sourcehansans", 1),
            ("notoserifcjk", 2),
            ("sourcehanserif", 3),
            ("wqy", 4),
            ("droidsansfallback", 4),
            ("hiraginosans", 5),
            ("pingfang", 5),
            ("hiragino", 6),
            ("applesdgothic", 6),
            ("cjk", 7),
        ]
        .iter()
        .find(|(p, _)| n.contains(p))
        .map(|(_, r)| *r),
        Class::Other => {
            if n.starts_with("notosans") && !n.contains("mono") && !n.contains("cjk") {
                // Regular weights first; script faces ("NotoSansThai") get checked by cmap.
                Some(if n.contains("regular") || !n.contains("bold") && !n.contains("light") && !n.contains("thin") && !n.contains("black") { 0 } else { 3 })
            } else if n.starts_with("dejavusans") && !n.contains("mono") {
                Some(1)
            } else if n.contains("symbol") {
                Some(2)
            } else {
                None
            }
        }
    }
}

/// Directories fonts live in, in the order they are listed.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(d) = std::env::var("RR_FONT_DIRS") {
        dirs.extend(std::env::split_paths(&d));
        return dirs;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
        dirs.extend(home.map(|h| h.join("Library/Fonts")));
    } else {
        if let Some(h) = &home {
            dirs.push(h.join(".local/share/fonts"));
            dirs.push(h.join(".fonts"));
        }
        let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        dirs.extend(std::env::split_paths(&data).map(|d| d.join("fonts")));
        dirs.push(PathBuf::from("/usr/share/fonts"));
    }
    dirs.dedup();
    dirs
}

fn list_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    let mut seen = HashSet::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let canon = d.canonicalize().unwrap_or(d);
        if !seen.insert(canon) {
            continue;
        }
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(_) => {
                    if p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc")) {
                        out.push(p);
                    }
                }
                Err(_) => {}
            }
        }
    }
    out.sort();
    out
}

/// Which of `chars` the file's faces map (any face of a collection). Reads the table directory
/// and the cmap only, a few kilobytes even for a 180 MB emoji collection.
fn file_covers(path: &Path, chars: &[char]) -> Vec<char> {
    use skrifa::raw::{tables::cmap::Cmap, FontData, FontRead};
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
    let mut read_at = |off: u64, len: usize| -> Option<Vec<u8>> {
        let mut b = vec![0; len];
        f.seek(SeekFrom::Start(off)).ok()?;
        f.read_exact(&mut b).ok()?;
        Some(b)
    };
    let be32 = |b: &[u8], i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let Some(head) = read_at(0, 12) else { return Vec::new() };
    let faces: Vec<u32> = if &head[..4] == b"ttcf" {
        let n = be32(&head, 8).min(256) as usize;
        read_at(12, 4 * n).map(|b| (0..n).map(|i| be32(&b, i * 4)).collect()).unwrap_or_default()
    } else {
        vec![0]
    };
    let mut out = Vec::new();
    for off in faces {
        let Some(h) = read_at(off as u64, 12) else { continue };
        let tables = u16::from_be_bytes([h[4], h[5]]) as usize;
        let Some(dir) = read_at(off as u64 + 12, 16 * tables) else { continue };
        let Some(rec) = dir.chunks(16).find(|r| &r[..4] == b"cmap") else { continue };
        let (at, len) = (be32(rec, 8) as u64, be32(rec, 12) as usize);
        let Some(bytes) = read_at(at, len.min(16 << 20)) else { continue };
        let Ok(cmap) = Cmap::read(FontData::new(&bytes)) else { continue };
        for &c in chars {
            if cmap.map_codepoint(c).is_some_and(|g| g.to_u32() != 0) && !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

pub struct Fallback {
    enabled: bool,
    files: Option<Vec<PathBuf>>,
    tried: HashSet<PathBuf>,
    /// Nothing covers these.
    dead: HashSet<char>,
    /// Sent to the background scan, answer pending.
    scanning: HashSet<char>,
    rx: Option<mpsc::Receiver<(Vec<PathBuf>, Vec<char>)>>,
    queued: Vec<char>,
    pub loaded: Vec<PathBuf>,
}

/// Files checked by name before the text is shaped again (the rest is the background's job).
const SYNC_CANDIDATES: usize = 4;

impl Fallback {
    pub fn new(enabled: bool) -> Fallback {
        Fallback { enabled, files: None, tried: HashSet::new(), dead: HashSet::new(), scanning: HashSet::new(), rx: None, queued: Vec::new(), loaded: Vec::new() }
    }

    fn files(&mut self) -> &[PathBuf] {
        self.files.get_or_insert_with(|| {
            let t = std::time::Instant::now();
            let f = list_files(&font_dirs());
            log::info!("text: listed {} font files in {:.1} ms", f.len(), t.elapsed().as_secs_f32() * 1000.0);
            f
        })
    }

    fn load(&mut self, fs: &mut FontSystem, path: &Path) {
        let t = std::time::Instant::now();
        fs.db_mut().load_font_source(fontdb::Source::File(path.to_path_buf()));
        log::info!("text: fallback font {} ({:.1} ms)", path.display(), t.elapsed().as_secs_f32() * 1000.0);
        self.loaded.push(path.to_path_buf());
    }

    /// Loads system faces for `missing` found by name. True when any was loaded (shape again).
    pub fn resolve(&mut self, fs: &mut FontSystem, missing: &[char]) -> bool {
        if !self.enabled {
            return false;
        }
        let mut want: Vec<char> = missing.iter().copied().filter(|c| !self.dead.contains(c) && !self.scanning.contains(c)).collect();
        if want.is_empty() {
            return false;
        }
        let mut any = false;
        for cls in [Class::Emoji, Class::Cjk, Class::Other] {
            let chars: Vec<char> = want.iter().copied().filter(|&c| class(c) == cls).collect();
            if chars.is_empty() {
                continue;
            }
            let mut cands: Vec<(u8, PathBuf)> = self
                .files()
                .iter()
                .filter_map(|p| Some((name_rank(cls, p.file_name()?.to_str()?)?, p.clone())))
                .collect();
            cands.retain(|(_, p)| !self.tried.contains(p));
            cands.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let mut left = chars;
            for (_, p) in cands.into_iter().take(SYNC_CANDIDATES) {
                let got = file_covers(&p, &left);
                self.tried.insert(p.clone());
                if got.is_empty() {
                    continue;
                }
                self.load(fs, &p);
                any = true;
                left.retain(|c| !got.contains(c));
                want.retain(|c| !got.contains(c));
                if left.is_empty() {
                    break;
                }
            }
        }
        if !want.is_empty() {
            self.scan(want);
        }
        any
    }

    /// Checks every untried file for `chars` on a worker thread. One scan at a time: characters
    /// that come up meanwhile wait for the next one.
    fn scan(&mut self, chars: Vec<char>) {
        self.scanning.extend(chars.iter().copied());
        if self.rx.is_some() {
            self.queued.extend(chars);
            return;
        }
        self.files();
        let files: Vec<PathBuf> = self.files.as_deref().unwrap_or_default().iter().filter(|p| !self.tried.contains(*p)).cloned().collect();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let _ = std::thread::Builder::new().name("rr-fontscan".into()).spawn(move || {
            let t = std::time::Instant::now();
            let mut left = chars;
            let mut found = Vec::new();
            for p in files {
                if left.is_empty() {
                    break;
                }
                let got = file_covers(&p, &left);
                if !got.is_empty() {
                    left.retain(|c| !got.contains(c));
                    found.push(p);
                }
            }
            log::info!("text: fallback scan {:.0} ms, {} files, {} chars uncovered", t.elapsed().as_secs_f32() * 1000.0, found.len(), left.len());
            let _ = tx.send((found, left));
        });
    }

    /// Loads what the background scan found. True when fonts were added.
    pub fn poll(&mut self, fs: &mut FontSystem) -> bool {
        let Some(rx) = &self.rx else { return false };
        let Ok((found, dead)) = rx.try_recv() else { return false };
        self.rx = None;
        let any = !found.is_empty();
        for p in found {
            self.tried.insert(p.clone());
            self.load(fs, &p);
        }
        self.dead.extend(dead);
        self.scanning.clear();
        let queued = std::mem::take(&mut self.queued);
        if !queued.is_empty() {
            self.scan(queued);
        }
        any
    }

    #[cfg(test)]
    pub fn wait(&mut self, fs: &mut FontSystem) -> bool {
        let Some(rx) = self.rx.take() else { return false };
        let Ok(msg) = rx.recv() else { return false };
        let (tx, rx2) = mpsc::channel();
        tx.send(msg).unwrap();
        self.rx = Some(rx2);
        self.poll(fs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{TextStyle, TextSys};

    #[test]
    fn classes() {
        assert_eq!(class('😀'), Class::Emoji);
        assert_eq!(class('漢'), Class::Cjk);
        assert_eq!(class('ア'), Class::Cjk);
        assert_eq!(class('한'), Class::Cjk);
        assert_eq!(class('ก'), Class::Other);
    }

    #[test]
    fn names() {
        assert_eq!(name_rank(Class::Emoji, "NotoColorEmoji.ttf"), Some(0));
        assert_eq!(name_rank(Class::Emoji, "Apple Color Emoji.ttc"), Some(0));
        assert_eq!(name_rank(Class::Cjk, "NotoSansCJK-VF.ttc"), Some(0));
        assert_eq!(name_rank(Class::Cjk, "NotoSansCJK-Regular.ttc"), Some(0));
        assert_eq!(name_rank(Class::Cjk, "Inter.ttf"), None);
        assert_eq!(name_rank(Class::Other, "NotoSans-Regular.ttf"), Some(0));
        assert_eq!(name_rank(Class::Other, "NotoSansMono-Regular.ttf"), None);
        assert_eq!(name_rank(Class::Other, "NotoSansThai-Regular.ttf"), Some(0));
        assert_eq!(name_rank(Class::Other, "readme.txt"), None);
    }

    #[test]
    fn bundled_text_needs_no_fallback() {
        let mut t = TextSys::new(&[], true, 512);
        let b = t.buffer("Continue playing 1,234.56 $ % … · ×", &TextStyle::new(20.0, 400));
        assert!(missing_chars(&b).is_empty(), "{:?}", missing_chars(&b));
        assert!(t.fallback.files.is_none(), "no directory listing without a miss");
    }

    #[test]
    fn cmap_read_from_the_file() {
        let dir = std::env::temp_dir().join(format!("rr-fallback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("Inter.ttf");
        std::fs::write(&p, crate::text::fonts::INTER_VARIABLE).unwrap();
        assert_eq!(file_covers(&p, &['A', '漢', '…']), vec!['A', '…']);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_chars_found() {
        let mut t = TextSys::new(&[], false, 512);
        let b = t.buffer("ok 漢字 😀\u{FE0F} ok", &TextStyle::new(20.0, 400));
        let m = missing_chars(&b);
        assert!(m.contains(&'漢') && m.contains(&'😀') && !m.contains(&'\u{FE0F}'), "{m:?}");
    }

    /// Uses the machine's fonts (the Mac's or the CI image's): skipped when it has none for CJK.
    #[test]
    fn cjk_and_emoji_load_on_first_use() {
        let mut t = TextSys::new(&[], true, 1024);
        let mut b = t.buffer("ゲーム 漢字", &TextStyle::new(20.0, 400));
        if t.fallback.loaded.is_empty() {
            t.fallback.wait(&mut t.fs);
            let st = TextStyle::new(20.0, 400);
            t.set(&mut b, "ゲーム 漢字", &st);
        }
        if t.fallback.loaded.is_empty() {
            eprintln!("no CJK font on this machine");
            return;
        }
        assert!(missing_chars(&b).is_empty(), "{:?} after {:?}", missing_chars(&b), t.fallback.loaded);
        assert!(t.fallback.loaded.len() <= 2, "{:?}", t.fallback.loaded);
    }
}

#[cfg(test)]
mod machine_tests {
    use super::*;
    use crate::gpu::DrawList;
    use crate::text::{TextPaint, TextStyle, TextSys};

    /// Emoji draw as colour images, not tinted masks (skipped without an emoji font).
    #[test]
    fn emoji_are_colour_glyphs() {
        let mut t = TextSys::new(&[], true, 1024);
        let st = TextStyle::new(24.0, 400);
        let mut b = t.buffer("ok 😀", &st);
        if !missing_chars(&b).is_empty() {
            t.fallback.wait(&mut t.fs);
            t.set(&mut b, "ok 😀", &st);
        }
        if !missing_chars(&b).is_empty() {
            eprintln!("no emoji font on this machine");
            return;
        }
        let mut out = DrawList::default();
        t.paint(&b, &TextPaint::at(0.0, 0.0, crate::Color::WHITE), &mut out);
        let pending = t.take_color_pending();
        if pending.is_empty() {
            // A monochrome face covered it (DejaVu, Symbola, Noto Emoji on a bare CI image).
            eprintln!("skipped: no colour emoji font on this machine (covered by {:?})", t.fallback_files());
            return;
        }
        assert_eq!(out.prims.len(), 2, "the emoji waits for its texture");
        assert_eq!(pending.len(), 1);
        let (key, w, h, rgba) = &pending[0];
        assert!(rgba.chunks(4).any(|p| p[0] != p[1] || p[1] != p[2]), "colour, not grey");
        t.set_color_tex(*key, 7);
        out.clear();
        t.paint(&b, &TextPaint::at(0.0, 0.0, crate::Color::WHITE), &mut out);
        let e = out.prims.last().unwrap();
        assert_eq!(e.p[2], crate::gpu::KIND_IMAGE);
        // Drawn about one em tall whatever the bitmap strike.
        assert!(e.rect[3] > 18.0 && e.rect[3] < 34.0, "{:?} from {w}x{h}", e.rect);
        eprintln!("emoji from {:?}: {w}x{h} drawn {:?}", t.fallback_files(), e.rect);
    }
}
