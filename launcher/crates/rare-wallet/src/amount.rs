//! Amount parsing and formatting for the send screens and the review sheet (ports
//! `src/wallet/tx/amount.ts`). Base units are integers (or decimal strings); nothing here goes
//! through floats except USD display. Every string matches the TS byte for byte (checked against
//! testdata/review-vectors.json).

/// Leave a little SUI behind when sending most of it, so the transfer can still pay gas.
pub const SUI_GAS_RESERVE: i128 = 10_000_000;

fn group3(whole: &str) -> String {
    let b = whole.as_bytes();
    let mut out = String::with_capacity(b.len() + b.len() / 3);
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c as char);
    }
    out
}

/// `formatUnits`: base units to "1,234.5678" with at most `max_frac` fraction digits, trailing
/// zeros dropped.
pub fn format_units(base: i128, decimals: u32, max_frac: usize) -> String {
    let neg = base < 0;
    let d = decimals as usize;
    let s = format!("{:0>width$}", base.unsigned_abs(), width = d + 1);
    let (whole, frac) = s.split_at(s.len() - d);
    let frac = frac[..max_frac.min(frac.len())].trim_end_matches('0');
    format!("{}{}{}{}", if neg { "-" } else { "" }, group3(whole), if frac.is_empty() { "" } else { "." }, frac)
}

/// `formatAmount`: 4 fraction digits for whole amounts, more for small ones so a fee like
/// 0.00199 SUI does not read as 0.0019 or 0.
pub fn format_amount(base: i128, decimals: u32) -> String {
    let abs = base.unsigned_abs();
    let unit = 10u128.checked_pow(decimals).unwrap_or(u128::MAX);
    if abs == 0 || abs >= unit {
        return format_units(base, decimals, 4);
    }
    // first significant digit position, then 3 more digits
    let frac = format!("{:0>width$}", abs, width = decimals as usize);
    let lead = frac.find(|c: char| c != '0').unwrap_or(0);
    format_units(base, decimals, (decimals as usize).min(lead + 4))
}

/// `parseUnits`: "1.5" to a base units string. None for empty or invalid input.
pub fn parse_units(input: &str, decimals: u32) -> Option<String> {
    let t = js_trim(input);
    if !is_decimal_text(t) || t.is_empty() || t == "." {
        return None;
    }
    let (w, f) = t.split_once('.').unwrap_or((t, ""));
    let frac: String = f.chars().take(decimals as usize).collect();
    let digits = format!("{}{:0<width$}", if w.is_empty() { "0" } else { w }, frac, width = decimals as usize);
    let n = digits.trim_start_matches('0');
    Some(if n.is_empty() { "0".into() } else { n.into() })
}

/// `toNumber`: base units to a float of whole tokens.
pub fn to_number(base: i128, decimals: u32) -> f64 {
    format_units(base, decimals, decimals as usize).replace(',', "").parse().unwrap_or(f64::NAN)
}

/// `usd`: "$1,234.56" (`toLocaleString("en-US", { style: "currency", currency: "USD" })`).
pub fn usd(n: f64) -> String {
    if !n.is_finite() {
        return "$0.00".into();
    }
    currency(n, 2)
}

/// `price`: a unit price with more digits for small prices ("$0.000123", "$0.4321", "$2.50").
pub fn price(n: f64) -> String {
    if n == 0.0 || n.is_nan() {
        return "$0.00".into();
    }
    let digits = if n < 0.01 {
        6
    } else if n < 1.0 {
        4
    } else {
        2
    };
    currency(n, digits)
}

/// A quick amount chip: 25 / 50 / 100 (Max) percent.
pub type Chip = u32;

/// `chipAmount`: base units for a quick chip. 25% / 50% never eat into the SUI gas reserve. Max on
/// SUI uses buildCoinSend's max mode instead. `pct` is 25, 50 or 100.
pub fn chip_amount(available: i128, pct: Chip, is_sui: bool) -> i128 {
    let cap = if is_sui {
        if available > SUI_GAS_RESERVE {
            available - SUI_GAS_RESERVE
        } else {
            0
        }
    } else {
        available
    };
    if pct == 100 {
        return cap;
    }
    let part = available * i128::from(pct) / 100;
    part.min(cap)
}

/// `toInput`: base units to plain decimal text for the input (no separators, full precision).
pub fn to_input(base: i128, decimals: u32) -> String {
    format_units(base, decimals, decimals as usize).replace(',', "")
}

