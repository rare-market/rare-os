//! The gRPC simulator (ports `review/grpc.ts`, feature `chain`): simulate the exact bytes on a Sui
//! fullnode over `rare_sui::Sui` (never executes), map the node's answer to a [`Simulation`], add
//! best-effort extras (Display names and images, coin metadata, original package ids), and build
//! a `rare_sui::ptb::Tx` for review, turning a build that would fail into a failed review.

use std::collections::BTreeMap;

use rare_sui::grpc::{ChainRead, Include};
use rare_sui::ptb::Tx;
use rare_sui::tx::BuildOpts;
use sui_rpc::proto::sui::rpc::v2 as pb;

use super::packages::{is_coin_object, norm_id};
use super::types::{ChainCoin, DisplayInfo, IdOperation, OutputKind, Owner, SimAbort, SimBalanceChange, SimFailure, SimGas, SimObjectChange, SimSource, SimStatus, Simulation};
use super::{calls_of, join_all, settle, BoxFut, KioskSource, ReviewInput, Simulator, TokenDetail, TokenFlags, TokenInfo};

/// The slice of the node the simulator uses (injectable in tests). The extras are optional: a
/// method answering None means "this client can't", and that extra is left out.
pub trait SimClient: Send + Sync {
    /// Simulate BCS TransactionData bytes.
    fn simulate_bytes<'a>(&'a self, tx_bytes: &'a [u8]) -> BoxFut<'a, rare_sui::Result<pb::SimulateTransactionResponse>>;
    /// Display name and image per object id.
    fn display<'a>(&'a self, _ids: &'a [String]) -> Option<BoxFut<'a, rare_sui::Result<BTreeMap<String, DisplayInfo>>>> {
        None
    }
    fn coin_metadata<'a>(&'a self, _coin_type: &'a str) -> Option<BoxFut<'a, rare_sui::Result<Option<ChainCoin>>>> {
        None
    }
    /// A package's original (first version) id.
    fn original_id<'a>(&'a self, _package: &'a str) -> Option<BoxFut<'a, rare_sui::Result<Option<String>>>> {
        None
    }
}

fn display_of(o: &rare_sui::grpc::ObjectInfo) -> Option<DisplayInfo> {
    o.display.as_ref().filter(|d| !d.is_null())?;
    let s = |k: &str| o.display_str(k).map(str::to_string);
    Some(DisplayInfo { name: s("name"), image: s("image_url").or_else(|| s("img_url")).or_else(|| s("image")) })
}

impl SimClient for rare_sui::Sui {
    fn simulate_bytes<'a>(&'a self, tx_bytes: &'a [u8]) -> BoxFut<'a, rare_sui::Result<pb::SimulateTransactionResponse>> {
        Box::pin(async move {
            // the node would refuse bytes that are not a transaction: same answer, no round trip
            let tx: sui_sdk_types::Transaction = bcs::from_bytes(tx_bytes).map_err(|e| rare_sui::Error::Grpc { code: GRPC_INVALID_ARGUMENT, message: format!("invalid transaction bytes: {e}") })?;
            self.simulate(&tx).await
        })
    }

    fn display<'a>(&'a self, ids: &'a [String]) -> Option<BoxFut<'a, rare_sui::Result<BTreeMap<String, DisplayInfo>>>> {
        Some(Box::pin(async move {
            let got = ChainRead::objects(self, ids, Include::DISPLAY).await?;
            Ok(got.into_iter().flatten().filter_map(|o| display_of(&o).map(|d| (o.object_id.clone(), d))).collect())
        }))
    }

    fn coin_metadata<'a>(&'a self, coin_type: &'a str) -> Option<BoxFut<'a, rare_sui::Result<Option<ChainCoin>>>> {
        Some(Box::pin(async move {
            let m = ChainRead::coin_metadata(self, coin_type).await?;
            Ok(m.map(|m| ChainCoin { symbol: Some(m.symbol), decimals: m.decimals, name: Some(m.name), icon_url: m.icon_url }))
        }))
    }

