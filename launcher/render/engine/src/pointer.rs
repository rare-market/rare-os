//! Touch and mouse for a controller-first UI: one finger at a time turned into a few gestures
//! (tap, long press, drag, a swipe in from the left edge), scrolling that follows the finger with
//! kinetic momentum and rubber-band edges, and the panel rotation a touchscreen may need.
//!
//! The app feeds `Recognizer` raw contacts in window pixels and acts on the `Gesture`s it gives
//! back (a tap focuses and activates what is under the finger, a long press opens its options, a
//! drag scrolls what is under it with `ScrollDrag`, a release hands over to `Fling`). A second
//! finger is never a gesture of its own: pinch and two-finger moves do nothing, and a second
//! finger on a tap that has not happened yet cancels it.

use crate::scene::{Id, Scene};

/// How a touchscreen's coordinates turn into the window's. Compositors (gamescope) rotate touch
/// with the panel themselves; this is for one that does not, or rotates the wrong way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    /// "0", "90", "180", "270" (also "normal", "left" = 90, "upsidedown", "right" = 270, as
    /// gamescope's --force-orientation names them).
    pub fn parse(s: &str) -> Option<Rotation> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "0" | "normal" | "none" | "" => Rotation::R0,
            "90" | "left" => Rotation::R90,
            "180" | "upsidedown" => Rotation::R180,
            "270" | "right" => Rotation::R270,
            _ => return None,
        })
    }

    /// Maps a contact at (`u`, `v`), normalised 0..1 over the touchscreen's own axes, to the
    /// window, normalised. The same table gamescope applies per panel orientation.
    pub fn map(self, u: f32, v: f32) -> (f32, f32) {
        match self {
            Rotation::R0 => (u, v),
            Rotation::R90 => (1.0 - v, u),
            Rotation::R180 => (1.0 - u, 1.0 - v),
            Rotation::R270 => (v, 1.0 - u),
        }
    }

    /// `map` in window pixels: a point the windowing system reported stretched over a `w` x `h`
    /// window from a touchscreen whose axes are turned.
    pub fn map_px(self, x: f32, y: f32, w: f32, h: f32) -> (f32, f32) {
        let (u, v) = self.map(x / w.max(1.0), y / h.max(1.0));
        (u * w, v * h)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// How far a finger moves before a touch is a drag, px.
    pub slop: f32,
    /// How long a still finger takes to become a long press, s.
    pub long_press: f64,
    /// A drag that starts this close to the left edge and goes right is Back, px.
    pub edge: f32,
    /// How far the edge swipe must travel, px (or a third of that when flicked).
    pub back: f32,
}

