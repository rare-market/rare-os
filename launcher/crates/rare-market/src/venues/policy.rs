//! Transfer policies: which rules the builders can prove, reading a policy's royalty, lock,
//! personal-kiosk and floor rules, and picking the policy to trade through. Ported from
//! launcher/src/marketplace/venues/policy.ts (rare.market packages/venues/src/policy.ts, json.ts
//! normType, web/lib/chain/policy.ts and rules.json).

use super::ids::{kiosk_ext, CLOCK};
use super::model::{PolicyRule, TransferPolicy};
use super::{BuildError, VenueConfig};
use crate::ptb::{Arg, Tx};
use serde_json::{Map, Value};

/// Type strings with full-length addresses, as the node prints them: every `0x<hex>` directly
/// followed by `::` is padded to 64 lowercase digits.
pub fn norm_type(t: &str) -> String {
    let b = t.as_bytes();
    let mut out = String::with_capacity(t.len() + 64);
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'0' && i + 1 < b.len() && b[i + 1] == b'x' {
            let start = i + 2;
            let mut j = start;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            let len = j - start;
            if (1..=64).contains(&len) && t[j..].starts_with("::") {
                out.push_str("0x");
                out.push_str(&format!("{:0>64}", t[start..j].to_ascii_lowercase()));
                i = j;
                continue;
            }
        }
        let c = t[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn rule(origin: &str, m: &str) -> String {
    norm_type(&format!("{origin}::{m}::Rule"))
}

/// The Mysten rule types the builders prove (rule types carry the id of the extension version that
/// introduced them).
pub struct Rules {
    pub royalty: String,
    pub kiosk_lock: String,
    pub personal_kiosk: String,
    pub floor_price: String,
}

pub fn rules() -> Rules {
    Rules {
        royalty: rule(kiosk_ext::ORIGIN, "royalty_rule"),
        kiosk_lock: rule(kiosk_ext::ORIGIN, "kiosk_lock_rule"),
        personal_kiosk: rule(kiosk_ext::PERSONAL_ORIGIN, "personal_kiosk_rule"),
        floor_price: rule(kiosk_ext::FLOOR_ORIGIN, "floor_price_rule"),
    }
}

pub(crate) fn norm_pkg(x: &str) -> String {
    let l = x.to_lowercase();
    let h = l.strip_prefix("0x").unwrap_or(&l);
    format!("0x{h:0>64}")
}

/// Rare Store: the `<origin>::resale::Rule` on TransferPolicy<ResaleLicence>. `origin` is the
/// package the rule type was defined in, `pkg` the current package that is called. Empty until the
/// store config is known.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resale {
    pub origin: String,
    pub pkg: String,
}

impl Resale {
    /// `configureResale`: ids normalised; missing ones leave the current value.
    pub fn configure(&mut self, origin: Option<&str>, pkg: Option<&str>) {
        if let Some(o) = origin.filter(|s| !s.is_empty()) {
            self.origin = norm_pkg(o);
        }
        if let Some(p) = pkg.filter(|s| !s.is_empty()) {
            self.pkg = norm_pkg(p);
        }
    }
    /// The resale rule type, or None while the store is not configured.
    pub fn rule(&self) -> Option<String> {
        (!self.origin.is_empty()).then(|| norm_type(&format!("{}::resale::Rule", self.origin)))
    }
    /// The ResaleLicence Move type, or None while the store is not configured.
    pub fn licence_type(&self) -> Option<String> {
        (!self.origin.is_empty()).then(|| norm_type(&format!("{}::licence::ResaleLicence", self.origin)))
    }
}

pub fn is_known_rule(cfg: &VenueConfig, r: &str) -> bool {
    let k = rules();
    [k.royalty, k.kiosk_lock, k.personal_kiosk, k.floor_price].iter().any(|x| x == r) || cfg.resale.rule().as_deref() == Some(r)
}

/// rare_store::resale::prove(game, req, kiosk, cap, pay, clock): hold check and royalty. Before
/// confirm_request.
pub fn add_resale_prove(tx: &mut Tx, cfg: &VenueConfig, game: Option<&str>, req: Arg, kiosk: Arg, cap: Arg, pay_from: Arg) -> Result<(), BuildError> {
    let game = game.ok_or_else(|| BuildError::Other("resale licence: the game id is needed to prove the resale rule".into()))?;
    if cfg.resale.pkg.is_empty() {
        return Err(BuildError::Other("resale licence: Rare Store package not configured".into()));
    }
    let g = tx.object(game);
    let clock = tx.object(CLOCK);
    tx.move_call(&format!("{}::resale::prove", cfg.resale.pkg), &[], &[g, req, kiosk, cap, pay_from, clock]);
    Ok(())
}

/// The builder's view of a policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub policy_id: String,
    pub nft_type: String,
    pub rules: Vec<String>,
    /// Rules no builder knows how to prove: such policies cannot be traded through.
    pub unknown_rules: Vec<String>,
    pub royalty: Option<Royalty>,
    pub floor: Option<u128>,
    pub shared_version: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Royalty {
    pub bps: u128,
    pub min: u128,
}

// ---------- web/lib/chain/policy.ts: picking the policy from /v1/policies ----------

struct RuleEntry {
    id: &'static str,
    module: &'static str,
    packages: &'static [&'static str],
}

