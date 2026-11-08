//! Rare's keyboard over a game or app: Rare + X, LC or RC set to Keyboard, Quick settings'
//! Keyboard, Y in an app the pad drives as a mouse. It types into the window that has focus
//! (Steam's sign-in and Steam Guard, a game's name field, Chrome), live, key by key.
//!
//! How it stays on screen and types into the app, under gamescope:
//! - It is drawn in the HUD's window (rare_system::hud), an override-redirect window on gamescope's
//!   first Xwayland that carries `STEAM_OVERLAY`: gamescope paints it on the overlay plane over the
//!   focused app, as it paints Steam's own keyboard, and it never becomes the focus or input window
//!   (no `STEAM_INPUT_FOCUS`). So the app keeps keyboard focus. The launcher's own keyboard
//!   (screens/osk.rs) is part of the launcher's window, which is behind the app: it never showed
//!   over Steam or a game, and its text only ever went to Rare's own fields.
//! - The keys go out on a virtual keyboard of Rare's own on /dev/uinput (US layout, as the phone's
//!   Type): gamescope sends them to the window that has keyboard focus, the app.
//! - While it is up, InputPlumber intercepts the pad (rare_input::ip::capture), so the app gets no
//!   pad input and every press comes to the launcher's pad thread, which hands it here first
//!   (`pad`). Rare and Quick still do what they always do; the keyboard goes first.
//!
//! The d-pad moves (rows wrap sideways), A types, X deletes, Y is Shift, Menu is Enter, View is
//! Tab, LB and RB move the cursor, B closes.

use rare_input::Pad;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const LOWER: [[&str; 10]; 4] = [
    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
    ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
    ["a", "s", "d", "f", "g", "h", "j", "k", "l", "@"],
    ["z", "x", "c", "v", "b", "n", "m", ".", "-", "_"],
];
const UPPER: [[&str; 10]; 4] = [
    ["!", "?", "#", "$", "%", "&", "*", "(", ")", "/"],
    ["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P"],
    ["A", "S", "D", "F", "G", "H", "J", "K", "L", ":"],
    ["Z", "X", "C", "V", "B", "N", "M", ",", "'", "+"],
];
/// The special row: what each key does, its label, its width.
const SPECIAL: [(Special, &str, f32); 5] = [(Special::Shift, "Shift", 1.4), (Special::Space, "Space", 4.0), (Special::Delete, "Delete", 1.4), (Special::Enter, "Enter", 1.4), (Special::Close, "Close", 1.4)];
const HINT: &str = "A Type    X Delete    Y Shift    Menu Enter    View Tab    LB RB Cursor    B Close";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Special {
    Shift,
    Space,
    Delete,
    Enter,
    Close,
}

// Linux input key codes.
const KEY_BACKSPACE: u16 = 14;
const KEY_TAB: u16 = 15;
const KEY_ENTER: u16 = 28;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_SPACE: u16 = 57;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;

/// What a press does to the app (sent to the typist thread).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    /// A key, with Shift held.
    Key(u16, bool),
}

/// What a button press asks of the keyboard (pure: tests drive it).
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Draw it again.
    Redraw,
    /// Type these, then draw it again.
    Type(Vec<Tap>),
    /// Take it down.
    Close,
    /// Nothing changed.
    Nothing,
}

/// The keyboard's state: where focus is and whether Shift is on.
#[derive(Clone, Debug, PartialEq)]
pub struct Kb {
    pub shift: bool,
    /// [row, col]; row 4 is the special row.
    pub pos: (usize, usize),
}

impl Default for Kb {
    fn default() -> Self {
        Kb { shift: false, pos: (1, 0) }
    }
}

