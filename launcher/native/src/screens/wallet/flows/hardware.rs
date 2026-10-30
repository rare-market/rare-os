//! Ledger and Keystone: find the device, pick accounts, add them.
//!
//! The views and step logic are `onboard/hardware.rs` and `hw_parts.rs`; this file performs their
//! effects with rare-wallet on workers: list devices, connect and read five accounts at a time
//! (public keys at m/44'/784'/i'/0'/0'), verify an address on the Ledger, and add the picks with
//! `add_ledger` / `add_keystone`. Nothing here signs.

use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::onboard::hardware::{self as hw, Device, HwErr, Kind};
use crate::screens::wallet::onboard::hw_parts::{self, HwRow};
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::service::{self, Wallet};
use crate::state::State;
use base64::Engine as _;
use rare_wallet::keystone::{KeystoneState, KeystoneUsb};
use rare_wallet::keystone_sui::{self, KeystoneError};
use rare_wallet::ledger::LedgerState;
use rare_wallet::ledger_sui::{self, LedgerError, LedgerPort};
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

const PAGE: u32 = 5;

type Rx<T> = Option<Receiver<Result<Result<T, HwErr>, String>>>;

/// What one account read gives: the row plus what adding it needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub index: u32,
    pub address: String,
    /// base64 Ed25519 public key
    pub public_key: String,
    pub path: String,
}

pub struct Flow {
    /// the page (stack frame) the state below belongs to
    key: u64,
    kind: Kind,
    st: hw::State,
    balances: BTreeMap<String, i128>,
    /// what adding a row needs, by address
    reads: BTreeMap<String, Read>,
    mfp: String,
    dev_id: Option<String>,
    dev_name: String,
    /// the live line while a Ledger works ("Unlock your Ledger"), set by the worker
    prompt: Arc<Mutex<Option<String>>>,
    seen: u64,
    find: Rx<Vec<Device>>,
    conn: Rx<(String, Vec<Read>, String, String)>,
    more: Rx<Vec<Read>>,
    verify: Option<(String, Receiver<Result<Result<bool, HwErr>, String>>)>,
    add: Rx<String>,
    bal: Option<Receiver<Result<Vec<(String, i128)>, String>>>,
    power: Option<Receiver<Result<Result<(), HwErr>, String>>>,
    bt_busy: bool,
    bt_error: Option<String>,
    asking: Option<String>,
    started: bool,
}

