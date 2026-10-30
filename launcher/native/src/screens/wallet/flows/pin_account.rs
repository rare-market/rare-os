//! "Create account with PIN" wired to rare-wallet: the PIN guard and device key
//! (`passkey_account::create_pin_account`), Bluetooth, each backup's CTAP ceremony on a worker
//! (phone over hybrid, security key over USB), the ceremony's live state, the security key's own
//! PIN, and rare.market's registry. Views and steps: `onboard/pin_account.rs`.

use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::onboard::ceremony::{Ceremony, Via};
use crate::screens::wallet::onboard::flow::{h, Failure};
use crate::screens::wallet::onboard::pin_account::{self as pa, Effect, Step};
use crate::screens::wallet::kit::page;
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::service;
use crate::state::State;
use rare_wallet::passkey_account::{self, Ctap, Via as WVia};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

type Rx<T> = Option<Receiver<Result<T, String>>>;
type Ran = (u64, Result<String, Failure>);

pub struct Flow {
    key: u64,
    st: pa::State,
    /// the backup the ceremony on screen is for (the worker moves it on)
    via: Arc<Mutex<Via>>,
    ceremony: Ceremony,
    qr: Option<String>,
    seen: u64,
    has_pin: bool,
    kept: String,
    bt: Rx<bool>,
    power: Rx<(bool, bool, Option<String>)>,
    runs: Vec<Receiver<Result<Ran, String>>>,
    /// restore: the typed recovery words (wiped on drop), and whether this SuiPlay lists the account
    words: Option<zeroize::Zeroizing<String>>,
    known: bool,
    /// made by the import page for the PinAccount page it pushes next: take that page's key
    adopt: bool,
}

impl Default for Flow {
    fn default() -> Self {
        Flow { key: 0, st: pa::State::default(), via: Arc::new(Mutex::new(Via::Phone)), ceremony: Ceremony::Idle, qr: None, seen: 0, has_pin: false, kept: String::new(), bt: None, power: None, runs: Vec::new(), words: None, known: false, adopt: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Pa(pa::Msg),
}

fn wvia(v: Via) -> WVia {
    match v {
        Via::Phone => WVia::Phone,
        Via::Usb => WVia::Usb,
    }
}

const FIXTURE_QR: &str = "FIDO:/123456789012345678901234567890123456789012345678901234567890";

impl Flow {
    /// A fresh page. Shots pick a step: RR_PIN_STEP=pin|backups|run|key|keypin|done|enter.
    fn initial(fixture: bool) -> Flow {
        let mut f = Flow::default();
        if let Some(w) = service::wallet().filter(|_| !fixture) {
            f.has_pin = !w.keystore.pin_accounts().unwrap_or_default().is_empty();
            f.kept = w.keystore.pin_device().map(|p| p.place().describe().to_string()).unwrap_or_default();
        } else {
            f.kept = "Checked by this SuiPlay's security chip".into();
        }
        if fixture {
            f.st.restore = std::env::var("RR_PIN_RESTORE").is_ok();
            match std::env::var("RR_PIN_STEP").as_deref() {
                Ok("backups") => f.st.step = Step::Backups,
                Ok("run") => {
                    f.st.step = Step::Run;
                    f.ceremony = Ceremony::Qr { url: FIXTURE_QR.into() };
                    f.qr = Some(FIXTURE_QR.into());
                }
                Ok("key") => {
                    f.st.step = Step::Run;
                    *f.via.lock().unwrap() = Via::Usb;
                    f.ceremony = Ceremony::Status { stage: "touch".into(), message: "Touch your security key".into() };
                }
                Ok("keypin") => {
                    f.st.step = Step::Run;
                    *f.via.lock().unwrap() = Via::Usb;
                    f.ceremony = Ceremony::Pin { attempts_left: Some(7) };
                }
                Ok("done") => {
                    f.st.step = Step::Done;
                    f.st.backups = pa::Backups::Both;
                    f.st.address = Some("0x2250ac61c912b3eec61d00989acdc1057ee3fbd688cdc577f5a5224d1163d34e".into());
                }
                Ok("enter") => f.has_pin = true,
                _ => {}
            }
        }
        f
    }

    /// Restore from the 12 words (the import page hands them over).
    pub fn restore(words: zeroize::Zeroizing<String>, fixture: bool) -> Flow {
        let mut f = Flow::initial(fixture);
        f.st.restore = true;
        if let (Some(w), Ok(kp)) = (service::wallet().filter(|_| !fixture), rare_wallet::keys::Keypair::from_mnemonic(&words, 0)) {
            let dk = kp.public_key_b64();
            f.known = w.keystore.list_accounts().unwrap_or_default().iter().any(|a| a.passkey.as_ref().is_some_and(|p| p.device_key == dk));
        }
        f.words = Some(words);
        f.adopt = true;
        f
    }

    fn props(&self) -> pa::Props<'_> {
        pa::Props { has_pin: self.has_pin, via: *self.via.lock().unwrap(), ceremony: &self.ceremony, qr_url: self.qr.as_deref(), kept: &self.kept, known: self.known }
    }
}

impl WalletTab {
    fn pa_fresh(&mut self) -> bool {
        let top = self.stack.top().key;
        if !matches!(self.stack.top().view, WalletView::PinAccount) {
            if self.flows.pin_account.key != 0 {
                self.pa_stop();
                self.flows.pin_account.key = 0;
            }
            return false;
        }
        if self.flows.pin_account.adopt {
            self.flows.pin_account.adopt = false;
            self.flows.pin_account.key = top;
            self.flows.pin_account.seen = service::wallet().map(|w| w.version()).unwrap_or(0);
        }
        if self.flows.pin_account.key != top {
            self.pa_stop();
            let mut f = Flow::initial(self.fixture);
            f.key = top;
            f.seen = service::wallet().map(|w| w.version()).unwrap_or(0);
            self.flows.pin_account = f;
        }
        true
    }

