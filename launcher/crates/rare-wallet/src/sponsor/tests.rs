//! Port of src/wallet/sponsor.test.ts and sponsor.game.test.ts against a mocked rare.market
//! sponsor API (the shapes and codes of market api/src/sponsor), plus TS cross-checks.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use sui_sdk_types::{Address, Digest, GasPayment, Input, ObjectReference, ProgrammableTransaction, TransactionExpiration, TransferObjects};

fn a(n: u64) -> String {
    format!("0x{n:064x}")
}
fn addr(n: u64) -> Address {
    a(n).parse().unwrap()
}
fn user() -> String {
    a(0xabc)
}
fn sponsor_addr() -> String {
    a(0x5905)
}
fn oref(n: u8) -> ObjectReference {
    ObjectReference::new(addr(n as u64), 7, Digest::new([n % 250; 32]))
}

/// A transfer with every input resolved; `gas` sends the gas coin itself.
fn transfer_kind(gas: bool, obj: u8, to: u64) -> String {
    let p = ProgrammableTransaction {
        inputs: vec![Input::ImmutableOrOwned(oref(obj)), Input::Pure(bcs::to_bytes(&addr(to)).unwrap())],
        commands: vec![Command::TransferObjects(TransferObjects { objects: vec![if gas { Argument::Gas } else { Argument::Input(0) }], address: Argument::Input(1) })],
    };
    B64.encode(bcs::to_bytes(&TransactionKind::ProgrammableTransaction(p)).unwrap())
}

fn transfer() -> String {
    transfer_kind(false, 0x77, 0xdef)
}

/// What the server answers: the kind wrapped in full TransactionData with `owner` paying gas.
fn sponsored_bytes(kind_b64: &str, owner: &str) -> String {
    let kind: TransactionKind = bcs::from_bytes(&B64.decode(kind_b64).unwrap()).unwrap();
    let tx = Transaction {
        kind,
        sender: user().parse().unwrap(),
        gas_payment: GasPayment { objects: vec![oref(0x99)], owner: owner.parse().unwrap(), price: 1000, budget: 5_000_000 },
        expiration: TransactionExpiration::None,
    };
    B64.encode(bcs::to_bytes(&tx).unwrap())
}

fn status_json() -> Value {
    json!({
        "enabled": true, "funded": true, "address": sponsor_addr(),
        "balanceMist": "5000000000", "dailyBudgetMist": "1000000000", "spentTodayMist": "0", "remainingTodayMist": "1000000000",
        "categories": { "rescue": true, "trophy": true, "trade": true, "walrus": false },
        "maxUserSuiMist": "50000000",
        "you": { "sponsored": 0, "gasMist": "0", "tradeUsed": false, "eligible": { "rescue": true, "trophy": true, "trade": true, "walrus": false } }
    })
}

fn game_status(game: Option<Value>, over: Value) -> Value {
    let mut s = status_json();
    s.as_object_mut().unwrap().remove("you");
    if let Some(g) = game {
        let mut base = json!({ "slug": "act", "covered": true, "availableMist": "2000000000", "paused": false, "you": { "todayTx": 0, "todayMist": "0", "eligible": true } });
        for (k, v) in g.as_object().unwrap() {
            base[k] = v.clone();
        }
        s["game"] = base;
    }
    for (k, v) in over.as_object().unwrap() {
        s[k] = v.clone();
    }
    s
}

type Post = Box<dyn Fn(&str, &Value) -> Result<(u16, Value), String> + Send + Sync>;

struct Mock {
    status: Option<Value>,
    post: Post,
    now: AtomicU64,
    gets: Mutex<Vec<String>>,
    posts: Mutex<Vec<(String, Value)>>,
}

impl SponsorApi for Arc<Mock> {
    fn get(&self, path: &str) -> Result<Value, String> {
        self.gets.lock().unwrap().push(path.to_string());
        self.status.clone().map(|s| json!({ "data": s })).ok_or_else(|| "rare.market 503".to_string())
    }
    fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), String> {
        self.posts.lock().unwrap().push((path.to_string(), body.clone()));
        (self.post)(path, body)
    }
    fn now_ms(&self) -> u64 {
        self.now.fetch_add(1000, Ordering::SeqCst) + 1000
    }
}

fn deps(status: Option<Value>, post: impl Fn(&str, &Value) -> Result<(u16, Value), String> + Send + Sync + 'static) -> (Sponsor<Arc<Mock>>, Arc<Mock>) {
    let m = Arc::new(Mock { status, post: Box::new(post), now: AtomicU64::new(1_000_000), gets: Mutex::default(), posts: Mutex::default() });
    (Sponsor::new(m.clone()), m)
}

