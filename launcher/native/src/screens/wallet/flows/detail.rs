//! Detail pages: one collectible (Nft.tsx, when opened outside home's item sheet) and one past
//! transaction (ActivityDetail.tsx). The collectible reads rare.market and the chain
//! (`rare_sui::nfts::nft_detail`); the transaction reads the node (`rare_sui::txdetail`) and coin
//! names and prices from the Trade API, then chain metadata.

use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::pages::{activity, nft};
use crate::screens::wallet::service::{self, poll};
use crate::screens::wallet::types as t;
use crate::state::State;
use rare_sui::BoxFut;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;

type Rx<T> = Option<Receiver<Result<T, String>>>;

type TxAnswer = (Option<activity::TxDetail>, BTreeMap<String, activity::CoinInfo>);

#[derive(Default)]
pub struct Flow {
    /// the stack frame the state below belongs to
    key: u64,
    nft: nft::Load,
    nft_copied: Option<String>,
    act: activity::State,
    /// None while loading; Some(None) when the node pruned it
    tx: Option<Option<activity::TxDetail>>,
    tx_loading: bool,
    tx_error: bool,
    coins: BTreeMap<String, activity::CoinInfo>,
    rx_nft: Rx<Result<t::NftDetail, String>>,
    rx_tx: Rx<Result<TxAnswer, String>>,
    /// a link for the shell to open
    pub web: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Nft(nft::Msg),
    Activity(activity::Msg),
}

/// rare.market over the launcher's cached net, for `rare_sui::nfts::nft_detail`.
struct Market;

impl rare_sui::market::MarketApi for Market {
    fn get(&self, path: &str) -> BoxFut<'_, Value> {
        let path = path.to_string();
        Box::pin(async move {
            use rare_market::net::{Call, Net};
            let net = crate::screens::wallet::live::market_net();
            let r = net.call(&Call::get(format!("{}{path}", net.market_origin()))).map_err(|e| rare_sui::Error::http(None, e.to_string()))?;
            if !r.ok() {
                return Err(rare_sui::Error::http(Some(r.status), format!("HTTP {}", r.status)));
            }
            Ok(r.json())
        })
    }
}

fn owner_of(o: &rare_sui::nfts::ChainOwner) -> t::OwnerKind {
    use rare_sui::nfts::ChainOwner as C;
    match o {
        C::Address { .. } => t::OwnerKind::Address,
        C::Object { .. } => t::OwnerKind::Object,
        C::Shared => t::OwnerKind::Shared,
        C::Immutable => t::OwnerKind::Immutable,
        C::Unknown => t::OwnerKind::Unknown,
    }
}

/// rare-sui's collectible in the page's shape.
pub fn nft_of(d: rare_sui::nfts::NftDetail) -> t::NftDetail {
    let s = |v: &Value, k: &str| match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };
    t::NftDetail {
        owner: owner_of(&d.owner),
        id: d.id,
        name: d.name,
        image_url: d.image_url,
        description: d.description,
        collection: d.collection.map(|c| t::CollectionRef { slug: c.slug, name: c.name, image_url: c.image_url, verified: c.verified }),
        attributes: d.attributes.iter().map(|a| t::Attribute { type_: s(a, "type"), value: s(a, "value"), pct: a.get("pct").and_then(Value::as_f64) }).collect(),
        rarity: d.rarity.map(|r| t::Rarity { rank: r.rank.max(0.0) as u32, tier: r.tier }),
        listing: d.listing.map(|l| t::Listing {
            venue: l.extra.get("venue").and_then(Value::as_str).unwrap_or_default().to_string(),
            price_mist: l.price_mist.parse().unwrap_or(0),
            all_in_mist: l.all_in_mist.and_then(|m| m.parse().ok()),
        }),
        kiosk_id: d.kiosk_id,
        in_kiosk: d.in_kiosk,
        can_send: d.can_send,
        cant_send: d.cant_send,
    }
}

