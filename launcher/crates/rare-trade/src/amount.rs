//! Amounts for the controller: exact base-unit math, the preset rows, and the ladders LT and RT
//! step through. Base units are `u128` (the web launcher's bigint): coin balances on Sui are u64,
//! so products of two of them still fit.

use crate::jsnum::{js_number, js_round, js_string, locale, to_fixed, Locale};

pub const SUI_TYPE: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
pub const USDC_TYPE: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";

/// SUI kept back for gas when buying with "Max".
pub const GAS_RESERVE_MIST: u128 = 50_000_000;

/// Buy amounts in SUI.
pub const BUY_PRESETS: [f64; 5] = [1.0, 5.0, 10.0, 25.0, 50.0];
pub const SUI_LADDER: [f64; 20] = [0.1, 0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 15.0, 25.0, 50.0, 75.0, 100.0, 150.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0];
/// Sell amounts as a share of the balance.
pub const SELL_PRESETS: [f64; 4] = [25.0, 50.0, 75.0, 100.0];
pub const PCT_LADDER: [f64; 14] = [5.0, 10.0, 15.0, 20.0, 25.0, 33.0, 40.0, 50.0, 60.0, 66.0, 75.0, 80.0, 90.0, 100.0];
/// Perps margin in USD.
pub const MARGIN_PRESETS: [f64; 5] = [5.0, 10.0, 25.0, 50.0, 100.0];
pub const USD_LADDER: [f64; 17] = [1.0, 2.0, 5.0, 10.0, 15.0, 20.0, 25.0, 50.0, 75.0, 100.0, 150.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0];
pub const LEVERAGE_LADDER: [f64; 18] = [1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 75.0, 100.0];

/// Slippage the swap is built with, in basis points.
pub const SLIPPAGE_BPS: u32 = 100;

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `0x<hex>::module::Name`, no type parameters.
pub fn is_coin_type(t: &str) -> bool {
    let Some(rest) = t.strip_prefix("0x") else { return false };
    let mut it = rest.split("::");
    let (Some(addr), Some(m), Some(n), None) = (it.next(), it.next(), it.next(), it.next()) else { return false };
    !addr.is_empty() && addr.bytes().all(|b| b.is_ascii_hexdigit()) && is_word(m) && is_word(n)
}

/// "1.5" with 9 decimals -> 1500000000. Digits past `decimals` are dropped, never rounded up. Bad
/// input is zero; an amount too large for u128 saturates.
pub fn to_base(amount: &str, decimals: u32) -> u128 {
    let s = amount.trim();
    if s.is_empty() || s == "." {
        return 0;
    }
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    let mut digits = String::from(if whole.is_empty() { "0" } else { whole });
    let d = decimals as usize;
    digits.push_str(&frac[..frac.len().min(d)]);
    for _ in frac.len().min(d)..d {
        digits.push('0');
    }
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return 0;
    }
    digits.parse::<u128>().unwrap_or(u128::MAX)
}

/// [`to_base`] for a number (the web launcher's `toBase(number)`).
pub fn to_base_num(amount: f64, decimals: u32) -> u128 {
    to_base(&number_string(amount), decimals)
}

/// A plain decimal string for a number (no exponent).
fn number_string(n: f64) -> String {
    if !n.is_finite() || n <= 0.0 {
        return "0".into();
    }
    locale(n, Locale::max_frac(20).no_grouping())
}

/// 1500000000 with 9 decimals -> "1.5". Exact, no trailing zeros.
pub fn from_base(raw: u128, decimals: u32) -> String {
    let d = decimals as usize;
    let s = format!("{raw:0>width$}", width = d + 1);
    let (whole, frac) = s.split_at(s.len() - d);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{frac}")
    }
}

/// [`from_base`] for a signed amount (balance changes).
pub fn from_base_signed(raw: i128, decimals: u32) -> String {
    let body = from_base(raw.unsigned_abs(), decimals);
    if raw < 0 {
        format!("-{body}")
    } else {
        body
    }
}

pub fn base_to_number(raw: u128, decimals: u32) -> f64 {
    js_number(&from_base(raw, decimals))
}

/// The next value on a ladder from `current`, up (+1) or down (-1). It never passes `max`: the step
/// that would cross it lands on `max` itself, so the top of the range is always reachable. `min`
/// works the same way at the bottom. Pass `f64::INFINITY` and 0 for no bounds.
pub fn step(ladder: &[f64], current: f64, dir: i32, max: f64, min: f64) -> f64 {
    let top = min.max(max);
    if dir > 0 {
        return match ladder.iter().copied().find(|&v| v > current + 1e-12) {
            Some(next) if next <= top => next,
            _ => {
                if current < top && top.is_finite() {
                    top
                } else {
                    current
                }
            }
        };
    }
    match ladder.iter().rev().copied().find(|&v| v < current - 1e-12 && v >= min) {
        Some(prev) => prev.min(top),
        None => {
            if current > min && min > 0.0 {
                min
            } else {
                current
            }
        }
    }
}

