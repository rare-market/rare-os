//! One finished transaction over gRPC, reduced to what the activity detail screen shows from this
//! wallet's point of view. Ports `src/wallet/data/txdetail.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sui_rpc::proto::sui::rpc::v2 as pb;

use crate::grpc::{Owner, Sui};
use crate::ids::{canon_addr, SUI_TYPE};
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    pub coin_type: String,
    /// Signed base units.
    pub amount: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub package: String,
    pub module: String,
    pub function: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjAction {
    Created,
    Received,
    Sent,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxObject {
    pub id: String,
    #[serde(rename = "type")]
    pub object_type: Option<String>,
    pub action: ObjAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxStatus {
    Success,
    Failure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDetail {
    pub digest: String,
    pub status: TxStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub timestamp_ms: Option<i64>,
    pub sender: String,
    /// Signed base units for THIS wallet, network fee already taken out of SUI.
    pub flows: Vec<Flow>,
    /// Net SUI the gas cost (MIST); negative = refund.
    pub fee_mist: String,
    /// False when someone else (a sponsor) paid.
    pub fee_paid: bool,
    /// The other side of a transfer, when it is one address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    pub calls: Vec<Call>,
    pub objects: Vec<TxObject>,
}

fn owner_addr(o: Option<&pb::Owner>) -> Option<String> {
    match Owner::from_proto(o) {
        Owner::Address { address } | Owner::ConsensusAddress { address } => Some(address),
        _ => None,
    }
}

fn norm(a: Option<&str>) -> String {
    a.filter(|s| !s.is_empty()).map(canon_addr).unwrap_or_default()
}

/// Coin<T> objects: covered by balance changes, never listed as objects.
pub fn is_coin_object(t: Option<&str>) -> bool {
    t.is_some_and(|t| t.strip_prefix("0x").is_some_and(|r| r.trim_start_matches('0').starts_with("2::coin::Coin<")))
}

/// True when the node no longer has (or never had) the transaction.
pub fn is_pruned(e: &Error) -> bool {
    let m = e.to_string().to_lowercase();
    e.is_not_found() || m.contains("pruned") || m.contains("does not exist") || m.contains("404") || m.contains("not found") || m.contains("notfound")
}

/// Read mask for [`parse_tx`].
pub const MASK: [&str; 5] = ["digest", "transaction", "effects", "balance_changes", "timestamp"];

/// Load one past transaction for wallet `me`. None when the node has pruned it.
pub async fn activity_detail(sui: &Sui, digest: &str, me: &str) -> Result<Option<TxDetail>> {
    match sui.transaction(digest, &MASK).await {
        Ok(t) => Ok(Some(parse_tx(&t, digest, me))),
        Err(e) if is_pruned(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

fn signed(v: i128) -> String {
    v.to_string()
}

/// Pure: a node transaction -> TxDetail for wallet `me`.
pub fn parse_tx(t: &pb::ExecutedTransaction, digest: &str, me: &str) -> TxDetail {
    let mine = canon_addr(me);
    let sender = t.transaction.as_ref().and_then(|x| x.sender.clone()).unwrap_or_default();
    let effects = t.effects.as_ref();
    let g = effects.and_then(|e| e.gas_used.as_ref());
    let net: i128 = g.map(|g| i128::from(g.computation_cost.unwrap_or(0)) + i128::from(g.storage_cost.unwrap_or(0)) - i128::from(g.storage_rebate.unwrap_or(0))).unwrap_or(0);
    let gas_obj = effects.and_then(|e| e.gas_object.as_ref());
    // the gas owner the transaction names (a fee paid from an address balance has no gas object),
    // else the gas coin's owner
    let gas_owner = t
        .transaction
        .as_ref()
        .and_then(|x| x.gas_payment.as_ref())
        .and_then(|g| g.owner.clone())
        .filter(|o| !o.is_empty())
        .or_else(|| owner_addr(gas_obj.and_then(|o| o.input_owner.as_ref())))
        .unwrap_or_else(|| sender.clone());
    let fee_paid = norm(Some(&gas_owner)) == mine;

    // balance changes for this wallet, merged per coin (in first-seen order); the fee on its own line
    let mut order: Vec<String> = vec![];
    let mut sums: BTreeMap<String, i128> = BTreeMap::new();
    for b in &t.balance_changes {
        if norm(b.address.as_deref()) != mine {
            continue;
        }
        let ct = b.coin_type.clone().unwrap_or_default();
        let amt: i128 = b.amount.as_deref().and_then(|a| a.parse().ok()).unwrap_or(0);
        if !sums.contains_key(&ct) {
            order.push(ct.clone());
        }
        *sums.entry(ct).or_insert(0) += amt;
    }
    if fee_paid {
        let key = order.iter().find(|k| k.ends_with("::sui::SUI") && k.strip_prefix("0x").is_some_and(|r| r.trim_start_matches('0').starts_with("2::"))).cloned().unwrap_or_else(|| SUI_TYPE.to_string());
        if !sums.contains_key(&key) {
            order.push(key.clone());
        }
        *sums.entry(key).or_insert(0) += net;
    }
    let flows: Vec<Flow> = order.iter().filter(|k| sums[*k] != 0).map(|k| Flow { coin_type: k.clone(), amount: signed(sums[k]) }).collect();

    // the other address: whoever got what we sent, or whoever sent to us
    let sent_any = flows.iter().any(|f| f.amount.starts_with('-'));
    let counterparty = if sent_any {
        t.balance_changes
            .iter()
            .find(|b| norm(b.address.as_deref()) != mine && b.amount.as_deref().is_some_and(|a| !a.starts_with('-') && a != "0"))
            .and_then(|b| b.address.clone())
    } else if !sender.is_empty() && canon_addr(&sender) != mine {
        Some(sender.clone())
    } else {
        None
    };

    let mut calls = vec![];
    if let Some(pb::transaction_kind::Data::ProgrammableTransaction(p)) = t.transaction.as_ref().and_then(|x| x.kind.as_ref()).and_then(|k| k.data.as_ref()) {
        for c in &p.commands {
            if let Some(pb::command::Command::MoveCall(m)) = &c.command {
                calls.push(Call { package: m.package.clone().unwrap_or_default(), module: m.module.clone().unwrap_or_default(), function: m.function.clone().unwrap_or_default() });
            }
        }
    }

    let mut objects = vec![];
    let gas_id = gas_obj.and_then(|o| o.object_id.clone());
    use pb::changed_object::{IdOperation, OutputObjectState};
    for o in effects.map(|e| e.changed_objects.as_slice()).unwrap_or_default() {
        let out_state = o.output_state.and_then(|s| OutputObjectState::try_from(s).ok());
        if o.object_id == gas_id || matches!(out_state, Some(OutputObjectState::PackageWrite) | Some(OutputObjectState::AccumulatorWrite)) {
            continue;
        }
        let ty = o.object_type.clone();
        if is_coin_object(ty.as_deref()) {
            continue;
        }
        let id = o.object_id.clone().unwrap_or_default();
        let from = owner_addr(o.input_owner.as_ref());
        let to = owner_addr(o.output_owner.as_ref());
        let was_mine = norm(from.as_deref()) == mine;
        let is_mine = norm(to.as_deref()) == mine;
        let gone = out_state == Some(OutputObjectState::DoesNotExist);
        let op = o.id_operation.and_then(|s| IdOperation::try_from(s).ok());
        if op == Some(IdOperation::Deleted) || (gone && was_mine) {
            if was_mine {
                objects.push(TxObject { id, object_type: ty, action: ObjAction::Deleted, to: None });
            }
        } else if op == Some(IdOperation::Created) && is_mine {
            objects.push(TxObject { id, object_type: ty, action: ObjAction::Created, to: None });
        } else if was_mine && !is_mine && to.is_some() {
            objects.push(TxObject { id, object_type: ty, action: ObjAction::Sent, to });
        } else if !was_mine && is_mine {
            objects.push(TxObject { id, object_type: ty, action: ObjAction::Received, to: None });
        }
    }

    let st = effects.and_then(|e| e.status.as_ref());
    let ok = st.and_then(|s| s.success).unwrap_or(false);
    TxDetail {
        digest: digest.to_string(),
        status: if ok { TxStatus::Success } else { TxStatus::Failure },
        error: (!ok).then(|| st.and_then(|s| s.error.as_ref()).and_then(|e| e.description.clone()).unwrap_or_else(|| "Transaction failed".into())),
        timestamp_ms: t.timestamp.as_ref().map(|ts| ts.seconds * 1000 + i64::from(ts.nanos) / 1_000_000),
        sender,
        flows,
        fee_mist: signed(net),
        fee_paid,
        counterparty,
        calls,
        objects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> String {
        format!("0x{}", "a1".repeat(32))
    }
    fn bob() -> String {
        format!("0x{}", "b2".repeat(32))
    }
    fn owner(a: &str) -> pb::Owner {
        let mut o = pb::Owner::default();
        o.kind = Some(pb::owner::OwnerKind::Address as i32);
        o.address = Some(a.into());
        o
    }
    fn change(addr: &str, amount: &str) -> pb::BalanceChange {
        let mut b = pb::BalanceChange::default();
        b.coin_type = Some(SUI_TYPE.into());
        b.address = Some(addr.into());
        b.amount = Some(amount.into());
        b
    }

    fn send_tx() -> pb::ExecutedTransaction {
        let mut t = pb::ExecutedTransaction::default();
        t.digest = Some("D1".into());
        let mut tx = pb::Transaction::default();
        tx.sender = Some(me());
        t.transaction = Some(tx);
        t.balance_changes = vec![change(&me(), &(-1_000_000_000i64 - 1_500_000).to_string()), change(&bob(), "1000000000")];
        let mut e = pb::TransactionEffects::default();
        let mut st = pb::ExecutionStatus::default();
        st.success = Some(true);
        e.status = Some(st);
        let mut g = pb::GasCostSummary::default();
        g.computation_cost = Some(1_000_000);
        g.storage_cost = Some(2_000_000);
        g.storage_rebate = Some(1_500_000);
        e.gas_used = Some(g);
        let mut gas = pb::ChangedObject::default();
        gas.object_id = Some(format!("0x{}", "11".repeat(32)));
        gas.input_owner = Some(owner(&me()));
        e.gas_object = Some(gas);
        t.effects = Some(e);
        t.timestamp = Some(prost_types::Timestamp { seconds: 1_700_000_000, nanos: 0 });
        t
    }

    // data.test.ts "activity detail"
    #[test]
    fn parses_a_sui_send_for_this_wallet() {
        let d = parse_tx(&send_tx(), "D1", &me());
        assert_eq!(d.status, TxStatus::Success);
        assert_eq!(d.flows, vec![Flow { coin_type: SUI_TYPE.into(), amount: "-1000000000".into() }]);
        assert_eq!(d.fee_mist, "1500000");
        assert!(d.fee_paid);
        assert_eq!(d.counterparty, Some(bob()));
        assert_eq!(d.timestamp_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn objects_received_sent_created_and_coins_skipped() {
        let mut t = send_tx();
        let obj = |id: u8, ty: &str, from: Option<&str>, to: Option<&str>, op: pb::changed_object::IdOperation| {
            let mut o = pb::ChangedObject::default();
            o.object_id = Some(format!("0x{:02x}", id));
            o.object_type = Some(ty.into());
            o.input_owner = from.map(owner);
            o.output_owner = to.map(owner);
            o.output_state = Some(pb::changed_object::OutputObjectState::ObjectWrite as i32);
            o.id_operation = Some(op as i32);
            o
        };
        use pb::changed_object::IdOperation as Op;
        t.effects.as_mut().unwrap().changed_objects = vec![
            obj(1, "0x9::a::Nft", Some(&me()), Some(&bob()), Op::None),
            obj(2, "0x9::a::Nft", Some(&bob()), Some(&me()), Op::None),
            obj(3, "0x9::a::Nft", None, Some(&me()), Op::Created),
            obj(4, "0x2::coin::Coin<0x2::sui::SUI>", Some(&me()), Some(&bob()), Op::None),
            obj(5, "0x9::a::Nft", Some(&me()), None, Op::Deleted),
        ];
        let d = parse_tx(&t, "D1", &me());
        let acts: Vec<(String, ObjAction)> = d.objects.iter().map(|o| (o.id.clone(), o.action)).collect();
        assert_eq!(acts, vec![("0x01".into(), ObjAction::Sent), ("0x02".into(), ObjAction::Received), ("0x03".into(), ObjAction::Created), ("0x05".into(), ObjAction::Deleted)]);
        assert_eq!(d.objects[0].to, Some(bob()));
    }

    #[test]
    fn a_failure_and_a_sponsor() {
        let mut t = send_tx();
        let e = t.effects.as_mut().unwrap();
        let mut err = pb::ExecutionError::default();
        err.description = Some("MoveAbort in 0x2::coin".into());
        e.status.as_mut().unwrap().success = Some(false);
        e.status.as_mut().unwrap().error = Some(err);
        e.gas_object.as_mut().unwrap().input_owner = Some(owner(&bob()));
        let d = parse_tx(&t, "D1", &me());
        assert_eq!(d.status, TxStatus::Failure);
        assert_eq!(d.error.as_deref(), Some("MoveAbort in 0x2::coin"));
        assert!(!d.fee_paid);
        assert_eq!(d.flows[0].amount, "-1001500000");
    }

    #[test]
    fn a_sponsor_paying_from_its_address_balance() {
        // no gas object at all: the gas payment's owner says who paid
        let mut t = send_tx();
        t.effects.as_mut().unwrap().gas_object = None;
        let mut gp = pb::GasPayment::default();
        gp.owner = Some(bob());
        t.transaction.as_mut().unwrap().gas_payment = Some(gp);
        assert!(!parse_tx(&t, "D1", &me()).fee_paid);
        t.transaction.as_mut().unwrap().gas_payment.as_mut().unwrap().owner = Some(me());
        assert!(parse_tx(&t, "D1", &me()).fee_paid);
    }

    #[test]
    fn pruned_means_not_found() {
        assert!(is_pruned(&Error::Grpc { code: crate::error::GRPC_NOT_FOUND, message: "Transaction not found".into() }));
        assert!(!is_pruned(&Error::Grpc { code: 14, message: "connect".into() }));
        assert!(is_coin_object(Some("0x0000000000000000000000000000000000000000000000000000000000000002::coin::Coin<0x2::sui::SUI>")));
        assert!(!is_coin_object(Some("0x12::coin::Coin<X>")));
    }
}
