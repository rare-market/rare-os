use super::*;
use crate::screens::wallet::kit::sheet_b;
use crate::screens::wallet::sign::fixtures::*;

struct Fx {
    req: SignRequest,
    flow: Option<Flow>,
    accounts: Vec<Account>,
    load: Load,
    items: BTreeMap<String, ItemInfo>,
    force: Option<Hw>,
    ledger: LedgerPrompt,
    keystone: KeystonePrompt,
    passkey: Passkey,
    method: Option<&'static str>,
}

impl Fx {
    fn new(req: SignRequest, load: Load) -> Fx {
        Fx { req, flow: None, accounts: accounts(), load, items: BTreeMap::new(), force: None, ledger: LedgerPrompt::default(), keystone: KeystonePrompt::default(), passkey: Passkey::Idle, method: None }
    }
    fn ready(req: SignRequest, r: Review) -> Fx {
        Fx::new(req, Load::Ready(Box::new(r)))
    }
    fn view(&self, st: &State) -> El<Msg> {
        view(
            &Props {
                req: &self.req,
                flow: self.flow.as_ref(),
                accounts: &self.accounts,
                load: &self.load,
                items: &self.items,
                force: self.force,
                ledger: &self.ledger,
                keystone: &self.keystone,
                passkey: &self.passkey,
                method: self.method,
                phone: false,
            },
            st,
        )
    }
}

fn working() -> Option<Flow> {
    Some(Flow { phase: Phase::Working, error: None, final_: false, result: None })
}

#[test]
fn loading_holds_approve() {
    let f = Fx::new(send_request(), Load::Loading);
    let v = f.view(&State::default());
    assert_eq!(v.texts(), ["Send 0.1 SUI", "Rare Wallet · Main", "Checking what this does", "Checking", "Cancel"]);
    assert!(v.find_nav("Checking").unwrap().on.is_none());
    assert_eq!(v.landing().as_deref(), Some("Cancel"));
    assert_eq!(sheet_b(&v), Some(Msg::Cancel));
}

#[test]
fn a_send_reads_like_the_web_sheet() {
    let f = Fx::ready(send_request(), send_review());
    let v = f.view(&State::default());
    assert_eq!(
        v.texts(),
        [
            "Send 0.1 SUI",
            "Rare Wallet · Main",
            "You send",
            "SUI",
            "$0.91",
            "-0.1 SUI",
            "$0.09",
            "Goes to",
            "0xabab...abab",
            "0.1 SUI",
            "Account",
            "Main ",
            "0x7d20...b58e",
            "Network fee",
            "0.001997 SUI",
            "<$0.01",
            "You have not sent to 0xabab...abab before.",
            "Approve",
            "Cancel",
        ]
    );
    assert_eq!(v.landing().as_deref(), Some("Approve"));
    assert_eq!(v.find_nav("Approve").unwrap().on, Some(Msg::Approve));
}

#[test]
fn rare_fee_is_a_fee_line_and_details_fold() {
    let f = Fx::ready(send_request(), swap_review());
    let mut st = State::default();
    let v = f.view(&st);
    let t = v.texts();
    assert!(t.contains(&"Rare fee 0.2%".to_string()));
    assert!(t.contains(&"0.02 SUI".to_string()));
    assert!(!t.contains(&"Goes to".to_string()));
    assert!(t.contains(&"+9.0412 USDC".to_string()));
    assert!(t.contains(&"$9.04".to_string()));
    // Details closed: the toggle only.
    assert!(t.contains(&"Details".to_string()));
    assert!(!t.contains(&"pool_script_v2::swap_a2b".to_string()));
    assert_eq!(update(&mut st, false, &Msg::Details), None);
    let t = f.view(&st).texts();
    let i = t.iter().position(|x| x == "pool_script_v2::swap_a2b").unwrap();
    assert_eq!(t[i + 1], "Cetus");
    assert_eq!(t[i + 2], "Cetus Pool");
    assert_eq!(t[i + 3], "0xcf99…8630");
}

#[test]
fn a_failing_transaction_blocks_until_approve_anyway() {
    let f = Fx::ready(send_request(), failed_review());
    let mut st = State::default();
    let v = f.view(&st);
    let t = v.texts();
    assert_eq!(&t[2..4], ["This would fail", "Not enough balance for this."]);
    // The "failed" risk is the banner already, not a note too.
    assert!(!t.contains(&"This transaction would fail.".to_string()));
    // Details start open for a failure.
    assert!(t.contains(&"balance::split".to_string()));
    assert!(v.contains_text("Approve is off because this would fail. "));
    assert!(v.find_nav("Approve").unwrap().on.is_none());
    assert_eq!(v.landing().as_deref(), Some("Cancel"));
    update(&mut st, true, &Msg::Anyway);
    let v = f.view(&st);
    assert!(v.contains_text("It will likely fail and still cost the network fee."));
    assert_eq!(v.find_nav("Approve").unwrap().on, Some(Msg::Approve));
    // Details toggle from open.
    update(&mut st, true, &Msg::Details);
    assert!(!f.view(&st).contains_text("balance::split"));
}

