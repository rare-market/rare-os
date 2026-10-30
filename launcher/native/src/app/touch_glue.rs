//! Touch and mouse everywhere (the SuiPlay's touchscreen, a trackpad or mouse on a desktop).
//!
//! `rare_render::pointer::Recognizer` turns contacts into gestures; this routes them the way a pad
//! press is routed (`App::input`): only the top layer receives a touch (the keyboard, the review
//! sheet, a scene-built surface such as the Rare menu, Share, the Options sheet, an overlay, then
//! the page), so nothing under a sheet ever reacts.
//!
//! - Tap: focus goes to the element under the finger, then A runs as a pad would (the screen's own
//!   handler first). A slider under the finger takes the value instead. The top bar's tabs, the
//!   Rare mark (the Rare menu) and the hint bar's buttons are tappable too.
//! - Long press: focus there, then Menu (the element's Options).
//! - Drag: the scroll container under the finger follows it, with kinetic momentum on release and
//!   rubber-band ends (`pointer::ScrollDrag`, `pointer::Fling`); a slider under it follows sideways.
//! - A swipe in from the left edge: B (Back). Pinch and two-finger moves do nothing.
//! - The focus ring hides while touching (`Scene::ring_hidden`) and is back with the next press.
//!
//! The SuiPlay's panel is portrait-native and turned; gamescope rotates touch with the panel, so
//! the window gets landscape points. `RR_TOUCH_ROTATE` (0, 90, 180, 270, or left / right) or
//! `~/.config/rare/touch-rotate` turns them further for a compositor that does not.

use super::*;
use crate::screens::overlay::{Out, Screen as _};
use rare_render::focus::FocusOpts;
use rare_render::pointer::{Config, Fling, Gesture, Recognizer, Rotation, ScrollDrag};

/// A contact's phase as the window reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// The mouse's finger id (the left button acts as one finger).
const MOUSE: u64 = u64::MAX;

/// What a drag moves.
enum Drag {
    Scroll(ScrollDrag),
    /// A slider in the app's views (`Page::slide`): the element and its rect.
    Slide { id: Id, rect: [f32; 4] },
    /// A slider in a scene-built screen (`Screen::touch_slide`): the surface or the tab page.
    SlideLegacy { page: bool },
    None,
}

/// Who receives a touch, in `App::input`'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Share,
    Legacy,
    LegacyPage,
    Views,
}

pub(super) struct Touch {
    rec: Recognizer,
    drag: Drag,
    fling: Option<Fling>,
    rotation: Rotation,
    mouse: (f32, f32),
    mouse_down: bool,
    /// The last contact from a touchscreen (pointer events the system makes from touch are ignored
    /// for a moment after it).
    last_touch: f64,
    /// Contacts logged so far (the first few are, to check the panel mapping on a device).
    logged: u32,
    /// This touch stopped a fling: it is not a tap.
    swallow: bool,
}

impl Touch {
    pub(super) fn new(rem: f32) -> Touch {
        let rotation = std::env::var("RR_TOUCH_ROTATE")
            .ok()
            .or_else(|| std::env::var_os("HOME").and_then(|h| std::fs::read_to_string(std::path::Path::new(&h).join(".config/rare/touch-rotate")).ok()))
            .and_then(|s| Rotation::parse(&s))
            .unwrap_or_default();
        Touch { rec: Recognizer::new(Config::for_rem(rem)), drag: Drag::None, fling: None, rotation, mouse: (0.0, 0.0), mouse_down: false, last_touch: -10.0, logged: 0, swallow: false }
    }
}

/// The hint bar's glyph as the button it stands for (tapping a hint presses it).
fn hint_btn(k: Key) -> Option<Btn> {
    Some(match k {
        Key::A => Btn::A,
        Key::B => Btn::B,
        Key::X => Btn::X,
        Key::Y => Btn::Y,
        Key::Lb => Btn::Lb,
        Key::Rb => Btn::Rb,
        Key::Lt => Btn::Lt,
        Key::Rt => Btn::Rt,
        Key::Start => Btn::Start,
        Key::Select => Btn::Select,
        Key::Quick | Key::Qam => Btn::Qam,
        Key::Rare | Key::Guide => Btn::Guide,
        _ => return None,
    })
}

