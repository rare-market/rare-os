//! launcher/src/ui/controls.tsx as view elements: Button, Toggle, Tabs, Segmented, Dot, LoadBar,
//! Empty, Panel, SectionHead, Rail, Row, HeadRow, InfoCard, CardRow, and the focus looks of
//! app.css (`.tile`, `.tile-wide`, `.btn`, rows) every focusable uses.

use super::icon::{icon_s, steam};
use super::tok;
use super::ui::{s, Track, Ty, Ui, S, T};
use rare_render::anim::{Ease, Keys};
use rare_render::focus::Group;
use rare_render::view::{key, El, Look, Transition};
use rare_render::Color;

// ---------------------------------------------------------------- focus looks

/// `.tile`: the element scales to `k` while focused (1.06; `.tile-wide` 1.03).
pub fn tile(el: El, k: f32) -> El {
    let fl = el.look.clone().scale(k);
    el.focus_look(fl)
}

/// Rows and list items: the fill goes to layer 04 and the text to the primary colour.
pub fn row_look(el: El) -> El {
    let fl = el.look.clone().bg(tok::L4);
    el.focus_look(fl)
}

/// `[data-nav]` with a tag and hints (data-hint-a / -x / -y).
pub fn nav(el: El, tag: u32, a: Option<&str>, x: Option<&str>, def: bool) -> El {
    el.nav(|f| {
        let f = f.tag(tag).hints(a, x, None);
        if def {
            f.default()
        } else {
            f
        }
    })
}

/// A `[data-nav-group]` (not a rail), optionally remembering its last focused child.
pub fn group(name: &str, remember: bool) -> Group {
    let g = Group::new(name, [0.0; 4]);
    if remember {
        g.remember()
    } else {
        g
    }
}

/// An icon that changes colour with focus: two stacked copies cross-fade (the engine has no
/// tint transition yet), the way `.btn:focus` turns the label and its `currentColor` icon dark.
pub fn focus_icon(ui: &mut Ui, name: &str, size: f32, fill: bool, color: Color, focus: Color) -> El {
    let a = icon_s(ui, name, size, color, 1.75, fill).with_style(s().fill().to_style(ui.rem)).transition(Transition::FOCUS);
    let a = a.clone().focus_look(a.look.clone().opacity(0.0));
    let b = icon_s(ui, name, size, focus, 1.75, fill).with_style(s().fill().to_style(ui.rem)).opacity(0.0).transition(Transition::FOCUS);
    let b = b.clone().focus_look(b.look.clone().opacity(1.0));
    ui.bx(s().size(size)).child(a).child(b)
}

// ---------------------------------------------------------------- Button

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Dimmed,
    Ghost,
    Outline,
    Buy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Sm,
    Md,
    Lg,
}

/// The icon in front of a button's label.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BtnIcon {
    /// A lucide icon, size in rem; `fill` for IconPlay.
    Lucide(&'static str, f32, bool),
    Steam(f32),
    /// The View list's check (`opacity-0` when off).
    Check(bool),
}

/// `Button` props: `button(ui, Btn::new("Play", tag).primary().lg())`.
#[derive(Clone, Debug)]
pub struct Btn {
    pub label: String,
    pub tag: u32,
    pub variant: Variant,
    pub size: Size,
    pub icon: Option<BtnIcon>,
    /// data-hint-a
    pub hint: Option<String>,
    pub x: Option<String>,
    pub def: bool,
    pub disabled: bool,
    pub min_w: Option<f32>,
    /// w-full
    pub full: bool,
    /// flex-1: shares the row's free width
    pub grow: bool,
    /// justify-start
    pub start: bool,
    /// gap override (rem)
    pub gap: Option<f32>,
    /// Inside `[data-game]`: Menu opens Options.
    pub game: bool,
    pub key: Option<u64>,
}