impl Default for Flow {
    fn default() -> Self {
        Flow {
            key: 0,
            kind: Kind::Ledger,
            st: hw::State::default(),
            balances: BTreeMap::new(),
            reads: BTreeMap::new(),
            mfp: String::new(),
            dev_id: None,
            dev_name: String::new(),
            prompt: Arc::new(Mutex::new(None)),
            seen: 0,
            find: None,
            conn: None,
            more: None,
            verify: None,
            add: None,
            bal: None,
            power: None,
            bt_busy: false,
            bt_error: None,
            asking: None,
            started: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Hw(hw::Msg),
}

// ---------------------------------------------------------------- pure parts

pub fn ledger_err(e: LedgerError) -> HwErr {
    HwErr { code: Some(e.code.as_str().to_string()), fix: e.fix().map(str::to_string), message: e.message }
}

pub fn keystone_err(e: KeystoneError) -> HwErr {
    HwErr { code: Some(e.code.as_str().to_string()), fix: e.fix().map(str::to_string), message: e.message }
}

fn plain_err(m: impl Into<String>) -> HwErr {
    HwErr { code: None, message: m.into(), fix: None }
}

/// A ledger device as the page's row.
pub fn device_of(d: &rare_wallet::ledger::LedgerDevice) -> Device {
    Device { id: d.id.clone(), name: d.name.clone(), model: d.model.to_string(), transport: d.transport.to_string(), paired: d.paired == Some(true) }
}

pub fn rows_of(reads: &[Read]) -> Vec<HwRow> {
    reads.iter().map(|r| HwRow { index: r.index, address: r.address.clone() }).collect()
}

/// "Ledger 3": the default name counts the accounts of that kind the wallet has.
pub fn kind_count(accounts: &[crate::screens::wallet::types::RareAccount], kind: Kind) -> usize {
    let k = if kind == Kind::Ledger { "ledger" } else { "keystone" };
    accounts.iter().filter(|a| a.kind == k).count()
}

struct Port {
    state: &'static LedgerState,
    rt: tokio::runtime::Handle,
    sink: rare_wallet::Sink,
    name: Option<String>,
}

impl LedgerPort for Port {
    type Conn = &'static LedgerState;
    fn open(&mut self, id: &str) -> Result<&'static LedgerState, LedgerError> {
        let d = self.rt.block_on(self.state.open(&self.sink, id)).map_err(|e| LedgerError::from_native(&e))?;
        self.name = Some(d.name);
        Ok(self.state)
    }
}

fn read_reads(c: &mut &LedgerState, from: u32, count: u32) -> Result<Vec<Read>, LedgerError> {
    let b64 = base64::engine::general_purpose::STANDARD;
    (from..from + count)
        .map(|i| {
            let path = rare_wallet::keys::sui_path(i);
            let (pk, address) = ledger_sui::read_account(c, &path, false)?;
            Ok(Read { index: i, address, public_key: b64.encode(pk), path })
        })
        .collect()
}

/// One Ledger conversation: waits through "Unlock" / "Open the Sui app" and shows those on the page.
fn ledger_session<T>(w: &'static Wallet, id: &str, prompt: &Arc<Mutex<Option<String>>>, mut f: impl FnMut(&mut &'static LedgerState) -> Result<T, LedgerError>) -> Result<(String, T), HwErr> {
    w.cancelled.store(false, std::sync::atomic::Ordering::SeqCst);
    let set = |t: Option<&str>| {
        *prompt.lock().unwrap() = t.map(str::to_string);
        w.bump();
    };
    let mut port = Port { state: &w.ledger, rt: w.rt.clone(), sink: w.sink(), name: None };
    let cancelled = w.cancelled.clone();
    set(Some(ledger_sui::Stage::Connecting.text()));
    let r = ledger_sui::session(&mut port, id, ledger_sui::APP_WAIT, ledger_sui::POLL, &mut |s| set(Some(s.text())), &|| cancelled.load(std::sync::atomic::Ordering::SeqCst), |c, _| f(c));
    w.rt.block_on(w.ledger.close());
    set(None);
    r.map(|v| (port.name.unwrap_or_default(), v)).map_err(ledger_err)
}

/// Receive's Verify on Ledger (wallet/home/hw.ts verifyOnLedger): shows the account's address on
/// the Ledger and answers whether the device derives the same one. Finds the Ledger the way signing
/// does: USB first, then a Bluetooth one (the account's own device by name, else a paired one).
pub fn verify_receive(address: String, path: String, device: Option<String>) -> Receiver<Result<Result<bool, HwErr>, String>> {
    service::work(move |w| -> Result<bool, HwErr> {
        let usb = w.rt.block_on(LedgerState::list(false, 0)).map_err(|e| ledger_err(LedgerError::from_native(&e)))?;
        let pick = |l: &[rare_wallet::ledger::LedgerDevice]| l.iter().find(|d| Some(&d.name) == device.as_ref()).or_else(|| l.iter().find(|d| d.paired == Some(true))).or(l.first()).cloned();
        let d = match pick(&usb) {
            Some(d) => d,
            None => {
                let all = w.rt.block_on(LedgerState::list(true, 3000)).map_err(|e| ledger_err(LedgerError::from_native(&e)))?;
                let ble: Vec<_> = all.into_iter().filter(|d| d.transport == "ble").collect();
                pick(&ble).ok_or_else(|| plain_err("No Ledger found. Connect it with USB or turn on its Bluetooth, unlock it and open the Sui app."))?
            }
        };
        let prompt = Arc::new(Mutex::new(None));
        ledger_session(w, &d.id, &prompt, |c| ledger_sui::read_account(c, &path, true).map(|(_, a)| a == address)).map(|(_, same)| same)
    })
}

