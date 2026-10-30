//! The retained scene: a Taffy tree whose nodes carry their look (fill, gradient, radius, border,
//! shadow, image, text) and their animated properties (opacity, scale about an origin, translate,
//! scroll offset, focus ring, colours). Layout runs only when structure, style or text changes;
//! motion is transform, opacity and scroll, evaluated at paint time, so animating never relayouts
//! (DESIGN.md's budget rule, enforced by construction).
//!
//! Stacking mirrors the web launcher's z-index: children paint in `z` order within their parent
//! (ties keep tree order), and `Scene::layer(z)` gives full-window roots above the page for pages,
//! sheets, toasts and modals (`layer::PAGE` .. `layer::MODAL`, the same numbers as the Tailwind
//! `z-30` .. `z-[60]` they replace). A layer is never clipped by the page under it.

use crate::anim::{Anim, CAnim};
use crate::color::Color;
use crate::gpu::{DrawList, Prim, KIND_GRADN, KIND_IMAGE, KIND_MASK, KIND_RECT, KIND_RING, KIND_SHADOW};
use crate::images::{ImageLoader, ImgState};
use crate::text::{TextPaint, TextStyle, TextSys};
use taffy::prelude::*;
use taffy::TraversePartialTree;

pub type Id = NodeId;

/// z values of the full-window layers (the web launcher's z-index classes).
pub mod layer {
    /// The page itself (the root's own children).
    pub const BASE: i16 = 0;
    /// Full-screen pages over the tabs: GameDetail, profile, store pages (`z-30`).
    pub const PAGE: i16 = 30;
    /// Sheets, dialogs, Quick Settings, the Rare menu (`z-40`).
    pub const SHEET: i16 = 40;
    /// Toasts and popups (`z-50`).
    pub const TOAST: i16 = 50;
    /// On-screen keyboard, Steam handoff (`z-[60]`).
    pub const MODAL: i16 = 60;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradDir {
    ToBottom,
    ToTop,
    ToRight,
    ToLeft,
}

/// A plain linear scrim: three stops (Tailwind's from / via / to), `mid` is where `via` sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    pub dir: GradDir,
    pub stops: [Color; 3],
    pub mid: f32,
}

impl Gradient {
    /// The same scrim as a `LinearGradient` (Tailwind v4 gradients interpolate in oklab).
    pub fn linear(&self) -> LinearGradient {
        let angle = match self.dir {
            GradDir::ToTop => 0.0,
            GradDir::ToRight => 90.0,
            GradDir::ToBottom => 180.0,
            GradDir::ToLeft => 270.0,
        };
        LinearGradient { angle, stops: vec![(self.stops[0], 0.0), (self.stops[1], self.mid), (self.stops[2], 1.0)], oklab: true }
    }
}

/// CSS `linear-gradient(<angle>, <stops>)`: any number of stops at positions 0..1 along the
/// gradient line (CSS angles: 0 = to top, 90 = to right, 180 = to bottom), interpolated in
/// premultiplied Oklab (`in oklab`, every Tailwind v4 gradient) or premultiplied sRGB (a plain
/// `bg-[linear-gradient(...)]`).
#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    pub angle: f32,
    pub stops: Vec<(Color, f32)>,
    pub oklab: bool,
}

impl LinearGradient {
    /// An oklab gradient with no stops yet (`.stop(c, at)` adds them in order).
    pub fn new(angle_deg: f32) -> LinearGradient {
        LinearGradient { angle: angle_deg, stops: Vec::new(), oklab: true }
    }
    /// Tailwind `bg-linear-to-*` / `bg-gradient-to-*` from `from`, `via` (at 50 %) and `to`.
    pub fn tw(angle_deg: f32, from: Color, via: Option<Color>, to: Color) -> LinearGradient {
        let mut g = LinearGradient::new(angle_deg).stop(from, 0.0);
        if let Some(v) = via {
            g = g.stop(v, 0.5);
        }
        g.stop(to, 1.0)
    }
    pub fn stop(mut self, c: Color, at: f32) -> LinearGradient {
        self.stops.push((c, at));
        self
    }
    /// Interpolate in sRGB instead (CSS's default for a gradient without `in oklab`).
    pub fn srgb(mut self) -> LinearGradient {
        self.oklab = false;
        self
    }
}

/// One CSS box-shadow layer in px. DESIGN.md allows exactly one use: the static shadow under
/// toasts and sheets (never on cards, never animated; it fades with its node's opacity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}

impl Shadow {
    pub const fn new(x: f32, y: f32, blur: f32, spread: f32, color: Color) -> Shadow {
        Shadow { x, y, blur, spread, color }
    }

    /// Tailwind `ring-<w> ring-<c>`: a solid band of `w` px outside the box (box-shadow
    /// `0 0 0 w c`), listed with the other shadow layers.
    pub const fn ring(w: f32, c: Color) -> Shadow {
        Shadow::new(0.0, 0.0, 0.0, w, c)
    }

    /// Tailwind `shadow-xl shadow-black/30` (toasts, trophy popups).
    pub const fn xl(a: f32) -> [Shadow; 2] {
        let c = Color::BLACK.alpha(a);
        [Shadow::new(0.0, 20.0, 25.0, -5.0, c), Shadow::new(0.0, 8.0, 10.0, -6.0, c)]
    }

    /// Tailwind `shadow-2xl shadow-black/40` (sheets, review dialogs).
    pub const fn xl2(a: f32) -> [Shadow; 1] {
        [Shadow::new(0.0, 25.0, 50.0, -12.0, Color::BLACK.alpha(a))]
    }

    /// A side sheet's edge shadow in rem: `shadow-[-2rem_0_4rem_rgb(0_0_0/0.45)]` is
    /// `Shadow::rem(-2.0, 0.0, 4.0, 0.45, rem)` (Quick Settings, the Rare menu).
    pub fn rem(x: f32, y: f32, blur: f32, a: f32, rem: f32) -> Shadow {
        Shadow::new(x * rem, y * rem, blur * rem, 0.0, Color::BLACK.alpha(a))
    }
}

pub struct ImgSlot {
    pub key: u64,
    /// Draw the image's alpha in this colour (icons).
    pub tint: Option<Color>,
    /// The tint while it transitions (a focused button's icon turning dark with its label); paint
    /// follows it while its target is still `tint`, so code that sets `tint` directly wins.
    pub ink: Option<CAnim>,
    /// Contain fit: where the image sits in the box (0,0 top-left .. 1,1 bottom-right).
    pub contain: Option<[f32; 2]>,
    pub fade: Anim,
    pub seen: bool,
    /// The slot has been through one `sync_images`; an image already on the GPU by then shows
    /// at once (a revisited game's art) instead of fading in.
    pub waited: bool,
    /// A texture of the app's own (`Gpu::create_stream`: video, camera) and its pixel size, drawn
    /// instead of the loader's image for `key`; it shows at once, with no fade.
    pub tex: Option<(crate::gpu::TexId, u32, u32)>,
}