impl App {
    /// A touchscreen contact at window pixels (`x`, `y`), as the window system reports it.
    pub fn touch(&mut self, phase: TouchPhase, id: u64, x: f32, y: f32) {
        let now = clock::secs();
        self.touch.last_touch = now;
        let (x, y) = self.touch.rotation.map_px(x, y, self.scene.w, self.scene.h);
        if phase == TouchPhase::Down && self.touch.logged < 40 {
            self.touch.logged += 1;
            log::info!("touch: down at {x:.0},{y:.0} (rotation {:?}, window {}x{})", self.touch.rotation, self.scene.w, self.scene.h);
        }
        self.contact(phase, id, x, y, now);
    }

    /// The mouse moved to window pixels (`x`, `y`).
    pub fn mouse_move(&mut self, x: f32, y: f32) {
        self.touch.mouse = (x, y);
        if self.touch.mouse_down {
            self.contact(TouchPhase::Move, MOUSE, x, y, clock::secs());
        }
    }

    /// The left mouse button (or a trackpad click): one finger.
    pub fn mouse_button(&mut self, down: bool) {
        let now = clock::secs();
        // Pointer events made from a touch: the touch already did it.
        if now - self.touch.last_touch < 0.8 || down == self.touch.mouse_down {
            return;
        }
        self.touch.mouse_down = down;
        let (x, y) = self.touch.mouse;
        self.contact(if down { TouchPhase::Down } else { TouchPhase::Up }, MOUSE, x, y, now);
    }

    /// A mouse wheel or a trackpad's two-finger scroll over the window: the scroller under the
    /// pointer moves (`pixels`: a trackpad's exact distance; otherwise wheel lines).
    pub fn wheel(&mut self, dx: f32, dy: f32, pixels: bool) {
        let now = clock::secs();
        let (k, (x, y)) = (if pixels { 1.0 } else { 3.0 * self.scene.rem }, self.touch.mouse);
        let (dx, dy) = (-dx * k, -dy * k);
        let horizontal = dx.abs() > dy.abs();
        let Some(node) = self.scene.scroller_at(x, y, now, horizontal).or_else(|| self.scene.scroller_at(x, y, now, !horizontal)) else { return };
        self.touch_mode(true);
        self.nav.stop_scrolls();
        if pixels {
            let i = (!horizontal) as usize;
            let (min, max) = (self.scene.min_scroll(node)[i], self.scene.max_scroll(node)[i]);
            let n = self.scene.n(node);
            let a = if horizontal { &mut n.scroll_x } else { &mut n.scroll_y };
            let v = (a.get(now) + if horizontal { dx } else { dy }).clamp(min, max.max(min));
            a.snap(v);
        } else {
            rare_render::scroll::by(&mut self.scene, node, if horizontal { dx } else { 0.0 }, if horizontal { 0.0 } else { dy }, now);
        }
        crate::features::services::touch();
    }

    /// True while a touch or its momentum needs frames.
    pub fn touch_busy(&self) -> bool {
        self.touch.fling.is_some() || self.touch.rec.active()
    }

    /// When the loop must wake for touch (a long press coming due).
    pub(super) fn touch_deadline(&self) -> Option<f64> {
        self.touch.rec.deadline()
    }

    /// Every frame: momentum and a long press that came due.
    pub(super) fn touch_frame(&mut self, now: f64) {
        if let Some(f) = self.touch.fling.as_mut() {
            if !f.step(&mut self.scene, now) {
                self.touch.fling = None;
            }
        }
        if let Some(g) = self.touch.rec.tick(now) {
            self.gesture(g, now);
        }
    }

    /// Touch shows no focus ring; the next pad press brings it back.
    fn touch_mode(&mut self, on: bool) {
        self.scene.ring_hidden = on;
    }

    /// A pad press ends touch mode: the ring is back and momentum stops where it is.
    pub(super) fn touch_pad(&mut self) {
        if self.scene.ring_hidden {
            self.touch_mode(false);
        }
        if let Some(f) = self.touch.fling.take() {
            f.stop(&mut self.scene, clock::secs());
        }
    }

    fn contact(&mut self, phase: TouchPhase, id: u64, x: f32, y: f32, now: f64) {
        self.animate = true;
        crate::features::services::touch();
        self.touch_mode(true);
        let gs = match phase {
            TouchPhase::Down => self.touch.rec.down(id, x, y, now),
            TouchPhase::Move => self.touch.rec.moved(id, x, y, now),
            TouchPhase::Up => self.touch.rec.up(id, x, y, now),
            TouchPhase::Cancel => self.touch.rec.cancel(id),
        };
        for g in gs {
            self.gesture(g, now);
        }
    }

