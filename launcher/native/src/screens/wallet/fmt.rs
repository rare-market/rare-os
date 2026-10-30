//! The number and id formatting the wallet screens show, ported from launcher/src/wallet/tx/amount.ts
//! (formatUnits, formatAmount, usd, price), review/packages.ts (shortId, normId) and the kit's Addr.
//! Base units are integers end to end; only USD display goes through floats, as in TS.

/// `normalizeSuiAddress`: lower case, `0x`, padded to 64 hex digits.
pub fn norm_id(id: &str) -> String {
    let s = id.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s).to_ascii_lowercase();
    format!("0x{:0>64}", s)
}

/// `shortId`: `0x1eab…b2fb`; framework ids stay `0x2`.
pub fn short_id(id: &str) -> String {
    let n = norm_id(id);
    let trimmed = format!("0x{}", n[2..].trim_start_matches('0'));
    if trimmed.chars().count() <= 6 {
        return if trimmed == "0x" { "0x0".into() } else { trimmed };
    }
    format!("{}…{}", &n[..6], &n[n.len() - 4..])
}

/// The kit's `Addr`: `0x1234...abcd` (three ASCII dots, as the web shows).
pub fn addr_short(a: &str) -> String {
    let c: Vec<char> = a.chars().collect();
    if c.len() <= 10 {
        // slice(0, 6) + "..." + slice(-4) on a short string repeats characters; keep the JS result.
        let head: String = c.iter().take(6).collect();
        let tail: String = c[c.len().saturating_sub(4)..].iter().collect();
        return format!("{head}...{tail}");
    }
    let head: String = c[..6].iter().collect();
    let tail: String = c[c.len() - 4..].iter().collect();
    format!("{head}...{tail}")
}

/// `normalizeStructTag`: every address in the tag padded, type parameters joined by ",". A tag
/// that does not parse comes back as it was (`normType`'s catch).
pub fn norm_type(t: &str) -> String {
    fn parse(s: &str) -> Option<String> {
        let s = s.trim();
        if !s.contains("::") {
            return Some(s.to_string());
        }
        let (base, params) = match s.find('<') {
            Some(i) => {
                let inner = s[i + 1..].strip_suffix('>')?;
                (&s[..i], Some(inner))
            }
            None => (s, None),
        };
        let parts: Vec<&str> = base.split("::").collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        let mut out = format!("{}::{}::{}", norm_id(parts[0]), parts[1], parts[2]);
        if let Some(inner) = params {
            let mut depth = 0i32;
            let mut start = 0;
            let mut items = Vec::new();
            for (i, ch) in inner.char_indices() {
                match ch {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    ',' if depth == 0 => {
                        items.push(&inner[start..i]);
                        start = i + 1;
                    }
                    _ => {}
                }
            }
            items.push(&inner[start..]);
            let norm: Option<Vec<String>> = items.iter().map(|p| parse(p)).collect();
            out.push('<');
            out.push_str(&norm?.join(","));
            out.push('>');
        }
        Some(out)
    }
    if t.starts_with("vector<") {
        return t.to_string();
    }
    parse(t).unwrap_or_else(|| t.to_string())
}

pub fn sui_type() -> String {
    norm_type("0x2::sui::SUI")
}

fn group3(whole: &str) -> String {
    let b = whole.as_bytes();
    let mut out = String::with_capacity(whole.len() + whole.len() / 3);
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

/// `formatUnits`: base units to "1,234.5678", at most `max_frac` fraction digits, trailing zeros dropped.
pub fn format_units(base: i128, decimals: u32, max_frac: usize) -> String {
    let neg = base < 0;
    let abs = base.unsigned_abs().to_string();
    let d = decimals as usize;
    let s = format!("{:0>width$}", abs, width = d + 1);
    let (whole, frac) = s.split_at(s.len() - d);
    let frac: String = frac.chars().take(max_frac).collect();
    let frac = frac.trim_end_matches('0');
    format!("{}{}{}", if neg { "-" } else { "" }, group3(whole), if frac.is_empty() { String::new() } else { format!(".{frac}") })
}

/// `formatAmount`: 4 fraction digits for whole amounts, more for small ones so a fee like 0.00199
/// SUI does not read as 0.0019 or 0.
pub fn format_amount(base: i128, decimals: u32) -> String {
    let abs = base.unsigned_abs();
    let unit = 10u128.pow(decimals);
    if abs == 0 || abs >= unit {
        return format_units(base, decimals, 4);
    }
    let frac = format!("{:0>width$}", abs, width = decimals as usize);
    let lead = frac.find(|c: char| c != '0').unwrap_or(0);
    format_units(base, decimals, (decimals as usize).min(lead + 4))
}

/// `n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits })`
/// with the currency's two-digit minimum.
fn currency(n: f64, max_frac: usize) -> String {
    let neg = n < 0.0;
    let a = n.abs();
    // Intl rounds half away from zero on the decimal value.
    let s = format!("{:.*}", max_frac, round_half_up(a, max_frac));
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut frac = frac.to_string();
    while frac.len() > 2 && frac.ends_with('0') {
        frac.pop();
    }
    let mut out = String::new();
    if neg && (whole != "0" || frac.chars().any(|c| c != '0')) {
        out.push('-');
    }
    out.push('$');
    out.push_str(&group3(whole));
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&frac);
    }
    out
}

