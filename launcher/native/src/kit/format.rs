//! Number rules, a port of launcher/src/ui/format.ts (the Trade API's) and the rare.market helpers in
//! launcher/src/marketplace/format.ts. An unknown value has no parts: the caller leaves it out.
//!
//!   money   your money: full, cents dimmed            $4,151.49
//!   pnl     profit and loss: money, always signed     +$4,151.49
//!   market  market figures: compact, 4 digits at most $10.91M, $247K
//!   price   3 significant digits, subscript zeros     $0.0109, $0.0₅6093
//!   change  sign and colour, 2 decimals under 100%    +4.15%, −9.46%
//!   count   exact under 1,000, then compact           901, 23.4K
//!   share   whole percent or 1 decimal                36%, 4.6%
//!   amount  a coin quantity                           12.5, 1.25M

pub const MINUS: &str = "\u{2212}";
pub const DASH: &str = "--";

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumTone {
    Up,
    Down,
    Flat,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NumOptions {
    /// money, pnl: false drops the cents.
    pub cents: Option<bool>,
    pub compact: bool,
    /// share: decimals (default 1).
    pub decimals: Option<u8>,
    /// Num `live`: changed digits roll and the background flashes when the value moves.
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumParts {
    pub sign: &'static str,
    pub body: String,
    /// Cents, shown dimmed.
    pub dim: String,
    pub tone: Option<NumTone>,
}

impl NumParts {
    pub fn text(&self) -> String {
        format!("{}{}{}", self.sign, self.body, self.dim)
    }
}

const UNITS: [(f64, &str); 5] = [(1e15, "Q"), (1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];

fn trim_zeros(t: &str) -> String {
    if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        t.to_string()
    }
}

fn fixed(v: f64, d: usize) -> String {
    format!("{v:.d$}")
}

fn compact_unit(abs: f64, wide_k: bool) -> String {
    for (i, (size, unit)) in UNITS.iter().enumerate() {
        if abs < *size {
            continue;
        }
        let n = abs / size;
        let decimals = if *unit == "K" {
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
        let text = fixed(n, decimals);
        if text.parse::<f64>().unwrap_or(0.0) >= 1000.0 && i > 0 {
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
        return fixed(abs, 0);
    }
    if abs >= 1.0 || abs == 0.0 {
        return if abs == 0.0 { "0".into() } else { fixed(abs, 2) };
    }
    if abs >= 0.01 {
        return fixed(abs, 2);
    }
    js_num(precision(abs, 2))
}

fn precision(v: f64, p: i32) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let e = v.abs().log10().floor() as i32;
    let k = 10f64.powi(p - 1 - e);
    (v * k).round() / k
}

/// Number's default string form for the values we print (no exponent below 1e21).
fn js_num(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('e') {
        format!("{v:.12}").trim_end_matches('0').to_string()
    } else {
        s
    }
}

/// 1234567.891 → "1,234,567.89" with `min..=max` fraction digits (toLocaleString en-US).
pub fn grouped(v: f64, min: usize, max: usize) -> String {
    let s = fixed(v.abs(), max);
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (s, String::new()),
    };
    let mut frac = frac;
    while frac.len() > min && frac.ends_with('0') {
        frac.pop();
    }
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&frac);
    }
    if v < 0.0 && out.chars().any(|c| c != '0' && c != '.' && c != ',') {
        format!("-{out}")
    } else {
        out
    }
}

fn full_money(abs: f64, cents: bool) -> (String, String) {
    if !cents {
        return (format!("${}", grouped(abs.round(), 0, 0)), String::new());
    }
    if abs > 0.0 && abs < 0.005 {
        return ("<$0".into(), ".01".into());
    }
    let g = grouped(abs, 2, 2);
    let (w, f) = g.split_once('.').unwrap_or((&g, "00"));
    (format!("${w}"), format!(".{f}"))
}

fn money_parts(value: f64, signed: bool, o: NumOptions) -> NumParts {
    let abs = value.abs();
    let cents = o.cents != Some(false);
    let (body, dim) = if o.compact { (format!("${}", compact_figure(abs)), String::new()) } else { full_money(abs, cents) };
    let zero = if !o.compact && cents { abs < 0.005 } else { abs == 0.0 };
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
    if zero && (signed || abs == 0.0) && !o.compact && cents {
        return NumParts { sign: "", body: "$0".into(), dim: ".00".into(), tone };
    }
    NumParts { sign, body, dim, tone }
}

fn change_body(abs: f64) -> String {
    if abs < 100.0 {
        fixed(abs, 2)
    } else if abs < 1000.0 {
        fixed(abs, 1)
    } else if abs < 100_000.0 {
        grouped(abs.round(), 0, 0)
    } else {
        compact_unit(abs, false)
    }
}

