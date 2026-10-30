//! The power button, whatever has the screen: a press sleeps, a hold of LONG_PRESS opens the power
//! menu (Sleep, Restart, Turn off) over anything. Thin wrapper: the reader lives
//! in the rare-input crate (launcher/crates/rare-input/src/power.rs).

pub fn start(app: tauri::AppHandle) {
    rare_input::power::start(move || crate::overlay::power_menu(&app));
}
