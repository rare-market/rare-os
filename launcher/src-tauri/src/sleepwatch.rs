//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
pub use rare_system::sleepwatch::*;

#[tauri::command]
pub fn sleep_state() -> SleepState {
    rare_system::sleepwatch::sleep_state()
}

pub fn start(app: tauri::AppHandle) {
    rare_system::sleepwatch::start(crate::rare_ctx::ctx(&app).sink);
}