    fn gesture(&mut self, g: Gesture, now: f64) {
        // The first touches of a run are logged with what they did (checking the panel mapping on a device).
        let log_on = std::env::var_os("RR_TOUCH_LOG").is_some() || self.touch.logged < 40;
        if log_on && !matches!(g, Gesture::Drag { .. }) {
            log::info!("touch: {g:?} for {:?}", self.target());
        }
        let tapped = matches!(g, Gesture::Tap { .. } | Gesture::LongPress { .. });
        let ended = matches!(g, Gesture::DragEnd { .. });
        self.gesture_inner(g, now);
        if log_on && tapped {
            let it = self.nav.current_item().map(|i| (i.scope, i.tag, i.label.clone()));
            log::info!("touch: focus {it:?}, tab {:?}, overlay {:?}, options {}, osk {}, legacy {}", self.st.tab, self.st.overlay, self.st.options.is_some(), self.st.osk.is_some(), self.legacy.is_some());
        }
        if log_on && ended {
            match &self.touch.fling {
                Some(f) => log::info!("touch: fling {:?} at {:.0} px/s", f.node, f.v),
                None => log::info!("touch: drag ended with no fling"),
            }
        }
    }

    fn gesture_inner(&mut self, g: Gesture, now: f64) {
        match g {
            Gesture::Press { .. } => {
                // A finger on moving content stops it, and that touch is not a tap on what was under it.
                self.touch.swallow = false;
                if let Some(f) = self.touch.fling.take() {
                    f.stop(&mut self.scene, now);
                    self.touch.swallow = true;
                }
            }
            Gesture::Tap { .. } | Gesture::LongPress { .. } if std::mem::take(&mut self.touch.swallow) => {}
            Gesture::Tap { x, y } => self.tap(x, y, false, now),
            Gesture::LongPress { x, y } => self.tap(x, y, true, now),
            Gesture::DragStart { x0, y0, x, y, horizontal } => self.drag_start(x0, y0, x, y, horizontal, now),
            Gesture::Drag { x, y } => self.drag_to(x, y, now),
            Gesture::DragEnd { vx, vy, .. } => {
                if let Drag::Scroll(d) = std::mem::replace(&mut self.touch.drag, Drag::None) {
                    self.touch.fling = d.release(&mut self.scene, if d.horizontal { vx } else { vy }, now);
                }
            }
            Gesture::Back => self.press(Btn::B),
            Gesture::Cancel => {
                if let Drag::Scroll(d) = std::mem::replace(&mut self.touch.drag, Drag::None) {
                    d.release(&mut self.scene, 0.0, now);
                }
            }
        }
    }

    /// A press from touch: as the pad's, without ending touch mode.
    fn press(&mut self, b: Btn) {
        self.input_inner(b, false);
    }

    fn target(&self) -> Target {
        let review_up = crate::screens::wallet::sign::host::active() && self.st.osk.is_none();
        let modal = self.st.osk.is_some() || crate::screens::wallet::sign::host::active();
        if self.share.is_some() && !review_up && self.legacy.is_none() && self.st.options.is_none() && self.st.osk.is_none() {
            return Target::Share;
        }
        if self.legacy.is_some() && !modal {
            return Target::Legacy;
        }
        if self.legacy_page.is_some() && !review_up && self.st.overlay.is_none() && self.st.options.is_none() && self.st.osk.is_none() {
            return Target::LegacyPage;
        }
        Target::Views
    }

    /// Focus on what is under (`x`, `y`) in a scene-built screen. False when nothing is there.
    fn legacy_focus(&mut self, t: Target, x: f32, y: f32, now: f64) -> bool {
        let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
        match t {
            Target::Legacy => self.legacy.as_mut().is_some_and(|(s, _)| s.touch_focus(&mut cx, x, y, now)),
            Target::LegacyPage => self.legacy_page.as_mut().is_some_and(|(_, s, _)| s.touch_focus(&mut cx, x, y, now)),
            Target::Share => self.share.as_mut().is_some_and(|s| s.page.touch_focus(&mut cx, x, y, now)),
            Target::Views => false,
        }
    }

    fn legacy_slide(&mut self, page: bool, x: f32, now: f64) -> Option<Out> {
        let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
        if page {
            self.legacy_page.as_mut().and_then(|(_, s, _)| s.touch_slide(&mut cx, x, now))
        } else {
            self.legacy.as_mut().and_then(|(s, _)| s.touch_slide(&mut cx, x, now))
        }
    }

