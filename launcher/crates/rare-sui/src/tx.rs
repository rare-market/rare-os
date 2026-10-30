//! Transaction building: an unresolved [`Tx`] to signable transaction data, over MystenLabs'
//! `sui-transaction-builder`. The fullnode resolves what the PTB leaves open (owned, immutable,
//! shared and receiving object inputs with mutability from the Move signatures, gas selection and
//! budget) in a simulation, exactly as `@mysten/sui`'s gRPC `build()` does, and the simulation must
//! succeed. This crate never signs: the wallet (rare-wallet) signs [`Built::bytes`] and calls
//! [`Sui::execute`].
//!
//! Also the coin send builder (`buildCoinSend` in src/wallet/tx/coin.ts).

use std::str::FromStr;

use sui_sdk_types::{Address, Digest, Identifier, TypeTag};
use sui_transaction_builder::intent::{Balance, Coin};
use sui_transaction_builder::{Argument, Function, ObjectInput, TransactionBuilder};

use crate::grpc::{ChainRead, CoinRef, Sui, MAX_GAS_COINS};
use crate::ids::{canon_addr, is_sui, SUI_TYPE};
use crate::ptb::{Arg, Command, Input, Tx};
use crate::{Error, Result};

/// Gas and sponsorship for a build. Left empty, the node picks the price, the budget and the coins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildOpts {
    pub gas_budget: Option<u64>,
    pub gas_price: Option<u64>,
    /// Pay with exactly these coins (owned by the sender, or by the sponsor when set).
    pub gas_payment: Vec<CoinRef>,
    pub sponsor: Option<String>,
}

/// A built transaction, ready to sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    pub data: sui_sdk_types::Transaction,
    /// BCS of the TransactionData: what gets signed (with the TransactionData intent) and sent.
    pub bytes: Vec<u8>,
    pub digest: String,
}

impl Built {
    pub fn from_data(data: sui_sdk_types::Transaction) -> Result<Self> {
        let bytes = bcs::to_bytes(&data).map_err(|e| Error::Build(e.to_string()))?;
        let digest = data.digest().to_string();
        Ok(Built { data, bytes, digest })
    }

    /// The base64 bytes the wallet's review and the TS signer take.
    pub fn base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.bytes)
    }
}

fn type_tag(t: &str) -> Result<TypeTag> {
    TypeTag::from_str(t.trim()).map_err(|_| Error::invalid(format!("not a Move type: {t}")))
}

fn ident(s: &str) -> Result<Identifier> {
    Identifier::from_str(s).map_err(|_| Error::invalid(format!("not a Move identifier: {s}")))
}

fn gas_object(c: &CoinRef) -> Result<ObjectInput> {
    let digest = Digest::from_str(&c.digest).map_err(|_| Error::invalid(format!("bad object digest {}", c.digest)))?;
    Ok(ObjectInput::owned(crate::ids::address(&c.object_id)?, c.version, digest))
}

/// A coin reservation ref's digest (base58): an address balance amount dressed as an object ref,
/// its last 20 bytes 0xac. There is no object behind it.
pub fn is_coin_reservation(digest: &str) -> bool {
    Digest::from_str(digest).is_ok_and(|d| d.inner()[12..32] == [0xac; 20])
}

