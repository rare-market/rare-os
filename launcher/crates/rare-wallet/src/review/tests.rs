//! Ports review.test.ts, simulate.test.ts, address.test.ts, fees.test.ts and live.test.ts
//! (`#[ignore]`, mainnet, simulate only).

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;

fn addr(b: &str) -> String {
    format!("0x{}", b.repeat(32))
}
fn me() -> String {
    addr("a1")
}
fn bob() -> String {
    addr("b2")
}
fn sponsor() -> String {
    addr("c3")
}
fn market() -> String {
    addr("d4")
}
fn gas() -> String {
    addr("11")
}
fn nft() -> String {
    addr("22")
}
fn usdc() -> String {
    norm_type("0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC")
}
fn nft_t() -> String {
    format!("{}::popkins::Popkins", addr("e5"))
}
const CETUS: &str = "0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb";
const SUI: &str = "0x2::sui::SUI";
// review.test.ts fee: 1_000_000 + 2_000_000 - 1_500_000 = 1_500_000 MIST
const FEE: i128 = 1_500_000;

fn own(a: &str) -> Option<Owner> {
    Some(Owner::address(a))
}
fn parent(id: &str) -> Option<Owner> {
    Some(Owner::object(id))
}

/// `obj()` in the TS tests: owned by me before and after, mutated in place.
fn obj_t(id: &str, t: Option<&str>) -> SimObjectChange {
    SimObjectChange {
        object_id: id.into(),
        object_type: t.map(str::to_string),
        input_owner: own(&me()),
        output_owner: own(&me()),
        id_operation: IdOperation::Unchanged,
        input_exists: true,
        output_exists: true,
        output_kind: Some(OutputKind::Object),
    }
}
fn obj(id: &str) -> SimObjectChange {
    obj_t(id, Some(&nft_t()))
}

fn bc(coin_type: &str, address: &str, amount: i128) -> SimBalanceChange {
    SimBalanceChange { coin_type: coin_type.into(), address: address.into(), amount: amount.to_string() }
}

/// `sim()` in review.test.ts.
fn sim() -> Simulation {
    Simulation {
        source: Some(SimSource::Node),
        status: SimStatus::Success,
        failure: None,
        balance_changes: vec![],
        objects: vec![],
        gas: SimGas { computation: "1000000".into(), storage: "2000000".into(), rebate: "1500000".into() },
        gas_owner: Some(me()),
        gas_object_id: Some(gas()),
        display: None,
        packages: None,
        coins: None,
    }
}

/// `sim()` in simulate.test.ts: fee 1000 + 5000 - 2000 = 4000 MIST, no source.
fn sim2() -> Simulation {
    Simulation { source: None, gas: SimGas { computation: "1000".into(), storage: "5000".into(), rebate: "2000".into() }, ..sim() }
}

fn meta() -> BTreeMap<String, CoinMeta> {
    BTreeMap::from([
        (SUI_TYPE.to_string(), CoinMeta { symbol: "SUI".into(), decimals: 9, icon_url: None }),
        (usdc(), CoinMeta { symbol: "USDC".into(), decimals: 6, icon_url: None }),
    ])
}

fn row_text(rows: &[ReviewRow]) -> Vec<String> {
    rows.iter().map(|r| format!("{}: {}{}", r.label, r.text, r.sub.as_ref().map(|s| format!(" ({s})")).unwrap_or_default())).collect()
}

fn describe(ins: &Insights) -> Vec<String> {
    let m = meta();
    row_text(&describe_review(ins, CoinMetaLookup::Map(&m), &DescribeExtras::default()))
}

fn flow(t: &str, amount: i128) -> CoinFlow {
    CoinFlow { coin_type: t.into(), amount }
}

fn ids(r: &[Risk]) -> Vec<String> {
    r.iter().map(|x| x.id.clone()).collect()
}

// ---------- review.test.ts ----------

#[test]
fn coin_send_one_sui_row_out_fee_on_its_own_line_bob_as_the_recipient() {
    let s = Simulation { balance_changes: vec![bc(SUI, &me(), -2_500_000_000 - FEE), bc(SUI, &bob(), 2_500_000_000)], ..sim() };
    let ins = analyze(&s, &me(), &[], &[]);
    assert_eq!(ins.send, vec![flow(SUI_TYPE, 2_500_000_000)]);
    assert_eq!(ins.receive, vec![]);
    assert_eq!(ins.recipients, vec![bob()]);
    assert_eq!(ins.fee.net, FEE);
    assert_eq!(describe(&ins), ["You send: 2.5 SUI", "Network fee: 0.0015 SUI"]);
}

#[test]
fn coin_send_formats_tokens_by_their_decimals() {
    let ins = analyze(&Simulation { balance_changes: vec![bc(&usdc(), &me(), -12_345_678), bc(SUI, &me(), -FEE)], ..sim() }, &me(), &[], &[]);
    assert_eq!(describe(&ins), ["You send: 12.3456 USDC", "Network fee: 0.0015 SUI"]);
}

#[test]
fn nfts_an_nft_out_to_another_address() {
    let s = Simulation { balance_changes: vec![bc(SUI, &me(), -FEE)], objects: vec![SimObjectChange { output_owner: own(&bob()), ..obj(&nft()) }], ..sim() };
    let ins = analyze(&s, &me(), &[], &[]);
    assert_eq!(ins.send, vec![]);
    assert_eq!(ins.objects_out, vec![ObjectFlow { object_id: nft(), object_type: Some(nft_t()), kind: ObjectFlowKind::Sent, to: Some(bob()), created: None }]);
    assert_eq!(ins.recipients, vec![bob()]);
    let display = BTreeMap::from([(nft(), DisplayInfo { name: Some("Popkins #8475".into()), image: Some("https://x/1.png".into()) })]);
    let m = meta();
    let rows = describe_review(&ins, CoinMetaLookup::Map(&m), &DescribeExtras { display: Some(&display), ..Default::default() });
    assert_eq!(row_text(&rows), ["You send: Popkins #8475 (To 0xb2b2\u{2026}b2b2)", "Network fee: 0.0015 SUI"]);
    assert_eq!(rows[0].object_id.as_deref(), Some(nft().as_str()));
    assert_eq!(rows[0].image.as_deref(), Some("https://x/1.png"));
}