impl Config {
    /// For a scene laid out at `rem` px.
    pub fn for_rem(rem: f32) -> Config {
        Config { slop: 0.5 * rem, long_press: 0.5, edge: 2.0 * rem, back: 5.0 * rem }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Gesture {
    /// A finger came down at (x, y): nothing happens yet (feedback only).
    Press { x: f32, y: f32 },
    /// Up again without moving: focus and activate what is there.
    Tap { x: f32, y: f32 },
    /// Held still: the options of what is there. The release after it does nothing.
    LongPress { x: f32, y: f32 },
    /// Moved past the slop from (x0, y0); `horizontal` is the axis it moved along most.
    DragStart { x0: f32, y0: f32, x: f32, y: f32, horizontal: bool },
    /// Still dragging, now at (x, y).
    Drag { x: f32, y: f32 },
    /// Lifted after a drag, moving at (vx, vy) px/s.
    DragEnd { x: f32, y: f32, vx: f32, vy: f32 },
    /// A swipe in from the left edge: Back.
    Back,
    /// The touch stopped being one (a second finger, the system took it): undo any drag.
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum St {
    Idle,
    /// One finger down, still within the slop.
    Down { id: u64, x0: f32, y0: f32, t0: f64 },
    /// Past the slop. `edge`: a swipe in from the left edge (never scrolls).
    Drag { id: u64, x0: f32, y0: f32, edge: bool },
    /// The long press fired: the rest of this touch is ignored.
    Held { id: u64 },
    /// More than one finger: nothing until every finger is up.
    Blocked,
}

/// One finger at a time, turned into gestures.
pub struct Recognizer {
    pub cfg: Config,
    st: St,
    /// Fingers down now.
    down: Vec<u64>,
    /// Recent (t, x, y) of the tracked finger, for the release velocity.
    samples: Vec<(f64, f32, f32)>,
}

/// Velocity is read over this much of the end of a drag.
const VEL_WINDOW: f64 = 0.1;
/// A finger that stood still this long before lifting has no velocity.
const VEL_STALE: f64 = 0.06;

impl Recognizer {
    pub fn new(cfg: Config) -> Recognizer {
        Recognizer { cfg, st: St::Idle, down: Vec::new(), samples: Vec::new() }
    }

    /// A touch is under way (a frame loop keeps waking for the long press).
    pub fn active(&self) -> bool {
        !self.down.is_empty()
    }

    /// When `tick` must run next (the long press), if a finger is waiting for it.
    pub fn deadline(&self) -> Option<f64> {
        match self.st {
            St::Down { t0, .. } => Some(t0 + self.cfg.long_press),
            _ => None,
        }
    }

    /// The long press, once its time has come.
    pub fn tick(&mut self, t: f64) -> Option<Gesture> {
        if let St::Down { id, x0, y0, t0 } = self.st {
            if t >= t0 + self.cfg.long_press {
                self.st = St::Held { id };
                return Some(Gesture::LongPress { x: x0, y: y0 });
            }
        }
        None
    }

    fn sample(&mut self, t: f64, x: f32, y: f32) {
        self.samples.push((t, x, y));
        self.samples.retain(|s| t - s.0 <= VEL_WINDOW);
    }

    fn velocity(&self, t: f64) -> (f32, f32) {
        let (Some(a), Some(b)) = (self.samples.first(), self.samples.last()) else { return (0.0, 0.0) };
        if t - b.0 > VEL_STALE || b.0 - a.0 < 0.008 {
            return (0.0, 0.0);
        }
        let dt = (b.0 - a.0) as f32;
        ((b.1 - a.1) / dt, (b.2 - a.2) / dt)
    }

    pub fn down(&mut self, id: u64, x: f32, y: f32, t: f64) -> Vec<Gesture> {
        if !self.down.contains(&id) {
            self.down.push(id);
        }
        if self.down.len() > 1 {
            // A second finger: a tap or long press still pending is off; a drag keeps its finger.
            return match self.st {
                St::Down { .. } => {
                    self.st = St::Blocked;
                    vec![Gesture::Cancel]
                }
                St::Idle => {
                    self.st = St::Blocked;
                    vec![]
                }
                _ => vec![],
            };
        }
        self.samples.clear();
        self.sample(t, x, y);
        self.st = St::Down { id, x0: x, y0: y, t0: t };
        vec![Gesture::Press { x, y }]
    }

    pub fn moved(&mut self, id: u64, x: f32, y: f32, t: f64) -> Vec<Gesture> {
        match self.st {
            St::Down { id: d, x0, y0, t0 } if d == id => {
                if t >= t0 + self.cfg.long_press {
                    self.st = St::Held { id };
                    return vec![Gesture::LongPress { x: x0, y: y0 }];
                }
                let (dx, dy) = (x - x0, y - y0);
                if dx.hypot(dy) < self.cfg.slop {
                    return vec![];
                }
                let horizontal = dx.abs() > dy.abs();
                let edge = x0 <= self.cfg.edge && horizontal && dx > 0.0;
                self.sample(t, x, y);
                self.st = St::Drag { id, x0, y0, edge };
                if edge {
                    return vec![];
                }
                vec![Gesture::DragStart { x0, y0, x, y, horizontal }, Gesture::Drag { x, y }]
            }
            St::Drag { id: d, edge, .. } if d == id => {
                self.sample(t, x, y);
                if edge {
                    return vec![];
                }
                vec![Gesture::Drag { x, y }]
            }
            _ => vec![],
        }
    }

    pub fn up(&mut self, id: u64, x: f32, y: f32, t: f64) -> Vec<Gesture> {
        self.down.retain(|&d| d != id);
        let out = match self.st {
            St::Down { id: d, x0, y0, t0 } if d == id => {
                if t >= t0 + self.cfg.long_press {
                    vec![Gesture::LongPress { x: x0, y: y0 }]
                } else {
                    vec![Gesture::Tap { x: x0, y: y0 }]
                }
            }
            St::Drag { id: d, x0, edge, .. } if d == id => {
                self.sample(t, x, y);
                let (vx, vy) = self.velocity(t);
                if edge {
                    let dx = x - x0;
                    if dx >= self.cfg.back || (dx >= self.cfg.back / 3.0 && vx > 600.0) {
                        vec![Gesture::Back]
                    } else {
                        vec![]
                    }
                } else {
                    vec![Gesture::DragEnd { x, y, vx, vy }]
                }
            }
            _ => vec![],
        };
        self.release_state(id);
        out
    }

    /// The system took the touch (a compositor gesture, the window lost it).
    pub fn cancel(&mut self, id: u64) -> Vec<Gesture> {
        self.down.retain(|&d| d != id);
        let out = if self.tracks(id) && !matches!(self.st, St::Held { .. }) { vec![Gesture::Cancel] } else { vec![] };
        self.release_state(id);
        out
    }

    fn tracks(&self, id: u64) -> bool {
        matches!(self.st, St::Down { id: d, .. } | St::Drag { id: d, .. } | St::Held { id: d } if d == id)
    }

    /// After finger `id` left: idle once no finger is down; ignore the others while some still are.
    fn release_state(&mut self, id: u64) {
        if self.down.is_empty() {
            self.st = St::Idle;
        } else if self.tracks(id) {
            self.st = St::Blocked;
        }
    }
}

// ---------------------------------------------------------------- scrolling under a finger

/// Momentum time constant: a flick glides about this long times its speed (UIScrollView's
/// normal deceleration is close to 0.33 s).
pub const FLING_TAU: f32 = 0.33;
/// Fastest fling, px/s.
pub const FLING_MAX: f32 = 6000.0;

/// How far content follows a finger pulling `over` px past its end in a viewport `dim` px long:
/// less and less, never more than the viewport (UIScrollView's rubber band).
pub fn rubber(over: f32, dim: f32) -> f32 {
    let dim = dim.max(1.0);
    (1.0 - 1.0 / (over.max(0.0) * 0.55 / dim + 1.0)) * dim
}

/// A drag that moves a scroll container with the finger.
#[derive(Clone, Copy, Debug)]
pub struct ScrollDrag {
    pub node: Id,
    pub horizontal: bool,
    start: f32,
    grab: f32,
    min: f32,
    max: f32,
    dim: f32,
}

impl ScrollDrag {
    fn axis(scene: &Scene, node: Id, horizontal: bool) -> (f32, f32, f32) {
        let i = !horizontal as usize;
        let r = scene.rect(node);
        (scene.min_scroll(node)[i], scene.max_scroll(node)[i].max(scene.min_scroll(node)[i]), if horizontal { r[2] } else { r[3] })
    }

    /// Grabs `node` with the finger at `finger` (x for a sideways scroller, y otherwise): whatever
    /// moved it stops where it is now.
    pub fn begin(scene: &mut Scene, node: Id, horizontal: bool, finger: f32, now: f64) -> ScrollDrag {
        let (min, max, dim) = Self::axis(scene, node, horizontal);
        let n = scene.n(node);
        let a = if horizontal { &mut n.scroll_x } else { &mut n.scroll_y };
        let start = a.get(now);
        a.snap(start);
        ScrollDrag { node, horizontal, start, grab: finger, min, max, dim }
    }

    /// The offset for a finger at `finger`: 1:1 inside the content, rubber-banded past its ends.
    pub fn offset_for(&self, finger: f32) -> f32 {
        let raw = self.start - (finger - self.grab);
        if raw < self.min {
            self.min - rubber(self.min - raw, self.dim)
        } else if raw > self.max {
            self.max + rubber(raw - self.max, self.dim)
        } else {
            raw
        }
    }

    pub fn update(&self, scene: &mut Scene, finger: f32) {
        let o = self.offset_for(finger);
        let n = scene.n(self.node);
        if self.horizontal {
            n.scroll_x.snap(o);
        } else {
            n.scroll_y.snap(o);
        }
    }

    /// The finger lifted, moving at `v` px/s along the axis: a fling, or a spring back from past an
    /// end (then the scene animates it and None comes back).
    pub fn release(self, scene: &mut Scene, v: f32, now: f64) -> Option<Fling> {
        let n = scene.n(self.node);
        let a = if self.horizontal { &mut n.scroll_x } else { &mut n.scroll_y };
        let x = a.get(now);
        let vel = (-v).clamp(-FLING_MAX, FLING_MAX);
        if x < self.min - 0.5 || x > self.max + 0.5 {
            let edge = x.clamp(self.min, self.max);
            a.spring_from(x, vel * 0.5, edge, now);
            return None;
        }
        if vel.abs() < 40.0 {
            return None;
        }
        Some(Fling { node: self.node, horizontal: self.horizontal, v: vel, last: now, min: self.min, max: self.max })
    }
}

/// Momentum after a flick: the offset slows with `FLING_TAU`, and bounces off an end it reaches.
#[derive(Clone, Copy, Debug)]
pub struct Fling {
    pub node: Id,
    pub horizontal: bool,
    /// Offset units per second.
    pub v: f32,
    last: f64,
    min: f32,
    max: f32,
}

impl Fling {
    /// Advances to `now`; false once it is done (stopped, or bouncing on the scene's own spring).
    pub fn step(&mut self, scene: &mut Scene, now: f64) -> bool {
        if !scene.alive(self.node) {
            return false;
        }
        let dt = ((now - self.last) as f32).clamp(0.0, 0.05);
        self.last = now;
        let n = scene.n(self.node);
        let a = if self.horizontal { &mut n.scroll_x } else { &mut n.scroll_y };
        let x = a.get(now);
        let k = (-dt / FLING_TAU).exp();
        let nx = x + self.v * FLING_TAU * (1.0 - k);
        self.v *= k;
        if nx < self.min || nx > self.max {
            // Past an end: the rest of the speed goes into a short bounce back to it.
            let edge = nx.clamp(self.min, self.max);
            a.spring_from(edge, self.v, edge, now);
            return false;
        }
        a.snap(nx);
        self.v.abs() > 8.0
    }

    /// Stops it where it is (a finger came down on it).
    pub fn stop(&self, scene: &mut Scene, now: f64) {
        if !scene.alive(self.node) {
            return;
        }
        let n = scene.n(self.node);
        let a = if self.horizontal { &mut n.scroll_x } else { &mut n.scroll_y };
        let x = a.get(now);
        a.snap(x.clamp(self.min, self.max));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> Recognizer {
        Recognizer::new(Config { slop: 12.0, long_press: 0.5, edge: 48.0, back: 120.0 })
    }

    #[test]
    fn rotations_match_gamescope() {
        // The SuiPlay's portrait-native panel turned right (270): its top edge is the window's left.
        assert_eq!(Rotation::R270.map(0.0, 0.0), (0.0, 1.0));
        assert_eq!(Rotation::R270.map(1.0, 0.0), (0.0, 0.0));
        assert_eq!(Rotation::R270.map(0.0, 1.0), (1.0, 1.0));
        assert_eq!(Rotation::R90.map(0.0, 0.0), (1.0, 0.0));
        assert_eq!(Rotation::R180.map(0.25, 0.75), (0.75, 0.25));
        assert_eq!(Rotation::R0.map(0.3, 0.6), (0.3, 0.6));
        assert_eq!(Rotation::parse("right"), Some(Rotation::R270));
        assert_eq!(Rotation::parse("left"), Some(Rotation::R90));
        assert_eq!(Rotation::parse("x"), None);
        // In pixels on a 1920x1200 window.
        let (x, y) = Rotation::R270.map_px(480.0, 300.0, 1920.0, 1200.0);
        assert!((x - 300.0 * 1.6).abs() < 0.01 && (y - (1.0 - 0.25) * 1200.0).abs() < 0.01, "{x} {y}");
        // Each rotation undone by its opposite.
        for (r, back) in [(Rotation::R90, Rotation::R270), (Rotation::R180, Rotation::R180)] {
            let (u, v) = r.map(0.2, 0.7);
            let (a, b) = back.map(u, v);
            assert!((a - 0.2).abs() < 1e-6 && (b - 0.7).abs() < 1e-6);
        }
    }

    #[test]
    fn tap_long_press_and_drag() {
        let mut r = rec();
        assert_eq!(r.down(1, 100.0, 100.0, 0.0), vec![Gesture::Press { x: 100.0, y: 100.0 }]);
        assert!(r.moved(1, 104.0, 103.0, 0.05).is_empty(), "jitter inside the slop");
        assert_eq!(r.up(1, 104.0, 103.0, 0.1), vec![Gesture::Tap { x: 100.0, y: 100.0 }]);
        assert!(!r.active());

        r.down(1, 100.0, 100.0, 1.0);
        assert_eq!(r.deadline(), Some(1.5));
        assert_eq!(r.tick(1.6), Some(Gesture::LongPress { x: 100.0, y: 100.0 }));
        assert!(r.up(1, 100.0, 100.0, 2.0).is_empty(), "no tap after a long press");

        r.down(1, 500.0, 600.0, 3.0);
        let g = r.moved(1, 500.0, 560.0, 3.02);
        assert_eq!(g[0], Gesture::DragStart { x0: 500.0, y0: 600.0, x: 500.0, y: 560.0, horizontal: false });
        r.moved(1, 500.0, 500.0, 3.04);
        match r.up(1, 500.0, 440.0, 3.06).as_slice() {
            [Gesture::DragEnd { vy, .. }] => assert!(*vy < -2000.0, "{vy}"),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn a_still_finger_before_lifting_has_no_speed() {
        let mut r = rec();
        r.down(1, 500.0, 600.0, 0.0);
        r.moved(1, 500.0, 500.0, 0.05);
        r.moved(1, 500.0, 400.0, 0.1);
        match r.up(1, 500.0, 400.0, 0.3).as_slice() {
            [Gesture::DragEnd { vy, .. }] => assert_eq!(*vy, 0.0),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn left_edge_swipe_is_back() {
        let mut r = rec();
        r.down(1, 10.0, 500.0, 0.0);
        assert!(r.moved(1, 60.0, 505.0, 0.05).is_empty(), "an edge swipe never scrolls");
        r.moved(1, 120.0, 505.0, 0.1);
        assert_eq!(r.up(1, 200.0, 505.0, 0.15), vec![Gesture::Back]);
        // Too short and slow: nothing.
        r.down(1, 10.0, 500.0, 1.0);
        r.moved(1, 40.0, 500.0, 1.2);
        assert!(r.up(1, 40.0, 500.0, 1.6).is_empty());
        // Away from the edge, a sideways drag is a drag.
        r.down(1, 300.0, 500.0, 2.0);
        assert!(matches!(r.moved(1, 400.0, 500.0, 2.05)[0], Gesture::DragStart { horizontal: true, .. }));
    }

    #[test]
    fn a_second_finger_cancels_a_tap_and_pinch_does_nothing() {
        let mut r = rec();
        r.down(1, 100.0, 100.0, 0.0);
        assert_eq!(r.down(2, 300.0, 300.0, 0.05), vec![Gesture::Cancel]);
        assert!(r.moved(1, 50.0, 50.0, 0.1).is_empty());
        assert!(r.moved(2, 400.0, 400.0, 0.1).is_empty());
        assert!(r.up(2, 400.0, 400.0, 0.2).is_empty());
        assert!(r.up(1, 50.0, 50.0, 0.2).is_empty());
        assert!(!r.active());
        // A drag keeps going with its own finger.
        r.down(1, 100.0, 600.0, 1.0);
        r.moved(1, 100.0, 500.0, 1.05);
        assert!(r.down(2, 300.0, 300.0, 1.06).is_empty());
        assert_eq!(r.moved(1, 100.0, 450.0, 1.08), vec![Gesture::Drag { x: 100.0, y: 450.0 }]);
        assert!(r.moved(2, 330.0, 330.0, 1.08).is_empty());
        assert!(matches!(r.up(1, 100.0, 450.0, 1.1).as_slice(), [Gesture::DragEnd { .. }]));
        r.up(2, 330.0, 330.0, 1.2);
        assert_eq!(r.down(1, 10.0, 10.0, 2.0), vec![Gesture::Press { x: 10.0, y: 10.0 }]);
    }

    #[test]
    fn rubber_band_gives_less_and_less() {
        assert_eq!(rubber(0.0, 800.0), 0.0);
        let a = rubber(100.0, 800.0);
        let b = rubber(200.0, 800.0);
        assert!(a < 100.0 && b < 200.0 && b - a < a, "{a} {b}");
        assert!(rubber(1e6, 800.0) < 800.0);
    }

    #[test]
    fn drag_follows_then_flings_and_stops_at_the_end() {
        use crate::scene::Node;
        use taffy::prelude::*;
        let mut s = Scene::new(800.0, 600.0, 16.0);
        let root = s.root;
        let list = s.add(root, Style { size: Size { width: length(800.0), height: length(600.0) }, flex_direction: FlexDirection::Column, ..Default::default() }, Node::default().scrolls());
        for _ in 0..30 {
            s.add(list, Style { size: Size { width: length(800.0), height: length(100.0) }, flex_shrink: 0.0, ..Default::default() }, Node::bg(crate::Color::WHITE));
        }
        let mut ts = crate::TextSys::new(&[], false, 256);
        s.layout(&mut ts);
        assert_eq!(s.scroller_at(400.0, 300.0, 0.0, false), Some(list));
        assert_eq!(s.scroller_at(400.0, 300.0, 0.0, true), None, "no sideways scroll there");
        let max = s.max_scroll(list)[1];
        assert!(max > 2000.0);
        let d = ScrollDrag::begin(&mut s, list, false, 500.0, 0.0);
        d.update(&mut s, 300.0);
        assert_eq!(s.get(list).scroll_y.get(0.0), 200.0, "1:1 with the finger");
        d.update(&mut s, 900.0);
        let o = s.get(list).scroll_y.get(0.0);
        assert!(o < 0.0 && o > -400.0, "rubber band at the top: {o}");
        d.update(&mut s, 300.0);
        let mut f = d.release(&mut s, -3000.0, 0.0).expect("a fling");
        let mut t = 0.0;
        while f.step(&mut s, t) && t < 5.0 {
            t += 1.0 / 60.0;
        }
        let end = s.get(list).scroll_y.get(t);
        assert!((end - (200.0 + 3000.0 * FLING_TAU)).abs() < 40.0, "glides about v * tau: {end}");
        // A big flick bounces at the end and comes to rest on it.
        let d = ScrollDrag::begin(&mut s, list, false, 500.0, t);
        let mut f = d.release(&mut s, -6000.0 * 3.0, t).unwrap();
        let mut steps = 0;
        while f.step(&mut s, t) {
            t += 1.0 / 60.0;
            steps += 1;
            assert!(steps < 600);
        }
        let later = t + 3.0;
        assert!((s.get(list).scroll_y.get(later) - max).abs() < 0.6, "rests on the end");
        // Released past an end: springs back.
        let d = ScrollDrag::begin(&mut s, list, false, 0.0, later);
        d.update(&mut s, -400.0);
        assert!(s.get(list).scroll_y.get(later) > max);
        assert!(d.release(&mut s, 0.0, later).is_none());
        assert!((s.get(list).scroll_y.get(later + 2.0) - max).abs() < 0.6);
    }
}
