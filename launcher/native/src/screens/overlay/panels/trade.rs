//! Selling from the overlay, the pure parts (overlay/trade.ts): which offers an item can be sold
//! into, what the seller receives, and the checks that come before a transaction is built. The
//! transaction builders need the Sui SDK and stay behind `TradeBackend`.

/// Bid sources the venue builders can fill (rare.market web/lib/sell/best-bid.ts SELLABLE).
const SELLABLE: [&str; 4] = ["tradeport/bids", "tradeport/escrow-bids", "tradeport/kiosk-bids", "drip/v2-bids"];
/// These take wallet-held items only (no kiosk).
const WALLET_ONLY: [&str; 1] = ["tradeport/escrow-bids"];
/// Sources whose seller net depends on the type's royalty.
const ROYALTY_SENSITIVE: [&str; 2] = ["tradeport/kiosk-bids", "drip/v2-bids"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OfferKind {
    Item,
    Collection,
    Trait,
}

/// An offer from /v1/offers (the fields selling reads).
#[derive(Clone, Debug)]
pub struct Offer {
    pub id: String,
    pub venue: String,
    pub source: String,
    pub nft_type: Option<String>,
    pub kind: OfferKind,
    pub item_id: Option<String>,
    pub price_mist: i128,
    pub commission_mist: i128,
    /// Tradeport kiosk bids: the bid's escrow.
    pub escrow: Option<i128>,
    /// The exact figure when the API or `best_offer` worked it out (extra.sellerNetMist).
    pub seller_net_mist: Option<i128>,
    pub quantity: u32,
    pub filled: u32,
    pub bidder: String,
    pub expires_at_ms: Option<f64>,
    pub open: bool,
    pub fillable: bool,
}

/// The royalty and kiosk rules of a type's transfer policy (policy.ts readPolicy, the parts the maths reads).
#[derive(Clone, Debug, Default)]
pub struct Policy {
    pub royalty_bps: i128,
    pub royalty_min: i128,
    pub has_royalty: bool,
    pub personal: bool,
}

/// Where the item is and who holds it.
#[derive(Clone, Debug, Default)]
pub struct ItemRef {
    pub id: String,
    pub nft_type: Option<String>,
    pub kiosk_id: Option<String>,
    pub owner: Option<String>,
}

pub fn norm_id(id: &str) -> String {
    format!("0x{:0>64}", id.trim_start_matches("0x").trim_start_matches("0X").to_ascii_lowercase())
}

fn same_id(a: Option<&str>, b: Option<&str>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() && norm_id(a) == norm_id(b))
}

/// normType: the address part padded, the rest as is, case folded.
pub fn norm_type(t: &str) -> String {
    match t.split_once("::") {
        Some((a, rest)) => format!("{}::{}", norm_id(a), rest.replace(' ', "")),
        None => t.to_string(),
    }
}

fn royalty_due(paid: i128, p: Option<&Policy>) -> i128 {
    let Some(p) = p.filter(|p| p.has_royalty) else { return 0 };
    let amount = paid * p.royalty_bps / 10_000;
    amount.max(p.royalty_min)
}

/// What the seller receives for an offer, before gas (venues sellerNetMist). `policy` is None when
/// the policy was not read (the reserve is assumed to cover the royalty), Some(None) when the type
/// has no usable policy.
pub fn seller_net_mist(o: &Offer, policy: Option<Option<&Policy>>) -> i128 {
    let price = o.price_mist;
    let Some(policy) = policy else { return price };
    if o.source == "tradeport/kiosk-bids" {
        let reserve = match o.escrow {
            None => royalty_due(price, policy),
            Some(e) => e - price - o.commission_mist,
        };
        return price + reserve.max(0) - royalty_due(price, policy);
    }
    if o.source == "drip/v2-bids" {
        return price - royalty_due(price, policy);
    }
    price
}

/// offerNetMist: the exact figure when known, else the price (or the policy-free maths).
pub fn offer_net_mist(o: &Offer) -> i128 {
    o.seller_net_mist.unwrap_or_else(|| seller_net_mist(o, None))
}

