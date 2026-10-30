//! The Tauri shell's side of rare-core's `Host`: the window, overlay, web game and gamescope
//! calls that rare-launch and rare-emu make through `Ctx.host` (or `host::global()` in free
//! functions). `rare_ctx::ctx` builds the `Ctx` with this host; `install_global` makes the same
//! host the process-wide one once the app exists.

use rare_core::{Error, Host};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub struct TauriHost(pub AppHandle);

impl Host for TauriHost {
    fn step_aside(&self) {
        if let Some(main) = self.0.get_webview_window("main") {
            let _ = main.minimize();
        }
    }
    fn power_running(&self, now: &[String]) {
        crate::power::running(&self.0, now);
    }
    fn window_open(&self, label: &str) -> bool {
        self.0.get_webview_window(label).is_some()
    }
    fn close_window(&self, label: &str) {
        if let Some(w) = self.0.get_webview_window(label) {
            let _ = w.close();
        }
    }
    fn open_web_window(&self, run: &str, title: &str, url: &str, profile: Option<serde_json::Value>) -> Result<(), Error> {
        Ok(crate::webgame::open_with(&self.0, run, title, url, profile)?)
    }
    fn current_web_id(&self) -> Option<String> {
        crate::webgame::current_id(&self.0)
    }
    fn resume_web(&self) -> bool {
        crate::webgame::resume(&self.0)
    }
    fn overlay_is_open(&self) -> bool {
        crate::overlay::is_open()
    }
    fn overlay_close(&self) -> Result<(), Error> {
        Ok(crate::overlay::close(&self.0)?)
    }
    fn log(&self, msg: &str) {
        crate::gamepad::log(msg);
    }
}

/// Makes the Tauri host the process-wide one (call once, from setup).
pub fn install_global(app: &AppHandle) {
    rare_core::host::set_global(Arc::new(TauriHost(app.clone())));
}
