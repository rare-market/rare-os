//! Native screens. WS 13 owns overlay, settings, emulators, apps, stream and notify; the other
//! workstreams add their own modules here.

pub mod apps;
pub mod emulators;
pub mod friends;
/// WS 15: Phase 4 (market, store, trade, friends, share).
pub mod market;
pub mod notify;
pub mod overlay;
pub mod settings;
pub mod share;
pub mod store;
pub mod stream;
pub mod trade;
// WS 14: Phase 3. `wallet` also holds the markup tree, kit and formatting the others share.
pub mod firstrun;
pub mod inventory;
pub mod perks;
pub mod signin;
pub mod sui;
pub mod trophies;
pub mod wallet;
// WS 12: Phase 1 (the app shell, Home, Library, game pages, Search, the keyboard, Quick settings).
pub mod game;
pub mod home;
pub mod library;
pub mod osk;
pub mod quick;
pub mod search;
pub mod shell;

use overlay::common::{Cx, Screen};

/// Builds any fixture screen by name (the golden harness and tests).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    overlay::fixture(name, cx, now)
        .or_else(|| settings::fixture(name, cx, now))
        .or_else(|| emulators::fixture(name, cx, now))
        .or_else(|| apps::fixture(name, cx, now))
        .or_else(|| stream::fixture(name, cx, now))
        .or_else(|| notify::fixture(name, cx, now))
        .or_else(|| trade::fixture(name, cx, now))
        .or_else(|| market::fixture(name, cx, now))
        .or_else(|| store::fixture(name, cx, now))
        .or_else(|| friends::fixture(name, cx, now))
        .or_else(|| share::fixture(name, cx, now))
}
