//! Adding Ledger and Keystone accounts (launcher/src/wallet/ui/pages/Ledger.tsx, Keystone.tsx): find
//! the device, read its accounts (address and SUI balance), verify an address on the Ledger,
//! pick one or several, name them, add.

use super::ceremony::{bluetooth_view, BtMsg};
use super::flow::h;
use super::hw_parts::{self, hw_error, ErrMsg, HwRow, Pick, Verify};
use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::kit::{btn, empty, list_button, page, panel, segmented, Variant};
use crate::screens::wallet::markup::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// "nanoS" | "nanoSP" | "nanoX" | "stax" | "flex" | "unknown" (Ledger)
    pub model: String,
    /// "usb" | "ble"
    pub transport: String,
    pub paired: bool,
}

pub fn model_name(m: &str) -> &'static str {
    match m {
        "nanoS" => "Nano S",
        "nanoSP" => "Nano S Plus",
        "nanoX" => "Nano X",
        "stax" => "Stax",
        "flex" => "Flex",
        _ => "Ledger",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Find,
    /// Ledger "connecting", Keystone "reading"
    Connecting,
    Accounts,
}

/// A device error (code "udev" carries the fix; "ble-off" asks for Bluetooth on the Ledger page).
#[derive(Clone, Debug, PartialEq)]
pub struct HwErr {
    pub code: Option<String>,
    pub message: String,
    pub fix: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub ble: bool,
    pub phase: Phase,
    /// None while searching the first time
    pub devices: Option<Vec<Device>>,
    pub searching: bool,
    pub rows: Vec<HwRow>,
    /// the connected Ledger's name, or the Keystone's
    pub model: String,
    pub err: Option<HwErr>,
    pub more: bool,
    pub adding: bool,
    pub verified: BTreeMap<String, Verify>,
    /// the Bluetooth pairing code to compare
    pub code: Option<String>,
    pub pick: Pick,
    pub copied: bool,
}