impl Btn {
    pub fn new(label: impl Into<String>, tag: u32) -> Btn {
        Btn { label: label.into(), tag, variant: Variant::Secondary, size: Size::Md, icon: None, hint: None, x: None, def: false, disabled: false, min_w: None, full: false, grow: false, start: false, gap: None, game: false, key: None }
    }
    pub fn v(mut self, v: Variant) -> Btn {
        self.variant = v;
        self
    }
    pub fn primary(self) -> Btn {
        self.v(Variant::Primary)
    }
    pub fn ghost(self) -> Btn {
        self.v(Variant::Ghost)
    }
    pub fn size(mut self, s: Size) -> Btn {
        self.size = s;
        self
    }
    pub fn lg(self) -> Btn {
        self.size(Size::Lg)
    }
    pub fn sm(self) -> Btn {
        self.size(Size::Sm)
    }
    pub fn icon(mut self, i: BtnIcon) -> Btn {
        self.icon = Some(i);
        self
    }
    pub fn hint(mut self, h: impl Into<String>) -> Btn {
        self.hint = Some(h.into());
        self
    }
    pub fn x(mut self, h: impl Into<String>) -> Btn {
        self.x = Some(h.into());
        self
    }
    pub fn def(mut self, d: bool) -> Btn {
        self.def = d;
        self
    }
    pub fn disabled(mut self, d: bool) -> Btn {
        self.disabled = d;
        self
    }
    pub fn min_w(mut self, w: f32) -> Btn {
        self.min_w = Some(w);
        self
    }
    /// w-full justify-start (the Options list)
    pub fn full_start(mut self) -> Btn {
        self.full = true;
        self.start = true;
        self
    }
    pub fn gap(mut self, g: f32) -> Btn {
        self.gap = Some(g);
        self
    }
    pub fn game(mut self) -> Btn {
        self.game = true;
        self
    }
    pub fn key(mut self, k: u64) -> Btn {
        self.key = Some(k);
        self
    }
}

/// (fill, text, weight, border) per variant (VARIANTS in controls.tsx).
pub fn variant_colors(v: Variant) -> (Color, Color, u16, Option<Color>) {
    match v {
        Variant::Primary => (tok::BRAND, tok::FG, 600, None),
        Variant::Secondary => (tok::BTN2, tok::FG, 500, None),
        Variant::Dimmed => (tok::BUTTON_DIMMED, tok::MUTED, 500, None),
        Variant::Ghost => (Color::TRANSPARENT, tok::MUTED, 500, None),
        Variant::Outline => (Color::TRANSPARENT, tok::FG, 500, Some(tok::BORDER_LIGHT)),
        Variant::Buy => (tok::BUY, tok::ON_BUY, 600, None),
    }
}