#[test]
fn a_lookalike_needs_the_extra_tick() {
    let f = Fx::ready(send_request(), lookalike_review());
    let mut st = State::default();
    let v = f.view(&st);
    let t = v.texts();
    // danger notes first
    let a = t.iter().position(|x| x.starts_with("This address looks like")).unwrap();
    let b = t.iter().position(|x| x.starts_with("You have not sent")).unwrap();
    assert!(a < b);
    assert!(v.contains_text("I checked the full address and it is right"));
    assert!(v.find_nav("Approve").unwrap().on.is_none());
    let ack = v.find_nav("I checked the full address and it is right").unwrap().on.clone().unwrap();
    assert_eq!(ack, Msg::Ack(true));
    update(&mut st, false, &ack);
    assert_eq!(f.view(&st).find_nav("Approve").unwrap().on, Some(Msg::Approve));
}

#[test]
fn sponsored_items_and_nothing_moving() {
    let f = Fx::ready(send_request(), sponsored_review());
    let v = f.view(&State::default());
    let t = v.texts();
    assert!(t.windows(3).any(|w| w == ["You receive", "Rusty Blade", "New"]));
    assert!(t.contains(&"Paid by ACT".to_string()));
    let f = Fx::ready(send_request(), nothing_review());
    let v = f.view(&State::default());
    assert!(v.contains_text("No coins or objects move in or out of your wallet."));
    assert!(v.contains_text("0.00012 SUI back"));
    assert!(v.contains_text("Storage refund is larger than the fee"));
}

#[test]
fn rare_market_names_an_object_row() {
    let mut f = Fx::ready(send_request(), sponsored_review());
    f.items.insert("0x77".into(), ItemInfo { name: Some("Rusty Blade #12".into()), image_url: None, collection: Some("ACT Items".into()) });
    let t = f.view(&State::default()).texts();
    // row.sub wins over the collection name
    assert!(t.windows(2).any(|w| w == ["Rusty Blade #12", "New"]));
}

#[test]
fn unavailable_says_so() {
    let mut req = send_request();
    req.origin = Origin { name: "Shady".into(), url: Some("http://shady.game".into()), icon: None, kind: OriginKind::Game };
    let f = Fx::new(req, Load::Unavailable);
    let v = f.view(&State::default());
    assert_eq!(&v.texts()[1], "Shady · shady.game · Main");
    assert!(v.contains_text("Could not preview this transaction. Approve only if you trust who is asking."));
    assert!(v.contains_text("This site is not using HTTPS."));
    assert_eq!(v.find_nav("Approve").unwrap().on, Some(Msg::Approve));
}

#[test]
fn a_message_shows_its_text_or_hex() {
    let f = Fx::new(message_request(), Load::Loading);
    let v = f.view(&State::default());
    let t = v.texts();
    assert_eq!(t[0], "Sign message");
    assert_eq!(t[1], "Rare Arcade · arcade.rare.market · Main");
    assert_eq!(t[2], "Message");
    assert_eq!(v.find_by_attr("data-testid", "message-text").unwrap().text.as_deref(), Some("Sign in to Rare Arcade\nNonce: 7f3a91c2"));
    assert_eq!(v.find_nav("Sign").unwrap().on, Some(Msg::Approve));
    let mut req = message_request();
    req.message = Some(vec![0, 1, 0xfe]);
    let f = Fx::new(req, Load::Loading);
    let v = f.view(&State::default());
    assert_eq!(v.texts()[2], "Message (hex)");
    let pre = v.find_by_attr("data-testid", "message-text").unwrap();
    assert_eq!(pre.text.as_deref(), Some("0001fe"));
    assert!(pre.class.contains("font-mono text-xs"));
}

