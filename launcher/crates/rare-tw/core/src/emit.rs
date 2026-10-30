//! Rust source for parsed values, so the tw! macro can write a `static` (`root` is the path the
//! model types are reachable at in the calling crate, `::rare_tw`).

use crate::model::*;
use std::borrow::Cow;

pub trait Emit {
    fn emit(&self, root: &str) -> String;
}

fn f(v: f32) -> String {
    if v.is_infinite() {
        return if v > 0.0 { "f32::INFINITY".into() } else { "f32::NEG_INFINITY".into() };
    }
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') {
        format!("{s}_f32")
    } else {
        format!("{s}.0_f32")
    }
}

impl Emit for f32 {
    fn emit(&self, _: &str) -> String {
        f(*self)
    }
}
impl Emit for u16 {
    fn emit(&self, _: &str) -> String {
        format!("{self}_u16")
    }
}
impl Emit for i16 {
    fn emit(&self, _: &str) -> String {
        format!("{self}_i16")
    }
}
impl Emit for i32 {
    fn emit(&self, _: &str) -> String {
        format!("{self}_i32")
    }
}
impl Emit for bool {
    fn emit(&self, _: &str) -> String {
        format!("{self}")
    }
}

impl<T: Emit> Emit for Option<T> {
    fn emit(&self, root: &str) -> String {
        match self {
            None => "::core::option::Option::None".into(),
            Some(v) => format!("::core::option::Option::Some({})", v.emit(root)),
        }
    }
}

impl<T: Emit, const N: usize> Emit for [T; N] {
    fn emit(&self, root: &str) -> String {
        let parts: Vec<String> = self.iter().map(|v| v.emit(root)).collect();
        format!("[{}]", parts.join(", "))
    }
}

impl<T: Emit + Clone> Emit for Cow<'static, [T]> {
    fn emit(&self, root: &str) -> String {
        let parts: Vec<String> = self.iter().map(|v| v.emit(root)).collect();
        format!("::std::borrow::Cow::Borrowed(&[{}])", parts.join(", "))
    }
}

impl Emit for Cow<'static, str> {
    fn emit(&self, _: &str) -> String {
        format!("::std::borrow::Cow::Borrowed({:?})", self.as_ref())
    }
}

impl Emit for Len {
    fn emit(&self, root: &str) -> String {
        match self {
            Len::Auto => format!("{root}::Len::Auto"),
            Len::Px(v) => format!("{root}::Len::Px({})", f(*v)),
            Len::Rem(v) => format!("{root}::Len::Rem({})", f(*v)),
            Len::Em(v) => format!("{root}::Len::Em({})", f(*v)),
            Len::Pct(v) => format!("{root}::Len::Pct({})", f(*v)),
        }
    }
}

impl Emit for Rgba {
    fn emit(&self, root: &str) -> String {
        format!("{root}::Rgba {{ r: {}, g: {}, b: {}, a: {} }}", f(self.r), f(self.g), f(self.b), f(self.a))
    }
}

macro_rules! unit_enum {
    ($t:ident { $($v:ident),* }) => {
        impl Emit for $t {
            fn emit(&self, root: &str) -> String {
                match self { $( $t::$v => format!(concat!("{}::", stringify!($t), "::", stringify!($v)), root), )* }
            }
        }
    };
}

unit_enum!(Display { Block, Inline, InlineBlock, Flex, InlineFlex, Grid, Contents, None, LineClamp });
unit_enum!(Position { Static, Relative, Absolute, Fixed, Sticky });
unit_enum!(FlexDirection { Row, Column });
unit_enum!(FlexWrap { NoWrap, Wrap });
unit_enum!(Align { Start, End, Center, Stretch, Baseline });
unit_enum!(Justify { Start, End, Center, Between });
unit_enum!(Overflow { Visible, Hidden, Auto });
unit_enum!(Family { Sans, Mono });
unit_enum!(TextAlign { Left, Center, Right });
unit_enum!(WhiteSpace { Normal, NoWrap, PreWrap });
unit_enum!(Decoration { None, Underline, LineThrough });
unit_enum!(Fit { Cover, Contain });
unit_enum!(Blend { Lighten });
unit_enum!(UserSelect { None, Text, All });
unit_enum!(OutlineStyle { None, Solid });
unit_enum!(TProp { Color, BackgroundColor, BorderColor, OutlineColor, TextDecorationColor, Fill, Stroke, GradientFrom, GradientVia, GradientTo, Opacity, BoxShadow, Transform, Translate, Scale, Rotate, Left, Width });
unit_enum!(Anim { Pulse, FadeIn, SlideInRight, SlideUp, LoadBar, NumRollUp, NumRollDown, TintUp, TintDown, Breathe, Rise, Fade, Pop, BootSweep });

