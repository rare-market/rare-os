//! The Rare Local API and deep links: thin Tauri layer over the `rare-localapi` crate (server,
//! tokens, deep-link parsing, and their tests). This file gives it the runner, the launcher and
//! web game windows (`TauriLocalApi`) and keeps the `crate::localapi::...` paths and the
//! `#[tauri::command]`s working.

use rare_localapi::LocalApiHost;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub use rare_localapi::*;

struct TauriLocalApi(AppHandle);

impl LocalApiHost for TauriLocalApi {
    fn running(&self) -> Vec<String> {
        self.0.state::<crate::AppState>().runner.running()
    }
    fn main_exists(&self) -> bool {
        self.0.get_webview_window("main").is_some()
    }
    fn web_approved(&self, origin: &str) -> bool {
        rare_webgame::state().approved.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|o| o == origin)
    }
    fn web_current_id(&self) -> Option<String> {
        crate::webgame::current_id(&self.0)
    }
    fn web_origin(&self) -> Option<String> {
        self.0.get_webview_window(crate::webgame::LABEL)?.url().ok().map(|u| u.origin().ascii_serialization())
    }
    fn web_eval(&self, js: &str) {
        if let Some(w) = self.0.get_webview_window(crate::webgame::LABEL) {
            let _ = w.eval(js);
        }
    }
    fn show_launcher(&self) {
        if crate::webgame::visible() {
            crate::webgame::suspend();
            if let Some(w) = self.0.get_webview_window(crate::webgame::LABEL) {
                let _ = w.hide();
            }
        }
        crate::apps::front::to_launcher();
        if let Some(main) = self.0.get_webview_window("main") {
            let _ = main.unminimize();
            let _ = main.show();
            let _ = main.set_focus();
        }
    }
}

/// Hands the crate this app's windows. Idempotent; the overlay it opens is installed too.
fn ensure(app: &AppHandle) {
    crate::overlay::ensure(app);
    rare_localapi::install(crate::rare_ctx::ctx(app), Arc::new(TauriLocalApi(app.clone())));
}

/// Binds the loopback port and starts serving.
pub fn start(app: AppHandle) {
    ensure(&app);
    rare_localapi::start();
}

/// `rare:platform` from a web game window.
pub fn web_call(app: &AppHandle, origin: &str, token: &str, call: &str, params: Value) -> Result<Value, String> {
    ensure(app);
    rare_localapi::web_call(origin, token, call, params)
}

/// Opens a link: the overlay over the game, or a page or sheet in the launcher.
pub fn open_link(app: &AppHandle, link: &str) {
    ensure(app);
    rare_localapi::open_link(link);
}

/// The UI listens now: answers the deep links that came before.
#[tauri::command]
pub fn localapi_ready() -> Vec<deeplink::Route> {
    rare_localapi::localapi_ready()
}

/// The UI's answer to a `rare-api` request. `value` is the result, or `{ code, message }`.
#[tauri::command]
pub fn localapi_reply(id: u64, ok: bool, value: Value) {
    rare_localapi::localapi_reply(id, ok, value);
}

/// A notice about one of this game's items (the UI filters by the game's collections).
#[tauri::command]
pub fn localapi_notice(app: AppHandle, game_id: String, notice: Value) {
    ensure(&app);
    rare_localapi::localapi_notice(game_id, notice);
}

/// Where the API listens, for Settings > Developer and the docs' curl examples.
#[tauri::command]
pub fn localapi_info() -> Value {
    rare_localapi::localapi_info()
}
