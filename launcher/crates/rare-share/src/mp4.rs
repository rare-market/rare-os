//! A small MP4 (ISO BMFF) reader: duration from `mvhd`, the video track's size and rotation
//! from its `tkhd`, and an in-place rewrite of that track's display matrix (used to turn a
//! clip grabbed from the portrait panel back to landscape without re-encoding).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Mp4Info {
    pub duration_ms: Option<u64>,
    /// Coded size of the video track (before the display matrix).
    pub width: u32,
    pub height: u32,
    /// Clockwise display rotation from the matrix: 0, 90, 180 or 270.
    pub rotation: u32,
    /// File offset of the video track's 36-byte matrix.
    pub matrix_at: Option<u64>,
}

impl Mp4Info {
    /// Size as shown, after rotation.
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation % 180 == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8).map(|s| u64::from_be_bytes(s.try_into().unwrap()))
}

/// Boxes directly inside `buf`: (type, payload start, payload end), offsets into `buf`.
fn boxes(buf: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= buf.len() {
        let size32 = be32(buf, at).unwrap_or(0) as u64;
        let kind: [u8; 4] = buf[at + 4..at + 8].try_into().unwrap();
        let (header, size) = match size32 {
            0 => (8u64, (buf.len() - at) as u64),
            1 => match be64(buf, at + 8) {
                Some(s) => (16, s),
                None => break,
            },
            s => (8, s),
        };
        if size < header || at as u64 + size > buf.len() as u64 {
            break;
        }
        out.push((kind, at + header as usize, at + size as usize));
        at += size as usize;
    }
    out
}

fn rotation_of(m: &[u8]) -> u32 {
    let v = |i: usize| be32(m, i * 4).map(|x| x as i32).unwrap_or(0);
    let one = 0x10000;
    match (v(0), v(1), v(3), v(4)) {
        (0, b, c, 0) if b == one && c == -one => 90,
        (a, 0, 0, d) if a == -one && d == -one => 180,
        (0, b, c, 0) if b == -one && c == one => 270,
        _ => 0,
    }
}

/// Parses a `moov` box payload. `base` is the payload's offset in the file.
pub fn parse_moov(moov: &[u8], base: u64) -> Mp4Info {
    let mut info = Mp4Info { duration_ms: None, width: 0, height: 0, rotation: 0, matrix_at: None };
    for (kind, s, e) in boxes(moov) {
        let b = &moov[s..e];
        match &kind {
            b"mvhd" => {
                let v1 = b.first() == Some(&1);
                let (ts, dur) = if v1 { (be32(b, 20), be64(b, 24)) } else { (be32(b, 12), be32(b, 16).map(u64::from)) };
                if let (Some(ts), Some(dur)) = (ts, dur) {
                    if ts > 0 && dur != u64::MAX && dur != u32::MAX as u64 {
                        info.duration_ms = Some(dur.saturating_mul(1000) / ts as u64);
                    }
                }
            }
            b"trak" => {
                let mut tkhd: Option<(usize, usize)> = None;
                let mut video = false;
                for (k, ts, te) in boxes(b) {
                    if &k == b"tkhd" {
                        tkhd = Some((ts, te));
                    } else if &k == b"mdia" {
                        for (k2, hs, he) in boxes(&b[ts..te]) {
                            if &k2 == b"hdlr" && b[ts..te].get(hs + 8..hs + 12) == Some(b"vide".as_slice()) && he > hs {
                                video = true;
                            }
                        }
                    }
                }
                let Some((ts, te)) = tkhd.filter(|_| video && info.matrix_at.is_none()) else { continue };
                let t = &b[ts..te];
                let m = 4 + if t.first() == Some(&1) { 32 } else { 20 } + 16;
                if t.len() < m + 36 + 8 {
                    continue;
                }
                info.rotation = rotation_of(&t[m..m + 36]);
                info.width = be32(t, m + 36).unwrap_or(0) >> 16;
                info.height = be32(t, m + 40).unwrap_or(0) >> 16;
                info.matrix_at = Some(base + (s + ts + m) as u64);
            }
            _ => {}
        }
    }
    info
}

/// Reads an MP4 file's info. Only the `moov` box is loaded.
pub fn parse_file(path: &Path) -> Option<Mp4Info> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut at = 0u64;
    while at + 8 <= len {
        f.seek(SeekFrom::Start(at)).ok()?;
        let mut h = [0u8; 16];
        let n = f.read(&mut h).ok()?;
        if n < 8 {
            return None;
        }
        let size32 = u32::from_be_bytes(h[0..4].try_into().unwrap()) as u64;
        let (header, size) = match size32 {
            0 => (8, len - at),
            1 if n >= 16 => (16, u64::from_be_bytes(h[8..16].try_into().unwrap())),
            1 => return None,
            s => (8, s),
        };
        if size < header {
            return None;
        }
        if &h[4..8] == b"moov" {
            let plen = size - header;
            if plen > 64 << 20 {
                return None;
            }
            let mut buf = vec![0u8; plen as usize];
            f.seek(SeekFrom::Start(at + header)).ok()?;
            f.read_exact(&mut buf).ok()?;
            return Some(parse_moov(&buf, at + header));
        }
        at += size;
    }
    None
}

