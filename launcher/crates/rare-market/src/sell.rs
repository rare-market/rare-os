//! Selling an item the player holds: list it at a price, sell it to the best offer, or take a
//! native listing down. Ported from launcher/src/overlay/trade.ts (used by the in-game overlay and
//! the inventory) and inventory/trade.ts `buildCancelTransaction`.
//!
//! Builders are rare.market's own ([`crate::venues`]): native kiosk listings, and `build_accept_bid`
//! for offers on every venue. The seller's kiosks and the type's transfer policies come from
//! rare.market's API, with the kiosk list read from the chain when the API can't answer. Nothing
//! here signs or executes.

use crate::api::{self, paths, ItemCard, Listing, Offer};
use crate::net::Net;
use crate::ptb::Tx;
use crate::types::norm_sui_address;
use crate::venues::{self, build_list, choose_policy, delist_from_own_kiosk, load_trade_context, norm_type, read_policy, seller_net_mist, ExistingListing, KioskReader, KioskScope, ListingItem, OfferSellContext, OwnedKioskRef, TransferPolicy, VenueConfig};
use crate::{Error, Result};

/// Bid sources the ported `build_accept_bid` can fill (rare.market web/lib/sell/best-bid.ts
/// SELLABLE). market/v1 is left out: rare's own contract is not deployed.
const SELLABLE: [&str; 4] = ["tradeport/bids", "tradeport/escrow-bids", "tradeport/kiosk-bids", "drip/v2-bids"];
/// These take wallet-held items only (no kiosk).
const WALLET_ONLY: [&str; 1] = ["tradeport/escrow-bids"];
/// Sources whose seller net depends on the type's royalty (see `seller_net_mist`), and which
/// can't take items whose policy needs a personal kiosk.
const ROYALTY_SENSITIVE: [&str; 2] = ["tradeport/kiosk-bids", "drip/v2-bids"];

const INDEXING: &str = "This item is still being indexed. Try again in a minute.";

fn norm_id(id: &str) -> String {
    let l = id.to_lowercase();
    format!("0x{:0>64}", l.strip_prefix("0x").unwrap_or(&l))
}

pub(crate) fn same_id(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => norm_id(a) == norm_id(b),
        _ => false,
    }
}

fn same_type(a: &str, b: &str) -> bool {
    norm_type(a) == norm_type(b)
}

fn is_int(s: &str) -> bool {
    let d = s.strip_prefix('-').unwrap_or(s);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

/// What the seller receives for the offer, before gas. Venue fee, royalty and commission are paid
/// by the bidder on top of the price on most sources, so this is the price. Offers from
/// [`best_offer`] carry the exact figure for the two sources where the royalty can come out of the
/// seller's side (Tradeport kiosk bids with a stale royalty reserve, Drip v2 bids).
pub fn offer_net_mist(offer: &Offer) -> i128 {
    if let Some(serde_json::Value::String(s)) = offer.extra.get("sellerNetMist") {
        if is_int(s) {
            if let Ok(v) = s.parse() {
                return v;
            }
        }
    }
    seller_net_mist(&VenueConfig::default(), offer, None)
}

/// Where an item is and who holds it, for [`can_fill`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemPlace<'a> {
    pub id: &'a str,
    pub kind: Option<&'a str>,
    pub kiosk_id: Option<&'a str>,
    pub owner: Option<&'a str>,
}

/// Whether this offer can take the item (rare.market web/lib/sell/best-bid.ts canFill).
pub fn can_fill(o: &Offer, item: &ItemPlace, now_ms: i64) -> bool {
    if o.status != "open" || !o.fillable {
        return false;
    }
    if o.filled.unwrap_or(0) >= o.quantity.unwrap_or(1) {
        return false;
    }
    if let Some(e) = o.expires_at.as_deref().filter(|s| !s.is_empty()) {
        if crate::time::parse_iso_ms(e).is_some_and(|t| t <= now_ms) {
            return false;
        }
    }
    if !SELLABLE.contains(&o.source.as_str()) {
        return false;
    }
    if WALLET_ONLY.contains(&o.source.as_str()) && item.kiosk_id.is_some_and(|k| !k.is_empty()) {
        return false;
    }
    if let (Some(ot), Some(it)) = (o.nft_type.as_deref().filter(|s| !s.is_empty()), item.kind.filter(|s| !s.is_empty())) {
        if !same_type(ot, it) {
            return false;
        }
    }
    if item.owner.is_some_and(|w| !w.is_empty()) && same_id(Some(&o.bidder), item.owner) {
        return false;
    }
    if o.kind == "item" {
        return same_id(o.item.as_ref().map(|i| i.id.as_str()), Some(item.id));
    }
    // Trait bids need the item's attributes; the API sends none for ?item= today.
    o.kind == "collection"
}

