//! The recipient field (`src/wallet/tx/recipient.ts`): parse a 0x address or a SuiNS name and
//! combine it with the name lookups into one field state. Parsing and the state machine are pure;
//! [`resolve_recipient`] does both lookups through a [`NameService`] (with feature `chain`,
//! [`ChainNames`] over rare-sui's cached gRPC name service).
//!
//! The field flow: on every keystroke `recipient_state(&parse_recipient(text), &lookup, reverse,
//! paused)`; after the user pauses, `resolve_recipient(names, text)` (or run the lookups yourself
//! and feed their results back in as [`NameLookup`] and the reverse name).

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// What the user typed into the recipient field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Recipient {
    Empty,
    /// A full 32-byte address, lowercased.
    Address { address: String },
    /// A SuiNS name in dot form (`alice.sui`, `pay.alice.sui`); `display` is what the user wrote.
    Name { name: String, display: String },
    Invalid { reason: String },
}

/// Where the forward lookup of a typed name stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum NameLookup {
    Idle,
    Loading,
    Found { address: String },
    Missing,
    Error,
}

/// How the ready address was entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Typed {
    Address,
    Name,
}

/// The recipient field's state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "s", rename_all = "lowercase")]
pub enum RecipientState {
    Empty,
    /// Still typing something that is not valid yet; no error until they pause.
    Typing,
    Invalid { msg: String },
    Loading { name: String },
    /// `suins` is the name typed, or the address's default name.
    Ready { address: String, suins: Option<String>, typed: Typed },
    Missing { name: String, msg: String },
    Error { name: String, msg: String },
}

impl RecipientState {
    /// The address to send to, once the field is ready.
    pub fn address(&self) -> Option<&str> {
        match self {
            RecipientState::Ready { address, .. } => Some(address),
            _ => None,
        }
    }
}