/// Whether this offer can take the item (best-bid.ts canFill).
pub fn can_fill(o: &Offer, item: &ItemRef, now_ms: f64) -> bool {
    if !o.open || !o.fillable {
        return false;
    }
    if o.filled >= o.quantity.max(1) {
        return false;
    }
    if o.expires_at_ms.is_some_and(|t| t <= now_ms) {
        return false;
    }
    if !SELLABLE.contains(&o.source.as_str()) {
        return false;
    }
    if WALLET_ONLY.contains(&o.source.as_str()) && item.kiosk_id.is_some() {
        return false;
    }
    if let (Some(a), Some(b)) = (&o.nft_type, &item.nft_type) {
        if norm_type(a) != norm_type(b) {
            return false;
        }
    }
    if same_id(item.owner.as_deref(), Some(&o.bidder)) {
        return false;
    }
    match o.kind {
        OfferKind::Item => same_id(o.item_id.as_deref(), Some(&item.id)),
        // Trait bids need the item's attributes; the API sends none for ?item= today.
        OfferKind::Collection => true,
        OfferKind::Trait => false,
    }
}

/// The best offer the item can be sold into right now. Ranked by what the seller receives; `policy`
/// is what the type's policy read returned (None: not read or failed).
pub fn best_offer(offers: &[Offer], item: &ItemRef, policy: Option<Option<&Policy>>, now_ms: f64) -> Option<Offer> {
    let mut fillable: Vec<Offer> = offers.iter().filter(|o| can_fill(o, item, now_ms)).cloned().collect();
    if let Some(pol) = policy {
        if fillable.iter().any(|o| ROYALTY_SENSITIVE.contains(&o.source.as_str())) {
            // Tradeport kiosk bids and Drip v2 bids can't deliver into the bidder's personal kiosk.
            if pol.is_some_and(|p| p.personal) {
                fillable.retain(|o| !ROYALTY_SENSITIVE.contains(&o.source.as_str()));
            }
            for o in fillable.iter_mut().filter(|o| ROYALTY_SENSITIVE.contains(&o.source.as_str())) {
                o.seller_net_mist = Some(seller_net_mist(o, Some(pol)));
            }
        }
    }
    let mut best: Option<Offer> = None;
    for o in fillable {
        if best.as_ref().is_none_or(|b| offer_net_mist(&o) > offer_net_mist(b)) {
            best = Some(o);
        }
    }
    best
}

/// An item as a listing check needs it.
#[derive(Clone, Debug, Default)]
pub struct Listed {
    pub venue: String,
}

fn venue_label(v: &str) -> String {
    match v {
        "tradeport" => "Tradeport".into(),
        "drip" => "Drip".into(),
        "market" => "Rare".into(),
        "kiosk" => "Kiosk".into(),
        o => o.into(),
    }
}

/// needOffVenue: a listing on Tradeport or Drip holds the item, so it comes off there first.
pub fn need_off_venue(l: Option<&Listed>) -> Result<(), String> {
    match l {
        Some(l) if l.venue != "kiosk" => Err(format!("Delist it on {} first.", venue_label(&l.venue))),
        _ => Ok(()),
    }
}

/// What buildListTransaction checks before it builds anything.
pub fn list_check(item: &ItemRef, price_mist: i128, listing: Option<&Listed>) -> Result<(), String> {
    if price_mist <= 0 {
        return Err("Price must be above zero.".into());
    }
    if item.nft_type.is_none() {
        return Err("This item is still being indexed. Try again in a minute.".into());
    }
    need_off_venue(listing)
}

/// What buildAcceptOfferTransaction checks before it builds anything.
pub fn accept_check(item: &ItemRef, o: &Offer, sender: &str, listing: Option<&Listed>) -> Result<(), String> {
    if item.nft_type.is_none() {
        return Err("This item is still being indexed. Try again in a minute.".into());
    }
    if !o.open || !o.fillable {
        return Err("This offer can't be filled any more.".into());
    }
    if let (Some(a), Some(b)) = (&o.nft_type, &item.nft_type) {
        if norm_type(a) != norm_type(b) {
            return Err("This offer is for another collection.".into());
        }
    }
    if o.kind == OfferKind::Item && o.item_id.is_some() && !same_id(o.item_id.as_deref(), Some(&item.id)) {
        return Err("This offer is for another item.".into());
    }
    if same_id(Some(&o.bidder), Some(sender)) {
        return Err("You can't accept your own offer.".into());
    }
    need_off_venue(listing)
}

