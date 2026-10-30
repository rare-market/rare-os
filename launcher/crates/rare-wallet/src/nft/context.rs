//! What the send builder needs to know about the sender's NFTs (`tx/nft/context.ts` and
//! `nftSendability` in `tx/nftSend.ts`): the kiosks the wallet owns and what sits in each one
//! (`rare_sui::kiosk`), which marketplace holds a listing, and each collection's transfer policies,
//! all read from the fullnode (the Trade API only names which policy objects exist for a type). Read only,
//! never signs.

use std::collections::HashMap;

use rare_sui::grpc::{has_store, ChainRead, Include, Owner};
use rare_sui::ids::{canon_addr, normalize_struct_tag};
use rare_sui::kiosk::{dynamic_field_value, listing_market, load_kiosk_context, ListingMarket};
use rare_sui::prices::HttpGet;
use rare_sui::{BoxFut, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::policy::{match_rule, policy_problem};
use super::{KioskItemRef, Market, PolicyRule, SendItem, TransferPolicyInfo};

/// The types of the objects a transaction touched (`getTransaction(...).objectTypes`, from its
/// effects' changed objects), in the node's order. Implemented for [`rare_sui::Sui`].
pub trait TxObjectTypes: Send + Sync {
    fn tx_object_types(&self, digest: &str) -> BoxFut<'_, Vec<(String, String)>>;
}

impl TxObjectTypes for rare_sui::Sui {
    fn tx_object_types(&self, digest: &str) -> BoxFut<'_, Vec<(String, String)>> {
        let digest = digest.to_string();
        Box::pin(async move {
            let t = self.transaction(&digest, &["effects.changed_objects.object_type", "effects.changed_objects.object_id"]).await?;
            Ok(t.effects.map(|e| e.changed_objects.iter().filter_map(|c| Some((c.object_id.clone()?, c.object_type.clone()?))).collect()).unwrap_or_default())
        })
    }
}

/// A collection's transfer policies, by NFT type (the item id helps find them from its history).
pub trait PolicySource: Send + Sync {
    fn policies<'a>(&'a self, nft_type: &'a str, nft_id: &'a str) -> BoxFut<'a, Vec<TransferPolicyInfo>>;
}

/// `TransferPolicy { id: UID, balance: Balance<SUI>, rules: VecSet<TypeName> }` read as
/// (address, u64, vector<string>); None when the bytes are not that.
fn parse_policy_rules(b: &[u8]) -> Option<Vec<String>> {
    let mut at = 32 + 8;
    let uleb = |b: &[u8], at: &mut usize| -> Option<usize> {
        let mut v: usize = 0;
        let mut shift = 0;
        loop {
            let byte = *b.get(*at)?;
            *at += 1;
            v |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 28 {
                return None;
            }
        }
    };
    if b.len() < at {
        return None;
    }
    let n = uleb(b, &mut at)?;
    let mut out = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        let len = uleb(b, &mut at)?;
        let s = b.get(at..at.checked_add(len)?)?;
        at += len;
        out.push(String::from_utf8_lossy(s).into_owned());
    }
    Some(out)
}

fn hex_at(b: &[u8], at: usize) -> Option<String> {
    let s = b.get(at..at + 32)?;
    Some(canon_addr(&s.iter().map(|x| format!("{x:02x}")).collect::<String>()))
}