/// JS `String.prototype.trim`: Unicode white space, line terminators and the BOM.
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn is_hex(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A full `0x` + 64 hex digit address (any case, surrounding space allowed).
pub fn is_address(s: &str) -> bool {
    let l = js_trim(s).to_lowercase();
    l.len() == 66 && l.starts_with("0x") && l[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `0x1234...abcd`.
pub fn short_address(a: &str) -> String {
    let u: Vec<u16> = a.encode_utf16().collect();
    if u.len() > 14 {
        format!("{}...{}", String::from_utf16_lossy(&u[..6]), String::from_utf16_lossy(&u[u.len() - 4..]))
    } else {
        a.to_string()
    }
}

/// One SuiNS label: letters, digits and inner hyphens, at most 63 characters.
fn label_ok(l: &str) -> bool {
    let b = l.as_bytes();
    let ok = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    !b.is_empty() && b.len() <= 63 && ok(b[0]) && ok(b[b.len() - 1]) && b.iter().all(|&c| ok(c) || c == b'-')
}

/// `@alice` -> `alice.sui`, `pay@alice` -> `pay.alice.sui`, `Alice.SUI` -> `alice.sui`. None when
/// not a well formed name.
pub fn to_dot_sui(input: &str) -> Option<String> {
    let s = js_trim(input).to_lowercase();
    let labels: Vec<&str> = if s.contains('@') {
        let parts: Vec<&str> = s.split('@').collect();
        let (sub, top) = (parts[0], parts.get(1).copied().unwrap_or(""));
        if parts.len() > 2 || top.is_empty() {
            return None;
        }
        let mut v: Vec<&str> = if sub.is_empty() { vec![] } else { sub.split('.').collect() };
        v.push(top);
        v
    } else {
        s.strip_suffix(".sui")?.split('.').collect()
    };
    if labels.is_empty() || !labels.iter().all(|l| label_ok(l)) {
        return None;
    }
    Some(format!("{}.sui", labels.join(".")))
}

/// What the user typed into the recipient field.
pub fn parse_recipient(input: &str) -> Recipient {
    let s = js_trim(input);
    if s.is_empty() {
        return Recipient::Empty;
    }
    if s.len() >= 2 && s.as_bytes()[0] == b'0' && (s.as_bytes()[1] | 0x20) == b'x' {
        if !(s.starts_with("0x") && is_hex(&s[2..])) {
            return Recipient::Invalid { reason: "Address has characters that are not hex".into() };
        }
        if s.len() != 66 {
            return Recipient::Invalid { reason: "Address should be 0x and 64 characters".into() };
        }
        return Recipient::Address { address: s.to_lowercase() };
    }
    if s.contains('@') || s.to_lowercase().ends_with(".sui") {
        return match to_dot_sui(s) {
            Some(name) => Recipient::Name { name, display: s.to_lowercase() },
            None => Recipient::Invalid { reason: "Names use letters, numbers and hyphens".into() },
        };
    }
    Recipient::Invalid { reason: "Enter a 0x address or a .sui name".into() }
}

/// Combine the parsed input, the forward lookup and the reverse name into one field state.
pub fn recipient_state(p: &Recipient, lookup: &NameLookup, reverse_name: Option<&str>, paused: bool) -> RecipientState {
    match p {
        Recipient::Empty => RecipientState::Empty,
        Recipient::Invalid { reason } => {
            if paused {
                RecipientState::Invalid { msg: reason.clone() }
            } else {
                RecipientState::Typing
            }
        }
        Recipient::Address { address } => RecipientState::Ready { address: address.clone(), suins: reverse_name.map(str::to_string), typed: Typed::Address },
        Recipient::Name { name, .. } => match lookup {
            NameLookup::Found { address } => RecipientState::Ready { address: address.clone(), suins: Some(name.clone()), typed: Typed::Name },
            NameLookup::Missing => RecipientState::Missing { name: name.clone(), msg: format!("No address is set for {name}") },
            NameLookup::Error => RecipientState::Error { name: name.clone(), msg: format!("Could not look up {name}. Check your connection.") },
            NameLookup::Idle | NameLookup::Loading => RecipientState::Loading { name: name.clone() },
        },
    }
}

/// A boxed lookup future.
pub type NameFut<'a> = Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + 'a>>;

/// SuiNS lookups (`nameToAddress` / `nameOf` in data/names.ts). Errors mean the lookup could not
/// be made (offline), not that the name is unknown.
pub trait NameService: Send + Sync {
    /// `alice.sui` -> its target address, None when it has none.
    fn name_to_address<'a>(&'a self, name: &'a str) -> NameFut<'a>;
    /// An address's default name, None when it has none.
    fn name_of<'a>(&'a self, address: &'a str) -> NameFut<'a>;
}

/// Parse and look up in one go (the field calls this after the user pauses typing).
pub async fn resolve_recipient(names: &dyn NameService, text: &str) -> RecipientState {
    let p = parse_recipient(text);
    match &p {
        Recipient::Address { address } => {
            let reverse = names.name_of(address).await.ok().flatten();
            recipient_state(&p, &NameLookup::Idle, reverse.as_deref(), true)
        }
        Recipient::Name { name, .. } => {
            let lookup = match names.name_to_address(name).await {
                Ok(Some(address)) => NameLookup::Found { address },
                Ok(None) => NameLookup::Missing,
                Err(_) => NameLookup::Error,
            };
            recipient_state(&p, &lookup, None, true)
        }
        _ => recipient_state(&p, &NameLookup::Idle, None, true),
    }
}

/// [`NameService`] over the node's gRPC name service, cached five minutes (rare-sui `names`).
#[cfg(feature = "chain")]
pub struct ChainNames<'a> {
    pub cache: &'a rare_sui::names::NameCache,
    pub chain: &'a dyn rare_sui::grpc::ChainRead,
}

#[cfg(feature = "chain")]
impl NameService for ChainNames<'_> {
    fn name_to_address<'a>(&'a self, name: &'a str) -> NameFut<'a> {
        Box::pin(async move { self.cache.name_to_address(self.chain, name).await.map_err(|e| e.to_string()) })
    }
    fn name_of<'a>(&'a self, address: &'a str) -> NameFut<'a> {
        Box::pin(async move { self.cache.name_of(self.chain, address).await.map_err(|e| e.to_string()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::HashMap;

    const A: &str = "0x1eb7c57e3f2bd0fc6cb9dcffd143ea957e4d98f805c358733f76dee0667fe0b1";

    fn st(input: &str, lookup: NameLookup, reverse: Option<&str>, paused: bool) -> RecipientState {
        recipient_state(&parse_recipient(input), &lookup, reverse, paused)
    }

    // tx.test.ts "recipient"
    #[test]
    fn parses_addresses_and_names_and_explains_what_is_wrong() {
        assert_eq!(parse_recipient(A), Recipient::Address { address: A.into() });
        assert_eq!(parse_recipient(&format!(" {} ", A.to_uppercase().replace("0X", "0x"))), Recipient::Address { address: A.into() });
        assert_eq!(parse_recipient("Adeniyi.SUI"), Recipient::Name { name: "adeniyi.sui".into(), display: "adeniyi.sui".into() });
        assert!(matches!(parse_recipient("@adeniyi"), Recipient::Name { name, .. } if name == "adeniyi.sui"));
        assert!(matches!(parse_recipient("pay@adeniyi"), Recipient::Name { name, .. } if name == "pay.adeniyi.sui"));
        assert_eq!(parse_recipient(""), Recipient::Empty);
        let reason = |s: &str| match parse_recipient(s) {
            Recipient::Invalid { reason } => reason,
            other => panic!("{other:?}"),
        };
        assert_eq!(reason("0x12"), "Address should be 0x and 64 characters");
        assert_eq!(reason("0x12zz"), "Address has characters that are not hex");
        assert_eq!(reason("bad_name.sui"), "Names use letters, numbers and hyphens");
        assert_eq!(reason("hello"), "Enter a 0x address or a .sui name");
        assert_eq!(to_dot_sui("@bob").as_deref(), Some("bob.sui"));
        assert_eq!(to_dot_sui("bob"), None);
        assert_eq!(short_address(A), "0x1eb7...e0b1");
    }

    #[test]
    fn field_state_address_name_and_invalid_input() {
        assert_eq!(st(A, NameLookup::Idle, Some("alice.sui"), true), RecipientState::Ready { address: A.into(), suins: Some("alice.sui".into()), typed: Typed::Address });
        assert_eq!(st("@alice", NameLookup::Found { address: A.into() }, None, true), RecipientState::Ready { address: A.into(), suins: Some("alice.sui".into()), typed: Typed::Name });
        assert_eq!(st("alice.sui", NameLookup::Loading, None, true), RecipientState::Loading { name: "alice.sui".into() });
        assert!(matches!(st("nobody.sui", NameLookup::Missing, None, true), RecipientState::Missing { msg, .. } if msg == "No address is set for nobody.sui"));
        assert!(matches!(st("nobody.sui", NameLookup::Error, None, true), RecipientState::Error { .. }));
        assert_eq!(st("0x12", NameLookup::Idle, None, false), RecipientState::Typing);
        assert!(matches!(st("0x12", NameLookup::Idle, None, true), RecipientState::Invalid { .. }));
        assert_eq!(st(A, NameLookup::Idle, None, true).address(), Some(A));
    }

    struct Names {
        forward: HashMap<String, Result<Option<String>, String>>,
        reverse: Result<Option<String>, String>,
    }
    impl NameService for Names {
        fn name_to_address<'a>(&'a self, name: &'a str) -> NameFut<'a> {
            let r = self.forward.get(name).cloned().unwrap_or(Ok(None));
            Box::pin(async move { r })
        }
        fn name_of<'a>(&'a self, _address: &'a str) -> NameFut<'a> {
            let r = self.reverse.clone();
            Box::pin(async move { r })
        }
    }

    #[tokio::test]
    async fn resolve_recipient_runs_the_lookups() {
        let mut forward = HashMap::new();
        forward.insert("alice.sui".to_string(), Ok(Some(A.to_string())));
        forward.insert("down.sui".to_string(), Err("offline".to_string()));
        let n = Names { forward, reverse: Ok(Some("alice.sui".into())) };
        assert_eq!(resolve_recipient(&n, "@alice").await, RecipientState::Ready { address: A.into(), suins: Some("alice.sui".into()), typed: Typed::Name });
        assert!(matches!(resolve_recipient(&n, "nobody.sui").await, RecipientState::Missing { .. }));
        assert!(matches!(resolve_recipient(&n, "down.sui").await, RecipientState::Error { msg, .. } if msg == "Could not look up down.sui. Check your connection."));
        assert_eq!(resolve_recipient(&n, A).await, RecipientState::Ready { address: A.into(), suins: Some("alice.sui".into()), typed: Typed::Address });
        // a failed reverse lookup still leaves the address ready
        let offline = Names { forward: HashMap::new(), reverse: Err("offline".into()) };
        assert_eq!(resolve_recipient(&offline, A).await, RecipientState::Ready { address: A.into(), suins: None, typed: Typed::Address });
        assert!(matches!(resolve_recipient(&n, "0x12").await, RecipientState::Invalid { .. }));
        assert_eq!(resolve_recipient(&n, "  ").await, RecipientState::Empty);
    }

    /// testdata/nft-vectors.json "recipient": every input and state the TS field produced.
    #[test]
    fn matches_the_ts_field() {
        let v: Value = serde_json::from_str(include_str!("../testdata/nft-vectors.json")).unwrap();
        let r = &v["recipient"];
        let parse = r["parse"].as_array().unwrap();
        assert!(parse.len() > 20);
        for c in parse {
            let input = c["input"].as_str().unwrap();
            assert_eq!(serde_json::to_value(parse_recipient(input)).unwrap(), c["parsed"], "parse {input:?}");
            assert_eq!(json!(to_dot_sui(input)), c["dotSui"], "toDotSui {input:?}");
            assert_eq!(json!(is_address(input)), c["isAddress"], "isAddress {input:?}");
        }
        for c in r["short"].as_array().unwrap() {
            assert_eq!(short_address(c[0].as_str().unwrap()), c[1].as_str().unwrap());
        }
        let states = r["states"].as_array().unwrap();
        assert!(states.len() > 100);
        for c in states {
            let lookup: NameLookup = serde_json::from_value(c["lookup"].clone()).unwrap();
            let got = recipient_state(&parse_recipient(c["input"].as_str().unwrap()), &lookup, c["reverse"].as_str(), c["paused"].as_bool().unwrap());
            assert_eq!(serde_json::to_value(&got).unwrap(), c["state"], "{c}");
        }
    }
}
