//! This launcher's device id: random, made once and kept in storage. Price alerts belong to it,
//! and the stream and recent calls pass it so device-only notices (a price alert) arrive.

use std::io::Read;

use crate::storage::{read_value, write_value, Storage};

const K_DEVICE: &str = "rare.notify.device";

/// A saved id is kept when it looks like one this launcher (or the web launcher) made.
pub fn valid_device_id(s: &str) -> bool {
    s.len() >= 16 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// The saved device id, or a new one (18 random bytes, base64url) saved for next time. The
/// caller keeps it for the session.
pub fn device_id(storage: &dyn Storage) -> String {
    if let Some(s) = read_value(storage, K_DEVICE).and_then(|v| v.as_str().map(str::to_string)) {
        if valid_device_id(&s) {
            return s;
        }
    }
    let id = base64url(&random_bytes::<18>());
    write_value(storage, K_DEVICE, Some(&id));
    id
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_ok() {
        return b;
    }
    // No /dev/urandom: std's per-process random hasher keys, mixed with the time.
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    for (i, chunk) in b.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(i);
        h.write_i64(crate::types::now_ms());
        let v = h.finish().to_le_bytes();
        chunk.copy_from_slice(&v[..chunk.len()]);
    }
    b
}

/// Unpadded base64url, as the web launcher's `btoa(...).replace(...)`.
fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        let chars = c.len() + 1;
        for i in 0..chars {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemStorage;

    #[test]
    fn makes_one_id_and_keeps_it() {
        let s = MemStorage::new();
        let a = device_id(&s);
        assert_eq!(a.len(), 24);
        assert!(valid_device_id(&a));
        assert_eq!(device_id(&s), a);
        s.set(K_DEVICE, Some("\"short\""));
        assert_ne!(device_id(&s), "short");
    }

    #[test]
    fn encodes_base64url() {
        assert_eq!(base64url(b"\xfb\xff\xbf"), "-_-_");
        assert_eq!(base64url(b"hi"), "aGk");
    }
}
