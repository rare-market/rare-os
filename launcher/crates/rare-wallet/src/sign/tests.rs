//! The signing pipeline (sign.ts) against a fake node and a mocked sponsor API: the queue, Cancel,
//! Approve for local keys, execution failures, dry runs, sponsored runs, and the game budget path
//! of signAndExecuteForGame (sponsor.game.test.ts).

use super::*;
use crate::keystore::{MemoryStore, PinOptions};
use crate::vault::{Kdf, Vault};
use serde_json::{json, Value};
use std::sync::atomic::AtomicU64;
use sui_sdk_types::{GasPayment, Transaction, TransactionExpiration, TransactionKind};

const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../testdata/ts-vectors.json")).unwrap()
}
fn game_bytes() -> Vec<u8> {
    B64.decode(vectors()["sponsor"]["gameBytes"].as_str().unwrap()).unwrap()
}

#[derive(Default)]
struct FakeChain {
    executes: Mutex<Vec<(Vec<u8>, Vec<String>)>>,
    fail: bool,
    builds: AtomicU64,
}

impl Chain for FakeChain {
    fn build(&self, _tx: &Tx, _sender: &str, _opts: &rare_sui::tx::BuildOpts) -> Result<Vec<u8>, String> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        Ok(game_bytes())
    }
    fn build_kind(&self, _tx: &Tx, _sender: &str, _sponsor: Option<&str>) -> Result<String, String> {
        crate::sponsor::kind_of_bytes(&game_bytes()).map_err(|f| f.message)
    }
    fn execute(&self, bytes: &[u8], signatures: &[String]) -> Result<Executed, String> {
        self.executes.lock().unwrap().push((bytes.to_vec(), signatures.to_vec()));
        let data: Transaction = bcs::from_bytes(bytes).unwrap();
        Ok(Executed { digest: data.digest().to_string(), success: !self.fail, effects: Some("ZWZm".into()) })
    }
    fn dry_run(&self, _bytes: &[u8]) -> Result<bool, String> {
        Ok(true)
    }
    fn sui_balance(&self, _owner: &str) -> Result<u64, String> {
        Ok(0)
    }
}

type Post = Box<dyn Fn(&str, &Value) -> Result<(u16, Value), String> + Send + Sync>;
struct Api {
    status: Option<Value>,
    post: Post,
    now: AtomicU64,
    posts: Mutex<Vec<(String, Value)>>,
    gets: Mutex<Vec<String>>,
}
impl SponsorApi for Arc<Api> {
    fn get(&self, path: &str) -> Result<Value, String> {
        self.gets.lock().unwrap().push(path.into());
        self.status.clone().map(|s| json!({ "data": s })).ok_or_else(|| "down".into())
    }
    fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), String> {
        self.posts.lock().unwrap().push((path.into(), body.clone()));
        (self.post)(path, body)
    }
    fn now_ms(&self) -> u64 {
        self.now.fetch_add(1000, Ordering::SeqCst)
    }
}

fn sponsor_addr() -> String {
    format!("0x{:064x}", 0x5905)
}

fn status(game: bool) -> Value {
    let mut s = json!({
        "enabled": true, "funded": true, "address": sponsor_addr(), "remainingTodayMist": "1000000000",
        "categories": { "rescue": true, "trophy": true, "trade": true, "walrus": false }, "maxUserSuiMist": "50000000",
    });
    if game {
        s["game"] = json!({ "slug": "act", "covered": true, "availableMist": "2000000000", "paused": false, "you": { "todayTx": 0, "todayMist": "0", "eligible": true } });
    }
    s
}

