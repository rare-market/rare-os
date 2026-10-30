//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
pub use rare_system::display::*;

#[tauri::command]
pub fn display_state() -> DisplayState {
    rare_system::display::display_state()
}