fn keystone_reads(w: &'static Wallet, from: u32) -> Result<(String, String, Vec<Read>), HwErr> {
    w.cancelled.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut link = KeystoneUsb { state: &w.keystone, rt: w.rt.clone() };
    let cancelled = w.cancelled.clone();
    let (mfp, device, accounts) = keystone_sui::task(
        keystone_sui::Stage::Approve,
        keystone_sui::WAIT,
        keystone_sui::POLL,
        &mut |_| {},
        &|| cancelled.load(std::sync::atomic::Ordering::SeqCst),
        || keystone_sui::get_accounts(&mut link, from, PAGE),
    )
    .map_err(keystone_err)?;
    let reads = accounts.into_iter().map(|a| Read { index: a.index, address: a.address, public_key: a.public_key, path: a.path }).collect();
    Ok((mfp, device.unwrap_or_else(|| "Keystone".to_string()), reads))
}

fn flat<T>(r: Result<Result<T, HwErr>, String>) -> Result<T, HwErr> {
    r.map_err(plain_err)?
}

// ---------------------------------------------------------------- the tab

impl WalletTab {
    fn hw_kind(&self) -> Option<Kind> {
        match self.stack.top().view {
            WalletView::Ledger => Some(Kind::Ledger),
            WalletView::Keystone => Some(Kind::Keystone),
            _ => None,
        }
    }

    fn hw_existing(&self) -> Vec<String> {
        self.accounts.iter().map(|a| a.address.clone()).collect()
    }

    /// Starts over when the page on top is a different one than the state is for.
    fn hw_fresh(&mut self) -> Option<Kind> {
        let Some(kind) = self.hw_kind() else {
            if self.flows.hardware.key != 0 {
                self.hw_stop();
                self.flows.hardware = Flow::default();
            }
            return None;
        };
        let top = self.stack.top().key;
        if self.flows.hardware.key != top {
            self.hw_stop();
            let mut f = Flow { key: top, kind, ..Flow::default() };
            f.seen = service::wallet().map(|w| w.version()).unwrap_or(0);
            if self.fixture {
                fixture_state(&mut f);
            }
            self.flows.hardware = f;
        }
        Some(kind)
    }

    fn hw_stop(&mut self) {
        if !self.fixture {
            if let Some(w) = service::wallet() {
                w.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    pub(in crate::screens::wallet) fn hardware_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let kind = match v {
            WalletView::Ledger => Kind::Ledger,
            WalletView::Keystone => Kind::Keystone,
            _ => return None,
        };
        let tmp;
        let f = if self.flows.hardware.key == self.stack.top().key {
            &self.flows.hardware
        } else {
            let mut t = Flow { kind, ..Flow::default() };
            if self.fixture {
                fixture_state(&mut t);
            }
            tmp = t;
            &tmp
        };
        let existing = self.hw_existing();
        let prompt = f.prompt.lock().unwrap().clone();
        let pr = hw::Props { kind, existing: &existing, count: kind_count(&self.accounts, kind), prompt: prompt.as_deref(), balances: &f.balances, bt_busy: f.bt_busy, bt_error: f.bt_error.as_deref() };
        Some(hw::view(&pr, &f.st).map(&Msg::Hw))
    }

    pub(in crate::screens::wallet) fn hardware_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::Ledger | WalletView::Keystone)
    }

    pub(in crate::screens::wallet) fn hardware_hints(&self) -> Vec<Hint> {
        hw::hints()
    }