#[test]
fn nfts_a_purchase_sui_out_to_the_seller_and_the_market_nft_in_from_a_kiosk() {
    let s = Simulation {
        balance_changes: vec![bc(SUI, &me(), -87_000_000_000 - FEE), bc(SUI, &bob(), 85_000_000_000), bc(SUI, &market(), 2_000_000_000)],
        objects: vec![
            SimObjectChange { input_owner: parent(&addr("99")), output_owner: own(&me()), ..obj(&nft()) },
            SimObjectChange { input_owner: Some(Owner::Shared), output_owner: Some(Owner::Shared), ..obj_t("0xk1", Some("0x2::kiosk::Kiosk")) },
        ],
        ..sim()
    };
    let ins = analyze(&s, &me(), &[], &[]);
    assert_eq!(ins.send, vec![flow(SUI_TYPE, 87_000_000_000)]);
    assert_eq!(ins.objects_in, vec![ObjectFlow { object_id: nft(), object_type: Some(nft_t()), kind: ObjectFlowKind::Received, to: None, created: Some(false) }]);
    let mut others: Vec<String> = ins.others_receive.iter().map(|o| o.address.clone()).collect();
    others.sort();
    let mut want = vec![bob(), market()];
    want.sort();
    assert_eq!(others, want);
    assert_eq!(ins.shared.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Sui Kiosk"]);
    assert_eq!(describe(&ins), ["You send: 87 SUI", "You receive: Popkins", "Network fee: 0.0015 SUI"]);
}

const FIELD_T: &str = "0x2::dynamic_field::Field<0x2::dynamic_object_field::Wrapper<0x2::kiosk::Item>, 0x2::object::ID>";
fn my_kiosk() -> String {
    addr("0a")
}
fn new_kiosk() -> String {
    addr("0b")
}
fn f_in() -> String {
    addr("f1")
}
fn f_out() -> String {
    addr("f2")
}

fn moved(output_owner: Option<Owner>) -> Simulation {
    Simulation {
        balance_changes: vec![bc(SUI, &me(), -FEE)],
        objects: vec![
            SimObjectChange { input_owner: parent(&f_in()), output_owner, ..obj(&nft()) },
            SimObjectChange { input_owner: parent(&my_kiosk()), output_owner: None, output_exists: false, id_operation: IdOperation::Deleted, ..obj_t(&f_in(), Some(FIELD_T)) },
            SimObjectChange { input_owner: None, input_exists: false, id_operation: IdOperation::Created, output_owner: parent(&new_kiosk()), ..obj_t(&f_out(), Some(FIELD_T)) },
            SimObjectChange { input_owner: Some(Owner::Shared), output_owner: Some(Owner::Shared), ..obj_t(&my_kiosk(), Some("0x2::kiosk::Kiosk")) },
        ],
        ..sim()
    }
}

#[test]
fn kiosk_an_item_leaving_my_kiosk_for_a_new_kiosk_counts_as_sent_out_once_my_kiosks_are_known() {
    let s = moved(parent(&f_out()));
    assert_eq!(analyze(&s, &me(), &[], &[]).objects_out, vec![]);
    let ins = analyze(&s, &me(), &[], &[my_kiosk()]);
    assert_eq!(ins.objects_out, vec![ObjectFlow { object_id: nft(), object_type: Some(nft_t()), kind: ObjectFlowKind::IntoObject, to: Some(new_kiosk()), created: None }]);
    assert_eq!(describe(&ins), ["You send: Popkins (Into another object)", "Network fee: 0.0015 SUI"]);
}

#[test]
fn kiosk_an_item_moved_into_a_kiosk_made_for_bob_is_a_send_to_bob() {
    let mut s = moved(parent(&f_out()));
    s.objects.push(SimObjectChange { input_owner: None, input_exists: false, id_operation: IdOperation::Created, output_owner: Some(Owner::Shared), ..obj_t(&new_kiosk(), Some("0x2::kiosk::Kiosk")) });
    s.objects.push(SimObjectChange { input_owner: None, input_exists: false, id_operation: IdOperation::Created, output_owner: own(&bob()), ..obj_t(&addr("0c"), Some("0x2::kiosk::KioskOwnerCap")) });
    let ins = analyze(&s, &me(), &[], &[my_kiosk()]);
    assert_eq!(ins.objects_out, vec![ObjectFlow { object_id: nft(), object_type: Some(nft_t()), kind: ObjectFlowKind::Sent, to: Some(bob()), created: None }]);
    assert_eq!(ins.recipients, vec![bob()]);
}

#[test]
fn kiosk_a_first_purchase_the_kiosk_made_here_with_its_cap_coming_to_me_is_mine() {
    let seller = addr("5e");
    let s = Simulation {
        balance_changes: vec![bc(SUI, &me(), -21_000_000_000 - FEE), bc(SUI, &bob(), 21_000_000_000)],
        objects: vec![
            SimObjectChange { input_owner: parent(&f_in()), output_owner: parent(&f_out()), ..obj(&nft()) },
            SimObjectChange { input_owner: parent(&seller), output_owner: None, output_exists: false, id_operation: IdOperation::Deleted, ..obj_t(&f_in(), Some(FIELD_T)) },
            SimObjectChange { input_owner: None, input_exists: false, id_operation: IdOperation::Created, output_owner: parent(&new_kiosk()), ..obj_t(&f_out(), Some(FIELD_T)) },
            SimObjectChange { input_owner: Some(Owner::Shared), output_owner: Some(Owner::Shared), ..obj_t(&seller, Some("0x2::kiosk::Kiosk")) },
            SimObjectChange { input_owner: None, input_exists: false, id_operation: IdOperation::Created, output_owner: Some(Owner::Shared), ..obj_t(&new_kiosk(), Some("0x2::kiosk::Kiosk")) },
            SimObjectChange { input_owner: None, input_exists: false, id_operation: IdOperation::Created, output_owner: own(&me()), ..obj_t(&addr("0c"), Some("0x2::kiosk::KioskOwnerCap")) },
        ],
        ..sim()
    };
    let ins = analyze(&s, &me(), &[], &[]);
    assert!(ins.objects_in.iter().any(|o| o.object_id == nft()));
    assert_eq!(ins.objects_out, vec![]);
}

#[test]
fn kiosk_an_item_taken_from_my_kiosk_and_sent_to_bob_is_a_send_to_bob() {
    let ins = analyze(&moved(own(&bob())), &me(), &[], &[my_kiosk()]);
    assert_eq!(ins.objects_out, vec![ObjectFlow { object_id: nft(), object_type: Some(nft_t()), kind: ObjectFlowKind::Sent, to: Some(bob()), created: None }]);
    assert_eq!(ins.recipients, vec![bob()]);
}

#[test]
fn kiosk_moving_between_my_own_kiosk_and_wallet_is_not_a_send() {
    let ins = analyze(&moved(own(&me())), &me(), &[], &[my_kiosk()]);
    assert_eq!(ins.objects_out, vec![]);
    assert_eq!(ins.objects_in, vec![]);
}

#[test]
fn failure_a_failed_simulation_says_why_and_keeps_the_fee() {
    let s = Simulation {
        status: SimStatus::Failure,
        failure: Some(SimFailure {
            message: "MoveAbort".into(),
            abort: Some(SimAbort { code: "2".into(), package: Some("0x2".into()), module: Some("balance".into()), function: Some("split".into()), constant: None }),
            ..Default::default()
        }),
        balance_changes: vec![bc(SUI, &me(), -FEE)],
        ..sim()
    };
    let ins = analyze(&s, &me(), &[], &[]);
    assert!(!ins.ok);
    assert_eq!(ins.reason.as_deref(), Some("Not enough balance."));
    assert_eq!(describe(&ins), ["Network fee: 0.0015 SUI"]);
}

#[test]
fn sponsored_gas_the_fee_stays_out_of_my_rows_and_the_fee_row_says_who_pays() {
    let s = Simulation { gas_owner: Some(sponsor()), balance_changes: vec![bc(&usdc(), &me(), -5_000_000), bc(&usdc(), &bob(), 5_000_000), bc(SUI, &sponsor(), -FEE)], ..sim() };
    let ins = analyze(&s, &me(), &[], &[]);
    assert!(!ins.fee_paid_by_signer);
    assert_eq!(ins.send, vec![flow(&usdc(), 5_000_000)]);
    assert_eq!(describe(&ins), ["You send: 5 USDC", "Network fee: 0 SUI (Paid by someone else)"]);
    let m = meta();
    let paid = describe_review(&ins, CoinMetaLookup::Map(&m), &DescribeExtras { covered_by_rare: true, paid_by: Some("Starlight Studio".into()), ..Default::default() });
    assert_eq!(row_text(&paid)[1], "Network fee: Paid by Starlight Studio");
}

#[test]
fn a_storage_refund_larger_than_the_cost_reads_as_sui_back() {
    let s = Simulation { gas: SimGas { computation: "1000000".into(), storage: "1000000".into(), rebate: "9000000".into() }, balance_changes: vec![bc(SUI, &me(), 7_000_000)], ..sim() };
    assert_eq!(describe(&analyze(&s, &me(), &[], &[])), ["Network fee: 0.007 SUI back (Storage refund is larger than the fee)"]);
}

#[test]
fn unknown_coins_fall_back_to_the_type_name_and_9_decimals() {
    let x = norm_type("0xabc::x::XCOIN");
    let ins = analyze(&Simulation { balance_changes: vec![bc(&x, &me(), 1_500_000_000), bc(SUI, &me(), -FEE)], ..sim() }, &me(), &[], &[]);
    assert_eq!(row_text(&describe_review(&ins, CoinMetaLookup::None, &DescribeExtras::default()))[0], "You receive: 1.5 XCOIN");
    let f = |t: &str| (t == x).then(|| CoinMeta { symbol: "X".into(), decimals: 3, icon_url: None });
    assert_eq!(row_text(&describe_review(&ins, CoinMetaLookup::Fn(&f), &DescribeExtras::default()))[0], "You receive: 1,500,000 X");
}

// ---------- reviewTransaction ----------

/// A simulator that answers one fixed simulation and records the bytes it was given.
struct FixedSim {
    sim: Result<Simulation, String>,
    seen: Mutex<Vec<Vec<u8>>>,
}

impl FixedSim {
    fn new(sim: Simulation) -> Self {
        FixedSim { sim: Ok(sim), seen: Mutex::new(vec![]) }
    }
}

impl Simulator for FixedSim {
    fn simulate<'a>(&'a self, tx_bytes: &'a [u8]) -> BoxFut<'a, Result<Simulation, String>> {
        self.seen.lock().unwrap().push(tx_bytes.to_vec());
        let r = self.sim.clone();
        Box::pin(async move { r })
    }
}

struct StubTrade(Option<TokenDetail>);

impl TokenInfo for StubTrade {
    fn token_detail<'a>(&'a self, _coin_type: &'a str) -> BoxFut<'a, Option<TokenDetail>> {
        let d = self.0.clone();
        Box::pin(async move { d })
    }
}

/// The simulation reviewTransaction's TS tests get from their SDK stub (1 SUI to Bob).
fn one_sui_to_bob() -> Simulation {
    Simulation {
        balance_changes: vec![bc(SUI, &me(), -1_000_000_000 - FEE), bc(SUI, &bob(), 1_000_000_000)],
        coins: Some(BTreeMap::from([(SUI.to_string(), ChainCoin { symbol: Some("SUI".into()), decimals: 9, name: Some("Sui".into()), icon_url: None })])),
        ..sim()
    }
}

#[tokio::test]
async fn review_simulates_the_given_bytes_and_returns_rows_risks_and_the_same_bytes() {
    let s = FixedSim::new(one_sui_to_bob());
    let bytes = vec![9u8, 9, 9];
    let opts = ReviewOptions { addresses: Some(AddressHistory::default()), ..Default::default() };
    let r = review_transaction(&s, ReviewInput::Bytes(bytes.clone()), &me(), &opts).await.unwrap();
    assert_eq!(*s.seen.lock().unwrap(), vec![bytes.clone()]);
    assert_eq!(r.bytes, Some(bytes));
    assert!(r.ok);
    assert_eq!(row_text(&r.rows), ["You send: 1 SUI", "Network fee: 0.0015 SUI"]);
    assert_eq!(ids(&r.risks), ["pays-others".to_string(), format!("new-recipient-{}", bob())]);
    let c = &r.coins[SUI_TYPE];
    assert_eq!((c.symbol.as_str(), c.decimals, c.price_usd, c.name.as_deref()), ("SUI", 9, None, Some("Sui")));
}

#[tokio::test]
async fn review_uses_trade_prices_for_the_portfolio_share_check_when_it_answers() {
    let s = FixedSim::new(one_sui_to_bob());
    let y = StubTrade(Some(TokenDetail { symbol: Some("SUI".into()), decimals: Some(9), price_usd: Some(2.0), ..Default::default() }));
    let opts = ReviewOptions { trade: Some(&y), portfolio_usd: Some(3.0), ..Default::default() };
    let r = review_transaction(&s, ReviewInput::Bytes(vec![1]), &me(), &opts).await.unwrap();
    assert_eq!(r.coins[SUI_TYPE].price_usd, Some(2.0));
    assert_eq!(r.risks.iter().find(|x| x.id == "large-share").map(|x| x.text.as_str()), Some("Sends about 67% of your portfolio."));
}

#[tokio::test]
async fn review_a_transaction_that_fails_during_build_is_a_failed_review_with_no_bytes() {
    let s = FixedSim::new(one_sui_to_bob());
    let f = SimFailure { message: "InsufficientCoinBalance in command 0".into(), kind: Some("InsufficientCoinBalance".into()), command: Some(0), abort: None };
    let r = review_transaction(&s, ReviewInput::WouldFail(f), &me(), &ReviewOptions::default()).await.unwrap();
    assert!(!r.ok);
    assert_eq!(r.bytes, None);
    assert_eq!(r.rows, vec![]);
    assert_eq!(r.insights.reason.as_deref(), Some("Not enough balance."));
    assert_eq!(ids(&r.risks), ["failed"]);
    assert!(s.seen.lock().unwrap().is_empty());
    // the node down is an error, not a review
    let down = FixedSim { sim: Err("network|fetch failed".into()), seen: Mutex::new(vec![]) };
    let e = review_transaction(&down, ReviewInput::Bytes(vec![1]), &me(), &ReviewOptions::default()).await.unwrap_err();
    assert!(e.contains("fetch failed"));
}

#[tokio::test]
async fn review_still_reviews_when_trade_is_down() {
    let s = FixedSim::new(one_sui_to_bob());
    let y = StubTrade(None);
    let r = review_transaction(&s, ReviewInput::Bytes(vec![1]), &me(), &ReviewOptions { trade: Some(&y), ..Default::default() }).await.unwrap();
    assert!(r.ok);
    assert_eq!(r.coins[SUI_TYPE].price_usd, None);
}

struct Kiosks(Vec<String>, Mutex<u32>);

impl KioskSource for Kiosks {
    fn owned_kiosk_ids<'a>(&'a self, _owner: &'a str) -> BoxFut<'a, Result<Vec<String>, String>> {
        *self.1.lock().unwrap() += 1;
        let k = self.0.clone();
        Box::pin(async move { Ok(k) })
    }
}

struct SlowKiosks;

impl KioskSource for SlowKiosks {
    fn owned_kiosk_ids<'a>(&'a self, _owner: &'a str) -> BoxFut<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok(vec![my_kiosk()])
        })
    }
}

