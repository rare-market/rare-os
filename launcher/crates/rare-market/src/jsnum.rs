//! JavaScript's number formatting, digit for digit, so ported screens print exactly what the web
//! launcher printed: `Number.prototype.toFixed` (exact binary value, ties away from zero) and
//! `toLocaleString("en-US", { minimumFractionDigits, maximumFractionDigits })` (the shortest
//! round-trip decimal, rounded half away from zero, grouped with commas).

/// Adds one unit in the last place to a string of decimal digits ("199" -> "200", "99" -> "100").
fn increment(digits: &str) -> String {
    let mut v: Vec<u8> = digits.bytes().collect();
    let mut i = v.len();
    loop {
        if i == 0 {
            v.insert(0, b'1');
            break;
        }
        i -= 1;
        if v[i] == b'9' {
            v[i] = b'0';
        } else {
            v[i] += 1;
            break;
        }
    }
    String::from_utf8(v).unwrap()
}

/// Splits "123.4500" into ("123", "4500").
fn split(s: &str) -> (&str, &str) {
    s.split_once('.').unwrap_or((s, ""))
}

/// Rounds the non-negative decimal `int.frac` to `d` fraction digits, ties away from zero when
/// `tie_up`; `rest_nonzero` says whether digits beyond `frac` are nonzero.
fn round_dec(int: &str, frac: &str, d: usize) -> (String, String) {
    if frac.len() <= d {
        return (int.to_string(), format!("{frac:0<d$}"));
    }
    let keep = &frac[..d];
    let next = frac.as_bytes()[d];
    let up = next >= b'5';
    if !up {
        return (int.to_string(), keep.to_string());
    }
    let joined = increment(&format!("{int}{keep}"));
    let (i, f) = joined.split_at(joined.len() - d);
    (if i.is_empty() { "0".to_string() } else { i.to_string() }, f.to_string())
}

/// `x.toFixed(d)` for d in 0..=100.
pub fn to_fixed(x: f64, d: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 || x.is_infinite() {
        return js_string(x);
    }
    let neg = x < 0.0;
    // The exact binary value to d + 30 places: a tie shows as 5 followed by zeros (a double cannot
    // carry a nonzero digit 30 places past a 5 at this magnitude).
    let exact = format!("{:.*}", d + 30, x.abs());
    let (int, frac) = split(&exact);
    let (i, f) = round_dec(int, frac, d);
    let body = if d == 0 { i } else { format!("{i}.{f}") };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// `String(x)` for the values the launcher prints (integers and plain decimals; exponent form for
/// very large or small magnitudes as JS does).
pub fn js_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return "0".into();
    }
    let a = x.abs();
    if (1e-6..1e21).contains(&a) {
        return format!("{x}");
    }
    // Exponent form: shortest digits with one before the point.
    let e = format!("{a:e}");
    let (m, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    format!("{}{m}e{}{}", if x < 0.0 { "-" } else { "" }, if exp >= 0 { "+" } else { "-" }, exp.abs())
}

fn group(int: &str) -> String {
    let mut out = String::with_capacity(int.len() + int.len() / 3);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `x.toLocaleString("en-US", { minimumFractionDigits: min, maximumFractionDigits: max })`.
pub fn locale(x: f64, min: usize, max: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞".into() } else { "-∞".into() };
    }
    let max = max.max(min);
    let shortest = format!("{}", x.abs());
    let (int, frac) = split(&shortest);
    let (i, f) = round_dec(int, frac, max);
    let mut f = f.trim_end_matches('0').to_string();
    while f.len() < min {
        f.push('0');
    }
    let body = if f.is_empty() { group(&i) } else { format!("{}.{f}", group(&i)) };
    if x.is_sign_negative() {
        format!("-{body}")
    } else {
        body
    }
}

/// `x.toLocaleString("en-US")` (0 to 3 fraction digits).
pub fn locale_default(x: f64) -> String {
    locale(x, 0, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Outputs of Node 22's V8 for the same calls.
    #[test]
    fn matches_v8() {
        assert_eq!(locale(1.005, 0, 2), "1.01");
        assert_eq!(locale(0.125, 0, 2), "0.13");
        assert_eq!(locale(-0.125, 0, 2), "-0.13");
        assert_eq!(locale(1_234_567.891, 2, 2), "1,234,567.89");
        assert_eq!(locale(-0.001, 0, 2), "-0");
        assert_eq!(locale(1e21, 0, 2), "1,000,000,000,000,000,000,000");
        assert_eq!(locale(2.5, 0, 0), "3");
        assert_eq!(locale(0.1 + 0.2, 0, 20), "0.30000000000000004");
        assert_eq!(locale(123.456, 3, 3), "123.456");
        assert_eq!(locale(5e-7, 0, 3), "0");
        assert_eq!(locale(-5e-7, 0, 3), "-0");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(-0.0001, 2), "-0.00");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(1e21, 2), "1e+21");
        assert_eq!(to_fixed(-1.5, 0), "-2");
        assert_eq!(to_fixed(0.000001234, 10), "0.0000012340");
        assert_eq!(to_fixed(999.996, 2), "1000.00");
        assert_eq!(js_string(1.5), "1.5");
        assert_eq!(js_string(1e-7), "1e-7");
    }
}
