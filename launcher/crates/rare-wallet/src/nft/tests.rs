//! The TS tests, ported: tx/nft/send.test.ts (builders, planning, sendability over gRPC, policies
//! from the Trade API) and the buildNftSend case of tx/tx.test.ts.

use std::collections::HashSet;

use rare_sui::ptb::{Arg, Command, Input, Tx};
use serde_json::{json, Value};

use super::context::{analyze_nfts, policy_ids_from_trade, AnalyzeDeps, NftRef, PolicyLoader, PolicySource, Sendability};
use super::ids::{DRIP_LISTING_REGISTRY, KIOSK_EXT_PACKAGE, TRADEPORT_ORDERBOOK_STORE, TRADEPORT_STORE};
use super::limits::{fits, measure, SUI_LIMITS};
use super::transfer::{build_nft_send, build_send_nfts, plan_nft_sends, NftSendParams};
use super::vectors::{id, locked, plain_kiosk, royalty_lock, unlocked, vectors, wallet, FakeTrade, JsonChain, NFT_T, SENDER, TO};
use super::{KioskItemRef, KioskRef, Market, NftSendError, PolicyRule, SendErrorCode, SendItem, TransferPolicyInfo};

fn personal_kiosk() -> KioskRef {
    KioskRef { kiosk_id: id(0xb000), owner_cap_id: id(0xb001), personal_cap_id: Some(id(0xb002)), is_personal: true }
}

fn rule(m: &str) -> PolicyRule {
    PolicyRule { type_: m.into(), rule_type: Some(format!("{KIOSK_EXT_PACKAGE}::{m}::Rule")), config: None }
}

fn personal_policy() -> TransferPolicyInfo {
    TransferPolicyInfo { id: id(0xf002), is_shared: Some(true), rules: vec![rule("kiosk_lock_rule"), rule("personal_kiosk_rule")] }
}

fn floor_policy() -> TransferPolicyInfo {
    TransferPolicyInfo { id: id(0xf003), is_shared: Some(true), rules: vec![PolicyRule { config: Some(json!({ "floor_price": "1000" })), ..rule("floor_price_rule") }] }
}

fn unlocked_from(n: u64, from: KioskRef, f: impl FnOnce(&mut KioskItemRef)) -> SendItem {
    let mut s = unlocked(n);
    s.from = Some(from);
    f(&mut s.item);
    s
}

fn locked_with(n: u64, policy: TransferPolicyInfo, from: KioskRef) -> SendItem {
    let mut s = locked(n);
    s.from = Some(from);
    s.policies = Some(vec![policy]);
    s
}

fn calls(tx: &Tx) -> Vec<String> {
    tx.commands
        .iter()
        .map(|c| match c {
            Command::MoveCall(m) => format!("{}::{}", m.module, m.function),
            Command::TransferObjects { .. } => "TransferObjects".into(),
            Command::SplitCoins { .. } => "SplitCoins".into(),
            Command::MergeCoins { .. } => "MergeCoins".into(),
            Command::MakeMoveVec { .. } => "MakeMoveVec".into(),
            Command::CoinWithBalance { .. } => "$Intent".into(),
        })
        .collect()
}

fn count(c: &[String], x: &str) -> usize {
    c.iter().filter(|s| *s == x).count()
}

