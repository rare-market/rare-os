//! Rare Wallet's UI kit (launcher/src/wallet/ui/kit.tsx) as markup, with the rare.market pieces it
//! re-exports from marketplace/parts.tsx (Btn, Avatar, WalletAvatar, Empty, Tabs, Segmented, Stat,
//! Delta) and the kit's Sui/SuiLogo. Class strings are the TSX ones, unchanged. Every Phase 3 screen
//! builds from these, so a look changes in one place, as on the web.

use super::fmt;
use super::markup::*;

// ------------------------------------------------------------------ rare.market parts

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Dimmed,
    Ghost,
    Outline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Sm,
    Md,
    Lg,
}

fn variant_class(v: Variant) -> &'static str {
    match v {
        Variant::Primary => "bg-button-primary text-on-brand font-semibold focus-visible:bg-button-primary-hover",
        Variant::Secondary => "bg-button-secondary text-primary focus-visible:bg-button-secondary-active",
        Variant::Dimmed => "bg-button-dimmed text-secondary focus-visible:text-primary",
        Variant::Ghost => "bg-transparent text-secondary focus-visible:text-primary",
        Variant::Outline => "border border-border-light bg-transparent text-primary",
    }
}

fn size_class(s: Size) -> &'static str {
    match s {
        Size::Sm => "h-8 px-3 text-sm gap-1.5",
        Size::Md => "h-10 px-4 text-sm gap-2",
        Size::Lg => "h-12 px-6 text-base gap-2",
    }
}

/// `<Btn variant size className>` (defaults: secondary, lg).
pub fn btn_full<M>(label: &str, variant: Variant, size: Size, round: bool, class: &str, on: Option<M>) -> El<M> {
    button(
        cn(&[
            "btn relative inline-flex shrink-0 items-center justify-center whitespace-nowrap font-medium transition-colors",
            if round { "rounded-full" } else { "rounded" },
            variant_class(variant),
            size_class(size),
            class,
        ]),
        on,
    )
    .text(label)
}

/// `<Btn><Icon size="1.1rem" /> Label</Btn>`: a lucide icon before the label.
pub fn btn_icon<M>(icon_name: &'static str, label: &str, variant: Variant, size: Size, class: &str, on: Option<M>) -> El<M> {
    let mut b = btn_full("", variant, size, false, class, on);
    b.text = None;
    b.kid(icon(icon_name, "").attr("size", "1.1rem")).kid(text(label))
}

pub fn btn<M>(label: &str, variant: Variant, class: &str, on: Option<M>) -> El<M> {
    btn_full(label, variant, Size::Lg, false, class, on)
}

/// `Avatar`: round (or square) art on layer-03; SUI with no icon is the droplet.
pub fn avatar<M>(src: Option<&str>, label: &str, size_rem: &str, square: bool) -> El<M> {
    if src.is_none_or(str::is_empty) && label == "SUI" {
        return span("inline-flex shrink-0 items-center justify-center rounded-full bg-layer-03")
            .attr("style", format!("width: {size_rem}; height: {size_rem}"))
            .kid(sui_logo(&format!("calc({size_rem} * 0.55)")));
    }
    span(cn(&["relative inline-block shrink-0 overflow-hidden bg-layer-03", if square { "rounded-md" } else { "rounded-full" }]))
        .attr("style", format!("width: {size_rem}; height: {size_rem}"))
        .kid(mimg(src, label, 120, "absolute inset-0 size-full"))
}

/// `MImg`: art that never shows broken (the initial on layer-03 when every source fails).
pub fn mimg<M>(src: Option<&str>, label: &str, width: u32, class: &str) -> El<M> {
    img(src, label, class).attr("data-width", width.to_string())
}

/// One of the player's own accounts: its picked picture, else its default picture.
pub fn account_avatar<M>(address: &str, size_rem: &str) -> El<M> {
    avatar(Some(&crate::data::live::picture(address)), "", size_rem, false)
}

/// `WalletAvatar`: the generated gem pattern for an address.
pub fn wallet_avatar<M>(address: &str, size_rem: &str) -> El<M> {
    el(Tag::WalletAvatar(address.to_string()), "inline-block shrink-0 rounded-full bg-cover bg-center").attr("style", format!("width: {size_rem}; height: {size_rem}"))
}

/// The Sui droplet (`SuiLogo`), sized by height.
pub fn sui_logo<M>(size: &str) -> El<M> {
    icon("sui", "inline-block shrink-0").attr("style", format!("height: {size}; width: calc({size} * 0.7823)")).label("SUI")
}

