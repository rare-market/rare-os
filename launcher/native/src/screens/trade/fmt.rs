//! The Trade screens' own figures and ladders: trade/amount.ts (presets, ladders, `step`,
//! `leverageStops`, `fmtLeverage`), trade/perps.ts (`perpPx`, `fundingPct`, `liqText`,
//! `fundingPlan`) and PerpPage's `numSize` and `untilText`. When rare-trade lands with these, the
//! screens switch to it.

/// The kit's `grouped` after JS rounding (ties away from zero), as toLocaleString does.
pub fn grouped(v: f64, min: usize, max: usize) -> String {
    crate::kit::format::grouped(round_dp(v, max), min, max)
}

/// Rounds half away from zero at `d` decimals (JS toFixed and Intl round ties up in magnitude;
/// Rust's formatter rounds exact ties to even).
pub fn round_dp(v: f64, d: usize) -> f64 {
    let k = 10f64.powi(d as i32);
    let x = (v * k).round() / k;
    if x.is_finite() {
        x
    } else {
        v
    }
}

/// JS `toFixed`.
pub fn to_fixed(v: f64, d: usize) -> String {
    format!("{:.*}", d, round_dp(v, d))
}

pub fn trim_zeros(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text.to_string()
    }
}

/// `toLocaleString("en-US", { maximumSignificantDigits })`, grouped.
pub fn sig_grouped(v: f64, sig: usize) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let mag = v.abs().log10().floor() as i32;
    let decimals = (sig as i32 - 1 - mag).max(0) as usize;
    grouped(round_dp(v, decimals), 0, decimals)
}

/// `Number(x.toPrecision(p))`.
pub fn precision_num(v: f64, p: usize) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return 0.0;
    }
    let mag = v.abs().log10().floor() as i32;
    let shift = p as i32 - 1 - mag;
    if shift < 0 {
        let k = 10f64.powi(-shift);
        (v / k).round() * k
    } else {
        round_dp(v, shift as usize)
    }
}

/// trade/format.ts `count`: exact under 1,000, then compact (901, 23.4K).
pub fn count(value: Option<f64>) -> String {
    let Some(v) = value.filter(|v| v.is_finite()) else { return String::new() };
    let abs = v.abs();
    let body = if abs < 999.5 {
        format!("{}", abs.round() as i64)
    } else if abs < 999_950.0 {
        let t = to_fixed(abs / 1000.0, 1);
        format!("{}K", t.strip_suffix(".0").unwrap_or(&t))
    } else {
        crate::kit::format::compact_figure(abs)
    };
    format!("{}{}", if v < 0.0 { "\u{2212}" } else { "" }, body)
}

/// trade/format.ts `age`: 3s, 14m, 5h, 2d, 3mo, 1y.
pub fn age(ms: Option<f64>, now_ms: f64) -> String {
    let Some(ms) = ms.filter(|m| *m != 0.0) else { return String::new() };
    let s = ((now_ms - ms) / 1000.0).max(0.0);
    if s < 60.0 {
        format!("{}s", s.floor())
    } else if s < 3600.0 {
        format!("{}m", (s / 60.0).floor())
    } else if s < 86400.0 {
        format!("{}h", (s / 3600.0).floor())
    } else if s < 86400.0 * 60.0 {
        format!("{}d", (s / 86400.0).floor())
    } else if s < 86400.0 * 365.0 {
        format!("{}mo", (s / (86400.0 * 30.0)).floor())
    } else {
        format!("{}y", (s / (86400.0 * 365.0)).floor())
    }
}

/// trade/format.ts `shortAddr`: 0x1234…abcd.
pub fn short_addr(addr: &str) -> String {
    if addr.is_empty() {
        return String::new();
    }
    let c: Vec<char> = addr.chars().collect();
    let head: String = c.iter().take(6).collect();
    let tail: String = c[c.len().saturating_sub(4)..].iter().collect();
    format!("{head}…{tail}")
}