/// rare.market's button, sized for the couch: rounded-chip, focus fills it white with dark text
/// and scales it to 1.04 (`.btn` in app.css).
pub fn button(ui: &mut Ui, b: Btn) -> El {
    let (fill, fg, weight, border) = variant_colors(b.variant);
    let (h, px, ty, gap) = match b.size {
        Size::Sm => (2.25, 0.9, Ty::Small, 0.4),
        Size::Md => (2.85, 1.35, Ty::Body, 0.55),
        Size::Lg => (3.25, 1.75, Ty::Row, 0.6),
    };
    let mut st = s().row().items_center().h(h).px(px).gap(b.gap.unwrap_or(gap));
    st = if b.start { st.justify_start() } else { st.justify_center() };
    if let Some(w) = b.min_w {
        st = st.min_w(w);
    }
    if b.full {
        st = st.wp(1.0);
    }
    if b.grow {
        st = st.grow();
    }
    let mut look = Look::default().bg(fill).radius(ui.r(0.25));
    if let Some(c) = border {
        look = look.border(1.0, c);
    }
    if b.disabled {
        look = look.opacity(0.4);
    }
    let focus = look.clone().bg(tok::FG).scale(1.04);
    let mut el = ui.bx(st).look(look).focus_look(focus);
    match b.icon {
        Some(BtnIcon::Lucide(name, size, fill_on)) => el = el.child(focus_icon(ui, name, size, fill_on, fg, tok::BG)),
        Some(BtnIcon::Steam(size)) => {
            let a = steam(ui, size, fg).with_style(s().fill().to_style(ui.rem)).transition(Transition::FOCUS);
            let a = a.clone().focus_look(a.look.clone().opacity(0.0));
            let c = steam(ui, size, tok::BG).with_style(s().fill().to_style(ui.rem)).opacity(0.0).transition(Transition::FOCUS);
            let c = c.clone().focus_look(c.look.clone().opacity(1.0));
            el = el.child(ui.bx(s().size(size)).child(a).child(c));
        }
        Some(BtnIcon::Check(on)) => {
            let ic = focus_icon(ui, "check", 1.1, false, fg, tok::BG);
            el = el.child(if on { ic } else { ic.opacity(0.0) });
        }
        None => {}
    }
    el = el.child(ui.text(b.label.clone(), T::new(ty).w(weight).c(fg)).focus_color(tok::BG));
    let (hint, x, def, disabled, game, tag) = (b.hint.clone(), b.x.clone(), b.def, b.disabled, b.game, b.tag);
    el = el.nav(move |f| {
        let mut f = f.tag(tag).hints(hint.as_deref(), x.as_deref(), None).disabled(disabled);
        if def {
            f = f.default();
        }
        if game {
            f = f.game();
        }
        f
    });
    if let Some(k) = b.key {
        el = el.key(k);
    }
    el
}

// ---------------------------------------------------------------- Toggle

/// rare.market's switch, display only (the row around it takes focus and A). Colours and the knob
/// move over 150 ms.
pub fn toggle(ui: &mut Ui, on: bool) -> El {
    let tr = Transition { dur: 0.15, ease: Ease::STANDARD, color_ease: Ease::STANDARD };
    let knob = ui
        .bx(s().abs(Some(0.2), None, None, Some(0.0)).size(1.1))
        .look(Look::default().bg(if on { tok::BG } else { tok::ICON2 }).radius(ui.r(0.55)).translate(ui.r(if on { 1.3 } else { 0.2 }), 0.0))
        .transition(tr);
    ui.bx(s().wh(2.6, 1.5)).bg(if on { tok::FG } else { tok::L4 }).radius(ui.r(0.75)).transition(tr).child(knob)
}

// ---------------------------------------------------------------- Tabs, Segmented

#[derive(Clone, Debug)]
pub struct TabItem {
    pub label: String,
    pub count: Option<u32>,
    /// A small dot over the label's top right (something new), taking no width.
    pub dot: bool,
}

impl TabItem {
    pub fn new(label: impl Into<String>) -> TabItem {
        TabItem { label: label.into(), count: None, dot: false }
    }
    pub fn dot(mut self, on: bool) -> TabItem {
        self.dot = on;
        self
    }
    pub fn count(mut self, n: u32) -> TabItem {
        self.count = Some(n);
        self
    }
}

/// A page's bar with this name stays at the top of its scrolling page once the page scrolls past
/// it, like `position: sticky; top: 0` (`pin_bar`, every frame, so it never lags a glide).
pub const PIN_BAR: &str = "pin-bar";

/// Moves the `PIN_BAR` of `view` by how far its page has scrolled past it.
pub fn pin_bar(view: &rare_render::view::View, scene: &mut rare_render::Scene, now: f64) {
    let Some(bar) = view.id(PIN_BAR) else { return };
    let Some(page) = rare_render::scroll::scrollers_above(scene, bar).first().copied() else { return };
    let sy = scene.get(page).scroll_y.get(now);
    let want = (sy - (scene.rect(bar)[1] - scene.rect(page)[1])).max(0.0);
    let n = scene.n(bar);
    if (n.ty.target() - want).abs() > 0.01 || (n.ty.get(now) - want).abs() > 0.01 {
        n.ty.snap(want);
    }
}

