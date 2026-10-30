//! Pure inventory logic: grouping, optimistic operations and when the indexer has caught up.
//! Ported from launcher/src/inventory/logic.ts.

use super::{InvGame, InvGroup, InvItem, InvOp, Inventory, Pending, SuiGame};
use crate::api::{Listing, OfferItem};
use crate::types::canon_type;
use std::collections::HashMap;

fn big(v: Option<&str>) -> i128 {
    v.and_then(|s| s.trim().parse::<i128>().ok()).unwrap_or(0)
}

/// Held for the player by a staking contract or one of their own objects: counted and valued, but
/// not listable, sellable or sendable.
pub fn is_staked(i: &InvItem) -> bool {
    i.staked.is_some()
}

/// The plain line shown where list and send would be.
pub fn unstake_line(i: &InvItem) -> String {
    format!("Unstake on {} to list or send", i.collection.as_ref().map(|c| c.name.as_str()).unwrap_or("its project"))
}

pub fn sum_floors(items: &[InvItem]) -> String {
    items.iter().map(|i| big(i.floor_mist.as_deref())).sum::<i128>().to_string()
}

fn make_group(game: Option<InvGame>, items: Vec<InvItem>) -> InvGroup {
    InvGroup { floor_value_mist: sum_floors(&items), game, items }
}

/// Groups items under the game whose collections hold their type; games keep catalog order, the
/// rest come last. Items without a floor take their collection's from `floors` (by canonical type).
pub fn group_items(items: Vec<InvItem>, games: &[SuiGame], floors: &HashMap<String, Option<String>>) -> Vec<InvGroup> {
    let mut owner: HashMap<String, &SuiGame> = HashMap::new();
    for g in games {
        for c in &g.collections {
            owner.insert(canon_type(&c.nft_type), g);
        }
    }
    let mut buckets: HashMap<String, Vec<InvItem>> = HashMap::new();
    for mut it in items {
        let t = canon_type(&it.kind);
        let key = owner.get(&t).map(|g| g.slug.clone()).unwrap_or_default();
        if it.floor_mist.is_none() {
            it.floor_mist = floors.get(&t).cloned().flatten();
        }
        buckets.entry(key).or_default().push(it);
    }
    let mut out = Vec::new();
    for g in games {
        if let Some(list) = buckets.remove(&g.slug).filter(|l| !l.is_empty()) {
            out.push(make_group(Some(InvGame { slug: g.slug.clone(), name: g.name.clone(), logo_url: g.logo_url.clone() }), list));
        }
    }
    if let Some(rest) = buckets.remove("").filter(|l| !l.is_empty()) {
        out.push(make_group(None, rest));
    }
    out
}

/// The API's grouped answer (its `data`, or anything shaped like it), made safe to render.
pub fn normalize_inventory(data: Option<&serde_json::Value>) -> Inventory {
    let list = data.and_then(|d| d.as_array()).cloned().unwrap_or_default();
    let groups: Vec<InvGroup> = list
        .into_iter()
        .filter(|g| g.get("items").and_then(|i| i.as_array()).is_some_and(|a| !a.is_empty()))
        .filter_map(|g| serde_json::from_value::<InvGroup>(g).ok())
        .map(|g| make_group(g.game, g.items))
        .collect();
    let total = groups.iter().map(|g| g.items.len()).sum();
    Inventory { groups, total }
}

fn rebuild(groups: Vec<InvGroup>) -> Inventory {
    let kept: Vec<InvGroup> = groups.into_iter().filter(|g| !g.items.is_empty()).map(|g| make_group(g.game, g.items)).collect();
    let total = kept.iter().map(|g| g.items.len()).sum();
    Inventory { groups: kept, total }
}

fn map_items(inv: &Inventory, f: impl Fn(&InvItem) -> InvItem) -> Inventory {
    rebuild(inv.groups.iter().map(|g| InvGroup { items: g.items.iter().map(&f).collect(), ..g.clone() }).collect())
}

