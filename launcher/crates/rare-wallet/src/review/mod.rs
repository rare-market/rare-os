//! Signing review (ports `src/wallet/review`): simulate the exact bytes (never executes) and say
//! in plain words what leaves the wallet, what arrives, the fee and anything risky. This is what
//! the review sheet shows before anything is signed.
//!
//! - [`analyze`], [`describe_review`], [`risk_checks`], [`split_fees`] and the [`packages`]
//!   helpers are pure and build without the `chain` feature. Every user-facing string matches the
//!   TS wallet byte for byte (testdata/review-vectors.json is written by the TS code itself).
//! - [`review_transaction`] runs a [`Simulator`], an optional price/safety source ([`TokenInfo`])
//!   and an optional kiosk reader ([`KioskSource`]); tests pass fixed ones.
//! - [`sim`] (feature `chain`): the gRPC simulator over `rare_sui::Sui`, the SDK-result mapping,
//!   building a `rare_sui::ptb::Tx` for review, and the Trade API and kiosk sources.

pub mod analyze;
pub mod describe;
pub mod fees;
pub mod packages;
pub mod risk;
#[cfg(feature = "chain")]
pub mod sim;
pub mod types;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_vectors;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use analyze::{analyze, call_label, friendly_failure, norm_type, CoinFlow, Fee, Insights, MoveCallRef, ObjectFlow, ObjectFlowKind, OtherReceive, SharedTouch, SUI_TYPE};
pub use describe::{describe_review, lookup_coin, CoinMeta, CoinMetaLookup, DescribeExtras, ReviewRow, RowKind};
pub use fees::{known_fee, split_fees, FeeLine, KnownFeeRecipient, SplitFees};
pub use packages::{coin_type_of, humanize, is_coin_object, known_package, norm_id, object_label, parse_type, short_id, Label, ParsedType};
pub use risk::{address_checks, flow_usd, lookalike_short, priced_usd, risk_checks, AddressHistory, Contact, Risk, RiskContext, TokenFlags};
pub use types::*;

/// A boxed future (object-safe async traits without a macro).
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Simulates exact signed-to-be bytes (BCS TransactionData). Errors only when the node can't be
/// reached; a transaction the node refuses is a failed [`Simulation`].
pub trait Simulator: Send + Sync {
    fn simulate<'a>(&'a self, tx_bytes: &'a [u8]) -> BoxFut<'a, Result<Simulation, String>>;
}

/// What the price/safety source (the Trade API) says about a coin.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenDetail {
    pub symbol: Option<String>,
    pub decimals: Option<u32>,
    pub name: Option<String>,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
    pub safety: Option<TokenFlags>,
}

/// Prices and spam flags (the Trade API). None when unknown or down.
pub trait TokenInfo: Send + Sync {
    fn token_detail<'a>(&'a self, coin_type: &'a str) -> BoxFut<'a, Option<TokenDetail>>;
}

/// The signer's kiosk ids (plain and personal kiosks).
pub trait KioskSource: Send + Sync {
    fn owned_kiosk_ids<'a>(&'a self, owner: &'a str) -> BoxFut<'a, Result<Vec<String>, String>>;
}

/// What to review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewInput {
    /// Built transaction bytes (BCS TransactionData): these are simulated and, if approved, signed.
    Bytes(Vec<u8>),
    /// The transaction would fail before it could be built (the build's own simulation failed):
    /// a failed review with the reason, never a crash.
    WouldFail(SimFailure),
}

pub struct ReviewOptions<'a> {
    /// Price/safety source; None skips it (`trade: null`).
    pub trade: Option<&'a dyn TokenInfo>,
    /// Kiosk reader, used when `kiosks` is None and the transaction touches object-owned items.
    pub kiosk_source: Option<&'a dyn KioskSource>,
    /// Wallet history for new-recipient and lookalike checks; None skips them.
    pub addresses: Option<AddressHistory>,
    /// Signer's portfolio value, for the "sends most of your portfolio" check.
    pub portfolio_usd: Option<f64>,
    /// The signer's kiosk ids, when already known.
    pub kiosks: Option<Vec<String>>,
    /// Rare's fee sponsor wallets: a gas owner among these shows "Paid by Rare".
    pub sponsors: Vec<String>,
    /// Who pays when a known sponsor is the gas owner: "Rare" (default) or a game's name.
    pub paid_by: Option<String>,
    /// Ms to wait for each best-effort extra (prices, kiosks get twice this).
    pub extras_ms: u64,
}