fn round_half_up(a: f64, digits: usize) -> f64 {
    let k = 10f64.powi(digits as i32);
    // Correct for binary representation (1.005 -> 1.01 like Intl, which rounds the decimal string).
    let shifted: f64 = format!("{:.12}", a * k).parse().unwrap_or(a * k);
    (shifted + 0.5).floor() / k
}

/// `usd`: "$1,234.56"; non-finite is "$0.00".
pub fn usd(n: f64) -> String {
    if !n.is_finite() {
        return "$0.00".into();
    }
    currency(n, 2)
}

/// `price`: more digits for small prices.
pub fn price(n: f64) -> String {
    if n == 0.0 || !n.is_finite() {
        return "$0.00".into();
    }
    let digits = if n < 0.01 { 6 } else if n < 1.0 { 4 } else { 2 };
    currency(n, digits)
}

/// Base units as a float (`toNumber`), for USD values only.
pub fn to_number(base: i128, decimals: u32) -> f64 {
    base as f64 / 10f64.powi(decimals as i32)
}

/// `n.toLocaleString("en-US", { minimumFractionDigits, maximumFractionDigits })`: grouped,
/// rounded half away from zero, trailing zeros past the minimum dropped.
pub fn num_locale(n: f64, min_frac: usize, max_frac: usize) -> String {
    if !n.is_finite() {
        return if n.is_nan() { "NaN".into() } else if n > 0.0 { "∞".into() } else { "-∞".into() };
    }
    let neg = n < 0.0;
    let s = format!("{:.*}", max_frac, round_half_up(n.abs(), max_frac));
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut frac = frac.to_string();
    while frac.len() > min_frac && frac.ends_with('0') {
        frac.pop();
    }
    let zero = whole == "0" && frac.chars().all(|c| c == '0');
    let mut out = String::new();
    if neg && !zero {
        out.push('-');
    }
    out.push_str(&group3(whole));
    if !frac.is_empty() {
        out.push('.');
        out.push_str(&frac);
    }
    out
}

/// rare.market's "unknown" (marketplace/format.ts DASH).
pub const DASH: &str = "--";

/// marketplace/format.ts `trimZeros`.
pub fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// marketplace/format.ts `fmtUsd`: "$1.24", "$12,400", "$12.4K", "$9.7M"; unknown is "--".
pub fn fmt_usd(v: Option<f64>) -> String {
    let Some(v) = v.filter(|v| v.is_finite()) else { return DASH.into() };
    let a = v.abs();
    if a >= 1_000_000.0 {
        return format!("${}M", trim_zeros(&to_fixed(v / 1_000_000.0, 1)));
    }
    if a >= 10_000.0 {
        return format!("${}K", trim_zeros(&to_fixed(v / 1_000.0, 1)));
    }
    let d = if a < 1000.0 { 2 } else { 0 };
    format!("${}", num_locale(v, d, d))
}

/// marketplace/format.ts `mistToSui`.
pub fn mist_to_sui(mist: i128) -> f64 {
    (mist / 1_000_000_000) as f64 + (mist % 1_000_000_000) as f64 / 1e9
}

/// marketplace/format.ts `fmtSui`: `dp` fixed decimals (cards 3), `compact` abbreviates; None is "--".
pub fn fmt_sui(mist: Option<i128>, dp: usize, compact: bool, trim: bool) -> String {
    let Some(m) = mist else { return DASH.into() };
    let n = mist_to_sui(m);
    if compact && n.abs() >= 1_000_000.0 {
        return format!("{}M", trim_zeros(&to_fixed(n / 1_000_000.0, 2)));
    }
    if compact && n.abs() >= 10_000.0 {
        return format!("{}K", trim_zeros(&to_fixed(n / 1_000.0, 3)));
    }
    let min = if trim {
        0
    } else if n != 0.0 && n.abs() < 1000.0 {
        dp
    } else {
        dp.min(2)
    };
    let max = if n.abs() >= 1000.0 { dp.min(2) } else { dp };
    num_locale(n, min.min(max), max)
}

