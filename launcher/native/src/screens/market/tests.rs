//! The Marketplace's pad behaviour, headless (no GPU): the page stack (Marketplace.tsx, nav.ts),
//! the collection's tabs and filters, the buy sheet, and the Market tab's areas (Market.tsx,
//! market/nav.test.ts's section steps across both areas).

use super::data;
use super::host::{Cx, Out};
use super::shell::Marketplace;
use super::tab::MarketTab;
use super::{collection, home, item, search};
use rare_market::nav::{Area, View};
use rare_render::input::Pad;
use rare_render::theme::rem_for;
use rare_render::{ImageLoader, Scene, TextSys};
use std::rc::Rc;
use std::sync::Arc;

static FONT: &[u8] = include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf");

struct Rig {
    scene: Scene,
    text: TextSys,
    img: ImageLoader,
}

impl Rig {
    fn new() -> Rig {
        let (w, h) = (1920.0, 1200.0);
        Rig { scene: Scene::new(w, h, rem_for(w, h)), text: TextSys::new(&[FONT], false, 1024), img: ImageLoader::new(1, Arc::new(|| {}), None) }
    }
    fn cx(&mut self) -> Cx<'_> {
        Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.img }
    }
}

fn market(rig: &mut Rig, a: data::Answers, views: Vec<View>) -> Marketplace {
    let mut cx = rig.cx();
    let main = crate::screens::trade::main_area(&mut cx);
    Marketplace::new(&mut cx, main, Rc::new(a), views, 0.0)
}

#[test]
fn back_returns_to_the_card_that_opened_the_page() {
    let mut rig = Rig::new();
    let mut m = market(&mut rig, data::home(), vec![]);
    assert_eq!(m.top().host().tag(), Some(home::T_TREND), "lands on the first trending card");
    let mut cx = rig.cx();
    assert_eq!(m.press(&mut cx, Pad::Right, 0.0), Out::Moved);
    assert_eq!(m.top().host().tag(), Some(home::T_TREND + 1));
    m.press(&mut cx, Pad::A, 0.0);
    assert_eq!(m.depth(), 2);
    assert!(matches!(m.nav.top().view, View::Collection { .. }));
    assert_eq!(m.press(&mut cx, Pad::B, 0.0), Out::Moved);
    assert_eq!(m.depth(), 1);
    assert_eq!(m.top().host().tag(), Some(home::T_TREND + 1), "focus is back where it was");
    assert_eq!(m.press(&mut cx, Pad::B, 0.0), Out::Close, "B on the front page is the launcher's");
}

#[test]
fn x_steps_the_range_and_y_opens_search() {
    let mut rig = Rig::new();
    let mut m = market(&mut rig, data::home(), vec![]);
    let mut cx = rig.cx();
    assert!(m.top().hints().contains(&("x", "Range 24h".to_string())));
    m.press(&mut cx, Pad::X, 0.0);
    assert!(m.top().hints().contains(&("x", "Range 7d".to_string())));
    m.press(&mut cx, Pad::Y, 0.0);
    assert!(m.top().is_search());
    assert_eq!(m.press(&mut cx, Pad::Y, 0.0), Out::Act(search::ASK.into()), "Y on search edits the term");
    assert_eq!(m.depth(), 2);
}

#[test]
fn collection_tabs_filters_and_list() {
    let mut rig = Rig::new();
    let mut m = market(&mut rig, data::collection(), vec![View::Collection { slug: "popkins".into() }]);
    let mut cx = rig.cx();
    assert_eq!(m.top().host().tag(), Some(collection::T_ITEM), "lands on the first item");
    m.press(&mut cx, Pad::Rt, 0.0);
    assert_eq!(m.top().host().tag(), Some(collection::T_TAB + 1), "focus follows the tab");
    assert!(!m.top().hints().iter().any(|h| h.0 == "x"), "Filters only on Items");
    m.press(&mut cx, Pad::Down, 0.0);
    assert!(m.top().host().tag().map(|t| t >= collection::T_EVENT).unwrap_or(false), "down enters the activity rows");
    m.press(&mut cx, Pad::Lt, 0.0);
    m.press(&mut cx, Pad::X, 0.0);
    assert_eq!(m.top().host().tag(), Some(collection::T_F_SORT), "filters land on the current sort");
    assert_eq!(m.top().hints(), vec![("a", "Select".to_string()), ("b", "Done".to_string())]);
    m.press(&mut cx, Pad::Down, 0.0);
    m.press(&mut cx, Pad::A, 0.0);
    m.press(&mut cx, Pad::B, 0.0);
    assert_eq!(m.depth(), 2, "B closes the filters, not the page");
    assert_eq!(m.top().host().tag(), Some(collection::T_SORT_BTN));
    m.press(&mut cx, Pad::Select, 0.0);
    assert!(m.top().hints().contains(&("select", "Grid view".to_string())));
    m.press(&mut cx, Pad::Select, 0.0);
}

