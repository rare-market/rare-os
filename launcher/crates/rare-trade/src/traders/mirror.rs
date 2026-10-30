//! Mirror a trader's trade: the same direction on the same coin, at the size this account chose
//! (SUI per buy, a share of what it holds per sell). The swap goes through the normal Trade flow:
//! router quote, unsigned build, dry run in the review sheet, then Rare Wallet's own review.
//! Nothing here signs or sends.

use super::api::{TokenChip, TraderTrade};
use crate::amount::{base_to_number, from_base, min_out, normalize_type, pct_of, spendable, to_base, to_base_num, SLIPPAGE_BPS, SUI_TYPE, USDC_TYPE};
use crate::chain::{coin_balance, ChainReader};
use crate::http::{Result, TradeApiError};
use crate::jsnum::{js_number, js_string, to_fixed};
use crate::kv::Kv;
use crate::num::{num, NumKind};
use crate::swap::{QuoteParams, Swap, SwapBuild, SwapQuote};
use rare_market::net::Net;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorSize {
    /// SUI spent on each mirrored buy
    pub buy_sui: f64,
    /// share of the holding sold on each mirrored sell, percent
    pub sell_pct: f64,
}

pub const DEFAULT_SIZE: MirrorSize = MirrorSize { buy_sui: 5.0, sell_pct: 100.0 };
pub const BUY_SIZES: [f64; 5] = [1.0, 5.0, 10.0, 25.0, 50.0];
pub const SELL_SIZES: [f64; 3] = [25.0, 50.0, 100.0];

#[derive(Clone, Debug, PartialEq)]
pub enum MirrorPlan {
    Swap { buy: bool, coin_in: String, coin_out: String, amount_in: u128, decimals_in: u32 },
    No { reason: String },
}

fn no(reason: &str) -> MirrorPlan {
    MirrorPlan::No { reason: reason.into() }
}

/// The swap that mirrors a trade (`buy`, `coin`, its `price_usd`) at `size`.
/// - Buy a coin: pay `buy_sui` SUI for it. Buying SUI itself pays USDC worth `buy_sui` SUI.
/// - Sell a coin: sell `sell_pct` of what `held` says the account has, for SUI (SUI sells for USDC).
///
/// `held` is the account's balance of the trade's coin in base units (None when unknown).
pub fn mirror_plan(buy: bool, coin: &str, price_usd: f64, size: MirrorSize, coin_decimals: u32, held: Option<u128>) -> MirrorPlan {
    let coin = normalize_type(coin);
    let is_sui = coin == SUI_TYPE;
    if buy {
        if size.buy_sui.is_nan() || size.buy_sui <= 0.0 {
            return no("Choose a size first.");
        }
        if is_sui {
            if price_usd.is_nan() || price_usd <= 0.0 {
                return no("No price for SUI right now.");
            }
            return MirrorPlan::Swap { buy: true, coin_in: USDC_TYPE.into(), coin_out: SUI_TYPE.into(), amount_in: to_base(&to_fixed(size.buy_sui * price_usd, 6), 6), decimals_in: 6 };
        }
        return MirrorPlan::Swap { buy: true, coin_in: SUI_TYPE.into(), coin_out: coin, amount_in: to_base_num(size.buy_sui, 9), decimals_in: 9 };
    }
    let Some(held) = held else { return no("Checking your balance.") };
    let amount_in = pct_of(spendable(held, &coin), size.sell_pct);
    if amount_in == 0 {
        return no("You hold none of this coin.");
    }
    MirrorPlan::Swap { buy: false, coin_out: if is_sui { USDC_TYPE.into() } else { SUI_TYPE.into() }, coin_in: coin, amount_in, decimals_in: coin_decimals }
}

fn size_key(account: Option<&str>) -> String {
    format!("rare.traders.mirror.{}", account.map(str::to_lowercase).unwrap_or_else(|| "device".into()))
}

