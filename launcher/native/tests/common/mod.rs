//! The headless app for focus tests: no window, no GPU. The app's scene is laid out and measured
//! where every motion lands (`Scene::window_rect` once the glides settle).
#![allow(dead_code)]

use rare_native::app::{App, Start0};
use rare_native::page::Btn;
use rare_native::state::Tab;
use rare_render::theme::rem_for;
use rare_render::{clock, Id, ImageLoader, Scene, TextSys};
use std::sync::Arc;
use std::time::{Duration, Instant};

static FONTS: [&[u8]; 5] = [
    include_bytes!("../../../render/assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../render/assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../render/assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../render/assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../../render/assets/fonts/FiraCode-Medium.ttf"),
];

/// The SuiPlay's 16:9 panel, scaled down, at the largest UI size (Settings > Display > UI size
/// "Larger", 1.3x): the tightest fit any list gets.
const W: f32 = 1280.0;
const H: f32 = 720.0;
const UI_SCALE: f32 = 1.3;

pub struct Harness {
    pub app: App,
}

impl Harness {
    pub fn new(tab: Option<Tab>, open: Option<&str>) -> Harness {
        std::env::set_var("RR_SILENT", "1");
        let text = TextSys::new(&FONTS, false, 2048);
        let images = ImageLoader::new(2, Arc::new(|| {}), None);
        let scene = Scene::new(W, H, rem_for(W, H) * UI_SCALE);
        let s0 = Start0 { tab, open: open.map(String::from), boot_hold: false, no_boot: true, fixture: true, running: vec![] };
        let backend: Arc<dyn rare_native::data::backend::Backend> = Arc::new(rare_native::data::backend::Mock::new(false));
        let mut h = Harness { app: App::new(scene, text, images, backend, Arc::new(|| {}), s0) };
        h.pump(400);
        h
    }