#[tokio::test]
async fn review_reads_my_kiosks_when_the_transaction_touches_object_owned_items() {
    let s = FixedSim::new(moved(own(&bob())));
    let k = Kiosks(vec![my_kiosk()], Mutex::new(0));
    let r = review_transaction(&s, ReviewInput::Bytes(vec![1]), &me(), &ReviewOptions { kiosk_source: Some(&k), ..Default::default() }).await.unwrap();
    assert_eq!(*k.1.lock().unwrap(), 1);
    assert_eq!(row_text(&r.rows)[0], "You send: Popkins (To 0xb2b2\u{2026}b2b2)");
    // known kiosks: no read; a plain coin send: no read
    let r = review_transaction(&s, ReviewInput::Bytes(vec![1]), &me(), &ReviewOptions { kiosk_source: Some(&k), kiosks: Some(vec![]), ..Default::default() }).await.unwrap();
    assert_eq!(r.insights.objects_out, vec![]);
    let coin = FixedSim::new(one_sui_to_bob());
    review_transaction(&coin, ReviewInput::Bytes(vec![1]), &me(), &ReviewOptions { kiosk_source: Some(&k), ..Default::default() }).await.unwrap();
    assert_eq!(*k.1.lock().unwrap(), 1);
    // a slow reader is given up on (best effort)
    let r = review_transaction(&s, ReviewInput::Bytes(vec![1]), &me(), &ReviewOptions { kiosk_source: Some(&SlowKiosks), extras_ms: 20, ..Default::default() }).await.unwrap();
    assert_eq!(r.insights.objects_out, vec![]);
}

