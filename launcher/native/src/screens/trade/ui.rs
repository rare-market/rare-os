//! The Trade tab's pieces (trade/parts.tsx) as view elements: the order button, preset chips, the
//! Buy/Sell switch, timeframe chips, the stepper, label-value lines and the coin logo. Every
//! interactive piece is a focusable; green only for money up and Buy, red only for money down and
//! Sell.

use crate::kit::format::{num_parts, NumKind, NumOptions};
use crate::kit::{group, num, Btn};
use crate::screens::market::host::*;
use rare_render::view::{El, Look};
use rare_render::Color;

/// The order button's tone (parts.tsx TONES).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Buy,
    Sell,
    Primary,
    Secondary,
}

impl Tone {
    pub fn colors(self) -> (Color, Color, u16) {
        match self {
            Tone::Buy => (tok::BUY, tok::ON_BUY, 600),
            Tone::Sell => (tok::NEG, tok::FG, 600),
            Tone::Primary => (tok::BRAND, tok::FG, 600),
            Tone::Secondary => (tok::BTN2, tok::FG, 500),
        }
    }
}

/// `Action`: ui Button's look at size lg (or md), `.btn` focus (white fill, dark text, 1.04). A
/// button that cannot act yet stays focusable at 40 % so the hint bar can say why.
pub struct Action {
    pub label: String,
    pub tone: Tone,
    pub lg: bool,
    pub disabled: bool,
    pub def: bool,
    pub full: bool,
    pub hint: Option<String>,
    pub tag: u32,
}

impl Action {
    pub fn new(label: impl Into<String>, tone: Tone, tag: u32) -> Action {
        Action { label: label.into(), tone, lg: true, disabled: false, def: false, full: false, hint: None, tag }
    }
}

pub fn action(ui: &mut Ui, st: S, a: Action) -> El {
    let (bg, fg, w) = a.tone.colors();
    let (h, px, ty) = if a.lg { (3.25, 1.75, Ty::Row) } else { (2.85, 1.35, Ty::Body) };
    let mut st = st.row().items_center().justify_center().gap(0.55).h(h).px(px);
    if a.full {
        st = st.wp(1.0);
    }
    let mut look = Look::default().bg(bg).radius(ui.r(0.25));
    if a.disabled {
        look = look.opacity(0.4);
    }
    let focus = look.clone().bg(tok::FG).scale(1.04);
    let el = ui.bx(st).look(look).focus_look(focus).child(ui.text(a.label.clone(), T::new(ty).w(w).c(fg)).focus_color(tok::BG));
    let (tag, def, hint) = (a.tag, a.def, a.hint.clone());
    el.nav(move |f| {
        let f = f.tag(tag).hints(hint.as_deref(), None, None);
        if def {
            f.default()
        } else {
            f
        }
    })
}

/// `Chip`: a preset in a row of amounts; the chosen one is a lighter fill, not a colour.
pub fn chip(ui: &mut Ui, label: &str, on: bool, tag: u32) -> El {
    let (bg, fg) = if on { (tok::BUTTON_SECONDARY_ACTIVE, tok::FG) } else { (tok::BUTTON_SECONDARY, tok::MUTED) };
    let el = ui.bx(s().row().items_center().justify_center().h(2.5).px(0.5).flex(1.0).min0()).bg(bg).radius(ui.r(0.25)).child(ui.text(label, T::new(Ty::Meta).w(500).tab().c(fg)).focus_color(tok::FG));
    fill_nav(el, tag, None, None, false)
}

/// A chip whose chosen one carries a check, so it still reads as chosen under the focus ring
/// (Mirror size).
pub fn chip_check(ui: &mut Ui, label: &str, on: bool, tag: u32) -> El {
    let (bg, fg) = if on { (tok::BUTTON_SECONDARY_ACTIVE, tok::FG) } else { (tok::BUTTON_SECONDARY, tok::MUTED) };
    let mut el = ui.bx(s().row().items_center().justify_center().gap(0.3).h(2.5).px(0.5).flex(1.0).min0()).bg(bg).radius(ui.r(0.25));
    if on {
        el = el.child(ui.bx(s().wh(0.85, 0.85)).child(crate::kit::focus_icon(ui, "check", 0.85, false, tok::FG, tok::FG)));
    }
    let el = el.child(ui.text(label, T::new(Ty::Meta).w(500).tab().c(fg)).focus_color(tok::FG));
    fill_nav(el, tag, None, None, false)
}

