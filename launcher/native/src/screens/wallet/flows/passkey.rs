//! Passkey accounts on this device: create one (phone QR or USB security key) and sign in with an existing Rare passkey.
//!
//! The views and the step logic are `onboard/passkey_flow.rs`; this file performs its effects with
//! rare-wallet: the vault gate and PIN, Bluetooth, the CTAP ceremony on a worker (`create_account`
//! / `sign_in`), the ceremony's live state from `w.passkey_event`, and the security key's PIN.

use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::home::stack::{PasskeyMode, WalletView};
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::onboard::ceremony::{Ceremony, Via};
use crate::screens::wallet::onboard::flow::{Failure, Gate};
use crate::screens::wallet::onboard::passkey_flow::{self as pk, Effect, Step};
use crate::screens::wallet::onboard::passkey_page;
use crate::screens::wallet::onboard::pin_setup;
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::service::{self, Wallet};
use crate::screens::wallet::sign::host::ask_secret;
use crate::screens::wallet::types::SecretKind;
use crate::state::State;
use rare_wallet::keystore::PinOptions;
use rare_wallet::passkey_account::{self, Ctap, History, PasskeyError, Via as WVia};
use std::sync::mpsc::Receiver;

const KEY_PASS: &str = "pk-pass";
const KEY_PASS2: &str = "pk-pass2";

type Rx<T> = Option<Receiver<Result<T, String>>>;
/// A finished run: its id and the account address with whether this device holds its key.
type Ran = (u64, Result<(String, bool), Failure>);

pub struct Flow {
    /// the page (stack frame) the state below belongs to
    key: u64,
    st: pk::State,
    via: Via,
    ceremony: Ceremony,
    qr: Option<String>,
    seen: u64,
    gate: Option<std::sync::mpsc::Receiver<bool>>,
    bt: Rx<bool>,
    power: Rx<(bool, bool, Option<String>)>,
    runs: Vec<Receiver<Result<Ran, String>>>,
}

impl Default for Flow {
    fn default() -> Self {
        Flow { key: 0, st: pk::State::default(), via: Via::Phone, ceremony: Ceremony::Idle, qr: None, seen: 0, gate: None, bt: None, power: None, runs: Vec::new() }
    }
}

impl Flow {
    /// A fresh page. Screenshots can start on the ceremony (`RR_WFLOW_STEP=run`).
    fn initial(via: Via, fixture: bool) -> Flow {
        let mut f = Flow { via, ..Flow::default() };
        if fixture && std::env::var("RR_WFLOW_STEP").as_deref() == Ok("run") {
            f.st.step = Step::Run;
            let url = "FIDO:/123456789012345678901234567890123456789012345678901234567890";
            f.ceremony = Ceremony::Qr { url: url.into() };
            f.qr = Some(url.into());
        }
        f
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Pk(pk::Msg),
}

fn to_wallet_via(v: Via) -> WVia {
    match v {
        Via::Phone => WVia::Phone,
        Via::Usb => WVia::Usb,
    }
}

/// The ceremony view of what the passkey code last said.
pub fn ceremony_of(e: &rare_wallet::passkey::Event) -> Ceremony {
    use rare_wallet::passkey::Event;
    match e {
        Event::Qr { url } => Ceremony::Qr { url: url.clone() },
        Event::Status { stage, message } => Ceremony::Status { stage: stage.to_string(), message: message.clone() },
        Event::Pin { attempts_left } => Ceremony::Pin { attempts_left: *attempts_left },
        Event::Error { code, message } => Ceremony::Error { code: code.clone(), message: message.clone() },
    }
}

/// An error from the passkey code as the flow's `Failure` (code, message, the udev fix).
pub fn failure_of(e: PasskeyError) -> Failure {
    match e {
        PasskeyError::Native(n) => {
            let fix = n.data.as_ref().and_then(|d| d.get("fix")).and_then(|f| f.as_str()).map(str::to_string);
            Failure { code: Some(n.code), message: Some(n.message), fix }
        }
        other => Failure::msg(&other.to_string()),
    }
}

/// Chain history for telling candidate passkey accounts apart: any balance, owned object or activity.
pub(in crate::screens::wallet) struct ChainHistory {
    pub(in crate::screens::wallet) w: &'static Wallet,
}

/// rare.market over ureq (blocking; sign-in runs on its own thread).
struct MarketFetch;

impl rare_wallet::passkey_registry::Fetch for MarketFetch {
    fn get(&self, url: &str) -> Result<String, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(6))).build().into();
        let mut r = agent.get(url).call().map_err(|e| e.to_string())?;
        r.body_mut().read_to_string().map_err(|e| e.to_string())
    }
}