#[derive(serde::Deserialize)]
struct Data<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

/// The best offer the item can be sold into right now, or None. Ranked by what the seller receives.
pub fn best_offer(n: &dyn Net, cfg: &VenueConfig, item_id: &str, now_ms: i64) -> Result<Option<Offer>> {
    let offers: Data<Offer> = api::get(n, &paths::offers_for_item(item_id, 10))?;
    // Where the item is and who holds it: wallet-only bids and the owner's own bids are skipped.
    let item: Option<ItemCard> = api::get(n, &paths::item(item_id)).ok();
    let place = ItemPlace {
        id: item_id,
        kind: item.as_ref().map(|i| i.kind.as_str()),
        kiosk_id: item.as_ref().and_then(|i| i.kiosk_id.as_deref()),
        owner: item.as_ref().and_then(|i| i.owner.as_deref()),
    };
    let mut fillable: Vec<Offer> = offers.data.into_iter().filter(|o| can_fill(o, &place, now_ms)).collect();
    let ty = match &item {
        Some(i) => Some(i.kind.clone()),
        None => fillable.iter().find_map(|o| o.nft_type.clone().filter(|s| !s.is_empty())),
    };
    if let Some(t) = ty.filter(|t| !t.is_empty()) {
        if fillable.iter().any(|o| ROYALTY_SENSITIVE.contains(&o.source.as_str())) {
            if let Ok(pols) = api::get::<Data<TransferPolicy>>(n, &paths::policies(&t)) {
                let policy = choose_policy(cfg, Some(&pols.data));
                // Tradeport kiosk bids and Drip v2 bids can't deliver into the bidder's personal kiosk.
                if read_policy(cfg, policy).personal {
                    fillable.retain(|o| !ROYALTY_SENSITIVE.contains(&o.source.as_str()));
                }
                for o in fillable.iter_mut().filter(|o| ROYALTY_SENSITIVE.contains(&o.source.as_str())) {
                    let net = seller_net_mist(cfg, o, Some(policy));
                    o.extra.insert("sellerNetMist".into(), serde_json::Value::String(net.to_string()));
                }
            }
        }
    }
    // The API sorts by price, highest first; keep that order among equal nets.
    let mut best: Option<Offer> = None;
    for o in fillable {
        if best.as_ref().is_none_or(|b| offer_net_mist(&o) > offer_net_mist(b)) {
            best = Some(o);
        }
    }
    Ok(best)
}

/// The kiosk of the sender's that holds the item, or None for a wallet item.
fn seller_kiosk(kiosk_id: Option<&str>, kiosks: &[OwnedKioskRef], sender: &str, chain: Option<&dyn KioskReader>) -> Result<Option<OwnedKioskRef>> {
    let Some(kiosk_id) = kiosk_id.filter(|s| !s.is_empty()) else { return Ok(None) };
    if let Some(hit) = kiosks.iter().find(|k| same_id(Some(&k.kiosk_id), Some(kiosk_id))) {
        return Ok(Some(hit.clone()));
    }
    // rare.market's kiosk list can trail the chain by a few seconds: ask the node before giving up.
    let fresh = chain.and_then(|c| c.owned_kiosks(sender).ok()).unwrap_or_default();
    fresh.into_iter().find(|k| same_id(Some(&k.kiosk_id), Some(kiosk_id))).map(Some).ok_or_else(|| Error::new("Your wallet doesn't hold the kiosk this item is in."))
}

fn need_off_venue(l: Option<&Listing>) -> Result<()> {
    match l {
        Some(l) if l.venue != "kiosk" => Err(Error(format!("Delist it on {} first.", venues::venue_label(&l.venue)))),
        _ => Ok(()),
    }
}