/// The account's mirror size as kept on the device, defaults for anything missing or bad.
pub fn read_size(account: Option<&str>, kv: &dyn Kv) -> MirrorSize {
    let v: Value = kv.get(&size_key(account)).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let n = |k: &str| v.get(k).map(crate::jsnum::value_number).filter(|x| *x > 0.0);
    MirrorSize { buy_sui: n("buySui").unwrap_or(DEFAULT_SIZE.buy_sui), sell_pct: n("sellPct").map(|p| p.min(100.0)).unwrap_or(DEFAULT_SIZE.sell_pct) }
}

/// The account's mirror size, kept on this device (the web launcher's `useMirrorSize`).
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorSizes {
    pub account: Option<String>,
    pub size: MirrorSize,
    loaded: bool,
}

impl Default for MirrorSizes {
    fn default() -> Self {
        MirrorSizes { account: None, size: DEFAULT_SIZE, loaded: false }
    }
}

impl MirrorSizes {
    pub fn use_account(&mut self, account: Option<&str>, kv: &dyn Kv) {
        let a = account.map(str::to_lowercase);
        if self.loaded && a == self.account {
            return;
        }
        self.size = read_size(a.as_deref(), kv);
        self.account = a;
        self.loaded = true;
    }

    pub fn set(&mut self, buy_sui: Option<f64>, sell_pct: Option<f64>, kv: &dyn Kv) {
        if let Some(b) = buy_sui {
            self.size.buy_sui = b;
        }
        if let Some(p) = sell_pct {
            self.size.sell_pct = p;
        }
        let v = serde_json::json!({ "buySui": crate::jsnum::json_number(self.size.buy_sui), "sellPct": crate::jsnum::json_number(self.size.sell_pct) });
        kv.set(&size_key(self.account.as_deref()), &v.to_string());
    }
}

/// Decimals and symbol for a coin: SUI and USDC known, anything else from the coin's page.
pub fn coin_info(net: &dyn Net, coin_type: &str) -> Result<(u32, String)> {
    let t = normalize_type(coin_type);
    if t == SUI_TYPE {
        return Ok((9, "SUI".into()));
    }
    if t == USDC_TYPE {
        return Ok((6, "USDC".into()));
    }
    let row: Value = crate::http::trade_get(net, &crate::api::token_paths::lean(&t))?;
    match row.get("decimals").and_then(Value::as_u64) {
        Some(d) => Ok((d as u32, row.get("symbol").and_then(Value::as_str).unwrap_or_default().to_string())),
        None => Err(TradeApiError::new(502, "invalid_answer", "Could not read this coin.")),
    }
}

/// Everything the mirror review sheet shows, and the unsigned swap.
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorReview {
    pub amount_in: u128,
    pub decimals_in: u32,
    pub in_symbol: String,
    pub out_symbol: String,
    pub out_decimals: u32,
    pub quote: SwapQuote,
    pub build: SwapBuild,
}

impl MirrorReview {
    /// "You pay"
    pub fn pay_line(&self) -> String {
        format!("{} {}", num(NumKind::Amount, base_to_number(self.amount_in, self.decimals_in)), self.in_symbol)
    }
    /// "You get about"
    pub fn get_line(&self) -> String {
        format!("{} {}", num(NumKind::Amount, base_to_number(self.quote.amount_out, self.out_decimals)), self.out_symbol)
    }
    /// "Minimum received"
    pub fn min_line(&self) -> String {
        format!("{} {}", num(NumKind::Amount, js_number(&from_base(min_out(self.quote.amount_out, SLIPPAGE_BPS), self.out_decimals))), self.out_symbol)
    }
}