/// The server's answer: the kind with the sponsor paying gas.
fn grant(body: &Value) -> Result<(u16, Value), String> {
    let kind: TransactionKind = bcs::from_bytes(&B64.decode(body["kind"].as_str().unwrap()).unwrap()).unwrap();
    let ref_ = sui_sdk_types::ObjectReference::new(format!("0x{:064x}", 0x99).parse().unwrap(), 7, sui_sdk_types::Digest::new([3; 32]));
    let tx = Transaction {
        kind,
        sender: body["sender"].as_str().unwrap().parse().unwrap(),
        gas_payment: GasPayment { objects: vec![ref_], owner: sponsor_addr().parse().unwrap(), price: 1000, budget: 5_000_000 },
        expiration: TransactionExpiration::None,
    };
    Ok((200, json!({ "data": { "bytes": B64.encode(bcs::to_bytes(&tx).unwrap()), "sponsorSignature": "c3Bvbg==", "digest": "D1", "sponsor": sponsor_addr(), "category": body["category"], "gasBudget": "5000000", "estimatedGasMist": "1", "expiresAt": 9_999_999_999_999u64 } })))
}

struct T {
    _dir: tempfile::TempDir,
    s: Signing<MemoryStore, FakeChain, Arc<Api>>,
    api: Arc<Api>,
    me: String,
}

fn mk(chain: FakeChain, status: Option<Value>, post: impl Fn(&str, &Value) -> Result<(u16, Value), String> + Send + Sync + 'static) -> T {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::new(dir.path().to_path_buf(), false, Kdf { m: 256, t: 1, p: 1 }));
    let ks = Arc::new(Keystore::new(MemoryStore::default(), vault));
    let me = ks.add_phrase(Some(PHRASE), None, Some(PinOptions { secret: "123456", kind: "pin" }), None).unwrap().address;
    let api = Arc::new(Api { status, post: Box::new(post), now: AtomicU64::new(1_000_000), posts: Mutex::default(), gets: Mutex::default() });
    T { s: Signing::new(ks, chain, Arc::new(Sponsor::new(api.clone()))), api, me, _dir: dir }
}

fn opts(title: &str) -> SignOptions {
    SignOptions { title: Some(title.into()), origin: Some(SignOrigin::new("Rare Marketplace", OriginKind::Market)), ..Default::default() }
}

