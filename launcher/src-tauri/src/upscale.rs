//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
pub use rare_system::upscale::*;

#[tauri::command]
pub fn upscale_get() -> Upscale {
    rare_system::upscale::upscale_get()
}

#[tauri::command]
pub async fn upscale_set(value: Upscale) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || rare_system::upscale::upscale_set(&value)).await.map_err(|e| e.to_string())?
}
