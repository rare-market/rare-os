//! "Move everything to this account" wired to rare-wallet: the old account's scan and the
//! transaction on a worker (`rare_wallet::move_all::plan_move_all`), then each transaction through
//! the review sheet as the old account (`service::send`, so it signs its own way: PIN, phone,
//! Ledger, A). View: `pages/move_all.rs`.

use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::pages::move_all::{self as mv, Phase, Source, Summary};
use crate::screens::wallet::service::{self, poll};
use crate::state::State;
use rare_wallet::move_all::MoveAll;
use rare_wallet::sign::TxInput;
use std::sync::mpsc::Receiver;

type Rx<T> = Option<Receiver<Result<T, String>>>;

#[derive(Default)]
pub struct Flow {
    key: u64,
    st: mv::State,
    plan: Option<MoveAll>,
    rx_scan: Rx<Result<MoveAll, String>>,
    rx_send: Rx<Result<Vec<String>, String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Page(mv::Msg),
}

/// Shots: RR_MOVE=pick|plan|done.
fn fixture_flow() -> mv::State {
    let a = format!("0x{}", "7c4e".repeat(16));
    let phase = match std::env::var("RR_MOVE").as_deref() {
        Ok("plan") => Phase::Plan(
            a,
            Summary {
                sui: 8_940_560_728,
                coins: vec![("USDC".into(), 1_500_000, 6), ("DEEP".into(), 120_000_000, 6)],
                nfts: 14,
                kiosks: 2,
                kiosk_items: 9,
                staked: 1,
                names: 1,
                stuck: vec![("Founders badge".into(), rare_wallet::rescue::stuck_reason("0xa::badge::Badge").into())],
                txs: 1,
            },
        ),
        Ok("done") => Phase::Done { from: a, digests: vec!["6mDaxBh81WjDBAMqwQCzDC2Ro9QSo23YZkeX96ot1nDd".into()] },
        Ok("reading") => Phase::Reading(a),
        _ => Phase::Pick,
    };
    mv::State { phase }
}

fn summary(m: &MoveAll) -> Summary {
    Summary {
        sui: m.sui,
        coins: m.coins.clone(),
        nfts: m.nfts,
        kiosks: m.kiosks,
        kiosk_items: m.kiosk_items,
        staked: m.staked,
        names: m.names,
        stuck: m.stuck.iter().map(|s| (s.label.clone(), s.reason.clone())).collect(),
        txs: m.txs.len(),
    }
}

impl WalletTab {
    fn mv_target(&self) -> Option<String> {
        match &self.stack.top().view {
            WalletView::MoveAll { to } if !to.is_empty() => Some(to.clone()),
            WalletView::MoveAll { .. } => self.current().map(|a| a.address.clone()),
            _ => None,
        }
    }

    /// Accounts here that can move their things: they sign, and they are not the target.
    fn mv_sources(&self, to: &str) -> Vec<Source> {
        if self.fixture {
            return vec![
                Source { address: format!("0x{}", "7c4e".repeat(16)), label: "Account 1".into(), method: "Signs with A on this SuiPlay".into() },
                Source { address: format!("0x{}", "91ab".repeat(16)), label: "Phone passkey account with a long name that keeps going".into(), method: "Signs with your phone".into() },
                Source { address: format!("0x{}", "03fe".repeat(16)), label: "Ledger".into(), method: "Signs on your Ledger".into() },
            ];
        }
        self.accounts
            .iter()
            .filter(|a| a.kind != "watch" && !a.address.eq_ignore_ascii_case(to))
            .map(|a| {
                let method = match a.kind.as_str() {
                    "passkey" if a.passkey.as_ref().is_some_and(|p| p.pin) => "Signs with your PIN",
                    "passkey" => "Signs with your phone or security key",
                    "ledger" => "Signs on your Ledger",
                    "keystone" => "Signs on your Keystone",
                    _ if a.device => "Signs with A on this SuiPlay",
                    _ => "Signs with your Rare Wallet PIN",
                };
                Source { address: a.address.clone(), label: a.label.clone(), method: method.into() }
            })
            .collect()
    }

    fn mv_fresh(&mut self) -> bool {
        if !matches!(self.stack.top().view, WalletView::MoveAll { .. }) {
            self.flows.move_all.key = 0;
            return false;
        }
        let top = self.stack.top().key;
        if self.flows.move_all.key != top {
            self.flows.move_all = Flow { key: top, st: if self.fixture { fixture_flow() } else { mv::State::default() }, ..Flow::default() };
        }
        true
    }

