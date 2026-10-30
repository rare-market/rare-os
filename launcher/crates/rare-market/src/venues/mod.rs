//! rare.market's own trade builders, ported from launcher/src/marketplace/venues (index.ts is this
//! file: buildCartBuy, buildAcceptOffer, toVenuePolicy, toActiveListing, toActiveBid, the buyer
//! kiosk choice, loadTradeContext; the builders are in [`build`], native listing in [`list`]).
//!
//! Inputs come from rare.market's API: the listing (on the item), the buyer's kiosks
//! (GET /v1/accounts/:address/kiosks) and the transfer policies of the NFT type
//! (GET /v1/policies/:type). The result is an unresolved [`Tx`] for `rare-sui` to build and the
//! wallet to sign.

pub mod build;
pub mod ids;
pub mod list;
pub mod model;
pub mod policy;

pub use build::{all_in_price, build_accept_bid, build_buy, royalty_of, venue_fee, NftAt, UserKiosk};
pub use list::{build_list, delist_from_own_kiosk, list_in_own_kiosk, ExistingListing, KioskArg, KioskScope, ListingItem};
pub use model::*;
pub use policy::{choose_policy, is_known_rule, norm_type, read_policy, rules, Policy, PolicyShape, Resale, Royalty};

use crate::api::{paths, Listing, Offer, Page};
use crate::net::{self, Net, NetError};
use crate::ptb::Tx;
use serde_json::Value;

/// Rare's own contract (venue `market`), off while its ids are empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ours {
    pub package: String,
    pub config: String,
}

/// What the builders need besides the trade: the Rare Store resale rule and rare's own contract,
/// both empty until configured (the web launcher's `configureResale` / `configureOurs`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VenueConfig {
    pub resale: Resale,
    pub ours: Ours,
}

/// A builder failure. `Unsupported` becomes the plain "not available" sentence for the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildError {
    Unsupported(String),
    Other(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Unsupported(s) | BuildError::Other(s) => f.write_str(s),
        }
    }
}

impl From<BuildError> for crate::Error {
    fn from(e: BuildError) -> Self {
        crate::Error(e.to_string())
    }
}

pub const VENUE_LABEL: [(&str, &str); 4] = [("market", "Rare"), ("tradeport", "Tradeport"), ("drip", "Drip"), ("kiosk", "Kiosk")];

/// "Tradeport" for "tradeport"; unknown venues as they are.
pub fn venue_label(v: &str) -> &str {
    VENUE_LABEL.iter().find(|(k, _)| *k == v).map(|(_, l)| *l).unwrap_or(v)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Buy,
    Sell,
}

/// Builder errors in plain words.
pub fn friendly(e: BuildError, venue: &str, action: Action) -> crate::Error {
    let msg = e.to_string();
    let low = msg.to_lowercase();
    if matches!(e, BuildError::Unsupported(_)) || low.contains("not deployed") || low.contains("not supported") {
        let name = venue_label(venue);
        return crate::Error(match action {
            Action::Buy => format!("Buying from {name} is not available for this item yet."),
            Action::Sell => format!("Selling into offers on {name} is not available for this item yet."),
        });
    }
    crate::Error(msg)
}

/// A /v1/policies policy to the builder's shape.
pub fn to_venue_policy(cfg: &VenueConfig, p: Option<&TransferPolicy>, nft_type: &str) -> Option<Policy> {
    let p = p?;
    let shape = read_policy(cfg, Some(p));
    let rules: Vec<String> = p
        .rules
        .iter()
        .map(|r| norm_type(r.rule_type.as_deref().unwrap_or(&r.kind)))
        .map(|t| if t.ends_with("::Rule") { t } else { t.split('<').next().unwrap_or("").to_string() })
        .collect();
    let unknown_rules = rules.iter().filter(|r| !is_known_rule(cfg, r)).cloned().collect();
    Some(Policy {
        policy_id: p.id.clone(),
        nft_type: norm_type(nft_type),
        rules,
        unknown_rules,
        royalty: shape.royalty.map(|r| Royalty { bps: r.bps, min: r.min }),
        floor: shape.floor,
        shared_version: None,
    })
}

fn to_user_kiosk(k: Option<&OwnedKioskRef>) -> Option<UserKiosk> {
    k.map(|k| UserKiosk { kiosk_id: k.kiosk_id.clone(), cap_id: k.owner_cap_id.clone(), personal_cap_id: k.personal_cap_id.clone(), personal: k.is_personal })
}

