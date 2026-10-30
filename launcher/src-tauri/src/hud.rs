//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
pub use rare_system::hud::*;

/// A toast from the page. Drawn only while a game or app has the screen.
#[tauri::command]
pub fn hud_toast(title: String, body: Option<String>) {
    rare_system::hud::hud_toast(title, body)
}
