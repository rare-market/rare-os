//! Inventory state. Ported from launcher/src/inventory/store.ts.
//!
//! `base` is what rare.market last said; `pending` is what the player just did. The screen shows
//! [`InventoryStore::current_view`] (`apply_pending(base, pending)`). A pending operation ends when
//! a fetch shows it (the market indexer caught up), after 90 seconds, or at once if the action fails
//! (rollback). Fetches are triggered by the notify stream (sold, bought, nft_received,
//! offer_received) and, after an own action, on a short schedule, since listing and sending make no
//! notice.
//!
//! No timers or threads here: the host asks [`InventoryStore::due`] when to look again and runs the
//! fetch off the UI thread between [`InventoryStore::start_refresh`] and
//! [`InventoryStore::finish_refresh`] (a newer fetch wins).

use super::logic::{apply_pending, settle, MOVES, SETTLE_MAX_AGE_MS};
use super::{InvOp, Inventory, Pending, Why};
use crate::net::Net;
use std::collections::HashSet;

/// After an own action: look again at 3, 8, 20 and 45 seconds, until nothing is pending.
pub const FOLLOW_UPS_MS: [i64; 4] = [3000, 8000, 20_000, 45_000];
/// A notice that moved items refetches after this pause (several notices, one fetch).
pub const NOTICE_DEBOUNCE_MS: i64 = 1200;

/// The rare-state key the last inventory of an address is kept under for the next start.
pub fn kept_key(address: &str) -> String {
    format!("ui:inv:{address}")
}

/// One fetch in flight; only the newest one's answer is taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshTicket {
    pub seq: u64,
    pub address: String,
}

#[derive(Debug, Default)]
pub struct InventoryStore {
    pub address: Option<String>,
    pub base: Option<Inventory>,
    pub pending: Vec<Pending>,
    pub loading: bool,
    pub error: bool,
    /// Item id whose sheet is open (the sheet host lives in the wallet screen).
    pub sheet_id: Option<String>,
    seq: u64,
    inflight: u64,
    follow_ups: Vec<i64>,
    debounce_at: Option<i64>,
    seen: HashSet<String>,
    started: bool,
}