pub fn to_active_listing(item_id: &str, l: &Listing) -> ActiveListing {
    ActiveListing {
        venue: l.venue.clone(),
        source: l.source.clone().unwrap_or_default(),
        listing_id: l.order_key.clone().unwrap_or_else(|| item_id.to_string()),
        object_id: l.object_id.clone(),
        nft_id: item_id.to_string(),
        nft_type: l.nft_type.clone(),
        seller: l.seller.clone(),
        kiosk_id: l.kiosk_id.clone(),
        price: l.price_mist.clone(),
        fee: l.fee_mist.clone().unwrap_or_else(|| "0".into()),
        commission: l.commission_mist.clone().unwrap_or_else(|| "0".into()),
        coin_type: "0x2::sui::SUI".into(),
        extra: l.extra.clone().unwrap_or_default(),
    }
}

pub fn to_active_bid(o: &Offer) -> ActiveBid {
    let mut extra = o.extra.clone();
    let dest = o.dest_kiosk_id.clone().map(Value::String).or_else(|| o.extra.get("destKiosk").filter(|v| !v.is_null()).cloned()).unwrap_or(Value::Null);
    extra.insert("destKiosk".into(), dest);
    ActiveBid {
        venue: o.venue.clone(),
        source: o.source.clone(),
        bid_id: o.id.clone(),
        kind: if o.kind == "item" { "item".into() } else { "collection".into() },
        nft_type: o.nft_type.clone(),
        nft_id: o.item.as_ref().map(|i| i.id.clone()),
        bidder: o.bidder.clone(),
        price: o.price_mist.clone(),
        fee: o.fee_mist.clone(),
        royalty: o.royalty_mist.clone(),
        commission: o.commission_mist.clone(),
        coin_type: "0x2::sui::SUI".into(),
        pool_id: o.pool_id.clone(),
        expire_at: o.expires_at.as_deref().filter(|s| !s.is_empty()).map(|s| crate::time::parse_iso_ms(s).map(|n| n.to_string()).unwrap_or_else(|| "NaN".into())),
        fillable: o.fillable && o.status == "open",
        extra,
    }
}

/// One line of a cart.
#[derive(Clone, Debug, Default)]
pub struct BuyLine {
    pub item_id: String,
    pub listing: Listing,
    /// Rare Store ResaleLicence: the Game object (resale::prove needs it).
    pub resale_game: Option<String>,
}

/// The buyer's side of a cart: their kiosks and the policies of the types in it.
#[derive(Clone, Debug, Default)]
pub struct TradeContext {
    pub sender: String,
    pub buyer_kiosks: Vec<OwnedKioskRef>,
    pub policies: PoliciesByType,
}

fn policies_for<'a>(ctx: &'a TradeContext, t: &str) -> Option<&'a [TransferPolicy]> {
    ctx.policies.get(t).map(|v| v.as_slice())
}

/// The buyer kiosk the whole cart uses: personal when any policy needs one.
fn buyer_kiosk<'a>(cfg: &VenueConfig, lines: &[BuyLine], ctx: &'a TradeContext) -> Option<&'a OwnedKioskRef> {
    let needs_personal = lines.iter().any(|l| read_policy(cfg, choose_policy(cfg, policies_for(ctx, l.listing.nft_type.as_deref().unwrap_or("")))).personal);
    let personal = ctx.buyer_kiosks.iter().find(|k| k.is_personal && k.personal_cap_id.as_deref().is_some_and(|s| !s.is_empty()));
    let plain = ctx.buyer_kiosks.iter().find(|k| !k.is_personal);
    if needs_personal {
        personal
    } else {
        plain.or(personal)
    }
}

/// One PTB buying every line, whatever venue each listing is on.
pub fn build_cart_buy(cfg: &VenueConfig, lines: &[BuyLine], ctx: &TradeContext) -> crate::Result<Tx> {
    if lines.is_empty() {
        return Err(crate::Error::new("Nothing to buy."));
    }
    let mut tx = Tx::new();
    tx.set_sender(&ctx.sender);
    let kiosk = to_user_kiosk(buyer_kiosk(cfg, lines, ctx));
    for line in lines {
        let t = line.listing.nft_type.clone().unwrap_or_default();
        let policy = to_venue_policy(cfg, choose_policy(cfg, policies_for(ctx, &t)), &t);
        let bc = build::BuyContext { sender: &ctx.sender, kiosk: kiosk.as_ref(), policy: policy.as_ref(), drip_direct: true, resale_game: line.resale_game.as_deref(), cfg };
        build_buy(&mut tx, &to_active_listing(&line.item_id, &line.listing), &bc).map_err(|e| friendly(e, &line.listing.venue, Action::Buy))?;
    }
    Ok(tx)
}

fn big_or0(v: &str) -> i128 {
    v.trim().parse::<i128>().unwrap_or(0)
}

/// royalty_rule::fee_amount: bps of `paid`, at least the minimum.
fn royalty_due(cfg: &VenueConfig, paid: i128, policy: Option<&TransferPolicy>) -> i128 {
    let Some(r) = read_policy(cfg, policy).royalty else { return 0 };
    let amount = paid * r.bps as i128 / 10000;
    if amount < r.min as i128 {
        r.min as i128
    } else {
        amount
    }
}

