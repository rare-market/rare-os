//! A swap ticket for the pad (trade/ticket.ts and the amount helpers of amount.ts): the amount in
//! coin units or as a share of the balance, the balance it draws from, LT/RT stepping, presets, and
//! the quote that follows the amount (the app fetches it; the ticket says what to ask for).

use super::fmt::{normalize_type, step, PCT_LADDER, SUI_LADDER, USD_LADDER};

pub const SUI_TYPE: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
/// SUI kept back for gas when buying with "Max".
pub const GAS_RESERVE_MIST: u128 = 50_000_000;
/// Slippage the swap is built with, in basis points.
pub const SLIPPAGE_BPS: u128 = 100;

#[derive(Clone, Debug, PartialEq)]
pub struct Coin {
    pub coin_type: String,
    pub symbol: String,
    pub decimals: u32,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
}

impl Coin {
    pub fn sui(price_usd: Option<f64>, logo: Option<String>) -> Coin {
        Coin { coin_type: SUI_TYPE.into(), symbol: "SUI".into(), decimals: 9, logo, price_usd }
    }
    pub fn is_sui(&self) -> bool {
        normalize_type(&self.coin_type) == SUI_TYPE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Units,
    Pct,
}

/// A live quote (swap.ts SwapQuote), base units.
#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub amount_in: u128,
    pub amount_out: u128,
    pub fee_bps: u32,
    pub hops: u32,
    pub venues: Vec<String>,
    pub expires_at: f64,
}

/// "1.5" with 9 decimals -> 1_500_000_000; digits past `decimals` are dropped, never rounded up.
pub fn to_base(amount: f64, decimals: u32) -> u128 {
    if !amount.is_finite() || amount <= 0.0 {
        return 0;
    }
    // The shortest decimal that reads back as this number (what JS prints), not its binary tail.
    to_base_str(&format!("{amount}"), decimals)
}

/// A decimal string ("4.600000") in base units; anything that is not a plain decimal is 0.
pub fn to_base_str(s: &str, decimals: u32) -> u128 {
    let s = s.trim();
    if s.is_empty() || s == "." || !s.chars().all(|c| c.is_ascii_digit() || c == '.') || s.matches('.').count() > 1 {
        return 0;
    }
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let mut frac: String = frac.chars().take(decimals as usize).collect();
    while frac.len() < decimals as usize {
        frac.push('0');
    }
    format!("{}{}", if whole.is_empty() { "0" } else { whole }, frac).parse().unwrap_or(0)
}

/// 1_500_000_000 with 9 decimals -> "1.5". Exact, no trailing zeros.
pub fn from_base(raw: u128, decimals: u32) -> String {
    let s = format!("{:0>width$}", raw, width = decimals as usize + 1);
    let (whole, frac) = s.split_at(s.len() - decimals as usize);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{frac}")
    }
}

pub fn base_to_number(raw: u128, decimals: u32) -> f64 {
    from_base(raw, decimals).parse().unwrap_or(0.0)
}

/// What can be spent of a balance: SUI keeps the gas reserve back.
pub fn spendable(raw: u128, coin_type: &str) -> u128 {
    if normalize_type(coin_type) != SUI_TYPE {
        return raw;
    }
    raw.saturating_sub(GAS_RESERVE_MIST)
}

/// `pct` percent of `raw`, exact at 100.
pub fn pct_of(raw: u128, pct: f64) -> u128 {
    if pct >= 100.0 {
        return raw;
    }
    if pct <= 0.0 {
        return 0;
    }
    raw * (pct * 100.0).round() as u128 / 10_000
}

/// The least the trader receives at the slippage, base units.
pub fn min_out(amount_out: u128) -> u128 {
    amount_out * (10_000 - SLIPPAGE_BPS) / 10_000
}

/// Ladders by what the amount is counted in.
pub fn ladder_for(mode: Mode, coin: Option<&Coin>) -> Vec<f64> {
    if mode == Mode::Pct {
        return PCT_LADDER.to_vec();
    }
    let Some(c) = coin.filter(|c| !c.is_sui()) else { return SUI_LADDER.to_vec() };
    let p = c.price_usd.unwrap_or(0.0);
    if p > 0.5 && p < 2.0 {
        return USD_LADDER.to_vec();
    }
    let unit = if p > 0.0 { 1.0 / p } else { 1.0 };
    USD_LADDER.iter().map(|usd| super::fmt::precision_num(usd * unit, 2)).collect()
}

