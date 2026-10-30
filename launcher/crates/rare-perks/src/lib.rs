//! Rareling perks, ported from launcher/src/perks (config.ts, detect.ts, store.ts, themes.ts and the
//! logic in ui.tsx). No UI.
//!
//! Rarelings are OFF for this release and come at a separate launch: nothing here checks
//! ownership, and no badge, label, theme or panel shows, until the launcher is built with the
//! `rarelings` feature ([`config::rarelings_on`]).

pub mod config;
pub mod detect;
pub mod launches;
pub mod store;
pub mod themes;

pub use config::{rareling_type, rarelings_on};
pub use detect::{check_rareling, RarelingReader, RarelingResult};