impl Default for State {
    fn default() -> Self {
        State { ble: false, phase: Phase::Find, devices: None, searching: true, rows: Vec::new(), model: String::new(), err: None, more: false, adding: false, verified: BTreeMap::new(), code: None, pick: Pick::default(), copied: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Transport(bool),
    Connect(String),
    Search,
    Err(ErrMsg),
    Bt(BtMsg),
    Rows(hw_parts::Msg),
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ledger,
    Keystone,
}

pub struct Props<'a> {
    pub kind: Kind,
    /// addresses already in the wallet
    pub existing: &'a [String],
    /// how many accounts of this kind the wallet has ("Ledger 3")
    pub count: usize,
    /// the Ledger prompt's live line while connecting
    pub prompt: Option<&'a str>,
    pub balances: &'a BTreeMap<String, i128>,
    pub bt_busy: bool,
    pub bt_error: Option<&'a str>,
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let ledger = pr.kind == Kind::Ledger;
    let accounts = st.phase == Phase::Accounts;
    let (title, sub) = if ledger {
        (if accounts { "Pick Ledger accounts" } else { "Connect your Ledger" }, if accounts { (!st.model.is_empty()).then(|| st.model.clone()) } else { Some("Unlock your Ledger and open the Sui app.".to_string()) })
    } else {
        (
            if accounts { "Pick Keystone accounts" } else { "Connect your Keystone" },
            if accounts { (!st.model.is_empty()).then(|| st.model.clone()) } else { Some("Connect your Keystone 3 Pro with a USB-C cable. On the Keystone: Settings > Connection > USB.".to_string()) },
        )
    };
    let mut kids = Vec::new();
    if st.searching || st.phase == Phase::Connecting {
        kids.push(div("pointer-events-none absolute left-0 right-0 top-0 z-30 h-[2px] load-bar"));
    }
    if !accounts {
        let mut d = div("flex flex-col gap-5");
        if ledger {
            d = d.kid(
                div("")
                    .kid(div("mb-1.5 text-sm text-secondary").text("How is it connected"))
                    .kid(segmented(&[("usb", "USB cable"), ("ble", "Bluetooth")], if st.ble { "ble" } else { "usb" }, |v| Msg::Transport(v == "ble"), ""))
                    .kid(p("mt-2 text-sm text-secondary").text(if st.ble { "Nano X, Stax and Flex. Turn Bluetooth on from the Ledger's settings." } else { "Plug it in with a USB-C or USB cable." })),
            );
            if let Some(c) = st.code.as_ref().filter(|_| st.phase == Phase::Connecting || st.searching) {
                d = d.kid(panel(
                    None,
                    None,
                    "px-5 py-4 text-center",
                    vec![div("text-sm text-secondary").text("Pairing code"), div("my-1 font-mono text-3xl tracking-widest").text(c), div("text-sm text-secondary").text("Check it matches your Ledger and confirm there.")],
                ));
            }
        }
        let body = if st.phase == Phase::Connecting {
            Some(if ledger {
                empty(pr.prompt.filter(|m| !m.is_empty()).unwrap_or("Connecting to your Ledger"), Some(text("Keep it unlocked with the Sui app open.")), None, true)
            } else {
                empty("Reading your Keystone", Some(text("Keep it unlocked and on the USB screen.")), None, true)
            })
        } else if let Some(e) = st.err.as_ref().filter(|e| ledger && st.ble && e.code.as_deref() == Some("ble-off")) {
            let _ = e;
            Some(bluetooth_view(Some("Turn Bluetooth on to connect your Ledger over Bluetooth."), pr.bt_busy, pr.bt_error, false).map(&Msg::Bt))
        } else if let Some(e) = &st.err {
            let fix = if ledger { e.fix.as_deref().filter(|_| e.code.as_deref() == Some("udev")) } else { e.fix.as_deref() };
            Some(hw_error(&e.message, fix, st.copied, true).map(&Msg::Err))
        } else if let Some(devs) = st.devices.as_ref().filter(|d| !d.is_empty()) {
            Some(panel(
                None,
                None,
                "divide-y divide-border-primary p-1",
                devs.iter()
                    .enumerate()
                    .map(|(i, d)| {
                        let (t, b) = if ledger {
                            (if d.name.is_empty() { model_name(&d.model).to_string() } else { d.name.clone() }, if d.transport == "ble" { if d.paired { "Bluetooth, paired" } else { "Bluetooth" } } else { "USB" })
                        } else {
                            (d.name.clone(), "USB")
                        };
                        list_button(None, &t, Some(text(b)), Some(text("Connect")), Msg::Connect(d.id.clone()), i == 0, "").key(&d.id)
                    })
                    .collect(),
            ))
        } else if st.devices.is_some() && !st.searching {
            Some(empty(
                if ledger { "No Ledger found" } else { "No Keystone found" },
                Some(text(if !ledger {
                    "Plug it in, unlock it and turn on USB in its connection settings."
                } else if st.ble {
                    "Turn on Bluetooth on your Ledger, unlock it and open the Sui app."
                } else {
                    "Plug it in, unlock it and open the Sui app."
                })),
                Some(btn("Search again", Variant::Secondary, "", Some(Msg::Search)).def(true)),
                true,
            ))
        } else {
            None
        };
        d = d.kid_opt(body);
        if !ledger {
            d = d.kid(p("text-sm text-secondary").text("The Keystone signs a transaction hash over USB. Rare shows you the decoded transaction first, so check it before you approve on the device."));
        }
        kids.push(d);
    } else {
        let base = pr.count;
        let name = move |n: usize| format!("{} {}", if ledger { "Ledger" } else { "Keystone" }, base + n + 1);
        let hp = hw_parts::Props {
            rows: &st.rows,
            existing: pr.existing,
            default_name: &name,
            can_verify: ledger,
            more: st.more,
            adding: st.adding,
            error: st.err.as_ref().map(|e| e.message.as_str()),
            verified: &st.verified,
            balances: pr.balances,
        };
        kids.push(hw_parts::view(&hp, &st.pick).map(&Msg::Rows));
    }
    let mut pg = page(Some(title), sub.map(text), None, true, "", kids);
    if accounts {
        pg = pg.on_pad("b", Msg::Back);
    }
    pg
}

pub fn hints() -> Vec<Hint> {
    vec![h::select(), h::back()]
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// list devices (Ledger: over USB or a 4 s Bluetooth scan); answer with `found`
    Search { ble: bool },
    /// connect (Ledger) or read (Keystone) and list accounts 0..5; answer with `connected`
    Connect(String),
    /// accounts from `from`, five more; answer with `more`
    More { from: usize },
    /// show the address on the Ledger; answer with `verified`
    Verify(HwRow),
    /// add each (index, label); then finishAdd(first)
    Add(Vec<(HwRow, String)>),
    Copy(String),
    PowerOnBluetooth,
    /// A on a name field: keyboard "Name for account N"
    AskName(String, u32),
}

pub fn update(pr: &Props, st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Transport(ble) => {
            st.ble = ble;
            return Some(search(st));
        }
        Msg::Search | Msg::Err(ErrMsg::Retry) => return Some(search(st)),
        Msg::Err(ErrMsg::CopyFix) => {
            st.copied = true;
            return st.err.as_ref().and_then(|e| e.fix.clone()).map(Effect::Copy);
        }
        Msg::Connect(id) => {
            st.err = None;
            st.phase = Phase::Connecting;
            return Some(Effect::Connect(id));
        }
        Msg::Bt(BtMsg::TurnOn) => return Some(Effect::PowerOnBluetooth),
        Msg::Bt(BtMsg::Cancel) => {}
        Msg::Rows(m) => {
            match &m {
                hw_parts::Msg::More => {
                    st.more = true;
                    st.err = None;
                    return Some(Effect::More { from: st.rows.len() });
                }
                hw_parts::Msg::Verify(a) => {
                    let r = st.rows.iter().find(|r| r.address == *a)?.clone();
                    st.verified.insert(a.clone(), Verify::Busy);
                    st.err = None;
                    return Some(Effect::Verify(r));
                }
                hw_parts::Msg::Name(a, n) => return Some(Effect::AskName(a.clone(), *n)),
                hw_parts::Msg::Add => {
                    let base = pr.count;
                    let ledger = pr.kind == Kind::Ledger;
                    let name = move |n: usize| format!("{} {}", if ledger { "Ledger" } else { "Keystone" }, base + n + 1);
                    let hp = hw_parts::Props { rows: &st.rows, existing: pr.existing, default_name: &name, can_verify: false, more: false, adding: false, error: None, verified: &st.verified, balances: pr.balances };
                    let picks = hw_parts::picks(&hp, &st.pick);
                    if picks.is_empty() || st.adding {
                        return None;
                    }
                    st.adding = true;
                    st.err = None;
                    return Some(Effect::Add(picks));
                }
                _ => hw_parts::update(&mut st.pick, &m),
            }
        }
        Msg::Back => st.phase = Phase::Find,
    }
    None
}

