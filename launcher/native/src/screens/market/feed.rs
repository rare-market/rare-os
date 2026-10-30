//! The Marketplace on live data: the paths the page on top reads (`Marketplace::missing`) come from
//! rare.market through the launcher's shared cache (`wallet::live::market_net`): the cached copy
//! at once, then the fresh answer from a worker. The page on top is re-read every 30 s while it is
//! up. Purchases (`market:buy:<id>`) build with `rare_market::buy` on a worker and go to Rare Wallet
//! through `crate::sign`; their phase comes back to the buy sheet.

use super::buy::Phase;
use super::data::{Account, Answers};
use super::shell::Marketplace;
use crate::screens::overlay::common::Cx;
use rare_market::api::{self, ItemDetail};
use serde_json::Value;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Re-read the page on top this often while it is up.
const REFRESH_S: f64 = 30.0;
const KEPT: &str = "market.feed";

pub enum Msg {
    Answer(String, Value),
    Failed(String),
    Buy(String, Phase),
}

pub struct Feed {
    pub a: Answers,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    flight: HashSet<String>,
    /// Paths that failed: not asked again until the next refresh.
    failed: HashSet<String>,
    next_refresh: f64,
    live: bool,
    /// The `keep::epoch` this saw last (a purchase, a notice, waking up: read the page again).
    epoch: u64,
}

pub fn net() -> Arc<dyn rare_market::net::Net> {
    Arc::new(crate::screens::wallet::live::market_net().clone())
}

pub fn wall_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

pub fn market_account() -> Option<Account> {
    crate::sign::account().map(|a| Account { address: a.address, label: a.label, signer: a.signer })
}

impl Feed {
    /// `live` false: the fixture answers, nothing fetched.
    pub fn new(a: Answers, live: bool) -> Feed {
        let (tx, rx) = channel();
        let mut f = Feed { a, tx, rx, flight: HashSet::new(), failed: HashSet::new(), next_refresh: 0.0, live, epoch: crate::data::keep::epoch() };
        if live {
            f.a.now_ms = wall_ms();
            f.a.account = market_account();
        }
        f
    }

    pub fn answers(&self) -> Rc<Answers> {
        Rc::new(self.a.clone())
    }

    /// The live feed the last Marketplace kept (`keep::hold`), else a new one: the answers outlive
    /// the tab, so coming back draws the last values at once.
    pub fn kept() -> Feed {
        let mut f = crate::data::keep::take::<Feed>(KEPT).unwrap_or_else(|| Feed::new(Answers::default(), true));
        f.a.now_ms = wall_ms();
        f.a.account = market_account();
        f
    }

    /// Keeps a live feed for the next Marketplace.
    pub fn keep(self) {
        if self.live {
            crate::data::keep::hold(KEPT, self);
        }
    }

    /// The cached copies of what the pages read, before their first frame (no empty frame first).
    pub fn prefill(&mut self, m: &mut Marketplace, cx: &mut Cx, now: f64) {
        if !self.live {
            return;
        }
        let mut changed = false;
        for p in m.missing() {
            if let Some(v) = api::cached::<Value>(crate::screens::wallet::live::market_net(), &p) {
                self.a.set(p, v);
                changed = true;
            }
        }
        if changed {
            self.a.now_ms = wall_ms();
            m.set_answers(cx, self.answers(), now);
        }
    }

    fn fetch(&mut self, path: String, fresh: bool) {
        if !self.flight.insert(path.clone()) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name("rare-market-read".into())
            .spawn(move || {
                let n = crate::screens::wallet::live::market_net();
                use rare_market::net::Net as _;
                let r = if fresh && rare_market::net::market_path_ok(&path) {
                    n.refresh(&rare_market::net::market_key(&n.market_origin(), &path)).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
                } else {
                    api::get::<Value>(n, &path).map_err(|e| e.0)
                };
                let _ = tx.send(match r {
                    Ok(v) => Msg::Answer(path, v),
                    Err(e) => {
                        log::warn!("market read {path}: {e}");
                        Msg::Failed(path)
                    }
                });
            })
            .ok();
    }

