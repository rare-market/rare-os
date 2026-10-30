//! `Tw` / `Decls` into a Taffy `Style` (feature `taffy`, on in the rare-tw crate).
//!
//! Lengths resolve at the live rem (the window's font size: `theme::rem_for(w, h)` times the UI
//! scale), so one static `Tw` serves every screen size. CSS details that matter for parity:
//! - an element without a display class is `display: block` (a `<div>`), not Taffy's flex default;
//! - Taffy has no inline formatting: `inline` / `inline-block` lay out as blocks and `inline-flex`
//!   as flex (in the launcher they are flex items or one-line labels, where the result is the same);
//! - `display: contents` is reported by `Decls::display`; the view layer splices the children;
//! - `overflow-*-auto` becomes Taffy's `Scroll` (the scroll offset is the engine's).

use crate::model::*;
use taffy::prelude::{auto, fr, length, line, minmax, percent, span, TaffyAuto};
use taffy::style::{
    AlignContent, AlignItems, Dimension, GridPlacement, GridTemplateComponent, LengthPercentage, LengthPercentageAuto, MaxTrackSizingFunction, MinTrackSizingFunction, RepetitionCount,
    Style, TrackSizingFunction,
};
use taffy::style_helpers::repeat;

/// Resolves lengths: `rem` px per rem, `em` px per em (the element's font size).
#[derive(Clone, Copy, Debug)]
pub struct Units {
    pub rem: f32,
    pub em: f32,
}

impl Units {
    pub fn new(rem: f32) -> Units {
        Units { rem, em: rem }
    }
    fn px(self, l: Len) -> Option<f32> {
        match l {
            Len::Pct(_) | Len::Auto => None,
            l => l.px(self.rem, self.em, 0.0),
        }
    }
    fn dim(self, l: Len) -> Dimension {
        match l {
            Len::Auto => Dimension::auto(),
            Len::Pct(p) => Dimension::percent(p / 100.0),
            l => Dimension::length(self.px(l).unwrap()),
        }
    }
    fn lpa(self, l: Len) -> LengthPercentageAuto {
        match l {
            Len::Auto => LengthPercentageAuto::auto(),
            Len::Pct(p) => LengthPercentageAuto::percent(p / 100.0),
            l => LengthPercentageAuto::length(self.px(l).unwrap()),
        }
    }
    fn lp(self, l: Len) -> LengthPercentage {
        match l {
            Len::Auto => LengthPercentage::length(0.0),
            Len::Pct(p) => LengthPercentage::percent(p / 100.0),
            l => LengthPercentage::length(self.px(l).unwrap()),
        }
    }
}

fn align(a: Align) -> AlignItems {
    match a {
        Align::Start => AlignItems::FLEX_START,
        Align::End => AlignItems::FLEX_END,
        Align::Center => AlignItems::CENTER,
        Align::Stretch => AlignItems::STRETCH,
        Align::Baseline => AlignItems::BASELINE,
    }
}

fn overflow(o: Overflow) -> taffy::style::Overflow {
    match o {
        Overflow::Visible => taffy::style::Overflow::Visible,
        Overflow::Hidden => taffy::style::Overflow::Hidden,
        Overflow::Auto => taffy::style::Overflow::Scroll,
    }
}

fn track_fn(s: &Sizing, u: Units) -> TrackSizingFunction {
    match s {
        Sizing::Len(l) => match l {
            Len::Pct(p) => percent(p / 100.0),
            Len::Auto => auto(),
            l => length(u.px(*l).unwrap()),
        },
        Sizing::Fr(f) => fr(*f),
        Sizing::Auto => auto(),
    }
}

fn min_fn(s: &Sizing, u: Units) -> MinTrackSizingFunction {
    match s {
        Sizing::Len(Len::Pct(p)) => percent(p / 100.0),
        Sizing::Len(Len::Auto) | Sizing::Auto | Sizing::Fr(_) => MinTrackSizingFunction::AUTO,
        Sizing::Len(l) => length(u.px(*l).unwrap()),
    }
}

