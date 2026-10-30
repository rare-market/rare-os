//! Fixture data for the panels: the web mocks' values (overlay items from the game's collections,
//! the Steam mock's achievements, a few game badges), behind the same `PanelData` the app fills.
//! Scenario names are the menu fixtures `menu-items`, `menu-market`, `menu-trophies`,
//! `menu-item-detail` and their state variants.

use super::data::{sort_items, Item, Listing, Trophy};
use super::money::MIST;
use super::trade::{Offer, OfferKind};
use super::trophies::{game_rows, summarize, ChainDef, ChainGame, ChainTrophy, SteamAchievement, SteamGame, TrophySource};
use super::{Account, DetailView, OfferState, PanelData, Phase, RareGame};
use crate::screens::overlay::menu::model::MenuTab;
use crate::screens::overlay::menu::MenuData;
use rare_render::Src;
use std::collections::HashMap;

fn act_cover() -> Src {
    Src::Static("fixtures/act-cover.jpg", include_bytes!("../../../../../render/assets/games/act/cover.jpg"))
}
fn act_hero() -> Src {
    Src::Static("fixtures/act-hero2.jpg", include_bytes!("../../../../../render/assets/games/act/hero.jpg"))
}
fn alt_cover() -> Src {
    Src::Static("fixtures/act-cover.jpg", include_bytes!("../../../../../render/assets/games/act/cover.jpg"))
}
fn alt_hero() -> Src {
    Src::Static("fixtures/act-hero.jpg", include_bytes!("../../../../../render/assets/games/act/hero.jpg"))
}
fn mark() -> Src {
    Src::Static("fixtures/mark.png", include_bytes!("../../../../../render/assets/brand/mark.png"))
}

const SWORD: &str = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::sword::Sword";
const SHIELD: &str = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::shield::Shield";

/// A SUI amount in tenths.
fn sui(tenths: i128) -> Option<i128> {
    Some(tenths * MIST / 10)
}

fn item(n: u32, name: &str, coll: &str, ty: &str, image: Option<Src>, floor: i128, offer: i128, last: i128) -> Item {
    Item {
        id: format!("0x{n:064x}"),
        name: name.into(),
        nft_type: Some(ty.into()),
        image,
        collection: Some(coll.into()),
        listing: None,
        floor_mist: sui(floor),
        top_offer_mist: if offer > 0 { sui(offer) } else { None },
        last_sale_mist: if last > 0 { sui(last) } else { None },
    }
}

fn listing(venue: &str, tenths: i128) -> Option<Listing> {
    Some(Listing { price_mist: tenths * MIST / 10, all_in_mist: sui(tenths + tenths / 12), venue: venue.into(), seller: Some("0x00000000000000000000000000000000000000000000000000000000000000aa".into()) })
}

fn my_items() -> Vec<Item> {
    let mut listed = item(77, "Sword of Dawn #77", "Sword of Dawn", SWORD, Some(act_hero()), 42, 0, 35);
    listed.listing = listing("kiosk", 45);
    sort_items(&[
        item(12, "Sword of Dawn #12", "Sword of Dawn", SWORD, Some(act_cover()), 42, 30, 38),
        listed,
        item(3, "Ember Shield #3", "Ember Shield", SHIELD, Some(alt_cover()), 130, 90, 125),
        item(9, "Ember Shield #9", "Ember Shield", SHIELD, None, 130, 0, 0),
    ])
}

fn market_items() -> Vec<Item> {
    let mk = |n: u32, name: &str, coll: &str, ty: &str, img: Option<Src>, venue: &str, tenths: i128| {
        let mut it = item(100 + n, name, coll, ty, img, 0, 0, 0);
        it.floor_mist = None;
        it.listing = listing(venue, tenths);
        it
    };
    vec![
        mk(1, "Sword of Dawn #204", "Sword of Dawn", SWORD, Some(act_cover()), "kiosk", 42),
        mk(2, "Sword of Dawn #31", "Sword of Dawn", SWORD, Some(act_hero()), "tradeport", 44),
        mk(3, "Ember Shield #118", "Ember Shield", SHIELD, Some(alt_hero()), "market", 128),
        mk(4, "Sword of Dawn #502", "Sword of Dawn", SWORD, None, "drip", 470),
        mk(5, "Ember Shield #77", "Ember Shield", SHIELD, Some(alt_cover()), "tradeport", 135),
        mk(6, "Ember Shield #402", "Ember Shield", SHIELD, None, "kiosk", 24_800),
    ]
}

