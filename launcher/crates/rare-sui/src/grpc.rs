//! The one Sui client: MystenLabs' `sui-rpc` gRPC against the public fullnodes (or the testnet
//! RPC override). Ports `src/sui/chain.ts` and `src/wallet/grpc.ts`.
//!
//! [`ChainRead`] is the slice of reads the launcher makes. [`Sui`] implements it over gRPC;
//! tests implement only the methods they need (the rest answer "not supported").

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use prost_types::FieldMask;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sui_rpc::field::FieldMaskUtil;
use sui_rpc::proto::sui::rpc::v2 as pb;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use crate::ids::{canon_addr, canon_type};
use crate::json::from_proto;
use crate::network::{Network, NetworkConfig};
use crate::{BoxFut, Error, Result};

/// Sui caps gas payment at 256 coins; list at most this many when paying with every coin.
pub const MAX_GAS_COINS: usize = 255;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Who owns an object (`Owner` in sui.rpc.v2, `ChainOwner`-like).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Owner {
    Address { address: String },
    /// Owned by another object: a dynamic field (kiosk items) or a wrapper.
    Object { id: String },
    Shared,
    Immutable,
    ConsensusAddress { address: String },
    Unknown,
}

impl Owner {
    pub(crate) fn from_proto(o: Option<&pb::Owner>) -> Self {
        let Some(o) = o else { return Owner::Unknown };
        let addr = || o.address.as_deref().map(canon_addr).unwrap_or_default();
        match pb::owner::OwnerKind::try_from(o.kind.unwrap_or(0)).unwrap_or(pb::owner::OwnerKind::Unknown) {
            pb::owner::OwnerKind::Address => Owner::Address { address: addr() },
            pb::owner::OwnerKind::Object => Owner::Object { id: addr() },
            pb::owner::OwnerKind::Shared => Owner::Shared,
            pb::owner::OwnerKind::Immutable => Owner::Immutable,
            pb::owner::OwnerKind::ConsensusAddress => Owner::ConsensusAddress { address: addr() },
            _ => Owner::Unknown,
        }
    }
}

/// What to read beyond id, version, digest, type and owner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Include {
    /// The fullnode's JSON rendering of the Move value.
    pub json: bool,
    /// The type's Display output.
    pub display: bool,
    /// The Move struct's BCS bytes.
    pub contents: bool,
    pub previous_transaction: bool,
    /// A coin's balance.
    pub balance: bool,
}

impl Include {
    pub const NONE: Include = Include { json: false, display: false, contents: false, previous_transaction: false, balance: false };
    pub const JSON: Include = Include { json: true, ..Include::NONE };
    pub const DISPLAY: Include = Include { display: true, ..Include::NONE };
    pub const CONTENTS: Include = Include { contents: true, ..Include::NONE };

    fn mask(self) -> FieldMask {
        let mut p = vec!["object_id", "version", "digest", "object_type", "owner", "has_public_transfer"];
        if self.json {
            p.push("json");
        }
        if self.display {
            p.push("display");
        }
        if self.contents {
            p.push("contents");
        }
        if self.previous_transaction {
            p.push("previous_transaction");
        }
        if self.balance {
            p.push("balance");
        }
        FieldMask::from_paths(p)
    }
}

/// One object as the launcher reads it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub object_id: String,
    pub version: u64,
    pub digest: String,
    /// The full Move type, as the node prints it.
    #[serde(rename = "type")]
    pub object_type: String,
    pub owner: Option<Owner>,
    pub has_public_transfer: Option<bool>,
    pub json: Option<Value>,
    /// The Display output (`display.output`).
    pub display: Option<Value>,
    /// Move struct BCS (the object's contents, starting with its UID).
    #[serde(skip)]
    pub contents: Option<Vec<u8>>,
    pub previous_transaction: Option<String>,
    pub balance: Option<u64>,
}

