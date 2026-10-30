//! Input for the native launcher: the port of `launcher/src/nav/input.ts` and the button model of
//! `App.tsx`, on top of the rare-input crate's pad state machine.
//!
//! Pads are read straight from evdev, one blocking reader thread per pad (InputPlumber's
//! composite device on Rare OS), so a press wakes the UI loop the moment the kernel delivers it;
//! the web launcher polls gilrs every 16 ms, which alone adds 0-16 ms. What a press means (the
//! 320 ms then 120 to 40 ms repeat, triggers, bounce, Guide press and hold, Guide + A, the share
//! chords, View + Menu) is rare-input's `pad::Machine`, the same code the Tauri shell runs. The
//! Rare, Quick, LC and RC buttons arrive as InputPlumber D-Bus actions (`rare_input::ip`).
//!
//! On the UI thread, `Dispatch` does what input.ts does with every press: one source at a time
//! (pad versus keyboard, a duplicate from Steam's keyboard emulation dropped), the View / Menu
//! chord window, controller mode versus touch, and what an unclaimed press does to focus (move,
//! LT / RT sections, A). `shell` is App.tsx's global handler as a pure function: LB / RB tabs, B
//! back, Quick, Rare press and hold, Menu = Options, View = the screen's view, Y = Search.

pub use rare_input::chords::Share;
pub use rare_input::side::Key as Side;
pub use rare_input::Pad;

use crate::focus::{Dir, Nav};
use crate::scene::Id;

#[derive(Clone, Copy, Debug)]
pub struct PadEvent {
    pub pad: Pad,
    pub repeat: bool,
    /// CLOCK_MONOTONIC ns of the kernel event (or of the repeat tick).
    pub t_ns: u64,
}

/// Everything the input threads send to the UI thread.
#[derive(Clone, Copy, Debug)]
pub enum Input {
    /// A press (or the repeat of a held direction).
    Press(PadEvent),
    Release(Pad),
    /// The right stick, (x, y) in -1..1 with down positive; (0, 0) back at rest.
    Stick(f32, f32),
    /// Every analog axis as read (sticks -1..1 up positive, triggers 0..1), for a web game's
    /// virtual gamepad; the launcher's own navigation ignores it.
    Analog(rare_input::pad::Axis, f32),
    /// A capture chord (Rare or Guide + Y / View): the app takes the screenshot or clip.
    Share(Share),
    /// View + Menu held 0.6 s: the Switcher, from anywhere (the silent rescue).
    Exit,
    /// LC or RC pressed or released: hand to `rare_input::side::press` with the app's actions.
    Side(Side, bool),
}

/// Starts the pad readers and the press dispatcher; `emit` gets presses only (the POC's API). No
/// InputPlumber listener: a bench build running next to the launcher must not take its buttons.
pub fn spawn(emit: impl Fn(PadEvent) + Send + 'static) {
    let emit = move |i| {
        if let Input::Press(e) = i {
            emit(e)
        }
    };
    #[cfg(target_os = "linux")]
    linux::spawn(emit, None::<fn() -> bool>);
    #[cfg(not(target_os = "linux"))]
    let _ = emit;
}

