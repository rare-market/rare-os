//! Studio vouchers for the Local API's `claim` (launcher/src/localapi/voucher.ts, methods.ts
//! `prepareClaim`, claim.ts `runClaim`; docs/LOCAL-API.md "Vouchers"). A voucher is the JSON a
//! game server signs, sent as base64 or base64url. The launcher checks its shape, the player and
//! the expiry, reads the item class or item from chain (never taken from the voucher), builds the
//! one transaction and signs it through Rare Wallet's review sheet. The signature itself is
//! checked by the Move package on chain.

use base64::Engine;
use rare_sui::grpc::{ChainRead, Include};
use rare_sui::ptb::{bcs, norm_addr, Arg, Tx};
use serde_json::Value;

const MAX_ATTRS: usize = 32;
const MAX_ATTR_LEN: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub enum Voucher {
    Item { class_id: String, player: String, kind: u64, keys: Vec<String>, vals: Vec<String>, nonce: u64, expires_ms: u64, sig: Vec<u8> },
    State { item_id: String, rev: u64, keys: Vec<String>, vals: Vec<String>, nonce: u64, expires_ms: u64, sig: Vec<u8> },
    Trophy { registry: String, player: String, game: String, trophy_id: u64, nonce: u64, expires_ms: u64, sig: Vec<u8> },
}

/// What a claim needs from chain besides the voucher (Item and State vouchers).
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub marker: String,
    pub policy_id: Option<String>,
    pub class_id: Option<String>,
}

fn u64_of(v: Option<&Value>, what: &str) -> Result<u64, String> {
    let bad = || format!("The voucher has no valid {what}");
    match v {
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 20 && s.bytes().all(|c| c.is_ascii_digit()) => s.parse().map_err(|_| bad()),
        Some(Value::Number(n)) => n.as_u64().ok_or_else(bad),
        _ => Err(bad()),
    }
}

fn addr_of(v: Option<&Value>, what: &str) -> Result<String, String> {
    match v {
        Some(Value::String(s)) if s.starts_with("0x") && (3..=66).contains(&s.len()) && s[2..].bytes().all(|c| c.is_ascii_hexdigit()) => Ok(norm_addr(s)),
        _ => Err(format!("The voucher has no valid {what}")),
    }
}

fn pairs(keys: Option<&Value>, vals: Option<&Value>) -> Result<(Vec<String>, Vec<String>), String> {
    let list = |v: Option<&Value>| -> Option<Vec<String>> {
        let a = v?.as_array()?;
        if a.len() > MAX_ATTRS {
            return None;
        }
        a.iter().map(|x| x.as_str().filter(|s| s.len() <= MAX_ATTR_LEN).map(str::to_string)).collect()
    };
    let (Some(k), Some(v)) = (list(keys), list(vals)) else { return Err("The voucher has no valid attributes".into()) };
    if k.len() != v.len() {
        return Err("The voucher's attributes don't line up".into());
    }
    let mut seen = std::collections::HashSet::new();
    if !k.iter().all(|x| seen.insert(x.clone())) {
        return Err("The voucher names an attribute twice".into());
    }
    Ok((k, v))
}

/// base64url or base64 text to bytes.
pub fn b64any(text: &str) -> Result<Vec<u8>, ()> {
    let s: String = text.trim().chars().map(|c| match c { '-' => '+', '_' => '/', c => c }).filter(|c| *c != '=').collect();
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(s).map_err(|_| ())
}

fn sig_of(v: Option<&Value>) -> Result<Vec<u8>, String> {
    let b = b64any(v.and_then(Value::as_str).unwrap_or("")).map_err(|_| "The voucher signature is not readable".to_string())?;
    if b.len() != 64 {
        return Err("The voucher signature is not readable".into());
    }
    Ok(b)
}