    fn original_id<'a>(&'a self, package: &'a str) -> Option<BoxFut<'a, rare_sui::Result<Option<String>>>> {
        Some(Box::pin(async move {
            let r = self.rpc().package_client().get_package(pb::GetPackageRequest::default().with_package_id(package)).await?.into_inner();
            Ok(r.package.and_then(|p| p.original_id))
        }))
    }
}

const GRPC_INVALID_ARGUMENT: i32 = 3;
const GRPC_NOT_FOUND: i32 = 5;
const GRPC_FAILED_PRECONDITION: i32 = 9;
const GRPC_OUT_OF_RANGE: i32 = 11;

/// gRPC codes that mean "the node checked it and it would not run", not "the node is down".
fn rejected(e: &rare_sui::Error) -> Option<&str> {
    match e {
        rare_sui::Error::Grpc { code, message } if [GRPC_INVALID_ARGUMENT, GRPC_FAILED_PRECONDITION, GRPC_NOT_FOUND, GRPC_OUT_OF_RANGE].contains(code) => Some(message),
        _ => None,
    }
}

fn map_owner(o: Option<&pb::Owner>) -> Option<Owner> {
    let o = o?;
    let addr = || o.address.clone().unwrap_or_default();
    use pb::owner::OwnerKind as K;
    Some(match o.kind.and_then(|k| K::try_from(k).ok()) {
        Some(K::Address) | Some(K::ConsensusAddress) => Owner::Address { address: addr() },
        Some(K::Object) => Owner::Object { id: addr() },
        Some(K::Shared) => Owner::Shared,
        Some(K::Immutable) => Owner::Immutable,
        _ => Owner::Unknown,
    })
}

/// 1st, 2nd, 3rd, 4th, 11th, 21st (`Intl.PluralRules("en-US", { type: "ordinal" })`).
fn ordinal(n: u64) -> String {
    let s = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{s}")
}

/// The SDK's MoveAbort message: `MoveAbort in 1st command, abort code: 2, in '0x2::balance::split' (instruction 7)`.
fn move_abort_message(command: Option<u64>, a: &pb::MoveAbort) -> String {
    let mut parts = vec![match command {
        Some(c) => format!("MoveAbort in {} command", ordinal(c + 1)),
        None => "MoveAbort".to_string(),
    }];
    let clever = a.clever_error.as_ref();
    match clever.and_then(|c| c.constant_name.as_deref()).filter(|n| !n.is_empty()) {
        Some(name) => match clever.and_then(|c| match &c.value {
            Some(pb::clever_error::Value::Rendered(v)) if !v.is_empty() => Some(v.as_str()),
            _ => None,
        }) {
            Some(v) => parts.push(format!("'{name}': {v}")),
            None => parts.push(format!("'{name}'")),
        },
        None => parts.push(format!("abort code: {}", a.abort_code.unwrap_or(0))),
    }
    if let Some(loc) = &a.location {
        if let (Some(pkg), Some(module)) = (loc.package.as_deref().filter(|s| !s.is_empty()), loc.module.as_deref().filter(|s| !s.is_empty())) {
            let pkg = if pkg.starts_with("0x") { pkg.to_string() } else { format!("0x{pkg}") };
            let mut path = vec![pkg, module.to_string()];
            if let Some(f) = loc.function_name.as_deref().filter(|s| !s.is_empty()) {
                path.push(f.to_string());
            }
            let mut s = format!("in '{}'", path.join("::"));
            if let Some(line) = clever.and_then(|c| c.line_number) {
                s.push_str(&format!(" (line {line})"));
            } else if let Some(i) = loc.instruction {
                s.push_str(&format!(" (instruction {i})"));
            }
            parts.push(s);
        }
    }
    parts.join(", ")
}

