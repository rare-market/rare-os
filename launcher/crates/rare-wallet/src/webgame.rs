//! Web-game requests (Wallet Standard in a game window) as pure mappings: request -> the sign
//! call to make, sign result -> the reply value. Port of src/wallet/ui/sign/webgame.ts; the host
//! (rare-webgame's bridge and the review sheet) does the side effects.
//!
//! A game sends its transaction as Transaction JSON (`await tx.toJSON()`, version 2) or as built
//! bytes in base64. JSON becomes an unresolved PTB (`rare_sui::ptb::Tx`) plus the gas the game set;
//! the node resolves the rest at review time, as `tx.build()` does in the web launcher.

use crate::keystore::RareAccount;
use crate::sign::{SignError, SignResult, TxInput};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rare_sui::grpc::CoinRef;
use rare_sui::ptb::{norm_addr, Arg, Command, Input, MoveCall, Tx};
use rare_sui::tx::BuildOpts;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum GameCall {
    Connect,
    Transaction { tx: TxInput, execute: bool },
    Message(Vec<u8>),
}

fn arg(v: &Value) -> Result<Arg, String> {
    let bad = || "unsupported argument".to_string();
    if v.get("GasCoin").is_some() {
        return Ok(Arg::Gas);
    }
    if let Some(i) = v.get("Input").and_then(Value::as_u64) {
        return Ok(Arg::Input(u16::try_from(i).map_err(|_| bad())?));
    }
    if let Some(i) = v.get("Result").and_then(Value::as_u64) {
        return Ok(Arg::Result(u16::try_from(i).map_err(|_| bad())?));
    }
    if let Some(n) = v.get("NestedResult").and_then(Value::as_array) {
        let (c, i) = (n.first().and_then(Value::as_u64).ok_or_else(bad)?, n.get(1).and_then(Value::as_u64).ok_or_else(bad)?);
        return Ok(Arg::Nested(u16::try_from(c).map_err(|_| bad())?, u16::try_from(i).map_err(|_| bad())?));
    }
    Err(bad())
}