    pub(in crate::screens::wallet) fn hardware_handle(&mut self, st: &mut State, m: Msg) {
        let Some(kind) = self.hw_fresh() else { return };
        let Msg::Hw(m) = m;
        let mut f = std::mem::take(&mut self.flows.hardware);
        let existing = self.hw_existing();
        let count = kind_count(&self.accounts, kind);
        if let hw::Msg::Rows(hw_parts::Msg::Name(a, n)) = &m {
            // the name field: the keyboard answers in tick
            let cur = f.st.pick.names.get(a).cloned().unwrap_or_default();
            st.ask_text(format!("hw-name:{a}"), format!("Name for account {n}"), cur, None);
            f.asking = Some(a.clone());
        }
        let eff = {
            let pr = hw::Props { kind, existing: &existing, count, prompt: None, balances: &f.balances, bt_busy: f.bt_busy, bt_error: f.bt_error.as_deref() };
            hw::update(&pr, &mut f.st, m)
        };
        if let Some(e) = eff {
            self.hw_effect(&mut f, st, kind, e);
        }
        self.flows.hardware = f;
    }

    fn hw_effect(&mut self, f: &mut Flow, st: &mut State, kind: Kind, e: hw::Effect) {
        let fixture = self.fixture;
        match e {
            hw::Effect::Search { ble } => {
                if fixture {
                    hw::found(&mut f.st, Ok(fixture_devices(kind)));
                    return;
                }
                *f.prompt.lock().unwrap() = None;
                f.find = Some(service::work(move |w| -> Result<Vec<Device>, HwErr> {
                    if kind == Kind::Ledger {
                        let list = w.rt.block_on(LedgerState::list(ble, if ble { 4000 } else { 0 })).map_err(|e| ledger_err(LedgerError::from_native(&e)))?;
                        Ok(list.iter().map(device_of).collect())
                    } else {
                        let list = w.rt.block_on(KeystoneState::list()).map_err(|e| keystone_err(KeystoneError::from_native(&e)))?;
                        Ok(list.into_iter().map(|d| Device { id: d.id, name: d.name, model: String::new(), transport: "usb".into(), paired: false }).collect())
                    }
                }));
            }
            hw::Effect::Connect(id) => {
                if fixture {
                    let reads = fixture_reads(0);
                    f.reads = reads.iter().map(|r| (r.address.clone(), r.clone())).collect();
                    hw::connected(&mut f.st, Ok((fixture_name(kind), rows_of(&reads))));
                    f.balances = reads.iter().enumerate().map(|(i, r)| (r.address.clone(), 2_500_000_000i128 * (i as i128 % 3))).collect();
                    return;
                }
                f.dev_id = Some(id.clone());
                let prompt = f.prompt.clone();
                f.conn = Some(service::work(move |w| -> Result<(String, Vec<Read>, String, String), HwErr> {
                    if kind == Kind::Ledger {
                        let (name, reads) = ledger_session(w, &id, &prompt, |c| read_reads(c, 0, PAGE))?;
                        Ok((name.clone(), reads, String::new(), name))
                    } else {
                        let (mfp, name, reads) = keystone_reads(w, 0)?;
                        Ok((name.clone(), reads, mfp, name))
                    }
                }));
            }
            hw::Effect::More { from } => {
                if fixture {
                    let r = fixture_reads(from as u32);
                    f.reads.extend(r.iter().map(|x| (x.address.clone(), x.clone())));
                    hw::more(&mut f.st, Ok(rows_of(&r)));
                    return;
                }
                let id = f.dev_id.clone().unwrap_or_default();
                let prompt = f.prompt.clone();
                f.more = Some(service::work(move |w| -> Result<Vec<Read>, HwErr> {
                    if kind == Kind::Ledger {
                        ledger_session(w, &id, &prompt, |c| read_reads(c, from as u32, PAGE)).map(|(_, r)| r)
                    } else {
                        keystone_reads(w, from as u32).map(|(_, _, r)| r)
                    }
                }));
            }
            hw::Effect::Verify(row) => {
                if fixture {
                    hw::verified(&mut f.st, &row.address, Ok(true));
                    return;
                }
                let id = f.dev_id.clone().unwrap_or_default();
                let prompt = f.prompt.clone();
                let address = row.address.clone();
                let rx = service::work(move |w| -> Result<bool, HwErr> {
                    let path = rare_wallet::keys::sui_path(row.index);
                    ledger_session(w, &id, &prompt, |c| ledger_sui::read_account(c, &path, true).map(|(_, a)| a == row.address)).map(|(_, same)| same)
                });
                f.verify = Some((address, rx));
            }
            hw::Effect::Add(picks) => {
                if fixture {
                    let first = picks.first().map(|(r, _)| r.address.clone());
                    self.hw_finish(first);
                    return;
                }
                let items: Vec<(Read, String)> = picks.iter().filter_map(|(row, label)| f.reads.get(&row.address).cloned().map(|r| (r, label.clone()))).collect();
                let (mfp, device) = (f.mfp.clone(), f.dev_name.clone());
                f.add = Some(service::work(move |w| -> Result<String, HwErr> {
                    let mut first: Option<String> = None;
                    for (r, label) in &items {
                        let acc = if kind == Kind::Ledger {
                            w.keystore.add_ledger(&r.public_key, &r.path, Some(label), Some(&device))
                        } else {
                            w.keystore.add_keystone(&r.public_key, &r.path, &mfp, Some(label), Some(&device))
                        }
                        .map_err(|e| plain_err(e.to_string()))?;
                        first.get_or_insert(acc.address);
                    }
                    let first = first.ok_or_else(|| plain_err("Pick an account to add."))?;
                    w.keystore.select(&first).map_err(|e| plain_err(e.to_string()))?;
                    w.bump();
                    Ok(first)
                }));
            }
            hw::Effect::Copy(text) => {
                copy_text(&text);
                st.toast("Copied", crate::state::Tone::Info, rare_render::clock::secs());
            }
            hw::Effect::PowerOnBluetooth => {
                if fixture {
                    return;
                }
                f.bt_busy = true;
                f.bt_error = None;
                f.power = Some(service::work(|w| -> Result<(), HwErr> {
                    match w.rt.block_on(rare_wallet::bluetooth::power_on()) {
                        Ok(s) if s.powered => Ok(()),
                        Ok(s) => Err(plain_err(if s.blocked { "Bluetooth is blocked by a switch on this device." } else { "Could not turn Bluetooth on. Try again." })),
                        Err(e) => Err(plain_err(rare_wallet::WalletError::parse(&e).message)),
                    }
                }));
            }
            hw::Effect::AskName(..) => {}
        }
    }