/// Every object id the transaction names: object inputs and 32-byte pure ids.
fn named_ids(tx: &Tx) -> HashSet<String> {
    tx.inputs
        .iter()
        .filter_map(|i| match i {
            Input::Object(o) => Some(o.clone()),
            Input::Pure(b) if b.len() == 32 => Some(format!("0x{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())),
            _ => None,
        })
        .collect()
}

fn transfers(tx: &Tx) -> Vec<(&Vec<Arg>, Arg)> {
    tx.commands
        .iter()
        .filter_map(|c| match c {
            Command::TransferObjects { objects, address } => Some((objects, *address)),
            _ => None,
        })
        .collect()
}

// ---------- buildSendNfts ----------

#[test]
fn sends_wallet_nfts_with_one_transfer_and_every_item_as_an_input() {
    let items: Vec<SendItem> = [1, 2, 3].map(wallet).to_vec();
    let tx = build_send_nfts(SENDER, TO, &items).unwrap();
    assert_eq!(calls(&tx), ["TransferObjects"]);
    let named = named_ids(&tx);
    for s in &items {
        assert!(named.contains(&s.item.nft_id));
    }
}

#[test]
fn splits_more_than_500_wallet_nfts_across_transfers() {
    let items: Vec<SendItem> = (1..=600).map(wallet).collect();
    let tx = build_send_nfts(SENDER, TO, &items).unwrap();
    assert_eq!(transfers(&tx).iter().map(|(o, _)| o.len()).collect::<Vec<_>>(), [500, 100]);
}

#[test]
fn takes_unlocked_kiosk_items_and_sends_them_with_the_wallet_ones() {
    let tx = build_send_nfts(SENDER, TO, &[unlocked(1), unlocked(2), wallet(3)]).unwrap();
    assert_eq!(calls(&tx), ["kiosk::take", "kiosk::take", "TransferObjects"]);
    assert_eq!(transfers(&tx)[0].0.len(), 3);
}

#[test]
fn cancels_listings_first() {
    let tx = build_send_nfts(
        SENDER,
        TO,
        &[
            unlocked_from(1, plain_kiosk(), |i| i.is_listed = true),
            unlocked_from(2, plain_kiosk(), |i| {
                i.is_listed = true;
                i.market = Some(Market::Drip);
            }),
            unlocked_from(3, plain_kiosk(), |i| {
                i.is_listed = true;
                i.is_exclusive_listing = true;
                i.market = Some(Market::Tradeport { orderbook: false });
            }),
            unlocked_from(4, plain_kiosk(), |i| {
                i.is_listed = true;
                i.is_exclusive_listing = true;
                i.market = Some(Market::Tradeport { orderbook: true });
            }),
        ],
    )
    .unwrap();
    assert_eq!(
        calls(&tx),
        [
            "kiosk::delist",
            "kiosk::take",
            "marketplace::delist",
            "kiosk::take",
            "tradeport_listings::cancel_listing_with_transfer_policy",
            "kiosk::take",
            "tradeport_listings::cancel_ob_listing_with_transfer_policy",
            "kiosk::take",
            "TransferObjects",
        ]
    );
    let named = named_ids(&tx);
    for o in [DRIP_LISTING_REGISTRY, TRADEPORT_STORE, TRADEPORT_ORDERBOOK_STORE] {
        assert!(named.contains(o));
    }
}

#[test]
fn refuses_an_item_listed_elsewhere_with_a_purchase_cap() {
    let e = build_send_nfts(
        SENDER,
        TO,
        &[unlocked_from(1, plain_kiosk(), |i| {
            i.is_listed = true;
            i.is_exclusive_listing = true;
        })],
    )
    .unwrap_err();
    assert!(e.message().contains("listed elsewhere"));
}

#[test]
fn moves_a_locked_royalty_item_into_a_new_kiosk_made_for_the_recipient() {
    let tx = build_send_nfts(SENDER, TO, &[locked(1)]).unwrap();
    assert_eq!(
        calls(&tx),
        [
            "kiosk::new",
            "kiosk::list_with_purchase_cap",
            "coin::zero",
            "kiosk::purchase_with_cap",
            "kiosk::lock",
            "transfer_policy::paid",
            "royalty_rule::fee_amount",
            "SplitCoins",
            "royalty_rule::pay",
            "kiosk_lock_rule::prove",
            "transfer_policy::confirm_request",
            "kiosk::set_owner_custom",
            "transfer::public_share_object",
            "TransferObjects",
        ]
    );
    // the new kiosk's cap goes to the recipient
    let (_, addr) = *transfers(&tx).last().unwrap();
    let Arg::Input(i) = addr else { panic!() };
    assert_eq!(tx.inputs[i as usize], Input::Pure(rare_sui::ptb::bcs::address(TO)));
}

#[test]
fn shares_one_recipient_kiosk_and_borrows_a_personal_source_kiosk_once() {
    let tx = build_send_nfts(SENDER, TO, &[locked_with(1, royalty_lock(), personal_kiosk()), locked_with(2, royalty_lock(), personal_kiosk())]).unwrap();
    let c = calls(&tx);
    assert_eq!(count(&c, "kiosk::new"), 1);
    assert_eq!(count(&c, "personal_kiosk::borrow_val"), 1);
    assert_eq!(count(&c, "personal_kiosk::return_val"), 1);
    assert_eq!(count(&c, "transfer_policy::confirm_request"), 2);
    // 2 + 4 for the kiosks, 10 per item
    assert_eq!(c.len(), 2 + 4 + 2 * 10);
}

#[test]
fn uses_a_personal_kiosk_made_with_create_for_proving_after_it_is_personal() {
    let tx = build_send_nfts(SENDER, TO, &[locked_with(1, personal_policy(), plain_kiosk())]).unwrap();
    let c = calls(&tx);
    assert_eq!(c[0], "kiosk::new");
    let at = |x: &str| c.iter().position(|s| s == x).unwrap();
    let create_for = at("personal_kiosk::create_for");
    assert!(create_for > 0);
    assert!(at("personal_kiosk_rule::prove") > create_for);
    assert!(at("transfer_policy::confirm_request") > create_for);
    assert_eq!(c.last().unwrap(), "transfer::public_share_object");
}

#[test]
fn buys_at_the_floor_price_and_withdraws_the_payment_back_to_the_sender() {
    let c = calls(&build_send_nfts(SENDER, TO, &[locked_with(1, floor_policy(), plain_kiosk())]).unwrap());
    assert!(c.contains(&"floor_price_rule::prove".to_string()));
    assert!(c.contains(&"kiosk::withdraw".to_string()));
    assert_eq!(count(&c, "SplitCoins"), 1);
}

#[test]
fn mixed_set_every_item_included_and_inside_the_limits() {
    let items = [wallet(1), unlocked(2), locked(3), locked_with(4, personal_policy(), personal_kiosk()), wallet(5)];
    let tx = build_send_nfts(SENDER, TO, &items).unwrap();
    let named = named_ids(&tx);
    for s in &items {
        assert!(named.contains(&s.item.nft_id));
    }
    // 1 take + 2 kiosks (4 + 3) + 1 personal borrow/return (2) + locked items (10 + 7) + 1 transfer
    assert_eq!(tx.commands.len(), 1 + 4 + 3 + 2 + 10 + 7 + 1);
    assert!(fits(&measure(&tx), &SUI_LIMITS));
}

#[test]
fn will_not_send_to_the_same_account() {
    assert!(build_send_nfts(SENDER, SENDER, &[wallet(1)]).is_err());
}

// ---------- planNftSends ----------

fn check_plan(items: &[SendItem]) -> Vec<Vec<SendItem>> {
    let batches = plan_nft_sends(SENDER, TO, items, &SUI_LIMITS).unwrap();
    let mut seen: Vec<&str> = batches.iter().flatten().map(|s| s.item.nft_id.as_str()).collect();
    let mut want: Vec<&str> = items.iter().map(|s| s.item.nft_id.as_str()).collect();
    seen.sort();
    want.sort();
    assert_eq!(seen, want);
    for b in &batches {
        assert!(fits(&measure(&build_send_nfts(SENDER, TO, b).unwrap()), &SUI_LIMITS));
    }
    // minimal: no batch could take the first item of the next one
    for w in batches.windows(2) {
        let mut more = w[0].clone();
        more.push(w[1][0].clone());
        assert!(!fits(&measure(&build_send_nfts(SENDER, TO, &more).unwrap()), &SUI_LIMITS));
    }
    batches
}

#[test]
fn keeps_a_normal_selection_in_one_transaction() {
    assert_eq!(check_plan(&[wallet(1), unlocked(2), locked(3)]).len(), 1);
}

#[test]
fn splits_thousands_of_wallet_nfts_by_size() {
    assert!(check_plan(&(1..=3000).map(wallet).collect::<Vec<_>>()).len() > 1);
}

#[test]
fn splits_many_locked_items_by_command_count_and_size() {
    let b = check_plan(&(1..=150).map(locked).collect::<Vec<_>>());
    assert!(b.len() > 1);
    for batch in &b {
        assert!(measure(&build_send_nfts(SENDER, TO, batch).unwrap()).commands <= SUI_LIMITS.commands);
    }
}

// ---------- analyzeNfts over gRPC ----------

#[tokio::test]
async fn marks_what_can_be_sent_and_says_why_the_rest_can_not() {
    // send.test.ts's fake chain, as recorded in the vectors
    let case = &vectors()["analyze"][0];
    let chain = JsonChain::new(&case["chain"]);
    struct Policies;
    impl PolicySource for Policies {
        fn policies<'a>(&'a self, t: &'a str, _id: &'a str) -> rare_sui::BoxFut<'a, Vec<TransferPolicyInfo>> {
            let blocked = t == format!("0x{}::sbt::Badge", "d4".repeat(32));
            Box::pin(async move {
                Ok(if blocked {
                    vec![TransferPolicyInfo {
                        id: id(0xf9),
                        is_shared: Some(true),
                        rules: vec![PolicyRule { type_: "disable_transfer_rule".into(), rule_type: Some("0xc88b1c7df0336eb4310b7d420ab93ce63a4190b1c9d5024a8df8f4ff4b8f1760::disable_transfer_rule::Rule".into()), config: None }],
                    }]
                } else {
                    vec![royalty_lock()]
                })
            })
        }
    }
    let nfts: Vec<NftRef> = [10u64, 11, 12, 13, 14, 15, 16, 17, 19, 21, 22, 18]
        .iter()
        .map(|&n| {
            let t = match n {
                14 => format!("0x{}::sbt::Badge", "d4".repeat(32)),
                18 => format!("0x{}::badge::Badge", "e5".repeat(32)),
                _ => NFT_T.to_string(),
            };
            NftRef { object_id: id(n), type_: t, kiosk: !matches!(n, 10 | 18) }
        })
        .collect();
    let r = analyze_nfts(&AnalyzeDeps { chain: &chain, policies: &Policies, check_store: true }, SENDER, &nfts).await.unwrap();
    let get = |n: u64| r.get(&id(n)).unwrap();
    let send = |n: u64| get(n).send().unwrap_or_else(|| panic!("{n}: {:?}", get(n))).clone();
    let reason = |n: u64| get(n).reason().unwrap_or_else(|| panic!("{n} can be sent")).to_string();
    assert_eq!(send(10).from, None);
    assert_eq!(send(11).from, Some(plain_kiosk()));
    assert!(!send(11).item.is_locked);
    assert!(send(12).item.is_locked);
    assert_eq!(send(12).policies, Some(vec![royalty_lock()]));
    assert!(reason(13).contains("can not delist from"));
    assert!(reason(14).contains("transfer rules block it"));
    assert_eq!(send(15).from, Some(personal_kiosk()));
    assert_eq!(send(15).item.market, Some(Market::Tradeport { orderbook: true }));
    assert_eq!(send(19).item.market, Some(Market::Tradeport { orderbook: false }));
    assert!(send(21).item.is_listed);
    assert_eq!(send(21).item.market, Some(Market::Drip));
    assert!(send(22).item.is_listed);
    assert_eq!(send(22).item.market, None);
    assert!(reason(16).contains("OriginByte"));
    assert!(reason(17).contains("Could not find"));
    assert!(reason(18).contains("can not be transferred"));

    // and the ones that can go build into one transaction
    let sendable: Vec<SendItem> = r.values().filter_map(Sendability::send).cloned().collect();
    assert_eq!(sendable.len(), 7);
    let tx = build_send_nfts(SENDER, TO, &sendable).unwrap();
    let named = named_ids(&tx);
    for s in &sendable {
        assert!(named.contains(&s.item.nft_id));
    }
}