fn search(st: &mut State) -> Effect {
    st.searching = true;
    st.err = None;
    st.devices = None;
    Effect::Search { ble: st.ble }
}

/// The device list (filtered to the transport on the Ledger page), or the error.
pub fn found(st: &mut State, res: Result<Vec<Device>, HwErr>) {
    st.searching = false;
    match res {
        Ok(d) => st.devices = Some(d.into_iter().filter(|x| !st.ble || x.transport == "ble").filter(|x| st.ble || x.transport != "ble").collect()),
        Err(e) => st.err = Some(e),
    }
}

/// Connected and read: the device name and its first accounts.
pub fn connected(st: &mut State, res: Result<(String, Vec<HwRow>), HwErr>) {
    match res {
        Ok((name, rows)) => {
            st.model = name;
            st.pick = Pick { first_free: None, ..Pick::default() };
            st.rows = rows;
            st.phase = Phase::Accounts;
        }
        Err(e) => {
            st.err = Some(e);
            st.phase = Phase::Find;
        }
    }
}

/// Call after `connected` with the existing addresses, so focus lands on the first free account.
pub fn land(st: &mut State, existing: &[String]) {
    st.pick.first_free = hw_parts::first_free(&st.rows, existing);
}

pub fn more(st: &mut State, res: Result<Vec<HwRow>, HwErr>) {
    st.more = false;
    match res {
        Ok(r) => st.rows.extend(r),
        Err(e) => st.err = Some(e),
    }
}