/// Reads a voucher (shape only). The Err is a sentence the sheet can show.
pub fn decode(text: &str) -> Result<Voucher, String> {
    let unreadable = || "This voucher is not readable".to_string();
    let bytes = b64any(text).map_err(|_| unreadable())?;
    let j: Value = serde_json::from_slice(&bytes).map_err(|_| unreadable())?;
    let o = j.as_object().ok_or_else(unreadable)?;
    if o.get("v").and_then(Value::as_u64) != Some(1) {
        return Err("This voucher is from a newer version of Rare".into());
    }
    let nonce = u64_of(o.get("nonce"), "nonce")?;
    let expires_ms = u64_of(o.get("expiresMs"), "expiry")?;
    match o.get("type").and_then(Value::as_str) {
        Some("item") => {
            let (keys, vals) = pairs(o.get("keys"), o.get("vals"))?;
            Ok(Voucher::Item { class_id: addr_of(o.get("class"), "item class")?, player: addr_of(o.get("player"), "player")?, kind: u64_of(o.get("kind"), "item kind")?, keys, vals, nonce, expires_ms, sig: sig_of(o.get("sig"))? })
        }
        Some("state") => {
            let (keys, vals) = pairs(o.get("keys"), o.get("vals"))?;
            if keys.is_empty() {
                return Err("The voucher changes nothing".into());
            }
            Ok(Voucher::State { item_id: addr_of(o.get("item"), "item")?, rev: u64_of(o.get("rev"), "item revision")?, keys, vals, nonce, expires_ms, sig: sig_of(o.get("sig"))? })
        }
        Some("trophy") => Ok(Voucher::Trophy {
            registry: addr_of(o.get("registry"), "trophy registry")?,
            player: addr_of(o.get("player"), "player")?,
            game: addr_of(o.get("game"), "trophy game")?,
            trophy_id: u64_of(o.get("trophyId"), "trophy")?,
            nonce,
            expires_ms,
            sig: sig_of(o.get("sig"))?,
        }),
        _ => Err("This voucher is for something Rare can't claim yet".into()),
    }
}

impl Voucher {
    fn expires(&self) -> u64 {
        match self {
            Voucher::Item { expires_ms, .. } | Voucher::State { expires_ms, .. } | Voucher::Trophy { expires_ms, .. } => *expires_ms,
        }
    }

    /// A line for the claim sheet's title.
    pub fn title(&self) -> &'static str {
        match self {
            Voucher::Item { .. } => "Claim an item",
            Voucher::State { .. } => "Update your item",
            Voucher::Trophy { .. } => "Claim a trophy",
        }
    }

    /// What the sheet says is being claimed.
    pub fn what(&self) -> String {
        let attrs = |k: &[String], v: &[String], joiner: &str| k.iter().zip(v).map(|(k, v)| format!("{k}{joiner}{v}")).collect::<Vec<_>>().join(", ");
        match self {
            Voucher::Trophy { trophy_id, .. } => format!("Trophy {trophy_id}"),
            Voucher::Item { kind, keys, vals, .. } => {
                if keys.is_empty() {
                    format!("Item kind {kind}")
                } else {
                    format!("Item kind {kind} with {}", attrs(keys, vals, " "))
                }
            }
            Voucher::State { keys, vals, .. } => attrs(keys, vals, " becomes "),
        }
    }
}

/// Why a well-formed voucher can't be used by this player now.
pub fn reject_reason(v: &Voucher, player: &str, now_ms: u64) -> Option<String> {
    if v.expires() <= now_ms {
        return Some("This voucher has expired. Ask the game for a new one.".into());
    }
    let mine = norm_addr(player);
    match v {
        Voucher::Item { player: p, .. } | Voucher::Trophy { player: p, .. } if *p != mine => Some("This voucher is for another account".into()),
        _ => None,
    }
}

// ---------------------------------------------------------------- chain reads

/// `0xpkg::module::Name<Inner>` to (canonical `0xpkg::module::Name`, `Inner`).
pub fn split_generic(t: &str) -> Option<(String, String)> {
    let i = t.find('<')?;
    if !t.ends_with('>') {
        return None;
    }
    let mut it = t[..i].split("::");
    let (a, m, n) = (it.next()?, it.next()?, it.next()?);
    if a.is_empty() || m.is_empty() || n.is_empty() || it.next().is_some() {
        return None;
    }
    Some((format!("{}::{m}::{n}", norm_addr(a)), t[i + 1..t.len() - 1].to_string()))
}

/// `Item<C>` for a class marker type.
pub fn item_type(pkg: &str, marker: &str) -> String {
    format!("{}::item::Item<{marker}>", norm_addr(pkg))
}

fn id_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if s.starts_with("0x") && s[2..].bytes().all(|c| c.is_ascii_hexdigit()) && s.len() > 2 => Some(norm_addr(s)),
        Value::Object(o) => o.get("id").or_else(|| o.get("bytes")).and_then(id_of),
        _ => None,
    }
}

async fn read_object(c: &dyn ChainRead, id: &str) -> Option<(String, Option<Value>)> {
    let o = c.object(id, Include::JSON).await.ok()?;
    Some((o.object_type, o.json))
}

/// What an item voucher's class says on chain: its marker type and its TransferPolicy.
pub async fn read_class(c: &dyn ChainRead, pkg: &str, class_id: &str) -> Result<Chain, String> {
    let bad = || "The voucher's item class isn't a Rare items class".to_string();
    let (ty, json) = read_object(c, class_id).await.ok_or_else(bad)?;
    let (outer, marker) = split_generic(&ty).ok_or_else(bad)?;
    if outer != format!("{}::class::Class", norm_addr(pkg)) {
        return Err(bad());
    }
    let policy_id = json.as_ref().and_then(|j| j.pointer("/policy_cap/policy_id")).and_then(id_of).ok_or("The item class has no transfer policy")?;
    Ok(Chain { marker, policy_id: Some(policy_id), class_id: Some(norm_addr(class_id)) })
}