    fn legacy_slid(&mut self, page: bool, out: Out) {
        if page {
            self.legacy_page_out(out);
        } else {
            self.legacy_out(out);
        }
    }

    /// The app's views: the focusable under (`x`, `y`) in the top scope, focused (no scroll).
    fn views_focus(&mut self, x: f32, y: f32, now: f64) -> Option<Id> {
        // Rects as they are now (a drag may have moved them since the last registration).
        self.settle(now);
        let ids = self.nav.touchable();
        let id = self.scene.hit_among(x, y, now, &ids)?;
        self.nav.focus(id, FocusOpts { scroll: false, smooth: false }, now);
        let cur = self.nav.current();
        for v in [&mut self.top.view, &mut self.page.view] {
            v.set_focus(&mut self.scene, &mut self.text, cur, now);
        }
        for slot in [self.overlay.as_mut(), self.options.as_mut(), self.review.as_mut(), self.osk.as_mut()].into_iter().flatten() {
            slot.view.set_focus(&mut self.scene, &mut self.text, cur, now);
        }
        Some(id)
    }

    /// Whether a slider of the app's views is under (`x`, `y`) (nothing is focused).
    fn views_slider_at(&mut self, x: f32, y: f32, now: f64) -> bool {
        self.settle(now);
        let ids = self.nav.touchable();
        let Some(it) = self.scene.hit_among(x, y, now, &ids).and_then(|id| self.nav.item(id).cloned()) else { return false };
        let act = self.slot_of(it.scope).and_then(|s| s.acts.get(it.tag).cloned());
        self.slot_of(it.scope).is_some_and(|s| s.page.is_slider((&it, act.as_ref())))
    }

    fn legacy_slider_at(&mut self, t: Target, x: f32, y: f32, now: f64) -> bool {
        let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
        match t {
            Target::Legacy => self.legacy.as_ref().is_some_and(|(s, _)| s.touch_is_slider(&mut cx, x, y, now)),
            Target::LegacyPage => self.legacy_page.as_ref().is_some_and(|(_, s, _)| s.touch_is_slider(&mut cx, x, y, now)),
            _ => false,
        }
    }

    /// `Page::slide` on the focused element of the app's views, at window `x`. False when it is not a slider.
    fn views_slide(&mut self, id: Id, rect: [f32; 4], x: f32) -> bool {
        let Some(it) = self.nav.item(id).cloned() else { return false };
        let inset = 0.95 * self.scene.rem;
        let frac = ((x - rect[0] - inset) / (rect[2] - 2.0 * inset).max(1.0)).clamp(0.0, 1.0);
        let act = self.slot_of(it.scope).and_then(|s| s.acts.get(it.tag).cloned());
        let (st, fx) = (&mut self.st, &mut self.fx);
        let slot = match it.scope {
            SCOPE_PAGE => Some(&mut self.page),
            SCOPE_OVERLAY => self.overlay.as_mut().or(Some(&mut self.page)),
            SCOPE_OPTIONS => self.options.as_mut(),
            SCOPE_OSK => self.osk.as_mut(),
            SCOPE_REVIEW => self.review.as_mut(),
            _ => self.overlay.as_mut().or(Some(&mut self.page)),
        };
        slot.is_some_and(|s| s.page.slide(st, fx, (&it, act.as_ref()), frac))
    }