    fn pa_stop(&mut self) {
        let f = &mut self.flows.pin_account;
        if f.st.step == Step::Run && !self.fixture {
            if let Some(w) = service::wallet() {
                w.passkey.cancel();
            }
        }
        f.st.run += 1;
        f.st.chosen = None;
    }

    pub(in crate::screens::wallet) fn pin_account_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let WalletView::PinAccount = v else { return None };
        let tmp;
        let f = if self.flows.pin_account.key == self.stack.top().key {
            &self.flows.pin_account
        } else {
            tmp = Flow::initial(self.fixture);
            &tmp
        };
        let st = &f.st;
        Some(page(pa::title(st), None, None, true, "", vec![pa::view(&f.props(), st)]).map(&Msg::Pa))
    }

    pub(in crate::screens::wallet) fn pin_account_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::PinAccount)
    }

    pub(in crate::screens::wallet) fn pin_account_hints(&self) -> Vec<Hint> {
        match self.flows.pin_account.st.step {
            Step::Pin => vec![h::select(), h::del(), h::ok(), h::back()],
            Step::Done => vec![h::select()],
            _ => vec![h::select(), h::back()],
        }
    }

    pub(in crate::screens::wallet) fn pin_account_handle(&mut self, st: &mut State, m: Msg) {
        if !self.pa_fresh() {
            return;
        }
        let Msg::Pa(m) = m;
        let mut f = std::mem::take(&mut self.flows.pin_account);
        let eff = {
            let via = *f.via.lock().unwrap();
            let pr = pa::Props { has_pin: f.has_pin, via, ceremony: &f.ceremony, qr_url: f.qr.as_deref(), kept: &f.kept, known: f.known };
            pa::update(&pr, &mut f.st, m)
        };
        self.pa_run(&mut f, st, eff);
        self.flows.pin_account = f;
        self.ver += 1;
    }

    fn pa_run(&mut self, f: &mut Flow, st: &mut State, mut eff: Option<Effect>) {
        let mut guard = 0;
        while let Some(e) = eff.take() {
            guard += 1;
            if guard > 8 {
                break;
            }
            eff = self.pa_effect(f, st, e);
        }
    }

    fn pa_effect(&mut self, f: &mut Flow, _st: &mut State, e: Effect) -> Option<Effect> {
        match e {
            Effect::CheckBluetooth => {
                if self.fixture {
                    return pa::bt_ready(&mut f.st, true);
                }
                f.bt = Some(service::work(|w| match w.rt.block_on(rare_wallet::bluetooth::state()) {
                    Ok(s) => s.powered || !s.present,
                    Err(_) => true,
                }));
                None
            }
            Effect::PowerOn => {
                if self.fixture {
                    return pa::bt_result(&mut f.st, true, false, None);
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
            Effect::Start { run, vias, pin } => {
                f.ceremony = Ceremony::Idle;
                f.qr = None;
                *f.via.lock().unwrap() = vias.first().copied().unwrap_or(Via::Phone);
                if self.fixture {
                    f.ceremony = Ceremony::Qr { url: FIXTURE_QR.into() };
                    f.qr = Some(FIXTURE_QR.into());
                    return None;
                }
                let via_now = f.via.clone();
                if let Some(words) = f.words.clone() {
                    let via = vias.first().copied().unwrap_or(Via::Phone);
                    f.runs.push(service::work(move |w| -> Ran {
                        *w.passkey_event.lock().unwrap() = None;
                        *w.passkey_note.lock().unwrap() = None;
                        let mut ctap = Ctap { state: &w.passkey, sink: w.sink(), rt: w.rt.clone() };
                        let res = passkey_account::restore_pin_account(&w.keystore, &mut ctap, &super::passkey::ChainHistory { w }, wvia(via), &words, &pin);
                        w.bump();
                        (run, res.map(|a| a.address).map_err(super::passkey::failure_of))
                    }));
                    return None;
                }
                f.runs.push(service::work(move |w| -> Ran {
                    *w.passkey_event.lock().unwrap() = None;
                    *w.passkey_note.lock().unwrap() = None;
                    let mut ctap = Ctap { state: &w.passkey, sink: w.sink(), rt: w.rt.clone() };
                    let wv: Vec<WVia> = vias.iter().copied().map(wvia).collect();
                    let n = vias.len();
                    let mut step = |i: usize| {
                        *via_now.lock().unwrap() = vias[i];
                        *w.passkey_event.lock().unwrap() = None;
                        *w.passkey_note.lock().unwrap() = (n > 1).then(|| match vias[i] {
                            Via::Phone => format!("Backup {} of {n}: your phone", i + 1),
                            Via::Usb => format!("Backup {} of {n}: your security key", i + 1),
                        });
                        w.bump();
                    };
                    let res = passkey_account::create_pin_account(&w.keystore, &mut ctap, &wv, None, &pin, &mut step);
                    *w.passkey_note.lock().unwrap() = None;
                    w.bump();
                    let out = match res {
                        Ok((acc, kp)) => {
                            let a = acc.clone();
                            std::thread::spawn(move || service::register_pin_account(&a, kp));
                            Ok(acc.address)
                        }
                        Err(e) => Err(super::passkey::failure_of(e)),
                    };
                    (run, out)
                }));
                None
            }
            Effect::CancelCeremony => {
                f.ceremony = Ceremony::Idle;
                f.qr = None;
                f.st.key_pin.clear();
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
            Effect::Move(address) => {
                self.pa_land(&address);
                self.stack.push(WalletView::MoveAll { to: address }, None);
                None
            }
            Effect::Finish(address) => {
                self.pa_land(&address);
                None
            }
            Effect::Exit => {
                self.stack.pop();
                None
            }
        }
    }

    /// The new account is the current one; back to Home.
    fn pa_land(&mut self, address: &str) {
        if let Some(w) = service::wallet().filter(|_| !self.fixture) {
            let _ = w.keystore.select(address);
            w.bump();
        }
        self.wallet_ver = 0;
        self.sync_wallet();
        self.selected = Some(address.to_string());
        crate::screens::wallet::home::page::account_changed(&mut self.home);
        self.stack.reset(Vec::new());
        self.ver += 1;
    }

    pub(in crate::screens::wallet) fn pin_account_tick(&mut self, _st: &mut State) -> bool {
        if !self.pa_fresh() {
            return false;
        }
        let mut f = std::mem::take(&mut self.flows.pin_account);
        let mut changed = false;
        let mut effs: Vec<Effect> = Vec::new();
        if let Some(r) = service::poll(&mut f.bt) {
            effs.extend(pa::bt_ready(&mut f.st, r.unwrap_or(true)));
            changed = true;
        }
        if let Some(r) = service::poll(&mut f.power) {
            let (powered, blocked, msg) = r.unwrap_or((false, false, Some("Could not turn Bluetooth on. Try again.".into())));
            effs.extend(pa::bt_result(&mut f.st, powered, blocked, msg.as_deref()));
            changed = true;
        }
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
            let (run, res) = r.unwrap_or_else(|m| (f.st.run, Err(Failure::msg(&m))));
            effs.extend(pa::ran(&mut f.st, run, res));
        }
        if f.st.step == Step::Run && !self.fixture {
            if let Some(w) = service::wallet() {
                let v = w.version();
                if v != f.seen {
                    f.seen = v;
                    changed = true;
                    let ev = w.passkey_event.lock().unwrap().clone();
                    let c = ev.as_ref().map(super::passkey::ceremony_of).unwrap_or(Ceremony::Idle);
                    if let Ceremony::Qr { url } = &c {
                        f.qr = Some(url.clone());
                    }
                    if !matches!(c, Ceremony::Pin { .. }) {
                        f.st.key_pin.clear();
                    }
                    if ev.is_none() {
                        f.qr = None;
                    }
                    f.ceremony = c;
                }
            }
        }
        for e in effs {
            self.pa_run(&mut f, _st, Some(e));
        }
        if f.st.step == Step::Done {
            // the words did their job
            f.words = None;
        }
        let busy = f.bt.is_some() || f.power.is_some() || !f.runs.is_empty() || f.st.step == Step::Run;
        self.flows.pin_account = f;
        if changed {
            self.ver += 1;
        }
        busy
    }
}
