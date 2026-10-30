//! Buying. Ported from launcher/src/marketplace/buy.ts: the confirm sheet's price split and the one
//! place a purchase transaction gets built (rare.market's own `build_cart_buy` for one line), which
//! then goes to Rare Wallet's sign-and-execute (review sheet, PIN or device, submit). Watch
//! accounts can't sign: for them the sheet hands off to a phone (QR of the rare.market item page).

use crate::api::{self, ItemCard, Listing};
use crate::net::Net;
use crate::ptb::Tx;
use crate::venues::{build_cart_buy, load_trade_context, BuyLine, KioskReader, VenueConfig};
use crate::{Error, Result};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    pub venue: String,
    pub price_mist: i128,
    pub fee_mist: i128,
    pub royalty_mist: i128,
    pub commission_mist: i128,
    pub total_mist: i128,
}

fn big(v: Option<&str>) -> i128 {
    v.map(str::trim).filter(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// What the buyer pays, split the way rare.market's API prices a listing (all-in = price + fees +
/// royalty).
pub fn quote_of(l: &Listing) -> Quote {
    let price = big(Some(&l.price_mist));
    let fee = big(l.fee_mist.as_deref());
    let commission = big(l.commission_mist.as_deref());
    let royalty_extra = l.extra_str("royaltyMist");
    let total = match &l.all_in_mist {
        Some(a) if !a.is_empty() => big(Some(a)),
        _ => price + fee + commission + big(royalty_extra.as_deref()),
    };
    // Royalty is whatever the all-in price carries beyond price and fees (kiosk policies vary).
    let royalty = match royalty_extra {
        Some(r) => big(Some(&r)),
        None => total - price - fee - commission,
    };
    Quote { venue: l.venue.clone(), price_mist: price, fee_mist: fee, royalty_mist: royalty.max(0), commission_mist: commission, total_mist: total }
}

/// The Rare Store package ids the resale rule needs (`/v1/store/config`, store/api.ts
/// `normalizeConfig`: snake_case, camelCase or the env names, flat or under `config`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreIds {
    pub package: Option<String>,
    pub origin: Option<String>,
}

pub fn normalize_store_ids(raw: &Value) -> StoreIds {
    let o = raw.get("config").filter(|v| !v.is_null()).unwrap_or(raw);
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| match o.get(*k) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        })
    };
    StoreIds { package: pick(&["package", "package_id", "packageId", "RARE_STORE_PACKAGE"]), origin: pick(&["origin", "origin_id", "originId", "original_package", "RARE_STORE_ORIGIN"]) }
}

/// Rare Store ResaleLicence: configures the resale rule from the store ids (`known`, or read from
/// `/v1/store/config` when the Store tab has not loaded them) and, when the item is a resale
/// licence, reads its Game (resale::prove needs it). None for any other item.
fn resale_game_for(n: &dyn Net, cfg: &mut VenueConfig, item_id: &str, nft_type: &str, known: Option<&StoreIds>) -> Result<Option<String>> {
    let ids = match known {
        Some(k) => k.clone(),
        None => api::get::<Value>(n, "/v1/store/config").map(|v| normalize_store_ids(&v)).unwrap_or_default(),
    };
    let Some(origin) = ids.origin.as_deref() else { return Ok(None) };
    cfg.resale.configure(Some(origin), Some(ids.package.as_deref().unwrap_or(origin)));
    let Some(t) = cfg.resale.licence_type() else { return Ok(None) };
    // The web launcher compares `nftType.toLowerCase()` with the mixed-case type, so a licence
    // never matches there and its purchase fails as "not available"; both sides are normalised here.
    if crate::venues::norm_type(nft_type) != t {
        return Ok(None);
    }
    let raw: Value = api::get(n, &format!("/v1/store/licences/{}", api::enc(item_id)))?;
    let game = raw.pointer("/licence/game_id").and_then(|v| v.as_str()).or_else(|| raw.get("game_id").and_then(|v| v.as_str())).filter(|s| !s.is_empty());
    game.map(|g| Some(g.to_string())).ok_or_else(|| Error::new("Could not read this game licence."))
}