pub const BUY_PRESETS: [f64; 5] = [1.0, 5.0, 10.0, 25.0, 50.0];
pub const SELL_PRESETS: [f64; 4] = [25.0, 50.0, 75.0, 100.0];
pub const MARGIN_PRESETS: [f64; 5] = [5.0, 10.0, 25.0, 50.0, 100.0];
pub const SUI_LADDER: [f64; 20] = [0.1, 0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 15.0, 25.0, 50.0, 75.0, 100.0, 150.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0];
pub const PCT_LADDER: [f64; 14] = [5.0, 10.0, 15.0, 20.0, 25.0, 33.0, 40.0, 50.0, 60.0, 66.0, 75.0, 80.0, 90.0, 100.0];
pub const USD_LADDER: [f64; 17] = [1.0, 2.0, 5.0, 10.0, 15.0, 20.0, 25.0, 50.0, 75.0, 100.0, 150.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0];
pub const LEVERAGE_LADDER: [f64; 18] = [1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 75.0, 100.0];

/// Candle resolutions the perps chart offers: (label, API resolution, window in ms).
pub const PERP_TIMEFRAMES: [(&str, &str, f64); 5] = [("5m", "5", 2.0 * 86_400_000.0), ("15m", "15", 6.0 * 86_400_000.0), ("1h", "60", 21.0 * 86_400_000.0), ("4h", "240", 90.0 * 86_400_000.0), ("1d", "1D", 400.0 * 86_400_000.0)];