fn ok(data: Value) -> Result<(u16, Value), String> {
    Ok((200, json!({ "data": data })))
}
fn err(status: u16, code: &str, message: &str) -> Result<(u16, Value), String> {
    Ok((status, json!({ "error": { "code": code, "message": message } })))
}

fn grant_with(body: &Value, over: Value) -> Result<(u16, Value), String> {
    let mut d = json!({
        "bytes": sponsored_bytes(body["kind"].as_str().unwrap(), &sponsor_addr()),
        "sponsorSignature": "c3BvbnNvcg==",
        "digest": "D1",
        "sponsor": sponsor_addr(),
        "category": body["category"],
        "gasBudget": "5000000",
        "estimatedGasMist": "1200000",
        "expiresAt": 9_999_999_999_999u64,
    });
    for (k, v) in over.as_object().unwrap() {
        d[k] = v.clone();
    }
    ok(d)
}
fn grant(body: &Value) -> Result<(u16, Value), String> {
    grant_with(body, json!({}))
}

// ---------------------------------------------------------------- sponsor.test.ts

#[test]
fn needs_sponsor_uses_the_estimate_plus_a_margin() {
    assert!(needs_sponsor(0, None));
    assert!(needs_sponsor(14_000_000, None)); // 0.01 + 0.005 margin
    assert!(!needs_sponsor(16_000_000, None));
    assert!(needs_sponsor(3_000_000, Some(1_000_000))); // 1M + 5M floor
    assert!(!needs_sponsor(7_000_000, Some(1_000_000)));
    assert!(needs_sponsor(100_000_000, Some(80_000_000))); // 80M + 40M
    assert!(needs_sponsor(0, Some(0)));
}

#[test]
fn offers_only_when_sui_is_short_and_the_category_is_eligible() {
    let (s, _) = deps(Some(status_json()), |_, _| ok(json!({})));
    assert!(s.offer(&user(), &Category::Trade, || Some(0), None).is_some());
    s.clear_status(None);
    assert!(s.offer(&user(), &Category::Trade, || Some(1_000_000_000), None).is_none());
    s.clear_status(None);
    assert!(s.offer(&user(), &Category::Walrus, || Some(0), None).is_none());
    let mut used = status_json();
    used["you"]["tradeUsed"] = json!(true);
    let (s, _) = deps(Some(used), |_, _| ok(json!({})));
    assert!(s.offer(&user(), &Category::Trade, || Some(0), None).is_none());
    // holds more than the server sponsors
    let mut capped = status_json();
    capped["maxUserSuiMist"] = json!("1000");
    let (s, _) = deps(Some(capped), |_, _| ok(json!({})));
    assert!(s.offer(&user(), &Category::Trade, || Some(5_000), None).is_none());
}

#[test]
fn not_funded_or_unreachable_no_offer() {
    let mut nf = status_json();
    nf["funded"] = json!(false);
    let (s, _) = deps(Some(nf), |_, _| ok(json!({})));
    assert!(s.offer(&user(), &Category::Trade, || Some(0), None).is_none());
    let (s, _) = deps(None, |_, _| ok(json!({})));
    assert!(s.offer(&user(), &Category::Trade, || Some(0), None).is_none());
    // the balance read failed: no offer
    let (s, _) = deps(Some(status_json()), |_, _| ok(json!({})));
    assert!(s.offer(&user(), &Category::Trade, || None, None).is_none());
}

#[test]
fn status_is_cached_for_30s_and_records_the_sponsor() {
    let (s, m) = deps(Some(status_json()), |_, _| ok(json!({})));
    s.status(&user());
    s.status(&user());
    assert_eq!(m.gets.lock().unwrap().len(), 1);
    assert_eq!(m.gets.lock().unwrap()[0], format!("/v1/sponsor/status?address={}", user()));
    assert_eq!(s.known_sponsors(), vec![sponsor_addr()]);
    m.now.fetch_add(31_000, Ordering::SeqCst);
    s.status(&user());
    assert_eq!(m.gets.lock().unwrap().len(), 2);
}

