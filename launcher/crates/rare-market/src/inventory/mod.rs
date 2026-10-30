//! Cross-game inventory: the player's collectibles from every Sui game, grouped by game, with
//! optimistic updates while the chain and the indexer catch up. Ported from
//! launcher/src/inventory (types.ts, logic.ts, api.ts, store.ts, trade.ts, notice.ts, mock.ts).

pub mod logic;
#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod notice;
pub mod store;
pub mod trade;

pub use logic::*;

use crate::api::{self, paths, CollectionDetail, ItemCard, Page};
use crate::net::Net;
use crate::types::canon_type;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An inventory item: an item card plus its collection's floor (`floorMist`, cheapest all-in
/// listing, MIST), which [`ItemCard`] already carries.
pub type InvItem = ItemCard;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InvGame {
    pub slug: String,
    pub name: String,
    pub logo_url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InvGroup {
    /// None: collectibles that belong to no listed game.
    pub game: Option<InvGame>,
    pub items: Vec<InvItem>,
    /// Sum of collection floors over the items, MIST. A floor value, not an estimate.
    pub floor_value_mist: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Inventory {
    pub groups: Vec<InvGroup>,
    pub total: usize,
}

/// Why items left the inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Why {
    Sold,
    Sent,
}

/// What the player just did, shown before the chain and the indexer catch up.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum InvOp {
    Remove { ids: Vec<String>, why: Why },
    List { id: String, #[serde(rename = "priceMist")] price_mist: String },
    Delist { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub key: String,
    pub op: InvOp,
    pub at_ms: i64,
}

/// A game in the Sui Games catalog, the part grouping needs (lib/rare.ts `SuiGame`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SuiGame {
    pub slug: String,
    pub name: String,
    pub logo_url: String,
    pub collections: Vec<GameCollection>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameCollection {
    pub nft_type: String,
    pub name: String,
}

/// A rare.market answer that means "not there" (`/\b404\b/` on the error).
pub fn is_missing(e: &crate::Error) -> bool {
    let m = &e.0;
    m.match_indices("404").any(|(i, _)| {
        let before = m[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after = m[i + 3..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_');
        before && after
    })
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    data: serde_json::Value,
}

/// Loads the inventory: rare.market's grouped endpoint when it is deployed; until then the same
/// answer is composed here from the wallet's items, the games catalog and collection floors.
pub fn load_inventory(n: &dyn Net, address: &str, games: impl FnOnce() -> crate::Result<Vec<SuiGame>>) -> crate::Result<Inventory> {
    match api::get::<Raw>(n, &format!("/v1/accounts/{}/inventory", api::enc(address))) {
        Ok(r) => return Ok(normalize_inventory(Some(&r.data))),
        Err(e) if !is_missing(&e) => return Err(e),
        Err(_) => {}
    }
    let mut items: Vec<ItemCard> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..4 {
        let c = cursor.as_ref().map(|c| format!("&cursor={}", api::enc(c))).unwrap_or_default();
        let page: Page<ItemCard> = api::get(n, &format!("/v1/accounts/{}/items?limit=100{c}", api::enc(address)))?;
        items.extend(page.data);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let catalog = games().unwrap_or_default();
    let mut types: Vec<String> = Vec::new();
    for t in items.iter().map(|x| canon_type(&x.kind)) {
        if !types.contains(&t) {
            types.push(t);
        }
    }
    types.truncate(24);
    let mut floors: HashMap<String, Option<String>> = HashMap::new();
    for t in types {
        let f = api::get::<CollectionDetail>(n, &paths::collection(&t)).ok().and_then(|d| d.stats.and_then(|s| s.floor_mist));
        floors.insert(t, f);
    }
    let total = items.len();
    Ok(Inventory { groups: group_items(items, &catalog, &floors), total })
}

#[cfg(test)]
mod tests;