impl Kb {
    fn rows(&self) -> &'static [[&'static str; 10]; 4] {
        if self.shift {
            &UPPER
        } else {
            &LOWER
        }
    }

    /// A button press (or a held direction's repeat).
    pub fn press(&mut self, p: Pad, repeat: bool) -> Out {
        let (r, c) = self.pos;
        let last = 4;
        let n = SPECIAL.len();
        let cols = |row: usize| if row == last { n } else { 10 };
        let next = match p {
            Pad::Up => Some(if r == 0 { (last, ((c as f32 / 9.0) * (n - 1) as f32).round() as usize) } else if r == last { (r - 1, ((c as f32 / (n - 1) as f32) * 9.0).round() as usize) } else { (r - 1, c) }),
            Pad::Down => Some(if r + 1 == last { (last, ((c as f32 / 9.0) * (n - 1) as f32).round() as usize) } else if r == last { (0, ((c as f32 / (n - 1) as f32) * 9.0).round() as usize) } else { (r + 1, c) }),
            Pad::Left => Some((r, (c + cols(r) - 1) % cols(r))),
            Pad::Right => Some((r, (c + 1) % cols(r))),
            _ => None,
        };
        if let Some(to) = next {
            if to == self.pos {
                return Out::Nothing;
            }
            self.pos = to;
            return Out::Redraw;
        }
        // Only directions repeat; a held A or X types once.
        if repeat {
            return Out::Nothing;
        }
        match p {
            Pad::A => {
                if r == last {
                    self.special(SPECIAL[c.min(n - 1)].0)
                } else {
                    let ch = self.rows()[r][c];
                    let taps: Vec<Tap> = ch.chars().filter_map(key_of).map(|(k, s)| Tap::Key(k, s)).collect();
                    if self.shift && ch.chars().all(|c| c.is_ascii_uppercase()) {
                        self.shift = false;
                    }
                    Out::Type(taps)
                }
            }
            Pad::X => self.special(Special::Delete),
            Pad::Y => self.special(Special::Shift),
            Pad::Start => self.special(Special::Enter),
            Pad::Select => Out::Type(vec![Tap::Key(KEY_TAB, false)]),
            Pad::Lb => Out::Type(vec![Tap::Key(KEY_LEFT, false)]),
            Pad::Rb => Out::Type(vec![Tap::Key(KEY_RIGHT, false)]),
            Pad::B => Out::Close,
            _ => Out::Nothing,
        }
    }

    fn special(&mut self, s: Special) -> Out {
        match s {
            Special::Shift => {
                self.shift = !self.shift;
                Out::Redraw
            }
            Special::Space => Out::Type(vec![Tap::Key(KEY_SPACE, false)]),
            Special::Delete => Out::Type(vec![Tap::Key(KEY_BACKSPACE, false)]),
            Special::Enter => Out::Type(vec![Tap::Key(KEY_ENTER, false)]),
            Special::Close => Out::Close,
        }
    }

    /// What the HUD draws.
    pub fn view(&self) -> rare_system::hud::Keyboard {
        rare_system::hud::Keyboard {
            rows: self.rows().iter().map(|r| r.iter().map(|k| k.to_string()).collect()).collect(),
            special: SPECIAL.iter().map(|(s, l, _)| if *s == Special::Shift && self.shift { "abc".to_string() } else { l.to_string() }).collect(),
            weights: SPECIAL.iter().map(|(_, _, w)| *w).collect(),
            focus: self.pos,
            shift: self.shift,
            hint: HINT.to_string(),
        }
    }
}

/// The key and whether Shift is held, for one character on a US layout (what the phone's Type
/// sends too). None for characters the layout has no key for.
pub fn key_of(c: char) -> Option<(u16, bool)> {
    const LOWER_ROW: &str = "qwertyuiop";
    const MID: &str = "asdfghjkl";
    const LOW: &str = "zxcvbnm";
    let l = c.to_ascii_lowercase();
    if c.is_ascii_alphabetic() {
        let code = LOWER_ROW.find(l).map(|i| 16 + i as u16).or_else(|| MID.find(l).map(|i| 30 + i as u16)).or_else(|| LOW.find(l).map(|i| 44 + i as u16))?;
        return Some((code, c.is_ascii_uppercase()));
    }
    if let Some(d) = c.to_digit(10) {
        return Some((if d == 0 { 11 } else { 1 + d as u16 }, false));
    }
    let plain = [(' ', 57), ('-', 12), ('=', 13), ('[', 26), (']', 27), (';', 39), ('\'', 40), ('`', 41), ('\\', 43), (',', 51), ('.', 52), ('/', 53), ('\n', 28), ('\t', 15)];
    if let Some((_, k)) = plain.iter().find(|(ch, _)| *ch == c) {
        return Some((*k, false));
    }
    let shifted = [('!', 2), ('@', 3), ('#', 4), ('$', 5), ('%', 6), ('^', 7), ('&', 8), ('*', 9), ('(', 10), (')', 11), ('_', 12), ('+', 13), ('{', 26), ('}', 27), (':', 39), ('"', 40), ('~', 41), ('|', 43), ('<', 51), ('>', 52), ('?', 53)];
    shifted.iter().find(|(ch, _)| *ch == c).map(|(_, k)| (*k, true))
}

