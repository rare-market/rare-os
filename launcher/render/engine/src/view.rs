//! Views: screens as functions from app state to an `El` tree with keyed children. A `View` keeps
//! what it mounted last time and turns a new tree into the smallest set of `Scene` edits: nodes
//! are created, removed, moved, restyled (relayout) or re-looked (paint only) one by one, and a
//! subtree whose description did not change costs a comparison and nothing else.
//!
//! Rules that keep a frame cheap:
//! - rebuild only when state changed (`View::update_stamped` skips the build when the stamp is the
//!   same), never per animation frame; animations run in the scene on their own;
//! - a look change (colour, opacity, transform) never relayouts; only `Style` and text do;
//! - give list items a stable `key` so inserting at the top moves nodes instead of rewriting
//!   every row.
//!
//! Focus is drawn by the view, not by the screen: an element with `focus` gets the ring and z 2
//! when it is `View::set_focus`ed, its `focus_look` (and its descendants' `focus_look` /
//! `focus_color`, like CSS's inherited `color`) while it holds focus, all through its
//! `transition` (default: app.css's `[data-nav]` 110 ms), with no rebuild. `View::register`
//! hands every focusable, group and scroll container to `focus::Nav`, and
//! `View::apply_scrolls` moves the scroll containers where Nav says.
//!
//! ```ignore
//! fn shelf(games: &[Game]) -> El {
//!     El::row().gap(16.0).scrolls_x(key("shelf")).group(Group::new("shelf", [0.0; 4]).rail())
//!         .kids(games.iter().map(|g| tile(g).key(key(&g.id))))
//! }
//! view.update_stamped(&mut scene, &mut text, state.version(), now, || shelf(&state.games));
//! ```

use crate::anim::Ease;
use crate::color::Color;
use crate::focus::{Focusable, Group, Nav, Scroller};
use crate::scene::{Gradient, Id, LinearGradient, Node, Scene, Shadow};
use crate::text::{TextStyle, TextSys};
use crate::theme::motion;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use taffy::prelude::*;

/// A child's identity among its siblings.
pub type Key = u64;

/// Hashes anything into a `Key` (stable across runs).
pub fn key<T: Hash + ?Sized>(v: &T) -> Key {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish() & !(1 << 63)
}

/// Unkeyed children are keyed by position in their own half of the space (matching between renders is `align`).
fn pos_key(i: usize) -> Key {
    (1 << 63) | i as u64
}

/// How a node looks, without its layout: everything paint-only. `tw!` produces one of these with
/// the `Style`.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub bg: Color,
    pub grad: Option<Gradient>,
    /// An N-stop gradient at any angle (painted over `grad`).
    pub lgrad: Option<LinearGradient>,
    pub radius: f32,
    /// Per-corner radii [tl, tr, br, bl]; overrides `radius`.
    pub corners: Option<[f32; 4]>,
    /// Radius as a fraction of the shorter side (`rounded-[22%]`); overrides both.
    pub radius_pct: f32,
    pub border: f32,
    /// Per-side border widths [top, right, bottom, left]; overrides `border`.
    pub borders: Option<[f32; 4]>,
    pub border_color: Color,
    pub shadows: Vec<Shadow>,
    /// overflow: hidden (children clipped to the rounded box).
    pub clip: bool,
    pub z: i16,
    pub opacity: f32,
    pub scale: f32,
    pub origin: [f32; 2],
    pub tx: f32,
    pub ty: f32,
    pub hidden: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            bg: Color::TRANSPARENT,
            grad: None,
            lgrad: None,
            radius: 0.0,
            corners: None,
            radius_pct: 0.0,
            border: 0.0,
            borders: None,
            border_color: Color::TRANSPARENT,
            shadows: Vec::new(),
            clip: false,
            z: 0,
            opacity: 1.0,
            scale: 1.0,
            origin: [0.5, 0.5],
            tx: 0.0,
            ty: 0.0,
            hidden: false,
        }
    }
}

impl Look {
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = c;
        self
    }
    pub fn radius(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
    pub fn border(mut self, w: f32, c: Color) -> Self {
        self.border = w;
        self.border_color = c;
        self
    }
    pub fn grad(mut self, g: Gradient) -> Self {
        self.grad = Some(g);
        self
    }
    pub fn lgrad(mut self, g: LinearGradient) -> Self {
        self.lgrad = Some(g);
        self
    }
    /// Per-corner radii [top-left, top-right, bottom-right, bottom-left] (`rounded-t-sheet` is
    /// [r, r, 0, 0]; see `group_corners` for `first:` / `last:` rounding).
    pub fn corners(mut self, r: [f32; 4]) -> Self {
        self.corners = Some(r);
        self
    }
    /// `rounded-[22%]`: `radius_pct(0.22)`.
    pub fn radius_pct(mut self, f: f32) -> Self {
        self.radius_pct = f;
        self
    }
    /// One-sided borders [top, right, bottom, left]: `border-b` is `borders([0, 0, 1, 0], c)`.
    pub fn borders(mut self, w: [f32; 4], c: Color) -> Self {
        self.borders = Some(w);
        self.border_color = c;
        self
    }
    pub fn shadow(mut self, s: &[Shadow]) -> Self {
        self.shadows = s.to_vec();
        self
    }
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }
    pub fn z(mut self, z: i16) -> Self {
        self.z = z;
        self
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.opacity = o;
        self
    }
    pub fn scale(mut self, s: f32) -> Self {
        self.scale = s;
        self
    }
    pub fn origin(mut self, x: f32, y: f32) -> Self {
        self.origin = [x, y];
        self
    }
    pub fn translate(mut self, x: f32, y: f32) -> Self {
        self.tx = x;
        self.ty = y;
        self
    }
    pub fn hidden(mut self, h: bool) -> Self {
        self.hidden = h;
        self
    }
}

/// Grouped rounding (`first:rounded-t-panel last:rounded-b-panel` on a stack of rows, or
/// `rounded-l` / `rounded-r` ends of a segmented row): the corners of item `i` of `n` in a group
/// whose outer corners are `r`. `column`: stacked top to bottom, else left to right.
pub fn group_corners(i: usize, n: usize, r: f32, column: bool) -> [f32; 4] {
    let (first, last) = (i == 0, i + 1 >= n);
    let f = |b: bool| if b { r } else { 0.0 };
    if column {
        [f(first), f(first), f(last), f(last)]
    } else {
        [f(first), f(last), f(last), f(first)]
    }
}

/// What a node draws besides its box.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Content {
    #[default]
    None,
    Text { text: String, style: TextStyle, color: Color },
    /// An `ImageLoader` key; `tint` draws its alpha in a colour (icons); `contain` aligns it.
    Image { key: u64, tint: Option<Color>, contain: Option<[f32; 2]> },
    /// An app texture (`Gpu::create_stream`, updated in place with `Gpu::write_texture`) of
    /// `w` x `h` pixels, cover fit: a video frame, a camera.
    Texture { tex: crate::gpu::TexId, w: u32, h: u32 },
}

/// CSS `transition` for look changes: transform and opacity on `ease`, colours on `color_ease`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub dur: f32,
    pub ease: Ease,
    pub color_ease: Ease,
}

impl Transition {
    /// app.css `[data-nav]`: transform 110 ms on the pro curve, colours 110 ms `ease`.
    pub const FOCUS: Transition = Transition { dur: motion::FOCUS, ease: Ease::PRO, color_ease: Ease::EASE };
    pub const fn pro(dur: f32) -> Transition {
        Transition { dur, ease: Ease::PRO, color_ease: Ease::PRO }
    }
}

/// A translate that may depend on the node's own size or the rem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shift {
    Px(f32),
    Rem(f32),
    /// Fraction of the node's own width (x) or height (y): CSS `translateX(100%)` is `Pct(1.0)`.
    Pct(f32),
}

/// An enter (`from`) or exit (`to`) state, like a CSS keyframe animation's other end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub opacity: Option<f32>,
    pub scale: Option<f32>,
    pub x: Option<Shift>,
    pub y: Option<Shift>,
    pub dur: f32,
    pub ease: Ease,
}

