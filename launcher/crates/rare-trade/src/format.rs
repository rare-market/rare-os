//! Plain-text formatters. They follow the same rules as
//! [`crate::num`] and return "" for an unknown value: a missing figure is left out, never shown as
//! dashes.

use crate::jsnum::{js_round, js_string, locale, to_fixed, Locale};
use crate::num::{compact_figure, MINUS};

pub use crate::num::usd_price;

/// A market figure as text: $1.23M, $45.6K, $50K, $812.
pub fn usd_compact(value: Option<f64>) -> String {
    let Some(v) = value.filter(|v| v.is_finite()) else { return String::new() };
    format!("{}${}", if v < 0.0 { MINUS } else { "" }, compact_figure(v.abs()))
}

/// [`usd_price`] with the digits a per-second chart needs: moves that the everyday label rounds
/// away stay visible.
pub fn usd_price_fine(value: Option<f64>) -> String {
    let Some(v) = value.filter(|v| v.is_finite() && *v > 0.0) else { return String::new() };
    if v >= 1000.0 {
        usd_price(Some(v))
    } else if v >= 100.0 {
        format!("${}", to_fixed(v, 3))
    } else if v >= 1.0 {
        format!("${}", to_fixed(v, 4))
    } else if v >= 0.001 {
        format!("${}", crate::jsnum::to_precision(v, 4))
    } else {
        usd_price(Some(v))
    }
}

/// A percentage without its sign, for places that draw the sign themselves.
pub fn pct(value: Option<f64>) -> String {
    let Some(v) = value.filter(|v| v.is_finite()) else { return String::new() };
    let abs = v.abs();
    let body = if abs >= 1000.0 { locale(abs, Locale::max_frac(0)) } else { to_fixed(abs, 2) };
    format!("{body}%")
}

/// Exact under 1,000, then compact: 901, 23.4K.
pub fn count(value: Option<f64>) -> String {
    let Some(v) = value.filter(|v| v.is_finite()) else { return String::new() };
    let abs = v.abs();
    let body = if abs < 999.5 {
        js_string(js_round(abs))
    } else if abs < 999_950.0 {
        let s = to_fixed(abs / 1000.0, 1);
        format!("{}K", s.strip_suffix(".0").unwrap_or(&s))
    } else {
        compact_figure(abs)
    };
    format!("{}{body}", if v < 0.0 { MINUS } else { "" })
}

/// 3s, 14m, 5h, 2d, 3mo, 1y since `ms` (both in milliseconds).
pub fn age(ms: Option<f64>, now: f64) -> String {
    let Some(ms) = ms.filter(|m| *m != 0.0 && !m.is_nan()) else { return String::new() };
    let s = ((now - ms) / 1000.0).max(0.0);
    let f = |x: f64| js_string(x.floor());
    if s < 60.0 {
        format!("{}s", f(s))
    } else if s < 3600.0 {
        format!("{}m", f(s / 60.0))
    } else if s < 86400.0 {
        format!("{}h", f(s / 3600.0))
    } else if s < 86400.0 * 60.0 {
        format!("{}d", f(s / 86400.0))
    } else if s < 86400.0 * 365.0 {
        format!("{}mo", f(s / (86400.0 * 30.0)))
    } else {
        format!("{}y", f(s / (86400.0 * 365.0)))
    }
}

/// 0x1234…abcd
pub fn short_addr(addr: &str) -> String {
    if addr.is_empty() {
        return String::new();
    }
    let c: Vec<char> = addr.chars().collect();
    let head: String = c.iter().take(6).collect();
    let tail: String = c[c.len().saturating_sub(4)..].iter().collect();
    format!("{head}…{tail}")
}

const WJ: char = '\u{2060}';

/// A name for running text: a bare address (or coin type) standing in for a name becomes
/// 0x1234…abcd so it never runs off the screen.
pub fn short_name(name: &str) -> String {
    let t = name.trim();
    let short = match bare_address(t) {
        Some(a) => short_addr(a),
        None => name.to_string(),
    };
    // Word joiners keep the line from breaking at the ellipsis.
    let c: Vec<char> = short.chars().collect();
    let mut out = String::with_capacity(short.len() + 8);
    for (i, &ch) in c.iter().enumerate() {
        if ch == '…' && joinable(&c, i) {
            out.push(WJ);
            out.push('…');
            out.push(WJ);
        } else {
            out.push(ch);
        }
    }
    out
}

/// `^(0x[0-9a-f]{16,})(::.*)?$`i: the address part of a bare address or coin type.
fn bare_address(t: &str) -> Option<&str> {
    let rest = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))?;
    let hex = rest.bytes().take_while(|b| b.is_ascii_hexdigit()).count();
    if hex < 16 {
        return None;
    }
    let after = &rest[hex..];
    (after.is_empty() || after.starts_with("::")).then(|| &t[..2 + hex])
}

