//! The ported launcher/src/inventory/inventory.test.ts.

use super::notice::{accept_from_notice, AcceptResult, NoticeHost, OfferNotice};
use super::store::InventoryStore;
use super::trade::{Quote, TradeApi};
use super::*;
use crate::api::{CollectionRef, Offer, Staked};
use crate::net::{MockNet, NetError};
use serde_json::json;
use std::cell::RefCell;
use std::sync::Mutex;

const HERO: &str = "0xa1::hero::Hero";
const POP: &str = "0xb2::popkin::Popkin";
const MISC: &str = "0xd4::art::Piece";

fn game(slug: &str, t: &str) -> SuiGame {
    SuiGame { slug: slug.into(), name: slug.to_uppercase(), logo_url: String::new(), collections: vec![GameCollection { nft_type: t.into(), name: slug.into() }] }
}
fn games() -> Vec<SuiGame> {
    vec![game("pawtato", HERO), game("popkins", POP)]
}
fn mk(id: &str, t: &str, floor: Option<&str>) -> InvItem {
    InvItem { id: id.into(), name: id.into(), kind: t.into(), floor_mist: floor.map(String::from), ..Default::default() }
}
fn none() -> HashMap<String, Option<String>> {
    HashMap::new()
}
fn grouped(items: Vec<InvItem>) -> serde_json::Value {
    serde_json::to_value(group_items(items, &games(), &none())).unwrap()
}

// ---------- home summary ----------

#[test]
fn summary_totals_game_items_ranks_counts_offers() {
    let mut i3 = mk("3", HERO, Some("7"));
    i3.top_offer_mist = Some("9".into());
    let mut i4 = mk("4", HERO, Some("3"));
    i4.top_offer_mist = Some("0".into());
    let inv = normalize_inventory(Some(&grouped(vec![mk("1", POP, Some("5")), mk("2", MISC, Some("100")), i3, i4])));
    let sum = summarize(&inv);
    assert_eq!(sum.items, 3);
    assert_eq!(sum.value_mist, "15");
    assert_eq!(sum.offers, 1);
    assert_eq!(sum.games.iter().map(|g| g.slug.as_str()).collect::<Vec<_>>(), ["pawtato", "popkins"]);
}

#[test]
fn summary_empty() {
    assert_eq!(summarize(&Inventory::default()), InvSummary { items: 0, value_mist: "0".into(), offers: 0, games: vec![] });
}

// ---------- grouping ----------

#[test]
fn groups_by_game_in_catalog_order_rest_last() {
    let g = group_items(vec![mk("1", POP, Some("5")), mk("2", MISC, None), mk("3", "0xA1::hero::Hero", Some("7")), mk("4", HERO, Some("3"))], &games(), &none());
    assert_eq!(g.iter().map(|x| x.game.as_ref().map(|g| g.slug.as_str())).collect::<Vec<_>>(), [Some("pawtato"), Some("popkins"), None]);
    assert_eq!(g[0].items.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["3", "4"]);
    assert_eq!(g[0].floor_value_mist, "10");
    assert_eq!(g[2].floor_value_mist, "0");
}

#[test]
fn fills_floors_from_collection_floors() {
    let mut f = HashMap::new();
    f.insert(crate::types::canon_type(HERO), Some("9".to_string()));
    let g = group_items(vec![mk("1", HERO, None)], &games(), &f);
    assert_eq!(g[0].items[0].floor_mist.as_deref(), Some("9"));
}

#[test]
fn normalizes_the_api_answer_drops_empty_groups() {
    let inv = normalize_inventory(Some(&json!([{"game": null, "items": [mk("1", MISC, Some("2"))], "floorValueMist": "x"}, {"game": null, "items": []}])));
    assert_eq!(inv.total, 1);
    assert_eq!(inv.groups[0].floor_value_mist, "2");
    assert_eq!(normalize_inventory(None), Inventory::default());
}

fn base() -> Inventory {
    Inventory { groups: group_items(vec![mk("1", HERO, Some("4")), mk("2", HERO, Some("4")), mk("3", POP, Some("50"))], &games(), &none()), total: 3 }
}
fn remove(ids: &[&str], why: Why) -> InvOp {
    InvOp::Remove { ids: ids.iter().map(|s| s.to_string()).collect(), why }
}