pub fn verified(st: &mut State, address: &str, res: Result<bool, HwErr>) {
    match res {
        Ok(same) => {
            st.verified.insert(address.to_string(), if same { Verify::Ok } else { Verify::Bad });
        }
        Err(e) => {
            st.verified.remove(address);
            st.err = Some(e);
        }
    }
}

pub fn add_failed(st: &mut State, e: HwErr) {
    st.err = Some(e);
    st.adding = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(kind: Kind) -> Props<'static> {
        Props { kind, existing: &[], count: 0, prompt: None, balances: Box::leak(Box::default()), bt_busy: false, bt_error: None }
    }

    #[test]
    fn ledger_find() {
        let pr = props(Kind::Ledger);
        let mut st = State::default();
        let v = view(&pr, &st);
        assert_eq!(v.texts(), ["Connect your Ledger", "Unlock your Ledger and open the Sui app.", "How is it connected", "USB cable", "Bluetooth", "Plug it in with a USB-C or USB cable."]);
        found(&mut st, Ok(vec![]));
        let v = view(&pr, &st);
        assert!(v.contains_text("No Ledger found"));
        assert_eq!(v.landing().as_deref(), Some("Search again"));
        assert_eq!(update(&pr, &mut st, Msg::Transport(true)), Some(Effect::Search { ble: true }));
        found(&mut st, Ok(vec![Device { id: "d1".into(), name: String::new(), model: "nanoX".into(), transport: "ble".into(), paired: true }]));
        let v = view(&pr, &st);
        assert!(v.contains_text("Nano X"));
        assert!(v.contains_text("Bluetooth, paired"));
        assert_eq!(update(&pr, &mut st, Msg::Connect("d1".into())), Some(Effect::Connect("d1".into())));
        st.code = Some("123456".into());
        assert!(view(&pr, &st).contains_text("Pairing code"));
        connected(&mut st, Ok(("Nano X".into(), vec![HwRow { index: 0, address: "0x00000000000000000000000000000000000000000000000000000000000000a1".into() }])));
        let v = view(&pr, &st);
        assert_eq!(v.texts()[..2], ["Pick Ledger accounts", "Nano X"]);
        assert!(v.contains_text("Verify on device"));
        assert_eq!(v.pad("b"), Some(&Msg::Back));
    }

    #[test]
    fn errors_and_keystone() {
        let pr = props(Kind::Ledger);
        let mut st = State::default();
        found(&mut st, Err(HwErr { code: Some("udev".into()), message: "Permission".into(), fix: Some("pkexec x".into()) }));
        let v = view(&pr, &st);
        assert!(v.contains_text("Rare OS needs permission to use this device."));
        assert_eq!(update(&pr, &mut st, Msg::Err(ErrMsg::CopyFix)), Some(Effect::Copy("pkexec x".into())));
        st.ble = true;
        st.err = Some(HwErr { code: Some("ble-off".into()), message: "Bluetooth is off".into(), fix: None });
        assert!(view(&pr, &st).contains_text("Turn Bluetooth on to connect your Ledger over Bluetooth."));

        let pk = props(Kind::Keystone);
        let mut st = State::default();
        found(&mut st, Ok(vec![Device { id: "k".into(), name: "Keystone 3 Pro".into(), model: String::new(), transport: "usb".into(), paired: false }]));
        let v = view(&pk, &st);
        assert_eq!(v.texts()[..5], ["Connect your Keystone", "Connect your Keystone 3 Pro with a USB-C cable. On the Keystone: Settings > Connection > USB.", "Keystone 3 Pro", "USB", "Connect"]);
        assert!(v.contains_text("The Keystone signs a transaction hash over USB. Rare shows you the decoded transaction first, so check it before you approve on the device."));
        update(&pk, &mut st, Msg::Connect("k".into()));
        assert!(view(&pk, &st).contains_text("Reading your Keystone"));
    }
}
