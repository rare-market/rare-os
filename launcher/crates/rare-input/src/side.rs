//! The SuiPlay's two small top buttons, whatever has the screen:
//!
//! - **LC** press: the action Settings > Controls picked (`lc_action`, screenshot by default).
//!   Held HOLD: save the last 30 seconds.
//! - **RC** press: the action picked (`rc_action`, the on-screen keyboard by default). Held HOLD:
//!   the next performance overlay level (Off, FPS, Basic, Full).
//!
//! They arrive as InputPlumber D-Bus actions (`ui_screenshot`, `ui_osk`), which ip.rs receives and
//! the front end hands to `press`. This module decides what a press and a hold mean, including the
//! Settings > Controls choice (a press action set to "none" also turns that button's hold off);
//! the front end runs the action.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Held this long: the hold action, fired while still held.
pub const HOLD: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Lc,
    Rc,
}

/// What a press does, from the config's `lc_action` / `rc_action` (None: the model's default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Capture,
    Keyboard,
    Perf,
    Nothing,
}

pub fn action(k: Key, configured: Option<&str>) -> Action {
    match configured {
        Some("capture") => Action::Capture,
        Some("keyboard") => Action::Keyboard,
        Some("perf") => Action::Perf,
        Some("none") => Action::Nothing,
        _ => match k {
            Key::Lc => Action::Capture,
            Key::Rc => Action::Keyboard,
        },
    }
}

/// What a hold does: LC saves the last 30 seconds, RC steps the performance overlay. Off when the
/// press action is "none".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldAction {
    Clip,
    Perf,
}

pub fn hold_action(k: Key, configured: Option<&str>) -> Option<HoldAction> {
    if configured == Some("none") {
        return None;
    }
    Some(match k {
        Key::Lc => HoldAction::Clip,
        Key::Rc => HoldAction::Perf,
    })
}

/// A press that ended before HOLD, or a hold that came due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fire {
    Tap,
    Hold,
}

/// Press generation per key: a hold timer fires only for the press that started it.
static GEN: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
/// Set once a press's hold fired, so its release does nothing.
static HELD_FIRED: [AtomicU64; 2] = [AtomicU64::new(u64::MAX), AtomicU64::new(u64::MAX)];

fn idx(k: Key) -> usize {
    match k {
        Key::Lc => 0,
        Key::Rc => 1,
    }
}

/// A press (`down`) or release of LC or RC. `fire` runs once per press: `Hold` from a timer
/// thread while still held, or `Tap` on a release before that.
pub fn press(k: Key, down: bool, fire: Arc<dyn Fn(Key, Fire) + Send + Sync>) {
    let i = idx(k);
    if down {
        let g = GEN[i].fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::spawn(move || {
            std::thread::sleep(HOLD);
            // Still the same press, still held (a release bumps the generation).
            if GEN[i].load(Ordering::SeqCst) == g {
                HELD_FIRED[i].store(g, Ordering::SeqCst);
                fire(k, Fire::Hold);
            }
        });
    } else {
        let g = GEN[i].fetch_add(1, Ordering::SeqCst);
        if HELD_FIRED[i].load(Ordering::SeqCst) != g {
            fire(k, Fire::Tap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn press_actions_follow_settings_with_the_models_defaults() {
        assert_eq!(action(Key::Lc, None), Action::Capture);
        assert_eq!(action(Key::Rc, None), Action::Keyboard);
        assert_eq!(action(Key::Lc, Some("perf")), Action::Perf);
        assert_eq!(action(Key::Rc, Some("none")), Action::Nothing);
        assert_eq!(action(Key::Rc, Some("capture")), Action::Capture);
        assert_eq!(action(Key::Lc, Some("something new")), Action::Capture);
    }

    #[test]
    fn holds_follow_the_model_unless_the_button_is_off() {
        assert_eq!(hold_action(Key::Lc, None), Some(HoldAction::Clip));
        assert_eq!(hold_action(Key::Rc, Some("capture")), Some(HoldAction::Perf));
        assert_eq!(hold_action(Key::Lc, Some("none")), None);
    }

    #[test]
    fn a_tap_fires_on_release_and_a_hold_while_held() {
        let seen: Arc<Mutex<Vec<(Key, Fire)>>> = Arc::default();
        let s = seen.clone();
        let fire: Arc<dyn Fn(Key, Fire) + Send + Sync> = Arc::new(move |k, f| s.lock().unwrap().push((k, f)));
        press(Key::Rc, true, fire.clone());
        press(Key::Rc, false, fire.clone());
        assert_eq!(*seen.lock().unwrap(), vec![(Key::Rc, Fire::Tap)]);
        press(Key::Rc, true, fire.clone());
        std::thread::sleep(HOLD + Duration::from_millis(150));
        press(Key::Rc, false, fire.clone());
        std::thread::sleep(HOLD + Duration::from_millis(100));
        // The first press's timer saw its release; the second fired its hold and no tap.
        assert_eq!(*seen.lock().unwrap(), vec![(Key::Rc, Fire::Tap), (Key::Rc, Fire::Hold)]);
    }
}