/// `SideSwitch`: Buy/Sell or Long/Short, two buttons the d-pad reaches (A picks, X flips).
pub fn side_switch(ui: &mut Ui, items: [(&str, Tone); 2], on: usize, group_name: &str, tag0: u32) -> El {
    let mut g = ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(0.25).p(0.25)).bg(tok::L3).radius(ui.r(0.5)).group(group(group_name, true));
    for (i, (label, tone)) in items.iter().enumerate() {
        let (bg, fg) = match (i == on, tone) {
            (true, Tone::Buy) => (tok::BUY, tok::ON_BUY),
            (true, _) => (tok::NEG, tok::FG),
            _ => (Color::TRANSPARENT, tok::MUTED),
        };
        let other = items[1 - i].0;
        let b = ui.bx(s().row().items_center().justify_center().h(2.5)).bg(bg).radius(ui.r(0.25)).child(ui.text(*label, T::new(Ty::Body).w(600).c(fg)).focus_color(tok::FG));
        g = g.child(fill_nav(b, tag0 + i as u32, Some(&format!("Switch to {label}")), Some(other), false));
    }
    g
}

/// `TfChips`: timeframe buttons, 1 px apart, rounded at the two ends.
pub fn tf_chips(ui: &mut Ui, items: &[&str], on: usize, group_name: &str, tag0: u32) -> El {
    let mut g = ui.bx(s().row()).gap(1.0).group(group(group_name, true));
    let n = items.len();
    for (i, label) in items.iter().enumerate() {
        let (bg, fg) = if i == on { (tok::BUTTON_SECONDARY_ACTIVE, tok::FG) } else { (tok::BUTTON_SECONDARY, tok::MUTED) };
        // Per-corner radii are an engine request: the end chips round all four corners for now.
        let rad = if i == 0 || i + 1 == n { ui.r(0.25) } else { 0.0 };
        let b = ui.bx(s().row().items_center().justify_center().h(2.25).min_w(2.6).px(0.7)).bg(bg).radius(rad).child(ui.text(*label, T::new(Ty::Meta).w(500).c(fg)).focus_color(tok::FG));
        g = g.child(fill_nav(b, tag0 + i as u32, Some(&format!("Show {label}")), None, false));
    }
    g
}

/// `Stepper`: a minus and a plus around `middle` (amount, leverage).
pub fn stepper(ui: &mut Ui, label: &str, group_name: &str, tag_minus: u32, tag_plus: u32, middle: El) -> El {
    let btn = |ui: &mut Ui, t: &str, tag: u32, hint: String| {
        let b = ui.bx(s().row().items_center().justify_center().h(2.5).w(3.0)).bg(tok::BUTTON_SECONDARY).radius(ui.r(0.25)).child(ui.text(t, T::new(Ty::Row).w(600).c(tok::MUTED)).focus_color(tok::FG));
        fill_nav(b, tag, Some(&hint), None, false)
    };
    let minus = btn(ui, "\u{2212}", tag_minus, format!("Less {label}"));
    let plus = btn(ui, "+", tag_plus, format!("More {label}"));
    ui.bx(s().row().items_center().gap(0.75)).group(group(group_name, true)).child(minus).child(ui.bx(s().flex(1.0).min0()).child(middle)).child(plus)
}

/// A value in a `Line`: text, or a Num.
#[derive(Clone, Debug)]
pub enum Val {
    None,
    Text(String),
    Num(NumKind, Option<f64>, NumOptions),
}

impl Val {
    pub fn t(s: impl Into<String>) -> Val {
        let s = s.into();
        if s.is_empty() {
            Val::None
        } else {
            Val::Text(s)
        }
    }
    pub fn n(kind: NumKind, v: Option<f64>) -> Val {
        Val::Num(kind, v, NumOptions::default())
    }
    pub fn shown(&self) -> bool {
        match self {
            Val::None => false,
            Val::Text(t) => !t.is_empty(),
            Val::Num(k, v, o) => num_parts(*k, *v, *o).is_some(),
        }
    }
    /// The value as elements with text look `t`.
    pub fn el(&self, ui: &mut Ui, t: T) -> Option<El> {
        match self {
            Val::None => None,
            Val::Text(s_) => Some(ui.text(s_.clone(), t.tab())),
            Val::Num(k, v, o) => num(ui, s(), *k, *v, *o, t, true),
        }
    }
}