/// `Empty`: centred secondary text, at most one action.
pub fn empty<M>(title: &str, body: Option<El<M>>, action: Option<El<M>>, compact: bool) -> El<M> {
    div(cn(&["flex flex-col items-center justify-center gap-3 px-6 text-center", if compact { "py-10" } else { "py-20" }]))
        .kid(p("text-sm text-secondary").text(title))
        .kid_opt(body.map(|b| div("max-w-sm text-xs text-secondary").kid(b)))
        .kid_opt(action)
}

/// `Tabs`: underline tabs, LT/RT switch them.
pub fn tabs<M: Clone>(items: &[(&str, &str)], value: &str, on: impl Fn(&str) -> M, class: &str) -> El<M> {
    let mut row = div(cn(&["flex items-end gap-8", class])).role("tablist").kid(span("pb-2.5 opacity-70").kid(glyph("lt")));
    for (v, label) in items {
        let active = *v == value;
        let mut b = button(
            cn(&["relative flex shrink-0 items-center whitespace-nowrap rounded px-1 pb-2.5 pt-1 text-base font-medium transition-colors", if active { "text-primary" } else { "text-secondary focus-visible:text-primary" }]),
            Some(on(v)),
        )
        .attr("data-tab", *v)
        .role("tab")
        .attr("aria-selected", active.to_string())
        .kid(text(*label));
        if active {
            b = b.kid(span("absolute inset-x-0 bottom-0 h-[2px] rounded-full bg-fg"));
        }
        row = row.kid(b);
    }
    row.kid(span("pb-2.5 opacity-70").kid(glyph("rt")))
}

/// `Segmented`.
pub fn segmented<M: Clone>(items: &[(&str, &str)], value: &str, on: impl Fn(&str) -> M, class: &str) -> El<M> {
    let n = items.len();
    div(cn(&["inline-flex gap-px", class])).role("radiogroup").kids(items.iter().enumerate().map(|(i, (v, label))| {
        let active = *v == value;
        button(
            cn(&[
                "inline-flex h-9 min-w-11 items-center justify-center px-3 text-sm transition-colors",
                when(i == 0, "rounded-l"),
                when(i + 1 == n, "rounded-r"),
                if active { "bg-button-secondary-active text-primary" } else { "bg-button-secondary text-secondary focus-visible:text-primary" },
            ]),
            Some(on(v)),
        )
        .role("radio")
        .attr("aria-checked", active.to_string())
        .text(*label)
    }))
}

/// `Stat`: label over a value.
pub fn stat<M>(label: &str, value: El<M>) -> El<M> {
    div("shrink-0").kid(p("text-xs text-secondary").text(label)).kid(div("mt-0.5 flex items-baseline gap-1 text-base font-semibold tabular-nums").kid(value))
}

/// `Delta`: percent change with rare.market's triangle; unknown is an empty cell.
pub fn delta<M>(value: Option<f64>, dp: usize, class: &str) -> El<M> {
    let Some(v) = value.filter(|v| v.is_finite()) else { return span(class) };
    let (up, down) = (v > 0.0, v < 0.0);
    let mut e = span(cn(&["whitespace-nowrap tabular-nums", if up { "text-positive" } else if down { "text-negative" } else { "text-secondary" }, class]));
    if up || down {
        e = e.kid(icon("triangle", cn(&["mr-0.5 inline", when(down, "rotate-180")])).attr("style", "width: 0.5rem; height: 0.5rem"));
    }
    e.kid(text(fmt_pct(v, dp)))
}

/// marketplace/format.ts `fmtPct`: "+16.3%" / "-1.6%" with a real minus? rare.market prints the
/// sign of the number and drops "+" (the triangle carries direction).
pub fn fmt_pct(v: f64, dp: usize) -> String {
    format!("{}%", super::fmt::to_fixed(v.abs(), dp))
}

// ------------------------------------------------------------------ wallet kit

/// A wallet page: title row, optional subtitle and right side, scrolling body. `narrow` centres a
/// form column (flows like send, import); default is the full launcher width.
pub fn page<M>(title: Option<&str>, subtitle: Option<El<M>>, right: Option<El<M>>, narrow: bool, class: &str, body: Vec<El<M>>) -> El<M> {
    page_with(title.map(text), subtitle, right, narrow, class, body)
}

