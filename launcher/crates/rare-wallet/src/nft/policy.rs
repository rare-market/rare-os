//! Transfer policies (`tx/nft/policy.ts`): pick a policy whose rules
//! can all be satisfied and resolve a TransferRequest before confirming it. The rule registry is
//! Drip's rules.json, read from the TS tree so one copy stays the truth.

use std::sync::OnceLock;

use rare_sui::ptb::{Arg, Tx};
use serde::Deserialize;
use serde_json::Value;

use super::ids::{KIOSK_EXT_PACKAGE, KIOSK_EXT_VERSIONS};
use super::kiosk::{Deferred, KioskSession};
use super::{build_err, NftSendError, PolicyRule, TransferPolicyInfo};

const RULES_JSON: &str = include_str!("../../../../src/wallet/tx/nft/rules.json");

/// A rule the builders know how to prove.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct RuleSpec {
    pub id: String,
    pub module: String,
    #[serde(default)]
    pub r#struct: Option<String>,
    pub packages: Vec<String>,
}

/// A rule no transaction can satisfy, and why.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct UnsupportedRuleSpec {
    pub module: String,
    #[serde(default)]
    pub r#struct: Option<String>,
    pub packages: Vec<String>,
    pub reason: String,
}

#[derive(Deserialize)]
struct Registry {
    rules: Vec<RuleSpec>,
    unsupported: Vec<UnsupportedRuleSpec>,
}

fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| serde_json::from_str(RULES_JSON).expect("rules.json"))
}

/// The rules the builders prove.
pub fn rules() -> &'static [RuleSpec] {
    &registry().rules
}

/// The rules known to block every transfer.
pub fn unsupported_rules() -> &'static [UnsupportedRuleSpec] {
    &registry().unsupported
}

fn norm_pkg(x: &str) -> String {
    let l = x.to_lowercase();
    let h = l.strip_prefix("0x").unwrap_or(&l);
    format!("0x{h:0>64}")
}

fn same_package(a: &str, b: &str) -> bool {
    norm_pkg(a) == norm_pkg(b)
}

fn is_mysten(pkg: &str) -> bool {
    KIOSK_EXT_VERSIONS.iter().any(|v| same_package(v, pkg))
}

/// Package, module and struct (without type arguments) of a rule type string.
pub fn parse_rule_type(rule_type: &str) -> Option<(String, String, String)> {
    let mut it = rule_type.split("::");
    let pkg = it.next().unwrap_or("");
    let module = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");
    if pkg.is_empty() || module.is_empty() {
        return None;
    }
    Some((norm_pkg(pkg), module.to_string(), rest.split('<').next().unwrap_or("").to_string()))
}

/// A policy rule matched to the registry.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchedRule {
    pub spec: RuleSpec,
    pub pkg: String,
    /// Package to call: the latest Mysten version for Mysten rules, else the defining package.
    pub call_pkg: String,
    pub config: Value,
}

/// The registry entry for a rule, or None when it can't be satisfied.
pub fn match_rule(rule: &PolicyRule) -> Option<MatchedRule> {
    let config = rule.config.clone().filter(|v| !v.is_null()).unwrap_or_else(|| Value::Object(Default::default()));
    if let Some((pkg, module, st)) = parse_rule_type(rule.rule_type.as_deref().unwrap_or("")) {
        let spec = rules().iter().find(|r| {
            r.module == module && (!matches!(r.r#struct.as_deref(), Some(x) if !x.is_empty()) || st.is_empty() || r.r#struct.as_deref() == Some(st.as_str())) && r.packages.iter().any(|p| same_package(p, &pkg))
        })?;
        let call_pkg = if is_mysten(&pkg) { KIOSK_EXT_PACKAGE.to_string() } else { pkg.clone() };
        return Some(MatchedRule { spec: spec.clone(), pkg, call_pkg, config });
    }
    let spec = rules().iter().find(|r| r.id == rule.type_ && r.packages.iter().any(|p| is_mysten(p)))?;
    Some(MatchedRule { spec: spec.clone(), pkg: KIOSK_EXT_PACKAGE.to_string(), call_pkg: KIOSK_EXT_PACKAGE.to_string(), config })
}

/// Why a rule can't be satisfied, in plain words.
pub fn unsupported_reason(rule: &PolicyRule) -> String {
    let parsed = parse_rule_type(rule.rule_type.as_deref().unwrap_or(""));
    let known = parsed.as_ref().and_then(|(pkg, module, _)| unsupported_rules().iter().find(|u| &u.module == module && u.packages.iter().any(|p| same_package(p, pkg))));
    if let Some(k) = known {
        return k.reason.clone();
    }
    let name = match &parsed {
        Some((_, module, st)) => format!("{module}::{st}"),
        None => rule.type_.clone(),
    };
    format!("{name} is a custom rule with no way to satisfy it")
}

/// What a policy asks of a transfer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolicyNeeds {
    pub lock: bool,
    pub personal: bool,
    pub royalty: bool,
    /// The floor price in MIST (`bigint` in TS; a config that does not parse is 0).
    pub floor: Option<i128>,
    pub rules: Vec<MatchedRule>,
    pub unsupported: Vec<String>,
}

/// TS `bigintOf`: numbers floor, strings parse as BigInt literals, anything else is the fallback.
fn bigint_of(v: Option<&Value>) -> i128 {
    match v {
        None | Some(Value::Null) => 0,
        Some(Value::Number(n)) => n.as_i64().map(i128::from).or_else(|| n.as_u64().map(i128::from)).or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f.floor() as i128)).unwrap_or(0),
        Some(Value::String(s)) => bigint_str(s).unwrap_or(0),
        Some(Value::Bool(_)) | Some(Value::Array(_)) | Some(Value::Object(_)) => 0,
    }
}