pub struct TextSlot {
    pub buf: cosmic_text::Buffer,
    pub style: TextStyle,
    pub color: CAnim,
    pub text: String,
    /// A live number: its roll, the flash's up and down colours and corner radius.
    pub roll: Option<Box<(crate::text::roll::NumRoll, [Color; 2], f32)>>,
}

pub struct Node {
    pub bg: CAnim,
    pub grad: Option<Gradient>,
    /// Painted after `grad`: an N-stop gradient at any angle.
    pub lgrad: Option<LinearGradient>,
    pub radius: f32,
    /// Per-corner radii [tl, tr, br, bl] in px; overrides `radius` when set.
    pub corners: Option<[f32; 4]>,
    /// Radius as a fraction of the box's shorter side (`rounded-[22%]` is 0.22); overrides both.
    pub radius_pct: f32,
    pub border: f32,
    /// Per-side border widths [top, right, bottom, left]; overrides `border` when set.
    pub borders: Option<[f32; 4]>,
    pub border_color: Color,
    /// Painted under the node, outside its box (see `Shadow`).
    pub shadows: Vec<Shadow>,
    pub img: Option<ImgSlot>,
    pub text: Option<TextSlot>,
    pub opacity: Anim,
    pub scale: Anim,
    /// Extra horizontal scale on top of `scale` (the loading bar's scaleX).
    pub scale_x: Anim,
    pub origin: [f32; 2],
    pub tx: Anim,
    pub ty: Anim,
    /// Scroll offset of a scroll container (`scroll`): children move by minus this.
    pub scroll_x: Anim,
    pub scroll_y: Anim,
    /// Clip children to the box (overflow hidden; scroll containers always clip).
    pub clip: bool,
    /// A scroll container (overflow auto): see `crate::scroll`.
    pub scroll: bool,
    /// Focus ring strength 0..1.
    pub ring: Anim,
    pub z: i16,
    pub hidden: bool,
    /// A paint slot: `Scene::paint_with` calls back here, in scene order (inside scroll
    /// containers and rounded clips, under later siblings, sheets and toasts), for what the scene
    /// cannot describe itself: charts, video frames, rolling Nums, edit fields.
    pub slot: Option<u32>,
}

impl Default for Node {
    fn default() -> Self {
        Node {
            bg: CAnim::new(Color::TRANSPARENT),
            grad: None,
            lgrad: None,
            radius: 0.0,
            corners: None,
            radius_pct: 0.0,
            border: 0.0,
            borders: None,
            border_color: Color::TRANSPARENT,
            shadows: Vec::new(),
            img: None,
            text: None,
            opacity: Anim::new(1.0),
            scale: Anim::new(1.0),
            scale_x: Anim::new(1.0),
            origin: [0.5, 0.5],
            tx: Anim::new(0.0),
            ty: Anim::new(0.0),
            scroll_x: Anim::new(0.0),
            scroll_y: Anim::new(0.0),
            clip: false,
            scroll: false,
            ring: Anim::new(0.0),
            z: 0,
            hidden: false,
            slot: None,
        }
    }
}

impl Node {
    pub fn bg(c: Color) -> Node {
        Node { bg: CAnim::new(c), ..Default::default() }
    }
    pub fn radius(mut self, r: f32) -> Node {
        self.radius = r;
        self
    }
    pub fn clip(mut self) -> Node {
        self.clip = true;
        self
    }
    /// Per-corner radii [top-left, top-right, bottom-right, bottom-left].
    pub fn corners(mut self, r: [f32; 4]) -> Node {
        self.corners = Some(r);
        self
    }
    /// One-sided or uneven borders [top, right, bottom, left] (`border-b` is [0, 0, 1, 0]).
    pub fn borders(mut self, w: [f32; 4], c: Color) -> Node {
        self.borders = Some(w);
        self.border_color = c;
        self
    }
    pub fn lgrad(mut self, g: LinearGradient) -> Node {
        self.lgrad = Some(g);
        self
    }
    /// The corner radii in px for a box of `w` x `h` (before transforms).
    pub fn radii(&self, w: f32, h: f32) -> [f32; 4] {
        if self.radius_pct > 0.0 {
            [self.radius_pct * w.min(h); 4]
        } else {
            self.corners.unwrap_or([self.radius; 4])
        }
    }
    fn border_widths(&self) -> [f32; 4] {
        self.borders.unwrap_or([self.border; 4])
    }
    pub fn border(mut self, w: f32, c: Color) -> Node {
        self.border = w;
        self.border_color = c;
        self
    }
    pub fn grad(mut self, g: Gradient) -> Node {
        self.grad = Some(g);
        self
    }
    pub fn shadow(mut self, s: &[Shadow]) -> Node {
        self.shadows = s.to_vec();
        self
    }
    pub fn image(mut self, key: u64) -> Node {
        self.img = Some(ImgSlot { key, tint: None, ink: None, contain: None, fade: Anim::new(0.0), seen: false, waited: false, tex: None });
        self
    }
    pub fn icon(mut self, key: u64, tint: Color) -> Node {
        self.img = Some(ImgSlot { key, tint: Some(tint), ink: None, contain: Some([0.5, 0.5]), fade: Anim::new(0.0), seen: false, waited: false, tex: None });
        self
    }
    /// Shows texture `tex` (`w` x `h` pixels) like an image: cover fit, clipped to the radius.
    pub fn image_tex(mut self, tex: crate::gpu::TexId, w: u32, h: u32) -> Node {
        self.img = Some(ImgSlot { key: 0, tint: None, ink: None, contain: None, fade: Anim::new(1.0), seen: true, waited: true, tex: Some((tex, w, h)) });
        self
    }
    pub fn contain(mut self, align: [f32; 2]) -> Node {
        if let Some(i) = self.img.as_mut() {
            i.contain = Some(align);
        }
        self
    }
    pub fn origin(mut self, x: f32, y: f32) -> Node {
        self.origin = [x, y];
        self
    }
    pub fn opacity(mut self, o: f32) -> Node {
        self.opacity = Anim::new(o);
        self
    }
    /// Marks the node as paint slot `n` (see `SlotPaint`).
    pub fn slot(mut self, n: u32) -> Node {
        self.slot = Some(n);
        self
    }
    pub fn z(mut self, z: i16) -> Node {
        self.z = z;
        self
    }
    /// A scroll container: clips, and its children move by the scroll offset.
    pub fn scrolls(mut self) -> Node {
        self.scroll = true;
        self.clip = true;
        self
    }

