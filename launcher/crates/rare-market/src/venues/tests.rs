//! The ported launcher/src/marketplace/venues/venues.test.ts: the PTB commands per listing source
//! on fixtures shaped like live /v1 listings. The mainnet simulation (live.test.ts) is not ported:
//! it needs `rare-sui`'s dry run.

use super::ids::{kiosk_ext, tradeport};
use super::*;
use crate::net::MockNet;
use serde_json::json;

pub(crate) const BUYER: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
pub(crate) const RETRO: &str = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::retrosui::Nft";
pub(crate) const ITEM: &str = "0x025231045a67bb6b53acdc6b178742e20853cf37bb7cc6b7114fb8df262b02f2";
pub(crate) const SELLER_KIOSK: &str = "0x10785ddeb5258be06802aed6d57ec411415d0a86902bf334cea00fd8573ee6f3";
pub(crate) const EXT: &str = "0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879";

pub(crate) fn rep(b: &str) -> String {
    format!("0x{}", b.repeat(32))
}

pub(crate) fn policy_json(v: serde_json::Value) -> TransferPolicy {
    serde_json::from_value(v).unwrap()
}

pub(crate) fn lock_royalty() -> TransferPolicy {
    policy_json(json!({
        "id": "0x7309bb80aee534f2b4334c9b97df0227412ed51e0a8b103fc3c8c751a925cf94",
        "isShared": true,
        "rules": [
            {"type": "kiosk_lock_rule", "ruleType": format!("{EXT}::kiosk_lock_rule::Rule")},
            {"type": "royalty_rule", "ruleType": format!("{EXT}::royalty_rule::Rule"), "config": {"amount_bp": 1000, "min_amount": "0"}}
        ]
    }))
}
pub(crate) fn royalty_only() -> TransferPolicy {
    TransferPolicy { id: rep("ab"), is_shared: Some(true), rules: vec![lock_royalty().rules[1].clone()] }
}
pub(crate) fn personal() -> TransferPolicy {
    let mut rules = vec![PolicyRule { kind: "personal_kiosk_rule".into(), rule_type: Some(format!("{}::personal_kiosk_rule::Rule", kiosk_ext::PERSONAL_ORIGIN)), config: None }];
    rules.extend(lock_royalty().rules);
    TransferPolicy { id: rep("cd"), is_shared: Some(true), rules }
}
fn custom() -> TransferPolicy {
    TransferPolicy { id: rep("ee"), is_shared: Some(true), rules: vec![PolicyRule { kind: "witness_rule".into(), rule_type: Some(format!("{}::witness_rule::Rule", rep("12"))), config: None }] }
}

pub(crate) fn plain_kiosk() -> OwnedKioskRef {
    OwnedKioskRef { kiosk_id: rep("11"), owner_cap_id: rep("22"), personal_cap_id: None, is_personal: false }
}
pub(crate) fn personal_kiosk() -> OwnedKioskRef {
    OwnedKioskRef { kiosk_id: rep("33"), owner_cap_id: rep("44"), personal_cap_id: Some(rep("55")), is_personal: true }
}

/// The test fixture listing with `over` merged in (JSON keys as the API sends them).
pub(crate) fn listing(over: serde_json::Value) -> Listing {
    let mut base = json!({
        "venue": "kiosk",
        "source": "kiosk/native",
        "orderKey": format!("{SELLER_KIOSK}:{ITEM}"),
        "objectId": null,
        "priceMist": "12000000",
        "allInMist": "13200000",
        "feeMist": "0",
        "commissionMist": "0",
        "seller": "",
        "kioskId": SELLER_KIOSK,
        "nftType": RETRO,
        "extra": {"royaltyMist": "1200000"}
    });
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
    serde_json::from_value(base).unwrap()
}