/// `BigInt(string)`: trimmed; empty is 0; decimal with an optional sign, or 0x/0o/0b without one.
fn bigint_str(s: &str) -> Option<i128> {
    let t = super::limits::js_trim(s);
    if t.is_empty() {
        return Some(0);
    }
    let lower = t.to_ascii_lowercase();
    for (p, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(d) = lower.strip_prefix(p) {
            return if !d.is_empty() && d.chars().all(|c| c.is_digit(radix)) { i128::from_str_radix(d, radix).ok() } else { None };
        }
    }
    let (neg, d) = match t.as_bytes()[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: i128 = d.parse().ok()?;
    Some(if neg { -v } else { v })
}

/// `config.floor_price ?? config.floorPrice ?? config.floor ?? config.price`.
fn floor_of(config: &Value) -> i128 {
    let v = ["floor_price", "floorPrice", "floor", "price"].iter().map(|k| config.get(*k)).find(|v| v.is_some_and(|v| !v.is_null())).flatten();
    bigint_of(v)
}

pub fn policy_needs(policy: Option<&TransferPolicyInfo>) -> PolicyNeeds {
    let mut needs = PolicyNeeds::default();
    for rule in policy.map(|p| p.rules.as_slice()).unwrap_or_default() {
        let Some(m) = match_rule(rule) else {
            let name = rule.rule_type.clone().filter(|s| !s.is_empty()).or_else(|| Some(rule.type_.clone()).filter(|s| !s.is_empty())).unwrap_or_else(|| "unknown".into());
            needs.unsupported.push(name);
            continue;
        };
        match m.spec.id.as_str() {
            "royalty_rule" | "royalty_rule_copy" => needs.royalty = true,
            "kiosk_lock_rule" | "kiosk_lock_rule_copy" => needs.lock = true,
            "personal_kiosk_rule" => needs.personal = true,
            "floor_price_rule" => needs.floor = Some(floor_of(&m.config)),
            _ => {}
        }
        needs.rules.push(m);
    }
    needs
}

pub fn is_policy_supported(policy: &TransferPolicyInfo) -> bool {
    policy.is_shared != Some(false) && policy_needs(Some(policy)).unsupported.is_empty()
}

fn policy_cost(policy: &TransferPolicyInfo) -> usize {
    let n = policy_needs(Some(policy));
    n.rules.len() * 10_000 + if n.personal { 5_000 } else { 0 } + if n.lock { 1_000 } else { 0 } + if n.royalty { 100 } else { 0 }
}

/// Among the shared policies whose rules can all be resolved, the cheapest (first on a tie).
pub fn pick_policy(policies: Option<&[TransferPolicyInfo]>) -> Option<&TransferPolicyInfo> {
    let list = policies.unwrap_or_default().iter().filter(|p| !p.id.is_empty() && is_policy_supported(p));
    // min_by_key keeps the first of equal costs, like the TS stable sort
    list.min_by_key(|p| policy_cost(p))
}

/// Why a locked item can't be moved under these policies, or None when one of them works.
pub fn policy_problem(policies: Option<&[TransferPolicyInfo]>) -> Option<String> {
    let shared: Vec<TransferPolicyInfo> = policies.unwrap_or_default().iter().filter(|p| !p.id.is_empty() && p.is_shared != Some(false)).cloned().collect();
    if shared.is_empty() {
        return Some("No transfer policy found for this collection, so a locked item can not leave its kiosk".into());
    }
    if pick_policy(Some(&shared)).is_some() {
        return None;
    }
    let mut reasons: Vec<String> = Vec::new();
    for p in &shared {
        for r in &p.rules {
            if match_rule(r).is_none() {
                let why = unsupported_reason(r);
                if !reasons.contains(&why) {
                    reasons.push(why);
                }
            }
        }
    }
    Some(format!("The collection's transfer rules block it: {}", reasons.join("; ")))
}

/// What [`resolve_transfer_request`] works with.
pub(crate) struct ResolveRequest<'a> {
    pub nft_type: &'a str,
    pub policy: &'a TransferPolicyInfo,
    pub request: Arg,
    pub item: Arg,
    /// Kiosk that receives the item (a session handle). Required for lock and personal rules.
    pub kiosk: Option<usize>,
    pub recipient: &'a str,
    /// Put the item into `kiosk` even when no rule requires it.
    pub prefer_kiosk: bool,
}

