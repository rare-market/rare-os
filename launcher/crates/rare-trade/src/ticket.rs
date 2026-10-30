//! A swap ticket for the pad: the amount (in coin units, or a share of the balance), the balance it
//! draws from, LT/RT stepping, presets, and a live quote that follows the amount. The web
//! launcher's `useTicket` hook as a struct: the screen tells it what changed and the time, and it
//! says when to read the balance or ask for a quote; the screen does the reads and hands the
//! answers back.

use crate::amount::{base_to_number, from_base, min_out, normalize_type, pct_of, spendable, step, to_base_num, PCT_LADDER, SLIPPAGE_BPS, SUI_LADDER, SUI_TYPE, USD_LADDER};
use crate::api::TokenRow;
use crate::jsnum::{js_number, locale, to_fixed, to_precision, Locale};
use crate::num::{num, num_text, NumKind, NumOptions};
use crate::swap::{QuoteParams, SwapQuote};
use crate::timing::{Activity, Cadence, Settle};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coin {
    pub coin_type: String,
    pub symbol: String,
    pub decimals: u32,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
}

impl Coin {
    /// A token row as a ticket coin (long coin type).
    pub fn from_row(t: &TokenRow) -> Coin {
        Coin { coin_type: normalize_type(&t.coin_type), symbol: t.symbol.clone(), decimals: t.decimals, logo: t.logo.clone(), price_usd: t.price_usd }
    }
    /// SUI as a Coin, from the lean token answer.
    pub fn sui(price_usd: Option<f64>, logo: Option<String>) -> Coin {
        Coin { coin_type: SUI_TYPE.into(), symbol: "SUI".into(), decimals: 9, logo, price_usd }
    }
}