/// The SDK's execution error, then `mapFailure`: message, kind, command and the abort location.
pub fn map_failure(err: Option<&pb::ExecutionError>) -> SimFailure {
    let Some(err) = err else { return SimFailure { message: "Transaction failed".into(), kind: Some("Unknown".into()), command: None, abort: None } };
    let message = err.description.clone().unwrap_or_else(|| "Unknown error".into());
    let command = err.command;
    use pb::execution_error::ErrorDetails as D;
    let (kind, message, abort) = match &err.error_details {
        Some(D::Abort(a)) => {
            let loc = a.location.as_ref();
            let abort = SimAbort {
                code: a.abort_code.unwrap_or(0).to_string(),
                package: loc.and_then(|l| l.package.clone()),
                module: loc.and_then(|l| l.module.clone()),
                function: loc.and_then(|l| l.function_name.clone()),
                constant: a.clever_error.as_ref().and_then(|c| c.constant_name.clone()),
            };
            ("MoveAbort", move_abort_message(command, a), Some(abort))
        }
        Some(D::SizeError(_)) => ("SizeError", message, None),
        Some(D::CommandArgumentError(_)) => ("CommandArgumentError", message, None),
        Some(D::TypeArgumentError(_)) => ("TypeArgumentError", message, None),
        Some(D::PackageUpgradeError(_)) => ("PackageUpgradeError", message, None),
        Some(D::IndexError(_)) => ("IndexError", message, None),
        Some(D::CoinDenyListError(_)) => ("CoinDenyListError", message, None),
        Some(D::CongestedObjects(_)) => ("CongestedObjects", message, None),
        Some(D::ObjectId(_)) => ("ObjectIdError", message, None),
        _ => ("Unknown", message, None),
    };
    SimFailure { message, kind: Some(kind.into()), command, abort }
}

fn change(o: &pb::ChangedObject) -> SimObjectChange {
    use pb::changed_object::{IdOperation as Op, InputObjectState as In, OutputObjectState as Out};
    let input = o.input_state.and_then(|s| In::try_from(s).ok());
    let output = o.output_state.and_then(|s| Out::try_from(s).ok());
    SimObjectChange {
        object_id: o.object_id.clone().unwrap_or_default(),
        object_type: o.object_id.as_ref().and(o.object_type.clone()),
        input_owner: map_owner(o.input_owner.as_ref()),
        output_owner: map_owner(o.output_owner.as_ref()),
        id_operation: match o.id_operation.map(Op::try_from) {
            Some(Ok(Op::Created)) => IdOperation::Created,
            Some(Ok(Op::Deleted)) => IdOperation::Deleted,
            Some(Ok(Op::None)) | Some(Ok(Op::Unknown)) => IdOperation::Unchanged,
            _ => IdOperation::Unknown,
        },
        input_exists: input == Some(In::Exists),
        output_exists: matches!(output, Some(Out::ObjectWrite) | Some(Out::PackageWrite)),
        output_kind: Some(match output {
            Some(Out::PackageWrite) => OutputKind::Package,
            Some(Out::AccumulatorWrite) => OutputKind::Accumulator,
            Some(Out::ObjectWrite) => OutputKind::Object,
            _ => OutputKind::Unknown,
        }),
    }
}