/// `cleanAmount`: accept what a number field should: digits and one dot (a comma typed as a dot).
pub fn clean_amount(v: &str) -> Option<String> {
    let s = v.replacen(',', ".", 1);
    is_decimal_text(&s).then_some(s)
}

/// `usdToBase`: USD text to token base units at `price_usd`, rounded down. None when empty,
/// invalid or no price.
pub fn usd_to_base(usd_text: &str, price_usd: Option<f64>, decimals: u32) -> Option<String> {
    let n = js_number(usd_text);
    let p = price_usd.filter(|p| *p != 0.0 && !p.is_nan())?;
    if usd_text.is_empty() || !n.is_finite() {
        return None;
    }
    // toFixed keeps it out of exponent form; trim to the coin's precision, rounding down
    parse_units(&js_to_fixed(n / p, decimals.min(20) as usize), decimals)
}

/// `baseToUsd`: token base units to a USD number.
pub fn base_to_usd(base: i128, decimals: u32, price_usd: Option<f64>) -> Option<f64> {
    let p = price_usd.filter(|p| *p != 0.0 && !p.is_nan())?;
    Some(js_number(&to_input(base, decimals)) * p)
}

// ---------- JS number semantics ----------

/// `/^\d*\.?\d*$/` (ASCII digits).
fn is_decimal_text(s: &str) -> bool {
    let mut dot = false;
    for c in s.chars() {
        match c {
            '0'..='9' => {}
            '.' if !dot => dot = true,
            _ => return false,
        }
    }
    true
}

fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `Number(text)`: JS string to number conversion (NaN for anything it rejects).
pub(crate) fn js_number(text: &str) -> f64 {
    let t = js_trim(text);
    if t.is_empty() {
        return 0.0;
    }
    for (p, r) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if t.len() > 2 && t.get(..2).is_some_and(|pre| pre.eq_ignore_ascii_case(p)) {
            let d = &t[2..];
            if !d.chars().all(|c| c.is_digit(r)) {
                return f64::NAN;
            }
            return d.chars().fold(0.0, |acc, c| acc * f64::from(r) + f64::from(c.to_digit(r).unwrap_or(0)));
        }
    }
    let (sign, body) = match t.as_bytes()[0] {
        b'-' => (-1.0, &t[1..]),
        b'+' => (1.0, &t[1..]),
        _ => (1.0, t),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    // StrDecimalLiteral: digits with an optional point and exponent, at least one digit
    let b = body.as_bytes();
    let mut i = 0;
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - f;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return f64::NAN;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    body.parse::<f64>().map(|v| sign * v).unwrap_or(f64::NAN)
}

/// Rounds a plain decimal string (no sign, no exponent) half up at `frac` fraction digits and
/// returns (whole, fraction) with exactly `frac` fraction digits.
fn round_decimal(s: &str, frac: usize) -> (String, String) {
    let (w, f) = s.split_once('.').unwrap_or((s, ""));
    let mut digits: Vec<u8> = w.bytes().chain(f.bytes().chain(std::iter::repeat(b'0')).take(frac)).collect();
    let round_up = f.as_bytes().get(frac).is_some_and(|d| *d >= b'5');
    if round_up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let split = digits.len() - frac;
    let whole = String::from_utf8_lossy(&digits[..split]).into_owned();
    let fr = String::from_utf8_lossy(&digits[split..]).into_owned();
    (if whole.is_empty() { "0".into() } else { whole }, fr)
}

/// `Number.prototype.toFixed`: exact decimal value of the double, ties to the larger magnitude.
pub(crate) fn js_to_fixed(x: f64, frac: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 || x.is_infinite() {
        return js_string(x);
    }
    let neg = x < 0.0;
    // 1074 fraction digits hold any double exactly
    let exact = format!("{:.1074}", x.abs());
    let (w, f) = round_decimal(&exact, frac);
    format!("{}{}{}{}", if neg { "-" } else { "" }, w, if frac > 0 { "." } else { "" }, f)
}

/// `String(x)` for the cases js_to_fixed hands over (large or infinite).
fn js_string(x: f64) -> String {
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let s = format!("{:e}", x);
    // Rust: "1e21", "1.5e22"; JS: "1e+21", "1.5e+22"
    match s.split_once('e') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
        _ => s,
    }
}