#[test]
fn buy_sheet_opens_on_done_without_a_signer_and_b_closes_it() {
    let mut rig = Rig::new();
    let mut m = market(&mut rig, data::item(), vec![View::Item { id: data::ITEM.into(), card: None, thumb: None }]);
    let mut cx = rig.cx();
    assert_eq!(m.top().host().tag(), Some(item::T_BUY_NOW));
    m.press(&mut cx, Pad::A, 0.0);
    assert_eq!(m.top().host().tag(), Some(super::buy::T_DONE));
    assert_eq!(m.top().hints(), vec![("a", "Select".to_string()), ("b", "Close".to_string())]);
    assert_eq!(m.press(&mut cx, Pad::Y, 0.0), Out::None, "the sheet keeps Y");
    m.press(&mut cx, Pad::B, 0.0);
    assert_eq!(m.depth(), 2);
    assert_eq!(m.top().host().tag(), Some(item::T_BUY_NOW));
    assert_eq!(m.top().host().tag(), Some(item::T_BUY_NOW));
    assert_eq!(m.press(&mut cx, Pad::Right, 0.0), Out::Moved);
    assert_eq!(m.top().host().tag(), Some(item::T_CART), "the middle button is there");
    assert_eq!(m.press(&mut cx, Pad::Right, 0.0), Out::Moved);
    assert!(matches!(m.press(&mut cx, Pad::A, 0.0), Out::Act(a) if a.starts_with("open:https://rare.market/item/")));
}

#[test]
fn signer_buys_in_app() {
    let mut rig = Rig::new();
    let mut a = data::item();
    a.account = Some(data::Account { address: data::DEMO.into(), label: "Main".into(), signer: true });
    let mut m = market(&mut rig, a, vec![View::Item { id: data::ITEM.into(), card: None, thumb: None }]);
    let mut cx = rig.cx();
    m.press(&mut cx, Pad::A, 0.0);
    assert_eq!(m.top().host().tag(), Some(super::buy::T_BUY));
    assert_eq!(m.press(&mut cx, Pad::A, 0.0), Out::Act(format!("market:buy:{}", data::ITEM)));
    assert_eq!(m.press(&mut cx, Pad::A, 0.0), Out::None, "no second purchase while one is being built");
}

#[test]
fn market_tab_steps_through_all_four_sections_and_wraps() {
    let mut rig = Rig::new();
    let mut cx = rig.cx();
    let main = crate::screens::trade::main_area(&mut cx);
    let mut tab = MarketTab::new(
        &mut cx,
        main,
        Area::Collections,
        |cx, n| Marketplace::new(cx, n, Rc::new(data::home()), vec![], 0.0),
        |cx, n| crate::screens::trade::tab::TradeTab::new(cx, n, Box::new(crate::screens::trade::tab::Fixtures), crate::screens::trade::root::Section::Home, 0.0),
    );
    let mut seen = Vec::new();
    for _ in 0..4 {
        tab.press(&mut cx, Pad::Rt, 0.0);
        seen.push((tab.area, tab.trade.section().tab()));
    }
    assert_eq!(seen, vec![(Area::Trade, 1), (Area::Trade, 2), (Area::Trade, 3), (Area::Collections, 3)]);
    tab.press(&mut cx, Pad::Lt, 0.0);
    assert_eq!((tab.area, tab.trade.section().tab()), (Area::Trade, 3), "LT from Collections is Traders");
}


#[test]
fn the_app_fetches_what_the_top_page_is_missing() {
    let mut rig = Rig::new();
    let mut m = market(&mut rig, data::home(), vec![]);
    assert!(m.missing().is_empty(), "the home fixture answers everything the page reads");
    let mut cx = rig.cx();
    m.press(&mut cx, Pad::X, 0.0);
    assert_eq!(m.missing(), vec![rare_market::api::paths::collections("7d", "volume", 25)]);
    let mut a = (*m.a).clone();
    a.set(rare_market::api::paths::collections("7d", "volume", 25), serde_json::json!({"data": [], "nextCursor": null}));
    m.set_answers(&mut cx, Rc::new(a), 0.0);
    assert!(m.missing().is_empty());
}

#[test]
fn search_takes_the_keyboard_and_closes_when_cancelled_empty() {
    let mut rig = Rig::new();
    let mut m = market(&mut rig, data::search(), vec![]);
    let mut cx = rig.cx();
    m.press(&mut cx, Pad::Y, 0.0);
    assert_eq!(m.depth(), 2);
    assert_eq!(m.search_answer(&mut cx, Some(" pop "), 0.0), Out::Moved);
    assert_eq!(m.nav.top().view, View::Search { q: "pop".into() });
    assert!(m.missing().is_empty(), "the fixture has /v1/search?q=pop");
    assert_eq!(m.top().host().tag(), Some(search::T_COLL), "lands on the first collection");
    m.search_answer(&mut cx, None, 0.0);
    assert_eq!(m.depth(), 2, "cancel keeps a page that has a term");
    m.press(&mut cx, Pad::B, 0.0);
    m.press(&mut cx, Pad::Y, 0.0);
    m.search_answer(&mut cx, None, 0.0);
    assert_eq!(m.depth(), 1, "cancel on a fresh search closes it");
}