pub fn is_sui(t: &str) -> bool {
    normalize_type(t) == SUI_TYPE
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmountMode {
    Units,
    Pct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeSide {
    Buy,
    Sell,
}

fn steps_in_sui(mode: AmountMode, coin: Option<&Coin>) -> bool {
    mode == AmountMode::Units && coin.map_or(true, |c| normalize_type(&c.coin_type) == SUI_TYPE)
}

/// Ladders by what the amount is counted in.
pub fn ladder_for(mode: AmountMode, coin: Option<&Coin>) -> Vec<f64> {
    if mode == AmountMode::Pct {
        return PCT_LADDER.to_vec();
    }
    if steps_in_sui(mode, coin) {
        return SUI_LADDER.to_vec();
    }
    let price = coin.and_then(|c| c.price_usd).unwrap_or(0.0);
    // Dollar coins step in dollars; anything else in round counts of the coin.
    if price > 0.5 && price < 2.0 {
        return USD_LADDER.to_vec();
    }
    let unit = if price > 0.0 { 1.0 / price } else { 1.0 };
    // About $1, $5, $10 ... worth of the coin, rounded to 2 significant digits.
    USD_LADDER.iter().map(|usd| js_number(&to_precision(usd * unit, 2))).collect()
}

pub fn presets_for(mode: AmountMode, coin: Option<&Coin>) -> Vec<f64> {
    if mode == AmountMode::Pct {
        return vec![25.0, 50.0, 75.0, 100.0];
    }
    if steps_in_sui(mode, coin) {
        return vec![1.0, 5.0, 10.0, 25.0, 50.0];
    }
    let l = ladder_for(mode, coin);
    [2, 3, 5, 7, 9].iter().filter_map(|&i| l.get(i).copied()).collect()
}

/// A number for a chip or the amount: six significant digits, grouped.
pub fn trim(v: f64) -> String {
    locale(js_number(&to_precision(v, 6)), Locale::max_frac(6))
}

/// The panel's verdict: whether its button is off, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelState {
    pub blocked: bool,
    pub why: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Ticket {
    pub from: Option<Coin>,
    pub to: Option<Coin>,
    pub owner: Option<String>,
    pub mode: AmountMode,
    pub value: f64,
    initial: f64,
    balance: Option<(String, u128)>,
    balance_at: Option<u64>,
    settle: Settle,
    now: u64,
    quote: Option<(String, SwapQuote)>,
    error: Option<(String, String)>,
    inflight: Option<String>,
    quoted_at: Option<u64>,
}

/// The balance is read again this often while the ticket is up.
pub const BALANCE_POLL_MS: u64 = 20_000;
/// A shown quote is refreshed this often.
pub const QUOTE_POLL_MS: u64 = 15_000;

impl Ticket {
    pub fn new(from: Option<Coin>, to: Option<Coin>, owner: Option<String>, mode: AmountMode, initial: f64) -> Ticket {
        Ticket { from, to, owner, mode, value: initial, initial, balance: None, balance_at: None, settle: Settle::default(), now: 0, quote: None, error: None, inflight: None, quoted_at: None }
    }

    /// The screen's coins, wallet or mode changed: a new coin or mode starts over at its default.
    pub fn update(&mut self, from: Option<Coin>, to: Option<Coin>, owner: Option<String>, mode: AmountMode, initial: f64) {
        let sig = |c: &Option<Coin>, m: AmountMode| (c.as_ref().map(|c| c.coin_type.clone()), m);
        if sig(&from, mode) != sig(&self.from, self.mode) {
            self.value = initial;
        }
        self.initial = initial;
        self.from = from;
        self.to = to;
        self.owner = owner;
        self.mode = mode;
    }

    // ---------- balance ----------

    /// What the balance read is keyed by (None: nothing to read).
    pub fn balance_key(&self) -> Option<String> {
        Some(format!("bal:{}:{}", self.owner.as_ref()?, self.from.as_ref()?.coin_type))
    }

    /// Whether to read the balance now (first time for a key, then every 20 s).
    pub fn balance_due(&self, now_ms: u64, activity: Activity) -> bool {
        let Some(key) = self.balance_key() else { return false };
        let have = self.balance.as_ref().is_some_and(|(k, _)| *k == key);
        !have || Cadence::slow_when_idle(BALANCE_POLL_MS).due(self.balance_at, now_ms, activity, false)
    }

    /// The balance read for `key` (ignored if the ticket moved on). None leaves the last value.
    pub fn set_balance(&mut self, key: &str, raw: Option<u128>, now_ms: u64) {
        self.balance_at = Some(now_ms);
        if self.balance_key().as_deref() != Some(key) {
            return;
        }
        if let Some(raw) = raw {
            self.balance = Some((key.to_string(), raw));
        }
    }

    pub fn balance(&self) -> Option<u128> {
        let key = self.balance_key()?;
        self.balance.as_ref().filter(|(k, _)| *k == key).map(|(_, b)| *b)
    }

    pub fn usable(&self) -> Option<u128> {
        Some(spendable(self.balance()?, &normalize_type(&self.from.as_ref()?.coin_type)))
    }

    /// The most the amount can be: 100 for a share, else the spendable balance (unbounded until it
    /// is known).
    pub fn max(&self) -> f64 {
        if self.mode == AmountMode::Pct {
            return 100.0;
        }
        match (self.usable(), &self.from) {
            (Some(u), Some(f)) => base_to_number(u, f.decimals),
            _ => f64::INFINITY,
        }
    }

    pub fn amount_in(&self) -> u128 {
        let Some(from) = &self.from else { return 0 };
        match self.mode {
            AmountMode::Pct => self.usable().map(|u| pct_of(u, self.value)).unwrap_or(0),
            AmountMode::Units => to_base_num(self.value, from.decimals),
        }
    }

    /// More than the wallet can spend.
    pub fn short(&self) -> bool {
        self.mode == AmountMode::Units && self.usable().is_some_and(|u| self.amount_in() > u)
    }

    pub fn ladder(&self) -> Vec<f64> {
        ladder_for(self.mode, self.from.as_ref())
    }

    pub fn presets(&self) -> Vec<f64> {
        presets_for(self.mode, self.from.as_ref())
    }

    pub fn set(&mut self, v: f64) {
        self.value = v;
    }

    /// LT / RT. The balance caps the ladder once it is known and not empty.
    pub fn bump(&mut self, dir: i32) {
        let max = self.max();
        self.value = step(&self.ladder(), self.value, dir, if max > 0.0 { max } else { f64::INFINITY }, 0.0);
    }

    // ---------- quote ----------

    pub fn quote_key(&self) -> Option<String> {
        let (from, to) = (self.from.as_ref()?, self.to.as_ref()?);
        let a = self.amount_in();
        (a > 0).then(|| format!("{}|{}|{a}|{}", from.coin_type, to.coin_type, self.owner.as_deref().unwrap_or("")))
    }

    fn settled(&self) -> Option<String> {
        let k = self.quote_key()?;
        (self.settle.settled(self.now) == Some(k.as_str())).then_some(k)
    }

    /// Call with the time whenever the screen updates. Returns a quote to ask for: 300 ms after the
    /// amount settles, then every 15 s (slower when idle). Hand the answer to
    /// [`Ticket::quote_done`] with the returned key.
    pub fn poll(&mut self, now_ms: u64, activity: Activity) -> Option<(String, QuoteParams)> {
        self.now = now_ms;
        self.settle.observe(self.quote_key(), now_ms);
        let key = self.settled()?;
        if self.inflight.as_deref() == Some(&key) {
            return None;
        }
        let answered = self.quote.as_ref().is_some_and(|(k, _)| *k == key) || self.error.as_ref().is_some_and(|(k, _)| *k == key);
        if answered && !Cadence::slow_when_idle(QUOTE_POLL_MS).due(self.quoted_at, now_ms, activity, false) {
            return None;
        }
        self.inflight = Some(key.clone());
        self.quoted_at = Some(now_ms);
        let (from, to) = (self.from.as_ref()?, self.to.as_ref()?);
        Some((key, QuoteParams { coin_in: from.coin_type.clone(), coin_out: to.coin_type.clone(), amount_in: self.amount_in(), owner: self.owner.clone() }))
    }

    /// The answer for a quote [`Ticket::poll`] asked for.
    pub fn quote_done(&mut self, key: &str, r: Result<SwapQuote, String>) {
        if self.inflight.as_deref() == Some(key) {
            self.inflight = None;
        }
        match r {
            Ok(q) => {
                self.quote = Some((key.to_string(), q));
                self.error = None;
            }
            Err(e) => self.error = Some((key.to_string(), e)),
        }
    }

    /// The quote for the amount showing, once it has settled.
    pub fn quote(&self) -> Option<&SwapQuote> {
        let key = self.settled()?;
        self.quote.as_ref().filter(|(k, _)| *k == key).map(|(_, q)| q)
    }

    /// Waiting for a quote for the amount showing.
    pub fn quoting(&self) -> bool {
        let Some(key) = self.quote_key() else { return false };
        let settled = self.settled();
        (settled.as_deref() != Some(key.as_str()) || self.inflight.as_deref() == Some(key.as_str())) && self.quote().is_none()
    }

    pub fn quote_error(&self) -> Option<&str> {
        let key = self.settled()?;
        self.error.as_ref().filter(|(k, _)| *k == key).map(|(_, e)| e.as_str())
    }

    // ---------- words for the panel ----------

    /// The amount's own words: "10 SUI" to buy, "50%" or "All" of the coin to sell.
    pub fn amount_label(&self, side: TradeSide) -> String {
        let Some(from) = &self.from else { return String::new() };
        match side {
            TradeSide::Sell if self.value >= 100.0 => "All".into(),
            TradeSide::Sell => num_text(NumKind::Share, Some(self.value), NumOptions::decimals(0)),
            TradeSide::Buy => format!("{} {}", trim(self.value), from.symbol),
        }
    }

    /// A preset chip: "Max" or "25%" to sell, the amount to buy.
    pub fn preset_label(side: TradeSide, v: f64) -> String {
        match side {
            TradeSide::Sell if v >= 100.0 => "Max".into(),
            TradeSide::Sell => format!("{}%", crate::jsnum::js_string(v)),
            TradeSide::Buy => trim(v),
        }
    }

    /// The "Max" chip's value on a buy, when the balance is known (four decimals).
    pub fn max_chip(&self) -> Option<f64> {
        let m = self.max();
        (m.is_finite() && m > 0.0).then(|| js_number(&to_fixed(m, 4)))
    }

    /// "You get about": the quote's amount, else what is happening.
    pub fn get_line(&self) -> String {
        match (self.quote(), &self.to) {
            (Some(q), Some(to)) => format!("{} {}", num(NumKind::Amount, base_to_number(q.amount_out, to.decimals)), to.symbol),
            _ if self.quoting() => "Getting a quote".into(),
            _ if self.quote_error().is_some() => "No route for this amount".into(),
            _ => String::new(),
        }
    }

    /// "Minimum received" at the build's slippage.
    pub fn min_line(&self) -> Option<String> {
        let (q, to) = (self.quote()?, self.to.as_ref()?);
        Some(format!("{} {}", num(NumKind::Amount, js_number(&from_base(min_out(q.amount_out, SLIPPAGE_BPS), to.decimals))), to.symbol))
    }

    /// "Balance": the coin balance, "" while unknown, "No wallet" without one.
    pub fn balance_line(&self, has_wallet: bool) -> String {
        match (&self.from, self.balance()) {
            (Some(f), Some(b)) => format!("{} {}", num(NumKind::Amount, base_to_number(b, f.decimals)), f.symbol),
            _ if has_wallet => String::new(),
            _ => "No wallet".into(),
        }
    }

    /// The trade panel's button: off without coins, an amount, enough balance or a quote.
    pub fn panel(&self, side: TradeSide, has_wallet: bool) -> PanelState {
        let nothing = side == TradeSide::Sell && self.balance() == Some(0);
        let blocked = self.from.is_none() || self.to.is_none() || self.amount_in() == 0 || self.short() || nothing || self.quote().is_none();
        let why = if !has_wallet {
            Some("Add a wallet first".to_string())
        } else if nothing {
            Some(format!("No {} to sell", self.from.as_ref().map(|f| f.symbol.as_str()).unwrap_or("")))
        } else if self.short() {
            Some("Not enough balance".to_string())
        } else {
            self.quote_error().map(str::to_string)
        };
        PanelState { blocked, why }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amount::USDC_TYPE;

    fn usdc() -> Coin {
        Coin { coin_type: USDC_TYPE.into(), symbol: "USDC".into(), decimals: 6, logo: None, price_usd: Some(1.0) }
    }
    fn meme(price: f64) -> Coin {
        Coin { coin_type: "0xabc::meme::MEME".into(), symbol: "MEME".into(), decimals: 6, logo: None, price_usd: Some(price) }
    }
    fn q(out: u128) -> SwapQuote {
        SwapQuote { id: "q".into(), coin_in: SUI_TYPE.into(), coin_out: USDC_TYPE.into(), amount_in: 1, amount_out: out, fee_bps: 20.0, hops: 1, venues: vec![], expires_at: 0.0 }
    }

    #[test]
    fn ladders_follow_the_coin() {
        assert_eq!(ladder_for(AmountMode::Pct, None), PCT_LADDER.to_vec());
        assert_eq!(ladder_for(AmountMode::Units, Some(&Coin::sui(Some(3.0), None))), SUI_LADDER.to_vec());
        assert_eq!(ladder_for(AmountMode::Units, Some(&usdc())), USD_LADDER.to_vec());
        assert_eq!(&ladder_for(AmountMode::Units, Some(&meme(0.003)))[..3], &[330.0, 670.0, 1700.0]);
        assert_eq!(presets_for(AmountMode::Units, None), vec![1.0, 5.0, 10.0, 25.0, 50.0]);
        assert_eq!(presets_for(AmountMode::Units, Some(&usdc())), vec![5.0, 10.0, 20.0, 50.0, 100.0]);
        assert_eq!(presets_for(AmountMode::Pct, Some(&usdc())), vec![25.0, 50.0, 75.0, 100.0]);
    }

    #[test]
    fn amounts_follow_the_balance() {
        let owner = Some("0xme".to_string());
        let mut t = Ticket::new(Some(Coin::sui(Some(3.0), None)), Some(usdc()), owner.clone(), AmountMode::Units, 5.0);
        assert_eq!(t.amount_in(), 5_000_000_000);
        assert_eq!(t.max(), f64::INFINITY);
        let key = t.balance_key().unwrap();
        assert!(t.balance_due(0, Activity::Active));
        t.set_balance(&key, Some(3_050_000_000), 0);
        assert!(!t.balance_due(1_000, Activity::Active));
        assert!(t.balance_due(20_000, Activity::Active));
        assert_eq!(t.usable(), Some(3_000_000_000));
        assert_eq!(t.max(), 3.0);
        assert!(t.short());
        assert_eq!(t.panel(TradeSide::Buy, true).why.as_deref(), Some("Not enough balance"));
        t.bump(-1);
        assert_eq!(t.value, 3.0);
        t.bump(1);
        assert_eq!(t.value, 3.0);
        assert_eq!(t.max_chip(), Some(3.0));
        assert_eq!(t.balance_line(true), "3.05 SUI");
        // Selling a share of a coin.
        let mut s = Ticket::new(Some(usdc()), Some(Coin::sui(None, None)), owner, AmountMode::Pct, 50.0);
        let k = s.balance_key().unwrap();
        s.set_balance(&k, Some(0), 0);
        assert_eq!(s.panel(TradeSide::Sell, true).why.as_deref(), Some("No USDC to sell"));
        s.set_balance(&k, Some(1_000_000), 0);
        assert_eq!(s.amount_in(), 500_000);
        assert_eq!(s.amount_label(TradeSide::Sell), "50%");
        s.set(100.0);
        assert_eq!(s.amount_label(TradeSide::Sell), "All");
        // A new coin starts over at its default.
        s.update(Some(meme(1.0)), Some(Coin::sui(None, None)), None, AmountMode::Pct, 25.0);
        assert_eq!(s.value, 25.0);
        assert_eq!(s.balance(), None);
        assert_eq!(s.balance_line(false), "No wallet");
    }

    #[test]
    fn quotes_follow_the_amount_once_it_settles() {
        let mut t = Ticket::new(Some(Coin::sui(Some(3.0), None)), Some(usdc()), None, AmountMode::Units, 1.0);
        assert!(t.poll(0, Activity::Active).is_none());
        assert!(t.quoting());
        let (key, p) = t.poll(300, Activity::Active).unwrap();
        assert_eq!(p.amount_in, 1_000_000_000);
        assert!(t.poll(310, Activity::Active).is_none());
        assert!(t.quoting());
        t.quote_done(&key, Ok(q(2_900_000)));
        assert!(!t.quoting());
        assert_eq!(t.quote().unwrap().amount_out, 2_900_000);
        assert_eq!(t.get_line(), "2.9 USDC");
        assert_eq!(t.min_line().as_deref(), Some("2.87 USDC"));
        assert!(!t.panel(TradeSide::Buy, true).blocked);
        // Refreshed every 15 s.
        assert!(t.poll(10_000, Activity::Active).is_none());
        assert!(t.poll(15_300, Activity::Active).is_some());
        // A new amount hides the old quote until its own arrives.
        t.bump(1);
        t.poll(15_400, Activity::Active);
        assert!(t.quote().is_none() && t.quoting());
        assert_eq!(t.get_line(), "Getting a quote");
        let (k2, _) = t.poll(15_700, Activity::Active).unwrap();
        t.quote_done(&k2, Err("No route for this pair right now.".into()));
        assert_eq!(t.quote_error(), Some("No route for this pair right now."));
        assert_eq!(t.get_line(), "No route for this amount");
        assert!(t.panel(TradeSide::Buy, true).blocked);
        assert_eq!(t.panel(TradeSide::Buy, false).why.as_deref(), Some("Add a wallet first"));
    }

    #[test]
    fn words() {
        assert_eq!(trim(1234.5678912), "1,234.57");
        assert_eq!(trim(0.000123456789), "0.000123");
        assert_eq!(Ticket::preset_label(TradeSide::Sell, 25.0), "25%");
        assert_eq!(Ticket::preset_label(TradeSide::Sell, 100.0), "Max");
        assert_eq!(Ticket::preset_label(TradeSide::Buy, 2500.0), "2,500");
        let t = Ticket::new(Some(Coin::sui(None, None)), None, None, AmountMode::Units, 2.5);
        assert_eq!(t.amount_label(TradeSide::Buy), "2.5 SUI");
        assert!(is_sui("0x2::sui::SUI"));
    }
}
