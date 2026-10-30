//! The browser's view of a parse: the computed CSS values an element with these classes gets,
//! formatted like Chrome's getComputedStyle (colours as `rgba(r,g,b,a)` 0-255). The parity tests
//! compare this with what headless Chromium computes for the same classes (corpus/computed.tsv),
//! so every class the web launcher uses is checked against the real stylesheet.
//!
//! The element sits in a plain block parent under the launcher's root: font-size 16px (1rem at
//! 1280x800), line-height 1.5, colour fg, Inter.

use crate::model::*;
use crate::tokens;

/// Inherited text context of the parent.
#[derive(Clone, Copy, Debug)]
pub struct Parent {
    pub font_px: f32,
    pub line: LineHeight,
    pub color: Rgba,
}

impl Parent {
    /// The launcher's root: html { line-height: 1.5 }, body colour fg, 1rem.
    pub fn root(rem: f32) -> Parent {
        Parent { font_px: rem, line: LineHeight::Num(1.5), color: tokens::color("fg").unwrap_or(Rgba::rgb(242, 242, 243)) }
    }
}

pub fn rgba(c: Rgba) -> String {
    format!("rgba({:.2},{:.2},{:.2},{:.4})", c.r * 255.0, c.g * 255.0, c.b * 255.0, c.a)
}

fn n(v: f32) -> String {
    let r = (v * 10000.0).round() / 10000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

fn px(v: f32) -> String {
    format!("{}px", n(v))
}

/// A length as Chrome reports a computed (not used) value: px, or % kept.
fn len(l: Len, rem: f32, em: f32) -> String {
    match l {
        Len::Auto => "auto".into(),
        Len::Pct(p) => format!("{}%", n(p)),
        Len::Px(v) if v.is_infinite() => "3.35544e+07px".into(),
        l => px(l.px(rem, em, 0.0).unwrap()),
    }
}

fn shadow(s: &Shadow, color: Rgba, rem: f32, em: f32) -> String {
    format!("{} {} {} {} {}", rgba(color), len(s.x, rem, em), len(s.y, rem, em), len(s.blur, rem, em), len(s.spread, rem, em))
}

fn bezier(e: [f32; 4]) -> String {
    match e {
        [0.25, 0.1, 0.25, 1.0] => "ease".into(),
        [0.42, 0.0, 0.58, 1.0] => "ease-in-out".into(),
        [0.0, 0.0, 1.0, 1.0] => "linear".into(),
        _ => format!("cubic-bezier({}, {}, {}, {})", n(e[0]), n(e[1]), n(e[2]), n(e[3])),
    }
}

fn secs(ms: f32) -> String {
    format!("{}s", n(ms / 1000.0))
}

fn track(t: &Track, rem: f32, em: f32) -> String {
    let sz = |s: &Sizing| match s {
        Sizing::Len(l) => len(*l, rem, em),
        Sizing::Fr(f) => format!("{}fr", n(*f)),
        Sizing::Auto => "auto".into(),
    };
    match t {
        Track::One(s) => sz(s),
        Track::MinMax(a, b) => format!("minmax({}, {})", sz(a), sz(b)),
        Track::Repeat(c, ts) => format!("repeat({c}, {})", ts.iter().map(|t| track(t, rem, em)).collect::<Vec<_>>().join(" ")),
        Track::AutoFill(ts) => format!("repeat(auto-fill, {})", ts.iter().map(|t| track(t, rem, em)).collect::<Vec<_>>().join(" ")),
    }
}

fn anim_name(a: Anim) -> &'static str {
    match a {
        Anim::Pulse => "pulse",
        Anim::FadeIn => "fade-in",
        Anim::SlideInRight => "slide-in-right",
        Anim::SlideUp => "slide-up",
        Anim::LoadBar => "load-bar",
        Anim::NumRollUp => "num-roll-up",
        Anim::NumRollDown => "num-roll-down",
        Anim::TintUp => "tint-up",
        Anim::TintDown => "tint-down",
        Anim::Breathe => "breathe",
        Anim::Rise => "rise",
        Anim::Fade => "fade",
        Anim::Pop => "pop",
        Anim::BootSweep => "boot-sweep",
    }
}

/// The element's computed font size in px.
pub fn font_px(d: &Decls, parent: &Parent, rem: f32) -> f32 {
    d.font_size.map(|l| l.px(rem, parent.font_px, 0.0).unwrap_or(parent.font_px)).unwrap_or(parent.font_px)
}