impl Default for ReviewOptions<'_> {
    fn default() -> Self {
        ReviewOptions { trade: None, kiosk_source: None, addresses: None, portfolio_usd: None, kiosks: None, sponsors: vec![], paid_by: None, extras_ms: 2500 }
    }
}

/// Coin metadata plus price, for every coin in the rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCoin {
    pub symbol: String,
    pub decimals: u32,
    pub icon_url: Option<String>,
    pub name: Option<String>,
    pub price_usd: Option<f64>,
}

impl ReviewCoin {
    pub fn meta(&self) -> CoinMeta {
        CoinMeta { symbol: self.symbol.clone(), decimals: self.decimals, icon_url: self.icon_url.clone() }
    }
}

/// The review sheet's content.
#[derive(Clone, Debug, PartialEq)]
pub struct Review {
    pub ok: bool,
    pub insights: Insights,
    pub risks: Vec<Risk>,
    /// "You send" / "You receive" / "Network fee" rows, ready to render.
    pub rows: Vec<ReviewRow>,
    /// The exact bytes that were simulated; sign these. None when the transaction would fail
    /// before it could be built (`insights.reason` says why): never offer Approve then.
    pub bytes: Option<Vec<u8>>,
    pub simulation: Simulation,
    /// Metadata for every coin in the rows, keyed by normalized coin type.
    pub coins: BTreeMap<String, ReviewCoin>,
}

/// Move calls in built transaction bytes, for the calls list and the unknown-contract check.
pub fn calls_of(bytes: &[u8]) -> Vec<MoveCallRef> {
    let Ok(tx) = bcs::from_bytes::<sui_sdk_types::Transaction>(bytes) else { return vec![] };
    let sui_sdk_types::TransactionKind::ProgrammableTransaction(p) = &tx.kind else { return vec![] };
    p.commands
        .iter()
        .filter_map(|c| match c {
            sui_sdk_types::Command::MoveCall(m) => Some(MoveCallRef { package: m.package.to_string(), module: m.module.as_str().to_string(), function: m.function.as_str().to_string() }),
            _ => None,
        })
        .collect()
}

/// Waits at most `ms` for `f`; None on timeout.
pub(crate) async fn settle<T>(f: impl Future<Output = T>, ms: u64) -> Option<T> {
    tokio::time::timeout(Duration::from_millis(ms), f).await.ok()
}