/// Read transfer policies by id: the objects give the rule types, and rule configs come from the
/// policy's dynamic fields when a rule needs them (floor price, staking registry). Objects that are
/// missing or are not policies are skipped.
pub async fn read_policies(chain: &dyn ChainRead, ids: &[String]) -> Result<Vec<TransferPolicyInfo>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut uniq: Vec<String> = Vec::new();
    for id in ids {
        if !uniq.contains(id) {
            uniq.push(id.clone());
        }
    }
    let objects = chain.objects(&uniq, Include::CONTENTS).await?;
    let mut out = Vec::new();
    for o in objects.into_iter().flatten() {
        let Some(content) = o.contents.as_deref() else { continue };
        let Some(raw_rules) = parse_policy_rules(content) else { continue };
        let shared = match &o.owner {
            None | Some(Owner::Unknown) => None,
            Some(Owner::Shared) | Some(Owner::ConsensusAddress { .. }) => Some(true),
            Some(_) => Some(false),
        };
        let mut rules: Vec<PolicyRule> = raw_rules
            .iter()
            .map(|r| {
                let rule_type = if r.starts_with("0x") { r.clone() } else { format!("0x{r}") };
                let type_ = rule_type.split("::").nth(1).unwrap_or(&rule_type).to_string();
                PolicyRule { type_, rule_type: Some(rule_type), config: None }
            })
            .collect();
        // configs only matter for rules whose proof takes a value from them
        let spec = |r: &PolicyRule| match_rule(r).map(|m| m.spec.id).unwrap_or_default();
        if rules.iter().any(|r| matches!(spec(r).as_str(), "floor_price_rule" | "staking_lock_rule")) {
            let fields = chain.dynamic_fields(&o.object_id, 2000).await.unwrap_or_default();
            for r in rules.iter_mut() {
                let id = spec(r);
                if id != "floor_price_rule" && id != "staking_lock_rule" {
                    continue;
                }
                let tail = r.rule_type.as_deref().unwrap_or("").split("::").skip(1).collect::<Vec<_>>().join("::");
                let Some(f) = fields.iter().find(|x| x.name_type.contains(&format!("::{tail}"))) else { continue };
                let Ok(b) = dynamic_field_value(chain, &o.object_id, &f.name_type, &f.name_bcs).await else { continue };
                if id == "floor_price_rule" && b.len() >= 8 {
                    let v = u64::from_le_bytes(b[..8].try_into().unwrap_or_default());
                    r.config = Some(serde_json::json!({ "floor_price": v.to_string() }));
                }
                if id == "staking_lock_rule" && b.len() >= 32 {
                    r.config = Some(serde_json::json!({ "registry_id": hex_at(&b, 0) }));
                }
            }
        }
        out.push(TransferPolicyInfo { id: canon_addr(&o.object_id), is_shared: shared, rules });
    }
    Ok(out)
}

/// Policy ids from the item's own history: the transaction that last moved a locked item into a
/// kiosk had to pass its `TransferPolicy<T>` (kiosk::lock, confirm_request), so it is among that
/// transaction's objects. Pruned fullnodes only keep recent transactions, so this is a fallback.
pub async fn policy_ids_from_history(chain: &dyn ChainRead, history: &dyn TxObjectTypes, nft_id: &str, nft_type: &str) -> Result<Vec<String>> {
    let inner = normalize_struct_tag(nft_type).ok_or_else(|| Error::invalid(format!("Invalid struct tag: {nft_type}")))?;
    let want = normalize_struct_tag(&format!("0x2::transfer_policy::TransferPolicy<{inner}>")).ok_or_else(|| Error::invalid("bad policy type"))?;
    let o = chain.object(nft_id, Include { previous_transaction: true, ..Include::NONE }).await?;
    let Some(digest) = o.previous_transaction.filter(|d| !d.is_empty()) else { return Ok(vec![]) };
    let types = history.tx_object_types(&digest).await?;
    Ok(types.into_iter().filter(|(_, t)| normalize_struct_tag(t).as_deref() == Some(want.as_str())).map(|(id, _)| canon_addr(&id)).collect())
}

/// Ids of every `TransferPolicy<T>` for a type, from the Trade API (`GET /ext/nft/policies`, found in
/// its indexed chain history: gRPC can't find shared objects by type). Fails when the Trade API can't answer.
pub async fn policy_ids_from_trade(base: &str, nft_type: &str, http: &dyn HttpGet) -> Result<Vec<String>> {
    let t = normalize_struct_tag(nft_type).ok_or_else(|| Error::invalid(format!("Invalid struct tag: {nft_type}")))?;
    let url = format!("{}/ext/nft/policies?type={}", base.trim_end_matches('/'), rare_sui::market::encode_component(&t));
    let body = match http.get_json(&url).await {
        Ok(v) => v,
        Err(Error::Http { status: Some(s), .. }) => return Err(Error::http(Some(s), format!("Policy lookup failed ({s})"))),
        Err(e) => return Err(e),
    };
    let Some(list) = body.get("policies").and_then(Value::as_array) else { return Err(Error::decode("Policy lookup failed")) };
    Ok(list.iter().filter_map(|p| p.get("id").and_then(Value::as_str)).map(canon_addr).collect())
}

