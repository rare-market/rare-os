//! Bridges to `rare-sui`, whose reads are async: the seller's kiosks from the chain (the fallback
//! when rare.market can't list them or trails the chain), and rare.market reads for `rare-sui`
//! through this crate's cached [`Net`] (its `market::MarketApi`), so both crates share one cache.
//!
//! The market builders are blocking (the screens run them on a worker thread), so the kiosk read
//! waits on the runtime that drives `rare-sui`. Never call it from inside that runtime.

use crate::net::{self, Net, NetError};
use crate::venues::{KioskReader, OwnedKioskRef};
use rare_sui::grpc::ChainRead;
use std::sync::Arc;

/// Kiosks owned by an address, read over gRPC.
pub struct ChainKiosks {
    pub chain: Arc<dyn ChainRead>,
    pub rt: tokio::runtime::Handle,
}

impl KioskReader for ChainKiosks {
    fn owned_kiosks(&self, owner: &str) -> crate::Result<Vec<OwnedKioskRef>> {
        let owned = self.rt.block_on(rare_sui::kiosk::owned_kiosks(&*self.chain, owner)).map_err(|e| crate::Error(e.to_string()))?;
        Ok(owned
            .kiosks
            .into_iter()
            .map(|k| OwnedKioskRef { kiosk_id: k.kiosk_id, owner_cap_id: k.owner_cap_id, personal_cap_id: k.personal_cap_id, is_personal: k.is_personal })
            .collect())
    }
}

/// rare.market reads for `rare-sui` through the shared data cache.
#[derive(Clone)]
pub struct NetMarket(pub Arc<dyn Net>);

impl rare_sui::market::MarketApi for NetMarket {
    fn get(&self, path: &str) -> rare_sui::BoxFut<'_, serde_json::Value> {
        let n = self.0.clone();
        let path = path.to_string();
        Box::pin(async move {
            let res = tokio::task::spawn_blocking(move || net::read_market(&*n, &path).map(|b| b.to_string())).await.map_err(|e| rare_sui::Error::http(None, e.to_string()))?;
            match res {
                Ok(body) => serde_json::from_str(&body).map_err(|e| rare_sui::Error::decode(e.to_string())),
                Err(NetError::Status(m)) => {
                    let status = m.rsplit("status code ").next().and_then(|s| s.trim().parse().ok());
                    Err(rare_sui::Error::http(status, m))
                }
                Err(e) => Err(rare_sui::Error::http(None, e.to_string())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::MockNet;

    #[test]
    fn market_reads_for_rare_sui_go_through_the_net() {
        let n = MockNet::new();
        n.on_read("/v1/status", r#"{"suiUsd": 3.5}"#);
        let m = NetMarket(Arc::new(n.clone()));
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let v = rt.block_on(rare_sui::market::MarketApi::get(&m, "/v1/status")).unwrap();
        assert_eq!(v["suiUsd"], 3.5);
        let e = rt.block_on(rare_sui::market::MarketApi::get(&m, "/v1/missing")).unwrap_err();
        assert!(e.is_not_found());
        assert_eq!(n.reads(), ["rare:/v1/status", "rare:/v1/missing"]);
    }
}
