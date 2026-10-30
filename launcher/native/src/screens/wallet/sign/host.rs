//! The review sheet in front of every signature (ReviewHost.tsx + UnlockSheet.tsx), over the whole
//! launcher: it shows the oldest request in rare-wallet's signing queue (Marketplace, Trade,
//! Friends, the wallet's own sends, games), simulates it, and on Approve asks for the PIN when the
//! account needs it, then signs through the account's key, Ledger, Keystone or passkey. Cancel
//! answers the caller "You declined the request".
//!
//! The app mounts one `ReviewHost` for its lifetime (scope `SCOPE`, above sheets, under the
//! keyboard); while `active()` it takes every press.

use super::review::{self, Flow, Hw, Load, Outcome, Phase};
use super::locked;
use super::pin_tasks;
use super::unlock::{self, UnlockError, VaultKind};
use crate::act::Act;
use crate::kit::{s, Ui};
use crate::page::{hash, Acts, Btn, Fx, Page};
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::render::{build, Built};
use crate::screens::wallet::service::{self, poll, Wallet};
use crate::screens::wallet::types::RareAccount;
use crate::state::{State, Tone};
use rare_render::focus::Focusable;
use rare_render::view::El;
use rare_wallet::sign::{SignError, SignKind, TxInput};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

/// Above every sheet (market and store sheets use 6 and 7), under the keyboard.
pub const SCOPE: u32 = 20;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Shots only (`RR_FIXTURE` with `RR_REVIEW=send|swap|failed|message|sponsored`): the review sheet over
/// the page with the web sheet's own fixture data.
fn fixture_review() -> Option<(review::SignRequest, Load)> {
    use super::fixtures as f;
    std::env::var("RR_FIXTURE").ok()?;
    let (req, rv) = match std::env::var("RR_REVIEW").ok()?.as_str() {
        "send" => (f::send_request(), Some(f::send_review())),
        "swap" => (f::send_request(), Some(f::swap_review())),
        "failed" => (f::send_request(), Some(f::failed_review())),
        "sponsored" => (f::send_request(), Some(f::sponsored_review())),
        "message" => (f::message_request(), None),
        _ => return None,
    };
    Some((req, rv.map(|r| Load::Ready(Box::new(r))).unwrap_or(Load::Unavailable)))
}

/// True while the sheet is up: the app routes every press to it.
pub fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

const ASK_PASS: &str = "wallet:review:passphrase";
const ASK_SKPIN: &str = "wallet:review:security-key-pin";

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Sheet(review::Msg),
    Unlock(unlock::Msg),
    /// The parent's PIN sheet (parental controls).
    Parental(crate::features::parental_sheet::Msg),
    /// A PIN account locked after 5 wrong PINs.
    Locked(locked::Msg),
    /// Change PIN, Show recovery words.
    Task(pin_tasks::Msg),
}

type Rx<T> = Option<Receiver<Result<T, String>>>;

#[derive(Default)]
pub struct ReviewHost {
    /// The request on screen (id) and the ones already answered here.
    shown: Option<rare_wallet::sign::SignRequest>,
    req: Option<review::SignRequest>,
    load: Option<Load>,
    sheet: review::State,
    host: review::Host,
    unlock: Option<(unlock::State, VaultKind)>,
    /// An unlock a page asked for (`service::ask_unlock`): its reason and who waits.
    standalone: Option<(String, std::sync::mpsc::Sender<bool>)>,
    accounts: Vec<RareAccount>,
    rx_review: Rx<Result<rare_wallet::review::Review, String>>,
    rx_sign: Rx<Result<rare_wallet::sign::SignResult, SignError>>,
    rx_unlock: Rx<Result<(), rare_wallet::keystore::KeystoreError>>,
    built: Built<Msg>,
    ver: u64,
    seen: u64,
    copied_until: f64,
    /// The parent's PIN sheet, over everything else here (`features::parental`).
    parental: Option<crate::features::parental_sheet::Sheet>,
    /// Approve waiting for the parent's PIN.
    rx_parental: Option<Receiver<bool>>,
    /// What the PIN sheet is for: opening the vault, or the send PIN of a big send.
    pin_for: PinFor,
    /// The send PIN was entered for the request on screen.
    send_pin_ok: bool,
    /// The person picked "Use my phone instead": the passkey signs on the phone (QR).
    use_phone: bool,
    /// How the account on screen approves (the line under the buttons).
    method: Option<String>,
    /// A PIN account's PIN opened its device key for the request on screen.
    pin_ok: bool,
    /// A PIN account locked after 5 wrong PINs: the unlock sheet.
    locked: Option<locked::State>,
    rx_status: Rx<bool>,
    rx_phone_unlock: Rx<Result<String, String>>,
    rx_reset: Rx<Result<(), String>>,
    /// Settings > Accounts: Change PIN or Show recovery words, and its work.
    task: Option<pin_tasks::State>,
    task_pin: String,
    rx_task_check: Rx<Result<Option<Vec<String>>, (bool, String)>>,
    rx_task_backup: Rx<Result<String, String>>,
    rx_task_save: Rx<Result<(), String>>,
    /// When the sheet reached its done state ("Your signature is ready."): a message signature
    /// closes by itself 1.5 s later, and focus goes to Done.
    done_at: Option<f64>,
    /// The Done button's tag in the last build, and whether focus was put on it.
    done_tag: Option<u32>,
    done_focused: bool,
}

/// A signed message's done state closes by itself after this long.
const DONE_CLOSE_S: f64 = 1.5;

/// Shots only (`RR_FIXTURE` with `RR_PIN_TASK=change|forgot|newpin|wrong|words|words-pin|shown`).
fn fixture_task() -> Option<pin_tasks::State> {
    std::env::var("RR_FIXTURE").ok()?;
    let k = std::env::var("RR_PIN_TASK").ok()?;
    let mut t = pin_tasks::State::new(if k.starts_with("words") || k == "shown" { pin_tasks::Task::Words } else { pin_tasks::Task::Change });
    match k.as_str() {
        "forgot" => t.phase = pin_tasks::Phase::Backup(locked::State::default()),
        "newpin" => t.phase = pin_tasks::Phase::New { pin: Default::default(), saving: false, error: None },
        "wrong" => pin_tasks::checked(&mut t, String::new(), Err((false, rare_wallet::pin_guard::rules::wrong_text(3)))),
        "words-pin" => t.phase = pin_tasks::Phase::Current(Default::default()),
        "shown" => t.phase = pin_tasks::Phase::Shown("orbit lumber velvet canyon gesture salmon frown island crater mention pottery hamster".split(' ').map(str::to_string).collect()),
        _ => {}
    }
    Some(t)
}