// ---------- transfer policies from the Trade API ----------

#[tokio::test]
async fn asks_trade_for_the_normalized_type_and_returns_the_policy_ids() {
    let ok = FakeTrade::new(json!({ "status": 200, "body": { "policies": [{ "id": "0xf001", "rules": [] }] } }));
    let short = format!("0x{}::punks::Punk", "c3".repeat(32));
    assert_eq!(policy_ids_from_trade("https://rare.market/v1/trade/", &short, &ok).await.unwrap(), vec![id(0xf001)]);
    assert_eq!(ok.seen.lock().unwrap()[0], format!("https://rare.market/v1/trade/ext/nft/policies?type={}", rare_sui::market::encode_component(NFT_T)));
    let down = FakeTrade::new(json!({ "status": 503, "body": { "error": "x" } }));
    assert!(policy_ids_from_trade("https://rare.market/v1/trade", NFT_T, &down).await.unwrap_err().to_string().contains("503"));
}

#[tokio::test]
async fn reads_the_policies_trade_names_without_touching_the_item_history() {
    let chain = JsonChain::new(&vectors()["readPolicies"]["chain"]);
    let trade = FakeTrade::new(json!({ "status": 200, "body": { "policies": [{ "id": id(0xf001) }] } }));
    let loader = PolicyLoader { chain: &chain, history: &chain, trade_base: "https://rare.market/v1/trade".into(), http: &trade };
    // 0x63 has no object at all: a history read would fail
    let out = loader.policies(NFT_T, &id(0x63)).await.unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id, id(0xf001));
    assert_eq!(out[0].is_shared, Some(true));
    assert_eq!(out[0].rules[0], PolicyRule { type_: "royalty_rule".into(), rule_type: Some(format!("{KIOSK_EXT_PACKAGE}::royalty_rule::Rule")), config: None });
}

