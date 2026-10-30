//! "Move from your old SuiPlay wallet" (Playtron.tsx, MoveAssets.tsx): the old wallet's address on
//! the keyboard (0x or a SuiNS name), a read-only scan on a worker (`rare_wallet::rescue`), and
//! the move plan in signing order. Like the web page, Rare only reads the old wallet and shows the
//! plan for the person to carry out in that wallet: the web page passes no `signCovered`, so the
//! covered sends are never offered here either.

use crate::screens::firstrun::move_assets as ma;
use crate::screens::wallet::home::lib::{hint, Hint};
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::service::{self, poll};
use crate::state::State;
use rare_wallet::rescue::{self, GasMode};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

const ASK_FROM: &str = "wallet:playtron:from";

type Rx<T> = Option<Receiver<Result<T, String>>>;

#[derive(Default)]
pub struct Flow {
    /// the stack frame this state belongs to
    key: u64,
    st: Option<ma::State>,
    scan: Option<(rescue::RescueScan, ma::Scan)>,
    stage: Arc<Mutex<Option<String>>>,
    rx_from: Rx<Result<String, ()>>,
    rx_scan: Rx<(Result<rescue::RescueScan, String>, bool)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Page(ma::Msg),
}

fn i(v: u128) -> i128 {
    v.min(i128::MAX as u128) as i128
}

fn item(x: &rescue::RescueItem) -> ma::Item {
    ma::Item { object_id: x.object_id.clone(), name: x.name.clone(), collection: x.collection.clone() }
}

/// The scan in the page's shape.
pub fn scan_of(s: &rescue::RescueScan) -> ma::Scan {
    ma::Scan {
        default_name: s.default_name.clone(),
        coins: s.coins.iter().map(|c| ma::Coin { coin_type: c.coin_type.clone(), symbol: c.symbol.clone(), decimals: c.decimals, raw: i(c.raw) }).collect(),
        nfts: s.nfts.iter().map(item).collect(),
        kiosks: s
            .kiosks
            .iter()
            .map(|k| ma::Kiosk {
                kiosk_id: k.kiosk_id.clone(),
                personal: k.personal,
                items: k.items.iter().map(|x| ma::KioskItem { object_id: x.item.object_id.clone(), name: x.item.name.clone(), listed: x.listed.map(i), locked: x.locked }).collect(),
                profits: i(k.profits),
            })
            .collect(),
        staked: s.staked.iter().map(|x| i(x.principal)).collect(),
        names: s.names.iter().map(|n| ma::Name { object_id: n.object_id.clone(), domain: n.domain.clone(), expires_ms: n.expires_ms as i64 }).collect(),
        soulbound: s.soulbound.iter().map(item).collect(),
        truncated: s.truncated,
    }
}

/// The plan in the page's shape.
pub fn plan_of(p: &rescue::MovePlan) -> ma::Plan {
    ma::Plan {
        to: p.to.clone(),
        batches: p
            .batches
            .iter()
            .map(|b| ma::Batch {
                index: b.index,
                title: b.title.clone(),
                lines: b.lines.iter().map(|l| ma::Line { kind: l.kind.into(), id: l.id.clone(), label: l.label.clone(), detail: l.detail.clone() }).collect(),
            })
            .collect(),
        stuck: p.stuck.iter().map(|s| ma::Stuck { id: s.id.clone(), label: s.label.clone(), reason: s.reason.clone() }).collect(),
        empty: p.empty,
        fee_note: p.fee_note.clone(),
    }
}

fn load_from() -> Option<String> {
    rare_wallet::prefs::PrefsFile::new(rare_wallet::vault::default_dir()).load().playtron_from
}

fn save_from(a: &str) {
    let f = rare_wallet::prefs::PrefsFile::new(rare_wallet::vault::default_dir());
    let mut p = f.load();
    p.playtron_from = Some(a.to_string());
    if let Err(e) = f.save(&p) {
        log::warn!("playtron address: {e}");
    }
}

impl WalletTab {
    /// The page's plan: the scan sorted for this wallet's address (None without a wallet to receive).
    fn playtron_plan(&self) -> Option<ma::Plan> {
        let to = self.current().filter(|a| a.kind != "watch")?.address.clone();
        let (rs, _) = self.flows.playtron.scan.as_ref()?;
        Some(plan_of(&rescue::plan_moves(rs, &to, GasMode::Own)))
    }

    fn playtron_run<R>(&self, f: impl FnOnce(&ma::Props) -> R) -> R {
        let plan = self.playtron_plan();
        let pr = ma::Props {
            account: Some(self.current()),
            scan: self.flows.playtron.scan.as_ref().map(|s| &s.1),
            plan: plan.as_ref(),
            covered: false,
            exit_label: "Done",
            has_back: true,
            tz_min: 0,
        };
        f(&pr)
    }