    fn animating(&self, now: f64) -> bool {
        self.bg.active(now)
            || self.opacity.active(now)
            || self.scale.active(now)
            || self.scale_x.active(now)
            || self.tx.active(now)
            || self.ty.active(now)
            || self.scroll_x.active(now)
            || self.scroll_y.active(now)
            || self.ring.active(now)
            || self.img.as_ref().is_some_and(|i| i.fade.active(now) || i.ink.as_ref().is_some_and(|k| k.active(now)))
            || self.text.as_ref().is_some_and(|t| t.color.active(now) || t.roll.as_ref().is_some_and(|r| r.0.active(now)))
    }
}

#[derive(Clone, Copy)]
struct Xf {
    sx: f32,
    sy: f32,
    x: f32,
    y: f32,
}

pub struct Scene {
    pub tree: TaffyTree<Node>,
    pub root: Id,
    pub w: f32,
    pub h: f32,
    pub rem: f32,
    dirty: bool,
    /// Page colour for the ring's gap.
    pub ring_gap_color: Color,
    pub ring_color: Color,
    /// Paints no focus ring (touch and mouse: the controller's focus shows again with the next
    /// press). Focus looks (a button's fill, a tile's scale) still show.
    pub ring_hidden: bool,
    pub layouts: u64,
    layers: Vec<(i16, Id)>,
    /// A focus-follow waiting for the next layout (`follow_focus` on a tree that changed).
    follow: Option<(Id, bool)>,
}

fn rect_isect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}

/// CSS: a shadow's corner radius grows by the spread, sharpened for radii below the spread.
fn spread_radius(r: f32, spread: f32) -> f32 {
    if spread > 0.0 && r < spread {
        let k = r / spread - 1.0;
        r + spread * (1.0 + k * k * k)
    } else {
        (r + spread).max(0.0)
    }
}

/// Every node is a containing block: `position: static` (taffy's default) becomes `relative`,
/// which lays out the same. An absolutely positioned child then sizes and places itself against
/// its parent's padding box, the way the screens are written (a Tailwind `absolute` inside a
/// `relative` box), and never against the window; paint and `rect` place it from its parent too.
fn positioned(mut s: Style) -> Style {
    if s.position == taffy::style::Position::Static {
        s.position = taffy::style::Position::Relative;
    }
    s
}

/// A scroll container lays out as CSS `overflow: auto` does: it never grows to fit its content as a
/// flex item (its automatic minimum size is 0), and what overflows it is its own scroll range, not
/// its parent's (a list nested in a page doesn't stretch the page's).
fn scroll_style(mut s: Style, node: &Node) -> Style {
    if node.scroll {
        use taffy::style::Overflow;
        s.overflow = taffy::geometry::Point { x: Overflow::Hidden, y: Overflow::Hidden };
    }
    s
}

/// Corner radii as a prim carries them: [r, 0, 0, 0] would read as "r on every corner", so a lone
/// top-left radius gets a vanishing bottom-left one.
fn corner_set(r: [f32; 4]) -> [f32; 4] {
    if r[0] > 0.0 && r[1] == 0.0 && r[2] == 0.0 && r[3] == 0.0 {
        [r[0], 0.0, 0.0, 1e-4]
    } else {
        r
    }
}

/// A linear gradient as one prim per pair of neighbouring stops (the first and last also cover
/// what lies before and after them).
fn push_gradient(out: &mut DrawList, rect: [f32; 4], clip: [f32; 4], r4: [f32; 4], g: &LinearGradient, op: f32) {
    let n = g.stops.len();
    if n == 0 {
        return;
    }
    let a = g.angle.to_radians();
    let dir = [a.sin(), -a.cos()];
    let space = if g.oklab { 1.0 } else { 0.0 };
    // CSS fixes up stop positions: never behind an earlier one.
    let mut at = 0.0f32;
    let stops: Vec<(Color, f32)> = g
        .stops
        .iter()
        .map(|(c, p)| {
            at = p.max(at);
            (*c, at)
        })
        .collect();
    if n == 1 {
        let c = stops[0].0.arr();
        out.push(Prim { rect, clip, c0: c, c1: c, c2: [1.0, 1.0, space, 0.0], p: [0.0, 0.0, KIND_GRADN, op], q: [dir[0], dir[1], 0.0, 1.0], radii: corner_set(r4), ..Default::default() }, None);
        return;
    }
    for i in 0..n - 1 {
        let ((c0, t0), (c1, t1)) = (stops[i], stops[i + 1]);
        let (first, last) = (i == 0, i + 2 == n);
        if t1 <= t0 && !(first || last) {
            continue;
        }
        out.push(
            Prim {
                rect,
                clip,
                c0: c0.arr(),
                c1: c1.arr(),
                c2: [first as u8 as f32, last as u8 as f32, space, 0.0],
                p: [0.0, 0.0, KIND_GRADN, op],
                q: [dir[0], dir[1], t0, t1],
                radii: corner_set(r4),
                ..Default::default()
            },
            None,
        );
    }
}

impl Scene {
    pub fn new(w: f32, h: f32, rem: f32) -> Scene {
        let mut tree: TaffyTree<Node> = TaffyTree::new();
        let root = tree
            .new_leaf_with_context(Style { size: Size { width: length(w), height: length(h) }, position: taffy::style::Position::Relative, ..Default::default() }, Node::default())
            .unwrap();
        Scene { tree, root, w, h, rem, dirty: true, ring_gap_color: Color::BLACK, ring_color: Color::WHITE, ring_hidden: false, layouts: 0, layers: Vec::new(), follow: None }
    }

    pub fn resize(&mut self, w: f32, h: f32, rem: f32) {
        self.w = w;
        self.h = h;
        self.rem = rem;
        let mut st = self.tree.style(self.root).unwrap().clone();
        st.size = Size { width: length(w), height: length(h) };
        self.tree.set_style(self.root, st).unwrap();
        self.dirty = true;
    }

    /// The full-window layer root at `z` (see `layer`), created on first use. Children of a layer
    /// lay out against the whole window (it is `position: fixed; inset: 0`) and paint above every
    /// lower layer and the page.
    pub fn layer(&mut self, z: i16) -> Id {
        if let Some((_, id)) = self.layers.iter().find(|(lz, _)| *lz == z) {
            return *id;
        }
        let style = Style {
            position: taffy::style::Position::Absolute,
            inset: taffy::geometry::Rect { left: length(0.0), right: length(0.0), top: length(0.0), bottom: length(0.0) },
            ..Default::default()
        };
        let id = self.add(self.root, style, Node::default().z(z));
        self.layers.push((z, id));
        id
    }

    /// The layer a node lives in (`layer::BASE` for the page).
    pub fn layer_of(&self, mut id: Id) -> i16 {
        while let Some(p) = self.tree.parent(id) {
            if p == self.root {
                return self.layers.iter().find(|(_, l)| *l == id).map(|(z, _)| *z).unwrap_or(layer::BASE);
            }
            id = p;
        }
        layer::BASE
    }

