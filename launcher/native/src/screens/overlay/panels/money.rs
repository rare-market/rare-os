//! SUI amounts and typed prices (marketplace/format.ts fmtSui, shortName, venueName; overlay/price.ts
//! parseSuiInput). Money is MIST (1e-9 SUI) in i128, so a negative net never wraps.

pub const MIST: i128 = 1_000_000_000;

pub const DASH: &str = "--";

#[derive(Clone, Copy, Debug)]
pub struct Fmt {
    pub dp: usize,
    pub compact: bool,
    pub trim: bool,
}

impl Fmt {
    pub const fn dp(dp: usize) -> Fmt {
        Fmt { dp, compact: false, trim: false }
    }
    pub const fn trim(dp: usize) -> Fmt {
        Fmt { dp, compact: false, trim: true }
    }
    pub const fn compact(dp: usize) -> Fmt {
        Fmt { dp, compact: true, trim: true }
    }
}

fn trim_zeros(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

fn group(whole: u128) -> String {
    let w = whole.to_string();
    let mut out = String::new();
    for (i, c) in w.chars().enumerate() {
        if i > 0 && (w.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// fmtSui: `dp` fixed decimals, `trim` drops trailing zeros, `compact` abbreviates ("16.571K").
/// Unknown is "--".
pub fn fmt_sui(v: Option<i128>, o: Fmt) -> String {
    let Some(m) = v else { return DASH.into() };
    let neg = if m < 0 { "-" } else { "" };
    let a = m.unsigned_abs();
    let n = a as f64 / 1e9;
    if o.compact && n >= 1_000_000.0 {
        return format!("{neg}{}M", trim_zeros(format!("{:.2}", n / 1_000_000.0)));
    }
    if o.compact && n >= 10_000.0 {
        return format!("{neg}{}K", trim_zeros(format!("{:.3}", n / 1_000.0)));
    }
    let min = if o.trim {
        0
    } else if a != 0 && n < 1000.0 {
        o.dp
    } else {
        o.dp.min(2)
    };
    let max = if n >= 1000.0 { o.dp.min(2) } else { o.dp }.min(9);
    let scale = 10u128.pow((9 - max) as u32);
    let r = (a + scale / 2) / scale;
    let p = 10u128.pow(max as u32);
    let (whole, frac) = (r / p, r % p);
    let mut f = if max == 0 { String::new() } else { format!("{frac:0max$}") };
    while f.len() > min && f.ends_with('0') {
        f.pop();
    }
    let dot = if f.is_empty() { String::new() } else { format!(".{f}") };
    format!("{neg}{}{dot}", group(whole))
}

/// `fmtSui(v, { dp: 3, trim: true })`, the Amount component's number.
pub fn amount(mist: i128) -> String {
    fmt_sui(Some(mist), Fmt::trim(3))
}

/// "12", "12.5", "0.25", "1,200" to MIST; None for anything else or zero.
pub fn parse_sui_input(text: &str) -> Option<i128> {
    let mut t: String = text.trim().replace(',', "");
    let lower = t.to_ascii_lowercase();
    if lower.ends_with("sui") {
        t.truncate(t.len() - 3);
        t = t.trim_end().to_string();
    }
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (t.as_str(), None),
    };
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if !digits(whole) || frac.is_some_and(|f| !digits(f) || f.len() > 9) {
        return None;
    }
    if whole.is_empty() && frac.unwrap_or("").is_empty() {
        return None;
    }
    let w: i128 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let f: i128 = format!("{:0<9}", frac.unwrap_or("")).parse().ok()?;
    let v = w.checked_mul(MIST)? + f;
    (v > 0).then_some(v)
}

/// "#7409" from "Collection Name #7409".
pub fn short_name<'a>(name: &'a str, collection: Option<&str>) -> &'a str {
    match collection {
        Some(c) if !c.is_empty() => name.strip_prefix(&format!("{c} ")).unwrap_or(name),
        _ => name,
    }
}

pub fn venue_name(v: &str) -> String {
    match v {
        "market" => "Rare".into(),
        "tradeport" => "Tradeport".into(),
        "drip" => "Drip".into(),
        "kiosk" => "Kiosk".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i128 = MIST;

    #[test]
    fn reads_sui_amounts() {
        assert_eq!(parse_sui_input("12"), Some(12 * S));
        assert_eq!(parse_sui_input("12.5"), Some(12_500_000_000));
        assert_eq!(parse_sui_input(".25"), Some(250_000_000));
        assert_eq!(parse_sui_input("1,200 SUI"), Some(1_200 * S));
        assert_eq!(parse_sui_input("0.000000001"), Some(1));
        assert_eq!(parse_sui_input("0"), None);
        assert_eq!(parse_sui_input("1.0000000001"), None);
        assert_eq!(parse_sui_input("abc"), None);
        assert_eq!(parse_sui_input(""), None);
        assert_eq!(parse_sui_input("."), None);
    }

    #[test]
    fn formats_like_fmt_sui() {
        assert_eq!(fmt_sui(Some(1_500_000_000), Fmt::dp(3)), "1.500");
        assert_eq!(fmt_sui(Some(1_500_000_000), Fmt::trim(3)), "1.5");
        assert_eq!(fmt_sui(Some(0), Fmt::dp(3)), "0.00");
        assert_eq!(fmt_sui(Some(0), Fmt::trim(3)), "0");
        assert_eq!(fmt_sui(Some(1_234_567_800_000), Fmt::dp(3)), "1,234.57");
        assert_eq!(fmt_sui(Some(125_000_000), Fmt::dp(2)), "0.13");
        assert_eq!(fmt_sui(Some(16_571_000_000_000), Fmt::compact(2)), "16.571K");
        assert_eq!(fmt_sui(Some(2_500_000_000_000_000), Fmt::compact(2)), "2.5M");
        assert_eq!(fmt_sui(Some(4_200_000_000), Fmt::compact(2)), "4.2");
        assert_eq!(fmt_sui(None, Fmt::dp(3)), "--");
        assert_eq!(fmt_sui(Some(-920_000), Fmt::trim(3)), "-0.001");
        assert_eq!(amount(111_000_000), "0.111");
    }

    #[test]
    fn short_names_drop_the_collection() {
        assert_eq!(short_name("RetroSui #1550", Some("RetroSui")), "#1550");
        assert_eq!(short_name("Lone", Some("RetroSui")), "Lone");
        assert_eq!(short_name("Lone", None), "Lone");
        assert_eq!(venue_name("market"), "Rare");
    }
}