/// `page` with a composed title (an avatar beside the name).
pub fn page_with<M>(title: Option<El<M>>, subtitle: Option<El<M>>, right: Option<El<M>>, narrow: bool, class: &str, body: Vec<El<M>>) -> El<M> {
    let mut inner = div(cn(&[if narrow { "mx-auto max-w-[44rem]" } else { "" }, class]));
    if title.is_some() || right.is_some() {
        let mut head = div("min-w-0");
        if let Some(t) = title {
            let mut h = h1("truncate text-2xl font-bold leading-[1.4] tracking-[-0.01em]");
            if t.tag == Tag::Span && t.class.is_empty() && t.kids.is_empty() {
                h.text = t.text;
            } else {
                h = h.kid(t);
            }
            head = head.kid(h);
        }
        if let Some(s) = subtitle {
            let mut sp = p("mt-1 text-sm text-secondary");
            if s.tag == Tag::Span && s.class.is_empty() && s.kids.is_empty() {
                sp.text = s.text;
            } else {
                sp = sp.kid(s);
            }
            head = head.kid(sp);
        }
        let mut row = div("mb-6 flex items-end justify-between gap-6").kid(head);
        if let Some(r) = right {
            row = row.kid(div("flex shrink-0 items-center gap-2").kid(r));
        }
        inner = inner.kid(row);
    }
    div("h-full overflow-y-auto px-[2.5rem] pb-[2.5rem] pt-[0.75rem]").attr("data-scroll", "").kid(inner.kids(body))
}

/// rare.market panel: layer-01 card with the primary border.
pub fn panel<M>(title: Option<&str>, right: Option<El<M>>, class: &str, body: Vec<El<M>>) -> El<M> {
    let mut s = section(cn(&["rounded-lg border border-border-primary bg-layer-01", class]));
    if title.is_some() || right.is_some() {
        let mut head = div("flex items-center justify-between gap-4 px-5 pt-4");
        if let Some(t) = title {
            head = head.kid(h2("text-base font-semibold").text(t));
        }
        s = s.kid(head.kid_opt(right));
    }
    s.kids(body)
}

/// Label on the left, value on the right (review sheets, details).
pub fn row<M>(label: &str, value: Vec<El<M>>, class: &str, muted: bool) -> El<M> {
    div(cn(&["flex items-baseline justify-between gap-6 text-sm", class]))
        .kid(span(if muted { "shrink-0 text-secondary" } else { "shrink-0" }).text(label))
        .kid(span("min-w-0 text-right").kids(value))
}

/// A focusable list row (menus, account lists, settings).
pub fn list_button<M>(icon_el: Option<El<M>>, title: &str, body: Option<El<M>>, right: Option<El<M>>, on: M, def: bool, class: &str) -> El<M> {
    let mut b = button(cn(&["flex w-full items-center gap-4 rounded px-4 py-3.5 text-left transition-colors", class]), Some(on)).def(def);
    if let Some(i) = icon_el {
        b = b.kid(span("flex shrink-0 items-center justify-center text-icon-primary").kid(i));
    }
    let mut mid = span("min-w-0 flex-1").kid(span("block truncate text-base font-medium").text(title));
    if let Some(bd) = body {
        mid = mid.kid(span("focus-text mt-0.5 block text-sm leading-snug text-secondary").kid(bd));
    }
    b = b.kid(mid);
    if let Some(r) = right {
        b = b.kid(span("shrink-0 text-sm text-secondary").kid(r));
    }
    b
}

/// A text field for the controller: A opens the on-screen keyboard. `secret` shows dots only (at
/// most 24), never the value.
#[derive(Default)]
pub struct FieldSpec<'a> {
    pub label: Option<&'a str>,
    pub value: &'a str,
    pub placeholder: Option<&'a str>,
    pub secret: bool,
    pub hint: Option<&'a str>,
    pub error: Option<&'a str>,
    pub def: bool,
    pub class: &'a str,
}