/// What the seller receives for an offer, before gas. rare.market (web/lib/sell/net.ts) takes the
/// price on every source; mainnet simulation of the ported builders refines two sources:
/// - tradeport/kiosk-bids: the seller pays the royalty (on the price) from gas and gets back the
///   bid's royalty reserve (escrow minus price minus commission). A bid placed under an older,
///   lower royalty nets less than its price.
/// - drip/v2-bids: the royalty is paid out of the sale proceeds.
///
/// Tradeport pooled bids and escrow bids pay everything from the bid: the price.
/// `policy` is the type's transfer policy (`Some(None)`: the type has none); with `None` the
/// reserve is assumed to cover the royalty.
pub fn seller_net_mist(cfg: &VenueConfig, o: &Offer, policy: Option<Option<&TransferPolicy>>) -> i128 {
    let price = big_or0(&o.price_mist);
    let Some(policy) = policy else { return price };
    if o.source == "tradeport/kiosk-bids" {
        let reserve = match o.extra.get("escrow").filter(|v| !v.is_null()) {
            None => royalty_due(cfg, price, policy),
            Some(e) => big_or0(&crate::api::json_num_str(e).unwrap_or_default()) - price - big_or0(&o.commission_mist),
        };
        return price + reserve.max(0) - royalty_due(cfg, price, policy);
    }
    if o.source == "drip/v2-bids" {
        return price - royalty_due(cfg, price, policy);
    }
    price
}

/// The seller's side of selling into an offer.
pub struct OfferSellContext<'a> {
    pub sender: &'a str,
    /// The seller's kiosk holding the item, or None for a wallet item.
    pub kiosk: Option<&'a OwnedKioskRef>,
    pub nft_id: &'a str,
    pub nft_type: &'a str,
    pub policies: &'a PoliciesByType,
    /// The item is listed with plain kiosk::list and must be delisted first.
    pub listed_native: bool,
}

/// Sell an item into an offer or bid on any venue. The item always reaches the bidder.
pub fn build_accept_offer(cfg: &VenueConfig, offer: &Offer, ctx: &OfferSellContext) -> crate::Result<Tx> {
    let mut tx = Tx::new();
    tx.set_sender(ctx.sender);
    let policy = to_venue_policy(cfg, choose_policy(cfg, ctx.policies.get(ctx.nft_type).map(|v| v.as_slice())), ctx.nft_type);
    let k = to_user_kiosk(ctx.kiosk);
    let nft = match &k {
        Some(k) => NftAt::Kiosk { kiosk: k, nft_id: ctx.nft_id, nft_type: ctx.nft_type, listed_native: ctx.listed_native },
        None => NftAt::Wallet { object_id: ctx.nft_id, nft_type: ctx.nft_type, kiosk: None },
    };
    let sc = build::SellContext { sender: ctx.sender, policy: policy.as_ref(), nft, cfg };
    build_accept_bid(&mut tx, &to_active_bid(offer), &sc).map_err(|e| friendly(e, &offer.venue, Action::Sell))?;
    Ok(tx)
}

/// Kiosks read from the chain (gRPC, `rare-sui`), for when rare.market cannot list them or trails
/// the chain by a few seconds.
pub trait KioskReader: Send + Sync {
    fn owned_kiosks(&self, owner: &str) -> crate::Result<Vec<OwnedKioskRef>>;
}

#[derive(serde::Deserialize)]
struct DataOf<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

/// What the builder needs for a sender: their kiosks and the policies of the given types.
/// `chain` stands in when rare.market can't list the kiosks.
pub fn load_trade_context(n: &dyn Net, sender: &str, nft_types: &[&str], chain: Option<&dyn KioskReader>) -> crate::Result<TradeContext> {
    let mut types: Vec<&str> = Vec::new();
    for t in nft_types.iter().filter(|t| !t.is_empty()) {
        if !types.contains(t) {
            types.push(t);
        }
    }
    let listed: Result<DataOf<OwnedKioskRef>, NetError> = net::get_market(n, &paths::kiosks(sender));
    let kiosks = match (listed, chain) {
        (Ok(k), _) => k.data,
        (Err(_), Some(c)) => c.owned_kiosks(sender)?,
        (Err(e), None) => return Err(e.into()),
    };
    let mut policies = PoliciesByType::new();
    for t in types {
        let p: DataOf<TransferPolicy> = net::get_market(n, &paths::policies(t))?;
        policies.insert(t.to_string(), p.data);
    }
    Ok(TradeContext { sender: sender.to_string(), buyer_kiosks: kiosks, policies })
}

/// GET /v1/offers?item= as a page.
pub fn offers_for_item(n: &dyn Net, item_id: &str, limit: u32) -> crate::Result<Vec<Offer>> {
    let p: Page<Offer> = net::get_market(n, &paths::offers_for_item(item_id, limit))?;
    Ok(p.data)
}

#[cfg(test)]
mod tests;