// ---------- optimistic operations ----------

#[test]
fn removes_sold_items_and_empty_groups() {
    let a = apply_op(&base(), &remove(&["3"], Why::Sold));
    assert_eq!(a.total, 2);
    assert_eq!(a.groups.iter().map(|g| g.game.as_ref().unwrap().slug.as_str()).collect::<Vec<_>>(), ["pawtato"]);
    assert_eq!(apply_op(&a, &remove(&["1"], Why::Sent)).groups[0].floor_value_mist, "4");
}

#[test]
fn lists_and_delists() {
    let l = apply_op(&base(), &InvOp::List { id: "1".into(), price_mist: "6".into() });
    assert_eq!(l.groups[0].items[0].listing.as_ref().unwrap().price_mist, "6");
    assert!(apply_op(&l, &InvOp::Delist { id: "1".into() }).groups[0].items[0].listing.is_none());
}

#[test]
fn knows_when_a_fetch_reflects_an_operation() {
    let b = base();
    let r = remove(&["1"], Why::Sold);
    assert!(!reflects(&b, &r));
    assert!(reflects(&apply_op(&b, &r), &r));
    let l = InvOp::List { id: "1".into(), price_mist: "6".into() };
    assert!(!reflects(&b, &l));
    assert!(reflects(&apply_op(&b, &l), &l));
    assert!(reflects(&b, &InvOp::Delist { id: "1".into() }));
}

#[test]
fn settles_caught_up_and_stale_operations() {
    let p = |key: &str, op: InvOp, at: i64| Pending { key: key.into(), op, at_ms: at };
    let fetched = apply_op(&base(), &remove(&["1"], Why::Sold));
    let left = settle(&[p("a", remove(&["1"], Why::Sold), 1000), p("b", remove(&["2"], Why::Sent), 1000), p("c", InvOp::List { id: "3".into(), price_mist: "9".into() }, 1000)], &fetched, 2000, 90_000);
    assert_eq!(left.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(), ["b", "c"]);
    assert!(settle(&left, &fetched, 200_000, 90_000).is_empty());
    assert_eq!(apply_pending(&base(), &left).total, 2);
}

// ---------- store: optimistic update and rollback ----------

fn store_with(inv: Inventory) -> InventoryStore {
    let mut s = InventoryStore::new();
    s.address = Some("0xme".into());
    s.base = Some(inv);
    s
}

#[test]
fn store_shows_change_at_once_keeps_on_success_rolls_back_on_failure() {
    let mut s = store_with(base());
    let mut during = 0;
    s.run_optimistic(remove(&["1"], Why::Sold), 0, |s| {
        during = s.current_view().total;
        Ok(())
    })
    .unwrap();
    assert_eq!(during, 2);
    assert_eq!(s.current_view().total, 2);
    s.stop_follow_ups();
    let e = s
        .run_optimistic(remove(&["2"], Why::Sold), 0, |s| {
            assert_eq!(s.current_view().total, 1);
            Err::<(), _>(crate::Error::new("rejected"))
        })
        .unwrap_err();
    assert_eq!(e.0, "rejected");
    assert_eq!(s.current_view().total, 2);
    assert_eq!(s.pending.len(), 1);
}

#[test]
fn a_fetch_that_shows_the_sale_ends_the_pending_operation() {
    let mut s = store_with(base());
    s.run_optimistic(remove(&["1"], Why::Sold), 0, |_| Ok(())).unwrap();
    s.stop_follow_ups();
    let after = apply_op(&base(), &remove(&["1"], Why::Sold));
    let n = MockNet::new();
    n.on_read_json("/v1/accounts/0xme/inventory", json!({"data": after.groups, "total": after.total}));
    s.refresh(&n, Some("0xme"), |a| load_inventory(&n, a, || Ok(games())), crate::time::now_ms());
    assert!(s.pending.is_empty());
    assert_eq!(s.current_view().total, 2);
    // The answer is kept for the next start.
    assert!(n.peek("ui:inv:0xme").is_some());
}