fn ach(api: &str, name: &str, desc: &str, hidden: bool, unlocked: bool, at: i64, pct: f64) -> SteamAchievement {
    SteamAchievement {
        api_name: api.into(),
        name: name.into(),
        description: desc.into(),
        icon_url: Some(format!("https://steam/{api}.jpg")),
        icon_gray_url: Some(format!("https://steam/{api}_gray.jpg")),
        hidden,
        unlocked,
        unlock_time: unlocked.then_some(at),
        global_percent: Some(pct),
    }
}

fn rare_game() -> RareGame {
    let steam = SteamGame {
        appid: 620,
        name: "Portal 2".into(),
        total: 6,
        unlocked: 4,
        achievements: vec![
            ach("WAKE_UP", "Wake Up Call", "Complete the first test chamber.", false, true, 1_799_000_000, 92.0),
            ach("LASER", "Laser Perception", "Redirect a laser through a cube.", false, true, 1_799_100_000, 71.0),
            ach("GRAVITY", "Triple Transit", "Use three gels in one chamber.", false, true, 1_798_000_000, 48.0),
            ach("SPEED", "Speed Run", "Finish a chapter in under ten minutes.", false, true, 1_799_500_000, 14.0),
            ach("PIT", "Bottomless", "Fall into the pit and live.", false, false, 0, 63.0),
            ach("SECRET", "Secret room", "Find the room behind the wall.", true, false, 0, 6.0),
        ],
    };
    let def = |id: u32, key: &str, tier: u8, unlocked: u32| ChainDef {
        trophy_id: id,
        key: key.into(),
        name: key.into(),
        description: String::new(),
        image_url: String::new(),
        tier,
        points: [15, 30, 90, 300][tier as usize],
        hidden: false,
        unlocked,
    };
    let chain = ChainGame { id: "0xg".into(), players: 40, defs: vec![def(0, "WAKE_UP", 0, 30), def(1, "LASER", 0, 22), def(2, "GRAVITY", 1, 14), def(3, "SPEED", 2, 4)] };
    let minted = vec![ChainTrophy { object_id: "0xt".into(), game: "0xg".into(), trophy_id: 0, name: "Wake Up Call".into(), tier: 0, points: 15, source: TrophySource::SelfReported, unlocked_at_ms: 1_799_000_000_000 }];
    let rows = game_rows(Some(&steam), Some(&chain), &minted);
    let sum = summarize(&rows);
    let mut art = HashMap::new();
    for (k, s) in [("WAKE_UP", act_cover()), ("LASER", alt_cover()), ("GRAVITY", act_hero()), ("SPEED", alt_hero())] {
        art.insert(k.to_string(), s);
    }
    RareGame { rows, sum, art }
}

fn badges() -> Vec<Trophy> {
    let t = |id: &str, name: &str, art: Option<Src>| Trophy { id: id.into(), nft_type: "0x1::badges::Badge".into(), name: name.into(), image_url: None, description: None, art };
    vec![t("0x1", "Season One Badge", Some(mark())), t("0x2", "Early Player", Some(alt_cover())), t("0x3", "Quest Reward", None), t("0x4", "Top 100", Some(act_cover())), t("0x5", "First Win", Some(alt_hero()))]
}

fn offer(tenths: i128, net: Option<i128>) -> Offer {
    Offer {
        id: "0x0f".into(),
        venue: "tradeport".into(),
        source: "tradeport/bids".into(),
        nft_type: Some(SHIELD.into()),
        kind: OfferKind::Collection,
        item_id: None,
        price_mist: tenths * MIST / 10,
        commission_mist: 0,
        escrow: None,
        seller_net_mist: net.map(|n| n * MIST / 10),
        quantity: 1,
        filled: 0,
        bidder: "0x3c2f7de4c6e2892a80d44b77fae4e14dcd95cf3b8f016c6b1f1da456df28587d".into(),
        expires_at_ms: None,
        open: true,
        fillable: true,
    }
}

