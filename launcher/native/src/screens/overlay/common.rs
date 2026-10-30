//! Interim building blocks for the Phase 2 screens, copied from launcher/src/ui/controls.tsx and the
//! focus rules in styles/app.css. They move onto the kit (launcher/native/src/kit) as it lands; the
//! screens only use the names here, so that is a swap of this one file.

use super::lay::s;
use rare_render::focus::{Dir, FocusMap};
use rare_render::input::Pad;
use rare_render::theme::{motion, size, tok};
use rare_render::{Color, Fit, Id, ImageLoader, ImgReq, Node, Scene, Src, TextStyle, TextSys};

/// Everything a screen needs to build and edit its scene.
pub struct Cx<'a> {
    pub scene: &'a mut Scene,
    pub t: &'a mut TextSys,
    pub img: &'a mut ImageLoader,
}

impl Cx<'_> {
    pub fn r(&self) -> f32 {
        self.scene.rem
    }
}

/// What a screen reports back after an input.
#[derive(Debug, PartialEq, Clone)]
pub enum Out {
    None,
    Moved,
    /// A closed the screen or picked an action; the string names it (the app maps it to a command).
    Act(String),
    /// B (or the Rare button) closed the screen.
    Close,
}

pub trait Screen {
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out;
    fn tick(&mut self, _cx: &mut Cx, _now: f64) {}
    fn next_deadline(&self) -> Option<f64> {
        None
    }
    /// The on-screen keyboard's answer to a question this screen asked through an `Out::Act` the
    /// app turns into a keyboard (`app::osk_for_act`): Some(text) when finished, None when
    /// cancelled. `key` is that act string.
    fn answer(&mut self, _cx: &mut Cx, _key: &str, _value: Option<String>, _now: f64) -> Out {
        Out::None
    }
    /// Something the app has for the screen: text a keyboard returned, a refresh after a reading.
    fn command(&mut self, _cx: &mut Cx, _cmd: &str, _arg: &str, _now: f64) {}
    /// The screen closed itself (a sheet that finished with nothing behind it): the app drops it.
    fn finished(&self) -> bool {
        false
    }
    /// An error sentence the screen wants toasted, once.
    fn take_message(&mut self) -> Option<String> {
        None
    }
    /// The node that has focus (the journey probe's focus line).
    fn focus_id(&self) -> Option<Id> {
        None
    }
    /// A live line under the keyboard's field for `key` while `value` is typed (askText's status).
    fn osk_status(&mut self, _key: &str, _value: &str) -> Option<String> {
        None
    }
    /// The screen's own hint bar (replaces the app's), or None.
    fn bar(&self) -> Option<Vec<rare_render::focus::hints::Hint>> {
        None
    }
    /// Touch: puts focus on what is under window point (`x`, `y`) in this screen's top layer (its
    /// sheet over its page). True when something took focus: the app then presses A for a tap or
    /// Menu for a long press, as a pad would.
    fn touch_focus(&mut self, _cx: &mut Cx, _x: f32, _y: f32, _now: f64) -> bool {
        false
    }
    /// Touch: whether a slider is under (`x`, `y`) (a sideways drag there moves it, not the page).
    fn touch_is_slider(&self, _cx: &mut Cx, _x: f32, _y: f32, _now: f64) -> bool {
        false
    }
    /// Touch: true when (`x`, `y`) is outside the screen's panel (a tap there closes it, as B).
    fn touch_outside(&self, _cx: &mut Cx, _x: f32, _y: f32, _now: f64) -> bool {
        false
    }
    /// Touch: a finger on the focused slider at window `x` (a tap on it or a drag along it). Some
    /// when it is a slider: the value follows the finger and this is what it changed.
    fn touch_slide(&mut self, _cx: &mut Cx, _x: f32, _now: f64) -> Option<Out> {
        None
    }
}

pub fn ts(rem: f32, size_rem: f32, line_rem: f32, weight: u16) -> TextStyle {
    TextStyle::new((size_rem * rem).round(), weight).line((line_rem * rem).round())
}