/// `n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits })`:
/// Intl rounds the shortest decimal form of the double half away from zero, keeps at least the
/// currency's two digits, and keeps the sign of a negative that rounds to zero ("-$0.00").
fn currency(n: f64, max_frac: usize) -> String {
    let sign = if n.is_sign_negative() { "-" } else { "" };
    if n.is_nan() {
        return "$NaN".into();
    }
    if n.is_infinite() {
        return format!("{sign}$\u{221e}");
    }
    let (whole, frac) = round_decimal(&format!("{}", n.abs()), max_frac);
    let mut frac = frac;
    while frac.len() > 2 && frac.ends_with('0') {
        frac.pop();
    }
    format!("{sign}${}.{frac}", group3(&whole))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: i128 = 1_000_000_000;

    // tx.test.ts "amount"
    #[test]
    fn chips_25_50_are_plain_fractions_max_on_sui_keeps_the_gas_reserve() {
        assert_eq!(chip_amount(10 * ONE, 25, true), 2_500_000_000);
        assert_eq!(chip_amount(10 * ONE, 50, true), 5 * ONE);
        assert_eq!(chip_amount(10 * ONE, 100, true), 10 * ONE - SUI_GAS_RESERVE);
    }

    #[test]
    fn max_on_other_coins_is_the_whole_balance() {
        assert_eq!(chip_amount(50_000_000, 100, false), 50_000_000);
        assert_eq!(chip_amount(50_000_000, 50, false), 25_000_000);
    }

    #[test]
    fn never_dips_into_the_reserve_on_a_tiny_sui_balance() {
        assert_eq!(chip_amount(SUI_GAS_RESERVE, 100, true), 0);
        assert_eq!(chip_amount(12_000_000, 50, true), 2_000_000);
    }

    #[test]
    fn usd_to_token_units_rounds_down_at_the_price() {
        assert_eq!(usd_to_base("10", Some(4.0), 9).as_deref(), Some("2500000000"));
        assert_eq!(usd_to_base("1", Some(3.0), 6).as_deref(), Some("333333"));
        assert_eq!(usd_to_base("", Some(4.0), 9), None);
        assert_eq!(usd_to_base("5", None, 9), None);
    }

    #[test]
    fn parses_and_formats_base_units() {
        assert_eq!(parse_units("1.5", 9).as_deref(), Some("1500000000"));
        assert_eq!(parse_units(".25", 6).as_deref(), Some("250000"));
        assert_eq!(parse_units("1.1234567", 6).as_deref(), Some("1123456"));
        assert_eq!(parse_units("", 9), None);
        assert_eq!(parse_units("1e5", 9), None);
        assert_eq!(format_units(1_234_567_890_000, 9, 4), "1,234.5678");
        assert_eq!(format_units(-1_500_000_000, 9, 4), "-1.5");
        assert_eq!(to_input(1_234_567_890_123, 9), "1234.567890123");
        assert_eq!(clean_amount("1,5").as_deref(), Some("1.5"));
        assert_eq!(clean_amount("1.2.3"), None);
    }

    #[test]
    fn format_amount_keeps_small_amounts_readable() {
        assert_eq!(format_amount(1_997_880, 9), "0.001997");
        assert_eq!(format_amount(2_500_000_000, 9), "2.5");
        assert_eq!(format_amount(0, 9), "0");
        assert_eq!(format_amount(1, 9), "0.000000001");
    }

    #[test]
    fn money_like_intl() {
        assert_eq!(usd(1234567.891), "$1,234,567.89");
        assert_eq!(usd(1.005), "$1.01");
        assert_eq!(usd(-0.0), "-$0.00");
        assert_eq!(usd(f64::NAN), "$0.00");
        assert_eq!(price(0.0000005), "$0.000001");
        assert_eq!(price(0.5), "$0.50");
        assert_eq!(price(0.0), "$0.00");
        assert_eq!(price(f64::INFINITY), "$\u{221e}");
        assert_eq!(js_to_fixed(0.5, 0), "1");
        assert_eq!(js_to_fixed(1.005, 2), "1.00");
        assert_eq!(js_to_fixed(1e21, 2), "1e+21");
        assert_eq!(js_number(" 0x1f "), 31.0);
        assert!(js_number("1e").is_nan());
        assert_eq!(base_to_usd(2_500_000_000, 9, Some(2.0)), Some(5.0));
        assert_eq!(to_number(-1_500_000_000, 9), -1.5);
    }
}