#[test]
fn a_sold_notice_triggers_a_refetch() {
    let mut s = InventoryStore::new();
    s.start_sync([]);
    s.address = Some("0xme".into());
    s.on_notices([("9", "sold")], 0);
    assert!(!s.due(1000));
    assert!(s.due(1500));
    assert!(!s.due(1600));
    // The backlog is never acted on.
    let mut t = InventoryStore::new();
    t.address = Some("0xme".into());
    t.start_sync(["9"]);
    t.on_notices([("9", "sold")], 0);
    assert!(!t.due(5000));
}

#[test]
fn follow_ups_only_while_something_is_pending() {
    let mut s = store_with(base());
    s.run_optimistic(remove(&["1"], Why::Sold), 0, |_| Ok(())).unwrap();
    let wake = s.next_wake().unwrap();
    assert!(s.due(wake));
    s.pending.clear();
    assert!(!s.due(wake + 60_000));
    assert!(s.next_wake().is_none());
}

#[test]
fn a_newer_fetch_wins() {
    let n = MockNet::new();
    let mut s = store_with(base());
    let t1 = s.start_refresh(&n, None).unwrap();
    let t2 = s.start_refresh(&n, None).unwrap();
    s.finish_refresh(&n, &t1, Ok(Inventory::default()), 0);
    assert!(s.loading);
    assert_eq!(s.current_view().total, 3);
    s.finish_refresh(&n, &t2, Err(crate::Error::new("rare.market 502")), 0);
    assert!(s.error && !s.loading);
    assert_eq!(s.current_view().total, 3);
}

// ---------- accept an offer from its notification ----------

struct Host {
    toasts: RefCell<Vec<String>>,
    tab: RefCell<bool>,
}
impl NoticeHost for Host {
    fn signing_account(&self) -> Option<String> {
        Some("0xme".into())
    }
    fn toast(&self, t: &str) {
        self.toasts.borrow_mut().push(t.into());
    }
    fn show_wallet_tab(&self) {
        *self.tab.borrow_mut() = true;
    }
}

struct FakeTrade {
    quote: Option<Quote>,
    sell_err: Option<String>,
    sold: Mutex<bool>,
}
impl TradeApi for FakeTrade {
    fn quote(&self, _: &InvItem, _: &str) -> crate::Result<Option<Quote>> {
        Ok(self.quote.clone())
    }
    fn sell(&self, _: &InvItem, _: &Offer, _: &str) -> crate::Result<()> {
        *self.sold.lock().unwrap() = true;
        match &self.sell_err {
            Some(e) => Err(crate::Error::new(e.clone())),
            None => Ok(()),
        }
    }
    fn list(&self, _: &InvItem, _: u64, _: &str) -> crate::Result<()> {
        Ok(())
    }
    fn cancel(&self, _: &InvItem, _: &str) -> crate::Result<()> {
        Ok(())
    }
}

fn quote(price: &str) -> Option<Quote> {
    Some(Quote { offer: Offer { price_mist: price.into(), ..Default::default() }, net_mist: price.parse().unwrap(), blocked: None, sponsored: false })
}

fn setup(q: Option<Quote>, sell_err: Option<&str>) -> (InventoryStore, MockNet, FakeTrade, Host) {
    let n = MockNet::new();
    n.on_read_json("/v1/items/0x1", json!({"id": "0x1", "name": "Hero #1", "type": HERO}));
    let s = store_with(Inventory { groups: group_items(vec![mk("0x1", HERO, Some("4"))], &games(), &none()), total: 1 });
    (s, n, FakeTrade { quote: q, sell_err: sell_err.map(String::from), sold: Mutex::new(false) }, Host { toasts: RefCell::new(vec![]), tab: RefCell::new(false) })
}

fn notice(raw: &str) -> OfferNotice {
    OfferNotice { kind: "offer_received".into(), item_id: Some("0x1".into()), amount_raw: Some(raw.into()) }
}

#[test]
fn sells_into_the_announced_offer_and_removes_the_item() {
    let (mut s, n, t, h) = setup(quote("64000000000"), None);
    assert_eq!(accept_from_notice(&notice("64000000000"), &n, &t, &mut s, &h, 0), AcceptResult::Sold);
    assert!(*t.sold.lock().unwrap());
    assert_eq!(s.current_view().total, 0);
    assert!(h.toasts.borrow().iter().any(|m| m.contains("Sold Hero #1 for 64")));
}