#[test]
fn success_sends_the_kind_and_checks_the_answer() {
    let (s, m) = deps(Some(status_json()), |_, b| grant(b));
    let r = s.sponsor_kind(&transfer(), &user(), &Category::Trade).unwrap();
    assert_eq!(r.sponsor, sponsor_addr());
    assert_eq!(r.gas_budget, 5_000_000);
    assert_eq!(r.estimated_gas_mist, 1_200_000);
    assert_eq!(r.category, "trade");
    let posts = m.posts.lock().unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].0, "/v1/sponsor");
    assert_eq!(posts[0].1["sender"], user());
    assert_eq!(posts[0].1["category"], "trade");
    assert_eq!(posts[0].1["kind"], transfer());
}

#[test]
fn no_answers_are_typed_results() {
    let r = |post: Result<(u16, Value), String>| {
        let (s, _) = deps(Some(status_json()), move |_, _| post.clone());
        s.sponsor_kind(&transfer(), &user(), &Category::Trade).unwrap_err()
    };
    let f = r(err(503, "sponsor_unavailable", "sponsor unavailable: not funded"));
    assert_eq!((f.reason, f.code.as_str(), f.message.as_str()), (FailReason::Unavailable, "sponsor_unavailable", "Rare can't cover fees right now."));
    let f = r(err(403, "not_sponsored", "first trade already used"));
    assert_eq!((f.reason, f.message.as_str()), (FailReason::Refused, "first trade already used"));
    assert_eq!(r(err(503, "sponsor_busy", "busy")).reason, FailReason::Busy);
    assert_eq!(r(err(429, "limit_reached", "limit")).reason, FailReason::Limited);
    assert_eq!(r(err(422, "x", "")).reason, FailReason::Simulation);
    let f = r(err(500, "boom", ""));
    assert_eq!((f.reason, f.message.as_str()), (FailReason::Failed, "Sponsor error (500)"));
    assert_eq!(r(Ok((500, json!({ "error": "teapot" })))).code, "teapot");
    assert_eq!(r(Err("offline".into())).reason, FailReason::Network);
    assert_eq!(r(ok(json!({ "bytes": "" }))).reason, FailReason::Mismatch);
}

#[test]
fn refuses_an_answer_whose_gas_owner_or_transaction_differ() {
    let (s, _) = deps(Some(status_json()), |_, b| grant_with(b, json!({ "bytes": sponsored_bytes(b["kind"].as_str().unwrap(), &a(0xbad)) })));
    let f = s.sponsor_kind(&transfer(), &user(), &Category::Trade).unwrap_err();
    assert_eq!(f.reason, FailReason::Mismatch);
    assert!(f.message.contains("gas owner is not the sponsor"));
    let other = transfer_kind(false, 0x78, 0xbad);
    let (s, _) = deps(Some(status_json()), move |_, b| grant_with(b, json!({ "bytes": sponsored_bytes(&other, &sponsor_addr()) })));
    let f = s.sponsor_kind(&transfer(), &user(), &Category::Trade).unwrap_err();
    assert!(f.message.contains("transaction changed"));
    // a sponsor that names the sender as the gas owner
    let (s, _) = deps(Some(status_json()), |_, b| grant_with(b, json!({ "bytes": sponsored_bytes(b["kind"].as_str().unwrap(), &user()), "sponsor": user() })));
    assert!(s.sponsor_kind(&transfer(), &user(), &Category::Trade).unwrap_err().message.contains("gas owner is the sender"));
}

#[test]
fn gas_coin_kinds_are_never_sent() {
    let (s, m) = deps(Some(status_json()), |_, b| grant(b));
    let f = s.sponsor_kind(&transfer_kind(true, 0x77, 0xdef), &user(), &Category::Trade).unwrap_err();
    assert_eq!(f.reason, FailReason::GasCoin);
    // split from the gas coin
    let p = ProgrammableTransaction {
        inputs: vec![Input::Pure(bcs::to_bytes(&1000u64).unwrap()), Input::Pure(bcs::to_bytes(&addr(0xdef)).unwrap())],
        commands: vec![
            Command::SplitCoins(sui_sdk_types::SplitCoins { coin: Argument::Gas, amounts: vec![Argument::Input(0)] }),
            Command::TransferObjects(TransferObjects { objects: vec![Argument::NestedResult(0, 0)], address: Argument::Input(1) }),
        ],
    };
    let split = B64.encode(bcs::to_bytes(&TransactionKind::ProgrammableTransaction(p)).unwrap());
    assert_eq!(s.sponsor_kind(&split, &user(), &Category::Rescue).unwrap_err().reason, FailReason::GasCoin);
    assert_eq!(s.sponsor_kind("!!", &user(), &Category::Trade).unwrap_err().reason, FailReason::Build);
    assert!(m.posts.lock().unwrap().is_empty());
}

