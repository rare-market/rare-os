//! Sui reads for the Trade tab. The reads themselves (gRPC in the web launcher) belong to
//! `rare-sui`; Trade asks for them through [`ChainReader`], which tests answer with a fake. What
//! stays here is Trade's own logic over those reads: the gas fix-up for router swaps, the dry-run
//! summary, and plain words for chain errors.

use crate::amount::{normalize_struct_tag, normalize_type};
use crate::http::{Result, TradeApiError};
use serde_json::{Map, Value};

/// A coin's on-chain metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoinMetadata {
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub icon_url: Option<String>,
}

/// The chain reads Trade needs. `rare-sui` implements it over gRPC.
pub trait ChainReader: Send + Sync {
    /// The wallet's whole balance of one coin (long coin type), base units.
    fn balance(&self, owner: &str, coin_type: &str) -> Result<u128>;
    /// Each object's type, in order (None for an object that does not exist or failed to load).
    fn object_types(&self, ids: &[String]) -> Result<Vec<Option<String>>>;
    /// A coin's metadata, None when the chain has none.
    fn coin_metadata(&self, coin_type: &str) -> Result<Option<CoinMetadata>>;
}

/// The wallet's whole balance of one coin, base units.
pub fn coin_balance(chain: &dyn ChainReader, owner: &str, coin_type: &str) -> Result<u128> {
    chain.balance(owner, &normalize_type(coin_type))
}

/// What a simulation of the transaction said, as `rare-sui` reads it from the effects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Simulation {
    /// None when there were no effects at all.
    pub success: Option<bool>,
    /// The chain's error when it failed.
    pub error: Option<String>,
    /// computation, storage, rebate (MIST).
    pub gas_used: Option<(u64, u64, u64)>,
    /// (address, coin type, signed amount) for every balance change.
    pub balance_changes: Vec<(String, String, i128)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BalanceChange {
    pub coin_type: String,
    pub amount: i128,
}

/// The check before handing a transaction to Rare Wallet: nothing is signed or sent.
#[derive(Clone, Debug, PartialEq)]
pub struct DryRun {
    pub ok: bool,
    pub error: Option<String>,
    /// coin type -> signed base units for the sender
    pub changes: Vec<BalanceChange>,
    pub gas_mist: Option<i128>,
    /// the chain's own words, for logs
    pub raw: Option<String>,
}

impl DryRun {
    /// The transaction could not even be built for the sender (gas selection, missing coins).
    pub fn build_failed(err: &str) -> DryRun {
        DryRun { ok: false, error: Some(plain_chain_error(err)), changes: Vec::new(), gas_mist: None, raw: Some(err.to_string()) }
    }

    /// The summary of a simulation for `sender`.
    pub fn from_simulation(sim: &Simulation, sender: &str) -> DryRun {
        let ok = sim.success == Some(true);
        let gas_mist = sim.gas_used.map(|(c, s, r)| c as i128 + s as i128 - r as i128);
        let me = rare_market::ptb::norm_addr(sender);
        let changes = sim
            .balance_changes
            .iter()
            .filter(|(a, _, _)| rare_market::ptb::norm_addr(a) == me)
            .map(|(_, t, amount)| BalanceChange { coin_type: normalize_type(t), amount: *amount })
            .collect();
        let failure = match sim.success {
            Some(false) => sim.error.clone().unwrap_or_default(),
            _ => "The transaction would fail".to_string(),
        };
        DryRun { ok, error: if ok { None } else { Some(plain_chain_error(&failure)) }, changes, gas_mist, raw: None }
    }
}

fn sui_coin() -> String {
    normalize_struct_tag("0x2::coin::Coin<0x2::sui::SUI>")
}

/// Every `{Input: n}` argument in the commands, rewritten by `f` (deep: vectors, nested arguments).
fn map_inputs(node: &Value, f: &dyn Fn(u64, &Map<String, Value>) -> Value) -> Value {
    match node {
        Value::Array(a) => Value::Array(a.iter().map(|x| map_inputs(x, f)).collect()),
        Value::Object(o) => {
            if let Some(n) = o.get("Input").and_then(Value::as_u64) {
                return f(n, o);
            }
            Value::Object(o.iter().map(|(k, v)| (k.clone(), map_inputs(v, f))).collect())
        }
        _ => node.clone(),
    }
}

