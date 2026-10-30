//! Selling and listing from the item page over a game (overlay/panels.tsx ItemDetail, trade.ts):
//! the best offer with its dry build, then the sale or the listing through rare-market's
//! `RealTrade` and Rare Wallet's review sheet. Everything here blocks: run it on a worker.

use crate::screens::overlay::panels::trade::{plain_error, Offer, OfferKind};
use crate::screens::overlay::panels::OfferState;
use rare_market::api::{self, ItemDetail};
use rare_market::inventory::trade::{Quote, RealTrade, TradeApi};

fn trader() -> RealTrade {
    RealTrade { net: crate::screens::market::feed::net(), cfg: Default::default(), chain: None, exec: crate::sign::executor() }
}

fn card(id: &str) -> Result<api::ItemCard, String> {
    api::get::<ItemDetail>(&*crate::screens::market::feed::net(), &api::paths::item(id)).map(|d| d.card).map_err(|_| "Couldn't read that item. Try again.".to_string())
}

fn num(s: &str) -> i128 {
    s.parse().unwrap_or(0)
}

/// The panels' view of an API offer, with the exact net the quote worked out.
pub fn panel_offer(q: &Quote) -> Offer {
    let o = &q.offer;
    Offer {
        id: o.id.clone(),
        venue: o.venue.clone(),
        source: o.source.clone(),
        nft_type: o.nft_type.clone(),
        kind: match o.kind.as_str() {
            "item" => OfferKind::Item,
            "trait" => OfferKind::Trait,
            _ => OfferKind::Collection,
        },
        item_id: o.item.as_ref().map(|i| i.id.clone()),
        price_mist: num(&o.price_mist),
        commission_mist: num(&o.commission_mist),
        escrow: None,
        seller_net_mist: Some(q.net_mist),
        quantity: o.quantity.unwrap_or(1) as u32,
        filled: o.filled.unwrap_or(0) as u32,
        bidder: o.bidder.clone(),
        expires_at_ms: None,
        open: true,
        fillable: true,
    }
}

/// bestOffer(item) and, for a signer, the dry build of the sale.
pub fn lookup(id: &str, address: &str) -> (OfferState, Option<Quote>) {
    let Ok(card) = card(id) else { return (OfferState::Nothing, None) };
    match trader().quote(&card, address) {
        Ok(Some(q)) => {
            let o = panel_offer(&q);
            let state = match q.blocked.clone() {
                Some(why) => OfferState::Blocked(o, plain_error(&why)),
                None => OfferState::Ready(o),
            };
            (state, Some(q))
        }
        _ => (OfferState::Nothing, None),
    }
}

/// What a finished trade came to: the line the page shows.
pub enum Did {
    Sold(i128),
    Listed(i128),
}

pub fn sell(id: &str, address: &str, q: &Quote) -> Result<Did, String> {
    let card = card(id)?;
    trader().sell(&card, &q.offer, address).map(|_| Did::Sold(q.net_mist)).map_err(|e| e.0)
}

pub fn list(id: &str, address: &str, price_mist: i128) -> Result<Did, String> {
    let card = card(id)?;
    let pm = u64::try_from(price_mist).map_err(|_| "Price must be above zero.".to_string())?;
    trader().list(&card, pm, address).map(|_| Did::Listed(price_mist)).map_err(|e| e.0)
}
