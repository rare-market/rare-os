//! JavaScript's number-to-text rules, so every figure reads exactly as the web launcher printed it.
//!
//! `toFixed`, `String(n)` and grouped `toLocaleString("en-US", { min/maxFractionDigits })` are
//! rare-market's (shared by every ported crate, checked against V8). This module adds what Trade
//! also needs on the same rules:
//! - [`to_precision`]: `toPrecision`, the exact binary value rounded half away from zero.
//! - [`locale`] with significant digits or without grouping (ICU rounds the shortest digits, not
//!   the exact value, so `1.005` with two decimals is `1.01` there but `1.00` in `toFixed`).
//! - [`js_round`] (`Math.round`, ties toward positive infinity), `Number(...)` of text and JSON,
//!   and JSON numbers written as `JSON.stringify` writes them.

pub use rare_market::jsnum::{js_string, to_fixed};

/// A non-negative decimal: value = 0.d1 d2 d3 ... x 10^point. No leading or trailing zero digits;
/// no digits means zero.
#[derive(Clone, Debug, PartialEq)]
struct Dec {
    digits: Vec<u8>,
    point: i32,
}

impl Dec {
    fn zero() -> Dec {
        Dec { digits: Vec::new(), point: 0 }
    }

    /// From fixed notation without a sign ("123.4500").
    fn from_fixed(s: &str) -> Dec {
        let (int, frac) = s.split_once('.').unwrap_or((s, ""));
        let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
        let mut point = int.len() as i32;
        let lead = digits.iter().take_while(|&&d| d == 0).count();
        digits.drain(..lead);
        point -= lead as i32;
        Dec::trimmed(digits, point)
    }

    fn trimmed(mut digits: Vec<u8>, point: i32) -> Dec {
        while digits.last() == Some(&0) {
            digits.pop();
        }
        if digits.is_empty() {
            Dec::zero()
        } else {
            Dec { digits, point }
        }
    }

    /// Rounds half away from zero, keeping `frac` digits after the point.
    fn round_frac(&self, frac: i32) -> Dec {
        if self.digits.is_empty() {
            return Dec::zero();
        }
        let keep = self.point + frac;
        if keep >= self.digits.len() as i32 {
            return self.clone();
        }
        if keep < 0 {
            return Dec::zero();
        }
        let keep = keep as usize;
        let up = self.digits[keep] >= 5;
        let mut digits = self.digits[..keep].to_vec();
        let mut point = self.point;
        if up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    point += 1;
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
        Dec::trimmed(digits, point)
    }

    /// Rounds half away from zero to `sig` significant digits.
    fn round_sig(&self, sig: i32) -> Dec {
        if self.digits.is_empty() {
            return Dec::zero();
        }
        self.round_frac(sig - self.point)
    }

    /// Fixed notation with at least `min_frac` digits after the point.
    fn fixed(&self, min_frac: usize, grouping: bool) -> String {
        let p = self.point;
        let mut int = String::new();
        if p <= 0 {
            int.push('0');
        } else {
            for i in 0..p as usize {
                int.push((b'0' + self.digits.get(i).copied().unwrap_or(0)) as char);
            }
        }
        let mut frac = String::new();
        if p < 0 {
            frac.extend(std::iter::repeat('0').take((-p) as usize));
        }
        let start = p.max(0) as usize;
        for &d in self.digits.iter().skip(start) {
            frac.push((b'0' + d) as char);
        }
        while frac.len() < min_frac {
            frac.push('0');
        }
        let int = if grouping { group(&int) } else { int };
        if frac.is_empty() {
            int
        } else {
            format!("{int}.{frac}")
        }
    }
}

