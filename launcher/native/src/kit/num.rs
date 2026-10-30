//! `Num` (ui/Num.tsx, the Trade API's): every money, price, percent and count. Tabular figures, cents
//! dimmed (opacity 50 %), P/L and change signed and coloured, never wraps or truncates; an
//! unknown value draws nothing (`None`).
//!
//! Rolling digits and the one-time tint of `live` numbers (`NumOptions::live`) are the text
//! engine's (`rare_render::text::roll`, carried by `El::roll`).

use super::format::{num_parts, NumKind, NumOptions, NumTone};
use super::tok;
use super::ui::{s, Ui, S, T};
use rare_render::view::El;
use rare_render::Color;

/// The colour a number takes: pnl and change colour themselves (`tone`), others keep `base`.
pub fn tone_color(tone: Option<NumTone>, base: Color) -> Color {
    match tone {
        Some(NumTone::Up) => tok::POS,
        Some(NumTone::Down) => tok::NEG,
        Some(NumTone::Flat) => tok::MUTED,
        None => base,
    }
}

/// `<Num kind value className>`; `t` is the surrounding text look, `tone` false is tone="none".
pub fn num(ui: &mut Ui, st: S, kind: NumKind, value: Option<f64>, opts: NumOptions, t: T, tone: bool) -> Option<El> {
    let p = num_parts(kind, value, opts)?;
    let c = if tone { tone_color(p.tone, t.color) } else { t.color };
    let t = t.tab().c(c);
    let live = |el: El| match (opts.live, value) {
        (true, Some(v)) => el.roll(v, tok::POS, tok::NEG, ui.r(0.25)),
        _ => el,
    };
    let mut row = ui.bx(st.row().items_end()).radius(ui.r(0.25)).child(live(ui.text(format!("{}{}", p.sign, p.body), t)));
    if !p.dim.is_empty() {
        row = row.child(live(ui.text(p.dim.clone(), t.op(0.5))));
    }
    Some(row)
}

/// The plain text of a number for sentences ("" when unknown).
pub fn num_str(kind: NumKind, value: Option<f64>) -> String {
    super::format::num_text(kind, value, NumOptions::default())
}

/// A number in a fixed slot: the slot keeps its place when the value is unknown.
pub fn num_or_empty(ui: &mut Ui, st: S, kind: NumKind, value: Option<f64>, opts: NumOptions, t: T, tone: bool) -> El {
    num(ui, st.clone(), kind, value, opts, t, tone).unwrap_or_else(|| ui.bx(s()))
}
