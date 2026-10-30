//! Rare Wallet behind other crates' wallet traits:
//! - `rare_friends::WalletSigner`: one personal-message signature through the review sheet
//!   (linking friends, claiming a username)
//! - `rare_trade::swap::Identity`: the throwaway device identity the swap API key is made with
//!   (a fresh Ed25519 key each time, never stored)
//! - `trade_sign`: a `rare_trade::wallet::SignRequest` through the review sheet (trade/wallet.ts)

use crate::keys::{Keypair, FLAG_ED25519};
use crate::keystore::AccountStore;
use crate::sign::{Chain, OriginKind, SignError, SignOptions, SignOrigin, Signing, TxInput};
use crate::sponsor::{Category, Sponsorship};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use sui_sdk_types::{Argument, Command, Input, TransactionKind};
use crate::sponsor::SponsorApi;
use rand_core::{OsRng, RngCore};
use std::sync::Arc;
use zeroize::Zeroizing;

/// `rare_friends::WalletSigner` over the signing pipeline. `sign_personal_message` blocks until
/// the person answers the review sheet: never call it on the thread that runs Approve.
pub struct FriendsWallet<S: AccountStore, C: Chain, A: SponsorApi>(pub Arc<Signing<S, C, A>>);

impl<S: AccountStore, C: Chain, A: SponsorApi> rare_friends::WalletSigner for FriendsWallet<S, C, A> {
    fn sign_personal_message(&self, message: &[u8], prompt: &rare_friends::api::SignPrompt) -> Result<String, rare_friends::Error> {
        let o = SignOptions { account: Some(prompt.account.clone()), origin: Some(SignOrigin::new(&prompt.origin, OriginKind::Wallet)), title: Some(prompt.title.clone()), ..Default::default() };
        let ticket = self.0.sign_personal_message(message.to_vec(), o).map_err(|e| rare_friends::Error::Other(e.to_string()))?;
        match ticket.wait() {
            Ok(r) => Ok(r.signature),
            Err(SignError::Rejected(_)) => Err(rare_friends::Error::rejected()),
            Err(e) => Err(rare_friends::Error::Other(e.to_string())),
        }
    }
}

/// `rare_friends::resolve::RecipientResolver` over the wallet's recipient field (recipient.ts
/// resolveRecipient), from a plain worker thread on `rt`.
pub struct FriendsRecipients<N: crate::recipient::NameService> {
    pub names: N,
    pub rt: tokio::runtime::Handle,
}

/// The wallet's field state in rare-friends' words.
pub fn friends_recipient(st: crate::recipient::RecipientState) -> rare_friends::resolve::Recipient {
    use crate::recipient::RecipientState as S;
    use rare_friends::resolve::Recipient as F;
    match st {
        S::Ready { address, suins, .. } => F::Ready { address, suins },
        S::Invalid { msg } => F::Invalid { msg },
        S::Missing { msg, .. } => F::Missing { msg },
        S::Error { msg, .. } => F::Error { msg },
        S::Empty | S::Typing | S::Loading { .. } => F::Other,
    }
}

impl<N: crate::recipient::NameService> rare_friends::resolve::RecipientResolver for FriendsRecipients<N> {
    fn resolve_recipient(&self, text: &str) -> rare_friends::resolve::Recipient {
        friends_recipient(self.rt.block_on(crate::recipient::resolve_recipient(&self.names, text)))
    }
}

/// A throwaway Ed25519 identity (the web launcher's `new Ed25519Keypair()`); the key is wiped
/// when it is dropped.
pub struct EphemeralIdentity(Keypair);

impl EphemeralIdentity {
    pub fn new() -> Self {
        let mut seed = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut seed[..]);
        EphemeralIdentity(Keypair::from_bytes(FLAG_ED25519, &seed[..]).expect("32 bytes"))
    }
}

impl Default for EphemeralIdentity {
    fn default() -> Self {
        Self::new()
    }
}

impl rare_trade::swap::Identity for EphemeralIdentity {
    fn address(&self) -> String {
        self.0.address()
    }
    fn sign_personal_message(&self, message: &[u8]) -> rare_trade::Result<String> {
        Ok(self.0.sign_personal_message(message))
    }
}

/// `rare_trade::swap::IdentityMaker`: a fresh identity each time a key is needed.
pub fn identity_maker() -> rare_trade::swap::IdentityMaker {
    Arc::new(|| Ok(Box::new(EphemeralIdentity::new()) as Box<dyn rare_trade::swap::Identity>))
}

