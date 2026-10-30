//! Rare's marketplace logic, ported from the web launcher's TypeScript (launcher/src/marketplace,
//! market, inventory, overlay/trade.ts): the rare.market /v1 client over `rare-state`, the venue
//! builders that buy, sell into offers, list and cancel, offer choice and inventory merging. No UI.
//!
//! Shared with the other logic crates (rare-trade, rare-friends, rare-notify): [`net`] (the one door
//! to the network) and [`ptb`] (unresolved programmable transactions, built and signed by
//! `rare-sui` and `rare-wallet`).

pub mod api;
pub mod buy;
pub mod chain;
pub mod format;
pub mod identicon;
pub mod inventory;
pub mod jsnum;
pub mod media;
pub mod nav;
pub mod net;
pub mod ptb;
pub mod sell;
pub mod time;
pub mod types;
pub mod venues;

/// Errors are messages for the player (`rare_core::Error`).
pub use rare_core::Error;
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod golden;