    pub fn add(&mut self, parent: Id, style: Style, node: Node) -> Id {
        let style = scroll_style(positioned(style), &node);
        let id = self.tree.new_leaf_with_context(style, node).unwrap();
        self.tree.add_child(parent, id).unwrap();
        self.dirty = true;
        id
    }

    /// Adds a node as `parent`'s child number `index` (clamped to the end).
    pub fn insert(&mut self, parent: Id, index: usize, style: Style, node: Node) -> Id {
        let style = scroll_style(positioned(style), &node);
        let id = self.tree.new_leaf_with_context(style, node).unwrap();
        let n = self.tree.child_count(parent);
        self.tree.insert_child_at_index(parent, index.min(n), id).unwrap();
        self.dirty = true;
        id
    }

    /// Replaces `parent`'s children with `kids` in that order (nodes left out are detached, not
    /// freed: remove them with `remove`).
    /// Removed ids in `kids` are skipped, and a removed `parent` is a no-op.
    pub fn set_children(&mut self, parent: Id, kids: &[Id]) {
        if !self.alive(parent) || self.tree.children(parent).map(|c| c == kids).unwrap_or(false) {
            return;
        }
        if kids.iter().all(|k| self.alive(*k)) {
            self.tree.set_children(parent, kids).unwrap();
        } else {
            let live: Vec<Id> = kids.iter().copied().filter(|k| self.alive(*k)).collect();
            self.tree.set_children(parent, &live).unwrap();
        }
        self.dirty = true;
    }

    /// Whether `id` is a node of this scene (false once it was removed: taffy's ids carry a
    /// generation, so a removed id never comes back as another node).
    pub fn alive(&self, id: Id) -> bool {
        self.tree.get_node_context(id).is_some()
    }

    pub fn add_text(&mut self, ts: &mut TextSys, parent: Id, style: Style, text: &str, st: TextStyle, color: Color) -> Id {
        let node = Scene::text_node(ts, text, st, color);
        self.add(parent, style, node)
    }

    /// A node that draws `text` (for `add`/`insert`).
    pub fn text_node(ts: &mut TextSys, text: &str, st: TextStyle, color: Color) -> Node {
        let buf = ts.buffer(text, &st);
        Node { text: Some(TextSlot { buf, style: st, color: CAnim::new(color), text: text.to_string(), roll: None }), ..Default::default() }
    }

    /// A live number's text is now `text` (value `value`): the characters that moved roll and the
    /// background flashes (`text::roll`). The first call only remembers it.
    pub fn roll_text(&mut self, id: Id, text: &str, value: f64, colors: [Color; 2], radius: f32, now: f64) {
        let Some(n) = self.tree.get_node_context_mut(id) else { return };
        let Some(t) = n.text.as_mut() else { return };
        let r = t.roll.get_or_insert_with(|| Box::new((crate::text::roll::NumRoll::new(), colors, radius)));
        r.1 = colors;
        r.2 = radius;
        if r.0.update(Some(text), Some(value), now, true) {
            self.dirty = true;
        }
    }

    pub fn set_text(&mut self, ts: &mut TextSys, id: Id, text: &str) {
        let Some(n) = self.tree.get_node_context_mut(id) else { return };
        let Some(t) = n.text.as_mut() else { return };
        if t.text == text {
            return;
        }
        t.text = text.to_string();
        let st = t.style;
        ts.set(&mut t.buf, text, &st);
        let _ = self.tree.mark_dirty(id);
        self.dirty = true;
    }

    /// Changes a text node's style (size, weight, wrap...), relaying it out.
    pub fn set_text_style(&mut self, ts: &mut TextSys, id: Id, st: TextStyle) {
        let Some(n) = self.tree.get_node_context_mut(id) else { return };
        let Some(t) = n.text.as_mut() else { return };
        if t.style == st {
            return;
        }
        t.style = st;
        let text = std::mem::take(&mut t.text);
        ts.set(&mut t.buf, &text, &st);
        t.text = text;
        let _ = self.tree.mark_dirty(id);
        self.dirty = true;
    }

    /// Makes `id` draw `text` in `st` (creating the text slot if it had none), relaying it out
    /// only when the text or its style changed. The colour snaps; animate it through `n(id)`.
    pub fn put_text(&mut self, ts: &mut TextSys, id: Id, text: &str, st: TextStyle, color: Color) {
        let Some(n) = self.tree.get_node_context_mut(id) else { return };
        if let Some(t) = n.text.as_mut() {
            t.color.snap(color);
            self.set_text_style(ts, id, st);
            self.set_text(ts, id, text);
        } else {
            let buf = ts.buffer(text, &st);
            n.text = Some(TextSlot { buf, style: st, color: CAnim::new(color), text: text.to_string(), roll: None });
            let _ = self.tree.mark_dirty(id);
            self.dirty = true;
        }
    }

