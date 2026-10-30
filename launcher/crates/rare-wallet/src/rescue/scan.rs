//! Read-only scan of a Playtron (GameOS) wallet (scan.ts): lists everything the address owns and
//! sorts it into coins, collectibles, kiosks (with listed and locked items), staked SUI, SuiNS
//! names and soulbound objects. Reads only, through rare-sui's `ChainRead`.

use super::*;
use rare_sui::grpc::{ChainRead, Include, ObjectInfo};
use rare_sui::ids::{canon_addr, canon_type, struct_tag};
use rare_sui::Result;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const SUI_TYPE: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
/// owned objects read per scan; a wallet past this is flagged as truncated
const MAX_OBJECTS: usize = 5000;
/// Mysten's kiosk extensions package (personal_kiosk); upgradeable, so calls go to its latest version.
pub const KIOSK_EXT_LATEST: &str = "0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3";

const FULL: Include = Include { json: true, display: true, balance: true, ..Include::NONE };

/// (long address, module, name) of a struct type.
fn parts(ty: &str) -> Option<(String, String, String)> {
    let t = struct_tag(ty).ok()?;
    Some((canon_addr(&t.address().to_string()), t.module().as_str().to_string(), t.name().as_str().to_string()))
}

fn is_type(ty: &str, addr: &str, module: &str, name: &str) -> bool {
    parts(ty).is_some_and(|(a, m, n)| a == canon_addr(addr) && m == module && n == name)
}

fn has_name(ty: &str, module: &str, name: &str) -> bool {
    parts(ty).is_some_and(|(_, m, n)| m == module && n == name)
}

/// `Coin<T>` to T, long form.
fn coin_type_of(ty: &str) -> Option<String> {
    if !is_type(ty, "0x2", "coin", "Coin") {
        return None;
    }
    let (a, b) = (ty.find('<')?, ty.rfind('>')?);
    (a < b).then(|| canon_type(&ty[a + 1..b]))
}

fn pretty(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.replace('_', " ").chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() && chars[i - 1].is_ascii_lowercase() {
            out.push(' ');
        }
        out.push(*c);
    }
    out
}

fn str_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// u64-ish JSON: a number, a string, or `{value: ..}` (a Balance).
fn big(v: Option<&Value>) -> u128 {
    match v {
        Some(Value::Object(o)) => big(o.get("value")),
        other => str_of(other).and_then(|s| s.parse().ok()).unwrap_or(0),
    }
}

fn item(o: &ObjectInfo) -> RescueItem {
    let ty = canon_type(&o.object_type);
    let (module, type_name) = parts(&ty).map(|(_, m, n)| (m, n)).unwrap_or_default();
    let d = |k: &str| o.display.as_ref().and_then(|d| str_of(d.get(k)));
    let j = |k: &str| o.json.as_ref().and_then(|d| str_of(d.get(k)));
    let name = d("name").or_else(|| j("name")).unwrap_or_else(|| pretty(if type_name.is_empty() { "Object" } else { &type_name }));
    let image = d("image_url").or_else(|| d("image")).or_else(|| j("image_url")).or_else(|| j("url"));
    let image = image.filter(|i| ["http:", "https:", "ipfs:", "data:"].iter().any(|p| i.starts_with(p)));
    let collection = d("collection_name").or_else(|| d("collection")).unwrap_or_else(|| pretty(if module.is_empty() { &type_name } else { &module }));
    RescueItem { object_id: canon_addr(&o.object_id), type_: ty, name, image, collection }
}

/// The KioskOwnerCap id inside a PersonalKioskCap's `cap` (an Option: a value, or `{vec: [..]}`).
fn personal_inner(j: Option<&Value>) -> Option<String> {
    let cap = j?.get("cap")?;
    let one = match cap {
        Value::Array(a) => a.first()?,
        Value::Object(o) => match o.get("vec") {
            Some(Value::Array(a)) => a.first()?,
            _ => cap,
        },
        other => other,
    };
    str_of(one.get("for"))
}

fn id_from_bcs(b: &[u8]) -> Option<String> {
    (b.len() >= 32).then(|| format!("0x{}", b[..32].iter().map(|x| format!("{x:02x}")).collect::<String>()))
}

