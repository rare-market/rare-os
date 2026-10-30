//! Chords on the system buttons, shared across the input threads:
//!
//! - Rare (or the pad's Guide) + Y: screenshot; + View: save the last 30 seconds.
//! - Rare + volume: brightness (volume.rs).
//! - View + Menu held 0.6 s: the way out of any game or app (the Switcher), whatever has focus.
//!
//! The pad loop, the InputPlumber listener and the volume key reader run on different threads, so
//! "Rare is held" and "a chord used the held Rare" are atomics.

use crate::Pad;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The pad's Guide is physically held.
static GUIDE_HELD: AtomicBool = AtomicBool::new(false);
/// A chord on another thread used the held Guide: its release and hold do nothing more.
static GUIDE_USED: AtomicBool = AtomicBool::new(false);
/// The Rare button is held (ip.rs, the InputPlumber D-Bus action).
pub static RARE_HELD: AtomicBool = AtomicBool::new(false);
/// A chord fired while Rare was held: Rare's release is not also a Rare press.
pub static RARE_CHORD_USED: AtomicBool = AtomicBool::new(false);

/// The pad's Guide or the Rare button is held.
pub fn guide_held() -> bool {
    GUIDE_HELD.load(Ordering::Relaxed) || RARE_HELD.load(Ordering::Relaxed)
}

/// A chord on another thread (Rare + volume) used the held Guide or Rare.
pub fn guide_chord_used() {
    GUIDE_USED.store(true, Ordering::Relaxed);
    RARE_CHORD_USED.store(true, Ordering::Relaxed);
}

pub(crate) fn set_guide_held(on: bool) {
    GUIDE_HELD.store(on, Ordering::Relaxed);
}
pub(crate) fn clear_guide_used() {
    GUIDE_USED.store(false, Ordering::Relaxed);
}
pub(crate) fn take_guide_used() -> bool {
    GUIDE_USED.swap(false, Ordering::Relaxed)
}
pub(crate) fn guide_used() -> bool {
    GUIDE_USED.load(Ordering::Relaxed)
}

/// A capture chord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Share {
    /// A screenshot.
    Screenshot,
    /// The last 30 seconds.
    Clip,
}

/// Share chords with Guide (or Rare) held: Y saves a screenshot, View the last 30 seconds.
/// Steam's own Guide chords (R1, X, A, B, L1, d-pad, triggers) are left alone. `p` is already put
/// back for pads that swap X and Y, so Y is the physical Y.
pub fn share_chord(p: Pad) -> Option<Share> {
    match p {
        Pad::Y => Some(Share::Screenshot),
        Pad::Select => Some(Share::Clip),
        _ => None,
    }
}

/// The way out of any game or app: hold View and Menu (Select + Start) together.
///
/// In Game Mode Steam takes Guide (and its own "Exit game" would end Rare too), and Steam has no
/// View + Menu chord. It is read from evdev in the input thread, so it works whatever has focus.
/// The game does see both presses; the chord only counts once both are held for EXIT_HOLD.
pub const EXIT_HOLD: Duration = Duration::from_millis(600);

#[derive(Debug, Default)]
pub struct ExitChord {
    select: bool,
    start: bool,
    since: Option<Instant>,
    fired: bool,
}

impl ExitChord {
    pub const fn new() -> Self {
        ExitChord { select: false, start: false, since: None, fired: false }
    }

    /// View (`select`) or Menu (`start`) went down or up. Other buttons don't matter.
    pub fn set(&mut self, select: bool, down: bool, now: Instant) {
        if select {
            self.select = down;
        } else {
            self.start = down;
        }
        if self.select && self.start {
            if self.since.is_none() {
                self.since = Some(now);
                self.fired = false;
            }
        } else {
            self.since = None;
            self.fired = false;
        }
    }

    /// Called every input pass: true once, when both have been held for EXIT_HOLD.
    pub fn poll(&mut self, now: Instant) -> bool {
        match self.since {
            Some(t) if !self.fired && now.duration_since(t) >= EXIT_HOLD => {
                self.fired = true;
                true
            }
            _ => false,
        }
    }

    /// When `poll` will fire, if both are held.
    pub fn due(&self) -> Option<Instant> {
        self.since.filter(|_| !self.fired).map(|t| t + EXIT_HOLD)
    }

    pub fn reset(&mut self) {
        *self = ExitChord::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_chords() {
        assert_eq!(share_chord(Pad::Y), Some(Share::Screenshot));
        assert_eq!(share_chord(Pad::Select), Some(Share::Clip));
        // Steam's Guide chords and quick access stay free.
        for p in [Pad::A, Pad::B, Pad::X, Pad::Rb, Pad::Lb, Pad::Lt, Pad::Rt, Pad::Up, Pad::Start] {
            assert_eq!(share_chord(p), None);
        }
    }

    #[test]
    fn exit_chord_needs_both_held() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut c = ExitChord::new();
        c.set(true, true, t);
        assert!(!c.poll(ms(900))); // View alone never fires
        c.set(false, true, ms(1000));
        assert_eq!(c.due(), Some(ms(1600)));
        assert!(!c.poll(ms(1599)));
        assert!(c.poll(ms(1600)));
        assert!(!c.poll(ms(2500))); // once per hold
        assert_eq!(c.due(), None);
        // Let go of Menu and press it again: a new hold.
        c.set(false, false, ms(2600));
        assert!(!c.poll(ms(3500)));
        c.set(false, true, ms(3600));
        assert!(c.poll(ms(4200)));
        // A quick press of both is nothing.
        c.reset();
        c.set(true, true, t);
        c.set(false, true, ms(10));
        c.set(true, false, ms(200));
        assert!(!c.poll(ms(1000)));
    }
}
