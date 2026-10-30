//! rare.market's by-passkey registry: which candidate passkey key is a known Rare account.
//! `GET /v1/accounts/by-passkey/{pk base64url}?device={dk base64url}` answers
//! `{"data":[{"address":"0x..","passkeyPublicKey":"<b64>","deviceKey":"<b64>","createdAt":".."}]}`.
//! The HTTP client is injected (`Fetch`) so the lookup is testable and the crate stays network-free.

use base64::{engine::general_purpose::STANDARD as B64, engine::general_purpose::URL_SAFE_NO_PAD, Engine};

pub const API: &str = "https://rare.market/v1";

/// A blocking GET returning the response body, or an error text (any non-200 is an error).
pub trait Fetch {
    fn get(&self, url: &str) -> Result<String, String>;
}

pub fn lookup_url(base: &str, passkey_pk: &[u8], device_key: &[u8]) -> String {
    format!("{}/accounts/by-passkey/{}?device={}", base.trim_end_matches('/'), URL_SAFE_NO_PAD.encode(passkey_pk), URL_SAFE_NO_PAD.encode(device_key))
}

/// The account registered for this passkey key and device key, if any. Any failure is None: the
/// caller falls through to chain history and the second touch.
pub fn lookup(fetch: &dyn Fetch, base: &str, passkey_pk: &[u8], device_key: &[u8]) -> Option<String> {
    let body = fetch.get(&lookup_url(base, passkey_pk, device_key)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let (pk, dk) = (B64.encode(passkey_pk), B64.encode(device_key));
    v.get("data")?.as_array()?.iter().find_map(|r| {
        let same = |k: &str, want: &str| r.get(k).and_then(|x| x.as_str()).map_or(true, |s| s == want);
        if !same("passkeyPublicKey", &pk) || !same("deviceKey", &dk) {
            return None;
        }
        r.get("address")?.as_str().map(|a| a.to_lowercase())
    })
}

/// The registered account with every passkey of it in committee order (`members`, base64;
/// absent for a one-passkey account, which is then just this key).
pub fn lookup_members(fetch: &dyn Fetch, base: &str, passkey_pk: &[u8], device_key: &[u8]) -> Option<(String, Vec<String>)> {
    let body = fetch.get(&lookup_url(base, passkey_pk, device_key)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let (pk, dk) = (B64.encode(passkey_pk), B64.encode(device_key));
    v.get("data")?.as_array()?.iter().find_map(|r| {
        let same = |k: &str, want: &str| r.get(k).and_then(|x| x.as_str()).map_or(true, |s| s == want);
        if !same("passkeyPublicKey", &pk) || !same("deviceKey", &dk) {
            return None;
        }
        let members = match r.get("members").and_then(|m| m.as_array()) {
            Some(m) => m.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            None => vec![pk.clone()],
        };
        Some((r.get("address")?.as_str()?.to_lowercase(), members))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Mock(Result<String, String>, RefCell<Vec<String>>);
    impl Fetch for Mock {
        fn get(&self, url: &str) -> Result<String, String> {
            self.1.borrow_mut().push(url.into());
            self.0.clone()
        }
    }

    /// Pinned with rare.market web (swarm doc, spec #7).
    const PK_HEX: &str = "026b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296";
    const ADDRESS: &str = "0x507050706b37b70b415cd710891f3f19649bcb764951383a9d8a33a27dfdf7f1";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// A phone and a security key (members 0 and 2, the device key between), pinned with
    /// rare.market web's core.test.ts.
    #[test]
    fn pinned_three_member_vector() {
        let (pk1, dk) = (hex(PK_HEX), (1..=32u8).collect::<Vec<_>>());
        let pk2 = hex("036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296");
        let c = crate::multisig::rare_committee(&[&pk1, &pk2], &dk).unwrap();
        assert_eq!(crate::multisig::multisig_address(&c), "0xd357bc37e514d439fefe18872672b9af3aae329e45bc295fb86bd0cb671e08a5");
        let body = format!(r#"{{"data":[{{"address":"0xD357bc37e514d439fefe18872672b9af3aae329e45bc295fb86bd0cb671e08a5","passkeyPublicKey":"{}","deviceKey":"{}","members":["{}","{}"]}}]}}"#, B64.encode(&pk2), B64.encode(&dk), B64.encode(&pk1), B64.encode(&pk2));
        let m = Mock(Ok(body), RefCell::default());
        let (a, members) = lookup_members(&m, API, &pk2, &dk).unwrap();
        assert_eq!(a, "0xd357bc37e514d439fefe18872672b9af3aae329e45bc295fb86bd0cb671e08a5");
        assert_eq!(members, vec![B64.encode(&pk1), B64.encode(&pk2)]);
    }

    #[test]
    fn pinned_vector_url_and_hit() {
        let (pk, dk) = (hex(PK_HEX), (1..=32u8).collect::<Vec<_>>());
        let addr = crate::multisig::multisig_address(&crate::multisig::passkey_multisig(pk.as_slice().try_into().unwrap(), &dk).unwrap());
        assert_eq!(addr, ADDRESS);
        let body = format!(r#"{{"data":[{{"address":"{addr}","passkeyPublicKey":"{}","deviceKey":"{}","createdAt":"2026-10-27T00:00:00Z"}}]}}"#, B64.encode(&pk), B64.encode(&dk));
        let m = Mock(Ok(body), RefCell::default());
        assert_eq!(lookup(&m, API, &pk, &dk).as_deref(), Some(addr.as_str()));
        let url = m.1.borrow()[0].clone();
        assert!(url.starts_with("https://rare.market/v1/accounts/by-passkey/AmsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKW?device="), "{url}");
        assert!(url.ends_with("?device=AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA"), "{url}");
        assert!(!url.contains('+') && url.matches('=').count() == 1);
    }

    #[test]
    fn empty_error_and_garbage_are_misses() {
        let (pk, dk) = (hex(PK_HEX), (1..=32u8).collect::<Vec<_>>());
        for r in [Ok(r#"{"data":[]}"#.to_string()), Ok("nope".into()), Err("500".to_string())] {
            assert_eq!(lookup(&Mock(r, RefCell::default()), API, &pk, &dk), None);
        }
    }
}
