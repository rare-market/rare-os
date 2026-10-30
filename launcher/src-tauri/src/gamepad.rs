//! Controller input, read natively (evdev on Linux through gilrs) and sent to the UI as
//! navigation events. The web Gamepad API is not used: WebKitGTK's support depends on how it was
//! built, and reading the devices here also works while the window is unfocused.
//!
//! Thin wrapper: what a press means (repeat, triggers, bounce, one pad at a time, Guide press and
//! hold, Guide + A, the share chords, View + Menu) lives in the rare-input crate's pad state
//! machine (launcher/crates/rare-input/src/pad.rs), shared with the native launcher. This module
//! is the gilrs event source, the rumble player and the web game / pointer mode analog state.

use rare_input::pad::xy_swapped;
pub use rare_input::{debug_input, log, Pad};

use crate::deckpad::Ev;
use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Repeat, Replay, Ticks};
use gilrs::{Axis, Button, EventType, Gilrs};
use rare_input::pad::{Btn, Machine, Out, Src, DECK};
use serde::Serialize;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone)]
pub struct PadEvent {
    pub button: Pad,
    /// True for the auto-repeat of a held direction.
    pub repeat: bool,
}

fn share(c: rare_input::chords::Share) -> crate::share::Chord {
    match c {
        rare_input::chords::Share::Screenshot => crate::share::Chord::Screenshot,
        rare_input::chords::Share::Clip => crate::share::Chord::Clip,
    }
}

const POLL: Duration = Duration::from_millis(16);
/// With nothing held, no hold timer or rumble running and no pad event for HOT, the loop waits
/// this long for the next event instead of POLL: the input thread wakes 4 times a second instead
/// of 60 when nobody touches the controller. A press still arrives at once (the wait ends on
/// any event); only a rumble asked for with no input in the last HOT can start up to this late.
const COLD_POLL: Duration = Duration::from_millis(250);
const HOT: Duration = Duration::from_secs(2);
const STICK_DEADZONE: f32 = rare_input::pad::STICK_DEADZONE;

/// Short controller rumbles the UI asks for (focus moves, select, back).
pub use rare_input::rumble::Rumble;

static RUMBLE: OnceLock<Mutex<Sender<Rumble>>> = OnceLock::new();

/// Queues a rumble on every connected pad that supports force feedback. Never blocks.
pub fn rumble(kind: &str, strength: f32) -> Result<(), String> {
    let r = Rumble::from_kind(kind).ok_or_else(|| format!("unknown rumble kind {kind}"))?.scaled(strength);
    if strength <= 0.0 {
        return Ok(());
    }
    if let Some(tx) = RUMBLE.get() {
        let _ = tx.lock().unwrap().send(r);
    }
    Ok(())
}

fn play_rumble(gilrs: &mut Gilrs, r: Rumble) -> Option<(Effect, Instant)> {
    let ids: Vec<_> = gilrs.gamepads().filter(|(_, g)| g.is_connected() && g.is_ff_supported()).map(|(id, _)| id).collect();
    if ids.is_empty() {
        return None;
    }
    // gilrs drives force feedback in 50 ms ticks, so very short pulses round up to one tick.
    let scheduling = Replay { play_for: Ticks::from_ms(r.ms), ..Default::default() };
    let mag = |v: f32| (v.clamp(0.0, 1.0) * u16::MAX as f32) as u16;
    let mut b = EffectBuilder::new();
    b.add_effect(BaseEffect { kind: BaseEffectType::Weak { magnitude: mag(r.weak) }, scheduling, envelope: Default::default() });
    if r.strong > 0.0 {
        b.add_effect(BaseEffect { kind: BaseEffectType::Strong { magnitude: mag(r.strong) }, scheduling, envelope: Default::default() });
    }
    let effect = b.gamepads(&ids).repeat(Repeat::For(Ticks::from_ms(r.ms))).finish(gilrs).ok()?;
    effect.play().ok()?;
    // Keep the effect alive until it has played, then drop it.
    Some((effect, Instant::now() + Duration::from_millis(r.ms as u64 + 120)))
}

fn pad_swaps(g: &gilrs::Gamepad) -> bool {
    xy_swapped(g.name(), g.vendor_id())
}

/// The full state of a pad in the web's standard mapping, for web games that read the
/// Gamepad API: 17 buttons (triggers analog) and 4 axes with down positive.
pub fn standard_state(g: &gilrs::Gamepad) -> ([f32; 17], [f32; 4]) {
    let swap = pad_swaps(g);
    let fix = |x: Button| match (swap, x) {
        (true, Button::West) => Button::North,
        (true, Button::North) => Button::West,
        _ => x,
    };
    let b = |x: Button| if g.is_pressed(fix(x)) { 1.0 } else { 0.0 };
    let trig = |x: Button, a: Axis| g.button_data(x).map(|d| d.value()).unwrap_or(0.0).max(g.value(a).max(0.0));
    let buttons = [
        b(Button::South),
        b(Button::East),
        b(Button::West),
        b(Button::North),
        b(Button::LeftTrigger),
        b(Button::RightTrigger),
        trig(Button::LeftTrigger2, Axis::LeftZ),
        trig(Button::RightTrigger2, Axis::RightZ),
        b(Button::Select),
        b(Button::Start),
        b(Button::LeftThumb),
        b(Button::RightThumb),
        b(Button::DPadUp),
        b(Button::DPadDown),
        b(Button::DPadLeft),
        b(Button::DPadRight),
        b(Button::Mode),
    ];
    let axes = [g.value(Axis::LeftStickX), -g.value(Axis::LeftStickY), g.value(Axis::RightStickX), -g.value(Axis::RightStickY)];
    (buttons, axes)
}

