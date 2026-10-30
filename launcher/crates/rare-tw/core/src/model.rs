//! What a class string means, as plain data the tw! macro writes out as a `static`.
//!
//! Every field is the CSS the web launcher gets from the same classes, in CSS units (rem, px, em,
//! %), unresolved: the engine resolves them against the live rem (the window's `font-size`) when it
//! builds the node. `None` means "these classes don't set it" (the element keeps its default or
//! inherits, exactly as in CSS). Lengths follow Tailwind v4: spacing is 0.25rem steps, `px` is 1px.

use std::borrow::Cow;

/// A CSS length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Auto,
    Px(f32),
    Rem(f32),
    /// Relative to the element's own font size (letter-spacing, `text-[0.9em]` against the parent's).
    Em(f32),
    /// Percent (0..100) of the containing block (or of the element itself for translate).
    Pct(f32),
}

impl Len {
    pub const ZERO: Len = Len::Px(0.0);
    /// `rounded-full`: CSS's `calc(infinity * 1px)`; the renderer clamps a radius to half the box.
    pub const FULL_RADIUS: Len = Len::Px(f32::INFINITY);

    pub const fn neg(self) -> Len {
        match self {
            Len::Auto => Len::Auto,
            Len::Px(v) => Len::Px(-v),
            Len::Rem(v) => Len::Rem(-v),
            Len::Em(v) => Len::Em(-v),
            Len::Pct(v) => Len::Pct(-v),
        }
    }

    /// Pixels at `rem` px per rem, `em` px per em, percent of `basis`. `None` for `auto`.
    pub fn px(self, rem: f32, em: f32, basis: f32) -> Option<f32> {
        match self {
            Len::Auto => None,
            Len::Px(v) => Some(v),
            Len::Rem(v) => Some(v * rem),
            Len::Em(v) => Some(v * em),
            Len::Pct(v) => Some(v / 100.0 * basis),
        }
    }
}

