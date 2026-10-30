//! Byte-for-byte parity with the web launcher's builders: every case in golden/ptb.test.ts, built
//! here, must print the same `getData()` JSON the TypeScript printed into golden/ptb.json (same
//! inputs in the same order, same commands, same BCS bytes).

use crate::api::{enc, ItemCard, Listing, Offer};
use crate::net::MockNet;
use crate::ptb::Tx;
use crate::sell::{build_accept_offer_transaction, build_cancel_transaction, build_list_transaction};
use crate::venues::ids::kiosk_ext;
use crate::venues::{build_cart_buy, BuyLine, OwnedKioskRef, PoliciesByType, TradeContext, TransferPolicy, VenueConfig};
use serde_json::{json, Value};

const BUYER: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const SELLER: &str = "0x7682c8538863a72d81f09c1673d50ffda1220d5ec680f7f7f0c527eed0324fac";
const BIDDER: &str = "0x3c2f7de4c6e2892a80d44b77fae4e14dcd95cf3b8f016c6b1f1da456df28587d";
const RETRO: &str = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::retrosui::Nft";
const ITEM: &str = "0x025231045a67bb6b53acdc6b178742e20853cf37bb7cc6b7114fb8df262b02f2";
const SELLER_KIOSK: &str = "0x10785ddeb5258be06802aed6d57ec411415d0a86902bf334cea00fd8573ee6f3";

fn rep(b: &str) -> String {
    format!("0x{}", b.repeat(32))
}