const MYSTEN: &[&str] = &kiosk_ext::ALL;
const ROYALTY_COPIES: &[&str] = &[
    "0x1f015c1a000631f2c0afd0258b299c63ffb8e5452e0c0f9be8fc28d2e20ad9b4",
    "0x93195daadbc4f26c0c498f4ceac92593682d2325ce3a0f5ba9f2db3b6a9733dd",
    "0x95bde20afc7cc60458d8360c87d4ed1f136dd0fee64828bb7b613aea2d98e9ff",
    "0xd441d82fa791d7e7fc89eb2a40b0714bd9a6a1aaf0c897d702802d30109c1f7b",
    "0xf74aa26054e5db492f93d88bce836466a799b70eae200bc254b5e3c9ed0cea8b",
    "0x05cf6401224b471a2489e0b13a0a23363378e89a0f14b6c58ffe9f71d1e36bc8",
    "0x34f9e32641795c57d936b3e387b9c7c941fd99b5a8a90e86b61e966e9da1d1f0",
    "0xd71803e11220094f86f3df6cd0c13655bf46f6d617f1bda40180f0e5b557bca3",
];
const LOCK_COPIES: &[&str] = &[
    "0x1f015c1a000631f2c0afd0258b299c63ffb8e5452e0c0f9be8fc28d2e20ad9b4",
    "0x93195daadbc4f26c0c498f4ceac92593682d2325ce3a0f5ba9f2db3b6a9733dd",
    "0x95bde20afc7cc60458d8360c87d4ed1f136dd0fee64828bb7b613aea2d98e9ff",
    "0xf74aa26054e5db492f93d88bce836466a799b70eae200bc254b5e3c9ed0cea8b",
];
const STAKING_LOCK: &[&str] = &[
    "0x3ab9fef3dcf219a23355fa0e3d4f7d9cdd421ddba4e3b2cb566543d4a10b4c38",
    "0x2e4b6da7ccda3acd22a89d07005f34857e38776f22a64183e819b2650fa3cb71",
    "0x515a552652d3d7cb4e219772ba78e17997bf7e2ffd9502a0275750574bfacdc8",
];

