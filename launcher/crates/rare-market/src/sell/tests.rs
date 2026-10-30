//! The ported launcher/src/overlay/trade.test.ts: the PTB commands buildListTransaction and
//! buildAcceptOfferTransaction produce for each case, on fixtures shaped like live rare.market /v1
//! answers (a mock net, no network). trade.live.test.ts (mainnet simulation) needs `rare-sui`.

use super::*;
use crate::api::enc;
use crate::net::MockNet;
use crate::ptb::{Command, Input};
use crate::venues::ids::{kiosk_ext, tradeport};
use crate::venues::PolicyRule;
use serde_json::{json, Value};

pub(crate) const SELLER: &str = "0x7682c8538863a72d81f09c1673d50ffda1220d5ec680f7f7f0c527eed0324fac";
pub(crate) const BIDDER: &str = "0x3c2f7de4c6e2892a80d44b77fae4e14dcd95cf3b8f016c6b1f1da456df28587d";
pub(crate) const RETRO: &str = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::retrosui::Nft";
pub(crate) const ITEM: &str = "0x025231045a67bb6b53acdc6b178742e20853cf37bb7cc6b7114fb8df262b02f2";

fn rep(b: &str) -> String {
    format!("0x{}", b.repeat(32))
}

fn royalty() -> PolicyRule {
    serde_json::from_value(json!({"type": "royalty_rule", "ruleType": format!("{}::royalty_rule::Rule", kiosk_ext::ORIGIN), "config": {"amount_bp": 1000, "min_amount": "0"}})).unwrap()
}
fn lock() -> PolicyRule {
    serde_json::from_value(json!({"type": "kiosk_lock_rule", "ruleType": format!("{}::kiosk_lock_rule::Rule", kiosk_ext::ORIGIN)})).unwrap()
}
fn personal_rule() -> PolicyRule {
    serde_json::from_value(json!({"type": "personal_kiosk_rule", "ruleType": format!("{}::personal_kiosk_rule::Rule", kiosk_ext::PERSONAL_ORIGIN)})).unwrap()
}
pub(crate) fn pol_royalty_only() -> TransferPolicy {
    TransferPolicy { id: rep("ab"), is_shared: Some(true), rules: vec![royalty()] }
}
pub(crate) fn pol_lock_royalty() -> TransferPolicy {
    TransferPolicy { id: "0x7309bb80aee534f2b4334c9b97df0227412ed51e0a8b103fc3c8c751a925cf94".into(), is_shared: Some(true), rules: vec![lock(), royalty()] }
}
pub(crate) fn pol_personal() -> TransferPolicy {
    TransferPolicy { id: rep("cd"), is_shared: Some(true), rules: vec![personal_rule(), lock(), royalty()] }
}

pub(crate) fn plain_kiosk() -> OwnedKioskRef {
    OwnedKioskRef { kiosk_id: "0x10785ddeb5258be06802aed6d57ec411415d0a86902bf334cea00fd8573ee6f3".into(), owner_cap_id: rep("22"), personal_cap_id: None, is_personal: false }
}
pub(crate) fn personal_kiosk() -> OwnedKioskRef {
    OwnedKioskRef { kiosk_id: rep("33"), owner_cap_id: rep("44"), personal_cap_id: Some(rep("55")), is_personal: true }
}

/// What the rare.market stand-in answers: the seller's kiosks, one policy for the type (None: the
/// type has none), offers and the item.
#[derive(Default)]
pub(crate) struct Api {
    pub kiosks: Vec<OwnedKioskRef>,
    pub policy: Option<Option<TransferPolicy>>,
    pub offers: Vec<Offer>,
    pub item: Value,
}

pub(crate) fn api(a: Api) -> MockNet {
    let n = MockNet::new();
    n.on_read_json(&format!("/v1/accounts/{SELLER}/kiosks"), json!({"data": a.kiosks}));
    let pols = match a.policy {
        Some(None) => vec![],
        Some(Some(p)) => vec![p],
        None => vec![pol_royalty_only()],
    };
    n.on_read_json(&format!("/v1/policies/{}", enc(RETRO)), json!({"data": pols}));
    n.on_read_json(&format!("/v1/offers?item={ITEM}&limit=10"), json!({"data": a.offers}));
    let mut item = serde_json::to_value(card(json!({}))).unwrap();
    if let Some(o) = a.item.as_object() {
        for (k, v) in o {
            item[k] = v.clone();
        }
    }
    n.on_read_json(&format!("/v1/items/{ITEM}"), item);
    n
}

