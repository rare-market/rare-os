//! What the inventory can do with an item: quote the best offer, sell into it, list, cancel a
//! listing. Ported from launcher/src/inventory/trade.ts. The builders are [`crate::sell`]'s; every
//! transaction goes through the wallet's review sheet ([`Executor`], implemented by `rare-wallet`).
//! Mock mode swaps in [`super::mock`] so the flows run without a chain.

use super::InvItem;
use crate::api::Offer;
use crate::net::Net;
use crate::ptb::Tx;
use crate::sell::{best_offer, build_accept_offer_transaction, build_cancel_transaction, build_list_transaction, offer_net_mist};
use crate::venues::{KioskReader, VenueConfig};
use crate::{Error, Result};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub offer: Offer,
    /// What the seller receives, MIST.
    pub net_mist: i128,
    /// Plain reason this offer can't be filled here, or None.
    pub blocked: Option<String>,
    /// Rare's gas sponsor will pay the network fee for this sale.
    pub sponsored: bool,
}

pub trait TradeApi: Send + Sync {
    fn quote(&self, item: &InvItem, sender: &str) -> Result<Option<Quote>>;
    fn sell(&self, item: &InvItem, offer: &Offer, sender: &str) -> Result<()>;
    fn list(&self, item: &InvItem, price_mist: u64, sender: &str) -> Result<()>;
    fn cancel(&self, item: &InvItem, sender: &str) -> Result<()>;
}

/// Who asks for the signature, shown on the review sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub name: &'static str,
    pub kind: &'static str,
}

pub const ORIGIN: Origin = Origin { name: "Rare Marketplace", kind: "market" };

/// How an executed transaction ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecStatus {
    Success,
    Failure,
}

/// Rare Wallet's side (`rare-wallet`): build, review, sign and execute a transaction for `account`
/// (`signAndExecuteWithCover`: Rare's gas sponsor pays when the wallet is short of SUI and the
/// sponsor offers it), and whether the sponsor would cover a trade now. A player who cancels the
/// sheet comes back as an error whose message the caller passes on (see [`super::notice`]).
pub trait Executor: Send + Sync {
    fn sign_and_execute(&self, tx: Tx, account: &str, origin: &Origin, title: &str) -> Result<ExecStatus>;
    fn sponsored(&self, sender: &str) -> bool;
}

/// The real trade path: rare.market reads through the net, builders, the wallet.
pub struct RealTrade {
    pub net: Arc<dyn Net>,
    pub cfg: VenueConfig,
    pub chain: Option<Arc<dyn KioskReader>>,
    pub exec: Arc<dyn Executor>,
}

impl RealTrade {
    fn run(&self, tx: Tx, sender: &str, title: &str) -> Result<()> {
        // Rare pays the fee when the wallet is short of SUI and the sponsor offers it; otherwise the wallet does.
        match self.exec.sign_and_execute(tx, sender, &ORIGIN, title)? {
            ExecStatus::Success => Ok(()),
            ExecStatus::Failure => Err(Error::new("The network rejected this transaction.")),
        }
    }
    fn chain(&self) -> Option<&dyn KioskReader> {
        self.chain.as_deref()
    }
}

impl TradeApi for RealTrade {
    fn quote(&self, item: &InvItem, sender: &str) -> Result<Option<Quote>> {
        let Some(offer) = best_offer(&*self.net, &self.cfg, &item.id, crate::time::now_ms()).ok().flatten() else { return Ok(None) };
        let blocked = build_accept_offer_transaction(&*self.net, &self.cfg, item, &offer, sender, self.chain())
            .err()
            .map(|e| if e.0.is_empty() { "This offer can't be filled here.".to_string() } else { e.0 });
        let sponsored = self.exec.sponsored(sender);
        Ok(Some(Quote { net_mist: offer_net_mist(&offer), offer, blocked, sponsored }))
    }
    fn sell(&self, item: &InvItem, offer: &Offer, sender: &str) -> Result<()> {
        let tx = build_accept_offer_transaction(&*self.net, &self.cfg, item, offer, sender, self.chain())?;
        self.run(tx, sender, &format!("Sell {}", item.name))
    }
    fn list(&self, item: &InvItem, price_mist: u64, sender: &str) -> Result<()> {
        let tx = build_list_transaction(&*self.net, &self.cfg, item, price_mist, sender, self.chain())?;
        self.run(tx, sender, &format!("List {}", item.name))
    }
    fn cancel(&self, item: &InvItem, sender: &str) -> Result<()> {
        let tx = build_cancel_transaction(&*self.net, item, sender, self.chain())?;
        self.run(tx, sender, &format!("Cancel listing of {}", item.name))
    }
}
