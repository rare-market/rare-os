//! Normalized pre-sign simulation result (ports `review/types.ts`). Produced by a [`Simulator`]
//! (gRPC against the fullnode, fixed results in tests) and read by `analyze` / `risk_checks`.
//! Serializes to the same JSON shape as the TS `Simulation`.
//!
//! [`Simulator`]: super::Simulator

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Owner {
    Address {
        address: String,
    },
    /// Dynamic field / child of another object (kiosks, tables, wrapped storage).
    Object {
        id: String,
    },
    Shared,
    Immutable,
    Unknown,
}

impl Owner {
    pub fn address(a: &str) -> Self {
        Owner::Address { address: a.to_string() }
    }
    pub fn object(id: &str) -> Self {
        Owner::Object { id: id.to_string() }
    }
}

/// Created / Deleted come from the node; `None` = the object existed before and after (or was wrapped).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdOperation {
    Created,
    Deleted,
    #[serde(rename = "None")]
    Unchanged,
    Unknown,
}

/// PackageWrite (publish/upgrade) and AccumulatorWriteV1 (address balances) are not user objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputKind {
    Object,
    Package,
    Accumulator,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimObjectChange {
    pub object_id: String,
    /// Full Move type, e.g. `0x2::coin::Coin<0x2::sui::SUI>`; None when the node didn't say.
    #[serde(rename = "type")]
    pub object_type: Option<String>,
    pub input_owner: Option<Owner>,
    pub output_owner: Option<Owner>,
    pub id_operation: IdOperation,
    pub input_exists: bool,
    pub output_exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_kind: Option<OutputKind>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimBalanceChange {
    pub coin_type: String,
    pub address: String,
    /// Signed base units.
    pub amount: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimAbort {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constant: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimFailure {
    /// Node message, e.g. `MoveAbort in 1st command, abort code: 2, in '0x2::balance::split'`.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort: Option<SimAbort>,
}

impl SimFailure {
    pub fn message(m: impl Into<String>) -> Self {
        SimFailure { message: m.into(), ..Default::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SimStatus {
    Success,
    Failure,
}

/// Who produced a simulation; Rare only simulates on a Sui fullnode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SimSource {
    Node,
}

/// Base units of SUI (MIST), as decimal strings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimGas {
    pub computation: String,
    pub storage: String,
    pub rebate: String,
}

impl SimGas {
    pub fn zero() -> Self {
        SimGas { computation: "0".into(), storage: "0".into(), rebate: "0".into() }
    }
}

/// An object's Display name and image.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// On-chain coin metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainCoin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub decimals: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Simulation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SimSource>,
    pub status: SimStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<SimFailure>,
    pub balance_changes: Vec<SimBalanceChange>,
    pub objects: Vec<SimObjectChange>,
    pub gas: SimGas,
    /// Owner of the gas coin before the tx; the signer unless sponsored.
    pub gas_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_object_id: Option<String>,
    /// Optional extras fetched after the simulation (best effort).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<BTreeMap<String, DisplayInfo>>,
    /// Called package id -> its original (first version) id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages: Option<BTreeMap<String, String>>,
    /// On-chain coin metadata for coins in the balance changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coins: Option<BTreeMap<String, ChainCoin>>,
}

impl Simulation {
    /// A failure with nothing else (the node rejected it, or it failed while building).
    pub fn failed(failure: SimFailure) -> Self {
        Simulation {
            source: Some(SimSource::Node),
            status: SimStatus::Failure,
            failure: Some(failure),
            balance_changes: vec![],
            objects: vec![],
            gas: SimGas::zero(),
            gas_owner: None,
            gas_object_id: None,
            display: None,
            packages: None,
            coins: None,
        }
    }
}