// ---------------------------------------------------------------- Trade

fn arg_json(a: &Argument, inputs: &[Input]) -> Value {
    match a {
        Argument::Gas => json!({ "GasCoin": true, "$kind": "GasCoin" }),
        Argument::Input(i) => {
            let ty = match inputs.get(*i as usize) {
                Some(Input::Pure(_)) => "pure",
                Some(Input::FundsWithdrawal(_)) => "withdrawal",
                _ => "object",
            };
            json!({ "Input": i, "type": ty, "$kind": "Input" })
        }
        Argument::Result(r) => json!({ "Result": r, "$kind": "Result" }),
        Argument::NestedResult(c, i) => json!({ "NestedResult": [c, i], "$kind": "NestedResult" }),
    }
}

/// A router's resolved transaction kind as Transaction JSON (v2, the shape `getData()` prints for
/// `Transaction.fromKind(kind)` with the sender set): what `rare_trade::chain::gas_from_spent_sui`
/// edits and `webgame::parse_transaction_json` reads.
pub fn kind_json(kind_b64: &str, sender: &str) -> Result<Value, String> {
    let kind: TransactionKind = bcs::from_bytes(&B64.decode(kind_b64).map_err(|_| "unreadable transaction kind")?).map_err(|e| e.to_string())?;
    let TransactionKind::ProgrammableTransaction(p) = kind else { return Err("not a programmable transaction".into()) };
    let inputs: Vec<Value> = p
        .inputs
        .iter()
        .map(|i| match i {
            Input::Pure(b) => Ok(json!({ "Pure": { "bytes": B64.encode(b) }, "$kind": "Pure" })),
            Input::ImmutableOrOwned(r) => Ok(json!({ "Object": { "ImmOrOwnedObject": { "objectId": r.object_id().to_string(), "version": r.version().to_string(), "digest": r.digest().to_string() }, "$kind": "ImmOrOwnedObject" }, "$kind": "Object" })),
            Input::Shared(s) => Ok(json!({ "Object": { "SharedObject": { "objectId": s.object_id().to_string(), "initialSharedVersion": s.version().to_string(), "mutable": s.mutability() == sui_sdk_types::Mutability::Mutable }, "$kind": "SharedObject" }, "$kind": "Object" })),
            Input::Receiving(r) => Ok(json!({ "Object": { "Receiving": { "objectId": r.object_id().to_string(), "version": r.version().to_string(), "digest": r.digest().to_string() }, "$kind": "Receiving" }, "$kind": "Object" })),
            // funds from the trader's address balance
            Input::FundsWithdrawal(w) if w.source() == sui_sdk_types::WithdrawFrom::Sender => Ok(json!({ "FundsWithdrawal": {
                "reservation": { "MaxAmountU64": w.amount().unwrap_or(0).to_string(), "$kind": "MaxAmountU64" },
                "typeArg": { "Balance": w.coin_type().to_string(), "$kind": "Balance" },
                "withdrawFrom": { "Sender": true, "$kind": "Sender" }
            }, "$kind": "FundsWithdrawal" })),
            _ => Err("This trade uses an input Rare Wallet does not support yet".to_string()),
        })
        .collect::<Result<_, _>>()?;
    let a = |v: &[Argument]| v.iter().map(|x| arg_json(x, &p.inputs)).collect::<Vec<_>>();
    let commands: Vec<Value> = p
        .commands
        .iter()
        .map(|c| match c {
            Command::MoveCall(m) => Ok(json!({ "MoveCall": { "package": m.package.to_string(), "module": m.module.to_string(), "function": m.function.to_string(), "typeArguments": m.type_arguments.iter().map(|t| t.to_string()).collect::<Vec<_>>(), "arguments": a(&m.arguments) }, "$kind": "MoveCall" })),
            Command::TransferObjects(t) => Ok(json!({ "TransferObjects": { "objects": a(&t.objects), "address": arg_json(&t.address, &p.inputs) }, "$kind": "TransferObjects" })),
            Command::SplitCoins(s) => Ok(json!({ "SplitCoins": { "coin": arg_json(&s.coin, &p.inputs), "amounts": a(&s.amounts) }, "$kind": "SplitCoins" })),
            Command::MergeCoins(m) => Ok(json!({ "MergeCoins": { "destination": arg_json(&m.coin, &p.inputs), "sources": a(&m.coins_to_merge) }, "$kind": "MergeCoins" })),
            Command::MakeMoveVector(v) => Ok(json!({ "MakeMoveVec": { "type": v.type_.as_ref().map(|t| t.to_string()), "elements": a(&v.elements) }, "$kind": "MakeMoveVec" })),
            _ => Err("This trade uses a step Rare Wallet does not support yet".to_string()),
        })
        .collect::<Result<_, _>>()?;
    Ok(json!({
        "version": 2,
        "sender": rare_sui::ptb::norm_addr(sender),
        "expiration": null,
        "gasData": { "budget": null, "price": null, "owner": null, "payment": null },
        "inputs": inputs,
        "commands": commands,
    }))
}