#[test]
fn execute_passes_digest_and_signature_and_maps_failures() {
    let (s, m) = deps(None, |_, _| ok(json!({ "digest": "D1", "ok": true, "gasUsedMist": "1500000" })));
    assert_eq!(s.execute("D1", "sig").unwrap(), Executed { digest: "D1".into(), gas_used_mist: 1_500_000 });
    assert_eq!(m.posts.lock().unwrap()[0], ("/v1/sponsor/execute".to_string(), json!({ "digest": "D1", "signature": "sig" })));
    let r = |post: Result<(u16, Value), String>| {
        let (s, _) = deps(None, move |_, _| post.clone());
        s.execute("D1", "sig").unwrap_err()
    };
    assert_eq!(r(err(410, "sponsorship_expired", "expired")).reason, FailReason::Expired);
    assert_eq!(r(err(409, "already_executed", "done")).reason, FailReason::AlreadyExecuted);
    let f = r(ok(json!({ "digest": "D1", "ok": false, "error": "MoveAbort", "gasUsedMist": "1" })));
    assert_eq!((f.reason, f.message.as_str()), (FailReason::Failed, "MoveAbort"));
}

#[test]
fn run_sponsors_signs_the_exact_bytes_executes() {
    let granted = Arc::new(Mutex::new(String::new()));
    let g = granted.clone();
    let (s, _) = deps(Some(status_json()), move |p, b| {
        if p == "/v1/sponsor" {
            let r = grant(b);
            *g.lock().unwrap() = r.as_ref().unwrap().1["data"]["bytes"].as_str().unwrap().to_string();
            return r;
        }
        ok(json!({ "digest": b["digest"], "ok": true, "gasUsedMist": "100" }))
    });
    let mut signed = 0;
    let r = s
        .run(&transfer(), &user(), &Category::Trade, None, |bytes| {
            signed += 1;
            assert_eq!(B64.encode(bytes), *granted.lock().unwrap());
            Ok::<_, ()>("user-sig".to_string())
        })
        .unwrap();
    assert_eq!((r.signature.as_str(), r.sponsor.as_str(), r.gas_used_mist), ("user-sig", sponsor_addr().as_str(), 100));
    assert_eq!(signed, 1);
}

#[test]
fn run_expired_at_execute_asks_again_once_then_gives_up() {
    let executes = Arc::new(AtomicU64::new(0));
    let e = executes.clone();
    let (s, m) = deps(Some(status_json()), move |p, b| {
        if p == "/v1/sponsor" {
            return grant(b);
        }
        if e.fetch_add(1, Ordering::SeqCst) == 0 {
            err(410, "sponsorship_expired", "expired")
        } else {
            ok(json!({ "digest": b["digest"], "ok": true, "gasUsedMist": "100" }))
        }
    });
    let mut signed = 0;
    assert!(s.run(&transfer(), &user(), &Category::Trade, None, |_| { signed += 1; Ok::<_, ()>("sig".into()) }).is_ok());
    assert_eq!(m.posts.lock().unwrap().iter().filter(|p| p.0 == "/v1/sponsor").count(), 2);
    assert_eq!(signed, 2);

    let (s, m) = deps(Some(status_json()), |p, b| if p == "/v1/sponsor" { grant(b) } else { err(410, "sponsorship_expired", "expired") });
    let f = s.run(&transfer(), &user(), &Category::Trade, None, |_| Ok::<_, ()>("sig".into())).unwrap_err().unwrap();
    assert_eq!(f.reason, FailReason::Expired);
    assert_eq!(m.posts.lock().unwrap().iter().filter(|p| p.0 == "/v1/sponsor").count(), 2);
}

#[test]
fn run_replaces_a_sponsorship_past_its_time_before_signing() {
    let (s, m) = deps(Some(status_json()), |p, b| if p == "/v1/sponsor" { grant(b) } else { ok(json!({ "digest": b["digest"], "ok": true, "gasUsedMist": "1" })) });
    let mut first = s.sponsor_kind(&transfer(), &user(), &Category::Trade).unwrap();
    first.expires_at = 0;
    assert!(s.run(&transfer(), &user(), &Category::Trade, Some(first), |_| Ok::<_, ()>("sig".into())).is_ok());
    assert_eq!(m.posts.lock().unwrap().iter().filter(|p| p.0 == "/v1/sponsor").count(), 2);
}

