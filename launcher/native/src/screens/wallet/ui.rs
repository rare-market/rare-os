//! The launcher's own kit (launcher/src/ui: controls.tsx, Num.tsx, StatLine.tsx, Thumb.tsx) as
//! markup, for the Phase 3 screens that use it (Sui tab, sign in, first run, inventory, trophies,
//! AddPasskey). Class strings are the TSX ones; numbers go through the kit's `num_parts`
//! (crate::kit::format, workstream 12's port of ui/format.ts).

use super::markup::*;
use crate::kit::format::{num_parts, NumKind, NumOptions, NumTone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BVariant {
    Primary,
    Secondary,
    Dimmed,
    Ghost,
    Outline,
    Buy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BSize {
    Sm,
    Md,
    Lg,
}

fn variant(v: BVariant) -> &'static str {
    match v {
        BVariant::Primary => "bg-brand text-fg font-semibold",
        BVariant::Secondary => "bg-btn-2 text-fg font-medium",
        BVariant::Dimmed => "bg-button-dimmed text-muted font-medium",
        BVariant::Ghost => "bg-transparent text-muted font-medium",
        BVariant::Outline => "border border-border-light bg-transparent text-fg font-medium",
        BVariant::Buy => "bg-[rgb(var(--button-buy))] text-[#0E0A14] font-semibold",
    }
}

fn size(s: BSize) -> &'static str {
    match s {
        BSize::Sm => "h-[2.25rem] px-[0.9rem] text-small gap-[0.4rem]",
        BSize::Md => "h-[2.85rem] px-[1.35rem] text-body gap-[0.55rem]",
        BSize::Lg => "h-[3.25rem] px-[1.75rem] text-row gap-[0.6rem]",
    }
}

/// `Button` (defaults: secondary, md). `hint` is the A label in the hint bar while focused.
pub struct ButtonSpec<'a> {
    pub label: &'a str,
    pub variant: BVariant,
    pub size: BSize,
    pub icon: Option<El<()>>,
    pub class: &'a str,
    pub def: bool,
    pub disabled: bool,
    pub hint: Option<&'a str>,
}

