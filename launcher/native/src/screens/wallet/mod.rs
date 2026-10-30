//! Rare Wallet, natively: launcher/src/screens/Wallet.tsx and launcher/src/wallet/ui/* (home,
//! pages, sign and review sheets, onboarding, unlock, Add passkey).
//!
//! `markup`, `fmt` and `kit` are shared by every Phase 3 screen (sui, signin, firstrun,
//! inventory, trophies, perks): the TSX trees as data, the wallet's number formatting, and the
//! wallet kit with the rare.market pieces it uses.

pub mod flows;
pub mod fmt;
pub mod home;
pub mod icons;
pub mod kit;
pub mod live;
pub mod markup;
pub mod market;
pub mod onboard;
pub mod page;
pub mod pages;
pub mod render;
pub mod service;
pub mod sign;
pub mod types;
pub mod ui;