/// One optimistic operation on an inventory.
pub fn apply_op(inv: &Inventory, op: &InvOp) -> Inventory {
    match op {
        InvOp::Remove { ids, .. } => rebuild(inv.groups.iter().map(|g| InvGroup { items: g.items.iter().filter(|i| !ids.contains(&i.id)).cloned().collect(), ..g.clone() }).collect()),
        InvOp::List { id, price_mist } => map_items(inv, |i| {
            if i.id == *id && !is_staked(i) {
                InvItem { listing: Some(Listing { venue: "kiosk".into(), price_mist: price_mist.clone(), all_in_mist: Some(price_mist.clone()), kiosk_id: i.kiosk_id.clone(), ..Default::default() }), ..i.clone() }
            } else {
                i.clone()
            }
        }),
        InvOp::Delist { id } => map_items(inv, |i| if i.id == *id { InvItem { listing: None, ..i.clone() } } else { i.clone() }),
    }
}

pub fn apply_pending(base: &Inventory, pending: &[Pending]) -> Inventory {
    pending.iter().fold(base.clone(), |inv, p| apply_op(&inv, &p.op))
}

fn find<'a>(inv: &'a Inventory, id: &str) -> Option<&'a InvItem> {
    inv.groups.iter().flat_map(|g| g.items.iter()).find(|i| i.id == id)
}

/// Whether a fetched inventory already shows what the operation did.
pub fn reflects(inv: &Inventory, op: &InvOp) -> bool {
    match op {
        InvOp::Remove { ids, .. } => ids.iter().all(|id| find(inv, id).is_none()),
        InvOp::List { id, price_mist } => find(inv, id).and_then(|i| i.listing.as_ref()).is_some_and(|l| big(Some(&l.price_mist)) == big(Some(price_mist))),
        InvOp::Delist { id } => find(inv, id).is_none_or(|i| i.listing.is_none()),
    }
}

/// Pending operations the fetched inventory has caught up with, or that waited too long, are dropped.
pub fn settle(pending: &[Pending], fetched: &Inventory, now_ms: i64, max_age_ms: i64) -> Vec<Pending> {
    pending.iter().filter(|p| now_ms - p.at_ms < max_age_ms && !reflects(fetched, &p.op)).cloned().collect()
}

pub const SETTLE_MAX_AGE_MS: i64 = 90_000;

/// Notices that mean the player's holdings changed on chain.
pub const MOVES: [&str; 4] = ["sold", "bought", "nft_received", "offer_received"];

pub fn total_floor_mist(inv: &Inventory) -> String {
    inv.groups.iter().map(|g| big(Some(&g.floor_value_mist))).sum::<i128>().to_string()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameValue {
    pub slug: String,
    pub name: String,
    pub logo_url: String,
    pub items: usize,
    pub value_mist: String,
}

/// What Home shows: the total floor value of the player's game items, and per game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvSummary {
    pub items: usize,
    pub value_mist: String,
    /// Items that have an open offer on them.
    pub offers: usize,
    /// Per game, most valuable first. Collectibles that belong to no listed game are left out.
    pub games: Vec<GameValue>,
}

pub fn summarize(inv: &Inventory) -> InvSummary {
    let in_games: Vec<&InvGroup> = inv.groups.iter().filter(|g| g.game.is_some()).collect();
    let mut games: Vec<GameValue> = in_games
        .iter()
        .map(|g| {
            let game = g.game.as_ref().unwrap();
            GameValue { slug: game.slug.clone(), name: game.name.clone(), logo_url: game.logo_url.clone(), items: g.items.len(), value_mist: g.floor_value_mist.clone() }
        })
        .collect();
    games.sort_by(|a, b| big(Some(&b.value_mist)).cmp(&big(Some(&a.value_mist))).then(b.items.cmp(&a.items)));
    InvSummary {
        items: in_games.iter().map(|g| g.items.len()).sum(),
        value_mist: in_games.iter().map(|g| big(Some(&g.floor_value_mist))).sum::<i128>().to_string(),
        offers: in_games.iter().map(|g| g.items.iter().filter(|i| big(i.top_offer_mist.as_deref()) > 0).count()).sum(),
        games,
    }
}

/// The item as the API's offer item shape (for a notice or a sheet).
pub fn offer_item(i: &InvItem) -> OfferItem {
    OfferItem { id: i.id.clone(), name: i.name.clone(), image_url: i.image_url.clone().unwrap_or_default() }
}