#[test]
fn working_shows_the_device() {
    // passkey account, ceremony not started: plain Signing / Cancel, and B does nothing
    let mut f = Fx::ready(send_request(), send_review());
    f.flow = working();
    let v = f.view(&State::default());
    assert!(v.find_nav("Signing").is_some());
    assert_eq!(v.landing().as_deref(), Some("Cancel"));
    assert_eq!(sheet_b(&v), None);
    // the phone ceremony started: its panel
    f.passkey = Passkey::Qr { url: "FIDO:/0123".into() };
    let v = f.view(&State::default());
    assert!(v.contains_text("Sign with your phone"));
    assert_eq!(v.find_nav("Cancel").unwrap().on, Some(Msg::Panel(PanelMsg::Cancel)));
    // a Ledger account
    let mut f = Fx::ready(send_request(), send_review());
    f.req.account = f.accounts[1].address.clone();
    f.flow = working();
    f.ledger = LedgerPrompt { message: "Confirm on your Ledger".into(), device: Some("Nano X".into()), error: None };
    let v = f.view(&State::default());
    assert!(v.contains_text("Confirm on your Ledger"));
    assert!(v.contains_text("Nano X"));
    // screenshots force a kind
    let mut f = Fx::ready(send_request(), send_review());
    f.flow = working();
    f.force = Some(Hw::Keystone);
    assert!(f.view(&State::default()).contains_text("Connecting to your Keystone"));
}

#[test]
fn done_and_errors() {
    let mut f = Fx::ready(send_request(), send_review());
    f.flow = Some(Flow { phase: Phase::Done, error: None, final_: false, result: Some(SignResult { digest: Some("8kQ1digest".into()), dry_run: false }) });
    let mut st = State::default();
    let v = f.view(&st);
    assert_eq!(v.texts(), ["Sent", "Your transaction went through.", "You send", "0.1 SUI", "Transaction ID", "8kQ1digest", "Copy ID", "Done"]);
    assert_eq!(sheet_b(&v), Some(Msg::Done));
    assert_eq!(update(&mut st, false, &Msg::CopyId("8kQ1digest".into())), Some(Effect::Copy("8kQ1digest".into())));
    assert!(f.view(&st).find_nav("Copied").is_some());

    f.flow = Some(Flow { phase: Phase::Error, error: Some("The network rejected this transaction.".into()), final_: true, result: None });
    let v = f.view(&State::default());
    assert!(v.contains_text("The network rejected this transaction."));
    assert_eq!(v.landing().as_deref(), Some("Done"));
    assert_eq!(sheet_b(&v), Some(Msg::Done));

    f.flow = Some(Flow { phase: Phase::Error, error: Some("Could not sign. Try again.".into()), final_: false, result: None });
    let v = f.view(&State::default());
    assert!(v.contains_text("Could not sign. Try again."));
    assert_eq!(v.find_nav("Retry").unwrap().on, Some(Msg::Approve));
}

#[test]
fn a_signature_only_says_signed() {
    let mut req = send_request();
    req.execute = false;
    req.title = None;
    let mut f = Fx::ready(req, send_review());
    assert_eq!(f.view(&State::default()).texts()[0], "Sign transaction");
    f.flow = Some(Flow { phase: Phase::Done, error: None, final_: false, result: Some(SignResult { digest: None, dry_run: true }) });
    let v = f.view(&State::default());
    assert_eq!(v.texts()[0], "Signed");
    assert!(v.contains_text("Your signature is ready."));
    assert!(v.contains_text("Test run. Nothing was sent to the network."));
    assert!(v.find_nav("Copy ID").is_none());
}

#[test]
fn host_flow() {
    let req = send_request();
    let mut h = Host::default();
    assert_eq!(h.current(Some(&req)).map(|r| r.id), Some(1));
    h.start(&req);
    assert_eq!(h.flow().unwrap().phase, Phase::Working);
    h.finish(Outcome::Signed { result: SignResult { digest: Some("d".into()), dry_run: false }, failed_on_chain: false });
    assert_eq!(h.flow().unwrap().phase, Phase::Done);
    h.done();
    assert!(h.flow().is_none());

    h.start(&req);
    h.finish(Outcome::Signed { result: SignResult::default(), failed_on_chain: true });
    let f = h.flow().unwrap();
    assert!(f.final_ && f.phase == Phase::Error);

    // games and the Marketplace show their own result
    h.start(&message_request());
    h.finish(Outcome::Signed { result: SignResult::default(), failed_on_chain: false });
    assert!(h.flow().is_none());

    h.start(&req);
    h.finish(Outcome::Threw { message: None, still_queued: true });
    assert_eq!(h.flow().unwrap().error.as_deref(), Some("Could not sign. Try again."));
    h.start(&req);
    h.finish(Outcome::Threw { message: Some("x".into()), still_queued: false });
    assert!(h.flow().is_none());
    h.start(&req);
    h.finish(Outcome::NotUnlocked);
    assert!(h.flow().is_none());
}
