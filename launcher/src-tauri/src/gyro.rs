//! Gyro in Steam games: InputPlumber's pad target switched to `deck-uhid` while a Steam game runs.
//! Thin wrapper: the decision (`desired`, `Switch`), the D-Bus calls, the IMU rate and the watcher
//! live in the rare-input crate (launcher/crates/rare-input/src/gyro.rs); this module answers the
//! watcher's questions from the launcher's state.

pub use rare_input::gyro::*;

struct TauriGyro(tauri::AppHandle);

impl Host for TauriGyro {
    fn enabled(&self) -> bool {
        use tauri::Manager;
        let st = self.0.state::<crate::AppState>();
        let c = st.config.lock().unwrap();
        c.gyro_in_steam == Some(true) && c.gyro_enabled != Some(false)
    }
    fn running(&self) -> Vec<String> {
        use tauri::Manager;
        self.0.state::<crate::AppState>().runner.running()
    }
    fn gamescope_active(&self) -> bool {
        crate::overlay::gamescope::active()
    }
    fn focused_app(&self) -> Option<u64> {
        crate::overlay::gamescope::focused_app()
    }
}

/// Starts the watcher thread (Linux only; nothing elsewhere).
pub fn start(app: tauri::AppHandle) {
    rare_input::gyro::start(TauriGyro(app));
}

#[tauri::command]
pub fn input_status() -> PadStatus {
    rare_input::gyro::input_status()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::overlay::appid;
    use crate::overlay::gamescope::RARE_APPID;

    /// Setting on, Steam client up, nothing running, nothing known on screen.
    fn base() -> Inputs {
        Inputs { enabled: true, steam_client: true, steam_game: false, focused_app: None }
    }

    /// What Rare's runner holds for these run ids.
    fn game_in(running: &[&str]) -> bool {
        running.iter().any(|r| steam_game_run(r))
    }

    #[test]
    fn the_crates_app_id_is_the_launchers() {
        assert_eq!(rare_input::gyro::RARE_APPID, RARE_APPID);
    }

    #[test]
    fn chrome_app_in_front_is_never_steam() {
        // The device case: YouTube TV in Chrome (flatpak), started by Rare in its own scope,
        // Steam not running. Its windows carry the run's shortcut-range id.
        let chrome = appid::for_run("app:youtube") as u64;
        assert!(chrome >= 0x8000_0000);
        let i = Inputs { steam_client: false, steam_game: game_in(&["app:youtube"]), focused_app: Some(chrome), ..base() };
        assert_eq!(desired(i), Target::Xbox);
        // Same with Steam running quietly next to it.
        assert_eq!(desired(Inputs { steam_client: true, ..i }), Target::Xbox);
        // A web game in Chrome, the in-app browser, the launcher itself.
        for f in [appid::for_run("web:act") as u64, RARE_APPID as u64] {
            assert_eq!(desired(Inputs { focused_app: Some(f), steam_game: game_in(&["web:act"]), ..base() }), Target::Xbox);
        }
    }

    #[test]
    fn emulator_is_never_steam() {
        let emu = appid::for_run("emu:duckstation:/games/ff7.chd") as u64;
        let i = Inputs { steam_game: game_in(&["emu:duckstation:/games/ff7.chd"]), focused_app: Some(emu), ..base() };
        assert_eq!(desired(i), Target::Xbox);
        // A non-Steam shortcut launched through Steam: its id has the top bit set too.
        assert_eq!(desired(Inputs { steam_game: game_in(&["shortcut:3221225473"]), focused_app: Some(0xC000_0001), ..base() }), Target::Xbox);
    }

}
