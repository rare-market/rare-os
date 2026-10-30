//! Send SUI and coins (Send.tsx): the amount pad, the recipient field on the keyboard (0x
//! addresses and SuiNS names, resolved both ways), recent recipients, then `buildCoinSend` and the
//! signing queue. The global review sheet takes over; a sent coin lands back on home.

use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::pages::send as page;
use crate::screens::wallet::service::{self, poll};
use crate::screens::wallet::sign::recipient::{Recent, RecipientState};
use crate::state::{State, Tone};
use rare_wallet::sign::{SignError, SignResult, TxInput};
use std::sync::mpsc::Receiver;

const ASK_TO: &str = "wallet:send:to";

type Rx<T> = Option<Receiver<Result<T, String>>>;

#[derive(Default)]
pub struct Flow {
    /// the stack frame this state belongs to
    key: u64,
    st: Option<page::State>,
    rs: Option<RecipientState>,
    recents: Vec<Recent>,
    rx_resolve: Rx<(String, rare_wallet::recipient::RecipientState)>,
    rx_send: Rx<(page::SendCoin, Result<SignResult, SignError>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Page(page::Msg),
}

/// rare-wallet's recipient field state in the page's words.
pub fn recipient_of(s: rare_wallet::recipient::RecipientState) -> RecipientState {
    use rare_wallet::recipient::RecipientState as R;
    match s {
        R::Empty => RecipientState::Empty,
        R::Typing => RecipientState::Typing,
        R::Invalid { msg } => RecipientState::Invalid(msg),
        R::Loading { name } => RecipientState::Loading { name },
        R::Ready { address, suins, .. } => RecipientState::Ready { address, suins },
        R::Missing { msg, .. } => RecipientState::Missing(msg),
        R::Error { msg, .. } => RecipientState::Error(msg),
    }
}

/// The field's state before the lookups answer (a name shows "Looking up name").
pub fn typed_state(text: &str) -> RecipientState {
    use rare_wallet::recipient::{parse_recipient, recipient_state, NameLookup, Recipient};
    let p = parse_recipient(text);
    let lookup = if matches!(p, Recipient::Name { .. }) { NameLookup::Loading } else { NameLookup::Idle };
    recipient_of(recipient_state(&p, &lookup, None, true))
}

fn load_recents() -> Vec<Recent> {
    let p = rare_wallet::prefs::PrefsFile::new(rare_wallet::vault::default_dir()).load();
    p.recents.into_iter().map(|r| Recent { address: r.address, suins: r.suins, at: r.at as i64 }).collect()
}

fn save_recent(address: &str, suins: Option<&str>) {
    let f = rare_wallet::prefs::PrefsFile::new(rare_wallet::vault::default_dir());
    let mut p = f.load();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    p.recents = rare_wallet::prefs::note_sent(&p.recents, address, suins, now);
    if let Err(e) = f.save(&p) {
        log::warn!("wallet recents: {e}");
    }
}

impl WalletTab {
    fn send_props<'a>(&'a self, rs: &'a RecipientState, recents: &'a [Recent]) -> page::Props<'a> {
        let cur = self.current();
        page::Props {
            account: cur,
            accounts: &self.accounts,
            can_sign: cur.is_some_and(|a| a.kind != "watch"),
            pf: &self.pf,
            rs,
            recents,
            sheet_open: crate::screens::wallet::sign::host::active(),
            active: true,
            now: self.now,
        }
    }

    pub(in crate::screens::wallet) fn send_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let WalletView::Send { .. } = v else { return None };
        let f = &self.flows.send;
        let st = f.st.as_ref()?;
        let rs = f.rs.clone().unwrap_or(RecipientState::Empty);
        page::view(&self.send_props(&rs, &f.recents), st).map(|m| m.map(&Msg::Page))
    }

    pub(in crate::screens::wallet) fn send_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::Send { .. })
    }

    /// The Send page's hints (A select, X delete, Menu review, B back).
    pub(in crate::screens::wallet) fn send_hints(&self) -> Option<Vec<crate::screens::wallet::home::lib::Hint>> {
        let f = &self.flows.send;
        let st = f.st.as_ref()?;
        let rs = f.rs.clone().unwrap_or(RecipientState::Empty);
        Some(page::hints(&self.send_props(&rs, &f.recents), st))
    }

    fn send_resolve(&mut self, text: String) {
        let f = &mut self.flows.send;
        f.rs = Some(if text.trim().is_empty() { RecipientState::Empty } else { typed_state(&text) });
        if text.trim().is_empty() || self.fixture {
            f.rx_resolve = None;
            return;
        }
        f.rx_resolve = Some(service::work(move |w| {
            let s = w.rt.block_on(rare_wallet::recipient::resolve_recipient(&w.names(), &text));
            (text, s)
        }));
    }

    pub(in crate::screens::wallet) fn send_handle(&mut self, st: &mut State, m: Msg) {
        let Msg::Page(m) = m;
        let Some(mut ps) = self.flows.send.st.take() else { return };
        let rs = self.flows.send.rs.clone().unwrap_or(RecipientState::Empty);
        let recents = std::mem::take(&mut self.flows.send.recents);
        let eff = page::update(&self.send_props(&rs, &recents), &mut ps, m);
        self.flows.send.recents = recents;
        self.flows.send.st = Some(ps);
        match eff {
            Some(page::Effect::AskTo(v)) => st.ask_text(ASK_TO, "Send to", v, Some("0x address or name.sui")),
            Some(page::Effect::Resolve(t)) => self.send_resolve(t),
            Some(page::Effect::Push(v)) => self.push(st, v),
            Some(page::Effect::Send(sc)) => {
                let Some(sender) = self.current().map(|a| a.address.clone()) else { return };
                if self.fixture {
                    return;
                }
                self.flows.send.rx_send = Some(service::work(move |w| {
                    let p = rare_sui::tx::CoinSend {
                        sender: sender.clone(),
                        to: sc.to.clone(),
                        coin_type: sc.coin_type.clone(),
                        amount: sc.amount.max(0) as u64,
                        max: sc.max,
                        balance: Some(sc.balance.max(0) as u64),
                    };
                    let r = match w.rt.block_on(rare_sui::tx::build_coin_send(&w.sui, &p)) {
                        Ok((tx, opts)) => service::send(TxInput::PtbGas(tx, opts), &sender, &sc.title),
                        Err(e) => Err(SignError::Chain(e.to_string())),
                    };
                    (sc, r)
                }));
            }
            None => {}
        }
    }

    pub(in crate::screens::wallet) fn send_tick(&mut self, st: &mut State) -> bool {
        let top = self.stack.top().clone();
        let WalletView::Send { coin_type, to } = &top.view else {
            if self.flows.send.st.is_some() {
                self.flows.send = Flow::default();
            }
            return false;
        };
        if self.flows.send.key != top.key || self.flows.send.st.is_none() {
            let f = &mut self.flows.send;
            f.key = top.key;
            f.st = Some(page::State::new(coin_type.as_deref(), to.as_deref()));
            f.recents = if self.fixture { Vec::new() } else { load_recents() };
            f.rs = None;
            f.rx_send = None;
            if let Some(t) = to.clone() {
                self.send_resolve(t);
            }
            self.ver += 1;
        }
        if let Some(v) = st.take_answer(ASK_TO) {
            if let Some(v) = v {
                self.send_handle(st, Msg::Page(page::Msg::To(v)));
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.send.rx_resolve) {
            let f = &mut self.flows.send;
            if let Ok((text, s)) = r {
                // only the answer for what is in the field now
                if f.st.as_ref().is_some_and(|ps| ps.to == text) {
                    f.rs = Some(recipient_of(s));
                }
            } else if let Err(e) = r {
                f.rs = Some(RecipientState::Error(e));
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.send.rx_send) {
            let now = rare_render::clock::secs();
            let res: Result<(), Option<String>> = match r {
                Ok((sc, Ok(res))) if res.status.as_deref() != Some("failure") => {
                    save_recent(&sc.to, sc.suins.as_deref());
                    st.toast(sc.title.replacen("Send", "Sent", 1), Tone::Info, now);
                    self.stack.reset(Vec::new());
                    // Balances, activity and items refresh now and again shortly (keep::nudge).
                    crate::data::keep::nudge();
                    self.flows.send = Flow::default();
                    self.ver += 1;
                    return false;
                }
                Ok((_, Ok(_))) => Err(Some("The network rejected this transaction.".into())),
                Ok((_, Err(SignError::Rejected(_)))) => Err(None),
                Ok((_, Err(SignError::Failed(m)))) => Err(Some(m)),
                Ok((_, Err(e))) => Err(Some(e.to_string())),
                Err(e) => Err(Some(e)),
            };
            if let Some(ps) = self.flows.send.st.as_mut() {
                page::sent(ps, res);
            }
            self.ver += 1;
        }
        let f = &self.flows.send;
        f.rx_resolve.is_some() || f.rx_send.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_states_map_and_names_wait_for_lookup() {
        assert_eq!(typed_state(""), RecipientState::Empty);
        assert!(matches!(typed_state("alice.sui"), RecipientState::Loading { .. }));
        let a = format!("0x{}", "ab".repeat(32));
        assert_eq!(typed_state(&a), RecipientState::Ready { address: a.clone(), suins: None });
        assert!(matches!(typed_state("0x12"), RecipientState::Invalid(_) | RecipientState::Typing));
    }
}
