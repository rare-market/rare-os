//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system).
use tauri::AppHandle;

pub use rare_system::devmode::*;

#[tauri::command]
pub fn dev_game_info() -> Option<DevGame> {
    rare_system::devmode::dev_game_info()
}

/// Opens (or reloads) the dev game. Waits for the launcher window and the Local API first.
pub fn launch(app: &AppHandle, g: DevGame) {
    set_active(g.clone());
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2500));
        eprintln!("[dev] opening {} as {}", g.url, g.id);
        match crate::webgame::open(&app, &g.id, &g.title, &g.url) {
            Ok(()) => crate::apps::front::to_front(&g.id),
            Err(e) => eprintln!("[dev] could not open the game: {e}"),
        }
    });
}