#[tokio::test]
async fn review_marks_rare_sponsorship_and_who_pays() {
    let s = FixedSim::new(Simulation { gas_owner: Some(sponsor()), balance_changes: vec![bc(SUI, &sponsor(), -FEE)], ..sim() });
    let opts = ReviewOptions { sponsors: vec![sponsor().to_uppercase().replacen("0X", "0x", 1)], paid_by: Some("Starlight Studio".into()), ..Default::default() };
    let r = review_transaction(&s, ReviewInput::Bytes(vec![1]), &me(), &opts).await.unwrap();
    assert_eq!(row_text(&r.rows), ["Network fee: Paid by Starlight Studio"]);
    let r = review_transaction(&s, ReviewInput::Bytes(vec![1]), &me(), &ReviewOptions::default()).await.unwrap();
    assert_eq!(row_text(&r.rows), ["Network fee: 0 SUI (Paid by someone else)"]);
}

#[test]
fn calls_of_reads_move_calls_from_transaction_bytes() {
    use sui_sdk_types::{Address, Argument, Command, GasPayment, Identifier, Input, MoveCall, ObjectReference, ProgrammableTransaction, Transaction, TransactionExpiration, TransactionKind};
    let a = |b: u8| Address::new([b; 32]);
    let tx = Transaction {
        kind: TransactionKind::ProgrammableTransaction(ProgrammableTransaction {
            inputs: vec![Input::Pure(vec![1])],
            commands: vec![
                Command::MoveCall(MoveCall { package: a(0x22), module: Identifier::new("game").unwrap(), function: Identifier::new("play").unwrap(), type_arguments: vec![], arguments: vec![Argument::Input(0)] }),
                Command::TransferObjects(sui_sdk_types::TransferObjects { objects: vec![Argument::Gas], address: Argument::Input(0) }),
            ],
        }),
        sender: a(0xa1),
        gas_payment: GasPayment { objects: vec![ObjectReference::new(a(0x11), 7, sui_sdk_types::Digest::new([3; 32]))], owner: a(0xa1), price: 1000, budget: 5_000_000 },
        expiration: TransactionExpiration::None,
    };
    let bytes = bcs::to_bytes(&tx).unwrap();
    assert_eq!(calls_of(&bytes), vec![MoveCallRef { package: addr("22"), module: "game".into(), function: "play".into() }]);
    assert_eq!(calls_of(&[9, 9, 9]), vec![]);
}

// ---------- simulate.test.ts ----------

#[test]
fn analyze_splits_send_receive_for_the_signer_and_keeps_the_fee_out_of_the_sui_row() {
    let ins = analyze(&Simulation { balance_changes: vec![bc(SUI, &me(), -10_000_000_000 - 4000), bc(&usdc(), &me(), 39_820_000)], ..sim2() }, &me(), &[], &[]);
    assert!(ins.ok);
    assert_eq!(ins.send, vec![flow(SUI_TYPE, 10_000_000_000)]);
    assert_eq!(ins.receive, vec![flow(&usdc(), 39_820_000)]);
    assert_eq!(ins.fee.net, 4000);
    assert_eq!(ins.others_receive, vec![]);
}

#[test]
fn analyze_a_pure_fee_leaves_no_coin_rows_a_storage_refund_is_not_a_receive() {
    assert_eq!(analyze(&Simulation { balance_changes: vec![bc(SUI, &me(), -4000)], ..sim2() }, &me(), &[], &[]).send, vec![]);
    let refund = analyze(&Simulation { gas: SimGas { computation: "1000".into(), storage: "1000".into(), rebate: "9000".into() }, balance_changes: vec![bc(SUI, &me(), 7000)], ..sim2() }, &me(), &[], &[]);
    assert_eq!(refund.receive, vec![]);
    assert_eq!(refund.fee.net, -7000);
}

#[test]
fn analyze_positive_changes_for_other_addresses_are_recipients_sponsored_gas_stays_out() {
    let ins = analyze(&Simulation { gas_owner: Some(bob()), balance_changes: vec![bc(&usdc(), &me(), -5_000_000), bc(&usdc(), &bob(), 5_000_000), bc(SUI, &bob(), -4000)], ..sim2() }, &me(), &[], &[]);
    assert!(!ins.fee_paid_by_signer);
    assert_eq!(ins.send, vec![flow(&usdc(), 5_000_000)]);
    assert_eq!(ins.others_receive, vec![OtherReceive { address: bob(), coin_type: usdc(), amount: 5_000_000 }]);
    assert_eq!(ins.recipients, vec![bob()]);
}