/// The active tab's name in its view (`follow_tabs` keeps it in sight).
pub const TAB_ON: &str = "tab-on";

/// rare.market's underline tabs, display only (LB/RB or LT/RT switch them, so they never take
/// focus). `md` is text-body, else text-small. `value` past the end marks none. The row never
/// pushes past its parent: it shrinks to the room it has and scrolls sideways, and after each
/// layout `follow_tabs` brings the active tab fully into view.
pub fn tabs(ui: &mut Ui, st: S, items: &[TabItem], value: usize, md: bool) -> El {
    let tr = Transition { dur: 0.2, ease: Ease::STANDARD, color_ease: Ease::STANDARD };
    let rail = key(&format!("tabs:{}", items.first().map(|i| i.label.as_str()).unwrap_or("")));
    let mut row = ui.bx(s().row().items_stretch().gap(1.25).shrink().min0()).scrolls_x(rail);
    for (i, it) in items.iter().enumerate() {
        let on = i == value;
        let mut span = ui.bx(s().row().items_center().gap(0.4)).child(ui.text(it.label.clone(), T::new(if md { Ty::Body } else { Ty::Small }).w(500).c(if on { tok::FG } else { tok::MUTED })).transition(tr));
        if let Some(c) = it.count {
            span = span.child(ui.text(c.to_string(), T::new(Ty::Meta).tab().c(if on { tok::MUTED } else { tok::MUTED.alpha(0.7) })).transition(tr));
        }
        if it.dot {
            span = span.child(ui.bx(s().abs(Some(-0.1), Some(-0.55), None, None).size(0.4)).bg(tok::FG).radius(ui.r(0.2)));
        }
        // The underline hugs the text (a few px below the baseline), not the full tab height.
        span = span.child(ui.bx(s().abs(None, Some(0.0), Some(-0.3), Some(0.0)).h_px(2.0)).bg(tok::FG).radius(1.0).opacity(if on { 1.0 } else { 0.0 }));
        let mut tab = ui.bx(s().row().items_center()).child(span).key(key(&it.label));
        if on {
            tab = tab.name(TAB_ON);
        }
        row = row.child(tab);
    }
    // Soft edges where tabs run past the row (`follow_tabs` shows them only then).
    let fade = |ui: &Ui, left: bool| {
        let at = if left { s().abs(Some(0.0), None, Some(0.0), Some(0.0)) } else { s().abs(Some(0.0), Some(0.0), Some(0.0), None) };
        let g = rare_render::LinearGradient::new(if left { 270.0 } else { 90.0 }).stop(tok::BG.alpha(0.0), 0.0).stop(tok::BG, 1.0);
        ui.bx(at.w(2.5)).lgrad(g).opacity(0.0).name(if left { TAB_FADE_L } else { TAB_FADE_R })
    };
    ui.bx(st.row().items_stretch().shrink().min0()).child(row).child(fade(ui, true)).child(fade(ui, false))
}

const TAB_FADE_L: &str = "tab-fade-l";
const TAB_FADE_R: &str = "tab-fade-r";

/// After a layout: scrolls a tab row (`tabs`) just enough that its active tab shows whole, with
/// room to see the next one. Only the row moves, never the page around it.
pub fn follow_tabs(view: &rare_render::view::View, scene: &mut rare_render::Scene, now: f64) {
    let Some(id) = view.id(TAB_ON) else { return };
    let Some(rail) = rare_render::scroll::scrollers_above(scene, id).first().copied() else { return };
    let pad = scene.h * 0.04;
    rare_render::scroll::reveal(scene, rail, id, rare_render::scroll::Reveal::Nearest { pad }, now, true);
    let x = rare_render::scroll::aim(scene, rail)[0];
    let max = scene.max_scroll(rail)[0];
    for (name, on) in [(TAB_FADE_L, x > 0.5), (TAB_FADE_R, x < max - 0.5)] {
        if let Some(f) = view.id(name) {
            let v = if on { 1.0 } else { 0.0 };
            if scene.get(f).opacity.target() != v {
                scene.n(f).opacity.set(v, now, 0.15);
            }
        }
    }
}