/// rules.json `rules`: every rule type (by defining package) the app can satisfy.
const REGISTRY: &[RuleEntry] = &[
    RuleEntry { id: "royalty_rule", module: "royalty_rule", packages: MYSTEN },
    RuleEntry { id: "royalty_rule_copy", module: "royalty_rule", packages: ROYALTY_COPIES },
    RuleEntry { id: "kiosk_lock_rule", module: "kiosk_lock_rule", packages: MYSTEN },
    RuleEntry { id: "kiosk_lock_rule_copy", module: "kiosk_lock_rule", packages: LOCK_COPIES },
    RuleEntry { id: "personal_kiosk_rule", module: "personal_kiosk_rule", packages: MYSTEN },
    RuleEntry { id: "floor_price_rule", module: "floor_price_rule", packages: MYSTEN },
    RuleEntry { id: "staking_lock_rule", module: "staking_lock_rule", packages: STAKING_LOCK },
];

fn same_pkg(a: &str, b: &str) -> bool {
    norm_pkg(a) == norm_pkg(b)
}

fn is_mysten(pkg: &str) -> bool {
    MYSTEN.iter().any(|v| same_pkg(v, pkg))
}

/// `pkg::module::Struct<...>` split; None without a module.
fn split_rule_type(rule_type: &str) -> Option<(String, String, String)> {
    let mut it = rule_type.split("::");
    let pkg = it.next().filter(|s| !s.is_empty())?;
    let module = it.next().filter(|s| !s.is_empty())?;
    let rest = it.next().unwrap_or("");
    Some((norm_pkg(pkg), module.to_string(), rest.split('<').next().unwrap_or("").to_string()))
}

fn recognise<'a>(cfg: &VenueConfig, r: &'a PolicyRule) -> Option<(&'static str, Option<&'a Map<String, Value>>)> {
    let parsed = split_rule_type(r.rule_type.as_deref().unwrap_or(""));
    if let Some((pkg, module, st)) = &parsed {
        if !cfg.resale.origin.is_empty() && module == "resale" && st == "Rule" && same_pkg(pkg, &cfg.resale.origin) {
            return Some(("resale_rule", r.config.as_ref()));
        }
        let e = REGISTRY.iter().find(|e| e.module == module && (st.is_empty() || st == "Rule") && e.packages.iter().any(|p| same_pkg(p, pkg)));
        return e.map(|e| (e.id, r.config.as_ref()));
    }
    // Payloads without a rule type: trust only the Mysten rule names.
    let e = REGISTRY.iter().find(|e| e.id == r.kind && e.packages.iter().any(|p| is_mysten(p)));
    e.map(|e| (e.id, r.config.as_ref()))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShapeRoyalty {
    pub bps: u128,
    pub min: u128,
    pub mysten: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyShape {
    pub lock: bool,
    pub personal: bool,
    pub royalty: Option<ShapeRoyalty>,
    pub floor: Option<u128>,
    pub unsupported: Vec<String>,
    pub known: u32,
}

/// `BigInt(v)` with a fallback: null, "" and anything unparsable give `fallback`; numbers floor.
pub(crate) fn big_of(v: Option<&Value>, fallback: u128) -> u128 {
    match v {
        None | Some(Value::Null) => fallback,
        Some(Value::Number(n)) => n.as_u64().map(u128::from).or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f.floor() as u128)).unwrap_or(fallback),
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                // BigInt("") is 0n, but the TS helper checks "" first and answers the fallback.
                return fallback;
            }
            match s.strip_prefix("0x") {
                Some(h) => u128::from_str_radix(h, 16).unwrap_or(fallback),
                None => s.parse::<u128>().unwrap_or(fallback),
            }
        }
        Some(Value::Bool(b)) => *b as u128,
        _ => fallback,
    }
}

fn cfg_get<'a>(cfg: Option<&'a Map<String, Value>>, keys: &[&str]) -> Option<&'a Value> {
    // `a ?? b ?? c`: the first key present and not null.
    let c = cfg?;
    keys.iter().find_map(|k| c.get(*k).filter(|v| !v.is_null()))
}