#[test]
fn analyze_classifies_objects_leaving_arriving_deleted_wrapped_and_moved_into_another_object() {
    let n = Some("0xabc::nft::Nft");
    let usdc_coin = format!("0x2::coin::Coin<{}>", usdc());
    let s = Simulation {
        objects: vec![
            obj_t(&gas(), Some("0x2::coin::Coin<0x2::sui::SUI>")),
            SimObjectChange { output_owner: own(&bob()), ..obj_t("0x01", n) },
            SimObjectChange { input_owner: None, input_exists: false, id_operation: IdOperation::Created, ..obj_t("0x02", n) },
            SimObjectChange { output_owner: None, output_exists: false, id_operation: IdOperation::Deleted, ..obj_t("0x03", n) },
            SimObjectChange { output_owner: None, output_exists: false, ..obj_t("0x04", n) },
            SimObjectChange { output_owner: parent("0xk1"), ..obj_t("0x05", n) },
            obj_t("0x06", n),
            SimObjectChange { output_owner: own(&bob()), ..obj_t("0x07", Some(&usdc_coin)) },
            SimObjectChange { input_owner: own(&bob()), output_owner: own(&me()), ..obj_t("0x08", n) },
            SimObjectChange { output_kind: Some(OutputKind::Accumulator), ..obj_t("0x09", Some("0x2::accumulator::X")) },
        ],
        ..sim2()
    };
    let ins = analyze(&s, &me(), &[], &[]);
    let out: Vec<(String, ObjectFlowKind, Option<String>)> = ins.objects_out.iter().map(|o| (o.object_id.clone(), o.kind, o.to.clone())).collect();
    assert_eq!(
        out,
        vec![
            ("0x01".into(), ObjectFlowKind::Sent, Some(bob())),
            ("0x03".into(), ObjectFlowKind::Deleted, None),
            ("0x04".into(), ObjectFlowKind::Wrapped, None),
            ("0x05".into(), ObjectFlowKind::IntoObject, Some("0xk1".into())),
        ]
    );
    let inn: Vec<(String, Option<bool>)> = ins.objects_in.iter().map(|o| (o.object_id.clone(), o.created)).collect();
    assert_eq!(inn, vec![("0x02".into(), Some(true)), ("0x08".into(), Some(false))]);
    assert_eq!(ins.recipients, vec![bob()]);
}

#[test]
fn analyze_lists_shared_objects_touched_with_the_protocol_name_and_flags_publishes() {
    let pool = format!("{CETUS}::pool::Pool<0x2::sui::SUI, {}>", usdc());
    let s = Simulation {
        objects: vec![
            SimObjectChange { input_owner: Some(Owner::Shared), output_owner: Some(Owner::Shared), ..obj_t("0xp1", Some(&pool)) },
            SimObjectChange { input_owner: Some(Owner::Shared), output_owner: Some(Owner::Shared), ..obj_t("0xp2", Some("0x9999::vault::Vault")) },
            SimObjectChange { input_owner: None, input_exists: false, output_kind: Some(OutputKind::Package), output_owner: Some(Owner::Immutable), ..obj_t("0xpk", None) },
        ],
        ..sim2()
    };
    let ins = analyze(&s, &me(), &[], &[]);
    let shared: Vec<(&str, &str)> = ins.shared.iter().map(|s| (s.label.as_str(), s.sub.as_str())).collect();
    assert_eq!(shared, [("Cetus Pool", "pool::Pool"), ("Vault", "0x9999::vault")]);
    assert!(ins.publishes);
}

fn abort(code: &str, package: &str, module: &str, function: Option<&str>, constant: Option<&str>) -> SimFailure {
    SimFailure {
        message: "x".into(),
        abort: Some(SimAbort { code: code.into(), package: Some(package.into()), module: Some(module.into()), function: function.map(str::to_string), constant: constant.map(str::to_string) }),
        ..Default::default()
    }
}

#[test]
fn friendly_failure_turns_node_errors_into_plain_words() {
    assert_eq!(friendly_failure(Some(&SimFailure::message("InsufficientCoinBalance in command 0"))), "Not enough balance.");
    assert_eq!(friendly_failure(Some(&SimFailure { message: "MoveAbort ...".into(), ..abort("2", "0x2", "balance", Some("split"), None) })), "Not enough balance.");
    assert_eq!(friendly_failure(Some(&abort("6", CETUS, "router", Some("swap"), Some("EAmountOutBelowMinLimit")))), "Amount out below min limit (router::swap).");
    assert_eq!(friendly_failure(Some(&abort("7", CETUS, "router", Some("swap"), None))), "Cetus stopped it in router::swap, code 7.");
    assert!(friendly_failure(Some(&SimFailure::message("Error checking transaction input objects: ObjectNotFound"))).contains("changed or no longer exists"));
    assert_eq!(friendly_failure(Some(&SimFailure::message("Balance of gas object 10 is lower than the needed amount: 100"))), "Not enough SUI to pay the network fee.");
    assert_eq!(friendly_failure(None), "Unknown error.");
}

fn ctx<'a>() -> RiskContext<'a> {
    let u = usdc();
    RiskContext {
        prices: BTreeMap::from([(SUI_TYPE.to_string(), Some(4.0)), (usdc(), Some(1.0))]),
        decimals: Box::new(move |t: &str| if t == u { 6 } else { 9 }),
        ..RiskContext::new()
    }
}

#[test]
fn risk_flags_failure_given_away_objects_and_big_portfolio_share_as_danger() {
    let s = Simulation {
        status: SimStatus::Failure,
        failure: Some(SimFailure::message("x")),
        balance_changes: vec![bc(SUI, &me(), -20_000_000_000 - 4000)],
        objects: vec![SimObjectChange { output_owner: own(&bob()), ..obj_t("0x01", Some("0xabc::nft::Nft")) }],
        ..sim2()
    };
    let ins = analyze(&s, &me(), &[], &[]);
    let r = risk_checks(&ins, &RiskContext { portfolio_usd: Some(100.0), ..ctx() });
    assert_eq!(ids(&r), ["failed", "gives-objects", "large-share"]);
    assert!(r.iter().all(|x| x.danger));
    assert_eq!(r[2].text, "Sends about 80% of your portfolio.");
    assert!(!ids(&risk_checks(&ins, &RiskContext { portfolio_usd: Some(1000.0), ..ctx() })).contains(&"large-share".to_string()));
}

#[test]
fn risk_honeypot_is_danger_spam_and_payments_to_others_are_muted() {
    let scam = norm_type("0xdead::scam::SCAM");
    let s = Simulation { balance_changes: vec![bc(&scam, &me(), 100), bc(&usdc(), &me(), -1_000_000), bc(&usdc(), &bob(), 1_000_000)], ..sim2() };
    let ins = analyze(&s, &me(), &[], &[]);
    let r = risk_checks(&ins, &RiskContext { safety: BTreeMap::from([(scam.clone(), Some(TokenFlags { spam: false, honeypot: true }))]), ..ctx() });
    assert!(r.iter().find(|x| x.id.starts_with("honeypot")).unwrap().danger);
    assert!(!r.iter().find(|x| x.id == "pays-others").unwrap().danger);
    let spam = risk_checks(&ins, &RiskContext { safety: BTreeMap::from([(scam, Some(TokenFlags { spam: true, honeypot: false }))]), ..ctx() });
    assert!(!spam.iter().find(|x| x.id.starts_with("spam")).unwrap().danger);
}

#[test]
fn risk_unknown_packages_are_muted_upgraded_known_packages_resolve_through_their_original_id() {
    let upgraded = addr("77");
    let calls = [
        MoveCallRef { package: upgraded.clone(), module: "pool_script".into(), function: "swap".into() },
        MoveCallRef { package: "0x2".into(), module: "coin".into(), function: "split".into() },
    ];
    let ins = analyze(&sim2(), &me(), &calls, &[]);
    let r = risk_checks(&ins, &ctx());
    let unknown = r.iter().find(|x| x.id == "unknown-package").unwrap();
    assert!(unknown.text.contains("doesn't recognize: 0x7777"));
    assert!(!unknown.danger);
    let known = risk_checks(&ins, &RiskContext { originals: Some(BTreeMap::from([(upgraded, CETUS.to_string())])), ..ctx() });
    assert!(!ids(&known).contains(&"unknown-package".to_string()));
    assert_eq!(risk_checks(&analyze(&sim2(), &me(), &[], &[]), &ctx()), vec![]);
}