/// Map the node's simulate answer into a [`Simulation`] (pure; `fromSdk`).
pub fn from_response(r: &pb::SimulateTransactionResponse) -> Simulation {
    let t = r.transaction.as_ref();
    let eff = t.and_then(|t| t.effects.as_ref());
    let status = eff.and_then(|e| e.status.as_ref());
    let ok = status.and_then(|s| s.success).unwrap_or(false);
    let gas_obj = eff.and_then(|e| e.gas_object.as_ref());
    let gas_owner = match map_owner(gas_obj.and_then(|g| g.input_owner.as_ref())) {
        Some(Owner::Address { address }) => Some(address),
        _ => None,
    };
    let g = eff.and_then(|e| e.gas_used.as_ref());
    Simulation {
        source: Some(SimSource::Node),
        status: if ok { SimStatus::Success } else { SimStatus::Failure },
        failure: (!ok).then(|| map_failure(status.and_then(|s| s.error.as_ref()))),
        balance_changes: t
            .map(|t| t.balance_changes.iter().map(|b| SimBalanceChange { coin_type: b.coin_type.clone().unwrap_or_default(), address: b.address.clone().unwrap_or_default(), amount: b.amount.clone().unwrap_or_default() }).collect())
            .unwrap_or_default(),
        objects: eff.map(|e| e.changed_objects.iter().map(change).collect()).unwrap_or_default(),
        gas: SimGas {
            computation: g.and_then(|g| g.computation_cost).unwrap_or(0).to_string(),
            storage: g.and_then(|g| g.storage_cost).unwrap_or(0).to_string(),
            rebate: g.and_then(|g| g.storage_rebate).unwrap_or(0).to_string(),
        },
        gas_owner,
        gas_object_id: gas_obj.and_then(|g| g.object_id.clone()),
        display: None,
        packages: None,
        coins: None,
    }
}

/// `0x..::dynamic_field::Field<`
fn is_field(t: Option<&str>) -> bool {
    t.is_some_and(|t| t.contains("::dynamic_field::Field<"))
}

/// Best-effort extras on a simulation: names/images of objects leaving, coin decimals, original package ids.
pub async fn add_extras(client: &dyn SimClient, sim: &mut Simulation, bytes: &[u8], extras_ms: u64) {
    // owned objects and kiosk items (owned by a dynamic field), not the field wrappers themselves
    let leaving: Vec<String> = sim
        .objects
        .iter()
        .filter(|o| {
            o.input_exists
                && !is_coin_object(o.object_type.as_deref())
                && (matches!(o.input_owner, Some(Owner::Address { .. })) || (matches!(o.input_owner, Some(Owner::Object { .. })) && !is_field(o.object_type.as_deref())))
                && o.output_kind != Some(OutputKind::Package)
        })
        .map(|o| o.object_id.clone())
        .take(50)
        .collect();
    let mut coin_types: Vec<String> = vec![];
    for b in &sim.balance_changes {
        if !coin_types.contains(&b.coin_type) {
            coin_types.push(b.coin_type.clone());
        }
    }
    let mut pkgs: Vec<String> = vec![];
    for c in calls_of(bytes) {
        let p = norm_id(&c.package);
        if !pkgs.contains(&p) {
            pkgs.push(p);
        }
    }

    let objs = async {
        if leaving.is_empty() {
            return None;
        }
        let f = client.display(&leaving)?;
        settle(f, extras_ms).await.and_then(Result::ok)
    };
    let metas = async {
        // probe: a client without coin metadata leaves `coins` out
        drop(client.coin_metadata(SUI_PROBE)?);
        let futs: Vec<BoxFut<'_, (String, Option<ChainCoin>)>> = coin_types
            .iter()
            .map(|t| -> BoxFut<'_, (String, Option<ChainCoin>)> {
                Box::pin(async move {
                    let m = match client.coin_metadata(t) {
                        Some(f) => settle(f, extras_ms).await.and_then(Result::ok).flatten(),
                        None => None,
                    };
                    (t.clone(), m)
                })
            })
            .collect();
        Some(join_all(futs).await)
    };
    let origs = async {
        drop(client.original_id(SUI_PROBE)?);
        let futs: Vec<BoxFut<'_, (String, Option<String>)>> = pkgs
            .iter()
            .map(|p| -> BoxFut<'_, (String, Option<String>)> {
                Box::pin(async move {
                    let o = match client.original_id(p) {
                        Some(f) => settle(f, extras_ms).await.and_then(Result::ok).flatten(),
                        None => None,
                    };
                    (p.clone(), o)
                })
            })
            .collect();
        Some(join_all(futs).await)
    };
    let (objs, metas, origs) = tokio::join!(objs, metas, origs);
    if let Some(d) = objs {
        sim.display = Some(d);
    }
    if let Some(m) = metas {
        sim.coins = Some(m.into_iter().filter_map(|(t, c)| c.map(|c| (t, c))).collect());
    }
    if let Some(o) = origs {
        sim.packages = Some(o.into_iter().filter_map(|(id, orig)| orig.filter(|s| !s.is_empty()).map(|orig| (id, norm_id(&orig)))).collect());
    }
}