    pub(in crate::screens::wallet) fn playtron_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let WalletView::Playtron = v else { return None };
        let st = self.flows.playtron.st.as_ref()?;
        Some(self.playtron_run(|pr| ma::view(pr, st)).map(&Msg::Page))
    }

    pub(in crate::screens::wallet) fn playtron_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::Playtron)
    }

    pub(in crate::screens::wallet) fn playtron_hints(&self) -> Vec<Hint> {
        vec![hint("a", "Select"), hint("b", "Back")]
    }

    pub(in crate::screens::wallet) fn playtron_handle(&mut self, st: &mut State, m: Msg) {
        let Msg::Page(m) = m;
        let Some(mut ps) = self.flows.playtron.st.take() else { return };
        let eff = self.playtron_run(|pr| ma::update(pr, &mut ps, m));
        self.flows.playtron.st = Some(ps);
        self.playtron_effect(st, eff);
        self.ver += 1;
    }

    fn playtron_effect(&mut self, st: &mut State, eff: Option<ma::Effect>) {
        use ma::Effect as E;
        match eff {
            Some(E::Exit) | Some(E::Back) => {
                self.stack.pop();
            }
            Some(E::NeedWallet) => {
                self.stack.pop();
                self.push(st, WalletView::Add);
            }
            Some(E::AskFrom(v)) => st.ask_text(ASK_FROM, "Your old SuiPlay wallet address", v.unwrap_or_default(), Some("0x... or name.sui")),
            Some(E::Abort) => {
                let f = &mut self.flows.playtron;
                f.rx_scan = None;
                f.rx_from = None;
            }
            Some(E::SendCovered(_)) => {}
            Some(E::Scan { address, again }) => {
                if self.fixture {
                    // no network in fixture mode
                    if let Some(ps) = self.flows.playtron.st.as_mut() {
                        ma::scanned(ps, false, again);
                    }
                    return;
                }
                let stage = self.flows.playtron.stage.clone();
                *stage.lock().unwrap() = Some("Starting".into());
                self.flows.playtron.rx_scan = Some(service::work(move |w| {
                    let say = |s: &str| *stage.lock().unwrap() = Some(s.to_string());
                    let r = w.rt.block_on(rescue::scan_playtron(&w.sui, &address, &say)).map_err(|e| e.to_string());
                    (r, again)
                }));
            }
            None => {}
        }
    }

    pub(in crate::screens::wallet) fn playtron_tick(&mut self, st: &mut State) -> bool {
        let top = self.stack.top().clone();
        if !matches!(top.view, WalletView::Playtron) {
            if self.flows.playtron.st.is_some() {
                self.flows.playtron = Flow::default();
            }
            return false;
        }
        if self.flows.playtron.key != top.key || self.flows.playtron.st.is_none() {
            let saved = if self.fixture { None } else { load_from() };
            self.flows.playtron = Flow { key: top.key, st: Some(ma::State::new(saved)), ..Flow::default() };
            self.ver += 1;
        }
        if let Some(v) = st.take_answer(ASK_FROM) {
            if let Some(text) = v.filter(|t| !t.trim().is_empty()) {
                if self.fixture {
                    let a = rare_sui::ids::canon_addr(&text);
                    self.playtron_from_answer(st, Ok(a));
                } else {
                    self.flows.playtron.rx_from = Some(service::work(move |w| {
                        match w.rt.block_on(rare_wallet::recipient::resolve_recipient(&w.names(), &text)) {
                            rare_wallet::recipient::RecipientState::Ready { address, .. } => Ok(address),
                            _ => Err(()),
                        }
                    }));
                }
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.playtron.rx_from) {
            self.playtron_from_answer(st, r.unwrap_or(Err(())));
        }
        if let Some(r) = poll(&mut self.flows.playtron.rx_scan) {
            let (res, again) = match r {
                Ok(x) => x,
                Err(e) => (Err(e), false),
            };
            let f = &mut self.flows.playtron;
            *f.stage.lock().unwrap() = None;
            match res {
                Ok(s) => {
                    let conv = scan_of(&s);
                    f.scan = Some((s, conv));
                    if let Some(ps) = f.st.as_mut() {
                        ma::scanned(ps, true, again);
                    }
                }
                Err(e) => {
                    log::warn!("playtron scan: {e}");
                    if let Some(ps) = f.st.as_mut() {
                        ma::scanned(ps, false, again);
                    }
                }
            }
            self.ver += 1;
        }
        // the scan's progress line
        let f = &mut self.flows.playtron;
        if f.rx_scan.is_some() {
            let now = f.stage.lock().unwrap().clone();
            if let Some(ps) = f.st.as_mut() {
                if ps.stage != now && now.is_some() {
                    ps.stage = now;
                    self.ver += 1;
                }
            }
        }
        let f = &self.flows.playtron;
        f.rx_from.is_some() || f.rx_scan.is_some()
    }

    fn playtron_from_answer(&mut self, st: &mut State, res: Result<String, ()>) {
        let Some(mut ps) = self.flows.playtron.st.take() else { return };
        if let Ok(a) = &res {
            if !self.fixture {
                save_from(a);
            }
        }
        let eff = self.playtron_run(|pr| ma::from_entered(pr, &mut ps, res));
        if matches!(eff, Some(ma::Effect::Scan { .. })) {
            self.flows.playtron.scan = None;
        }
        self.flows.playtron.st = Some(ps);
        self.playtron_effect(st, eff);
        self.ver += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_and_scan_keep_their_words() {
        let a = |n: u64| format!("0x{n:064x}");
        let s = rescue::RescueScan {
            address: a(1),
            coins: vec![rescue::RescueCoin { coin_type: rescue::SUI_TYPE.into(), symbol: "SUI".into(), decimals: 9, raw: 2_000_000_000, coin_raw: 2_000_000_000, object_ids: vec![a(7)], object_balances: vec![2_000_000_000], ..Default::default() }],
            nfts: vec![rescue::RescueItem { object_id: a(2), name: "N".into(), collection: "C".into(), ..Default::default() }],
            ..Default::default()
        };
        let sc = scan_of(&s);
        assert_eq!(sc.coins[0].raw, 2_000_000_000);
        let p = plan_of(&rescue::plan_moves(&s, &a(9), GasMode::Own));
        assert_eq!(p.batches.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(), ["Collectibles", "Coins"]);
        assert!(p.fee_note.is_none());
        assert_eq!(p.to, a(9));
    }
}
