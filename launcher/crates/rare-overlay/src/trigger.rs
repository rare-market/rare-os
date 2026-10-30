//! The overlay's buttons: hold View for half a second while a game has the screen (the panel), or
//! hold View + Menu together (the way out: what is running, with Resume and Quit).
//!
//! Why this one: in Game Mode Steam owns Guide and the quick access button, and Guide+A is
//! InputPlumber's quick access chord. View is a plain button Steam passes through. A short press
//! stays a press for the game (web games get it as a tap once it's released); only a deliberate
//! hold opens the overlay. Any other button while View is down makes it a chord for the game.

use std::time::{Duration, Instant};

pub const HOLD: Duration = Duration::from_millis(500);

#[derive(Debug, PartialEq, Eq)]
pub enum Release {
    /// View went down and up quickly: an ordinary press for the game.
    Tap,
    /// The hold fired the overlay; the release belongs to it.
    Held,
    /// View was part of a chord; the game already got View down, so it gets the release too.
    Chord,
    /// Nothing was being tracked.
    Idle,
}

#[derive(Debug, Default)]
pub struct Hold {
    down_at: Option<Instant>,
    fired: bool,
    chord: bool,
}

impl Hold {
    pub const fn new() -> Self {
        Hold { down_at: None, fired: false, chord: false }
    }

    pub fn press(&mut self, now: Instant) {
        self.down_at = Some(now);
        self.fired = false;
        self.chord = false;
    }

    /// True while View is down and still might become the overlay hold.
    pub fn pending(&self) -> bool {
        self.down_at.is_some() && !self.fired && !self.chord
    }

    /// Another button went down while View was held: it's a chord for the game, not ours.
    /// Returns true when it turned a pending hold into a chord (the caller passes View down on).
    pub fn chord(&mut self) -> bool {
        if self.pending() {
            self.chord = true;
            return true;
        }
        false
    }

    pub fn release(&mut self, _now: Instant) -> Release {
        let r = match (self.down_at.take(), self.fired, self.chord) {
            (None, _, _) => Release::Idle,
            (Some(_), true, _) => Release::Held,
            (Some(_), _, true) => Release::Chord,
            (Some(_), false, false) => Release::Tap,
        };
        self.fired = false;
        self.chord = false;
        r
    }

    /// Called often (every input pass): true once, when the hold reaches HOLD.
    pub fn poll(&mut self, now: Instant) -> bool {
        match self.down_at {
            Some(t) if self.pending() && now.duration_since(t) >= HOLD => {
                self.fired = true;
                true
            }
            _ => false,
        }
    }

    pub fn reset(&mut self) {
        *self = Hold::new();
    }
}

/// The way out of any game or app: hold View and Menu (Select + Start) together.
///
/// Why this one: in Game Mode Steam takes Guide (and its own "Exit game" would end Rare too), and
/// Steam has no View + Menu chord. It is read from evdev in the input thread, so it works whatever
/// has focus. The game does see both presses; the chord only counts once both are held for EXIT_HOLD.
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

    pub fn reset(&mut self) {
        *self = ExitChord::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_chord_needs_both_held() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut c = ExitChord::new();
        c.set(true, true, t);
        assert!(!c.poll(ms(900))); // View alone never fires
        c.set(false, true, ms(1000));
        assert!(!c.poll(ms(1599)));
        assert!(c.poll(ms(1600)));
        assert!(!c.poll(ms(2500))); // once per hold
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

    #[test]
    fn a_quick_press_is_a_tap() {
        let t = Instant::now();
        let mut h = Hold::new();
        h.press(t);
        assert!(!h.poll(t + Duration::from_millis(200)));
        assert_eq!(h.release(t + Duration::from_millis(250)), Release::Tap);
        assert!(!h.poll(t + Duration::from_millis(900)));
    }

    #[test]
    fn holding_fires_once() {
        let t = Instant::now();
        let mut h = Hold::new();
        h.press(t);
        assert!(!h.poll(t + Duration::from_millis(499)));
        assert!(h.poll(t + Duration::from_millis(500)));
        assert!(!h.poll(t + Duration::from_millis(800)));
        assert_eq!(h.release(t + Duration::from_millis(900)), Release::Held);
    }

    #[test]
    fn another_button_makes_a_chord() {
        let t = Instant::now();
        let mut h = Hold::new();
        h.press(t);
        assert!(h.chord());
        assert!(!h.chord());
        assert!(!h.poll(t + Duration::from_millis(700)));
        assert_eq!(h.release(t + Duration::from_millis(800)), Release::Chord);
    }

    #[test]
    fn a_release_without_press_is_idle() {
        let mut h = Hold::new();
        assert_eq!(h.release(Instant::now()), Release::Idle);
        assert!(!h.chord());
    }
}