/// `Line`: a label and value on one line, a hairline under every line but the last (the border
/// adds its pixel to the line).
pub fn line(ui: &mut Ui, label: &str, value: &Val, strong: bool, last: bool) -> El {
    let w = if strong { 600 } else { 400 };
    let pb = if last { 0.45 } else { 0.45 + 1.0 / ui.rem };
    let mut row = ui.bx(s().row().items_end().justify_between().gap(1.0).pt(0.45).pb(pb));
    if !last {
        row = row.child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(1.0)).bg(tok::HAIR));
    }
    row = row.child(ui.text(label, T::new(Ty::Small).w(w).c(if strong { tok::FG } else { tok::MUTED })));
    let mut right = ui.bx(s().row().justify_end().shrink().min0());
    if let Some(v) = value.el(ui, T::new(Ty::Small).w(w)) {
        right = right.child(v);
    }
    row.child(right)
}

/// A `Line` whose value is an element (a styled span).
pub fn line_el(ui: &mut Ui, label: &str, value: Option<El>, strong: bool, last: bool) -> El {
    let w = if strong { 600 } else { 400 };
    let pb = if last { 0.45 } else { 0.45 + 1.0 / ui.rem };
    let mut row = ui.bx(s().row().items_end().justify_between().gap(1.0).pt(0.45).pb(pb));
    if !last {
        row = row.child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(1.0)).bg(tok::HAIR));
    }
    row = row.child(ui.text(label, T::new(Ty::Small).w(w).c(if strong { tok::FG } else { tok::MUTED })));
    row.child(ui.bx(s().row().justify_end().shrink().min0()).maybe(value))
}

/// Lines in a block; the last has no hairline.
pub fn lines(ui: &mut Ui, items: &[(&str, Val, bool)]) -> El {
    let n = items.len();
    let mut b = ui.bx(s().col());
    for (i, (l, v, strong)) in items.iter().enumerate() {
        b = b.child(line(ui, l, v, *strong, i + 1 == n));
    }
    b
}

/// `CoinLogo`: a round logo on layer-03 with the symbol's initial (0.9 x 0.45 of the size) under it
/// until the picture lands; never a broken image.
pub fn coin_logo(ui: &mut Ui, src: Option<&str>, symbol: &str, size: f32) -> El {
    let fs = size * 0.45 * 0.9;
    let label = if symbol.is_empty() { "?" } else { symbol };
    let fb = ui.bx(s().fill().row().items_center().justify_center()).child(ui.text(initial(label), T::new(Ty::Px(fs, fs * 1.2)).w(600).c(tok::fg(0.35))));
    let sources: Vec<String> = src.filter(|u| !u.is_empty()).map(String::from).into_iter().collect();
    // MImg: the picture (fading in over layer-03) while it loads or shows, the initial once every source failed.
    let img = kit::art(ui, s().fill(), &sources, size, size, kit::ArtFit::Cover(0.5), Some(fb)).radius(ui.r(size / 2.0));
    ui.bx(s().size(size)).bg(tok::L3).radius(ui.r(size / 2.0)).clip().child(img)
}

/// A Num with the change kind (sign and colour), text look `t`.
pub fn change(ui: &mut Ui, v: Option<f64>, t: T) -> Option<El> {
    num(ui, s(), NumKind::Change, v, NumOptions::default(), t, true)
}

/// A plain `Btn` for places that use the kit's button as is.
pub fn btn(label: &str, tag: u32) -> Btn {
    Btn::new(label, tag)
}

/// The thin bar along the top of a page that is still waiting for its first answer (a calm pulse,
/// never a skeleton).
pub fn loading_bar(ui: &mut Ui) -> El {
    let name = ui.loop_name(&rare_render::anim::BREATHE, 1.4);
    ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(2.0)).bg(tok::fg(0.55)).name(name)
}
