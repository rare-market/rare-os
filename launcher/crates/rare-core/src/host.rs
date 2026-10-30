//! The few things only a front end can do. The Tauri shell implements [`Host`] over its windows
//! and the overlay module; the native launcher implements it over its own window. Every method
//! has a harmless default, so a crate's tests (and tools like a headless library scan) need none.
//!
//! This is the seam for code still living in the Tauri crate. As the overlay, freeze, power,
//! localapi and webgame crates are extracted, the methods that belong to them move there and
//! disappear from here.

use crate::Error;
use serde_json::Value;

/// The web game window's label (also the Tauri window label), shared by the runner and the host.
pub const WEB_WINDOW: &str = "webgame";

pub trait Host: Send + Sync {
    /// The launcher steps aside so an app's own window shows (desktop sessions).
    fn step_aside(&self) {}

    /// The set of run ids changed: power follows the next game's settings.
    fn power_running(&self, _now: &[String]) {}

    /// Whether the window with this label still exists.
    fn window_open(&self, _label: &str) -> bool {
        false
    }
    fn close_window(&self, _label: &str) {}

    /// Opens a web game or webview app in the web game window.
    fn open_web_window(&self, _run: &str, _title: &str, _url: &str, _profile: Option<Value>) -> Result<(), Error> {
        Err(Error::new("No window available"))
    }
    /// The run shown in the web game window now.
    fn current_web_id(&self) -> Option<String> {
        None
    }
    /// Shows the web game window again; false when there is none.
    fn resume_web(&self) -> bool {
        false
    }

    fn overlay_is_open(&self) -> bool {
        false
    }
    fn overlay_close(&self) -> Result<(), Error> {
        Ok(())
    }

    /// Debug line to stderr and, when the debug flag is set, the input log.
    fn log(&self, msg: &str) {
        eprintln!("{msg}");
    }
}

/// No front end: every hook does nothing.
pub struct NoHost;

impl Host for NoHost {}

// ---- the process-wide host, for free functions that have no `Ctx` to carry one ----

use std::sync::{Arc, OnceLock};

static GLOBAL: OnceLock<Arc<dyn Host>> = OnceLock::new();

/// Installs the process-wide host (once, at start; later calls are ignored). `Ctx` carries the
/// same host for commands; helpers like `launch::host_command_unit` read it from here.
pub fn set_global(h: Arc<dyn Host>) {
    let _ = GLOBAL.set(h);
}

/// The installed host, or one that does nothing.
pub fn global() -> Arc<dyn Host> {
    GLOBAL.get().cloned().unwrap_or_else(|| Arc::new(NoHost))
}
