//! The item sheet's actions (ItemSheet.tsx) and Send collectibles (SendNfts.tsx): the best offer's
//! quote, sell into it, list at a price (the keyboard), cancel a listing (rare-market's builders,
//! signed through Rare Wallet's executor: Rare covers the fee when the wallet is short), and sending
//! items (rare-wallet's sendability check, batches that fit Sui's limits, one review each).

use crate::screens::inventory::sheet as inv_sheet;
use crate::screens::wallet::flows::send::{recipient_of, typed_state};
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::pages::send_nfts as page;
use crate::screens::wallet::service::{self, poll};
use crate::screens::wallet::sign::recipient::{Recent, RecipientState};
use crate::screens::wallet::types::ItemCard;
use crate::state::{State, Tone};
use rare_market::inventory::trade::{Quote, RealTrade, TradeApi};
use rare_market::inventory::InvItem;
use rare_wallet::nft::SendItem;
use rare_wallet::sign::{SignError, TxInput};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

const ASK_PRICE: &str = "wallet:item:price";
const ASK_TO: &str = "wallet:send-nfts:to";

type Rx<T> = Option<Receiver<Result<T, String>>>;

#[derive(Default)]
pub struct Flow {
    /// the best offer for the item on the sheet: (item id, None while looking, Some(None) when none)
    quote: Option<(String, Option<Option<Quote>>)>,
    rx_quote: Rx<(String, Option<Quote>)>,
    rx_act: Rx<(String, Result<(), Option<String>>)>,
    // Send collectibles
    key: u64,
    st: Option<page::State>,
    rs: Option<RecipientState>,
    recents: Vec<Recent>,
    cards: Option<Vec<ItemCard>>,
    can: Option<BTreeMap<String, page::Sendability>>,
    send: HashMap<String, SendItem>,
    rx_can: Rx<Result<HashMap<String, rare_wallet::nft::Sendability>, String>>,
    rx_resolve: Rx<(String, rare_wallet::recipient::RecipientState)>,
    rx_send: Rx<Result<usize, Option<String>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Page(page::Msg),
}

fn trade() -> RealTrade {
    let w = service::wallet();
    RealTrade {
        net: Arc::new(crate::screens::wallet::live::market_net().clone()),
        cfg: Default::default(),
        chain: w.map(|w| Arc::new(rare_market::chain::ChainKiosks { chain: Arc::new(w.sui.clone()), rt: w.rt.clone() }) as Arc<dyn rare_market::venues::KioskReader>),
        exec: service::executor(),
    }
}

/// The trade's answer as the sheet reads it: Ok, declined (None), or the message.
fn outcome(r: rare_market::Result<()>) -> Result<(), Option<String>> {
    match r {
        Ok(()) => Ok(()),
        Err(e) if e.0 == rare_market::inventory::notice::SIGN_REJECTED => Err(None),
        Err(e) => Err(Some(e.0)),
    }
}

/// Plain words for a failed send: the network's refusal, or nothing when the person declined.
fn send_error(e: SignError) -> Option<String> {
    match e {
        SignError::Rejected(_) => None,
        e => Some(e.to_string()),
    }
}

impl WalletTab {
    fn signer(&self) -> Option<String> {
        self.current().filter(|a| a.kind != "watch").map(|a| a.address.clone())
    }

    /// The quote the item sheet shows for `id`.
    pub(in crate::screens::wallet) fn item_quote(&self, id: &str) -> Option<Option<&Quote>> {
        match &self.flows.items.quote {
            Some((q_id, q)) if q_id == id => q.as_ref().map(|o| o.as_ref()),
            _ if self.fixture => Some(None),
            _ => None,
        }
    }

    /// Opens the sheet's quote for `id` (asked once per item).
    fn items_ensure_quote(&mut self, id: &str) {
        if self.fixture || self.flows.items.quote.as_ref().is_some_and(|(q, _)| q == id) {
            return;
        }
        let Some(sender) = self.signer() else {
            self.flows.items.quote = Some((id.to_string(), Some(None)));
            return;
        };
        let Some(item) = self.find_item(id).cloned() else { return };
        self.flows.items.quote = Some((id.to_string(), None));
        let id = id.to_string();
        self.flows.items.rx_quote = Some(service::work(move |_| {
            let q = trade().quote(&item, &sender).unwrap_or_else(|e| {
                log::warn!("item quote: {e}");
                None
            });
            (id, q)
        }));
    }