/// rare-sui's transaction in the page's shape.
pub fn tx_of(d: rare_sui::txdetail::TxDetail) -> activity::TxDetail {
    use rare_sui::txdetail::{ObjAction, TxStatus};
    activity::TxDetail {
        flows: d.flows.into_iter().map(|f| activity::Flow { coin_type: f.coin_type, amount: f.amount }).collect(),
        counterparty: d.counterparty,
        failed: d.status == TxStatus::Failure,
        error: d.error,
        timestamp_ms: d.timestamp_ms,
        fee_mist: d.fee_mist.parse().unwrap_or(0),
        fee_paid: d.fee_paid,
        sender: Some(d.sender).filter(|s| !s.is_empty()),
        calls: d.calls.into_iter().map(|c| (c.module, c.function)).collect(),
        objects: d
            .objects
            .into_iter()
            .map(|o| activity::TxObject {
                id: o.id,
                action: match o.action {
                    ObjAction::Sent => "sent",
                    ObjAction::Created => "created",
                    ObjAction::Deleted => "deleted",
                    ObjAction::Received => "received",
                }
                .into(),
                type_: o.object_type,
            })
            .collect(),
    }
}

/// `useCoinInfo`: the Trade API first, then what the list recorded, then the chain's metadata.
async fn coin_infos(w: &'static service::Wallet, types: Vec<String>, hints: Vec<t::ActivityChange>) -> BTreeMap<String, activity::CoinInfo> {
    use rare_sui::grpc::ChainRead;
    let norm = |s: &str| rare_sui::ids::canon_type(s);
    let http: std::sync::Arc<dyn rare_sui::prices::HttpGet> = std::sync::Arc::new(rare_sui::prices::ReqwestGet::default());
    let trade = rare_sui::prices::PriceClient::new(rare_sui::prices::TRADE_URL, http);
    let metas = trade.tokens(&types).await;
    let mut out = BTreeMap::new();
    for ty in types {
        let h = hints.iter().find(|h| norm(&h.coin_type) == norm(&ty));
        let m = metas.get(&ty).or_else(|| metas.get(&norm(&ty)));
        let info = if let Some(m) = m {
            activity::CoinInfo { symbol: m.symbol.clone(), decimals: m.decimals, logo: m.logo.clone(), price: m.price_usd, known: true }
        } else if let Some((sym, dec)) = h.and_then(|h| h.decimals.map(|d| (h.symbol.clone(), d))) {
            activity::CoinInfo { symbol: sym.unwrap_or_else(|| ty.rsplit("::").next().unwrap_or("?").to_string()), decimals: dec, logo: None, price: None, known: true }
        } else {
            let meta = w.sui.coin_metadata(&ty).await.ok().flatten();
            activity::CoinInfo {
                symbol: meta.as_ref().map(|m| m.symbol.clone()).unwrap_or_else(|| ty.rsplit("::").next().unwrap_or("?").to_string()),
                decimals: meta.as_ref().map(|m| m.decimals).unwrap_or(9),
                logo: meta.as_ref().and_then(|m| m.icon_url.clone()),
                price: None,
                known: meta.is_some(),
            }
        };
        out.insert(ty, info);
    }
    out
}