fn merge(mut base: Value, over: Value) -> Value {
    for (k, v) in over.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

fn pol(name: &str) -> TransferPolicy {
    let ext = kiosk_ext::ORIGIN;
    let royalty = json!({"type": "royalty_rule", "ruleType": format!("{ext}::royalty_rule::Rule"), "config": {"amount_bp": 1000, "min_amount": "0"}});
    let lock = json!({"type": "kiosk_lock_rule", "ruleType": format!("{ext}::kiosk_lock_rule::Rule")});
    let personal = json!({"type": "personal_kiosk_rule", "ruleType": format!("{}::personal_kiosk_rule::Rule", kiosk_ext::PERSONAL_ORIGIN)});
    let floor = json!({"type": "floor_price_rule", "ruleType": format!("{}::floor_price_rule::Rule", kiosk_ext::FLOOR_ORIGIN), "config": {"floor_price": "5"}});
    let v = match name {
        "royaltyOnly" => json!({"id": rep("ab"), "isShared": true, "rules": [royalty]}),
        "lockRoyalty" => json!({"id": "0x7309bb80aee534f2b4334c9b97df0227412ed51e0a8b103fc3c8c751a925cf94", "isShared": true, "rules": [lock, royalty]}),
        "personal" => json!({"id": rep("cd"), "isShared": true, "rules": [personal, lock, royalty]}),
        "floor" => json!({"id": rep("ce"), "isShared": true, "rules": [royalty, floor]}),
        _ => unreachable!(),
    };
    serde_json::from_value(v).unwrap()
}

fn plain_kiosk() -> OwnedKioskRef {
    OwnedKioskRef { kiosk_id: rep("11"), owner_cap_id: rep("22"), personal_cap_id: None, is_personal: false }
}
fn personal_kiosk() -> OwnedKioskRef {
    OwnedKioskRef { kiosk_id: rep("33"), owner_cap_id: rep("44"), personal_cap_id: Some(rep("55")), is_personal: true }
}
fn seller_kiosk() -> OwnedKioskRef {
    OwnedKioskRef { kiosk_id: SELLER_KIOSK.into(), owner_cap_id: rep("22"), personal_cap_id: None, is_personal: false }
}

fn listing(over: Value) -> Listing {
    let base = json!({
        "venue": "kiosk", "source": "kiosk/native", "orderKey": format!("{SELLER_KIOSK}:{ITEM}"), "objectId": null,
        "priceMist": "12000000", "allInMist": "13200000", "feeMist": "0", "commissionMist": "0", "seller": "",
        "kioskId": SELLER_KIOSK, "nftType": RETRO, "extra": {"royaltyMist": "1200000"}
    });
    serde_json::from_value(merge(base, over)).unwrap()
}

fn buy(cfg: &VenueConfig, lines: Vec<(Listing, Option<String>)>, policies: Vec<(&str, Vec<TransferPolicy>)>, kiosks: Vec<OwnedKioskRef>) -> Tx {
    let mut p = PoliciesByType::new();
    for (t, v) in policies {
        p.insert(t.to_string(), v);
    }
    let lines: Vec<BuyLine> = lines.into_iter().map(|(l, game)| BuyLine { item_id: ITEM.into(), listing: l, resale_game: game }).collect();
    build_cart_buy(cfg, &lines, &TradeContext { sender: BUYER.into(), buyer_kiosks: kiosks, policies: p }).unwrap()
}

/// The TS `api()` stand-in: kiosks, and one policy for RETRO (None: none; default royaltyOnly).
fn api(kiosks: Vec<OwnedKioskRef>, policy: Option<Option<TransferPolicy>>) -> MockNet {
    let n = MockNet::new();
    n.on_read_json(&format!("/v1/accounts/{SELLER}/kiosks"), json!({"data": kiosks}));
    let pols = match policy {
        Some(None) => vec![],
        Some(Some(p)) => vec![p],
        None => vec![pol("royaltyOnly")],
    };
    n.on_read_json(&format!("/v1/policies/{}", enc(RETRO)), json!({"data": pols}));
    n
}

fn card(over: Value) -> ItemCard {
    let base = json!({
        "id": ITEM, "name": "RetroSui #1550", "type": RETRO, "imageUrl": null,
        "collection": {"slug": "retrosui", "name": "RetroSui", "imageUrl": null, "verified": false},
        "listing": null, "owner": SELLER, "kioskId": null
    });
    serde_json::from_value(merge(base, over)).unwrap()
}

fn native_listing() -> Value {
    json!({"venue": "kiosk", "source": "kiosk/native", "orderKey": format!("{SELLER_KIOSK}:{ITEM}"), "priceMist": "12000000", "kioskId": SELLER_KIOSK, "nftType": RETRO})
}

fn offer(over: Value) -> Offer {
    let base = json!({
        "id": rep("0f"), "venue": "tradeport", "source": "tradeport/bids", "nftType": RETRO,
        "feeMist": "3330000", "royaltyMist": "11100000", "commissionMist": "0",
        "poolId": "0x14af0f71ded139ff5b41cacd37b8b924e8298c9de37c6b2f8fde0aea3a66605e",
        "fillable": true, "extra": {}, "kind": "collection",
        "collection": {"slug": "retrosui", "name": "RetroSui", "imageUrl": null, "verified": false},
        "item": null, "trait": null, "priceMist": "111000000", "quantity": 1, "filled": 0,
        "bidder": BIDDER, "destKioskId": null, "createdAt": "2026-10-25T22:55:39.131Z", "expiresAt": null, "status": "open"
    });
    serde_json::from_value(merge(base, over)).unwrap()
}

fn item_offer(source: &str, extra: Value) -> Value {
    json!({"source": source, "kind": "item", "item": {"id": ITEM, "name": "", "imageUrl": ""}, "poolId": null, "extra": extra})
}

fn cases() -> Vec<(&'static str, Tx)> {
    let c = VenueConfig::default();
    let lr = || vec![(RETRO, vec![pol("lockRoyalty")])];
    let ro = || vec![(RETRO, vec![pol("royaltyOnly")])];
    let in_kiosk = json!({"kioskId": SELLER_KIOSK});
    let in_kiosk_listed = json!({"kioskId": SELLER_KIOSK, "listing": native_listing()});
    let list = |item: ItemCard, price: u64, n: MockNet| build_list_transaction(&n, &c, &item, price, SELLER, None).unwrap();
    let accept = |item: ItemCard, o: Offer, n: MockNet| build_accept_offer_transaction(&n, &c, &item, &o, SELLER, None).unwrap();
    let mut out = vec![
        ("buy/kiosk-native", buy(&c, vec![(listing(json!({})), None)], lr(), vec![plain_kiosk()])),
        ("buy/kiosk-native-new-kiosk", buy(&c, vec![(listing(json!({})), None)], ro(), vec![])),
        ("buy/personal", buy(&c, vec![(listing(json!({})), None)], vec![(RETRO, vec![pol("personal")])], vec![plain_kiosk(), personal_kiosk()])),
        ("buy/floor", buy(&c, vec![(listing(json!({})), None)], vec![(RETRO, vec![pol("floor")])], vec![plain_kiosk()])),
        (
            "buy/tradeport-simple-kiosk",
            buy(
                &c,
                vec![(listing(json!({"venue": "tradeport", "source": "tradeport/simple", "orderKey": ITEM, "objectId": rep("77"), "priceMist": "1000000000", "feeMist": "30000000", "kioskId": rep("88"), "extra": {"feeBps": 300, "inOrderbook": true, "listingType": 1}})), None)],
                lr(),
                vec![plain_kiosk()],
            ),
        ),
        (
            "buy/tradeport-simple-personal",
            buy(
                &c,
                vec![(listing(json!({"venue": "tradeport", "source": "tradeport/simple", "orderKey": ITEM, "priceMist": "1000000000", "feeMist": "30000000", "kioskId": rep("88"), "extra": {"inOrderbook": false, "listingType": "1"}})), None)],
                vec![(RETRO, vec![pol("personal")])],
                vec![personal_kiosk()],
            ),
        ),
        (
            "buy/tradeport-simple-wallet",
            buy(&c, vec![(listing(json!({"venue": "tradeport", "source": "tradeport/simple", "orderKey": ITEM, "priceMist": "500000000", "feeMist": "15000000", "kioskId": null, "extra": {"inOrderbook": false, "listingType": 0}})), None)], vec![], vec![plain_kiosk()]),
        ),
        (
            "buy/tradeport-kiosk",
            buy(&c, vec![(listing(json!({"venue": "tradeport", "source": "tradeport/kiosk", "orderKey": ITEM, "priceMist": "226600000", "commissionMist": "6600000", "kioskId": rep("99"), "extra": {}})), None)], lr(), vec![plain_kiosk()]),
        ),
        ("buy/tradeport-kiosk-early", buy(&c, vec![(listing(json!({"venue": "tradeport", "source": "tradeport/kiosk-early", "priceMist": "100", "commissionMist": "3", "kioskId": rep("99")})), None)], ro(), vec![plain_kiosk()])),
        ("buy/tradeport-escrow", buy(&c, vec![(listing(json!({"venue": "tradeport", "source": "tradeport/escrow", "orderKey": rep("66"), "kioskId": null, "priceMist": "5000"})), None)], vec![], vec![plain_kiosk()])),
        ("buy/drip-v2", buy(&c, vec![(listing(json!({"venue": "drip", "source": "drip/v2", "feeMist": "240000"})), None)], lr(), vec![plain_kiosk()])),
        (
            "buy/cart",
            buy(
                &c,
                vec![
                    (listing(json!({})), None),
                    (listing(json!({"venue": "drip", "source": "drip/v2", "priceMist": "7", "feeMist": "1", "kioskId": rep("98")})), None),
                    (listing(json!({"venue": "tradeport", "source": "tradeport/escrow", "orderKey": rep("66"), "kioskId": null, "priceMist": "5000"})), None),
                ],
                lr(),
                vec![plain_kiosk()],
            ),
        ),
        ("list/wallet-no-kiosk", list(card(json!({})), 5_000_000_000, api(vec![], None))),
        ("list/in-kiosk", list(card(in_kiosk.clone()), 1_000_000_000, api(vec![seller_kiosk()], None))),
        ("list/lock", list(card(json!({})), 1_000_000_000, api(vec![seller_kiosk()], Some(Some(pol("lockRoyalty")))))),
        ("list/personal-new", list(card(json!({})), 1_000_000_000, api(vec![seller_kiosk()], Some(Some(pol("personal")))))),
        ("list/personal-existing", list(card(json!({})), 1_000_000_000, api(vec![seller_kiosk(), personal_kiosk()], Some(Some(pol("personal")))))),
        ("list/relist", list(card(in_kiosk_listed.clone()), 20_000_000, api(vec![seller_kiosk()], None))),
        ("accept/tradeport-bids-listed", accept(card(in_kiosk_listed.clone()), offer(json!({})), api(vec![seller_kiosk()], Some(Some(pol("lockRoyalty")))))),
        ("accept/tradeport-bids-wallet", accept(card(json!({})), offer(json!({})), api(vec![seller_kiosk()], Some(Some(pol("lockRoyalty")))))),
        ("accept/tradeport-bids-no-policy", accept(card(json!({})), offer(json!({})), api(vec![], Some(None)))),
        (
            "accept/tradeport-kiosk-bids",
            accept(
                card(in_kiosk.clone()),
                offer(merge(item_offer("tradeport/kiosk-bids", json!({"escrow": "904000000"})), json!({"commissionMist": "24000000", "priceMist": "800000000"}))),
                api(vec![seller_kiosk()], Some(Some(pol("lockRoyalty")))),
            ),
        ),
        ("accept/tradeport-escrow-bids", accept(card(json!({})), offer(json!({"source": "tradeport/escrow-bids", "poolId": null})), api(vec![], None))),
        ("accept/drip-v2-item", accept(card(in_kiosk.clone()), offer(item_offer("drip/v2-bids", json!({}))), api(vec![seller_kiosk()], Some(Some(pol("lockRoyalty")))))),
        ("accept/drip-v2-collection-wallet", accept(card(json!({})), offer(json!({"venue": "drip", "source": "drip/v2-bids", "poolId": null})), api(vec![personal_kiosk()], Some(Some(pol("lockRoyalty")))))),
        ("cancel/native", build_cancel_transaction(&api(vec![seller_kiosk()], None), &card(in_kiosk_listed), SELLER, None).unwrap()),
    ];
    let mut rc = VenueConfig::default();
    rc.resale.configure(Some(&rep("9a")), Some(&rep("9b")));
    let licence = format!("{}::licence::ResaleLicence", rep("9a"));
    let policy: TransferPolicy = serde_json::from_value(json!({"id": rep("9d"), "isShared": true, "rules": [
        {"type": "kiosk_lock_rule", "ruleType": format!("{}::kiosk_lock_rule::Rule", kiosk_ext::ORIGIN)},
        {"type": "rule", "ruleType": format!("{}::resale::Rule", rep("9a"))}
    ]}))
    .unwrap();
    out.push(("buy/resale", buy(&rc, vec![(listing(json!({"nftType": licence})), Some(rep("9c")))], vec![(&licence, vec![policy])], vec![plain_kiosk()])));
    out
}

#[test]
fn every_case_matches_the_typescript_ptb() {
    let golden: serde_json::Map<String, Value> = serde_json::from_str(include_str!("../golden/ptb.json")).unwrap();
    let cases = cases();
    assert_eq!(cases.len(), golden.len(), "case lists differ");
    let mut bad = Vec::new();
    for (name, tx) in &cases {
        let want = golden.get(*name).unwrap_or_else(|| panic!("{name} missing from golden/ptb.json"));
        if tx.to_json() != *want {
            bad.push(format!("{name}:\n  rust {}\n  ts   {}", tx.to_json(), want));
        }
    }
    assert!(bad.is_empty(), "{} of {} PTBs differ:\n{}", bad.len(), cases.len(), bad.join("\n"));
}