impl InventoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_view(&self) -> Inventory {
        match &self.base {
            Some(b) => apply_pending(b, &self.pending),
            None => Inventory::default(),
        }
    }

    /// Begins a fetch for `address` (or the current one). Another account shows its last inventory
    /// from the cache at once (`ui:inv:<address>`). None when there is no address.
    pub fn start_refresh(&mut self, net: &dyn Net, address: Option<&str>) -> Option<RefreshTicket> {
        let addr = address.map(String::from).or_else(|| self.address.clone())?;
        if self.address.as_deref() != Some(addr.as_str()) {
            self.base = net.peek(&kept_key(&addr)).and_then(|b| serde_json::from_str(&b).ok());
            self.address = Some(addr.clone());
            self.pending.clear();
        }
        self.inflight += 1;
        self.loading = true;
        Some(RefreshTicket { seq: self.inflight, address: addr })
    }

    /// Takes a fetch's answer. A newer fetch wins; a failure keeps what is on screen.
    pub fn finish_refresh(&mut self, net: &dyn Net, ticket: &RefreshTicket, result: crate::Result<Inventory>, now_ms: i64) {
        if ticket.seq != self.inflight {
            return;
        }
        match result {
            Ok(inv) => {
                self.pending = settle(&self.pending, &inv, now_ms, SETTLE_MAX_AGE_MS);
                if let Ok(json) = serde_json::to_string(&inv) {
                    let _ = net.put(&kept_key(&ticket.address), &json);
                }
                self.base = Some(inv);
                self.error = false;
            }
            Err(_) => self.error = true,
        }
        self.loading = false;
    }

    /// Start and finish in one blocking call (for a worker thread that owns the store).
    pub fn refresh(&mut self, net: &dyn Net, address: Option<&str>, load: impl FnOnce(&str) -> crate::Result<Inventory>, now_ms: i64) {
        if let Some(t) = self.start_refresh(net, address) {
            let r = load(&t.address);
            self.finish_refresh(net, &t, r, now_ms);
        }
    }

    /// Shows `op` at once, before the action runs. Returns its key for [`Self::op_done`] or
    /// [`Self::op_failed`].
    pub fn begin_op(&mut self, op: InvOp, now_ms: i64) -> String {
        self.seq += 1;
        let key = format!("p{}", self.seq);
        self.pending.push(Pending { key: key.clone(), op, at_ms: now_ms });
        key
    }

    /// The action went through: the operation stays until the indexer shows it, and the
    /// inventory is looked at again on the follow-up schedule.
    pub fn op_done(&mut self, key: &str, now_ms: i64) {
        if let Some(p) = self.pending.iter_mut().find(|p| p.key == key) {
            p.at_ms = now_ms;
        }
        self.follow_up(now_ms);
    }

    /// The action failed: roll it back.
    pub fn op_failed(&mut self, key: &str) {
        self.pending.retain(|p| p.key != key);
    }

    /// `runOptimistic`: applies `op` on screen, runs the action, rolls back if it fails (the error
    /// goes on to the caller), keeps it if it succeeds.
    pub fn run_optimistic<T>(&mut self, op: InvOp, now_ms: i64, action: impl FnOnce(&Self) -> crate::Result<T>) -> crate::Result<T> {
        let key = self.begin_op(op, now_ms);
        match action(self) {
            Ok(v) => {
                self.op_done(&key, crate::time::now_ms().max(now_ms));
                Ok(v)
            }
            Err(e) => {
                self.op_failed(&key);
                Err(e)
            }
        }
    }

    fn follow_up(&mut self, now_ms: i64) {
        self.follow_ups.extend(FOLLOW_UPS_MS.iter().map(|d| now_ms + d));
        self.follow_ups.sort_unstable();
    }

    pub fn stop_follow_ups(&mut self) {
        self.follow_ups.clear();
    }

    /// Items the player just sent: gone from the screen now, confirmed by the next fetches.
    pub fn note_sent_items(&mut self, ids: Vec<String>, now_ms: i64) {
        self.begin_op(InvOp::Remove { ids, why: Why::Sent }, now_ms);
        self.follow_up(now_ms);
    }

    pub fn open_item_sheet(&mut self, id: Option<String>) {
        self.sheet_id = id;
    }

    /// Called once with the notices already there (the backlog), which are not acted on.
    pub fn start_sync<'a>(&mut self, backlog: impl IntoIterator<Item = &'a str>) {
        if self.started {
            return;
        }
        self.started = true;
        self.seen.extend(backlog.into_iter().map(String::from));
    }

    /// Notices arrived (id, kind). When a new one means the holdings moved, a fetch is due after
    /// [`NOTICE_DEBOUNCE_MS`].
    pub fn on_notices<'a>(&mut self, list: impl IntoIterator<Item = (&'a str, &'a str)>, now_ms: i64) {
        let mut moved = false;
        for (id, kind) in list {
            if !self.seen.insert(id.to_string()) {
                continue;
            }
            if MOVES.contains(&kind) {
                moved = true;
            }
        }
        if !moved || self.address.is_none() {
            return;
        }
        self.debounce_at = Some(now_ms + NOTICE_DEBOUNCE_MS);
    }

    /// Whether a fetch is due now (a notice's debounce passed, or a follow-up while something is
    /// pending). Consumes what came due.
    pub fn due(&mut self, now_ms: i64) -> bool {
        let mut due = false;
        if self.debounce_at.is_some_and(|t| t <= now_ms) {
            self.debounce_at = None;
            due = true;
        }
        let before = self.follow_ups.len();
        self.follow_ups.retain(|t| *t > now_ms);
        if self.follow_ups.len() < before && !self.pending.is_empty() {
            due = true;
        }
        due
    }

    /// When to ask [`Self::due`] next, if anything is scheduled.
    pub fn next_wake(&self) -> Option<i64> {
        self.debounce_at.into_iter().chain(self.follow_ups.first().copied()).min()
    }
}