impl ObjectInfo {
    pub fn from_proto(o: &pb::Object) -> Self {
        ObjectInfo {
            object_id: o.object_id.as_deref().map(canon_addr).unwrap_or_default(),
            version: o.version.unwrap_or(0),
            digest: o.digest.clone().unwrap_or_default(),
            object_type: o.object_type.clone().unwrap_or_default(),
            owner: o.owner.as_ref().map(|w| Owner::from_proto(Some(w))),
            has_public_transfer: o.has_public_transfer,
            json: o.json.as_deref().map(from_proto),
            display: o.display.as_deref().and_then(|d| d.output.as_ref()).map(from_proto),
            contents: o.contents.as_ref().and_then(|b| b.value.as_ref()).map(|b| b.to_vec()),
            previous_transaction: o.previous_transaction.clone(),
            balance: o.balance,
        }
    }

    /// A string field of the Display output.
    pub fn display_str(&self, key: &str) -> Option<&str> {
        self.display.as_ref()?.get(key)?.as_str()
    }
}

/// A coin object, for gas payment and merges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoinRef {
    pub object_id: String,
    pub version: u64,
    pub digest: String,
    pub balance: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinBalance {
    pub coin_type: String,
    /// Total: coins plus the address balance.
    pub balance: u64,
    pub coin_balance: Option<u64>,
    pub address_balance: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinMeta {
    pub decimals: u32,
    pub symbol: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    /// A dynamic field (the value lives in the Field object).
    Field,
    /// A dynamic object field (the value is its own object, `child_id`).
    Object,
    Unknown,
}

/// One dynamic field of a parent object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynField {
    pub kind: FieldKind,
    pub field_id: String,
    /// The child object for a dynamic object field.
    pub child_id: Option<String>,
    /// The name's Move type (`0x2::kiosk::Item`).
    pub name_type: String,
    /// The name's BCS bytes.
    #[serde(skip)]
    pub name_bcs: Vec<u8>,
    pub value_type: String,
}

impl DynField {
    fn from_proto(f: &pb::DynamicField) -> Self {
        let kind = match f.kind.map(pb::dynamic_field::DynamicFieldKind::try_from) {
            Some(Ok(pb::dynamic_field::DynamicFieldKind::Field)) => FieldKind::Field,
            Some(Ok(pb::dynamic_field::DynamicFieldKind::Object)) => FieldKind::Object,
            _ => FieldKind::Unknown,
        };
        DynField {
            kind,
            field_id: f.field_id.as_deref().map(canon_addr).unwrap_or_default(),
            child_id: f.child_id.as_deref().map(canon_addr),
            name_type: f.name.as_ref().and_then(|b| b.name.clone()).unwrap_or_default(),
            name_bcs: f.name.as_ref().and_then(|b| b.value.as_ref()).map(|b| b.to_vec()).unwrap_or_default(),
            value_type: f.value_type.clone().unwrap_or_default(),
        }
    }
}

fn unsupported<T: Send + 'static>(what: &'static str) -> BoxFut<'static, T> {
    Box::pin(async move { Err(Error::invalid(format!("{what}: not supported by this reader"))) })
}