fn max_fn(s: &Sizing, u: Units) -> MaxTrackSizingFunction {
    match s {
        Sizing::Len(Len::Pct(p)) => percent(p / 100.0),
        Sizing::Len(Len::Auto) | Sizing::Auto => MaxTrackSizingFunction::AUTO,
        Sizing::Fr(f) => fr(*f),
        Sizing::Len(l) => length(u.px(*l).unwrap()),
    }
}

fn simple(t: &Track, u: Units) -> TrackSizingFunction {
    match t {
        Track::One(s) => track_fn(s, u),
        Track::MinMax(a, b) => minmax(min_fn(a, u), max_fn(b, u)),
        // Nested repeats are not valid CSS; the parser never makes them.
        Track::Repeat(..) | Track::AutoFill(..) => auto(),
    }
}

fn template(ts: &[Track], u: Units) -> Vec<GridTemplateComponent<String>> {
    ts.iter()
        .map(|t| match t {
            Track::Repeat(n, inner) => repeat(RepetitionCount::Count(*n), inner.iter().map(|t| simple(t, u)).collect()),
            Track::AutoFill(inner) => repeat(RepetitionCount::AutoFill, inner.iter().map(|t| simple(t, u)).collect()),
            t => GridTemplateComponent::Single(simple(t, u)),
        })
        .collect()
}