/// Move calls as "module::function", short package aliases for the venues.
pub(crate) fn calls(tx: &Tx) -> Vec<String> {
    let alias = |p: &str| -> String {
        let n = format!("0x{}", p.trim_start_matches("0x").trim_start_matches('0'));
        if n == "0x2" {
            return "0x2".into();
        }
        if p == kiosk_ext::LATEST {
            return "ext".into();
        }
        for (name, latest) in [("escrow", tradeport::escrow::LATEST), ("kiosk", tradeport::kiosk::LATEST), ("kioskEarly", tradeport::kiosk_early::LATEST), ("bids", tradeport::bids::LATEST), ("simple", tradeport::simple::LATEST)] {
            if latest == p {
                return format!("tp.{name}");
            }
        }
        p[..6].to_string()
    };
    tx.move_calls().map(|m| format!("{}::{}::{}", alias(&m.package), m.module, m.function)).collect()
}

fn ctx(policies: Vec<TransferPolicy>, kiosks: Vec<OwnedKioskRef>) -> TradeContext {
    let mut p = PoliciesByType::new();
    p.insert(RETRO.into(), policies);
    TradeContext { sender: BUYER.into(), buyer_kiosks: kiosks, policies: p }
}

fn line(l: Listing) -> Vec<BuyLine> {
    vec![BuyLine { item_id: ITEM.into(), listing: l, resale_game: None }]
}

fn cfg() -> VenueConfig {
    VenueConfig::default()
}

#[test]
fn kiosk_native_purchase_lock_pay_royalty_prove_confirm() {
    let tx = build_cart_buy(&cfg(), &line(listing(json!({}))), &ctx(vec![lock_royalty()], vec![plain_kiosk()])).unwrap();
    assert_eq!(
        calls(&tx),
        ["0x2::kiosk::purchase", "0x2::kiosk::lock", "0x2::transfer_policy::paid", "ext::royalty_rule::fee_amount", "ext::royalty_rule::pay", "ext::kiosk_lock_rule::prove", "0x2::transfer_policy::confirm_request"]
    );
    assert_eq!(tx.split_amounts(), [12_000_000]);
    assert_eq!(tx.sender.as_deref(), Some(BUYER));
}

#[test]
fn kiosk_native_without_buyer_kiosk_creates_and_shares_one() {
    let tx = build_cart_buy(&cfg(), &line(listing(json!({}))), &ctx(vec![royalty_only()], vec![])).unwrap();
    let c = calls(&tx);
    assert_eq!(c[0], "0x2::kiosk::new");
    assert!(c.contains(&"0x2::kiosk::place".to_string()));
    assert!(!c.contains(&"0x2::kiosk::lock".to_string()));
    assert_eq!(c.last().unwrap(), "0x2::transfer::public_share_object");
    assert!(matches!(tx.commands.last(), Some(crate::ptb::Command::TransferObjects { .. })));
}

#[test]
fn personal_kiosk_rule_borrows_proves_returns() {
    let tx = build_cart_buy(&cfg(), &line(listing(json!({}))), &ctx(vec![personal()], vec![plain_kiosk(), personal_kiosk()])).unwrap();
    let c = calls(&tx);
    assert_eq!(c[0], "ext::personal_kiosk::borrow_val");
    assert!(c.contains(&"ext::personal_kiosk_rule::prove".to_string()));
    assert_eq!(c.last().unwrap(), "ext::personal_kiosk::return_val");
}

#[test]
fn tradeport_simple_kiosk_listing_fee_and_royalty_in_payment() {
    let l = listing(json!({
        "venue": "tradeport", "source": "tradeport/simple", "orderKey": ITEM, "objectId": rep("77"),
        "priceMist": "1000000000", "feeMist": "30000000", "allInMist": "1130000000", "kioskId": rep("88"),
        "extra": {"feeBps": 300, "inOrderbook": true, "listingType": 1, "royaltyMist": "100000000"}
    }));
    let tx = build_cart_buy(&cfg(), &line(l), &ctx(vec![lock_royalty()], vec![plain_kiosk()])).unwrap();
    assert_eq!(calls(&tx), ["ext::royalty_rule::fee_amount", "tp.simple::tradeport_listings::buy_ob_listing_with_transfer_policy", "0x2::transfer_policy::confirm_request"]);
    assert_eq!(tx.split_amounts(), [1_030_000_000]);
}

