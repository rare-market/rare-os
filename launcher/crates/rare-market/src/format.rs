//! Number, price, address and time formatting as rare.market does it (English only). Ported from
//! launcher/src/marketplace/format.ts. Amounts arrive as MIST decimal strings.

use crate::jsnum::{locale, to_fixed};

pub const DASH: &str = "--";
const MIST_PER_SUI: i128 = 1_000_000_000;

/// `toBigInt` for a MIST string: an optional minus and digits only.
pub fn to_big(v: Option<&str>) -> Option<i128> {
    let s = v?;
    let d = s.strip_prefix('-').unwrap_or(s);
    if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// MIST to SUI as a float. Display only.
pub fn mist_to_sui(v: Option<&str>) -> Option<f64> {
    to_big(v).map(big_to_sui)
}

/// The same for an amount already parsed (`Number(b / MIST) + Number(b % MIST) / 1e9`).
pub fn big_to_sui(b: i128) -> f64 {
    (b / MIST_PER_SUI) as f64 + (b % MIST_PER_SUI) as f64 / 1e9
}

fn trim_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    t.strip_suffix('.').unwrap_or(t).to_string()
}

#[derive(Clone, Copy, Debug)]
pub struct SuiOpts {
    /// Fixed decimals (cards 3).
    pub dp: usize,
    /// Abbreviates ("16.571K").
    pub compact: bool,
    pub trim: bool,
}

impl Default for SuiOpts {
    fn default() -> Self {
        SuiOpts { dp: 3, compact: false, trim: false }
    }
}

/// A SUI amount from MIST. Unknown is "--".
pub fn fmt_sui(v: Option<&str>, o: SuiOpts) -> String {
    mist_to_sui(v).map_or_else(|| DASH.to_string(), |n| fmt_sui_num(n, o))
}

/// [`fmt_sui`] for a SUI amount already in SUI.
pub fn fmt_sui_num(n: f64, o: SuiOpts) -> String {
    let a = n.abs();
    if o.compact && a >= 1_000_000.0 {
        return format!("{}M", trim_zeros(&to_fixed(n / 1_000_000.0, 2)));
    }
    if o.compact && a >= 10_000.0 {
        return format!("{}K", trim_zeros(&to_fixed(n / 1_000.0, 3)));
    }
    let min = if o.trim {
        0
    } else if n != 0.0 && a < 1000.0 {
        o.dp
    } else {
        o.dp.min(2)
    };
    let max = if a >= 1000.0 { o.dp.min(2) } else { o.dp };
    locale(n, min, max)
}

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
    format!("${}", locale(v, d, d))
}

pub fn mist_to_usd(v: Option<&str>, sui_usd: Option<f64>) -> Option<f64> {
    let n = mist_to_sui(v)?;
    match sui_usd {
        Some(u) if u != 0.0 && !u.is_nan() => Some(n * u),
        _ => None,
    }
}

/// `Math.round`: halves go toward +infinity.
pub fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

pub fn fmt_int(v: Option<f64>, compact: bool) -> String {
    let Some(v) = v.filter(|v| v.is_finite()) else { return DASH.into() };
    if compact && v.abs() >= 1_000_000.0 {
        return format!("{}M", trim_zeros(&to_fixed(v / 1_000_000.0, 2)));
    }
    if compact && v.abs() >= 10_000.0 {
        return format!("{}K", trim_zeros(&to_fixed(v / 1_000.0, 2)));
    }
    locale(js_round(v), 0, 3)
}

pub fn fmt_pct(v: Option<f64>, dp: usize) -> String {
    match v.filter(|v| v.is_finite()) {
        Some(v) => format!("{}%", to_fixed(v.abs(), dp)),
        None => DASH.into(),
    }
}

/// "0x7zcR…yUg4" style, 4 + 4 after the 0x.
pub fn short_addr(addr: Option<&str>) -> String {
    let Some(addr) = addr.filter(|a| !a.is_empty()) else { return DASH.into() };
    let a = addr.strip_prefix("0x").unwrap_or(addr);
    let c: Vec<char> = a.chars().collect();
    if c.len() <= 10 {
        return addr.to_string();
    }
    format!("0x{}…{}", c[..4].iter().collect::<String>(), c[c.len() - 4..].iter().collect::<String>())
}

/// Terse relative time: "20s", "2h", "1d", "12mo", "2y". `at` is an ISO string.
pub fn time_ago(at: Option<&str>, now_ms: i64) -> String {
    match at.filter(|s| !s.is_empty()).and_then(crate::time::parse_iso_ms) {
        Some(t) => time_ago_ms(t, now_ms),
        None => DASH.into(),
    }
}