#[test]
fn packages_name_known_protocols_and_shorten_ids() {
    assert_eq!(known_package(CETUS, None), Some("Cetus"));
    assert_eq!(known_package("0x0000000000000000000000000000000000000000000000000000000000000002", None), Some("Sui"));
    assert_eq!(short_id("0x2"), "0x2");
    assert_eq!(short_id(CETUS), "0x1eab\u{2026}b2fb");
    assert_eq!(object_label(Some("0x3::sui_system::SuiSystemState"), None).title, "Sui System State");
}

// ---------- address.test.ts ----------

fn mk(head: &str, tail: &str, mid: &str) -> String {
    format!("0x{head}{}{tail}", mid.repeat(64 - 8))
}
fn known_addr() -> String {
    mk("abcd", "1234", "0")
}
fn fake_addr() -> String {
    mk("abcd", "1234", "7")
}
fn new_addr() -> String {
    mk("9999", "8888", "0")
}
fn hist() -> AddressHistory {
    AddressHistory::default()
}

#[test]
fn address_flags_a_first_time_recipient_plainly() {
    let r = address_checks(&[new_addr()], Some(&AddressHistory { sent_to: vec![known_addr()], ..hist() }), None);
    assert_eq!(r, vec![Risk { id: format!("new-recipient-{}", new_addr()), text: "First time sending to this address.".into(), danger: false, requires_confirm: false }]);
}

#[test]
fn address_is_quiet_for_an_address_already_sent_to() {
    let upper = known_addr().to_uppercase().replacen("0X", "0x", 1);
    assert_eq!(address_checks(&[known_addr()], Some(&AddressHistory { sent_to: vec![upper], ..hist() }), None), vec![]);
}

#[test]
fn address_marks_a_lookalike_of_a_sent_to_address_as_danger_and_asks_for_a_confirm() {
    let r = address_checks(&[fake_addr()], Some(&AddressHistory { sent_to: vec![known_addr()], ..hist() }), None);
    assert!(r[0].danger);
    assert!(r[0].requires_confirm);
    assert_eq!(r[0].text, "Looks like 0xabcd\u{2026}1234 you've used before, but it's a different address.");
}

#[test]
fn address_compares_against_received_from_addresses_and_contacts_too() {
    assert!(address_checks(&[fake_addr()], Some(&AddressHistory { received_from: vec![known_addr()], ..hist() }), None)[0].danger);
    assert!(address_checks(&[fake_addr()], Some(&AddressHistory { contacts: vec![Contact { address: known_addr(), name: "Alice".into() }], ..hist() }), None)[0].danger);
}

#[test]
fn address_needs_both_the_first_4_and_last_4_to_match() {
    assert!(!address_checks(&[mk("abcd", "9999", "0")], Some(&AddressHistory { sent_to: vec![known_addr()], ..hist() }), None)[0].danger);
    assert!(!address_checks(&[mk("abce", "1234", "0")], Some(&AddressHistory { sent_to: vec![known_addr()], ..hist() }), None)[0].danger);
}

#[test]
fn address_shows_the_contact_name_instead_of_the_first_time_line() {
    let r = address_checks(&[new_addr()], Some(&AddressHistory { contacts: vec![Contact { address: new_addr(), name: "Alice".into() }], ..hist() }), None);
    assert_eq!(r, vec![Risk { id: format!("contact-{}", new_addr()), text: "Sending to Alice, a saved contact.".into(), danger: false, requires_confirm: false }]);
}

#[test]
fn address_skips_the_first_time_line_when_the_flow_pays_protocol_fees_but_still_catches_lookalikes() {
    assert_eq!(address_checks(&[new_addr()], Some(&AddressHistory { warn_new: false, ..hist() }), None), vec![]);
    assert!(address_checks(&[fake_addr()], Some(&AddressHistory { sent_to: vec![known_addr()], warn_new: false, ..hist() }), None)[0].danger);
}

#[test]
fn address_never_flags_the_signer_and_does_nothing_without_history() {
    assert_eq!(address_checks(&[new_addr()], Some(&hist()), Some(&new_addr())), vec![]);
    assert_eq!(address_checks(&[fake_addr()], None, None), vec![]);
}

fn ins_to(recipients: Vec<String>) -> Insights {
    Insights { recipients, ..Insights::empty() }
}

fn ctx_addr<'a>(addresses: Option<AddressHistory>) -> RiskContext<'a> {
    RiskContext { symbol: Box::new(|_| "X".into()), addresses, ..RiskContext::new() }
}

#[test]
fn risk_with_addresses_puts_the_lookalike_first_so_dapp_approvals_show_it_at_the_top() {
    let r = risk_checks(&ins_to(vec![fake_addr()]), &ctx_addr(Some(AddressHistory { sent_to: vec![known_addr()], ..hist() })));
    assert_eq!(r[0].id, format!("lookalike-{}", fake_addr()));
    assert!(r[0].requires_confirm);
}

#[test]
fn risk_with_addresses_adds_nothing_when_no_history_is_given() {
    assert_eq!(risk_checks(&ins_to(vec![fake_addr()]), &ctx_addr(None)), vec![]);
}

#[test]
fn risk_with_addresses_a_new_recipient_of_a_dapp_transfer_is_noted() {
    assert_eq!(ids(&risk_checks(&ins_to(vec![new_addr()]), &ctx_addr(Some(hist())))), [format!("new-recipient-{}", new_addr())]);
}

// ---------- fees.test.ts ----------

const ROUTER_FEE: &str = "0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06";
const SUI_LONG: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";

/// A 100 SUI swap: 0.2 SUI router fee to the fee wallet, plus optionally coins to a stranger.
fn swap(extra: Vec<OtherReceive>) -> Insights {
    let mut others_receive = vec![OtherReceive { address: ROUTER_FEE.into(), coin_type: SUI_LONG.into(), amount: 200_000_000 }];
    others_receive.extend(extra);
    let recipients = others_receive.iter().map(|o| o.address.clone()).collect();
    Insights { send: vec![flow(SUI_LONG, 100_000_000_000)], others_receive, recipients, ..Insights::empty() }
}

fn fee_ctx<'a>() -> RiskContext<'a> {
    RiskContext { symbol: Box::new(|_| "SUI".into()), addresses: Some(hist()), ..RiskContext::new() }
}

#[test]
fn fees_knows_the_router_fee_wallet_in_any_address_spelling() {
    assert_eq!(known_fee(ROUTER_FEE).map(|k| k.label), Some("Rare fee"));
    assert_eq!(known_fee(&ROUTER_FEE.to_uppercase().replacen("0X", "0x", 1)).map(|k| k.label), Some("Rare fee"));
    assert_eq!(known_fee(&addr("77")), None);
}

#[test]
fn fees_shows_the_swap_fee_as_rare_fee_0_2_pct_not_as_a_recipient() {
    let s = split_fees(&swap(vec![]).others_receive, &swap(vec![]).send);
    assert_eq!(s.fees, vec![FeeLine { address: ROUTER_FEE.into(), label: "Rare fee".into(), coin_type: SUI_LONG.into(), amount: 200_000_000, pct: Some("0.2%".into()) }]);
    assert_eq!(s.others, vec![]);
}