/// The launcher's input: the pad readers, InputPlumber's Rare, Quick, LC and RC buttons (and the
/// whole pad while a Rare surface intercepts it) and the dispatcher; `emit` runs on the dispatcher
/// thread for every input. `surface_up` says whether a Rare surface is over a game (InputPlumber's
/// intercept mode follows it).
pub fn spawn_with(emit: impl Fn(Input) + Send + 'static, surface_up: impl Fn() -> bool + Send + 'static) {
    #[cfg(target_os = "linux")]
    linux::spawn(emit, Some(surface_up));
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (emit, surface_up);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{Input, PadEvent};
    use rare_input::evdev::{mono_ns, Raw};
    use rare_input::ip::Out as IpOut;
    use rare_input::pad::{Machine, Out};
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Instant;

    enum Msg {
        Pad(Raw),
        Ip(IpOut),
        /// Input changed hands (rare_input::reset): let go of what isn't held any more.
        Reset,
    }

    pub fn spawn(emit: impl Fn(Input) + Send + 'static, surface_up: Option<impl Fn() -> bool + Send + 'static>) {
        let (tx, rx) = channel::<Msg>();
        let (rtx, rrx) = channel::<Raw>();
        rare_input::evdev::spawn(rtx);
        {
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("rr-pad-fwd".into())
                .spawn(move || {
                    while let Ok(r) = rrx.recv() {
                        if tx.send(Msg::Pad(r)).is_err() {
                            return;
                        }
                    }
                })
                .ok();
        }
        {
            let tx = std::sync::Mutex::new(tx.clone());
            rare_input::reset::on_request(move || {
                let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(Msg::Reset);
            });
        }
        if let Some(surface_up) = surface_up {
            let tx = std::sync::Mutex::new(tx);
            rare_input::ip::start(
                move |o| {
                    let _ = tx.lock().unwrap().send(Msg::Ip(o));
                },
                surface_up,
            );
        }
        std::thread::Builder::new()
            .name("rr-pad".into())
            .spawn(move || {
                let mut m = Machine::new();
                m.debug = rare_input::debug_input();
                // A held direction with no event for a while is read from the pad before it
                // repeats.
                m.set_probe(rare_input::evdev::probe);
                let mut out = Vec::new();
                // The input log can be turned on while running ($XDG_RUNTIME_DIR/rare-debug-input).
                let mut debug_at = Instant::now();
                loop {
                    if debug_at.elapsed().as_secs() >= 2 {
                        debug_at = Instant::now();
                        m.debug = rare_input::debug_input();
                    }
                    let msg = match m.due() {
                        Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    let mut t_ns = mono_ns();
                    match msg {
                        Ok(Msg::Pad(r)) => {
                            t_ns = r.t_ns;
                            if let rare_input::pad::Ev::Axis(a, v) = r.ev {
                                emit(Input::Analog(a, v));
                            }
                            m.on(r.src, r.ev, r.swaps, Instant::now(), false, &mut out);
                        }
                        Ok(Msg::Reset) => {
                            m.reset(Instant::now(), &mut out);
                            // A web game's virtual pad gets the sticks and triggers as they are
                            // now (at rest unless still held), so nothing stays pushed in it.
                            use rare_input::pad::{Axis, Probe};
                            let now = m.active_src().map(rare_input::evdev::probe);
                            let p = match now {
                                Some(Probe::Now(p)) => [p.lx, p.ly, p.rx, p.ry, p.lt, p.rt],
                                _ => [0.0; 6],
                            };
                            for (a, v) in [Axis::LeftStickX, Axis::LeftStickY, Axis::RightStickX, Axis::RightStickY, Axis::LeftZ, Axis::RightZ].into_iter().zip(p) {
                                emit(Input::Analog(a, v));
                            }
                        }
                        Ok(Msg::Ip(o)) => match o {
                            IpOut::Press(pad, repeat) => emit(Input::Press(PadEvent { pad, repeat, t_ns })),
                            IpOut::Release(p) => emit(Input::Release(p)),
                            IpOut::Side(k, down) => emit(Input::Side(k, down)),
                            // The same steps, Rare + volume brightness and HUD bar as the keys.
                            IpOut::Volume(k) => rare_input::volume::key(k),
                        },
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    m.tick(Instant::now(), &mut out);
                    for o in out.drain(..) {
                        emit(match o {
                            Out::Press(pad, repeat) => Input::Press(PadEvent { pad, repeat, t_ns: if repeat { mono_ns() } else { t_ns } }),
                            Out::Release(p) => Input::Release(p),
                            Out::Share(c) => Input::Share(c),
                            Out::Exit => Input::Exit,
                            Out::Stick(x, y) => Input::Stick(x, y),
                        });
                    }
                }
            })
            .expect("pad thread");
    }
}

// ---- keyboard ----

/// Keyboard keys (winit's logical key names, as the browser's `KeyboardEvent.key`) that map to
/// pad buttons, for the desktop and the handheld's extra keys (input.ts `KEYS`).
pub fn key_to_pad(key: &str) -> Option<Pad> {
    Some(match key {
        "ArrowUp" => Pad::Up,
        "ArrowDown" => Pad::Down,
        "ArrowLeft" => Pad::Left,
        "ArrowRight" => Pad::Right,
        "Enter" | " " => Pad::A,
        "Escape" | "Backspace" => Pad::B,
        "x" => Pad::X,
        "y" => Pad::Y,
        "q" => Pad::Lb,
        "e" => Pad::Rb,
        "Tab" => Pad::Start,
        "PageUp" => Pad::Lt,
        "PageDown" => Pad::Rt,
        // SuiPlay0X1 quick access key (gameos hwdb maps it to F18) and its "Aya Space" key.
        "F18" => Pad::Qam,
        "F17" | "Home" => Pad::Guide,
        "s" => Pad::Select,
        _ => return None,
    })
}

/// The handheld's own keys never come from Steam's keyboard emulation.
pub fn own_key(key: &str) -> bool {
    key == "F17" || key == "F18"
}

// ---- dispatch ----

/// Where a press came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Pad,
    Key,
}

/// Keyboard keys that map to pad buttons are dropped while the controller was used this recently.
const PAD_OWNS_KEYS: f64 = 1.5;
/// The same button from the other source within this long is a duplicate.
const DUPLICATE: f64 = 0.15;
/// View or Menu waits this long for the other one (the way-out chord) before it acts.
const CHORD_WINDOW: f64 = 0.18;
/// An A or a mouse click this recent makes an activation deliberate (a game may start).
const DELIBERATE: f64 = 0.8;
/// A mouse click after the controller was quiet this long leaves controller mode.
const MOUSE_QUIET: f64 = 3.0;
/// Held-direction move ticks closer than this play no second sound.
pub const MOVE_TICK: f64 = 0.045;

/// What an unclaimed press did to focus (the caller plays the feedback and runs A).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Focus moved (feedback "move").
    Moved,
    /// A: run this node's action (feedback "select").
    Activate(Id),
    /// Nothing happened (an edge, or focus was only placed).
    None,
}