impl Motion {
    const fn base(dur: f32, ease: Ease) -> Motion {
        Motion { opacity: None, scale: None, x: None, y: None, dur, ease }
    }
    /// `.pop`: opacity 0 and scale 0.96, 320 ms (toasts, popups, handoff card).
    pub const POP: Motion = Motion { opacity: Some(0.0), scale: Some(0.96), ..Motion::base(motion::SLOW, Ease::PRO) };
    /// `.rise`: opacity 0 and 0.75 rem down, 320 ms (full-screen pages).
    pub const RISE: Motion = Motion { opacity: Some(0.0), y: Some(Shift::Rem(0.75)), ..Motion::base(motion::SLOW, Ease::PRO) };
    /// `.fade`: opacity 0, 200 ms.
    pub const FADE: Motion = Motion { opacity: Some(0.0), ..Motion::base(motion::BASE, Ease::PRO) };
    /// `.fade-in`: opacity 0 and 0.4 rem down, 220 ms `ease` (sheets, review dialogs).
    pub const FADE_IN: Motion = Motion { opacity: Some(0.0), y: Some(Shift::Rem(0.4)), ..Motion::base(0.22, Ease::EASE) };
    /// `.slide-in-right`: from its own width to the right, 200 ms (Quick Settings, the Rare menu).
    pub const SLIDE_IN_RIGHT: Motion = Motion { x: Some(Shift::Pct(1.0)), ..Motion::base(motion::BASE, Ease::PRO) };
    /// `.slide-up`: from its own height below, 200 ms (the on-screen keyboard).
    pub const SLIDE_UP: Motion = Motion { y: Some(Shift::Pct(1.0)), ..Motion::base(motion::BASE, Ease::PRO) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// A `[data-rail]` strip: scrolls sideways.
    X,
    /// A `[data-scroll]` page: scrolls vertically.
    Y,
}

/// One element of a view: a Taffy style, a look, content and children.
#[derive(Clone, Debug, Default)]
pub struct El {
    pub key: Option<Key>,
    pub style: Style,
    pub look: Look,
    /// The look while the nearest focusable (itself or an ancestor) has focus.
    pub focus_look: Option<Look>,
    /// Text colour while the nearest focusable has focus (a white button's dark label).
    pub focus_color: Option<Color>,
    pub content: Content,
    pub kids: Vec<El>,
    pub transition: Option<Transition>,
    pub enter: Option<Motion>,
    pub exit: Option<Motion>,
    /// `[data-nav]`: a template (`id` and `rect` are filled in by `register`; group, scroll and
    /// rail come from the ancestors unless set).
    pub focus: Option<Focusable>,
    /// `[data-nav-group]` (its rect is filled in).
    pub group: Option<Group>,
    /// A `<section>` around a group: the page keeps that whole row in view.
    pub section: bool,
    /// `[data-nav-scope]`: focusables inside take this scope (higher traps focus).
    pub scope: Option<u32>,
    /// `[inert]`: focusables inside are skipped.
    pub inert: bool,
    /// A scroll container with Nav's scroller key.
    pub scrolls: Option<(Axis, u64)>,
    /// `[data-sticky]`: a header that stays on top of its page; reveal keeps rows below it.
    pub sticky: bool,
    /// Look the node up with `View::id(name)` after an update.
    pub name: Option<&'static str>,
    /// Mount into the full-window layer at this z (`scene::layer`), like `position: fixed`.
    pub portal: Option<i16>,
    /// A live number's text (Num `live`): when the text changes the moved characters roll and the
    /// background flashes (`text::roll`). The value says up or down.
    pub roll: Option<RollSpec>,
    /// A paint slot (`Node::slot`): `Scene::paint_with` draws a chart, video, Num roll or edit
    /// field here, in scene order.
    pub slot: Option<u32>,
}

/// What a rolling number needs: its value and the up and down colours of the flash.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollSpec {
    pub value: f64,
    pub colors: [Color; 2],
    pub radius: f32,
}

impl El {
    pub fn new(style: Style) -> El {
        El { style, ..Default::default() }
    }
    /// Rolls this text's changed characters when it changes (a live number).
    pub fn roll(mut self, value: f64, up: Color, down: Color, radius: f32) -> El {
        self.roll = Some(RollSpec { value, colors: [up, down], radius });
        self
    }
    /// A flex row.
    pub fn row() -> El {
        El::new(Style { flex_direction: FlexDirection::Row, ..Default::default() })
    }
    /// A flex column.
    pub fn col() -> El {
        El::new(Style { flex_direction: FlexDirection::Column, ..Default::default() })
    }
    /// A text leaf.
    pub fn text(text: impl Into<String>, style: TextStyle, color: Color) -> El {
        El { content: Content::Text { text: text.into(), style, color }, ..Default::default() }
    }
    /// An image leaf (cover-fit pixels from the loader, clipped to the radius).
    pub fn image(img: u64) -> El {
        El { content: Content::Image { key: img, tint: None, contain: None }, ..Default::default() }
    }
    /// An app texture leaf (video, camera): see `Content::Texture`.
    pub fn texture(tex: crate::gpu::TexId, w: u32, h: u32) -> El {
        El { content: Content::Texture { tex, w, h }, ..Default::default() }
    }
    /// An icon: the image's alpha in `tint`, centred.
    pub fn icon(img: u64, tint: Color) -> El {
        El { content: Content::Image { key: img, tint: Some(tint), contain: Some([0.5, 0.5]) }, ..Default::default() }
    }

    pub fn key(mut self, k: Key) -> El {
        self.key = Some(k);
        self
    }
    pub fn style(mut self, f: impl FnOnce(&mut Style)) -> El {
        f(&mut self.style);
        self
    }
    pub fn with_style(mut self, s: Style) -> El {
        self.style = s;
        self
    }
    pub fn look(mut self, l: Look) -> El {
        self.look = l;
        self
    }
    pub fn bg(mut self, c: Color) -> El {
        self.look.bg = c;
        self
    }
    pub fn radius(mut self, r: f32) -> El {
        self.look.radius = r;
        self
    }
    /// Per-corner radii [tl, tr, br, bl].
    pub fn corners(mut self, r: [f32; 4]) -> El {
        self.look.corners = Some(r);
        self
    }
    /// Per-side borders [top, right, bottom, left].
    pub fn borders(mut self, w: [f32; 4], c: Color) -> El {
        self.look.borders = Some(w);
        self.look.border_color = c;
        self
    }
    pub fn lgrad(mut self, g: LinearGradient) -> El {
        self.look.lgrad = Some(g);
        self
    }
    pub fn border(mut self, w: f32, c: Color) -> El {
        self.look.border = w;
        self.look.border_color = c;
        self
    }
    pub fn shadow(mut self, s: &[Shadow]) -> El {
        self.look.shadows = s.to_vec();
        self
    }
    pub fn clip(mut self) -> El {
        self.look.clip = true;
        self
    }
    pub fn opacity(mut self, o: f32) -> El {
        self.look.opacity = o;
        self
    }
    pub fn gap(mut self, g: f32) -> El {
        self.style.gap = Size { width: length(g), height: length(g) };
        self
    }
    pub fn focus_look(mut self, l: Look) -> El {
        self.focus_look = Some(l);
        self
    }
    pub fn focus_color(mut self, c: Color) -> El {
        self.focus_color = Some(c);
        self
    }
    pub fn transition(mut self, t: Transition) -> El {
        self.transition = Some(t);
        self
    }
    pub fn enter(mut self, m: Motion) -> El {
        self.enter = Some(m);
        self
    }
    pub fn exit(mut self, m: Motion) -> El {
        self.exit = Some(m);
        self
    }
    /// `[data-nav]` from a blank template: `.nav(|f| f.tag(i).default())`.
    pub fn nav(self, f: impl FnOnce(Focusable) -> Focusable) -> El {
        self.focusable(f(Focusable::new(NodeId::from(u64::MAX), [0.0; 4])))
    }
    /// `[data-nav]`, with app.css's focus transition unless one is set.
    pub fn focusable(mut self, f: Focusable) -> El {
        self.focus = Some(f);
        if self.transition.is_none() {
            self.transition = Some(Transition::FOCUS);
        }
        self
    }
    pub fn group(mut self, g: Group) -> El {
        self.group = Some(g);
        self
    }
    pub fn section(mut self) -> El {
        self.section = true;
        self
    }
    pub fn scope(mut self, s: u32) -> El {
        self.scope = Some(s);
        self
    }
    pub fn inert(mut self, i: bool) -> El {
        self.inert = i;
        self
    }
    /// A vertical scroll page (`[data-scroll]`) with Nav key `k`.
    pub fn scrolls_y(mut self, k: u64) -> El {
        self.scrolls = Some((Axis::Y, k));
        self
    }
    /// A sideways rail (`[data-rail]`) with Nav key `k`.
    pub fn scrolls_x(mut self, k: u64) -> El {
        self.scrolls = Some((Axis::X, k));
        self
    }
    pub fn sticky(mut self) -> El {
        self.sticky = true;
        self
    }
    pub fn name(mut self, n: &'static str) -> El {
        self.name = Some(n);
        self
    }
    /// Paint slot `n` (see `scene::SlotPaint`): size it with the style, draw it in
    /// `Scene::paint_with`'s callback.
    pub fn slot(mut self, n: u32) -> El {
        self.slot = Some(n);
        self
    }
    pub fn portal(mut self, z: i16) -> El {
        self.portal = Some(z);
        self
    }
    pub fn child(mut self, c: El) -> El {
        self.kids.push(c);
        self
    }
    pub fn kids(mut self, cs: impl IntoIterator<Item = El>) -> El {
        self.kids.extend(cs);
        self
    }
    /// Adds the child only when `Some` (conditional rendering keeps positional keys stable when
    /// the slot is given a `key`).
    pub fn maybe(mut self, c: Option<El>) -> El {
        if let Some(c) = c {
            self.kids.push(c);
        }
        self
    }
}

/// App state with a version (the web launcher's zustand stores): every change bumps the version,
/// and a view stamped with the versions it reads rebuilds only when one of them moved.
///
/// ```ignore
/// let stamp = view::stamp(&[library.version(), settings.version()]);
/// view.update_stamped(&mut scene, &mut text, stamp, now, || home(library.get(), settings.get()));
/// ```
#[derive(Debug, Default, Clone)]
pub struct Store<T> {
    value: T,
    version: u64,
}