#[test]
fn fees_no_sends_coins_to_or_first_time_warning_for_the_fee_wallet_alone() {
    let r = ids(&risk_checks(&swap(vec![]), &fee_ctx()));
    assert!(!r.contains(&"pays-others".to_string()));
    assert!(!r.iter().any(|i| i.starts_with("new-recipient")));
}

#[test]
fn fees_unknown_recipients_keep_the_full_warning_next_to_the_fee() {
    let stranger = addr("77");
    let ins = swap(vec![OtherReceive { address: stranger.clone(), coin_type: SUI_LONG.into(), amount: 5_000_000_000 }]);
    let s = split_fees(&ins.others_receive, &ins.send);
    assert_eq!(s.fees.len(), 1);
    assert_eq!(s.others.iter().map(|o| o.address.clone()).collect::<Vec<_>>(), vec![stranger.clone()]);
    let r = risk_checks(&ins, &fee_ctx());
    let t = &r.iter().find(|x| x.id == "pays-others").unwrap().text;
    assert!(t.starts_with("Sends coins to 0x7777") && t.ends_with("7777, not you."), "{t}");
    assert!(ids(&r).contains(&format!("new-recipient-{stranger}")));
}

#[test]
fn fees_formats_other_fee_shares() {
    let at = |fee: i128| split_fees(&[OtherReceive { address: ROUTER_FEE.into(), coin_type: SUI_LONG.into(), amount: fee }], &swap(vec![]).send).fees[0].pct.clone();
    assert_eq!(at(1_000_000_000).as_deref(), Some("1%"));
    assert_eq!(at(250_000_000).as_deref(), Some("0.25%"));
    assert_eq!(split_fees(&[OtherReceive { address: ROUTER_FEE.into(), coin_type: SUI_LONG.into(), amount: 1 }], &[]).fees[0].pct, None);
}

// ---------- grpc simulator (feature chain) ----------

#[cfg(feature = "chain")]
mod grpc {
    use super::*;
    use crate::review::sim::{from_response, GrpcSimulator, SimClient};
    use sui_rpc::proto::sui::rpc::v2 as pb;

    fn owner(kind: pb::owner::OwnerKind, a: &str) -> pb::Owner {
        let mut o = pb::Owner::default();
        o.kind = Some(kind as i32);
        o.address = Some(a.into());
        o
    }

    fn changed(id: &str, t: &str, input: pb::Owner, output: pb::Owner) -> pb::ChangedObject {
        use pb::changed_object::{IdOperation, InputObjectState, OutputObjectState};
        let mut c = pb::ChangedObject::default();
        c.object_id = Some(id.into());
        c.object_type = Some(t.into());
        c.input_state = Some(InputObjectState::Exists as i32);
        c.output_state = Some(OutputObjectState::ObjectWrite as i32);
        c.id_operation = Some(IdOperation::None as i32);
        c.input_owner = Some(input);
        c.output_owner = Some(output);
        c
    }

    /// The failed MoveAbort answer from simulate.test.ts.
    fn sdk_result() -> pb::SimulateTransactionResponse {
        let mut loc = pb::MoveLocation::default();
        loc.package = Some("0x2".into());
        loc.module = Some("balance".into());
        loc.function_name = Some("split".into());
        let mut a = pb::MoveAbort::default();
        a.abort_code = Some(2);
        a.location = Some(loc);
        let mut err = pb::ExecutionError::default();
        err.description = Some("MoveAbort".into());
        err.command = Some(0);
        err.error_details = Some(pb::execution_error::ErrorDetails::Abort(a));
        let mut status = pb::ExecutionStatus::default();
        status.success = Some(false);
        status.error = Some(err);
        let mut g = pb::GasCostSummary::default();
        g.computation_cost = Some(100_000);
        g.storage_cost = Some(988_000);
        g.storage_rebate = Some(987_901);
        g.non_refundable_storage_fee = Some(99);
        let gas_change = changed(&gas(), "0x2::coin::Coin<0x2::sui::SUI>", owner(pb::owner::OwnerKind::Address, &me()), owner(pb::owner::OwnerKind::Address, &me()));
        let mut eff = pb::TransactionEffects::default();
        eff.status = Some(status);
        eff.gas_used = Some(g);
        eff.gas_object = Some(gas_change.clone());
        eff.changed_objects = vec![gas_change, changed("0xn1", "0xabc::nft::Nft", owner(pb::owner::OwnerKind::ConsensusAddress, &me()), owner(pb::owner::OwnerKind::Address, &bob()))];
        let mut b = pb::BalanceChange::default();
        b.coin_type = Some(SUI.into());
        b.address = Some(me());
        b.amount = Some("-100099".into());
        let mut t = pb::ExecutedTransaction::default();
        t.effects = Some(eff);
        t.balance_changes = vec![b];
        let mut r = pb::SimulateTransactionResponse::default();
        r.transaction = Some(t);
        r
    }

    #[test]
    fn maps_the_sdk_result_status_abort_location_owners_gas() {
        let s = from_response(&sdk_result());
        assert_eq!(s.status, SimStatus::Failure);
        let f = s.failure.clone().unwrap();
        assert_eq!(f.abort, Some(SimAbort { code: "2".into(), package: Some("0x2".into()), module: Some("balance".into()), function: Some("split".into()), constant: None }));
        assert_eq!(f.message, "MoveAbort in 1st command, abort code: 2, in '0x2::balance::split'");
        assert_eq!(f.kind.as_deref(), Some("MoveAbort"));
        assert_eq!(s.gas_owner, Some(me()));
        assert_eq!(s.gas, SimGas { computation: "100000".into(), storage: "988000".into(), rebate: "987901".into() });
        let o = &s.objects[1];
        assert_eq!((o.object_type.as_deref(), &o.input_owner, &o.output_owner, o.input_exists, o.output_exists), (Some("0xabc::nft::Nft"), &own(&me()), &own(&bob()), true, true));
        assert_eq!(analyze(&s, &me(), &[], &[]).reason.as_deref(), Some("Not enough balance."));
    }

    #[test]
    fn maps_execution_errors_like_the_sdk() {
        use crate::review::sim::map_failure;
        assert_eq!(map_failure(None), SimFailure { message: "Transaction failed".into(), kind: Some("Unknown".into()), command: None, abort: None });
        let mut e = pb::ExecutionError::default();
        e.description = Some("InsufficientCoinBalance in command 0".into());
        e.command = Some(0);
        let f = map_failure(Some(&e));
        assert_eq!((f.message.as_str(), f.kind.as_deref(), f.command), ("InsufficientCoinBalance in command 0", Some("Unknown"), Some(0)));
        let mut c = pb::CleverError::default();
        c.constant_name = Some("EAmountOutBelowMinLimit".into());
        c.line_number = Some(42);
        c.value = Some(pb::clever_error::Value::Rendered("too low".into()));
        let mut loc = pb::MoveLocation::default();
        loc.package = Some(CETUS.trim_start_matches("0x").into());
        loc.module = Some("router".into());
        loc.instruction = Some(9);
        let mut a = pb::MoveAbort::default();
        a.abort_code = Some(6);
        a.location = Some(loc);
        a.clever_error = Some(c);
        e.command = Some(11);
        e.error_details = Some(pb::execution_error::ErrorDetails::Abort(a));
        let f = map_failure(Some(&e));
        assert_eq!(f.message, format!("MoveAbort in 12th command, 'EAmountOutBelowMinLimit': too low, in '{CETUS}::router' (line 42)"));
        assert_eq!(f.abort.unwrap().constant.as_deref(), Some("EAmountOutBelowMinLimit"));
    }