/// rare.market's segmented control, display only. Per-corner radii are an engine request; the two
/// end segments round all four corners for now.
pub fn segmented(ui: &mut Ui, items: &[&str], value: usize) -> El {
    let mut row = ui.bx(s().row()).style(|st| st.gap = rare_render::taffy::prelude::Size { width: rare_render::taffy::prelude::length(1.0), height: rare_render::taffy::prelude::length(0.0) });
    for (i, it) in items.iter().enumerate() {
        let on = i == value;
        let r = if i == 0 || i + 1 == items.len() { ui.r(0.25) } else { 0.0 };
        row = row.child(
            ui.bx(s().row().items_center().justify_center().h(1.9).min_w(2.4).px(0.6))
                .bg(if on { tok::BUTTON_SECONDARY_ACTIVE } else { tok::BUTTON_SECONDARY })
                .radius(r)
                .child(ui.text(*it, T::new(Ty::Meta).w(500).c(if on { tok::FG } else { tok::MUTED }))),
        );
    }
    row
}

// ---------------------------------------------------------------- Dot, LoadBar

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DotTone {
    Muted,
    /// Renders white: accent blue is for primary actions and focus only.
    Brand,
    Positive,
    Negative,
    Attention,
    Primary,
}

/// The only coloured status marker (no pills). `live` breathes (opacity 1 → 0.35 → 1, 2.4 s).
pub fn dot(ui: &mut Ui, st: S, tone: DotTone, live: bool) -> El {
    let c = match tone {
        DotTone::Muted => tok::ICON2,
        DotTone::Brand | DotTone::Primary => tok::FG,
        DotTone::Positive => tok::POS,
        DotTone::Negative => tok::NEG,
        DotTone::Attention => tok::ATTN,
    };
    let mut el = ui.bx(st.size(0.45)).bg(c).radius(ui.r(0.225));
    if live {
        el = el.name(ui.loop_name(&rare_render::anim::BREATHE, 2.4));
    }
    el
}

/// The loading bar's segment: translateX(-100% → 30% → 110%) with scaleX(0.4 → 0.6 → 0.4), 1.1 s
/// cubic-bezier(0.4, 0, 0.2, 1) (rare.market's loading-bar.tsx).
pub static LOAD_X: Keys = Keys { frames: &[(0.0, -1.0), (0.5, 0.3), (1.0, 1.1)], ease: Ease(0.4, 0.0, 0.2, 1.0) };
pub static LOAD_SX: Keys = Keys { frames: &[(0.0, 0.4), (0.5, 0.6), (1.0, 0.4)], ease: Ease(0.4, 0.0, 0.2, 1.0) };

// ---------------------------------------------------------------- Empty, Panel, SectionHead

/// Calm empty state: a sentence, an optional line, then `actions` (one at most, by the rules).
pub fn empty(ui: &mut Ui, st: S, title: &str, body: Option<&str>, actions: Vec<El>) -> El {
    let mut b = ui
        .bx(st.col().items_start().gap(0.5).px(1.5).py(1.3))
        .look(Look::default().bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR))
        .child(ui.text(title, T::new(Ty::Row).w(600)));
    if let Some(body) = body {
        b = b.child(ui.text_s(s().max_w(34.0), body, T::new(Ty::Small).c(tok::MUTED).wrap()));
    }
    if !actions.is_empty() {
        b = b.child(ui.bx(s().row().gap(0.75).mt(0.5)).kids(actions));
    }
    b
}

/// rare.market's home table card: rounded-panel bg-l1.
pub fn panel(ui: &Ui, st: S) -> El {
    ui.bx(st).bg(tok::L1).radius(ui.r(1.0))
}

