//! Rare Trophies, native side: thin Tauri wrappers over the `rare-trophies` crate (Steam
//! achievement reading, the unlock watcher and their tests).

use tauri::AppHandle;

pub use rare_trophies::steam;

#[tauri::command]
pub async fn trophies_steam() -> Result<Vec<steam::GameAchievements>, String> {
    tauri::async_runtime::spawn_blocking(rare_trophies::trophies_steam).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trophies_steam_app(appid: u32) -> Result<Option<steam::GameAchievements>, String> {
    tauri::async_runtime::spawn_blocking(move || rare_trophies::trophies_steam_app(appid)).await.map_err(|e| e.to_string())
}

/// POST https://rare.market/v1/trophies/attest. Answers `{ status, body }` so service errors reach the UI.
#[tauri::command]
pub async fn trophy_attest(body: serde_json::Value) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || rare_trophies::trophy_attest(body).map_err(String::from)).await.map_err(|e| e.to_string())?
}

/// Polls Steam's stats files; new unlocks go to the UI as `steam-achievements`.
pub fn watch(app: AppHandle) {
    rare_trophies::watch(crate::rare_ctx::ctx(&app));
}
