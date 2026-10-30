//! The controller state machine: raw events from any pad in, launcher presses out.
//!
//! Pure: time comes in, outputs go out, so the Tauri shell's gilrs loop and the native launcher's
//! evdev readers share one implementation and one set of tests. What it does:
//!
//! - D-pad and left stick (radial, with hysteresis) are directions; a held direction repeats
//!   after 320 ms, 120 ms apart easing to 40 ms.
//! - LT and RT are analog: a press above 0.6, a release below 0.3, no repeat.
//! - One pad at a time: the pad used last owns input until it is quiet for 1.5 s (Handheld
//!   Daemon's and Steam's virtual pads can both show the same presses).
//! - Contact bounce, presses right after a (re)connect and a double-reported A are dropped.
//! - Guide: a press on release; held 0.6 s, GuideHold (the Switcher) and no press. Guide + A is
//!   quick access (as InputPlumber and Steam do). Guide or Rare + Y / View are the share chords.
//! - View + Menu held 0.6 s: the way out (`Out::Exit`), whatever has focus.
//! - The right stick goes out as analog values (chart pan and zoom, scrolling), at most every
//!   33 ms, and once more when it comes back to rest.
//! - A held direction only repeats while it is still held: a stick direction while that pad's
//!   latest value is past the threshold, and when its last event is older than `FRESH` the pad is
//!   read again (`set_probe`, evdev's EVIOCGABS / EVIOCGKEY) before the repeat fires. `reset` (an
//!   intercept mode or pad target change, window focus, a Rare surface, resume: reset.rs) lets go
//!   of everything a fresh read doesn't show held.

use crate::chords::{self, ExitChord, Share};
use crate::{is_dir, log, Pad};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// A standard gamepad button (gilrs' names; evdev codes in evdev.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Btn {
    South,
    East,
    North,
    West,
    LeftTrigger,
    RightTrigger,
    LeftTrigger2,
    RightTrigger2,
    Select,
    Start,
    Mode,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    LeftStickX,
    /// Up positive.
    LeftStickY,
    RightStickX,
    /// Up positive.
    RightStickY,
    /// Left trigger, 0..=1.
    LeftZ,
    /// Right trigger, 0..=1.
    RightZ,
}

/// One input event of a pad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    Pressed(Btn),
    Released(Btn),
    /// An analog button's value (triggers), 0..=1.
    Changed(Btn, f32),
    /// A stick axis, -1..=1, up positive.
    Axis(Axis, f32),
    /// The quick access button (only the Deck report has one; on the Xbox pad it is Guide + A).
    Qam(bool),
    Connected,
    Disconnected,
}

/// Which pad an event came from (any stable number per device).
pub type Src = u64;
/// The virtual Deck pad read from hidraw (deck.rs).
pub const DECK: Src = u64::MAX;

/// What the launcher does with an event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    /// A press (or the repeat of a held direction).
    Press(Pad, bool),
    Release(Pad),
    /// A capture chord fired (Guide or Rare + Y / View).
    Share(Share),
    /// View + Menu held: the way out.
    Exit,
    /// The right stick, (x, y) in -1..1 with down positive; (0, 0) when it comes back to rest.
    Stick(f32, f32),
}

/// First auto-repeat of a held direction.
pub const REPEAT_DELAY: Duration = Duration::from_millis(320);
/// Repeat interval eases from SLOW to FAST: max(FAST, SLOW - repeating_ms / 15).
pub const REPEAT_SLOW_MS: u64 = 120;
pub const REPEAT_FAST_MS: u64 = 40;
/// Left stick as a d-pad, radial: a direction fires when the stick leaves STICK_ON (well past the
/// drift deadzone) and releases below STICK_OFF, so drift and wobble never move focus.
pub const STICK_ON: f32 = 0.6;
pub const STICK_OFF: f32 = 0.35;
pub const STICK_DEADZONE: f32 = 0.25;
pub const TRIGGER_ON: f32 = 0.6;
pub const TRIGGER_OFF: f32 = 0.3;
/// Another pad's input is ignored while the active pad was used this recently.
pub const PAD_LOCK: Duration = Duration::from_millis(1500);
/// Guide held this long is a long press (the Switcher) instead of a press (the Rare menu).
pub const GUIDE_HOLD: Duration = Duration::from_millis(600);
/// A press this soon after the same button's release is contact bounce.
pub const BOUNCE: Duration = Duration::from_millis(35);
/// Presses this soon after a pad (re)connects are buttons that were already held (Steam
/// recreating its virtual pad, a resume from sleep), not presses.
pub const CONNECT_SETTLE: Duration = Duration::from_millis(300);
/// A second A this soon after the last one is a double report, not a second press.
pub const A_DOUBLE: Duration = Duration::from_millis(150);
/// Right stick values go out at most this often while it moves.
pub const STICK_EVERY: Duration = Duration::from_millis(33);
/// A held direction whose last event is older than this is read from the pad again before it
/// repeats (a release that went another way, or never came, must not leave it repeating).
pub const FRESH: Duration = Duration::from_millis(100);

/// A pad as a fresh read of the kernel's state shows it (`evdev::probe`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadNow {
    /// Left stick, -1..1, up positive.
    pub lx: f32,
    pub ly: f32,
    /// Right stick, up positive.
    pub rx: f32,
    pub ry: f32,
    /// Triggers, 0..1.
    pub lt: f32,
    pub rt: f32,
    /// D-pad (buttons or hat): up, down, left, right.
    pub dpad: [bool; 4],
}

/// What a fresh read of a pad says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Probe {
    /// This pad can't be read again (the Deck pad over hidraw, gilrs): its events are trusted.
    Unknown,
    /// The pad is gone.
    Gone,
    Now(PadNow),
}

fn dpad_index(p: Pad) -> Option<usize> {
    Some(match p {
        Pad::Up => 0,
        Pad::Down => 1,
        Pad::Left => 2,
        Pad::Right => 3,
        _ => return None,
    })
}

