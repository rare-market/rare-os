//! Number rules for Rare OS (the web launcher's ui/format.ts). Trade
//! prints every figure through these; num.test.ts pinned them from the Trade tab, so they live here.
//!
//! ```text
//!   money   your money: full, cents dimmed            $4,151.49
//!   pnl     profit and loss: money, always signed     +$4,151.49
//!   market  market figures: compact, 4 digits at most $10.91M, $247K
//!   price   3 significant digits, subscript zeros     $0.0109, $0.0₅6093
//!   change  sign and colour, 2 decimals under 100%    +4.15%, −9.46%
//!   count   exact under 1,000, then compact           901, 23.4K
//!   share   whole percent or 1 decimal                36%, 4.6%
//!   amount  a coin quantity                           12.5, 1.25M
//! ```
//!
//! An unknown value has no parts: the caller leaves it out.

use crate::jsnum::{js_number, js_round, js_string, locale, to_fixed, to_precision, Locale};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumKind {
    Money,
    Pnl,
    Market,
    Price,
    Change,
    Count,
    Share,
    Amount,
}

pub const KINDS: [NumKind; 8] = [NumKind::Money, NumKind::Pnl, NumKind::Market, NumKind::Price, NumKind::Change, NumKind::Count, NumKind::Share, NumKind::Amount];

/// Up and down colour the number; flat is a signed kind sitting at zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumTone {
    Up,
    Down,
    Flat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumOptions {
    /// money, pnl: false drops the cents ("+$412" in a headline).
    pub cents: bool,
    /// money, pnl: compact like a market figure ("+$35.6K") for totals that are not one person's
    /// balance.
    pub compact: bool,
    /// share: 0 in a sentence ("36%"), 1 in a table ("4.6%"), 2 for a rate ("5.27%" APY).
    pub decimals: u8,
}

impl Default for NumOptions {
    fn default() -> Self {
        NumOptions { cents: true, compact: false, decimals: 1 }
    }
}

