//! Perps: markets, charts, accounts, order previews and unsigned order transactions, from the
//! the Trade API's perps routes (the perps venue's request schema, served for wallet clients under
//! /ext/perps). Types and helpers follow the perps venue's request schema, and the
//! order ticket's rules follow the perps market page. Nothing here signs.

use crate::amount::{fmt_leverage, leverage_stops, step, MARGIN_PRESETS};
use crate::http::{error_of, get, query, trade_call, Result};
use crate::jsnum::{js_string, json_number, locale, to_fixed, Locale};
use crate::timing::Settle;
use crate::tx::UnsignedTx;
use rare_market::net::{Method, Net};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Long,
    Short,
}

impl Side {
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Long => "long",
            Side::Short => "short",
        }
    }
    /// "Long", "Short"
    pub fn label(&self) -> &'static str {
        match self {
            Side::Long => "Long",
            Side::Short => "Short",
        }
    }
    pub fn flip(&self) -> Side {
        match self {
            Side::Long => Side::Short,
            Side::Short => Side::Long,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpMarket {
    pub market_id: String,
    pub symbol: String,
    pub pair: String,
    pub price: f64,
    pub change_pct: Option<f64>,
    pub volume_usd: Option<f64>,
    pub max_leverage: Option<f64>,
    pub funding_rate: Option<f64>,
    pub next_funding_ms: Option<f64>,
    pub open_interest_usd: Option<f64>,
    pub taker_fee: Option<f64>,
    pub min_order_usd: Option<f64>,
}

impl PerpMarket {
    /// "BTC/USD" from "BTCUSD".
    pub fn pair_label(&self) -> String {
        match self.pair.strip_suffix("USD") {
            Some(head) => format!("{head}/USD"),
            None => self.pair.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpPosition {
    pub market_id: String,
    pub symbol: String,
    pub side: Side,
    pub size: f64,
    pub notional_usd: f64,
    pub margin_usd: f64,
    pub leverage: f64,
    pub entry_price: f64,
    pub mark_price: Option<f64>,
    pub liquidation_price: f64,
    pub pnl_usd: f64,
}

impl PerpPosition {
    /// What comes back to the account if the position is closed now.
    pub fn margin_back(&self) -> f64 {
        (self.margin_usd + self.pnl_usd).max(0.0)
    }
    /// "Long 5x"
    pub fn side_line(&self) -> String {
        format!("{} {}", self.side.label(), fmt_leverage(self.leverage))
    }
    /// The review sheet's title for closing it: "Close BTC long".
    pub fn close_title(&self) -> String {
        format!("Close {} {}", self.symbol, self.side.as_str())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpOrder {
    pub market_id: String,
    pub symbol: String,
    pub order_id: String,
    pub side: Side,
    pub size: f64,
    pub price: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpsAccount {
    pub account_id: String,
    pub equity_usd: f64,
    pub available_usd: f64,
    pub unrealized_pnl_usd: f64,
    pub positions: Vec<PerpPosition>,
    pub orders: Vec<PerpOrder>,
    pub builder_approved: bool,
    pub needs_approval: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderPreview {
    pub size: String,
    pub size_units: f64,
    pub entry_price: f64,
    pub liquidation_price: Option<f64>,
    pub notional_usd: f64,
    pub margin_usd: f64,
    pub fee_usd: f64,
    pub builder_fee_usd: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpBar {
    /// ms since the epoch
    pub time: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl PerpBar {
    /// As a chart candle (time in seconds).
    pub fn candle(&self) -> crate::api::Candle {
        crate::api::Candle { time: (self.time / 1000.0).floor(), open: self.open, high: self.high, low: self.low, close: self.close, volume: Some(self.volume) }
    }
}

/// What the perps API builds.
#[derive(Clone, Debug, PartialEq)]
pub enum PerpTxRequest {
    CreateAccount,
    Deposit { usd: f64 },
    Open { market_id: String, side: Side, margin_usd: f64, leverage: f64, slippage: Option<f64>, deposit_usd: Option<f64> },
    Close { market_id: String, fraction: Option<f64> },
}

impl PerpTxRequest {
    /// The request as the API reads it (numbers written as JS writes them).
    pub fn to_json(&self) -> Value {
        match self {
            PerpTxRequest::CreateAccount => json!({ "kind": "create-account" }),
            PerpTxRequest::Deposit { usd } => json!({ "kind": "deposit", "usd": json_number(*usd) }),
            PerpTxRequest::Open { market_id, side, margin_usd, leverage, slippage, deposit_usd } => {
                let mut v = json!({ "kind": "open", "marketId": market_id, "side": side.as_str(), "marginUsd": json_number(*margin_usd), "leverage": json_number(*leverage) });
                if let Some(s) = slippage {
                    v["slippage"] = json_number(*s);
                }
                if let Some(d) = deposit_usd {
                    v["depositUsd"] = json_number(*d);
                }
                v
            }
            PerpTxRequest::Close { market_id, fraction } => {
                let mut v = json!({ "kind": "close", "marketId": market_id });
                if let Some(f) = fraction {
                    v["fraction"] = json_number(*f);
                }
                v
            }
        }
    }
}

const BASE: &str = "/ext/perps";

pub mod perp_paths {
    use super::*;
    pub fn markets() -> String {
        format!("{BASE}/markets")
    }
    pub fn account(address: &str) -> String {
        format!("{BASE}/account?address={}", address.to_lowercase())
    }
    pub fn candles(market_id: &str, resolution: &str, from_ms: f64, to_ms: f64) -> String {
        let (from, to) = (js_string(from_ms.floor()), js_string(to_ms.ceil()));
        format!("{BASE}/candles?{}", query(&[("market", market_id), ("resolution", resolution), ("from", &from), ("to", &to)]))
    }
    pub fn preview(market_id: &str, side: Side, margin_usd: f64, leverage: f64) -> String {
        let (m, l) = (js_string(margin_usd), js_string(leverage));
        format!("{BASE}/preview?{}", query(&[("market", market_id), ("side", side.as_str()), ("margin", &m), ("leverage", &l)]))
    }
}

/// the Trade API's full perps list, read only for each market's picture (the ext list has none).
pub const PERP_LOGO_PATH: &str = crate::api::PERP_LOGO_PATH;

/// marketId -> picture, from the Trade API's perps list.
pub fn logo_map(body: &Value) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    for it in body.get("items").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(id), Some(logo)) = (it.get("id").and_then(Value::as_str), it.get("logo").and_then(Value::as_str)) {
            if !id.is_empty() && !logo.is_empty() {
                m.insert(id.to_lowercase(), logo.to_string());
            }
        }
    }
    m
}

pub fn fetch_markets(net: &dyn Net) -> Result<Vec<PerpMarket>> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct R {
        items: Vec<PerpMarket>,
    }
    Ok(get::<R>(net, &perp_paths::markets())?.items)
}

/// The markets biggest volume first, as the perps table and the home rail list them.
pub fn by_volume(mut markets: Vec<PerpMarket>) -> Vec<PerpMarket> {
    markets.sort_by(|a, b| b.volume_usd.unwrap_or(0.0).total_cmp(&a.volume_usd.unwrap_or(0.0)));
    markets
}

/// The wallet's perps account; None when it has not set one up.
pub fn fetch_account(net: &dyn Net, address: &str) -> Result<Option<PerpsAccount>> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct R {
        account: Option<PerpsAccount>,
    }
    Ok(get::<R>(net, &perp_paths::account(address))?.account)
}

pub fn fetch_bars(net: &dyn Net, market_id: &str, resolution: &str, from_ms: f64, to_ms: f64) -> Result<Vec<PerpBar>> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct R {
        bars: Vec<PerpBar>,
    }
    Ok(get::<R>(net, &perp_paths::candles(market_id, resolution, from_ms, to_ms))?.bars)
}

pub fn preview_open(net: &dyn Net, market_id: &str, side: Side, margin_usd: f64, leverage: f64) -> Result<OrderPreview> {
    get(net, &perp_paths::preview(market_id, side, margin_usd, leverage))
}

/// The unsigned order for this wallet (sender set by the API), for Rare Wallet.
pub fn build_perp_tx(net: &dyn Net, address: &str, req: &PerpTxRequest) -> Result<UnsignedTx> {
    let r = trade_call(net, Method::Post, &format!("{BASE}/tx"), Some(&json!({ "address": address.to_lowercase(), "req": req.to_json() })), None)?;
    let tx = r.body.get("tx").and_then(Value::as_str).filter(|t| !t.is_empty());
    match tx {
        Some(tx) if r.status == 200 => Ok(UnsignedTx::from_api(tx, address)),
        _ => {
            let body = if r.body.is_null() { json!({ "error": "Could not build the transaction." }) } else { r.body };
            Err(error_of(r.status, &body))
        }
    }
}

/// A candle resolution the perps chart offers, with the window it loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerpTimeframe {
    pub tf: &'static str,
    pub res: &'static str,
    pub span_ms: u64,
}

pub const PERP_TIMEFRAMES: [PerpTimeframe; 5] = [
    PerpTimeframe { tf: "5m", res: "5", span_ms: 2 * 86_400_000 },
    PerpTimeframe { tf: "15m", res: "15", span_ms: 6 * 86_400_000 },
    PerpTimeframe { tf: "1h", res: "60", span_ms: 21 * 86_400_000 },
    PerpTimeframe { tf: "4h", res: "240", span_ms: 90 * 86_400_000 },
    PerpTimeframe { tf: "1d", res: "1D", span_ms: 400 * 86_400_000 },
];

/// What an order does about money: nothing, top up the perps account from the wallet first, or
/// stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FundingPlan {
    pub short_by: f64,
    pub blocked: bool,
    pub needs_account: bool,
}

/// `account`: None while it is loading, Some(None) when the wallet has no perps account, else its
/// available USD. `wallet_usdc`: the wallet's USDC, None while unknown.
pub fn funding_plan(account: Option<Option<f64>>, margin_usd: f64, fee_usd: f64, wallet_usdc: Option<f64>) -> FundingPlan {
    let available = account.flatten().unwrap_or(0.0);
    let short_by = ((margin_usd * 1.02 + fee_usd - available) * 100.0).ceil() / 100.0;
    let short_by = short_by.max(0.0);
    let blocked = short_by > 0.0 && wallet_usdc.is_some_and(|w| w + 1e-9 < short_by);
    FundingPlan { short_by, blocked, needs_account: account == Some(None) }
}

/// The open request for a ticket, with the top-up folded in when the account is short.
pub fn open_request(market_id: &str, side: Side, margin_usd: f64, leverage: f64, short_by: f64) -> PerpTxRequest {
    PerpTxRequest::Open { market_id: market_id.to_string(), side, margin_usd, leverage, slippage: Some(0.01), deposit_usd: (short_by > 0.0).then_some(short_by) }
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

/// A perps price: 2 decimals from $100, 4 from $1, else 5 significant digits.
pub fn perp_px(v: Option<f64>) -> String {
    let Some(v) = v.filter(|v| v.is_finite() && *v > 0.0) else { return String::new() };
    let o = if v.abs() >= 100.0 {
        Locale::frac(2, 2)
    } else if v.abs() >= 1.0 {
        Locale::max_frac(4)
    } else {
        Locale::sig(5)
    };
    format!("${}", locale(v, o))
}

/// Price at which a position has made (or lost) `pct` of its margin.
pub fn price_at_return(side: Side, entry_price: f64, size: f64, margin_usd: f64, pct: f64) -> Option<f64> {
    if size == 0.0 || size.is_nan() {
        return None;
    }
    let mv = (pct / 100.0) * margin_usd / size;
    let v = if side == Side::Long { entry_price + mv } else { entry_price - mv };
    (v > 0.0).then_some(v)
}

/// A liquidation price, or plain words when it is effectively unreachable (a 1x long).
pub fn liq_text(liq: Option<f64>, entry: f64) -> String {
    let Some(liq) = liq.filter(|l| l.is_finite()) else { return String::new() };
    if liq <= entry * 1e-3 {
        return "Near $0".into();
    }
    perp_px(Some(liq))
}

/// "1,234", "12.345", "0.000123": a position or order size.
pub fn num_size(v: f64) -> String {
    if v >= 1000.0 {
        locale(v, Locale::max_frac(0))
    } else {
        locale(v, Locale::max_frac(if v >= 1.0 { 3 } else { 6 }))
    }
}

/// "3h 12m" until `ms` (the next funding).
pub fn until_text(ms: f64, now_ms: f64) -> String {
    let s = crate::jsnum::js_round((ms - now_ms) / 1000.0).max(0.0) as u64;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/// The perps order ticket on a market page: side, margin and leverage, the preview it asks for
/// once still, and why the order button is off.
#[derive(Clone, Debug, PartialEq)]
pub struct PerpTicket {
    pub market_id: String,
    pub side: Side,
    pub margin: f64,
    pub leverage: f64,
    settle: Settle,
}

/// Why an order cannot go, or the reason to show under the button.
#[derive(Clone, Debug, PartialEq)]
pub struct TicketState {
    pub plan: FundingPlan,
    pub too_small: bool,
    pub blocked: bool,
    pub why: Option<String>,
}

impl PerpTicket {
    pub fn new(market_id: &str) -> PerpTicket {
        PerpTicket { market_id: market_id.to_string(), side: Side::Long, margin: 10.0, leverage: 5.0, settle: Settle::default() }
    }

    pub fn presets() -> &'static [f64] {
        &MARGIN_PRESETS
    }

    /// X flips Long and Short.
    pub fn flip(&mut self) {
        self.side = self.side.flip();
    }

    /// LT and RT set the leverage (only while the order panel has focus, the screen decides).
    pub fn bump_leverage(&mut self, dir: i32, max_leverage: Option<f64>) {
        let stops = leverage_stops(max_leverage);
        let max = *stops.last().unwrap_or(&1.0);
        self.leverage = step(&stops, self.leverage, dir, max, 1.0);
    }

    /// Once the market is known, a leverage over its maximum comes down to it.
    pub fn fit_market(&mut self, max_leverage: Option<f64>) {
        let max = *leverage_stops(max_leverage).last().unwrap_or(&1.0);
        if self.leverage > max {
            self.leverage = max;
        }
    }

    /// The preview key for the current ticket.
    pub fn key(&self) -> String {
        format!("{}|{}|{}|{}", self.market_id, self.side.as_str(), js_string(self.margin), js_string(self.leverage))
    }

    /// Call each frame (or on change) with the time; returns the preview to ask for once the ticket
    /// has been still for 300 ms.
    pub fn settled(&mut self, now_ms: u64) -> Option<String> {
        self.settle.observe(Some(self.key()), now_ms);
        self.settle.settled(now_ms).map(str::to_string)
    }

    /// Everything the order button needs. `account` as [`funding_plan`] takes it.
    pub fn state(&self, market: Option<&PerpMarket>, has_wallet: bool, preview: Option<&OrderPreview>, preview_error: Option<&str>, account: Option<Option<f64>>, wallet_usdc: Option<f64>) -> TicketState {
        let plan = funding_plan(account, self.margin, preview.map(|p| p.fee_usd).unwrap_or(0.0), wallet_usdc);
        let min_order = market.and_then(|m| m.min_order_usd).unwrap_or(1.0);
        let too_small = self.margin * self.leverage < min_order;
        let blocked = market.is_none() || preview.is_none() || plan.blocked || too_small;
        let why = if !has_wallet {
            Some("Add a wallet first".to_string())
        } else if plan.needs_account {
            Some("One time, before your first order".to_string())
        } else if plan.blocked {
            Some("Not enough USDC in your wallet".to_string())
        } else if too_small {
            Some(format!("Minimum order {}", perp_px(Some(min_order))))
        } else {
            preview_error.map(str::to_string)
        };
        TicketState { plan, too_small, blocked, why }
    }

    /// The open request for this ticket.
    pub fn request(&self, short_by: f64) -> PerpTxRequest {
        open_request(&self.market_id, self.side, self.margin, self.leverage, short_by)
    }
}

/// The candles path for a timeframe ending now.
pub fn candles_now(market_id: &str, tf: &PerpTimeframe, now_ms: f64) -> String {
    perp_paths::candles(market_id, tf.res, now_ms - tf.span_ms as f64, now_ms)
}

#[cfg(test)]
mod tests {
    //! perps.test.ts
    use super::*;
    use rare_market::net::MockNet;

    fn addr() -> String {
        format!("0x{}", "B2".repeat(32))
    }
    fn market() -> String {
        format!("0x{}", "c3".repeat(32))
    }

    /// What the perps API answers: Transaction JSON v2 with the sender set.
    fn server_tx(sender: &str) -> String {
        json!({
            "version": 2, "sender": sender, "expiration": null,
            "gasData": {"budget": null, "price": null, "owner": null, "payment": null},
            "inputs": [{"Pure": {"bytes": "6AMAAAAAAAA="}, "$kind": "Pure"}, {"Pure": {"bytes": "srKysrKysrKysrKysrKysrKysrKysrKysrKysrKysrI="}, "$kind": "Pure"}],
            "commands": [
                {"SplitCoins": {"coin": {"GasCoin": true, "$kind": "GasCoin"}, "amounts": [{"Input": 0, "type": "pure", "$kind": "Input"}]}, "$kind": "SplitCoins"},
                {"TransferObjects": {"objects": [{"NestedResult": [0, 0], "$kind": "NestedResult"}], "address": {"Input": 1, "type": "pure", "$kind": "Input"}}, "$kind": "TransferObjects"}
            ]
        })
        .to_string()
    }

    const TX_URL: &str = "https://rare.market/v1/trade/ext/perps/tx";

    fn sent(n: &MockNet) -> Value {
        serde_json::from_str(n.calls()[0].body.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn posts_the_request_for_the_wallet_and_returns_the_unsigned_transaction() {
        let n = MockNet::new();
        n.on_call(Method::Post, TX_URL, 200, json!({ "tx": server_tx(&addr().to_lowercase()) }).to_string());
        let tx = build_perp_tx(&n, &addr(), &open_request(&market(), Side::Long, 10.0, 5.0, 0.0)).unwrap();
        assert_eq!(sent(&n), json!({ "address": addr().to_lowercase(), "req": { "kind": "open", "marketId": market(), "side": "long", "marginUsd": 10, "leverage": 5, "slippage": 0.01 } }));
        assert!(n.calls()[0].body.as_deref().unwrap().contains("\"marginUsd\":10,"));
        let UnsignedTx::Json(data) = tx else { panic!("json tx") };
        assert_eq!(data["sender"], addr().to_lowercase());
        let kinds: Vec<&str> = data["commands"].as_array().unwrap().iter().map(|c| c["$kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["SplitCoins", "TransferObjects"]);
    }

    #[test]
    fn builds_close_orders() {
        let n = MockNet::new();
        n.on_call(Method::Post, TX_URL, 200, json!({ "tx": server_tx(&addr().to_lowercase()) }).to_string());
        build_perp_tx(&n, &addr(), &PerpTxRequest::Close { market_id: market(), fraction: None }).unwrap();
        assert_eq!(sent(&n), json!({ "address": addr().to_lowercase(), "req": { "kind": "close", "marketId": market() } }));
    }

    #[test]
    fn passes_the_apis_reason_on() {
        let n = MockNet::new();
        n.on_call(Method::Post, TX_URL, 422, json!({ "error": "Set up your perps account first." }).to_string());
        assert_eq!(build_perp_tx(&n, &addr(), &PerpTxRequest::Close { market_id: market(), fraction: None }).unwrap_err().message, "Set up your perps account first.");
        let n = MockNet::new();
        n.on_call(Method::Post, TX_URL, 500, "");
        assert_eq!(build_perp_tx(&n, &addr(), &PerpTxRequest::CreateAccount).unwrap_err().message, "Could not build the transaction.");
    }

    #[test]
    fn folds_the_shortfall_into_the_order_as_a_deposit() {
        let plan = funding_plan(Some(Some(4.0)), 10.0, 0.05, Some(50.0));
        assert_eq!(plan, FundingPlan { short_by: 6.25, blocked: false, needs_account: false });
        let PerpTxRequest::Open { deposit_usd, side, .. } = open_request(&market(), Side::Short, 10.0, 3.0, plan.short_by) else { panic!() };
        assert_eq!((deposit_usd, side), (Some(6.25), Side::Short));
    }

    #[test]
    fn blocks_when_the_wallet_cannot_cover_it() {
        assert!(funding_plan(Some(Some(0.0)), 10.0, 0.05, Some(2.0)).blocked);
        assert!(funding_plan(Some(None), 10.0, 0.0, Some(100.0)).needs_account);
        assert_eq!(funding_plan(Some(Some(100.0)), 10.0, 0.05, Some(0.0)), FundingPlan { short_by: 0.0, blocked: false, needs_account: false });
        assert!(!funding_plan(None, 10.0, 0.0, None).needs_account);
    }

    #[test]
    fn prints_funding_rates() {
        assert_eq!(funding_pct(0.0000027839562203562616, 1.0), "0.000278%");
        assert_eq!(funding_pct(0.0002, 8.0), "0.1600%");
    }

    #[test]
    fn prints_perps_prices() {
        assert_eq!(perp_px(Some(343.17069)), "$343.17");
        assert_eq!(perp_px(Some(2.90502)), "$2.905");
        assert_eq!(perp_px(Some(0.14604001378161055)), "$0.14604");
        assert_eq!(perp_px(Some(0.0)), "");
        assert_eq!(perp_px(None), "");
    }

    #[test]
    fn says_when_a_liquidation_price_is_out_of_reach() {
        assert_eq!(liq_text(Some(8.187e-15), 79_935.97), "Near $0");
        assert_eq!(liq_text(Some(27_303.06), 79_935.97), "$27,303.06");
        assert_eq!(liq_text(None, 1.0), "");
    }

    #[test]
    fn finds_the_price_for_a_return_on_margin() {
        assert_eq!(price_at_return(Side::Long, 100.0, 1.0, 10.0, 50.0), Some(105.0));
        assert_eq!(price_at_return(Side::Short, 100.0, 1.0, 10.0, 50.0), Some(95.0));
        assert_eq!(price_at_return(Side::Long, 100.0, 0.0, 10.0, 50.0), None);
    }

    #[test]
    fn builds_query_paths() {
        assert_eq!(perp_paths::preview(&market(), Side::Long, 10.0, 5.0), format!("/ext/perps/preview?market={}&side=long&margin=10&leverage=5", market()));
        assert_eq!(perp_paths::account(&addr()), format!("/ext/perps/account?address={}", addr().to_lowercase()));
        assert_eq!(perp_paths::candles("0xab", "1D", 1000.4, 2000.2), "/ext/perps/candles?market=0xab&resolution=1D&from=1000&to=2001");
    }

    #[test]
    fn the_ticket_explains_itself() {
        let m = PerpMarket { market_id: market(), min_order_usd: Some(25.0), max_leverage: Some(3.0), ..PerpMarket::default() };
        let mut t = PerpTicket::new(&market());
        t.fit_market(m.max_leverage);
        assert_eq!(t.leverage, 3.0);
        t.bump_leverage(1, m.max_leverage);
        assert_eq!(t.leverage, 3.0);
        t.bump_leverage(-1, m.max_leverage);
        assert_eq!(t.leverage, 2.0);
        let p = OrderPreview { fee_usd: 0.05, ..OrderPreview::default() };
        assert_eq!(t.state(Some(&m), false, Some(&p), None, None, None).why.as_deref(), Some("Add a wallet first"));
        assert_eq!(t.state(Some(&m), true, Some(&p), None, Some(None), None).why.as_deref(), Some("One time, before your first order"));
        assert_eq!(t.state(Some(&m), true, Some(&p), None, Some(Some(0.0)), Some(1.0)).why.as_deref(), Some("Not enough USDC in your wallet"));
        let s = t.state(Some(&m), true, Some(&p), None, Some(Some(100.0)), None);
        assert!(s.too_small && s.blocked);
        assert_eq!(s.why.as_deref(), Some("Minimum order $25"));
        t.margin = 25.0;
        let s = t.state(Some(&m), true, Some(&p), Some("Market closed"), Some(Some(100.0)), None);
        assert!(!s.blocked);
        assert_eq!(s.why.as_deref(), Some("Market closed"));
        assert!(t.state(Some(&m), true, None, None, Some(Some(100.0)), None).blocked);
        t.flip();
        assert_eq!(t.settled(0), None);
        assert_eq!(t.settled(300), Some(format!("{}|short|25|2", market())));
    }

    #[test]
    fn position_words() {
        let p = PerpPosition { symbol: "BTC".into(), side: Side::Short, leverage: 5.0, margin_usd: 10.0, pnl_usd: -12.0, ..PerpPosition::default() };
        assert_eq!(p.margin_back(), 0.0);
        assert_eq!(p.side_line(), "Short 5x");
        assert_eq!(p.close_title(), "Close BTC short");
        assert_eq!(PerpMarket { pair: "BTCUSD".into(), ..PerpMarket::default() }.pair_label(), "BTC/USD");
        assert_eq!(num_size(12.34567), "12.346");
        assert_eq!(num_size(1234.5), "1,235");
        assert_eq!(until_text(3_600_000.0 * 3.0 + 12.0 * 60_000.0, 0.0), "3h 12m");
        assert_eq!(until_text(0.0, 5_000.0), "0m");
        let bars = by_volume(vec![PerpMarket { symbol: "A".into(), volume_usd: Some(1.0), ..PerpMarket::default() }, PerpMarket { symbol: "B".into(), volume_usd: Some(5.0), ..PerpMarket::default() }, PerpMarket { symbol: "C".into(), ..PerpMarket::default() }]);
        assert_eq!(bars.iter().map(|m| m.symbol.as_str()).collect::<Vec<_>>(), ["B", "A", "C"]);
    }
}