    /// Shapes every text again (after `TextSys::poll_fonts` found a face for missing glyphs, or a
    /// text look change) and relayouts.
    pub fn reshape_text(&mut self, ts: &mut TextSys) {
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if let Some(t) = self.tree.get_node_context_mut(id).and_then(|n| n.text.as_mut()) {
                let st = t.style;
                ts.set(&mut t.buf, &t.text, &st);
                let _ = self.tree.mark_dirty(id);
            }
            stack.extend(self.tree.child_ids(id));
        }
        self.dirty = true;
    }

    /// Drops a node's text (it measures as an empty box again).
    pub fn clear_text(&mut self, id: Id) {
        if let Some(n) = self.tree.get_node_context_mut(id) {
            if n.text.take().is_some() {
                let _ = self.tree.mark_dirty(id);
                self.dirty = true;
            }
        }
    }

    /// Sets or clears a node's image. A new key fades in once ready; the same key keeps its state.
    pub fn put_image(&mut self, id: Id, img: Option<(u64, Option<Color>, Option<[f32; 2]>)>) {
        let Some(n) = self.tree.get_node_context_mut(id) else { return };
        match (img, n.img.as_mut()) {
            (None, _) => n.img = None,
            (Some((key, tint, contain)), Some(s)) if s.key == key => {
                if s.tint != tint {
                    s.ink = None;
                }
                s.tint = tint;
                s.contain = contain;
            }
            (Some((key, tint, contain)), _) => n.img = Some(ImgSlot { key, tint, ink: None, contain, fade: Anim::new(0.0), seen: false, waited: false, tex: None }),
        }
    }

    /// Shows an app texture on `id` (see `Node::image_tex`); a new size or texture snaps.
    pub fn put_texture(&mut self, id: Id, tex: crate::gpu::TexId, w: u32, h: u32) {
        if let Some(n) = self.tree.get_node_context_mut(id) {
            match n.img.as_mut() {
                Some(s) if s.tex.is_some() => s.tex = Some((tex, w, h)),
                _ => *n = std::mem::take(n).image_tex(tex, w, h),
            }
        }
    }

    pub fn n(&mut self, id: Id) -> &mut Node {
        self.tree.get_node_context_mut(id).expect("node")
    }

    pub fn get(&self, id: Id) -> &Node {
        self.tree.get_node_context(id).expect("node")
    }

    pub fn try_get(&self, id: Id) -> Option<&Node> {
        self.tree.get_node_context(id)
    }

    pub fn set_style(&mut self, id: Id, style: Style) {
        if self.alive(id) {
            let style = match self.tree.get_node_context(id) {
                Some(n) => scroll_style(positioned(style), n),
                None => positioned(style),
            };
            self.tree.set_style(id, style).unwrap();
            self.dirty = true;
        }
    }

    pub fn style(&self, id: Id) -> Style {
        self.tree.style(id).unwrap().clone()
    }

    /// Frees `id` and its subtree. Removing an id that is already gone (a leaving node whose
    /// ancestor went first, a second `clear`) is a no-op.
    pub fn remove(&mut self, id: Id) {
        if !self.alive(id) {
            return;
        }
        for c in self.tree.children(id).unwrap_or_default() {
            self.remove(c);
        }
        self.layers.retain(|(_, l)| *l != id);
        let _ = self.tree.remove(id);
        self.dirty = true;
    }

    pub fn clear_children(&mut self, id: Id) {
        for c in self.tree.children(id).unwrap_or_default() {
            self.remove(c);
        }
        self.dirty = true;
    }

    pub fn children(&self, id: Id) -> Vec<Id> {
        self.tree.children(id).unwrap_or_default()
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        self.tree.parent(id)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// True when the next `layout` call will recompute.
    pub fn needs_layout(&self) -> bool {
        self.dirty
    }

    /// Recomputes layout if anything changed. True when it ran.
    pub fn layout(&mut self, ts: &mut TextSys) -> bool {
        if !self.dirty {
            return false;
        }
        let avail = Size { width: AvailableSpace::Definite(self.w), height: AvailableSpace::Definite(self.h) };
        self.tree
            .compute_layout_with_measure(self.root, avail, |inputs, _id, ctx, style| {
                taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known, avail| {
                    if let Size { width: Some(w), height: Some(h) } = known {
                        return Size { width: w, height: h };
                    }
                    let Some(t) = ctx.and_then(|n| n.text.as_mut()) else { return Size::ZERO };
                    let width = if t.style.needs_width() {
                        known.width.or(match avail.width {
                            AvailableSpace::Definite(w) => Some(w),
                            _ => None,
                        })
                    } else {
                        None
                    };
                    let (w, h) = ts.measure(&mut t.buf, width);
                    Size { width: known.width.unwrap_or(w), height: known.height.unwrap_or(h) }
                })
            })
            .unwrap();
        // Text that depends on its width (wrap, truncate, clamp, align) is laid out again at its
        // final width: the unrounded one, so a shrink-to-fit box rounded a fraction below its own
        // text neither wraps its last word nor ellipsizes.
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            let w = self.tree.unrounded_layout(id).size.width;
            if let Some(t) = self.tree.get_node_context_mut(id).and_then(|n| n.text.as_mut()) {
                if t.style.needs_width() {
                    ts.measure(&mut t.buf, Some(w));
                }
            }
            stack.extend(self.tree.child_ids(id));
        }
        self.dirty = false;
        self.layouts += 1;
        if let Some((id, smooth)) = self.follow.take() {
            if self.try_get(id).is_some() {
                crate::scroll::follow(self, id, crate::clock::secs(), smooth);
            }
        }
        true
    }

    /// Focus moved to `id`: every scroll container above it follows (`scroll::follow`). On a tree
    /// that changed since the last layout (a list rebuilt around the new focus) it waits for the
    /// next layout, so it measures where the rows really are.
    pub fn follow_focus(&mut self, id: Id, now: f64, smooth: bool) {
        if self.dirty {
            self.follow = Some((id, smooth));
        } else {
            self.follow = None;
            crate::scroll::follow(self, id, now, smooth);
        }
    }

    /// Layout-space rectangle (no transforms, no scroll) relative to the window: [x, y, w, h].
    pub fn rect(&self, id: Id) -> [f32; 4] {
        let l = self.tree.layout(id).unwrap();
        let (mut x, mut y) = (l.location.x, l.location.y);
        let mut p = self.tree.parent(id);
        while let Some(pid) = p {
            let pl = self.tree.layout(pid).unwrap();
            x += pl.location.x;
            y += pl.location.y;
            p = self.tree.parent(pid);
        }
        [x, y, l.size.width, l.size.height]
    }

    /// Where `id` sits inside `ancestor`'s box in layout space, independent of any scroll
    /// (nav/focus.ts's `offsetIn`). None if `ancestor` is not an ancestor.
    pub fn offset_in(&self, id: Id, ancestor: Id) -> Option<[f32; 4]> {
        let l = self.tree.layout(id).ok()?;
        let (mut x, mut y) = (l.location.x, l.location.y);
        let mut p = self.tree.parent(id)?;
        while p != ancestor {
            let pl = self.tree.layout(p).ok()?;
            x += pl.location.x;
            y += pl.location.y;
            p = self.tree.parent(p)?;
        }
        Some([x, y, l.size.width, l.size.height])
    }

    /// The rectangle as it lands once motion settles: layout plus the targets of every ancestor's
    /// translate and scroll (no scale). Spatial focus and touch hit tests use this, so a scrolled
    /// grid navigates by where its tiles will be.
    pub fn settled_rect(&self, id: Id) -> [f32; 4] {
        let [mut x, mut y, w, h] = self.rect(id);
        if let Some(n) = self.try_get(id) {
            x += n.tx.target();
            y += n.ty.target();
        }
        let mut p = self.tree.parent(id);
        while let Some(pid) = p {
            if let Some(n) = self.try_get(pid) {
                x += n.tx.target() - n.scroll_x.target();
                y += n.ty.target() - n.scroll_y.target();
            }
            p = self.tree.parent(pid);
        }
        [x, y, w, h]
    }

    /// Where the box is on screen right now, without scale: layout plus the current translate of it
    /// and its ancestors, minus their current scroll (getBoundingClientRect for unscaled boxes).
    /// This is the space `focus::Nav` works in.
    pub fn window_rect(&self, id: Id, now: f64) -> [f32; 4] {
        let [mut x, mut y, w, h] = self.rect(id);
        if let Some(n) = self.try_get(id) {
            x += n.tx.get(now);
            y += n.ty.get(now);
        }
        let mut p = self.tree.parent(id);
        while let Some(pid) = p {
            if let Some(n) = self.try_get(pid) {
                x += n.tx.get(now);
                y += n.ty.get(now);
                if n.scroll {
                    x -= n.scroll_x.get(now);
                    y -= n.scroll_y.get(now);
                }
            }
            p = self.tree.parent(pid);
        }
        [x, y, w, h]
    }

    /// The resolved padding of a node: [top, right, bottom, left].
    pub fn padding(&self, id: Id) -> [f32; 4] {
        match self.tree.layout(id) {
            Ok(l) => [l.padding.top, l.padding.right, l.padding.bottom, l.padding.left],
            Err(_) => [0.0; 4],
        }
    }

    /// What a focus ring takes outside its box, in px (`theme::ring::OUTER`).
    pub fn ring_room(&self) -> f32 {
        crate::theme::ring::OUTER * self.rem
    }

    /// Maximum scroll offset [x, y] of a scroll container: the content's far end plus room for a
    /// focus ring there, so the last row's ring shows in full (app.css gives scrollers that
    /// padding). 0 when the content and the ring fit.
    pub fn max_scroll(&self, id: Id) -> [f32; 2] {
        match self.tree.layout(id) {
            Ok(l) => {
                // The room goes along the axis the box lays its children out on (a strip
                // sideways, a list or a page down); the other axis keeps its plain extent.
                let (rx, ry) = if self.lays_out_in_a_row(id) { (self.ring_room(), 0.0) } else { (0.0, self.ring_room()) };
                let raw_x = l.scrollable_overflow_rect.right - l.size.width + l.border.left + l.border.right;
                let raw_y = l.scrollable_overflow_rect.bottom - l.size.height + l.border.top + l.border.bottom;
                [if rx > 0.0 { (raw_x + rx).max(0.0) } else { l.scroll_width() }, if ry > 0.0 { (raw_y + ry).max(0.0) } else { l.scroll_height() }]
            }
            Err(_) => [0.0, 0.0],
        }
    }

    fn lays_out_in_a_row(&self, id: Id) -> bool {
        self.tree.style(id).is_ok_and(|st| matches!(st.flex_direction, taffy::style::FlexDirection::Row | taffy::style::FlexDirection::RowReverse) && st.display == taffy::style::Display::Flex)
    }

    /// Minimum scroll offset [x, y] (0 or below): a scroller whose padding is thinner than a focus
    /// ring may scroll back past its start by the difference, so the first row's ring shows.
    pub fn min_scroll(&self, id: Id) -> [f32; 2] {
        match self.tree.layout(id) {
            Ok(l) => {
                let room = self.ring_room();
                if self.lays_out_in_a_row(id) {
                    [-(room - l.padding.left).max(0.0), 0.0]
                } else {
                    [0.0, -(room - l.padding.top).max(0.0)]
                }
            }
            Err(_) => [0.0, 0.0],
        }
    }

    /// Anything still moving (the frame loop keeps drawing while true).
    pub fn animating(&self, now: f64) -> bool {
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if let Some(n) = self.tree.get_node_context(id) {
                if n.hidden {
                    continue;
                }
                if n.animating(now) {
                    return true;
                }
            }
            stack.extend(self.tree.child_ids(id));
        }
        false
    }

    /// Starts the fade of images that just became ready. True when any did.
    pub fn sync_images(&mut self, now: f64, images: &ImageLoader, fade: f32) -> bool {
        let mut any = false;
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if let Some(slot) = self.tree.get_node_context_mut(id).and_then(|n| n.img.as_mut()) {
                if !slot.seen {
                    if let ImgState::Ready { .. } = images.peek(slot.key) {
                        slot.seen = true;
                        // From the disk cache: a revisit, shown at once like one still on the GPU.
                        if slot.waited && !images.instant(slot.key) {
                            slot.fade.restart(0.0, 1.0, now, fade);
                        } else {
                            slot.fade.snap(1.0);
                        }
                        any = true;
                    }
                    slot.waited = true;
                }
            }
            stack.extend(self.tree.child_ids(id));
        }
        any
    }

    pub fn paint(&self, now: f64, ts: &mut TextSys, images: &ImageLoader, out: &mut DrawList) {
        self.paint_with(now, ts, images, out, &mut |_, _, _| {});
    }

    /// `paint`, calling `slots` for every node marked `Node::slot` / `El::slot` where it paints:
    /// after its own fill, image and text, before its children. Prims the callback pushes are
    /// clipped like the node (its clip rect and rounded clip are applied afterwards, so it may use
    /// `SlotPaint::clip` or no clip at all) and should multiply their opacity by `opacity`.
    pub fn paint_with(&self, now: f64, ts: &mut TextSys, images: &ImageLoader, out: &mut DrawList, slots: &mut SlotFn<'_>) {
        out.clear();
        let clip = [0.0, 0.0, self.w, self.h];
        let ctx = PaintCx { now, rem: self.rem, ring_gap: self.ring_gap_color, ring: self.ring_color, ring_on: !self.ring_hidden };
        paint_node(&self.tree, self.root, Xf { sx: 1.0, sy: 1.0, x: 0.0, y: 0.0 }, 1.0, clip, None, clip, &ctx, ts, images, out, slots);
    }

    // ---- touch ----

    /// The topmost node at window point (`x`, `y`) that `pick` accepts, in paint order (z, then
    /// tree order, children over their parent), inside every clip and scroll viewport above it.
    /// Hidden and fully transparent boxes (and what is inside them) are never hit.
    pub fn hit_where(&self, x: f32, y: f32, now: f64, pick: &dyn Fn(Id, &Node) -> bool) -> Option<Id> {
        let mut best = None;
        hit_node(&self.tree, self.root, Xf { sx: 1.0, sy: 1.0, x: 0.0, y: 0.0 }, 1.0, [0.0, 0.0, self.w, self.h], [x, y], now, pick, &mut best);
        best
    }

    /// The topmost node at (`x`, `y`) that paints something (a fill, gradient, border, image or
    /// text): what a finger lands on. A sheet's dimmed backdrop counts, so the page under it is
    /// never what a touch reaches.
    pub fn hit(&self, x: f32, y: f32, now: f64) -> Option<Id> {
        self.hit_where(x, y, now, &|_, n| paints(n, now))
    }

    /// The topmost of `ids` at (`x`, `y`): a focus list's item under a finger.
    pub fn hit_among(&self, x: f32, y: f32, now: f64, ids: &[Id]) -> Option<Id> {
        self.hit_where(x, y, now, &|id, _| ids.contains(&id))
    }

    /// The scroll container a drag that starts at (`x`, `y`) moves: the innermost one above the
    /// node under the finger that can scroll that way (a rail sideways, the page up and down).
    pub fn scroller_at(&self, x: f32, y: f32, now: f64, horizontal: bool) -> Option<Id> {
        let mut at = self.hit(x, y, now);
        while let Some(id) = at {
            if self.try_get(id).is_some_and(|n| n.scroll) {
                let (min, max) = (self.min_scroll(id), self.max_scroll(id));
                let i = horizontal as usize ^ 1;
                if max[i] > 0.5 || min[i] < -0.5 {
                    return Some(id);
                }
            }
            at = self.parent(id);
        }
        None
    }

}