/// sRGB colour, straight alpha, 0..1 (the same encoding as rare_render::Color).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }
    /// Tailwind's `/60`: the same colour at `a` times its alpha.
    pub const fn alpha(self, a: f32) -> Rgba {
        Rgba { a: self.a * a, ..self }
    }
    pub fn to_u8(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Display {
    Block,
    Inline,
    InlineBlock,
    Flex,
    InlineFlex,
    Grid,
    Contents,
    None,
    /// `line-clamp-N`: display -webkit-box, vertical, clamped (see `Decls::line_clamp`).
    LineClamp,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlexDirection {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
}

/// align-items / align-self.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Start,
    End,
    Center,
    Stretch,
    Baseline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Justify {
    Start,
    End,
    Center,
    Between,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overflow {
    Visible,
    Hidden,
    Auto,
}

/// One grid track size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sizing {
    Len(Len),
    Fr(f32),
    Auto,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Track {
    One(Sizing),
    MinMax(Sizing, Sizing),
    /// `repeat(n, ...)`.
    Repeat(u16, Cow<'static, [Track]>),
    /// `repeat(auto-fill, ...)`.
    AutoFill(Cow<'static, [Track]>),
}

/// `grid-column` / `grid-row` placement start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Line {
    Span(u16),
    At(i16),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    /// A multiple of the font size (inherits as the number).
    Num(f32),
    /// A length (inherits as the computed length).
    Len(Len),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Family {
    Sans,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WhiteSpace {
    Normal,
    NoWrap,
    PreWrap,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decoration {
    None,
    Underline,
    LineThrough,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    Cover,
    Contain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    Lighten,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UserSelect {
    None,
    Text,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutlineStyle {
    None,
    Solid,
}

/// One box-shadow / drop-shadow / text-shadow layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: Len,
    pub y: Len,
    pub blur: Len,
    pub spread: Len,
    /// The layer's own colour; `Decls::shadow_color` replaces it (Tailwind's `shadow-black/30`).
    pub color: Rgba,
}

/// A gradient stop: colour and position in percent (None = spread evenly, as CSS).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    pub color: Rgba,
    pub pos: Option<f32>,
}

/// background-image.
#[derive(Clone, Debug, PartialEq)]
pub enum BgImage {
    /// `bg-gradient-to-*`: the stops come from `from-*`, `via-*`, `to-*` (Decls::grad), interpolated
    /// in oklab like Tailwind v4. Angle in CSS degrees (to top = 0, to right = 90).
    Tailwind { angle: f32 },
    /// `bg-[linear-gradient(...)]`, interpolated in sRGB (premultiplied) like plain CSS.
    Linear { angle: f32, stops: Cow<'static, [Stop]> },
}

/// A gradient ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub angle: f32,
    pub stops: Vec<Stop>,
    pub oklab: bool,
}

/// One transitioned property (transition-property lists them in the order written).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TProp {
    Color,
    BackgroundColor,
    BorderColor,
    OutlineColor,
    TextDecorationColor,
    Fill,
    Stroke,
    GradientFrom,
    GradientVia,
    GradientTo,
    Opacity,
    BoxShadow,
    Transform,
    Translate,
    Scale,
    Rotate,
    Left,
    Width,
}

impl TProp {
    pub fn css(self) -> &'static str {
        match self {
            TProp::Color => "color",
            TProp::BackgroundColor => "background-color",
            TProp::BorderColor => "border-color",
            TProp::OutlineColor => "outline-color",
            TProp::TextDecorationColor => "text-decoration-color",
            TProp::Fill => "fill",
            TProp::Stroke => "stroke",
            TProp::GradientFrom => "--tw-gradient-from",
            TProp::GradientVia => "--tw-gradient-via",
            TProp::GradientTo => "--tw-gradient-to",
            TProp::Opacity => "opacity",
            TProp::BoxShadow => "box-shadow",
            TProp::Transform => "transform",
            TProp::Translate => "translate",
            TProp::Scale => "scale",
            TProp::Rotate => "rotate",
            TProp::Left => "left",
            TProp::Width => "width",
        }
    }
    pub fn from_css(s: &str) -> Option<TProp> {
        const ALL: [TProp; 18] = [
            TProp::Color,
            TProp::BackgroundColor,
            TProp::BorderColor,
            TProp::OutlineColor,
            TProp::TextDecorationColor,
            TProp::Fill,
            TProp::Stroke,
            TProp::GradientFrom,
            TProp::GradientVia,
            TProp::GradientTo,
            TProp::Opacity,
            TProp::BoxShadow,
            TProp::Transform,
            TProp::Translate,
            TProp::Scale,
            TProp::Rotate,
            TProp::Left,
            TProp::Width,
        ];
        ALL.into_iter().find(|p| p.css() == s)
    }
}

/// Keyframe animations from app.css (and Tailwind's animate-pulse).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anim {
    Pulse,
    FadeIn,
    SlideInRight,
    SlideUp,
    LoadBar,
    NumRollUp,
    NumRollDown,
    TintUp,
    TintDown,
    Breathe,
    Rise,
    Fade,
    Pop,
    BootSweep,
}

/// A keyframe animation as app.css plays it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Animation {
    pub kind: Anim,
    pub ms: f32,
    pub ease: [f32; 4],
    pub delay_ms: f32,
    pub infinite: bool,
}

/// Classes that carry behaviour rather than properties (app.css component classes), so the kit and
/// the focus system know what the web launcher's CSS does with them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Marks(pub u32);

impl Marks {
    pub const NONE: Marks = Marks(0);
    /// `.tile`: focus scales 1.06 (focus::TILE_SCALE).
    pub const TILE: Marks = Marks(1 << 0);
    /// `.tile-wide`: focus scales 1.03.
    pub const TILE_WIDE: Marks = Marks(1 << 1);
    /// `.btn`: focus fills fg, text bg, scale 1.04.
    pub const BTN: Marks = Marks(1 << 2);
    /// `.strip-tile`: Home's strip tile (grows 45 % from its corner, later tiles slide).
    pub const STRIP_TILE: Marks = Marks(1 << 3);
    /// `.strip-label`: shows when its strip tile has focus.
    pub const STRIP_LABEL: Marks = Marks(1 << 4);
    /// `.focus-text`: brightens to fg when its focusable ancestor has focus.
    pub const FOCUS_TEXT: Marks = Marks(1 << 5);
    /// `.num`: mono, tabular, no ligatures (also in the declarations).
    pub const NUM: Marks = Marks(1 << 6);
    /// `.trow`: a table row; every child shrinks, clips with an ellipsis and uses tabular figures.
    pub const TROW: Marks = Marks(1 << 7);
    /// `.cv-row` / `.cv-cell`: off-screen rows skip layout and paint (content-visibility).
    pub const CV_ROW: Marks = Marks(1 << 8);
    pub const CV_CELL: Marks = Marks(1 << 9);
    /// `.backdrop-layer`: a backdrop art layer that cross-fades on opacity (motion SLOW).
    pub const BACKDROP_LAYER: Marks = Marks(1 << 10);
    /// `.splash` / `.splash-gem`: the boot splash (index.html), equal to Plymouth's frame.
    pub const SPLASH: Marks = Marks(1 << 11);
    pub const SPLASH_GEM: Marks = Marks(1 << 12);
    /// `.group`: a Tailwind group parent.
    pub const GROUP: Marks = Marks(1 << 13);
    /// `.load-bar`: the thin loading bar (its ::after segment slides).
    pub const LOAD_BAR: Marks = Marks(1 << 14);

    pub const fn contains(self, m: Marks) -> bool {
        self.0 & m.0 == m.0
    }
    pub const fn union(self, m: Marks) -> Marks {
        Marks(self.0 | m.0)
    }
}

/// When a state's declarations apply.
#[derive(Clone, Debug, PartialEq)]
pub enum When {
    /// `focus-visible:` (in Rare: the element has controller focus).
    FocusVisible,
    /// `first:` / `last:`: first or last child of its parent.
    First,
    Last,
    /// `disabled:`
    Disabled,
    /// `placeholder:`: the placeholder text of an input.
    Placeholder,
    /// `space-*` / `divide-*`: every child but the last.
    ChildrenNotLast,
    /// `.trow > *`: every child.
    Children,
    /// `[&>svg]:` / `[&>span]:` / `[&>div]:`: direct children of that kind.
    Child(Cow<'static, str>),
    /// `[[data-area=chart]:focus_&]:`: an ancestor with that attribute has focus.
    AncestorFocus { attr: Cow<'static, str>, value: Cow<'static, str> },
}

macro_rules! decls {
    ($( $(#[$m:meta])* $name:ident : $ty:ty = $none:expr ),* $(,)?) => {
        /// The declarations a set of classes makes (one state, or the element itself).
        #[derive(Clone, Debug, PartialEq)]
        pub struct Decls {
            $( $(#[$m])* pub $name: $ty, )*
        }

        impl Decls {
            pub const NONE: Decls = Decls { $( $name: $none, )* };

            /// Every field `over` sets replaces this one's (a later rule in the cascade).
            pub fn overlay(&mut self, over: &Decls) {
                $( crate::model::Overlay::overlay(&mut self.$name, &over.$name); )*
            }

            pub fn is_empty(&self) -> bool {
                *self == Decls::NONE
            }

            /// Rust source for this value: every field spelled out (struct update from a const
            /// with `Cow` fields cannot be evaluated in a `static`).
            pub fn emit(&self, root: &str) -> String {
                let mut s = format!("{root}::Decls {{ ");
                $(
                    s.push_str(concat!(stringify!($name), ": "));
                    if self.$name != Decls::NONE.$name {
                        s.push_str(&crate::emit::Emit::emit(&self.$name, root));
                    } else {
                        s.push_str(stringify!($none));
                    }
                    s.push_str(", ");
                )*
                s.push('}');
                s
            }

            /// Names of the set fields, for reports.
            pub fn fields(&self) -> Vec<&'static str> {
                let mut v = Vec::new();
                $( if self.$name != Decls::NONE.$name { v.push(stringify!($name)); } )*
                v
            }
        }
    };
}

decls! {
    // Box and layout.
    display: Option<Display> = None,
    position: Option<Position> = None,
    /// top, right, bottom, left
    inset: [Option<Len>; 4] = [None; 4],
    z: Option<i32> = None,
    /// top, right, bottom, left
    margin: [Option<Len>; 4] = [None; 4],
    /// top, right, bottom, left
    padding: [Option<Len>; 4] = [None; 4],
    width: Option<Len> = None,
    height: Option<Len> = None,
    min_width: Option<Len> = None,
    min_height: Option<Len> = None,
    max_width: Option<Len> = None,
    max_height: Option<Len> = None,
    aspect: Option<f32> = None,
    flex_grow: Option<f32> = None,
    flex_shrink: Option<f32> = None,
    flex_basis: Option<Len> = None,
    flex_direction: Option<FlexDirection> = None,
    flex_wrap: Option<FlexWrap> = None,
    align_items: Option<Align> = None,
    align_self: Option<Align> = None,
    justify_content: Option<Justify> = None,
    /// row-gap, column-gap
    gap: [Option<Len>; 2] = [None; 2],
    grid_cols: Option<Cow<'static, [Track]>> = None,
    grid_rows: Option<Cow<'static, [Track]>> = None,
    grid_column: Option<Line> = None,
    grid_row: Option<Line> = None,
    /// x, y
    overflow: [Option<Overflow>; 2] = [None; 2],
    visible: Option<bool> = None,
    pointer_events: Option<bool> = None,
    user_select: Option<UserSelect> = None,

    // Paint.
    bg: Option<Rgba> = None,
    bg_image: Option<BgImage> = None,
    /// from, via, to (`bg-gradient-to-*` stops)
    grad: [Option<Rgba>; 3] = [None; 3],
    /// background-size: cover
    bg_cover: Option<bool> = None,
    /// background-position (x, y)
    bg_position: Option<[Len; 2]> = None,
    /// top-left, top-right, bottom-right, bottom-left
    radius: [Option<Len>; 4] = [None; 4],
    /// top, right, bottom, left
    border: [Option<Len>; 4] = [None; 4],
    border_color: Option<Rgba> = None,
    opacity: Option<f32> = None,
    shadow: Option<Cow<'static, [Shadow]>> = None,
    shadow_color: Option<Rgba> = None,
    ring: Option<Len> = None,
    /// None in a set ring = currentColor
    ring_color: Option<Rgba> = None,
    outline_style: Option<OutlineStyle> = None,
    outline_width: Option<Len> = None,
    outline_color: Option<Rgba> = None,
    outline_offset: Option<Len> = None,
    /// translate x, y (percent is of the element itself)
    translate: [Option<Len>; 2] = [None; 2],
    scale: Option<f32> = None,
    /// degrees
    rotate: Option<f32> = None,
    transform_origin: Option<[Len; 2]> = None,
    blend: Option<Blend> = None,
    /// filter: grayscale(amount)
    grayscale: Option<f32> = None,
    drop_shadow: Option<Shadow> = None,
    backdrop_blur: Option<Len> = None,
    mask_image: Option<Cow<'static, str>> = None,
    mask_position: Option<[Len; 2]> = None,
    mask_repeat: Option<bool> = None,
    mask_size: Option<[Len; 2]> = None,
    object_fit: Option<Fit> = None,
    object_position: Option<[Len; 2]> = None,

    // Text (inherited by children in CSS; the engine cascades them).
    color: Option<Rgba> = None,
    family: Option<Family> = None,
    font_size: Option<Len> = None,
    /// The line height a `text-*` size brings (loses to `leading-*` on the same element).
    size_line_height: Option<LineHeight> = None,
    /// The letter spacing a `text-*` size brings (loses to `tracking-*`).
    size_tracking: Option<Len> = None,
    /// `leading-*`
    line_height: Option<LineHeight> = None,
    /// `tracking-*`
    tracking: Option<Len> = None,
    weight: Option<u16> = None,
    text_align: Option<TextAlign> = None,
    uppercase: Option<bool> = None,
    decoration: Option<Decoration> = None,
    white_space: Option<WhiteSpace> = None,
    ellipsis: Option<bool> = None,
    line_clamp: Option<u16> = None,
    break_all: Option<bool> = None,
    break_words: Option<bool> = None,
    tabular: Option<bool> = None,
    ligatures: Option<bool> = None,
    text_shadow: Option<Shadow> = None,

    // Motion.
    /// transition-property
    transition: Option<Cow<'static, [TProp]>> = None,
    /// transition-duration, ms (`transition_ms()` applies Tailwind's 150 ms default)
    duration: Option<f32> = None,
    /// transition-timing-function (`transition_ease()` applies Tailwind's default curve)
    ease: Option<[f32; 4]> = None,
    anim: Option<Animation> = None,
}

impl Decls {
    /// The line height in effect: `leading-*` wins over the size's own (Tailwind's --tw-leading).
    pub fn effective_line_height(&self) -> Option<LineHeight> {
        self.line_height.or(self.size_line_height)
    }
    /// The letter spacing in effect: `tracking-*` wins over the size's own.
    pub fn effective_tracking(&self) -> Option<Len> {
        self.tracking.or(self.size_tracking)
    }
    /// The transition's duration: `duration-*` or Tailwind's default (150 ms) when a transition is set.
    pub fn transition_ms(&self) -> Option<f32> {
        self.transition.as_ref()?;
        Some(self.duration.unwrap_or(crate::tokens::DEFAULT_TRANSITION_MS))
    }
    pub fn transition_ease(&self) -> Option<[f32; 4]> {
        self.transition.as_ref()?;
        Some(self.ease.unwrap_or(crate::tokens::DEFAULT_TRANSITION_EASE))
    }
    /// The background gradient, stops resolved (Tailwind's from/via/to default to transparent).
    pub fn gradient(&self) -> Option<Gradient> {
        match self.bg_image.as_ref()? {
            BgImage::Tailwind { angle } => {
                let t = Rgba::TRANSPARENT;
                let [from, via, to] = self.grad;
                // No stops: the gradient is invalid and CSS draws no background image.
                if from.is_none() && via.is_none() && to.is_none() {
                    return None;
                }
                let mut stops = vec![Stop { color: from.unwrap_or(t), pos: Some(0.0) }];
                if let Some(v) = via {
                    stops.push(Stop { color: v, pos: Some(50.0) });
                }
                stops.push(Stop { color: to.unwrap_or(t), pos: Some(100.0) });
                Some(Gradient { angle: *angle, stops, oklab: true })
            }
            BgImage::Linear { angle, stops } => Some(Gradient { angle: *angle, stops: stops.to_vec(), oklab: false }),
        }
    }
}

/// One state's declarations.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub when: When,
    pub decls: Decls,
}

/// A parsed class string: what `tw!("...")` returns (as `&'static Tw`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tw {
    /// The class string as written (normalised whitespace).
    pub src: Cow<'static, str>,
    /// The element's own declarations, cascade already applied.
    pub base: Decls,
    /// Declarations that apply in a state or to children.
    pub states: Cow<'static, [State]>,
    pub marks: Marks,
}

impl Tw {
    pub const EMPTY: Tw = Tw { src: Cow::Borrowed(""), base: Decls::NONE, states: Cow::Borrowed(&[]), marks: Marks::NONE };

    pub fn state(&self, when: &When) -> Option<&Decls> {
        self.states.iter().find(|s| &s.when == when).map(|s| &s.decls)
    }
    pub fn has(&self, m: Marks) -> bool {
        self.marks.contains(m)
    }
    /// The base declarations with a state's on top (e.g. the focused look).
    pub fn with_state(&self, when: &When) -> Decls {
        let mut d = self.base.clone();
        if let Some(s) = self.state(when) {
            d.overlay(s);
        }
        d
    }
}

/// Field-wise "later wins".
pub trait Overlay {
    fn overlay(&mut self, over: &Self);
}

impl<T: Clone> Overlay for Option<T> {
    fn overlay(&mut self, over: &Self) {
        if over.is_some() {
            self.clone_from(over);
        }
    }
}

impl<T: Clone, const N: usize> Overlay for [Option<T>; N] {
    fn overlay(&mut self, over: &Self) {
        for (a, b) in self.iter_mut().zip(over.iter()) {
            a.overlay(b);
        }
    }
}