impl Decls {
    /// Writes every layout property these declarations set onto `s` (others stay as they are).
    pub fn apply_layout(&self, s: &mut Style, u: Units) {
        if let Some(d) = self.display {
            s.display = match d {
                Display::Flex | Display::InlineFlex => taffy::style::Display::Flex,
                Display::Grid => taffy::style::Display::Grid,
                Display::None => taffy::style::Display::None,
                Display::Block | Display::Inline | Display::InlineBlock | Display::Contents | Display::LineClamp => taffy::style::Display::Block,
            };
        }
        if let Some(p) = self.position {
            s.position = match p {
                Position::Static => taffy::style::Position::Static,
                Position::Relative | Position::Sticky => taffy::style::Position::Relative,
                // Fixed is absolute to the screen root; the engine puts fixed nodes on its top layer.
                Position::Absolute | Position::Fixed => taffy::style::Position::Absolute,
            };
        }
        let [t, r, b, l] = self.inset;
        if let Some(v) = t {
            s.inset.top = u.lpa(v);
        }
        if let Some(v) = r {
            s.inset.right = u.lpa(v);
        }
        if let Some(v) = b {
            s.inset.bottom = u.lpa(v);
        }
        if let Some(v) = l {
            s.inset.left = u.lpa(v);
        }
        let [t, r, b, l] = self.margin;
        if let Some(v) = t {
            s.margin.top = u.lpa(v);
        }
        if let Some(v) = r {
            s.margin.right = u.lpa(v);
        }
        if let Some(v) = b {
            s.margin.bottom = u.lpa(v);
        }
        if let Some(v) = l {
            s.margin.left = u.lpa(v);
        }
        let [t, r, b, l] = self.padding;
        if let Some(v) = t {
            s.padding.top = u.lp(v);
        }
        if let Some(v) = r {
            s.padding.right = u.lp(v);
        }
        if let Some(v) = b {
            s.padding.bottom = u.lp(v);
        }
        if let Some(v) = l {
            s.padding.left = u.lp(v);
        }
        let [t, r, b, l] = self.border;
        if let Some(v) = t {
            s.border.top = u.lp(v);
        }
        if let Some(v) = r {
            s.border.right = u.lp(v);
        }
        if let Some(v) = b {
            s.border.bottom = u.lp(v);
        }
        if let Some(v) = l {
            s.border.left = u.lp(v);
        }
        if let Some(v) = self.width {
            s.size.width = u.dim(v);
        }
        if let Some(v) = self.height {
            s.size.height = u.dim(v);
        }
        if let Some(v) = self.min_width {
            s.min_size.width = u.lpa(v);
        }
        if let Some(v) = self.min_height {
            s.min_size.height = u.lpa(v);
        }
        if let Some(v) = self.max_width {
            s.max_size.width = u.lpa(v);
        }
        if let Some(v) = self.max_height {
            s.max_size.height = u.lpa(v);
        }
        if let Some(a) = self.aspect {
            s.aspect_ratio = Some(a);
        }
        if let Some(g) = self.flex_grow {
            s.flex_grow = g;
        }
        if let Some(v) = self.flex_shrink {
            s.flex_shrink = v;
        }
        if let Some(v) = self.flex_basis {
            s.flex_basis = u.dim(v);
        }
        if let Some(d) = self.flex_direction {
            s.flex_direction = match d {
                FlexDirection::Row => taffy::style::FlexDirection::Row,
                FlexDirection::Column => taffy::style::FlexDirection::Column,
            };
        }
        if let Some(w) = self.flex_wrap {
            s.flex_wrap = match w {
                FlexWrap::Wrap => taffy::style::FlexWrap::Wrap,
                FlexWrap::NoWrap => taffy::style::FlexWrap::NoWrap,
            };
        }
        if let Some(a) = self.align_items {
            s.align_items = align(a);
        }
        if let Some(a) = self.align_self {
            s.align_self = Some(align(a));
        }
        if let Some(j) = self.justify_content {
            s.justify_content = match j {
                Justify::Start => AlignContent::FLEX_START,
                Justify::End => AlignContent::FLEX_END,
                Justify::Center => AlignContent::CENTER,
                Justify::Between => AlignContent::SPACE_BETWEEN,
            };
        }
        // CSS gap is (row, column); Taffy's Size is (width = column gap, height = row gap).
        if let Some(v) = self.gap[0] {
            s.gap.height = u.lp(v);
        }
        if let Some(v) = self.gap[1] {
            s.gap.width = u.lp(v);
        }
        if let Some(t) = &self.grid_cols {
            s.grid_template_columns = template(t, u);
        }
        if let Some(t) = &self.grid_rows {
            s.grid_template_rows = template(t, u);
        }
        match self.grid_column {
            Some(Line::Span(n)) => s.grid_column = taffy::geometry::Line { start: span(n), end: span(n) },
            Some(Line::At(i)) => s.grid_column = taffy::geometry::Line { start: line(i), end: GridPlacement::AUTO },
            None => {}
        }
        match self.grid_row {
            Some(Line::Span(n)) => s.grid_row = taffy::geometry::Line { start: span(n), end: span(n) },
            Some(Line::At(i)) => s.grid_row = taffy::geometry::Line { start: line(i), end: GridPlacement::AUTO },
            None => {}
        }
        // CSS: a visible axis next to a clipped one computes to auto.
        let [ox, oy] = self.overflow;
        let fix = |a: Option<Overflow>, b: Option<Overflow>| match (a, b) {
            (None | Some(Overflow::Visible), Some(Overflow::Hidden | Overflow::Auto)) => Some(Overflow::Auto),
            (a, _) => a,
        };
        if let Some(o) = fix(ox, oy) {
            s.overflow.x = overflow(o);
        }
        if let Some(o) = fix(oy, ox) {
            s.overflow.y = overflow(o);
        }
        if self.overflow != [None, None] {
            s.scrollbar_width = 0.0;
        }
    }
}

/// CSS's initial style for a `<div>`: display block, everything else Taffy's (static position,
/// shrink 1, border-box, which Tailwind's preflight sets on every element).
pub fn block() -> Style {
    Style { display: taffy::style::Display::Block, ..Style::default() }
}

impl Tw {
    /// The element's Taffy style at this rem (a `<div>` with these classes).
    pub fn style(&self, rem: f32) -> Style {
        let mut s = block();
        let em = self.base.font_size.and_then(|l| l.px(rem, rem, 0.0)).unwrap_or(rem);
        self.base.apply_layout(&mut s, Units { rem, em });
        s
    }

    /// The style with a state's declarations on top (`focus-visible:` and friends).
    pub fn style_in(&self, when: &When, rem: f32) -> Style {
        let mut s = self.style(rem);
        if let Some(d) = self.state(when) {
            d.apply_layout(&mut s, Units::new(rem));
        }
        s
    }
}