/// The builder for a PTB: inputs first in their order (so input indices match the TS layout),
/// then the commands; gas options applied.
pub fn to_builder(tx: &Tx, opts: &BuildOpts) -> Result<TransactionBuilder> {
    let mut b = TransactionBuilder::new();
    let sender = tx.sender.as_deref().ok_or_else(|| Error::Build("the transaction has no sender".into()))?;
    b.set_sender(crate::ids::address(sender)?);
    if let Some(s) = &opts.sponsor {
        b.set_sponsor(crate::ids::address(s)?);
    }
    if let Some(v) = opts.gas_budget {
        b.set_gas_budget(v);
    }
    if let Some(v) = opts.gas_price {
        b.set_gas_price(v);
    }
    // a payment that names a coin reservation (a game's SDK backing the gas coin with the address
    // balance) is left to the node, which picks the coins and the reservation again and sets the
    // validity window such a payment needs
    if !opts.gas_payment.is_empty() && !opts.gas_payment.iter().any(|c| is_coin_reservation(&c.digest)) {
        let objs = opts.gas_payment.iter().take(MAX_GAS_COINS).map(gas_object).collect::<Result<Vec<_>>>()?;
        b.add_gas_objects(objs);
    }

    let mut inputs: Vec<Argument> = Vec::with_capacity(tx.inputs.len());
    for i in &tx.inputs {
        inputs.push(match i {
            Input::Pure(bytes) => b.pure_bytes_unique(bytes.clone()),
            Input::Object(id) => b.object(ObjectInput::new(crate::ids::address(id)?)),
            Input::Withdrawal { coin_type, amount } => b.funds_withdrawal(type_tag(coin_type)?, *amount),
        });
    }
    // A command's result handle; None for commands without results (merge, transfer).
    let mut results: Vec<Option<Argument>> = Vec::with_capacity(tx.commands.len());
    let gas = b.gas();
    let arg = |a: Arg, results: &[Option<Argument>]| -> Result<Argument> {
        let res = |c: u16| results.get(c as usize).copied().flatten().ok_or_else(|| Error::Build(format!("result {c} is not a value")));
        Ok(match a {
            Arg::Gas => gas,
            Arg::Input(i) => *inputs.get(i as usize).ok_or_else(|| Error::Build(format!("input {i} out of range")))?,
            Arg::Result(c) => res(c)?,
            // to_nested replaces the sub index, so any handle of command c gives NestedResult(c, i)
            Arg::Nested(c, i) => res(c)?.to_nested(i as usize + 1)[i as usize],
        })
    };
    let args = |v: &[Arg], results: &[Option<Argument>]| v.iter().map(|a| arg(*a, results)).collect::<Result<Vec<_>>>();

    for c in &tx.commands {
        let out = match c {
            Command::MoveCall(m) => {
                let f = Function::new(crate::ids::address(&m.package)?, ident(&m.module)?, ident(&m.function)?).with_type_args(m.type_args.iter().map(|t| type_tag(t)).collect::<Result<Vec<_>>>()?);
                let a = args(&m.args, &results)?;
                Some(b.move_call(f, a))
            }
            Command::SplitCoins { coin, amounts } => {
                let (coin, amounts) = (arg(*coin, &results)?, args(amounts, &results)?);
                // split_coins hands back NestedResult handles; `Result(c)` of a one-amount split
                // is the same value as NestedResult(c, 0)
                b.split_coins(coin, amounts).first().copied()
            }
            Command::MergeCoins { destination, sources } => {
                let (d, s) = (arg(*destination, &results)?, args(sources, &results)?);
                b.merge_coins(d, s);
                None
            }
            Command::TransferObjects { objects, address } => {
                let (o, a) = (args(objects, &results)?, arg(*address, &results)?);
                b.transfer_objects(o, a);
                None
            }
            Command::MakeMoveVec { type_, elements } => {
                let t = type_.as_deref().map(type_tag).transpose()?;
                let e = args(elements, &results)?;
                Some(b.make_move_vec(t, e))
            }
            Command::CoinWithBalance { coin_type, balance, use_gas, as_balance } => {
                // the gas coin of a sponsored transaction is the sponsor's: SUI comes from the
                // sender's own coins and address balance instead
                let use_gas = *use_gas && opts.sponsor.is_none();
                let t = crate::ids::struct_tag(coin_type)?;
                Some(if *as_balance { b.intent(Balance::new(t, *balance).with_use_gas_coin(use_gas)) } else { b.intent(Coin::new(t, *balance).with_use_gas_coin(use_gas)) })
            }
        };
        results.push(out);
    }
    Ok(b)
}

/// Resolves and builds `tx` against the node (a simulation that must succeed).
///
/// Address balances: with no gas coins named, the node pays from the sender's SUI address balance
/// when the gas coin is unused, and backs the gas coin with the address balance (a coin
/// reservation, plus any SUI coins) when it is used. The node's budget estimate misses the storage
/// of a gas coin made from a reservation and then sent away (max sends, "move everything"), so an
/// out-of-gas build without a set budget is tried again with larger budgets (only the real cost
/// is charged).
pub async fn build(sui: &Sui, tx: &Tx, opts: &BuildOpts) -> Result<Built> {
    let first = build_once(sui, tx, opts).await;
    let out_of_gas = |e: &Error| {
        let m = e.to_string();
        m.contains("INSUFFICIENT_GAS") || m.contains("InsufficientGas")
    };
    match first {
        Err(e) if opts.gas_budget.is_none() && out_of_gas(&e) => {
            let price = match opts.gas_price {
                Some(p) => p,
                None => sui.reference_gas_price().await?,
            };
            let owner = opts.sponsor.clone().or_else(|| tx.sender.clone()).unwrap_or_default();
            let have = sui.balance(&owner, SUI_TYPE).await.map(|b| b.balance).unwrap_or(u64::MAX);
            let mut last = e;
            for units in retry_budget_units() {
                let budget = price.saturating_mul(units).min(have);
                match build_once(sui, tx, &BuildOpts { gas_budget: Some(budget), ..opts.clone() }).await {
                    Ok(b) => return Ok(b),
                    Err(e) if out_of_gas(&e) && budget < have => last = e,
                    Err(e) => return Err(e),
                }
            }
            Err(last)
        }
        other => other,
    }
}