/// Where a paint slot lands this frame (see `Scene::paint_with`).
#[derive(Clone, Copy, Debug)]
pub struct SlotPaint {
    pub slot: u32,
    pub id: Id,
    /// The node's box on screen [x, y, w, h], transforms and scroll applied.
    pub rect: [f32; 4],
    /// The clip [x0, y0, x1, y1] the node is drawn under.
    pub clip: [f32; 4],
    /// Opacity of the node and its ancestors.
    pub opacity: f32,
    /// Scale of the node and its ancestors [x, y].
    pub scale: [f32; 2],
    pub now: f64,
}

/// The paint slot callback: `(where, text, draw list)`.
pub type SlotFn<'a> = dyn FnMut(&SlotPaint, &mut TextSys, &mut DrawList) + 'a;

struct PaintCx {
    now: f64,
    rem: f32,
    ring_gap: Color,
    ring: Color,
    ring_on: bool,
}

#[allow(clippy::too_many_arguments)]
fn paint_node(tree: &TaffyTree<Node>, id: Id, parent: Xf, opacity: f32, clip: [f32; 4], rclip: Option<([f32; 4], [f32; 4])>, ring_clip: [f32; 4], cx: &PaintCx, ts: &mut TextSys, images: &ImageLoader, out: &mut DrawList, slots: &mut SlotFn<'_>) {
    let Some(n) = tree.get_node_context(id) else { return };
    if n.hidden {
        return;
    }
    let now = cx.now;
    let l = tree.layout(id).unwrap();
    let (w, h) = (l.size.width, l.size.height);
    let s = n.scale.get(now);
    let (s_x, s_y) = (s * n.scale_x.get(now), s);
    let (ox, oy) = (w * n.origin[0], h * n.origin[1]);
    let lx = l.location.x + n.tx.get(now) + ox - s_x * ox;
    let ly = l.location.y + n.ty.get(now) + oy - s_y * oy;
    let (gsx, gsy) = (parent.sx * s_x, parent.sy * s_y);
    let (fx, fy) = (parent.x + parent.sx * lx, parent.y + parent.sy * ly);
    let (mut gx, mut gy) = (fx, fy);
    let unscaled = (gsx - 1.0).abs() < 1e-4 && (gsy - 1.0).abs() < 1e-4;
    if unscaled {
        gx = gx.round();
        gy = gy.round();
    }
    let gs = gsx.min(gsy);
    let (gw, gh) = (w * gsx, h * gsy);
    let op = opacity * n.opacity.get(now);
    if op < 0.002 {
        return;
    }
    let kids_len = tree.child_count(id);
    if (n.clip || kids_len == 0) && (gx > clip[2] || gy > clip[3] || gx + gw < clip[0] || gy + gh < clip[1]) {
        // Off screen or clipped away, children included; a ring could still poke in.
        if n.ring.get(now) <= 0.0 && n.shadows.is_empty() {
            return;
        }
    }
    let rect = [gx, gy, gw, gh];
    let r4 = n.radii(w, h).map(|r| r * gs);
    let radius = r4.iter().copied().fold(0.0, f32::max);
    let first = out.prims.len();

    // Box-shadow layers paint first to last under the box (CSS paints the first one on top, so
    // they go in reverse).
    for sh in n.shadows.iter().rev() {
        let sp = sh.spread * gs;
        let srect = [gx + sh.x * gsx - sp, gy + sh.y * gsy - sp, gw + 2.0 * sp, gh + 2.0 * sp];
        if srect[2] <= 0.0 || srect[3] <= 0.0 {
            continue;
        }
        out.push(
            Prim {
                rect: srect,
                clip,
                c0: sh.color.arr(),
                uv: rect,
                p: [0.0, sh.blur * 0.5 * gs, KIND_SHADOW, op],
                radii: corner_set(r4.map(|r| spread_radius(r, sp))),
                bw: corner_set(r4),
                ..Default::default()
            },
            None,
        );
    }
    let bg = n.bg.get(now);
    let bw = n.border_widths().map(|b| b * gs);
    if bg.a > 0.0 || bw.iter().any(|b| *b > 0.0) {
        out.push(Prim { rect, clip, c0: bg.arr(), c1: n.border_color.arr(), p: [0.0, 0.0, KIND_RECT, op], radii: corner_set(r4), bw, ..Default::default() }, None);
    }
    if let Some(g) = n.grad {
        push_gradient(out, rect, clip, r4, &g.linear(), op);
    }
    if let Some(g) = &n.lgrad {
        push_gradient(out, rect, clip, r4, g, op);
    }
    if let Some(slot) = &n.img {
        let state = match slot.tex {
            Some((tex, w, h)) => {
                // Cover: crop the texture to the box's aspect, centred (contain fits it whole).
                let (ta, ba) = (w as f32 / h.max(1) as f32, gw / gh.max(1e-3));
                let uv = if slot.contain.is_some() || (ta - ba).abs() < 1e-3 {
                    [0.0, 0.0, 1.0, 1.0]
                } else if ta > ba {
                    let k = ba / ta;
                    [(1.0 - k) / 2.0, 0.0, (1.0 + k) / 2.0, 1.0]
                } else {
                    let k = ta / ba;
                    [0.0, (1.0 - k) / 2.0, 1.0, (1.0 + k) / 2.0]
                };
                ImgState::Ready { tex, w, h, uv }
            }
            None => images.state(slot.key),
        };
        if let ImgState::Ready { tex, w: tw, h: th, uv } = state {
            let f = slot.fade.get(now);
            if f > 0.0 {
                let r = match slot.contain {
                    Some(al) => {
                        // Never upscale a contain image past its own pixels by more than the transform.
                        let k = (gw / tw as f32).min(gh / th as f32).min(gs);
                        let (dw, dh) = (tw as f32 * k, th as f32 * k);
                        let (mut dx, mut dy) = (gx + (gw - dw) * al[0], gy + (gh - dh) * al[1]);
                        if unscaled {
                            dx = dx.round();
                            dy = dy.round();
                        }
                        [dx, dy, dw, dh]
                    }
                    None => rect,
                };
                let (kind, c0) = match slot.tint {
                    Some(c) => (KIND_MASK, slot.ink.as_ref().filter(|k| k.target() == c).map_or(c, |k| k.get(now)).arr()),
                    None => (KIND_IMAGE, [1.0; 4]),
                };
                let rr = if slot.contain.is_some() { [0.0; 4] } else { corner_set(r4) };
                out.push(Prim { rect: r, clip, c0, uv, p: [0.0, 0.0, kind, op * f], radii: rr, ..Default::default() }, Some(tex));
            }
        }
    }
    if let Some(t) = &n.text {
        // Glyph pens keep the node's fractional x (TextSys bins them to a quarter pixel), scaled
        // labels rasterise at their drawn size, and line-clamp gets its ellipsis.
        let tp = TextPaint { clip, opacity: op, ..TextPaint::at(fx, fy, t.color.get(now)).scaled(gsx, gsy) };
        match t.roll.as_deref().filter(|r| r.0.active(now)) {
            Some((roll, colors, r)) => ts.paint_roll(&t.buf, &tp, roll, now, [gw, gh], *r, *colors, out),
            None => ts.paint(&t.buf, &tp, out),
        }
    }
    if let Some(slot) = n.slot {
        let from = out.prims.len();
        slots(&SlotPaint { slot, id, rect, clip, opacity: op, scale: [gsx, gsy], now }, ts, out);
        for p in &mut out.prims[from..] {
            p.clip = rect_isect(p.clip, clip);
        }
    }
    if let Some((rc, r)) = rclip {
        for p in &mut out.prims[first..] {
            p.rclip = rc;
            p.cr = r;
        }
    }
    if kids_len > 0 {
        let clips = n.clip || n.scroll;
        let child_clip = if clips { rect_isect(clip, [gx, gy, gx + gw, gy + gh]) } else { clip };
        // A rounded box that clips rounds its children's corners (its own box, not the clip it
        // is cut by, so a tile half out of a rail keeps a square cut edge).
        let child_r = if clips && radius > 0.0 { Some(([gx, gy, gx + gw, gy + gh], corner_set(r4))) } else { rclip };
        // A focus ring is cut only by scroll containers (their viewport), never by a box that clips
        // for its rounded corners (a settings panel): app.css panels don't clip, so a row's ring
        // shows whole, the last row's included.
        // Along its scroll axis the range keeps room for a ring at both ends (max_scroll /
        // min_scroll); across it a list sits flush with its sides, so the ring may use that much
        // outside the box there.
        let child_ring = if n.scroll {
            let room = crate::theme::ring::OUTER * cx.rem * gs;
            let row = tree.style(id).is_ok_and(|st| matches!(st.flex_direction, taffy::style::FlexDirection::Row | taffy::style::FlexDirection::RowReverse));
            let (ex, ey) = if row { (0.0, room) } else { (room, 0.0) };
            rect_isect(ring_clip, [gx - ex, gy - ey, gx + gw + ex, gy + gh + ey])
        } else {
            ring_clip
        };
        let z = |k: &Id| tree.get_node_context(*k).map(|n| n.z).unwrap_or(0);
        let (mut cx0, mut cy0) = (gx, gy);
        if n.scroll {
            cx0 -= n.scroll_x.get(now) * gsx;
            cy0 -= n.scroll_y.get(now) * gsy;
            if unscaled {
                cx0 = cx0.round();
                cy0 = cy0.round();
            }
        }
        let xf = Xf { sx: gsx, sy: gsy, x: cx0, y: cy0 };
        // Paint order is z, then tree order; the common case (no z set) needs no sort and no
        // allocation.
        if kids_len > 1 && tree.child_ids(id).any(|k| z(&k) != 0) {
            let mut kids: Vec<Id> = tree.child_ids(id).collect();
            kids.sort_by_key(z);
            for k in kids {
                paint_node(tree, k, xf, op, child_clip, child_r, child_ring, cx, ts, images, out, slots);
            }
        } else {
            for k in tree.child_ids(id) {
                paint_node(tree, k, xf, op, child_clip, child_r, child_ring, cx, ts, images, out, slots);
            }
        }
    }
    let ring = if cx.ring_on { n.ring.get(now) } else { 0.0 };
    if ring > 0.001 {
        let gap = crate::theme::ring::GAP * cx.rem * gs;
        let band = (crate::theme::ring::OUTER - crate::theme::ring::GAP) * cx.rem * gs;
        out.push(
            Prim {
                rect,
                clip: ring_clip,
                c0: cx.ring.alpha(ring).arr(),
                c1: cx.ring_gap.alpha(ring).arr(),
                p: [0.0, gap, KIND_RING, op],
                q: [0.0, 0.0, band, 0.0],
                radii: corner_set(r4),
                ..Default::default()
            },
            None,
        );
    }
}

