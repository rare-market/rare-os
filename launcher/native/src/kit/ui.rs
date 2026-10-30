//! The build context and the two builders every kit function and screen uses until `tw!`
//! (rare-tw, WS 3) lands: `S` for layout in rem, the Tailwind way (`s().row().items_center()
//! .h(4.25).px(2.5)` is `flex h-[4.25rem] items-center px-[2.5rem]`), and `T` for text
//! (`T::new(Ty::Body).w(500)` is `text-body font-medium`). Both turn into engine values at the
//! page's rem, so a 1280 x 800 window gets 16 px per rem and the SuiPlay 24.

use super::art::Arts;
use super::tok;
use rare_render::taffy::prelude::*;
use rare_render::taffy::style::{AlignItems, AlignSelf, JustifyContent, Position};
use rare_render::taffy::{GridTemplateComponent, MaxTrackSizingFunction, MinTrackSizingFunction, TrackSizingFunction};
use rare_render::text::TextAlign;
use rare_render::view::El;
use rare_render::{Color, ImageLoader, TextStyle};

/// What a view build reads besides the state: the rem, the image loader (icons and art are
/// requested while building), art fallbacks, and the tags A maps back to actions.
pub struct Ui<'a> {
    pub rem: f32,
    /// Window size in px.
    pub w: f32,
    pub h: f32,
    pub img: &'a mut ImageLoader,
    pub arts: &'a mut Arts,
    /// Frame time of the build (clock::secs).
    pub now: f64,
    /// Elements that loop (the breathing dot): `(name, keys, period)`, started after the update.
    pub loops: Vec<(&'static str, &'static rare_render::anim::Keys, f32)>,
}

impl<'a> Ui<'a> {
    pub fn new(rem: f32, w: f32, h: f32, img: &'a mut ImageLoader, arts: &'a mut Arts, now: f64) -> Ui<'a> {
        Ui { rem, w, h, img, arts, now, loops: Vec::new() }
    }

    /// `v` rem in px.
    pub fn r(&self, v: f32) -> f32 {
        v * self.rem
    }

    /// A box with this layout.
    pub fn bx(&self, s: S) -> El {
        El::new(s.to_style(self.rem))
    }

    /// A text leaf: `ui.text("Play", T::new(Ty::Row).w(600))`. Truncating, wrapping and aligned text
    /// gets `min-w-0` so it can give way in a row.
    pub fn text(&self, text: impl Into<String>, t: T) -> El {
        self.text_s(s(), text, t)
    }

    pub fn text_s(&self, s: S, text: impl Into<String>, t: T) -> El {
        let s = if t.truncate { s.shrink().min0() } else { s };
        let mut style = s.to_style(self.rem);
        if t.truncate {
            // The engine ellipsizes text that exactly fits its measured width (a WS 2 fix): 2 px of
            // padding the margin takes back give the final shaping room without moving anything.
            style.padding.right = rare_render::taffy::prelude::length(2.0);
            style.margin.right = rare_render::taffy::prelude::LengthPercentageAuto::length(-2.0);
        }
        let mut el = El::text(text, t.style(self.rem), t.color).with_style(style);
        if t.opacity < 1.0 {
            el = el.opacity(t.opacity);
        }
        el
    }

    /// The name of the `i`th looping element of this build (names must be `'static`).
    pub fn loop_name(&mut self, keys: &'static rare_render::anim::Keys, period: f32) -> &'static str {
        const NAMES: [&str; 24] = [
            "loop0", "loop1", "loop2", "loop3", "loop4", "loop5", "loop6", "loop7", "loop8", "loop9", "loop10", "loop11", "loop12", "loop13", "loop14", "loop15", "loop16", "loop17", "loop18", "loop19", "loop20", "loop21",
            "loop22", "loop23",
        ];
        let n = NAMES[self.loops.len().min(NAMES.len() - 1)];
        self.loops.push((n, keys, period));
        n
    }
}

/// The type scale (DESIGN.md "Type", app.css `--text-*`): size and line height in rem, tracking in em.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ty {
    Label,
    Meta,
    Small,
    Body,
    Row,
    Title,
    Display,
    Hero,
    /// `text-[size]` with `leading-[line]`, both rem. Without a leading the browser's default
    /// (1.5 times the size, Tailwind's `html { line-height: 1.5 }`) applies: `Ty::px(size)`.
    Px(f32, f32),
}