impl History for ChainHistory {
    fn registered(&self, passkey_public_key: &[u8], device_key: &[u8]) -> Option<String> {
        rare_wallet::passkey_registry::lookup(&MarketFetch, rare_wallet::passkey_registry::API, passkey_public_key, device_key)
    }

    fn registered_members(&self, passkey_public_key: &[u8], device_key: &[u8]) -> Option<(String, Vec<String>)> {
        rare_wallet::passkey_registry::lookup_members(&MarketFetch, rare_wallet::passkey_registry::API, passkey_public_key, device_key)
    }

    fn has_history(&self, address: &str) -> bool {
        use rare_sui::grpc::{ChainRead, Include};
        let a = address.to_string();
        self.w.rt.block_on(async {
            if self.w.sui.list_balances(&a).await.map(|b| b.iter().any(|x| x.balance > 0)).unwrap_or(false) {
                return true;
            }
            if self.w.sui.owned_objects(&a, None, 1, Include::default()).await.map(|o| !o.is_empty()).unwrap_or(false) {
                return true;
            }
            let feed = rare_sui::activity::ActivityFeed::new(None, rare_sui::Network::Mainnet, None);
            feed.list(&a, None).await.map(|p| !p.items.is_empty()).unwrap_or(false)
        })
    }
}

impl WalletTab {
    fn pk_create(&self) -> bool {
        !matches!(self.stack.top().view, WalletView::Passkey { mode: PasskeyMode::Signin })
    }

    /// Starts over when the page on top is a different one than the state is for.
    fn pk_fresh(&mut self) -> bool {
        let top = self.stack.top().key;
        if !matches!(self.stack.top().view, WalletView::Passkey { .. }) {
            if self.flows.passkey.key != 0 {
                self.pk_stop();
                self.flows.passkey.key = 0;
            }
            return false;
        }
        if self.flows.passkey.key != top {
            self.pk_stop();
            let via = self.flows.passkey.via;
            let mut f = Flow::initial(via, self.fixture);
            f.key = top;
            f.seen = service::wallet().map(|w| w.version()).unwrap_or(0);
            self.flows.passkey = f;
        }
        true
    }

    /// Stops a ceremony still running for the page that went away.
    fn pk_stop(&mut self) {
        let f = &mut self.flows.passkey;
        if f.st.step == Step::Run && !self.fixture {
            if let Some(w) = service::wallet() {
                w.passkey.cancel();
            }
        }
        f.st.run += 1;
    }

