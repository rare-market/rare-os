//! MD5 (RFC 1321), for BIOS checks and RetroAchievements' ROM hashes. Not for security.

use std::io::Read;

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

fn k(i: usize) -> u32 {
    ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32
}

pub struct Md5 {
    st: [u32; 4],
    buf: Vec<u8>,
    len: u64,
    k: [u32; 64],
}

impl Default for Md5 {
    fn default() -> Self {
        let mut kk = [0u32; 64];
        for (i, v) in kk.iter_mut().enumerate() {
            *v = k(i);
        }
        Md5 { st: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476], buf: Vec::with_capacity(64), len: 0, k: kk }
    }
}

impl Md5 {
    fn block(&mut self, b: &[u8]) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
        }
        let [mut a, mut bb, mut c, mut d] = self.st;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((bb & c) | (!bb & d), i),
                1 => ((d & bb) | (!d & c), (5 * i + 1) % 16),
                2 => (bb ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (bb | !d), (7 * i) % 16),
            };
            let t = d;
            d = c;
            c = bb;
            bb = bb.wrapping_add(a.wrapping_add(f).wrapping_add(self.k[i]).wrapping_add(m[g]).rotate_left(S[i]));
            a = t;
        }
        self.st[0] = self.st[0].wrapping_add(a);
        self.st[1] = self.st[1].wrapping_add(bb);
        self.st[2] = self.st[2].wrapping_add(c);
        self.st[3] = self.st[3].wrapping_add(d);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u64;
        if !self.buf.is_empty() {
            let take = (64 - self.buf.len()).min(data.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buf.len() == 64 {
                let b = std::mem::take(&mut self.buf);
                self.block(&b);
            }
        }
        while data.len() >= 64 {
            self.block(&data[..64]);
            data = &data[64..];
        }
        self.buf.extend_from_slice(data);
    }

    pub fn hex(mut self) -> String {
        let bits = self.len.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        while (self.buf.len() + pad.len()) % 64 != 56 {
            pad.push(0);
        }
        pad.extend_from_slice(&bits.to_le_bytes());
        let len = self.len;
        self.update(&pad);
        self.len = len;
        self.st.iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
    }
}

pub fn hex(data: &[u8]) -> String {
    let mut m = Md5::default();
    m.update(data);
    m.hex()
}

/// The MD5 of a file, skipping its first `skip` bytes, reading at most `max` bytes after them.
pub fn file(p: &std::path::Path, skip: u64, max: u64) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    if skip > 0 {
        std::io::copy(&mut (&mut f).take(skip), &mut std::io::sink())?;
    }
    let mut m = Md5::default();
    let mut buf = vec![0u8; 1 << 20];
    let mut left = max;
    loop {
        let want = (buf.len() as u64).min(left) as usize;
        if want == 0 {
            break;
        }
        let n = f.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        m.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(m.hex())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rfc_vectors() {
        assert_eq!(super::hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(super::hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(super::hex(b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"), "57edf4a22be3c955ac49da2e2107b67a");
        let mut m = super::Md5::default();
        for c in b"message digest".chunks(3) {
            m.update(c);
        }
        assert_eq!(m.hex(), "f96b697d7cb7938d525a2f31aaf161d0");
    }
}