/// Type scale rows from DESIGN.md: (size, line height) in rem.
pub mod ty {
    pub const LABEL: (f32, f32) = (0.75, 1.0);
    pub const META: (f32, f32) = (0.8125, 1.125);
    pub const SMALL: (f32, f32) = (0.875, 1.25);
    pub const BODY: (f32, f32) = (0.9375, 1.375);
    pub const ROW: (f32, f32) = (1.0625, 1.5);
    pub const TITLE: (f32, f32) = (1.375, 1.75);
}

pub fn text(cx: &mut Cx, parent: Id, t: (f32, f32), weight: u16, color: Color, s_: &str) -> Id {
    let r = cx.r();
    cx.scene.add_text(cx.t, parent, s().done(), s_, ts(r, t.0, t.1, weight), color)
}

pub fn text_tr(cx: &mut Cx, parent: Id, t: (f32, f32), weight: u16, color: Color, s_: &str) -> Id {
    let r = cx.r();
    cx.scene.add_text(cx.t, parent, s().shrink().done(), s_, ts(r, t.0, t.1, weight).truncate(), color)
}

pub fn icon(cx: &mut Cx, parent: Id, name: &str, svg: &str, rem: f32, tint: Color) -> Id {
    let r = cx.r();
    let k = cx.img.request(svg_req(name, svg, rem * r));
    cx.scene.add(parent, s().wh(rem * r, rem * r).done(), Node::default().icon(k, tint))
}

/// A player's picture, round, `px` square: `url` is their picture (None: their default by
/// address). A Rare default is drawn from its SVG; an NFT comes through rare.market's image path
/// at this size (`data::art::single`, as the menu's rows), on layer 03 while it loads. None when
/// the picture cannot be fetched.
pub fn player_avatar(cx: &mut Cx, parent: Id, address: &str, url: Option<&str>, px: f32) -> Option<Id> {
    let def = match url {
        Some(u) => crate::avatars::id_of_src(u),
        None => Some(crate::avatars::starter(address)),
    };
    if let Some(d) = def.and_then(crate::avatars::get) {
        let k = cx.img.request(svg_req(&format!("avatar-{}", d.id), d.svg, px));
        return Some(cx.scene.add(parent, s().wh(px, px).no_shrink_().done(), Node::default().image(k).radius(px / 2.0).clip()));
    }
    let src = crate::data::art::single(url?, px)?;
    let p = px.round().max(1.0) as u32;
    let k = cx.img.request(ImgReq { src, w: p, h: p, fit: Fit::Cover { fy: 0.5 } });
    let wrap = cx.scene.add(parent, s().wh(px, px).no_shrink_().done(), Node::bg(tok::L3).radius(px / 2.0).clip());
    cx.scene.add(wrap, s().fill().done(), Node::default().image(k));
    Some(wrap)
}

pub fn svg_req(name: &str, svg: &str, px: f32) -> ImgReq {
    let name: &'static str = Box::leak(format!("{name}.svg").into_boxed_str());
    let bytes: &'static [u8] = Box::leak(svg.as_bytes().to_vec().into_boxed_slice());
    let p = px.round().max(1.0) as u32;
    ImgReq { src: Src::Static(name, bytes), w: p, h: p, fit: Fit::Contain }
}

/// 1 px rule (border-l/t/b border-hair). `side`: 0 top, 1 right, 2 bottom, 3 left.
pub fn edge(cx: &mut Cx, parent: Id, side: u8, color: Color) -> Id {
    let st = match side {
        0 => s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h(1.0),
        1 => s().abs(Some(0.0), Some(0.0), Some(0.0), None).w(1.0),
        2 => s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h(1.0),
        _ => s().abs(Some(0.0), None, Some(0.0), Some(0.0)).w(1.0),
    };
    cx.scene.add(parent, st.done(), Node::bg(color))
}

// ---- Glyphs (controls.tsx Glyph) ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum G {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    Start,
    Select,
    Rare,
    Quick,
}

const GEM: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M6 1.2 10.2 4.6 6 10.8 1.8 4.6Z" fill="#F2F2F3"/><path d="M1.8 4.6h8.4M4.2 4.6 6 1.2l1.8 3.4" fill="none" stroke="#100C18" stroke-width="0.7" stroke-linejoin="round"/></svg>"##;
const DOTS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><circle cx="2.4" cy="6" r="1.05" fill="#F2F2F3"/><circle cx="6" cy="6" r="1.05" fill="#F2F2F3"/><circle cx="9.6" cy="6" r="1.05" fill="#F2F2F3"/></svg>"##;
const START: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M2 3h8M2 6h8M2 9h8" stroke="#F2F2F3" stroke-width="1.6" stroke-linecap="round" fill="none"/></svg>"##;
const SELECT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><rect x="1.5" y="1.5" width="6" height="6" rx="1" fill="none" stroke="#F2F2F3" stroke-width="1.3"/><rect x="4.5" y="4.5" width="6" height="6" rx="1" fill="#F2F2F3"/></svg>"##;

