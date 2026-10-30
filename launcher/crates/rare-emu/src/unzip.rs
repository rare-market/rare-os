//! Takes one file out of a zip (libretro's buildbot ships each core as `<core>_libretro.so.zip`).
//! Stored and deflated entries, read through the central directory.

use std::io::Read;

fn u16le(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]) as usize)
}
fn u32le(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]) as usize)
}

/// The bytes of the entry whose name ends with `suffix` (the first one).
pub fn entry(zip: &[u8], suffix: &str) -> Result<Vec<u8>, String> {
    let bad = || "The download is not a zip file".to_string();
    // End of central directory: the last "PK\x05\x06".
    let eocd = (0..zip.len().saturating_sub(21)).rev().find(|&i| zip[i..i + 4] == [0x50, 0x4b, 0x05, 0x06]).ok_or_else(bad)?;
    let count = u16le(zip, eocd + 10).ok_or_else(bad)?;
    let mut at = u32le(zip, eocd + 16).ok_or_else(bad)?;
    for _ in 0..count {
        if zip.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(bad());
        }
        let method = u16le(zip, at + 10).ok_or_else(bad)?;
        let csize = u32le(zip, at + 20).ok_or_else(bad)?;
        let usize_ = u32le(zip, at + 24).ok_or_else(bad)?;
        let nlen = u16le(zip, at + 28).ok_or_else(bad)?;
        let elen = u16le(zip, at + 30).ok_or_else(bad)?;
        let clen = u16le(zip, at + 32).ok_or_else(bad)?;
        let local = u32le(zip, at + 42).ok_or_else(bad)?;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + nlen).ok_or_else(bad)?).into_owned();
        at += 46 + nlen + elen + clen;
        if !name.ends_with(suffix) || name.ends_with('/') {
            continue;
        }
        if zip.get(local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
            return Err(bad());
        }
        let start = local + 30 + u16le(zip, local + 26).ok_or_else(bad)? + u16le(zip, local + 28).ok_or_else(bad)?;
        let data = zip.get(start..start + csize).ok_or_else(bad)?;
        let out = match method {
            0 => data.to_vec(),
            8 => {
                let mut v = Vec::with_capacity(usize_);
                flate2::read::DeflateDecoder::new(data).read_to_end(&mut v).map_err(|e| format!("The zip didn't unpack: {e}"))?;
                v
            }
            m => return Err(format!("The zip uses a packing Rare can't read ({m})")),
        };
        if out.len() != usize_ {
            return Err("The zip is damaged".into());
        }
        return Ok(out);
    }
    Err(format!("The zip has no {suffix} in it"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    /// A one-entry zip, deflated, built by hand.
    fn make(name: &str, body: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(body).unwrap();
        let c = enc.finish().unwrap();
        let mut z = Vec::new();
        let le16 = |v: usize| (v as u16).to_le_bytes();
        let le32 = |v: usize| (v as u32).to_le_bytes();
        z.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&le32(c.len()));
        z.extend_from_slice(&le32(body.len()));
        z.extend_from_slice(&le16(name.len()));
        z.extend_from_slice(&le16(0));
        z.extend_from_slice(name.as_bytes());
        z.extend_from_slice(&c);
        let cd = z.len();
        z.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        z.extend_from_slice(&le32(c.len()));
        z.extend_from_slice(&le32(body.len()));
        z.extend_from_slice(&le16(name.len()));
        z.extend_from_slice(&[0; 12]);
        z.extend_from_slice(&le32(0));
        z.extend_from_slice(name.as_bytes());
        let cdl = z.len() - cd;
        z.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 1, 0, 1, 0]);
        z.extend_from_slice(&le32(cdl));
        z.extend_from_slice(&le32(cd));
        z.extend_from_slice(&[0, 0]);
        z
    }

    #[test]
    fn takes_the_core_out() {
        let body: Vec<u8> = (0..5000u32).map(|i| (i % 7) as u8).collect();
        let z = make("snes9x_libretro.so", &body);
        assert_eq!(super::entry(&z, "_libretro.so").unwrap(), body);
        assert!(super::entry(&z, ".dll").is_err());
        assert!(super::entry(b"<html>", ".so").is_err());
    }
}