pub(crate) fn card(over: Value) -> ItemCard {
    let mut base = json!({
        "id": ITEM, "name": "RetroSui #1550", "type": RETRO, "imageUrl": null,
        "collection": {"slug": "retrosui", "name": "RetroSui", "imageUrl": null, "verified": false},
        "listing": null, "owner": SELLER, "kioskId": null
    });
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
    serde_json::from_value(base).unwrap()
}

pub(crate) fn native_listing() -> Value {
    json!({"venue": "kiosk", "source": "kiosk/native", "orderKey": format!("{}:{ITEM}", plain_kiosk().kiosk_id), "priceMist": "12000000", "kioskId": plain_kiosk().kiosk_id, "nftType": RETRO})
}

pub(crate) fn offer(over: Value) -> Offer {
    let mut base = json!({
        "id": rep("0f"), "venue": "tradeport", "source": "tradeport/bids", "nftType": RETRO,
        "feeMist": "3330000", "royaltyMist": "11100000", "commissionMist": "0",
        "poolId": "0x14af0f71ded139ff5b41cacd37b8b924e8298c9de37c6b2f8fde0aea3a66605e",
        "fillable": true, "extra": {"nftBcs": null, "balance": "0"}, "kind": "collection",
        "collection": {"slug": "retrosui", "name": "RetroSui", "imageUrl": null, "verified": false},
        "item": null, "trait": null, "priceMist": "111000000", "quantity": 1, "filled": 0,
        "bidder": BIDDER, "destKioskId": null, "createdAt": "2026-10-25T22:55:39.131Z", "expiresAt": null, "status": "open"
    });
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
    serde_json::from_value(base).unwrap()
}

/// Move calls as "module::function".
fn calls(tx: &Tx) -> Vec<String> {
    tx.move_calls().map(|m| format!("{}::{}", m.module, m.function)).collect()
}
fn kinds(tx: &Tx) -> Vec<&'static str> {
    tx.commands
        .iter()
        .map(|c| match c {
            Command::MoveCall(_) => "MoveCall",
            Command::SplitCoins { .. } => "SplitCoins",
            Command::MergeCoins { .. } => "MergeCoins",
            Command::TransferObjects { .. } => "TransferObjects",
            Command::MakeMoveVec { .. } => "MakeMoveVec",
            Command::CoinWithBalance { .. } => "$Intent",
        })
        .collect()
}
fn pkg_of(tx: &Tx, f: &str) -> Option<String> {
    tx.move_calls().find(|m| format!("{}::{}", m.module, m.function) == f).map(|m| m.package.clone())
}
/// The u64 pure inputs in input order (prices).
fn u64s(tx: &Tx) -> Vec<u64> {
    tx.inputs
        .iter()
        .filter_map(|i| match i {
            Input::Pure(b) if b.len() == 8 => Some(u64::from_le_bytes(b[..].try_into().unwrap())),
            _ => None,
        })
        .collect()
}
fn norm(id: &str) -> String {
    crate::ptb::norm_addr(id)
}
fn cfg() -> VenueConfig {
    VenueConfig::default()
}

// ---------- buildListTransaction ----------