pub fn presets_for(mode: Mode, coin: Option<&Coin>) -> Vec<f64> {
    if mode == Mode::Pct {
        return vec![25.0, 50.0, 75.0, 100.0];
    }
    if coin.is_none_or(|c| c.is_sui()) {
        return vec![1.0, 5.0, 10.0, 25.0, 50.0];
    }
    let l = ladder_for(mode, coin);
    [2, 3, 5, 7, 9].iter().filter_map(|&i| l.get(i).copied()).collect()
}

/// The ticket's state and what follows from it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ticket {
    pub mode: Mode,
    pub value: f64,
    /// The from coin's balance in base units (None while loading or without a wallet).
    pub balance: Option<u128>,
    pub quote: Option<Quote>,
    pub quoting: bool,
    pub quote_error: Option<String>,
}

impl Ticket {
    pub fn new(mode: Mode, initial: f64) -> Ticket {
        Ticket { mode, value: initial, balance: None, quote: None, quoting: false, quote_error: None }
    }

    pub fn usable(&self, from: Option<&Coin>) -> Option<u128> {
        Some(spendable(self.balance?, &from?.coin_type))
    }

    /// The most the amount can be: 100 % in Pct, the spendable balance in Units (unbounded until known).
    pub fn max(&self, from: Option<&Coin>) -> f64 {
        if self.mode == Mode::Pct {
            return 100.0;
        }
        match (self.usable(from), from) {
            (Some(u), Some(c)) => base_to_number(u, c.decimals),
            _ => f64::INFINITY,
        }
    }

    pub fn amount_in(&self, from: Option<&Coin>) -> u128 {
        let Some(c) = from else { return 0 };
        match self.mode {
            Mode::Pct => self.usable(from).map(|u| pct_of(u, self.value)).unwrap_or(0),
            Mode::Units => to_base(self.value, c.decimals),
        }
    }

    /// The amount is more than the wallet can spend.
    pub fn short(&self, from: Option<&Coin>) -> bool {
        self.mode == Mode::Units && self.usable(from).is_some_and(|u| self.amount_in(from) > u)
    }

    /// LT / RT. The balance caps the ladder once it is known and not empty.
    pub fn bump(&mut self, dir: i32, from: Option<&Coin>) {
        let max = self.max(from);
        let ladder = ladder_for(self.mode, from);
        self.value = step(&ladder, self.value, dir, if max > 0.0 { max } else { f64::INFINITY }, 0.0);
    }
}

/// swap.ts `venueName`: "cetus" -> "Cetus", "flowx-clmm" -> "FlowX", "turbosclmm" -> "Turbos CLMM".
pub fn venue_name(v: &str) -> String {
    const VENUES: [(&str, &str); 16] = [
        ("cetus", "Cetus"),
        ("aftermath", "Aftermath"),
        ("bluefin", "Bluefin"),
        ("turbos", "Turbos"),
        ("deepbook", "DeepBook"),
        ("deepbookv3", "DeepBook"),
        ("flowx", "FlowX"),
        ("flowxv3", "FlowX"),
        ("kriya", "Kriya"),
        ("kriyav3", "Kriya"),
        ("momentum", "Momentum"),
        ("haedal", "Haedal"),
        ("obric", "Obric"),
        ("steamm", "Steamm"),
        ("magma", "Magma"),
        ("bluemove", "BlueMove"),
    ];
    let get = |k: &str| VENUES.iter().find(|(a, _)| *a == k).map(|(_, b)| b.to_string());
    let k = v.to_ascii_lowercase();
    let mut base = k.split(['_', '-']).next().unwrap_or("").to_string();
    // replace(/v\d+$/, "")
    if let Some(i) = base.rfind('v') {
        if i + 1 < base.len() && base[i + 1..].chars().all(|c| c.is_ascii_digit()) {
            base.truncate(i);
        }
    }
    if get(&k).is_none() && get(&base).is_none() {
        for amm in ["dlmm", "clmm"] {
            if let Some(stem) = base.strip_suffix(amm) {
                if !stem.is_empty() {
                    if let Some(n) = get(stem) {
                        return format!("{n} {}", amm.to_ascii_uppercase());
                    }
                }
            }
        }
    }
    get(&k).or_else(|| get(&base)).unwrap_or_else(|| {
        let mut c = base.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    })
}