/// panels.tsx plain(): what a failure says.
pub fn plain_error(m: &str) -> String {
    let l = m.to_ascii_lowercase();
    let rare_status = l.strip_prefix("rare.market ").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()));
    if rare_status || l.contains("failed to fetch") || l.contains("networkerror") || l.contains("load failed") {
        return "Couldn't reach rare.market. Check the connection and try again.".into();
    }
    if m.is_empty() {
        "Something went wrong. Try again.".into()
    } else {
        m.to_string()
    }
}

/// What a sale or listing came to (the Local API's sell answer).
#[derive(Clone, Debug, PartialEq)]
pub enum TradeResult {
    Sold { digest: String, received_mist: i128 },
    Listed { digest: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum TradeError {
    /// The signer's review sheet was dismissed: back to idle, no message.
    Rejected,
    Failed(String),
}

/// What the Sui SDK side provides once wired (rare-wallet): the best offer with its dry-run, and the
/// two signed transactions. The panels only call these through the app's Act handler.
pub trait TradeBackend {
    /// bestOffer(item) and, when the account can sign, the dry build of the sale: Err is the plain reason.
    fn best_offer(&self, item_id: &str) -> (Option<Offer>, Option<String>);
    fn sell(&self, item_id: &str, offer: &Offer) -> Result<TradeResult, TradeError>;
    fn list(&self, item_id: &str, price_mist: i128) -> Result<TradeResult, TradeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const SELLER: &str = "0x7682c8538863a72d81f09c1673d50ffda1220d5ec680f7f7f0c527eed0324fac";
    const BIDDER: &str = "0x3c2f7de4c6e2892a80d44b77fae4e14dcd95cf3b8f016c6b1f1da456df28587d";
    const RETRO: &str = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::retrosui::Nft";
    const ITEM: &str = "0x025231045a67bb6b53acdc6b178742e20853cf37bb7cc6b7114fb8df262b02f2";

    fn offer() -> Offer {
        Offer {
            id: "0x0f".into(),
            venue: "tradeport".into(),
            source: "tradeport/bids".into(),
            nft_type: Some(RETRO.into()),
            kind: OfferKind::Collection,
            item_id: None,
            price_mist: 111_000_000,
            commission_mist: 0,
            escrow: None,
            seller_net_mist: None,
            quantity: 1,
            filled: 0,
            bidder: BIDDER.into(),
            expires_at_ms: None,
            open: true,
            fillable: true,
        }
    }

    fn item(kiosk: bool) -> ItemRef {
        ItemRef { id: ITEM.into(), nft_type: Some(RETRO.into()), kiosk_id: kiosk.then(|| "0x1078".into()), owner: Some(SELLER.into()) }
    }

    fn lock_royalty() -> Policy {
        Policy { has_royalty: true, royalty_bps: 1000, royalty_min: 0, personal: false }
    }

    /// A live RetroSui kiosk bid placed at a 2% royalty reserve while the policy now takes 10%.
    fn stale() -> Offer {
        Offer { id: "stale".into(), source: "tradeport/kiosk-bids".into(), commission_mist: 30_000, price_mist: 1_000_000, escrow: Some(1_050_000), ..offer() }
    }

    #[test]
    fn the_price_on_most_sources_kiosk_bids_pay_the_royalty_gap() {
        assert_eq!(offer_net_mist(&offer()), 111_000_000);
        let pol = lock_royalty();
        assert_eq!(seller_net_mist(&stale(), Some(Some(&pol))), 920_000);
        let full = Offer { source: "tradeport/kiosk-bids".into(), price_mist: 800_000_000, commission_mist: 24_000_000, escrow: Some(904_000_000), ..offer() };
        assert_eq!(seller_net_mist(&full, Some(Some(&pol))), 800_000_000);
        let exact = Offer { seller_net_mist: Some(920_000), ..stale() };
        assert_eq!(offer_net_mist(&exact), 920_000);
    }

    #[test]
    fn picks_the_best_fillable_offer_by_net() {
        let other_item = Offer { id: "other".into(), price_mist: 7_000_000, kind: OfferKind::Item, item_id: Some(format!("0x{}", "99".repeat(32))), ..offer() };
        let offers = vec![
            Offer { id: "own".into(), price_mist: 9_000_000, bidder: SELLER.into(), ..offer() },
            Offer { id: "escrow".into(), price_mist: 8_000_000, source: "tradeport/escrow-bids".into(), ..offer() },
            other_item,
            Offer { id: "dead".into(), price_mist: 6_000_000, fillable: false, ..offer() },
            stale(),
            Offer { id: "good".into(), price_mist: 950_000, ..offer() },
        ];
        let pol = lock_royalty();
        let best = best_offer(&offers, &item(true), Some(Some(&pol)), 0.0).unwrap();
        assert_eq!(best.id, "good");
        // Without the policy read the stale kiosk bid would have looked like 1,000,000.
        let blind = best_offer(&offers, &item(true), None, 0.0).unwrap();
        assert_eq!(blind.id, "stale");
    }

    #[test]
    fn kiosk_bids_drop_for_personal_kiosk_types_and_nothing_fillable_is_none() {
        let personal = Policy { personal: true, ..lock_royalty() };
        assert!(best_offer(&[stale()], &item(true), Some(Some(&personal)), 0.0).is_none());
        assert!(best_offer(&[], &item(false), None, 0.0).is_none());
    }

    #[test]
    fn closed_expired_and_filled_offers_cannot_fill() {
        let i = item(false);
        assert!(can_fill(&offer(), &i, 0.0));
        assert!(!can_fill(&Offer { open: false, ..offer() }, &i, 0.0));
        assert!(!can_fill(&Offer { expires_at_ms: Some(10.0), ..offer() }, &i, 10.0));
        assert!(!can_fill(&Offer { filled: 1, ..offer() }, &i, 0.0));
        assert!(!can_fill(&Offer { nft_type: Some("0x2::other::Nft".into()), ..offer() }, &i, 0.0));
        assert!(!can_fill(&Offer { source: "market/v1".into(), ..offer() }, &i, 0.0));
        assert!(!can_fill(&Offer { source: "tradeport/escrow-bids".into(), ..offer() }, &item(true), 0.0));
    }

    #[test]
    fn listing_checks() {
        let tp = Listed { venue: "tradeport".into() };
        assert_eq!(list_check(&item(true), 1, Some(&tp)), Err("Delist it on Tradeport first.".into()));
        assert_eq!(list_check(&item(true), 1, Some(&Listed { venue: "drip".into() })), Err("Delist it on Drip first.".into()));
        assert_eq!(list_check(&item(false), 0, None), Err("Price must be above zero.".into()));
        assert_eq!(list_check(&item(false), 5, Some(&Listed { venue: "kiosk".into() })), Ok(()));
    }

    #[test]
    fn accepting_refuses_own_other_closed_and_elsewhere() {
        let i = item(false);
        assert_eq!(accept_check(&i, &Offer { bidder: SELLER.into(), ..offer() }, SELLER, None), Err("You can't accept your own offer.".into()));
        let other = Offer { kind: OfferKind::Item, item_id: Some(format!("0x{}", "99".repeat(32))), ..offer() };
        assert_eq!(accept_check(&i, &other, SELLER, None), Err("This offer is for another item.".into()));
        assert_eq!(accept_check(&i, &Offer { open: false, ..offer() }, SELLER, None), Err("This offer can't be filled any more.".into()));
        assert_eq!(accept_check(&i, &Offer { nft_type: Some("0x2::other::Nft".into()), ..offer() }, SELLER, None), Err("This offer is for another collection.".into()));
        assert_eq!(accept_check(&i, &offer(), SELLER, Some(&Listed { venue: "tradeport".into() })), Err("Delist it on Tradeport first.".into()));
        assert_eq!(accept_check(&i, &offer(), SELLER, None), Ok(()));
    }

    #[test]
    fn errors_read_plainly() {
        assert_eq!(plain_error("rare.market 503"), "Couldn't reach rare.market. Check the connection and try again.");
        assert_eq!(plain_error("Failed to fetch"), "Couldn't reach rare.market. Check the connection and try again.");
        assert_eq!(plain_error("Delist it on Drip first."), "Delist it on Drip first.");
        assert_eq!(plain_error(""), "Something went wrong. Try again.");
    }
}
