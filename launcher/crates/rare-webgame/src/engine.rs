//! The browser engine that shows a web game, behind a trait so the host can change without the
//! wallet bridge, controller routing or window rules changing. The native launcher's engine is
//! `process::Spawner`: Chromium (CEF, launcher/native/webhost) in its own process and X11 window,
//! tagged with Rare's app id, driven over a Unix socket (open, eval, show, hide, focus, close,
//! navigation and closed events back). The old Tauri shell keeps its in-process WebKitGTK window.
//!
//! One web game at a time, in one window labelled [`LABEL`](crate::LABEL).

use std::path::PathBuf;
use std::sync::Arc;

/// What to open.
#[derive(Clone, Debug)]
pub struct OpenSpec {
    pub label: &'static str,
    pub title: String,
    pub url: String,
    /// Runs in every page before its own scripts (the controller profile, the Local API grant,
    /// webgame.js, the dev banner). The page gets no other bridge to Rare.
    pub init_script: String,
    /// One storage profile per game (cookies, localStorage, IndexedDB).
    pub data_dir: Option<PathBuf>,
    /// Rare Wallet and the Local API are in the page: web games always, webview apps (X, Telegram
    /// Web) only when their catalog way asks for it. False is "app mode": `init_script` carries no
    /// wallet and `rare-wallet:` requests are refused. Sent to the host as `"wallet"` (an older host
    /// ignores it, which is safe: the script is what injects the wallet).
    pub wallet: bool,
    /// The page's user agent, when a service gates on it (cloud gaming); the engine's own otherwise.
    pub user_agent: Option<String>,
    /// A cloud game stream: the engine decodes video on the GPU (VA-API) and skips WebGPU.
    pub stream: bool,
}

/// What the engine reports back.
pub trait EngineEvents: Send + Sync {
    /// The page is about to navigate. False blocks it (`rare-wallet:` requests and `rare://` links
    /// are handled here instead of loading).
    fn navigation(&self, url: &str) -> bool;
    /// The window is gone.
    fn closed(&self);
}

pub trait WebEngine: Send + Sync {
    /// Opens the window fullscreen over the launcher's size, shown and focused. An open window
    /// with the same label is closed first.
    fn open(&self, spec: &OpenSpec, events: Arc<dyn EngineEvents>) -> Result<(), String>;
    fn is_open(&self) -> bool;
    /// The page's current URL.
    fn url(&self) -> Option<String>;
    /// Runs a script in the page.
    fn eval(&self, js: &str);
    /// Shows the window and focuses it.
    fn show(&self);
    fn hide(&self);
    fn focus(&self);
    fn close(&self);
}
