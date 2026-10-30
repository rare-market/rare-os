//! Input resets: a moment after which no held button, direction or axis from before may repeat.
//!
//! The launcher reads the pad on two paths that InputPlumber's intercept mode switches between:
//! evdev from its virtual pad (`Pass`) and D-Bus `ui_*` actions (`Always`, a Rare surface over a
//! game). A press that starts on one path and ends on the other, or ends while the target pad is
//! re-created, never reaches the state machine that holds it, and that direction auto-repeats
//! until it is pressed again. InputPlumber 0.83.0 (the SuiPlay image) also never releases what its
//! D-Bus target holds when the mode goes back to `Pass`. So every switch of the input's owner asks
//! for a reset: an intercept mode change (ip.rs), a pad target change (gyro.rs), the launcher's
//! window gaining or losing focus, a Rare surface opening or closing, and a resume from sleep.
//! Each state machine then lets go of what it holds and keeps only what a fresh read of the pad
//! (`evdev::probe`) still shows held.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static EPOCH: AtomicU64 = AtomicU64::new(0);
static LAST: Mutex<Option<Instant>> = Mutex::new(None);
type Hook = Box<dyn Fn() + Send + Sync>;
static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());

/// Asks every input state machine to let go of what it holds (`why` goes to the input log).
pub fn request(why: &str) {
    {
        let mut l = LAST.lock().unwrap_or_else(|e| e.into_inner());
        *l = Some(Instant::now());
        EPOCH.fetch_add(1, Ordering::SeqCst);
    }
    if crate::debug_input() {
        crate::log(&format!("[input] reset: {why}"));
    }
    for h in HOOKS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        h();
    }
}

/// Counts resets: a loop that polls compares it with the value it last saw.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::SeqCst)
}

/// The epoch and when the latest reset was asked for (input read before it belongs to the old
/// owner).
pub fn last() -> (u64, Instant) {
    let l = LAST.lock().unwrap_or_else(|e| e.into_inner());
    (EPOCH.load(Ordering::SeqCst), l.unwrap_or_else(Instant::now))
}

/// Runs `f` on every reset (on the requesting thread: send yourself a message, don't block).
pub fn on_request(f: impl Fn() + Send + Sync + 'static) {
    HOOKS.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    #[test]
    fn a_request_bumps_the_epoch_and_runs_the_hooks() {
        let n = Arc::new(AtomicUsize::new(0));
        let m = n.clone();
        on_request(move || {
            m.fetch_add(1, Ordering::SeqCst);
        });
        let e = epoch();
        request("test");
        assert!(epoch() > e);
        assert!(n.load(Ordering::SeqCst) >= 1);
    }
}