impl Ty {
    /// `text-[x rem]` with the inherited line height (1.5).
    pub fn px(size: f32) -> Ty {
        Ty::Px(size, size * 1.5)
    }

    /// (size, line height) in rem, and letter spacing in em.
    pub fn metrics(self) -> (f32, f32, f32) {
        match self {
            Ty::Label => (0.75, 1.0, 0.0),
            Ty::Meta => (0.8125, 1.125, 0.0),
            Ty::Small => (0.875, 1.25, 0.0),
            Ty::Body => (0.9375, 1.375, 0.0),
            Ty::Row => (1.0625, 1.5, 0.0),
            Ty::Title => (1.375, 1.75, -0.01),
            Ty::Display => (2.0, 2.25, -0.02),
            Ty::Hero => (2.75, 3.0, -0.03),
            Ty::Px(s, l) => (s, l, 0.0),
        }
    }
}

/// Text look: size, weight, colour and the Tailwind text utilities the launcher uses.
#[derive(Clone, Copy, Debug)]
pub struct T {
    pub ty: Ty,
    pub weight: u16,
    pub color: Color,
    pub tabular: bool,
    pub truncate: bool,
    pub wrap: bool,
    pub clamp: u16,
    pub mono: bool,
    pub align: TextAlign,
    pub tracking: Option<f32>,
    pub leading: Option<f32>,
    pub opacity: f32,
}

impl T {
    pub fn new(ty: Ty) -> T {
        T { ty, weight: 400, color: tok::FG, tabular: false, truncate: false, wrap: false, clamp: 0, mono: false, align: TextAlign::Left, tracking: None, leading: None, opacity: 1.0 }
    }
    /// font-medium is 500, font-semibold 600, font-bold 700.
    pub fn w(mut self, w: u16) -> T {
        self.weight = w;
        self
    }
    pub fn c(mut self, c: Color) -> T {
        self.color = c;
        self
    }
    /// tabular-nums
    pub fn tab(mut self) -> T {
        self.tabular = true;
        self
    }
    /// truncate
    pub fn trunc(mut self) -> T {
        self.truncate = true;
        self
    }
    pub fn wrap(mut self) -> T {
        self.wrap = true;
        self
    }
    /// line-clamp-N
    pub fn clamp(mut self, n: u16) -> T {
        self.clamp = n;
        self.wrap = n > 0;
        self
    }
    /// `.num`: Fira Code, tabular.
    pub fn mono(mut self) -> T {
        self.mono = true;
        self.tabular = true;
        self
    }
    pub fn center(mut self) -> T {
        self.align = TextAlign::Center;
        self
    }
    pub fn right(mut self) -> T {
        self.align = TextAlign::Right;
        self
    }
    /// tracking-[x em]
    pub fn track(mut self, em: f32) -> T {
        self.tracking = Some(em);
        self
    }
    /// leading-[x rem]
    pub fn lead(mut self, rem: f32) -> T {
        self.leading = Some(rem);
        self
    }
    /// The whole text at this opacity (`opacity-50`, or `text-fg/60` without a colour change).
    pub fn op(mut self, o: f32) -> T {
        self.opacity = o;
        self
    }
    pub fn style(&self, rem: f32) -> TextStyle {
        let (size, line, track) = self.ty.metrics();
        let line = self.leading.unwrap_or(line);
        let mut st = TextStyle::new(size * rem, self.weight).line((line * rem).round()).tracking(self.tracking.unwrap_or(track)).align(self.align);
        st.tabular = self.tabular;
        st.truncate = self.truncate;
        st.wrap = self.wrap;
        st.clamp = self.clamp;
        if self.mono {
            st = st.mono();
        }
        st
    }
}

/// A grid track (`grid-cols-[1.25rem_minmax(0,1fr)_6.5rem]`), sizes in rem.
#[derive(Clone, Copy, Debug)]
pub enum Track {
    Rem(f32),
    /// minmax(0, Nfr)
    Fr(f32),
    /// minmax(rem, Nfr)
    MinFr(f32, f32),
    Auto,
}