/// Xbox-style pads on Linux (xpad, Handheld Daemon's virtual pad, InputPlumber's xbox-elite)
/// report X as evdev 307 and Y as 308. Those codes are named BTN_NORTH and BTN_WEST, so X and Y
/// arrive swapped. Seen on the SuiPlay.
pub fn xbox_xy_swapped(name: &str) -> bool {
    xy_swapped(name, None)
}

/// Valve's USB vendor id: the Steam Deck pad InputPlumber's `deck-uhid` target creates.
pub const VALVE_VENDOR: u16 = 0x28de;

/// `xbox_xy_swapped` for a pad by name and USB vendor. The Deck pad reports X and Y on the
/// positional codes (West, North), like Sony and Nintendo pads, so it is never swapped, whatever
/// its name says.
pub fn xy_swapped(name: &str, vendor: Option<u16>) -> bool {
    let n = name.to_lowercase();
    if vendor == Some(VALVE_VENDOR) || n.contains("steam") || n.contains("valve") {
        return false;
    }
    cfg!(target_os = "linux") && (n.contains("handheld daemon") || n.contains("xbox") || n.contains("x-box") || n.contains("microsoft"))
}

/// The launcher button for a pad button, with a swapped X and Y put back.
pub fn map_button_for(b: Btn, swapped: bool) -> Option<Pad> {
    let b = match (swapped, b) {
        (true, Btn::West) => Btn::North,
        (true, Btn::North) => Btn::West,
        (_, o) => o,
    };
    map_button(b)
}

pub fn map_button(b: Btn) -> Option<Pad> {
    Some(match b {
        Btn::DPadUp => Pad::Up,
        Btn::DPadDown => Pad::Down,
        Btn::DPadLeft => Pad::Left,
        Btn::DPadRight => Pad::Right,
        Btn::South => Pad::A,
        Btn::East => Pad::B,
        Btn::West => Pad::X,
        Btn::North => Pad::Y,
        Btn::LeftTrigger => Pad::Lb,
        Btn::RightTrigger => Pad::Rb,
        Btn::LeftTrigger2 => Pad::Lt,
        Btn::RightTrigger2 => Pad::Rt,
        Btn::Start => Pad::Start,
        Btn::Select => Pad::Select,
        Btn::Mode => Pad::Guide,
        Btn::LeftThumb | Btn::RightThumb => return None,
    })
}

/// The direction a stick position points to, or None inside the deadzone. `cur` is the direction
/// held now: it stays until the stick comes back below STICK_OFF, and only hands over to another
/// axis when that axis clearly dominates (no flicker on diagonals). Up is positive y.
pub fn stick_direction(x: f32, y: f32, cur: Option<Pad>) -> Option<Pad> {
    let mag = (x * x + y * y).sqrt();
    if mag < STICK_DEADZONE {
        return None;
    }
    let dominant = if x.abs() > y.abs() {
        if x > 0.0 {
            Pad::Right
        } else {
            Pad::Left
        }
    } else if y > 0.0 {
        Pad::Up
    } else {
        Pad::Down
    };
    match cur {
        None => (mag > STICK_ON).then_some(dominant),
        Some(_) if mag < STICK_OFF => None,
        Some(c) if dominant != c => {
            let (a, b) = if matches!(dominant, Pad::Left | Pad::Right) { (x.abs(), y.abs()) } else { (y.abs(), x.abs()) };
            if mag > STICK_ON && a > b * 1.4 {
                Some(dominant)
            } else {
                Some(c)
            }
        }
        Some(c) => Some(c),
    }
}

/// Interval between repeats after `ms` of repeating: 120 ms easing down to 40 ms over ~1.2 s
/// (input.ts `repeatEvery`).
pub fn repeat_every(ms: u64) -> Duration {
    Duration::from_millis(REPEAT_SLOW_MS.saturating_sub(ms / 15).max(REPEAT_FAST_MS))
}

/// A held direction and its auto-repeat timing.
#[derive(Clone, Copy, Debug)]
pub struct Held {
    pub pad: Pad,
    since: Instant,
    first_repeat: Option<Instant>,
    last: Instant,
    /// The pad it is held on, whether it is the left stick (else the d-pad), and when an event
    /// (or a fresh read) last showed it held.
    src: Src,
    stick: bool,
    fresh: Instant,
}

impl Held {
    pub fn new(pad: Pad, now: Instant) -> Self {
        Held::on(pad, 0, false, now)
    }
    fn on(pad: Pad, src: Src, stick: bool, now: Instant) -> Self {
        Held { pad, since: now, first_repeat: None, last: now, src, stick, fresh: now }
    }
    /// When the next repeat is due.
    pub fn due(&self) -> Instant {
        match self.first_repeat {
            None => self.since + REPEAT_DELAY,
            Some(first) => self.last + repeat_every(self.last.duration_since(first).as_millis() as u64),
        }
    }
    fn fired(&mut self, now: Instant) {
        self.first_repeat.get_or_insert(now);
        self.last = now;
    }
    /// A repeat went out at `now` (for drivers that time repeats themselves, like the headless
    /// script's `held:`).
    pub fn fire(&mut self, now: Instant) {
        self.fired(now);
    }
}

/// The pad state machine. Feed it with `on`, call `tick` every pass (and at `due`).
pub struct Machine {
    /// Log decisions (RARE_DEBUG_INPUT).
    pub debug: bool,
    held: Option<Held>,
    /// The direction the left stick holds, and the pad it is held on.
    stick_dir: Option<Pad>,
    stick_src: Option<Src>,
    /// Every pad's left stick as last read (one pad's rest noise never moves another's).
    left: HashMap<Src, (f32, f32)>,
    probe: Option<Box<dyn FnMut(Src) -> Probe + Send>>,
    rsx: f32,
    rsy: f32,
    rs_at: Option<Instant>,
    rs_zero: bool,
    active: Option<(Src, Instant)>,
    released: HashMap<Pad, Instant>,
    connected: HashMap<Src, Instant>,
    last_a: Option<Instant>,
    exit: ExitChord,
    /// Guide went down and nothing used it yet: its release is a Guide press.
    guide_down: bool,
    guide_at: Instant,
    /// Guide physically held (guide_down is cleared once a chord used it).
    guide_held: bool,
    lt_down: bool,
    rt_down: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Machine::new()
    }
}