/// A trade's unsigned transaction as the pipeline's input. A router kind with
/// `gas_from_spent_sui` pays gas from the SUI coin it spends (when `reader` can tell which).
pub fn trade_input(tx: &rare_trade::tx::UnsignedTx, reader: Option<&dyn rare_trade::chain::ChainReader>) -> Result<TxInput, String> {
    use rare_trade::tx::UnsignedTx;
    let from_json = |j: &Value, sender: Option<&str>| -> Result<TxInput, String> {
        let (mut t, opts) = crate::webgame::parse_transaction_json(j)?;
        if t.sender.is_none() {
            if let Some(s) = sender {
                t.set_sender(s);
            }
        }
        Ok(if opts == Default::default() { TxInput::Ptb(t) } else { TxInput::PtbGas(t, opts) })
    };
    match tx {
        UnsignedTx::Bytes { b64, .. } => B64.decode(b64).map(TxInput::Bytes).map_err(|_| "unreadable transaction".into()),
        UnsignedTx::Json(j) => from_json(j, None),
        UnsignedTx::Kind { kind, sender, gas_from_spent_sui } => {
            let mut j = kind_json(kind, sender)?;
            if *gas_from_spent_sui {
                if let Some(r) = reader {
                    if let Ok(Some(edited)) = rare_trade::chain::gas_from_spent_sui(&j, r) {
                        j = edited;
                    }
                }
            }
            from_json(&j, Some(sender))
        }
    }
}