/// The one place a purchase transaction gets built: `build_cart_buy` for one line. The buyer's
/// kiosks and the type's transfer policies come from rare.market's API; when the kiosk list can't be
/// read there it falls back to the chain.
pub fn build_buy_transaction(n: &dyn Net, cfg: &VenueConfig, item: &ItemCard, listing: &Listing, sender: &str, chain: Option<&dyn KioskReader>, store_ids: Option<&StoreIds>) -> Result<Tx> {
    let t = listing.nft_type.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| Error::new("This listing is still being indexed. Try again in a minute."))?;
    let ctx = load_trade_context(n, sender, &[t], chain)?;
    let mut cfg = cfg.clone();
    let resale_game = resale_game_for(n, &mut cfg, &item.id, t, store_ids)?;
    build_cart_buy(&cfg, &[BuyLine { item_id: item.id.clone(), listing: listing.clone(), resale_game }], &ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::MockNet;
    use serde_json::json;

    fn l(v: Value) -> Listing {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn quotes_split_like_the_api() {
        let q = quote_of(&l(json!({"venue": "kiosk", "priceMist": "12000000", "allInMist": "13200000", "feeMist": "0", "extra": {"royaltyMist": "1200000"}})));
        assert_eq!((q.price_mist, q.royalty_mist, q.total_mist), (12_000_000, 1_200_000, 13_200_000));
        let q = quote_of(&l(json!({"venue": "tradeport", "priceMist": "100", "feeMist": "3", "allInMist": "113"})));
        assert_eq!((q.fee_mist, q.royalty_mist), (3, 10));
        let q = quote_of(&l(json!({"venue": "drip", "priceMist": "100", "commissionMist": "2", "extra": {"royaltyMist": 5}})));
        assert_eq!(q.total_mist, 107);
    }

    #[test]
    fn store_ids_from_any_spelling() {
        assert_eq!(normalize_store_ids(&json!({"config": {"package_id": " 0x1 ", "RARE_STORE_ORIGIN": "0x2"}})), StoreIds { package: Some("0x1".into()), origin: Some("0x2".into()) });
        assert_eq!(normalize_store_ids(&json!({"packageId": ""})), StoreIds::default());
    }

    #[test]
    fn buys_a_resale_licence_with_its_game() {
        let origin = format!("0x{}", "9a".repeat(32));
        let game = format!("0x{}", "9c".repeat(32));
        let licence = format!("{origin}::licence::ResaleLicence");
        let item_id = format!("0x{}", "01".repeat(32));
        let sender = format!("0x{}", "aa".repeat(32));
        let n = MockNet::new();
        n.on_read_json(&format!("/v1/accounts/{sender}/kiosks"), json!({"data": []}));
        n.on_read_json(&format!("/v1/policies/{}", api::enc(&licence)), json!({"data": [{"id": format!("0x{}", "9d".repeat(32)), "isShared": true, "rules": [
            {"type": "rule", "ruleType": format!("{origin}::resale::Rule")}
        ]}]}));
        n.on_read_json("/v1/store/config", json!({"config": {"origin": origin, "package": format!("0x{}", "9b".repeat(32))}}));
        n.on_read_json(&format!("/v1/store/licences/{item_id}"), json!({"licence": {"game_id": game}}));
        let item = ItemCard { id: item_id.clone(), kind: licence.clone(), ..Default::default() };
        let listing = l(json!({"venue": "kiosk", "source": "kiosk/native", "priceMist": "5", "kioskId": format!("0x{}", "77".repeat(32)), "nftType": licence}));
        let tx = build_buy_transaction(&n, &VenueConfig::default(), &item, &listing, &sender, None, None).unwrap();
        assert!(tx.targets().iter().any(|t| t.ends_with("::resale::prove")));
        assert!(tx.object_ids().contains(&game.as_str()));
        let no_type = l(json!({"venue": "kiosk", "priceMist": "5"}));
        assert_eq!(build_buy_transaction(&n, &VenueConfig::default(), &item, &no_type, &sender, None, None).unwrap_err().0, "This listing is still being indexed. Try again in a minute.");
    }
}