    enum Answer {
        Ok,
        Code(i32, &'static str),
    }

    struct FakeClient {
        answer: Answer,
        seen: Mutex<Vec<Vec<u8>>>,
        extras: bool,
    }

    impl SimClient for FakeClient {
        fn simulate_bytes<'a>(&'a self, tx_bytes: &'a [u8]) -> BoxFut<'a, rare_sui::Result<pb::SimulateTransactionResponse>> {
            self.seen.lock().unwrap().push(tx_bytes.to_vec());
            let r = match self.answer {
                Answer::Ok => Ok(sdk_result()),
                Answer::Code(code, m) => Err(rare_sui::Error::Grpc { code, message: m.into() }),
            };
            Box::pin(async move { r })
        }
        fn display<'a>(&'a self, ids: &'a [String]) -> Option<BoxFut<'a, rare_sui::Result<BTreeMap<String, DisplayInfo>>>> {
            if !self.extras {
                return None;
            }
            assert_eq!(ids, ["0xn1".to_string()]);
            Some(Box::pin(async move { Ok(BTreeMap::from([("0xn1".to_string(), DisplayInfo { name: Some("Nft #1".into()), image: Some("https://x/1.png".into()) })])) }))
        }
        fn coin_metadata<'a>(&'a self, _coin_type: &'a str) -> Option<BoxFut<'a, rare_sui::Result<Option<ChainCoin>>>> {
            if !self.extras {
                return None;
            }
            Some(Box::pin(async move { Ok(Some(ChainCoin { symbol: Some("SUI".into()), decimals: 9, name: None, icon_url: None })) }))
        }
    }

    fn client(answer: Answer, extras: bool) -> GrpcSimulator<FakeClient> {
        GrpcSimulator::new(FakeClient { answer, seen: Mutex::new(vec![]), extras })
    }

    #[tokio::test]
    async fn simulates_the_exact_bytes_adds_display_and_coin_metadata_maps_rejections_rethrows_network_errors() {
        let bytes = [1u8, 2, 3];
        let s = client(Answer::Ok, true);
        let r = s.simulate(&bytes).await.unwrap();
        assert_eq!(*s.client.seen.lock().unwrap(), vec![bytes.to_vec()]);
        assert_eq!(r.display, Some(BTreeMap::from([("0xn1".to_string(), DisplayInfo { name: Some("Nft #1".into()), image: Some("https://x/1.png".into()) })])));
        let c = &r.coins.as_ref().unwrap()[SUI];
        assert_eq!((c.decimals, c.symbol.as_deref()), (9, Some("SUI")));
        // no package service on this client: packages left out
        assert_eq!(r.packages, None);
        assert_eq!(client(Answer::Ok, false).simulate(&bytes).await.unwrap().display, None);

        let rejected = client(Answer::Code(3, "Error checking transaction input objects: ObjectNotFound"), false);
        let r = rejected.simulate(&bytes).await.unwrap();
        assert_eq!(r.status, SimStatus::Failure);
        assert!(analyze(&r, &me(), &[], &[]).reason.unwrap().contains("no longer exists"));

        let down = client(Answer::Code(14, "fetch failed"), false);
        assert!(down.simulate(&bytes).await.unwrap_err().contains("fetch failed"));
    }

    /// The node down while building is an error (nothing listens on port 1).
    #[tokio::test]
    async fn building_with_the_node_down_is_an_error_not_a_review() {
        let sui = rare_sui::Sui::new(rare_sui::Network::Testnet, "http://127.0.0.1:1").unwrap();
        let mut tx = rare_sui::ptb::Tx::new();
        tx.transfer_to(&[rare_sui::ptb::Arg::Gas], &bob());
        let r = crate::review::sim::build_for_review(&sui, &tx, &me(), &rare_sui::tx::BuildOpts::default()).await;
        assert!(r.is_err(), "{r:?}");
    }

    // live.test.ts: mainnet dry run, simulate only. Builds sends FROM a real funded watch address
    // TO a throwaway address, then reviews them. Nothing is signed and nothing is executed.
    //   cargo test -p rare-wallet review::tests::grpc::live -- --ignored --nocapture
    const WATCH: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

    fn show(title: &str, r: &Review) {
        println!(
            "\n== {title}: ok={}{} bytes={}",
            r.ok,
            r.insights.reason.as_ref().map(|x| format!(" reason=\"{x}\"")).unwrap_or_default(),
            r.bytes.as_ref().map_or("none".to_string(), |b| b.len().to_string())
        );
        for row in &r.rows {
            println!("  {}: {}{}", row.label, row.text, row.sub.as_ref().map(|s| format!(" ({s})")).unwrap_or_default());
        }
        for k in &r.risks {
            println!("  risk{}: {}", if k.danger { " [danger]" } else { "" }, k.text);
        }
    }

    #[tokio::test]
    #[ignore = "live mainnet (simulate only)"]
    async fn live_sui_send_sui_max_usdc_send_too_large_send() {
        use rare_sui::tx::{build_coin_send, CoinSend};
        let sui = rare_sui::Sui::new(rare_sui::Network::Mainnet, rare_sui::Network::Mainnet.default_rpc()).unwrap();
        let trade = rare_sui::prices::PriceClient::default();
        let sim = GrpcSimulator::new(sui.clone());
        let to = format!("0x{}", "5a".repeat(32));
        let review = |p: CoinSend, addresses: Option<AddressHistory>| {
            let (sui, sim, trade) = (&sui, &sim, &trade);
            async move {
                let (tx, opts) = build_coin_send(sui, &p).await.map_err(|e| e.to_string())?;
                let input = crate::review::sim::build_for_review(sui, &tx, WATCH, &opts).await?;
                review_transaction(sim, input, WATCH, &ReviewOptions { trade: Some(trade), kiosk_source: Some(sui), addresses, ..Default::default() }).await
            }
        };
        let send = |coin_type: &str, amount: u64| CoinSend { sender: WATCH.into(), to: to.clone(), coin_type: coin_type.into(), amount, ..Default::default() };

        let a = review(send(SUI, 100_000_000), Some(AddressHistory::default())).await.unwrap();
        show("0.1 SUI", &a);
        assert!(a.ok);
        assert_eq!((a.rows[0].label.as_str(), a.rows[0].text.as_str()), ("You send", "0.1 SUI"));

        let m = review(CoinSend { max: true, ..send(SUI, 0) }, None).await.unwrap();
        show("max SUI", &m);
        assert!(m.ok);

        match review(send("0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC", 10_000), None).await {
            Ok(u) => show("0.1 USDC", &u),
            Err(e) => println!("\nUSDC send: {e}"),
        }

        // passes our balance check (balance given) but not the chain: shows the failure path
        let big = review(CoinSend { balance: Some(u64::MAX), ..send(SUI, 10_000_000_000_000) }, None).await.unwrap();
        show("too much SUI", &big);
        assert!(!big.ok);
        assert_eq!(big.bytes, None);
    }
}
