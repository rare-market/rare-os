//! Browser games as installed web apps: thin Tauri layer over the `rare-webgame` crate (wallet
//! bridge, controller routing, window rules, tests). This file is the WebKitGTK browser-engine
//! host (`WebKitEngine`, a Tauri webview window) behind the crate's `WebEngine` trait, so a CEF
//! host in its own process can replace it, and keeps the old `crate::webgame::...` paths and
//! `#[tauri::command]`s working.

use crate::gamepad::Pad;
use rare_webgame::engine::{EngineEvents, OpenSpec, WebEngine};
use rare_webgame::WebHost;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub use rare_webgame::{url_allowed, visible, wants_chrome, WalletSession, LABEL};

/// The window of one web game, as a Tauri webview (WebKitGTK on Linux).
struct WebKitEngine(AppHandle);

impl WebKitEngine {
    fn window(&self) -> Option<tauri::WebviewWindow> {
        self.0.get_webview_window(LABEL)
    }
}

impl WebEngine for WebKitEngine {
    fn open(&self, spec: &OpenSpec, events: Arc<dyn EngineEvents>) -> Result<(), String> {
        let app = &self.0;
        if let Some(w) = self.window() {
            let _ = w.close();
        }
        let url: tauri::Url = spec.url.parse().map_err(|e: url::ParseError| e.to_string())?;
        let nav = events.clone();
        let mut b = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::External(url))
            .title(&spec.title)
            .initialization_script(&spec.init_script)
            .on_navigation(move |u| nav.navigation(u.as_str()));
        if let Some(dir) = &spec.data_dir {
            b = b.data_directory(dir.clone());
        }
        if let Some(main) = app.get_webview_window("main") {
            if let (Ok(size), Ok(scale)) = (main.inner_size(), main.scale_factor()) {
                b = b.inner_size(size.width as f64 / scale, size.height as f64 / scale);
            }
            if main.is_fullscreen().unwrap_or(false) || std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() {
                b = b.fullscreen(true).decorations(false);
            }
        }
        let w = b.build().map_err(|e| e.to_string())?;
        w.on_window_event(move |e| {
            if matches!(e, tauri::WindowEvent::Destroyed) {
                events.closed();
            }
        });
        Ok(())
    }
    fn is_open(&self) -> bool {
        self.window().is_some()
    }
    fn url(&self) -> Option<String> {
        self.window()?.url().ok().map(|u| u.to_string())
    }
    fn eval(&self, js: &str) {
        if let Some(w) = self.window() {
            let _ = w.eval(js);
        }
    }
    fn show(&self) {
        if let Some(w) = self.window() {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
    fn hide(&self) {
        if let Some(w) = self.window() {
            let _ = w.hide();
        }
    }
    fn focus(&self) {
        if let Some(w) = self.window() {
            let _ = w.set_focus();
        }
    }
    fn close(&self) {
        if let Some(w) = self.window() {
            let _ = w.close();
        }
    }
}

/// The launcher side of the web game code.
struct Launcher(AppHandle);

impl WebHost for Launcher {
    fn main_exists(&self) -> bool {
        self.0.get_webview_window("main").is_some()
    }
    fn main_show(&self) {
        if let Some(main) = self.0.get_webview_window("main") {
            let _ = main.show();
        }
    }
    fn main_focus(&self) {
        if let Some(main) = self.0.get_webview_window("main") {
            let _ = main.set_focus();
        }
    }
    fn testnet(&self) -> bool {
        crate::network::is_testnet(&self.0.state::<crate::AppState>().config.lock().unwrap())
    }
}

/// Hands the crate this app's windows. Idempotent; the overlay and the Local API it relies on
/// are installed too.
fn ensure(app: &AppHandle) {
    crate::overlay::ensure(app);
    if !rare_webgame::installed() {
        rare_webgame::install(crate::rare_ctx::ctx(app), Arc::new(WebKitEngine(app.clone())), Arc::new(Launcher(app.clone())));
    }
}

pub fn open(app: &AppHandle, game_id: &str, title: &str, url: &str) -> Result<(), String> {
    ensure(app);
    rare_webgame::open(game_id, title, url)
}

/// `open`, with a controller profile given by the caller (apps) instead of profiles.json.
pub fn open_with(app: &AppHandle, game_id: &str, title: &str, url: &str, profile: Option<Value>) -> Result<(), String> {
    ensure(app);
    rare_webgame::open_with(game_id, title, url, profile)
}

/// Full analog controller state for the open web game (virtual Gamepad, pointer stick).
pub fn pad_state(_app: &AppHandle, buttons: &[f32; 17], axes: &[f32; 4]) {
    rare_webgame::pad_state(buttons, axes);
}

/// Routes a controller event to the open web game.
pub fn pad(_app: &AppHandle, button: Pad, down: bool) -> bool {
    rare_webgame::pad(crate::overlay::to_core(button), down)
}

/// Id of the game or app in the web window, if one is open.
pub fn current_id(_app: &AppHandle) -> Option<String> {
    rare_webgame::current_id()
}

pub fn suspend() {
    rare_webgame::suspend();
}

/// Brings a running web game back to the screen.
pub fn resume(_app: &AppHandle) -> bool {
    rare_webgame::resume()
}

/// The account the launcher's Rare Wallet has selected, pushed from the UI.
pub fn set_session(session: WalletSession) {
    *rare_webgame::state().session.lock().unwrap() = session;
}

/// The UI needs to show a sheet for a request Rust expected to answer silently.
#[tauri::command]
pub fn wallet_focus(app: AppHandle, id: u64) {
    ensure(&app);
    rare_webgame::wallet_focus(id);
}

/// The launcher UI answers a web game request. `value` is the result JSON, or an error string.
#[tauri::command]
pub fn wallet_reply(app: AppHandle, id: u64, ok: bool, value: Value) {
    ensure(&app);
    rare_webgame::wallet_reply(id, ok, value);
}

/// Origins the player approved for the current session.
#[tauri::command]
pub fn wallet_set_approved(origins: Vec<String>) {
    rare_webgame::wallet_set_approved(origins);
}