/// input.ts's dispatcher state: arbitration, the View / Menu window, controller mode.
#[derive(Default)]
pub struct Dispatch {
    pad_mode: bool,
    last_pad: Option<f64>,
    last: Option<(Pad, f64, Source)>,
    last_a: Option<f64>,
    last_mouse: Option<f64>,
    pending: Option<(Pad, f64)>,
    /// RARE_DEBUG_INPUT: log every source and decision.
    pub debug: bool,
}

/// A pointer event from the window (winit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Touch,
    MouseDown,
    /// A wheel or trackpad scroll.
    Wheel,
}

impl Dispatch {
    fn say(&self, m: impl FnOnce() -> String) {
        if self.debug {
            rare_input::log(&format!("input {}", m()));
        }
    }

    /// Controller mode: started by any controller press, ended by a real touch or a mouse click
    /// after the controller has been quiet a while.
    pub fn controller_mode(&self) -> bool {
        self.pad_mode
    }

    /// Starting a game takes a deliberate A (never a repeat) or a mouse click; a touch (a palm on
    /// the screen while holding the handheld) never starts one.
    pub fn deliberate_activation(&self, now: f64) -> bool {
        self.last_a.is_some_and(|t| now - t < DELIBERATE) || self.last_mouse.is_some_and(|t| now - t < DELIBERATE)
    }

    /// A stick move counts as controller use.
    pub fn stick(&mut self, now: f64) {
        self.last_pad = Some(now);
    }