fn count_body(abs: f64) -> String {
    if abs < 999.5 {
        format!("{}", abs.round() as i64)
    } else {
        compact_unit(abs.max(1000.0), true)
    }
}

fn amount_body(abs: f64) -> String {
    if abs >= 1000.0 {
        compact_unit(abs, false)
    } else if abs >= 1.0 {
        trim_zeros(&fixed(abs, 2))
    } else if abs == 0.0 {
        "0".into()
    } else {
        js_num(precision(abs, 4))
    }
}

/// The pieces of a formatted number, or None when the value is unknown.
pub fn num_parts(kind: NumKind, value: Option<f64>, o: NumOptions) -> Option<NumParts> {
    let value = value.filter(|v| v.is_finite())?;
    let abs = value.abs();
    let minus = if value < 0.0 { MINUS } else { "" };
    Some(match kind {
        NumKind::Money => money_parts(value, false, o),
        NumKind::Pnl => money_parts(value, true, o),
        NumKind::Market => NumParts { sign: minus, body: format!("${}", compact_figure(abs)), dim: String::new(), tone: None },
        NumKind::Price => {
            if value <= 0.0 {
                return None;
            }
            NumParts { sign: "", body: usd_price(value), dim: String::new(), tone: None }
        }
        NumKind::Change => {
            let body = change_body(abs);
            let zero = body.replace(',', "").parse::<f64>().unwrap_or(1.0) == 0.0;
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
        NumKind::Count => NumParts { sign: minus, body: count_body(abs), dim: String::new(), tone: None },
        NumKind::Share => NumParts { sign: minus, body: format!("{}%", fixed(abs, o.decimals.unwrap_or(1) as usize)), dim: String::new(), tone: None },
        NumKind::Amount => NumParts { sign: minus, body: amount_body(abs), dim: String::new(), tone: None },
    })
}

/// The same number as plain text ("" when unknown).
pub fn num_text(kind: NumKind, value: Option<f64>, o: NumOptions) -> String {
    num_parts(kind, value, o).map(|p| p.text()).unwrap_or_default()
}

/// Which characters of `next` differ from `prev`, compared from the right (rolled digits).
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

/// the Trade API's price label: 3 significant digits, subscript zeros for tiny prices.
pub fn usd_price(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return String::new();
    }
    if value >= 1000.0 {
        return format!("${}", grouped(value, 2, 2));
    }
    if value >= 1.0 {
        return format!("${}", fixed(value, if value >= 100.0 { 2 } else { 3 }));
    }
    if value >= 0.001 {
        // toPrecision(3)
        let e = value.log10().floor() as i32;
        let d = (2 - e).max(0) as usize;
        return format!("${}", fixed(value, d));
    }
    let zeros = (-value.log10()).ceil() as i32 - 1;
    let digits = (value * 10f64.powi(zeros + 4)).round() as i64;
    let ds = digits.to_string();
    let ds = ds.trim_end_matches('0');
    format!("$0.0{}{}", subscript(zeros as u32), if ds.is_empty() { "0" } else { ds })
}

fn subscript(n: u32) -> String {
    const SUB: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
    n.to_string().chars().map(|d| SUB[d.to_digit(10).unwrap_or(0) as usize]).collect()
}

pub const MIST_PER_SUI: u128 = 1_000_000_000;

pub fn mist_to_sui(mist: Option<u128>) -> Option<f64> {
    let b = mist?;
    Some((b / MIST_PER_SUI) as f64 + (b % MIST_PER_SUI) as f64 / 1e9)
}

/// SUI amount (fmtSui): `dp` fixed decimals, `compact` abbreviates, `trim` drops trailing zeros.
pub fn fmt_sui(mist: Option<u128>, dp: usize, compact: bool, trim: bool) -> String {
    let Some(n) = mist_to_sui(mist) else { return DASH.into() };
    if compact && n.abs() >= 1_000_000.0 {
        return format!("{}M", trim_zeros(&fixed(n / 1_000_000.0, 2)));
    }
    if compact && n.abs() >= 10_000.0 {
        return format!("{}K", trim_zeros(&fixed(n / 1_000.0, 3)));
    }
    let min = if trim {
        0
    } else if n != 0.0 && n.abs() < 1000.0 {
        dp
    } else {
        dp.min(2)
    };
    let max = if n.abs() >= 1000.0 { dp.min(2) } else { dp };
    grouped(n, min, max)
}

pub fn fmt_int(v: Option<f64>) -> String {
    match v.filter(|v| v.is_finite()) {
        Some(v) => grouped(v.round(), 0, 0),
        None => DASH.into(),
    }
}

pub fn fmt_pct(v: Option<f64>, dp: usize) -> String {
    match v.filter(|v| v.is_finite()) {
        Some(v) => format!("{}%", fixed(v.abs(), dp)),
        None => DASH.into(),
    }
}