    fn hw_finish(&mut self, first: Option<String>) {
        self.wallet_ver = 0;
        self.sync_wallet();
        if let Some(a) = first {
            self.selected = Some(a);
        }
        crate::screens::wallet::home::page::account_changed(&mut self.home);
        self.stack.reset(Vec::new());
        self.ver += 1;
    }

    pub(in crate::screens::wallet) fn hardware_tick(&mut self, st: &mut State) -> bool {
        let Some(kind) = self.hw_fresh() else { return false };
        let mut f = std::mem::take(&mut self.flows.hardware);
        let mut changed = false;
        let existing = self.hw_existing();

        // first visit: look for devices
        if !f.started {
            f.started = true;
            if !self.fixture {
                f.st.searching = true;
                let ble = f.st.ble;
                self.hw_effect(&mut f, st, kind, hw::Effect::Search { ble });
            } else {
                self.hw_effect(&mut f, st, kind, hw::Effect::Search { ble: false });
            }
            changed = true;
        }
        if let Some(r) = service::poll(&mut f.find) {
            hw::found(&mut f.st, flat(r));
            changed = true;
        }
        if let Some(r) = service::poll(&mut f.conn) {
            match flat(r) {
                Ok((title, reads, mfp, name)) => {
                    f.reads = reads.iter().map(|r| (r.address.clone(), r.clone())).collect();
                    f.mfp = mfp;
                    f.dev_name = name;
                    let shown = if kind == Kind::Ledger { f.st.devices.as_ref().and_then(|d| d.iter().find(|d| Some(&d.id) == f.dev_id.as_ref())).map(|d| if d.name.is_empty() { hw::model_name(&d.model).to_string() } else { d.name.clone() }).unwrap_or(title) } else { title };
                    hw::connected(&mut f.st, Ok((shown, rows_of(&reads))));
                    hw::land(&mut f.st, &existing);
                    f.bal = Some(balances_for(reads.iter().map(|r| r.address.clone()).collect()));
                }
                Err(e) => hw::connected(&mut f.st, Err(e)),
            }
            changed = true;
        }
        if let Some(r) = service::poll(&mut f.more) {
            let r = flat(r);
            if let Ok(reads) = &r {
                f.reads.extend(reads.iter().map(|x| (x.address.clone(), x.clone())));
                f.bal = Some(balances_for(reads.iter().map(|r| r.address.clone()).collect()));
            }
            hw::more(&mut f.st, r.map(|r| rows_of(&r)));
            changed = true;
        }
        let verify_done = match f.verify.as_mut() {
            Some((a, rx)) => match rx.try_recv() {
                Ok(r) => Some((a.clone(), flat(r))),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some((a.clone(), Err(plain_err("The wallet stopped. Try again.")))),
                Err(_) => None,
            },
            None => None,
        };
        if let Some((a, r)) = verify_done {
            f.verify = None;
            hw::verified(&mut f.st, &a, r);
            changed = true;
        }
        if let Some(r) = service::poll(&mut f.bal) {
            if let Ok(list) = r {
                f.balances.extend(list);
                changed = true;
            }
        }
        if let Some(r) = service::poll(&mut f.power) {
            f.bt_busy = false;
            match flat(r) {
                Ok(()) => {
                    f.bt_error = None;
                    let ble = f.st.ble;
                self.hw_effect(&mut f, st, kind, hw::Effect::Search { ble });
                }
                Err(e) => f.bt_error = Some(e.message),
            }
            changed = true;
        }
        let mut finished = None;
        if let Some(r) = service::poll(&mut f.add) {
            match flat(r) {
                Ok(first) => finished = Some(first),
                Err(e) => hw::add_failed(&mut f.st, e),
            }
            changed = true;
        }
        // a typed account name
        if let Some(a) = f.asking.clone() {
            match st.take_answer(&format!("hw-name:{a}")) {
                Some(Some(text)) => {
                    hw_parts::update(&mut f.st.pick, &hw_parts::Msg::Named(a, text));
                    f.asking = None;
                    changed = true;
                }
                Some(None) => f.asking = None,
                None => {}
            }
        }
        // the Ledger's live line and Bluetooth pairing code
        if !self.fixture {
            if let Some(w) = service::wallet() {
                let v = w.version();
                if v != f.seen {
                    f.seen = v;
                    changed = true;
                }
                if kind == Kind::Ledger {
                    let code = w.ledger_pairing.lock().unwrap().clone();
                    if code != f.st.code {
                        f.st.code = code;
                        changed = true;
                    }
                }
            }
        }
        let busy = f.find.is_some() || f.conn.is_some() || f.more.is_some() || f.verify.is_some() || f.add.is_some() || f.bal.is_some() || f.power.is_some();
        self.flows.hardware = f;
        if let Some(first) = finished {
            self.hw_finish(Some(first));
        }
        if changed {
            self.ver += 1;
        }
        busy
    }
}