#[derive(Clone, Copy)]
enum RemField {
    W(f32),
    H(f32),
    MinW(f32),
    MinH(f32),
    MaxW(f32),
    MaxH(f32),
    Gap(f32, f32),
    Pad(f32, u8),
    Mar(f32, u8),
    Inset(Option<f32>, Option<f32>, Option<f32>, Option<f32>),
}

const TOP: u8 = 1;
const RIGHT: u8 = 2;
const BOTTOM: u8 = 4;
const LEFT: u8 = 8;

/// Layout in rem: a Taffy style whose lengths become px at the page's rem. Every node is a
/// containing block (position: relative) and does not shrink unless asked (`shrink`, `grow`),
/// the way the launcher's markup behaves under `shrink-0` and `relative`.
#[derive(Clone)]
pub struct S {
    st: Style,
    grid_cols: Option<Vec<Track>>,
    rem_fields: Vec<RemField>,
}

pub fn s() -> S {
    S { st: Style { flex_shrink: 0.0, position: Position::Relative, ..Default::default() }, grid_cols: None, rem_fields: Vec::new() }
}

impl S {
    fn f(mut self, f: RemField) -> S {
        self.rem_fields.push(f);
        self
    }
    pub fn row(mut self) -> S {
        self.st.display = Display::Flex;
        self.st.flex_direction = FlexDirection::Row;
        self
    }
    pub fn col(mut self) -> S {
        self.st.display = Display::Flex;
        self.st.flex_direction = FlexDirection::Column;
        self
    }
    /// display: grid with these columns.
    pub fn grid(mut self, cols: &[Track]) -> S {
        self.st.display = Display::Grid;
        self.grid_cols = Some(cols.to_vec());
        self
    }
    /// display: block (a column whose children stretch), the default for a plain div.
    pub fn block(mut self) -> S {
        self.st.display = Display::Block;
        self
    }
    pub fn w(self, v: f32) -> S {
        self.f(RemField::W(v))
    }
    pub fn h(self, v: f32) -> S {
        self.f(RemField::H(v))
    }
    pub fn wh(self, w: f32, h: f32) -> S {
        self.w(w).h(h)
    }
    /// `size-[v]`
    pub fn size(self, v: f32) -> S {
        self.w(v).h(v)
    }
    pub fn w_px(mut self, v: f32) -> S {
        self.st.size.width = length(v);
        self
    }
    pub fn h_px(mut self, v: f32) -> S {
        self.st.size.height = length(v);
        self
    }
    /// w-[p%] (0..1)
    pub fn wp(mut self, p: f32) -> S {
        self.st.size.width = percent(p);
        self
    }
    pub fn hp(mut self, p: f32) -> S {
        self.st.size.height = percent(p);
        self
    }
    /// w-full h-full
    pub fn full(self) -> S {
        self.wp(1.0).hp(1.0)
    }
    pub fn min_w(self, v: f32) -> S {
        self.f(RemField::MinW(v))
    }
    pub fn min_h(self, v: f32) -> S {
        self.f(RemField::MinH(v))
    }
    pub fn max_w(self, v: f32) -> S {
        self.f(RemField::MaxW(v))
    }
    pub fn max_h(self, v: f32) -> S {
        self.f(RemField::MaxH(v))
    }
    pub fn max_wp(mut self, p: f32) -> S {
        self.st.max_size.width = percent(p);
        self
    }
    pub fn max_hp(mut self, p: f32) -> S {
        self.st.max_size.height = percent(p);
        self
    }
    /// min-w-0
    pub fn min0(mut self) -> S {
        self.st.min_size.width = length(0.0);
        self
    }
    /// min-h-0
    pub fn min_h0(mut self) -> S {
        self.st.min_size.height = length(0.0);
        self
    }
    /// aspect-[w/h]
    pub fn aspect(mut self, r: f32) -> S {
        self.st.aspect_ratio = Some(r);
        self
    }
    /// flex-1 (with min-w-0 min-h-0, as the launcher writes it)
    pub fn grow(mut self) -> S {
        self.st.flex_grow = 1.0;
        self.st.flex_shrink = 1.0;
        self.st.flex_basis = length(0.0);
        self.st.min_size.width = length(0.0);
        self.st.min_size.height = length(0.0);
        self
    }
    /// flex-[n]
    pub fn flex(mut self, n: f32) -> S {
        self.st.flex_grow = n;
        self.st.flex_shrink = 1.0;
        self.st.flex_basis = length(0.0);
        self
    }
    /// flex-shrink: 1
    pub fn shrink(mut self) -> S {
        self.st.flex_shrink = 1.0;
        self
    }
    pub fn gap(self, v: f32) -> S {
        self.f(RemField::Gap(v, v))
    }
    /// gap-x / gap-y
    pub fn gap_xy(self, x: f32, y: f32) -> S {
        self.f(RemField::Gap(x, y))
    }
    pub fn p(self, v: f32) -> S {
        self.f(RemField::Pad(v, TOP | RIGHT | BOTTOM | LEFT))
    }
    pub fn px(self, v: f32) -> S {
        self.f(RemField::Pad(v, RIGHT | LEFT))
    }
    pub fn py(self, v: f32) -> S {
        self.f(RemField::Pad(v, TOP | BOTTOM))
    }
    pub fn pt(self, v: f32) -> S {
        self.f(RemField::Pad(v, TOP))
    }
    pub fn pb(self, v: f32) -> S {
        self.f(RemField::Pad(v, BOTTOM))
    }
    pub fn pl(self, v: f32) -> S {
        self.f(RemField::Pad(v, LEFT))
    }
    pub fn pr(self, v: f32) -> S {
        self.f(RemField::Pad(v, RIGHT))
    }
    pub fn m(self, v: f32) -> S {
        self.f(RemField::Mar(v, TOP | RIGHT | BOTTOM | LEFT))
    }
    pub fn mt(self, v: f32) -> S {
        self.f(RemField::Mar(v, TOP))
    }
    pub fn mb(self, v: f32) -> S {
        self.f(RemField::Mar(v, BOTTOM))
    }
    pub fn ml(self, v: f32) -> S {
        self.f(RemField::Mar(v, LEFT))
    }
    pub fn mr(self, v: f32) -> S {
        self.f(RemField::Mar(v, RIGHT))
    }
    pub fn mx(self, v: f32) -> S {
        self.f(RemField::Mar(v, LEFT | RIGHT))
    }
    pub fn my(self, v: f32) -> S {
        self.f(RemField::Mar(v, TOP | BOTTOM))
    }
    pub fn ml_auto(mut self) -> S {
        self.st.margin.left = auto();
        self
    }
    pub fn mr_auto(mut self) -> S {
        self.st.margin.right = auto();
        self
    }
    pub fn mt_auto(mut self) -> S {
        self.st.margin.top = auto();
        self
    }
    pub fn mx_auto(mut self) -> S {
        self.st.margin.left = auto();
        self.st.margin.right = auto();
        self
    }
    pub fn items_center(mut self) -> S {
        self.st.align_items = AlignItems::CENTER;
        self
    }
    pub fn items_end(mut self) -> S {
        self.st.align_items = AlignItems::FLEX_END;
        self
    }
    pub fn items_start(mut self) -> S {
        self.st.align_items = AlignItems::FLEX_START;
        self
    }
    pub fn items_stretch(mut self) -> S {
        self.st.align_items = AlignItems::STRETCH;
        self
    }
    pub fn justify_center(mut self) -> S {
        self.st.justify_content = JustifyContent::CENTER;
        self
    }
    pub fn justify_end(mut self) -> S {
        self.st.justify_content = JustifyContent::FLEX_END;
        self
    }
    pub fn justify_between(mut self) -> S {
        self.st.justify_content = JustifyContent::SPACE_BETWEEN;
        self
    }
    pub fn justify_start(mut self) -> S {
        self.st.justify_content = JustifyContent::FLEX_START;
        self
    }
    pub fn self_end(mut self) -> S {
        self.st.align_self = Some(AlignSelf::FLEX_END);
        self
    }
    pub fn self_start(mut self) -> S {
        self.st.align_self = Some(AlignSelf::FLEX_START);
        self
    }
    pub fn self_center(mut self) -> S {
        self.st.align_self = Some(AlignSelf::CENTER);
        self
    }
    pub fn self_stretch(mut self) -> S {
        self.st.align_self = Some(AlignSelf::STRETCH);
        self
    }
    pub fn wrap(mut self) -> S {
        self.st.flex_wrap = FlexWrap::Wrap;
        self
    }
    /// position: absolute with insets in rem (None = auto).
    pub fn abs(mut self, top: Option<f32>, right: Option<f32>, bottom: Option<f32>, left: Option<f32>) -> S {
        self.st.position = Position::Absolute;
        self.f(RemField::Inset(top, right, bottom, left))
    }
    /// absolute inset-0
    pub fn fill(self) -> S {
        self.abs(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
    }
    /// position: absolute with percentage insets (0..1); None is auto.
    pub fn abs_pct(mut self, top: Option<f32>, right: Option<f32>, bottom: Option<f32>, left: Option<f32>) -> S {
        self.st.position = Position::Absolute;
        let v = |x: Option<f32>| x.map(percent).unwrap_or(auto());
        self.st.inset = Rect { top: v(top), right: v(right), bottom: v(bottom), left: v(left) };
        self
    }
    /// display: none
    pub fn none(mut self) -> S {
        self.st.display = Display::None;
        self
    }
    /// Any other Taffy field.
    pub fn with(mut self, f: impl FnOnce(&mut Style)) -> S {
        f(&mut self.st);
        self
    }

    /// The Taffy style with rem lengths in px.
    pub fn to_style(mut self, rem: f32) -> Style {
        let l = |v: f32| -> LengthPercentage { length(v * rem) };
        let la = |v: f32| -> LengthPercentageAuto { length(v * rem) };
        for f in std::mem::take(&mut self.rem_fields) {
            match f {
                RemField::W(v) => self.st.size.width = length(v * rem),
                RemField::H(v) => self.st.size.height = length(v * rem),
                RemField::MinW(v) => self.st.min_size.width = length(v * rem),
                RemField::MinH(v) => self.st.min_size.height = length(v * rem),
                RemField::MaxW(v) => self.st.max_size.width = length(v * rem),
                RemField::MaxH(v) => self.st.max_size.height = length(v * rem),
                RemField::Gap(x, y) => self.st.gap = Size { width: l(x), height: l(y) },
                RemField::Pad(v, m) => {
                    if m & TOP != 0 {
                        self.st.padding.top = l(v)
                    }
                    if m & RIGHT != 0 {
                        self.st.padding.right = l(v)
                    }
                    if m & BOTTOM != 0 {
                        self.st.padding.bottom = l(v)
                    }
                    if m & LEFT != 0 {
                        self.st.padding.left = l(v)
                    }
                }
                RemField::Mar(v, m) => {
                    if m & TOP != 0 {
                        self.st.margin.top = la(v)
                    }
                    if m & RIGHT != 0 {
                        self.st.margin.right = la(v)
                    }
                    if m & BOTTOM != 0 {
                        self.st.margin.bottom = la(v)
                    }
                    if m & LEFT != 0 {
                        self.st.margin.left = la(v)
                    }
                }
                RemField::Inset(t, r, b, lf) => {
                    let v = |x: Option<f32>| x.map(la).unwrap_or(auto());
                    self.st.inset = Rect { top: v(t), right: v(r), bottom: v(b), left: v(lf) };
                }
            }
        }
        if let Some(cols) = self.grid_cols.take() {
            self.st.grid_template_columns = cols
                .iter()
                .map(|t| {
                    let f: TrackSizingFunction = match *t {
                        Track::Rem(v) => length(v * rem),
                        Track::Fr(v) => minmax(MinTrackSizingFunction::length(0.0), MaxTrackSizingFunction::fr(v)),
                        Track::MinFr(m, v) => minmax(MinTrackSizingFunction::length(m * rem), MaxTrackSizingFunction::fr(v)),
                        Track::Auto => auto(),
                    };
                    GridTemplateComponent::Single(f)
                })
                .collect();
        }
        self.st
    }
}