    /// An offer notice opened this sheet: once the best offer is known, sell into it when it is
    /// still the announced one (notice.ts acceptFromNotice); otherwise say why and leave the sheet.
    fn items_accept_pending(&mut self, st: &mut State) {
        use crate::screens::wallet::page::{has_accept, pending_accept};
        let Some(id) = self.item.as_ref().map(|(i, _)| i.clone()) else { return };
        if !has_accept(&id) {
            return;
        }
        if self.fixture {
            pending_accept(&id);
            return;
        }
        let now = rare_render::clock::secs();
        let Some(q) = self.item_quote(&id).map(|q| q.cloned()) else { return };
        let Some(announced) = pending_accept(&id) else { return };
        if self.signer().is_none() {
            st.toast("Add an account that can sign to accept offers", Tone::Info, now);
            return;
        }
        let Some(q) = q else {
            st.toast("That offer is no longer open", Tone::Info, now);
            return;
        };
        let differs = announced.as_deref().and_then(|a| a.trim().parse::<i128>().ok()).is_some_and(|a| a != 0 && q.offer.price_mist.trim().parse::<i128>().ok() != Some(a));
        if let Some(why) = &q.blocked {
            st.toast(why.clone(), Tone::Info, now);
        } else if differs {
            st.toast("A better offer is open now. Check it before selling", Tone::Info, now);
        } else {
            self.items_sheet(st, inv_sheet::Msg::Sell);
        }
        self.ver += 1;
    }

    /// A message from the item sheet over home.
    pub(in crate::screens::wallet) fn items_sheet(&mut self, st: &mut State, m: inv_sheet::Msg) {
        let Some((id, mut ist)) = self.item.take() else { return };
        let can_sign = self.signer().is_some();
        let eff = {
            let item = self.find_item(&id);
            let sp = inv_sheet::Props { item: Some(item), quote: self.item_quote(&id), can_sign, signed_out: self.accounts.is_empty() };
            inv_sheet::update(&sp, &mut ist, m)
        };
        let item: Option<InvItem> = self.find_item(&id).cloned();
        let sender = self.signer();
        let quote = self.item_quote(&id).flatten().cloned();
        match eff {
            Some(inv_sheet::Effect::Close) => return,
            Some(inv_sheet::Effect::Send(i)) => {
                self.push(st, WalletView::SendNfts { ids: vec![i], to: None });
                return;
            }
            Some(inv_sheet::Effect::AddPasskey) => {
                self.push(st, WalletView::Add);
                return;
            }
            Some(inv_sheet::Effect::AskPrice(cur)) => {
                let start = if cur > 0 { crate::screens::wallet::fmt::fmt_sui(Some(cur), 9, false, true).replace(',', "") } else { String::new() };
                st.ask_text(ASK_PRICE, "Price in SUI", start, Some("e.g. 12.5"));
            }
            Some(inv_sheet::Effect::Sell { done }) => {
                if let (Some(item), Some(sender), Some(q)) = (item, sender, quote) {
                    self.flows.items.rx_act = Some(service::work(move |_| (done, outcome(trade().sell(&item, &q.offer, &sender)))));
                }
            }
            Some(inv_sheet::Effect::List { price, done }) => {
                if let (Some(item), Some(sender)) = (item, sender) {
                    let p = price.max(0) as u64;
                    self.flows.items.rx_act = Some(service::work(move |_| (done, outcome(trade().list(&item, p, &sender)))));
                }
            }
            Some(inv_sheet::Effect::Cancel { done }) => {
                if let (Some(item), Some(sender)) = (item, sender) {
                    self.flows.items.rx_act = Some(service::work(move |_| (done, outcome(trade().cancel(&item, &sender)))));
                }
            }
            None => {}
        }
        self.item = Some((id, ist));
    }

    // ---------------------------------------------------------------- Send collectibles