impl NumOptions {
    pub fn no_cents() -> Self {
        NumOptions { cents: false, ..Self::default() }
    }
    pub fn compact() -> Self {
        NumOptions { compact: true, ..Self::default() }
    }
    pub fn decimals(d: u8) -> Self {
        NumOptions { decimals: d, ..Self::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumParts {
    /// "+", "−" (a real minus sign) or "".
    pub sign: &'static str,
    /// The value itself, without the sign.
    pub body: String,
    /// Cents, shown dimmed: ".49". Empty when there are none.
    pub dim: String,
    /// None for kinds that carry no colour.
    pub tone: Option<NumTone>,
}

pub const MINUS: &str = "\u{2212}";

const UNITS: [(f64, &str); 5] = [(1e15, "Q"), (1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];

fn parts(sign: &'static str, body: String) -> NumParts {
    NumParts { sign, body, dim: String::new(), tone: None }
}

/// 1,000 and up as at most four digits and a unit, never padded with zeros: 1.98M, 10.91M, 247.3M,
/// 123M, 6.3K, 81K. `wide_k` keeps one decimal on hundreds of thousands (counts: 317.3K); money
/// drops it ($247K).
pub(crate) fn compact_unit(abs: f64, wide_k: bool) -> String {
    for (i, &(size, unit)) in UNITS.iter().enumerate() {
        if abs < size {
            continue;
        }
        let n = abs / size;
        let decimals = if unit == "K" {
            if n < 100.0 || wide_k {
                1
            } else {
                0
            }
        } else if n < 100.0 {
            2
        } else {
            1
        };
        let text = to_fixed(n, decimals);
        // 999.96K rounds up to 1000K: say 1M instead.
        if js_number(&text) >= 1000.0 && i > 0 {
            return format!("1{}", UNITS[i - 1].1);
        }
        return format!("{}{unit}", trim_zeros(&text));
    }
    String::new()
}

/// A market figure without its currency sign.
pub fn compact_figure(abs: f64) -> String {
    if abs >= 999.5 {
        return compact_unit(abs.max(1000.0), false);
    }
    if abs >= 100.0 {
        return to_fixed(abs, 0);
    }
    if abs >= 1.0 || abs == 0.0 {
        return if abs == 0.0 { "0".into() } else { to_fixed(abs, 2) };
    }
    if abs >= 0.01 {
        return to_fixed(abs, 2);
    }
    js_string(js_number(&to_precision(abs, 2)))
}

fn full_money(abs: f64, cents: bool) -> (String, String) {
    if !cents {
        return (format!("${}", locale(js_round(abs), Locale::default())), String::new());
    }
    if abs > 0.0 && abs < 0.005 {
        return ("<$0".into(), ".01".into());
    }
    let s = locale(abs, Locale::frac(2, 2));
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    (format!("${whole}"), format!(".{frac}"))
}

fn money_parts(value: f64, signed: bool, o: NumOptions) -> NumParts {
    let abs = value.abs();
    let (body, dim) = if o.compact { (format!("${}", compact_figure(abs)), String::new()) } else { full_money(abs, o.cents) };
    // What is shown decides the sign: −$0.001 reads as $0.00, not −$0.00.
    let zero = if !o.compact && o.cents { abs < 0.005 } else { abs == 0.0 };
    let sign = if value < 0.0 && !zero {
        MINUS
    } else if signed && !zero {
        "+"
    } else {
        ""
    };
    let tone = if !signed {
        None
    } else if zero {
        Some(NumTone::Flat)
    } else if value > 0.0 {
        Some(NumTone::Up)
    } else {
        Some(NumTone::Down)
    };
    // A balance of dust says "<$0.01"; a result of dust is simply flat.
    if zero && (signed || abs == 0.0) && !o.compact && o.cents {
        return NumParts { sign: "", body: "$0".into(), dim: ".00".into(), tone };
    }
    NumParts { sign, body, dim, tone }
}

fn change_body(abs: f64) -> String {
    if abs < 100.0 {
        to_fixed(abs, 2)
    } else if abs < 1000.0 {
        to_fixed(abs, 1)
    } else if abs < 100_000.0 {
        locale(js_round(abs), Locale::default())
    } else {
        compact_unit(abs, false)
    }
}

fn change_parts(value: f64) -> NumParts {
    let body = change_body(value.abs());
    let zero = js_number(&body) == 0.0;
    NumParts {
        sign: if zero {
            ""
        } else if value > 0.0 {
            "+"
        } else {
            MINUS
        },
        body: format!("{body}%"),
        dim: String::new(),
        tone: Some(if zero {
            NumTone::Flat
        } else if value > 0.0 {
            NumTone::Up
        } else {
            NumTone::Down
        }),
    }
}

fn count_body(abs: f64) -> String {
    if abs < 999.5 {
        js_string(js_round(abs))
    } else {
        compact_unit(abs.max(1000.0), true)
    }
}

fn amount_body(abs: f64) -> String {
    if abs >= 1000.0 {
        compact_unit(abs, false)
    } else if abs >= 1.0 {
        trim_zeros(&to_fixed(abs, 2))
    } else if abs == 0.0 {
        "0".into()
    } else {
        js_string(js_number(&to_precision(abs, 4)))
    }
}

/// "12.50" -> "12.5", "3.00" -> "3"; text without a point is left alone.
pub(crate) fn trim_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    let t = text.trim_end_matches('0');
    t.strip_suffix('.').unwrap_or(t).to_string()
}

/// The pieces of a formatted number, or None when the value is unknown.
pub fn num_parts(kind: NumKind, value: Option<f64>, o: NumOptions) -> Option<NumParts> {
    let value = value.filter(|v| v.is_finite())?;
    let abs = value.abs();
    let minus = if value < 0.0 { MINUS } else { "" };
    Some(match kind {
        NumKind::Money => money_parts(value, false, o),
        NumKind::Pnl => money_parts(value, true, o),
        NumKind::Market => parts(minus, format!("${}", compact_figure(abs))),
        // A price of zero or less is not a price we know.
        NumKind::Price if value <= 0.0 => return None,
        NumKind::Price => parts("", usd_price(Some(value))),
        NumKind::Change => change_parts(value),
        NumKind::Count => parts(minus, count_body(abs)),
        NumKind::Share => parts(minus, format!("{}%", to_fixed(abs, o.decimals as usize))),
        NumKind::Amount => parts(minus, amount_body(abs)),
    })
}

/// The same number as plain text ("" when unknown): for titles, labels and sentences.
pub fn num_text(kind: NumKind, value: Option<f64>, o: NumOptions) -> String {
    num_parts(kind, value, o).map(|p| format!("{}{}{}", p.sign, p.body, p.dim)).unwrap_or_default()
}

/// [`num_text`] with default options for a known value.
pub fn num(kind: NumKind, value: f64) -> String {
    num_text(kind, Some(value), NumOptions::default())
}

/// Which characters of `next` differ from `prev`, compared from the right so the cents line up
/// when a digit is added on the left. Used to roll only the digits that moved.
pub fn changed_chars(prev: &str, next: &str) -> Vec<bool> {
    let p: Vec<char> = prev.chars().collect();
    let n: Vec<char> = next.chars().collect();
    (0..n.len())
        .map(|i| {
            let from_end = n.len() - 1 - i;
            let before = if from_end < p.len() { Some(p[p.len() - 1 - from_end]) } else { None };
            before != Some(n[i])
        })
        .collect()
}

const SUB: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];

fn subscript(n: i64) -> String {
    n.to_string().chars().map(|d| SUB[d.to_digit(10).unwrap_or(0) as usize]).collect()
}

/// the Trade API's price label: 3 significant digits, subscript zeros for tiny prices. Prices span twelve
/// orders of magnitude on a memecoin terminal, so $0.0₅4123 means five zeros after the point.
pub fn usd_price(value: Option<f64>) -> String {
    let Some(value) = value.filter(|v| v.is_finite() && *v > 0.0) else { return String::new() };
    if value >= 1000.0 {
        return format!("${}", locale(value, Locale::frac(2, 2)));
    }
    if value >= 1.0 {
        return format!("${}", to_fixed(value, if value >= 100.0 { 2 } else { 3 }));
    }
    if value >= 0.001 {
        return format!("${}", to_precision(value, 3));
    }
    let zeros = (-value.log10()).ceil() as i64 - 1;
    let digits = js_round(value * 10f64.powi((zeros + 4) as i32));
    let d = js_string(digits);
    let d = d.trim_end_matches('0');
    format!("$0.0{}{}", subscript(zeros), if d.is_empty() { "0" } else { d })
}

#[cfg(test)]
mod tests {
    //! num.test.ts
    use super::*;

    fn np(kind: NumKind, v: f64) -> Option<NumParts> {
        num_parts(kind, Some(v), NumOptions::default())
    }
    fn t(kind: NumKind, v: f64) -> String {
        num(kind, v)
    }
    fn p(sign: &'static str, body: &str, dim: &str, tone: Option<NumTone>) -> Option<NumParts> {
        Some(NumParts { sign, body: body.into(), dim: dim.into(), tone })
    }
    use NumKind::*;

    #[test]
    fn money_shows_full_cents_and_splits_them_off_for_dimming() {
        assert_eq!(np(Money, 4151.49), p("", "$4,151", ".49", None));
        assert_eq!(t(Money, 9366.96), "$9,366.96");
        assert_eq!(t(Money, 0.0), "$0.00");
    }

    #[test]
    fn money_uses_a_real_minus_and_no_plus() {
        assert_eq!(t(Money, -12.0), "−$12.00");
        assert_eq!(t(Money, 12.0), "$12.00");
    }

    #[test]
    fn money_never_rounds_a_dust_balance_to_a_lie() {
        assert_eq!(t(Money, 0.004), "<$0.01");
        assert_eq!(t(Money, 0.006), "$0.01");
    }

    #[test]
    fn money_can_drop_cents_or_go_compact_when_asked() {
        assert_eq!(num_text(Money, Some(412.6), NumOptions::no_cents()), "$413");
        assert_eq!(num_text(Money, Some(35_550.12), NumOptions::compact()), "$35.6K");
    }

    #[test]
    fn pnl_is_always_signed_and_toned() {
        assert_eq!(np(Pnl, 4151.49), p("+", "$4,151", ".49", Some(NumTone::Up)));
        assert_eq!(np(Pnl, -12.0), p("−", "$12", ".00", Some(NumTone::Down)));
    }

    #[test]
    fn pnl_is_flat_and_unsigned_at_zero_including_values_that_show_as_zero() {
        assert_eq!(np(Pnl, 0.0), p("", "$0", ".00", Some(NumTone::Flat)));
        assert_eq!(t(Pnl, -0.001), "$0.00");
        assert_eq!(np(Pnl, 0.001).unwrap().tone, Some(NumTone::Flat));
    }

    #[test]
    fn pnl_supports_headline_and_compact_forms() {
        assert_eq!(num_text(Pnl, Some(412.6), NumOptions::no_cents()), "+$413");
        assert_eq!(num_text(Pnl, Some(-35_600.0), NumOptions::compact()), "−$35.6K");
    }

    #[test]
    fn market_is_compact_with_at_most_four_digits() {
        assert_eq!(t(Market, 10_910_000.0), "$10.91M");
        assert_eq!(t(Market, 247_300.0), "$247K");
        assert_eq!(t(Market, 54_110_000.0), "$54.11M");
        assert_eq!(t(Market, 1_980_000.0), "$1.98M");
        assert_eq!(t(Market, 247_300_000.0), "$247.3M");
        assert_eq!(t(Market, 5_010_000_000.0), "$5.01B");
    }

    #[test]
    fn market_never_pads_with_zeros() {
        assert_eq!(t(Market, 81_000.0), "$81K");
        assert_eq!(t(Market, 6_300.0), "$6.3K");
        assert_eq!(t(Market, 123_000_000.0), "$123M");
        assert_eq!(t(Market, 94_050_000.0), "$94.05M");
        assert_eq!(t(Market, 973_000.0), "$973K");
        assert_eq!(t(Market, 100_000.0), "$100K");
        assert_eq!(t(Market, 500_000_000.0), "$500M");
        assert_eq!(t(Count, 10_000.0), "10K");
        assert_eq!(t(Market, 1_500_000.0), "$1.5M");
        assert_eq!(t(Market, 16_000.0), "$16K");
    }

    #[test]
    fn market_handles_small_figures_and_unit_edges() {
        assert_eq!(t(Market, 812.0), "$812");
        assert_eq!(t(Market, 1.11), "$1.11");
        assert_eq!(t(Market, 0.0), "$0");
        assert_eq!(t(Market, 0.0042), "$0.0042");
        assert_eq!(t(Market, 999_960.0), "$1M");
        assert_eq!(t(Market, 999.6), "$1K");
    }

    #[test]
    fn price_uses_three_significant_digits() {
        assert_eq!(t(Price, 0.0109), "$0.0109");
        assert_eq!(t(Price, 0.223), "$0.223");
        assert_eq!(t(Price, 0.00931), "$0.00931");
    }

    #[test]
    fn price_uses_subscript_zeros_under_0_001() {
        assert_eq!(t(Price, 0.0000041234), "$0.0₅4123");
    }

    #[test]
    fn price_keeps_large_prices_whole() {
        assert_eq!(t(Price, 4104.58), "$4,104.58");
    }

    #[test]
    fn price_has_nothing_to_show_for_zero() {
        assert_eq!(np(Price, 0.0), None);
    }

    #[test]
    fn change_always_carries_a_sign_and_a_tone() {
        assert_eq!(np(Change, 4.15), p("+", "4.15%", "", Some(NumTone::Up)));
        assert_eq!(np(Change, -9.456), p("−", "9.46%", "", Some(NumTone::Down)));
    }

    #[test]
    fn change_uses_fewer_decimals_from_100_up() {
        assert_eq!(t(Change, 255.55), "+255.6%");
        assert_eq!(t(Change, 4321.4), "+4,321%");
        assert_eq!(t(Change, 240_016_237.0), "+240M%");
    }

    #[test]
    fn change_is_flat_when_it_shows_as_zero() {
        assert_eq!(np(Change, 0.0), p("", "0.00%", "", Some(NumTone::Flat)));
        assert_eq!(t(Change, -0.001), "0.00%");
    }

    #[test]
    fn count_is_exact_under_1000_and_compact_after() {
        assert_eq!(t(Count, 901.0), "901");
        assert_eq!(t(Count, 23_400.0), "23.4K");
        assert_eq!(t(Count, 317_300.0), "317.3K");
        assert_eq!(t(Count, 1_500.0), "1.5K");
        assert_eq!(t(Count, 2_500_000.0), "2.5M");
    }

    #[test]
    fn share_has_one_decimal_in_tables_and_none_in_sentences() {
        assert_eq!(t(Share, 4.56), "4.6%");
        assert_eq!(num_text(Share, Some(36.2), NumOptions::decimals(0)), "36%");
    }

    #[test]
    fn amount_formats_coin_quantities() {
        assert_eq!(t(Amount, 12.5), "12.5");
        assert_eq!(t(Amount, 3.0), "3");
        assert_eq!(t(Amount, 1_250_000.0), "1.25M");
        assert_eq!(t(Amount, 0.000123456), "0.0001235");
    }

    #[test]
    fn unknown_values_have_no_parts_and_no_text_never_dashes() {
        for kind in KINDS {
            assert_eq!(num_parts(kind, None, NumOptions::default()), None);
            assert_eq!(num_parts(kind, Some(f64::NAN), NumOptions::default()), None);
            assert_eq!(num_parts(kind, Some(f64::INFINITY), NumOptions::default()), None);
            assert_eq!(num_text(kind, None, NumOptions::default()), "");
        }
    }

    #[test]
    fn compact_figure_has_no_currency_sign() {
        assert_eq!(compact_figure(10_910_000.0), "10.91M");
    }

    #[test]
    fn changed_chars_marks_only_the_digits_that_moved_aligned_from_the_right() {
        let mut want = vec![false; 9];
        want[8] = true;
        assert_eq!(changed_chars("$1,234.56", "$1,234.57"), want);
        assert_eq!(changed_chars("$999.00", "$1,000.00").iter().filter(|b| **b).count(), 6);
    }

    #[test]
    fn changed_chars_marks_everything_when_there_was_nothing_before() {
        assert!(changed_chars("", "$1").iter().all(|b| *b));
    }
}