/// List the item at `price_mist` with 0x2::kiosk::list in the seller's own kiosk (no marketplace
/// fee). A wallet item is placed (or locked) into the seller's kiosk first, and a kiosk is created in
/// the same transaction when they have none. An item already listed natively is relisted at the new
/// price.
pub fn build_list_transaction(n: &dyn Net, cfg: &VenueConfig, item: &ItemCard, price_mist: u64, sender: &str, chain: Option<&dyn KioskReader>) -> Result<Tx> {
    if price_mist == 0 {
        return Err(Error::new("Price must be above zero."));
    }
    if item.kind.is_empty() {
        return Err(Error::new(INDEXING));
    }
    let listing = item.listing.as_ref();
    need_off_venue(listing)?;
    let ctx = load_trade_context(n, sender, &[&item.kind], chain)?;
    let kiosk = seller_kiosk(listing.and_then(|l| l.kiosk_id.as_deref()).or(item.kiosk_id.as_deref()), &ctx.buyer_kiosks, sender, chain)?;
    let list = ctx.policies.get(&item.kind);
    let policy = choose_policy(cfg, list.map(|v| v.as_slice())).or_else(|| list.and_then(|v| v.first())).cloned();
    let li = ListingItem {
        nft_id: item.id.clone(),
        nft_type: item.kind.clone(),
        price_mist,
        kiosk,
        policy,
        listing: listing.map(|l| ExistingListing { venue: l.venue.clone(), order_key: l.order_key.clone() }),
    };
    Ok(build_list(cfg, sender, &[li], &ctx.buyer_kiosks)?)
}

/// Sell the item into `offer` on whatever venue it lives. The NFT always reaches the bidder and the
/// seller receives [`offer_net_mist`]. An item listed natively in the seller's kiosk is delisted in
/// the same transaction; one listed on Tradeport or Drip must be delisted there first.
pub fn build_accept_offer_transaction(n: &dyn Net, cfg: &VenueConfig, item: &ItemCard, offer: &Offer, sender: &str, chain: Option<&dyn KioskReader>) -> Result<Tx> {
    if item.kind.is_empty() {
        return Err(Error::new(INDEXING));
    }
    if offer.status != "open" || !offer.fillable {
        return Err(Error::new("This offer can't be filled any more."));
    }
    if let Some(t) = offer.nft_type.as_deref().filter(|s| !s.is_empty()) {
        if !same_type(t, &item.kind) {
            return Err(Error::new("This offer is for another collection."));
        }
    }
    if offer.kind == "item" {
        if let Some(oi) = &offer.item {
            if !same_id(Some(&oi.id), Some(&item.id)) {
                return Err(Error::new("This offer is for another item."));
            }
        }
    }
    if same_id(Some(&offer.bidder), Some(sender)) {
        return Err(Error::new("You can't accept your own offer."));
    }
    let listing = item.listing.as_ref();
    need_off_venue(listing)?;
    let ctx = load_trade_context(n, sender, &[&item.kind], chain)?;
    let kiosk = seller_kiosk(listing.and_then(|l| l.kiosk_id.as_deref()).or(item.kiosk_id.as_deref()), &ctx.buyer_kiosks, sender, chain)?;
    let mut o = offer.clone();
    o.nft_type = offer.nft_type.as_ref().filter(|s| !s.is_empty()).map(|_| item.kind.clone());
    venues::build_accept_offer(
        cfg,
        &o,
        &OfferSellContext { sender, kiosk: kiosk.as_ref(), nft_id: &item.id, nft_type: &item.kind, policies: &ctx.policies, listed_native: listing.is_some_and(|l| l.venue == "kiosk") },
    )
}

/// Takes a native kiosk listing off sale. Listings on Tradeport or Drip come off there.
pub fn build_cancel_transaction(n: &dyn Net, item: &ItemCard, sender: &str, chain: Option<&dyn KioskReader>) -> Result<Tx> {
    let l = item.listing.as_ref().ok_or_else(|| Error::new("This item isn't listed."))?;
    if l.venue != "kiosk" {
        return Err(Error::new("Cancel it where it is listed."));
    }
    if item.kind.is_empty() {
        return Err(Error::new(INDEXING));
    }
    let ctx = load_trade_context(n, sender, &[], chain)?;
    let kiosk_id = l.kiosk_id.as_deref().or(item.kiosk_id.as_deref()).filter(|s| !s.is_empty());
    let norm = |s: &str| norm_sui_address(s);
    let found = kiosk_id.and_then(|id| ctx.buyer_kiosks.iter().find(|k| norm(&k.kiosk_id) == norm(id)).cloned());
    let found = match (found, kiosk_id) {
        (Some(k), _) => Some(k),
        (None, Some(id)) => chain.and_then(|c| c.owned_kiosks(sender).ok()).unwrap_or_default().into_iter().find(|k| norm(&k.kiosk_id) == norm(id)),
        (None, None) => None,
    };
    let r = found.ok_or_else(|| Error::new("Your wallet doesn't hold the kiosk this item is in."))?;
    let mut tx = Tx::new();
    tx.set_sender(sender);
    let mut scope = KioskScope::new(sender);
    let k = scope.open(&mut tx, &r)?;
    delist_from_own_kiosk(&mut tx, &k, &item.id, &item.kind);
    scope.close(&mut tx);
    Ok(tx)
}

#[cfg(test)]
mod tests;