pub fn glyph(cx: &mut Cx, parent: Id, g: G, small: bool) -> Id {
    let r = cx.r();
    let (d, fs) = if small { (1.2, 0.62) } else { (1.45, 0.72) };
    let round = |cx: &mut Cx, bg: Color, border: Option<Color>, label: Option<(&str, Color)>| {
        let mut n = Node::bg(bg).radius(d * r / 2.0);
        if let Some(c) = border {
            n = n.border(1.0, c);
        }
        let id = cx.scene.add(parent, s().wh(d * r, d * r).items_center().justify_center().done(), n);
        if let Some((l, c)) = label {
            cx.scene.add_text(cx.t, id, s().done(), l, ts(r, fs, fs * 1.2, 700), c);
        }
        id
    };
    match g {
        G::A => round(cx, tok::FG, None, Some(("A", tok::BG))),
        G::B => round(cx, tok::FG.alpha(0.85), None, Some(("B", tok::BG))),
        G::X => round(cx, tok::FG.alpha(0.85), None, Some(("X", tok::BG))),
        G::Y => round(cx, tok::FG.alpha(0.85), None, Some(("Y", tok::BG))),
        G::Lb | G::Rb | G::Lt | G::Rt => {
            let l = match g {
                G::Lb => "LB",
                G::Rb => "RB",
                G::Lt => "LT",
                _ => "RT",
            };
            let (h, f) = if small { (1.1, 0.58) } else { (1.3, 0.66) };
            let id = cx.scene.add(parent, s().row().items_center().h(h * r).px(0.4 * r).done(), Node::bg(tok::FG.alpha(0.12)).radius(0.4 * r));
            cx.scene.add_text(cx.t, id, s().done(), l, ts(r, f, f * 1.2, 700).tracking(0.025), tok::FG.alpha(0.9));
            id
        }
        G::Start | G::Select | G::Rare | G::Quick => {
            let (svg, name, bd, ic) = match g {
                G::Start => (START, "g-start", 0.4, 0.62),
                G::Select => (SELECT, "g-select", 0.4, 0.62),
                G::Rare => (GEM, "g-gem", 0.7, 0.72),
                _ => (DOTS, "g-dots", 0.7, 0.72),
            };
            let id = round(cx, Color::TRANSPARENT, Some(tok::FG.alpha(bd)), None);
            let k = cx.img.request(svg_req(name, svg, ic * r));
            let inner = cx.scene.add(id, s().wh(ic * r, ic * r).done(), Node::default().image(k).contain([0.5, 0.5]));
            let _ = inner;
            id
        }
    }
}

/// A glyph and its label (Hint).
pub fn hint(cx: &mut Cx, parent: Id, g: G, label: &str) -> Id {
    let r = cx.r();
    let h = cx.scene.add(parent, s().row().items_center().gap(0.45 * r).done(), Node::default());
    glyph(cx, h, g, false);
    text(cx, h, ty::SMALL, 400, tok::FG.alpha(0.75), label);
    HINTS.with(|v| {
        let mut v = v.borrow_mut();
        v.retain(|(id, _)| cx.scene.alive(*id));
        v.push((h, g));
    });
    h
}