pub fn field<M>(f: FieldSpec, on: M) -> El<M> {
    let shown = if f.value.is_empty() {
        f.placeholder.unwrap_or("").to_string()
    } else if f.secret {
        "•".repeat(f.value.chars().count().min(24))
    } else {
        f.value.to_string()
    };
    let mut w = div(f.class);
    if let Some(l) = f.label {
        w = w.kid(div("mb-1.5 text-sm text-secondary").text(l));
    }
    w = w.kid(
        button(
            cn(&["flex h-12 w-full min-w-0 items-center rounded border bg-transparent px-4 text-left text-base transition-colors", if f.error.is_some() { "border-negative/60" } else { "border-border-light" }]),
            Some(on),
        )
        .def(f.def)
        .hint_a("Type")
        .kid(span(cn(&["min-w-0 flex-1 truncate", if f.value.is_empty() { "text-secondary" } else { "text-primary" }])).text(shown)),
    );
    if let Some(e) = f.error {
        w = w.kid(div("mt-1.5 text-sm text-negative").text(e));
    } else if let Some(h) = f.hint {
        w = w.kid(div("mt-1.5 text-sm text-secondary").text(h));
    }
    w
}

/// rare.market's Toggle as a focusable row with its label.
pub fn toggle_row<M>(label: &str, body: Option<&str>, checked: bool, on: M) -> El<M> {
    let mut mid = span("min-w-0 flex-1").kid(span("block text-base font-medium").text(label));
    if let Some(b) = body {
        mid = mid.kid(span("focus-text mt-0.5 block text-sm text-secondary").text(b));
    }
    button("flex w-full items-center gap-4 rounded px-4 py-3.5 text-left", Some(on))
        .role("switch")
        .attr("aria-checked", checked.to_string())
        .kid(mid)
        .kid(
            span(cn(&["relative h-6 w-[2.6rem] shrink-0 rounded-full transition-colors", if checked { "bg-primary" } else { "bg-layer-04" }]))
                .kid(span(cn(&["absolute top-1/2 h-[1.1rem] w-[1.1rem] -translate-y-1/2 rounded-full transition-[left]", if checked { "left-[1.3rem] bg-background" } else { "left-[0.2rem] bg-icon-secondary" }]))),
        )
}

/// rare.market's modal: dimmed backdrop, header with a close button (when `close` is set), the
/// body, an optional footer. B sends `close`; without one B does nothing (the sheet is busy).
pub struct SheetSpec<M> {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<El<M>>,
    pub footer: Option<Vec<El<M>>>,
    pub close: Option<M>,
    /// rem, e.g. "44rem"; the default is 52rem
    pub width: &'static str,
}

pub fn sheet<M: Clone>(s: SheetSpec<M>, body: Vec<El<M>>) -> El<M> {
    let mut head_left = div("flex min-w-0 items-center gap-3");
    if let Some(i) = s.icon {
        head_left = head_left.kid(i);
    }
    let mut titles = div("min-w-0").kid(h2("truncate text-xl font-bold leading-[1.55]").text(s.title.clone()));
    if let Some(sub) = &s.subtitle {
        titles = titles.kid(div("truncate text-sm text-secondary").text(sub.clone()));
    }
    head_left = head_left.kid(titles);
    let mut head = div("flex shrink-0 items-start justify-between gap-4 border-b border-border-primary px-8 pb-4 pt-6").kid(head_left);
    if let Some(c) = &s.close {
        head = head.kid(button("-mr-1 mt-1 flex size-10 items-center justify-center rounded text-icon-primary", Some(c.clone())).label("Close").kid(icon("x", "").attr("size", "1.25rem")));
    }
    let mut dialog = div("fade-in relative flex max-h-full w-full flex-col overflow-hidden rounded-modal border border-border-light bg-background")
        .attr("data-nav-scope", "")
        .role("dialog")
        .attr("style", format!("max-width: {}", s.width))
        .kid(head)
        .kid(div("min-h-0 flex-1 overflow-y-auto px-8 py-6").attr("data-scroll", "").kids(body));
    if let Some(f) = s.footer {
        dialog = dialog.kid(div("flex shrink-0 gap-2 border-t border-border-primary px-8 py-4").kids(f));
    }
    let mut root = div("fixed inset-x-0 bottom-[3rem] top-0 z-40 flex items-center justify-center p-6").kid(div("absolute inset-0 bg-background/50 backdrop-blur-sm").attr("aria-hidden", "true")).kid(dialog);
    if let Some(c) = s.close {
        root = root.on_pad("b", c);
    }
    root
}

/// The B action of a sheet built by `sheet` (None while the sheet is busy).
pub fn sheet_b<M: Clone>(s: &El<M>) -> Option<M> {
    s.pads.iter().find(|(b, _)| *b == "b").map(|(_, m)| m.clone())
}

pub const PIN_KEYS: [&str; 12] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "back", "0", "ok"];