/// The reads the launcher makes. Implemented by [`Sui`]; fakes implement what a test needs.
pub trait ChainRead: Send + Sync {
    /// Reference gas price in MIST.
    fn reference_gas_price(&self) -> BoxFut<'_, u64> {
        unsupported("reference_gas_price")
    }
    fn balance(&self, _owner: &str, _coin_type: &str) -> BoxFut<'_, CoinBalance> {
        unsupported("balance")
    }
    /// Every coin type the address holds (up to 10 pages of 50).
    fn list_balances(&self, _owner: &str) -> BoxFut<'_, Vec<CoinBalance>> {
        unsupported("list_balances")
    }
    fn coin_metadata(&self, _coin_type: &str) -> BoxFut<'_, Option<CoinMeta>> {
        unsupported("coin_metadata")
    }
    /// Coins of one type the address owns, largest first is not promised; up to `max`.
    fn list_coins(&self, _owner: &str, _coin_type: &str, _max: usize) -> BoxFut<'_, Vec<CoinRef>> {
        unsupported("list_coins")
    }
    /// Objects the address owns directly (not inside kiosks), optionally of one type, up to `max`.
    fn owned_objects(&self, _owner: &str, _object_type: Option<&str>, _max: usize, _include: Include) -> BoxFut<'_, Vec<ObjectInfo>> {
        unsupported("owned_objects")
    }
    fn object(&self, _id: &str, _include: Include) -> BoxFut<'_, ObjectInfo> {
        unsupported("object")
    }
    /// Several objects; a missing one is an `Err` in its slot.
    fn objects(&self, _ids: &[String], _include: Include) -> BoxFut<'_, Vec<Result<ObjectInfo>>> {
        unsupported("objects")
    }
    /// Dynamic fields of a parent, up to `max`.
    fn dynamic_fields(&self, _parent: &str, _max: usize) -> BoxFut<'_, Vec<DynField>> {
        unsupported("dynamic_fields")
    }
    /// The address's default SuiNS name.
    fn default_name(&self, _address: &str) -> BoxFut<'_, Option<String>> {
        unsupported("default_name")
    }
    /// `alice.sui` to its target address.
    fn resolve_name(&self, _name: &str) -> BoxFut<'_, Option<String>> {
        unsupported("resolve_name")
    }
    /// A Move datatype's abilities (`Ability` numbers in sui.rpc.v2: 1 copy, 2 drop, 3 store, 4 key).
    fn datatype_abilities(&self, _package: &str, _module: &str, _name: &str) -> BoxFut<'_, Vec<i32>> {
        unsupported("datatype_abilities")
    }
}

/// Every object of one type the owner holds directly, up to `max`: `ownedOfType` in chain.ts.
pub async fn owned_of_type(c: &dyn ChainRead, owner: &str, object_type: &str, max: usize) -> Result<Vec<ObjectInfo>> {
    c.owned_objects(owner, Some(object_type), max, Include::DISPLAY).await
}

/// Whether a struct type has `store` (without it an object can't leave the owner). None when the
/// node does not say. `hasStore` in wallet/tx/nftSend.ts.
pub async fn has_store(c: &dyn ChainRead, object_type: &str) -> Result<Option<bool>> {
    let tag = crate::ids::struct_tag(object_type)?;
    let abilities = c.datatype_abilities(&tag.address().to_string(), tag.module().as_str(), tag.name().as_str()).await?;
    Ok((!abilities.is_empty()).then(|| abilities.contains(&(pb::Ability::Store as i32))))
}

/// The gRPC client for one network and endpoint.
#[derive(Clone)]
pub struct Sui {
    network: Network,
    url: String,
    rpc: sui_rpc::Client,
    names: pb::name_service_client::NameServiceClient<Channel>,
}

impl Sui {
    /// A client for `url`. Needs a tokio runtime (the channels connect lazily on it).
    pub fn new(network: Network, url: &str) -> Result<Self> {
        let rpc = sui_rpc::Client::new(url.to_string())?.with_response_headers_timeout(TIMEOUT);
        let mut ep = Endpoint::from_shared(url.to_string()).map_err(|e| Error::invalid(e.to_string()))?.connect_timeout(TIMEOUT).timeout(TIMEOUT);
        if url.starts_with("https://") {
            ep = ep.tls_config(ClientTlsConfig::new().with_enabled_roots()).map_err(|e| Error::invalid(e.to_string()))?;
        }
        let names = pb::name_service_client::NameServiceClient::new(ep.connect_lazy());
        Ok(Sui { network, url: url.to_string(), rpc, names })
    }

    /// The shared client for the launcher's current network (one per network and endpoint, so
    /// the Developer setting's switch and testnet RPC override apply at once).
    pub fn current(cfg: &NetworkConfig) -> Result<Self> {
        Self::for_network(cfg, cfg.current())
    }

    pub fn for_network(cfg: &NetworkConfig, network: Network) -> Result<Self> {
        static CLIENTS: OnceLock<Mutex<HashMap<String, Sui>>> = OnceLock::new();
        let url = cfg.rpc_url(network);
        let key = format!("{}|{url}", network.as_str());
        let mut m = CLIENTS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = m.get(&key) {
            return Ok(c.clone());
        }
        let c = Sui::new(network, &url)?;
        m.insert(key, c.clone());
        Ok(c)
    }