async fn read_kiosk(c: &dyn ChainRead, kiosk_id: &str, cap_id: &str, personal_package: Option<&str>) -> Result<RescueKiosk> {
    let fields = c.dynamic_fields(kiosk_id, 2000).await?;
    let mut ids: Vec<String> = Vec::new();
    let mut listing_fields: BTreeMap<String, String> = BTreeMap::new();
    let mut locked: BTreeSet<String> = BTreeSet::new();
    let mut exclusive: BTreeSet<String> = BTreeSet::new();
    for f in &fields {
        let t = canon_type(&f.name_type);
        let Some(id) = id_from_bcs(&f.name_bcs) else { continue };
        if is_type(&t, "0x2", "kiosk", "Item") {
            ids.push(id);
        } else if is_type(&t, "0x2", "kiosk", "Listing") {
            if f.name_bcs.get(32) == Some(&1) {
                exclusive.insert(id.clone());
            }
            listing_fields.insert(id, f.field_id.clone());
        } else if is_type(&t, "0x2", "kiosk", "Lock") {
            locked.insert(id);
        }
    }
    // A listing's price is the u64 value of its Field object: UID (32) + name (ID + bool = 33) + price.
    let lf: Vec<String> = listing_fields.values().cloned().collect();
    let mut prices: BTreeMap<String, u128> = BTreeMap::new();
    if !lf.is_empty() {
        let objs = c.objects(&lf, Include::CONTENTS).await?;
        for ((item_id, _), o) in listing_fields.iter().zip(objs) {
            let price = o.ok().and_then(|o| o.contents).and_then(|b| b.get(65..73).map(|s| u64::from_le_bytes(s.try_into().unwrap()))).unwrap_or(0);
            prices.insert(item_id.clone(), price as u128);
        }
    }
    let kiosk = c.object(kiosk_id, Include::JSON).await.ok();
    let objs: Vec<ObjectInfo> = c.objects(&ids, FULL).await?.into_iter().filter_map(|r| r.ok()).collect();
    let items = objs
        .iter()
        .map(|o| {
            let it = item(o);
            let id = it.object_id.clone();
            RescueKioskItem { item: it, listed: prices.get(&id).copied(), locked: locked.contains(&id), transferable: o.has_public_transfer.unwrap_or(false), exclusive: exclusive.contains(&id) }
        })
        .collect();
    Ok(RescueKiosk {
        kiosk_id: kiosk_id.to_string(),
        cap_id: cap_id.to_string(),
        personal: personal_package.is_some(),
        personal_package: personal_package.map(str::to_string),
        items,
        profits: big(kiosk.as_ref().and_then(|k| k.json.as_ref()).and_then(|j| j.get("profits"))),
    })
}

