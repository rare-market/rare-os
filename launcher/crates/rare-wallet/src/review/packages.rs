//! Known protocols, short ids and readable object names (ports `review/packages.ts`).

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Original (v1) package ids of well-known protocols, checked on mainnet 2026-10-24. Upgraded
/// packages resolve to these through `Simulation::packages` (id -> original id).
const KNOWN: &[(&str, &str)] = &[
    ("0x1", "Move stdlib"),
    ("0x2", "Sui"),
    ("0x3", "Sui system"),
    // Rare's own contracts (rare.market's review names them the same way)
    ("0x465753839605ea682f147cfe5db736c7e8718798d64cd8f0fe7bd60869d64b03", "Rare"),
    ("0x94b8e1cf51652ea35680a55cb8a7b1a16978f2ba1dc15e0c93ed7536d7659475", "Rare"),
    ("0x9252bf93053dcd0cd5ad0562303e66d6a5526d6022453b88c00dc463a012a7bf", "Rare"),
    ("0xda09fa9db4a54d4b0c337fb7ee41311f4679a7f6049e58372a4951968a32f73c", "Rare"),
    ("0xdee9", "DeepBook"),
    ("0x2c8d603bc51326b8c13cef9dd07031a408a48dddb541963357661df5d3204809", "DeepBook"),
    ("0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb", "Cetus"),
    ("0x91bfbc386a41afcfd9b2533058d7e915a1d3829089cc268ff4333d54d6339ca1", "Turbos"),
    ("0x3492c874c1e3b3e2984e8c41b589e642d4d0a5d6459e5a9cfc2d52fd7c89c267", "Bluefin"),
    ("0xefe170ec0be4d762196bedecd7a065816576198a6527c99282a2551aaa7da38c", "Aftermath"),
    ("0xa0eba10b173538c8fecca1dff298e488402cc9ff374f8a12ca7758eebe830b66", "Kriya"),
    ("0xba153169476e8c3114962261d1edc70de5ad9781b83cc617ecc8c1923191cae0", "FlowX"),
    ("0xf95b06141ed4a174f239417323bde3f209b972f5930d8521ea38a52aff3a6ddf", "Suilend"),
    ("0xefe8b36d5b2e43728cc323298626b83177803521d195cfb11e15b910e892fddf", "Scallop"),
    ("0xd899cf7d2b5db716bd2cf55599fb0d5ee38a3061e7b6bb6eebf73fa5bc4c81ca", "NAVI"),
    ("0xd22b24490e0bae52676651b4f56660a5ff8022a2576e0089f79b3c88d44e08f0", "SuiNS"),
    // Mysten kiosk extensions (royalty, lock, personal kiosk rules), original id and the v4 we call
    ("0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879", "Kiosk"),
    ("0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3", "Kiosk"),
    // NFT marketplaces Rare Marketplace buys from (rare.market packages/venues/src/ids.ts), origin and latest ids
    ("0xb42dbb7413b79394e1a0175af6ae22b69a5c7cc5df259cd78072b6818217c027", "Tradeport"),
    ("0xd85a131b82ef9f4aa5eb99a0ea758a3f209a8771eba7e9402e8181273734bf9a", "Tradeport"),
    ("0xec175e537be9e48f75fa6929291de6454d2502f1091feb22c0d26a22821bbf28", "Tradeport"),
    ("0xafc035fc8e321da96a2cb1bb88cec64ef6bf7c83483653c259184312b3c0bb96", "Tradeport"),
    ("0xf938f27ce9460838643ac04fac05254f73329caf59643b7843c883bb4fa7402f", "Tradeport"),
    ("0x53134eb544c5a0b5085e99efaf7eab13b28ad123de35d61f941f8c8c40b72033", "Tradeport"),
    ("0xe2680c552bd3a5540c2f2986e92927ef2ebb6308e41ae395a1cec24e51d2d951", "Tradeport"),
    ("0xff2251ea99230ed1cbe3a347a209352711c6723fcdcd9286e16636e65bb55cab", "Tradeport"),
    ("0x81e16ff388eacc2908f4e8248f54d140eb6d7834ff01dc7a86042a3b701b68f6", "Tradeport"),
    ("0x06cefebb11191abd347654200a6fd8ae47fa97b54506e4e680db64dae54d0b12", "Tradeport"),
    ("0xe50387176587c707c428c537b7b8fb530c3eaeadd16a45b1d975369f372dbc0a", "Drip"),
    ("0x7dcbc405496f4f6ecb70921b571ea9bfca09db7004efddd3dad33a524e7489e4", "Drip"),
    ("0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802", "Kiosk"),
    ("0x34cc6762780f4f6f153c924c0680cfe2a1fb4601e7d33cc28a92297b62de1e0e", "Kiosk"),
];