/// `(0x[0-9a-f]+)…([0-9a-f]+)`i around the ellipsis at `i`.
fn joinable(c: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 && c[j - 1].is_ascii_hexdigit() {
        j -= 1;
    }
    let before = i - j;
    // "0x" must sit right before the digits (x is not a hex digit, so the run stops there).
    let has_0x = j >= 2 && (c[j - 1] == 'x' || c[j - 1] == 'X') && c[j - 2] == '0';
    has_0x && before >= 1 && c.get(i + 1).is_some_and(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    //! format.test.ts
    use super::*;

    #[test]
    fn usd_compact_compacts_by_magnitude() {
        let u = |v: f64| usd_compact(Some(v));
        assert_eq!(u(812.0), "$812");
        assert_eq!(u(45_600.0), "$45.6K");
        assert_eq!(u(1_230_000.0), "$1.23M");
        assert_eq!(u(5_010_000_000.0), "$5.01B");
        assert_eq!(u(50_000.0), "$50K");
        assert_eq!(u(2_500_000.0), "$2.5M");
        assert_eq!(u(0.5), "$0.50");
        assert_eq!(u(100.0), "$100");
    }

    #[test]
    fn usd_compact_leaves_missing_values_out() {
        assert_eq!(usd_compact(None), "");
        assert_eq!(usd_compact(Some(f64::NAN)), "");
    }

    #[test]
    fn usd_price_uses_subscript_zeros_for_tiny_prices() {
        assert_eq!(usd_price(Some(0.0000041234)), "$0.0₅4123");
    }

    #[test]
    fn usd_price_keeps_normal_prices_readable() {
        assert_eq!(usd_price(Some(1.2134)), "$1.213");
        assert_eq!(usd_price(Some(0.0242)), "$0.0242");
        assert_eq!(usd_price(Some(86_597.34)), "$86,597.34");
        assert_eq!(usd_price(Some(4109.9)), "$4,109.90");
    }

    #[test]
    fn usd_price_leaves_zero_or_missing_prices_out() {
        assert_eq!(usd_price(Some(0.0)), "");
        assert_eq!(usd_price(None), "");
    }

    #[test]
    fn formats_percentages_without_a_sign() {
        assert_eq!(pct(Some(-28.4567)), "28.46%");
        assert_eq!(pct(Some(240_016_237.0)), "240,016,237%");
    }

    #[test]
    fn compacts_counts() {
        let c = |v: f64| count(Some(v));
        assert_eq!(c(901.0), "901");
        assert_eq!(c(14_820.0), "14.8K");
        assert_eq!(c(317_300.0), "317.3K");
        assert_eq!(c(2_500_000_000.0), "2.5B");
        assert_eq!(c(1.2e15), "1.2Q");
        assert_eq!(c(18_000.0), "18K");
        assert_eq!(count(None), "");
    }

    #[test]
    fn gives_short_ages() {
        let now = 1_000_000_000_000.0;
        assert_eq!(age(Some(now - 30_000.0), now), "30s");
        assert_eq!(age(Some(now - 14.0 * 60_000.0), now), "14m");
        assert_eq!(age(Some(now - 5.0 * 3_600_000.0), now), "5h");
        assert_eq!(age(Some(now - 3.0 * 86_400_000.0), now), "3d");
    }

    #[test]
    fn short_name_shortens_a_bare_address_used_as_a_name() {
        assert_eq!(short_name(&format!("0x{}", "ab".repeat(32))), "0xabab\u{2060}…\u{2060}abab");
    }

    #[test]
    fn short_name_shortens_a_coin_type() {
        assert_eq!(short_name(&format!("0x{}::foo::FOO", "cd".repeat(32))), "0xcdcd\u{2060}…\u{2060}cdcd");
    }

    #[test]
    fn short_name_keeps_an_already_short_address_on_one_line() {
        assert_eq!(short_name("@0x9a5b…5f2e"), "@0x9a5b\u{2060}…\u{2060}5f2e");
    }

    #[test]
    fn short_name_keeps_real_names() {
        assert_eq!(short_name("degen"), "degen");
        assert_eq!(short_name("Big Whale 0x"), "Big Whale 0x");
    }

    #[test]
    fn usd_price_fine_shows_per_second_moves_a_normal_price_label_rounds_away() {
        assert_eq!(usd_price_fine(Some(86_597.34)), "$86,597.34");
        assert_eq!(usd_price(Some(81.2341)), usd_price(Some(81.2344)));
        assert_ne!(usd_price_fine(Some(81.2341)), usd_price_fine(Some(81.2344)));
        assert_ne!(usd_price_fine(Some(0.02421)), usd_price_fine(Some(0.02424)));
        assert_eq!(usd_price_fine(Some(0.0)), "");
    }
}