impl<'a> ButtonSpec<'a> {
    pub fn new(label: &'a str) -> Self {
        ButtonSpec { label, variant: BVariant::Secondary, size: BSize::Md, icon: None, class: "", def: false, disabled: false, hint: None }
    }
    pub fn v(mut self, v: BVariant) -> Self {
        self.variant = v;
        self
    }
    pub fn size(mut self, s: BSize) -> Self {
        self.size = s;
        self
    }
    pub fn class(mut self, c: &'a str) -> Self {
        self.class = c;
        self
    }
    pub fn def(mut self, d: bool) -> Self {
        self.def = d;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn hint(mut self, h: &'a str) -> Self {
        self.hint = Some(h);
        self
    }
    pub fn icon(mut self, name: &'static str, class: &str) -> Self {
        self.icon = Some(icon(name, class));
        self
    }
}

pub fn button_el<M>(b: ButtonSpec, on: Option<M>) -> El<M> {
    let mut e = button(cn(&["btn inline-flex shrink-0 items-center justify-center whitespace-nowrap rounded-chip disabled:opacity-40", variant(b.variant), size(b.size), b.class]), on).def(b.def).disabled(b.disabled);
    if let Some(h) = b.hint {
        e = e.hint_a(h);
    }
    if let Some(i) = b.icon {
        e = e.kid(i.map(&|_| unreachable_msg()));
    }
    if !b.label.is_empty() {
        if e.kids.is_empty() {
            e = e.text(b.label);
        } else {
            e = e.kid(text(b.label));
        }
    }
    e
}

// An icon carries no message; `map` needs a function anyway.
fn unreachable_msg<M>() -> M {
    unreachable!("icons have no nav")
}

/// `Toggle`: display only; the row around it takes focus and A.
pub fn toggle<M>(on: bool) -> El<M> {
    span(cn(&["relative inline-block h-[1.5rem] w-[2.6rem] shrink-0 rounded-full transition-colors duration-150", if on { "bg-fg" } else { "bg-l4" }]))
        .attr("aria-hidden", "true")
        .kid(span(cn(&["absolute top-1/2 h-[1.1rem] w-[1.1rem] -translate-y-1/2 rounded-full transition-transform duration-150", if on { "translate-x-[1.3rem] bg-bg" } else { "translate-x-[0.2rem] bg-icon-2" }])))
}

/// `Tabs` (display only; LB/RB or X switch them). `count` is the muted number after a label.
pub fn tabs<M>(items: &[(&str, &str, Option<u32>)], value: &str, small: bool, class: &str) -> El<M> {
    div(cn(&["flex items-stretch gap-[1.25rem]", class])).role("tablist").kids(items.iter().map(|(v, label, count)| {
        let on = *v == value;
        let mut inner = span("relative flex items-center gap-[0.4rem]").kid(text(*label));
        if let Some(c) = count {
            inner = inner.kid(span(cn(&["tabular-nums text-meta", if on { "text-secondary" } else { "text-secondary/70" }])).text(c.to_string()));
        }
        if on {
            inner = inner.kid(span("absolute inset-x-0 -bottom-[0.3rem] h-[2px] rounded-full bg-fg"));
        }
        span(cn(&["flex shrink-0 items-center whitespace-nowrap font-medium transition-colors duration-200", if small { "text-small" } else { "text-body" }, if on { "text-primary" } else { "text-secondary" }]))
            .role("tab")
            .attr("aria-selected", on.to_string())
            .key(*v)
            .kid(inner)
    }))
}

/// `Segmented` (display only).
pub fn segmented<M>(items: &[(&str, &str)], value: &str, class: &str) -> El<M> {
    let n = items.len();
    div(cn(&["inline-flex gap-px", class])).role("radiogroup").kids(items.iter().enumerate().map(|(i, (v, label))| {
        span(cn(&[
            "inline-flex h-[1.9rem] min-w-[2.4rem] items-center justify-center px-[0.6rem] text-meta font-medium",
            when(i == 0, "rounded-l-chip"),
            when(i + 1 == n, "rounded-r-chip"),
            if *v == value { "bg-button-secondary-active text-primary" } else { "bg-button-secondary text-secondary" },
        ]))
        .role("radio")
        .attr("aria-checked", (*v == value).to_string())
        .key(*v)
        .text(*label)
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Brand,
    Positive,
    Negative,
    Attention,
    Primary,
}

/// `Dot`: the only coloured marker allowed. `live` breathes.
pub fn dot<M>(tone: Tone, live: bool, class: &str) -> El<M> {
    let c = match tone {
        Tone::Muted => "bg-icon-2",
        Tone::Brand | Tone::Primary => "bg-fg",
        Tone::Positive => "bg-positive",
        Tone::Negative => "bg-negative",
        Tone::Attention => "bg-attention",
    };
    span(cn(&["inline-block h-[0.45rem] w-[0.45rem] shrink-0 rounded-full", c, when(live, "breathe"), class])).attr("aria-hidden", "true")
}

/// `LoadBar`: the thin loading bar; place it in a relative box.
pub fn load_bar<M>(on: bool, class: &str) -> El<M> {
    div(cn(&["pointer-events-none absolute inset-x-0 z-30 h-[2px] transition-opacity duration-200", if on { "load-bar opacity-100" } else { "opacity-0" }, if class.is_empty() { "top-0" } else { class }])).attr("aria-hidden", "true")
}

/// `Empty` (the launcher's): a sentence, an optional line, one action.
pub fn empty<M>(title: &str, body: Option<El<M>>, actions: Vec<El<M>>, class: &str) -> El<M> {
    let mut d = div(cn(&["flex flex-col items-start gap-[0.5rem] rounded-card border border-hair bg-l1 px-[1.5rem] py-[1.3rem]", class])).kid(div("text-row font-semibold").text(title));
    if let Some(b) = body {
        d = d.kid(div("max-w-[34rem] text-small text-muted").kid(b));
    }
    if !actions.is_empty() {
        d = d.kid(div("mt-[0.5rem] flex gap-[0.75rem]").kids(actions));
    }
    d
}

/// `Panel` (the launcher's: rounded-panel layer-01, no border).
pub fn panel<M>(class: &str, kids: Vec<El<M>>) -> El<M> {
    div(cn(&["rounded-panel bg-l1", class])).kids(kids)
}

/// `SectionHead`.
pub fn section_head<M>(title: El<M>, note: Option<El<M>>, right: Option<El<M>>, class: &str) -> El<M> {
    let mut h = h2("text-row font-semibold tracking-[-0.01em]");
    if title.tag == Tag::Span && title.class.is_empty() && title.kids.is_empty() {
        h.text = title.text;
    } else {
        h = h.kid(title);
    }
    div(cn(&["flex items-center gap-[0.75rem]", class]))
        .kid(h)
        .kid_opt(note.map(|n| span("text-small text-muted").kid(n)))
        .kid_opt(right.map(|r| div("ml-auto flex items-center gap-[0.75rem] text-small text-muted").kid(r)))
}

/// `Rail`: a horizontal shelf of tiles; focus scrolls it.
pub fn rail<M>(id: &str, title: Option<El<M>>, note: Option<El<M>>, right: Option<El<M>>, class: &str, kids: Vec<El<M>>) -> El<M> {
    let mut s = section(cn(&["relative", class]));
    if let Some(t) = title {
        s = s.kid(section_head(t, note, right, "mb-[0.9rem] px-[2.5rem]"));
    }
    s.kid(div("relative flex items-start gap-[1rem] overflow-x-auto px-[2.5rem]").attr("data-rail", "").attr("data-nav-group", id).kids(kids))
}

/// `Row`: a focusable table row, 3.25rem high.
pub fn row<M>(cols: &str, idx: Option<usize>, hint: Option<&str>, class: &str, on: Option<M>, kids: Vec<El<M>>) -> El<M> {
    let mut b = button(cn(&["trow grid h-[3.25rem] w-full items-center gap-x-[0.75rem] rounded-card px-[0.75rem] text-left text-small", cols, class]), on);
    if let Some(i) = idx {
        b = b.attr("data-idx", i.to_string());
    }
    if let Some(h) = hint {
        b = b.hint_a(h);
    }
    b.kids(kids)
}

pub fn head_row<M>(cols: &str, class: &str, kids: Vec<El<M>>) -> El<M> {
    div(cn(&["trow grid h-[2rem] items-center gap-x-[0.75rem] px-[0.75rem] text-label font-medium text-secondary", cols, class])).kids(kids)
}

/// `InfoCard`: label, one big value, a muted line; a focusable tile when it has an action.
pub fn info_card<M>(label: &str, value: El<M>, sub: Option<El<M>>, on: Option<M>, hint: Option<&str>, class: &str) -> El<M> {
    let body = vec![
        span("block text-label font-medium text-secondary").text(label),
        span("mt-auto block truncate text-title font-semibold tabular-nums text-fg").kid(value),
    ];
    let cls = cn(&["flex h-[6rem] w-[14rem] shrink-0 flex-col rounded-card bg-l1 px-[1rem] py-[0.875rem] text-left", class]);
    let mut e = match on {
        Some(m) => {
            let mut b = button(cn(&["tile tile-wide", &cls]), Some(m));
            if let Some(h) = hint {
                b = b.hint_a(h);
            }
            b
        }
        None => div(cls),
    };
    e = e.kids(body);
    if let Some(s) = sub {
        e = e.kid(span("mt-[0.15rem] block truncate text-meta text-muted").kid(s));
    }
    e
}

pub fn card_row<M>(id: &str, class: &str, kids: Vec<El<M>>) -> El<M> {
    div(cn(&["flex gap-[1rem] overflow-x-auto", class])).attr("data-rail", "").attr("data-nav-group", id).kids(kids)
}

/// `Hint`: a glyph and its label.
pub fn hint_el<M>(b: &'static str, label: &str) -> El<M> {
    span("flex items-center gap-[0.45rem] whitespace-nowrap").kid(glyph(b)).kid(text(label))
}

/// `Num`: tabular, cents dimmed, P/L signed, an unknown value renders nothing (None).
pub fn num<M>(kind: NumKind, value: Option<f64>, opts: NumOptions, class: &str, hidden: bool, auto_tone: bool) -> Option<El<M>> {
    const BASE: &str = "tabular-nums shrink-0 whitespace-nowrap rounded-[0.25rem]";
    if hidden {
        return Some(span(cn(&[BASE, class])).text("••••"));
    }
    let parts = num_parts(kind, value, opts)?;
    let color = match (auto_tone, parts.tone) {
        (true, Some(NumTone::Up)) => "text-positive",
        (true, Some(NumTone::Down)) => "text-negative",
        (true, Some(NumTone::Flat)) => "text-secondary",
        _ => "",
    };
    let mut s = span(cn(&[BASE, color, class])).kid(text(format!("{}{}", parts.sign, parts.body)));
    if !parts.dim.is_empty() {
        s = s.kid(span("opacity-50").text(parts.dim));
    }
    Some(s)
}

/// `StatLine`: label over value with hairlines; items with nothing to show are left out.
pub fn stat_line<M>(items: Vec<(String, Option<El<M>>)>, fill: bool, large: bool, class: &str) -> Option<El<M>> {
    let shown: Vec<(String, El<M>)> = items.into_iter().filter_map(|(l, v)| v.map(|v| (l, v))).collect();
    if shown.is_empty() {
        return None;
    }
    let n = shown.len();
    Some(el(Tag::Div, cn(&["flex", class])).attr("data-tag", "dl").kids(shown.into_iter().enumerate().map(|(i, (l, v))| {
        div(cn(&["flex min-w-0 shrink-0 flex-col border-hair", when(fill, "flex-1"), when(i > 0, "border-l pl-[1rem]"), when(i + 1 < n, "pr-[1rem]")]))
            .key(i.to_string())
            .kid(div("whitespace-nowrap text-label text-secondary").text(l))
            .kid(div(cn(&["tabular-nums truncate whitespace-nowrap font-medium text-primary", if large { "text-row" } else { "text-body" }])).kid(v))
    })))
}

/// `Thumb`: remote art that never shows broken (the initial on layer-03).
pub fn thumb<M>(src: Option<&str>, label: &str, class: &str, contain: bool) -> El<M> {
    img(src, label, class).attr("data-thumb", if contain { "contain" } else { "cover" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces() {
        let b: El<u8> = button_el(ButtonSpec::new("Sign in").v(BVariant::Primary).hint("Sign in"), Some(1));
        assert_eq!(b.texts(), ["Sign in"]);
        assert!(b.class.contains("bg-brand"));
        assert_eq!(b.nav.as_ref().unwrap().hint_a.as_deref(), Some("Sign in"));
        let b: El<u8> = button_el(ButtonSpec::new("Play").icon("play", ""), Some(1));
        assert_eq!(b.texts(), ["Play"]);
        assert_eq!(b.kids.len(), 2);
        let s: Option<El<()>> = stat_line(vec![("Price".into(), Some(text("$1"))), ("Volume".into(), None)], true, false, "");
        assert_eq!(s.unwrap().texts(), ["Price", "$1"]);
        let n: El<()> = num(NumKind::Money, Some(1284.07), NumOptions::default(), "", false, true).unwrap();
        assert_eq!(n.content(), "$1,284.07");
        assert!(num::<()>(NumKind::Money, None, NumOptions::default(), "", false, true).is_none());
    }
}