/// "1 hop via Cetus", "2 hops via Cetus, Turbos".
pub fn route_text(q: &Quote) -> String {
    let mut names: Vec<String> = Vec::new();
    for v in &q.venues {
        let n = venue_name(v);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names.truncate(3);
    format!("{} {}{}", q.hops, if q.hops == 1 { "hop" } else { "hops" }, if names.is_empty() { String::new() } else { format!(" via {}", names.join(", ")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_units() {
        assert_eq!(to_base(1.5, 9), 1_500_000_000);
        assert_eq!(to_base(0.123456789123, 6), 123_456);
        assert_eq!(from_base(1_500_000_000, 9), "1.5");
        assert_eq!(from_base(276_426_792, 6), "276.426792");
        assert_eq!(from_base(5, 0), "5");
        assert_eq!(spendable(57_497_388_949, SUI_TYPE), 57_447_388_949);
        assert_eq!(pct_of(1_000, 33.0), 330);
        assert_eq!(pct_of(1_000, 100.0), 1_000);
        assert_eq!(min_out(276_426_792), 273_662_524);
    }

    #[test]
    fn ladders_and_presets() {
        assert_eq!(presets_for(Mode::Units, Some(&Coin::sui(Some(0.92), None))), vec![1.0, 5.0, 10.0, 25.0, 50.0]);
        assert_eq!(presets_for(Mode::Pct, None), vec![25.0, 50.0, 75.0, 100.0]);
        let deep = Coin { coin_type: "0xdeeb::deep::DEEP".into(), symbol: "DEEP".into(), decimals: 6, logo: None, price_usd: Some(0.0166) };
        let l = ladder_for(Mode::Units, Some(&deep));
        assert_eq!(l[0], 60.0);
        assert_eq!(presets_for(Mode::Units, Some(&deep)).len(), 5);
        let usdc = Coin { price_usd: Some(1.0), ..deep.clone() };
        assert_eq!(ladder_for(Mode::Units, Some(&usdc))[0], 1.0);
    }

    #[test]
    fn ticket_steps_inside_the_balance() {
        let sui = Coin::sui(Some(0.92), None);
        let mut t = Ticket::new(Mode::Units, 5.0);
        t.balance = Some(57_497_388_949);
        assert_eq!(t.amount_in(Some(&sui)), 5_000_000_000);
        t.bump(1, Some(&sui));
        assert_eq!(t.value, 10.0);
        for _ in 0..10 {
            t.bump(1, Some(&sui));
        }
        assert!((t.value - 57.447388949).abs() < 1e-9);
        assert!(!t.short(Some(&sui)));
        t.value = 100.0;
        assert!(t.short(Some(&sui)));
        let mut p = Ticket::new(Mode::Pct, 50.0);
        p.balance = Some(1_000);
        assert_eq!(p.amount_in(Some(&Coin { coin_type: "0x1::a::A".into(), symbol: "A".into(), decimals: 0, logo: None, price_usd: None })), 500);
    }

    #[test]
    fn venues() {
        assert_eq!(venue_name("cetus"), "Cetus");
        assert_eq!(venue_name("flowx-clmm"), "FlowX");
        assert_eq!(venue_name("deepbookv3"), "DeepBook");
        assert_eq!(venue_name("turbosclmm"), "Turbos CLMM");
        assert_eq!(venue_name("mmt-b5f5"), "Mmt");
        let q = Quote { amount_in: 0, amount_out: 0, fee_bps: 20, hops: 1, venues: vec!["cetus".into()], expires_at: 0.0 };
        assert_eq!(route_text(&q), "1 hop via Cetus");
    }
}