/// Every key the virtual keyboard can send.
pub fn device_keys() -> Vec<u16> {
    let mut keys: Vec<u16> = (1..=57).collect();
    keys.extend([KEY_LEFT, KEY_RIGHT]);
    keys
}

struct Live {
    kb: Kb,
    open: bool,
}

static LIVE: Mutex<Live> = Mutex::new(Live { kb: Kb { shift: false, pos: (1, 0) }, open: false });

fn live() -> std::sync::MutexGuard<'static, Live> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The typist: one thread that owns the virtual keyboard, so a press never waits for the keys.
enum Job {
    /// Open the device ahead of the first key (gamescope needs a moment to pick it up).
    Warm,
    Taps(Vec<Tap>),
}

fn typist() -> &'static Mutex<Sender<Job>> {
    static TX: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("rare-typist".into())
            .spawn(move || {
                let mut dev: Option<rare_launch::apps::pointer::uinput::Device> = None;
                while let Ok(job) = rx.recv() {
                    if dev.is_none() {
                        match rare_launch::apps::pointer::uinput::Device::open_keys("Rare keyboard", &device_keys()) {
                            Ok(d) => {
                                dev = Some(d);
                                // The compositor needs a moment to see a new input device.
                                std::thread::sleep(Duration::from_millis(350));
                            }
                            Err(e) => {
                                log::warn!("keyboard: no virtual keyboard ({e})");
                                crate::side::toast("The keyboard can't type here: no virtual keyboard on this device", true);
                                continue;
                            }
                        }
                    }
                    let Some(d) = dev.as_mut() else { continue };
                    let Job::Taps(taps) = job else { continue };
                    for Tap::Key(code, shift) in taps {
                        if shift {
                            d.key(KEY_LEFTSHIFT, true);
                            d.syn();
                        }
                        d.key(code, true);
                        d.syn();
                        std::thread::sleep(Duration::from_millis(6));
                        d.key(code, false);
                        d.syn();
                        if shift {
                            d.key(KEY_LEFTSHIFT, false);
                            d.syn();
                        }
                        std::thread::sleep(Duration::from_millis(4));
                    }
                }
            })
            .expect("typist thread");
        Mutex::new(tx)
    })
}

fn send(job: Job) {
    let _ = typist().lock().unwrap_or_else(|e| e.into_inner()).send(job);
}

/// Rare OS game mode: the HUD window and InputPlumber's capture are there. Elsewhere (a desktop
/// session, Rare as a shortcut in someone else's Steam) the launcher's own keyboard is used.
pub fn available() -> bool {
    rare_overlay::gamescope::session_client() || std::env::var_os("RARE_TYPEKB").is_some()
}

/// A game or app has the screen (gamescope shows it, not Rare): the keyboard goes over it.
pub fn wanted() -> bool {
    available() && rare_overlay::gamescope::other_app_shown() && !crate::app::surface_up()
}

pub fn is_open() -> bool {
    live().open
}

/// Brings the keyboard up over the app in front.
pub fn open() {
    {
        let mut l = live();
        if l.open {
            return;
        }
        l.open = true;
        l.kb = Kb::default();
        rare_system::hud::keyboard(Some(l.kb.view()));
    }
    log::info!("keyboard: open over the app in front");
    rare_input::ip::capture(true);
    send(Job::Warm);
}

/// Takes it down; the app gets the pad back.
pub fn close() {
    {
        let mut l = live();
        if !l.open {
            return;
        }
        l.open = false;
    }
    log::info!("keyboard: closed");
    rare_system::hud::keyboard(None);
    rare_input::ip::capture(false);
}

/// A press from the pad thread, before anything else sees it: true when the keyboard took it.
/// Rare and Quick close it and go on to do what they always do.
pub fn pad(p: Pad, repeat: bool) -> bool {
    let out = {
        let mut l = live();
        if !l.open {
            return false;
        }
        if matches!(p, Pad::Guide | Pad::GuideHold | Pad::Qam) {
            drop(l);
            close();
            return false;
        }
        let out = l.kb.press(p, repeat);
        if matches!(out, Out::Redraw | Out::Type(_)) {
            rare_system::hud::keyboard(Some(l.kb.view()));
        }
        out
    };
    match out {
        Out::Type(taps) if !taps.is_empty() => send(Job::Taps(taps)),
        Out::Close => close(),
        _ => {}
    }
    true
}