impl Emit for Animation {
    fn emit(&self, root: &str) -> String {
        format!(
            "{root}::Animation {{ kind: {}, ms: {}, ease: {}, delay_ms: {}, infinite: {} }}",
            self.kind.emit(root),
            f(self.ms),
            self.ease.emit(root),
            f(self.delay_ms),
            self.infinite
        )
    }
}

impl Emit for Sizing {
    fn emit(&self, root: &str) -> String {
        match self {
            Sizing::Len(l) => format!("{root}::Sizing::Len({})", l.emit(root)),
            Sizing::Fr(v) => format!("{root}::Sizing::Fr({})", f(*v)),
            Sizing::Auto => format!("{root}::Sizing::Auto"),
        }
    }
}

impl Emit for Track {
    fn emit(&self, root: &str) -> String {
        match self {
            Track::One(s) => format!("{root}::Track::One({})", s.emit(root)),
            Track::MinMax(a, b) => format!("{root}::Track::MinMax({}, {})", a.emit(root), b.emit(root)),
            Track::Repeat(n, t) => format!("{root}::Track::Repeat({}, {})", n.emit(root), t.emit(root)),
            Track::AutoFill(t) => format!("{root}::Track::AutoFill({})", t.emit(root)),
        }
    }
}

impl Emit for Line {
    fn emit(&self, root: &str) -> String {
        match self {
            Line::Span(n) => format!("{root}::Line::Span({})", n.emit(root)),
            Line::At(n) => format!("{root}::Line::At({})", n.emit(root)),
        }
    }
}

impl Emit for LineHeight {
    fn emit(&self, root: &str) -> String {
        match self {
            LineHeight::Num(v) => format!("{root}::LineHeight::Num({})", f(*v)),
            LineHeight::Len(l) => format!("{root}::LineHeight::Len({})", l.emit(root)),
        }
    }
}

impl Emit for Shadow {
    fn emit(&self, root: &str) -> String {
        format!(
            "{root}::Shadow {{ x: {}, y: {}, blur: {}, spread: {}, color: {} }}",
            self.x.emit(root),
            self.y.emit(root),
            self.blur.emit(root),
            self.spread.emit(root),
            self.color.emit(root)
        )
    }
}

impl Emit for Stop {
    fn emit(&self, root: &str) -> String {
        format!("{root}::Stop {{ color: {}, pos: {} }}", self.color.emit(root), self.pos.emit(root))
    }
}

impl Emit for BgImage {
    fn emit(&self, root: &str) -> String {
        match self {
            BgImage::Tailwind { angle } => format!("{root}::BgImage::Tailwind {{ angle: {} }}", f(*angle)),
            BgImage::Linear { angle, stops } => format!("{root}::BgImage::Linear {{ angle: {}, stops: {} }}", f(*angle), stops.emit(root)),
        }
    }
}

impl Emit for When {
    fn emit(&self, root: &str) -> String {
        match self {
            When::FocusVisible => format!("{root}::When::FocusVisible"),
            When::First => format!("{root}::When::First"),
            When::Last => format!("{root}::When::Last"),
            When::Disabled => format!("{root}::When::Disabled"),
            When::Placeholder => format!("{root}::When::Placeholder"),
            When::ChildrenNotLast => format!("{root}::When::ChildrenNotLast"),
            When::Children => format!("{root}::When::Children"),
            When::Child(t) => format!("{root}::When::Child({})", t.emit(root)),
            When::AncestorFocus { attr, value } => format!("{root}::When::AncestorFocus {{ attr: {}, value: {} }}", attr.emit(root), value.emit(root)),
        }
    }
}

impl Emit for Decls {
    fn emit(&self, root: &str) -> String {
        Decls::emit(self, root)
    }
}

impl Emit for State {
    fn emit(&self, root: &str) -> String {
        format!("{root}::State {{ when: {}, decls: {} }}", self.when.emit(root), self.decls.emit(root))
    }
}

impl Emit for Tw {
    fn emit(&self, root: &str) -> String {
        format!(
            "{root}::Tw {{ src: {}, base: {}, states: {}, marks: {root}::Marks({}_u32) }}",
            self.src.emit(root),
            self.base.emit(root),
            self.states.emit(root),
            self.marks.0
        )
    }
}