/// Deliver the item, satisfy every rule on the policy, then confirm the request. A personal kiosk
/// created in this transaction proves (and confirms) only once `finish` has made it personal.
pub(crate) fn resolve_transfer_request(tx: &mut Tx, session: &mut KioskSession, o: ResolveRequest<'_>) -> Result<(), NftSendError> {
    let needs = policy_needs(Some(o.policy));
    if !needs.unsupported.is_empty() {
        return Err(build_err(format!("Transfer policy {} has rules that can't be resolved: {}", o.policy.id, needs.unsupported.join(", "))));
    }
    let policy_arg = tx.object(&o.policy.id);
    let nft = o.nft_type;

    if needs.lock || needs.personal || (o.prefer_kiosk && o.kiosk.is_some()) {
        let Some(h) = o.kiosk else { return Err(build_err("This collection requires the item to go into a kiosk")) };
        let k = session.handle(h).clone();
        if needs.personal && !k.is_personal {
            return Err(build_err("This collection requires a personal kiosk"));
        }
        if needs.lock {
            tx.move_call("0x2::kiosk::lock", &[nft], &[k.kiosk, k.cap, policy_arg, o.item]);
        } else {
            tx.move_call("0x2::kiosk::place", &[nft], &[k.kiosk, k.cap, o.item]);
        }
        session.handle_mut(h).used = true;
    } else {
        tx.transfer_to(&[o.item], o.recipient);
    }

    let mut deferred = false;
    for rule in &needs.rules {
        let pkg = rule.call_pkg.as_str();
        let kiosk = o.kiosk.map(|h| session.handle(h).clone());
        match rule.spec.id.as_str() {
            "royalty_rule" | "royalty_rule_copy" => {
                // royalty computed on chain with fee_amount(policy, paid), so the split is exact
                let paid = tx.move_call("0x2::transfer_policy::paid", &[nft], &[o.request]);
                let fee = tx.move_call(&format!("{pkg}::royalty_rule::fee_amount"), &[nft], &[policy_arg, paid]);
                let fee_coin = tx.split_coins(Arg::Gas, &[fee]).nth(0);
                tx.move_call(&format!("{pkg}::royalty_rule::pay"), &[nft], &[policy_arg, o.request, fee_coin]);
            }
            "kiosk_lock_rule" | "kiosk_lock_rule_copy" => {
                let k = kiosk.ok_or_else(|| build_err("This collection requires the item to go into a kiosk"))?;
                tx.move_call(&format!("{pkg}::kiosk_lock_rule::prove"), &[nft], &[o.request, k.kiosk]);
            }
            "personal_kiosk_rule" => {
                let k = kiosk.ok_or_else(|| build_err("This collection requires a personal kiosk"))?;
                if k.defers_personal() {
                    deferred = true;
                    let d = Deferred { call_pkg: pkg.to_string(), nft_type: nft.to_string(), policy: policy_arg, request: o.request };
                    if let Some(list) = session.handle_mut(o.kiosk.unwrap_or_default()).deferred.as_mut() {
                        list.push(d);
                    }
                    continue;
                }
                tx.move_call(&format!("{pkg}::personal_kiosk_rule::prove"), &[nft], &[k.kiosk, o.request]);
            }
            "floor_price_rule" => {
                tx.move_call(&format!("{pkg}::floor_price_rule::prove"), &[nft], &[policy_arg, o.request]);
            }
            "staking_lock_rule" => {
                let reg = ["registry_id", "registryId"].iter().filter_map(|k| rule.config.get(*k)).find(|v| !v.is_null()).cloned();
                let reg = match reg {
                    Some(Value::String(s)) if !s.is_empty() => s,
                    _ => return Err(build_err("staking_lock_rule config is missing registry_id")),
                };
                let r = tx.object(&reg);
                tx.move_call(&format!("{pkg}::staking_lock_rule::prove"), &[], &[policy_arg, o.request, r]);
            }
            other => return Err(build_err(format!("No resolver for {other}"))),
        }
    }
    if !deferred {
        tx.move_call("0x2::transfer_policy::confirm_request", &[nft], &[policy_arg, o.request]);
    }
    Ok(())
}
