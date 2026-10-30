//! Rare's Sui core, for the native launcher and (through thin wrappers) the Tauri shell.
//!
//! The Rust port of the TypeScript launcher's `src/sui/*`, `src/wallet/grpc.ts`, the read half of
//! `src/wallet/data/*` and `src/lib/live.ts`:
//!
//! * [`network`]: which Sui network the launcher reads (Settings > Developer), the RPC and
//!   rare.market URLs for it, and the deployed package ids per network.
//! * [`grpc`]: the one Sui client (MystenLabs' `sui-rpc` gRPC), per network, with the reads the
//!   launcher makes: gas price, balances, coin metadata, owned objects, objects, dynamic fields,
//!   SuiNS names, transactions, plus simulate and execute for the wallet.
//! * [`market`]: rare.market's `/v1` reads (status, account, portfolio, held collections,
//!   launches, mints) behind a [`market::MarketApi`] the host implements (or [`market::HttpMarket`]).
//! * [`the Trade API`]: the Trade API's public extension API (prices, 24 h change, portfolios, candles).
//! * [`ownership`], [`licence`], [`kiosk`], [`balances`], [`names`], [`nfts`], [`profile`]:
//!   game ownership, Rare Store licences, kiosks (contents, listings, dynamic fields by name),
//!   coin balances with prices, SuiNS, NFT detail and the player's profile.
//! * [`live`] and [`shell_live`]: the Home strip's live Sui data (`sui/live.ts`) and the shell's
//!   live feed (`lib/live.ts`), as state plus the polling schedule.
//! * [`ptb`] and [`tx`]: the unresolved programmable transaction every builder produces, and
//!   building it over `sui-transaction-builder` (inputs and gas resolved on the node) for the
//!   wallet to sign (workstream 10; this crate never holds a key). Coin sends live here too.
//!
//! Every function that reads the network takes its reader as a parameter (a trait) so the ported
//! TS tests run with fakes and no network.

#[cfg(feature = "chain")]
pub mod activity;
#[cfg(feature = "chain")]
pub mod balances;
pub mod error;
#[cfg(feature = "chain")]
pub mod grpc;
#[cfg(feature = "chain")]
pub mod ids;
#[cfg(feature = "chain")]
pub mod json;
#[cfg(feature = "chain")]
pub mod kiosk;
#[cfg(feature = "chain")]
pub mod licence;
#[cfg(feature = "chain")]
pub mod live;
#[cfg(feature = "chain")]
pub mod market;
#[cfg(feature = "chain")]
pub mod names;
#[cfg(feature = "chain")]
pub mod network;
#[cfg(feature = "chain")]
pub mod nfts;
#[cfg(feature = "chain")]
pub mod ownership;
#[cfg(feature = "chain")]
pub mod profile;
#[cfg(feature = "chain")]
pub mod ptb;
pub mod saves;
#[cfg(feature = "chain")]
pub mod shell_live;
#[cfg(feature = "chain")]
pub mod signer;
#[cfg(feature = "chain")]
pub mod tx;
#[cfg(feature = "chain")]
pub mod txdetail;
#[cfg(feature = "chain")]
pub mod prices;

pub use error::{Error, Result};
#[cfg(feature = "chain")]
pub use grpc::Sui;
#[cfg(feature = "chain")]
pub use network::{Network, NetworkConfig};

/// A boxed future, so reader traits stay object safe (fakes in tests, the real client in the app).
pub type BoxFut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>;
