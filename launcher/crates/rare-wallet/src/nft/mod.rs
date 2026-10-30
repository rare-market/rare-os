//! Sending NFTs: which of the wallet's NFTs can go (wallet objects, kiosk items in plain and
//! personal kiosks, locked items under transfer policies, Drip and Tradeport listings cancelled
//! first) and the programmable transaction that sends them. Ports `src/wallet/tx/nft/*` and
//! `src/wallet/tx/nftSend.ts` (Drip's `lib/tx` send flow).
//!
//! Builders produce an unresolved [`rare_sui::ptb::Tx`] laid out exactly like the TS builders'
//! `Transaction` (its `to_json()` equals `tx.getData()`, checked against testdata/nft-vectors.json);
//! `rare_sui::tx::build` resolves it on the node for signing. Builders never read the chain and
//! never sign; the reads ([`context`]) go through rare-sui's [`rare_sui::grpc::ChainRead`] so tests
//! run on fakes.
//!
//! The Send NFT screen calls, in order:
//! 1. [`context::nft_sendability`] (or [`context::analyze_nfts`] with its own readers): for each
//!    NFT either a [`SendItem`] or the plain reason it can't be sent.
//! 2. [`transfer::plan_nft_send`] when the selection may be too big for one transaction.
//! 3. [`transfer::build_nft_send`] per batch, then `rare_sui::tx::build` and the signing pipeline.

pub mod context;
pub mod ids;
pub mod kiosk;
pub mod limits;
pub mod policy;
pub mod transfer;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use rare_sui::kiosk::{KioskRef, Market};
pub use rare_sui::tx::{SendError, SendErrorCode};

pub use context::{analyze_nfts, nft_sendability, policy_ids_from_history, policy_ids_from_trade, read_policies, AnalyzeDeps, HistoryPolicies, NftRef, PolicyLoader, PolicySource, Sendability, TxObjectTypes};
pub use kiosk::{KioskHandle, KioskSession};
pub use limits::{fits, measure, plan_batches, type_tag_bytes, Limits, TxSize, ARGS_PER_COMMAND, SUI_LIMITS};
pub use policy::{match_rule, pick_policy, policy_needs, policy_problem, unsupported_reason, MatchedRule, PolicyNeeds};
pub use transfer::{build_nft_send, build_send_nfts, plan_nft_send, plan_nft_sends, NftSendParams};

/// One item in a kiosk the sender owns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KioskItemRef {
    pub nft_id: String,
    pub nft_type: String,
    pub is_locked: bool,
    #[serde(default)]
    pub is_listed: bool,
    #[serde(default)]
    pub is_exclusive_listing: bool,
    /// Where it is listed, when a marketplace has to cancel the listing (not a plain kiosk::list).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market: Option<Market>,
}

/// One rule of a transfer policy.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyRule {
    /// Registry id ("royalty_rule") or the rule's module name.
    #[serde(rename = "type")]
    pub type_: String,
    /// Full Move type of the rule, e.g. `0x434b...::royalty_rule::Rule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_type: Option<String>,
    /// The rule's config, when its proof needs a value from it (floor price, staking registry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<Value>,
}

/// A collection's transfer policy and its rules.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPolicyInfo {
    pub id: String,
    /// None when unknown (counts as shared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_shared: Option<bool>,
    pub rules: Vec<PolicyRule>,
}

/// One NFT to send.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendItem {
    /// Kiosk the item is in, or None for an NFT held directly in the wallet.
    pub from: Option<KioskRef>,
    pub item: KioskItemRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policies: Option<Vec<TransferPolicyInfo>>,
}

/// Why a send can't be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NftSendError {
    /// Bad input from the screen: the address, an empty selection, the same account
    /// (`SendError` in coin.ts, shared with coin sends).
    Input(SendError),
    /// The selection can't be sent as asked (listed elsewhere, no usable transfer policy, one item
    /// too big for a transaction); the message is the TS builder's.
    Build(String),
}

impl NftSendError {
    pub fn message(&self) -> &str {
        match self {
            NftSendError::Input(e) => &e.message,
            NftSendError::Build(m) => m,
        }
    }
}

impl std::fmt::Display for NftSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for NftSendError {}

impl From<SendError> for NftSendError {
    fn from(e: SendError) -> Self {
        NftSendError::Input(e)
    }
}

pub(crate) fn build_err(m: impl Into<String>) -> NftSendError {
    NftSendError::Build(m.into())
}