/// Number pad for PINs: the d-pad moves over the keys, A presses, X deletes, Start confirms.
/// Dots show the PIN length; the digits never show when masked.
pub struct PinPadSpec<'a> {
    pub length: Option<usize>,
    pub value: &'a str,
    pub min_length: usize,
    pub masked: bool,
    pub disabled: bool,
    pub ok_label: &'a str,
}

impl Default for PinPadSpec<'_> {
    fn default() -> Self {
        PinPadSpec { length: None, value: "", min_length: 6, masked: true, disabled: false, ok_label: "OK" }
    }
}

/// A press on the pad: the new value and whether it submits (the PinPad's `press`).
pub fn pin_press(value: &str, key: &str, length: Option<usize>, min_length: usize, max_length: usize) -> (String, bool) {
    match key {
        "back" => {
            let mut v = value.to_string();
            v.pop();
            (v, false)
        }
        "ok" => (value.to_string(), value.chars().count() >= min_length),
        d => {
            if value.chars().count() < length.unwrap_or(max_length) {
                let next = format!("{value}{d}");
                let done = length.is_some_and(|l| next.chars().count() == l);
                (next, done)
            } else {
                (value.to_string(), false)
            }
        }
    }
}

pub fn pin_pad<M: Clone>(s: PinPadSpec, on_key: impl Fn(&'static str) -> M) -> El<M> {
    let n = s.value.chars().count();
    let shown = s.length.unwrap_or(s.min_length.max(n));
    let dots = if s.masked {
        div("flex h-6 items-center gap-3").label(&format!("{n} digits")).kids((0..shown).map(|i| span(cn(&["size-3.5 rounded-full transition-colors", if i < n { "bg-primary" } else { "bg-layer-04" }]))))
    } else {
        div("flex h-6 items-center gap-3").label(&format!("{n} digits")).kid(span("font-mono text-2xl tabular-nums").text(if s.value.is_empty() { " " } else { s.value }))
    };
    let keys = div("grid grid-cols-3 gap-2").kids(PIN_KEYS.iter().map(|k| {
        let label = match *k {
            "back" => "Del",
            "ok" => s.ok_label,
            d => d,
        };
        button(
            cn(&[
                "btn flex h-16 w-24 items-center justify-center rounded text-2xl font-medium tabular-nums disabled:opacity-40",
                if *k == "ok" { "bg-button-primary text-base font-semibold" } else { "bg-button-secondary" },
            ]),
            Some(on_key(k)),
        )
        .def(*k == "5")
        .disabled(s.disabled || (*k == "ok" && n < s.min_length))
        .key(*k)
        .text(label)
    }));
    div("flex flex-col items-center gap-6").kid(dots).kid(keys)
}

/// `Addr`: address in mono, middle shortened (or in full, breaking anywhere).
pub fn addr<M>(address: &str, full: bool, class: &str) -> El<M> {
    span(cn(&["font-mono tabular-nums [font-variant-ligatures:none]", when(full, "break-all"), class])).text(if full { address.to_string() } else { fmt::addr_short(address) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_pad_rules() {
        assert_eq!(pin_press("12345", "6", None, 6, 12), ("123456".into(), false));
        assert_eq!(pin_press("123456", "ok", None, 6, 12), ("123456".into(), true));
        assert_eq!(pin_press("12345", "ok", None, 6, 12), ("12345".into(), false));
        assert_eq!(pin_press("12345", "6", Some(6), 6, 12), ("123456".into(), true));
        assert_eq!(pin_press("123456789012", "3", None, 6, 12), ("123456789012".into(), false));
        assert_eq!(pin_press("12", "back", None, 6, 12), ("1".into(), false));
    }

    #[test]
    fn secret_field_never_shows_the_value() {
        let f: El<()> = field(FieldSpec { label: Some("Passphrase"), value: "hunter2", secret: true, ..Default::default() }, ());
        assert!(!f.content().contains("hunter2"));
        assert!(f.content().contains("•••••••"));
    }

    #[test]
    fn pin_pad_masks_and_lands_on_five() {
        let p: El<&str> = pin_pad(PinPadSpec { value: "1234", ..Default::default() }, |k| k);
        // the dots row holds no digits, only one dot per digit typed
        assert!(p.kids[0].texts().is_empty());
        assert_eq!(p.kids[0].kids.iter().filter(|d| d.class.contains("bg-primary")).count(), 4);
        assert_eq!(p.landing().as_deref(), Some("5"));
        assert!(p.find_nav("OK").unwrap().disabled);
    }
}