impl Machine {
    pub fn new() -> Machine {
        Machine {
            debug: false,
            held: None,
            stick_dir: None,
            stick_src: None,
            left: HashMap::new(),
            probe: None,
            rsx: 0.0,
            rsy: 0.0,
            rs_at: None,
            rs_zero: true,
            active: None,
            released: HashMap::new(),
            connected: HashMap::new(),
            last_a: None,
            exit: ExitChord::new(),
            guide_down: false,
            guide_at: Instant::now(),
            guide_held: false,
            lt_down: false,
            rt_down: false,
        }
    }

    /// A hold or repeat is timing: the loop should poll fast.
    pub fn busy(&self) -> bool {
        self.held.is_some() || self.guide_down || self.exit.due().is_some()
    }

    /// The next moment `tick` has something to do.
    pub fn due(&self) -> Option<Instant> {
        let g = self.guide_down.then(|| self.guide_at + GUIDE_HOLD);
        [self.held.map(|h| h.due()), g, self.exit.due()].into_iter().flatten().min()
    }

    fn say(&self, msg: impl FnOnce() -> String) {
        if self.debug {
            log(&msg());
        }
    }

    /// How to read a pad again (evdev::probe on the native launcher). Without one, events are
    /// trusted as they come.
    pub fn set_probe(&mut self, f: impl FnMut(Src) -> Probe + Send + 'static) {
        self.probe = Some(Box::new(f));
    }

    fn read(&mut self, src: Src) -> Probe {
        match self.probe.as_mut() {
            Some(f) => f(src),
            None => Probe::Unknown,
        }
    }

    /// The pad that owns input now.
    pub fn active_src(&self) -> Option<Src> {
        self.active.map(|(s, _)| s)
    }

    /// Lets go of the held direction and the stick's direction (a Release for each).
    fn let_go(&mut self, out: &mut Vec<Out>) {
        if let Some(h) = self.held.take() {
            out.push(Out::Release(h.pad));
        }
        if let Some(d) = self.stick_dir.take() {
            if !out.contains(&Out::Release(d)) {
                out.push(Out::Release(d));
            }
        }
        self.stick_src = None;
    }

    /// Whether `h` is still held as `now` shows it.
    fn still_held(&self, h: &Held, now: &PadNow) -> bool {
        if h.stick {
            stick_direction(now.lx, now.ly, Some(h.pad)) == Some(h.pad)
        } else {
            dpad_index(h.pad).is_some_and(|i| now.dpad[i])
        }
    }

    /// Input changed hands (reset.rs): an intercept mode or pad target change, the window's
    /// focus, a Rare surface, a resume. Every pad is read again; whatever it doesn't show held
    /// is released (directions, triggers, the right stick back to rest). A pad that can't be read
    /// again is taken as at rest.
    pub fn reset(&mut self, now: Instant, out: &mut Vec<Out>) {
        let mut srcs: Vec<Src> = self.left.keys().copied().collect();
        srcs.extend(self.held.map(|h| h.src));
        srcs.extend(self.active.map(|(s, _)| s));
        srcs.sort_unstable();
        srcs.dedup();
        let mut fresh: HashMap<Src, PadNow> = HashMap::new();
        for s in srcs {
            match self.read(s) {
                Probe::Now(p) => {
                    fresh.insert(s, p);
                    self.left.insert(s, (p.lx, p.ly));
                }
                Probe::Gone => {
                    self.left.remove(&s);
                }
                Probe::Unknown => {
                    self.left.insert(s, (0.0, 0.0));
                }
            }
        }
        let keep = self.held.filter(|h| fresh.get(&h.src).is_some_and(|p| self.still_held(h, p)));
        match keep {
            Some(mut h) => {
                h.fresh = now;
                self.held = Some(h);
                // The stick's own direction, if it is not the held one, goes.
                if self.stick_dir.is_some_and(|d| d != h.pad) {
                    out.push(Out::Release(self.stick_dir.take().unwrap()));
                }
                if !h.stick {
                    self.stick_dir = None;
                    self.stick_src = None;
                }
                self.say(|| format!("[pad] reset: {} still held", h.pad.name()));
            }
            None => {
                if self.held.is_some() || self.stick_dir.is_some() {
                    self.say(|| "[pad] reset: let go".into());
                }
                self.let_go(out);
            }
        }
        let active = self.active.and_then(|(s, _)| fresh.get(&s).copied());
        if self.lt_down && !active.is_some_and(|p| p.lt >= TRIGGER_OFF) {
            self.lt_down = false;
            out.push(Out::Release(Pad::Lt));
        }
        if self.rt_down && !active.is_some_and(|p| p.rt >= TRIGGER_OFF) {
            self.rt_down = false;
            out.push(Out::Release(Pad::Rt));
        }
        self.rsx = 0.0;
        self.rsy = 0.0;
        if !self.rs_zero {
            self.rs_zero = true;
            self.rs_at = Some(now);
            out.push(Out::Stick(0.0, 0.0));
        }
    }

    fn trigger(&mut self, p: Pad, v: f32, out: &mut Vec<Out>) {
        let down = if p == Pad::Lt { &mut self.lt_down } else { &mut self.rt_down };
        if !*down && v > TRIGGER_ON {
            *down = true;
            out.push(Out::Press(p, false));
        } else if *down && v < TRIGGER_OFF {
            *down = false;
            out.push(Out::Release(p));
        }
    }

