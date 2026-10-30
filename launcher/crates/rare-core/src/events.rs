//! Events the backend sends to whichever front end is running. The Tauri shell turns each into
//! `app.emit(event.name(), event.payload())`; the native launcher matches on the enum.
//!
//! Adding an event: a variant, its `name`, its `payload`. Keep names identical to the web UI's.

use serde::Serialize;
use serde_json::{json, Value};

/// A start that went wrong, for the player (`launch-failed`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchFailed {
    pub id: String,
    /// Didn't start: "steam" (Steam never ran it), "steamui" (Steam never came up), "never" (the
    /// launcher it went through, Heroic, never started it), "quick" (gone before a window, with
    /// an error), "stuck" (still running, no window after `NO_WINDOW`).
    /// Closed unexpectedly: "crash" (within `CRASH_WINDOW` of its first window, or a non-zero exit).
    pub reason: &'static str,
    /// The program's exit code, when Rare started it.
    pub code: Option<i32>,
}

/// The run ids that are open now (`game-state`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningState {
    pub running: Vec<String>,
}

/// Progress of a Flathub install (`app-install`).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstallEvent {
    pub id: String,
    /// 0 to 1, when flatpak reports it.
    pub progress: Option<f32>,
    pub done: bool,
    pub error: Option<String>,
}

/// Progress of a Proton download (`proton-install`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtonProgress {
    pub kind: String,
    pub phase: String,
    pub done: u64,
    pub total: u64,
}

/// Progress of an emulator update (`emu-update-progress`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmuProgress {
    pub id: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A games folder changed (`library-changed`).
    LibraryChanged,
    /// Steam is about to take the screen; the value is how long to show the hint, in ms (`steam-handoff`).
    SteamHandoff(u64),
    LaunchFailed(LaunchFailed),
    GameState(RunningState),
    AppInstall(InstallEvent),
    ProtonInstall(ProtonProgress),
    EmuUpdateProgress(EmuProgress),
    /// Any other event by name, for crates that have not got a variant yet.
    Other { name: String, payload: Value },
}

impl Event {
    /// The event name the web UI listens for.
    pub fn name(&self) -> &str {
        match self {
            Event::LibraryChanged => "library-changed",
            Event::SteamHandoff(_) => "steam-handoff",
            Event::LaunchFailed(_) => "launch-failed",
            Event::GameState(_) => "game-state",
            Event::AppInstall(_) => "app-install",
            Event::ProtonInstall(_) => "proton-install",
            Event::EmuUpdateProgress(_) => "emu-update-progress",
            Event::Other { name, .. } => name,
        }
    }

    /// The payload as JSON, exactly what `app.emit` was given.
    pub fn payload(&self) -> Value {
        fn to<T: Serialize>(v: &T) -> Value {
            serde_json::to_value(v).unwrap_or(Value::Null)
        }
        match self {
            Event::LibraryChanged => Value::Null,
            Event::SteamHandoff(ms) => json!(ms),
            Event::LaunchFailed(v) => to(v),
            Event::GameState(v) => to(v),
            Event::AppInstall(v) => to(v),
            Event::ProtonInstall(v) => to(v),
            Event::EmuUpdateProgress(v) => to(v),
            Event::Other { payload, .. } => payload.clone(),
        }
    }

    /// True when only the main window gets it (`emit_to("main", ..)`), not every window.
    pub fn main_only(&self) -> bool {
        matches!(self, Event::LibraryChanged | Event::SteamHandoff(_))
    }
}

/// Where events go. Implemented by the Tauri shell (emit) and by the native UI (a channel).
pub trait EventSink: Send + Sync {
    fn send(&self, event: Event);
}

impl<F: Fn(Event) + Send + Sync> EventSink for F {
    fn send(&self, event: Event) {
        self(event)
    }
}

/// Drops every event.
pub struct NullSink;

impl EventSink for NullSink {
    fn send(&self, _: Event) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn names_and_payloads_match_the_web_ui() {
        assert_eq!(Event::LibraryChanged.name(), "library-changed");
        assert_eq!(Event::LibraryChanged.payload(), Value::Null);
        assert_eq!(Event::SteamHandoff(1600).payload(), json!(1600));
        let e = Event::LaunchFailed(LaunchFailed { id: "steam:1".into(), reason: "quick", code: Some(1) });
        assert_eq!(e.name(), "launch-failed");
        assert_eq!(e.payload(), json!({ "id": "steam:1", "reason": "quick", "code": 1 }));
        let p = Event::ProtonInstall(ProtonProgress { kind: "ge".into(), phase: "download".into(), done: 1, total: 2 });
        assert_eq!(p.payload(), json!({ "kind": "ge", "phase": "download", "done": 1, "total": 2 }));
        assert!(Event::SteamHandoff(1).main_only() && !e.main_only());
    }

    #[test]
    fn a_closure_is_a_sink() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let sink: Arc<dyn EventSink> = Arc::new(move |e: Event| s.lock().unwrap().push(e));
        sink.send(Event::LibraryChanged);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