    pub(in crate::screens::wallet) fn move_all_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let WalletView::MoveAll { .. } = v else { return None };
        let to = self.mv_target().unwrap_or_default();
        let to_label = self.accounts.iter().find(|a| a.address.eq_ignore_ascii_case(&to)).map(|a| a.label.clone()).unwrap_or_else(|| "this account".into());
        let froms = self.mv_sources(&to);
        let st = if self.flows.move_all.key == self.stack.top().key { self.flows.move_all.st.clone() } else if self.fixture { fixture_flow() } else { mv::State::default() };
        let pr = mv::Props { to_label: &to_label, to: &to, froms: &froms };
        Some(mv::view(&pr, &st).map(&Msg::Page))
    }

    pub(in crate::screens::wallet) fn move_all_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::MoveAll { .. })
    }

    pub(in crate::screens::wallet) fn move_all_hints(&self) -> Vec<Hint> {
        mv::hints()
    }

    pub(in crate::screens::wallet) fn move_all_handle(&mut self, _st: &mut State, m: Msg) {
        if !self.mv_fresh() {
            return;
        }
        let Msg::Page(m) = m;
        let to = self.mv_target().unwrap_or_default();
        let eff = mv::update(&mut self.flows.move_all.st, m);
        match eff {
            Some(mv::Effect::Scan(from)) if !self.fixture => {
                let to2 = to.clone();
                self.flows.move_all.rx_scan = Some(service::work(move |w| {
                    let sui = w.sui.clone();
                    let owner = from.clone();
                    let nfts = move |refs: Vec<rare_wallet::nft::NftRef>| -> rare_sui::BoxFut<'static, std::collections::HashMap<String, rare_wallet::nft::Sendability>> {
                        let (sui, owner) = (sui.clone(), owner.clone());
                        Box::pin(async move { rare_wallet::nft::nft_sendability(&sui, &rare_sui::prices::ReqwestGet::default(), &owner, &refs).await })
                    };
                    w.rt.block_on(rare_wallet::move_all::plan_move_all(&w.sui, &nfts, &from, &to2))
                }));
            }
            Some(mv::Effect::Send(from)) if !self.fixture => {
                let Some(plan) = self.flows.move_all.plan.clone() else { return };
                let label = self.accounts.iter().find(|a| a.address.eq_ignore_ascii_case(&to)).map(|a| a.label.clone()).unwrap_or_else(|| "this account".into());
                self.flows.move_all.rx_send = Some(service::work(move |_w| {
                    let n = plan.txs.len();
                    let mut digests = Vec::new();
                    for (i, (tx, opts)) in plan.txs.into_iter().enumerate() {
                        let title = if n > 1 { format!("Move everything to {label} ({} of {n})", i + 1) } else { format!("Move everything to {label}") };
                        let r = service::send(TxInput::PtbGas(tx, opts), &from, &title).map_err(|e| e.to_string())?;
                        if r.status.as_deref() == Some("failure") {
                            return Err("The network rejected the move. Nothing left the account.".to_string());
                        }
                        digests.push(r.digest.unwrap_or_default());
                    }
                    Ok(digests)
                }));
            }
            Some(mv::Effect::Close) => {
                crate::data::keep::nudge();
                self.stack.reset(Vec::new());
            }
            _ => {}
        }
        self.ver += 1;
    }

    pub(in crate::screens::wallet) fn move_all_tick(&mut self, _st: &mut State) -> bool {
        if !self.mv_fresh() {
            return false;
        }
        let f = &mut self.flows.move_all;
        let mut changed = false;
        if let Some(r) = poll(&mut f.rx_scan) {
            let from = match &f.st.phase {
                Phase::Reading(a) => a.clone(),
                _ => String::new(),
            };
            f.st.phase = match r {
                Ok(Ok(m)) => {
                    let s = summary(&m);
                    f.plan = Some(m);
                    Phase::Plan(from, s)
                }
                Ok(Err(e)) | Err(e) => Phase::Failed(format!("Could not read that account: {e}")),
            };
            changed = true;
        }
        if let Some(r) = poll(&mut f.rx_send) {
            let from = match &f.st.phase {
                Phase::Sending(a) => a.clone(),
                _ => String::new(),
            };
            f.st.phase = match r {
                Ok(Ok(d)) => {
                    crate::data::keep::nudge();
                    Phase::Done { from, digests: d }
                }
                Ok(Err(e)) | Err(e) => Phase::Failed(e),
            };
            changed = true;
        }
        let busy = f.rx_scan.is_some() || f.rx_send.is_some();
        if changed {
            self.ver += 1;
        }
        busy
    }
}