/// [`time_ago`] from ms since the epoch.
pub fn time_ago_ms(t: i64, now_ms: i64) -> String {
    let s = js_round((now_ms - t) as f64 / 1000.0).max(0.0) as i64;
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

/// "#7409" from "Collection Name #7409".
pub fn short_name<'a>(name: &'a str, collection: Option<&str>) -> &'a str {
    match collection {
        Some(c) => {
            let prefix = format!("{c} ");
            name.strip_prefix(prefix.as_str()).unwrap_or(name)
        }
        None => name,
    }
}

/// Venue label and icon (paths under the launcher's public assets).
pub const VENUES: [(&str, &str, &str); 4] = [("market", "Rare", "/venues/market.png"), ("tradeport", "Tradeport", "/venues/tradeport.svg"), ("drip", "Drip", "/venues/drip.svg"), ("kiosk", "Kiosk", "/venues/kiosk.svg")];

pub fn venue_name(v: Option<&str>) -> String {
    match v.filter(|s| !s.is_empty()) {
        Some(v) => VENUES.iter().find(|(k, _, _)| *k == v).map(|(_, l, _)| l.to_string()).unwrap_or_else(|| v.to_string()),
        None => String::new(),
    }
}

pub fn venue_icon(v: &str) -> Option<&'static str> {
    VENUES.iter().find(|(k, _, _)| *k == v).map(|(_, _, i)| *i)
}

pub fn event_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "sale" => "Sale",
        "list" => "List",
        "delist" => "Delist",
        "offer" => "Offer",
        "offer_cancel" => "Offer cancel",
        "transfer" => "Transfer",
        "mint" => "Mint",
        _ => return None,
    })
}

pub fn tier_label(t: &str) -> Option<&'static str> {
    Some(match t {
        "common" => "Common",
        "uncommon" => "Uncommon",
        "rare" => "Rare",
        "epic" => "Epic",
        "legendary" => "Legendary",
        "mythic" => "Mythic",
        _ => return None,
    })
}

/// The price a buyer pays for a listing: all-in when the API knows it.
pub fn all_in(l: Option<&crate::api::Listing>) -> Option<&str> {
    l.map(|l| l.all_in_mist.as_deref().unwrap_or(&l.price_mist))
}

pub fn item_url(id: &str) -> String {
    format!("https://rare.market/item/{id}")
}

pub fn collection_url(slug: &str) -> String {
    format!("https://rare.market/collection/{slug}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sui_amounts_like_the_web() {
        let s = |v: &str, o: SuiOpts| fmt_sui(Some(v), o);
        assert_eq!(s("1234567000", SuiOpts::default()), "1.235");
        assert_eq!(s("0", SuiOpts::default()), "0.00");
        assert_eq!(s("16571000000000", SuiOpts { compact: true, ..Default::default() }), "16.571K");
        assert_eq!(s("2500000000000000", SuiOpts { compact: true, ..Default::default() }), "2.5M");
        assert_eq!(s("1500000000000", SuiOpts::default()), "1,500.00");
        assert_eq!(s("1500000000", SuiOpts { trim: true, ..Default::default() }), "1.5");
        assert_eq!(fmt_sui(Some("x"), SuiOpts::default()), DASH);
        assert_eq!(fmt_sui(None, SuiOpts::default()), DASH);
    }

    #[test]
    fn usd_ints_pcts_addrs() {
        assert_eq!(fmt_usd(Some(3.456)), "$3.46");
        assert_eq!(fmt_usd(Some(1234.5)), "$1,235");
        assert_eq!(fmt_usd(Some(12_345.0)), "$12.3K");
        assert_eq!(fmt_usd(Some(2_000_000.0)), "$2M");
        assert_eq!(fmt_usd(None), DASH);
        assert_eq!(fmt_int(Some(12_345.0), true), "12.35K");
        assert_eq!(fmt_int(Some(1234.5), false), "1,235");
        assert_eq!(fmt_pct(Some(-12.345), 1), "12.3%");
        assert_eq!(short_addr(Some("0x7682c8538863a72d81f09c1673d50ffda1220d5ec680f7f7f0c527eed0324fac")), "0x7682…4fac");
        assert_eq!(short_addr(Some("0x1234")), "0x1234");
    }

    #[test]
    fn relative_times_and_names() {
        let now = 1_000_000_000_000;
        assert_eq!(time_ago_ms(now - 20_000, now), "20s");
        assert_eq!(time_ago_ms(now - 2 * 3_600_000, now), "2h");
        assert_eq!(time_ago_ms(now - 400 * 86_400_000, now), "1y");
        assert_eq!(time_ago_ms(now - 40 * 86_400_000, now), "1mo");
        assert_eq!(time_ago(Some("garbage"), now), DASH);
        assert_eq!(short_name("RetroSui #7409", Some("RetroSui")), "#7409");
        assert_eq!(venue_name(Some("tradeport")), "Tradeport");
        assert_eq!(venue_name(Some("other")), "other");
    }
}