#[test]
fn tradeport_simple_wallet_listing_reports_to_orderbook_first() {
    let l = listing(json!({"venue": "tradeport", "source": "tradeport/simple", "orderKey": ITEM, "priceMist": "500000000", "feeMist": "15000000", "kioskId": null, "extra": {"inOrderbook": false, "listingType": 0}}));
    let tx = build_cart_buy(&cfg(), &line(l), &ctx(vec![], vec![plain_kiosk()])).unwrap();
    assert_eq!(calls(&tx), ["tp.simple::tradeport_listings::report_ob_listing", "tp.simple::tradeport_listings::buy_ob_listing_without_transfer_policy"]);
    assert_eq!(tx.split_amounts(), [515_000_000]);
}

#[test]
fn tradeport_kiosk_legacy_buy_then_settle() {
    let l = listing(json!({"venue": "tradeport", "source": "tradeport/kiosk", "orderKey": ITEM, "priceMist": "226600000", "commissionMist": "6600000", "kioskId": rep("99"), "extra": {}}));
    let tx = build_cart_buy(&cfg(), &line(l.clone()), &ctx(vec![lock_royalty()], vec![plain_kiosk()])).unwrap();
    let c = calls(&tx);
    assert_eq!(c[0], "tp.kiosk::kiosk_listings::buy");
    assert_eq!(c[1..], ["0x2::kiosk::lock", "0x2::transfer_policy::paid", "ext::royalty_rule::fee_amount", "ext::royalty_rule::pay", "ext::kiosk_lock_rule::prove", "0x2::transfer_policy::confirm_request"]);
    assert_eq!(tx.split_amounts(), [226_600_000]);
    // royalty on price minus commission, as the API prices it (RetroSui #76: 220 * 10% = 22)
    let p = to_venue_policy(&cfg(), Some(&lock_royalty()), RETRO);
    assert_eq!(all_in_price(&to_active_listing(ITEM, &l), p.as_ref(), true), 248_600_000);
}

#[test]
fn tradeport_kiosk_early_commission_on_top() {
    let l = listing(json!({"venue": "tradeport", "source": "tradeport/kiosk-early", "priceMist": "100", "commissionMist": "3", "kioskId": rep("99")}));
    let tx = build_cart_buy(&cfg(), &line(l), &ctx(vec![royalty_only()], vec![plain_kiosk()])).unwrap();
    assert_eq!(calls(&tx)[0], "tp.kioskEarly::kiosk_listings::buy");
    assert_eq!(tx.split_amounts(), [103]);
}

#[test]
fn tradeport_escrow_by_listing_id_no_policy() {
    let l = listing(json!({"venue": "tradeport", "source": "tradeport/escrow", "orderKey": rep("66"), "kioskId": null, "priceMist": "5000"}));
    let tx = build_cart_buy(&cfg(), &line(l.clone()), &ctx(vec![], vec![plain_kiosk()])).unwrap();
    assert_eq!(calls(&tx), ["tp.escrow::listings::buy"]);
    let p = to_venue_policy(&cfg(), Some(&lock_royalty()), RETRO);
    assert_eq!(all_in_price(&to_active_listing(ITEM, &l), p.as_ref(), true), 5000);
}

#[test]
fn drip_v2_direct_then_record_cleared() {
    let l = listing(json!({"venue": "drip", "source": "drip/v2", "feeMist": "240000"}));
    let tx = build_cart_buy(&cfg(), &line(l), &ctx(vec![lock_royalty()], vec![plain_kiosk()])).unwrap();
    let c = calls(&tx);
    assert_eq!(c[0], "0x2::kiosk::purchase");
    assert_eq!(c[c.len() - 2..], ["0x2::kiosk::list", "0xe503::marketplace::delist"]);
    assert_eq!(tx.split_amounts(), [12_000_000]);
}