#[test]
fn list_wallet_item_without_kiosk_makes_one() {
    let tx = build_list_transaction(&api(Api::default()), &cfg(), &card(json!({})), 5_000_000_000, SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk::new", "kiosk::place", "kiosk::list", "transfer::public_share_object"]);
    assert_eq!(*kinds(&tx).last().unwrap(), "TransferObjects");
    assert!(u64s(&tx).contains(&5_000_000_000));
    assert_eq!(tx.sender.as_deref(), Some(SELLER));
}

#[test]
fn list_item_already_in_kiosk_lists_only() {
    let tx = build_list_transaction(&api(Api { kiosks: vec![plain_kiosk()], ..Default::default() }), &cfg(), &card(json!({"kioskId": plain_kiosk().kiosk_id})), 1_000_000_000, SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk::list"]);
    assert_eq!(kinds(&tx), ["MoveCall"]);
}

#[test]
fn list_lock_rule_locks_wallet_item_with_policy() {
    let tx = build_list_transaction(&api(Api { kiosks: vec![plain_kiosk()], policy: Some(Some(pol_lock_royalty())), ..Default::default() }), &cfg(), &card(json!({})), 1_000_000_000, SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk::lock", "kiosk::list"]);
    let objs: Vec<String> = tx.object_ids().iter().map(|s| norm(s)).collect();
    assert!(objs.contains(&norm(&pol_lock_royalty().id)));
    assert!(objs.contains(&norm(&plain_kiosk().kiosk_id)));
}

#[test]
fn list_personal_policy_makes_personal_kiosk() {
    let tx = build_list_transaction(&api(Api { kiosks: vec![plain_kiosk()], policy: Some(Some(pol_personal())), ..Default::default() }), &cfg(), &card(json!({})), 1_000_000_000, SELLER, None).unwrap();
    assert_eq!(
        calls(&tx),
        ["kiosk::new", "personal_kiosk::new", "personal_kiosk::borrow_val", "kiosk::lock", "kiosk::list", "personal_kiosk::return_val", "personal_kiosk::transfer_to_sender", "transfer::public_share_object"]
    );
    assert_eq!(pkg_of(&tx, "personal_kiosk::new").as_deref(), Some(kiosk_ext::LATEST));
}

#[test]
fn list_personal_policy_uses_existing_personal_kiosk() {
    let tx = build_list_transaction(&api(Api { kiosks: vec![plain_kiosk(), personal_kiosk()], policy: Some(Some(pol_personal())), ..Default::default() }), &cfg(), &card(json!({})), 1_000_000_000, SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["personal_kiosk::borrow_val", "kiosk::lock", "kiosk::list", "personal_kiosk::return_val"]);
}

#[test]
fn relist_native_listing_delists_then_lists() {
    let tx = build_list_transaction(&api(Api { kiosks: vec![plain_kiosk()], ..Default::default() }), &cfg(), &card(json!({"kioskId": plain_kiosk().kiosk_id, "listing": native_listing()})), 20_000_000, SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk::delist", "kiosk::list"]);
    assert_eq!(u64s(&tx), [20_000_000]);
}

#[test]
fn listed_elsewhere_must_be_delisted_there() {
    let n = api(Api { kiosks: vec![plain_kiosk()], ..Default::default() });
    let e = build_list_transaction(&n, &cfg(), &card(json!({"kioskId": plain_kiosk().kiosk_id, "listing": {"venue": "tradeport", "priceMist": "1"}})), 1, SELLER, None).unwrap_err();
    assert_eq!(e.0, "Delist it on Tradeport first.");
    let e = build_list_transaction(&n, &cfg(), &card(json!({"kioskId": plain_kiosk().kiosk_id, "listing": {"venue": "drip", "priceMist": "1"}})), 1, SELLER, None).unwrap_err();
    assert_eq!(e.0, "Delist it on Drip first.");
    assert!(n.reads().is_empty());
}

#[test]
fn zero_price_refused() {
    assert_eq!(build_list_transaction(&api(Api::default()), &cfg(), &card(json!({})), 0, SELLER, None).unwrap_err().0, "Price must be above zero.");
}

// ---------- buildAcceptOfferTransaction ----------

#[test]
fn tradeport_bids_natively_listed_delist_accept_confirm() {
    let n = api(Api { kiosks: vec![plain_kiosk()], policy: Some(Some(pol_lock_royalty())), ..Default::default() });
    let tx = build_accept_offer_transaction(&n, &cfg(), &card(json!({"kioskId": plain_kiosk().kiosk_id, "listing": native_listing()})), &offer(json!({})), SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk::delist", "tradeport_biddings::accept_bid_with_transfer_policy", "transfer_policy::confirm_request"]);
    assert_eq!(pkg_of(&tx, "tradeport_biddings::accept_bid_with_transfer_policy").as_deref(), Some(tradeport::bids::LATEST));
}

#[test]
fn tradeport_bids_from_wallet_new_kiosk_shared() {
    let n = api(Api { kiosks: vec![plain_kiosk()], policy: Some(Some(pol_lock_royalty())), ..Default::default() });
    let tx = build_accept_offer_transaction(&n, &cfg(), &card(json!({})), &offer(json!({})), SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk::new", "kiosk::place", "tradeport_biddings::accept_bid_with_transfer_policy", "transfer_policy::confirm_request", "transfer::public_share_object"]);
}

#[test]
fn tradeport_bids_from_wallet_without_policy_plain_accept() {
    let n = api(Api { policy: Some(None), ..Default::default() });
    let tx = build_accept_offer_transaction(&n, &cfg(), &card(json!({})), &offer(json!({})), SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["tradeport_biddings::accept_bid_without_transfer_policy"]);
}

#[test]
fn tradeport_kiosk_bids_escrow_kiosk_royalty_lock_leftover() {
    let o = offer(json!({"source": "tradeport/kiosk-bids", "kind": "item", "item": {"id": ITEM, "name": "RetroSui #1550", "imageUrl": ""}, "poolId": null, "feeMist": "0", "royaltyMist": "0", "commissionMist": "24000000", "priceMist": "800000000", "extra": {"escrow": "904000000"}}));
    let n = api(Api { kiosks: vec![plain_kiosk()], policy: Some(Some(pol_lock_royalty())), ..Default::default() });
    let tx = build_accept_offer_transaction(&n, &cfg(), &card(json!({"kioskId": plain_kiosk().kiosk_id})), &o, SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk_biddings::accept_bid", "transfer_policy::paid", "royalty_rule::fee_amount", "royalty_rule::pay", "kiosk_lock_rule::prove", "transfer_policy::confirm_request"]);
    assert_eq!(*kinds(&tx).last().unwrap(), "TransferObjects");
    let objs: Vec<String> = tx.object_ids().iter().map(|s| norm(s)).collect();
    assert!(objs.contains(&norm(tradeport::kiosk::ESCROW_KIOSK)));
}

#[test]
fn tradeport_escrow_bids_take_wallet_item() {
    let tx = build_accept_offer_transaction(&api(Api::default()), &cfg(), &card(json!({})), &offer(json!({"source": "tradeport/escrow-bids", "poolId": null})), SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["biddings::accept_bid"]);
    assert_eq!(pkg_of(&tx, "biddings::accept_bid").as_deref(), Some(tradeport::escrow::LATEST));
}

#[test]
fn drip_v2_bids_accept_withdraw_settle_for_bidder() {
    let o = offer(json!({"venue": "drip", "source": "drip/v2-bids", "kind": "item", "item": {"id": ITEM, "name": "", "imageUrl": ""}, "poolId": null}));
    let n = api(Api { kiosks: vec![plain_kiosk()], policy: Some(Some(pol_lock_royalty())), ..Default::default() });
    let tx = build_accept_offer_transaction(&n, &cfg(), &card(json!({"kioskId": plain_kiosk().kiosk_id})), &o, SELLER, None).unwrap();
    assert_eq!(
        calls(&tx),
        ["bids::accept_offer", "kiosk::withdraw", "kiosk::new", "kiosk::lock", "transfer_policy::paid", "royalty_rule::fee_amount", "royalty_rule::pay", "kiosk_lock_rule::prove", "transfer_policy::confirm_request", "transfer::public_share_object"]
    );
}

#[test]
fn unsupported_venues_one_plain_sentence() {
    let n = api(Api { kiosks: vec![plain_kiosk()], policy: Some(Some(pol_personal())), ..Default::default() });
    let in_kiosk = card(json!({"kioskId": plain_kiosk().kiosk_id}));
    let e = |o: Offer| build_accept_offer_transaction(&n, &cfg(), &in_kiosk, &o, SELLER, None).unwrap_err().0;
    assert_eq!(e(offer(json!({"venue": "market", "source": "market/v1"}))), "Selling into offers on Rare is not available for this item yet.");
    assert_eq!(e(offer(json!({"source": "tradeport/multi-bids"}))), "Selling into offers on Tradeport is not available for this item yet.");
    // Escrow bids take wallet items only; kiosk bids can't satisfy a personal kiosk rule.
    assert_eq!(e(offer(json!({"source": "tradeport/escrow-bids"}))), "Selling into offers on Tradeport is not available for this item yet.");
    assert_eq!(e(offer(json!({"source": "tradeport/kiosk-bids"}))), "Selling into offers on Tradeport is not available for this item yet.");
}

#[test]
fn refuses_own_other_closed_and_elsewhere() {
    let n = api(Api { kiosks: vec![plain_kiosk()], ..Default::default() });
    let e = |c: ItemCard, o: Offer| build_accept_offer_transaction(&n, &cfg(), &c, &o, SELLER, None).unwrap_err().0;
    assert_eq!(e(card(json!({})), offer(json!({"bidder": SELLER}))), "You can't accept your own offer.");
    assert_eq!(e(card(json!({})), offer(json!({"kind": "item", "item": {"id": rep("99"), "name": "", "imageUrl": ""}}))), "This offer is for another item.");
    assert_eq!(e(card(json!({})), offer(json!({"status": "cancelled"}))), "This offer can't be filled any more.");
    assert_eq!(e(card(json!({})), offer(json!({"nftType": "0x2::other::Nft"}))), "This offer is for another collection.");
    assert_eq!(e(card(json!({"kioskId": plain_kiosk().kiosk_id, "listing": {"venue": "tradeport", "priceMist": "1"}})), offer(json!({}))), "Delist it on Tradeport first.");
}

// ---------- offerNetMist and bestOffer ----------

/// A live RetroSui kiosk bid placed at a 2% royalty reserve while the policy now takes 10%.
fn stale() -> Offer {
    offer(json!({"id": "0x04f5", "source": "tradeport/kiosk-bids", "poolId": null, "feeMist": "0", "royaltyMist": "0", "commissionMist": "30000", "priceMist": "1000000", "extra": {"escrow": "1050000"}}))
}

#[test]
fn net_is_the_price_except_kiosk_bid_royalty_gap() {
    let c = cfg();
    assert_eq!(offer_net_mist(&offer(json!({}))), 111_000_000);
    assert_eq!(seller_net_mist(&c, &stale(), Some(Some(&pol_lock_royalty()))), 920_000);
    let o = offer(json!({"source": "tradeport/kiosk-bids", "priceMist": "800000000", "commissionMist": "24000000", "extra": {"escrow": "904000000"}}));
    assert_eq!(seller_net_mist(&c, &o, Some(Some(&pol_lock_royalty()))), 800_000_000);
    let mut s = stale();
    s.extra.insert("sellerNetMist".into(), json!("920000"));
    assert_eq!(offer_net_mist(&s), 920_000);
}

#[test]
fn best_offer_by_net_skipping_unfillable() {
    let mut st = stale();
    st.id = "stale".into();
    let offers = vec![
        offer(json!({"id": "own", "priceMist": "9000000", "bidder": SELLER})),
        offer(json!({"id": "escrow", "priceMist": "8000000", "source": "tradeport/escrow-bids"})),
        offer(json!({"id": "other", "priceMist": "7000000", "kind": "item", "item": {"id": rep("99"), "name": "", "imageUrl": ""}})),
        offer(json!({"id": "dead", "priceMist": "6000000", "fillable": false})),
        st,
        offer(json!({"id": "good", "priceMist": "950000"})),
    ];
    let n = api(Api { offers, item: json!({"kioskId": plain_kiosk().kiosk_id}), policy: Some(Some(pol_lock_royalty())), ..Default::default() });
    let best = best_offer(&n, &cfg(), ITEM, crate::time::now_ms()).unwrap();
    assert_eq!(best.unwrap().id, "good");
    assert_eq!(n.reads()[0], format!("rare:/v1/offers?item={ITEM}&limit=10"));
    // Without the policy read the stale kiosk bid would have looked like 1,000,000.
    assert!(n.reads().iter().any(|p| p.starts_with("rare:/v1/policies/")));
}

#[test]
fn best_offer_drops_kiosk_bids_for_personal_types_and_none_when_empty() {
    let n = api(Api { offers: vec![stale()], item: json!({"kioskId": plain_kiosk().kiosk_id}), policy: Some(Some(pol_personal())), ..Default::default() });
    assert!(best_offer(&n, &cfg(), ITEM, crate::time::now_ms()).unwrap().is_none());
    assert!(best_offer(&api(Api::default()), &cfg(), ITEM, crate::time::now_ms()).unwrap().is_none());
}

// ---------- cancel (inventory/trade.ts) ----------

#[test]
fn cancel_native_listing_delists_in_own_kiosk() {
    let n = api(Api { kiosks: vec![plain_kiosk()], ..Default::default() });
    let tx = build_cancel_transaction(&n, &card(json!({"kioskId": plain_kiosk().kiosk_id, "listing": native_listing()})), SELLER, None).unwrap();
    assert_eq!(calls(&tx), ["kiosk::delist"]);
    assert_eq!(build_cancel_transaction(&n, &card(json!({})), SELLER, None).unwrap_err().0, "This item isn't listed.");
    assert_eq!(build_cancel_transaction(&n, &card(json!({"listing": {"venue": "drip", "priceMist": "1"}})), SELLER, None).unwrap_err().0, "Cancel it where it is listed.");
}