    /// One event from pad `src`. `swaps`: the pad reports X and Y swapped (`xy_swapped`).
    /// `pointer`: an app in pointer mode has the sticks as its mouse (no directions, no stick out).
    pub fn on(&mut self, src: Src, ev: Ev, swaps: bool, now: Instant, pointer: bool, out: &mut Vec<Out>) {
        // One controller at a time: the pad used last owns input until it goes quiet.
        let meaningful = match ev {
            Ev::Pressed(_) | Ev::Released(_) | Ev::Qam(_) => true,
            Ev::Axis(_, v) | Ev::Changed(_, v) => v.abs() > STICK_DEADZONE,
            _ => false,
        };
        if meaningful {
            match self.active {
                Some((id, at)) if id != src && now.saturating_duration_since(at) < PAD_LOCK => {
                    self.say(|| format!("[pad] ignored: pad {id} is active"));
                    return;
                }
                Some((id, _)) if id != src => {
                    // Hand over: the old pad's held state goes.
                    self.let_go(out);
                    self.left.remove(&id);
                    self.active = Some((src, now));
                }
                _ => self.active = Some((src, now)),
            }
        }
        match ev {
            Ev::Qam(true) => out.push(Out::Press(Pad::Qam, false)),
            Ev::Qam(false) => {}
            Ev::Pressed(b) => {
                let Some(p) = map_button_for(b, swaps) else { return };
                if self.released.get(&p).is_some_and(|t| now.saturating_duration_since(*t) < BOUNCE) {
                    self.say(|| format!("[pad] ignored bounce {}", p.name()));
                    return;
                }
                if self.connected.get(&src).is_some_and(|t| now.saturating_duration_since(*t) < CONNECT_SETTLE) {
                    self.say(|| format!("[pad] ignored {} right after the pad connected", p.name()));
                    return;
                }
                if p == Pad::A {
                    if self.last_a.is_some_and(|t| now.saturating_duration_since(t) < A_DOUBLE) {
                        self.say(|| "[pad] ignored a double A".into());
                        return;
                    }
                    self.last_a = Some(now);
                }
                if matches!(p, Pad::Select | Pad::Start) {
                    self.exit.set(p == Pad::Select, true, now);
                }
                match p {
                    Pad::Guide => {
                        self.guide_down = true;
                        self.guide_at = now;
                        self.guide_held = true;
                        chords::set_guide_held(true);
                        chords::clear_guide_used();
                        return;
                    }
                    Pad::Lt | Pad::Rt => return self.trigger(p, 1.0, out),
                    _ => {}
                }
                // Guide + A is the quick access chord InputPlumber and Steam use.
                if self.guide_down && p == Pad::A {
                    out.push(Out::Press(Pad::Qam, false));
                    self.guide_down = false;
                    return;
                }
                // Share chords work while a game has focus: the pad is read whatever is in front.
                // Guide stays held for another chord; its release no longer brings Rare up.
                if self.guide_held || chords::RARE_HELD.load(Ordering::Relaxed) {
                    if let Some(c) = chords::share_chord(p) {
                        self.guide_down = false;
                        chords::RARE_CHORD_USED.store(true, Ordering::Relaxed);
                        out.push(Out::Share(c));
                        return;
                    }
                }
                out.push(Out::Press(p, false));
                if is_dir(p) {
                    self.say(|| format!("[pad] {} down on pad {src}", p.name()));
                    self.held = Some(Held::on(p, src, false, now));
                }
            }
            Ev::Released(b) => {
                let Some(p) = map_button_for(b, swaps) else { return };
                self.released.insert(p, now);
                if matches!(p, Pad::Select | Pad::Start) {
                    self.exit.set(p == Pad::Select, false, now);
                }
                if matches!(p, Pad::Lt | Pad::Rt) {
                    return self.trigger(p, 0.0, out);
                }
                if p == Pad::Guide {
                    self.guide_held = false;
                    chords::set_guide_held(false);
                    if chords::take_guide_used() {
                        self.guide_down = false;
                    }
                    if self.guide_down {
                        self.guide_down = false;
                        out.push(Out::Press(Pad::Guide, false));
                    }
                }
                out.push(Out::Release(p));
                if self.held.map(|h| h.pad) == Some(p) {
                    self.held = None;
                }
            }
            Ev::Changed(Btn::LeftTrigger2, v) | Ev::Axis(Axis::LeftZ, v) => self.trigger(Pad::Lt, v, out),
            Ev::Changed(Btn::RightTrigger2, v) | Ev::Axis(Axis::RightZ, v) => self.trigger(Pad::Rt, v, out),
            Ev::Changed(..) => {}
            Ev::Axis(axis @ (Axis::RightStickX | Axis::RightStickY), v) => {
                if axis == Axis::RightStickX {
                    self.rsx = v
                } else {
                    self.rsy = -v
                }
                if pointer {
                    return;
                }
                let zero = (self.rsx * self.rsx + self.rsy * self.rsy).sqrt() < STICK_DEADZONE;
                let due = !self.rs_at.is_some_and(|t| now.saturating_duration_since(t) < STICK_EVERY);
                if (!zero && due) || (zero && !self.rs_zero) {
                    self.rs_at = Some(now);
                    self.rs_zero = zero;
                    out.push(if zero { Out::Stick(0.0, 0.0) } else { Out::Stick(self.rsx, self.rsy) });
                }
            }
            Ev::Axis(axis, v) => {
                let (lx, ly) = {
                    let e = self.left.entry(src).or_insert((0.0, 0.0));
                    match axis {
                        Axis::LeftStickX => e.0 = v,
                        _ => e.1 = v,
                    }
                    *e
                };
                // An app driven as a mouse scrolls with the left stick: no directions from it.
                if pointer {
                    if let Some(old) = self.stick_dir.take() {
                        out.push(Out::Release(old));
                        if self.held.is_some_and(|h| h.pad == old) {
                            self.held = None;
                        }
                    }
                    self.stick_src = None;
                    return;
                }
                // Another pad's rest noise (let through the one-pad lock) never moves this one.
                if self.stick_src.is_some_and(|s| s != src) {
                    return;
                }
                let dir = stick_direction(lx, ly, self.stick_dir);
                if dir == self.stick_dir {
                    if let Some(h) = self.held.as_mut().filter(|h| h.stick && h.src == src) {
                        h.fresh = now;
                    }
                    return;
                }
                if let Some(old) = self.stick_dir {
                    out.push(Out::Release(old));
                    if self.held.map(|h| h.pad) == Some(old) {
                        self.held = None;
                    }
                }
                self.stick_dir = dir;
                self.stick_src = dir.map(|_| src);
                if let Some(d) = dir {
                    self.say(|| format!("[pad] stick {} on pad {src} ({lx:.2}, {ly:.2})", d.name()));
                    out.push(Out::Press(d, false));
                    self.held = Some(Held::on(d, src, true, now));
                }
            }
            Ev::Connected => {
                self.connected.insert(src, now);
            }
            Ev::Disconnected => {
                self.left.remove(&src);
                self.connected.remove(&src);
                // Only the pad that owns input or holds something lets go: other devices (Steam's
                // virtual pads, a keyboard with pad keys) come and go while a direction is held.
                let holds = self.held.is_some_and(|h| h.src == src) || self.stick_src == Some(src);
                let owns = self.active.map_or(true, |(s, _)| s == src);
                if holds || owns {
                    self.let_go(out);
                }
                if owns {
                    self.exit.reset();
                    self.guide_held = false;
                    chords::set_guide_held(false);
                    if self.lt_down {
                        self.lt_down = false;
                        out.push(Out::Release(Pad::Lt));
                    }
                    if self.rt_down {
                        self.rt_down = false;
                        out.push(Out::Release(Pad::Rt));
                    }
                    self.active = None;
                }
            }
        }
    }