    pub(in crate::screens::wallet) fn passkey_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let WalletView::Passkey { mode } = v else { return None };
        let tmp;
        let f = if self.flows.passkey.key == self.stack.top().key {
            &self.flows.passkey
        } else {
            tmp = Flow::initial(self.flows.passkey.via, self.fixture);
            &tmp
        };
        let pr = pk::Props { create: *mode == PasskeyMode::Create, via: f.via, ceremony: &f.ceremony, qr_url: f.qr.as_deref(), can_exit: false };
        Some(passkey_page::page_view(&pr, &f.st).map(&Msg::Pk))
    }

    pub(in crate::screens::wallet) fn passkey_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::Passkey { .. })
    }

    pub(in crate::screens::wallet) fn passkey_hints(&self) -> Vec<Hint> {
        let st = if self.flows.passkey.key == self.stack.top().key { self.flows.passkey.st.clone() } else { pk::State::default() };
        passkey_page::page_hints(&st)
    }

    pub(in crate::screens::wallet) fn passkey_handle(&mut self, st: &mut State, m: Msg) {
        if !self.pk_fresh() {
            return;
        }
        let Msg::Pk(m) = m;
        let mut f = std::mem::take(&mut self.flows.passkey);
        let create = self.pk_create();
        let eff = {
            let pr = pk::Props { create, via: f.via, ceremony: &f.ceremony, qr_url: f.qr.as_deref(), can_exit: false };
            pk::update(&pr, &mut f.st, m)
        };
        self.pk_run(&mut f, st, create, eff);
        self.flows.passkey = f;
    }

    /// Performs effects (and the follow-ups their answers produce) until none are left.
    fn pk_run(&mut self, f: &mut Flow, st: &mut State, create: bool, mut eff: Option<Effect>) {
        let mut guard = 0;
        while let Some(e) = eff.take() {
            guard += 1;
            if guard > 8 {
                break;
            }
            eff = self.pk_effect(f, st, create, e);
        }
    }

    /// The flow's answer function for a `Gate`, `bt_ready`, ... call: runs it against `f`.
    fn pk_answer(&mut self, f: &mut Flow, create: bool, g: impl FnOnce(&pk::Props, &mut pk::State) -> Option<Effect>) -> Option<Effect> {
        let pr = pk::Props { create, via: f.via, ceremony: &f.ceremony, qr_url: f.qr.as_deref(), can_exit: false };
        g(&pr, &mut f.st)
    }

    fn pk_effect(&mut self, f: &mut Flow, st: &mut State, create: bool, e: Effect) -> Option<Effect> {
        match e {
            Effect::SetVia(v) => {
                f.via = v;
                if let Some(w) = service::wallet().filter(|_| !self.fixture) {
                    *w.via.lock().unwrap() = to_wallet_via(v);
                }
                None
            }
            Effect::Gate => {
                let Some(w) = service::wallet().filter(|_| !self.fixture) else { return self.pk_answer(f, create, |pr, s| pk::gate(pr, s, Gate::Ready)) };
                if !w.keystore.has_vault().unwrap_or(false) {
                    return self.pk_answer(f, create, |pr, s| pk::gate(pr, s, Gate::Pin));
                }
                if w.keystore.is_unlocked().unwrap_or(false) {
                    return self.pk_answer(f, create, |pr, s| pk::gate(pr, s, Gate::Ready));
                }
                f.gate = Some(service::ask_unlock("Enter your PIN to add this account"));
                None
            }
            Effect::CheckBluetooth => {
                if self.fixture {
                    return self.pk_answer(f, create, |pr, s| pk::bt_ready(pr, s, true));
                }
                f.bt = Some(service::work(|w| match w.rt.block_on(rare_wallet::bluetooth::state()) {
                    Ok(s) => s.powered || !s.present,
                    Err(_) => true,
                }));
                None
            }
            Effect::PowerOn => {
                if self.fixture {
                    return self.pk_answer(f, create, |pr, s| pk::bt_result(pr, s, true, false, None));
                }
                f.power = Some(service::work(|w| match w.rt.block_on(rare_wallet::bluetooth::power_on()) {
                    Ok(s) => (s.powered, s.blocked, None),
                    Err(e) => {
                        let we = rare_wallet::WalletError::parse(&e);
                        (false, we.code == "bt-blocked", (we.code != "bt-blocked").then_some(we.message))
                    }
                }));
                None
            }
            Effect::Start { run, create: make, pin } => {
                f.ceremony = Ceremony::Idle;
                f.qr = None;
                if self.fixture {
                    let url = "FIDO:/123456789012345678901234567890123456789012345678901234567890";
                    f.ceremony = Ceremony::Qr { url: url.into() };
                    f.qr = Some(url.into());
                    return None;
                }
                let via = to_wallet_via(f.via);
                let pin = pin.map(|p| (p.secret, if p.kind == SecretKind::Pin { "pin" } else { "passphrase" }));
                f.runs.push(service::work(move |w| -> Ran {
                    *w.passkey_event.lock().unwrap() = None;
                    let mut ctap = Ctap { state: &w.passkey, sink: w.sink(), rt: w.rt.clone() };
                    *w.passkey_note.lock().unwrap() = None;
                    let res = if make {
                        passkey_account::create_account(&w.keystore, &mut ctap, via, None, pin.as_ref().map(|(s, k)| PinOptions { secret: s, kind: k }))
                    } else {
                        let mut auth = SecondTouch { inner: ctap, w, asked: 0 };
                        passkey_account::sign_in(&w.keystore, &mut auth, &ChainHistory { w }, via, None)
                    };
                    *w.passkey_note.lock().unwrap() = None;
                    if let (true, Ok(a)) = (make, &res) {
                        let addr = a.address.clone();
                        std::thread::spawn(move || service::register_passkey(w, &addr));
                    }
                    w.bump();
                    (
                        run,
                        res.map(|a| {
                            let has_key = a.passkey.as_ref().and_then(|p| p.local_key).unwrap_or_else(|| w.keystore.has_device_key(&a.address));
                            (a.address, has_key)
                        })
                        .map_err(failure_of),
                    )
                }));
                None
            }
            Effect::CancelCeremony => {
                f.ceremony = Ceremony::Idle;
                f.qr = None;
                f.key_pin_clear();
                if let Some(w) = service::wallet().filter(|_| !self.fixture) {
                    w.passkey.cancel();
                }
                None
            }
            Effect::KeyPin(p) => {
                if let Some(w) = service::wallet().filter(|_| !self.fixture) {
                    w.passkey.pin(p);
                }
                None
            }
            Effect::AskPass { confirm } => {
                if confirm {
                    ask_secret(st, KEY_PASS2, "Confirm passphrase");
                } else {
                    ask_secret(st, KEY_PASS, "Passphrase");
                }
                None
            }
            Effect::Finish(address) => {
                if let Some(w) = service::wallet().filter(|_| !self.fixture) {
                    // the lib already selected it; sign_in on a known account too
                    let _ = w.keystore.select(&address);
                    w.bump();
                }
                self.wallet_ver = 0;
                self.sync_wallet();
                self.selected = Some(address);
                crate::screens::wallet::home::page::account_changed(&mut self.home);
                f.st.step = Step::Choose;
                self.stack.reset(Vec::new());
                self.ver += 1;
                None
            }
            Effect::Exit => None,
        }
    }

    pub(in crate::screens::wallet) fn passkey_tick(&mut self, st: &mut State) -> bool {
        if !self.pk_fresh() {
            return false;
        }
        let create = self.pk_create();
        let mut f = std::mem::take(&mut self.flows.passkey);
        let mut changed = false;
        let mut effs: Vec<Effect> = Vec::new();

        // the unlock sheet's answer
        if let Some(rx) = &f.gate {
            match rx.try_recv() {
                Ok(ok) => {
                    f.gate = None;
                    let g = if ok { Gate::Ready } else { Gate::Cancelled };
                    effs.extend(self.pk_answer(&mut f, create, |pr, s| pk::gate(pr, s, g)));
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    f.gate = None;
                    changed = true;
                }
                Err(_) => {}
            }
        }
        if let Some(r) = service::poll(&mut f.bt) {
            let ready = r.unwrap_or(true);
            effs.extend(self.pk_answer(&mut f, create, |pr, s| pk::bt_ready(pr, s, ready)));
            changed = true;
        }
        if let Some(r) = service::poll(&mut f.power) {
            let (powered, blocked, msg) = r.unwrap_or((false, false, Some("Could not turn Bluetooth on. Try again.".into())));
            effs.extend(self.pk_answer(&mut f, create, |pr, s| pk::bt_result(pr, s, powered, blocked, msg.as_deref())));
            changed = true;
        }
        // finished runs
        let mut done: Vec<Result<Ran, String>> = Vec::new();
        f.runs.retain(|rx| match rx.try_recv() {
            Ok(r) => {
                done.push(r);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for r in done {
            changed = true;
            let (run, res) = match r {
                Ok(v) => v,
                Err(m) => (f.st.run, Err(Failure::msg(&m))),
            };
            effs.extend(self.pk_answer(&mut f, create, |pr, s| pk::ran(pr, s, run, res)));
        }
        // the passphrase fields
        for (key, second) in [(KEY_PASS, false), (KEY_PASS2, true)] {
            if let Some(Some(text)) = st.take_answer(key) {
                let m = if second { pin_setup::Msg::Pass2(text) } else { pin_setup::Msg::Pass(text) };
                let eff = {
                    let pr = pk::Props { create, via: f.via, ceremony: &f.ceremony, qr_url: f.qr.as_deref(), can_exit: false };
                    pk::update(&pr, &mut f.st, pk::Msg::Pin(m))
                };
                effs.extend(eff);
                changed = true;
            } else if let Some(None) = st.take_answer(key) {
                changed = true;
            }
        }
        // the live ceremony
        if f.st.step == Step::Run && !self.fixture {
            if let Some(w) = service::wallet() {
                let v = w.version();
                if v != f.seen {
                    f.seen = v;
                    let ev = w.passkey_event.lock().unwrap().clone();
                    if let Some(e) = ev {
                        let c = ceremony_of(&e);
                        if let Ceremony::Qr { url } = &c {
                            f.qr = Some(url.clone());
                        }
                        if c != f.ceremony {
                            if !matches!(c, Ceremony::Pin { .. }) {
                                f.key_pin_clear();
                            }
                            f.ceremony = c;
                            changed = true;
                        }
                    }
                }
            }
        }
        for e in effs {
            self.pk_run(&mut f, st, create, Some(e));
        }
        let busy = f.gate.is_some() || f.bt.is_some() || f.power.is_some() || !f.runs.is_empty() || f.st.step == Step::Run;
        self.flows.passkey = f;
        if changed {
            self.ver += 1;
        }
        busy
    }
}

impl Flow {
    fn key_pin_clear(&mut self) {
        self.st.key_pin.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_wallet::passkey::Event;

    #[test]
    fn ceremony_events() {
        assert_eq!(ceremony_of(&Event::Qr { url: "FIDO:/1".into() }), Ceremony::Qr { url: "FIDO:/1".into() });
        assert_eq!(ceremony_of(&Event::Pin { attempts_left: Some(3) }), Ceremony::Pin { attempts_left: Some(3) });
        assert_eq!(ceremony_of(&Event::Status { stage: "touch", message: "Touch".into() }), Ceremony::Status { stage: "touch".into(), message: "Touch".into() });
    }

    #[test]
    fn failures() {
        let n = rare_wallet::WalletError::parse("udev|No permission|{\"fix\":\"pkexec x\"}");
        let f = failure_of(PasskeyError::Native(n));
        assert_eq!(f.code.as_deref(), Some("udev"));
        assert_eq!(f.fix.as_deref(), Some("pkexec x"));
        let f = failure_of(PasskeyError::Invalid("This passkey was not made in Rare Wallet. Pick your Rare passkey.".into()));
        assert_eq!(f.code, None);
        assert!(f.message.unwrap().starts_with("This passkey"));
    }

    #[test]
    fn page_markup_and_fresh_state() {
        let mut t = WalletTab { fixture: true, ..WalletTab::default() };
        t.stack.push(WalletView::Passkey { mode: PasskeyMode::Signin }, None);
        let m = t.markup();
        assert!(m.texts().iter().any(|s| s == "Sign in with a passkey"));
        let mut st = crate::state::State::default();
        // fixture: Continue on the phone goes straight to the ceremony
        t.passkey_handle(&mut st, Msg::Pk(pk::Msg::Continue));
        assert!(t.passkey_tick(&mut st));
        assert!(t.markup().texts().iter().any(|s| s.starts_with("Scan this code")));
        // leaving the page and coming back starts over
        t.stack.pop();
        t.stack.push(WalletView::Passkey { mode: PasskeyMode::Create }, None);
        assert!(t.markup().texts().iter().any(|s| s == "Create account"));
    }
}

/// The passkey for sign in, saying why when it asks a second time.
struct SecondTouch<'a> {
    inner: Ctap<'a>,
    w: &'static crate::screens::wallet::service::Wallet,
    asked: u32,
}

/// Plain words for the second touch of sign in.
pub const SECOND_TOUCH: &str = "Use your passkey once more. Two Rare accounts fit the first signature, and a second one tells which is yours. This happens only the first time on this device.";

impl passkey_account::Authenticator for SecondTouch<'_> {
    fn register(&mut self, user_id: &str, user_name: &str, display_name: &str, challenge: &str, via: passkey_account::Via) -> Result<passkey_account::Registration, String> {
        self.inner.register(user_id, user_name, display_name, challenge, via)
    }
    fn assert(&mut self, challenge: &str, credential_ids: &[String], via: passkey_account::Via, prf_salt: Option<&str>) -> Result<passkey_account::Assertion, String> {
        self.asked += 1;
        if self.asked > 1 {
            *self.w.passkey_note.lock().unwrap() = Some(SECOND_TOUCH.into());
            self.w.bump();
        }
        self.inner.assert(challenge, credential_ids, via, prf_salt)
    }
    fn prf(&mut self, credential_ids: &[String], salt: &str, via: passkey_account::Via) -> Result<String, String> {
        self.inner.prf(credential_ids, salt, via)
    }
}