    /// Asks for what the page on top is missing, applies what arrived. True when the pages should
    /// get new answers.
    pub fn tick(&mut self, m: &mut Marketplace, cx: &mut Cx, now: f64) -> bool {
        if !self.live {
            return false;
        }
        let mut changed = false;
        let ep = crate::data::keep::epoch();
        if ep != self.epoch {
            self.epoch = ep;
            self.refresh_now();
        }
        // The cached copy first (instant::read's seed), then the network.
        for p in m.missing() {
            if self.failed.contains(&p) || self.flight.contains(&p) {
                continue;
            }
            if let Some(v) = api::cached::<Value>(crate::screens::wallet::live::market_net(), &p) {
                self.a.set(p.clone(), v);
                changed = true;
            }
            self.fetch(p, false);
        }
        if now >= self.next_refresh {
            if self.next_refresh > 0.0 {
                self.failed.clear();
                for p in m.top().wants() {
                    self.fetch(p, true);
                }
                // The account can change in Rare Wallet.
                let acct = market_account();
                if acct != self.a.account {
                    self.a.account = acct;
                    changed = true;
                }
            }
            self.next_refresh = now + REFRESH_S;
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Answer(p, v) => {
                    self.flight.remove(&p);
                    self.a.unfail(&p);
                    if self.a.get::<Value>(&p).as_ref() != Some(&v) {
                        self.a.set(p, v);
                        changed = true;
                    }
                }
                Msg::Failed(p) => {
                    self.flight.remove(&p);
                    if !self.a.has(&p) {
                        self.a.fail(p.clone());
                        changed = true;
                    }
                    self.failed.insert(p);
                }
                Msg::Buy(id, phase) => {
                    if matches!(phase, Phase::Done(_)) {
                        // The item changed hands: read it again.
                        self.fetch(api::paths::item(&id), true);
                    }
                    m.buy_phase(cx, phase, now);
                }
            }
        }
        if changed {
            self.a.now_ms = wall_ms();
            m.set_answers(cx, self.answers(), now);
        }
        changed
    }

    /// `market:buy:<id>`: builds the purchase of the item's listing and hands it to Rare Wallet.
    pub fn buy(&mut self, id: &str) {
        let Some(acct) = self.a.account.clone().filter(|a| a.signer) else {
            let _ = self.tx.send(Msg::Buy(id.to_string(), Phase::Error(crate::sign::NO_SIGNER.into())));
            return;
        };
        let Some(d) = self.a.get::<ItemDetail>(&api::paths::item(id)) else {
            let _ = self.tx.send(Msg::Buy(id.to_string(), Phase::Error("This item is still loading. Try again in a moment.".into())));
            return;
        };
        let Some(listing) = d.card.listing.clone() else {
            let _ = self.tx.send(Msg::Buy(id.to_string(), Phase::Error("This item is not listed anymore.".into())));
            return;
        };
        let (tx, id) = (self.tx.clone(), id.to_string());
        std::thread::Builder::new()
            .name("rare-market-buy".into())
            .spawn(move || {
                let n = net();
                let built = rare_market::buy::build_buy_transaction(&*n, &Default::default(), &d.card, &listing, &acct.address, None, None);
                let tx_ = match built {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = tx.send(Msg::Buy(id, Phase::Error(if e.0.is_empty() { "This item can't be bought here.".into() } else { e.0 })));
                        return;
                    }
                };
                let _ = tx.send(Msg::Buy(id.clone(), Phase::Signing));
                let title = format!("Buy {}", d.card.name);
                let phase = match crate::sign::sign_and_execute(tx_, &acct.address, &title) {
                    Ok(digest) => Phase::Done(digest),
                    Err(e) if e == rare_market::inventory::notice::SIGN_REJECTED => Phase::Idle,
                    Err(e) => Phase::Error(e),
                };
                let _ = tx.send(Msg::Buy(id, phase));
            })
            .ok();
    }

    /// Re-reads the page on top on the next tick (after a purchase).
    pub fn refresh_now(&mut self) {
        self.next_refresh = 0.01;
    }

    /// Something is in flight (the shell keeps frames coming).
    pub fn busy(&self) -> bool {
        !self.flight.is_empty()
    }
}