#[test]
fn run_not_funded_signs_nothing_and_signer_errors_pass_through() {
    let (s, _) = deps(Some(status_json()), |_, _| err(503, "sponsor_unavailable", "sponsor unavailable: not funded"));
    let mut signed = false;
    let f = s.run(&transfer(), &user(), &Category::Rescue, None, |_| { signed = true; Ok::<_, ()>("sig".into()) }).unwrap_err().unwrap();
    assert_eq!(f.reason, FailReason::Unavailable);
    assert!(!signed);
    let (s, _) = deps(Some(status_json()), |_, b| grant(b));
    let e = s.run(&transfer(), &user(), &Category::Trade, None, |_| Err::<String, _>("rejected")).unwrap_err();
    assert_eq!(e.unwrap_err(), "rejected");
}

// ---------------------------------------------------------------- sponsor.game.test.ts

#[test]
fn game_categories_for_slugs_the_api_accepts() {
    assert_eq!(game_category(Some("pawtato-land")), Some(Category::Game("pawtato-land".into())));
    assert_eq!(game_category(Some("pawtato-land")).unwrap().as_str(), "game:pawtato-land");
    assert_eq!(game_category(None), None);
    assert_eq!(game_category(Some("Bad Slug")), None);
    assert_eq!(game_category(Some("-x")), None);
    assert_eq!(game_category(Some(&"a".repeat(65))), None);
    assert!(game_category(Some(&"a".repeat(64))).is_some());
}

fn parsed(v: Value) -> SponsorStatus {
    serde_json::from_value(v).unwrap()
}

#[test]
fn reads_the_games_budget_from_status() {
    let r = |game: Option<Value>, over: Value| game_not_covered(Some(&parsed(game_status(game, over))), "act").map(|f| (f.reason, f.code));
    assert_eq!(r(Some(json!({})), json!({})), None);
    assert_eq!(r(Some(json!({ "availableMist": "0", "covered": false })), json!({})), Some((FailReason::Exhausted, "budget_exhausted".into())));
    assert_eq!(r(Some(json!({ "paused": true })), json!({})).unwrap().0, FailReason::NotCovered);
    assert_eq!(r(Some(json!({ "you": { "todayTx": 50, "todayMist": "1", "eligible": false } })), json!({})).unwrap().0, FailReason::Limited);
    assert_eq!(r(None, json!({})).unwrap().0, FailReason::NotCovered);
    assert_eq!(r(Some(json!({})), json!({ "funded": false })).unwrap().0, FailReason::Unavailable);
    assert_eq!(r(Some(json!({ "slug": "other" })), json!({})).unwrap().0, FailReason::NotCovered);
    assert_eq!(game_not_covered(None, "act").unwrap().reason, FailReason::Network);
}

fn game_kind() -> String {
    let p = ProgrammableTransaction {
        inputs: vec![Input::ImmutableOrOwned(oref(0x77))],
        commands: vec![Command::MoveCall(sui_sdk_types::MoveCall {
            package: addr(0x9a),
            module: sui_sdk_types::Identifier::new("game").unwrap(),
            function: sui_sdk_types::Identifier::new("play").unwrap(),
            type_arguments: vec![],
            arguments: vec![Argument::Input(0)],
        })],
    };
    B64.encode(bcs::to_bytes(&TransactionKind::ProgrammableTransaction(p)).unwrap())
}

#[test]
fn sponsor_for_game_asks_with_category_game_slug() {
    let (s, m) = deps(Some(game_status(Some(json!({})), json!({}))), |_, b| grant(b));
    let (r, kind, cat) = s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "act").unwrap();
    assert_eq!(m.gets.lock().unwrap()[0], format!("/v1/sponsor/status?address={}&game=act", user()));
    assert_eq!(m.posts.lock().unwrap()[0].1["category"], "game:act");
    assert_eq!(cat, Category::Game("act".into()));
    assert_eq!(r.category, "game:act");
    assert_eq!(kind, game_kind());
}

#[test]
fn sponsor_for_game_takes_full_bytes_and_sends_only_their_kind() {
    let kind: TransactionKind = bcs::from_bytes(&B64.decode(game_kind()).unwrap()).unwrap();
    let tx = Transaction {
        kind,
        sender: user().parse().unwrap(),
        gas_payment: GasPayment { objects: vec![oref(0x55)], owner: user().parse().unwrap(), price: 1000, budget: 3_000_000 },
        expiration: TransactionExpiration::None,
    };
    let bytes = bcs::to_bytes(&tx).unwrap();
    assert_eq!(kind_of_bytes(&bytes).unwrap(), game_kind());
    let (s, m) = deps(Some(game_status(Some(json!({})), json!({}))), |_, b| grant(b));
    s.sponsor_for_game(GameTx::Bytes(&bytes), &user(), "act").unwrap();
    assert_eq!(m.posts.lock().unwrap()[0].1["kind"], game_kind());
}