#[test]
fn market_v1_unpublished_is_plainly_unsupported() {
    let e = build_cart_buy(&cfg(), &line(listing(json!({"venue": "market", "source": "market/v1"}))), &ctx(vec![lock_royalty()], vec![plain_kiosk()])).unwrap_err();
    assert_eq!(e.0, "Buying from Rare is not available for this item yet.");
}

#[test]
fn unprovable_policy_never_chosen() {
    let c = cfg();
    assert!(choose_policy(&c, Some(&[custom()])).is_none());
    let both = [custom(), lock_royalty()];
    assert_eq!(choose_policy(&c, Some(&both)).unwrap().id, lock_royalty().id);
    let e = build_cart_buy(&c, &line(listing(json!({}))), &ctx(vec![custom()], vec![plain_kiosk()])).unwrap_err();
    assert_eq!(e.0, "Buying from Kiosk is not available for this item yet.");
}

struct Chain(Vec<OwnedKioskRef>);
impl KioskReader for Chain {
    fn owned_kiosks(&self, _: &str) -> crate::Result<Vec<OwnedKioskRef>> {
        Ok(self.0.clone())
    }
}

#[test]
fn load_trade_context_reads_rare_paths_with_chain_fallback() {
    let n = MockNet::new();
    n.fail_read(&format!("/v1/accounts/{BUYER}/kiosks"), crate::net::NetError::Status("rare.market 502".into()));
    n.on_read_json(&format!("/v1/policies/{}", crate::api::enc(RETRO)), json!({"data": [lock_royalty()]}));
    let c = load_trade_context(&n, BUYER, &[RETRO, RETRO], Some(&Chain(vec![plain_kiosk()]))).unwrap();
    assert_eq!(n.reads(), [format!("rare:/v1/accounts/{BUYER}/kiosks"), format!("rare:/v1/policies/{}", crate::api::enc(RETRO))]);
    assert_eq!(c.buyer_kiosks, [plain_kiosk()]);
    assert_eq!(c.policies[RETRO], [lock_royalty()]);
}

#[test]
fn resale_licence_lock_prove_resale_confirm() {
    let origin = rep("9a");
    let pkg = rep("9b");
    let game = rep("9c");
    let licence = format!("{origin}::licence::ResaleLicence");
    let policy = TransferPolicy { id: rep("9d"), is_shared: Some(true), rules: vec![lock_royalty().rules[0].clone(), PolicyRule { kind: "rule".into(), rule_type: Some(format!("{origin}::resale::Rule")), config: None }] };
    let lic = || listing(json!({"nftType": licence}));
    let c = || {
        let mut p = PoliciesByType::new();
        p.insert(licence.clone(), vec![policy.clone()]);
        TradeContext { sender: BUYER.into(), buyer_kiosks: vec![plain_kiosk()], policies: p }
    };
    let with_game = || vec![BuyLine { item_id: ITEM.into(), listing: lic(), resale_game: Some(game.clone()) }];
    // Until the store config has a package the rule is unknown: not buyable.
    let mut cf = cfg();
    assert!(build_cart_buy(&cf, &with_game(), &c()).unwrap_err().0.contains("not available"));
    cf.resale.configure(Some(&origin), Some(&pkg));
    let tx = build_cart_buy(&cf, &with_game(), &c()).unwrap();
    assert_eq!(calls(&tx), ["0x2::kiosk::purchase", "0x2::kiosk::lock", "ext::kiosk_lock_rule::prove", &format!("{}::resale::prove", &pkg[..6]), "0x2::transfer_policy::confirm_request"]);
    // The game id is required.
    let no_game = vec![BuyLine { item_id: ITEM.into(), listing: lic(), resale_game: None }];
    assert!(build_cart_buy(&cf, &no_game, &c()).unwrap_err().0.contains("not available"));
}