/// The first PIN account here (its passkeys unlock the device for Change PIN).
fn first_pin_account(w: &Wallet) -> Option<rare_wallet::keystore::RareAccount> {
    w.keystore.pin_accounts().ok()?.into_iter().next()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PinFor {
    #[default]
    Vault,
    Send,
    /// a PIN account's device key, checked by the PIN guard
    Device,
}

/// Which backups a PIN account has: (phone, security key).
fn backups_of(a: Option<&rare_wallet::keystore::RareAccount>) -> (bool, bool) {
    let Some(info) = a.and_then(|a| a.passkey.as_ref()) else { return (true, false) };
    let all = info.passkeys();
    let key = all.iter().any(|m| m.authenticator.as_deref() == Some("security-key"));
    let phone = all.iter().any(|m| m.authenticator.as_deref() != Some("security-key"));
    (phone, key)
}

/// The request's account in the keystore.
fn account_of(w: &Wallet, address: &str) -> Option<rare_wallet::keystore::RareAccount> {
    w.keystore.meta().ok()?.accounts.into_iter().find(|a| a.address == address)
}

/// What a reviewed transaction sends out, in US dollars. None: unknown (no review, or a coin
/// without a price), which counts as over any line.
fn usd_out(load: Option<&Load>) -> Option<f64> {
    let Some(Load::Ready(r)) = load else { return None };
    let mut total = 0.0;
    for f in &r.send {
        if f.amount == 0 {
            continue;
        }
        let c = r.coins.get(&crate::screens::wallet::fmt::norm_type(&f.coin_type))?;
        total += (f.amount.unsigned_abs() as f64) / 10f64.powi(c.decimals as i32) * c.price_usd?;
    }
    Some(total)
}

/// The keyboard for a secret (dots, never the text).
pub fn ask_secret(st: &mut State, key: &str, title: &str) {
    st.ask_text(key, title, "", None);
    if let Some(o) = st.osk.as_mut() {
        o.secret = true;
    }
}

fn now_ms() -> u64 {
    (rare_render::clock::secs() * 1000.0) as u64
}

// ---------------------------------------------------------------- rare-wallet to the sheet's types

pub fn origin_of(o: &rare_wallet::sign::SignOrigin) -> review::Origin {
    review::Origin {
        name: o.name.clone(),
        url: o.url.clone(),
        icon: o.icon.clone(),
        kind: match o.kind {
            rare_wallet::sign::OriginKind::Wallet => review::OriginKind::Wallet,
            rare_wallet::sign::OriginKind::Market => review::OriginKind::Market,
            rare_wallet::sign::OriginKind::Game => review::OriginKind::Game,
        },
    }
}

pub fn request_of(r: &rare_wallet::sign::SignRequest) -> review::SignRequest {
    review::SignRequest {
        id: r.id,
        kind: if r.kind == SignKind::Message { review::ReqKind::Message } else { review::ReqKind::Transaction },
        account: r.account.clone(),
        origin: origin_of(&r.origin),
        execute: r.execute,
        dry_run: r.dry_run,
        title: r.title.clone(),
        fee_note: r.fee_note.clone(),
        message: r.message.clone(),
    }
}

/// A build the node refused because the account lacks a coin ("address 0x.. does not have
/// sufficient balance of 0x..::wal::WAL: requested N available M"): plain words for the sheet, so it
/// says what is missing instead of offering Approve on a transaction that cannot run.
pub fn short_of_funds(e: &str) -> Option<String> {
    let lower = e.to_ascii_lowercase();
    if let Some(i) = e.find("sufficient balance of ") {
        let ty = e[i + "sufficient balance of ".len()..].split([':', ' ']).filter(|s| !s.is_empty()).collect::<Vec<_>>();
        // "0x..", "wal", "WAL" (split on ':' leaves the parts of 0x..::wal::WAL)
        let sym = ty.get(2).copied().filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("coins");
        return Some(format!("This account doesn't have enough {sym}. Add {sym} to it and try again."));
    }
    if lower.contains("insufficientgas") || lower.contains("insufficient gas") || lower.contains("no valid gas coins") || lower.contains("insufficientcoinbalance") {
        return Some("This account doesn't have enough SUI to pay the network fee. Add SUI to it and try again.".into());
    }
    None
}

/// The sheet's review for a transaction that could not be built: failed, nothing to sign.
pub fn cannot_build(reason: String) -> review::Review {
    review::Review { ok: false, reason: Some(reason), risks: vec![], rows: vec![], coins: Default::default(), send: vec![], others_receive: vec![], calls: vec![], shared: vec![], originals: Default::default(), bytes: None }
}

pub fn review_of(r: rare_wallet::review::Review) -> review::Review {
    use super::packages::{CoinFlow, MoveCallRef, OtherReceive};
    use rare_wallet::review::RowKind as K;
    review::Review {
        ok: r.ok,
        reason: r.insights.reason.clone(),
        risks: r.risks.iter().map(|x| super::site::Risk { id: x.id.clone(), text: x.text.clone(), danger: x.danger, requires_confirm: x.requires_confirm }).collect(),
        rows: r
            .rows
            .iter()
            .map(|x| review::ReviewRow {
                kind: match x.kind {
                    K::Send => review::RowKind::Send,
                    K::Receive => review::RowKind::Receive,
                    K::Fee => review::RowKind::Fee,
                },
                label: x.label.clone(),
                text: x.text.clone(),
                sub: x.sub.clone(),
                coin_type: x.coin_type.clone(),
                amount: x.amount,
                icon_url: x.icon_url.clone(),
                object_id: x.object_id.clone(),
                image: x.image.clone(),
            })
            .collect(),
        coins: r.coins.iter().map(|(k, c)| (k.clone(), review::ReviewCoin { symbol: c.symbol.clone(), decimals: c.decimals, icon_url: c.icon_url.clone(), price_usd: c.price_usd })).collect(),
        send: r.insights.send.iter().map(|f| CoinFlow { coin_type: f.coin_type.clone(), amount: f.amount }).collect(),
        others_receive: r.insights.others_receive.iter().map(|o| OtherReceive { address: o.address.clone(), coin_type: o.coin_type.clone(), amount: o.amount }).collect(),
        calls: r.insights.calls.iter().map(|c| MoveCallRef { package: c.package.clone(), module: c.module.clone(), function: c.function.clone() }).collect(),
        shared: r.insights.shared.iter().map(|x| review::SharedTouch { object_id: x.object_id.clone(), label: x.label.clone() }).collect(),
        originals: r.simulation.packages.clone().unwrap_or_default(),
        bytes: r.bytes,
    }
}

/// Simulates and explains a request (the wallet worker thread).
fn run_review(w: &'static Wallet, r: &rare_wallet::sign::SignRequest) -> Result<rare_wallet::review::Review, String> {
    use rare_wallet::review::{review_transaction, sim, ReviewInput, ReviewOptions};
    let input = match &r.tx {
        Some(TxInput::Bytes(b)) => ReviewInput::Bytes(b.clone()),
        Some(TxInput::Ptb(p)) => w.rt.block_on(sim::build_for_review(&w.sui, p, &r.account, &Default::default()))?,
        Some(TxInput::PtbGas(p, o)) => w.rt.block_on(sim::build_for_review(&w.sui, p, &r.account, o))?,
        None => return Err("No transaction".into()),
    };
    let trade = rare_sui::prices::PriceClient::default();
    let mut sponsors = w.signing.sponsor.known_sponsors();
    if let Some(sp) = &r.sponsored {
        sponsors.push(sp.sponsor.clone());
    }
    let opts = ReviewOptions { trade: Some(&trade), kiosk_source: Some(&w.sui), sponsors, paid_by: r.sponsored.as_ref().and_then(|s| s.payer.clone()), ..Default::default() };
    let sim = sim::GrpcSimulator::new(w.sui.clone());
    w.rt.block_on(review_transaction(&sim, input, &r.account, &opts))
}

fn unlock_error(e: rare_wallet::keystore::KeystoreError) -> UnlockError {
    use rare_wallet::keystore::KeystoreError as E;
    match e {
        E::Vault(v) => {
            let n = |k: &str| v.data.as_ref().and_then(|d| d.get(k)).and_then(|x| x.as_u64());
            match v.code.as_str() {
                "wrong" => UnlockError::Wrong { failures: n("failures").map(|x| x as u32), retry_after_ms: n("retryAfterMs").unwrap_or(0) },
                "throttled" => UnlockError::Throttled { retry_after_ms: n("retryAfterMs") },
                _ => UnlockError::Other(Some(v.message)),
            }
        }
        other => UnlockError::Other(Some(other.to_string())),
    }
}

fn hw_prompts(w: &Wallet) -> (review::Hw, super::panels::LedgerPrompt, super::panels::KeystonePrompt, super::panels::Passkey) {
    use super::panels::{KeystonePrompt, LedgerPrompt, Passkey};
    use rare_wallet::devices::DeviceStage;
    let mut l = LedgerPrompt::default();
    let mut k = KeystonePrompt::default();
    let mut hw = Hw::Passkey;
    match w.stage.lock().unwrap().clone() {
        Some(DeviceStage::Ledger { stage, device }) => {
            hw = Hw::Ledger;
            l.message = stage.text().into();
            l.device = device;
        }
        Some(DeviceStage::Keystone { stage, digest }) => {
            hw = Hw::Keystone;
            k.message = stage.text().into();
            k.digest_hex = digest;
        }
        _ => {}
    }
    let p = match w.passkey_event.lock().unwrap().clone() {
        None => Passkey::Idle,
        Some(rare_wallet::passkey::Event::Qr { url }) => Passkey::Qr { url },
        Some(rare_wallet::passkey::Event::Status { message, .. }) => Passkey::Status { message },
        Some(rare_wallet::passkey::Event::Pin { attempts_left }) => Passkey::Pin { attempts_left },
        Some(rare_wallet::passkey::Event::Error { message, .. }) => Passkey::Error { message },
    };
    (hw, l, k, p)
}

impl ReviewHost {
    /// The sheet shows its done state (signed or sent), nothing over it.
    fn done(&self) -> bool {
        self.parental.is_none() && self.unlock.is_none() && self.locked.is_none() && self.host.flow().is_some_and(|f| f.phase == Phase::Done)
    }

    fn visible(&self) -> bool {
        self.parental.is_some() || self.task.is_some() || fixture_task().is_some() || self.locked.is_some() || self.req.is_some() || self.standalone.is_some() || fixture_review().is_some()
    }

    fn vault_kind(w: &Wallet) -> VaultKind {
        match w.keystore.status().ok().and_then(|s| s.kind) {
            Some(k) if k == "passphrase" => VaultKind::Passphrase,
            _ => VaultKind::Pin,
        }
    }

    /// The unlock sheet finished (true: unlocked).
    fn unlocked(&mut self, st: &mut State, ok: bool) {
        self.unlock = None;
        if self.pin_for == PinFor::Send && ok {
            self.send_pin_ok = true;
        }
        if self.pin_for == PinFor::Device && ok {
            self.pin_ok = true;
        }
        if let Some((_, tx)) = self.standalone.take() {
            let _ = tx.send(ok);
            self.ver += 1;
            return;
        }
        if ok {
            self.approve(st);
        } else {
            self.host.finish(Outcome::NotUnlocked);
        }
    }

    fn reset_to(&mut self, r: Option<rare_wallet::sign::SignRequest>) {
        self.sheet = review::State::default();
        self.done_at = None;
        self.done_focused = false;
        self.unlock = None;
        self.rx_review = None;
        self.load = None;
        self.req = r.as_ref().map(request_of);
        self.pin_for = PinFor::Vault;
        self.send_pin_ok = false;
        self.use_phone = false;
        self.pin_ok = false;
        self.locked = None;
        self.rx_status = None;
        self.rx_phone_unlock = None;
        self.rx_reset = None;
        self.method = r.as_ref().map(|r| service::sign_method(&r.account)).filter(|m| !m.is_empty() && m != service::METHOD_SILENT);
        if let Some(r) = &r {
            self.accounts = service::wallet().map(|w| w.accounts().0).unwrap_or_default().into_iter().map(|a| RareAccount::from_wallet(&a)).collect();
            if r.kind == SignKind::Transaction {
                self.load = Some(Load::Loading);
                let req = r.clone();
                self.rx_review = Some(service::work(move |w| run_review(w, &req)));
            } else {
                self.load = Some(Load::Unavailable);
            }
        }
        self.shown = r;
        self.ver += 1;
    }

    fn approve(&mut self, st: &mut State) {
        let (Some(w), Some(r)) = (service::wallet(), self.shown.clone()) else { return };
        if self.rx_sign.is_some() {
            return;
        }
        // Parental controls: a transaction (every purchase, every signer) needs the parent's PIN.
        // Signing a message (signing in to a site or game) needs it too.
        if crate::features::parental::needs_pin(rare_render::clock::secs()) {
            if self.rx_parental.is_none() {
                let why = if r.kind == SignKind::Transaction { "Enter the PIN to approve this transaction." } else { "Enter the PIN to sign this message." };
                self.rx_parental = Some(crate::features::parental::ask("Parent's PIN", why, rare_render::clock::secs()));
            }
            self.ver += 1;
            return;
        }
        let acc = account_of(w, &r.account);
        let passkey = acc.as_ref().is_some_and(|a| a.kind == rare_wallet::keystore::AccountKind::Passkey);
        // A PIN account: the PIN for every signature (the guard counts wrong ones), unless its
        // backup signs this one. A lock shows the unlock sheet instead.
        if w.keystore.is_pin_account(&r.account) && !self.use_phone && !self.pin_ok {
            self.pin_for = PinFor::Device;
            self.unlock = Some((unlock::State::default(), VaultKind::Pin));
            self.rx_status = Some(service::work(|w| w.keystore.pin_device().and_then(|p| p.status().ok()).is_some_and(|s| s.locked)));
            self.ver += 1;
            return;
        }
        if !(passkey && self.use_phone) && w.keystore.needs_unlock(&r.account).unwrap_or(false) {
            let kind = Self::vault_kind(w);
            self.pin_for = PinFor::Vault;
            self.unlock = Some((unlock::State::default(), kind));
            if kind == VaultKind::Passphrase {
                ask_secret(st, ASK_PASS, "Passphrase");
            }
            self.ver += 1;
            return;
        }
        // The optional send PIN (Settings > Accounts): accounts that sign with A alone ask for it
        // before a transaction that sends more than its line.
        let device = acc.as_ref().is_some_and(rare_wallet::keystore::in_device_vault);
        if device && r.kind == SignKind::Transaction && !self.send_pin_ok && w.send_pin.needed_for(usd_out(self.load.as_ref())) {
            self.pin_for = PinFor::Send;
            self.unlock = Some((unlock::State::default(), VaultKind::Pin));
            self.ver += 1;
            return;
        }
        if passkey && self.use_phone {
            // a PIN account with only a security key signs with the key
            let (phone, _) = backups_of(acc.as_ref());
            *w.via.lock().unwrap() = if phone || !w.keystore.is_pin_account(&r.account) { rare_wallet::passkey_account::Via::Phone } else { rare_wallet::passkey_account::Via::Usb };
        }
        let built = match &self.load {
            Some(Load::Ready(rv)) => rv.bytes.clone(),
            _ => None,
        };
        if let Some(req) = &self.req {
            self.host.start(req);
        }
        let id = r.id;
        self.rx_sign = Some(service::work(move |w| {
            let mut d = w.devices();
            let res = w.signing.approve(id, built, &mut d);
            w.bump();
            res
        }));
        self.ver += 1;
    }

    fn cancel(&mut self) {
        if let (Some(w), Some(r)) = (service::wallet(), &self.shown) {
            w.keystore.disarm();
            w.cancel_devices();
            w.signing.reject(r.id, None);
        }
        self.host.cancel();
        self.rx_sign = None;
        self.reset_to(None);
    }

    fn markup(&self) -> Option<M<Msg>> {
        if let Some(p) = &self.parental {
            return Some(p.view(rare_render::clock::secs()).map(&Msg::Parental));
        }
        if let Some(t) = fixture_task() {
            return Some(pin_tasks::view(&t, true, true, &super::panels::Passkey::Idle, 0).map(&Msg::Task));
        }
        if let Some(t) = &self.task {
            let w = service::wallet();
            let passkey = w.map(|w| hw_prompts(w).3).unwrap_or(super::panels::Passkey::Idle);
            let (phone, key) = w.map(|w| backups_of(first_pin_account(w).as_ref())).unwrap_or((true, true));
            return Some(pin_tasks::view(t, phone, key, &passkey, now_ms()).map(&Msg::Task));
        }
        if let Some((req, load)) = fixture_review() {
            use super::panels::{KeystonePrompt, LedgerPrompt, Passkey};
            let accounts: Vec<RareAccount> = super::fixtures::accounts();
            let items = BTreeMap::new();
            // Shots: the PIN sheet of a passkey account (RR_REVIEW_UNLOCK=passkey) or of a big send (send).
            match std::env::var("RR_REVIEW_UNLOCK").as_deref() {
                Ok("passkey") => {
                    let p = unlock::Pending { reason: "Enter your PIN, or sign with the passkey on your phone.".into(), phone: true, title: None, phone_label: None };
                    return Some(unlock::view(&p, VaultKind::Pin, &unlock::State::default(), 0).map(&Msg::Unlock));
                }
                Ok("send") => {
                    let p = unlock::Pending { reason: "This sends about $182.40. Enter your PIN to approve.".into(), phone: false, title: Some("Enter your PIN".into()), phone_label: None };
                    return Some(unlock::view(&p, VaultKind::Pin, &unlock::State::default(), 0).map(&Msg::Unlock));
                }
                // Shots: a PIN account's PIN sheet, after 3 and 4 wrong PINs, and the lock.
                Ok(k @ ("pin" | "pin3" | "pin4")) => {
                    let p = unlock::Pending { reason: "Enter your PIN to approve".into(), phone: true, title: Some("Enter your PIN".into()), phone_label: Some("Sign with my phone instead".into()) };
                    let mut u = unlock::State::default();
                    u.error = match k {
                        "pin3" => Some(rare_wallet::pin_guard::rules::wrong_text(3)),
                        "pin4" => Some(rare_wallet::pin_guard::rules::wrong_text(4)),
                        _ => None,
                    };
                    return Some(unlock::view(&p, VaultKind::Pin, &u, 0).map(&Msg::Unlock));
                }
                Ok(k @ ("locked" | "locked-qr" | "locked-key" | "newpin")) => {
                    let mut l = locked::State::default();
                    let qr = Passkey::Qr { url: "FIDO:/0123456789012345678901234567890123456789012345678901234567890123456789".into() };
                    let touch = Passkey::Status { message: "Touch your security key".into() };
                    let pk = match k {
                        "locked-qr" => {
                            l.phase = locked::Phase::Unlocking(crate::screens::wallet::onboard::ceremony::Via::Phone);
                            &qr
                        }
                        "locked-key" => {
                            l.phase = locked::Phase::Unlocking(crate::screens::wallet::onboard::ceremony::Via::Usb);
                            &touch
                        }
                        "newpin" => {
                            l.phase = locked::Phase::NewPin;
                            &Passkey::Idle
                        }
                        _ => &Passkey::Idle,
                    };
                    return Some(locked::view(&locked::Props { phone: true, key: true, passkey: pk, forgot: false }, &l).map(&Msg::Locked));
                }
                _ => {}
            }
            // Shots: the passkey signing on the phone (RR_REVIEW_PASSKEY=qr|idle).
            if let Ok(pk) = std::env::var("RR_REVIEW_PASSKEY") {
                let passkey = if pk == "qr" { Passkey::Qr { url: "FIDO:/0123456789012345678901234567890123456789012345678901234567890123456789".into() } } else { Passkey::Idle };
                let flow = Flow { phase: Phase::Working, error: None, final_: false, result: None };
                let pr = review::Props { req: &req, flow: Some(&flow), accounts: &accounts, load: &load, items: &items, force: Some(Hw::Passkey), ledger: &LedgerPrompt::default(), keystone: &KeystonePrompt::default(), passkey: &passkey, method: None, phone: true };
                return Some(review::view(&pr, &self.sheet).map(&Msg::Sheet));
            }
            // Shots: the done state (RR_REVIEW_PHASE=done), "Your signature is ready." for a message.
            if std::env::var("RR_REVIEW_PHASE").as_deref() == Ok("done") {
                let digest = (req.kind != review::ReqKind::Message).then(|| "9Xq3kVhRZb6yQ1m2pTnCw8aFsL4oD7eJuGiK5vBxY0Nc".to_string());
                let flow = Flow { phase: Phase::Done, error: None, final_: false, result: Some(review::SignResult { digest, dry_run: false }) };
                let pr = review::Props { req: &req, flow: Some(&flow), accounts: &accounts, load: &load, items: &items, force: None, ledger: &LedgerPrompt::default(), keystone: &KeystonePrompt::default(), passkey: &Passkey::Idle, method: None, phone: false };
                return Some(review::view(&pr, &self.sheet).map(&Msg::Sheet));
            }
            let method = std::env::var("RR_REVIEW_METHOD").ok().map(|m| match m.as_str() {
                "both" => service::METHOD_PASSKEY_BOTH,
                _ => service::METHOD_PASSKEY_PHONE,
            });
            let pr = review::Props { req: &req, flow: None, accounts: &accounts, load: &load, items: &items, force: None, ledger: &LedgerPrompt::default(), keystone: &KeystonePrompt::default(), passkey: &Passkey::Idle, method, phone: false };
            return Some(review::view(&pr, &self.sheet).map(&Msg::Sheet));
        }
        if let Some(l) = &self.locked {
            let w = service::wallet()?;
            let (_, _, _, passkey) = hw_prompts(w);
            let acc = self.shown.as_ref().and_then(|r| account_of(w, &r.account));
            let (phone, key) = backups_of(acc.as_ref());
            return Some(locked::view(&locked::Props { phone, key, passkey: &passkey, forgot: false }, l).map(&Msg::Locked));
        }
        if let (Some((ust, _)), PinFor::Device) = (&self.unlock, self.pin_for) {
            let acc = service::wallet().and_then(|w| self.shown.as_ref().and_then(|r| account_of(w, &r.account)));
            let (phone, _) = backups_of(acc.as_ref());
            let reason = if self.req.as_ref().is_some_and(|q| q.kind == review::ReqKind::Message) { "Enter your PIN to sign" } else { "Enter your PIN to approve" };
            let pend = unlock::Pending { reason: reason.into(), phone: true, title: Some("Enter your PIN".into()), phone_label: Some(if phone { "Sign with my phone instead" } else { "Sign with my security key instead" }.into()) };
            return Some(unlock::view(&pend, VaultKind::Pin, ust, now_ms()).map(&Msg::Unlock));
        }
        if let Some((ust, kind)) = &self.unlock {
            let passkey = self.standalone.is_none() && self.pin_for == PinFor::Vault && self.req.as_ref().is_some_and(|q| self.accounts.iter().any(|a| a.address == q.account && a.kind == "passkey"));
            let reason = match (&self.standalone, &self.req) {
                _ if self.pin_for == PinFor::Send => match usd_out(self.load.as_ref()) {
                    Some(v) => format!("This sends about ${v:.2}. Enter your PIN to approve."),
                    None => "Enter your PIN to approve this send.".into(),
                },
                (Some((r, _)), _) => r.clone(),
                _ if passkey => "Enter your PIN, or sign with the passkey on your phone.".into(),
                (None, Some(req)) if req.kind == review::ReqKind::Message => "Enter your PIN to sign".into(),
                _ => "Enter your PIN to approve".into(),
            };
            let title = (self.pin_for == PinFor::Send).then(|| "Enter your PIN".to_string());
            return Some(unlock::view(&unlock::Pending { reason, phone: passkey, title, phone_label: None }, *kind, ust, now_ms()).map(&Msg::Unlock));
        }
        let req = self.req.as_ref()?;
        let w = service::wallet()?;
        let (_, ledger, keystone, passkey) = hw_prompts(w);
        let load = self.load.clone().unwrap_or(Load::Unavailable);
        let items = BTreeMap::new();
        let shown_req = self.host.current(Some(req)).cloned().unwrap_or_else(|| req.clone());
        let phone = self.accounts.iter().any(|a| a.address == req.account && a.kind == "passkey") && (self.use_phone || !w.keystore.has_device_key(&req.account));
        let pr = review::Props { req: &shown_req, flow: self.host.flow(), accounts: &self.accounts, load: &load, items: &items, force: None, ledger: &ledger, keystone: &keystone, passkey: &passkey, method: self.method.as_deref(), phone };
        Some(review::view(&pr, &self.sheet).map(&Msg::Sheet))
    }

    fn handle(&mut self, st: &mut State, m: Msg) {
        let now = rare_render::clock::secs();
        match m {
            Msg::Parental(pm) => {
                if self.parental.as_mut().is_some_and(|p| p.update(st, pm, now)) {
                    self.parental = None;
                }
            }
            Msg::Task(tm) => {
                let Some(t) = self.task.as_mut() else { return };
                match pin_tasks::update(t, tm, now_ms()) {
                    Some(pin_tasks::Effect::Close) => {
                        if let Some(w) = service::wallet() {
                            w.passkey.cancel();
                        }
                        self.task = None;
                        self.task_pin.clear();
                    }
                    Some(pin_tasks::Effect::Check(pin)) => {
                        let words = t.task == pin_tasks::Task::Words;
                        self.task_pin = pin.clone();
                        self.rx_task_check = Some(service::work(move |w| {
                            let r = if words {
                                w.keystore.pin_words(&pin).map(|s| Some(s.split_whitespace().map(str::to_string).collect::<Vec<_>>()))
                            } else {
                                match w.keystore.pin_device() {
                                    Some(p) => p.open(&pin).map(|_| None).map_err(|e| rare_wallet::keystore::KeystoreError::Vault(rare_wallet::WalletError::parse(&e))),
                                    None => Err(rare_wallet::keystore::KeystoreError::Locked),
                                }
                            };
                            r.map_err(|e| (e.code() == "pin-locked", e.to_string()))
                        }));
                    }
                    Some(pin_tasks::Effect::Backup(via)) => {
                        let wv = match via {
                            crate::screens::wallet::onboard::ceremony::Via::Phone => rare_wallet::passkey_account::Via::Phone,
                            crate::screens::wallet::onboard::ceremony::Via::Usb => rare_wallet::passkey_account::Via::Usb,
                        };
                        if let Some(w) = service::wallet() {
                            *w.passkey_event.lock().unwrap() = None;
                        }
                        self.rx_task_backup = Some(service::work(move |w| {
                            let acc = first_pin_account(w).ok_or_else(|| "No PIN account on this SuiPlay".to_string())?;
                            let mut ctap = rare_wallet::passkey_account::Ctap { state: &w.passkey, sink: w.sink(), rt: w.rt.clone() };
                            let r = rare_wallet::passkey_account::unlock_with_phone(&w.keystore, &acc, &mut ctap, wv).map_err(|e| e.to_string());
                            *w.passkey_event.lock().unwrap() = None;
                            w.bump();
                            r
                        }));
                    }
                    Some(pin_tasks::Effect::Save { old, new }) => {
                        let token = match &t.phase {
                            pin_tasks::Phase::Backup(l) => l.token.clone(),
                            _ => None,
                        };
                        self.rx_task_save = Some(service::work(move |w| match (old, token) {
                            (Some(old), _) => w.keystore.change_pin(&old, &new).map_err(|e| e.to_string()),
                            (None, Some(token)) => {
                                let acc = first_pin_account(w).ok_or_else(|| "No PIN account on this SuiPlay".to_string())?;
                                let r = rare_wallet::passkey_account::reset_pin(&w.keystore, &acc.address, &token, &new).map_err(|e| e.to_string());
                                w.keystore.disarm();
                                r
                            }
                            _ => Err("Unlock with your backup again".to_string()),
                        }));
                    }
                    None => {}
                }
            }
            Msg::Locked(lm) => {
                let Some(l) = self.locked.as_mut() else { return };
                match locked::update(l, lm) {
                    Some(locked::Effect::Unlock(via)) => {
                        let Some(w) = service::wallet() else { return };
                        let Some(acc) = self.shown.as_ref().and_then(|r| account_of(w, &r.account)) else { return };
                        *w.passkey_event.lock().unwrap() = None;
                        let wv = match via {
                            crate::screens::wallet::onboard::ceremony::Via::Phone => rare_wallet::passkey_account::Via::Phone,
                            crate::screens::wallet::onboard::ceremony::Via::Usb => rare_wallet::passkey_account::Via::Usb,
                        };
                        self.rx_phone_unlock = Some(service::work(move |w| {
                            let mut ctap = rare_wallet::passkey_account::Ctap { state: &w.passkey, sink: w.sink(), rt: w.rt.clone() };
                            let r = rare_wallet::passkey_account::unlock_with_phone(&w.keystore, &acc, &mut ctap, wv).map_err(|e| e.to_string());
                            *w.passkey_event.lock().unwrap() = None;
                            w.bump();
                            r
                        }));
                    }
                    Some(locked::Effect::Phone) => {
                        if let Some(w) = service::wallet() {
                            w.passkey.cancel();
                        }
                        self.locked = None;
                        self.unlock = None;
                        self.use_phone = true;
                        self.approve(st);
                    }
                    Some(locked::Effect::Reset(pin)) => {
                        let token = l.token.clone().unwrap_or_default();
                        let account = self.shown.as_ref().map(|r| r.account.clone()).unwrap_or_default();
                        self.rx_reset = Some(service::work(move |w| rare_wallet::passkey_account::reset_pin(&w.keystore, &account, &token, &pin).map_err(|e| e.to_string())));
                    }
                    Some(locked::Effect::Cancel) => {
                        if let Some(w) = service::wallet() {
                            w.passkey.cancel();
                        }
                        self.locked = None;
                        self.unlock = None;
                        self.host.finish(Outcome::NotUnlocked);
                    }
                    None => {}
                }
            }
            Msg::Unlock(um) => {
                let Some((ust, kind)) = self.unlock.as_mut() else { return };
                let kind = *kind;
                if um == unlock::Msg::Type {
                    ask_secret(st, ASK_PASS, "Passphrase");
                    return;
                }
                match unlock::update(ust, um, now_ms()) {
                    Some(unlock::Effect::Unlock(secret)) if self.pin_for == PinFor::Device => {
                        let account = self.shown.as_ref().map(|r| r.account.clone()).unwrap_or_default();
                        self.rx_unlock = Some(service::work(move |w| w.keystore.arm_pin(&account, &secret)))
                    }
                    Some(unlock::Effect::Unlock(secret)) if self.pin_for == PinFor::Send => {
                        self.rx_unlock = Some(service::work(move |w| w.send_pin.check(&secret).map_err(|e| rare_wallet::keystore::KeystoreError::Vault(rare_wallet::WalletError::parse(&e)))))
                    }
                    Some(unlock::Effect::Unlock(secret)) => {
                        self.rx_unlock = Some(service::work(move |w| {
                            let r = w.keystore.unlock(&secret);
                            w.bump();
                            r
                        }))
                    }
                    Some(unlock::Effect::Phone) => {
                        self.unlock = None;
                        self.use_phone = true;
                        self.approve(st);
                    }
                    Some(unlock::Effect::AskPassphrase) => ask_secret(st, ASK_PASS, "Passphrase"),
                    Some(unlock::Effect::Finish(ok)) => self.unlocked(st, ok),
                    None => {}
                }
                let _ = kind;
            }
            Msg::Sheet(sm) => {
                let failed = matches!(&self.load, Some(Load::Ready(r)) if !r.ok);
                match review::update(&mut self.sheet, failed, &sm) {
                    Some(review::Effect::Approve) => self.approve(st),
                    Some(review::Effect::Cancel) => self.cancel(),
                    Some(review::Effect::Done) => {
                        self.host.done();
                        self.reset_to(None);
                    }
                    Some(review::Effect::Copy(_)) => {
                        self.copied_until = now + 1.5;
                        st.toast("Transaction ID copied", Tone::Info, now);
                    }
                    Some(review::Effect::AskPasskeyPin) => ask_secret(st, ASK_SKPIN, "Security key PIN"),
                    None => {}
                }
            }
        }
        self.ver += 1;
    }
}

impl Page for ReviewHost {
    fn stamp(&self, _st: &State) -> u64 {
        let unlocking = self.locked.as_ref().is_some_and(|l| matches!(l.phase, locked::Phase::Unlocking(_))) || self.task.as_ref().is_some_and(|t| matches!(&t.phase, pin_tasks::Phase::Backup(l) if matches!(l.phase, locked::Phase::Unlocking(_))));
        let wv = if unlocking || self.host.flow().is_some_and(|f| f.phase == Phase::Working) { service::wallet().map(|w| w.version()).unwrap_or(0) } else { 0 };
        let pw = self.parental.as_ref().is_some_and(|p| p.waiting(rare_render::clock::secs())) as u64;
        hash((self.ver, wv, (now_ms() / 1000) * (self.unlock.as_ref().map_or(0, |(u, _)| (u.until > 0) as u64) | pw)))
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        ACTIVE.store(self.visible(), Ordering::SeqCst);
        match self.markup() {
            Some(m) => {
                let (mut el, built) = build(ui, acts, &m);
                self.built = built;
                let done = self.built.msgs.iter().rposition(|x| *x == Some(Msg::Sheet(review::Msg::Done))).map(|i| Act::Local(i as u32));
                self.done_tag = done.and_then(|d| (0..).map_while(|t| acts.get(t).map(|a| (t, a))).find(|(_, a)| **a == d).map(|(t, _)| t));
                // The sheet's [data-nav-scope] is the overlay's (1) by default: its presses must
                // come here, and it must hold focus over an overlay too.
                crate::screens::wallet::render::rescope(&mut el, 1, SCOPE);
                el
            }
            None => {
                self.built = Built::default();
                ui.bx(s())
            }
        }
    }

    fn pad(&mut self, st: &mut State, _fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if !self.visible() {
            return false;
        }
        if b == Btn::B {
            let m = if self.parental.is_some() {
                Some(Msg::Parental(crate::features::parental_sheet::Msg::Cancel))
            } else if self.task.is_some() {
                Some(Msg::Task(pin_tasks::Msg::Done))
            } else if self.locked.is_some() {
                Some(Msg::Locked(locked::Msg::Cancel))
            } else if self.unlock.is_some() {
                Some(Msg::Unlock(unlock::Msg::Cancel))
            } else {
                match self.host.flow().map(|f| (f.phase, f.final_)) {
                    Some((Phase::Working, _)) => None,
                    Some((Phase::Done, _)) | Some((_, true)) => Some(Msg::Sheet(review::Msg::Done)),
                    _ => Some(Msg::Sheet(review::Msg::Cancel)),
                }
            };
            if let Some(m) = m {
                self.handle(st, m);
            }
            return true;
        }
        // Done: A closes it wherever focus is (it is the only thing to do there but copy an ID).
        if b == Btn::A && self.done() && self.req.as_ref().is_some_and(|r| r.kind == review::ReqKind::Message) {
            self.handle(st, Msg::Sheet(review::Msg::Done));
            return true;
        }
        let name = match b {
            Btn::X => "x",
            Btn::Y => "y",
            Btn::Start => "start",
            Btn::Lt => "lt",
            Btn::Rt => "rt",
            _ => return false,
        };
        if let Some(m) = self.built.pad(name) {
            self.handle(st, m);
        }
        true
    }

    fn take_focus(&mut self, cur: Option<&Focusable>) -> Option<u32> {
        // The done state: Done has focus, never a control the working state left behind.
        let t = self.done_tag.filter(|_| self.done() && !self.done_focused)?;
        self.done_focused = true;
        (cur.map(|c| c.tag) != Some(t)).then_some(t)
    }

    fn act(&mut self, st: &mut State, _fx: &mut Fx, code: u32) {
        if let Some(m) = self.built.msg(code) {
            self.handle(st, m);
        }
    }

    fn tick(&mut self, st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        // The parent's PIN: a request from anywhere, and an approval that waited for it.
        if self.parental.is_none() {
            if let Some(a) = crate::features::parental::take_ask() {
                self.parental = Some(crate::features::parental_sheet::Sheet::new(a));
                self.ver += 1;
                ACTIVE.store(true, Ordering::SeqCst);
            }
        }
        if let Some(rx) = self.rx_parental.as_ref() {
            match rx.try_recv() {
                Ok(true) => {
                    self.rx_parental = None;
                    self.approve(st);
                }
                Ok(false) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.rx_parental = None;
                    self.host.finish(Outcome::NotUnlocked);
                    self.ver += 1;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.parental.is_some() && (std::env::var("RR_FIXTURE").is_ok() || service::wallet().is_none()) {
            return Some(now + 0.25);
        }
        // golden images never touch the real wallet
        if std::env::var("RR_FIXTURE").is_ok() {
            return None;
        }
        let w = service::wallet()?;
        // the keyboard's answers
        if let Some(v) = st.take_answer(ASK_PASS) {
            match v {
                Some(v) if !v.is_empty() => self.handle(st, Msg::Unlock(unlock::Msg::Typed(v))),
                _ => {}
            }
        }
        if let Some(v) = st.take_answer(ASK_SKPIN) {
            w.passkey.pin(v.filter(|p| !p.is_empty()));
        }
        // the queue
        let head = w.signing.requests().into_iter().next();
        if self.standalone.is_none() && self.host.flow().is_none() && self.rx_sign.is_none() && head.as_ref().map(|r| r.id) != self.shown.as_ref().map(|r| r.id) {
            self.reset_to(head);
        }
        // an unlock a page asked for, when no request is on screen
        if self.req.is_none() && self.standalone.is_none() && self.unlock.is_none() {
            if let Some(u) = service::take_unlock() {
                let kind = Self::vault_kind(w);
                if w.keystore.is_unlocked().unwrap_or(false) {
                    let _ = u.1.send(true);
                } else {
                    self.standalone = Some(u);
                    self.unlock = Some((unlock::State::default(), kind));
                    if kind == VaultKind::Passphrase {
                        ask_secret(st, ASK_PASS, "Passphrase");
                    }
                    self.ver += 1;
                }
            }
        }
        if let Some(r) = poll(&mut self.rx_review) {
            self.load = Some(match r {
                Ok(Ok(rv)) => Load::Ready(Box::new(review_of(rv))),
                Ok(Err(e)) | Err(e) => {
                    log::warn!("review: {e}");
                    match short_of_funds(&e) {
                        Some(reason) => Load::Ready(Box::new(cannot_build(reason))),
                        None => Load::Unavailable,
                    }
                }
            });
            self.ver += 1;
        }
        if self.task.is_none() && self.req.is_none() && self.standalone.is_none() && self.locked.is_none() {
            if let Some(t) = service::take_pin_task() {
                self.task = Some(pin_tasks::State::new(t));
                self.ver += 1;
                ACTIVE.store(true, Ordering::SeqCst);
            }
        }
        if let Some(r) = poll(&mut self.rx_task_check) {
            let pin = std::mem::take(&mut self.task_pin);
            if let Some(t) = self.task.as_mut() {
                pin_tasks::checked(t, pin, r.unwrap_or_else(|e| Err((false, e))));
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.rx_task_backup) {
            if let Some(t) = self.task.as_mut() {
                pin_tasks::backed(t, r.and_then(|x| x));
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.rx_task_save) {
            if let Some(t) = self.task.as_mut() {
                if pin_tasks::saved(t, r.and_then(|x| x)) {
                    self.task = None;
                    st.toast("PIN changed", Tone::Info, rare_render::clock::secs());
                }
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.rx_status) {
            if r.unwrap_or(false) && self.pin_for == PinFor::Device && self.unlock.is_some() {
                self.unlock = None;
                self.locked = Some(locked::State::default());
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.rx_phone_unlock) {
            if let Some(l) = self.locked.as_mut() {
                locked::unlocked(l, r.and_then(|x| x));
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.rx_reset) {
            if let Some(l) = self.locked.as_mut() {
                if locked::saved(l, r.and_then(|x| x)) {
                    self.locked = None;
                    self.pin_ok = true;
                    self.approve(st);
                }
            }
            self.ver += 1;
        }
        if self.pin_for == PinFor::Device && self.unlock.is_some() && self.rx_unlock.is_some() {
            if let Some(r) = poll(&mut self.rx_unlock) {
                let e = match r {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some((e.code().to_string(), e.to_string())),
                    Err(e) => Some(("error".to_string(), e)),
                };
                match e {
                    None => self.unlocked(st, true),
                    Some((code, _)) if code == "pin-locked" => {
                        self.unlock = None;
                        self.locked = Some(locked::State::default());
                    }
                    Some((_, m)) => {
                        if let Some((ust, _)) = self.unlock.as_mut() {
                            ust.busy = false;
                            ust.value.clear();
                            ust.error = Some(m);
                        }
                    }
                }
                self.ver += 1;
            }
        }
        if let Some(r) = poll(&mut self.rx_unlock) {
            if let Some((ust, kind)) = self.unlock.as_mut() {
                let res = match r {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(e)) => Err(unlock_error(e)),
                    Err(e) => Err(UnlockError::Other(Some(e))),
                };
                if let Some(unlock::Effect::Finish(ok)) = unlock::on_result(ust, *kind, res, now_ms()) {
                    self.unlocked(st, ok);
                }
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.rx_sign) {
            let id = self.shown.as_ref().map(|r| r.id);
            let out = match r {
                Ok(Ok(res)) => Outcome::Signed { failed_on_chain: res.status.as_deref() == Some("failure"), result: review::SignResult { digest: res.digest, dry_run: res.dry_run } },
                Ok(Err(SignError::Gone)) => Outcome::Threw { message: None, still_queued: false },
                Ok(Err(e)) => Outcome::Threw { message: Some(e.to_string()), still_queued: w.signing.requests().iter().any(|q| Some(q.id) == id) },
                Err(e) => Outcome::Threw { message: Some(e), still_queued: true },
            };
            self.host.finish(out);
            if self.host.flow().is_none() {
                self.reset_to(None);
            }
            self.ver += 1;
        }
        // The done state: noted once; a message signature closes by itself.
        if self.done() {
            let at = *self.done_at.get_or_insert(now);
            let message = self.req.as_ref().is_some_and(|r| r.kind == review::ReqKind::Message);
            if message && now >= at + DONE_CLOSE_S {
                self.handle(st, Msg::Sheet(review::Msg::Done));
                ACTIVE.store(self.visible(), Ordering::SeqCst);
                return Some(now + 0.25);
            }
            if message {
                return Some((at + DONE_CLOSE_S).min(now + 0.25));
            }
        } else {
            self.done_at = None;
        }
        if let Some((ust, _)) = self.unlock.as_mut() {
            unlock::tick(ust, now_ms());
        }
        if self.sheet.copied && now >= self.copied_until {
            self.sheet.copied = false;
            self.ver += 1;
        }
        let v = w.version();
        if v != self.seen {
            self.seen = v;
            self.ver += 1;
        }
        ACTIVE.store(self.visible(), Ordering::SeqCst);
        let busy = self.rx_review.is_some() || self.rx_sign.is_some() || self.rx_unlock.is_some() || self.rx_phone_unlock.is_some() || self.rx_reset.is_some() || self.rx_status.is_some() || self.rx_task_check.is_some() || self.rx_task_backup.is_some() || self.rx_task_save.is_some() || self.unlock.as_ref().is_some_and(|(u, _)| u.until > 0);
        Some(now + if busy { 0.1 } else { 0.25 })
    }
}

#[allow(dead_code)]
fn _flow_type_check(f: &Flow) -> bool {
    f.final_
}

#[cfg(test)]
mod funds_tests {
    #[test]
    fn a_missing_coin_reads_plainly() {
        let e = "build|Conversion error due to input issue: address 0xae21 does not have sufficient balance of 0x356a26eb9e012a68958082340d4c4116e7f55615cf27affcff209cf0ae544f59::wal::WAL: requested 19279701 available 0";
        assert_eq!(super::short_of_funds(e).as_deref(), Some("This account doesn't have enough WAL. Add WAL to it and try again."));
        assert_eq!(super::short_of_funds("timeout"), None);
    }
}