/// The next value on a ladder, up (+1) or down (-1); never past `max` (lands on it), `min` alike.
pub fn step(ladder: &[f64], current: f64, dir: i32, max: f64, min: f64) -> f64 {
    let top = min.max(max);
    if dir > 0 {
        return match ladder.iter().copied().find(|v| *v > current + 1e-12) {
            Some(n) if n <= top => n,
            _ => {
                if current < top && top.is_finite() {
                    top
                } else {
                    current
                }
            }
        };
    }
    match ladder.iter().rev().copied().find(|v| *v < current - 1e-12 && *v >= min) {
        Some(p) => p.min(top),
        None => {
            if current > min && min > 0.0 {
                min
            } else {
                current
            }
        }
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

/// "5x", "1.5x".
pub fn fmt_leverage(x: f64) -> String {
    format!("{}x", trim_zeros(&to_fixed(x, 1)))
}

/// A perps price: 2 decimals from $100, up to 4 from $1, else 5 significant digits; "" if unknown.
pub fn perp_px(v: Option<f64>) -> String {
    let Some(v) = v.filter(|v| v.is_finite() && *v > 0.0) else { return String::new() };
    let body = if v >= 100.0 {
        grouped(v, 2, 2)
    } else if v >= 1.0 {
        grouped(v, 0, 4)
    } else {
        sig_grouped(v, 5)
    };
    format!("${body}")
}

/// Funding as a percent over `hours`.
pub fn funding_pct(rate: f64, hours: f64) -> String {
    let pct = rate * hours * 100.0;
    let abs = pct.abs();
    let digits = if abs >= 1.0 {
        2
    } else if abs >= 0.01 {
        4
    } else if abs >= 0.001 {
        5
    } else {
        6
    };
    format!("{}%", to_fixed(pct, digits))
}

/// A liquidation price, or plain words when it is effectively unreachable (a 1x long).
pub fn liq_text(liq: Option<f64>, entry: f64) -> String {
    match liq.filter(|l| l.is_finite()) {
        None => String::new(),
        Some(l) if l <= entry * 1e-3 => "Near $0".into(),
        Some(l) => perp_px(Some(l)),
    }
}

/// What an order does about money: nothing, top up the perps account from the wallet, or stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FundingPlan {
    pub short_by: f64,
    pub blocked: bool,
    pub needs_account: bool,
}

/// `account`: None = not loaded, Some(None) = no account, Some(Some(available)).
pub fn funding_plan(account: Option<Option<f64>>, margin_usd: f64, fee_usd: f64, wallet_usdc: Option<f64>) -> FundingPlan {
    let available = account.flatten().unwrap_or(0.0);
    let short_by = (((margin_usd * 1.02 + fee_usd - available) * 100.0).ceil() / 100.0).max(0.0);
    let blocked = short_by > 0.0 && wallet_usdc.is_some_and(|w| w + 1e-9 < short_by);
    FundingPlan { short_by, blocked, needs_account: account == Some(None) }
}

/// PerpPage's `numSize`: whole above 1,000, 3 decimals above 1, else 6.
pub fn num_size(v: f64) -> String {
    if v >= 1000.0 {
        grouped(v, 0, 0)
    } else {
        grouped(v, 0, if v >= 1.0 { 3 } else { 6 })
    }
}

/// "2h 5m" or "11m" until `ms`.
pub fn until_text(ms: f64, now_ms: f64) -> String {
    let s = ((ms - now_ms) / 1000.0).round().max(0.0) as i64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// amount.ts `normalizeType`: `0x2::sui::SUI` -> the long form, so types compare equal.
pub fn normalize_type(t: &str) -> String {
    let t = t.trim();
    if let Some(rest) = t.strip_prefix("0x") {
        if let Some(i) = rest.find("::") {
            let (hex, tail) = rest.split_at(i);
            if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return format!("0x{:0>64}{}", hex.to_ascii_lowercase(), tail);
            }
        }
    }
    t.to_string()
}

/// "BTCUSD" -> "BTC/USD".
pub fn pair_label(pair: &str) -> String {
    match pair.strip_suffix("USD") {
        Some(base) => format!("{base}/USD"),
        None => pair.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladders_step_and_stop_at_max() {
        let stops = leverage_stops(Some(25.0));
        assert_eq!(stops.last(), Some(&25.0));
        assert_eq!(stops.len(), 13);
        assert_eq!(step(&stops, 5.0, 1, 25.0, 1.0), 6.0);
        assert_eq!(step(&stops, 25.0, 1, 25.0, 1.0), 25.0);
        assert_eq!(step(&stops, 1.0, -1, 25.0, 1.0), 1.0);
        assert_eq!(leverage_stops(Some(7.0)), vec![1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(step(&USD_LADDER, 9000.0, 1, 9500.0, 0.0), 9500.0);
    }

    #[test]
    fn perp_figures() {
        assert_eq!(perp_px(Some(80_504.17)), "$80,504.17");
        assert_eq!(perp_px(Some(2.972717)), "$2.9727");
        assert_eq!(perp_px(Some(0.92213)), "$0.92213");
        assert_eq!(perp_px(Some(0.0)), "");
        assert_eq!(fmt_leverage(5.0), "5x");
        assert_eq!(fmt_leverage(1.5), "1.5x");
        assert_eq!(funding_pct(7.01e-5, 1.0), "0.00701%");
        assert_eq!(funding_pct(0.0125, 1.0), "1.25%");
        assert_eq!(liq_text(Some(66_054.700_3), 80_508.0), "$66,054.70");
        assert_eq!(liq_text(Some(1.0), 80_508.0), "Near $0");
        assert_eq!(num_size(0.000621475), "0.000621");
        assert_eq!(num_size(1234.5), "1,235");
        assert_eq!(until_text(1_793_124_000_000.0, 1_793_123_919_842.0), "1m");
        assert_eq!(pair_label("BTCUSD"), "BTC/USD");
        assert_eq!(count(Some(40_000.0)), "40K");
        assert_eq!(count(Some(24_100.0)), "24.1K");
        assert_eq!(precision_num(24_123.0, 3), 24_100.0);
        assert_eq!(sig_grouped(0.000123456, 5), "0.00012346");
        assert_eq!(short_addr("0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e"), "0xa049…887e");
        assert_eq!(age(Some(1_000.0), 61_000.0), "1m");
        assert_eq!(normalize_type("0x2::sui::SUI"), "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI");
        assert_eq!(normalize_type("SUI"), "SUI");
    }

    #[test]
    fn funding_plans() {
        let p = funding_plan(Some(None), 10.0, 0.05, Some(0.0012));
        assert!(p.needs_account);
        assert!(p.blocked);
        assert_eq!(p.short_by, 10.25);
        let ok = funding_plan(Some(Some(50.0)), 10.0, 0.05, None);
        assert_eq!(ok, FundingPlan { short_by: 0.0, blocked: false, needs_account: false });
        assert!(!funding_plan(None, 10.0, 0.0, None).needs_account);
    }
}