/// Runs futures concurrently on the current task and returns their outputs in order.
pub(crate) async fn join_all<'a, T>(futs: Vec<BoxFut<'a, T>>) -> Vec<T> {
    let mut futs: Vec<Option<BoxFut<'a, T>>> = futs.into_iter().map(Some).collect();
    let mut out: Vec<Option<T>> = futs.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (i, slot) in futs.iter_mut().enumerate() {
            if let Some(f) = slot {
                match f.as_mut().poll(cx) {
                    Poll::Ready(v) => {
                        out[i] = Some(v);
                        *slot = None;
                    }
                    Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    out.into_iter().flatten().collect()
}

fn last_segment(t: &str) -> String {
    t.rsplit("::").next().unwrap_or("?").to_string()
}

/// Simulate (bytes) and explain. Errors only when the node can't be reached.
pub async fn review_transaction(simulator: &dyn Simulator, input: ReviewInput, sender: &str, opts: &ReviewOptions<'_>) -> Result<Review, String> {
    let extras_ms = opts.extras_ms;
    let (bytes, simulation) = match input {
        ReviewInput::Bytes(b) => {
            let mut s = simulator.simulate(&b).await?;
            // gas paid from an address balance leaves no gas object: the gas owner the bytes name
            if s.gas_owner.as_deref().filter(|g| !g.is_empty()).is_none() {
                if let Ok(t) = bcs::from_bytes::<sui_sdk_types::Transaction>(&b) {
                    s.gas_owner = Some(t.gas_payment.owner.to_string());
                }
            }
            (Some(b), s)
        }
        ReviewInput::WouldFail(f) => (None, Simulation::failed(f)),
    };
    // items in the signer's kiosks are owned by the kiosk, so read which kiosks are theirs (best effort)
    let mut kiosks = opts.kiosks.clone();
    if let (None, Some(src)) = (&kiosks, opts.kiosk_source) {
        let touches_items = simulation.objects.iter().any(|o| matches!(o.input_owner, Some(Owner::Object { .. })) || matches!(o.output_owner, Some(Owner::Object { .. })));
        if touches_items {
            kiosks = Some(settle(src.owned_kiosk_ids(sender), extras_ms * 2).await.and_then(Result::ok).unwrap_or_default());
        }
    }
    let calls = bytes.as_deref().map(calls_of).unwrap_or_default();
    let insights = analyze(&simulation, sender, &calls, kiosks.as_deref().unwrap_or(&[]));

    // prices and spam flags from the Trade API, best effort
    let mut types: Vec<String> = vec![SUI_TYPE.to_string()];
    for f in insights.send.iter().chain(insights.receive.iter()) {
        if !types.contains(&f.coin_type) {
            types.push(f.coin_type.clone());
        }
    }
    let mut details: BTreeMap<String, Option<TokenDetail>> = BTreeMap::new();
    if let Some(y) = opts.trade {
        let got = join_all(types.iter().map(|t| -> BoxFut<'_, Option<TokenDetail>> { Box::pin(async move { settle(y.token_detail(t), extras_ms).await.flatten() }) }).collect()).await;
        for (t, d) in types.iter().zip(got) {
            details.insert(t.clone(), d);
        }
    }

    let mut coins: BTreeMap<String, ReviewCoin> = BTreeMap::new();
    for t in &types {
        let chain = simulation.coins.as_ref().and_then(|c| c.get(t).or_else(|| c.iter().find(|(k, _)| norm_type(k) == *t).map(|(_, v)| v)));
        let d = details.get(t).and_then(Option::as_ref);
        let symbol = chain
            .and_then(|c| c.symbol.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| d.and_then(|d| d.symbol.clone()).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| if t == SUI_TYPE { "SUI".into() } else { last_segment(t) });
        coins.insert(
            t.clone(),
            ReviewCoin {
                symbol,
                decimals: chain.map(|c| c.decimals).or_else(|| d.and_then(|d| d.decimals)).unwrap_or(9),
                name: chain.and_then(|c| c.name.clone()).or_else(|| d.and_then(|d| d.name.clone())),
                icon_url: chain.and_then(|c| c.icon_url.clone()).or_else(|| d.and_then(|d| d.logo.clone())),
                price_usd: d.and_then(|d| d.price_usd),
            },
        );
    }

    let risks = {
        let ctx = RiskContext {
            prices: types.iter().map(|t| (t.clone(), coins[t].price_usd)).collect(),
            decimals: Box::new(|t: &str| coins.get(&norm_type(t)).map_or(9, |c| c.decimals)),
            symbol: Box::new(|t: &str| coins.get(&norm_type(t)).map_or_else(|| last_segment(t), |c| c.symbol.clone())),
            portfolio_usd: opts.portfolio_usd,
            safety: types.iter().map(|t| (t.clone(), details.get(t).and_then(Option::as_ref).and_then(|d| d.safety))).collect(),
            originals: simulation.packages.clone(),
            addresses: opts.addresses.clone(),
            self_address: Some(sender.to_string()),
        };
        risk_checks(&insights, &ctx)
    };
    // no fee row for a transaction that could not even be built: there is nothing to pay for
    let gas_owner = simulation.gas_owner.as_deref().filter(|g| !g.is_empty()).map(norm_id);
    let covered_by_rare = gas_owner.as_ref().is_some_and(|g| *g != norm_id(sender) && opts.sponsors.iter().any(|a| norm_id(a) == *g));
    let rows = if bytes.is_some() {
        let metas: BTreeMap<String, CoinMeta> = coins.iter().map(|(k, c)| (k.clone(), c.meta())).collect();
        let extras = DescribeExtras { display: simulation.display.as_ref(), originals: simulation.packages.as_ref(), covered_by_rare, paid_by: opts.paid_by.clone() };
        describe_review(&insights, CoinMetaLookup::Map(&metas), &extras)
    } else {
        vec![]
    };
    Ok(Review { ok: insights.ok && bytes.is_some(), insights, risks, rows, bytes, simulation, coins })
}