    fn accept(&mut self, b: Pad, repeat: bool, src: Source, now: f64) -> bool {
        if src == Source::Pad {
            self.last_pad = Some(now);
            if !self.pad_mode {
                self.say(|| "controller mode on".into());
            }
            self.pad_mode = true;
        } else if self.last_pad.is_some_and(|t| now - t < PAD_OWNS_KEYS) {
            self.say(|| format!("drop key {}: controller active", b.name()));
            return false;
        }
        if !repeat {
            if let Some((lb, at, ls)) = self.last {
                if lb == b && ls != src && now - at < DUPLICATE {
                    self.say(|| format!("drop {src:?} {}: duplicate of {ls:?}", b.name()));
                    return false;
                }
            }
            self.last = Some((b, now, src));
        }
        true
    }

    /// A press from a pad or the keyboard (`own`: the handheld's own keys, which skip the
    /// arbitration). Returns the presses to dispatch now, in order (View and Menu may wait for
    /// each other; `tick` releases them).
    pub fn press(&mut self, b: Pad, repeat: bool, src: Source, own: bool, now: f64) -> Vec<(Pad, bool)> {
        if !own && !self.accept(b, repeat, src, now) {
            return Vec::new();
        }
        if !repeat && b == Pad::A {
            self.last_a = Some(now);
        }
        if src == Source::Pad && matches!(b, Pad::Select | Pad::Start) {
            return self.chordable(b, now);
        }
        vec![(b, repeat)]
    }

    /// View + Menu: a View or Menu press waits CHORD_WINDOW; when the other one comes in that
    /// time, neither acts (the native side opens the Switcher after 0.6 s).
    fn chordable(&mut self, b: Pad, now: f64) -> Vec<(Pad, bool)> {
        let mut out = Vec::new();
        if let Some((first, _)) = self.pending.take() {
            if first != b {
                self.say(|| "View + Menu: chord, neither acts".into());
                return out;
            }
            out.push((first, false));
        }
        self.pending = Some((b, now + CHORD_WINDOW));
        out
    }

    /// A View or Menu whose window ran out.
    pub fn tick(&mut self, now: f64) -> Vec<(Pad, bool)> {
        match self.pending {
            Some((b, at)) if now >= at => {
                self.pending = None;
                vec![(b, false)]
            }
            _ => Vec::new(),
        }
    }

    pub fn next_wake(&self) -> Option<f64> {
        self.pending.map(|(_, at)| at)
    }

    /// A pointer event: true when the app should act on it. In controller mode wheel and mouse
    /// clicks are swallowed (Steam's Desktop Configuration turns the sticks and trackpads into
    /// mouse moves, clicks and scrolling; stick drift would scroll pages).
    pub fn pointer(&mut self, p: Pointer, now: f64) -> bool {
        if p == Pointer::MouseDown {
            self.last_mouse = Some(now);
        }
        let quiet = !self.last_pad.is_some_and(|t| now - t <= MOUSE_QUIET);
        if p == Pointer::Touch || (quiet && p == Pointer::MouseDown) {
            if self.pad_mode {
                self.say(|| format!("controller mode off ({})", if p == Pointer::Touch { "touch" } else { "mouse click" }));
            }
            self.pad_mode = false;
            return true;
        }
        if self.pad_mode {
            self.say(|| format!("drop {p:?}"));
            return false;
        }
        true
    }

    /// A tap on the touchscreen at a window point: focus goes to the focusable under it and it is
    /// activated, exactly as A would. The activation is not
    /// deliberate, so a game never starts from a touch.
    pub fn tap(&mut self, nav: &mut Nav, x: f32, y: f32, now: f64) -> Effect {
        if !self.pointer(Pointer::Touch, now) {
            return Effect::None;
        }
        match nav.hit(x, y) {
            Some(id) => {
                nav.focus(id, crate::focus::FocusOpts { scroll: false, smooth: false }, now);
                Effect::Activate(id)
            }
            None => Effect::None,
        }
    }
}