fn btn(b: Button) -> Option<Btn> {
    Some(match b {
        Button::South => Btn::South,
        Button::East => Btn::East,
        Button::North => Btn::North,
        Button::West => Btn::West,
        Button::LeftTrigger => Btn::LeftTrigger,
        Button::RightTrigger => Btn::RightTrigger,
        Button::LeftTrigger2 => Btn::LeftTrigger2,
        Button::RightTrigger2 => Btn::RightTrigger2,
        Button::Select => Btn::Select,
        Button::Start => Btn::Start,
        Button::Mode => Btn::Mode,
        Button::LeftThumb => Btn::LeftThumb,
        Button::RightThumb => Btn::RightThumb,
        Button::DPadUp => Btn::DPadUp,
        Button::DPadDown => Btn::DPadDown,
        Button::DPadLeft => Btn::DPadLeft,
        Button::DPadRight => Btn::DPadRight,
        _ => return None,
    })
}

/// A gilrs event as the state machine takes it (gilrs reports stick Y up positive, as it does).
fn from_gilrs(e: EventType) -> Option<Ev> {
    use rare_input::pad::Axis as A;
    Some(match e {
        EventType::ButtonPressed(b, _) => Ev::Pressed(btn(b)?),
        EventType::ButtonReleased(b, _) => Ev::Released(btn(b)?),
        EventType::ButtonChanged(b, v, _) => Ev::Changed(btn(b)?, v),
        EventType::AxisChanged(a, v, _) => Ev::Axis(
            match a {
                Axis::LeftStickX => A::LeftStickX,
                Axis::LeftStickY => A::LeftStickY,
                Axis::RightStickX => A::RightStickX,
                Axis::RightStickY => A::RightStickY,
                Axis::LeftZ => A::LeftZ,
                Axis::RightZ => A::RightZ,
                _ => return None,
            },
            v,
        ),
        EventType::Connected => Ev::Connected,
        EventType::Disconnected => Ev::Disconnected,
        _ => return None,
    })
}

/// Starts the input thread. `route` gets every press and release (`down`) and returns
/// whether the launcher UI should see it (only presses are ever sent to the UI).
type RouteFn = dyn Fn(&AppHandle, Pad, bool) -> bool + Send + Sync;

/// The router given to `start`, for presses that come from elsewhere (InputPlumber's D-Bus
/// target: the Rare and Quick buttons, LC, and the whole pad while a Rare surface intercepts).
static ROUTE: OnceLock<Arc<RouteFn>> = OnceLock::new();

fn send(app: &AppHandle, route: &RouteFn, button: Pad, repeat: bool) {
    if route(app, button, true) {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.emit("pad", PadEvent { button, repeat });
        }
    }
}

/// A press from outside the pad loop, routed and sent to the UI exactly like a pad press.
pub fn press(app: &AppHandle, button: Pad, repeat: bool) {
    if let Some(route) = ROUTE.get() {
        send(app, route.as_ref(), button, repeat);
    }
}

/// The matching release.
pub fn release_pad(app: &AppHandle, button: Pad) {
    if let Some(route) = ROUTE.get() {
        route(app, button, false);
    }
}

