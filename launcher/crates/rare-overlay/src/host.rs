//! What the overlay needs from the front end and from the parts of Rare that still live outside
//! this crate. The Tauri shell implements [`OverlayHost`] over its windows, the runner and the
//! modules not yet extracted; the native launcher implements it over its own window. Every method
//! has an inert default, so the crate's tests and tools need none of it.
//!
//! The overlay is a process-wide singleton (one screen, one focus), so the host and the event
//! sink are installed once with [`install`] and the functions take no handle.

use rare_core::{Ctx, Event, Pad};
use serde_json::Value;
use std::sync::{Arc, OnceLock};

pub trait OverlayHost: Send + Sync {
    /// A line for stderr and, with the debug flag set, the input log.
    fn log(&self, msg: &str) {
        eprintln!("{msg}");
    }

    // The launcher's main window.
    /// The window exists (the overlay needs it to come up).
    fn main_exists(&self) -> bool {
        false
    }
    /// The main window's X11 id (GTK on X11); None on Wayland or other platforms.
    fn main_xid(&self) -> Option<u64> {
        None
    }
    /// The X11 id of the window with this label.
    fn window_xid(&self, _label: &str) -> Option<u64> {
        None
    }
    /// Unminimize and show the main window.
    fn main_show(&self) {}
    fn main_focus(&self) {}
    fn main_focused(&self) -> bool {
        true
    }
    fn main_always_on_top(&self, _on: bool) {}
    fn main_minimize(&self) {}
    /// Labels of the visible windows other than the main one (a web game, the in-app browser).
    fn visible_other_windows(&self) -> Vec<String> {
        Vec::new()
    }
    fn hide_window(&self, _label: &str) -> bool {
        false
    }
    /// Shows a window again and focuses it.
    fn show_window(&self, _label: &str) {}
    /// Hides the web game window (it stays where it was).
    fn hide_web_window(&self) {}
    /// Rare runs as a Wayland client (`RARE_WEBKIT=wayland`).
    fn wayland_client(&self) -> bool {
        false
    }

    // The runner.
    fn running(&self) -> Vec<String> {
        Vec::new()
    }
    fn any_running(&self) -> bool {
        false
    }
    /// (run id, pid) of the programs Rare started as children.
    fn child_pids(&self) -> Vec<(String, u32)> {
        Vec::new()
    }
    /// The pids and all their descendants.
    fn with_descendants(&self, pids: &[u32]) -> Vec<u32> {
        pids.to_vec()
    }
    /// The keeper's look at gamescope's focusable app list: a game that ended hands the screen
    /// back, and the runs that now have a window count as shown (launch progress).
    fn focusable_apps_seen(&self, _apps: &[u64]) {}

    // What is in front (apps::front).
    fn pointer_active(&self) -> bool {
        false
    }
    fn launcher_front(&self) -> bool {
        true
    }
    fn steam_front(&self) -> bool {
        false
    }
    fn front_run(&self) -> Option<String> {
        None
    }
    fn to_launcher(&self) {}
    fn to_front(&self, _run: &str) {}

    // Web games.
    fn web_visible(&self) -> bool {
        false
    }
    /// The run id of the web game shown.
    fn web_current(&self) -> Option<String> {
        None
    }
    fn web_pad(&self, _button: Pad, _down: bool) {}
    /// The web game stops getting the controller; its window stays mapped.
    fn web_suspend(&self) {}
    /// Shows and focuses the web game window again and hands it the controller.
    fn web_resume(&self) {}

    // Input.
    /// A Rare surface is over a game (true) or went away (false).
    fn input_surface(&self, _up: bool) {}
    /// The virtual keyboard, so gamescope has it by the time keys go.
    fn pointer_prepare(&self) {}
    fn pointer_combo(&self, _keys: &[u16]) {}

    // Freezing (quick resume).
    fn thaw(&self, _run: &str) {}
    fn is_frozen(&self, _run: &str) -> bool {
        false
    }
    fn max_frozen(&self) -> usize {
        2
    }
    /// Freezes `run` if its decision says so. Some(payload) when the set is full: the `freeze-full`
    /// event the UI asks which paused game to close with.
    fn freeze_for_switch(&self, _run: &str) -> Option<Value> {
        None
    }
    /// Keeps the frozen set in step with what runs; a game waiting for room is returned once
    /// there is room for it.
    fn freeze_sync(&self, _running: &[String]) -> Option<String> {
        None
    }
    fn freeze_drop_pending(&self) {}

    /// Opens a steam:// URL in the running Steam client.
    fn steam_url(&self, _url: &str) -> Result<(), String> {
        Err("No Steam client".into())
    }
}

struct Inert;
impl OverlayHost for Inert {}

struct Env {
    ctx: Ctx,
    host: Arc<dyn OverlayHost>,
}

static ENV: OnceLock<Env> = OnceLock::new();
static INERT: Inert = Inert;

/// Installs the front end. The first call wins; later ones are ignored.
pub fn install(ctx: Ctx, host: Arc<dyn OverlayHost>) {
    let _ = ENV.set(Env { ctx, host });
}

pub fn installed() -> bool {
    ENV.get().is_some()
}

pub(crate) fn host() -> &'static dyn OverlayHost {
    match ENV.get() {
        Some(e) => e.host.as_ref(),
        None => &INERT,
    }
}

pub(crate) fn log(msg: &str) {
    host().log(msg);
}

/// Sends an event the web UI (or the native one) listens for.
pub(crate) fn emit(name: &str, payload: Value) {
    if let Some(e) = ENV.get() {
        e.ctx.send(Event::Other { name: name.to_string(), payload });
    }
}