    pub fn network(&self) -> Network {
        self.network
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    /// The underlying `sui-rpc` client, for reads this crate does not wrap and for
    /// `TransactionBuilder::build`.
    pub fn rpc(&self) -> sui_rpc::Client {
        self.rpc.clone()
    }

    fn req<T>(m: T) -> tonic::Request<T> {
        let mut r = tonic::Request::new(m);
        r.set_timeout(TIMEOUT);
        r
    }

    /// A transaction with the fields in `mask` (e.g. `["digest", "effects", "balance_changes"]`).
    pub async fn transaction(&self, digest: &str, mask: &[&str]) -> Result<pb::ExecutedTransaction> {
        let r = self
            .rpc
            .clone()
            .ledger_client()
            .get_transaction(Self::req(pb::GetTransactionRequest::default().with_digest(digest).with_read_mask(FieldMask::from_paths(mask))))
            .await?
            .into_inner();
        r.transaction.ok_or_else(|| Error::decode("no transaction in the answer"))
    }

    /// Dry run (checks on) of a built transaction, with the effects and balance changes.
    pub async fn simulate(&self, tx: &sui_sdk_types::Transaction) -> Result<pb::SimulateTransactionResponse> {
        let req = pb::SimulateTransactionRequest::default()
            .with_transaction(pb::Transaction::from(tx.clone()))
            .with_read_mask(FieldMask::from_paths(["transaction.effects", "transaction.balance_changes", "transaction.events", "transaction.objects"]));
        Ok(self.rpc.clone().execution_client().simulate_transaction(Self::req(req)).await?.into_inner())
    }

    /// Executes a signed transaction and waits until it is in a checkpoint, so reads after it see
    /// it. `signatures` are the serialized user signatures (sender, and sponsor if any).
    pub async fn execute(&self, tx: &sui_sdk_types::Transaction, signatures: Vec<sui_sdk_types::UserSignature>) -> Result<pb::ExecutedTransaction> {
        let req = pb::ExecuteTransactionRequest::default()
            .with_transaction(pb::Transaction::from(tx.clone()))
            .with_signatures(signatures.into_iter().map(pb::UserSignature::from).collect::<Vec<_>>())
            .with_read_mask(FieldMask::from_paths(["digest", "effects", "balance_changes", "checkpoint", "timestamp"]));
        let r = self
            .rpc
            .clone()
            .execute_transaction_and_wait_for_checkpoint(req, Duration::from_secs(30))
            .await
            .map_err(|e| Error::Grpc { code: crate::error::GRPC_UNKNOWN, message: e.to_string() })?
            .into_inner();
        r.transaction.ok_or_else(|| Error::decode("no transaction in the answer"))
    }
}

impl ChainRead for Sui {
    fn reference_gas_price(&self) -> BoxFut<'_, u64> {
        Box::pin(async move {
            let r = self.rpc.clone().ledger_client().get_epoch(Self::req(pb::GetEpochRequest::default().with_read_mask(FieldMask::from_paths(["reference_gas_price"])))).await?.into_inner();
            r.epoch.and_then(|e| e.reference_gas_price).ok_or_else(|| Error::decode("no reference gas price"))
        })
    }

