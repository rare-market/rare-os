//! Features the web launcher has that the native shell mounts through here: first run, trophies and
//! perks, cloud saves, the Local API sheets. Each lives in its own file; app.rs only calls the
//! functions below (boot, events, overlay pages), so the shell's structure stays the shell's.
//!
//! - `bus`: the one `rare_core::Ctx` the backends send events to; the app drains it every update.
//! - `config`: reads and merges ~/.config/rare/launcher.json fields.
//! - `signer`: the wallet's `TxSigner` slot (N2 installs it; features ask for it when they sign).

pub mod android;
pub mod agent;
pub mod apps;
pub mod bus;
pub mod capture;
pub mod claim;
pub mod cloudsync;
pub mod companion;
pub mod config;
pub mod copies;
pub mod devicepass;
pub mod downloads;
pub mod emulation;
pub mod fancurve;
pub mod gamebuttons;
pub mod firstrun;
pub mod gamemgmt;
pub mod ingame;
pub mod localapi;
pub mod parental;
pub mod parental_sheet;
pub mod perks;
pub mod portal;
pub mod saves;
pub mod services;
pub mod signer;
pub mod stats;
pub mod storage;
pub mod stores;
pub mod trophies;
pub mod trophy_case;
pub mod uninstall;
pub mod webgame;
pub mod wishlist_page;

use crate::page::{Fx, Page};
use crate::state::State;

/// What the features keep in `State` (each module owns its struct).
#[derive(Default)]
pub struct Shared {
    pub trophies: trophies::Shared,
    pub localapi: localapi::Shared,
    pub downloads: downloads::Shared,
}

/// Once, when the app starts: opens first run when setup is not done (App.tsx).
pub fn boot(st: &mut State) {
    firstrun::boot(st);
    trophies::start();
    localapi::boot(st);
    stores::boot();
    cloudsync::boot();
    companion::boot();
    android::boot();
    // Now playing: the first look at Quick settings or the Rare menu already has the player.
    rare_system::media::warm();
}

/// Timers the features keep (trophy pop-ups). Returns the next wake.
pub fn tick(st: &State, now: f64) -> Option<f64> {
    [trophies::tick(st, now), parental::next_wake(st, now)].into_iter().flatten().reduce(f64::min)
}

/// Every update, state only.
pub fn tick_state(st: &mut State) {
    firstrun::tick(st);
    localapi::tick(st);
    android::tick_state(st);
}

/// Every update: what needs workers.
pub fn poll(st: &mut State, fx: &mut Fx, now: f64) {
    localapi::poll(st, fx, now);
    agent::poll(st, fx, now);
    parental::poll(st, fx, now);
    downloads::poll(st, fx, now);
    cloudsync::poll(st, now);
    companion::poll(st, fx, now);
    crate::data::knowledge::poll(fx, now);
    crate::data::wishlist::poll(fx, now);
    emulation::poll(st, now);
}

/// An event from the backends (`Event::Other` by the web UI's names).
pub fn on_event(st: &mut State, fx: &mut Fx, ev: rare_core::Event, now: f64) {
    apps::on_event(&ev);
    stores::on_event(&ev);
    if let rare_core::Event::Other { name, payload } = &ev {
        match name.as_str() {
            "steam-achievements" => trophies::on_unlocks(st, fx, payload, now),
            "store-installed" => stores::on_installed(st, payload),
            "emu-setup" => emulation::on_setup_event(st, fx, payload, now),
            "note" => {
                let text = payload.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                let tone = if payload.get("error").and_then(|e| e.as_bool()).unwrap_or(false) { crate::state::Tone::Error } else { crate::state::Tone::Info };
                st.toast(text, tone, now);
            }
            "rare-api" | "rare-api-cancel" | "deep-link" => localapi::on_event(st, fx, name, payload, now),
            "wallet-request" | "wallet-connected" => webgame::on_event(st, name, payload, now),
            _ => {}
        }
    }
}

/// The page for an `Overlay::Other(kind)` the features own.
pub fn overlay_page(kind: &str) -> Option<Box<dyn Page>> {
    match kind {
        "firstrun" => Some(Box::new(firstrun::FirstRunPage::default())),
        "saves" => Some(Box::new(saves::SavesPage::default())),
        "trophies" => Some(Box::new(trophies::CasePage::default())),
        "apisheet" => Some(Box::new(localapi::SheetPage::default())),
        webgame::CONNECT => Some(Box::new(webgame::ConnectPage::default())),
        uninstall::KIND => Some(Box::new(uninstall::UninstallPage::default())),
        gamemgmt::PICK | gamemgmt::ART => gamemgmt::overlay_page(kind),
        downloads::KIND => Some(Box::new(downloads::DownloadsPage)),
        fancurve::KIND => Some(Box::new(fancurve::FanCurvePage::default())),
        gamebuttons::KIND => Some(Box::new(gamebuttons::GameButtonsPage)),
        cloudsync::CONFLICT => Some(Box::new(crate::screens::game::saves::ConflictPage)),
        cloudsync::VERSIONS => Some(Box::new(crate::screens::game::saves::VersionsPage::default())),
        stats::STATS => Some(Box::new(stats::StatsPage::default())),
        stats::YEAR => Some(Box::new(stats::YearPage::default())),
        wishlist_page::KIND => Some(Box::new(wishlist_page::WishlistPage::default())),
        android::TOUCH => Some(Box::new(android::TouchPage::default())),
        emulation::SETUP => Some(Box::new(emulation::SetupPage::default())),
        emulation::BIOS => Some(Box::new(emulation::BiosPage::default())),
        emulation::RA => Some(Box::new(emulation::RetroPage)),
        _ => None,
    }
}

/// Every overlay kind above, for the shell's layer and focus rules.
pub const KINDS: [&str; 18] = ["firstrun", "saves", "trophies", "apisheet", webgame::CONNECT, uninstall::KIND, downloads::KIND, cloudsync::CONFLICT, cloudsync::VERSIONS, stats::STATS, stats::YEAR, wishlist_page::KIND, fancurve::KIND, android::TOUCH, emulation::SETUP, emulation::BIOS, emulation::RA, gamebuttons::KIND];