    fn nft_props<'a>(&'a self, ids: &'a [String], rs: &'a RecipientState) -> page::Props<'a> {
        let cur = self.current();
        let f = &self.flows.items;
        page::Props {
            account: cur,
            accounts: &self.accounts,
            can_sign: cur.is_some_and(|a| a.kind != "watch"),
            ids,
            cards: f.cards.as_deref(),
            can: f.can.as_ref(),
            rs,
            recents: &f.recents,
            now: self.now,
        }
    }

    pub(in crate::screens::wallet) fn items_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let WalletView::SendNfts { ids, .. } = v else { return None };
        let st = self.flows.items.st.as_ref()?;
        let rs = self.flows.items.rs.clone().unwrap_or(RecipientState::Empty);
        page::view(&self.nft_props(ids, &rs), st).map(|m| m.map(&Msg::Page))
    }

    pub(in crate::screens::wallet) fn items_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::SendNfts { .. })
    }

    fn nft_resolve(&mut self, text: String) {
        let f = &mut self.flows.items;
        f.rs = Some(if text.trim().is_empty() { RecipientState::Empty } else { typed_state(&text) });
        if text.trim().is_empty() || self.fixture {
            f.rx_resolve = None;
            return;
        }
        f.rx_resolve = Some(service::work(move |w| {
            let s = w.rt.block_on(rare_wallet::recipient::resolve_recipient(&w.names(), &text));
            (text, s)
        }));
    }

    pub(in crate::screens::wallet) fn items_handle(&mut self, st: &mut State, m: Msg) {
        let Msg::Page(m) = m;
        let WalletView::SendNfts { ids, .. } = self.stack.top().view.clone() else { return };
        let Some(mut ps) = self.flows.items.st.take() else { return };
        let rs = self.flows.items.rs.clone().unwrap_or(RecipientState::Empty);
        let eff = page::update(&self.nft_props(&ids, &rs), &mut ps, m);
        self.flows.items.st = Some(ps);
        match eff {
            Some(page::Effect::AskTo(v)) => st.ask_text(ASK_TO, "Send to", v, Some("0x address or name.sui")),
            Some(page::Effect::Resolve(t)) => self.nft_resolve(t),
            Some(page::Effect::Send { to, ids, .. }) => {
                let Some(sender) = self.signer() else { return };
                let items: Vec<SendItem> = ids.iter().filter_map(|i| self.flows.items.send.get(i).cloned()).collect();
                let names: HashMap<String, String> = self.flows.items.cards.iter().flatten().map(|c| (c.id.clone(), c.name.clone())).collect();
                self.flows.items.rx_send = Some(service::work(move |_| {
                    let p = rare_wallet::nft::NftSendParams { sender: sender.clone(), to, items };
                    let batches = rare_wallet::nft::plan_nft_send(&p).map_err(|e| Some(e.to_string()))?;
                    let n = batches.len();
                    for (i, b) in batches.into_iter().enumerate() {
                        let bn: Vec<String> = b.iter().map(|s| names.get(&s.item.nft_id).cloned().unwrap_or_else(|| "item".into())).collect();
                        let tx = rare_wallet::nft::build_nft_send(&rare_wallet::nft::NftSendParams { items: b, ..p.clone() }).map_err(|e| Some(e.to_string()))?;
                        let r = service::send(TxInput::Ptb(tx), &sender, &page::batch_title(&bn, i, n)).map_err(send_error)?;
                        if r.status.as_deref() == Some("failure") {
                            return Err(Some("The network rejected this transaction.".into()));
                        }
                    }
                    Ok(n)
                }));
            }
            None => {}
        }
    }

    pub(in crate::screens::wallet) fn items_tick(&mut self, st: &mut State) -> bool {
        let now = rare_render::clock::secs();
        // the item sheet
        if let Some(id) = self.item.as_ref().map(|(i, _)| i.clone()) {
            self.items_ensure_quote(&id);
        }
        if let Some(r) = poll(&mut self.flows.items.rx_quote) {
            if let Ok((id, q)) = r {
                if self.flows.items.quote.as_ref().is_some_and(|(i, _)| *i == id) {
                    self.flows.items.quote = Some((id, Some(q)));
                }
            }
            self.ver += 1;
        }
        self.items_accept_pending(st);
        if let Some(v) = st.take_answer(ASK_PRICE) {
            if let Some(m) = v.as_deref().and_then(crate::screens::overlay::panels::money::parse_sui_input) {
                self.items_sheet(st, inv_sheet::Msg::Price(m));
                self.items_sheet(st, inv_sheet::Msg::List);
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.items.rx_act) {
            let (done, res) = r.unwrap_or_else(|e| (String::new(), Err(Some(e))));
            let ok = res.is_ok();
            if let Some((_, ist)) = self.item.as_mut() {
                inv_sheet::finished(ist, &done, res);
            }
            if ok {
                st.toast(done, Tone::Info, now);
                self.item = None;
                self.flows.items.quote = None;
                // Balances, activity and items refresh now and again shortly (keep::nudge).
                crate::data::keep::nudge();
            }
            self.ver += 1;
        }

        // Send collectibles
        let top = self.stack.top().clone();
        let WalletView::SendNfts { ids, to } = &top.view else {
            if self.flows.items.st.is_some() {
                let quote = self.flows.items.quote.take();
                self.flows.items = Flow { quote, ..Flow::default() };
            }
            return self.flows.items.rx_quote.is_some() || self.flows.items.rx_act.is_some();
        };
        if self.flows.items.key != top.key || self.flows.items.st.is_none() {
            let cards: Vec<ItemCard> = ids.iter().filter_map(|i| self.find_item(i)).map(crate::screens::wallet::market::card_from).collect();
            let refs: Vec<rare_wallet::nft::NftRef> = ids.iter().filter_map(|i| self.find_item(i)).map(|c| rare_wallet::nft::NftRef { object_id: c.id.clone(), type_: c.kind.clone(), kiosk: c.kiosk_id.is_some() }).collect();
            let owner = self.current().map(|a| a.address.clone());
            let f = &mut self.flows.items;
            f.key = top.key;
            f.st = Some(page::State { to: to.clone().unwrap_or_default(), ..Default::default() });
            f.cards = Some(cards);
            f.can = None;
            f.send.clear();
            f.rs = None;
            f.recents = Vec::new();
            if self.fixture {
                f.can = Some(ids.iter().map(|i| (i.clone(), page::Sendability::Ok)).collect());
            } else if let Some(owner) = owner {
                f.rx_can = Some(service::work(move |w| {
                    let _g = w.rt.enter();
                    let http = rare_sui::prices::ReqwestGet::default();
                    w.rt.block_on(async { rare_wallet::nft::nft_sendability(&w.sui, &http, &owner, &refs).await.map_err(|e| e.to_string()) })
                }));
            }
            if let Some(t) = to.clone() {
                self.nft_resolve(t);
            }
            self.ver += 1;
        }
        if let Some(v) = st.take_answer(ASK_TO) {
            if let Some(v) = v {
                self.items_handle(st, Msg::Page(page::Msg::To(v)));
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.items.rx_can) {
            let f = &mut self.flows.items;
            let mut can = BTreeMap::new();
            match r {
                Ok(Ok(m)) => {
                    for (id, s) in m {
                        match s {
                            rare_wallet::nft::Sendability::Ok(item) => {
                                f.send.insert(id.clone(), item);
                                can.insert(id, page::Sendability::Ok);
                            }
                            rare_wallet::nft::Sendability::No(why) => {
                                can.insert(id, page::Sendability::Blocked(why));
                            }
                        }
                    }
                }
                Ok(Err(e)) | Err(e) => {
                    log::warn!("nft sendability: {e}");
                    for c in f.cards.iter().flatten() {
                        can.insert(c.id.clone(), page::Sendability::Blocked("Couldn't check this item. Try again.".into()));
                    }
                }
            }
            f.can = Some(can);
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.items.rx_resolve) {
            let f = &mut self.flows.items;
            match r {
                Ok((text, s)) if f.st.as_ref().is_some_and(|ps| ps.to == text) => f.rs = Some(recipient_of(s)),
                Ok(_) => {}
                Err(e) => f.rs = Some(RecipientState::Error(e)),
            }
            self.ver += 1;
        }
        if let Some(r) = poll(&mut self.flows.items.rx_send) {
            match r.unwrap_or_else(|e| Err(Some(e))) {
                Ok(n) => {
                    st.toast(if n > 1 { "Collectibles sent" } else { "Sent" }, Tone::Info, now);
                    self.stack.reset(Vec::new());
                    // Balances, activity and items refresh now and again shortly (keep::nudge).
                    crate::data::keep::nudge();
                    self.flows.items = Flow::default();
                    return false;
                }
                Err(e) => {
                    if let Some(ps) = self.flows.items.st.as_mut() {
                        ps.busy = false;
                        ps.error = e;
                    }
                }
            }
            self.ver += 1;
        }
        let f = &self.flows.items;
        f.rx_quote.is_some() || f.rx_act.is_some() || f.rx_can.is_some() || f.rx_resolve.is_some() || f.rx_send.is_some()
    }
}