/// The mirror sheet's build: plan the swap for `sender` at `size`, quote it, and build it unsigned.
pub fn prepare(net: &dyn Net, chain: &dyn ChainReader, swap: &Swap, trade: &TraderTrade, size: MirrorSize, sender: &str) -> Result<MirrorReview> {
    let (decimals, _) = coin_info(net, &trade.coin)?;
    let held = if trade.buy { None } else { Some(coin_balance(chain, sender, &trade.coin)?) };
    let (coin_in, coin_out, amount_in, decimals_in) = match mirror_plan(trade.buy, &trade.coin, trade.price_usd, size, decimals, held) {
        MirrorPlan::Swap { coin_in, coin_out, amount_in, decimals_in, .. } => (coin_in, coin_out, amount_in, decimals_in),
        MirrorPlan::No { reason } => return Err(TradeApiError::new(400, "mirror", reason)),
    };
    let (_, in_symbol) = coin_info(net, &coin_in)?;
    let (out_decimals, out_symbol) = coin_info(net, &coin_out)?;
    let quote = swap.quote(&QuoteParams { coin_in, coin_out, amount_in, owner: Some(sender.to_string()) })?;
    let build = swap.build(&quote, sender, SLIPPAGE_BPS, false)?;
    Ok(MirrorReview { amount_in, decimals_in, in_symbol, out_symbol, out_decimals, quote, build })
}

/// The sheet's title and lines about the trader's own trade.
pub fn mirror_words(trade: &TraderTrade, token: Option<&TokenChip>, trader_name: &str, size: MirrorSize) -> (String, String, String) {
    let sym = token.map(|t| t.symbol.clone()).unwrap_or_else(|| trade.coin.rsplit("::").next().unwrap_or("coin").to_string());
    let usd = num(NumKind::Money, trade.usd);
    let title = format!("Mirror {} of {sym}", if trade.buy { "buy" } else { "sell" });
    let your = if trade.buy { format!("{} SUI", js_string(size.buy_sui)) } else { format!("{}% of your {sym}", js_string(size.sell_pct)) };
    let sub = format!("{trader_name} {} {usd}. Your size: {your}.", if trade.buy { "bought" } else { "sold" });
    let line = format!("{} {usd} of {sym}", if trade.buy { "Bought" } else { "Sold" });
    (title, sub, line)
}

#[cfg(test)]
mod tests {
    //! mirror.test.ts
    use super::*;
    use crate::amount::GAS_RESERVE_MIST;
    use crate::kv::MemKv;

    const DEEP: &str = "0xdeeb7a4662eec9f2f3def03fb937a663dddaa2e215b8078a284d026b7946c270::deep::DEEP";
    const SIZE: MirrorSize = MirrorSize { buy_sui: 5.0, sell_pct: 50.0 };

    fn swap(buy: bool, coin_in: &str, coin_out: &str, amount_in: u128, decimals_in: u32) -> MirrorPlan {
        MirrorPlan::Swap { buy, coin_in: coin_in.into(), coin_out: coin_out.into(), amount_in, decimals_in }
    }

    #[test]
    fn a_buy_spends_the_chosen_sui_on_the_same_coin() {
        assert_eq!(mirror_plan(true, DEEP, 0.1, SIZE, 6, None), swap(true, SUI_TYPE, DEEP, 5_000_000_000, 9));
    }

    #[test]
    fn a_sell_sells_the_chosen_share_of_what_the_account_holds_for_sui() {
        assert_eq!(mirror_plan(false, DEEP, 0.1, SIZE, 6, Some(1_000_001)), swap(false, DEEP, SUI_TYPE, 500_000, 6));
        let MirrorPlan::Swap { amount_in, .. } = mirror_plan(false, DEEP, 0.1, MirrorSize { sell_pct: 100.0, ..SIZE }, 6, Some(7)) else { panic!() };
        assert_eq!(amount_in, 7);
    }

    #[test]
    fn nothing_held_or_not_known_yet_is_not_a_trade() {
        assert_eq!(mirror_plan(false, DEEP, 0.1, SIZE, 6, Some(0)), MirrorPlan::No { reason: "You hold none of this coin.".into() });
        assert!(matches!(mirror_plan(false, DEEP, 0.1, SIZE, 6, None), MirrorPlan::No { .. }));
    }

