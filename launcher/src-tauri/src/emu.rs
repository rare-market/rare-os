//! Thin wrapper: the code lives in the rare-emu crate (launcher/crates/rare-emu). The Tauri
//! commands stay here, one line each, off the main thread where they were.

use rare_emu::{EmuInfo, UpdateDone, UpdateInfo};

pub use rare_emu::prepare_launch;

/// Gives an emulator game its emulator's power profile when nothing is saved for the game yet.
pub fn seed_power(c: &mut crate::config::Config, game_id: &str, program: Option<&str>, title: &str) -> bool {
    rare_emu::seed_power(&mut c.game_power, game_id, program, title)
}

#[tauri::command]
pub fn emu_list() -> Vec<EmuInfo> {
    rare_emu::emu_list()
}

#[tauri::command]
pub async fn emu_check(id: String) -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || rare_emu::emu_check(&id).map_err(String::from)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn emu_update(app: tauri::AppHandle, id: String) -> Result<UpdateDone, String> {
    let ctx = crate::rare_ctx::ctx(&app);
    tauri::async_runtime::spawn_blocking(move || rare_emu::emu_update(&ctx, id).map_err(String::from)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn emu_apply_defaults(id: String) -> Result<rare_emu::defaults::Report, String> {
    tauri::async_runtime::spawn_blocking(move || rare_emu::emu_apply_defaults(&id).map_err(String::from)).await.map_err(|e| e.to_string())?
}

pub mod actions {
    use rare_emu::actions::EmuAction;

    /// The rows for a game in the library.
    #[tauri::command]
    pub fn emu_actions(state: tauri::State<crate::AppState>, app_id: String) -> Result<Vec<EmuAction>, String> {
        let g = crate::find_game(&state, &app_id)?;
        let program = if let crate::model::LaunchSpec::Exec { program, .. } = &g.launch { Some(program.as_str()) } else { None };
        Ok(rare_emu::actions::emu_actions(&app_id, program, &g.title))
    }

    /// Does one row for a running game.
    #[tauri::command]
    pub async fn emu_do(app: tauri::AppHandle, state: tauri::State<'_, crate::AppState>, action: EmuAction) -> Result<(), String> {
        if !state.runner.running().contains(&action.app_id) {
            return Err("That game is not running".into());
        }
        let ctx = crate::rare_ctx::ctx(&app);
        tauri::async_runtime::spawn_blocking(move || rare_emu::actions::emu_do(&ctx, &action).map_err(String::from)).await.map_err(|e| e.to_string())?
    }
}
