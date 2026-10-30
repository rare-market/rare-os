//! An unresolved programmable transaction: what `@mysten/sui`'s `Transaction` holds before
//! `build()`. Builders add commands; object inputs are ids only (owner, version and mutability are
//! resolved against the chain by [`crate::tx::build`]), pure inputs are BCS bytes.
//!
//! Moved here from `rare-market` (WS 11's request) so every crate that builds transactions shares
//! one type; `rare_market::ptb` re-exports it.
//!
//! The layout mirrors `Transaction` exactly so the ported builders produce the same PTB as the web
//! launcher: object inputs are deduplicated by normalised id, pure inputs never are, package ids
//! are normalised and type arguments are kept as written. [`Tx::to_json`] prints the same JSON as
//! `tx.getData()`, which the golden tests compare against the TypeScript builders' output.

use base64::Engine;
use serde_json::{json, Value};

/// A command argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arg {
    Gas,
    Input(u16),
    /// A command's whole result (a call returning one value, used without destructuring).
    Result(u16),
    /// One value of a command's result tuple (`const [a, b] = tx.moveCall(...)`).
    Nested(u16, u16),
}

impl Arg {
    /// The i-th value of a command's result (`Result(c)` -> `Nested(c, i)`).
    pub fn nth(self, i: u16) -> Arg {
        match self {
            Arg::Result(c) | Arg::Nested(c, _) => Arg::Nested(c, i),
            other => other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// BCS bytes of a pure value.
    Pure(Vec<u8>),
    /// An object by normalised id; resolved when the transaction is built.
    Object(String),
    /// `tx.withdrawal({ amount, type })`: up to `amount` of `coin_type` reserved from the sender's
    /// address balance (funds held in the address's balance accumulator, not as coin objects).
    /// Redeem it with `0x2::coin::redeem_funds` / `0x2::balance::redeem_funds`.
    Withdrawal { coin_type: String, amount: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveCall {
    /// Normalised package id.
    pub package: String,
    pub module: String,
    pub function: String,
    pub type_args: Vec<String>,
    pub args: Vec<Arg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    MoveCall(MoveCall),
    SplitCoins { coin: Arg, amounts: Vec<Arg> },
    MergeCoins { destination: Arg, sources: Vec<Arg> },
    TransferObjects { objects: Vec<Arg>, address: Arg },
    MakeMoveVec { type_: Option<String>, elements: Vec<Arg> },
    /// `coinWithBalance({ type, balance, useGasCoin })` (or `createBalance` when `as_balance`): a
    /// coin (a `Balance<T>`) of `coin_type` worth `balance`, taken from the sender's address balance
    /// when it covers it, else merged and split from coin objects plus the address balance, when the
    /// transaction is built. SUI with `use_gas` draws from the gas coin (never in a sponsored
    /// transaction, where the gas coin is the sponsor's).
    CoinWithBalance { coin_type: String, balance: u64, use_gas: bool, as_balance: bool },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tx {
    /// Normalised sender address.
    pub sender: Option<String>,
    pub inputs: Vec<Input>,
    pub commands: Vec<Command>,
}

/// `0x` + 64 lowercase hex digits (`normalizeSuiAddress`).
pub fn norm_addr(a: &str) -> String {
    let h = a.trim().trim_start_matches("0x").trim_start_matches("0X").to_ascii_lowercase();
    format!("0x{h:0>64}")
}

/// 32 address bytes, or None when `a` is not hex of at most 64 digits.
pub fn addr_bytes(a: &str) -> Option<[u8; 32]> {
    let n = norm_addr(a);
    let h = &n[2..];
    if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn uleb(mut n: usize, out: &mut Vec<u8>) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// BCS encoders for the pure values the builders pass.
pub mod bcs {
    use super::*;
    pub fn u64(v: u64) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }
    pub fn u8(v: u8) -> Vec<u8> {
        vec![v]
    }
    pub fn bool(v: bool) -> Vec<u8> {
        vec![v as u8]
    }
    /// An address or ID. Panics on a malformed id: callers pass ids that came from the API or chain.
    pub fn address(a: &str) -> Vec<u8> {
        addr_bytes(a).unwrap_or_else(|| panic!("not a Sui address: {a}")).to_vec()
    }
    pub fn option(v: Option<Vec<u8>>) -> Vec<u8> {
        match v {
            None => vec![0],
            Some(mut b) => {
                b.insert(0, 1);
                b
            }
        }
    }
    pub fn bytes(b: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(b.len() + 2);
        uleb(b.len(), &mut out);
        out.extend_from_slice(b);
        out
    }
    pub fn string(s: &str) -> Vec<u8> {
        bytes(s.as_bytes())
    }
    pub fn vec_bytes(v: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        uleb(v.len(), &mut out);
        for b in v {
            out.extend(bytes(b));
        }
        out
    }
}

/// `pkg::module::function` split, with the package normalised.
fn split_target(target: &str) -> (String, String, String) {
    let mut it = target.splitn(3, "::");
    let pkg = it.next().unwrap_or_default();
    let module = it.next().unwrap_or_default().to_string();
    let function = it.next().unwrap_or_default().to_string();
    (norm_addr(pkg), module, function)
}

impl Tx {
    pub fn new() -> Self {
        Tx::default()
    }

    pub fn set_sender(&mut self, sender: &str) {
        self.sender = Some(norm_addr(sender));
    }

    /// The gas coin.
    pub fn gas(&self) -> Arg {
        Arg::Gas
    }

    /// An object input by id; the same object is one input however often it is used.
    pub fn object(&mut self, id: &str) -> Arg {
        let id = norm_addr(id);
        if let Some(i) = self.inputs.iter().position(|x| matches!(x, Input::Object(o) if *o == id)) {
            return Arg::Input(i as u16);
        }
        self.inputs.push(Input::Object(id));
        Arg::Input((self.inputs.len() - 1) as u16)
    }

    /// A pure input from BCS bytes.
    pub fn pure(&mut self, bytes: Vec<u8>) -> Arg {
        self.inputs.push(Input::Pure(bytes));
        Arg::Input((self.inputs.len() - 1) as u16)
    }
    pub fn pure_u64(&mut self, v: u64) -> Arg {
        self.pure(bcs::u64(v))
    }
    pub fn pure_bool(&mut self, v: bool) -> Arg {
        self.pure(bcs::bool(v))
    }
    pub fn pure_address(&mut self, a: &str) -> Arg {
        self.pure(bcs::address(a))
    }
    pub fn pure_id(&mut self, id: &str) -> Arg {
        self.pure(bcs::address(id))
    }
    pub fn pure_option_address(&mut self, a: Option<&str>) -> Arg {
        self.pure(bcs::option(a.map(bcs::address)))
    }
    pub fn pure_option_u64(&mut self, v: Option<u64>) -> Arg {
        self.pure(bcs::option(v.map(bcs::u64)))
    }
    pub fn pure_string(&mut self, s: &str) -> Arg {
        self.pure(bcs::string(s))
    }
    /// `vector<vector<u8>>`.
    pub fn pure_vec_bytes(&mut self, v: &[Vec<u8>]) -> Arg {
        self.pure(bcs::vec_bytes(v))
    }

    fn push(&mut self, c: Command) -> Arg {
        self.commands.push(c);
        Arg::Result((self.commands.len() - 1) as u16)
    }

    /// `tx.moveCall({ target, typeArguments, arguments })`. The result is the whole command result;
    /// take values with [`Arg::nth`].
    pub fn move_call(&mut self, target: &str, type_args: &[&str], args: &[Arg]) -> Arg {
        let (package, module, function) = split_target(target);
        self.push(Command::MoveCall(MoveCall { package, module, function, type_args: type_args.iter().map(|s| s.to_string()).collect(), args: args.to_vec() }))
    }

    /// `tx.splitCoins(coin, amounts)`.
    pub fn split_coins(&mut self, coin: Arg, amounts: &[Arg]) -> Arg {
        self.push(Command::SplitCoins { coin, amounts: amounts.to_vec() })
    }

    /// `const [c] = tx.splitCoins(coin, [amount])` for a literal amount.
    pub fn split_u64(&mut self, coin: Arg, amount: u64) -> Arg {
        let a = self.pure_u64(amount);
        self.split_coins(coin, &[a]).nth(0)
    }

    pub fn merge_coins(&mut self, destination: Arg, sources: &[Arg]) {
        self.push(Command::MergeCoins { destination, sources: sources.to_vec() });
    }

    pub fn transfer_objects(&mut self, objects: &[Arg], address: Arg) {
        self.push(Command::TransferObjects { objects: objects.to_vec(), address });
    }

    /// `tx.transferObjects(objects, "0x...")`: the address becomes a pure input.
    pub fn transfer_to(&mut self, objects: &[Arg], address: &str) {
        let a = self.pure_address(address);
        self.transfer_objects(objects, a);
    }

    pub fn make_move_vec(&mut self, type_: Option<&str>, elements: &[Arg]) -> Arg {
        self.push(Command::MakeMoveVec { type_: type_.map(String::from), elements: elements.to_vec() })
    }

    /// `coinWithBalance({ type, balance })` (type normalised like the TS intent).
    pub fn coin_with_balance(&mut self, coin_type: &str, balance: u64) -> Arg {
        self.push(Command::CoinWithBalance { coin_type: crate::ids::canon_type(coin_type), balance, use_gas: true, as_balance: false })
    }

    /// `tx.withdrawal({ amount, type })`: a FundsWithdrawal input on the sender's address balance.
    pub fn withdrawal(&mut self, coin_type: &str, amount: u64) -> Arg {
        self.inputs.push(Input::Withdrawal { coin_type: crate::ids::canon_type(coin_type), amount });
        Arg::Input((self.inputs.len() - 1) as u16)
    }

    /// A `Coin<T>` of exactly `amount` straight out of the sender's address balance
    /// (`0x2::coin::redeem_funds(withdrawal)`).
    pub fn withdraw_coin(&mut self, coin_type: &str, amount: u64) -> Arg {
        let w = self.withdrawal(coin_type, amount);
        let t = crate::ids::canon_type(coin_type);
        self.move_call("0x2::coin::redeem_funds", &[&t], &[w])
    }

    // ---------- inspection ----------

    /// Every Move call as `package::module::function` (package normalised).
    pub fn targets(&self) -> Vec<String> {
        self.commands
            .iter()
            .filter_map(|c| match c {
                Command::MoveCall(m) => Some(format!("{}::{}::{}", m.package, m.module, m.function)),
                _ => None,
            })
            .collect()
    }

    pub fn move_calls(&self) -> impl Iterator<Item = &MoveCall> {
        self.commands.iter().filter_map(|c| match c {
            Command::MoveCall(m) => Some(m),
            _ => None,
        })
    }

    /// Literal u64 amounts split off any coin, in order.
    pub fn split_amounts(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for c in &self.commands {
            if let Command::SplitCoins { amounts, .. } = c {
                for a in amounts {
                    if let Arg::Input(i) = a {
                        if let Some(Input::Pure(b)) = self.inputs.get(*i as usize) {
                            if b.len() == 8 {
                                out.push(u64::from_le_bytes(b[..8].try_into().unwrap()));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Ids of every object input.
    pub fn object_ids(&self) -> Vec<&str> {
        self.inputs
            .iter()
            .filter_map(|i| match i {
                Input::Object(o) => Some(o.as_str()),
                _ => None,
            })
            .collect()
    }

    // ---------- JSON ----------

    fn arg_json(&self, a: Arg) -> Value {
        match a {
            Arg::Gas => json!({"GasCoin": true, "$kind": "GasCoin"}),
            Arg::Input(i) => {
                let ty = match self.inputs.get(i as usize) {
                    Some(Input::Object(_)) => "object",
                    Some(Input::Withdrawal { .. }) => "withdrawal",
                    _ => "pure",
                };
                json!({"Input": i, "type": ty, "$kind": "Input"})
            }
            Arg::Result(c) => json!({"Result": c, "$kind": "Result"}),
            Arg::Nested(c, i) => json!({"NestedResult": [c, i], "$kind": "NestedResult"}),
        }
    }

    fn args_json(&self, v: &[Arg]) -> Value {
        Value::Array(v.iter().map(|a| self.arg_json(*a)).collect())
    }

    /// The same JSON as `@mysten/sui`'s `tx.getData()` for an unbuilt transaction.
    pub fn to_json(&self) -> Value {
        let b64 = base64::engine::general_purpose::STANDARD;
        let inputs: Vec<Value> = self
            .inputs
            .iter()
            .map(|i| match i {
                Input::Pure(b) => json!({"Pure": {"bytes": b64.encode(b)}, "$kind": "Pure"}),
                Input::Object(o) => json!({"UnresolvedObject": {"objectId": o}, "$kind": "UnresolvedObject"}),
                Input::Withdrawal { coin_type, amount } => json!({"FundsWithdrawal": {
                    "reservation": {"MaxAmountU64": amount.to_string(), "$kind": "MaxAmountU64"},
                    "typeArg": {"Balance": coin_type, "$kind": "Balance"},
                    "withdrawFrom": {"Sender": true, "$kind": "Sender"}
                }, "$kind": "FundsWithdrawal"}),
            })
            .collect();
        let commands: Vec<Value> = self
            .commands
            .iter()
            .map(|c| match c {
                Command::MoveCall(m) => json!({"MoveCall": {
                    "package": m.package, "module": m.module, "function": m.function,
                    "typeArguments": m.type_args, "arguments": self.args_json(&m.args)
                }, "$kind": "MoveCall"}),
                Command::SplitCoins { coin, amounts } => json!({"SplitCoins": {"coin": self.arg_json(*coin), "amounts": self.args_json(amounts)}, "$kind": "SplitCoins"}),
                Command::MergeCoins { destination, sources } => json!({"MergeCoins": {"destination": self.arg_json(*destination), "sources": self.args_json(sources)}, "$kind": "MergeCoins"}),
                Command::TransferObjects { objects, address } => json!({"TransferObjects": {"objects": self.args_json(objects), "address": self.arg_json(*address)}, "$kind": "TransferObjects"}),
                Command::MakeMoveVec { type_, elements } => json!({"MakeMoveVec": {"type": type_, "elements": self.args_json(elements)}, "$kind": "MakeMoveVec"}),
                Command::CoinWithBalance { coin_type, balance, use_gas, as_balance } => {
                    let ty = if *use_gas && crate::ids::is_sui(coin_type) { "gas".to_string() } else { coin_type.clone() };
                    let mut data = json!({"type": ty, "balance": balance.to_string()});
                    if *as_balance {
                        data["outputKind"] = json!("balance");
                    }
                    json!({"$Intent": {"name": "CoinWithBalance", "inputs": {}, "data": data}, "$kind": "$Intent"})
                }
            })
            .collect();
        json!({
            "version": 2,
            "sender": self.sender,
            "expiration": null,
            "gasData": {"budget": null, "price": null, "owner": null, "payment": null},
            "inputs": inputs,
            "commands": commands,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON @mysten/sui 2.38 printed for the same calls (probe run on 2026-10-27).
    #[test]
    fn matches_mysten_get_data() {
        let mut tx = Tx::new();
        tx.set_sender("0x1");
        let r = tx.move_call("0x2::kiosk::new", &[], &[]);
        let (k, c) = (r.nth(0), r.nth(1));
        let p = tx.split_u64(Arg::Gas, 12);
        let a = [tx.object("0x6"), tx.object("0x6"), k, p];
        let mut args = a.to_vec();
        args.push(tx.pure_id("0x5"));
        args.push(tx.pure_option_address(None));
        args.push(tx.pure_option_u64(Some(5)));
        args.push(tx.pure_vec_bytes(&[vec![1, 2], vec![3]]));
        args.push(tx.pure_address("0xab"));
        tx.move_call("0x0dfb::m::f", &["0x44::retro::Nft<0x2::sui::SUI>"], &args);
        tx.transfer_to(&[c], "0x1");
        tx.transfer_to(&[c], "0x2");
        tx.merge_coins(Arg::Gas, &[p]);
        let want: Value = serde_json::from_str(r#"{"version":2,"sender":"0x0000000000000000000000000000000000000000000000000000000000000001","expiration":null,"gasData":{"budget":null,"price":null,"owner":null,"payment":null},"inputs":[{"Pure":{"bytes":"DAAAAAAAAAA="},"$kind":"Pure"},{"UnresolvedObject":{"objectId":"0x0000000000000000000000000000000000000000000000000000000000000006"},"$kind":"UnresolvedObject"},{"Pure":{"bytes":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAU="},"$kind":"Pure"},{"Pure":{"bytes":"AA=="},"$kind":"Pure"},{"Pure":{"bytes":"AQUAAAAAAAAA"},"$kind":"Pure"},{"Pure":{"bytes":"AgIBAgED"},"$kind":"Pure"},{"Pure":{"bytes":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAKs="},"$kind":"Pure"},{"Pure":{"bytes":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE="},"$kind":"Pure"},{"Pure":{"bytes":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAI="},"$kind":"Pure"}],"commands":[{"MoveCall":{"package":"0x0000000000000000000000000000000000000000000000000000000000000002","module":"kiosk","function":"new","typeArguments":[],"arguments":[]},"$kind":"MoveCall"},{"SplitCoins":{"coin":{"GasCoin":true,"$kind":"GasCoin"},"amounts":[{"Input":0,"type":"pure","$kind":"Input"}]},"$kind":"SplitCoins"},{"MoveCall":{"package":"0x0000000000000000000000000000000000000000000000000000000000000dfb","module":"m","function":"f","typeArguments":["0x44::retro::Nft<0x2::sui::SUI>"],"arguments":[{"Input":1,"type":"object","$kind":"Input"},{"Input":1,"type":"object","$kind":"Input"},{"NestedResult":[0,0],"$kind":"NestedResult"},{"NestedResult":[1,0],"$kind":"NestedResult"},{"Input":2,"type":"pure","$kind":"Input"},{"Input":3,"type":"pure","$kind":"Input"},{"Input":4,"type":"pure","$kind":"Input"},{"Input":5,"type":"pure","$kind":"Input"},{"Input":6,"type":"pure","$kind":"Input"}]},"$kind":"MoveCall"},{"TransferObjects":{"objects":[{"NestedResult":[0,1],"$kind":"NestedResult"}],"address":{"Input":7,"type":"pure","$kind":"Input"}},"$kind":"TransferObjects"},{"TransferObjects":{"objects":[{"NestedResult":[0,1],"$kind":"NestedResult"}],"address":{"Input":8,"type":"pure","$kind":"Input"}},"$kind":"TransferObjects"},{"MergeCoins":{"destination":{"GasCoin":true,"$kind":"GasCoin"},"sources":[{"NestedResult":[1,0],"$kind":"NestedResult"}]},"$kind":"MergeCoins"}]}"#).unwrap();
        assert_eq!(tx.to_json(), want);
        assert_eq!(tx.split_amounts(), vec![12]);
    }

    #[test]
    fn addresses_normalise() {
        assert_eq!(norm_addr("0x2"), format!("0x{}2", "0".repeat(63)));
        assert_eq!(norm_addr("0XAB"), format!("0x{}ab", "0".repeat(62)));
        assert!(addr_bytes("0xzz").is_none());
        assert_eq!(bcs::bytes(&[0u8; 200])[..2], [0xc8, 0x01]);
    }
}
