//! The backends' events, queued for the UI thread. Crates take a `rare_core::Ctx`; this is the one
//! the native launcher gives them, and `App::update` drains it.

use rare_core::{Ctx, Event};
use std::sync::{Arc, Mutex, OnceLock};

static QUEUE: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static WAKER: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Asks the frame loop for an update whenever an event arrives.
pub fn set_waker(w: Arc<dyn Fn() + Send + Sync>) {
    let _ = WAKER.set(w);
}

pub fn ctx() -> Ctx {
    Ctx::new(
        Arc::new(|e: Event| {
            if let Ok(mut q) = QUEUE.lock() {
                if q.len() < 512 {
                    q.push(e);
                }
            }
            if let Some(w) = WAKER.get() {
                w();
            }
        }),
        // Web game windows (rare-launch opens, tracks and quits them through the host).
        Arc::new(super::webgame::WebWindows),
    )
}

pub fn drain() -> Vec<Event> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// Asks the frame loop for an update (a worker thread finished something).
pub fn wake() {
    if let Some(w) = WAKER.get() {
        w();
    }
}

/// A line for the player from a worker thread: shown as a toast.
pub fn note(text: impl Into<String>, error: bool) {
    ctx().send(Event::Other { name: "note".into(), payload: serde_json::json!({ "text": text.into(), "error": error }) });
}
