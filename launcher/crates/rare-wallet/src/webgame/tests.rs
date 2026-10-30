//! Port of src/wallet/ui/sign/webgame.test.ts (the request mapping half), plus the Transaction
//! JSON a game sends checked against what @mysten/sui's getData() says it is.

use super::*;
use crate::keystore::AccountKind;

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../testdata/ts-vectors.json")).unwrap()
}

fn acc(a: &str) -> RareAccount {
    RareAccount { address: a.into(), label: "x".into(), kind: AccountKind::Native, hd: None, passkey: None, vault_id: None, ledger: None, keystone: None }
}

#[test]
fn maps_connect_transactions_and_messages() {
    assert_eq!(map_game_request("connect", &json!({})).unwrap(), GameCall::Connect);
    let v = vectors();
    let raw = v["webgame"]["transactions"][0]["json"].as_str().unwrap();
    let GameCall::Transaction { tx, execute } = map_game_request("signAndExecuteTransaction", &json!({ "transaction": raw })).unwrap() else { panic!() };
    assert!(execute);
    assert!(matches!(tx, TxInput::Ptb(_)));
    let GameCall::Transaction { execute, .. } = map_game_request("signTransaction", &json!({ "transaction": raw })).unwrap() else { panic!() };
    assert!(!execute);
    // built base64 bytes stay bytes
    let GameCall::Transaction { tx, .. } = map_game_request("signTransaction", &json!({ "transaction": B64.encode([1, 2, 3]) })).unwrap() else { panic!() };
    assert_eq!(tx, TxInput::Bytes(vec![1, 2, 3]));
    assert_eq!(map_game_request("signPersonalMessage", &json!({ "message": B64.encode("hi") })).unwrap(), GameCall::Message(b"hi".to_vec()));
}

#[test]
fn rejects_missing_params_and_bad_json_like_ts() {
    let v = vectors();
    let want: Vec<String> = v["webgame"]["errors"].as_array().unwrap().iter().map(|e| e["error"].as_str().unwrap().to_string()).collect();
    let got: Vec<String> = [("signTransaction", json!({})), ("signPersonalMessage", json!({})), ("signTransaction", json!({ "transaction": "{not json" })), ("foo", json!({}))]
        .iter()
        .map(|(m, p)| map_game_request(m, p).unwrap_err())
        .collect();
    assert_eq!(got, want);
}

#[test]
fn ts_transaction_json_parses_to_the_same_transaction() {
    let v = vectors();
    for (i, t) in v["webgame"]["transactions"].as_array().unwrap().iter().enumerate() {
        let j: Value = serde_json::from_str(t["json"].as_str().unwrap()).unwrap();
        let (tx, opts) = parse_transaction_json(&j).unwrap();
        let mut want = t["data"].clone();
        // the PTB model prints no gas (it travels in BuildOpts) and no intent output kind
        want["gasData"] = json!({ "budget": null, "price": null, "owner": null, "payment": null });
        for c in want["commands"].as_array_mut().unwrap() {
            if let Some(d) = c.get_mut("$Intent").and_then(|i| i.get_mut("data")).and_then(Value::as_object_mut) {
                d.remove("outputKind");
            }
        }
        assert_eq!(tx.to_json(), want, "transaction {i}");
        let g = &t["data"]["gasData"];
        if g["budget"].is_null() {
            assert_eq!(opts, BuildOpts::default());
        } else {
            assert_eq!(opts.gas_budget, Some(3_000_000));
            assert_eq!(opts.gas_price, Some(1000));
            assert_eq!(opts.sponsor.as_deref(), g["owner"].as_str());
            assert_eq!(opts.gas_payment[0].object_id, g["payment"][0]["objectId"].as_str().unwrap());
            assert_eq!(opts.gas_payment[0].version, 9);
            assert!(matches!(parse_game_transaction(t["json"].as_str().unwrap()).unwrap(), TxInput::PtbGas(..)));
        }
    }
}

#[test]
fn untyped_values_and_unknown_steps_are_refused_with_plain_words() {
    let j = json!({ "version": 2, "inputs": [{ "UnresolvedPure": { "value": 5 } }], "commands": [] });
    assert!(parse_transaction_json(&j).unwrap_err().contains("untyped"));
    let j = json!({ "version": 2, "inputs": [], "commands": [{ "Publish": { "modules": [], "dependencies": [] } }] });
    assert!(parse_transaction_json(&j).unwrap_err().contains("does not support"));
    assert!(parse_transaction_json(&json!({ "version": 1 })).is_err());
}

#[test]
fn builds_replies() {
    let r = SignResult { bytes: "AA==".into(), signature: "sig".into(), digest: Some("D1".into()), ..Default::default() };
    assert_eq!(reply_value(&GameCall::Message(vec![]), &r), json!({ "bytes": "AA==", "signature": "sig" }));
    assert_eq!(reply_value(&GameCall::Transaction { tx: TxInput::Bytes(vec![]), execute: false }, &r), json!({ "bytes": "AA==", "signature": "sig" }));
    assert_eq!(reply_value(&GameCall::Transaction { tx: TxInput::Bytes(vec![]), execute: true }, &r)["digest"], "D1");
}

#[test]
fn words_errors_for_the_game() {
    assert_eq!(error_text(&SignError::Rejected("You declined the request".into())), "User rejected the request");
    assert_eq!(error_text(&SignError::Chain("boom".into())), "boom");
    assert_eq!(norm_origin("https://Game.example/"), "https://game.example");
}