/// Router transactions spend one of the wallet's own SUI coins as an input. A wallet that holds a
/// single SUI coin then has nothing left to pay gas with, so the build fails. Paying gas from that
/// same coin (it becomes the gas coin, and the trade splits from it) works for every wallet.
///
/// `tx` is the transaction's JSON (v2). Returns None when the transaction is left as it is (gas
/// already set, or no owned SUI coin among its inputs).
pub fn gas_from_spent_sui(tx: &Value, chain: &dyn ChainReader) -> Result<Option<Value>> {
    if tx.pointer("/gasData/payment").and_then(Value::as_array).is_some_and(|p| !p.is_empty()) {
        return Ok(None);
    }
    let inputs = tx.get("inputs").and_then(Value::as_array).cloned().unwrap_or_default();
    let owned: Vec<(usize, Value)> = inputs.iter().enumerate().filter_map(|(i, inp)| inp.pointer("/Object/ImmOrOwnedObject").map(|r| (i, r.clone()))).collect();
    if owned.is_empty() {
        return Ok(None);
    }
    let ids: Vec<String> = owned.iter().map(|(_, r)| r.get("objectId").and_then(Value::as_str).unwrap_or_default().to_string()).collect();
    let types = chain.object_types(&ids)?;
    let want = sui_coin();
    let Some((at, coin)) = owned.iter().enumerate().find(|(k, _)| types.get(*k).cloned().flatten().is_some_and(|t| normalize_struct_tag(&t) == want)).map(|(_, (i, r))| (*i, r.clone())) else {
        return Ok(None);
    };
    let at = at as u64;
    let mut out = tx.clone();
    let commands = tx.get("commands").cloned().unwrap_or(Value::Array(Vec::new()));
    out["commands"] = map_inputs(&commands, &|n, o| {
        if n == at {
            serde_json::json!({"GasCoin": true, "$kind": "GasCoin"})
        } else {
            let mut m = Map::new();
            m.insert("Input".into(), Value::from(if n > at { n - 1 } else { n }));
            if let Some(t) = o.get("type") {
                m.insert("type".into(), t.clone());
            }
            m.insert("$kind".into(), Value::from("Input"));
            Value::Object(m)
        }
    });
    let mut inputs = inputs;
    inputs.remove(at as usize);
    out["inputs"] = Value::Array(inputs);
    let payment = serde_json::json!([{ "objectId": coin.get("objectId"), "version": coin.get("version"), "digest": coin.get("digest") }]);
    match out.get_mut("gasData") {
        Some(g @ Value::Object(_)) => g["payment"] = payment,
        _ => out["gasData"] = serde_json::json!({"budget": null, "price": null, "owner": null, "payment": payment}),
    }
    Ok(Some(out))
}