/// Gas units for the out-of-gas retries (times the gas price): 0.002, 0.01 and 0.05 SUI at a
/// price of 100 MIST.
fn retry_budget_units() -> [u64; 3] {
    [20_000, 100_000, 500_000]
}

async fn build_once(sui: &Sui, tx: &Tx, opts: &BuildOpts) -> Result<Built> {
    let b = to_builder(tx, opts)?;
    let mut rpc = sui.rpc();
    Built::from_data(b.build(&mut rpc).await?)
}

/// Builds `tx` for `sender`, signs the bytes with `sign` (answers a base64 serialized signature)
/// and executes it. For keys the launcher holds itself (the cloud saves writer), never the wallet.
/// Answers the digest; a failed transaction is an error.
pub async fn execute_signed(sui: &Sui, tx: &Tx, sender: &str, sign: &(dyn Fn(&[u8]) -> String + Sync)) -> Result<String> {
    let mut t = tx.clone();
    t.set_sender(sender);
    let built = build(sui, &t, &BuildOpts::default()).await?;
    let sig = sui_sdk_types::UserSignature::from_base64(&sign(&built.bytes)).map_err(|e| Error::invalid(e.to_string()))?;
    let done = sui.execute(&built.data, vec![sig]).await?;
    let ok = done.effects.as_ref().and_then(|e| e.status.as_ref()).and_then(|s| s.success).unwrap_or(false);
    if !ok {
        return Err(Error::invalid(format!("transaction failed ({})", built.digest)));
    }
    Ok(done.digest.clone().unwrap_or(built.digest))
}

/// Builds offline: every object input must be a gas coin or pure; the gas price, budget and
/// payment must be set. For fully specified transactions (and tests).
pub fn build_offline(tx: &Tx, opts: &BuildOpts) -> Result<Built> {
    Built::from_data(to_builder(tx, opts)?.try_build()?)
}

// ---------- coin send (src/wallet/tx/coin.ts) ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendErrorCode {
    BadAddress,
    SameAddress,
    BadAmount,
    NoBalance,
    FeeReserve,
}

/// Bad input to a send, with the plain message the sheet shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendError {
    pub code: SendErrorCode,
    pub message: String,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SendError {}

fn send_err(code: SendErrorCode, m: &str) -> SendError {
    SendError { code, message: m.to_string() }
}

/// A 0x address of up to 64 hex digits, normalised.
pub fn check_address(a: &str) -> std::result::Result<String, SendError> {
    let v = a.trim();
    let ok = v.len() > 2 && v.len() <= 66 && (v.starts_with("0x") || v.starts_with("0X")) && v[2..].bytes().all(|b| b.is_ascii_hexdigit());
    if !ok {
        return Err(send_err(SendErrorCode::BadAddress, "Enter a valid Sui address"));
    }
    Ok(canon_addr(v))
}

#[derive(Clone, Debug, Default)]
pub struct CoinSend {
    pub sender: String,
    pub to: String,
    pub coin_type: String,
    /// Base units; ignored when `max` is set.
    pub amount: u64,
    /// Send the whole balance (SUI: minus the network fee).
    pub max: bool,
    /// Known balance in base units; read over gRPC when None.
    pub balance: Option<u64>,
}

#[derive(Debug)]
pub enum CoinSendError {
    Send(SendError),
    Chain(Error),
}

impl From<SendError> for CoinSendError {
    fn from(e: SendError) -> Self {
        CoinSendError::Send(e)
    }
}
impl From<Error> for CoinSendError {
    fn from(e: Error) -> Self {
        CoinSendError::Chain(e)
    }
}
impl std::fmt::Display for CoinSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoinSendError::Send(e) => e.fmt(f),
            CoinSendError::Chain(e) => e.fmt(f),
        }
    }
}

