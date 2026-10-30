//! The Share button: thin Tauri wrappers over the `rare-share` crate. The capture logic, tools
//! and tests live in `launcher/crates/rare-share`; this file gives it the launcher's state.

use rare_share::ShareHost;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub use rare_share::*;

struct TauriShare(AppHandle);

impl ShareHost for TauriShare {
    fn clips_enabled(&self) -> bool {
        self.0.state::<crate::AppState>().config.lock().unwrap().share_clips.unwrap_or(true)
    }
    fn running(&self) -> Vec<String> {
        self.0.state::<crate::AppState>().runner.running()
    }
    fn title_of(&self, id: &str) -> Option<String> {
        let st = self.0.state::<crate::AppState>();
        let lib = st.library.lock().unwrap();
        lib.as_ref().and_then(|l| l.games.iter().find(|g| g.id == id).map(|g| g.title.clone()))
    }
    fn rumble_enabled(&self) -> bool {
        self.0.state::<crate::AppState>().config.lock().unwrap().rumble != Some(false)
    }
    fn rumble(&self) {
        let _ = crate::gamepad::rumble("capture", 1.0);
    }
    fn launcher_front(&self) -> bool {
        crate::apps::front::launcher_front()
    }
    fn toast(&self, text: &str) {
        crate::hud::toast(text, None);
    }
}

fn host(app: &AppHandle) -> Arc<dyn ShareHost> {
    Arc::new(TauriShare(app.clone()))
}

/// Starts the clip recorder supervisor.
pub fn start(app: AppHandle) {
    rare_share::set_host_env(|cmd| {
        crate::launch::host_env(cmd);
    });
    rare_share::start(host(&app));
}

/// A controller or key chord asked for a capture.
pub fn chord(app: &AppHandle, c: Chord) {
    rare_share::chord(&crate::rare_ctx::ctx(app), &host(app), c);
}

#[tauri::command]
pub async fn share_status(app: AppHandle) -> Result<ShareStatus, String> {
    tauri::async_runtime::spawn_blocking(move || status(&TauriShare(app))).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn share_set_clips(app: AppHandle, enabled: bool) -> Result<ShareStatus, String> {
    {
        let st = app.state::<crate::AppState>();
        let mut c = st.config.lock().unwrap();
        c.share_clips = Some(enabled);
        crate::config::save(&c)?;
    }
    tauri::async_runtime::spawn_blocking(move || status(&TauriShare(app))).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn share_screenshot(app: AppHandle) -> Result<Capture, String> {
    tauri::async_runtime::spawn_blocking(move || take_screenshot(&TauriShare(app))).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn share_clip(app: AppHandle) -> Result<Capture, String> {
    tauri::async_runtime::spawn_blocking(move || save_clip(&TauriShare(app))).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn share_list() -> Result<Vec<Capture>, String> {
    tauri::async_runtime::spawn_blocking(list).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn share_read(id: String) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || read(&id)).await.map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn share_delete(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete(&id)).await.map_err(|e| e.to_string())?
}
