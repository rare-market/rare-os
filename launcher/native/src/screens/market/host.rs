//! A Phase 4 page's host: its `View` (the El tree diffed into the scene), the focus engine (`Nav`)
//! and the kit's art fallbacks. A page is a view function over its state; `Host::render` builds,
//! diffs, lays out, registers focus and draws the focused look. Pads go through `step` (directions)
//! and `tag` (what A activates).

pub use crate::kit::ui::{s, Track, Ty, Ui, S, T};
pub use crate::kit::{self, tok, Arts};
pub use crate::screens::overlay::common::{Cx, Out, Screen};
use rare_render::chart::Textures;
use rare_render::focus::{Dir, FocusOpts, Nav};
use rare_render::input::Pad;
use rare_render::view::{El, View};
use rare_render::scene::SlotPaint;
use rare_render::{DrawList, Id, Scene, TextSys};

/// The element name of a bar that pins to the top of its scrolling page.
pub const PIN: &str = "pin-bar";

/// A Phase 4 page: a Screen that may paint in the scene's paint slots (the chart) or after it
/// (video), and says what the hint bar shows. The golden harness renders any of them.
pub trait Page: Screen {
    /// Video and other frames painted after the scene (Share's player).
    fn paint_extra(&mut self, _scene: &Scene, _ts: &mut TextSys, _tex: &mut dyn Textures, _out: &mut DrawList) {}
    /// Before the scene paints: makes the textures the page's paint slots need.
    fn prepare_paint(&mut self, _scene: &Scene, _ts: &mut TextSys, _tex: &mut dyn Textures) {}
    /// From the scene's slot callback: pushes what a paint slot shows (the chart).
    fn paint_slot(&mut self, _sp: &SlotPaint, _ts: &mut TextSys, _out: &mut DrawList) {}
    /// The hint bar's buttons (hints.ts names) and labels for where focus is.
    fn hints(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
    /// A front page's current section (Trade's root: Home 0, Perps 1, Traders 2, Positions 3).
    fn section(&self) -> Option<u8> {
        None
    }
    /// The page itself, for an owner that hands it live data (Trade's pages).
    fn any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

pub struct Host {
    pub view: View,
    pub nav: Nav,
    pub arts: Arts,
    pub rem: f32,
    pub w: f32,
    pub h: f32,
}

impl Host {
    /// A host whose page mounts under `parent` (the shell's main area).
    pub fn new(cx: &Cx, parent: Id) -> Host {
        Host { view: View::new(parent), nav: Nav::new(cx.scene.w, cx.scene.h), arts: Arts::default(), rem: cx.scene.rem, w: cx.scene.w, h: cx.scene.h }
    }

    /// Builds the page's tree, diffs it into the scene, lays out and puts focus where it belongs.
    pub fn render(&mut self, cx: &mut Cx, now: f64, build: impl FnOnce(&mut Ui) -> El) {
        let el = {
            let mut ui = Ui::new(self.rem, self.w, self.h, cx.img, &mut self.arts, now);
            build(&mut ui)
        };
        self.view.update(cx.scene, cx.t, el, now);
        self.settle(cx, now);
    }

    /// Layout, focus registration and the focused look, after the tree changed.
    pub fn settle(&mut self, cx: &mut Cx, now: f64) {
        cx.scene.layout(cx.t);
        self.view.after_layout(cx.scene, now);
        self.nav.begin();
        self.view.register(cx.scene, &mut self.nav, now);
        self.nav.ensure(now);
        if self.nav.current().is_none() {
            self.nav.focus_first(now);
        }
        self.view.set_focus(cx.scene, cx.t, self.nav.current(), now);
    }

    /// The focused element's tag.
    pub fn tag(&self) -> Option<u32> {
        self.nav.current_item().map(|f| f.tag)
    }

    /// Puts focus on the element tagged `tag`. True when there is one.
    pub fn focus_tag(&mut self, cx: &mut Cx, tag: u32, now: f64) -> bool {
        let Some(id) = self.nav.items.iter().find(|f| f.tag == tag && !f.disabled).map(|f| f.id) else { return false };
        self.nav.focus(id, FocusOpts { scroll: true, smooth: false }, now);
        self.view.set_focus(cx.scene, cx.t, self.nav.current(), now);
        true
    }

    /// Touch: focus on the element under window point (`x`, `y`) in the top scope (a sheet over
    /// the page), with no scrolling. True when one is there (the page then presses A or Menu).
    pub fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        // The rects as they are now: a finger may have scrolled the page since the last build.
        self.nav.begin();
        self.view.register(cx.scene, &mut self.nav, now);
        self.nav.stop_scrolls();
        let ids = self.nav.touchable();
        let Some(id) = cx.scene.hit_among(x, y, now, &ids) else { return false };
        self.nav.focus(id, FocusOpts { scroll: false, smooth: false }, now);
        self.view.set_focus(cx.scene, cx.t, self.nav.current(), now);
        true
    }

    /// A direction press: moves focus (and scrolls toward it). True when focus moved.
    pub fn step(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> bool {
        let dir = match pad {
            Pad::Up => Dir::Up,
            Pad::Down => Dir::Down,
            Pad::Left => Dir::Left,
            Pad::Right => Dir::Right,
            _ => return false,
        };
        let moved = self.nav.move_dir(dir, now);
        self.scroll(cx, now);
        self.view.set_focus(cx.scene, cx.t, self.nav.current(), now);
        moved
    }

    /// LT / RT: a section jump (or a page inside a long list).
    pub fn jump(&mut self, cx: &mut Cx, forward: bool, now: f64) -> bool {
        let moved = self.nav.jump(forward, now);
        self.scroll(cx, now);
        self.view.set_focus(cx.scene, cx.t, self.nav.current(), now);
        moved
    }

    fn scroll(&mut self, cx: &mut Cx, now: f64) {
        self.nav.tick(now);
        let sc = self.nav.take_scrolls();
        if !sc.is_empty() {
            self.view.apply_scrolls(cx.scene, &mut self.nav, &sc);
        }
    }

    /// Per frame: scroll glides, late focus, art failing over. True when art changed (rebuild).
    pub fn tick(&mut self, cx: &mut Cx, now: f64) -> bool {
        let art = self.arts.poll(cx.img);
        self.scroll(cx, now);
        if self.view.gc(cx.scene, now) {
            cx.scene.mark_dirty();
        }
        self.nav.ensure(now);
        self.view.set_focus(cx.scene, cx.t, self.nav.current(), now);
        self.pin(cx, now);
        art
    }

    /// A bar named `PIN` stays at the top of its scrolling page once the page scrolls past it
    /// (the collection's tabs once its header has gone), like `position: sticky; top: 0`.
    fn pin(&mut self, cx: &mut Cx, now: f64) {
        let Some(bar) = self.view.id(PIN) else { return };
        let Some(page) = cx.scene.parent(bar) else { return };
        let sy = cx.scene.n(page).scroll_y.get(now);
        let want = (sy - (cx.scene.rect(bar)[1] - cx.scene.rect(page)[1])).max(0.0);
        let n = cx.scene.n(bar);
        if (n.ty.target() - want).abs() > 0.01 || (n.ty.get(now) - want).abs() > 0.01 {
            n.ty.snap(want);
        }
    }

    /// The node of an element given a `name` in the last build.
    pub fn id(&self, name: &str) -> Option<Id> {
        self.view.id(name)
    }
}

/// `flex items-baseline`: text runs sharing a baseline (Inter's metrics, CSS half-leading), as a
/// row aligned at the bottom with each run lifted by its distance below the baseline.
pub fn baseline_row(ui: &Ui, st: S, runs: Vec<(T, String)>) -> El {
    const ASC: f32 = 0.96875;
    const DESC: f32 = 0.2421875;
    let below = |t: &T| {
        let (size, line, _) = t.ty.metrics();
        let (size, line) = (size * ui.rem, t.leading.map(|l| l * ui.rem).unwrap_or(line * ui.rem));
        let half = (line - (ASC + DESC) * size) / 2.0;
        line - (half + ASC * size)
    };
    let d: Vec<f32> = runs.iter().map(|(t, _)| below(t)).collect();
    let dmax = d.iter().copied().fold(0.0, f32::max);
    let mut row = ui.bx(st.row().items_end());
    for ((t, text), di) in runs.into_iter().zip(d) {
        if text.is_empty() {
            continue;
        }
        row = row.child(ui.text_s(s().mb(((dmax - di) / ui.rem * 100.0).round() / 100.0), text, t));
    }
    row
}

/// A live price: the first text run of `el` (a `baseline_row`) rolls its changed digits when
/// `value` moves (Num `live`).
pub fn roll_first(ui: &Ui, mut el: El, value: Option<f64>) -> El {
    if let (Some(v), Some(k)) = (value, el.kids.first_mut()) {
        *k = std::mem::take(k).roll(v, tok::POS, tok::NEG, ui.r(0.25));
    }
    el
}

/// One line of text centred in a full-width box (text-center; container alignment, so it needs no
/// laid-out text width).
pub fn center_text(ui: &Ui, st: S, text: impl Into<String>, t: T) -> El {
    ui.bx(st.row().justify_center()).child(ui.text(text, t))
}

/// One line of text at the end of its box (text-right).
pub fn right_text(ui: &Ui, st: S, text: impl Into<String>, t: T) -> El {
    ui.bx(st.row().justify_end()).child(ui.text(text, t))
}

/// A `[data-nav]` that is not a button or a tile: fills layer-04 and turns its text primary when
/// focused (app.css), with the ring.
pub fn fill_nav(el: El, tag: u32, hint: Option<&str>, x: Option<&str>, def: bool) -> El {
    let fl = el.look.clone().bg(tok::L4);
    kit::nav(el.focus_look(fl), tag, hint, x, def)
}

/// A stat for `stat_line`: its label and its value (text or a Num); None is left out.
pub enum Stat {
    Text(String),
    Num(crate::kit::NumKind, Option<f64>, crate::kit::NumOptions),
}

/// A stat line with Num values (cents dimmed, P/L coloured): label over value, a hairline
/// between items; items with nothing to show are left out.
pub fn stat_line(ui: &mut Ui, st: S, items: Vec<(&str, Stat)>, fill: bool, lg: bool) -> Option<El> {
    use crate::kit::format::num_parts;
    let shown: Vec<(&str, Stat)> = items
        .into_iter()
        .filter(|(_, v)| match v {
            Stat::Text(t) => !t.is_empty(),
            Stat::Num(k, n, o) => num_parts(*k, *n, *o).is_some(),
        })
        .collect();
    if shown.is_empty() {
        return None;
    }
    let n = shown.len();
    let mut row = ui.bx(st.row());
    for (i, (label, v)) in shown.into_iter().enumerate() {
        let mut cs = s().col().min0();
        if fill {
            cs = cs.flex(1.0);
        }
        if i > 0 {
            cs = cs.pl(1.0);
        }
        if i + 1 < n {
            cs = cs.pr(1.0);
        }
        let mut c = ui.bx(cs);
        if i > 0 {
            c = c.child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).w_px(1.0)).bg(tok::HAIR));
        }
        let t = T::new(if lg { Ty::Row } else { Ty::Body }).w(500).tab();
        let value = match v {
            Stat::Text(text) => Some(ui.text(text, t)),
            Stat::Num(k, n, o) => crate::kit::num(ui, s(), k, n, o, t, true),
        };
        c = c.child(ui.text(label, T::new(Ty::Label).c(tok::MUTED))).maybe(value);
        row = row.child(c);
    }
    Some(row)
}

/// `kit::sheet` re-rooted in flow (full size, the hint bar's 3rem kept clear) instead of absolute:
/// an absolute root left a stale out-of-flow entry that crashed the next layout once it closed
/// (a WS 1 request).
pub fn in_flow_sheet(ui: &Ui, el: El) -> El {
    el.with_style(s().full().row().items_center().justify_center().p(1.5).pb(4.5).to_style(ui.rem))
}

/// Marks an element's focusable as the page's default (`data-nav-default`), keeping its tag and hints.
pub fn default_nav(mut el: El) -> El {
    if let Some(f) = el.focus.as_mut() {
        f.default = true;
    }
    el
}

/// The first letter or digit of a name, upper case (MImg's fallback).
pub fn initial(label: &str) -> String {
    kit::market::initial(label)
}