fn base() -> PanelData {
    let items = my_items();
    let price = items.first().and_then(|i| i.floor_mist);
    PanelData {
        items: Some(items),
        market: Some(market_items()),
        trophies: vec![],
        rare: None,
        trophies_ready: true,
        game: "Word Game".into(),
        detail: DetailView { account: Account::Signer, offer: OfferState::Ready(offer(90, None)), price },
        phase: Phase::Idle,
    }
}

/// The menu fixture names this module owns, with the tab they open on.
pub fn fixture_tab(name: &str) -> Option<MenuTab> {
    Some(match name {
        "menu-items" | "menu-items-empty" | "menu-items-loading" | "menu-items-signed-out" => MenuTab::Items,
        "menu-market" | "menu-market-empty" | "menu-market-loading" => MenuTab::Market,
        "menu-trophies" | "menu-trophies-game" | "menu-trophies-both" | "menu-trophies-empty" => MenuTab::Trophies,
        n if n.starts_with("menu-item-detail") => MenuTab::Items,
        _ => return None,
    })
}

pub fn scenario(name: &str) -> PanelData {
    let mut d = base();
    let name = name.strip_prefix("menu-").unwrap_or(name);
    match name {
        "items-empty" => d.items = Some(vec![]),
        "items-loading" | "market-loading" => {
            d.items = None;
            d.market = None;
        }
        "market-empty" => d.market = Some(vec![]),
        "trophies" => {
            d.game = "Portal 2".into();
            d.rare = Some(rare_game());
        }
        "trophies-game" => d.trophies = badges(),
        "trophies-both" => {
            d.rare = Some(rare_game());
            d.trophies = badges();
        }
        "trophies-empty" => d.game = "Portal 2".into(),
        n if n.starts_with("item-detail") => {
            let first = d.items.as_mut().and_then(|v| v.first_mut()).unwrap();
            match n {
                "item-detail-fees" => d.detail.offer = OfferState::Ready(offer(90, Some(81))),
                "item-detail-nooffer" => {
                    d.detail.offer = OfferState::Nothing;
                    first.top_offer_mist = None;
                }
                "item-detail-looking" => d.detail.offer = OfferState::Looking,
                "item-detail-blocked" => d.detail.offer = OfferState::Blocked(offer(90, None), "Delist it on Drip first.".into()),
                "item-detail-watch" => d.detail.account = Account::WatchOnly,
                "item-detail-signedout" => d.detail.account = Account::SignedOut,
                "item-detail-elsewhere" => first.listing = listing("tradeport", 140),
                "item-detail-listed" => {
                    first.listing = listing("kiosk", 145);
                    d.detail.price = first.listing.as_ref().map(|l| l.price_mist);
                }
                "item-detail-building" => d.phase = Phase::Building("Getting the sale ready...".into()),
                "item-detail-signing" => d.phase = Phase::Signing,
                "item-detail-error" => d.phase = Phase::Error("Couldn't reach rare.market. Check the connection and try again.".into()),
                "item-detail-done" => d.phase = Phase::Done("Sold for 9 SUI. It can take a minute to show.".into()),
                _ => {}
            }
        }
        _ => {}
    }
    d
}

/// Opens the menu on the tab: the web `RareMenu` props the fixture needs, and the panels' data.
pub fn fixture_menu(name: &str, d: &mut MenuData) {
    let sc = scenario(name);
    super::set_data(sc.clone());
    d.tab = fixture_tab(name);
    let steam = matches!(d.tab, Some(MenuTab::Trophies)) && !name.contains("game") && !name.contains("both");
    if steam {
        d.rid = Some("steam:620".into());
        d.game.title = "Portal 2".into();
        d.trophies = true;
    } else {
        d.collections = true;
        d.trophies = matches!(d.tab, Some(MenuTab::Trophies));
        d.items = sc.items.as_ref().map(|v| v.iter().map(|i| crate::screens::overlay::menu::model::OverlayItem { name: i.name.clone(), floor_mist: i.floor_mist.map(|m| m as u128), top_offer_mist: i.top_offer_mist.map(|m| m as u128) }).collect());
    }
    if matches!(d.tab, Some(MenuTab::Trophies)) {
        d.trophies_count = (sc.rare.as_ref().map(|r| r.sum.unlocked).unwrap_or(0) + sc.trophies.len()) as u32;
    }
    d.loading = name.ends_with("-loading");
    d.signed_out = name.ends_with("signed-out") && !name.contains("detail");
    if d.signed_out {
        d.items = None;
        d.address = None;
    }
}