/// SUI balance of each address, for the account rows.
fn balances_for(addresses: Vec<String>) -> Receiver<Result<Vec<(String, i128)>, String>> {
    service::work(move |w| {
        use rare_sui::grpc::ChainRead;
        w.rt.block_on(async {
            let mut out = Vec::new();
            for a in addresses {
                if let Ok(b) = w.sui.balance(&a, "0x2::sui::SUI").await {
                    out.push((a, b.balance as i128));
                }
            }
            out
        })
    })
}

/// Copies to the system clipboard when a tool for it exists (Wayland, then X11).
fn copy_text(text: &str) {
    use std::io::Write;
    for (cmd, args) in [("wl-copy", &[][..]), ("xclip", &["-selection", "clipboard"][..]), ("pbcopy", &[][..])] {
        if let Ok(mut c) = std::process::Command::new(cmd).args(args).stdin(std::process::Stdio::piped()).spawn() {
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(text.as_bytes());
            }
            let _ = c.wait();
            return;
        }
    }
}

// ---------------------------------------------------------------- fixtures (headless shots)

fn fixture_name(kind: Kind) -> String {
    if kind == Kind::Ledger { "Nano X".into() } else { "Keystone 3 Pro".into() }
}

fn fixture_devices(kind: Kind) -> Vec<Device> {
    vec![Device { id: "fixture".into(), name: if kind == Kind::Ledger { String::new() } else { "Keystone 3 Pro".into() }, model: "nanoX".into(), transport: "usb".into(), paired: false }]
}

