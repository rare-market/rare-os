//! What the Local API needs from the front end: the runner's list, the web game window and the
//! launcher window. The Tauri shell implements [`LocalApiHost`]; the native launcher implements it
//! over its own windows. Installed once with [`install`] (the API is a process-wide singleton: one
//! loopback port, one set of tokens).

use rare_core::{Ctx, Event};
use std::sync::{Arc, OnceLock};

pub trait LocalApiHost: Send + Sync {
    /// Run ids of the games running now (tokens of the rest go away).
    fn running(&self) -> Vec<String> {
        Vec::new()
    }
    /// The launcher's main window exists, so a request for the UI can be delivered.
    fn main_exists(&self) -> bool {
        false
    }
    /// The player approved this origin for Rare Wallet (set by the launcher UI).
    fn web_approved(&self, _origin: &str) -> bool {
        false
    }
    /// The run id of the game or app in the web window, if one is open.
    fn web_current_id(&self) -> Option<String> {
        None
    }
    /// The web window's real origin.
    fn web_origin(&self) -> Option<String> {
        None
    }
    /// Runs a script in the web game's page.
    fn web_eval(&self, _js: &str) {}
    /// Brings the launcher in front of the game, the way Guide does (the game keeps running): the
    /// web game window is suspended and hidden, the pad is the launcher's, the main window is raised.
    fn show_launcher(&self) {}
    /// The virtual pad for network apps: made while "Send controller input" and the network
    /// listener are both on, removed (buttons released) when either goes off.
    fn net_input_ready(&self, _on: bool) {}
    /// Presses and releases a network app asked for (checked and normalised by `validate`).
    fn net_input(&self, _events: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("Controller input isn't available here".into())
    }
    /// The screen as an image: (content type, bytes).
    fn net_screenshot(&self) -> Result<(String, Vec<u8>), String> {
        Err("Screenshots aren't available here".into())
    }
}

struct Inert;
impl LocalApiHost for Inert {}

struct Env {
    ctx: Ctx,
    host: Arc<dyn LocalApiHost>,
}

static ENV: OnceLock<Env> = OnceLock::new();
static INERT: Inert = Inert;

/// Installs the front end. The first call wins.
pub fn install(ctx: Ctx, host: Arc<dyn LocalApiHost>) {
    let _ = ENV.set(Env { ctx, host });
}

pub(crate) fn host() -> &'static dyn LocalApiHost {
    match ENV.get() {
        Some(e) => e.host.as_ref(),
        None => &INERT,
    }
}

/// Events for the launcher's UI (`rare-api`, `rare-api-cancel`, `deep-link`): main window only.
pub(crate) fn emit(event: Event) {
    if let Some(e) = ENV.get() {
        e.ctx.send(event);
    }
}