/// Terse relative time: "20s", "2h", "1d", "12mo", "2y".
pub fn time_ago(t_ms: Option<f64>, now_ms: f64) -> String {
    let Some(t) = t_ms.filter(|t| t.is_finite()) else { return DASH.into() };
    let s = ((now_ms - t) / 1000.0).round().max(0.0) as i64;
    if s < 60 {
        return format!("{s}s");
    }
    let m = s / 60;
    if m < 60 {
        return format!("{m}m");
    }
    let h = m / 60;
    if h < 24 {
        return format!("{h}h");
    }
    let d = h / 24;
    if d < 30 {
        return format!("{d}d");
    }
    let mo = d / 30;
    if mo < 12 {
        return format!("{mo}mo");
    }
    format!("{}y", d / 365)
}

/// "8 GB", "512 MB" (components/ui.tsx fmtSize).
pub fn fmt_size(b: Option<f64>) -> String {
    match b.filter(|b| *b > 0.0) {
        None => String::new(),
        Some(b) if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        Some(b) => format!("{} MB", ((b / 1e6).round() as i64).max(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(kind: NumKind, v: f64) -> String {
        num_text(kind, Some(v), NumOptions::default())
    }

    #[test]
    fn money_and_pnl() {
        assert_eq!(t(NumKind::Money, 4151.49), "$4,151.49");
        let p = num_parts(NumKind::Money, Some(1284.07), NumOptions::default()).unwrap();
        assert_eq!((p.body.as_str(), p.dim.as_str()), ("$1,284", ".07"));
        assert_eq!(t(NumKind::Pnl, 12.4), "+$12.40");
        assert_eq!(t(NumKind::Pnl, -12.4), "\u{2212}$12.40");
        assert_eq!(t(NumKind::Pnl, -0.001), "$0.00");
        assert_eq!(t(NumKind::Money, 0.001), "<$0.01");
        assert_eq!(num_text(NumKind::Money, Some(412.4), NumOptions { cents: Some(false), ..Default::default() }), "$412");
    }

    #[test]
    fn market_and_count() {
        assert_eq!(t(NumKind::Market, 10_910_000.0), "$10.91M");
        assert_eq!(t(NumKind::Market, 247_000.0), "$247K");
        assert_eq!(t(NumKind::Market, 6_300.0), "$6.3K");
        assert_eq!(t(NumKind::Market, 999_960.0), "$1M");
        assert_eq!(t(NumKind::Count, 901.0), "901");
        assert_eq!(t(NumKind::Count, 23_400.0), "23.4K");
        assert_eq!(t(NumKind::Count, 317_300.0), "317.3K");
    }

    #[test]
    fn prices() {
        assert_eq!(usd_price(0.906), "$0.906");
        assert_eq!(usd_price(0.0109), "$0.0109");
        assert_eq!(usd_price(0.000_006_093), "$0.0₅6093");
        assert_eq!(usd_price(3.4567), "$3.457");
        assert_eq!(usd_price(123.456), "$123.46");
        assert_eq!(usd_price(1234.5), "$1,234.50");
        assert!(num_parts(NumKind::Price, Some(0.0), NumOptions::default()).is_none());
    }

    #[test]
    fn change() {
        assert_eq!(t(NumKind::Change, 4.15), "+4.15%");
        assert_eq!(t(NumKind::Change, -9.46), "\u{2212}9.46%");
        assert_eq!(t(NumKind::Change, 0.001), "0.00%");
        assert_eq!(num_parts(NumKind::Change, Some(-0.76), NumOptions::default()).unwrap().tone, Some(NumTone::Down));
    }

    #[test]
    fn sui_amounts() {
        assert_eq!(fmt_sui(Some(98_600_000_000), 2, false, true), "98.6");
        assert_eq!(fmt_sui(Some(1_500_000_000), 3, false, false), "1.500");
        assert_eq!(fmt_sui(Some(16_571_000_000_000), 3, true, false), "16.571K");
        assert_eq!(fmt_sui(None, 2, false, false), "--");
    }

    #[test]
    fn rolled_digits() {
        assert_eq!(changed_chars("$0.906", "$0.907"), vec![false, false, false, false, false, true]);
        assert_eq!(changed_chars("$9.99", "$10.01"), vec![true, true, true, false, true, true]);
    }

    #[test]
    fn misc() {
        assert_eq!(time_ago(Some(0.0), 7_200_000.0), "2h");
        assert_eq!(fmt_size(Some(9.8e9)), "9.8 GB");
        assert_eq!(fmt_size(Some(2e8)), "200 MB");
        assert_eq!(grouped(1234567.891, 2, 2), "1,234,567.89");
    }
}