#[test]
fn budget_exhausted_the_player_pays_with_a_note() {
    let (s, _) = deps(Some(game_status(Some(json!({})), json!({}))), |_, _| err(429, "budget_exhausted", "This game's gas budget is used up"));
    let f = s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "act").unwrap_err();
    assert_eq!(f.reason, FailReason::Exhausted);
    assert_eq!(game_fee_note(&f, "ACT").as_deref(), Some("ACT's gas budget is used up, so you pay this fee."));
}

#[test]
fn unavailable_refused_and_limits_fall_back_quietly_or_with_the_limit_note() {
    for (answer, reason, note) in [
        (err(503, "sponsor_unavailable", "sponsor_unavailable"), FailReason::Unavailable, None),
        (err(403, "not_sponsored", "Not covered by ACT's gas"), FailReason::Refused, None),
        (err(429, "rate_limited", "rate_limited"), FailReason::Limited, Some("ACT has covered as many of your fees as it can today, so you pay this one.")),
    ] {
        let (s, _) = deps(Some(game_status(Some(json!({})), json!({}))), move |_, _| answer.clone());
        let f = s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "act").unwrap_err();
        assert_eq!(f.reason, reason);
        assert_eq!(game_fee_note(&f, "ACT").as_deref(), note);
    }
}

#[test]
fn does_not_ask_when_status_says_no_or_the_kind_uses_gas() {
    let (s, m) = deps(Some(game_status(Some(json!({ "availableMist": "0", "covered": false })), json!({}))), |_, b| grant(b));
    assert_eq!(s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "act").unwrap_err().reason, FailReason::Exhausted);
    assert!(m.posts.lock().unwrap().is_empty());
    let (s, m) = deps(None, |_, b| grant(b));
    assert_eq!(s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "act").unwrap_err().reason, FailReason::Network);
    assert!(m.posts.lock().unwrap().is_empty());
    let (s, m) = deps(Some(game_status(Some(json!({})), json!({}))), |_, b| grant(b));
    assert_eq!(s.sponsor_for_game(GameTx::Kind(&transfer_kind(true, 1, 2)), &user(), "act").unwrap_err().reason, FailReason::GasCoin);
    assert!(m.posts.lock().unwrap().is_empty());
    assert_eq!(s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "Bad Slug").unwrap_err().reason, FailReason::NotCovered);
}

#[test]
fn an_exhausted_or_limited_answer_drops_the_cached_game_status() {
    let (s, m) = deps(Some(game_status(Some(json!({})), json!({}))), |_, _| err(429, "budget_exhausted", "x"));
    s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "act").unwrap_err();
    s.sponsor_for_game(GameTx::Kind(&game_kind()), &user(), "act").unwrap_err();
    assert_eq!(m.gets.lock().unwrap().len(), 2);
}

// ---------------------------------------------------------------- TS cross-checks

#[test]
fn ts_kind_of_bytes_and_rules_match() {
    let v: Value = serde_json::from_str(include_str!("../../testdata/ts-vectors.json")).unwrap();
    let sp = &v["sponsor"];
    let game = B64.decode(sp["gameBytes"].as_str().unwrap()).unwrap();
    assert_eq!(kind_of_bytes(&game).unwrap(), sp["gameKind"].as_str().unwrap());
    let gas = B64.decode(sp["gasBytes"].as_str().unwrap()).unwrap();
    let f = kind_of_bytes(&gas).unwrap_err();
    assert_eq!((f.reason.as_str(), f.code.as_str(), f.message.as_str()), (sp["gasResult"]["reason"].as_str().unwrap(), sp["gasResult"]["code"].as_str().unwrap(), sp["gasResult"]["message"].as_str().unwrap()));
    let needs: Vec<bool> = [0, 14_000_000, 16_000_000].iter().map(|b| needs_sponsor(*b, None)).collect();
    assert_eq!(json!(needs), sp["needs"]);
    let cats: Vec<Value> = ["pawtato-land", "Bad Slug", "-x", &"a".repeat(64), &"a".repeat(65)].iter().map(|s| game_category(Some(s)).map(|c| json!(c.as_str())).unwrap_or(Value::Null)).collect();
    assert_eq!(json!(cats), sp["categories"]);
}