    #[test]
    fn sui_itself_trades_against_usdc_keeping_gas_back_on_a_sell() {
        assert_eq!(mirror_plan(true, "0x2::sui::SUI", 0.9, SIZE, 9, None), swap(true, USDC_TYPE, SUI_TYPE, 4_500_000, 6));
        let held = 10_000_000_000u128;
        assert_eq!(mirror_plan(false, SUI_TYPE, 0.9, MirrorSize { sell_pct: 100.0, ..SIZE }, 9, Some(held)), swap(false, SUI_TYPE, USDC_TYPE, held - GAS_RESERVE_MIST, 9));
    }

    #[test]
    fn no_size_no_trade() {
        assert!(matches!(mirror_plan(true, DEEP, 0.1, MirrorSize { buy_sui: 0.0, sell_pct: 50.0 }, 6, None), MirrorPlan::No { .. }));
    }

    #[test]
    fn sizes_are_kept_per_account() {
        let kv = MemKv::new();
        let mut s = MirrorSizes::default();
        s.use_account(Some("0xAB"), &kv);
        assert_eq!(s.size, DEFAULT_SIZE);
        s.set(Some(10.0), None, &kv);
        let mut t = MirrorSizes::default();
        t.use_account(Some("0xab"), &kv);
        assert_eq!(t.size, MirrorSize { buy_sui: 10.0, sell_pct: 100.0 });
        kv.set(&size_key(None), r#"{"buySui":-1,"sellPct":250}"#);
        assert_eq!(read_size(None, &kv), MirrorSize { buy_sui: 5.0, sell_pct: 100.0 });
    }

    #[test]
    fn prepares_the_review_for_a_sell() {
        use crate::chain::FakeChain;
        use rare_market::net::{Method, MockNet};
        use serde_json::json;
        use std::sync::Arc;
        let me = format!("0x{}", "a1".repeat(32));
        let n = MockNet::new();
        n.on_read_json(&format!("trade:{}", crate::api::token_paths::lean(DEEP)), json!({ "coinType": DEEP, "symbol": "DEEP", "decimals": 6 }));
        n.on_call(Method::Post, "https://rare.market/v1/trade/swap/quote", 200, json!({ "data": { "id": "q", "amountOut": "2000000000" } }).to_string());
        n.on_call(Method::Post, "https://rare.market/v1/trade/swap/build", 200, json!({ "data": { "kind": "AAE=" } }).to_string());
        let mut c = FakeChain::default();
        c.balances.insert((me.clone(), DEEP.to_string()), 3_000_000);
        let s = Swap::new(Arc::new(n.clone()), Arc::new(crate::swap::MemKeys::with(Some("k"))), Arc::new(|| Err(TradeApiError::network("no identity"))));
        let tr = TraderTrade { buy: false, coin: DEEP.into(), price_usd: 0.1, ..TraderTrade::default() };
        let r = prepare(&n, &c, &s, &tr, SIZE, &me).unwrap();
        assert_eq!(r.pay_line(), "1.5 DEEP");
        assert_eq!(r.get_line(), "2 SUI");
        assert_eq!(r.min_line(), "1.98 SUI");
        assert_eq!(r.build.tx.sender(), Some(me.as_str()));
        let c0 = FakeChain::default();
        assert_eq!(prepare(&n, &c0, &s, &tr, SIZE, &me).unwrap_err().message, "You hold none of this coin.");
    }

    #[test]
    fn sheet_words() {
        let tr = TraderTrade { buy: false, usd: 120.0, coin: DEEP.into(), ..TraderTrade::default() };
        let (title, sub, line) = mirror_words(&tr, None, "@whale", SIZE);
        assert_eq!(title, "Mirror sell of DEEP");
        assert_eq!(sub, "@whale sold $120.00. Your size: 50% of your DEEP.");
        assert_eq!(line, "Sold $120.00 of DEEP");
    }
}