/// What a state voucher's item says on chain: its marker type, class and current revision.
pub async fn read_item(c: &dyn ChainRead, pkg: &str, item_id: &str) -> Result<(Chain, u64), String> {
    let bad = || "The voucher's item isn't a Rare item".to_string();
    let (ty, json) = read_object(c, item_id).await.ok_or_else(bad)?;
    let (outer, marker) = split_generic(&ty).ok_or_else(bad)?;
    if outer != format!("{}::item::Item", norm_addr(pkg)) {
        return Err(bad());
    }
    let json = json.unwrap_or(Value::Null);
    let class_id = json.get("class").and_then(id_of).ok_or("The item has no class")?;
    let rev = u64_of(Some(json.get("rev").unwrap_or(&Value::String("0".into()))), "item revision")?;
    Ok((Chain { marker, policy_id: None, class_id: Some(class_id) }, rev))
}

// ---------------------------------------------------------------- transactions

/// A plain kiosk the player owns: (kiosk id, owner cap id).
pub type Kiosk = (String, String);

fn strings(tx: &mut Tx, v: &[String]) -> Arg {
    // vector<string>: a length (under 128 here, so one byte) and each string.
    let mut b = vec![v.len() as u8];
    for s in v {
        b.extend(bcs::string(s));
    }
    tx.pure(b)
}

/// The item claim: `mint::claim<C>` into the player's kiosk, or `mint::claim_fresh<C>` (a new
/// kiosk, cap to the player) when they have no plain kiosk.
pub fn build_item_claim(pkg: &str, v: &Voucher, marker: &str, policy_id: &str, player: &str, kiosk: Option<&Kiosk>) -> Option<Tx> {
    let Voucher::Item { class_id, kind, keys, vals, nonce, expires_ms, sig, .. } = v else { return None };
    let mut tx = Tx::new();
    tx.set_sender(player);
    let p = norm_addr(pkg);
    let class = tx.object(class_id);
    let policy = tx.object(policy_id);
    let k = tx.pure_u64(*kind);
    let ks = strings(&mut tx, keys);
    let vs = strings(&mut tx, vals);
    let n = tx.pure_u64(*nonce);
    let e = tx.pure_u64(*expires_ms);
    let s = tx.pure(bcs::bytes(sig));
    let clock = tx.object("0x6");
    match kiosk {
        Some((kid, cap)) => {
            let (kid, cap) = (tx.object(kid), tx.object(cap));
            tx.move_call(&format!("{p}::mint::claim"), &[marker], &[class, policy, kid, cap, k, ks, vs, n, e, s, clock]);
        }
        None => {
            tx.move_call(&format!("{p}::mint::claim_fresh"), &[marker], &[class, policy, k, ks, vs, n, e, s, clock]);
        }
    }
    Some(tx)
}