    /// Holds, chords and repeats that came due.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<Out>) {
        // View + Menu held: the way out, whatever has the screen.
        if self.exit.poll(now) {
            log("[pad] View + Menu held: the way out");
            out.push(Out::Exit);
        }
        // Rare + volume (brightness) used the held Guide: no Guide press or hold from it.
        if self.guide_down && chords::guide_used() {
            self.guide_down = false;
        }
        // Guide held long enough: the Switcher, and no Guide press on release.
        if self.guide_down && now.saturating_duration_since(self.guide_at) >= GUIDE_HOLD {
            self.guide_down = false;
            out.push(Out::Press(Pad::GuideHold, false));
        }
        let Some(mut h) = self.held else { return };
        if now < h.due() {
            return;
        }
        // A stick direction repeats only while that pad's latest value is still past the threshold.
        let last = self.left.get(&h.src).copied().unwrap_or((0.0, 0.0));
        let mut held = !h.stick || stick_direction(last.0, last.1, Some(h.pad)) == Some(h.pad);
        // No event for a while: read the pad before repeating from an old value.
        if held && now.saturating_duration_since(h.fresh) >= FRESH {
            match self.read(h.src) {
                Probe::Unknown => {}
                Probe::Gone => held = false,
                Probe::Now(p) => {
                    if h.stick {
                        self.left.insert(h.src, (p.lx, p.ly));
                    }
                    held = self.still_held(&h, &p);
                    h.fresh = now;
                }
            }
        }
        if !held {
            self.say(|| format!("[pad] {} is no longer held: released", h.pad.name()));
            self.let_go(out);
            return;
        }
        self.say(|| format!("[pad] repeat {} (pad {}, {}, last seen held {} ms ago)", h.pad.name(), h.src, if h.stick { "stick" } else { "d-pad" }, now.saturating_duration_since(h.fresh).as_millis()));
        out.push(Out::Press(h.pad, true));
        h.fired(now);
        self.held = Some(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The chord flags are process-wide: tests that touch them run one at a time.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn ms(t: Instant, n: u64) -> Instant {
        t + Duration::from_millis(n)
    }

    fn run(m: &mut Machine, src: Src, ev: Ev, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        m.on(src, ev, false, now, false, &mut out);
        out
    }

    fn tick(m: &mut Machine, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        m.tick(now, &mut out);
        out
    }

    #[test]
    fn stick_is_radial_with_hysteresis() {
        assert_eq!(stick_direction(0.2, 0.1, None), None); // drift
        assert_eq!(stick_direction(0.5, 0.0, None), None); // not far enough to fire
        assert_eq!(stick_direction(0.7, 0.1, None), Some(Pad::Right));
        assert_eq!(stick_direction(0.4, 0.0, Some(Pad::Right)), Some(Pad::Right)); // holds
        assert_eq!(stick_direction(0.3, 0.0, Some(Pad::Right)), None); // releases
        assert_eq!(stick_direction(0.5, 0.55, Some(Pad::Right)), Some(Pad::Right)); // diagonal: no flicker
        assert_eq!(stick_direction(0.1, -0.8, Some(Pad::Right)), Some(Pad::Down)); // clear hand-over
    }

    #[test]
    fn xbox_x_and_y_are_put_back() {
        if cfg!(target_os = "linux") {
            // Handheld Daemon's pad: evdev 307 (X) arrives as North, 308 (Y) as West.
            assert_eq!(map_button_for(Btn::North, xbox_xy_swapped("Handheld Daemon Controller")), Some(Pad::X));
            assert_eq!(map_button_for(Btn::West, xbox_xy_swapped("Handheld Daemon Controller")), Some(Pad::Y));
            assert_eq!(map_button_for(Btn::West, xbox_xy_swapped("Wireless Controller")), Some(Pad::X));
            // InputPlumber's xbox-elite target on the SuiPlay.
            assert!(xbox_xy_swapped("Microsoft X-Box One Elite 2 pad"));
        }
    }

    #[test]
    fn the_deck_pad_is_never_swapped() {
        assert!(!xy_swapped("Steam Deck", Some(0x28de)));
        assert!(!xy_swapped("Valve Software Steam Deck Controller", None));
        assert!(!xy_swapped("Microsoft X-Box 360 pad", Some(0x28de)));
        assert_eq!(map_button_for(Btn::West, xy_swapped("Steam Deck", Some(0x28de))), Some(Pad::X));
        assert_eq!(map_button_for(Btn::North, xy_swapped("Steam Deck", Some(0x28de))), Some(Pad::Y));
        if cfg!(target_os = "linux") {
            assert!(xy_swapped("Microsoft X-Box One Elite 2 pad", Some(0x045e)));
            assert!(xy_swapped("Microsoft X-Box 360 pad", None));
        }
    }

    #[test]
    fn share_chords_after_the_swap() {
        if cfg!(target_os = "linux") {
            // On the SuiPlay's pad the physical Y arrives as West and is put back first.
            assert_eq!(map_button_for(Btn::West, xbox_xy_swapped("Handheld Daemon Controller")).and_then(chords::share_chord), Some(Share::Screenshot));
            assert_eq!(map_button_for(Btn::North, xbox_xy_swapped("Handheld Daemon Controller")).and_then(chords::share_chord), None);
        }
    }

    #[test]
    fn repeat_accelerates() {
        let t0 = Instant::now();
        let mut h = Held::new(Pad::Down, t0);
        assert_eq!(h.due(), t0 + REPEAT_DELAY);
        let first = t0 + REPEAT_DELAY;
        h.fired(first);
        assert_eq!(h.due(), first + Duration::from_millis(120));
        h.last = first + Duration::from_millis(600);
        assert_eq!(h.due(), h.last + Duration::from_millis(80));
        h.last = first + Duration::from_millis(5000);
        assert_eq!(h.due(), h.last + Duration::from_millis(40));
        // input.ts repeatEvery(ms) = max(40, 120 - ms / 15).
        assert_eq!(repeat_every(0), Duration::from_millis(120));
        assert_eq!(repeat_every(600), Duration::from_millis(80));
        assert_eq!(repeat_every(5000), Duration::from_millis(40));
    }

    #[test]
    fn directions_press_repeat_and_release() {
        let t = Instant::now();
        let mut m = Machine::new();
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::DPadDown), t), vec![Out::Press(Pad::Down, false)]);
        assert_eq!(m.due(), Some(ms(t, 320)));
        assert!(tick(&mut m, ms(t, 300)).is_empty());
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Down, true)]);
        assert_eq!(m.due(), Some(ms(t, 440)));
        assert_eq!(tick(&mut m, ms(t, 440)), vec![Out::Press(Pad::Down, true)]);
        assert_eq!(run(&mut m, 1, Ev::Released(Btn::DPadDown), ms(t, 500)), vec![Out::Release(Pad::Down)]);
        assert!(tick(&mut m, ms(t, 2000)).is_empty());
        assert!(!m.busy());
    }

    #[test]
    fn the_left_stick_is_a_dpad() {
        let t = Instant::now();
        let mut m = Machine::new();
        assert!(run(&mut m, 1, Ev::Axis(Axis::LeftStickX, 0.5), t).is_empty());
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::LeftStickX, 0.8), t), vec![Out::Press(Pad::Right, false)]);
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Right, true)]);
        // Up is positive: a push up while right is held hands over cleanly.
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::LeftStickX, 0.1), ms(t, 400)), vec![Out::Release(Pad::Right)]);
        let mut o = run(&mut m, 1, Ev::Axis(Axis::LeftStickY, 0.9), ms(t, 410));
        assert_eq!(o.pop(), Some(Out::Press(Pad::Up, false)));
        // Pointer mode: the stick is the app's, a held direction is let go.
        let mut out = Vec::new();
        m.on(1, Ev::Axis(Axis::LeftStickY, 0.95), false, ms(t, 420), true, &mut out);
        assert_eq!(out, vec![Out::Release(Pad::Up)]);
    }

    #[test]
    fn triggers_are_analog_with_hysteresis() {
        let t = Instant::now();
        let mut m = Machine::new();
        assert!(run(&mut m, 1, Ev::Axis(Axis::RightZ, 0.5), t).is_empty());
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::RightZ, 0.7), t), vec![Out::Press(Pad::Rt, false)]);
        assert!(run(&mut m, 1, Ev::Axis(Axis::RightZ, 0.4), t).is_empty());
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::RightZ, 0.1), t), vec![Out::Release(Pad::Rt)]);
        // A digital trigger button too, and no repeat.
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::LeftTrigger2), ms(t, 100)), vec![Out::Press(Pad::Lt, false)]);
        assert!(tick(&mut m, ms(t, 2000)).is_empty());
        assert_eq!(run(&mut m, 1, Ev::Released(Btn::LeftTrigger2), ms(t, 2100)), vec![Out::Release(Pad::Lt)]);
    }

    #[test]
    fn bounce_settle_and_double_a_are_dropped() {
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 1, Ev::Pressed(Btn::East), t);
        run(&mut m, 1, Ev::Released(Btn::East), ms(t, 50));
        assert!(run(&mut m, 1, Ev::Pressed(Btn::East), ms(t, 60)).is_empty(), "bounce");
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::East), ms(t, 100)), vec![Out::Press(Pad::B, false)]);
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::South), ms(t, 200)), vec![Out::Press(Pad::A, false)]);
        assert!(run(&mut m, 1, Ev::Pressed(Btn::South), ms(t, 300)).is_empty(), "double A");
        run(&mut m, 1, Ev::Connected, ms(t, 1000));
        assert!(run(&mut m, 1, Ev::Pressed(Btn::West), ms(t, 1100)).is_empty(), "held through a reconnect");
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::West), ms(t, 1400)), vec![Out::Press(Pad::X, false)]);
    }

    #[test]
    fn one_pad_at_a_time() {
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 1, Ev::Pressed(Btn::DPadLeft), t);
        assert!(run(&mut m, 2, Ev::Pressed(Btn::DPadLeft), ms(t, 10)).is_empty(), "the twin virtual pad");
        assert!(run(&mut m, 2, Ev::Axis(Axis::LeftStickX, 0.1), ms(t, 20)).is_empty());
        // Quiet for PAD_LOCK: the other pad takes over and the old held direction is let go.
        assert_eq!(run(&mut m, 2, Ev::Pressed(Btn::South), ms(t, 1600)), vec![Out::Release(Pad::Left), Out::Press(Pad::A, false)]);
        assert!(tick(&mut m, ms(t, 5000)).is_empty());
    }

    #[test]
    fn guide_press_hold_quick_and_share() {
        let _g = SERIAL.lock().unwrap();
        let t = Instant::now();
        let mut m = Machine::new();
        // A press: the Rare menu on release.
        assert!(run(&mut m, 1, Ev::Pressed(Btn::Mode), t).is_empty());
        assert!(chords::guide_held());
        assert_eq!(run(&mut m, 1, Ev::Released(Btn::Mode), ms(t, 100)), vec![Out::Press(Pad::Guide, false), Out::Release(Pad::Guide)]);
        assert!(!chords::guide_held());
        // Held: the Switcher once, nothing on release.
        run(&mut m, 1, Ev::Pressed(Btn::Mode), ms(t, 1000));
        assert_eq!(m.due(), Some(ms(t, 1600)));
        assert_eq!(tick(&mut m, ms(t, 1600)), vec![Out::Press(Pad::GuideHold, false)]);
        assert!(tick(&mut m, ms(t, 2000)).is_empty());
        assert_eq!(run(&mut m, 1, Ev::Released(Btn::Mode), ms(t, 2100)), vec![Out::Release(Pad::Guide)]);
        // Guide + A: quick access, no Rare menu.
        run(&mut m, 1, Ev::Pressed(Btn::Mode), ms(t, 3000));
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::South), ms(t, 3100)), vec![Out::Press(Pad::Qam, false)]);
        assert_eq!(run(&mut m, 1, Ev::Released(Btn::Mode), ms(t, 3200)), vec![Out::Release(Pad::Guide)]);
        // Guide + Y and Guide + View: the share chords, Guide stays held for another.
        run(&mut m, 1, Ev::Pressed(Btn::Mode), ms(t, 4000));
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::North), ms(t, 4100)), vec![Out::Share(Share::Screenshot)]);
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::Select), ms(t, 4200)), vec![Out::Share(Share::Clip)]);
        assert!(tick(&mut m, ms(t, 5000)).is_empty(), "no hold after a chord");
        assert_eq!(run(&mut m, 1, Ev::Released(Btn::Mode), ms(t, 5100)), vec![Out::Release(Pad::Guide)]);
        // Rare + volume on another thread used the held Guide: no press, no hold.
        run(&mut m, 1, Ev::Pressed(Btn::Mode), ms(t, 6000));
        chords::guide_chord_used();
        assert!(tick(&mut m, ms(t, 6100)).is_empty());
        assert_eq!(run(&mut m, 1, Ev::Released(Btn::Mode), ms(t, 6200)), vec![Out::Release(Pad::Guide)]);
        chords::RARE_CHORD_USED.store(false, Ordering::Relaxed);
    }

    #[test]
    fn rare_held_makes_the_share_chords() {
        let _g = SERIAL.lock().unwrap();
        let t = Instant::now();
        let mut m = Machine::new();
        chords::RARE_HELD.store(true, Ordering::Relaxed);
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::North), t), vec![Out::Share(Share::Screenshot)]);
        assert!(chords::RARE_CHORD_USED.load(Ordering::Relaxed));
        chords::RARE_HELD.store(false, Ordering::Relaxed);
        chords::RARE_CHORD_USED.store(false, Ordering::Relaxed);
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::North), ms(t, 500)), vec![Out::Press(Pad::Y, false)]);
    }

    #[test]
    fn view_and_menu_held_is_the_way_out() {
        let t = Instant::now();
        let mut m = Machine::new();
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::Select), t), vec![Out::Press(Pad::Select, false)]);
        assert_eq!(run(&mut m, 1, Ev::Pressed(Btn::Start), ms(t, 50)), vec![Out::Press(Pad::Start, false)]);
        assert!(m.busy());
        assert!(tick(&mut m, ms(t, 600)).is_empty());
        assert_eq!(tick(&mut m, ms(t, 650)), vec![Out::Exit]);
        assert!(tick(&mut m, ms(t, 2000)).is_empty());
    }

    #[test]
    fn the_right_stick_goes_out_throttled_and_rests_once() {
        let t = Instant::now();
        let mut m = Machine::new();
        assert!(run(&mut m, 1, Ev::Axis(Axis::RightStickX, 0.1), t).is_empty(), "drift");
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::RightStickY, -0.5), t), vec![Out::Stick(0.1, 0.5)]);
        assert!(run(&mut m, 1, Ev::Axis(Axis::RightStickY, -0.6), ms(t, 10)).is_empty(), "throttled");
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::RightStickY, -0.7), ms(t, 40)), vec![Out::Stick(0.1, 0.7)]);
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::RightStickY, 0.0), ms(t, 45)), vec![Out::Stick(0.0, 0.0)]);
        assert!(run(&mut m, 1, Ev::Axis(Axis::RightStickX, 0.0), ms(t, 100)).is_empty());
    }

    fn rest() -> PadNow {
        PadNow { lx: 0.0, ly: 0.0, rx: 0.0, ry: 0.0, lt: 0.0, rt: 0.0, dpad: [false; 4] }
    }

    /// A pad whose kernel state the test sets (what evdev::probe reads).
    fn probed(m: &mut Machine) -> std::sync::Arc<std::sync::Mutex<PadNow>> {
        let now = std::sync::Arc::new(std::sync::Mutex::new(rest()));
        let n = now.clone();
        m.set_probe(move |_| Probe::Now(*n.lock().unwrap()));
        now
    }

    #[test]
    fn a_held_stick_is_read_again_before_it_repeats_from_an_old_value() {
        let t = Instant::now();
        let mut m = Machine::new();
        let pad = probed(&mut m);
        pad.lock().unwrap().lx = -0.9;
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::LeftStickX, -0.9), t), vec![Out::Press(Pad::Left, false)]);
        // Held and still read as held: it repeats.
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Left, true)]);
        // The release went another way (InputPlumber switched paths): the next read lets it go.
        pad.lock().unwrap().lx = 0.0;
        assert_eq!(tick(&mut m, ms(t, 440)), vec![Out::Release(Pad::Left)]);
        assert!(tick(&mut m, ms(t, 3000)).is_empty());
        assert!(!m.busy());
    }

    #[test]
    fn a_held_dpad_is_read_again_too() {
        let t = Instant::now();
        let mut m = Machine::new();
        let pad = probed(&mut m);
        pad.lock().unwrap().dpad[2] = true;
        run(&mut m, 1, Ev::Pressed(Btn::DPadLeft), t);
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Left, true)]);
        pad.lock().unwrap().dpad[2] = false;
        assert_eq!(tick(&mut m, ms(t, 440)), vec![Out::Release(Pad::Left)]);
        assert!(tick(&mut m, ms(t, 3000)).is_empty());
    }

    #[test]
    fn an_intercept_switch_mid_hold_lets_go_unless_the_pad_still_holds_it() {
        let t = Instant::now();
        let mut m = Machine::new();
        let pad = probed(&mut m);
        pad.lock().unwrap().lx = -0.9;
        run(&mut m, 1, Ev::Axis(Axis::LeftStickX, -0.9), t);
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Left, true)]);
        // Mode 1 -> 2 with the stick still held: the target pad still shows it, so it stays.
        let mut out = Vec::new();
        m.reset(ms(t, 350), &mut out);
        assert!(out.is_empty());
        assert_eq!(tick(&mut m, ms(t, 440)), vec![Out::Press(Pad::Left, true)]);
        // Now InputPlumber's target is at rest (it went quiet for D-Bus): the switch lets go.
        pad.lock().unwrap().lx = 0.0;
        m.reset(ms(t, 500), &mut out);
        assert_eq!(out, vec![Out::Release(Pad::Left)]);
        for n in (500..3000).step_by(40) {
            assert!(tick(&mut m, ms(t, n)).is_empty(), "repeat at {n} ms after the switch");
        }
        // The stick comes back on the evdev path: a fresh press, nothing left over.
        pad.lock().unwrap().lx = 0.9;
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::LeftStickX, 0.9), ms(t, 3100)), vec![Out::Press(Pad::Right, false)]);
    }

    #[test]
    fn a_reset_without_a_way_to_read_the_pad_lets_everything_go() {
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 1, Ev::Axis(Axis::LeftStickX, -0.9), t);
        run(&mut m, 1, Ev::Axis(Axis::RightZ, 0.9), t);
        run(&mut m, 1, Ev::Axis(Axis::RightStickX, 0.8), t);
        let mut out = Vec::new();
        m.reset(ms(t, 100), &mut out);
        assert_eq!(out, vec![Out::Release(Pad::Left), Out::Release(Pad::Rt), Out::Stick(0.0, 0.0)]);
        assert!(tick(&mut m, ms(t, 3000)).is_empty());
        // The old value is gone: a small move of the other axis doesn't bring Left back.
        assert!(run(&mut m, 1, Ev::Axis(Axis::LeftStickY, 0.1), ms(t, 3100)).is_empty());
    }

    #[test]
    fn a_stale_stick_value_never_comes_back_after_its_pad_goes() {
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 1, Ev::Axis(Axis::LeftStickX, -0.9), t);
        assert_eq!(run(&mut m, 1, Ev::Disconnected, ms(t, 50)), vec![Out::Release(Pad::Left)]);
        // The re-created pad's first event (Y wobble) used to bring Left back from the old X.
        assert!(run(&mut m, 2, Ev::Axis(Axis::LeftStickY, 0.05), ms(t, 2000)).is_empty());
        assert!(tick(&mut m, ms(t, 4000)).is_empty());
    }

    #[test]
    fn a_pad_that_goes_away_releases_everything_it_held() {
        // The journeys harness's virtual pad destroyed (or the harness killed) with Right still down
        // on it: its release never arrives. Seen as ~13 s of "pad right repeat" after a run.
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 5, Ev::Pressed(Btn::DPadRight), t);
        run(&mut m, 5, Ev::Axis(Axis::RightZ, 1.0), t);
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Right, true)]);
        assert_eq!(run(&mut m, 5, Ev::Disconnected, ms(t, 400)), vec![Out::Release(Pad::Right), Out::Release(Pad::Rt)]);
        for n in (400..15_000).step_by(40) {
            assert!(tick(&mut m, ms(t, n)).is_empty(), "repeat at {n} ms after the pad went away");
        }
        assert!(!m.busy());
    }

    #[test]
    fn a_pad_whose_reader_is_gone_stops_repeating_before_the_disconnect_arrives() {
        let t = Instant::now();
        let mut m = Machine::new();
        m.set_probe(|_| Probe::Gone);
        run(&mut m, 5, Ev::Pressed(Btn::DPadRight), t);
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Release(Pad::Right)]);
        assert!(tick(&mut m, ms(t, 2000)).is_empty());
    }

    #[test]
    fn another_device_coming_and_going_keeps_a_held_direction() {
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 1, Ev::Axis(Axis::LeftStickX, -0.9), t);
        // A keyboard with pad keys (libvirtualhid) opened and closed every 2 s on the SuiPlay.
        run(&mut m, 7, Ev::Connected, ms(t, 100));
        assert!(run(&mut m, 7, Ev::Disconnected, ms(t, 110)).is_empty());
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Left, true)]);
    }

    #[test]
    fn another_pads_rest_noise_never_moves_the_held_stick() {
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 1, Ev::Axis(Axis::LeftStickX, -0.9), t);
        // Pad 2 at rest (let through the lock: below the deadzone) used to overwrite pad 1's X.
        assert!(run(&mut m, 2, Ev::Axis(Axis::LeftStickX, 0.02), ms(t, 10)).is_empty());
        assert_eq!(tick(&mut m, ms(t, 320)), vec![Out::Press(Pad::Left, true)]);
        assert_eq!(run(&mut m, 1, Ev::Axis(Axis::LeftStickX, 0.0), ms(t, 400)), vec![Out::Release(Pad::Left)]);
    }

    #[test]
    fn a_disconnect_lets_everything_go() {
        let t = Instant::now();
        let mut m = Machine::new();
        run(&mut m, 1, Ev::Pressed(Btn::DPadUp), t);
        run(&mut m, 1, Ev::Pressed(Btn::Select), t);
        run(&mut m, 1, Ev::Pressed(Btn::Start), t);
        run(&mut m, 1, Ev::Disconnected, ms(t, 10));
        assert!(tick(&mut m, ms(t, 1000)).is_empty());
        assert!(!m.busy());
    }
}