pub fn read_policy(cfg: &VenueConfig, policy: Option<&TransferPolicy>) -> PolicyShape {
    let mut shape = PolicyShape::default();
    for r in policy.map(|p| p.rules.as_slice()).unwrap_or(&[]) {
        let Some((id, c)) = recognise(cfg, r) else {
            let name = r.rule_type.clone().filter(|s| !s.is_empty()).or_else(|| Some(r.kind.clone()).filter(|s| !s.is_empty())).unwrap_or_else(|| "unknown".into());
            shape.unsupported.push(name);
            continue;
        };
        shape.known += 1;
        match id {
            "royalty_rule" | "royalty_rule_copy" => {
                let roy = ShapeRoyalty { bps: big_of(cfg_get(c, &["amount_bp", "amountBp"]), 0), min: big_of(cfg_get(c, &["min_amount", "minAmount"]), 0), mysten: id == "royalty_rule" };
                if shape.royalty.is_none() || roy.mysten {
                    shape.royalty = Some(roy);
                }
            }
            "kiosk_lock_rule" | "kiosk_lock_rule_copy" => shape.lock = true,
            "personal_kiosk_rule" => shape.personal = true,
            "floor_price_rule" => shape.floor = Some(big_of(cfg_get(c, &["floor_price", "floorPrice", "price"]), 0)),
            _ => {}
        }
    }
    shape
}

fn can_satisfy(cfg: &VenueConfig, p: &TransferPolicy) -> bool {
    p.is_shared != Some(false) && read_policy(cfg, Some(p)).unsupported.is_empty()
}

fn cost(cfg: &VenueConfig, p: &TransferPolicy) -> u128 {
    let s = read_policy(cfg, Some(p));
    s.known as u128 * 10_000 + if s.personal { 5_000 } else { 0 } + if s.lock { 1_000 } else { 0 } + s.royalty.map(|r| r.bps).unwrap_or(0)
}

/// The cheapest shared policy whose rules we can all resolve.
pub fn choose_policy<'a>(cfg: &VenueConfig, policies: Option<&'a [TransferPolicy]>) -> Option<&'a TransferPolicy> {
    let mut ok: Vec<(usize, &TransferPolicy)> = policies.unwrap_or(&[]).iter().enumerate().filter(|(_, p)| !p.id.is_empty() && can_satisfy(cfg, p)).collect();
    // A stable sort keeps the API's order among equal costs.
    ok.sort_by_key(|(i, p)| (cost(cfg, p), *i));
    ok.first().map(|(_, p)| *p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_normalise_only_before_double_colons() {
        assert_eq!(norm_type("0x2::kiosk::Kiosk"), format!("0x{}2::kiosk::Kiosk", "0".repeat(63)));
        assert_eq!(norm_type("0xAB::m::T<0x2::sui::SUI>"), format!("0x{}ab::m::T<0x{}2::sui::SUI>", "0".repeat(62), "0".repeat(63)));
        assert_eq!(norm_type("0x12 not a type"), "0x12 not a type");
    }

    #[test]
    fn rule_copies_and_names() {
        let cfg = VenueConfig::default();
        let p = TransferPolicy {
            id: "0x1".into(),
            is_shared: Some(true),
            rules: vec![
                PolicyRule { kind: "royalty_rule".into(), rule_type: Some(format!("{}::royalty_rule::Rule", ROYALTY_COPIES[0])), config: Some(serde_json::from_str(r#"{"amountBp": 500, "minAmount": "7"}"#).unwrap()) },
                PolicyRule { kind: "kiosk_lock_rule".into(), rule_type: None, config: None },
                PolicyRule { kind: "witness_rule".into(), rule_type: None, config: None },
            ],
        };
        let s = read_policy(&cfg, Some(&p));
        assert_eq!(s.royalty, Some(ShapeRoyalty { bps: 500, min: 7, mysten: false }));
        assert!(s.lock);
        assert_eq!(s.unsupported, vec!["witness_rule".to_string()]);
        assert_eq!(s.known, 2);
        assert!(choose_policy(&cfg, Some(std::slice::from_ref(&p))).is_none());
    }
}