/// What a press nobody claimed does (the tail of input.ts `dispatch`): directions move focus,
/// LT / RT jump sections, A activates.
pub fn default_action(nav: &mut Nav, b: Pad, repeat: bool, now: f64) -> Effect {
    let dir = match b {
        Pad::Up => Some(Dir::Up),
        Pad::Down => Some(Dir::Down),
        Pad::Left => Some(Dir::Left),
        Pad::Right => Some(Dir::Right),
        _ => None,
    };
    if let Some(d) = dir {
        return if nav.move_dir(d, now) { Effect::Moved } else { Effect::None };
    }
    match b {
        Pad::Lt | Pad::Rt => {
            if nav.jump(b == Pad::Rt, now) {
                Effect::Moved
            } else {
                Effect::None
            }
        }
        Pad::A if !repeat => nav.activate(now).map(Effect::Activate).unwrap_or(Effect::None),
        _ => Effect::None,
    }
}

/// App.tsx's global pad handler as a pure decision: what the launcher shell does with a press no
/// screen claimed first.
pub mod shell {
    use super::Pad;

    /// The overlay over the launcher's tabs, as far as the buttons care.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Overlay {
        /// First run and sign-in keep every system button.
        FirstRun,
        SignIn,
        /// Quick Settings.
        Quick,
        RareMenu,
        /// The Switcher (what is running).
        Running,
        /// A game's page.
        Game,
        /// Any other page or sheet over a tab.
        Other,
    }

    /// The shell's state for one decision.
    #[derive(Clone, Copy, Debug)]
    pub struct State {
        pub overlay: Option<Overlay>,
        /// The current tab's index in the bar, None when it is not in the bar (Friends, Settings).
        pub tab: Option<usize>,
        pub tabs: usize,
        /// A game is focused (or a game page is open): Menu opens its Options.
        pub game: bool,
        /// A launch still going after 15 s with no window (lib/progress.ts): B cancels it.
        pub slow_start: bool,
    }

    /// What to do.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Act {
        /// Close any overlay, go to this tab (feedback "tab").
        Tab(usize),
        /// Cancel the slow start (feedback "back", toast "Cancelled").
        CancelStart,
        /// Go back one step: overlay, history, then Home (feedback "back" when it went somewhere).
        Back,
        /// Quick Settings open (feedback "open") or closed (feedback "close").
        Quick { open: bool },
        /// Close everything and open the Rare menu.
        RareMenu,
        /// The Rare menu was open: close it and go Home (feedback "back").
        Home,
        /// The Switcher.
        Switcher,
        /// Options for the focused game (or the game page's game).
        Options,
        /// Search (feedback "open").
        Search,
        /// Claimed, nothing to do (setup keeps the button).
        Swallow,
    }

    /// LB / RB through the bar's tabs, wrapping. From a page that is not in the bar the bumpers go
    /// to the first or last tab.
    pub fn tab_step(cur: Option<usize>, n: usize, forward: bool) -> usize {
        match cur {
            None => {
                if forward {
                    0
                } else {
                    n - 1
                }
            }
            Some(i) => (i + if forward { 1 } else { n - 1 }) % n,
        }
    }

    /// The decision for `b` (None: the press is not the shell's; it goes to the default action).
    pub fn decide(b: Pad, s: &State) -> Option<Act> {
        let setup = matches!(s.overlay, Some(Overlay::FirstRun | Overlay::SignIn));
        Some(match b {
            // LB / RB are always the launcher's tabs. A page or sheet over the tab closes first;
            // only the first run and sign-in keep them.
            Pad::Lb | Pad::Rb => {
                if setup {
                    Act::Swallow
                } else {
                    Act::Tab(tab_step(s.tab, s.tabs, b == Pad::Rb))
                }
            }
            Pad::B => {
                if s.overlay.is_none() && s.slow_start {
                    Act::CancelStart
                } else {
                    Act::Back
                }
            }
            // Setup: Quick opens Quick Settings (Wi-Fi, brightness, volume are needed).
            Pad::Qam => Act::Quick { open: s.overlay != Some(Overlay::Quick) },
            // The Rare button with the launcher in front: the Rare menu; pressed again, Home.
            Pad::Guide => {
                if setup {
                    Act::Swallow
                } else if s.overlay == Some(Overlay::RareMenu) {
                    Act::Home
                } else {
                    Act::RareMenu
                }
            }
            // The Rare button held, or View + Menu: the Switcher.
            Pad::GuideHold | Pad::ExitHold => {
                if setup || s.overlay == Some(Overlay::Running) {
                    Act::Swallow
                } else {
                    Act::Switcher
                }
            }
            // Menu: Options for the focused item. Only on the launcher's screens and a game's page.
            Pad::Start => {
                if (s.overlay.is_none() || s.overlay == Some(Overlay::Game)) && s.game {
                    Act::Options
                } else {
                    return None;
                }
            }
            Pad::Y => {
                if s.overlay.is_some() {
                    return None;
                }
                Act::Search
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::shell::{decide, tab_step, Act, Overlay, State};
    use super::*;
    use crate::focus::{Focusable, Group};
    use taffy::NodeId;

    #[test]
    fn keys_map_like_the_web_launcher() {
        assert_eq!(key_to_pad("ArrowUp"), Some(Pad::Up));
        assert_eq!(key_to_pad(" "), Some(Pad::A));
        assert_eq!(key_to_pad("Backspace"), Some(Pad::B));
        assert_eq!(key_to_pad("Tab"), Some(Pad::Start));
        assert_eq!(key_to_pad("PageDown"), Some(Pad::Rt));
        assert_eq!(key_to_pad("F18"), Some(Pad::Qam));
        assert_eq!(key_to_pad("F17"), Some(Pad::Guide));
        assert_eq!(key_to_pad("z"), None);
        assert!(own_key("F17") && !own_key("Home"));
    }

    #[test]
    fn the_controller_owns_input_and_steam_duplicates_are_dropped() {
        let mut d = Dispatch::default();
        assert_eq!(d.press(Pad::Down, false, Source::Pad, false, 0.0), vec![(Pad::Down, false)]);
        assert!(d.controller_mode());
        // Steam's Desktop Configuration sends the same press as a key: dropped.
        assert!(d.press(Pad::Down, false, Source::Key, false, 0.05).is_empty());
        // Any key while the pad was used in the last 1.5 s.
        assert!(d.press(Pad::A, false, Source::Key, false, 1.0).is_empty());
        assert_eq!(d.press(Pad::A, false, Source::Key, false, 1.6), vec![(Pad::A, false)]);
        // The handheld's own keys always count.
        d.press(Pad::Up, false, Source::Pad, false, 2.0);
        assert_eq!(d.press(Pad::Qam, false, Source::Key, true, 2.01), vec![(Pad::Qam, false)]);
        // A key first, then the pad's copy of it within 150 ms: a duplicate too.
        let mut d = Dispatch::default();
        d.press(Pad::B, false, Source::Key, false, 0.0);
        assert!(d.press(Pad::B, false, Source::Pad, false, 0.1).is_empty());
        assert_eq!(d.press(Pad::B, false, Source::Pad, false, 0.3), vec![(Pad::B, false)]);
    }

    #[test]
    fn view_and_menu_wait_for_each_other() {
        let mut d = Dispatch::default();
        // Alone: Menu acts once its window runs out.
        assert!(d.press(Pad::Start, false, Source::Pad, false, 0.0).is_empty());
        assert_eq!(d.next_wake(), Some(0.18));
        assert!(d.tick(0.1).is_empty());
        assert_eq!(d.tick(0.18), vec![(Pad::Start, false)]);
        // Together (the way out): neither acts.
        assert!(d.press(Pad::Select, false, Source::Pad, false, 1.0).is_empty());
        assert!(d.press(Pad::Start, false, Source::Pad, false, 1.05).is_empty());
        assert!(d.tick(2.0).is_empty());
        // Twice the same: the first acts at once, the second waits.
        assert!(d.press(Pad::Select, false, Source::Pad, false, 3.0).is_empty());
        assert_eq!(d.press(Pad::Select, false, Source::Pad, false, 3.1), vec![(Pad::Select, false)]);
        assert_eq!(d.tick(3.3), vec![(Pad::Select, false)]);
        // From the keyboard there is no chord: Tab is Menu at once.
        assert_eq!(d.press(Pad::Start, false, Source::Key, false, 10.0), vec![(Pad::Start, false)]);
    }

    #[test]
    fn touch_leaves_controller_mode_and_never_starts_a_game() {
        let mut d = Dispatch::default();
        let mut nav = Nav::new(1920.0, 1200.0);
        let g = nav.add_group(Group::new("strip", [0.0, 0.0, 1000.0, 200.0]).rail());
        nav.add(Focusable::new(NodeId::from(1u64), [0.0, 0.0, 200.0, 200.0]).group(g));
        nav.add(Focusable::new(NodeId::from(2u64), [220.0, 0.0, 200.0, 200.0]).group(g));
        d.press(Pad::Right, false, Source::Pad, false, 0.0);
        assert!(!d.pointer(Pointer::Wheel, 0.5), "stick drift as scrolling is swallowed");
        assert!(!d.pointer(Pointer::MouseDown, 0.6), "a trackpad click while the pad is in use");
        assert_eq!(d.tap(&mut nav, 300.0, 100.0, 1.0), Effect::Activate(NodeId::from(2u64)));
        assert_eq!(nav.current(), Some(NodeId::from(2u64)));
        assert!(!d.controller_mode());
        assert!(!d.deliberate_activation(1.5), "a tap is not a deliberate A");
        assert_eq!(d.tap(&mut nav, 1500.0, 100.0, 1.1), Effect::None);
        d.press(Pad::A, false, Source::Pad, false, 2.0);
        assert!(d.deliberate_activation(2.5));
        assert!(!d.deliberate_activation(3.0));
        // A mouse click after the pad was quiet for 3 s leaves controller mode and is deliberate.
        assert!(d.pointer(Pointer::MouseDown, 6.0));
        assert!(d.deliberate_activation(6.1));
    }

    #[test]
    fn unclaimed_presses_move_focus_jump_and_activate() {
        let mut nav = Nav::new(1920.0, 1200.0);
        let a = nav.add_group(Group::new("a", [0.0, 0.0, 1000.0, 200.0]));
        let b = nav.add_group(Group::new("b", [0.0, 400.0, 1000.0, 200.0]));
        nav.add(Focusable::new(NodeId::from(1u64), [0.0, 0.0, 200.0, 200.0]).group(a));
        nav.add(Focusable::new(NodeId::from(2u64), [220.0, 0.0, 200.0, 200.0]).group(a));
        nav.add(Focusable::new(NodeId::from(3u64), [0.0, 400.0, 200.0, 200.0]).group(b));
        // Nothing focused yet: A places focus and does not activate.
        assert_eq!(default_action(&mut nav, Pad::A, false, 0.0), Effect::None);
        assert_eq!(default_action(&mut nav, Pad::Right, false, 0.0), Effect::Moved);
        assert_eq!(default_action(&mut nav, Pad::Right, true, 0.0), Effect::None, "an edge");
        assert_eq!(default_action(&mut nav, Pad::Rt, false, 0.0), Effect::Moved);
        assert_eq!(nav.current(), Some(NodeId::from(3u64)));
        assert_eq!(default_action(&mut nav, Pad::A, true, 0.0), Effect::None, "a held A never repeats");
        assert_eq!(default_action(&mut nav, Pad::A, false, 0.0), Effect::Activate(NodeId::from(3u64)));
        assert_eq!(default_action(&mut nav, Pad::X, false, 0.0), Effect::None);
    }

    fn st(overlay: Option<Overlay>, tab: Option<usize>) -> State {
        State { overlay, tab, tabs: 5, game: false, slow_start: false }
    }

    #[test]
    fn bumpers_wrap_through_five_tabs_in_at_most_two_presses() {
        // market/nav.test.ts: any tab is at most two presses away with wrap-around.
        for from in 0..5 {
            for to in 0..5 {
                let (mut r, mut l, mut i, mut j) = (0, 0, from, from);
                while i != to {
                    i = tab_step(Some(i), 5, true);
                    r += 1;
                }
                while j != to {
                    j = tab_step(Some(j), 5, false);
                    l += 1;
                }
                assert!(r.min(l) <= 2, "{from} -> {to}");
            }
        }
        assert_eq!(tab_step(Some(4), 5, true), 0);
        assert_eq!(tab_step(Some(0), 5, false), 4);
        // From Friends or Settings (not in the bar): the first or last tab.
        assert_eq!(tab_step(None, 5, true), 0);
        assert_eq!(tab_step(None, 5, false), 4);
        assert_eq!(decide(Pad::Rb, &st(Some(Overlay::Other), Some(1))), Some(Act::Tab(2)));
        assert_eq!(decide(Pad::Lb, &st(Some(Overlay::FirstRun), Some(0))), Some(Act::Swallow));
    }

    #[test]
    fn the_system_buttons_follow_the_ux_model() {
        // Rare: the menu; again: Home; held (or View + Menu): the Switcher; nothing in setup.
        assert_eq!(decide(Pad::Guide, &st(None, Some(0))), Some(Act::RareMenu));
        assert_eq!(decide(Pad::Guide, &st(Some(Overlay::Quick), Some(0))), Some(Act::RareMenu));
        assert_eq!(decide(Pad::Guide, &st(Some(Overlay::RareMenu), Some(2))), Some(Act::Home));
        assert_eq!(decide(Pad::Guide, &st(Some(Overlay::SignIn), None)), Some(Act::Swallow));
        assert_eq!(decide(Pad::GuideHold, &st(None, Some(0))), Some(Act::Switcher));
        assert_eq!(decide(Pad::ExitHold, &st(Some(Overlay::Running), Some(0))), Some(Act::Swallow));
        assert_eq!(decide(Pad::ExitHold, &st(Some(Overlay::FirstRun), None)), Some(Act::Swallow));
        // Quick: open, or close when it is open; also during setup.
        assert_eq!(decide(Pad::Qam, &st(None, Some(0))), Some(Act::Quick { open: true }));
        assert_eq!(decide(Pad::Qam, &st(Some(Overlay::Quick), Some(0))), Some(Act::Quick { open: false }));
        assert_eq!(decide(Pad::Qam, &st(Some(Overlay::FirstRun), None)), Some(Act::Quick { open: true }));
    }

    #[test]
    fn b_menu_view_and_y() {
        assert_eq!(decide(Pad::B, &st(None, Some(1))), Some(Act::Back));
        assert_eq!(decide(Pad::B, &State { slow_start: true, ..st(None, Some(0)) }), Some(Act::CancelStart));
        assert_eq!(decide(Pad::B, &State { slow_start: true, ..st(Some(Overlay::Other), Some(0)) }), Some(Act::Back));
        // Menu = Options for a focused game, on a tab or a game's page; elsewhere the screen's.
        assert_eq!(decide(Pad::Start, &State { game: true, ..st(None, Some(0)) }), Some(Act::Options));
        assert_eq!(decide(Pad::Start, &State { game: true, ..st(Some(Overlay::Game), Some(0)) }), Some(Act::Options));
        assert_eq!(decide(Pad::Start, &State { game: true, ..st(Some(Overlay::Quick), Some(0)) }), None);
        assert_eq!(decide(Pad::Start, &st(None, Some(0))), None);
        // View is always the screen's (sort and filter, list or grid, chart mode).
        assert_eq!(decide(Pad::Select, &st(None, Some(1))), None);
        // Y: Search on the tabs; panels keep their own Y.
        assert_eq!(decide(Pad::Y, &st(None, Some(0))), Some(Act::Search));
        assert_eq!(decide(Pad::Y, &st(Some(Overlay::Game), Some(0))), None);
        // LT / RT and the d-pad are never the shell's.
        for b in [Pad::Lt, Pad::Rt, Pad::Up, Pad::A, Pad::X] {
            assert_eq!(decide(b, &st(None, Some(0))), None);
        }
    }
}
