//! The `rare_core::Ctx` the Tauri shell hands to the extracted backend crates: events go out as
//! `app.emit`, host hooks call the modules that still live in this crate.

use rare_core::{Ctx, Event, EventSink, Host};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn send(&self, e: Event) {
        // Events that were `main` window events.
        if e.main_only() || matches!(e.name(), "share-captured" | "share-error" | "overlay" | "rare-api" | "rare-api-cancel" | "deep-link" | "wallet-request" | "wallet-connected" | "steam-achievements") {
            let _ = self.0.emit_to("main", e.name(), e.payload());
        } else {
            let _ = self.0.emit(e.name(), e.payload());
        }
    }
}

/// Events through `app.emit`. Host hooks default to doing nothing until the crate that needs one is wired in.
pub fn ctx(app: &AppHandle) -> Ctx {
    let host: Arc<dyn Host> = Arc::new(crate::ctx::TauriHost(app.clone()));
    Ctx::new(Arc::new(TauriSink(app.clone())), host)
}