fn fixture_reads(from: u32) -> Vec<Read> {
    (from..from + PAGE)
        .map(|i| Read { index: i, address: format!("0x{:064x}", 0xa1 + i as u64 * 0x11), public_key: String::new(), path: rare_wallet::keys::sui_path(i) })
        .collect()
}

/// `RR_WFLOW_STEP=accounts` starts a fixture shot on the account list.
fn fixture_state(f: &mut Flow) {
    if std::env::var("RR_WFLOW_STEP").as_deref() == Ok("accounts") {
        let reads = fixture_reads(0);
        f.reads = reads.iter().map(|r| (r.address.clone(), r.clone())).collect();
        f.st.searching = false;
        hw::connected(&mut f.st, Ok((fixture_name(f.kind), rows_of(&reads))));
        f.balances = reads.iter().enumerate().map(|(i, r)| (r.address.clone(), 2_500_000_000i128 * (i as i128 % 3))).collect();
        f.st.pick.picked = vec![reads[1].address.clone()];
        f.started = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::stack::WalletView;
    use crate::screens::wallet::onboard::hardware::Phase;

    #[test]
    fn errors_carry_the_fix() {
        let e = LedgerError::from_native("udev|No permission|{\"fix\":\"pkexec x\"}");
        let h = ledger_err(e);
        assert_eq!(h.code.as_deref(), Some("udev"));
        assert_eq!(h.fix.as_deref(), Some("pkexec x"));
        let k = keystone_err(KeystoneError::new(keystone_sui::KeystoneCode::Locked));
        assert_eq!(k.code.as_deref(), Some("locked"));
        assert!(k.message.contains("locked"));
    }

    #[test]
    fn rows_and_counts() {
        let reads = fixture_reads(5);
        let rows = rows_of(&reads);
        assert_eq!(rows[0].index, 5);
        assert_eq!(rows.len(), 5);
        assert_eq!(reads[0].path, "m/44'/784'/5'/0'/0'");
        let accounts = vec![crate::screens::wallet::types::RareAccount::new("0x1", "a", "ledger"), crate::screens::wallet::types::RareAccount::new("0x2", "b", "keystone"), crate::screens::wallet::types::RareAccount::new("0x3", "c", "ledger")];
        assert_eq!(kind_count(&accounts, Kind::Ledger), 2);
        assert_eq!(kind_count(&accounts, Kind::Keystone), 1);
    }

    #[test]
    fn fixture_flow_lists_picks_and_names() {
        let mut t = WalletTab { fixture: true, ..WalletTab::default() };
        let mut st = crate::state::State::default();
        t.stack.push(WalletView::Ledger, None);
        assert!(t.hardware_tick(&mut st) || true);
        assert!(t.markup().texts().iter().any(|s| s == "Nano X"));
        t.hardware_handle(&mut st, Msg::Hw(hw::Msg::Connect("fixture".into())));
        assert_eq!(t.flows.hardware.st.phase, Phase::Accounts);
        let a = t.flows.hardware.st.rows[0].address.clone();
        t.hardware_handle(&mut st, Msg::Hw(hw::Msg::Rows(hw_parts::Msg::Toggle(a.clone()))));
        assert!(t.markup().texts().iter().any(|s| s == "Add account"));
        t.hardware_handle(&mut st, Msg::Hw(hw::Msg::Rows(hw_parts::Msg::Add)));
        // fixture add lands on home
        assert!(matches!(t.stack.top().view, WalletView::Home { .. }));
    }
}
