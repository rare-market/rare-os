//! PC game streaming: thin Tauri wrappers over the `rare-stream` crate (logic and tests live in
//! `launcher/crates/rare-stream`), plus the host it needs from the launcher's runner and Apps.

use rare_stream::cli::Settings;
use rare_stream::store::{HostView, StreamApp};
use rare_stream::StreamHost;
use std::process::Command;
use tauri::{AppHandle, Manager};

pub use rare_stream::{cli, pair, store, Status};
pub use rare_stream::{add, apps, refresh, scan, set_settings, wake};

/// The launcher side of streaming.
pub struct Host<'a>(pub &'a AppHandle);

impl StreamHost for Host<'_> {
    fn flatpak_installed(&self, id: &str) -> bool {
        crate::apps::flatpak_installed(id)
    }
    fn which(&self, program: &str) -> bool {
        crate::library::steam::which(program)
    }
    fn host_env(&self, cmd: &mut Command) {
        crate::launch::host_env(cmd);
    }
    fn host_command_unit(&self, program: &str, args: &[String], run_id: &str) -> (Command, Option<String>) {
        crate::launch::host_command_unit(program, args, run_id)
    }
    fn user_flathub(&self) -> bool {
        crate::apps::env().user_flathub
    }
    fn install_command(&self, flatpak: &str, user_flathub: bool) -> String {
        crate::apps::catalog::install_command(flatpak, user_flathub)
    }
    fn running(&self) -> Vec<String> {
        self.0.state::<crate::AppState>().runner.running()
    }
    fn quit_run(&self, id: &str) {
        let _ = self.0.state::<crate::AppState>().runner.quit(&crate::rare_ctx::ctx(self.0), id);
    }
    fn track_child(&self, run_id: &str, child: std::process::Child, unit: Option<String>) {
        self.0.state::<crate::AppState>().runner.track_child(run_id, child, unit);
    }
    fn to_front(&self, run_id: &str) {
        to_front(run_id)
    }
    fn step_aside(&self) {
        crate::apps::step_aside(&crate::rare_ctx::ctx(self.0));
    }
}

/// While a stream is in front the pad belongs to Moonlight: never Rare's pointer mode.
pub fn to_front(run_id: &str) {
    crate::apps::front::set_pointer(run_id, None);
    crate::apps::front::to_front(run_id);
}

pub fn quit(app: &AppHandle, uuid: &str) -> Result<(), String> {
    rare_stream::quit(&Host(app), uuid)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stream_status(app: AppHandle) -> Result<Status, String> {
    blocking(move || rare_stream::status(&Host(&app))).await
}

#[tauri::command]
pub async fn stream_refresh() -> Result<Vec<HostView>, String> {
    blocking(refresh).await
}

#[tauri::command]
pub async fn stream_discover(ms: Option<u64>) -> Result<Vec<HostView>, String> {
    blocking(move || scan(ms.unwrap_or(2500))).await
}

#[tauri::command]
pub async fn stream_add(address: String) -> Result<HostView, String> {
    blocking(move || add(&address)).await?
}

#[tauri::command]
pub async fn stream_pair_start(app: AppHandle, address: String) -> Result<pair::PairView, String> {
    blocking(move || rare_stream::pair_start(&Host(&app), &address)).await?
}

#[tauri::command]
pub fn stream_pair_poll() -> Option<pair::PairView> {
    pair::poll()
}

#[tauri::command]
pub fn stream_pair_cancel() {
    pair::cancel()
}

#[tauri::command]
pub async fn stream_apps(app: AppHandle, uuid: String) -> Result<Vec<StreamApp>, String> {
    blocking(move || apps(&Host(&app), &uuid)).await?
}

#[tauri::command]
pub async fn stream_launch(app: AppHandle, uuid: String, app_id: u32, replace: Option<bool>) -> Result<(), String> {
    blocking(move || rare_stream::launch(&Host(&app), &uuid, app_id, replace.unwrap_or(false))).await?
}

#[tauri::command]
pub fn stream_resume(app: AppHandle, id: String) -> Result<(), String> {
    rare_stream::resume(&Host(&app), &id)
}

#[tauri::command]
pub async fn stream_quit(app: AppHandle, uuid: String) -> Result<(), String> {
    blocking(move || quit(&app, &uuid)).await?
}

#[tauri::command]
pub async fn stream_wake(uuid: String) -> Result<(), String> {
    blocking(move || wake(&uuid)).await?
}

#[tauri::command]
pub fn stream_settings_set(settings: Settings) -> Result<(), String> {
    set_settings(settings)
}

/// Installs Moonlight from Flathub for this user (no sudo), with the Apps installer: progress
/// arrives as `app-install` events with id "moonlight".
#[tauri::command]
pub fn stream_install(app: AppHandle) -> Result<(), String> {
    let e = crate::apps::env();
    crate::apps::install::start(crate::rare_ctx::ctx(&app), "moonlight".into(), cli::FLATPAK.into(), e.user_flathub).map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shares front.rs's lock: the front state is process-wide.
    #[test]
    fn pointer_mode_is_off_while_streaming() {
        let _g = crate::apps::front::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use crate::apps::catalog::Back;
        use crate::apps::front;
        // Netflix in pointer mode has the screen: the pad is a mouse.
        front::set_pointer("app:netflix", Some(Back::AltLeft));
        front::to_front("app:netflix");
        assert!(front::pointer_active());
        // A stream starts: the pad goes to Moonlight, Rare sends no mouse or keys.
        let id = store::run_id("A", 7);
        to_front(&id);
        assert!(!front::pointer_active());
        assert_eq!(front::pointer_back(), None);
        assert!(!front::launcher_front());
        // Guide: Rare has the screen, the stream keeps running.
        front::to_launcher();
        assert!(front::launcher_front());
        // Resume: still no pointer mode, even with Netflix open behind it.
        to_front(&id);
        assert!(!front::pointer_active());
        front::sync(&[]);
        assert!(front::launcher_front());
    }
}