#[test]
fn queues_in_order_and_cancel_rejects_with_the_reason() {
    let t = mk(FakeChain::default(), None, |_, _| Err("x".into()));
    let a = t.s.sign_and_execute(TxInput::Bytes(game_bytes()), opts("Buy")).unwrap();
    let b = t.s.sign_personal_message(b"hi".to_vec(), SignOptions::default()).unwrap();
    let v = t.s.version();
    let reqs = t.s.requests();
    assert_eq!(reqs.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    assert_eq!(reqs[0].account, t.me);
    assert_eq!((reqs[0].title.as_deref(), reqs[0].origin.name.as_str(), reqs[0].execute), (Some("Buy"), "Rare Marketplace", true));
    assert_eq!(reqs[1].origin, SignOrigin::wallet());
    t.s.reject(a.id, None);
    assert!(t.s.version() > v);
    assert_eq!(a.wait().unwrap_err(), SignError::Rejected(DECLINED.into()));
    t.s.reject(b.id, Some("Not now"));
    assert_eq!(b.wait().unwrap_err(), SignError::Rejected("Not now".into()));
    assert!(t.s.requests().is_empty());
    assert_eq!(t.s.approve(a_id_gone(), None, &mut NoDevices).unwrap_err(), SignError::Gone);
}

fn a_id_gone() -> u64 {
    999
}

#[test]
fn approve_signs_the_exact_bytes_and_executes() {
    let t = mk(FakeChain::default(), None, |_, _| Err("x".into()));
    let ticket = t.s.sign_and_execute(TxInput::Bytes(game_bytes()), opts("Buy")).unwrap();
    let r = t.s.approve(ticket.id, None, &mut NoDevices).unwrap();
    let kp = crate::keys::Keypair::from_mnemonic(PHRASE, 0).unwrap();
    assert_eq!(r.signature, kp.sign_transaction(&game_bytes()));
    assert_eq!(r.bytes, B64.encode(game_bytes()));
    assert_eq!(r.status.as_deref(), Some("success"));
    assert_eq!(r.effects.as_deref(), Some("ZWZm"));
    assert_eq!(ticket.wait().unwrap(), r);
    let ex = t.s.chain.executes.lock().unwrap();
    assert_eq!((ex[0].0.clone(), ex[0].1.clone()), (game_bytes(), vec![r.signature.clone()]));
}

#[test]
fn a_ptb_is_built_at_approve_with_the_account_as_sender() {
    let t = mk(FakeChain::default(), None, |_, _| Err("x".into()));
    let ticket = t.s.sign_and_execute(TxInput::Ptb(Tx::new()), SignOptions::default()).unwrap();
    t.s.approve(ticket.id, None, &mut NoDevices).unwrap();
    assert_eq!(t.s.chain.builds.load(Ordering::SeqCst), 1);
    // bytes the review already built are signed as they are
    let ticket = t.s.sign_and_execute(TxInput::Ptb(Tx::new()), SignOptions::default()).unwrap();
    t.s.approve(ticket.id, Some(game_bytes()), &mut NoDevices).unwrap();
    assert_eq!(t.s.chain.builds.load(Ordering::SeqCst), 1);
}

#[test]
fn a_rejected_transaction_finishes_the_request_with_a_failure() {
    let t = mk(FakeChain { fail: true, ..Default::default() }, None, |_, _| Err("x".into()));
    let ticket = t.s.sign_and_execute(TxInput::Bytes(game_bytes()), SignOptions::default()).unwrap();
    let r = t.s.approve(ticket.id, None, &mut NoDevices).unwrap();
    assert_eq!(r.status.as_deref(), Some("failure"));
    assert_eq!(ticket.wait().unwrap_err(), SignError::Failed("The network rejected this transaction".into()));
    assert!(t.s.requests().is_empty());
}

#[test]
fn locked_vault_and_device_errors_keep_the_request_open() {
    let t = mk(FakeChain::default(), None, |_, _| Err("x".into()));
    let ticket = t.s.sign_and_execute(TxInput::Bytes(game_bytes()), SignOptions::default()).unwrap();
    t.s.keystore.lock();
    assert_eq!(t.s.approve(ticket.id, None, &mut NoDevices).unwrap_err(), SignError::Keystore(KeystoreError::Locked));
    assert_eq!(t.s.requests().len(), 1);
    t.s.keystore.unlock("123456").unwrap();
    t.s.approve(ticket.id, None, &mut NoDevices).unwrap();

    // a Ledger account signs through the devices; a device failure leaves the request open
    let pk = crate::keys::Keypair::from_mnemonic(PHRASE, 4).unwrap().public_key_b64();
    let l = t.s.keystore.add_ledger(&pk, "m/44'/784'/4'/0'/0'", None, None).unwrap();
    struct Dev(u32);
    impl ExternalSigner for Dev {
        fn sign(&mut self, account: &RareAccount, public_key: &str, transaction: bool, bytes: &[u8]) -> Result<String, SignError> {
            self.0 += 1;
            if self.0 == 1 {
                return Err(SignError::Device { code: "rejected".into(), message: "Rejected on your Ledger.".into() });
            }
            assert!(transaction);
            assert_eq!(account.kind, AccountKind::Ledger);
            assert!(!public_key.is_empty());
            Ok(crate::keys::Keypair::from_mnemonic(PHRASE, 4).unwrap().sign_transaction(bytes))
        }
    }
    let mut dev = Dev(0);
    let ticket = t.s.sign_and_execute(TxInput::Bytes(game_bytes()), SignOptions { account: Some(l.address.clone()), ..Default::default() }).unwrap();
    assert!(matches!(t.s.approve(ticket.id, None, &mut dev).unwrap_err(), SignError::Device { .. }));
    assert_eq!(t.s.requests().len(), 1);
    assert!(t.s.approve(ticket.id, None, &mut dev).is_ok());
}

#[test]
fn messages_and_sign_only_and_dry_runs() {
    let t = mk(FakeChain::default(), None, |_, _| Err("x".into()));
    let m = t.s.sign_personal_message(b"Sign in to Rare".to_vec(), SignOptions::default()).unwrap();
    let r = t.s.approve(m.id, None, &mut NoDevices).unwrap();
    assert_eq!(r.bytes, B64.encode(b"Sign in to Rare"));
    assert_eq!(r.signature, crate::keys::Keypair::from_mnemonic(PHRASE, 0).unwrap().sign_personal_message(b"Sign in to Rare"));
    let s = t.s.sign_transaction(TxInput::Bytes(game_bytes()), SignOptions::default()).unwrap();
    let r = t.s.approve(s.id, None, &mut NoDevices).unwrap();
    assert_eq!((r.digest.clone(), r.status.clone()), (None, None));
    let d = t.s.sign_and_execute(TxInput::Bytes(game_bytes()), SignOptions { dry_run: true, ..Default::default() }).unwrap();
    let r = t.s.approve(d.id, None, &mut NoDevices).unwrap();
    assert!(r.dry_run);
    assert_eq!(r.status.as_deref(), Some("success"));
    let data: Transaction = bcs::from_bytes(&game_bytes()).unwrap();
    assert_eq!(r.digest, Some(data.digest().to_string()));
    assert!(t.s.chain.executes.lock().unwrap().is_empty());
}

#[test]
fn watch_accounts_cannot_be_the_default_signer() {
    let dir = tempfile::tempdir().unwrap();
    let ks = Arc::new(Keystore::new(MemoryStore::default(), Arc::new(Vault::new(dir.path().to_path_buf(), false, Kdf { m: 256, t: 1, p: 1 }))));
    let api = Arc::new(Api { status: None, post: Box::new(|_, _| Err("x".into())), now: AtomicU64::new(0), posts: Mutex::default(), gets: Mutex::default() });
    let s = Signing::new(ks.clone(), FakeChain::default(), Arc::new(Sponsor::new(api)));
    assert_eq!(s.sign_personal_message(vec![], SignOptions::default()).unwrap_err(), SignError::Chain("Add a Rare Wallet account first".into()));
    ks.add_watch(&format!("0x{}", "ab".repeat(32)), None).unwrap();
    assert!(s.sign_personal_message(vec![], SignOptions::default()).unwrap_err().to_string().contains("watch only"));
}

#[test]
fn sponsored_approve_signs_the_sponsored_bytes_and_rare_market_submits() {
    let t = mk(FakeChain::default(), Some(status(false)), |p, b| if p == "/v1/sponsor" { grant(b) } else { Ok((200, json!({ "data": { "digest": b["digest"], "ok": true, "gasUsedMist": "1" } }))) });
    let kind = crate::sponsor::kind_of_bytes(&game_bytes()).unwrap();
    let s = t.s.sponsor.sponsor_kind(&kind, &t.me, &Category::Trophy).unwrap();
    let ticket = t.s.sign_and_execute_sponsored(kind, s.clone(), Category::Trophy, SignOptions::default(), None).unwrap();
    let req = &t.s.requests()[0];
    assert_eq!(req.tx, Some(TxInput::Bytes(B64.decode(&s.bytes).unwrap())));
    assert_eq!(req.sponsored.as_ref().unwrap().sponsor, sponsor_addr());
    let r = t.s.approve(ticket.id, None, &mut NoDevices).unwrap();
    assert_eq!(r.bytes, s.bytes);
    assert_eq!(r.signature, crate::keys::Keypair::from_mnemonic(PHRASE, 0).unwrap().sign_transaction(&B64.decode(&s.bytes).unwrap()));
    assert_eq!(r.digest.as_deref(), Some("D1"));
    assert!(t.s.chain.executes.lock().unwrap().is_empty());
    let execute = t.api.posts.lock().unwrap().iter().find(|p| p.0 == "/v1/sponsor/execute").cloned().unwrap();
    assert_eq!(execute.1["signature"], r.signature);
}

#[test]
fn sponsored_execution_failure_finishes_the_request_and_other_noes_keep_it_open() {
    let t = mk(FakeChain::default(), Some(status(false)), |p, b| if p == "/v1/sponsor" { grant(b) } else { Ok((200, json!({ "data": { "ok": false, "error": "MoveAbort" } }))) });
    let kind = crate::sponsor::kind_of_bytes(&game_bytes()).unwrap();
    let s = t.s.sponsor.sponsor_kind(&kind, &t.me, &Category::Trophy).unwrap();
    let ticket = t.s.sign_and_execute_sponsored(kind.clone(), s.clone(), Category::Trophy, SignOptions::default(), None).unwrap();
    assert_eq!(t.s.approve(ticket.id, None, &mut NoDevices).unwrap().status.as_deref(), Some("failure"));
    assert_eq!(ticket.wait().unwrap_err(), SignError::Failed("MoveAbort".into()));

    let t = mk(FakeChain::default(), Some(status(false)), |p, b| if p == "/v1/sponsor" { grant(b) } else { Ok((503, json!({ "error": { "code": "sponsor_busy", "message": "busy" } }))) });
    let s = t.s.sponsor.sponsor_kind(&kind, &t.me, &Category::Trophy).unwrap();
    let ticket = t.s.sign_and_execute_sponsored(kind, s, Category::Trophy, SignOptions::default(), None).unwrap();
    assert!(matches!(t.s.approve(ticket.id, None, &mut NoDevices).unwrap_err(), SignError::Sponsor(f) if f.reason == FailReason::Busy));
    assert_eq!(t.s.requests().len(), 1);
}

// ---------------------------------------------------------------- sponsor.game.test.ts: who pays

#[test]
fn the_games_budget_pays_the_review_gets_the_sponsored_bytes_and_the_game_as_payer() {
    let t = mk(FakeChain::default(), Some(status(true)), |_, b| grant(b));
    t.s.sign_and_execute_for_game(TxInput::Ptb(Tx::new()), Some("act"), "ACT", SignOptions::default(), None).unwrap();
    let req = t.s.requests().pop().unwrap();
    let sp = req.sponsored.unwrap();
    assert_eq!((sp.category.as_str(), sp.payer.as_deref(), sp.sponsor.as_str()), ("game:act".to_string(), Some("ACT"), sponsor_addr().as_str()));
    assert!(matches!(req.tx, Some(TxInput::Bytes(_))));
    // a web game's full bytes go by their kind
    t.s.sign_and_execute_for_game(TxInput::Bytes(game_bytes()), Some("act"), "ACT", SignOptions::default(), None).unwrap();
    let kind = crate::sponsor::kind_of_bytes(&game_bytes()).unwrap();
    assert_eq!(t.api.posts.lock().unwrap().last().unwrap().1["kind"], kind);
}

#[test]
fn budget_used_up_the_player_pays_and_the_review_says_why() {
    let t = mk(FakeChain::default(), Some(status(true)), |_, _| Ok((429, json!({ "error": { "code": "budget_exhausted", "message": "x" } }))));
    t.s.sign_and_execute_for_game(TxInput::Ptb(Tx::new()), Some("act"), "ACT", SignOptions::default(), None).unwrap();
    let req = t.s.requests().pop().unwrap();
    assert!(req.sponsored.is_none());
    assert_eq!(req.fee_note.as_deref(), Some("ACT's gas budget is used up, so you pay this fee."));
}

#[test]
fn refused_runs_the_fallback_with_no_note_and_no_slug_never_asks() {
    let t = mk(FakeChain::default(), Some(status(true)), |_, _| Ok((403, json!({ "error": { "code": "not_sponsored", "message": "no" } }))));
    let fell = Mutex::new(None);
    let fallback = |o: SignOptions| {
        *fell.lock().unwrap() = Some(o.clone());
        t.s.sign_and_execute(TxInput::Bytes(game_bytes()), o)
    };
    t.s.sign_and_execute_for_game(TxInput::Ptb(Tx::new()), Some("act"), "ACT", SignOptions::default(), Some(&fallback)).unwrap();
    let o = fell.lock().unwrap().clone().unwrap();
    assert_eq!((o.account, o.fee_note), (Some(t.me.clone()), None));

    let t = mk(FakeChain::default(), Some(status(true)), |_, b| grant(b));
    t.s.sign_and_execute_for_game(TxInput::Ptb(Tx::new()), None, "Some game", SignOptions::default(), None).unwrap();
    assert!(t.api.gets.lock().unwrap().is_empty());
    assert!(t.s.requests()[0].sponsored.is_none());
}

#[test]
fn cover_rare_pays_when_the_wallet_is_short_and_the_category_is_offered() {
    let t = mk(FakeChain::default(), Some(status(false)), |_, b| grant(b));
    t.s.sign_and_execute_with_cover(Tx::new(), Category::Trophy, SignOptions::default()).unwrap();
    let req = t.s.requests().pop().unwrap();
    assert_eq!(req.sponsored.unwrap().category, Category::Trophy);
    // not offered (walrus is off): the wallet pays
    t.s.sign_and_execute_with_cover(Tx::new(), Category::Walrus, SignOptions::default()).unwrap();
    let req = t.s.requests().pop().unwrap();
    assert!(req.sponsored.is_none());
    assert_eq!(req.tx, Some(TxInput::Ptb(Tx::new())));
}

// ---------------------------------------------------------------- secret.ts deriveSecret

#[test]
fn derive_secret_per_account_kind() {
    use crate::passkey_account::{soft::SoftAuthenticator, Via};
    use crate::secret::derive::{derive_secret, DeriveError};
    let t = Arc::new(mk(FakeChain::default(), None, |_, _| Err("x".into())));
    let mut soft = SoftAuthenticator::new();
    // a phrase account: HKDF over its key (pinned in secret.test.ts)
    let s = derive_secret(&t.s, &mut soft, Via::Usb, "cloud-saves", None, None).unwrap();
    assert_eq!(crate::keys::hex(&s[..]), "22d6b803437a60cca9d5566218a1c8464c29da5859ea69fb6eac7895976a076c");
    assert!(matches!(derive_secret(&t.s, &mut soft, Via::Usb, "", None, None), Err(DeriveError::Unavailable(_))));
    let w = t.s.keystore.add_watch(&format!("0x{}", "ab".repeat(32)), None).unwrap();
    assert!(derive_secret(&t.s, &mut soft, Via::Usb, "x", Some(&w.address), None).unwrap_err().to_string().contains("watch only"));

    // a Ledger holding the same key: the device signs the fixed message through the sheet
    let pk = crate::keys::Keypair::from_mnemonic(PHRASE, 0).unwrap().public_key_b64();
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::new(dir.path().to_path_buf(), false, Kdf { m: 256, t: 1, p: 1 }));
    let ks = Arc::new(Keystore::new(MemoryStore::default(), vault));
    let l = ks.add_ledger(&pk, "m/44'/784'/0'/0'/0'", None, None).unwrap();
    let api = Arc::new(Api { status: None, post: Box::new(|_, _| Err("x".into())), now: AtomicU64::new(0), posts: Mutex::default(), gets: Mutex::default() });
    let s = Arc::new(Signing::new(ks, FakeChain::default(), Arc::new(Sponsor::new(api))));
    let sheet = {
        let s = s.clone();
        std::thread::spawn(move || {
            struct Dev;
            impl ExternalSigner for Dev {
                fn sign(&mut self, _: &RareAccount, _: &str, transaction: bool, bytes: &[u8]) -> Result<String, SignError> {
                    assert!(!transaction);
                    Ok(crate::keys::Keypair::from_mnemonic(PHRASE, 0).unwrap().sign_personal_message(bytes))
                }
            }
            loop {
                if let Some(r) = s.requests().first().cloned() {
                    assert_eq!(r.title.as_deref(), Some("Unlock cloud saves"));
                    s.approve(r.id, None, &mut Dev).unwrap();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        })
    };
    let out = derive_secret(&s, &mut soft, Via::Usb, "cloud-saves", Some(&l.address), Some("Unlock cloud saves")).unwrap();
    sheet.join().unwrap();
    // pinned in secret.test.ts (V.ledger)
    assert_eq!(crate::keys::hex(&out[..]), "e5c10994260a631eea86a4252e3984ac69de5c45e9c6c486d4f1db687b3e1bf3");
}