/// The computed properties the declarations set, in Chrome's formats. Only properties that
/// differ from the CSS initial/inherited value are meaningful to compare; this lists every
/// property the declarations touch (plus the ones that follow `color`).
pub fn computed(d: &Decls, rem: f32) -> Vec<(&'static str, String)> {
    let parent = Parent::root(rem);
    let mut o: Vec<(&'static str, String)> = Vec::new();
    let fs = font_px(d, &parent, rem);
    let l = |x: Len| len(x, rem, fs);

    if let Some(v) = d.display {
        o.push((
            "display",
            match v {
                Display::Block => "block",
                Display::Inline => "inline",
                Display::InlineBlock => "inline-block",
                Display::Flex => "flex",
                Display::InlineFlex => "inline-flex",
                Display::Grid => "grid",
                Display::Contents => "contents",
                Display::None => "none",
                // -webkit-box inside a display:none subtree reports as flow-root in Chrome.
                Display::LineClamp => "flow-root",
            }
            .into(),
        ));
    }
    if let Some(v) = d.position {
        o.push(("position", format!("{v:?}").to_lowercase()));
    }
    for (i, k) in ["top", "right", "bottom", "left"].into_iter().enumerate() {
        if let Some(v) = d.inset[i] {
            o.push((k, l(v)));
        }
    }
    if let Some(z) = d.z {
        o.push(("z-index", z.to_string()));
    }
    for (i, k) in ["margin-top", "margin-right", "margin-bottom", "margin-left"].into_iter().enumerate() {
        if let Some(v) = d.margin[i] {
            o.push((k, l(v)));
        }
    }
    for (i, k) in ["padding-top", "padding-right", "padding-bottom", "padding-left"].into_iter().enumerate() {
        if let Some(v) = d.padding[i] {
            o.push((k, l(v)));
        }
    }
    let sizes = [("width", d.width, "auto"), ("height", d.height, "auto"), ("min-width", d.min_width, "auto"), ("min-height", d.min_height, "auto"), ("max-width", d.max_width, "none"), ("max-height", d.max_height, "none")];
    for (k, v, auto) in sizes {
        if let Some(v) = v {
            o.push((k, if v == Len::Auto { auto.into() } else { l(v) }));
        }
    }
    if let Some(a) = d.aspect {
        o.push(("aspect-ratio", n(a)));
    }
    if let Some(v) = d.flex_grow {
        o.push(("flex-grow", n(v)));
    }
    if let Some(v) = d.flex_shrink {
        o.push(("flex-shrink", n(v)));
    }
    if let Some(v) = d.flex_basis {
        o.push(("flex-basis", l(v)));
    }
    if let Some(v) = d.flex_direction {
        o.push(("flex-direction", if v == FlexDirection::Row { "row" } else { "column" }.into()));
    }
    if let Some(v) = d.flex_wrap {
        o.push(("flex-wrap", if v == FlexWrap::Wrap { "wrap" } else { "nowrap" }.into()));
    }
    let align = |a: Align| match a {
        Align::Start => "flex-start",
        Align::End => "flex-end",
        Align::Center => "center",
        Align::Stretch => "stretch",
        Align::Baseline => "baseline",
    };
    if let Some(v) = d.align_items {
        o.push(("align-items", align(v).into()));
    }
    if let Some(v) = d.align_self {
        o.push(("align-self", align(v).into()));
    }
    if let Some(v) = d.justify_content {
        o.push((
            "justify-content",
            match v {
                Justify::Start => "flex-start",
                Justify::End => "flex-end",
                Justify::Center => "center",
                Justify::Between => "space-between",
            }
            .into(),
        ));
    }
    if let Some(v) = d.gap[0] {
        o.push(("row-gap", l(v)));
    }
    if let Some(v) = d.gap[1] {
        o.push(("column-gap", l(v)));
    }
    if let Some(t) = &d.grid_cols {
        o.push(("grid-template-columns", t.iter().map(|t| track(t, rem, fs)).collect::<Vec<_>>().join(" ")));
    }
    if let Some(t) = &d.grid_rows {
        o.push(("grid-template-rows", t.iter().map(|t| track(t, rem, fs)).collect::<Vec<_>>().join(" ")));
    }
    match d.grid_column {
        Some(Line::Span(s)) => {
            o.push(("grid-column-start", format!("span {s}")));
            o.push(("grid-column-end", format!("span {s}")));
        }
        Some(Line::At(a)) => o.push(("grid-column-start", a.to_string())),
        None => {}
    }
    if let Some(Line::At(a)) = d.grid_row {
        o.push(("grid-row-start", a.to_string()));
    }
    let ov = |v: Overflow| format!("{v:?}").to_lowercase();
    // CSS: when one axis is not visible, a visible other axis computes to auto.
    let [ox, oy] = d.overflow;
    let fix = |a: Option<Overflow>, b: Option<Overflow>| match (a, b) {
        (None | Some(Overflow::Visible), Some(Overflow::Hidden | Overflow::Auto)) => Some(Overflow::Auto),
        (a, _) => a,
    };
    if let Some(v) = fix(ox, oy) {
        o.push(("overflow-x", ov(v)));
    }
    if let Some(v) = fix(oy, ox) {
        o.push(("overflow-y", ov(v)));
    }
    if let Some(v) = d.visible {
        o.push(("visibility", if v { "visible" } else { "hidden" }.into()));
    }
    if let Some(v) = d.pointer_events {
        o.push(("pointer-events", if v { "auto" } else { "none" }.into()));
    }
    if let Some(v) = d.user_select {
        o.push(("user-select", format!("{v:?}").to_lowercase()));
    }
    if let Some(c) = d.bg {
        o.push(("background-color", rgba(c)));
    }
    if let Some(g) = d.gradient() {
        let dir = match g.angle {
            180.0 => String::new(),
            0.0 => "to top".into(),
            90.0 => "to right".into(),
            270.0 => "to left".into(),
            a => format!("{}deg", n(a)),
        };
        let mut head = dir;
        // Chrome leaves out `in oklab` where it is already the default: when a stop is a
        // non-legacy colour (Tailwind's `/alpha` colours are color-mix()es).
        let legacy = g.stops.iter().all(|s| s.color.a == 0.0 || s.color.a == 1.0);
        if g.oklab && legacy {
            head = if head.is_empty() { "in oklab".into() } else { format!("{head} in oklab") };
        }
        let stops: Vec<String> = g.stops.iter().map(|s| match s.pos {
            Some(p) => format!("{} {}%", rgba(s.color), n(p)),
            None => rgba(s.color),
        }).collect();
        let body = if head.is_empty() { stops.join(", ") } else { format!("{head}, {}", stops.join(", ")) };
        o.push(("background-image", format!("linear-gradient({body})")));
    }
    if d.bg_cover == Some(true) {
        o.push(("background-size", "cover".into()));
    }
    if let Some([x, y]) = d.bg_position {
        o.push(("background-position", format!("{} {}", l(x), l(y))));
    }
    for (i, k) in ["border-top-width", "border-right-width", "border-bottom-width", "border-left-width"].into_iter().enumerate() {
        if let Some(v) = d.border[i] {
            o.push((k, l(v)));
        }
    }
    let color = d.color.unwrap_or(parent.color);
    if d.border_color.is_some() || d.color.is_some() {
        let bc = rgba(d.border_color.unwrap_or(color));
        for k in ["border-top-color", "border-right-color", "border-bottom-color", "border-left-color"] {
            o.push((k, bc.clone()));
        }
    }
    for (i, k) in ["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"].into_iter().enumerate() {
        if let Some(v) = d.radius[i] {
            o.push((k, l(v)));
        }
    }
    if let Some(v) = d.opacity {
        o.push(("opacity", n(v)));
    }
    if d.ring.is_some() || d.shadow.is_some() {
        let mut layers = Vec::new();
        if let Some(w) = d.ring {
            layers.push(format!("{} 0px 0px 0px {}", rgba(d.ring_color.unwrap_or(color)), l(w)));
        }
        if let Some(sh) = &d.shadow {
            for s in sh.iter() {
                layers.push(shadow(s, d.shadow_color.unwrap_or(s.color), rem, fs));
            }
        }
        o.push(("box-shadow", layers.join(", ")));
    }
    if let Some(s) = d.outline_style {
        o.push(("outline-style", if s == OutlineStyle::Solid { "solid" } else { "none" }.into()));
    }
    if let Some(w) = d.outline_width {
        o.push(("outline-width", l(w)));
    }
    if d.outline_color.is_some() || d.color.is_some() {
        o.push(("outline-color", rgba(d.outline_color.unwrap_or(color))));
    }
    if let Some(v) = d.outline_offset {
        o.push(("outline-offset", l(v)));
    }
    if d.translate != [None, None] {
        let x = l(d.translate[0].unwrap_or(Len::ZERO));
        let y = d.translate[1].map(l);
        o.push(("translate", match y {
            Some(y) if y != "0px" => format!("{x} {y}"),
            _ => x,
        }));
    }
    if let Some(s) = d.scale {
        o.push(("scale", n(s)));
    }
    if let Some(r) = d.rotate {
        o.push(("rotate", format!("{}deg", n(r))));
    }
    if let Some([x, y]) = d.transform_origin {
        o.push(("transform-origin", format!("{} {}", l(x), l(y))));
    }
    if let Some(Blend::Lighten) = d.blend {
        o.push(("mix-blend-mode", "lighten".into()));
    }
    let mut filters = Vec::new();
    if let Some(g) = d.grayscale {
        filters.push(format!("grayscale({})", n(g)));
    }
    if let Some(s) = &d.drop_shadow {
        let t = shadow(s, s.color, rem, fs);
        // drop-shadow has no spread
        filters.push(format!("drop-shadow({})", t.rsplit_once(' ').map(|(a, _)| a.to_string()).unwrap_or(t)));
    }
    if !filters.is_empty() {
        o.push(("filter", filters.join(" ")));
    }
    if let Some(b) = d.backdrop_blur {
        o.push(("backdrop-filter", format!("blur({})", l(b))));
    }
    if let Some(u) = &d.mask_image {
        o.push(("mask-image", format!("url(\"{u}\")")));
    }
    if let Some([x, y]) = d.mask_position {
        o.push(("mask-position", format!("{} {}", l(x), l(y))));
    }
    if let Some(r) = d.mask_repeat {
        o.push(("mask-repeat", if r { "repeat" } else { "no-repeat" }.into()));
    }
    if let Some([x, y]) = d.mask_size {
        o.push(("mask-size", if y == Len::Auto { l(x) } else { format!("{} {}", l(x), l(y)) }));
    }
    if let Some(f) = d.object_fit {
        o.push(("object-fit", format!("{f:?}").to_lowercase()));
    }
    if let Some([x, y]) = d.object_position {
        o.push(("object-position", format!("{} {}", l(x), l(y))));
    }
    if let Some(c) = d.color {
        o.push(("color", rgba(c)));
    }
    if let Some(f) = d.family {
        o.push(("font-family", tokens::family_stack(f)));
    }
    if d.font_size.is_some() {
        o.push(("font-size", px(fs)));
    }
    let lh = d.effective_line_height();
    if lh.is_some() || d.font_size.is_some() {
        let v = match lh.unwrap_or(parent.line) {
            LineHeight::Num(k) => k * fs,
            LineHeight::Len(x) => x.px(rem, fs, 0.0).unwrap_or(fs),
        };
        o.push(("line-height", px(v)));
    }
    if let Some(t) = d.effective_tracking() {
        o.push(("letter-spacing", l(t)));
    }
    if let Some(w) = d.weight {
        o.push(("font-weight", w.to_string()));
    }
    if let Some(a) = d.text_align {
        o.push(("text-align", format!("{a:?}").to_lowercase()));
    }
    if let Some(u) = d.uppercase {
        o.push(("text-transform", if u { "uppercase" } else { "none" }.into()));
    }
    if let Some(dec) = d.decoration {
        o.push((
            "text-decoration-line",
            match dec {
                Decoration::None => "none",
                Decoration::Underline => "underline",
                Decoration::LineThrough => "line-through",
            }
            .into(),
        ));
    }
    match d.white_space {
        Some(WhiteSpace::NoWrap) => o.push(("text-wrap-mode", "nowrap".into())),
        Some(WhiteSpace::PreWrap) => o.push(("white-space-collapse", "preserve".into())),
        _ => {}
    }
    if d.ellipsis == Some(true) {
        o.push(("text-overflow", "ellipsis".into()));
    }
    if let Some(c) = d.line_clamp {
        o.push(("-webkit-line-clamp", c.to_string()));
    }
    if d.break_all == Some(true) {
        o.push(("word-break", "break-all".into()));
    }
    if d.break_words == Some(true) {
        o.push(("overflow-wrap", "break-word".into()));
    }
    if d.tabular == Some(true) {
        o.push(("font-variant-numeric", "tabular-nums".into()));
    }
    if d.ligatures == Some(false) {
        o.push(("font-variant-ligatures", "none".into()));
    }
    if let Some(s) = &d.text_shadow {
        let t = shadow(s, s.color, rem, fs);
        o.push(("text-shadow", t.rsplit_once(' ').map(|(a, _)| a.to_string()).unwrap_or(t)));
    }
    if let Some(t) = &d.transition {
        o.push(("transition-property", t.iter().map(|p| p.css()).collect::<Vec<_>>().join(", ")));
    }
    if let Some(ms) = d.transition_ms().or(d.duration) {
        o.push(("transition-duration", secs(ms)));
    }
    if let Some(e) = d.transition_ease().or(d.ease) {
        o.push(("transition-timing-function", bezier(e)));
    }
    if let Some(a) = d.anim {
        o.push(("animation-name", anim_name(a.kind).into()));
        o.push(("animation-duration", secs(a.ms)));
        o.push(("animation-timing-function", bezier(a.ease)));
        o.push(("animation-iteration-count", if a.infinite { "infinite".into() } else { "1".into() }));
        o.push(("animation-delay", secs(a.delay_ms)));
    }
    o
}