fn args(v: Option<&Value>) -> Result<Vec<Arg>, String> {
    v.and_then(Value::as_array).map(|a| a.iter().map(arg).collect()).unwrap_or_else(|| Ok(vec![]))
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn input(v: &Value) -> Result<Input, String> {
    if let Some(p) = v.get("Pure") {
        let b = p.get("bytes").and_then(Value::as_str).ok_or("bad pure input")?;
        return B64.decode(b).map(Input::Pure).map_err(|_| "bad pure input".to_string());
    }
    if let Some(o) = v.get("Object") {
        let inner = o.get("ImmOrOwnedObject").or_else(|| o.get("SharedObject")).or_else(|| o.get("Receiving")).ok_or("unsupported object input")?;
        let id = inner.get("objectId").and_then(Value::as_str).ok_or("bad object input")?;
        return Ok(Input::Object(norm_addr(id)));
    }
    if let Some(w) = v.get("FundsWithdrawal") {
        // the sender's address balance (a sponsor's or an allowance is not something a game asks
        // this wallet to sign for)
        if w.pointer("/withdrawFrom/Sender").is_none() {
            return Err("This game's transaction withdraws from an address balance other than this wallet's".into());
        }
        let amount = text(w.pointer("/reservation/MaxAmountU64")).and_then(|a| a.parse().ok()).ok_or("bad withdrawal input")?;
        let coin_type = rare_sui::ids::canon_type(w.pointer("/typeArg/Balance").and_then(Value::as_str).ok_or("bad withdrawal input")?);
        return Ok(Input::Withdrawal { coin_type, amount });
    }
    if let Some(o) = v.get("UnresolvedObject") {
        let id = o.get("objectId").and_then(Value::as_str).ok_or("bad object input")?;
        return Ok(Input::Object(norm_addr(id)));
    }
    // UnresolvedPure carries a JS value without its Move type: only the node's function
    // signatures could encode it, which the web launcher's build does and this port does not.
    Err("This game's transaction has an untyped value Rare Wallet cannot read".into())
}

fn command(v: &Value) -> Result<Command, String> {
    if let Some(m) = v.get("MoveCall") {
        let s = |k: &str| m.get(k).and_then(Value::as_str).map(str::to_string).ok_or(format!("bad move call ({k})"));
        let type_args = m.get("typeArguments").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        return Ok(Command::MoveCall(MoveCall { package: norm_addr(&s("package")?), module: s("module")?, function: s("function")?, type_args, args: args(m.get("arguments"))? }));
    }
    if let Some(t) = v.get("TransferObjects") {
        return Ok(Command::TransferObjects { objects: args(t.get("objects"))?, address: arg(t.get("address").ok_or("bad transfer")?)? });
    }
    if let Some(t) = v.get("SplitCoins") {
        return Ok(Command::SplitCoins { coin: arg(t.get("coin").ok_or("bad split")?)?, amounts: args(t.get("amounts"))? });
    }
    if let Some(t) = v.get("MergeCoins") {
        return Ok(Command::MergeCoins { destination: arg(t.get("destination").ok_or("bad merge")?)?, sources: args(t.get("sources"))? });
    }
    if let Some(t) = v.get("MakeMoveVec") {
        return Ok(Command::MakeMoveVec { type_: t.get("type").and_then(Value::as_str).map(str::to_string), elements: args(t.get("elements"))? });
    }
    if let Some(i) = v.get("$Intent") {
        if i.get("name").and_then(Value::as_str) == Some("CoinWithBalance") {
            let d = i.get("data").ok_or("bad coin intent")?;
            let ty = d.get("type").and_then(Value::as_str).ok_or("bad coin intent")?;
            let balance = text(d.get("balance")).and_then(|b| b.parse().ok()).ok_or("bad coin intent")?;
            // "gas": SUI from the gas coin (useGasCoin, the default); SUI by type: from coins and
            // the address balance only
            let (coin_type, use_gas) = if ty == "gas" { (rare_sui::ids::SUI_TYPE.to_string(), true) } else { (rare_sui::ids::canon_type(ty), false) };
            let as_balance = d.get("outputKind").and_then(Value::as_str) == Some("balance");
            return Ok(Command::CoinWithBalance { coin_type, balance, use_gas, as_balance });
        }
        return Err("This game's transaction uses a step Rare Wallet does not support yet".into());
    }
    Err("This game's transaction uses a step Rare Wallet does not support yet".into())
}

/// Transaction JSON (version 2) -> the PTB and the gas the game set.
pub fn parse_transaction_json(j: &Value) -> Result<(Tx, BuildOpts), String> {
    if j.get("version").and_then(Value::as_u64) != Some(2) {
        return Err("Unsupported transaction format".into());
    }
    let mut tx = Tx::new();
    if let Some(s) = j.get("sender").and_then(Value::as_str) {
        tx.set_sender(s);
    }
    tx.inputs = j.get("inputs").and_then(Value::as_array).map(|a| a.iter().map(input).collect::<Result<_, _>>()).unwrap_or_else(|| Ok(vec![]))?;
    tx.commands = j.get("commands").and_then(Value::as_array).map(|a| a.iter().map(command).collect::<Result<_, _>>()).unwrap_or_else(|| Ok(vec![]))?;
    let g = j.get("gasData").cloned().unwrap_or(Value::Null);
    let num = |k: &str| text(g.get(k)).and_then(|s| s.parse::<u64>().ok());
    let owner = g.get("owner").and_then(Value::as_str).map(norm_addr);
    let payment = g
        .get("payment")
        .and_then(Value::as_array)
        .map(|p| {
            p.iter()
                .map(|o| {
                    Some(CoinRef {
                        object_id: norm_addr(o.get("objectId")?.as_str()?),
                        version: text(o.get("version"))?.parse().ok()?,
                        digest: o.get("digest")?.as_str()?.to_string(),
                        balance: 0,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or("bad gas payment")
        })
        .transpose()?
        .unwrap_or_default();
    let sponsor = owner.filter(|o| Some(o) != tx.sender.as_ref());
    Ok((tx, BuildOpts { gas_budget: num("budget"), gas_price: num("price"), gas_payment: payment, sponsor }))
}

/// The game's transaction: Transaction JSON becomes a PTB, base64 stays the built bytes.
pub fn parse_game_transaction(raw: &str) -> Result<TxInput, String> {
    let s = raw.trim();
    if s.starts_with('{') {
        let j: Value = serde_json::from_str(s).map_err(|_| "Could not read the game's request".to_string())?;
        let (tx, opts) = parse_transaction_json(&j)?;
        return Ok(if opts == BuildOpts::default() { TxInput::Ptb(tx) } else { TxInput::PtbGas(tx, opts) });
    }
    B64.decode(s).map(TxInput::Bytes).map_err(|_| "Could not read the game's request".into())
}

/// The sender a game wrote in its transaction (normalised), if any: the PTB's, or the one in
/// built TransactionData bytes. None for bytes that are not TransactionData (the review refuses them).
pub fn tx_sender(tx: &TxInput) -> Option<String> {
    match tx {
        TxInput::Ptb(t) | TxInput::PtbGas(t, _) => t.sender.as_deref().map(norm_addr),
        TxInput::Bytes(b) => bcs::from_bytes::<sui_sdk_types::Transaction>(b).ok().map(|t| norm_addr(&t.sender.to_string())),
    }
}

/// The game's transaction is for `account` (or names no sender). Error text for the game otherwise.
pub fn check_sender(tx: &TxInput, account: &str) -> Result<(), String> {
    match tx_sender(tx) {
        Some(s) if s != norm_addr(account) => Err("This transaction is for a different account".into()),
        _ => Ok(()),
    }
}

/// A request -> the call to make, or the error text the game gets.
pub fn map_game_request(method: &str, params: &Value) -> Result<GameCall, String> {
    match method {
        "connect" => Ok(GameCall::Connect),
        "signTransaction" | "signAndExecuteTransaction" => {
            let raw = params.get("transaction").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("The game did not send a transaction")?;
            let tx = parse_game_transaction(raw).map_err(|e| if e.contains("support") || e.contains("untyped") { e } else { "Could not read the game's request".into() })?;
            Ok(GameCall::Transaction { tx, execute: method == "signAndExecuteTransaction" })
        }
        "signPersonalMessage" => {
            let raw = params.get("message").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("The game did not send a message")?;
            B64.decode(raw).map(GameCall::Message).map_err(|_| "Could not read the game's request".into())
        }
        _ => Err("This request is not supported".into()),
    }
}

/// What goes back to the game for a finished signature.
pub fn reply_value(call: &GameCall, r: &SignResult) -> Value {
    let mut v = json!({ "bytes": r.bytes, "signature": r.signature });
    if let GameCall::Transaction { execute: true, .. } = call {
        v["digest"] = json!(r.digest.clone().unwrap_or_default());
        v["effects"] = json!(r.effects.clone().unwrap_or_default());
    }
    v
}

/// The error text the game sees (it only gets a string).
pub fn error_text(e: &SignError) -> String {
    match e {
        SignError::Rejected(_) => "User rejected the request".into(),
        other => other.to_string(),
    }
}

/// An origin normalised for the approved list.
pub fn norm_origin(o: &str) -> String {
    o.trim().trim_end_matches('/').to_lowercase()
}

/// A connect from an origin the player already approved is answered without a sheet: with the
/// selected account (or the first one). None when a sheet is needed.
pub fn silent_connect_account<'a>(approved: &[String], origin: &str, accounts: &'a [RareAccount], current: Option<&RareAccount>) -> Option<&'a RareAccount> {
    if !approved.contains(&norm_origin(origin)) || accounts.is_empty() {
        return None;
    }
    current.and_then(|c| accounts.iter().find(|a| a.address == c.address)).or(accounts.first())
}

#[cfg(test)]
mod tests;