/// Used only to ask a client whether it has an extra at all (the future is dropped unpolled).
const SUI_PROBE: &str = "0x2::sui::SUI";

/// Simulator backed by a Sui fullnode. Simulates the exact bytes; never executes.
pub struct GrpcSimulator<C> {
    pub client: C,
    /// Ms to wait for each best-effort extra.
    pub extras_ms: u64,
}

impl<C: SimClient> GrpcSimulator<C> {
    pub fn new(client: C) -> Self {
        GrpcSimulator { client, extras_ms: 2500 }
    }
}

impl<C: SimClient> Simulator for GrpcSimulator<C> {
    fn simulate<'a>(&'a self, tx_bytes: &'a [u8]) -> BoxFut<'a, Result<Simulation, String>> {
        Box::pin(async move {
            let mut sim = match self.client.simulate_bytes(tx_bytes).await {
                Ok(r) => from_response(&r),
                Err(e) => {
                    return match rejected(&e) {
                        Some(m) => Ok(Simulation::failed(SimFailure { message: m.to_string(), kind: Some("Rejected".into()), command: None, abort: None })),
                        None => Err(crate::err("network", e)),
                    };
                }
            };
            add_extras(&self.client, &mut sim, tx_bytes, self.extras_ms).await;
            Ok(sim)
        })
    }
}

/// Build a PTB for review against the node (sender set if missing). A build whose own simulation
/// fails is [`ReviewInput::WouldFail`] with the reason; anything else (node down, bad input) is
/// an error, like the TS `reviewTransaction` rethrowing.
pub async fn build_for_review(sui: &rare_sui::Sui, tx: &Tx, sender: &str, opts: &BuildOpts) -> Result<ReviewInput, String> {
    let mut tx = tx.clone();
    if tx.sender.is_none() {
        tx.set_sender(sender);
    }
    let b = rare_sui::tx::to_builder(&tx, opts).map_err(|e| crate::err("build", e))?;
    match b.build(&mut sui.rpc()).await {
        Ok(data) => bcs::to_bytes(&data).map(ReviewInput::Bytes).map_err(|e| crate::err("build", e)),
        Err(sui_transaction_builder::Error::SimulationFailure(f)) => Ok(ReviewInput::WouldFail(map_failure(Some(f.execution_error())))),
        Err(e) => Err(crate::err("build", e)),
    }
}

impl TokenInfo for rare_sui::prices::PriceClient {
    fn token_detail<'a>(&'a self, coin_type: &'a str) -> BoxFut<'a, Option<TokenDetail>> {
        Box::pin(async move {
            let d = rare_sui::prices::PriceClient::token_detail(self, coin_type).await?;
            Some(TokenDetail {
                symbol: Some(d.meta.symbol),
                decimals: Some(d.meta.decimals),
                name: Some(d.meta.name),
                logo: d.meta.logo,
                price_usd: d.meta.price_usd,
                safety: d.safety.map(|s| TokenFlags { spam: s.spam, honeypot: s.honeypot }),
            })
        })
    }
}

impl KioskSource for rare_sui::Sui {
    fn owned_kiosk_ids<'a>(&'a self, owner: &'a str) -> BoxFut<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            let k = rare_sui::kiosk::owned_kiosks(self, owner).await.map_err(|e| e.to_string())?;
            Ok(k.kiosks.into_iter().map(|k| k.kiosk_id).collect())
        })
    }
}