#[test]
fn silent_connect_for_approved_sites() {
    let (a, b) = (acc("0xa"), acc("0xb"));
    let both = [a.clone(), b.clone()];
    assert_eq!(silent_connect_account(&["https://play.example.com".into()], "https://play.example.com/", &both, Some(&b)), Some(&both[1]));
    assert_eq!(silent_connect_account(&["http://localhost:3000".into()], "HTTP://localhost:3000", &both, None), Some(&both[0]));
    assert_eq!(silent_connect_account(&["https://play.example.com".into()], "https://other.example.com", &both[..1], Some(&a)), None);
    assert_eq!(silent_connect_account(&["https://play.example.com".into()], "https://play.example.com", &[], None), None);
    assert_eq!(silent_connect_account(&[], "https://play.example.com", &both[..1], Some(&a)), None);
    assert_eq!(silent_connect_account(&["https://g.example".into()], "https://g.example", &both, Some(&acc("0xc"))), Some(&both[0]));
}

#[test]
fn the_sender_comes_from_the_ptb_or_the_built_bytes() {
    use sui_sdk_types::{Address, GasPayment, ProgrammableTransaction, Transaction, TransactionExpiration, TransactionKind};
    let me = format!("0x{}", "ab".repeat(32));
    let other = format!("0x{}", "cd".repeat(32));
    let bytes = |sender: &str| {
        let tx = Transaction {
            kind: TransactionKind::ProgrammableTransaction(ProgrammableTransaction { inputs: vec![], commands: vec![] }),
            sender: sender.parse::<Address>().unwrap(),
            gas_payment: GasPayment { objects: vec![], owner: sender.parse().unwrap(), price: 1000, budget: 5_000_000 },
            expiration: TransactionExpiration::None,
        };
        TxInput::Bytes(bcs::to_bytes(&tx).unwrap())
    };
    assert_eq!(tx_sender(&bytes(&me)).as_deref(), Some(me.as_str()));
    assert!(check_sender(&bytes(&me), &me).is_ok());
    assert_eq!(check_sender(&bytes(&other), &me).unwrap_err(), "This transaction is for a different account");
    // Short forms compare equal; unreadable bytes and a PTB without a sender pass to the review.
    assert!(check_sender(&bytes(&me), &me.to_uppercase().replace("0X", "0x")).is_ok());
    assert!(check_sender(&TxInput::Bytes(vec![1, 2, 3]), &me).is_ok());
    let mut p = rare_sui::ptb::Tx::new();
    assert!(check_sender(&TxInput::Ptb(p.clone()), &me).is_ok());
    p.set_sender(&other);
    assert!(check_sender(&TxInput::Ptb(p), &me).is_err());
}

#[test]
fn reads_address_balance_withdrawals_and_coin_intents_from_the_ts_sdk() {
    // what @mysten/sui 2.x prints for tx.withdrawal(), coinWithBalance() (SUI from the gas coin and
    // not) and createBalance()
    let j = json!({
        "version": 2,
        "sender": format!("0x{}", "a1".repeat(32)),
        "expiration": null,
        "gasData": { "budget": null, "price": null, "owner": null, "payment": null },
        "inputs": [{ "FundsWithdrawal": { "reservation": { "$kind": "MaxAmountU64", "MaxAmountU64": "7" }, "typeArg": { "$kind": "Balance", "Balance": "0x2::sui::SUI" }, "withdrawFrom": { "$kind": "Sender", "Sender": true } }, "$kind": "FundsWithdrawal" }],
        "commands": [
            { "$Intent": { "name": "CoinWithBalance", "inputs": {}, "data": { "type": "gas", "balance": "5", "outputKind": "coin" } }, "$kind": "$Intent" },
            { "$Intent": { "name": "CoinWithBalance", "inputs": {}, "data": { "type": "0x2::sui::SUI", "balance": "6", "outputKind": "coin" } }, "$kind": "$Intent" },
            { "$Intent": { "name": "CoinWithBalance", "inputs": {}, "data": { "type": "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC", "balance": "8", "outputKind": "balance" } }, "$kind": "$Intent" },
            { "MoveCall": { "package": "0x2", "module": "balance", "function": "redeem_funds", "typeArguments": ["0x2::sui::SUI"], "arguments": [{ "$kind": "Input", "Input": 0, "type": "withdrawal" }] }, "$kind": "MoveCall" }
        ]
    });
    let (tx, _) = parse_transaction_json(&j).unwrap();
    assert_eq!(tx.inputs[0], Input::Withdrawal { coin_type: rare_sui::ids::canon_type("0x2::sui::SUI"), amount: 7 });
    let sui = rare_sui::ids::SUI_TYPE.to_string();
    assert_eq!(tx.commands[0], Command::CoinWithBalance { coin_type: sui.clone(), balance: 5, use_gas: true, as_balance: false });
    assert_eq!(tx.commands[1], Command::CoinWithBalance { coin_type: rare_sui::ids::canon_type(&sui), balance: 6, use_gas: false, as_balance: false });
    assert!(matches!(&tx.commands[2], Command::CoinWithBalance { balance: 8, use_gas: false, as_balance: true, .. }));
    assert_eq!(tx.commands[3], Command::MoveCall(MoveCall { package: norm_addr("0x2"), module: "balance".into(), function: "redeem_funds".into(), type_args: vec!["0x2::sui::SUI".into()], args: vec![Arg::Input(0)] }));
    // a sponsor's address balance is not this wallet's to spend
    let mut other = j.clone();
    other["inputs"][0]["FundsWithdrawal"]["withdrawFrom"] = json!({ "$kind": "Sponsor", "Sponsor": true });
    assert!(parse_transaction_json(&other).is_err());
}
