//! Accept an offer straight from its notification. Ported from launcher/src/inventory/notice.ts.
//!
//! The notice carries the item and the offer's price, so: load the item, take the best fillable
//! offer, and if it is still the one the notice announced, sell into it through the wallet's review
//! sheet. If a better offer exists now, or this one is gone, nothing is signed and the item sheet
//! opens with what is best right now.

use super::store::InventoryStore;
use super::trade::TradeApi;
use super::{InvItem, InvOp, Why};
use crate::api::{self, paths};
use crate::format::{fmt_sui_num, big_to_sui, SuiOpts};
use crate::net::Net;

/// The message a wallet cancellation carries (`SignRejected`). [`super::trade::Executor`]
/// implementations return exactly this when the player declines the sheet.
pub const SIGN_REJECTED: &str = "You declined the request";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptResult {
    Sold,
    Cancelled,
    Sheet,
    Failed,
}

/// What an `offer_received` notice says (notify's `Notice`, the fields used here).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OfferNotice {
    pub kind: String,
    pub item_id: Option<String>,
    /// The announced price, MIST (`amount.raw`).
    pub amount_raw: Option<String>,
}

pub fn is_offer_notice(n: &OfferNotice) -> bool {
    n.kind == "offer_received" && n.item_id.as_deref().is_some_and(|s| !s.is_empty())
}

/// What the flow needs from the launcher.
pub trait NoticeHost {
    /// The account that will sign (None: only watch accounts).
    fn signing_account(&self) -> Option<String>;
    fn toast(&self, text: &str);
    /// Shows the Wallet tab (the inventory sheet's host).
    fn show_wallet_tab(&self);
}

/// Opens the inventory's item sheet from anywhere (a notice, a search).
pub fn show_item(store: &mut InventoryStore, host: &dyn NoticeHost, id: &str) {
    store.open_item_sheet(Some(id.to_string()));
    host.show_wallet_tab();
}

fn reach_error(msg: &str) -> bool {
    let low = msg.to_lowercase();
    if low.contains("failed to fetch") {
        return true;
    }
    // /rare\.market \d+/
    low.match_indices("rare.market ").any(|(i, m)| low[i + m.len()..].starts_with(|c: char| c.is_ascii_digit()))
}

pub fn accept_from_notice(n: &OfferNotice, net: &dyn Net, trade: &dyn TradeApi, store: &mut InventoryStore, host: &dyn NoticeHost, now_ms: i64) -> AcceptResult {
    if !is_offer_notice(n) {
        return AcceptResult::Failed;
    }
    let id = n.item_id.clone().unwrap_or_default();
    let Some(account) = host.signing_account() else {
        host.toast("Add an account that can sign to accept offers");
        show_item(store, host, &id);
        return AcceptResult::Sheet;
    };
    let mut run = || -> crate::Result<AcceptResult> {
        let item: InvItem = api::get(net, &paths::item(&id))?;
        Ok(match trade.quote(&item, &account)? {
            None => {
                host.toast("That offer is no longer open");
                show_item(store, host, &id);
                AcceptResult::Sheet
            }
            Some(q) => {
                let announced = n.amount_raw.as_deref().filter(|s| !s.is_empty());
                let moved = announced.is_some_and(|a| a.trim().parse::<i128>().ok() != q.offer.price_mist.trim().parse::<i128>().ok());
                if q.blocked.is_some() || moved {
                    host.toast(q.blocked.as_deref().unwrap_or("A better offer is open now. Check it before selling"));
                    show_item(store, host, &id);
                    AcceptResult::Sheet
                } else {
                    store.run_optimistic(InvOp::Remove { ids: vec![id.clone()], why: Why::Sold }, now_ms, |_| trade.sell(&item, &q.offer, &account))?;
                    host.toast(&format!("Sold {} for {} SUI", item.name, fmt_sui_num(big_to_sui(q.net_mist), SuiOpts { dp: 3, compact: false, trim: true })));
                    AcceptResult::Sold
                }
            }
        })
    };
    match run() {
        Ok(r) => r,
        Err(e) if e.0 == SIGN_REJECTED => AcceptResult::Cancelled,
        Err(e) => {
            host.toast(if reach_error(&e.0) {
                "Couldn't reach rare.market"
            } else if e.0.is_empty() {
                "Couldn't accept the offer"
            } else {
                &e.0
            });
            AcceptResult::Failed
        }
    }
}
