//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
pub use rare_system::freeze::*;

#[tauri::command]
pub fn freeze_policy(state: tauri::State<crate::AppState>, id: String) -> Background {
    let target = state.runner.freeze_target(&id);
    rare_system::freeze::freeze_policy(target, id)
}

#[tauri::command]
pub fn freeze_policy_set(id: String, keep: Option<bool>) -> Result<(), String> {
    rare_system::freeze::freeze_policy_set(id, keep)
}

#[tauri::command]
pub fn freeze_note(id: String, online: bool) {
    rare_system::freeze::freeze_note(id, online)
}