impl<T> Store<T> {
    pub fn new(value: T) -> Store<T> {
        Store { value, version: 1 }
    }
    pub fn get(&self) -> &T {
        &self.value
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn set(&mut self, value: T) {
        self.value = value;
        self.version += 1;
    }
    /// Changes the value in place (always counts as a change).
    pub fn update(&mut self, f: impl FnOnce(&mut T)) {
        f(&mut self.value);
        self.version += 1;
    }
    /// Sets the value only if it differs: true when it changed.
    pub fn replace(&mut self, value: T) -> bool
    where
        T: PartialEq,
    {
        if self.value == value {
            return false;
        }
        self.set(value);
        true
    }
}

/// One stamp from several store versions (and any other inputs, like the rem or a tab index).
pub fn stamp(parts: &[u64]) -> u64 {
    key(parts)
}

/// What one update changed (the frame-budget gates read these).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edits {
    pub created: u32,
    pub removed: u32,
    pub moved: u32,
    /// Style changes: these relayout.
    pub restyled: u32,
    /// Text or text style changes: these relayout the text.
    pub retexted: u32,
    /// Look-only changes: paint only.
    pub relooked: u32,
}

impl Edits {
    /// True when the edits need a layout pass.
    pub fn layout(&self) -> bool {
        self.created + self.removed + self.moved + self.restyled + self.retexted > 0
    }
    fn add(&mut self, o: Edits) {
        self.created += o.created;
        self.removed += o.removed;
        self.moved += o.moved;
        self.restyled += o.restyled;
        self.retexted += o.retexted;
        self.relooked += o.relooked;
    }
}

/// What an unkeyed child is, for matching it against the previous render's children.
fn sig(e: &El) -> u64 {
    let kind = match e.content {
        Content::Text { .. } => 1u64,
        Content::Image { .. } => 2,
        Content::Texture { .. } => 3,
        Content::None => 0,
    };
    key(&(kind, e.focus.as_ref().map(|f| f.tag), e.scrolls.map(|s| s.1), e.name, e.slot, e.portal))
}