/// A section title with a muted note, and an optional right side (small muted text).
pub fn section_head(ui: &mut Ui, st: S, title: &str, note: Option<&str>, right: Option<El>) -> El {
    let mut row = ui.bx(st.row().items_center().gap(0.75)).child(ui.text(title, T::new(Ty::Row).w(600).track(-0.01)));
    if let Some(n) = note {
        row = row.child(ui.text(n, T::new(Ty::Small).c(tok::MUTED)));
    }
    if let Some(r) = right {
        row = row.child(ui.bx(s().row().items_center().gap(0.75).ml_auto()).child(r));
    }
    row
}

// ---------------------------------------------------------------- Rail, CardRow

/// A horizontal shelf of tiles under an optional title: `[data-rail]`, scrolled by focus, keeping
/// the 2.5rem gutter clear, with 1.4rem above and below (pulled back by negative margins) so the
/// ring and the scale-up are never clipped.
pub fn rail(ui: &mut Ui, st: S, id: &str, title: Option<&str>, note: Option<&str>, tiles: Vec<El>) -> El {
    let mut section = ui.bx(st.col()).section();
    if let Some(t) = title {
        section = section.child(section_head(ui, s().mb(0.9).px(2.5), t, note, None));
    }
    let strip = ui
        .bx(s().row().items_start().gap(1.0).px(2.5).py(1.4).my(-1.4))
        .scrolls_x(key(&format!("rail:{id}")))
        .group(Group::new(id, [0.0; 4]).rail())
        .kids(tiles);
    section.child(strip)
}

/// A row of InfoCards that scrolls sideways under focus (a rail without a title or gutter).
pub fn card_row(ui: &mut Ui, st: S, id: &str, cards: Vec<El>) -> El {
    ui.bx(st.row().gap(1.0).py(1.4).my(-1.4)).scrolls_x(key(&format!("rail:{id}"))).group(Group::new(id, [0.0; 4]).rail()).kids(cards)
}

// ---------------------------------------------------------------- Row, HeadRow

/// A focusable table row: hairline-free, 3.25rem, cells on a shared grid; `.trow`
/// gives the columns a 0.5rem gap. The cells shrink to their column and clip (`cell`).
pub fn row(ui: &mut Ui, cols: &[Track], tag: u32, hint: Option<&str>, cells: Vec<El>) -> El {
    let el = ui.bx(s().grid(cols).items_center().h(3.25).wp(1.0).gap_xy(0.5, 0.0).px(0.75)).radius(ui.r(0.5)).kids(cells);
    nav(row_look(el), tag, hint, None, false)
}

pub fn head_row(ui: &mut Ui, cols: &[Track], cells: Vec<El>) -> El {
    ui.bx(s().grid(cols).items_center().h(2.0).gap_xy(0.5, 0.0).px(0.75)).kids(cells)
}

/// A table cell: shrinks to its column, clips; `end` right-aligns it.
pub fn cell(ui: &Ui, end: bool) -> El {
    let mut st = s().row().items_center().min0().shrink();
    if end {
        st = st.justify_end();
    }
    ui.bx(st).clip()
}

// ---------------------------------------------------------------- InfoCard

/// An activity card: a small label, one big value, a muted line. Focusable
/// (`.tile.tile-wide`) when `tag` is given.
pub fn info_card(ui: &mut Ui, label: &str, value: &str, sub: Option<&str>, tag: Option<u32>) -> El {
    let mut c = ui
        .bx(s().col().h(6.0).w(14.0).px(1.0).py(0.875))
        .bg(tok::L1)
        .radius(ui.r(0.5))
        .child(ui.text(label, T::new(Ty::Label).w(500).c(tok::MUTED)))
        .child(ui.text_s(s().mt_auto(), value, T::new(Ty::Title).w(600).tab().trunc()));
    if let Some(sub) = sub {
        c = c.child(ui.text_s(s().mt(0.15), sub, T::new(Ty::Meta).c(tok::MUTED).trunc()));
    }
    match tag {
        Some(t) => nav(tile(c, 1.03), t, None, None, false),
        None => c,
    }
}
