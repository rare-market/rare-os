//! The Wallet tab's flows wired to rare-wallet, one file per area. Each file adds an
//! `impl WalletTab` block (fields and helpers in `page.rs` are `pub(super)`), so page.rs keeps
//! only the dispatch lines.
//!
//! Blocking wallet work runs through `service::work` (a worker thread; drain with `service::poll`
//! in `tick`); PIN prompts through `service::ask_unlock(reason)`; text through
//! `State::ask_text(key, ..)` + `State::take_answer(key)` (secrets: `sign::host::ask_secret`).

use super::home::stack::WalletView;
use super::markup::El as M;
use super::page::WalletTab;
use crate::state::State;

pub mod onboard;
pub mod passkey;
pub mod hardware;
pub mod send;
pub mod items;
pub mod manage;
pub mod detail;
pub mod playtron;
pub mod pin_account;
pub mod move_all;

/// Every flow's own state (one field per file).
#[derive(Default)]
pub struct Flows {
    pub onboard: onboard::Flow,
    pub passkey: passkey::Flow,
    pub hardware: hardware::Flow,
    pub send: send::Flow,
    pub items: items::Flow,
    pub manage: manage::Flow,
    pub detail: detail::Flow,
    pub playtron: playtron::Flow,
    pub pin_account: pin_account::Flow,
    pub move_all: move_all::Flow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Onboard(onboard::Msg),
    Passkey(passkey::Msg),
    Hardware(hardware::Msg),
    Send(send::Msg),
    Items(items::Msg),
    Manage(manage::Msg),
    Detail(detail::Msg),
    Playtron(playtron::Msg),
    PinAccount(pin_account::Msg),
    MoveAll(move_all::Msg),
}

impl WalletTab {
    /// A page a flow draws; None when no flow owns `v`.
    pub(super) fn flows_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        None
            .or_else(|| self.onboard_markup(v).map(|m| m.map(&Msg::Onboard)))
            .or_else(|| self.passkey_markup(v).map(|m| m.map(&Msg::Passkey)))
            .or_else(|| self.hardware_markup(v).map(|m| m.map(&Msg::Hardware)))
            .or_else(|| self.send_markup(v).map(|m| m.map(&Msg::Send)))
            .or_else(|| self.items_markup(v).map(|m| m.map(&Msg::Items)))
            .or_else(|| self.manage_markup(v).map(|m| m.map(&Msg::Manage)))
            .or_else(|| self.detail_markup(v).map(|m| m.map(&Msg::Detail)))
            .or_else(|| self.playtron_markup(v).map(|m| m.map(&Msg::Playtron)))
            .or_else(|| self.pin_account_markup(v).map(|m| m.map(&Msg::PinAccount)))
            .or_else(|| self.move_all_markup(v).map(|m| m.map(&Msg::MoveAll)))
    }

    /// True when a flow draws `v` (the stack opens it instead of toasting).
    pub(super) fn flows_wired(v: &WalletView) -> bool {
        false || Self::onboard_wired(v) || Self::passkey_wired(v) || Self::hardware_wired(v) || Self::send_wired(v) || Self::items_wired(v) || Self::manage_wired(v) || Self::detail_wired(v) || Self::playtron_wired(v) || Self::pin_account_wired(v) || Self::move_all_wired(v)
    }

    pub(super) fn flows_handle(&mut self, st: &mut State, m: Msg) {
        match m {
            Msg::Onboard(m) => self.onboard_handle(st, m),
            Msg::Passkey(m) => self.passkey_handle(st, m),
            Msg::Hardware(m) => self.hardware_handle(st, m),
            Msg::Send(m) => self.send_handle(st, m),
            Msg::Items(m) => self.items_handle(st, m),
            Msg::Manage(m) => self.manage_handle(st, m),
            Msg::Detail(m) => self.detail_handle(st, m),
            Msg::Playtron(m) => self.playtron_handle(st, m),
            Msg::PinAccount(m) => self.pin_account_handle(st, m),
            Msg::MoveAll(m) => self.move_all_handle(st, m),
        }
    }

    /// Answers from workers and the keyboard. True while something is in flight.
    pub(super) fn flows_tick(&mut self, st: &mut State) -> bool {
        let mut busy = false;
        busy |= self.onboard_tick(st);
        busy |= self.passkey_tick(st);
        busy |= self.hardware_tick(st);
        busy |= self.send_tick(st);
        busy |= self.items_tick(st);
        busy |= self.manage_tick(st);
        busy |= self.detail_tick(st);
        busy |= self.playtron_tick(st);
        busy |= self.pin_account_tick(st);
        busy |= self.move_all_tick(st);
        busy
    }
}
