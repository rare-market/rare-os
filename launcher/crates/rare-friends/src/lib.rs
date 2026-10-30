//! Rare's friends logic: device-key auth, friends/party/presence API, usernames, live updates.
//! Ported from launcher/src/friends (logic only; no UI).
//!
//! - [`sign`]: the per-account Ed25519 device key, the texts it signs and the request headers,
//!   byte for byte what the web launcher sends (the server verifies them).
//! - [`api`]: rare.market's /v1/friends, /v1/presence and /v1/usernames over `rare_market::net`,
//!   behind the [`api::FriendsApi`] trait (the `mock` feature adds sample data in [`mock`]).
//! - [`store`]: the friends state with its pure transitions, and [`store::Friends`], the account,
//!   key and API together.
//! - [`live`]: heartbeat, polls and notices as functions of the time the host passes in.
//! - [`join`], [`display`], [`format`], [`username`], [`username_rules`], [`names`], [`resolve`],
//!   [`nav`], [`pick_username`]: the smaller pieces the screens use.

pub mod api;
pub mod display;
pub mod format;
pub mod join;
pub mod live;
#[cfg(feature = "mock")]
pub mod mock;
pub mod names;
pub mod nav;
pub mod pick_username;
pub mod resolve;
pub mod sign;
pub mod storage;
pub mod store;
pub mod types;
pub mod username;
pub mod username_rules;

pub use api::{Error, FriendsApi, Remote, WalletSigner};
pub use store::{Account, Applied, Friends, FriendsState, FriendsStatus};
pub use types::*;