/// The 3x3 display matrix (16.16 fixed, last column 2.30) that shows a `w` x `h` track
/// rotated clockwise by `deg`.
pub fn matrix_for(deg: u32, w: u32, h: u32) -> [i32; 9] {
    let one = 0x10000;
    let fix = |v: u32| (v as i32).wrapping_shl(16);
    let (a, b, c, d, tx, ty) = match deg % 360 {
        // x' = -y + h, y' = x
        90 => (0, one, -one, 0, fix(h), 0),
        180 => (-one, 0, 0, -one, fix(w), fix(h)),
        // x' = y, y' = -x + w
        270 => (0, -one, one, 0, 0, fix(w)),
        _ => (one, 0, 0, one, 0, 0),
    };
    [a, b, 0, c, d, 0, tx, ty, 0x4000_0000]
}

/// Rewrites the video track's display matrix in place.
pub fn set_rotation(path: &Path, deg: u32) -> Result<(), String> {
    let info = parse_file(path).ok_or("not an mp4")?;
    let at = info.matrix_at.ok_or("no video track")?;
    let mut bytes = Vec::with_capacity(36);
    for v in matrix_for(deg, info.width, info.height) {
        bytes.extend_from_slice(&v.to_be_bytes());
    }
    let mut f = std::fs::OpenOptions::new().write(true).open(path).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    f.write_all(&bytes).map_err(|e| e.to_string())?;
    f.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
pub mod fixture {
    //! A minimal MP4 with an audio track and a video track, built by hand.

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    fn tkhd(w: u32, h: u32) -> Vec<u8> {
        let mut p = vec![0, 0, 0, 3]; // version 0, flags
        p.extend_from_slice(&[0u8; 20]); // times, track id, reserved, duration
        p.extend_from_slice(&[0u8; 16]); // reserved, layer, group, volume, reserved
        for v in super::matrix_for(0, w, h) {
            p.extend_from_slice(&v.to_be_bytes());
        }
        p.extend_from_slice(&(w << 16).to_be_bytes());
        p.extend_from_slice(&(h << 16).to_be_bytes());
        bx(b"tkhd", &p)
    }

    fn trak(handler: &[u8; 4], w: u32, h: u32) -> Vec<u8> {
        let mut hd = vec![0u8; 8];
        hd.extend_from_slice(handler);
        hd.extend_from_slice(&[0u8; 13]);
        let mdia = bx(b"mdia", &bx(b"hdlr", &hd));
        let mut t = tkhd(w, h);
        t.extend_from_slice(&mdia);
        bx(b"trak", &t)
    }

    /// ftyp, moov (mvhd 30.5 s at timescale 1000, an audio trak, a WxH video trak), mdat.
    pub fn mp4(w: u32, h: u32, moov_last: bool) -> Vec<u8> {
        let mut mvhd = vec![0u8; 4];
        mvhd.extend_from_slice(&[0u8; 8]);
        mvhd.extend_from_slice(&1000u32.to_be_bytes());
        mvhd.extend_from_slice(&30_500u32.to_be_bytes());
        mvhd.extend_from_slice(&[0u8; 80]);
        let mut moov = bx(b"mvhd", &mvhd);
        moov.extend(trak(b"soun", 0, 0));
        moov.extend(trak(b"vide", w, h));
        let moov = bx(b"moov", &moov);
        let mdat = bx(b"mdat", &[7u8; 64]);
        let mut out = bx(b"ftyp", b"isom\0\0\x02\0isomiso2avc1mp41");
        if moov_last {
            out.extend(mdat);
            out.extend(moov);
        } else {
            out.extend(moov);
            out.extend(mdat);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rotates() {
        for moov_last in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("clip.mp4");
            std::fs::write(&p, fixture::mp4(1200, 1920, moov_last)).unwrap();
            let i = parse_file(&p).unwrap();
            assert_eq!((i.width, i.height, i.rotation, i.duration_ms), (1200, 1920, 0, Some(30_500)));
            assert_eq!(i.display_size(), (1200, 1920));
            set_rotation(&p, 270).unwrap();
            let j = parse_file(&p).unwrap();
            assert_eq!(j.rotation, 270);
            assert_eq!(j.display_size(), (1920, 1200));
            assert_eq!(j.duration_ms, Some(30_500));
            set_rotation(&p, 90).unwrap();
            assert_eq!(parse_file(&p).unwrap().rotation, 90);
            // Nothing else in the file moved.
            assert_eq!(std::fs::metadata(&p).unwrap().len() as usize, fixture::mp4(1200, 1920, moov_last).len());
        }
    }

    #[test]
    fn ccw_matrix_maps_corners() {
        // A 1200x1920 frame shown rotated 270 clockwise (90 counter-clockwise).
        let m = matrix_for(270, 1200, 1920);
        let f = |v: i32| v as f64 / 65536.0;
        let map = |x: f64, y: f64| (f(m[0]) * x + f(m[3]) * y + f(m[6]), f(m[1]) * x + f(m[4]) * y + f(m[7]));
        // Top-right of the portrait frame lands top-left.
        assert_eq!(map(1200.0, 0.0), (0.0, 0.0));
        // Top-left lands bottom-left.
        assert_eq!(map(0.0, 0.0), (0.0, 1200.0));
        assert_eq!(map(1200.0, 1920.0), (1920.0, 0.0));
    }

    #[test]
    fn junk_is_not_an_mp4() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.mp4");
        std::fs::write(&p, b"hello world, not a box").unwrap();
        assert!(parse_file(&p).is_none());
        std::fs::write(&p, []).unwrap();
        assert!(parse_file(&p).is_none());
    }
}