/// A coin transfer. SUI is split from the gas coin; "max" SUI pays with every SUI coin (and the
/// address balance) and sends the gas coin itself, so the recipient gets everything minus the exact
/// fee. Other coins use coin-with-balance: the address balance first, else merged and split coins.
/// SUI held only in the address balance works too: the node backs the gas coin with it.
pub async fn build_coin_send(chain: &dyn ChainRead, p: &CoinSend) -> std::result::Result<(Tx, BuildOpts), CoinSendError> {
    let sender = check_address(&p.sender)?;
    let to = check_address(&p.to)?;
    if to == sender {
        return Err(send_err(SendErrorCode::SameAddress, "Pick an address other than this account").into());
    }
    if !p.max && p.amount == 0 {
        return Err(send_err(SendErrorCode::BadAmount, "Enter an amount above zero").into());
    }
    let sui = is_sui(&p.coin_type);
    let balance = match p.balance {
        Some(b) => b,
        None => chain.balance(&sender, &p.coin_type).await?.balance,
    };
    if balance == 0 {
        return Err(send_err(SendErrorCode::NoBalance, "Nothing to send").into());
    }
    if !p.max && p.amount > balance {
        return Err(send_err(SendErrorCode::NoBalance, "Not enough balance").into());
    }
    // splitting all of it out of the gas coin leaves nothing for the fee
    if sui && !p.max && p.amount >= balance {
        return Err(send_err(SendErrorCode::FeeReserve, "Leave some SUI for the network fee, or use Max").into());
    }

    let mut tx = Tx::new();
    tx.set_sender(&sender);
    let mut opts = BuildOpts::default();
    if sui && p.max {
        // SUI in the address balance: the node backs the gas coin with all of it (a coin
        // reservation) plus every SUI coin, so sending the gas coin sends everything less the fee
        let in_address = chain.balance(&sender, SUI_TYPE).await?.address_balance.unwrap_or(0);
        if in_address == 0 {
            let coins = chain.list_coins(&sender, SUI_TYPE, MAX_GAS_COINS).await?;
            if coins.is_empty() {
                return Err(send_err(SendErrorCode::NoBalance, "Nothing to send").into());
            }
            opts.gas_payment = coins;
        }
        tx.transfer_to(&[Arg::Gas], &to);
    } else if sui {
        let c = tx.split_u64(Arg::Gas, p.amount);
        tx.transfer_to(&[c], &to);
    } else {
        let c = tx.coin_with_balance(&p.coin_type, if p.max { balance } else { p.amount });
        tx.transfer_to(&[c], &to);
    }
    Ok((tx, opts))
}

