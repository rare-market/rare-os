//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
use tauri::{AppHandle, Manager};

pub use rare_system::power::*;

fn config(app: &AppHandle) -> crate::config::Config {
    app.state::<crate::AppState>().config.lock().unwrap().clone()
}

/// Rare started this game: its power settings go on now, then `then` runs.
pub fn started(app: &AppHandle, id: &str, then: impl FnOnce() + Send + 'static) {
    let app = app.clone();
    rare_system::power::started(move || config(&app), id, then)
}

/// The runner's running set changed.
pub fn running(app: &AppHandle, now: &[String]) {
    let app = app.clone();
    rare_system::power::running(move || config(&app), now)
}

// Concrete-Config forms of the generic functions, so callers holding a MutexGuard<Config> keep
// deref-coercing (a generic parameter does not).
use crate::config::Config;

pub fn launcher_power(c: &Config) -> GamePower {
    rare_system::power::launcher_power(c)
}

pub fn target(c: &Config, s: &Session) -> GamePower {
    rare_system::power::target(c, s)
}

pub fn wanted_now(c: &Config) -> rare_system::system::Wanted {
    rare_system::power::wanted_now(c)
}

pub fn change(c: &mut Config, f: impl FnOnce(GamePower) -> GamePower) -> Option<GamePower> {
    rare_system::power::change(c, f)
}

pub fn save_for_game(c: &mut Config, save: bool) -> bool {
    rare_system::power::save_for_game(c, save)
}

pub fn wanted_hud(c: &Config) -> rare_system::perfhud::HudLevel {
    rare_system::power::wanted_hud(c)
}

pub fn view(c: &Config) -> Option<GameView> {
    rare_system::power::view(c)
}