/// The state update: `state::apply<C>` on the item, borrowed out of the player's kiosk and put
/// back (`kiosk::borrow_val` / `return_val`), or held in the wallet.
pub fn build_state_claim(pkg: &str, v: &Voucher, marker: &str, class_id: &str, player: &str, kiosk: Option<&Kiosk>) -> Option<Tx> {
    let Voucher::State { item_id, keys, vals, nonce, expires_ms, sig, .. } = v else { return None };
    let mut tx = Tx::new();
    tx.set_sender(player);
    let p = norm_addr(pkg);
    let ty = item_type(&p, marker);
    let class = tx.object(class_id);
    let ks = strings(&mut tx, keys);
    let vs = strings(&mut tx, vals);
    let n = tx.pure_u64(*nonce);
    let e = tx.pure_u64(*expires_ms);
    let s = tx.pure(bcs::bytes(sig));
    let clock = tx.object("0x6");
    match kiosk {
        Some((kid, cap)) => {
            let (kid, cap) = (tx.object(kid), tx.object(cap));
            let id = tx.pure_id(item_id);
            let r = tx.move_call("0x2::kiosk::borrow_val", &[&ty], &[kid, cap, id]);
            let (item, borrow) = (r.nth(0), r.nth(1));
            tx.move_call(&format!("{p}::state::apply"), &[marker], &[class, item, ks, vs, n, e, s, clock]);
            tx.move_call("0x2::kiosk::return_val", &[&ty], &[kid, item, borrow]);
        }
        None => {
            let item = tx.object(item_id);
            tx.move_call(&format!("{p}::state::apply"), &[marker], &[class, item, ks, vs, n, e, s, clock]);
        }
    }
    Some(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enc(v: Value) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string())
    }

    fn sig() -> String {
        base64::engine::general_purpose::STANDARD.encode([7u8; 64])
    }

    const P: &str = "0x00000000000000000000000000000000000000000000000000000000000000aa";

    #[test]
    fn decodes_each_kind_in_either_base64() {
        let item = json!({ "v": 1, "type": "item", "class": "0x1", "player": P, "kind": "3", "keys": ["power"], "vals": ["12"], "nonce": "9", "expiresMs": "2000", "sig": sig() });
        let v = decode(&enc(item.clone())).unwrap();
        assert!(matches!(&v, Voucher::Item { kind: 3, nonce: 9, .. }));
        let padded = base64::engine::general_purpose::STANDARD.encode(item.to_string());
        assert_eq!(decode(&padded).unwrap(), v);
        let st = json!({ "v": 1, "type": "state", "item": "0x2", "rev": "4", "keys": ["power"], "vals": ["13"], "nonce": "1", "expiresMs": "2000", "sig": sig() });
        assert!(matches!(decode(&enc(st)).unwrap(), Voucher::State { rev: 4, .. }));
        let tr = json!({ "v": 1, "type": "trophy", "registry": "0x3", "player": P, "game": "0x4", "trophyId": "2", "nonce": "1", "expiresMs": 2000, "sig": sig() });
        assert!(matches!(decode(&enc(tr)).unwrap(), Voucher::Trophy { trophy_id: 2, .. }));
    }

    #[test]
    fn refuses_bad_vouchers_with_a_sentence() {
        assert_eq!(decode("%%%").unwrap_err(), "This voucher is not readable");
        assert!(decode(&enc(json!({ "v": 2, "type": "item" }))).unwrap_err().contains("newer version"));
        let dup = json!({ "v": 1, "type": "item", "class": "0x1", "player": P, "kind": "3", "keys": ["a", "a"], "vals": ["1", "2"], "nonce": "9", "expiresMs": "2000", "sig": sig() });
        assert!(decode(&enc(dup)).unwrap_err().contains("twice"));
        let short = json!({ "v": 1, "type": "state", "item": "0x2", "rev": "4", "keys": ["a"], "vals": ["1"], "nonce": "9", "expiresMs": "2000", "sig": "AAAA" });
        assert!(decode(&enc(short)).unwrap_err().contains("signature"));
    }

    #[test]
    fn rejects_expired_and_other_players() {
        let tr = json!({ "v": 1, "type": "trophy", "registry": "0x3", "player": P, "game": "0x4", "trophyId": "2", "nonce": "1", "expiresMs": "2000", "sig": sig() });
        let v = decode(&enc(tr)).unwrap();
        assert!(reject_reason(&v, P, 3000).unwrap().contains("expired"));
        assert!(reject_reason(&v, "0xbb", 1000).unwrap().contains("another account"));
        assert_eq!(reject_reason(&v, "0xaa", 1000), None);
    }

    #[test]
    fn builds_the_claim_calls() {
        let item = json!({ "v": 1, "type": "item", "class": "0x1", "player": P, "kind": "3", "keys": ["power"], "vals": ["12"], "nonce": "9", "expiresMs": "2000", "sig": sig() });
        let v = decode(&enc(item)).unwrap();
        let fresh = build_item_claim("0x5", &v, "0x5::marks::Hero", "0x6", P, None).unwrap();
        assert!(fresh.targets().iter().any(|t| t.ends_with("::mint::claim_fresh")));
        let k = ("0x7".to_string(), "0x8".to_string());
        let into = build_item_claim("0x5", &v, "0x5::marks::Hero", "0x6", P, Some(&k)).unwrap();
        assert!(into.targets().iter().any(|t| t.ends_with("::mint::claim")));
        let st = json!({ "v": 1, "type": "state", "item": "0x2", "rev": "4", "keys": ["power"], "vals": ["13"], "nonce": "1", "expiresMs": "2000", "sig": sig() });
        let v = decode(&enc(st)).unwrap();
        let tx = build_state_claim("0x5", &v, "0x5::marks::Hero", "0x1", P, Some(&k)).unwrap();
        let t = tx.targets();
        assert!(t.iter().any(|x| x.ends_with("::kiosk::borrow_val")) && t.iter().any(|x| x.ends_with("::kiosk::return_val")) && t.iter().any(|x| x.ends_with("::state::apply")));
    }

    #[test]
    fn splits_generic_types() {
        let (o, m) = split_generic("0x5::class::Class<0x5::marks::Hero>").unwrap();
        assert!(o.ends_with("::class::Class") && o.starts_with("0x000"));
        assert_eq!(m, "0x5::marks::Hero");
        assert!(split_generic("0x5::class::Class").is_none());
    }
}