/// What can be spent of a balance: SUI keeps the gas reserve back.
pub fn spendable(raw: u128, coin_type: &str) -> u128 {
    if coin_type != SUI_TYPE {
        return raw;
    }
    raw.saturating_sub(GAS_RESERVE_MIST)
}

/// `pct` percent of `raw`, exact at 100.
pub fn pct_of(raw: u128, pct: f64) -> u128 {
    if pct >= 100.0 {
        return raw;
    }
    // NaN counts as nothing too.
    if pct.is_nan() || pct <= 0.0 {
        return 0;
    }
    let bp = js_round(pct * 100.0) as u128;
    match raw.checked_mul(bp) {
        Some(x) => x / 10_000,
        None => raw / 10_000 * bp,
    }
}

/// The leverage stops for a market whose maximum is `max`.
pub fn leverage_stops(max: Option<f64>) -> Vec<f64> {
    let m = max.filter(|m| *m > 0.0).unwrap_or(1.0);
    let mut stops: Vec<f64> = LEVERAGE_LADDER.iter().copied().filter(|v| *v <= m).collect();
    if stops.last() != Some(&m) {
        stops.push(m);
    }
    stops
}

/// "5x", "1.5x"
pub fn fmt_leverage(x: f64) -> String {
    format!("{}x", js_string(js_number(&to_fixed(x, 1))))
}

/// The least the trader receives at a slippage, base units.
pub fn min_out(amount_out: u128, slippage_bps: u32) -> u128 {
    let keep = 10_000u128.saturating_sub(slippage_bps as u128);
    match amount_out.checked_mul(keep) {
        Some(x) => x / 10_000,
        None => amount_out / 10_000 * keep,
    }
}

/// Price of one `coinOut` in `coinIn` units implied by a quote.
pub fn implied_price(amount_in: u128, dec_in: u32, amount_out: u128, dec_out: u32) -> Option<f64> {
    if amount_in == 0 || amount_out == 0 {
        return None;
    }
    Some(base_to_number(amount_in, dec_in) / base_to_number(amount_out, dec_out))
}

pub fn valid_pair(coin_in: &str, coin_out: &str) -> bool {
    is_coin_type(coin_in) && is_coin_type(coin_out) && normalize_type(coin_in) != normalize_type(coin_out)
}

/// 0x2::sui::SUI -> the long form, so types compare equal. Anything else comes back unchanged.
pub fn normalize_type(t: &str) -> String {
    let s = t.trim();
    if let Some(rest) = s.strip_prefix("0x") {
        let hex = rest.bytes().take_while(|b| b.is_ascii_hexdigit()).count();
        let tail = &rest[hex..];
        if hex > 0 && tail.starts_with("::") && tail.len() > 2 {
            return format!("0x{:0>64}{tail}", rest[..hex].to_ascii_lowercase());
        }
    }
    t.to_string()
}

