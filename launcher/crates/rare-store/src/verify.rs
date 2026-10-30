//! Hashing while downloading, and the checks against the build on chain.

use sha2::{Digest, Sha256};
use std::io::{self, Write};

pub const HASH_MISMATCH: &str = "The download didn't match the hash on chain. Nothing was installed.";

/// A writer that hashes (SHA-256) and counts every byte on its way to `inner`.
pub struct HashingWriter<W: Write> {
    inner: W,
    hasher: Sha256,
    count: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        HashingWriter { inner, hasher: Sha256::new(), count: 0 }
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    /// The inner writer, the lowercase hex digest and the number of bytes written.
    pub fn finish(self) -> (W, String, u64) {
        (self.inner, format!("{:x}", self.hasher.finalize()), self.count)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.count += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn valid_slug(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The downloaded archive must have exactly the hash and size recorded on chain.
pub fn check_download(digest: &str, size: u64, expected_sha256: &str, expected_size: u64) -> Result<(), String> {
    if size != expected_size || !is_hex64(expected_sha256) || !digest.eq_ignore_ascii_case(expected_sha256) {
        return Err(HASH_MISMATCH.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn hash(chunks: &[&[u8]]) -> (Vec<u8>, String, u64) {
        let mut w = HashingWriter::new(Vec::new());
        for c in chunks {
            w.write_all(c).unwrap();
        }
        w.finish()
    }

    #[test]
    fn hashes_known_bytes() {
        let (out, d, n) = hash(&[]);
        assert_eq!((out.len(), d.as_str(), n), (0, EMPTY, 0));
        let (out, d, n) = hash(&[b"a", b"", b"bc"]);
        assert_eq!((out.as_slice(), d.as_str(), n), (&b"abc"[..], ABC, 3));
    }

    #[test]
    fn checks_hash_and_size() {
        assert!(check_download(ABC, 3, ABC, 3).is_ok());
        assert!(check_download(ABC, 3, &ABC.to_uppercase(), 3).is_ok());
        assert!(check_download(EMPTY, 0, EMPTY, 0).is_ok());
        assert_eq!(check_download(ABC, 3, EMPTY, 3).unwrap_err(), HASH_MISMATCH);
        assert_eq!(check_download(ABC, 4, ABC, 3).unwrap_err(), HASH_MISMATCH);
        assert_eq!(check_download(ABC, 3, &ABC[..63], 3).unwrap_err(), HASH_MISMATCH);
    }

    #[test]
    fn validates_slugs_and_hex() {
        assert!(valid_slug("my-game-2"));
        assert!(!valid_slug(""));
        assert!(!valid_slug("My-Game"));
        assert!(!valid_slug("../x"));
        assert!(!valid_slug(&"a".repeat(65)));
        assert!(is_hex64(ABC) && is_hex64(&ABC.to_uppercase()));
        assert!(!is_hex64(&format!("{}g", &ABC[..63])));
    }
}