/// wallet/ui/home/lib.tsx `fmtAmount`: token amounts grouped, decimals that fit ("1,204.5", "0.000123").
pub fn fmt_token(n: Option<f64>, max: usize) -> String {
    let Some(n) = n.filter(|n| n.is_finite()) else { return DASH.into() };
    let a = n.abs();
    let dp = if a >= 1000.0 { 2 } else if a >= 1.0 { 4 } else { max };
    num_locale(n, 0, dp)
}

/// wallet/ui/home/lib.tsx `unitsToNumber`: base units (maybe signed) to whole tokens.
pub fn units_to_number(raw: &str, decimals: u32) -> f64 {
    let neg = raw.starts_with('-');
    let digits = raw.trim_start_matches('-');
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return 0.0;
    }
    let Ok(v) = digits.parse::<u128>() else { return 0.0 };
    let unit = 10u128.pow(decimals);
    let n = (v / unit) as f64 + (v % unit) as f64 / unit as f64;
    if neg {
        -n
    } else {
        n
    }
}

/// Days since 1970-01-01 to (year, month, day).
pub fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Local wall time: ms since the epoch shifted by the device's UTC offset (minutes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// days since the epoch in local time (same-day checks)
    pub days: i64,
}

pub fn local(ms: i64, tz_min: i32) -> Local {
    let t = ms + tz_min as i64 * 60_000;
    let days = t.div_euclid(86_400_000);
    let rem = t.rem_euclid(86_400_000);
    let (year, month, day) = civil(days);
    Local { year, month, day, hour: (rem / 3_600_000) as u32, minute: ((rem / 60_000) % 60) as u32, days }
}

/// `toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })` in en-US: "3:07 PM".
pub fn time_hm(ms: i64, tz_min: i32) -> String {
    let l = local(ms, tz_min);
    let h12 = match l.hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{h12}:{:02} {}", l.minute, if l.hour < 12 { "AM" } else { "PM" })
}

/// `toLocaleDateString([], { month: "short", day: "numeric" })`: "Mar 4" (with ", 2025" when `year`).
pub fn date_md(ms: i64, tz_min: i32, year: bool) -> String {
    let l = local(ms, tz_min);
    if year {
        format!("{} {}, {}", MONTHS[(l.month - 1) as usize], l.day, l.year)
    } else {
        format!("{} {}", MONTHS[(l.month - 1) as usize], l.day)
    }
}

/// `Number.prototype.toPrecision(p)` for positive finite numbers.
pub fn to_precision(x: f64, p: usize) -> String {
    if x == 0.0 {
        return format!("{:.*}", p.saturating_sub(1), 0.0);
    }
    let e = x.abs().log10().floor() as i32;
    // the rounded mantissa can carry into the next power (9.999 -> 10.0)
    let m = format!("{:.*e}", p - 1, x);
    let (mant, exp) = m.split_once('e').unwrap_or((&m, "0"));
    let exp: i32 = exp.parse().unwrap_or(e);
    if exp < -6 || exp >= p as i32 {
        let sign = if exp < 0 { "-" } else { "+" };
        return format!("{mant}e{sign}{}", exp.abs());
    }
    let digits = (p as i32 - 1 - exp).max(0) as usize;
    to_fixed(x, digits)
}

/// `toLocaleString([], { dateStyle: "medium", timeStyle: "short" })` in en-US: "Oct 27, 2026, 3:07 PM".
pub fn date_time_medium(ms: i64, tz_min: i32) -> String {
    format!("{}, {}", date_md(ms, tz_min, true), time_hm(ms, tz_min))
}

/// lib/fail.ts `failed`: the one sentence a player sees ("Couldn't ... . Try again."); the raw
/// error goes to the log only.
pub fn failed(what: &str) -> String {
    log::warn!("error: {what}");
    format!("{what} Try again.")
}