thread_local! {
    /// Every hint drawn by a scene-built screen (its footer's buttons), for touch.
    static HINTS: std::cell::RefCell<Vec<(Id, G)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Touch: the hint (glyph and label) a finger at (`x`, `y`) is on, if any: the node on top there
/// or one of its ancestors is a hint.
pub fn hint_at(scene: &Scene, x: f32, y: f32, now: f64) -> Option<G> {
    let mut at = scene.hit(x, y, now);
    HINTS.with(|v| {
        let v = v.borrow();
        while let Some(id) = at {
            if let Some((_, g)) = v.iter().find(|(h, _)| *h == id) {
                return Some(*g);
            }
            at = scene.parent(id);
        }
        None
    })
}

/// Two glyphs side by side (ltrt).
pub fn glyph_pair(cx: &mut Cx, parent: Id, a: G, b: G, small: bool) -> Id {
    let r = cx.r();
    let h = cx.scene.add(parent, s().row().items_center().gap(0.2 * r).done(), Node::default());
    glyph(cx, h, a, small);
    glyph(cx, h, b, small);
    h
}

// ---- Focusables ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Variant {
    Primary,
    Secondary,
    Dimmed,
    Ghost,
}

impl Variant {
    fn look(self) -> (Color, Color, u16) {
        match self {
            Variant::Primary => (tok::BRAND, tok::FG, 600),
            Variant::Secondary => (tk::BTN2, tok::FG, 500),
            Variant::Dimmed => (tk::DIMMED, tok::MUTED, 500),
            Variant::Ghost => (Color::TRANSPARENT, tok::MUTED, 500),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sz {
    Sm,
    Md,
    Lg,
}

/// A button node (not yet registered with a Nav). `grow` makes it fill its row (w-full).
pub fn button(cx: &mut Cx, parent: Id, label: &str, v: Variant, sz: Sz, full: bool) -> Focusable {
    let r = cx.r();
    let (h, px, fs, lh, gap) = match sz {
        Sz::Sm => (2.25, 0.9, 0.875, 1.25, 0.4),
        Sz::Md => (2.85, 1.35, 0.9375, 1.375, 0.55),
        Sz::Lg => (3.25, 1.75, 1.0625, 1.5, 0.6),
    };
    let (base, fg, w) = v.look();
    let mut st = s().row().items_center().justify_center().h(h * r).px(px * r).gap(gap * r);
    if full {
        st = st.wp(1.0);
    }
    let id = cx.scene.add(parent, st.done(), Node::bg(base).radius(0.25 * r));
    let tid = cx.scene.add_text(cx.t, id, s().done(), label, ts(r, fs, lh, w), fg);
    Focusable { id, kind: Kind::Btn { base, fg, text: tid }, group: 0, hint: String::new(), tag: 0, sid: String::new() }
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Btn { base: Color, fg: Color, text: Id },
    /// A list row: the fill goes to layer-04 under focus.
    Row,
    /// A card: scales up.
    Tile { scale: f32 },
}

#[derive(Clone, Debug)]
pub struct Focusable {
    pub id: Id,
    pub kind: Kind,
    pub group: u32,
    pub hint: String,
    pub tag: u32,
    /// A stable name (data-setting-id): search and journeys find a row by it.
    pub sid: String,
}

impl Focusable {
    pub fn sid(mut self, v: &str) -> Self {
        self.sid = v.to_string();
        self
    }
    pub fn group(mut self, g: u32) -> Self {
        self.group = g;
        self
    }
    pub fn hint(mut self, h: &str) -> Self {
        self.hint = h.to_string();
        self
    }
    pub fn tag(mut self, t: u32) -> Self {
        self.tag = t;
        self
    }
}

/// Focus state for a screen: the registered items, spatial moves, and app.css's focus look.
#[derive(Default)]
pub struct Nav {
    pub items: Vec<Focusable>,
    map: FocusMap,
    pub cur: Option<Id>,
    /// Scroll containers. Not needed any more: focus follows through every `Node::scrolls` box above
    /// the focused row on its own (`Scene::follow_focus`); kept so older screens still build.
    pub scrollers: Vec<Id>,
    /// A row's resting fill when it is not transparent (the current section in a list).
    pub row_base: std::collections::HashMap<Id, Color>,
}

impl Nav {
    pub fn add(&mut self, f: Focusable) -> Id {
        let id = f.id;
        self.map.push(id, f.group, self.items.len() as u32);
        self.items.push(f);
        id
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.map.clear();
        self.cur = None;
        self.row_base.clear();
    }

    /// Drops every item after the first `n` (a pane that was rebuilt). Focus falls back to None when
    /// it was on a dropped item.
    pub fn truncate(&mut self, n: usize) {
        self.items.truncate(n);
        self.map.clear();
        for (i, f) in self.items.iter().enumerate() {
            self.map.push(f.id, f.group, i as u32);
        }
        if let Some(c) = self.cur {
            if !self.items.iter().any(|f| f.id == c) {
                self.cur = None;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current(&self) -> Option<&Focusable> {
        self.cur.and_then(|c| self.items.iter().find(|f| f.id == c))
    }

    fn find(&self, id: Id) -> Option<&Focusable> {
        self.items.iter().find(|f| f.id == id)
    }

    pub fn focus(&mut self, scene: &mut Scene, id: Id, now: f64, animate: bool) {
        let d = |v: f32| if animate { v } else { 0.0 };
        if let Some(old) = self.cur.take() {
            if let Some(f) = self.find(old).cloned() {
                scene.n(old).ring.snap(0.0);
                scene.n(old).z = 0;
                scene.n(old).scale.set(1.0, now, d(motion::FOCUS));
                match f.kind {
                    Kind::Btn { base, fg, text } => {
                        scene.n(old).bg.set(base, now, d(motion::FOCUS));
                        scene.n(text).text.as_mut().unwrap().color.set(fg, now, d(motion::FOCUS));
                    }
                    Kind::Row => scene.n(old).bg.set(self.row_base.get(&old).copied().unwrap_or(Color::TRANSPARENT), now, d(motion::FOCUS)),
                    Kind::Tile { .. } => {}
                }
            }
        }
        let Some(f) = self.find(id).cloned() else { return };
        scene.n(id).ring.snap(1.0);
        scene.n(id).z = 2;
        match f.kind {
            Kind::Btn { text, .. } => {
                scene.n(id).bg.set(tok::FG, now, d(motion::FOCUS));
                scene.n(id).scale.set(1.04, now, d(motion::FOCUS));
                scene.n(text).text.as_mut().unwrap().color.set(tok::BG, now, d(motion::FOCUS));
            }
            Kind::Row => scene.n(id).bg.set(Color::rgb(35, 30, 47), now, d(motion::FOCUS)),
            Kind::Tile { scale } => scene.n(id).scale.set(scale, now, d(motion::FOCUS)),
        }
        self.map.note(id);
        self.cur = Some(id);
        if animate {
            self.reveal(scene, id, now);
        }
    }

    /// Brings `id` into view in every scroll container above it (`Node::scrolls`, registered or
    /// not), with nav/focus.ts's rules and 65 ms glide (`rare_render::scroll::follow`). A list
    /// rebuilt in the same step is measured after its next layout.
    pub fn reveal(&self, scene: &mut Scene, id: Id, now: f64) {
        scene.follow_focus(id, now, true);
    }

    /// Touch: focus on the topmost item under window point (`x`, `y`), with no scrolling (it is
    /// where the finger is). True when one is there.
    pub fn focus_at(&mut self, scene: &mut Scene, x: f32, y: f32, now: f64) -> bool {
        let ids: Vec<Id> = self.items.iter().map(|f| f.id).collect();
        let Some(id) = scene.hit_among(x, y, now, &ids) else { return false };
        if self.cur != Some(id) {
            self.focus(scene, id, now, false);
        }
        true
    }

    /// A directional press: the next focus, applied. True when focus moved.
    pub fn step(&mut self, scene: &mut Scene, pad: Pad, now: f64) -> bool {
        let dir = match pad {
            Pad::Up => Dir::Up,
            Pad::Down => Dir::Down,
            Pad::Left => Dir::Left,
            Pad::Right => Dir::Right,
            _ => return false,
        };
        let Some(cur) = self.cur else { return false };
        match self.map.step(scene, cur, dir) {
            Some(n) => {
                self.focus(scene, n, now, true);
                true
            }
            None => false,
        }
    }

    /// A directional press that may only land on the items whose index `keep` allows (one column of
    /// a two-column screen: layout rects ignore scrolling, so two scrolled columns can't be compared).
    pub fn step_in(&mut self, scene: &mut Scene, pad: Pad, now: f64, keep: impl Fn(usize) -> bool) -> bool {
        let dir = match pad {
            Pad::Up => Dir::Up,
            Pad::Down => Dir::Down,
            Pad::Left => Dir::Left,
            Pad::Right => Dir::Right,
            _ => return false,
        };
        let Some(cur) = self.cur else { return false };
        let mut m = FocusMap::default();
        for (i, f) in self.items.iter().enumerate() {
            if f.id == cur || keep(i) {
                m.push(f.id, f.group, i as u32);
            }
        }
        match m.step(scene, cur, dir) {
            Some(n) => {
                self.focus(scene, n, now, true);
                true
            }
            None => false,
        }
    }
}

// ---- Rows (controls.tsx Row, menu Line) ----

/// A table-style row: h-[3.25rem], rounded-card, px-[0.75rem], title (+ sub) left and an optional right slot.
pub struct RowParts {
    pub f: Focusable,
    pub title: Id,
    pub sub: Option<Id>,
    pub right: Id,
}

pub fn row(cx: &mut Cx, parent: Id, title: &str, sub: Option<&str>) -> RowParts {
    let r = cx.r();
    let id = cx.scene.add(parent, s().row().items_center().gap(0.75 * r).h(3.25 * r).wp(1.0).px(0.75 * r).done(), Node::default().radius(0.5 * r));
    let col = cx.scene.add(id, s().col().grow().done(), Node::default());
    let tt = cx.scene.add_text(cx.t, col, s().done(), title, ts(r, ty::SMALL.0, ty::SMALL.1, 500).truncate(), tok::FG);
    let sub_id = sub.map(|x| cx.scene.add_text(cx.t, col, s().done(), x, ts(r, ty::LABEL.0, ty::LABEL.1, 400).truncate(), tok::MUTED));
    let right = cx.scene.add(id, s().row().items_center().gap(0.3 * r).no_shrink_().done(), Node::default());
    RowParts { f: Focusable { id, kind: Kind::Row, group: 0, hint: String::new(), tag: 0, sid: String::new() }, title: tt, sub: sub_id, right }
}

/// Tabs (underline, text + optional count).
pub fn tabs(cx: &mut Cx, parent: Id, items: &[(&str, Option<u32>)], on: usize, small: bool) -> Id {
    let r = cx.r();
    let row_ = cx.scene.add(parent, s().row().items_stretch().gap(1.25 * r).done(), Node::default());
    let fs = if small { ty::SMALL } else { ty::BODY };
    for (i, (label, count)) in items.iter().enumerate() {
        let cell = cx.scene.add(row_, s().row().items_center().done(), Node::default());
        let inner = cx.scene.add(cell, s().row().items_center().gap(0.4 * r).done(), Node::default());
        let c = if i == on { tok::FG } else { tok::MUTED };
        text(cx, inner, fs, 500, c, label);
        if let Some(n) = count {
            text(cx, inner, ty::META, 400, if i == on { tok::MUTED } else { tok::MUTED.alpha(0.7) }, &n.to_string());
        }
        if i == on {
            cx.scene.add(inner, s().abs(None, Some(0.0), Some(-0.3 * r), Some(0.0)).h(2.0).done(), Node::bg(tok::FG).radius(1.0));
        }
    }
    row_
}

pub fn size_unused() -> f32 {
    size::BODY
}

/// overlay/panels.tsx Frozen: the game's frame, dimmed black/65, with a black/55 fade up the right 45rem.
pub fn frozen(cx: &mut Cx, parent: Id, src: Option<Src>) -> Id {
    use rare_render::scene::{GradDir, Gradient};
    let r = cx.r();
    let (w, h) = (cx.scene.w, cx.scene.h);
    let wrap = cx.scene.add(parent, s().fill().done(), Node::bg(tok::BG).clip());
    if let Some(src) = src {
        let k = cx.img.request(ImgReq { src, w: w as u32, h: h as u32, fit: Fit::Cover { fy: 0.5 } });
        cx.scene.add(wrap, s().fill().done(), Node::default().image(k));
    }
    cx.scene.add(wrap, s().fill().done(), Node::bg(Color::BLACK.alpha(0.65)));
    let b = Color::BLACK;
    cx.scene.add(wrap, s().abs(Some(0.0), Some(0.0), Some(0.0), None).w(45.0 * r).done(), Node::default().grad(Gradient { dir: GradDir::ToLeft, stops: [b.alpha(0.55), b.alpha(0.275), b.alpha(0.0)], mid: 0.5 }));
    wrap
}

/// .fade-in: 220 ms ease, from 0.4rem down.
pub fn fade_in(cx: &mut Cx, id: Id, now: f64) {
    let r = cx.r();
    cx.scene.n(id).opacity.restart(0.0, 1.0, now, 0.22);
    cx.scene.n(id).ty.restart(0.4 * r, 0.0, now, 0.22);
}

/// Tokens beyond theme::tok (app.css :root).
pub mod tk {
    use rare_render::Color;
    pub const BORDER_LIGHT: Color = Color::rgb(50, 45, 62);
    pub const BTN2: Color = Color::rgb(62, 56, 77).alpha(0.6);
    pub const BTN_ACTIVE: Color = Color::rgb(71, 64, 89);
    pub const SEG: Color = Color::rgb(62, 56, 77).alpha(0.6);
    pub const SEG_ACTIVE: Color = Color::rgb(71, 64, 89).alpha(0.9);
    pub const DIMMED: Color = Color::rgb(42, 37, 55).alpha(0.4);
    pub const NEUTRAL: Color = Color::rgb(46, 38, 64);
    pub const BUY: Color = Color::rgb(42, 223, 114);
}

/// ui Toggle: h 1.5rem, w 2.6rem pill; the knob 1.1rem. Returns (track, knob) so a screen can flip it.
pub fn toggle(cx: &mut Cx, parent: Id, on: bool) -> (Id, Id) {
    let r = cx.r();
    let track = cx.scene.add(parent, s().wh(2.6 * r, 1.5 * r).no_shrink_().done(), Node::bg(if on { tok::FG } else { tok::L4 }).radius(0.75 * r));
    let x = if on { 1.3 } else { 0.2 };
    let knob = cx.scene.add(track, s().abs(Some(0.2 * r), None, None, Some(x * r)).wh(1.1 * r, 1.1 * r).done(), Node::bg(if on { tok::BG } else { tok::MUTED }).radius(0.55 * r));
    (track, knob)
}

pub fn set_toggle(cx: &mut Cx, track: Id, knob: Id, on: bool, now: f64) {
    let r = cx.r();
    cx.scene.n(track).bg.set(if on { tok::FG } else { tok::L4 }, now, 0.15);
    cx.scene.n(knob).bg.set(if on { tok::BG } else { tok::MUTED }, now, 0.15);
    let mut st = cx.scene.style(knob);
    st.inset.left = rare_render::taffy::prelude::length((if on { 1.3 } else { 0.2 }) * r);
    cx.scene.set_style(knob, st);
}

/// ui Dot: 0.45rem, the only coloured marker.
pub fn dot(cx: &mut Cx, parent: Id, c: Color) -> Id {
    let r = cx.r();
    cx.scene.add(parent, s().wh(0.45 * r, 0.45 * r).no_shrink_().done(), Node::bg(c).radius(0.225 * r))
}

/// ui Empty: a calm card with a title, an optional line and optional actions row (returned).
pub fn empty(cx: &mut Cx, parent: Id, title: &str, body: Option<&str>) -> Id {
    let r = cx.r();
    let card = cx.scene.add(parent, s().col().items_start().gap(0.5 * r).pad(1.5 * r, 1.3 * r, 1.5 * r, 1.3 * r).done(), Node::bg(tok::L1).radius(0.5 * r).border(1.0, tok::HAIR));
    text(cx, card, ty::ROW, 600, tok::FG, title);
    if let Some(b) = body {
        let r_ = cx.r();
        cx.scene.add_text(cx.t, card, s().max_w(34.0 * r_).done(), b, ts(r_, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
    }
    cx.scene.add(card, s().row().gap(0.75 * r).done(), Node::default())
}

/// trade/parts Sheet: a dimmed page, a centred card (max 44rem, or 60 wide) with a title header, a
/// body and an optional footer row. Returns (card, body, footer).
pub struct SheetParts {
    pub card: Id,
    pub body: Id,
    pub footer: Option<Id>,
}

pub fn sheet(cx: &mut Cx, parent: Id, title: &str, sub: Option<&str>, with_footer: bool, wide: bool, now: f64) -> SheetParts {
    let r = cx.r();
    let page = cx.scene.add(parent, s().abs(Some(0.0), Some(0.0), Some(3.0 * r), Some(0.0)).items_center().justify_center().p(1.5 * r).done(), Node::default());
    cx.scene.add(page, s().fill().done(), Node::bg(tok::BG.alpha(0.8)));
    let mut card_style = s().col().wp(1.0).max_w(if wide { 60.0 } else { 44.0 } * r).done();
    // max-h-full: a tall body scrolls inside the card (below) instead of running off the page.
    card_style.max_size.height = rare_render::taffy::prelude::percent(1.0);
    let card = cx.scene.add(page, card_style, Node::bg(tok::BG).radius(1.25 * r).border(1.0, tk::BORDER_LIGHT).clip().shadow(&rare_render::scene::Shadow::xl2(0.4)));
    fade_in(cx, card, now);
    let head = cx.scene.add(card, s().col().pad(2.0 * r, 1.5 * r, 2.0 * r, 1.0 * r).done(), Node::default());
    text(cx, head, ty::ROW, 600, tok::FG, title);
    if let Some(sb) = sub {
        let r_ = cx.r();
        cx.scene.add_text(cx.t, head, s().done(), sb, ts(r_, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
    }
    edge(cx, head, 2, tok::HAIR);
    // overflow-y-auto: focus follows inside it on its own (Scene::follow_focus).
    let mut body_style = s().col().px(2.0 * r).py(1.25 * r).shrink().done();
    body_style.min_size.height = rare_render::taffy::prelude::length(0.0);
    let body = cx.scene.add(card, body_style, Node::default().scrolls());
    let footer = if with_footer {
        let f = cx.scene.add(card, s().row().gap(0.5 * r).px(2.0 * r).py(1.0 * r).done(), Node::default());
        edge(cx, f, 0, tok::HAIR);
        Some(f)
    } else {
        None
    };
    SheetParts { card, body, footer }
}

/// A thin 2 px loading bar across the top of a box (static at its start; the engine animates it later).
pub fn load_bar(cx: &mut Cx, parent: Id, on: bool) -> Id {
    let id = cx.scene.add(parent, s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h(2.0).done(), Node::default().opacity(if on { 1.0 } else { 0.0 }));
    cx.scene.add(id, s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(0.4).done(), Node::bg(tok::FG.alpha(0.55)));
    id
}

/// StatLine: label over value, a hairline between items.
pub fn stat_line(cx: &mut Cx, parent: Id, items: &[(&str, &str)]) -> Id {
    stat_line_sized(cx, parent, items, false)
}

/// `large` is StatLine size="lg": the values at text-row.
pub fn stat_line_sized(cx: &mut Cx, parent: Id, items: &[(&str, &str)], large: bool) -> Id {
    let r = cx.r();
    let dl = cx.scene.add(parent, s().row().done(), Node::default());
    let n = items.len();
    for (i, (l, v)) in items.iter().enumerate() {
        let pr = if i < n - 1 { 1.0 * r } else { 0.0 };
        let pl = if i > 0 { 1.0 * r } else { 0.0 };
        let cell = cx.scene.add(dl, s().col().grow().pad(pl, 0.0, pr, 0.0).done(), Node::default());
        if i > 0 {
            edge(cx, cell, 3, tok::HAIR);
        }
        text(cx, cell, ty::LABEL, 400, tok::MUTED, l);
        text(cx, cell, if large { ty::ROW } else { ty::BODY }, 500, tok::FG, v);
    }
    dl
}

/// Segmented control (display only): equal chips, first and last rounded.
pub fn segmented(cx: &mut Cx, parent: Id, items: &[&str], on: usize) -> Id {
    let r = cx.r();
    let row_ = cx.scene.add(parent, s().row().gap(1.0).done(), Node::default());
    for (i, l) in items.iter().enumerate() {
        let c = cx.scene.add(row_, s().row().items_center().justify_center().h(1.9 * r).min_w(2.4 * r).px(0.6 * r).done(), Node::bg(if i == on { tk::SEG_ACTIVE } else { tk::SEG }).radius(0.25 * r));
        text(cx, c, ty::META, 500, if i == on { tok::FG } else { tok::MUTED }, l);
    }
    row_
}

/// A screen that is just a focus list: the D-pad moves, B closes. For fixtures of loose components.
pub struct NavScreen {
    pub nav: Nav,
}

impl Screen for NavScreen {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            _ => Out::None,
        }
    }
}
