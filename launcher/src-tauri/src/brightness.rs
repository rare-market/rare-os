//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
pub use rare_system::brightness::*;

#[tauri::command]
pub fn brightness_info() -> Info {
    rare_system::brightness::brightness_info()
}

#[tauri::command]
pub fn brightness_dim_idle(on: bool) {
    rare_system::brightness::brightness_dim_idle(on)
}

#[tauri::command]
pub async fn brightness_idle(dim: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || rare_system::brightness::brightness_idle(dim)).await.map_err(|e| e.to_string())?
}