fn by_id() -> &'static HashMap<String, &'static str> {
    static M: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    M.get_or_init(|| KNOWN.iter().map(|(k, v)| (norm_id(k), *v)).collect())
}

/// `normalizeSuiAddress`: lower case, `0x`, left-padded to 64 characters. Never fails.
pub fn norm_id(id: &str) -> String {
    let a = id.to_lowercase();
    let h = a.strip_prefix("0x").unwrap_or(&a);
    let pad = 64usize.saturating_sub(h.encode_utf16().count());
    format!("0x{}{h}", "0".repeat(pad))
}

/// `0x1eab…b2fb`; framework ids stay `0x2`.
pub fn short_id(id: &str) -> String {
    let n = norm_id(id);
    let rest = n[2..].trim_start_matches('0');
    let trimmed = format!("0x{rest}");
    if trimmed.chars().count() <= 6 {
        return if trimmed == "0x" { "0x0".into() } else { trimmed };
    }
    let c: Vec<char> = n.chars().collect();
    let head: String = c[..6.min(c.len())].iter().collect();
    let tail: String = c[c.len().saturating_sub(4)..].iter().collect();
    format!("{head}\u{2026}{tail}")
}

/// Protocol name for a package (or its original id), else None.
pub fn known_package(id: &str, originals: Option<&BTreeMap<String, String>>) -> Option<&'static str> {
    let n = norm_id(id);
    if let Some(p) = by_id().get(&n) {
        return Some(p);
    }
    let orig = originals.and_then(|o| o.get(&n).or_else(|| o.get(id))).filter(|s| !s.is_empty())?;
    by_id().get(&norm_id(orig)).copied()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedType {
    pub pkg: String,
    pub module: String,
    pub name: String,
}

/// `0xabc::pool::Pool<A, B>` -> { pkg, module, name }; None for primitives.
pub fn parse_type(t: Option<&str>) -> Option<ParsedType> {
    let t = t.filter(|t| !t.is_empty())?;
    let base = t.split('<').next().unwrap_or("");
    let parts: Vec<&str> = base.split("::").collect();
    if parts.len() != 3 {
        return None;
    }
    Some(ParsedType { pkg: norm_id(parts[0]), module: parts[1].into(), name: parts[2].into() })
}

/// `/^0x0*2::<rest>/`
pub(crate) fn is_framework_prefixed(t: &str, rest: &str) -> bool {
    t.strip_prefix("0x").is_some_and(|r| r.trim_start_matches('0').starts_with(&format!("2::{rest}")))
}

/// Coin<T> objects: covered by balance changes, never listed as objects.
pub fn is_coin_object(t: Option<&str>) -> bool {
    t.is_some_and(|t| is_framework_prefixed(t, "coin::Coin<"))
}

/// `T` out of `0x2::coin::Coin<T>`.
pub fn coin_type_of(t: &str) -> String {
    let s = if is_coin_object(Some(t)) { &t[t.find('<').map(|i| i + 1).unwrap_or(0)..] } else { t };
    s.strip_suffix('>').unwrap_or(s).to_string()
}

/// `StakedSui` -> `Staked Sui`, `PositionNFT` stays readable.
pub fn humanize(name: &str) -> String {
    let c: Vec<char> = name.chars().map(|c| if c == '_' { ' ' } else { c }).collect();
    let mut out = String::with_capacity(c.len() + 4);
    let mut i = 0;
    while i < c.len() {
        if c[i].is_ascii_lowercase() && c.get(i + 1).is_some_and(|n| n.is_ascii_uppercase()) {
            out.push(c[i]);
            out.push(' ');
            out.push(c[i + 1]);
            i += 2;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    // /\s+/g -> ' ', then trim
    let mut collapsed = String::with_capacity(out.len());
    let mut in_space = false;
    for ch in out.chars() {
        if ch.is_whitespace() || ch == '\u{feff}' {
            if !in_space {
                collapsed.push(' ');
            }
            in_space = true;
        } else {
            collapsed.push(ch);
            in_space = false;
        }
    }
    collapsed.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}').to_string()
}

/// A readable name for an object type and its second line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub title: String,
    pub sub: String,
}

/// "Cetus Pool", or `0x1eab…b2fb::pool` for unknown packages.
pub fn object_label(object_type: Option<&str>, originals: Option<&BTreeMap<String, String>>) -> Label {
    let Some(p) = parse_type(object_type) else { return Label { title: "Object".into(), sub: String::new() } };
    let name = humanize(&p.name);
    match known_package(&p.pkg, originals) {
        Some(proto) => Label {
            title: if name.to_lowercase().starts_with(&proto.to_lowercase()) { name } else { format!("{proto} {name}") },
            sub: format!("{}::{}", p.module, p.name),
        },
        None => Label { title: name, sub: format!("{}::{}", short_id(&p.pkg), p.module) },
    }
}