impl WalletTab {
    pub(in crate::screens::wallet) fn detail_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::Nft { .. } | WalletView::Activity { .. })
    }

    fn known_activity(&self, digest: &str) -> Option<&t::ActivityItem> {
        self.act.items.iter().find(|i| i.digest == digest)
    }

    pub(in crate::screens::wallet) fn detail_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let f = &self.flows.detail;
        match v {
            WalletView::Nft { .. } => {
                let can_sign = self.current().is_some_and(|a| a.kind != "watch");
                Some(nft::view(&f.nft, can_sign, f.nft_copied.as_deref()).map(&Msg::Nft))
            }
            WalletView::Activity { digest } => {
                let item = self.known_activity(digest);
                let pr = activity::Props {
                    digest,
                    item,
                    detail: f.tx.as_ref().map(|d| d.as_ref()),
                    loading: f.tx_loading,
                    error: f.tx_error,
                    coins: &f.coins,
                    tz_min: 0,
                };
                Some(activity::view(&pr, &f.act).map(&Msg::Activity))
            }
            _ => None,
        }
    }

    pub(in crate::screens::wallet) fn detail_handle(&mut self, st: &mut State, m: Msg) {
        match m {
            Msg::Nft(nft::Msg::Push(v)) => self.push(st, v),
            Msg::Nft(nft::Msg::OpenInMarket(id)) => self.flows.detail.web = Some(format!("https://rare.market/item/{id}")),
            Msg::Nft(nft::Msg::Copy(v)) => {
                crate::screens::wallet::flows::manage::copy_text(&v);
                self.flows.detail.nft_copied = Some(v);
            }
            Msg::Nft(nft::Msg::Nothing) => {}
            Msg::Activity(m) => match activity::update(&mut self.flows.detail.act, m) {
                Some(activity::Msg::Copy(v)) => crate::screens::wallet::flows::manage::copy_text(&v),
                Some(activity::Msg::OpenWeb(u)) => self.flows.detail.web = Some(u),
                _ => {}
            },
        }
    }

    pub(in crate::screens::wallet) fn detail_tick(&mut self, _st: &mut State) -> bool {
        let top = self.stack.top().clone();
        let me = self.current().map(|a| a.address.clone());
        if matches!(top.view, WalletView::Nft { .. } | WalletView::Activity { .. }) && self.flows.detail.key != top.key {
            self.flows.detail = Flow { key: top.key, ..Default::default() };
            self.ver += 1;
            match &top.view {
                WalletView::Nft { id } => {
                    self.flows.detail.nft.loading = true;
                    if self.fixture {
                        self.flows.detail.nft = nft::Load { data: None, loading: false, error: true };
                    } else {
                        let (id, me) = (id.clone(), me.clone());
                        self.flows.detail.rx_nft = Some(service::work(move |w| {
                            let r = w.rt.block_on(rare_sui::nfts::nft_detail(&Market, &w.sui, &id, me.as_deref()));
                            r.map(nft_of).map_err(|e| e.to_string())
                        }));
                    }
                }
                WalletView::Activity { digest } => {
                    if !self.fixture {
                        if let Some(me) = me {
                            self.flows.detail.tx_loading = true;
                            let digest = digest.clone();
                            let hints: Vec<t::ActivityChange> = self.known_activity(&digest).map(|i| i.changes.clone()).unwrap_or_default();
                            self.flows.detail.rx_tx = Some(service::work(move |w| {
                                w.rt.block_on(async {
                                    let d = rare_sui::txdetail::activity_detail(&w.sui, &digest, &me).await.map_err(|e| e.to_string())?;
                                    let mut types: Vec<String> = hints.iter().map(|h| h.coin_type.clone()).collect();
                                    if let Some(d) = &d {
                                        types.extend(d.flows.iter().map(|f| f.coin_type.clone()));
                                    }
                                    types.push(rare_sui::ids::SUI_TYPE.to_string());
                                    types.sort();
                                    types.dedup();
                                    let coins = coin_infos(w, types, hints).await;
                                    Ok::<_, String>((d.map(tx_of), coins))
                                })
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(r) = poll(&mut self.flows.detail.rx_nft) {
            let f = &mut self.flows.detail;
            f.nft.loading = false;
            match r.and_then(|x| x) {
                Ok(d) => f.nft.data = Some(d),
                Err(e) => {
                    log::warn!("wallet nft detail: {e}");
                    f.nft.error = true;
                }
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.detail.rx_tx) {
            let f = &mut self.flows.detail;
            f.tx_loading = false;
            match r.and_then(|x| x) {
                Ok((d, coins)) => {
                    f.tx = Some(d);
                    f.coins = coins;
                }
                Err(e) => {
                    log::warn!("wallet tx detail: {e}");
                    f.tx_error = true;
                }
            }
            self.ver += 1;
        }
        self.flows.detail.rx_nft.is_some() || self.flows.detail.rx_tx.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trait_values_come_through_as_text() {
        let d = rare_sui::nfts::NftDetail {
            id: "0x1".into(),
            name: "Popkins #1".into(),
            image_url: None,
            description: String::new(),
            collection: None,
            attributes: vec![json!({"type": "Hat", "value": "Crown", "pct": 2.4}), json!({"type": "Level", "value": 7})],
            rarity: None,
            listing: None,
            kiosk_id: None,
            nft_type: None,
            owner: rare_sui::nfts::ChainOwner::Shared,
            in_kiosk: false,
            has_store: None,
            can_send: false,
            cant_send: Some("It is shared on chain and has no owner to send it".into()),
            rare_missing: false,
        };
        let n = nft_of(d);
        assert_eq!(n.attributes[0], t::Attribute { type_: "Hat".into(), value: "Crown".into(), pct: Some(2.4) });
        assert_eq!(n.attributes[1].value, "7");
        assert_eq!(n.owner, t::OwnerKind::Shared);
    }
}