/// An address for the builder APIs.
pub fn parse_address(a: &str) -> Result<Address> {
    crate::ids::address(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::CoinBalance;
    use crate::BoxFut;
    use serde_json::Value;

    fn me() -> String {
        format!("0x{}", "a1".repeat(32))
    }
    fn bob() -> String {
        format!("0x{}", "b2".repeat(32))
    }
    const USDC: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
    const DIGEST: &str = "4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi";

    struct Coins {
        balance: u64,
        coins: usize,
        /// of `balance`, how much is in the address balance
        ab: u64,
    }
    impl ChainRead for Coins {
        fn balance(&self, _o: &str, t: &str) -> BoxFut<'_, CoinBalance> {
            let r = CoinBalance { coin_type: t.into(), balance: self.balance, coin_balance: Some(self.balance - self.ab), address_balance: Some(self.ab) };
            Box::pin(async move { Ok(r) })
        }
        fn list_coins(&self, _o: &str, _t: &str, max: usize) -> BoxFut<'_, Vec<CoinRef>> {
            let v = (0..self.coins.min(max)).map(|i| CoinRef { object_id: canon_addr(&format!("0x{:x}", i + 1)), version: 1, digest: DIGEST.into(), balance: 1 }).collect();
            Box::pin(async move { Ok(v) })
        }
    }
    fn kinds(tx: &Tx) -> Vec<String> {
        tx.to_json()["commands"].as_array().unwrap().iter().map(|c| c["$kind"].as_str().unwrap().to_string()).collect()
    }
    fn send(to: String, coin_type: &str, amount: u64) -> CoinSend {
        CoinSend { sender: me(), to, coin_type: coin_type.into(), amount, ..Default::default() }
    }

    // tx.test.ts "buildCoinSend"
    #[tokio::test]
    async fn sui_split_from_gas_and_transfer_sender_set() {
        let (tx, _) = build_coin_send(&Coins { balance: 5_000_000_000, coins: 3, ab: 0 }, &send(bob(), "0x2::sui::SUI", 1_000_000_000)).await.unwrap();
        let d = tx.to_json();
        assert_eq!(d["sender"], Value::String(me()));
        assert_eq!(kinds(&tx), ["SplitCoins", "TransferObjects"]);
        assert_eq!(d["commands"][0]["SplitCoins"]["coin"], serde_json::json!({ "$kind": "GasCoin", "GasCoin": true }));
    }

    #[tokio::test]
    async fn sui_max_pays_with_every_sui_coin_and_sends_the_gas_coin_itself() {
        let mut p = send(bob(), SUI_TYPE, 0);
        p.max = true;
        let (tx, opts) = build_coin_send(&Coins { balance: 5_000_000_000, coins: 3, ab: 0 }, &p).await.unwrap();
        assert_eq!(kinds(&tx), ["TransferObjects"]);
        assert_eq!(tx.to_json()["commands"][0]["TransferObjects"]["objects"], serde_json::json!([{ "$kind": "GasCoin", "GasCoin": true }]));
        assert_eq!(opts.gas_payment.len(), 3);
        // and it builds offline once price and budget are known
        let built = build_offline(&tx, &BuildOpts { gas_budget: Some(5_000_000), gas_price: Some(1000), ..opts }).unwrap();
        assert_eq!(built.data.gas_payment.objects.len(), 3);
        assert!(!built.bytes.is_empty());
    }

    #[tokio::test]
    async fn sui_max_from_the_address_balance_lets_the_node_back_the_gas_coin() {
        let mut p = send(bob(), SUI_TYPE, 0);
        p.max = true;
        // only in the address balance (no coin objects), and coins plus address balance
        for c in [Coins { balance: 5_000_000_000, coins: 0, ab: 5_000_000_000 }, Coins { balance: 5_000_000_000, coins: 2, ab: 3_000_000_000 }] {
            let (tx, opts) = build_coin_send(&c, &p).await.unwrap();
            assert_eq!(tx.to_json()["commands"][0]["TransferObjects"]["objects"], serde_json::json!([{ "$kind": "GasCoin", "GasCoin": true }]));
            assert!(opts.gas_payment.is_empty());
        }
    }

    #[test]
    fn withdrawals_from_the_address_balance() {
        let mut tx = Tx::new();
        tx.set_sender("0x1");
        let c = tx.withdraw_coin(USDC, 250);
        tx.transfer_to(&[c], "0x2");
        let j = tx.to_json();
        assert_eq!(j["inputs"][0]["FundsWithdrawal"]["reservation"]["MaxAmountU64"], "250");
        assert_eq!(j["inputs"][0]["FundsWithdrawal"]["typeArg"]["Balance"], crate::ids::canon_type(USDC));
        assert_eq!(j["commands"][0]["MoveCall"]["function"], "redeem_funds");
        assert_eq!(j["commands"][0]["MoveCall"]["arguments"][0]["type"], "withdrawal");
        let gas = vec![CoinRef { object_id: canon_addr("0x9"), version: 1, digest: DIGEST.into(), balance: 1 }];
        let built = build_offline(&tx, &BuildOpts { gas_budget: Some(1), gas_price: Some(1), gas_payment: gas, sponsor: None }).unwrap();
        let sui_sdk_types::TransactionKind::ProgrammableTransaction(p) = &built.data.kind else { panic!() };
        match &p.inputs[0] {
            sui_sdk_types::Input::FundsWithdrawal(w) => {
                assert_eq!(w.amount(), Some(250));
                assert_eq!(w.coin_type().to_string(), crate::ids::canon_type(USDC));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sui_coin_intents_print_like_the_ts_sdk() {
        let mut tx = Tx::new();
        tx.coin_with_balance(SUI_TYPE, 5);
        tx.coin_with_balance(USDC, 6);
        let j = tx.to_json();
        assert_eq!(j["commands"][0]["$Intent"]["data"]["type"], "gas");
        assert_eq!(j["commands"][1]["$Intent"]["data"]["type"], crate::ids::canon_type(USDC));
    }

    #[tokio::test]
    async fn other_coins_use_coin_with_balance() {
        let mut p = send(bob(), USDC, 2_000_000);
        p.balance = Some(5_000_000);
        let (tx, _) = build_coin_send(&Coins { balance: 0, coins: 0, ab: 0 }, &p).await.unwrap();
        assert_eq!(kinds(&tx), ["$Intent", "TransferObjects"]);
    }

    #[tokio::test]
    async fn validates_address_amount_and_balance() {
        let c = Coins { balance: 0, coins: 0, ab: 0 };
        let base = || CoinSend { sender: me(), to: bob(), coin_type: USDC.into(), amount: 1, balance: Some(10), max: false };
        let code = |r: std::result::Result<(Tx, BuildOpts), CoinSendError>| match r {
            Err(CoinSendError::Send(e)) => e,
            other => panic!("{:?}", other.map(|_| ())),
        };
        assert_eq!(code(build_coin_send(&c, &CoinSend { to: "0x12zz".into(), ..base() }).await).code, SendErrorCode::BadAddress);
        assert_eq!(code(build_coin_send(&c, &CoinSend { to: me(), ..base() }).await).code, SendErrorCode::SameAddress);
        assert_eq!(code(build_coin_send(&c, &CoinSend { amount: 0, ..base() }).await).code, SendErrorCode::BadAmount);
        assert_eq!(code(build_coin_send(&c, &CoinSend { amount: 11, ..base() }).await).message, "Not enough balance");
        assert_eq!(code(build_coin_send(&c, &CoinSend { balance: Some(0), max: true, ..base() }).await).message, "Nothing to send");
        assert_eq!(code(build_coin_send(&c, &CoinSend { coin_type: SUI_TYPE.into(), amount: 10, ..base() }).await).code, SendErrorCode::FeeReserve);
    }

    #[test]
    fn layout_survives_the_builder() {
        // pure inputs keep their order and are never deduplicated; nested results map through
        let mut tx = Tx::new();
        tx.set_sender("0x1");
        let a = tx.split_u64(Arg::Gas, 7);
        let b = tx.split_u64(Arg::Gas, 7);
        tx.merge_coins(a, &[b]);
        tx.transfer_to(&[a], "0x2");
        let opts = BuildOpts { gas_budget: Some(1), gas_price: Some(1), gas_payment: vec![CoinRef { object_id: canon_addr("0x9"), version: 1, digest: DIGEST.into(), balance: 1 }], sponsor: None };
        let built = build_offline(&tx, &opts).unwrap();
        let sui_sdk_types::TransactionKind::ProgrammableTransaction(p) = &built.data.kind else { panic!() };
        assert_eq!(p.inputs.len(), 3);
        assert_eq!(p.commands.len(), 4);
        match &p.commands[2] {
            sui_sdk_types::Command::MergeCoins(m) => {
                assert_eq!(m.coin, sui_sdk_types::Argument::NestedResult(0, 0));
                assert_eq!(m.coins_to_merge, vec![sui_sdk_types::Argument::NestedResult(1, 0)]);
            }
            other => panic!("{other:?}"),
        }
        assert!(built.digest.len() > 30);
    }

    #[test]
    fn a_payment_with_a_coin_reservation_is_left_to_the_node() {
        let mut d = [0u8; 32];
        d[12..32].copy_from_slice(&[0xac; 20]);
        let reservation = Digest::new(d).to_string();
        assert!(is_coin_reservation(&reservation));
        assert!(!is_coin_reservation(DIGEST));
        let mut tx = Tx::new();
        tx.set_sender("0x1");
        let c = tx.split_u64(Arg::Gas, 1);
        tx.transfer_to(&[c], "0x2");
        let coin = |digest: &str| CoinRef { object_id: canon_addr("0x9"), version: 0, digest: digest.into(), balance: 0 };
        let opts = BuildOpts { gas_budget: Some(1), gas_price: Some(1), gas_payment: vec![coin(DIGEST), coin(&reservation)], sponsor: None };
        // no gas objects reach the builder (offline that is an error; the node would pick them)
        assert!(matches!(build_offline(&tx, &opts), Err(_)));
        assert!(build_offline(&tx, &BuildOpts { gas_payment: vec![coin(DIGEST)], ..opts }).is_ok());
    }

    #[test]
    fn addresses_check() {
        assert_eq!(check_address(" 0xAB ").unwrap(), canon_addr("0xab"));
        assert!(check_address("0x").is_err());
        assert!(check_address(&format!("0x{}", "1".repeat(65))).is_err());
    }
}