fn group(int: &str) -> String {
    let n = int.len();
    let mut out = String::with_capacity(n + n / 3);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The exact decimal value of a finite double (at most 1074 fraction digits).
fn exact(x: f64) -> Dec {
    Dec::from_fixed(&format!("{:.1100}", x.abs()))
}

/// The shortest digits that read back as the same double.
fn shortest(x: f64) -> Dec {
    let s = format!("{:e}", x.abs());
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: Vec<u8> = mant.bytes().filter(|b| b.is_ascii_digit()).map(|b| b - b'0').collect();
    let lead = digits.iter().take_while(|&&d| d == 0).count();
    if lead == digits.len() {
        return Dec::zero();
    }
    Dec::trimmed(digits[lead..].to_vec(), exp + 1 - lead as i32)
}

fn special(x: f64) -> Option<String> {
    if x.is_nan() {
        Some("NaN".into())
    } else if x.is_infinite() {
        Some(if x > 0.0 { "Infinity" } else { "-Infinity" }.into())
    } else {
        None
    }
}

/// `x.toPrecision(p)`, p from 1 to 100.
pub fn to_precision(x: f64, p: usize) -> String {
    if let Some(s) = special(x) {
        return s;
    }
    let p = p.max(1);
    let sign = if x < 0.0 { "-" } else { "" };
    if x == 0.0 {
        return if p > 1 { format!("0.{}", "0".repeat(p - 1)) } else { "0".into() };
    }
    let d = exact(x).round_sig(p as i32);
    let e = d.point - 1;
    if e < -6 || e >= p as i32 {
        let mut m = String::new();
        m.push((b'0' + d.digits[0]) as char);
        if p > 1 {
            m.push('.');
            for i in 1..p {
                m.push((b'0' + d.digits.get(i).copied().unwrap_or(0)) as char);
            }
        }
        return format!("{sign}{m}e{}{}", if e >= 0 { "+" } else { "-" }, e.abs());
    }
    let frac = (p as i32 - d.point).max(0) as usize;
    format!("{sign}{}", d.fixed(frac, false))
}

/// Options for [`locale`], as `Intl.NumberFormat("en-US", ...)` takes them.
#[derive(Clone, Copy, Debug)]
pub struct Locale {
    pub min_frac: usize,
    pub max_frac: usize,
    /// `maximumSignificantDigits`; when set the fraction digit options are ignored.
    pub max_sig: Option<usize>,
    pub grouping: bool,
}

impl Default for Locale {
    /// `toLocaleString("en-US")` with no options: up to three decimals, grouped.
    fn default() -> Self {
        Locale { min_frac: 0, max_frac: 3, max_sig: None, grouping: true }
    }
}

impl Locale {
    pub fn frac(min_frac: usize, max_frac: usize) -> Locale {
        Locale { min_frac, max_frac: max_frac.max(min_frac), ..Locale::default() }
    }
    pub fn max_frac(max_frac: usize) -> Locale {
        Locale { min_frac: 0, max_frac, ..Locale::default() }
    }
    pub fn sig(max_sig: usize) -> Locale {
        Locale { max_sig: Some(max_sig), ..Locale::default() }
    }
    pub fn no_grouping(mut self) -> Locale {
        self.grouping = false;
        self
    }
}

/// `x.toLocaleString("en-US", options)`.
pub fn locale(x: f64, o: Locale) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞" } else { "-∞" }.into();
    }
    if o.max_sig.is_none() && o.grouping {
        return rare_market::jsnum::locale(x, o.min_frac, o.max_frac);
    }
    let d = shortest(x);
    let body = match o.max_sig {
        Some(sig) => d.round_sig(sig as i32).fixed(0, o.grouping),
        None => d.round_frac(o.max_frac as i32).fixed(o.min_frac, o.grouping),
    };
    if x.is_sign_negative() {
        format!("-{body}")
    } else {
        body
    }
}

/// `Math.round(x)`: halves go up (toward positive infinity).
pub fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// `Number(text)` for the decimal strings the formatters produce (NaN when it is not a number).
pub fn js_number(text: &str) -> f64 {
    let t = text.trim();
    if t.is_empty() {
        return 0.0;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Number(v)` for a JSON value.
pub fn value_number(v: &serde_json::Value) -> f64 {
    match v {
        serde_json::Value::Null => 0.0,
        serde_json::Value::Bool(b) => f64::from(u8::from(*b)),
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        serde_json::Value::String(s) => js_number(s),
        _ => f64::NAN,
    }
}

/// A JSON number written the way `JSON.stringify` writes a JS number: integral values as integers.
pub fn json_number(x: f64) -> serde_json::Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        if x >= 0.0 {
            serde_json::Value::from(x as u64)
        } else {
            serde_json::Value::from(x as i64)
        }
    } else if x.is_finite() {
        serde_json::Value::from(x)
    } else {
        serde_json::Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_rounds_ties_away_from_zero() {
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(-0.0001, 2), "-0.00");
        assert_eq!(to_fixed(0.0, 2), "0.00");
        assert_eq!(to_fixed(1234.5678, 1), "1234.6");
        assert_eq!(to_fixed(9.99, 1), "10.0");
    }

    #[test]
    fn to_precision_matches_js() {
        assert_eq!(to_precision(0.0242, 3), "0.0242");
        assert_eq!(to_precision(0.000123456, 4), "0.0001235");
        assert_eq!(to_precision(1e-7, 2), "1.0e-7");
        assert_eq!(to_precision(123456.0, 2), "1.2e+5");
        assert_eq!(to_precision(0.0, 3), "0.00");
        assert_eq!(to_precision(99.99, 3), "100");
    }

    #[test]
    fn js_string_matches_js() {
        assert_eq!(js_string(1e-7), "1e-7");
        assert_eq!(js_string(0.0042), "0.0042");
        assert_eq!(js_string(123.0), "123");
        assert_eq!(js_string(1e21), "1e+21");
        assert_eq!(js_string(-2.5), "-2.5");
        assert_eq!(js_string(0.1 + 0.2), "0.30000000000000004");
    }

    #[test]
    fn locale_matches_icu() {
        assert_eq!(locale(1.005, Locale::frac(2, 2)), "1.01");
        assert_eq!(locale(86_597.34, Locale::frac(2, 2)), "86,597.34");
        assert_eq!(locale(240_016_237.0, Locale::max_frac(0)), "240,016,237");
        assert_eq!(locale(0.14604001378161055, Locale::sig(5)), "0.14604");
        assert_eq!(locale(1e-7, Locale::max_frac(20).no_grouping()), "0.0000001");
        assert_eq!(locale(1234567.0, Locale::default()), "1,234,567");
    }

    #[test]
    fn math_round() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }
}