/// A trade through Rare Wallet's review sheet: `covered` is Rare's sponsorship (from
/// `Sponsor::sponsor_kind` with `Category::Trade`) and its kind when Rare pays the fee. Blocks
/// until the person answers the sheet.
pub fn trade_sign<S: AccountStore, C: Chain, A: SponsorApi>(
    signing: &Signing<S, C, A>,
    req: &rare_trade::wallet::SignRequest,
    covered: Option<(String, Sponsorship)>,
    reader: Option<&dyn rare_trade::chain::ChainReader>,
) -> rare_trade::wallet::SignOutcome {
    use rare_trade::wallet::SignOutcome;
    let o = SignOptions {
        account: Some(req.account.clone()),
        origin: Some(SignOrigin::new(req.origin_name, if req.origin_kind == "market" { OriginKind::Market } else { OriginKind::Wallet })),
        title: Some(req.title.clone()),
        ..Default::default()
    };
    let ticket = match covered {
        Some((kind, s)) => signing.sign_and_execute_sponsored(kind, s, Category::Trade, o, None),
        None => match trade_input(&req.tx, reader) {
            Ok(input) => signing.sign_and_execute(input, o),
            Err(e) => return SignOutcome::Failed(e),
        },
    };
    match ticket.and_then(|t| t.wait()) {
        Ok(r) => SignOutcome::Sent { digest: r.digest, dry_run: r.dry_run },
        Err(SignError::Rejected(_)) => SignOutcome::Rejected,
        Err(e) => SignOutcome::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sui_crypto::Verifier as _;

    #[test]
    fn identities_are_fresh_and_sign_personal_messages() {
        let make = identity_maker();
        let (a, b) = (make().unwrap(), make().unwrap());
        assert_ne!(a.address(), b.address());
        let sig = a.sign_personal_message(b"swap key").unwrap();
        let user = sui_sdk_types::UserSignature::from_base64(&sig).unwrap();
        sui_crypto::UserSignatureVerifier::new().verify(&crate::keys::personal_message_digest(b"swap key"), &user).unwrap();
        let sui_sdk_types::UserSignature::Simple(sui_sdk_types::SimpleSignature::Ed25519 { public_key, .. }) = user else { panic!() };
        assert_eq!(public_key.derive_address().to_string(), a.address());
    }

    #[test]
    fn friends_resolve_through_the_recipient_field() {
        use crate::recipient::{NameFut, NameService};
        use rare_friends::resolve::{Recipient, RecipientResolver};
        struct Names;
        impl NameService for Names {
            fn name_to_address<'a>(&'a self, name: &'a str) -> NameFut<'a> {
                Box::pin(async move { Ok((name == "alice.sui").then(|| format!("0x{}", "a1".repeat(32)))) })
            }
            fn name_of<'a>(&'a self, _: &'a str) -> NameFut<'a> {
                Box::pin(async { Ok(Some("bob.sui".to_string())) })
            }
        }
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let r = FriendsRecipients { names: Names, rt: rt.handle().clone() };
        assert_eq!(r.resolve_recipient("@alice"), Recipient::Ready { address: format!("0x{}", "a1".repeat(32)), suins: Some("alice.sui".into()) });
        assert!(matches!(r.resolve_recipient("nobody.sui"), Recipient::Missing { .. }));
        assert!(matches!(r.resolve_recipient(&format!("0x{}", "b2".repeat(32))), Recipient::Ready { suins: Some(_), .. }));
        assert_eq!(r.resolve_recipient(""), Recipient::Other);
    }

    #[test]
    fn router_kinds_may_withdraw_from_the_address_balance() {
        use rare_sui::ptb::Input as PIn;
        let usdc = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
        let sender = format!("0x{}", "a1".repeat(32));
        let mut tx = rare_sui::ptb::Tx::new();
        tx.set_sender(&sender);
        let c = tx.withdraw_coin(usdc, 42);
        tx.transfer_to(&[c], "0x2");
        let gas = vec![rare_sui::grpc::CoinRef { object_id: rare_sui::ptb::norm_addr("0x9"), version: 1, digest: "4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi".into(), balance: 1 }];
        let built = rare_sui::tx::build_offline(&tx, &rare_sui::tx::BuildOpts { gas_budget: Some(1), gas_price: Some(1), gas_payment: gas, sponsor: None }).unwrap();
        let kind = B64.encode(bcs::to_bytes(&built.data.kind).unwrap());
        let j = kind_json(&kind, &sender).unwrap();
        assert_eq!(j["inputs"][0]["FundsWithdrawal"]["reservation"]["MaxAmountU64"], "42");
        assert_eq!(j["commands"][0]["MoveCall"]["arguments"][0]["type"], "withdrawal");
        let (back, _) = crate::webgame::parse_transaction_json(&j).unwrap();
        assert_eq!(back.inputs[0], PIn::Withdrawal { coin_type: rare_sui::ids::canon_type(usdc), amount: 42 });
        assert_eq!(back.commands.len(), 2);
    }

    #[test]
    fn router_kinds_become_the_same_transaction_json() {
        let v: Value = serde_json::from_str(include_str!("../testdata/ts-vectors.json")).unwrap();
        let kind = v["sponsor"]["gameKind"].as_str().unwrap();
        let sender = "0x5e93a736d04fbb25737aa40bee40171ef79f65fae833749e3c089fe7cc2161f1";
        let j = kind_json(kind, sender).unwrap();
        assert_eq!(j["sender"], sender);
        assert_eq!(j["commands"][0]["MoveCall"]["function"], "play");
        assert_eq!(j["inputs"][0]["$kind"], "Pure");
        let input = trade_input(&rare_trade::tx::UnsignedTx::Kind { kind: kind.into(), sender: sender.into(), gas_from_spent_sui: true }, None).unwrap();
        let TxInput::Ptb(t) = input else { panic!("no gas set") };
        assert_eq!(t.sender.as_deref(), Some(sender));
        assert_eq!(t.commands.len(), 1);
        // the same PTB a game's JSON gives
        let (from_json, _) = crate::webgame::parse_transaction_json(&j).unwrap();
        assert_eq!(t, from_json);
        assert_eq!(trade_input(&rare_trade::tx::UnsignedTx::Bytes { b64: B64.encode([1, 2]), sender: sender.into() }, None).unwrap(), TxInput::Bytes(vec![1, 2]));
    }
}