/// A collection's policies from the item's history only (no the Trade API).
pub struct HistoryPolicies<'a> {
    pub chain: &'a dyn ChainRead,
    pub history: &'a dyn TxObjectTypes,
}

impl PolicySource for HistoryPolicies<'_> {
    fn policies<'a>(&'a self, nft_type: &'a str, nft_id: &'a str) -> BoxFut<'a, Vec<TransferPolicyInfo>> {
        Box::pin(async move {
            let ids = policy_ids_from_history(self.chain, self.history, nft_id, nft_type).await?;
            read_policies(self.chain, &ids).await
        })
    }
}

/// `policyLoader`: ids from the Trade API, each policy's rules and configs read over gRPC; when the Trade API can't
/// answer or knows none, the item's last transaction names the policy it was locked under.
pub struct PolicyLoader<'a> {
    pub chain: &'a dyn ChainRead,
    pub history: &'a dyn TxObjectTypes,
    pub trade_base: String,
    pub http: &'a dyn HttpGet,
}

impl PolicySource for PolicyLoader<'_> {
    fn policies<'a>(&'a self, nft_type: &'a str, nft_id: &'a str) -> BoxFut<'a, Vec<TransferPolicyInfo>> {
        Box::pin(async move {
            let ids = policy_ids_from_trade(&self.trade_base, nft_type, self.http).await.unwrap_or_default();
            let found = if ids.is_empty() { vec![] } else { read_policies(self.chain, &ids).await? };
            if !found.is_empty() {
                return Ok(found);
            }
            HistoryPolicies { chain: self.chain, history: self.history }.policies(nft_type, nft_id).await
        })
    }
}

/// One NFT as the send screen knows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NftRef {
    pub object_id: String,
    #[serde(rename = "type")]
    pub type_: String,
    /// the Trade API / rare.market says it sits in a kiosk.
    #[serde(default)]
    pub kiosk: bool,
}

/// Whether one NFT can be sent: the builder input, or the reason in plain words. Serialises as the
/// TS `{ ok: true, send } | { ok: false, reason }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawSendability", into = "RawSendability")]
pub enum Sendability {
    Ok(SendItem),
    No(String),
}

#[derive(Clone, Serialize, Deserialize)]
struct RawSendability {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    send: Option<SendItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl From<RawSendability> for Sendability {
    fn from(r: RawSendability) -> Self {
        match (r.ok, r.send) {
            (true, Some(s)) => Sendability::Ok(s),
            _ => Sendability::No(r.reason.unwrap_or_default()),
        }
    }
}

impl From<Sendability> for RawSendability {
    fn from(s: Sendability) -> Self {
        match s {
            Sendability::Ok(send) => RawSendability { ok: true, send: Some(send), reason: None },
            Sendability::No(reason) => RawSendability { ok: false, send: None, reason: Some(reason) },
        }
    }
}

impl Sendability {
    /// The builder input when it can be sent.
    pub fn send(&self) -> Option<&SendItem> {
        match self {
            Sendability::Ok(send) => Some(send),
            Sendability::No(_) => None,
        }
    }
    /// Why it can't be sent.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Sendability::Ok(_) => None,
            Sendability::No(reason) => Some(reason),
        }
    }
}

/// What [`analyze_nfts`] reads with.
pub struct AnalyzeDeps<'a> {
    pub chain: &'a dyn ChainRead,
    pub policies: &'a dyn PolicySource,
    /// Check that wallet NFTs have `store` (without it they can't leave the wallet).
    pub check_store: bool,
}

const SOULBOUND: &str = "It is bound to this account. Its creator made it so it can not be transferred";