/// Every update: the keyboard goes when a Rare surface comes up or the app it was over left the
/// screen.
pub fn tick() {
    if is_open() && (crate::app::surface_up() || !rare_overlay::gamescope::other_app_shown()) {
        close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_moves_and_shifts() {
        let mut k = Kb::default();
        // Starts on q.
        assert_eq!(k.press(Pad::A, false), Out::Type(vec![Tap::Key(16, false)]));
        assert_eq!(k.press(Pad::Right, false), Out::Redraw);
        assert_eq!(k.pos, (1, 1));
        // Y: Shift, then W is typed with Shift and Shift goes off again.
        assert_eq!(k.press(Pad::Y, false), Out::Redraw);
        assert!(k.shift);
        assert_eq!(k.press(Pad::A, false), Out::Type(vec![Tap::Key(17, true)]));
        assert!(!k.shift);
        // Symbols stay shifted while Shift is on.
        k.press(Pad::Y, false);
        k.pos = (0, 0);
        assert_eq!(k.press(Pad::A, false), Out::Type(vec![Tap::Key(2, true)]));
        assert!(k.shift);
    }

    #[test]
    fn buttons() {
        let mut k = Kb::default();
        assert_eq!(k.press(Pad::X, false), Out::Type(vec![Tap::Key(KEY_BACKSPACE, false)]));
        assert_eq!(k.press(Pad::Start, false), Out::Type(vec![Tap::Key(KEY_ENTER, false)]));
        assert_eq!(k.press(Pad::Select, false), Out::Type(vec![Tap::Key(KEY_TAB, false)]));
        assert_eq!(k.press(Pad::Lb, false), Out::Type(vec![Tap::Key(KEY_LEFT, false)]));
        assert_eq!(k.press(Pad::B, false), Out::Close);
        // Held A does not type again; held directions move.
        assert_eq!(k.press(Pad::A, true), Out::Nothing);
        assert_eq!(k.press(Pad::Down, true), Out::Redraw);
    }

    #[test]
    fn wraps_and_reaches_the_special_row() {
        let mut k = Kb::default();
        k.press(Pad::Left, false);
        assert_eq!(k.pos, (1, 9));
        k.press(Pad::Down, false);
        k.press(Pad::Down, false);
        k.press(Pad::Down, false);
        assert_eq!(k.pos, (4, 4));
        assert_eq!(k.press(Pad::A, false), Out::Close);
        k.pos = (4, 1);
        assert_eq!(k.press(Pad::A, false), Out::Type(vec![Tap::Key(KEY_SPACE, false)]));
        // Down from the special row wraps to the top, Up from the top to the special row.
        k.press(Pad::Down, false);
        assert_eq!(k.pos.0, 0);
        k.press(Pad::Up, false);
        assert_eq!(k.pos.0, 4);
    }

    #[test]
    fn every_key_can_be_typed() {
        let keys = device_keys();
        for rows in [&LOWER, &UPPER] {
            for row in rows.iter() {
                for ch in row.iter().flat_map(|k| k.chars()) {
                    let (code, _) = key_of(ch).unwrap_or_else(|| panic!("no key for {ch}"));
                    assert!(keys.contains(&code), "{ch}");
                }
            }
        }
        for (s, _, _) in SPECIAL {
            if let Out::Type(t) = Kb::default().special(s) {
                assert!(t.iter().all(|&Tap::Key(c, _)| keys.contains(&c)));
            }
        }
        assert!(keys.contains(&KEY_LEFTSHIFT));
    }

    #[test]
    fn view_matches_the_layout() {
        let mut k = Kb::default();
        let v = k.view();
        assert_eq!(v.rows.len(), 4);
        assert_eq!(v.special.len(), SPECIAL.len());
        assert_eq!(v.focus, (1, 0));
        k.press(Pad::Y, false);
        let v = k.view();
        assert_eq!(v.rows[1][0], "Q");
        assert_eq!(v.special[0], "abc");
    }
}