fn has_word_after(raw: &str, needle: &str) -> bool {
    raw.match_indices(needle).any(|(i, _)| !raw[i + needle.len()..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_'))
}

fn insufficient_then_balance(lower: &str) -> bool {
    lower.lines().any(|l| l.find("insufficient").is_some_and(|i| l[i + 12..].contains("balance")))
}

/// Short, plain words for chain errors.
pub fn plain_chain_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("insufficient sui balance") || lower.contains("insufficientgas") || lower.contains("gas selection") {
        return "You need SUI for the network fee. Add SUI from Wallet, then Receive.".into();
    }
    if lower.contains("insufficientcoinbalance") || insufficient_then_balance(&lower) {
        return "Not enough balance for this trade.".into();
    }
    if has_word_after(raw, "abort code: 30") {
        return "Not enough margin for this order. Add funds or lower the size.".into();
    }
    if has_word_after(raw, "abort code: 41") {
        return "Your position in this market changed while the order was being built. Try again.".into();
    }
    if lower.contains("abort") {
        return "The trade would fail at current prices. Get a new quote.".into();
    }
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() > 160 {
        format!("{}...", chars[..157].iter().collect::<String>())
    } else {
        raw.to_string()
    }
}

/// [`plain_chain_error`] for an error that is not text (the web launcher printed it as JSON).
pub fn plain_chain_error_value(e: &Value) -> String {
    match e {
        Value::String(s) => plain_chain_error(s),
        other => plain_chain_error(&other.to_string()),
    }
}

/// A fake chain for tests (here and in the screens): balances, object types and metadata from maps;
/// `fail` makes every read fail.
#[derive(Default)]
pub struct FakeChain {
    pub balances: std::collections::HashMap<(String, String), u128>,
    pub types: std::collections::HashMap<String, String>,
    pub metadata: std::collections::HashMap<String, CoinMetadata>,
    pub fail: bool,
}

impl ChainReader for FakeChain {
    fn balance(&self, owner: &str, coin_type: &str) -> Result<u128> {
        if self.fail {
            return Err(TradeApiError::network("down"));
        }
        Ok(self.balances.get(&(owner.to_string(), coin_type.to_string())).copied().unwrap_or(0))
    }
    fn object_types(&self, ids: &[String]) -> Result<Vec<Option<String>>> {
        if self.fail {
            return Err(TradeApiError::network("down"));
        }
        Ok(ids.iter().map(|id| self.types.get(id).cloned()).collect())
    }
    fn coin_metadata(&self, coin_type: &str) -> Result<Option<CoinMetadata>> {
        if self.fail {
            return Err(TradeApiError::network("down"));
        }
        Ok(self.metadata.get(coin_type).cloned())
    }
}

#[cfg(test)]
mod tests {
    //! chain.test.ts
    use super::*;
    use serde_json::json;

    fn me() -> String {
        format!("0x{}", "d4".repeat(32))
    }
    fn coin() -> Value {
        json!({"objectId": format!("0x{}", "e5".repeat(32)), "version": "7", "digest": "Gj9jDVxbq7hU7DPdBcrcB1PLYjK3Eh7X6eo9kkJ2G3F"})
    }
    fn other() -> Value {
        json!({"objectId": format!("0x{}", "f6".repeat(32)), "version": "9", "digest": "Gj9jDVxbq7hU7DPdBcrcB1PLYjK3Eh7X6eo9kkJ2G3F"})
    }
    fn input(i: u64, ty: &str) -> Value {
        json!({"Input": i, "type": ty, "$kind": "Input"})
    }

    /// A router-style transaction as `toJSON()` prints it: an object input, a pure, the wallet's SUI
    /// coin split for the trade.
    fn router_tx() -> Value {
        json!({
            "version": 2, "sender": me(), "expiration": null,
            "gasData": {"budget": null, "price": null, "owner": null, "payment": null},
            "inputs": [
                {"Object": {"ImmOrOwnedObject": other()}, "$kind": "Object"},
                {"Pure": {"bytes": "9AEAAAAAAAA="}, "$kind": "Pure"},
                {"Object": {"ImmOrOwnedObject": coin()}, "$kind": "Object"},
                {"Pure": {"bytes": "1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NQ="}, "$kind": "Pure"}
            ],
            "commands": [
                {"SplitCoins": {"coin": input(2, "object"), "amounts": [input(1, "pure")]}, "$kind": "SplitCoins"},
                {"MoveCall": {"package": format!("0x{:0>64}", "2"), "module": "pay", "function": "keep", "typeArguments": [], "arguments": [input(0, "object"), {"NestedResult": [0, 0], "$kind": "NestedResult"}]}, "$kind": "MoveCall"},
                {"TransferObjects": {"objects": [input(2, "object")], "address": input(3, "pure")}, "$kind": "TransferObjects"}
            ]
        })
    }

    fn chain(types: &[(&Value, &str)]) -> FakeChain {
        let mut c = FakeChain::default();
        for (o, t) in types {
            c.types.insert(o["objectId"].as_str().unwrap().to_string(), t.to_string());
        }
        c
    }

    #[test]
    fn pays_gas_from_the_sui_coin_the_trade_spends_and_points_the_trade_at_the_gas_coin() {
        let (c, o) = (coin(), other());
        let out = gas_from_spent_sui(&router_tx(), &chain(&[(&c, "0x2::coin::Coin<0x2::sui::SUI>"), (&o, "0xabc::pool::Pool")])).unwrap().unwrap();
        assert_eq!(out["gasData"]["payment"], json!([coin()]));
        // pool, amount, recipient: the coin input is gone.
        assert_eq!(out["inputs"].as_array().unwrap().len(), 3);
        assert_eq!(out["inputs"][0]["Object"]["ImmOrOwnedObject"]["objectId"], other()["objectId"]);
        let gas = json!({"$kind": "GasCoin", "GasCoin": true});
        assert_eq!(out["commands"][0]["SplitCoins"]["coin"], gas);
        assert_eq!(out["commands"][0]["SplitCoins"]["amounts"][0]["Input"], 1);
        assert_eq!(out["commands"][1]["MoveCall"]["arguments"][0]["Input"], 0);
        assert_eq!(out["commands"][2]["TransferObjects"]["objects"][0], gas);
        // Inputs after the removed one move down by one.
        assert_eq!(out["commands"][2]["TransferObjects"]["address"]["Input"], 2);
        assert_eq!(out["sender"], me());
    }

    #[test]
    fn leaves_transactions_without_a_sui_coin_input_alone() {
        let (c, o) = (coin(), other());
        assert_eq!(gas_from_spent_sui(&router_tx(), &chain(&[(&c, "0x2::coin::Coin<0xdee::deep::DEEP>"), (&o, "0xabc::pool::Pool")])).unwrap(), None);
    }

    #[test]
    fn keeps_a_gas_payment_that_is_already_set() {
        let mut tx = router_tx();
        tx["gasData"]["payment"] = json!([other()]);
        let c = FakeChain { fail: true, ..FakeChain::default() };
        assert_eq!(gas_from_spent_sui(&tx, &c).unwrap(), None);
    }

    #[test]
    fn says_what_went_wrong_in_plain_words() {
        assert!(plain_chain_error("Unable to perform gas selection due to insufficient SUI balance").contains("need SUI for the network fee"));
        assert!(plain_chain_error("MoveAbort(... abort code: 30)").contains("margin"));
        assert!(plain_chain_error_value(&json!({"kind": "MoveAbort"})).contains("would fail"));
        assert!(plain_chain_error("abort code: 41").contains("changed"));
        assert_eq!(plain_chain_error("abort code: 300"), "The trade would fail at current prices. Get a new quote.");
        assert_eq!(plain_chain_error("InsufficientCoinBalance in command 0"), "Not enough balance for this trade.");
        assert_eq!(plain_chain_error(&"x".repeat(200)).chars().count(), 160);
    }

    #[test]
    fn dry_run_summary() {
        let sim = Simulation {
            success: Some(true),
            error: None,
            gas_used: Some((1_000, 2_000, 500)),
            balance_changes: vec![(me(), "0x2::sui::SUI".into(), -5), (format!("0x{}", "aa".repeat(32)), "0x2::sui::SUI".into(), 5)],
        };
        let r = DryRun::from_simulation(&sim, &me());
        assert!(r.ok && r.error.is_none());
        assert_eq!(r.gas_mist, Some(2_500));
        assert_eq!(r.changes, vec![BalanceChange { coin_type: crate::amount::SUI_TYPE.into(), amount: -5 }]);
        let failed = DryRun::from_simulation(&Simulation { success: Some(false), error: Some("MoveAbort in 1, abort code: 30".into()), ..Simulation::default() }, &me());
        assert_eq!(failed.error.as_deref(), Some("Not enough margin for this order. Add funds or lower the size."));
        let none = DryRun::from_simulation(&Simulation::default(), &me());
        assert_eq!(none.error.as_deref(), Some("The transaction would fail"));
        assert!(DryRun::build_failed("No valid gas coins found; gas selection failed").error.unwrap().contains("network fee"));
    }
}