/// Whether each NFT can be sent, and the builder input for the ones that can (keyed by the
/// `object_id` as given).
pub async fn analyze_nfts(deps: &AnalyzeDeps<'_>, owner: &str, nfts: &[NftRef]) -> Result<HashMap<String, Sendability>> {
    let mut out = HashMap::new();
    let outer = |t: &str| t.split('<').next().unwrap_or("").to_string();
    // objects without `store` (soulbound badges and the like) can't leave the wallet
    let mut store_of: HashMap<String, Option<bool>> = HashMap::new();
    if deps.check_store {
        for n in nfts.iter().filter(|n| !n.kiosk) {
            let t = outer(&n.type_);
            if let std::collections::hash_map::Entry::Vacant(e) = store_of.entry(t) {
                let v = has_store(deps.chain, e.key()).await.ok().flatten();
                e.insert(v);
            }
        }
    }
    for n in nfts.iter().filter(|n| !n.kiosk) {
        let s = if store_of.get(&outer(&n.type_)) == Some(&Some(false)) {
            Sendability::No(SOULBOUND.into())
        } else {
            Sendability::Ok(SendItem { from: None, item: KioskItemRef { nft_id: canon_addr(&n.object_id), nft_type: n.type_.clone(), ..Default::default() }, policies: None })
        };
        out.insert(n.object_id.clone(), s);
    }
    if !nfts.iter().any(|n| n.kiosk) {
        return Ok(out);
    }

    let ctx = load_kiosk_context(deps.chain, owner).await?;
    let mut policy_cache: HashMap<String, Option<Vec<TransferPolicyInfo>>> = HashMap::new();
    for n in nfts.iter().filter(|n| n.kiosk) {
        let id = canon_addr(&n.object_id);
        let Some(slot) = ctx.items.get(&id) else {
            let reason = if ctx.ob_items.contains(&id) {
                "It sits in an OriginByte kiosk, which only OriginByte trading can move"
            } else if ctx.partial {
                "Could not read its kiosk. Try again in a moment"
            } else {
                "Could not find the kiosk that holds it"
            };
            out.insert(n.object_id.clone(), Sendability::No(reason.into()));
            continue;
        };
        let mut market: Option<Market> = None;
        if slot.is_listed {
            match listing_market(deps.chain, &id, slot.is_exclusive_listing).await? {
                ListingMarket::Unknown => {
                    out.insert(n.object_id.clone(), Sendability::No("It is listed on a marketplace this wallet can not delist from. Cancel that listing first".into()));
                    continue;
                }
                ListingMarket::At(m) => market = Some(m),
                ListingMarket::PlainKiosk => {}
            }
        }
        let mut policies = None;
        if slot.is_locked {
            if !policy_cache.contains_key(&n.type_) {
                let p = deps.policies.policies(&n.type_, &id).await.ok();
                policy_cache.insert(n.type_.clone(), p);
            }
            let Some(p) = policy_cache[&n.type_].clone() else {
                out.insert(n.object_id.clone(), Sendability::No("Could not load the collection rules. Try again in a moment".into()));
                continue;
            };
            if let Some(problem) = policy_problem(Some(&p)) {
                out.insert(n.object_id.clone(), Sendability::No(problem));
                continue;
            }
            policies = Some(p);
        }
        let item = KioskItemRef { nft_id: id, nft_type: n.type_.clone(), is_locked: slot.is_locked, is_listed: slot.is_listed, is_exclusive_listing: slot.is_exclusive_listing, market };
        out.insert(n.object_id.clone(), Sendability::Ok(SendItem { from: Some(slot.kiosk.clone()), item, policies }));
    }
    Ok(out)
}

/// `nftSendability`: [`analyze_nfts`] over the live node, with policy ids from the Trade API (falling back to
/// the item's history) and the soulbound check.
pub async fn nft_sendability(sui: &rare_sui::Sui, http: &dyn HttpGet, owner: &str, nfts: &[NftRef]) -> Result<HashMap<String, Sendability>> {
    let loader = PolicyLoader { chain: sui, history: sui, trade_base: rare_sui::prices::TRADE_URL.to_string(), http };
    analyze_nfts(&AnalyzeDeps { chain: sui, policies: &loader, check_store: true }, owner, nfts).await
}
