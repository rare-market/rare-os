//! What the device layer needs from the rest of the launcher, without depending on it.
//!
//! The launcher installs a `Host` once at start (`set`); until then every hook is a safe
//! default (no gamescope, nothing in front, no log). This keeps rare-system free of the
//! launch, overlay, input and wallet crates, which are separate workstreams.

use std::process::Command;
use std::sync::OnceLock;

pub trait Host: Send + Sync {
    /// Gives a command the host environment instead of the AppImage's (`launch::host_env`).
    fn env(&self, _cmd: &mut Command) {}
    /// A line in the launcher's log (`gamepad::log`).
    fn log(&self, line: &str) {
        eprintln!("{line}");
    }
    /// gamescope is the compositor (`overlay::gamescope::active`).
    fn gamescope_active(&self) -> bool {
        false
    }
    /// The app id of gamescope's focused window.
    fn gamescope_focused_app(&self) -> Option<u64> {
        None
    }
    /// The X display of gamescope's base layer.
    fn gamescope_base_display(&self) -> String {
        String::new()
    }
    /// The launcher itself is the front app and no web game is on top (`apps::front::launcher_front`).
    fn launcher_front(&self) -> bool {
        true
    }
    /// Bluetooth power: Some(powered) when there is an adapter (`wallet::bluetooth::state_now`).
    fn bluetooth_state(&self) -> Option<bool> {
        None
    }
    fn bluetooth_set(&self, _on: bool) -> Result<(), String> {
        Err("Bluetooth isn't available".into())
    }
    /// Whether a URL may be a web game (`webgame::url_allowed`).
    fn web_url_allowed(&self, u: &url::Url) -> bool {
        u.scheme() == "https" || (u.scheme() == "http" && matches!(u.host_str(), Some("localhost" | "127.0.0.1")))
    }
}

struct Nothing;
impl Host for Nothing {}

static HOST: OnceLock<Box<dyn Host>> = OnceLock::new();

/// Installs the host (once; later calls are ignored).
pub fn set(h: impl Host + 'static) {
    let _ = HOST.set(Box::new(h));
}

pub fn get() -> &'static dyn Host {
    HOST.get().map(|b| b.as_ref()).unwrap_or(&Nothing)
}

/// `host_env` for a command.
pub fn env(cmd: &mut Command) -> &mut Command {
    get().env(cmd);
    cmd
}

pub fn log(line: &str) {
    get().log(line);
}