/// Every `0x<hex>` address followed by `::` in a type tag, long and lower case (`normalizeStructTag`
/// for the tags this crate compares: `0x2::coin::Coin<0x2::sui::SUI>`).
pub fn normalize_struct_tag(t: &str) -> String {
    let b = t.trim().as_bytes();
    let mut out = String::with_capacity(b.len() + 64);
    let mut i = 0;
    while i < b.len() {
        let at_start = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if at_start && b[i] == b'0' && b.get(i + 1) == Some(&b'x') {
            let hex = b[i + 2..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
            if hex > 0 && b[i + 2 + hex..].starts_with(b"::") {
                let h = std::str::from_utf8(&b[i + 2..i + 2 + hex]).unwrap_or("").to_ascii_lowercase();
                out.push_str(&format!("0x{h:0>64}"));
                i += 2 + hex;
                continue;
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    //! amount.test.ts
    use super::*;
    const INF: f64 = f64::INFINITY;

    #[test]
    fn converts_whole_token_amounts_exactly() {
        assert_eq!(to_base("1.5", 9), 1_500_000_000);
        assert_eq!(to_base("0.000000001", 9), 1);
        assert_eq!(to_base("25", 6), 25_000_000);
        assert_eq!(to_base_num(0.1, 9), 100_000_000);
        assert_eq!(to_base_num(1e-7, 9), 100);
    }

    #[test]
    fn drops_digits_past_the_coins_decimals_instead_of_rounding_up() {
        assert_eq!(to_base("1.0000000019", 9), 1_000_000_001);
        assert_eq!(to_base("0.1234567", 6), 123_456);
    }

    #[test]
    fn treats_bad_input_as_zero() {
        assert_eq!(to_base("", 9), 0);
        assert_eq!(to_base(".", 9), 0);
        assert_eq!(to_base("1e3", 9), 0);
        assert_eq!(to_base("-1", 9), 0);
        assert_eq!(to_base_num(f64::NAN, 9), 0);
    }

    #[test]
    fn prints_base_units_back_without_trailing_zeros() {
        assert_eq!(from_base(1_500_000_000, 9), "1.5");
        assert_eq!(from_base(1, 9), "0.000000001");
        assert_eq!(from_base(25_000_000, 6), "25");
        assert_eq!(from_base(0, 9), "0");
        assert_eq!(from_base_signed(-2_500_000, 6), "-2.5");
        assert_eq!(from_base(7, 0), "7");
    }

    #[test]
    fn round_trips() {
        for s in ["0.1", "12.345678", "1000000", "0.000001"] {
            assert_eq!(from_base(to_base(s, 6), 6), s);
        }
    }

    #[test]
    fn walks_the_ladder_both_ways() {
        assert_eq!(step(&SUI_LADDER, 1.0, 1, INF, 0.0), 2.0);
        assert_eq!(step(&SUI_LADDER, 2.0, -1, INF, 0.0), 1.0);
        assert_eq!(step(&SUI_LADDER, 1.3, 1, INF, 0.0), 2.0);
        assert_eq!(step(&SUI_LADDER, 1.3, -1, INF, 0.0), 1.0);
    }

    #[test]
    fn lands_on_the_maximum_instead_of_passing_it() {
        assert_eq!(step(&SUI_LADDER, 10.0, 1, 12.4, 0.0), 12.4);
        assert_eq!(step(&SUI_LADDER, 12.4, 1, 12.4, 0.0), 12.4);
        assert_eq!(step(&SUI_LADDER, 12.4, -1, 12.4, 0.0), 10.0);
    }

    #[test]
    fn stops_at_the_ends() {
        assert_eq!(step(&SUI_LADDER, 0.1, -1, INF, 0.0), 0.1);
        assert_eq!(step(&SUI_LADDER, 10000.0, 1, INF, 0.0), 10000.0);
        assert_eq!(step(&[1.0, 2.0, 3.0], 2.0, -1, INF, 1.5), 1.5);
    }

    #[test]
    fn clamps_a_step_down_to_the_maximum_when_the_balance_shrank() {
        assert_eq!(step(&SUI_LADDER, 100.0, -1, 20.0, 0.0), 20.0);
    }

    #[test]
    fn keeps_gas_back_on_sui_only() {
        assert_eq!(spendable(1_000_000_000, SUI_TYPE), 1_000_000_000 - GAS_RESERVE_MIST);
        assert_eq!(spendable(10_000_000, SUI_TYPE), 0);
        assert_eq!(spendable(5_000_000, USDC_TYPE), 5_000_000);
    }

    #[test]
    fn takes_exact_shares() {
        assert_eq!(pct_of(1_000, 25.0), 250);
        assert_eq!(pct_of(999, 100.0), 999);
        assert_eq!(pct_of(1_000, 33.0), 330);
        assert_eq!(pct_of(1_000, 0.0), 0);
    }

    #[test]
    fn offers_the_ladder_up_to_the_markets_maximum_maximum_included() {
        assert_eq!(leverage_stops(Some(5.0)), vec![1.0, 1.5, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(*leverage_stops(Some(15.0)).last().unwrap(), 15.0);
        assert_eq!(leverage_stops(Some(7.0)), vec![1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(leverage_stops(None), vec![1.0]);
    }

    #[test]
    fn prints_leverage_compactly() {
        assert_eq!(fmt_leverage(5.0), "5x");
        assert_eq!(fmt_leverage(1.5), "1.5x");
    }

    #[test]
    fn applies_slippage_to_the_minimum_received() {
        assert_eq!(min_out(10_000, 100), 9_900);
        assert_eq!(min_out(90_459, SLIPPAGE_BPS), 89_554);
    }

    #[test]
    fn derives_a_price_from_a_quote() {
        assert_eq!(implied_price(1_000_000_000, 9, 2_000_000, 6), Some(0.5));
        assert_eq!(implied_price(0, 9, 1, 6), None);
    }

    #[test]
    fn compares_coin_types_in_their_long_form() {
        assert_eq!(normalize_type("0x2::sui::SUI"), SUI_TYPE);
        assert!(!valid_pair("0x2::sui::SUI", SUI_TYPE));
        assert!(valid_pair("0x2::sui::SUI", USDC_TYPE));
        assert!(!valid_pair("sui", USDC_TYPE));
    }

    #[test]
    fn struct_tags_normalise_inside_type_parameters() {
        assert_eq!(normalize_struct_tag("0x2::coin::Coin<0x2::sui::SUI>"), format!("{}::coin::Coin<{SUI_TYPE}>", &SUI_TYPE[..66]));
    }
}