    /// The chrome's tappable text: a top bar tab, the Rare mark, a hint bar button.
    /// Words under the finger in the top view (the page, or the overlay over it) that its screen
    /// takes as a tap (`Page::tap_text`: the Library's filter tabs), nearest first.
    fn text_tap(&mut self, x: f32, y: f32, now: f64) -> bool {
        if self.st.options.is_some() || self.st.osk.is_some() {
            return false;
        }
        // Wide enough sideways that a tab's count beside its label counts as the tab.
        let (px, py) = (1.6 * self.scene.rem, 0.9 * self.scene.rem);
        let root = match self.overlay.as_ref() {
            Some(o) => o.view.root_id(),
            None => self.page.view.root_id(),
        };
        let mut found: Vec<(f32, String)> = Vec::new();
        let mut stack: Vec<Id> = root.into_iter().collect();
        while let Some(id) = stack.pop() {
            if let Some(t) = self.scene.try_get(id).and_then(|n| n.text.as_ref()) {
                let b = self.scene.window_rect(id, now);
                if x >= b[0] - px && x <= b[0] + b[2] + px && y >= b[1] - py && y <= b[1] + b[3] + py {
                    found.push(((x - (b[0] + b[2] / 2.0)).abs() + (y - (b[1] + b[3] / 2.0)).abs(), t.text.clone()));
                }
            }
            stack.extend(self.scene.children(id));
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        let slot = match self.overlay.as_mut() {
            Some(o) => o,
            None => &mut self.page,
        };
        for (_, t) in found {
            if slot.page.tap_text(&mut self.st, &mut self.fx, &t) {
                self.after_input();
                return true;
            }
        }
        false
    }

    fn chrome_tap(&mut self, x: f32, y: f32, now: f64) -> bool {
        let r = self.scene.rem;
        // Text nodes under a view's root whose words `want` accepts, with their rects on screen.
        let labels_in = |scene: &Scene, root: Option<Id>, want: &dyn Fn(&str) -> bool| -> Vec<(String, [f32; 4])> {
            let mut out = Vec::new();
            let mut stack: Vec<Id> = root.into_iter().collect();
            while let Some(id) = stack.pop() {
                if let Some(t) = scene.try_get(id).and_then(|n| n.text.as_ref()) {
                    if want(&t.text) {
                        out.push((t.text.clone(), scene.window_rect(id, now)));
                    }
                }
                stack.extend(scene.children(id));
            }
            out
        };
        // The one whose span (its glyph `left` of it, `pad` either side) holds x, nearest first.
        let nearest = |labels: Vec<(String, [f32; 4])>, left: &dyn Fn(&str) -> f32, pad: f32| {
            labels.into_iter().filter(|(l, b)| x >= b[0] - left(l) - pad && x <= b[0] + b[2] + pad).min_by(|a, b| {
                let d = |q: &[f32; 4]| (x - (q[0] + q[2] / 2.0)).abs();
                d(&a.1).total_cmp(&d(&b.1))
            })
        };
        // The hint bar (the bottom 3rem): its buttons, glyph and label.
        if y > self.scene.h - 3.0 * r {
            let hints = self.hints.clone();
            let labels = labels_in(&self.scene, self.footer.view.root_id(), &|t| hints.iter().any(|h| h.label == t));
            if let Some((label, _)) = nearest(labels, &|_| 2.0 * r, 0.4 * r) {
                if let Some(b) = hints.iter().find(|h| h.label == label).and_then(|h| hint_btn(h.b)) {
                    self.press(b);
                    return true;
                }
            }
            return false;
        }
        // The top bar (4.25rem), while nothing is over the page: anywhere in its height, the tab
        // (or the Rare mark) whose label spans the finger's x, give or take half the gap.
        if y < 4.25 * r && self.st.overlay.is_none() && self.st.options.is_none() && self.st.osk.is_none() && !crate::screens::wallet::sign::host::active() && self.legacy.is_none() {
            let labels = labels_in(&self.scene, self.top.view.root_id(), &|t| TABS.iter().any(|(_, l)| *l == t) || t == "rare");
            let near = nearest(labels, &|l| if l == "rare" { 2.5 * r } else { 0.0 }, 0.65 * r);
            if let Some(label) = near.map(|n| n.0) {
                if label == "rare" {
                    self.press(Btn::Guide);
                } else if let Some((tab, _)) = TABS.iter().find(|(_, l)| *l == label) {
                    if *tab != self.st.tab {
                        self.st.close();
                        self.st.set_tab(*tab, false);
                        self.fx.fb(Feedback::Tab);
                    }
                }
                return true;
            }
        }
        false
    }

    fn tap(&mut self, x: f32, y: f32, long: bool, now: f64) {
        let b = if long { Btn::Start } else { Btn::A };
        let t = self.target();
        if t != Target::Views {
            if self.legacy_focus(t, x, y, now) {
                if !long && t != Target::Share {
                    if let Some(out) = self.legacy_slide(t == Target::LegacyPage, x, now) {
                        self.legacy_slid(t == Target::LegacyPage, out);
                        self.after_input();
                        return;
                    }
                }
                self.press(b);
                return;
            }
            // The tab page shares the window with the top bar and the hint bar.
            if t == Target::LegacyPage && self.views_tap(x, y, long, now) {
                return;
            }
            // A hint in the surface's footer (B Close, A Select, Y Search settings...): its button.
            if !long && t != Target::Share {
                use crate::screens::overlay::common::{hint_at, G};
                let b = hint_at(&self.scene, x, y, now).and_then(|g| {
                    Some(match g {
                        G::A => Btn::A,
                        G::B => Btn::B,
                        G::X => Btn::X,
                        G::Y => Btn::Y,
                        G::Lb => Btn::Lb,
                        G::Rb => Btn::Rb,
                        G::Lt => Btn::Lt,
                        G::Rt => Btn::Rt,
                        G::Start => Btn::Start,
                        G::Select => Btn::Select,
                        G::Rare => Btn::Guide,
                        G::Quick => Btn::Qam,
                    })
                });
                if let Some(b) = b {
                    self.press(b);
                    return;
                }
            }
            // Outside a side panel (the Rare menu): it closes.
            if t == Target::Legacy && !long {
                let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                if self.legacy.as_ref().is_some_and(|(s, _)| s.touch_outside(&mut cx, x, y, now)) {
                    self.press(Btn::B);
                }
            }
            return;
        }
        self.views_tap(x, y, long, now);
    }

    /// A tap for the app's views and the chrome. True when something took it.
    fn views_tap(&mut self, x: f32, y: f32, long: bool, now: f64) -> bool {
        if let Some(id) = self.views_focus(x, y, now) {
            if !long {
                let rect = self.nav.item(id).map(|i| i.rect).unwrap_or_default();
                if self.views_slide(id, rect, x) {
                    self.after_input();
                    return true;
                }
            }
            self.press(if long { Btn::Start } else { Btn::A });
            return true;
        }
        if !long && self.chrome_tap(x, y, now) {
            return true;
        }
        if !long && self.text_tap(x, y, now) {
            return true;
        }
        // On a sheet's dimmed backdrop (a window-sized fill in the sheet layers: Options, the
        // launcher's sheets): it closes, as B.
        if !long {
            if let Some(h) = self.scene.hit(x, y, now) {
                let r = self.scene.window_rect(h, now);
                let z = self.scene.layer_of(h);
                if (layer::SHEET..layer::TOAST).contains(&z) && r[2] >= self.scene.w - 1.0 && r[3] >= self.scene.h * 0.75 {
                    self.press(Btn::B);
                    return true;
                }
            }
        }
        // Outside Quick settings' sheet (27rem on the right): it closes, as on the web.
        if !long && self.st.overlay == Some(Overlay::Qam) && self.st.options.is_none() && self.st.osk.is_none() && x < self.scene.w - 27.0 * self.scene.rem {
            self.press(Btn::B);
            return true;
        }
        false
    }

    fn drag_start(&mut self, x0: f32, y0: f32, x: f32, y: f32, horizontal: bool, now: f64) {
        self.touch.drag = Drag::None;
        let t = self.target();
        // A slider under the finger, dragged sideways.
        if horizontal {
            if t == Target::Views {
                if self.views_slider_at(x0, y0, now) {
                    let Some(id) = self.views_focus(x0, y0, now) else { return };
                    let rect = self.nav.item(id).map(|i| i.rect).unwrap_or_default();
                    if self.views_slide(id, rect, x) {
                        self.touch.drag = Drag::Slide { id, rect };
                        self.after_input();
                        return;
                    }
                }
            } else if t != Target::Share && self.legacy_slider_at(t, x0, y0, now) && self.legacy_focus(t, x0, y0, now) {
                let page = t == Target::LegacyPage;
                if let Some(out) = self.legacy_slide(page, x, now) {
                    self.legacy_slid(page, out);
                    self.after_input();
                    self.touch.drag = Drag::SlideLegacy { page };
                    return;
                }
            }
        }
        let node = self.scene.scroller_at(x0, y0, now, horizontal);
        if let Some(node) = node {
            self.nav.stop_scrolls();
            let d = ScrollDrag::begin(&mut self.scene, node, horizontal, if horizontal { x0 } else { y0 }, now);
            d.update(&mut self.scene, if horizontal { x } else { y });
            self.touch.drag = Drag::Scroll(d);
        }
    }

    fn drag_to(&mut self, x: f32, y: f32, now: f64) {
        match self.touch.drag {
            Drag::Scroll(d) => d.update(&mut self.scene, if d.horizontal { x } else { y }),
            Drag::Slide { id, rect } => {
                if self.views_slide(id, rect, x) {
                    self.after_input();
                }
            }
            Drag::SlideLegacy { page } => {
                if let Some(out) = self.legacy_slide(page, x, now) {
                    self.legacy_slid(page, out);
                    self.after_input();
                }
            }
            Drag::None => {}
        }
    }
}