    fn balance(&self, owner: &str, coin_type: &str) -> BoxFut<'_, CoinBalance> {
        let (owner, coin_type) = (owner.to_string(), coin_type.to_string());
        Box::pin(async move {
            let r = self.rpc.clone().state_client().get_balance(Self::req(pb::GetBalanceRequest::default().with_owner(owner).with_coin_type(coin_type.clone()))).await?.into_inner();
            Ok(r.balance.map(|b| balance_from(&b)).unwrap_or(CoinBalance { coin_type, ..Default::default() }))
        })
    }

    fn list_balances(&self, owner: &str) -> BoxFut<'_, Vec<CoinBalance>> {
        let owner = owner.to_string();
        Box::pin(async move {
            let s = self.rpc.list_balances(pb::ListBalancesRequest::default().with_owner(owner).with_page_size(50));
            let v: Vec<pb::Balance> = s.take(500).try_collect().await?;
            Ok(v.iter().map(balance_from).collect())
        })
    }

    fn coin_metadata(&self, coin_type: &str) -> BoxFut<'_, Option<CoinMeta>> {
        let coin_type = coin_type.to_string();
        Box::pin(async move {
            let r = self.rpc.clone().state_client().get_coin_info(Self::req(pb::GetCoinInfoRequest::default().with_coin_type(coin_type))).await?.into_inner();
            Ok(r.metadata.map(|m| CoinMeta {
                decimals: m.decimals.unwrap_or(9),
                symbol: m.symbol.unwrap_or_default(),
                name: m.name.unwrap_or_default(),
                description: m.description.unwrap_or_default(),
                icon_url: m.icon_url.filter(|s| !s.is_empty()),
            }))
        })
    }

    fn list_coins(&self, owner: &str, coin_type: &str, max: usize) -> BoxFut<'_, Vec<CoinRef>> {
        let owner = owner.to_string();
        let t = format!("0x2::coin::Coin<{}>", canon_type(coin_type));
        Box::pin(async move {
            let req = pb::ListOwnedObjectsRequest::default()
                .with_owner(owner)
                .with_object_type(t)
                .with_page_size(50)
                .with_read_mask(FieldMask::from_paths(["object_id", "version", "digest", "balance"]));
            let v: Vec<pb::Object> = self.rpc.list_owned_objects(req).take(max).try_collect().await?;
            Ok(v.iter()
                .map(|o| CoinRef {
                    object_id: o.object_id.as_deref().map(canon_addr).unwrap_or_default(),
                    version: o.version.unwrap_or(0),
                    digest: o.digest.clone().unwrap_or_default(),
                    balance: o.balance.unwrap_or(0),
                })
                .collect())
        })
    }

    fn owned_objects(&self, owner: &str, object_type: Option<&str>, max: usize, include: Include) -> BoxFut<'_, Vec<ObjectInfo>> {
        let owner = owner.to_string();
        let t = object_type.map(str::to_string);
        Box::pin(async move {
            let mut req = pb::ListOwnedObjectsRequest::default().with_owner(owner).with_page_size(max.clamp(1, 50) as u32).with_read_mask(include.mask());
            if let Some(t) = t {
                req = req.with_object_type(t);
            }
            let v: Vec<pb::Object> = self.rpc.list_owned_objects(req).take(max).try_collect().await?;
            Ok(v.iter().map(ObjectInfo::from_proto).collect())
        })
    }

    fn object(&self, id: &str, include: Include) -> BoxFut<'_, ObjectInfo> {
        let id = id.to_string();
        Box::pin(async move {
            let r = self.rpc.clone().ledger_client().get_object(Self::req(pb::GetObjectRequest::default().with_object_id(id).with_read_mask(include.mask()))).await?.into_inner();
            r.object.as_ref().map(ObjectInfo::from_proto).ok_or_else(|| Error::decode("no object in the answer"))
        })
    }

    fn objects(&self, ids: &[String], include: Include) -> BoxFut<'_, Vec<Result<ObjectInfo>>> {
        let ids = ids.to_vec();
        Box::pin(async move {
            if ids.is_empty() {
                return Ok(vec![]);
            }
            let mut out = Vec::with_capacity(ids.len());
            // The node caps a batch; 50 at a time.
            for chunk in ids.chunks(50) {
                let req = pb::BatchGetObjectsRequest::default()
                    .with_requests(chunk.iter().map(|id| pb::GetObjectRequest::default().with_object_id(id.clone())).collect::<Vec<_>>())
                    .with_read_mask(include.mask());
                let r = self.rpc.clone().ledger_client().batch_get_objects(Self::req(req)).await?.into_inner();
                for o in r.objects {
                    out.push(match o.result {
                        Some(pb::get_object_result::Result::Object(o)) => Ok(ObjectInfo::from_proto(&o)),
                        Some(pb::get_object_result::Result::Error(s)) => Err(Error::Grpc { code: s.code, message: s.message }),
                        _ => Err(Error::decode("empty object result")),
                    });
                }
            }
            Ok(out)
        })
    }

    fn dynamic_fields(&self, parent: &str, max: usize) -> BoxFut<'_, Vec<DynField>> {
        let parent = parent.to_string();
        Box::pin(async move {
            let req = pb::ListDynamicFieldsRequest::default()
                .with_parent(parent)
                .with_page_size(max.clamp(1, 50) as u32)
                .with_read_mask(FieldMask::from_paths(["kind", "parent", "field_id", "child_id", "name", "value_type"]));
            let v: Vec<pb::DynamicField> = self.rpc.list_dynamic_fields(req).take(max).try_collect().await?;
            Ok(v.iter().map(DynField::from_proto).collect())
        })
    }

    fn default_name(&self, address: &str) -> BoxFut<'_, Option<String>> {
        let address = canon_addr(address);
        Box::pin(async move {
            match self.names.clone().reverse_lookup_name(Self::req(pb::ReverseLookupNameRequest::default().with_address(address))).await {
                Ok(r) => Ok(r.into_inner().record.and_then(|n| n.name)),
                Err(s) if s.code() == tonic::Code::NotFound => Ok(None),
                Err(s) => Err(s.into()),
            }
        })
    }

    fn resolve_name(&self, name: &str) -> BoxFut<'_, Option<String>> {
        let name = name.trim().to_lowercase();
        Box::pin(async move {
            match self.names.clone().lookup_name(Self::req(pb::LookupNameRequest::default().with_name(name))).await {
                Ok(r) => Ok(r.into_inner().record.and_then(|n| n.target_address).map(|a| canon_addr(&a))),
                Err(s) if s.code() == tonic::Code::NotFound => Ok(None),
                Err(s) => Err(s.into()),
            }
        })
    }

    fn datatype_abilities(&self, package: &str, module: &str, name: &str) -> BoxFut<'_, Vec<i32>> {
        let req = pb::GetDatatypeRequest::default().with_package_id(package).with_module_name(module).with_name(name);
        Box::pin(async move {
            let r = self.rpc.clone().package_client().get_datatype(Self::req(req)).await?.into_inner();
            Ok(r.datatype.map(|d| d.abilities).unwrap_or_default())
        })
    }
}