/// Whether a node puts anything on screen of its own (touch hit testing).
fn paints(n: &Node, now: f64) -> bool {
    n.bg.get(now).a > 0.01 || n.grad.is_some() || n.lgrad.is_some() || n.img.is_some() || n.text.is_some() || (n.border > 0.0 && n.border_color.a > 0.01) || n.borders.is_some() || n.slot.is_some()
}

/// `paint_node`'s geometry without the painting: records the topmost accepted node under `p`.
#[allow(clippy::too_many_arguments)]
fn hit_node(tree: &TaffyTree<Node>, id: Id, parent: Xf, opacity: f32, clip: [f32; 4], p: [f32; 2], now: f64, pick: &dyn Fn(Id, &Node) -> bool, best: &mut Option<Id>) {
    let Some(n) = tree.get_node_context(id) else { return };
    if n.hidden {
        return;
    }
    let Ok(l) = tree.layout(id) else { return };
    let (w, h) = (l.size.width, l.size.height);
    let s = n.scale.get(now);
    let (s_x, s_y) = (s * n.scale_x.get(now), s);
    let (ox, oy) = (w * n.origin[0], h * n.origin[1]);
    let lx = l.location.x + n.tx.get(now) + ox - s_x * ox;
    let ly = l.location.y + n.ty.get(now) + oy - s_y * oy;
    let (gsx, gsy) = (parent.sx * s_x, parent.sy * s_y);
    let (gx, gy) = (parent.x + parent.sx * lx, parent.y + parent.sy * ly);
    let (gw, gh) = (w * gsx, h * gsy);
    let op = opacity * n.opacity.get(now);
    if op < 0.002 {
        return;
    }
    let inside = |r: [f32; 4]| p[0] >= r[0] && p[0] < r[2] && p[1] >= r[1] && p[1] < r[3];
    let own = rect_isect(clip, [gx, gy, gx + gw, gy + gh]);
    if inside(own) && pick(id, n) {
        *best = Some(id);
    }
    let clips = n.clip || n.scroll;
    if clips && !inside(own) {
        return;
    }
    let child_clip = if clips { own } else { clip };
    let (mut cx0, mut cy0) = (gx, gy);
    if n.scroll {
        cx0 -= n.scroll_x.get(now) * gsx;
        cy0 -= n.scroll_y.get(now) * gsy;
    }
    let xf = Xf { sx: gsx, sy: gsy, x: cx0, y: cy0 };
    let z = |k: &Id| tree.get_node_context(*k).map(|n| n.z).unwrap_or(0);
    let mut kids: Vec<Id> = tree.child_ids(id).collect();
    if kids.iter().any(|k| z(k) != 0) {
        kids.sort_by_key(z);
    }
    for k in kids {
        hit_node(tree, k, xf, op, child_clip, p, now, pick, best);
    }
}