/// In-order matching of `a` to `b` (longest common subsequence): for each `b` the index of the `a`
/// it pairs with. Long lists pair by position.
fn align(a: &[u64], b: &[u64]) -> Vec<Option<usize>> {
    let (n, m) = (a.len(), b.len());
    let mut out = vec![None; m];
    if n * m > 65_536 {
        for j in 0..m.min(n) {
            out[j] = Some(j);
        }
        return out;
    }
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[at(i, j)] = if a[i] == b[j] { t[at(i + 1, j + 1)] + 1 } else { t[at(i + 1, j)].max(t[at(i, j + 1)]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out[j] = Some(i);
            i += 1;
            j += 1;
        } else if t[at(i + 1, j)] >= t[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// A mounted element: its node and the description it was last given (children moved to `kids`).
struct M {
    id: Id,
    key: Key,
    el: El,
    kids: Vec<M>,
    /// The look and text colour targets currently on the node.
    look: Look,
    color: Option<Color>,
    focused: bool,
    /// Whether the look was computed in focus (the nearest focusable held focus).
    in_focus: bool,
}

/// Removes the portals mounted under `m`: they live in their layers, not under `m`'s node, so
/// removing `m`'s node alone would leave them on screen (a sheet inside a page that went away).
fn remove_portals(scene: &mut Scene, m: &M) {
    for k in &m.kids {
        if k.el.portal.is_some() {
            scene.remove(k.id);
        }
        remove_portals(scene, k);
    }
}

struct Leaving {
    id: Id,
    parent: Id,
    index: usize,
    until: f64,
}

/// One mounted view tree under a host node (the root, a layer, or any node of a screen).
pub struct View {
    host: Id,
    root: Option<M>,
    leaving: Vec<Leaving>,
    enters: Vec<(Id, Motion)>,
    focused: Option<Id>,
    names: HashMap<&'static str, Id>,
    scroll_nodes: HashMap<u64, (Id, Axis)>,
    stamp: Option<u64>,
    /// Totals since the view was made.
    pub total: Edits,
    /// Updates that built a new tree / that were skipped by their stamp.
    pub builds: u64,
    pub skips: u64,
}

struct Cx<'a> {
    scene: &'a mut Scene,
    ts: &'a mut TextSys,
    now: f64,
    focused: Option<Id>,
    edits: Edits,
    enters: Vec<(Id, Motion)>,
    leaving: Vec<Leaving>,
}

fn effective(el: &El, in_focus: bool) -> (Look, Option<Color>) {
    let look = match (&el.focus_look, in_focus) {
        (Some(f), true) => f.clone(),
        _ => el.look.clone(),
    };
    // `focus_color` is CSS's inherited `color`: it tints text and `currentColor` icons alike.
    let color = match &el.content {
        Content::Text { color, .. } | Content::Image { tint: Some(color), .. } => Some(if in_focus { el.focus_color.unwrap_or(*color) } else { *color }),
        _ => None,
    };
    (look, color)
}

/// Moves a node's text colour or icon tint to `c`, through `tr` when given.
fn ink_to(scene: &mut Scene, id: Id, c: Color, tr: Option<Transition>, now: f64) {
    let Some(n) = scene.tree.get_node_context_mut(id) else { return };
    if let Some(t) = n.text.as_mut() {
        match tr {
            Some(tr) => t.color.set_ease(c, now, tr.dur, tr.color_ease),
            None => t.color.snap(c),
        }
    } else if let Some(img) = n.img.as_mut().filter(|i| i.tint.is_some()) {
        let from = img.ink.as_ref().filter(|k| Some(k.target()) == img.tint).map_or(img.tint.unwrap(), |k| k.get(now));
        let mut k = crate::anim::CAnim::new(from);
        match tr {
            Some(tr) => k.set_ease(c, now, tr.dur, tr.color_ease),
            None => k.snap(c),
        }
        img.tint = Some(c);
        img.ink = Some(k);
    }
}

fn node_for(look: &Look, el: &El) -> Node {
    let mut n = Node::default();
    write_look(&mut n, None, look, None, 0.0);
    if let Some((_, _)) = el.scrolls {
        n.scroll = true;
        n.clip = true;
    }
    n.slot = el.slot;
    if let Content::Texture { tex, w, h } = el.content {
        n = n.image_tex(tex, w, h);
    }
    if let Content::Image { key, tint, contain } = el.content {
        n = Node { img: Node::default().image(key).img, ..n };
        if let Some(s) = n.img.as_mut() {
            s.tint = tint;
            s.contain = contain;
        }
    }
    n
}

/// Writes a look's targets onto a node; animatable properties move through `tr` when given.
/// Returns true when anything changed.
fn write_look(n: &mut Node, old: Option<&Look>, new: &Look, tr: Option<Transition>, now: f64) -> bool {
    if old == Some(new) {
        return false;
    }
    let (dur, ease, cease) = match tr {
        Some(t) if old.is_some() => (t.dur, t.ease, t.color_ease),
        _ => (0.0, Ease::PRO, Ease::PRO),
    };
    let ch = |o: Option<f32>, v: f32| o.is_none_or(|o| (o - v).abs() > 1e-6);
    if old.is_none_or(|o| o.bg != new.bg) {
        if dur > 0.0 {
            n.bg.set_ease(new.bg, now, dur, cease);
        } else {
            n.bg.snap(new.bg);
        }
    }
    let anim = |a: &mut crate::anim::Anim, o: Option<f32>, v: f32| {
        if ch(o, v) {
            if dur > 0.0 {
                a.set_ease(v, now, dur, ease);
            } else {
                a.snap(v);
            }
        }
    };
    anim(&mut n.opacity, old.map(|o| o.opacity), new.opacity);
    anim(&mut n.scale, old.map(|o| o.scale), new.scale);
    anim(&mut n.tx, old.map(|o| o.tx), new.tx);
    anim(&mut n.ty, old.map(|o| o.ty), new.ty);
    n.grad = new.grad;
    if n.lgrad != new.lgrad {
        n.lgrad = new.lgrad.clone();
    }
    n.radius = new.radius;
    n.corners = new.corners;
    n.radius_pct = new.radius_pct;
    n.border = new.border;
    n.borders = new.borders;
    n.border_color = new.border_color;
    if n.shadows != new.shadows {
        n.shadows = new.shadows.clone();
    }
    n.clip = new.clip || n.scroll;
    n.z = new.z;
    n.origin = new.origin;
    n.hidden = new.hidden;
    true
}

fn shift_px(s: Shift, size: f32, rem: f32) -> f32 {
    match s {
        Shift::Px(v) => v,
        Shift::Rem(v) => v * rem,
        Shift::Pct(v) => v * size,
    }
}

/// Starts an enter motion: from its `from` state to the look's values.
fn start_enter(scene: &mut Scene, id: Id, m: &Motion, now: f64) {
    let [_, _, w, h] = scene.rect(id);
    let rem = scene.rem;
    let n = scene.n(id);
    if let Some(o) = m.opacity {
        let to = n.opacity.target();
        n.opacity.restart_ease(o, to, now, m.dur, m.ease);
    }
    if let Some(s) = m.scale {
        let to = n.scale.target();
        n.scale.restart_ease(s, to, now, m.dur, m.ease);
    }
    if let Some(x) = m.x {
        let to = n.tx.target();
        n.tx.restart_ease(to + shift_px(x, w, rem), to, now, m.dur, m.ease);
    }
    if let Some(y) = m.y {
        let to = n.ty.target();
        n.ty.restart_ease(to + shift_px(y, h, rem), to, now, m.dur, m.ease);
    }
}

fn start_exit(scene: &mut Scene, id: Id, m: &Motion, now: f64) -> f64 {
    let [_, _, w, h] = scene.rect(id);
    let rem = scene.rem;
    let n = scene.n(id);
    if let Some(o) = m.opacity {
        n.opacity.set_ease(o, now, m.dur, m.ease);
    }
    if let Some(s) = m.scale {
        n.scale.set_ease(s, now, m.dur, m.ease);
    }
    if let Some(x) = m.x {
        let v = n.tx.target() + shift_px(x, w, rem);
        n.tx.set_ease(v, now, m.dur, m.ease);
    }
    if let Some(y) = m.y {
        let v = n.ty.target() + shift_px(y, h, rem);
        n.ty.set_ease(v, now, m.dur, m.ease);
    }
    now + m.dur as f64
}

impl Cx<'_> {
    fn mount(&mut self, parent: Id, index: usize, mut el: El, k: Key, in_focus: bool) -> M {
        // A new node cannot hold focus yet: it is in focus only through an ancestor.
        let in_focus = in_focus && el.focus.is_none();
        let (look, color) = effective(&el, in_focus);
        let node = node_for(&look, &el);
        let id = match el.portal {
            Some(z) => {
                let layer = self.scene.layer(z);
                self.scene.add(layer, el.style.clone(), node)
            }
            None => self.scene.insert(parent, index, el.style.clone(), node),
        };
        match &el.content {
            Content::Text { text, style, color: c } => {
                self.scene.put_text(self.ts, id, text, *style, color.unwrap_or(*c));
                if let Some(r) = el.roll {
                    self.scene.roll_text(id, text, r.value, r.colors, r.radius, self.now);
                }
            }
            Content::Image { key, tint: Some(_), contain } => self.scene.put_image(id, Some((*key, color, *contain))),
            _ => {}
        }
        self.edits.created += 1;
        if let Some(m) = el.enter {
            self.enters.push((id, m));
        }
        let kid_focus = in_focus;
        let kids_el = std::mem::take(&mut el.kids);
        let mut kids = Vec::with_capacity(kids_el.len());
        for (i, c) in kids_el.into_iter().enumerate() {
            let ck = c.key.unwrap_or(pos_key(i));
            let idx = kids.iter().filter(|m: &&M| m.el.portal.is_none()).count();
            kids.push(self.mount(id, idx, c, ck, kid_focus));
        }
        M { id, key: k, el, kids, look, color, focused: false, in_focus }
    }

    fn unmount(&mut self, parent: Id, index: usize, m: M) {
        match m.el.exit {
            Some(x) if self.scene.try_get(m.id).is_some() => {
                let until = start_exit(self.scene, m.id, &x, self.now);
                let p = if m.el.portal.is_some() { self.scene.parent(m.id).unwrap_or(parent) } else { parent };
                self.leaving.push(Leaving { id: m.id, parent: p, index, until });
            }
            _ => {
                remove_portals(self.scene, &m);
                self.scene.remove(m.id);
            }
        }
        self.edits.removed += 1;
    }

    /// Brings mounted `m` in line with `el`.
    fn patch(&mut self, parent: Id, mut m: M, mut el: El, in_focus: bool) -> M {
        if m.el.portal != el.portal || m.el.scrolls.map(|s| s.0) != el.scrolls.map(|s| s.0) {
            // A different kind of node: replace it.
            let idx = self.scene.children(parent).iter().position(|c| *c == m.id).unwrap_or(usize::MAX);
            let k = m.key;
            remove_portals(self.scene, &m);
            self.scene.remove(m.id);
            self.edits.removed += 1;
            return self.mount(parent, idx, el, k, in_focus);
        }
        let id = m.id;
        if m.el.style != el.style {
            self.scene.set_style(id, el.style.clone());
            self.edits.restyled += 1;
        }
        let focused = self.focused == Some(id);
        let in_focus = if el.focus.is_some() { focused } else { in_focus };
        let tr = el.transition;
        // Unchanged description in the same focus state: nothing to recompute.
        let same = m.in_focus == in_focus && m.el.look == el.look && m.el.focus_look == el.focus_look && m.el.focus_color == el.focus_color;
        let color = if same {
            match (&m.el.content, &el.content) {
                (Content::Text { color: a, .. }, Content::Text { color: b, .. }) if a == b => m.color,
                _ => effective(&el, in_focus).1,
            }
        } else {
            let (look, color) = effective(&el, in_focus);
            if m.look != look {
                if let Some(n) = self.scene.tree.get_node_context_mut(id) {
                    write_look(n, Some(&m.look), &look, tr, self.now);
                }
                self.edits.relooked += 1;
                m.look = look;
            }
            color
        };
        m.in_focus = in_focus;
        self.apply_focus_marks(id, &m.look, focused);
        m.focused = focused;
        if m.el.content != el.content {
            match &el.content {
                Content::Text { text, style, .. } => match &m.el.content {
                    Content::Text { text: t0, style: s0, .. } => {
                        if t0 != text || s0 != style {
                            self.scene.set_text_style(self.ts, id, *style);
                            self.scene.set_text(self.ts, id, text);
                            self.edits.retexted += 1;
                        }
                        if let Some(r) = el.roll {
                            self.scene.roll_text(id, text, r.value, r.colors, r.radius, self.now);
                        }
                    }
                    _ => {
                        self.scene.put_image(id, None);
                        self.scene.put_text(self.ts, id, text, *style, color.unwrap_or(Color::WHITE));
                        self.edits.retexted += 1;
                        m.color = color;
                    }
                },
                Content::Image { key, tint, contain } => {
                    self.scene.clear_text(id);
                    self.scene.put_image(id, Some((*key, tint.and(color), *contain)));
                    self.edits.relooked += 1;
                    m.color = color;
                }
                Content::Texture { tex, w, h } => {
                    self.scene.clear_text(id);
                    self.scene.put_texture(id, *tex, *w, *h);
                    self.edits.relooked += 1;
                    m.color = None;
                }
                Content::None => {
                    self.scene.clear_text(id);
                    self.scene.put_image(id, None);
                    self.edits.relooked += 1;
                    m.color = None;
                }
            }
        }
        if m.color != color {
            if let Some(c) = color {
                ink_to(self.scene, id, c, tr.filter(|_| m.color.is_some()), self.now);
            }
            m.color = color;
        }
        if el.scrolls.is_some() {
            let n = self.scene.n(id);
            n.scroll = true;
            n.clip = true;
        }
        if m.el.slot != el.slot {
            self.scene.n(id).slot = el.slot;
        }
        let kid_focus = in_focus;
        let kids_el = std::mem::take(&mut el.kids);
        let old_kids = std::mem::take(&mut m.kids);
        m.kids = self.patch_kids(id, old_kids, kids_el, kid_focus);
        m.el = el;
        m
    }

    fn apply_focus_marks(&mut self, id: Id, look: &Look, focused: bool) {
        let n = self.scene.n(id);
        let (ring, z) = if focused { (1.0, look.z.max(2)) } else { (0.0, look.z) };
        if n.ring.target() != ring {
            n.ring.snap(ring);
        }
        n.z = z;
    }

    fn patch_kids(&mut self, parent: Id, old: Vec<M>, new: Vec<El>, in_focus: bool) -> Vec<M> {
        self.patch_kids_in(parent, old, new, in_focus, true)
    }

    /// `order`: the view owns `parent`'s children and keeps them in description order (false for
    /// the host, which may hold other nodes: layers, other views).
    fn patch_kids_in(&mut self, parent: Id, old: Vec<M>, new: Vec<El>, in_focus: bool, order: bool) -> Vec<M> {
        // Keyed children match by key. Unkeyed ones match by an in-order alignment of what they
        // are (content kind, focus tag, scroller, name, slot, portal), so a child inserted or
        // removed between two others is a mount or an unmount and the rest keep their nodes.
        let mut old_by_key: HashMap<Key, (usize, M)> = HashMap::with_capacity(old.len());
        let old_count = old.iter().filter(|m| m.el.portal.is_none()).count();
        let mut old_loose: Vec<Option<(usize, M)>> = Vec::new();
        for (i, m) in old.into_iter().enumerate() {
            if m.el.key.is_some() {
                old_by_key.insert(m.key, (i, m));
            } else {
                old_loose.push(Some((i, m)));
            }
        }
        let old_sigs: Vec<u64> = old_loose.iter().map(|o| sig(&o.as_ref().unwrap().1.el)).collect();
        let new_sigs: Vec<u64> = new.iter().filter(|e| e.key.is_none()).map(sig).collect();
        let pairs = align(&old_sigs, &new_sigs);
        let mut loose_i = 0;
        let mut out: Vec<M> = Vec::with_capacity(new.len());
        for (i, el) in new.into_iter().enumerate() {
            let k = el.key.unwrap_or(pos_key(i));
            let found = match el.key {
                Some(_) => old_by_key.remove(&k).map(|(_, m)| m),
                None => {
                    let j = loose_i;
                    loose_i += 1;
                    pairs[j].and_then(|o| old_loose[o].take()).map(|(_, m)| m)
                }
            };
            let m = match found {
                Some(mut m) => {
                    m.key = k;
                    self.patch(parent, m, el, in_focus)
                }
                None => {
                    let idx = if order { out.iter().filter(|m| m.el.portal.is_none()).count() } else { usize::MAX };
                    self.mount(parent, idx, el, k, in_focus)
                }
            };
            out.push(m);
        }
        let mut gone: Vec<(usize, M)> = old_by_key.into_values().chain(old_loose.into_iter().flatten()).collect();
        gone.sort_by_key(|(i, _)| *i);
        for (i, m) in gone {
            self.unmount(parent, i, m);
        }
        if !order {
            return out;
        }
        // Order: the new children, with nodes still leaving at their old places.
        let mut order: Vec<Id> = out.iter().filter(|m| m.el.portal.is_none()).map(|m| m.id).collect();
        let mut leaving: Vec<&Leaving> = self.leaving.iter().filter(|l| l.parent == parent && self.scene.try_get(l.id).is_some()).collect();
        leaving.sort_by_key(|l| l.index);
        for l in leaving {
            if !order.contains(&l.id) {
                order.insert(l.index.min(order.len()), l.id);
            }
        }
        if self.scene.children(parent) != order {
            // Mounting inserts in place, so this only counts real moves (and removals).
            if self.edits.created == 0 || order.len() == old_count {
                self.edits.moved += 1;
            }
            self.scene.set_children(parent, &order);
        }
        out
    }

    /// Re-applies focus-dependent looks without a rebuild.
    fn refocus(&mut self, m: &mut M, in_focus: bool) {
        let focused = self.focused == Some(m.id);
        let in_focus = if m.el.focus.is_some() { focused } else { in_focus };
        let (look, color) = effective(&m.el, in_focus);
        m.in_focus = in_focus;
        let tr = m.el.transition;
        if m.look != look {
            if let Some(n) = self.scene.tree.get_node_context_mut(m.id) {
                write_look(n, Some(&m.look), &look, tr.or(Some(Transition::FOCUS)), self.now);
            }
            m.look = look;
            self.edits.relooked += 1;
        }
        if m.focused != focused || focused {
            self.apply_focus_marks(m.id, &m.look, focused);
            m.focused = focused;
        }
        if m.color != color {
            if let Some(c) = color {
                ink_to(self.scene, m.id, c, Some(tr.unwrap_or(Transition::FOCUS)), self.now);
            }
            m.color = color;
        }
        for k in m.kids.iter_mut() {
            self.refocus(k, in_focus);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct RegCx {
    group: Option<usize>,
    section: Option<[f32; 4]>,
    scroll: Option<u64>,
    rail: Option<u64>,
    scope: u32,
    inert: bool,
}

fn find_sticky(m: &M, scene: &Scene) -> f32 {
    for k in &m.kids {
        if k.el.sticky {
            return scene.rect(k.id)[3];
        }
        if k.el.scrolls.is_none() {
            let h = find_sticky(k, scene);
            if h > 0.0 {
                return h;
            }
        }
    }
    0.0
}

impl View {
    /// A view mounted under `host` (its children are the view's; the host itself is not touched).
    pub fn new(host: Id) -> View {
        View {
            host,
            root: None,
            leaving: Vec::new(),
            enters: Vec::new(),
            focused: None,
            names: HashMap::new(),
            scroll_nodes: HashMap::new(),
            stamp: None,
            total: Edits::default(),
            builds: 0,
            skips: 0,
        }
    }

    pub fn host(&self) -> Id {
        self.host
    }

    /// The root element's node, once mounted.
    pub fn root_id(&self) -> Option<Id> {
        self.root.as_ref().map(|m| m.id)
    }

    /// Diffs `el` against what is mounted and edits the scene to match. Returns what changed.
    pub fn update(&mut self, scene: &mut Scene, ts: &mut TextSys, el: El, now: f64) -> Edits {
        self.builds += 1;
        let mut cx = Cx { scene, ts, now, focused: self.focused, edits: Edits::default(), enters: Vec::new(), leaving: std::mem::take(&mut self.leaving) };
        let old: Vec<M> = self.root.take().into_iter().collect();
        let k = el.key.unwrap_or(pos_key(0));
        let mut el = el;
        el.key = Some(k);
        let mut kids = cx.patch_kids_in(self.host, old, vec![el], false, false);
        self.root = kids.pop();
        self.leaving = cx.leaving;
        let edits = cx.edits;
        self.enters.extend(cx.enters);
        self.total.add(edits);
        self.index();
        edits
    }

    /// `update` only when `stamp` (a store version, a hash of the inputs) differs from the last
    /// one; otherwise the build closure is not even called.
    pub fn update_stamped(&mut self, scene: &mut Scene, ts: &mut TextSys, stamp: u64, now: f64, build: impl FnOnce() -> El) -> Option<Edits> {
        if self.stamp == Some(stamp) && self.root.is_some() {
            self.skips += 1;
            return None;
        }
        self.stamp = Some(stamp);
        Some(self.update(scene, ts, build(), now))
    }

    /// Forgets the stamp so the next `update_stamped` rebuilds.
    pub fn invalidate(&mut self) {
        self.stamp = None;
    }

    fn index(&mut self) {
        self.names.clear();
        self.scroll_nodes.clear();
        fn walk(m: &M, names: &mut HashMap<&'static str, Id>, sc: &mut HashMap<u64, (Id, Axis)>) {
            if let Some(n) = m.el.name {
                names.insert(n, m.id);
            }
            if let Some((a, k)) = m.el.scrolls {
                sc.insert(k, (m.id, a));
            }
            for k in &m.kids {
                walk(k, names, sc);
            }
        }
        if let Some(r) = &self.root {
            walk(r, &mut self.names, &mut self.scroll_nodes);
        }
    }

    /// The node of the element named `n` in the last update.
    pub fn id(&self, n: &str) -> Option<Id> {
        self.names.get(n).copied()
    }

    /// The scroll container with Nav key `k`.
    pub fn scroller(&self, k: u64) -> Option<Id> {
        self.scroll_nodes.get(&k).map(|(id, _)| *id)
    }

    /// Call after `Scene::layout`: starts enter motions that needed the laid-out size and keeps
    /// scroll offsets inside content that shrank.
    pub fn after_layout(&mut self, scene: &mut Scene, now: f64) {
        for (id, m) in std::mem::take(&mut self.enters) {
            if scene.try_get(id).is_some() {
                start_enter(scene, id, &m, now);
            }
        }
        for (id, _) in self.scroll_nodes.values() {
            crate::scroll::clamp(scene, *id);
        }
    }

    /// Enter motions waiting for a layout (call `after_layout` before painting when true).
    pub fn has_pending_enters(&self) -> bool {
        !self.enters.is_empty()
    }

    /// Removes nodes whose exit motion has finished. True when it removed any (relayout).
    pub fn gc(&mut self, scene: &mut Scene, now: f64) -> bool {
        let before = self.leaving.len();
        self.leaving.retain(|l| {
            if now >= l.until {
                scene.remove(l.id);
                false
            } else {
                true
            }
        });
        before != self.leaving.len()
    }

    /// When the next leaving node can be removed.
    pub fn next_deadline(&self) -> Option<f64> {
        self.leaving.iter().map(|l| l.until).reduce(f64::min)
    }

    /// Moves focus to `id` (from `Nav::current`): the ring, z and focus looks follow at once, with
    /// their transitions, and nothing is rebuilt.
    pub fn set_focus(&mut self, scene: &mut Scene, ts: &mut TextSys, id: Option<Id>, now: f64) -> Edits {
        if self.focused == id {
            return Edits::default();
        }
        self.focused = id;
        let mut cx = Cx { scene, ts, now, focused: id, edits: Edits::default(), enters: Vec::new(), leaving: Vec::new() };
        if let Some(r) = self.root.as_mut() {
            cx.refocus(r, false);
        }
        self.total.add(cx.edits);
        cx.edits
    }

    pub fn focused(&self) -> Option<Id> {
        self.focused
    }

    /// Hands this view's focusables, groups and scroll containers to `nav`, in document order, at
    /// their current on-screen rects. Call `nav.begin()` first (once for all views) and
    /// `nav.ensure(now)` after.
    pub fn register(&self, scene: &Scene, nav: &mut Nav, now: f64) {
        if let Some(r) = &self.root {
            Self::reg(r, scene, nav, now, RegCx::default());
        }
    }

    fn reg(m: &M, scene: &Scene, nav: &mut Nav, now: f64, mut c: RegCx) {
        if scene.try_get(m.id).is_none_or(|n| n.hidden) {
            return;
        }
        let el = &m.el;
        if el.inert {
            c.inert = true;
        }
        if let Some(s) = el.scope {
            c.scope = s;
        }
        let rect = scene.window_rect(m.id, now);
        if el.section {
            c.section = Some(rect);
        }
        if let Some((axis, k)) = el.scrolls {
            let max = scene.max_scroll(m.id);
            let n = scene.get(m.id);
            let pad = scene.padding(m.id);
            nav.add_scroller(Scroller {
                key: k,
                rect,
                content: [rect[2] + max[0], rect[3] + max[1]],
                offset: [n.scroll_x.get(now), n.scroll_y.get(now)],
                pad_left: pad[3],
                start: scene.min_scroll(m.id),
                sticky: find_sticky(m, scene),
                parent: match axis {
                    Axis::X => c.rail,
                    Axis::Y => c.scroll,
                },
            });
            match axis {
                Axis::X => c.rail = Some(k),
                Axis::Y => c.scroll = Some(k),
            }
        }
        if let Some(g) = &el.group {
            let mut g = g.clone();
            g.rect = rect;
            if g.section.is_none() {
                g.section = c.section;
            }
            c.group = Some(nav.add_group(g));
        }
        if let Some(f) = &el.focus {
            let mut f = f.clone();
            f.id = m.id;
            f.rect = rect;
            if f.group.is_none() {
                f.group = c.group;
            }
            if f.scope == 0 {
                f.scope = c.scope;
            }
            if f.scroll.is_none() {
                f.scroll = c.scroll;
            }
            if f.rail.is_none() {
                f.rail = c.rail;
            }
            f.inert |= c.inert;
            nav.add(f);
        }
        for k in &m.kids {
            Self::reg(k, scene, nav, now, c);
        }
    }

    /// Applies `Nav::take_scrolls` output to this view's scroll containers (keys it does not own
    /// are left for other views) and tells `nav` the new offsets.
    pub fn apply_scrolls(&self, scene: &mut Scene, nav: &mut Nav, scrolls: &[(u64, [f32; 2])]) {
        for (k, o) in scrolls {
            if let Some((id, _)) = self.scroll_nodes.get(k) {
                let (min, max) = (scene.min_scroll(*id), scene.max_scroll(*id));
                let o = [o[0].clamp(min[0], max[0]), o[1].clamp(min[1], max[1])];
                let n = scene.n(*id);
                n.scroll_x.snap(o[0]);
                n.scroll_y.snap(o[1]);
                nav.scrolled(*k, o);
            }
        }
    }

    /// Number of nodes the view has mounted (leaving ones excluded).
    pub fn len(&self) -> usize {
        fn count(m: &M) -> usize {
            1 + m.kids.iter().map(count).sum::<usize>()
        }
        self.root.as_ref().map(count).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Unmounts everything (exit motions are skipped).
    pub fn clear(&mut self, scene: &mut Scene) {
        if let Some(r) = self.root.take() {
            remove_portals(scene, &r);
            scene.remove(r.id);
        }
        for l in self.leaving.drain(..) {
            scene.remove(l.id);
        }
        self.enters.clear();
        self.names.clear();
        self.scroll_nodes.clear();
        self.stamp = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::Focusable;

    static INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");

    fn sys() -> (Scene, TextSys) {
        (Scene::new(400.0, 300.0, 16.0), TextSys::new(&[INTER], false, 256))
    }

    fn sz(w: f32, h: f32) -> Style {
        Style { size: Size { width: length(w), height: length(h) }, flex_shrink: 0.0, ..Default::default() }
    }

    fn list(items: &[u32]) -> El {
        El::col().kids(items.iter().map(|i| El::new(sz(100.0, 20.0)).bg(Color::rgb(*i as u8, 0, 0)).key(key(i))))
    }

    fn plain(items: &[u32]) -> El {
        El::col().kids(items.iter().map(|i| El::new(sz(100.0, 20.0)).bg(Color::rgb(*i as u8, 0, 0))))
    }

    fn mixed(items: &[(u32, bool)]) -> El {
        El::col().kids(items.iter().map(|(i, k)| {
            let e = El::new(sz(100.0, 20.0)).bg(Color::rgb(*i as u8, 0, 0));
            if *k { e.key(key(i)) } else { e }
        }))
    }

    #[test]
    fn unkeyed_button_inserted_between_siblings_keeps_the_others() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let btn = |tag: u32, label: &str| {
            El::row().child(El::text(label, TextStyle::new(16.0, 400), Color::WHITE)).focusable(Focusable::new(NodeId::from(0u64), [0.0; 4]).tag(tag))
        };
        let row = |with_cart: bool| {
            let mut r = El::row().child(btn(1, "Buy now"));
            if with_cart {
                r = r.child(btn(2, "Add to cart"));
            }
            r.child(btn(3, "Open"))
        };
        v.update(&mut s, &mut ts, row(false), 0.0);
        let r = v.root_id().unwrap();
        let before = s.children(r);
        let e = v.update(&mut s, &mut ts, row(true), 0.0);
        s.layout(&mut ts);
        let after = s.children(r);
        assert_eq!(after.len(), 3);
        assert_eq!((after[0], after[2]), (before[0], before[1]), "Buy and Open keep their nodes");
        assert_eq!(e.created, 2, "only the inserted button and its label are created");
        v.update(&mut s, &mut ts, row(false), 0.0);
        assert_eq!(s.children(r), vec![before[0], before[1]]);
    }

    #[test]
    fn unkeyed_child_inserted_between_others_is_kept() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        v.update(&mut s, &mut ts, plain(&[1, 2]), 0.0);
        let col = v.root_id().unwrap();
        v.update(&mut s, &mut ts, plain(&[1, 5, 2]), 0.0);
        assert_eq!(s.children(col).len(), 3);
        v.update(&mut s, &mut ts, plain(&[1, 2]), 0.0);
        assert_eq!(s.children(col).len(), 2);
        // Unkeyed inserts around keyed siblings.
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        v.update(&mut s, &mut ts, mixed(&[(1, true), (2, false), (3, true)]), 0.0);
        let col = v.root_id().unwrap();
        let ids = s.children(col);
        v.update(&mut s, &mut ts, mixed(&[(1, true), (9, false), (2, false), (3, true)]), 0.0);
        let now = s.children(col);
        assert_eq!(now.len(), 4);
        assert_eq!((now[0], now[3]), (ids[0], ids[2]), "keyed siblings keep their nodes");
        v.update(&mut s, &mut ts, mixed(&[(8, false), (1, true), (9, false), (2, false), (3, true)]), 0.0);
        assert_eq!(s.children(col).len(), 5);
    }

    fn shape(s: &Scene, id: Id) -> String {
        let kids: Vec<String> = s.children(id).iter().map(|c| shape(s, *c)).collect();
        format!("({})", kids.join(""))
    }

    fn shape_el(e: &El) -> String {
        let kids: Vec<String> = e.kids.iter().map(shape_el).collect();
        format!("({})", kids.join(""))
    }

    fn rand_el(r: &mut u64, depth: u32) -> El {
        let mut next = |n: u64| {
            *r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (*r >> 33) % n
        };
        let mut e = match next(3) {
            0 => El::text("t", TextStyle::new(16.0, 400), Color::WHITE),
            1 => El::new(sz(50.0, 20.0)).bg(Color::WHITE),
            _ => El::row(),
        };
        if depth > 0 && matches!(e.content, Content::None) {
            let n = next(4);
            for _ in 0..n {
                let k = rand_el(r, depth - 1);
                let keyed = {
                    *r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (*r >> 33) % 4 == 0
                };
                let kk = (*r >> 40) % 6;
                e = e.child(if keyed { k.key(key(&kk)) } else { k });
            }
        }
        e
    }

    #[test]
    fn random_trees_always_mirror_the_description() {
        for seed in 0..300u64 {
            let (mut s, mut ts) = sys();
            let mut v = View::new(s.root);
            let mut r = seed.wrapping_mul(7919) + 1;
            for step in 0..12 {
                let mut e = El::col();
                let n = (r >> 20) % 5;
                for _ in 0..n {
                    e = e.child(rand_el(&mut r, 2));
                }
                r = r.wrapping_add(step);
                let want = shape_el(&e);
                v.update(&mut s, &mut ts, e, 0.0);
                s.layout(&mut ts);
                assert_eq!(shape(&s, v.root_id().unwrap()), want, "seed {seed} step {step}");
            }
        }
    }

    #[test]
    fn live_text_rolls_when_it_changes() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let num = |t: &str, x: f64| El::text(t, TextStyle::new(16.0, 400), Color::WHITE).roll(x, Color::WHITE, Color::BLACK, 4.0);
        v.update(&mut s, &mut ts, num("$1.20", 1.2), 0.0);
        s.layout(&mut ts);
        assert!(!s.animating(0.0), "the first value only sets the number");
        v.update(&mut s, &mut ts, num("$1.25", 1.25), 1.0);
        assert!(s.animating(1.05), "a changed value rolls");
        assert!(!s.animating(2.0), "and settles");
    }

    #[test]
    fn same_tree_is_free_and_keyed_insert_moves_nodes() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let e = v.update(&mut s, &mut ts, list(&[1, 2, 3]), 0.0);
        assert_eq!(e.created, 4);
        let col = v.root_id().unwrap();
        let ids = s.children(col);
        assert_eq!(ids.len(), 3);
        // The same description: no edits at all.
        let e = v.update(&mut s, &mut ts, list(&[1, 2, 3]), 0.0);
        assert_eq!(e, Edits::default());
        // Insert at the top: one node created, the others kept (same ids) and moved.
        let e = v.update(&mut s, &mut ts, list(&[0, 1, 2, 3]), 0.0);
        assert_eq!((e.created, e.removed, e.relooked), (1, 0, 0));
        let ids2 = s.children(col);
        assert_eq!(&ids2[1..], &ids[..]);
        // Remove the middle one.
        let e = v.update(&mut s, &mut ts, list(&[0, 1, 3]), 0.0);
        assert_eq!(e.removed, 1);
        assert_eq!(s.children(col), vec![ids2[0], ids[0], ids[2]]);
        // Reorder.
        v.update(&mut s, &mut ts, list(&[3, 0, 1]), 0.0);
        assert_eq!(s.children(col), vec![ids[2], ids2[0], ids[0]]);
    }

    #[test]
    fn look_changes_never_relayout_style_changes_do() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let mk = |c: Color, w: f32| El::new(sz(w, 10.0)).bg(c).transition(Transition::pro(0.2));
        v.update(&mut s, &mut ts, mk(Color::BLACK, 10.0), 0.0);
        s.layout(&mut ts);
        let e = v.update(&mut s, &mut ts, mk(Color::WHITE, 10.0), 1.0);
        assert!(!e.layout() && e.relooked == 1);
        assert!(!s.needs_layout());
        let id = v.root_id().unwrap();
        // Transitions from the old colour.
        assert!(s.get(id).bg.active(1.1));
        assert_eq!(s.get(id).bg.get(1.3), Color::WHITE);
        let e = v.update(&mut s, &mut ts, mk(Color::WHITE, 20.0), 2.0);
        assert!(e.layout() && e.restyled == 1);
        assert!(s.needs_layout());
    }

    #[test]
    fn text_updates_in_place() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let st = TextStyle::new(14.0, 400);
        v.update(&mut s, &mut ts, El::text("a", st, Color::WHITE), 0.0);
        let id = v.root_id().unwrap();
        let e = v.update(&mut s, &mut ts, El::text("b", st, Color::WHITE), 0.0);
        assert_eq!((e.retexted, e.created), (1, 0));
        assert_eq!(v.root_id(), Some(id));
        assert_eq!(s.get(id).text.as_ref().unwrap().text, "b");
        let e = v.update(&mut s, &mut ts, El::text("b", st, Color::BLACK), 0.0);
        assert_eq!((e.retexted, e.relooked), (0, 0));
        assert_eq!(s.get(id).text.as_ref().unwrap().color.target(), Color::BLACK);
    }

    #[test]
    fn focus_draws_ring_and_focus_looks_without_rebuild() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let st = TextStyle::new(14.0, 400);
        let btn = |n: &'static str| {
            El::row()
                .bg(Color::rgb(30, 25, 41))
                .focus_look(Look::default().bg(Color::WHITE))
                .focusable(Focusable::new(NodeId::from(0u64), [0.0; 4]))
                .name(n)
                .child(El::text(n, st, Color::WHITE).focus_color(Color::BLACK).name(if n == "a" { "a_label" } else { "b_label" }))
        };
        v.update(&mut s, &mut ts, El::row().kids([btn("a"), btn("b")]), 0.0);
        let (a, al) = (v.id("a").unwrap(), v.id("a_label").unwrap());
        let e = v.set_focus(&mut s, &mut ts, Some(a), 1.0);
        assert!(!e.layout());
        assert_eq!(s.get(a).ring.target(), 1.0);
        assert_eq!(s.get(a).z, 2);
        assert_eq!(s.get(a).bg.target(), Color::WHITE);
        assert!(s.get(a).bg.active(1.05), "110 ms transition");
        assert_eq!(s.get(al).text.as_ref().unwrap().color.target(), Color::BLACK);
        // A rebuild keeps the focus look.
        v.update(&mut s, &mut ts, El::row().kids([btn("a"), btn("b")]), 2.0);
        assert_eq!(s.get(a).bg.target(), Color::WHITE);
        // Moving on restores it.
        let b = v.id("b").unwrap();
        v.set_focus(&mut s, &mut ts, Some(b), 3.0);
        assert_eq!(s.get(a).ring.target(), 0.0);
        assert_eq!(s.get(a).z, 0);
        assert_eq!(s.get(a).bg.target(), Color::rgb(30, 25, 41));
        assert_eq!(s.get(al).text.as_ref().unwrap().color.target(), Color::WHITE);
        assert_eq!(s.get(b).ring.target(), 1.0);
    }

    #[test]
    fn focus_color_tints_icons_too() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let btn = || {
            El::row()
                .focus_look(Look::default().bg(Color::WHITE))
                .focusable(Focusable::new(NodeId::from(0u64), [0.0; 4]))
                .name("b")
                .child(El::icon(7, Color::WHITE).focus_color(Color::BLACK).name("i"))
        };
        v.update(&mut s, &mut ts, btn(), 0.0);
        let (b, i) = (v.id("b").unwrap(), v.id("i").unwrap());
        assert_eq!(s.get(i).img.as_ref().unwrap().tint, Some(Color::WHITE));
        v.set_focus(&mut s, &mut ts, Some(b), 1.0);
        let img = s.get(i).img.as_ref().unwrap();
        assert_eq!(img.tint, Some(Color::BLACK));
        assert!(img.ink.as_ref().unwrap().active(1.05), "eases with the label");
        v.update(&mut s, &mut ts, btn(), 2.0);
        assert_eq!(s.get(i).img.as_ref().unwrap().tint, Some(Color::BLACK));
        v.set_focus(&mut s, &mut ts, None, 3.0);
        assert_eq!(s.get(i).img.as_ref().unwrap().tint, Some(Color::WHITE));
    }

    #[test]
    fn enter_and_exit_motions() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let toast = || El::new(sz(200.0, 40.0)).bg(Color::WHITE).enter(Motion::POP).exit(Motion::FADE).key(key("t"));
        v.update(&mut s, &mut ts, El::col().child(toast()), 0.0);
        s.layout(&mut ts);
        v.after_layout(&mut s, 0.0);
        let t = s.children(v.root_id().unwrap())[0];
        assert_eq!(s.get(t).opacity.get(0.0), 0.0);
        assert!((s.get(t).scale.get(0.0) - 0.96).abs() < 1e-6);
        assert_eq!(s.get(t).opacity.get(0.4), 1.0);
        // Gone from the description: fades, stays in place, then is removed.
        v.update(&mut s, &mut ts, El::col(), 1.0);
        assert_eq!(s.children(v.root_id().unwrap()), vec![t]);
        assert!(s.get(t).opacity.get(1.1) < 1.0);
        assert_eq!(v.next_deadline(), Some(1.0 + motion::BASE as f64));
        assert!(!v.gc(&mut s, 1.1));
        assert!(v.gc(&mut s, 1.25));
        assert!(s.children(v.root_id().unwrap()).is_empty());
        assert!(s.try_get(t).is_none());
    }

    #[test]
    fn slide_in_right_uses_the_laid_out_width() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        v.update(&mut s, &mut ts, El::new(sz(120.0, 300.0)).enter(Motion::SLIDE_IN_RIGHT).portal(crate::scene::layer::SHEET), 0.0);
        let id = v.root_id().unwrap();
        assert_eq!(s.layer_of(id), crate::scene::layer::SHEET);
        assert!(v.has_pending_enters());
        s.layout(&mut ts);
        v.after_layout(&mut s, 0.0);
        assert_eq!(s.get(id).tx.get(0.0), 120.0);
        assert_eq!(s.get(id).tx.get(0.3), 0.0);
    }

    fn fill() -> Style {
        let z = length(0.0);
        Style { position: Position::Absolute, inset: Rect { left: z, right: z, top: z, bottom: z }, ..Default::default() }
    }

    fn abs(w: f32, h: f32) -> Style {
        Style { position: Position::Absolute, ..sz(w, h) }
    }

    /// The boot splash on the device: a layer (absolute, so the containing block of its absolute
    /// children) loses its last child. taffy kept the layer's hoisted child list from when it had
    /// children, and the next layout's rounding pass read the removed id ("invalid SlotMap key").
    #[test]
    fn removing_a_layers_last_absolute_child_then_layout() {
        let (mut s, mut ts) = sys();
        s.add(s.root, sz(100.0, 50.0), Node::default());
        let l = s.layer(80);
        let splash = s.add(l, fill(), Node::default());
        s.add(splash, abs(10.0, 10.0), Node::default());
        s.layout(&mut ts);
        s.remove(splash);
        s.layout(&mut ts);
        // And back again: a new child is laid out against the layer.
        let again = s.add(l, fill(), Node::default());
        s.layout(&mut ts);
        assert_eq!(s.rect(again), [0.0, 0.0, 400.0, 300.0]);
    }

    /// WS 14: an absolute side border (`top: 0; bottom: 0; width: 1px`) inside a flex column is
    /// as tall as the column, measured or not, positioned parent or static.
    #[test]
    fn absolute_child_sizes_against_its_containing_block() {
        for pos in [Position::Relative, Position::Static] {
            let (mut s, mut ts) = sys();
            let z = length(0.0);
            let outer = s.add(s.root, Style { size: Size { width: auto(), height: length(200.0) }, ..Default::default() }, Node::default());
            let row = s.add(outer, Style { flex_direction: FlexDirection::Row, padding: Rect { left: length(16.0), right: length(16.0), top: length(16.0), bottom: length(16.0) }, ..Default::default() }, Node::default());
            s.add(row, Style { flex_grow: 1.0, ..sz(10.0, 20.0) }, Node::default());
            let col = s.add(row, Style { position: pos, flex_grow: 1.0, flex_direction: FlexDirection::Column, padding: Rect { left: length(16.0), right: z, top: z, bottom: z }, ..Default::default() }, Node::default());
            s.add(col, sz(30.0, 18.0), Node::default());
            s.add(col, sz(30.0, 18.0), Node::default());
            let hair = s.add(col, Style { position: Position::Absolute, inset: Rect { left: length(0.0), top: length(0.0), bottom: length(0.0), right: auto() }, size: Size { width: length(1.0), height: auto() }, ..Default::default() }, Node::default());
            s.layout(&mut ts);
            let want = s.rect(col);
            let got = s.rect(hair);
            assert_eq!((got[0], got[1], got[3]), (want[0], want[1], want[3]), "{pos:?}: hair {got:?} col {want:?}");
            // A relayout, and a restyle of the parent, keep it on the column.
            s.mark_dirty();
            s.layout(&mut ts);
            assert_eq!(s.rect(hair), got);
            s.set_style(col, Style { position: pos, ..s.style(col) });
            s.layout(&mut ts);
            assert_eq!(s.rect(hair), got);
        }
    }

    /// The same through a view: a portal (a sheet, an overlay) unmounts and leaves its layer empty.
    #[test]
    fn unmounting_a_portal_that_empties_its_layer_then_layout() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let sheet = || El::new(fill()).portal(crate::scene::layer::SHEET).child(El::new(abs(120.0, 300.0)).child(El::new(abs(20.0, 20.0))));
        v.update(&mut s, &mut ts, El::col().child(El::new(sz(50.0, 50.0))).child(sheet().key(key("sheet"))), 0.0);
        s.layout(&mut ts);
        v.update(&mut s, &mut ts, El::col().child(El::new(sz(50.0, 50.0))), 1.0);
        s.layout(&mut ts);
        v.update(&mut s, &mut ts, El::col().child(El::new(sz(50.0, 50.0))).child(sheet().key(key("sheet"))), 2.0);
        s.layout(&mut ts);
    }

    /// A sheet (a portal) inside a page's portal goes with the page: on clear and on unmount.
    #[test]
    fn nested_portals_go_with_their_page() {
        let (mut s, mut ts) = sys();
        let layer_kids = |s: &mut Scene| {
            let (a, b) = (s.layer(crate::scene::layer::SHEET), s.layer(30));
            s.children(a).len() + s.children(b).len()
        };
        let page = || El::new(fill()).portal(30).child(El::new(sz(50.0, 50.0))).child(El::new(fill()).portal(crate::scene::layer::SHEET).child(El::new(sz(20.0, 20.0))));
        let mut v = View::new(s.root);
        v.update(&mut s, &mut ts, El::col().child(page().key(key("page"))), 0.0);
        s.layout(&mut ts);
        assert_eq!(layer_kids(&mut s), 2);
        v.update(&mut s, &mut ts, El::col().child(El::new(sz(50.0, 50.0))), 1.0);
        s.layout(&mut ts);
        assert_eq!(layer_kids(&mut s), 0, "unmounted with its sheet");
        v.update(&mut s, &mut ts, El::col().child(page().key(key("page"))), 2.0);
        s.layout(&mut ts);
        assert_eq!(layer_kids(&mut s), 2);
        v.clear(&mut s);
        s.layout(&mut ts);
        assert_eq!(layer_kids(&mut s), 0, "cleared with its sheet");
    }

    /// A containing block that is `display: none` (or inside one) when its absolute child goes.
    #[test]
    fn removing_an_absolute_box_under_a_hidden_container_then_layout() {
        let (mut s, mut ts) = sys();
        let outer = s.add(s.root, Style { position: Position::Relative, ..sz(200.0, 200.0) }, Node::default());
        let cb = s.add(outer, Style { position: Position::Relative, ..sz(100.0, 100.0) }, Node::default());
        let a = s.add(cb, abs(10.0, 10.0), Node::default());
        let b = s.add(cb, abs(10.0, 10.0), Node::default());
        s.layout(&mut ts);
        // Hide the containing block itself, then its ancestor, removing one box each time.
        s.set_style(cb, Style { display: Display::None, ..s.style(cb) });
        s.remove(a);
        s.layout(&mut ts);
        s.set_style(cb, Style { display: Display::Flex, ..s.style(cb) });
        s.set_style(outer, Style { display: Display::None, ..s.style(outer) });
        s.layout(&mut ts);
        s.remove(b);
        s.layout(&mut ts);
        s.set_style(outer, Style { display: Display::Flex, ..s.style(outer) });
        s.layout(&mut ts);
    }

    /// Removed ids are inert: a leaving node removed with its parent, then collected or cleared.
    #[test]
    fn removed_ids_are_never_touched_again() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let toast = || El::new(abs(200.0, 40.0)).exit(Motion::FADE).key(key("t"));
        v.update(&mut s, &mut ts, El::col().child(El::col().key(key("box")).child(toast())), 0.0);
        s.layout(&mut ts);
        let t = s.children(s.children(v.root_id().unwrap())[0])[0];
        // The toast starts leaving, then its parent goes (no exit motion: removed at once).
        v.update(&mut s, &mut ts, El::col().child(El::col().key(key("box"))), 1.0);
        v.update(&mut s, &mut ts, El::col(), 1.05);
        assert!(!s.alive(t));
        s.layout(&mut ts);
        assert!(v.gc(&mut s, 5.0));
        s.layout(&mut ts);
        s.set_children(s.root, &[t]);
        s.set_style(t, sz(1.0, 1.0));
        v.clear(&mut s);
        v.clear(&mut s);
        s.layout(&mut ts);
    }

    #[test]
    fn stores_version_their_changes() {
        let mut st = Store::new(vec![1, 2]);
        let v0 = st.version();
        assert!(!st.replace(vec![1, 2]));
        assert_eq!(st.version(), v0);
        assert!(st.replace(vec![1, 2, 3]));
        st.update(|v| v.push(4));
        assert_eq!(st.version(), v0 + 2);
        assert_ne!(stamp(&[v0, 1]), stamp(&[v0 + 1, 1]));
    }

    #[test]
    fn stamped_updates_skip_the_build() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let mut built = 0;
        for stamp in [1, 1, 1, 2, 2] {
            v.update_stamped(&mut s, &mut ts, stamp, 0.0, || {
                built += 1;
                list(&[1])
            });
        }
        assert_eq!(built, 2);
        assert_eq!(v.skips, 3);
    }

    #[test]
    fn registers_with_nav_and_follows_its_scrolls() {
        let (mut s, mut ts) = sys();
        let mut v = View::new(s.root);
        let row = |i: u32| El::new(sz(380.0, 100.0)).key(key(&i)).focusable(Focusable::new(NodeId::from(0u64), [0.0; 4]).tag(i));
        let page = El::col()
            .with_style(Style { flex_direction: FlexDirection::Column, size: Size { width: length(400.0), height: length(300.0) }, ..Default::default() })
            .scrolls_y(7)
            .group(Group::new("rows", [0.0; 4]))
            .kids((0..6).map(row));
        v.update(&mut s, &mut ts, page, 0.0);
        s.layout(&mut ts);
        v.after_layout(&mut s, 0.0);
        let mut nav = Nav::new(400.0, 300.0);
        nav.begin();
        v.register(&s, &mut nav, 0.0);
        nav.ensure(0.0);
        nav.ensure(0.1);
        assert_eq!(nav.items.len(), 6);
        assert_eq!(nav.scrollers.len(), 1);
        // 600 px of rows plus the ring's room at the end (0.3125 rem).
        assert_eq!(nav.scrollers[0].content[0], 400.0);
        assert!((nav.scrollers[0].content[1] - (600.0 + crate::theme::ring::OUTER * s.rem)).abs() < 0.01);
        assert!(nav.items.iter().all(|i| i.scroll == Some(7) && i.group == Some(0)));
        let first = nav.current().unwrap();
        v.set_focus(&mut s, &mut ts, Some(first), 0.0);
        // Walk down to the last row: Nav scrolls the page, the view applies it.
        let mut t = 0.0;
        for _ in 0..5 {
            t += 0.2;
            nav.move_dir(crate::focus::Dir::Down, t);
            for _ in 0..20 {
                t += 0.016;
                nav.tick(t);
                let sc = nav.take_scrolls();
                v.apply_scrolls(&mut s, &mut nav, &sc);
            }
        }
        let page_id = v.scroller(7).unwrap();
        let end = 300.0 + crate::theme::ring::OUTER * s.rem;
        assert!((s.get(page_id).scroll_y.get(t) - end).abs() < 0.01, "scrolled to the end, the ring's room included");
        let cur = nav.current().unwrap();
        assert_eq!(nav.item(cur).unwrap().tag, 5);
        let r = s.window_rect(cur, t);
        let room = crate::theme::ring::OUTER * s.rem;
        assert!(r[1] >= 0.0 && r[1] + r[3] + room <= 300.0 + 0.01, "focused row and its ring on screen: {r:?}");
    }
}