#[tokio::test]
async fn falls_back_to_the_item_last_transaction_when_trade_fails_or_knows_none() {
    let chain = JsonChain::new(&vectors()["readPolicies"]["chain"]);
    for answer in [json!({ "status": 503, "body": {} }), json!({ "status": 200, "body": { "policies": [] } }), json!("offline")] {
        let trade = FakeTrade::new(answer);
        let loader = PolicyLoader { chain: &chain, history: &chain, trade_base: "https://rare.market/v1/trade".into(), http: &trade };
        let out = loader.policies(NFT_T, &id(12)).await.unwrap();
        // the item's last transaction touched policies 0xf001 and 0xf003 for this type
        assert_eq!(out.iter().map(|p| p.id.clone()).collect::<Vec<_>>(), vec![id(0xf001), id(0xf003)]);
    }
}

// ---------- buildNftSend (tx.test.ts) ----------

#[test]
fn build_nft_send_wraps_the_builder_with_sender_set_and_address_checks() {
    let me = format!("0x{}", "a1".repeat(32));
    let bob = format!("0x{}", "b2".repeat(32));
    let item = SendItem { from: None, item: KioskItemRef { nft_id: id(5), nft_type: "0x9::a::B".into(), ..Default::default() }, policies: None };
    let tx = build_nft_send(&NftSendParams { sender: me.clone(), to: bob.clone(), items: vec![item] }).unwrap();
    assert_eq!(tx.to_json()["sender"], Value::String(me.clone()));
    assert_eq!(calls(&tx), ["TransferObjects"]);
    let e = build_nft_send(&NftSendParams { sender: me.clone(), to: bob, items: vec![] }).unwrap_err();
    assert!(matches!(&e, NftSendError::Input(s) if s.code == SendErrorCode::BadAmount && s.message == "Pick at least one item"));
    let e = build_nft_send(&NftSendParams { sender: me, to: "nope".into(), items: vec![] }).unwrap_err();
    assert_eq!(e.message(), "Enter a valid Sui address");
}