#[test]
fn signs_nothing_when_a_better_offer_is_open() {
    let (mut s, n, t, h) = setup(quote("70000000000"), None);
    assert_eq!(accept_from_notice(&notice("64000000000"), &n, &t, &mut s, &h, 0), AcceptResult::Sheet);
    assert!(!*t.sold.lock().unwrap());
    assert_eq!(s.sheet_id.as_deref(), Some("0x1"));
    assert!(*h.tab.borrow());
}

#[test]
fn rolls_the_item_back_when_the_sale_fails() {
    let (mut s, n, t, h) = setup(quote("64000000000"), Some("The network rejected this transaction."));
    assert_eq!(accept_from_notice(&notice("64000000000"), &n, &t, &mut s, &h, 0), AcceptResult::Failed);
    assert_eq!(s.current_view().total, 1);
    assert_eq!(h.toasts.borrow().last().unwrap(), "The network rejected this transaction.");
    let (mut s, n, t, h) = setup(quote("64000000000"), Some(super::notice::SIGN_REJECTED));
    assert_eq!(accept_from_notice(&notice("64000000000"), &n, &t, &mut s, &h, 0), AcceptResult::Cancelled);
    assert!(h.toasts.borrow().is_empty());
}

// ---------- staked items ----------

fn stake() -> Staked {
    Staked { protocol: Some("0x38f0::staking_contract".into()), name: Some("staking_contract".into()), via: "staked".into() }
}
fn staked() -> InvItem {
    let mut i = mk("s1", POP, Some("50"));
    i.staked = Some(stake());
    i.collection = Some(CollectionRef { slug: "popkins".into(), name: "Popkins".into(), image_url: None, verified: true });
    i
}

#[test]
fn staked_items_counted_and_valued() {
    let inv = normalize_inventory(Some(&grouped(vec![mk("1", POP, Some("50")), staked()])));
    assert_eq!(inv.total, 2);
    assert_eq!(inv.groups[0].floor_value_mist, "100");
    assert_eq!(inv.groups[0].items[1].staked, Some(stake()));
    let s = summarize(&inv);
    assert_eq!((s.items, s.value_mist.as_str()), (2, "100"));
    assert!(is_staked(&inv.groups[0].items[1]));
    assert!(!is_staked(&inv.groups[0].items[0]));
}

#[test]
fn says_where_to_unstake() {
    assert_eq!(unstake_line(&staked()), "Unstake on Popkins to list or send");
    assert_eq!(unstake_line(&mk("x", POP, None)), "Unstake on its project to list or send");
}

#[test]
fn never_lists_a_staked_item() {
    let inv = Inventory { groups: group_items(vec![staked()], &games(), &none()), total: 1 };
    assert!(apply_op(&inv, &InvOp::List { id: "s1".into(), price_mist: "9".into() }).groups[0].items[0].listing.is_none());
}

#[test]
fn fallback_items_path_carries_staked_items() {
    let n = MockNet::new();
    n.fail_read("/v1/accounts/0xabc/inventory", NetError::Status("rare.market 404".into()));
    n.on_read_json("/v1/accounts/0xabc/items?limit=100", json!({"data": [staked()], "nextCursor": null}));
    n.on_read_json(&crate::api::paths::collection(&crate::types::canon_type(POP)), json!({"stats": {"floorMist": "50"}}));
    let inv = load_inventory(&n, "0xabc", || Ok(games())).unwrap();
    assert_eq!(inv.total, 1);
    assert_eq!(inv.groups[0].items[0].staked, Some(stake()));
}

#[test]
fn mock_fixtures_trade_end_to_end() {
    let db = mock::Db::default();
    let n = db.net();
    let inv = load_inventory(&n, mock::OWNER, || Ok(vec![])).unwrap();
    assert_eq!(inv.total, 8);
    assert_eq!(summarize(&inv).games.len(), 3);
    let t = mock::MockTrade(db.clone());
    let pop = db.all().into_iter().find(|i| i.name == "Popkins #412").unwrap();
    let q = t.quote(&pop, mock::OWNER).unwrap().unwrap();
    assert_eq!(q.net_mist, 64_000_000_000);
    t.sell(&pop, &q.offer, mock::OWNER).unwrap();
    assert_eq!(db.all().len(), 7);
}
