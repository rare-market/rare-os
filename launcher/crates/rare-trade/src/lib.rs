//! Rare's Trade logic: the Trade API router swaps, perps, positions, top traders (Trade API over
//! rare-state). Ported from the web launcher's launcher/src/trade, logic only: the screens
//! workstreams draw it.
//!
//! - The network goes through `rare_market::net::Net`: read-only Trade paths are cached reads
//!   (`trade:<path>`), everything else an uncached call on the host's allow-list ([`http`]).
//! - Chain reads (balances, object types, coin metadata) come through [`chain::ChainReader`], which
//!   `rare-sui` implements; signing is `rare-wallet`'s, handed a [`wallet::SignRequest`] with an
//!   [`tx::UnsignedTx`].
//! - The web launcher's hooks and stores are plain structs here ([`ticket::Ticket`],
//!   [`perps::PerpTicket`], [`nav::TradeNav`], [`traders::feed::FollowFeed`], ...): the screen
//!   passes the time in, does the reads they ask for, and hands the answers back.
//! - Numbers print exactly as the web launcher printed them ([`num`], [`format`], [`jsnum`]).

pub mod amount;
pub mod api;
pub mod chain;
pub mod chartmode;
pub mod format;
pub mod http;
pub mod jsnum;
pub mod kv;
pub mod nav;
pub mod num;
pub mod perps;
pub mod positions;
pub mod search;
pub mod swap;
pub mod ticket;
pub mod timing;
pub mod traders;
pub mod tx;
pub mod wallet;

pub use http::{Result, TradeApiError};