/// Everything `address` owns, sorted for the move plan. `progress` gets the stage lines.
pub async fn scan_playtron(c: &dyn ChainRead, address: &str, progress: &(dyn Fn(&str) + Sync)) -> Result<RescueScan> {
    let owner = canon_addr(address);
    progress("Reading what this wallet holds");
    let objects = c.owned_objects(&owner, None, MAX_OBJECTS + 1, FULL).await?;
    let truncated = objects.len() > MAX_OBJECTS;
    let objects = &objects[..objects.len().min(MAX_OBJECTS)];
    let balances = c.list_balances(&owner).await?;
    let default_name = c.default_name(&owner).await.ok().flatten();

    let mut coin_objects: BTreeMap<String, Vec<(String, u128)>> = BTreeMap::new();
    let mut nfts = Vec::new();
    let mut soulbound = Vec::new();
    let mut staked = Vec::new();
    let mut names = Vec::new();
    let mut caps: Vec<(String, String, Option<String>)> = Vec::new();

    for o in objects {
        let ty = canon_type(&o.object_type);
        if let Some(ct) = coin_type_of(&ty) {
            coin_objects.entry(ct).or_default().push((canon_addr(&o.object_id), o.balance.unwrap_or(0) as u128));
            continue;
        }
        let j = o.json.as_ref();
        if is_type(&ty, "0x3", "staking_pool", "StakedSui") {
            staked.push(RescueStake {
                object_id: canon_addr(&o.object_id),
                principal: big(j.and_then(|j| j.get("principal"))),
                pool_id: str_of(j.and_then(|j| j.get("pool_id"))).unwrap_or_default(),
                activation_epoch: str_of(j.and_then(|j| j.get("stake_activation_epoch"))).and_then(|s| s.parse().ok()).unwrap_or(0),
            });
            continue;
        }
        if is_type(&ty, "0x2", "kiosk", "KioskOwnerCap") {
            if let Some(k) = str_of(j.and_then(|j| j.get("for"))) {
                caps.push((canon_addr(&k), canon_addr(&o.object_id), None));
            }
            continue;
        }
        if has_name(&ty, "personal_kiosk", "PersonalKioskCap") {
            match personal_inner(j) {
                Some(k) => caps.push((canon_addr(&k), canon_addr(&o.object_id), Some(KIOSK_EXT_LATEST.to_string()))),
                None => soulbound.push(item(o)),
            }
            continue;
        }
        let sub = has_name(&ty, "subdomain_registration", "SubDomainRegistration");
        if sub || has_name(&ty, "suins_registration", "SuinsRegistration") {
            let inner = if sub { j.and_then(|j| j.get("nft")) } else { j };
            let domain = str_of(inner.and_then(|j| j.get("domain_name")))
                .or_else(|| {
                    let labels = inner?.get("domain")?.get("labels")?.as_array()?;
                    Some(labels.iter().rev().filter_map(|l| l.as_str()).collect::<Vec<_>>().join("."))
                })
                .unwrap_or_else(|| "SuiNS name".into());
            names.push(RescueName {
                object_id: canon_addr(&o.object_id),
                domain,
                expires_ms: str_of(inner.and_then(|j| j.get("expiration_timestamp_ms"))).and_then(|s| s.parse().ok()).unwrap_or(0),
                subname: sub,
            });
            continue;
        }
        if o.has_public_transfer.unwrap_or(false) {
            nfts.push(item(o));
        } else {
            soulbound.push(item(o));
        }
    }

    progress("Reading coins");
    let mut coins = Vec::new();
    for b in balances.iter().filter(|b| b.balance > 0) {
        let coin_type = canon_type(&b.coin_type);
        let meta = c.coin_metadata(&coin_type).await.ok().flatten();
        let coin_raw = b.coin_balance.unwrap_or(if b.address_balance.is_none() { b.balance } else { 0 }) as u128;
        let address_raw = b.address_balance.unwrap_or(0) as u128;
        let mut own = coin_objects.get(&coin_type).cloned().unwrap_or_default();
        own.sort_by(|x, y| y.1.cmp(&x.1));
        let symbol = meta.as_ref().map(|m| m.symbol.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| coin_type.rsplit("::").next().unwrap_or("?").to_string());
        coins.push(RescueCoin {
            symbol,
            name: meta.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
            decimals: meta.as_ref().map_or(9, |m| m.decimals),
            icon_url: meta.and_then(|m| m.icon_url),
            raw: coin_raw + address_raw,
            coin_raw,
            address_raw,
            object_ids: own.iter().map(|x| x.0.clone()).collect(),
            object_balances: own.iter().map(|x| x.1).collect(),
            coin_type,
        });
    }
    let amount = |c: &RescueCoin| c.raw as f64 / 10f64.powi(c.decimals as i32);
    coins.sort_by(|a, b| {
        if a.coin_type == SUI_TYPE {
            std::cmp::Ordering::Less
        } else if b.coin_type == SUI_TYPE {
            std::cmp::Ordering::Greater
        } else {
            amount(b).partial_cmp(&amount(a)).unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    if !caps.is_empty() {
        progress("Reading kiosks");
    }
    let mut kiosks = Vec::new();
    for (kiosk, cap, pkg) in &caps {
        kiosks.push(read_kiosk(c, kiosk, cap, pkg.as_deref()).await?);
    }
    staked.sort_by(|a: &RescueStake, b| b.principal.cmp(&a.principal));
    Ok(RescueScan { address: owner, default_name, coins, nfts, kiosks, staked, names, soulbound, truncated })
}

/// Plain-words reason an owned object can't be moved by its owner.
pub fn stuck_reason(ty: &str) -> &'static str {
    if has_name(ty, "suiplay_nft", "SuiPlay0x1PreorderNFT") {
        "Made to stay with the wallet it was sent to. Claim anything it unlocks from this wallet first."
    } else if has_name(ty, "personal_kiosk", "PersonalKioskCap") {
        "A personal kiosk stays with its owner. Items inside are moved one by one."
    } else if is_type(ty, "0x2", "token", "Token") {
        "A closed-loop token: only its app can send it."
    } else {
        "Its app doesn't allow transfers. If it holds something of value, withdraw it in that app."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_types() {
        assert_eq!(coin_type_of("0x2::coin::Coin<0x2::sui::SUI>").as_deref(), Some(SUI_TYPE));
        assert!(coin_type_of("0x2::kiosk::Kiosk").is_none());
        assert!(is_type("0x2::kiosk::KioskOwnerCap", "0x2", "kiosk", "KioskOwnerCap"));
        assert_eq!(pretty("SuiFren_nft"), "Sui Fren nft");
        assert_eq!(big(Some(&serde_json::json!({"value": "42"}))), 42);
        let cap = serde_json::json!({"cap": {"vec": [{"for": "0xabc"}]}});
        assert_eq!(personal_inner(Some(&cap)).as_deref(), Some("0xabc"));
        assert!(stuck_reason("0x2::token::Token<0x1::a::B>").contains("closed-loop"));
    }
}