fn balance_from(b: &pb::Balance) -> CoinBalance {
    CoinBalance {
        coin_type: b.coin_type.clone().unwrap_or_default(),
        balance: b.balance.unwrap_or(0),
        coin_balance: b.coin_balance,
        address_balance: b.address_balance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_kinds_map() {
        let mut o = pb::Owner::default();
        o.kind = Some(pb::owner::OwnerKind::Address as i32);
        o.address = Some("0x1".into());
        assert_eq!(Owner::from_proto(Some(&o)), Owner::Address { address: canon_addr("0x1") });
        o.kind = Some(pb::owner::OwnerKind::Shared as i32);
        assert_eq!(Owner::from_proto(Some(&o)), Owner::Shared);
        assert_eq!(Owner::from_proto(None), Owner::Unknown);
    }

    struct Abilities(Vec<i32>);
    impl ChainRead for Abilities {
        fn datatype_abilities(&self, _p: &str, _m: &str, _n: &str) -> BoxFut<'_, Vec<i32>> {
            let v = self.0.clone();
            Box::pin(async move { Ok(v) })
        }
    }

    #[tokio::test]
    async fn has_store_reads_abilities() {
        assert_eq!(has_store(&Abilities(vec![3, 4]), "0x1::a::B").await.unwrap(), Some(true));
        assert_eq!(has_store(&Abilities(vec![4]), "0x1::a::B").await.unwrap(), Some(false));
        assert_eq!(has_store(&Abilities(vec![]), "0x1::a::B<0x2::c::D>").await.unwrap(), None);
    }

    #[tokio::test]
    async fn unsupported_reads_say_so() {
        let e = Abilities(vec![]).reference_gas_price().await.unwrap_err();
        assert!(e.to_string().contains("not supported"));
    }
}
