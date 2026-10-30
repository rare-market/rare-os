//! What every backend crate shares, so the Tauri shell and the native launcher call the same code.
//!
//! - [`Ctx`]: handed to every command (`pub fn x(&Ctx, args) -> Result<T, Error>`): the event
//!   sink and the host hooks.
//! - [`Error`]: a message for the player (serialises as a plain string, like the Tauri commands'
//!   `String` errors it replaces).
//! - [`EventSink`] / [`Event`]: what `app.emit("name", payload)` was.
//! - [`paths`]: the config, cache, data and runtime directories.
//! - [`Host`]: the few things only a front end can do (hide the launcher window, open a web game
//!   window, the overlay), with no-op defaults so a crate's tests need none of it.

pub mod error;
pub mod events;
pub mod host;
pub mod paths;
pub mod proc;
pub mod types;

pub use error::Error;
pub use events::{Event, EventSink, NullSink};
pub use host::{Host, NoHost, WEB_WINDOW};
pub use types::{Pad, Profile, WebGame};

use std::sync::Arc;

/// Everything a command needs besides its arguments. Cheap to clone.
#[derive(Clone)]
pub struct Ctx {
    pub sink: Arc<dyn EventSink>,
    pub host: Arc<dyn Host>,
}

impl Ctx {
    pub fn new(sink: Arc<dyn EventSink>, host: Arc<dyn Host>) -> Ctx {
        Ctx { sink, host }
    }

    /// Events dropped, no host hooks: for tests and tools.
    pub fn headless() -> Ctx {
        Ctx { sink: Arc::new(NullSink), host: Arc::new(NoHost) }
    }

    pub fn send(&self, e: Event) {
        self.sink.send(e);
    }
}