/// `Number.prototype.toFixed(d)`: the exact binary value rounded to `d` decimals, a tie going
/// up in magnitude (4.25 is "4.3", where Rust's `{:.1}` gives "4.2").
pub fn to_fixed(x: f64, d: usize) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    let long = format!("{:.*}", d + 40, x.abs());
    let (int_part, frac) = long.split_once('.').unwrap_or((&long, ""));
    let keep = &frac[..d.min(frac.len())];
    let rest = &frac[d.min(frac.len())..];
    let up = match rest.as_bytes().first() {
        Some(b) if *b > b'5' => true,
        Some(b'5') => true,
        _ => false,
    };
    let mut digits: Vec<u8> = format!("{int_part}{keep}").bytes().map(|b| b - b'0').collect();
    if up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
                break;
            }
            i -= 1;
            if digits[i] == 9 {
                digits[i] = 0;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let s: String = digits.iter().map(|d| (b'0' + d) as char).collect();
    let (w, f) = s.split_at(s.len() - d);
    let w = if w.is_empty() { "0" } else { w };
    let neg = x < 0.0 && s.bytes().any(|b| b != b'0');
    format!("{}{}{}{}", if neg { "-" } else { "" }, w, if d > 0 { "." } else { "" }, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_amounts() {
        assert_eq!(format_units(1_234_567_800_000, 9, 4), "1,234.5678");
        assert_eq!(format_units(1_500_000_000, 9, 4), "1.5");
        assert_eq!(format_units(-2_000_000_000, 9, 4), "-2");
        assert_eq!(format_amount(1_990_000, 9), "0.00199");
        assert_eq!(format_amount(1_500_000_000, 9), "1.5");
        assert_eq!(format_amount(0, 9), "0");
        assert_eq!(format_amount(123, 0), "123");
    }

    #[test]
    fn money() {
        assert_eq!(usd(1234.567), "$1,234.57");
        assert_eq!(usd(0.5), "$0.50");
        assert_eq!(usd(-3.0), "-$3.00");
        assert_eq!(usd(f64::NAN), "$0.00");
        assert_eq!(usd(1.005), "$1.01");
        assert_eq!(price(0.906), "$0.906");
        assert_eq!(price(0.00012345), "$0.000123");
        assert_eq!(price(12.0), "$12.00");
    }

    #[test]
    fn locale_numbers() {
        assert_eq!(num_locale(1234.5, 0, 2), "1,234.5");
        assert_eq!(num_locale(0.000123, 0, 6), "0.000123");
        assert_eq!(num_locale(-0.0001, 0, 2), "0");
        assert_eq!(fmt_usd(Some(1.239)), "$1.24");
        assert_eq!(fmt_usd(Some(1284.07)), "$1,284");
        assert_eq!(fmt_usd(Some(12_400.0)), "$12.4K");
        assert_eq!(fmt_usd(Some(9_660_000.0)), "$9.7M");
        assert_eq!(fmt_usd(None), "--");
        assert_eq!(fmt_sui(Some(69_100_000_000), 3, false, false), "69.100");
        assert_eq!(fmt_sui(Some(69_100_000_000), 3, false, true), "69.1");
        assert_eq!(fmt_sui(Some(16_571_000_000_000), 3, true, false), "16.571K");
        assert_eq!(fmt_sui(Some(1_234_500_000_000), 3, false, false), "1,234.50");
        assert_eq!(fmt_sui(None, 3, false, false), "--");
        assert_eq!(fmt_token(Some(1204.5), 6), "1,204.5");
        assert_eq!(fmt_token(Some(0.000123), 6), "0.000123");
        assert_eq!(fmt_token(Some(12.34567), 6), "12.3457");
        assert_eq!(fmt_token(None, 6), "--");
        assert_eq!(units_to_number("-1500000000", 9), -1.5);
        assert_eq!(units_to_number("x", 9), 0.0);
    }

    #[test]
    fn fixed() {
        assert_eq!(to_fixed(4.25, 1), "4.3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(-2.5, 0), "-3");
        assert_eq!(to_fixed(9.96, 1), "10.0");
        assert_eq!(to_fixed(0.0, 2), "0.00");
        assert_eq!(to_fixed(-0.001, 2), "0.00");
    }

    #[test]
    fn precision() {
        assert_eq!(to_precision(0.906, 3), "0.906");
        assert_eq!(to_precision(0.0123456, 3), "0.0123");
        assert_eq!(to_precision(0.0000123456, 3), "0.0000123");
        assert_eq!(to_precision(0.00000012, 3), "1.20e-7");
        assert_eq!(to_precision(0.9999, 3), "1.00");
    }

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        // 2026-10-27 15:07 UTC
        let t = 1_793_113_620_000;
        assert_eq!(time_hm(t, 0), "3:07 PM");
        assert_eq!(time_hm(t, -4 * 60), "11:07 AM");
        assert_eq!(date_md(t, 0, false), "Oct 27");
        assert_eq!(date_md(t, 0, true), "Oct 27, 2026");
        assert_eq!(time_hm(1_793_059_200_000, 0), "12:00 AM");
        assert_eq!(date_time_medium(t, 0), "Oct 27, 2026, 3:07 PM");
    }

    #[test]
    fn ids() {
        assert_eq!(short_id("0x2"), "0x2");
        assert_eq!(short_id("0x0"), "0x0");
        assert_eq!(short_id("0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb"), "0x1eab…b2fb");
        assert_eq!(addr_short("0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb"), "0x1eab...b2fb");
        assert_eq!(sui_type(), "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI");
        assert_eq!(norm_type("0x2::coin::Coin<0x2::sui::SUI>"), format!("{}::coin::Coin<{}>", norm_id("0x2"), sui_type()));
        assert_eq!(norm_type("0xab::pool::Pool<0x2::sui::SUI, u64>"), format!("{}::pool::Pool<{},u64>", norm_id("0xab"), sui_type()));
        assert_eq!(norm_type("not a type"), "not a type");
    }
}