    /// Runs the app for `ms` of wall time (results land, views rebuild, glides move).
    pub fn pump(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        loop {
            self.app.update();
            if self.app.scene.needs_layout() {
                self.app.scene.layout(&mut self.app.text);
            }
            if Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    pub fn press(&mut self, b: Btn) {
        self.app.input(b, false);
        self.pump(40);
    }

    /// Lets every glide finish: until the focused boxes stop moving (at most half a second).
    pub fn settle(&mut self) {
        let t0 = Instant::now();
        let mut last: Vec<[f32; 4]> = Vec::new();
        let mut still = 0;
        while t0.elapsed() < Duration::from_millis(500) {
            self.pump(16);
            let t = clock::secs();
            let now: Vec<[f32; 4]> = self.focused().iter().map(|f| self.app.scene.window_rect(*f, t)).collect();
            still = if now == last && !self.app.nav.scrolling() { still + 1 } else { 0 };
            if still >= 3 {
                break;
            }
            last = now;
        }
    }

    /// The nodes that draw the focus ring now (every focus system in the app draws one).
    pub fn focused(&self) -> Vec<Id> {
        let s = &self.app.scene;
        let mut out = Vec::new();
        let mut stack = vec![s.root];
        while let Some(id) = stack.pop() {
            let Some(n) = s.try_get(id) else { continue };
            if n.hidden {
                continue;
            }
            if n.ring.target() > 0.5 {
                out.push(id);
            }
            stack.extend(s.children(id));
        }
        if out.is_empty() {
            out.extend(self.app.nav.current());
        }
        out
    }

    /// The first clipping box (or the window) the focused node pokes out of, if any.
    pub fn clipped(&self, id: Id) -> Option<String> {
        let s = &self.app.scene;
        let now = clock::secs();
        let r = s.window_rect(id, now);
        let inside = |b: [f32; 4]| r[0] >= b[0] - 1.5 && r[1] >= b[1] - 1.5 && r[0] + r[2] <= b[0] + b[2] + 1.5 && r[1] + r[3] <= b[1] + b[3] + 1.5;
        if !inside([0.0, 0.0, s.w, s.h]) {
            return Some(format!("focused {r:?} is outside the window"));
        }
        let mut p = s.parent(id);
        while let Some(a) = p {
            if let Some(n) = s.try_get(a) {
                if n.clip || n.scroll {
                    let b = s.window_rect(a, now);
                    if !inside(b) {
                        return Some(format!("focused {r:?} is cut off by {a:?} {b:?} (scroll {})", n.scroll));
                    }
                }
            }
            p = s.parent(a);
        }
        None
    }

    /// Presses `dir` until focus stops moving (at most `max` times), checking after every press
    /// that the focused node is on screen. Returns how many moves it made.
    pub fn walk(&mut self, name: &str, dir: Btn, max: usize, errs: &mut Vec<String>) -> usize {
        let mut moves = 0;
        let mut last = self.focused();
        for _ in 0..max {
            self.press(dir);
            self.settle();
            let now = self.focused();
            if std::env::var("FV_DEBUG").is_ok() {
                let t = clock::secs();
                eprintln!("  {name} {dir:?}: {:?}", now.iter().map(|f| self.app.scene.window_rect(*f, t)).collect::<Vec<_>>());
            }
            for f in &now {
                if let Some(e) = self.clipped(*f) {
                    errs.push(format!("{name}: after {} x {dir:?}: {e}", moves + 1));
                    return moves;
                }
            }
            if now == last {
                break;
            }
            moves += 1;
            last = now;
        }
        moves
    }
}


/// Fails with every surface whose focus left the visible area.
pub fn report(errs: Vec<String>) {
    if !errs.is_empty() {
        panic!("focus left the visible area:\n  {}", errs.join("\n  "));
    }
}

impl Harness {
    /// The text under the focused nodes (to see where a press sequence landed).
    pub fn focus_text(&self) -> String {
        fn walk(s: &Scene, id: Id, out: &mut String) {
            if let Some(n) = s.try_get(id) {
                if let Some(t) = n.text.as_ref() {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&t.text);
                }
            }
            for k in s.children(id) {
                walk(s, k, out);
            }
        }
        let mut out = String::new();
        for f in self.focused() {
            walk(&self.app.scene, f, &mut out);
            out.push_str(" | ");
        }
        out
    }

    /// Presses each button in turn, letting glides settle after each.
    pub fn seq(&mut self, bs: &[Btn]) {
        for b in bs {
            self.press(*b);
            self.settle();
        }
    }
}

impl Harness {
    /// Moves focus until the focused text contains `needle`: Down while it moves, then Right,
    /// then Up. True when found within `max` presses.
    pub fn seek(&mut self, needle: &str, max: usize) -> bool {
        let dirs = [Btn::Down, Btn::Right, Btn::Up, Btn::Left];
        let mut d = 0;
        for _ in 0..max {
            if self.focus_text().contains(needle) {
                return true;
            }
            let before = self.focused();
            self.press(dirs[d]);
            self.settle();
            if self.focused() == before {
                d = (d + 1) % dirs.len();
            }
        }
        self.focus_text().contains(needle)
    }
}

/// The app keeps process-wide state (the event bus, env-driven fixtures): one case at a time.
pub fn one() -> std::sync::MutexGuard<'static, ()> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

impl Harness {
    /// The focused boxes on screen now.
    pub fn focus_rects(&self) -> Vec<[f32; 4]> {
        let t = clock::secs();
        self.focused().iter().map(|f| self.app.scene.window_rect(*f, t)).collect()
    }

    /// `walk`, stopping (without an error) once focus leaves the area `keep` accepts (a pane's
    /// column): the walk covers one list, not the screen.
    pub fn walk_in(&mut self, name: &str, dir: Btn, max: usize, errs: &mut Vec<String>, keep: impl Fn([f32; 4]) -> bool) -> usize {
        let mut moves = 0;
        let mut last = self.focused();
        for _ in 0..max {
            self.press(dir);
            self.settle();
            let now = self.focused();
            if self.focus_rects().iter().any(|r| !keep(*r)) {
                break;
            }
            for f in &now {
                if let Some(e) = self.clipped(*f) {
                    errs.push(format!("{name}: after {} x {dir:?}: {e}", moves + 1));
                    return moves;
                }
            }
            if now == last {
                break;
            }
            moves += 1;
            last = now;
        }
        moves
    }
}