pub fn start<F>(app: AppHandle, route: F)
where
    F: Fn(&AppHandle, Pad, bool) -> bool + Send + Sync + 'static,
{
    let route: Arc<RouteFn> = Arc::new(route);
    let _ = ROUTE.set(route.clone());
    let (tx, rx): (Sender<Rumble>, Receiver<Rumble>) = channel();
    let _ = RUMBLE.set(Mutex::new(tx));
    // The Deck pad has no evdev device (deckpad.rs): its reader feeds this thread directly.
    let (deck_tx, deck_rx): (Sender<Ev>, Receiver<Ev>) = channel();
    crate::deckpad::start(deck_tx);
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[pad] no gamepad backend: {e}");
                return;
            }
        };
        {
            let pads: Vec<String> = gilrs.gamepads().map(|(_, g)| format!("{} (rumble {})", g.name(), if g.is_ff_supported() { "yes" } else { "no" })).collect();
            let ff = gilrs.gamepads().filter(|(_, g)| g.is_ff_supported()).count();
            eprintln!("[pad] {} pad(s), {} with force feedback: {}", pads.len(), ff, pads.join(", "));
        }
        let mut m = Machine::new();
        m.debug = debug_input();
        let mut dbg_at = Instant::now();
        let mut out: Vec<Out> = Vec::new();
        let mut playing: Option<(Effect, Instant)> = None;
        let mut last_event = Instant::now();
        let mut last_state: Option<([f32; 17], [f32; 4])> = None;
        loop {
            // One event per pass, then the repeat and rumble checks: a steady stream of axis
            // events can never starve the auto-repeat.
            // The Deck pad reader wakes nobody: while it runs, look at its queue every POLL.
            let pointer = crate::apps::pointer::active();
            let hot = m.busy() || playing.is_some() || last_event.elapsed() < HOT || pointer || crate::webgame::visible() || crate::deckpad::active();
            let poll = if hot { POLL } else { COLD_POLL };
            let timeout = m.due().map(|d| d.saturating_duration_since(Instant::now()).min(poll)).unwrap_or(poll);
            let next: Option<(Src, Ev, bool)> = match deck_rx.try_recv() {
                Ok(e) => Some((DECK, e, false)),
                Err(_) => gilrs.next_event_blocking(Some(timeout)).and_then(|ev| {
                    last_event = Instant::now();
                    // X and Y come swapped on Xbox-named pads; the Deck report is positional.
                    let swaps = pad_swaps(&gilrs.gamepad(ev.id));
                    from_gilrs(ev.event).map(|e| (usize::from(ev.id) as Src, e, swaps))
                }),
            };
            if let Some((src, ev, swaps)) = next {
                last_event = Instant::now();
                if m.debug && !matches!(ev, Ev::Axis(_, v) | Ev::Changed(_, v) if v.abs() < STICK_DEADZONE) {
                    let name = if src == DECK { "deck \"Deck pad (hidraw)\"".to_string() } else { format!("{src} {:?}", gilrs.gamepads().find(|(id, _)| usize::from(*id) as Src == src).map(|(_, g)| g.name().to_string()).unwrap_or_default()) };
                    log(&format!("[pad] {name} {ev:?}"));
                }
                m.on(src, ev, swaps, Instant::now(), pointer, &mut out);
            }
            m.tick(Instant::now(), &mut out);
            for o in out.drain(..) {
                match o {
                    Out::Press(p, repeat) => send(&app, route.as_ref(), p, repeat),
                    Out::Release(p) => {
                        route(&app, p, false);
                    }
                    // Share chords work while a game has focus: evdev is read regardless, and the
                    // press never reaches `route`.
                    Out::Share(c) => crate::share::chord(&app, share(c)),
                    Out::Exit => {
                        let app = app.clone();
                        std::thread::spawn(move || crate::overlay::exit_chord(&app));
                    }
                    // The right stick goes to the UI as analog values (chart zoom and scroll).
                    Out::Stick(x, y) => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.emit("padstick", serde_json::json!({ "x": x, "y": y }));
                        }
                    }
                }
            }
            if dbg_at.elapsed() >= Duration::from_secs(3) {
                dbg_at = Instant::now();
                m.debug = debug_input();
            }
            // An app in pointer mode: sticks move the pointer and scroll, every pass.
            if crate::apps::pointer::active() {
                if let Some((_, g)) = gilrs.gamepads().find(|(_, g)| g.is_connected()) {
                    crate::apps::pointer::analog(&standard_state(&g).1);
                }
            }
            // The in-game overlay's hold-View timer.
            crate::overlay::tick(&app);
            // A web game on screen also gets the analog state (sticks, triggers), when it changes.
            if crate::webgame::visible() {
                let st = gilrs.gamepads().find(|(_, g)| g.is_connected()).map(|(_, g)| standard_state(&g));
                if let Some(st) = st.filter(|st| last_state.as_ref() != Some(st)) {
                    crate::webgame::pad_state(&app, &st.0, &st.1);
                    last_state = Some(st);
                }
            } else {
                last_state = None;
            }
            // Newest rumble wins; dropping an Effect stops it.
            let mut latest = None;
            while let Ok(r) = rx.try_recv() {
                latest = Some(r);
            }
            if let Some(r) = latest {
                playing = play_rumble(&mut gilrs, r);
            }
            if playing.as_ref().is_some_and(|(_, until)| Instant::now() >= *until) {
                playing = None;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gilrs_buttons_reach_the_state_machine() {
        assert_eq!(btn(Button::Mode), Some(Btn::Mode));
        assert_eq!(btn(Button::North), Some(Btn::North));
        assert_eq!(btn(Button::DPadLeft), Some(Btn::DPadLeft));
        assert_eq!(btn(Button::C), None);
        assert_eq!(btn(Button::Unknown), None);
    }

    #[test]
    fn share_chords_map_to_captures() {
        use crate::share::Chord;
        let c = |p| rare_input::chords::share_chord(p).map(share);
        assert_eq!(c(Pad::Y), Some(Chord::Screenshot));
        assert_eq!(c(Pad::Select), Some(Chord::Clip));
        assert_eq!(c(Pad::A), None);
    }
}
