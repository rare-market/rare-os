//! Inventory fixtures (launcher/src/inventory/mock.ts, the same data as rare-market's mock): three
//! games and a group of other collectibles, a listed item, items with offers, a staked item.

use rare_market::api::{CollectionRef, ItemCard, Listing, Staked};
use rare_market::inventory::{InvGame, InvGroup, InvItem, Inventory};

pub const POPKIN: &str = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";
pub const OWNER: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

pub fn sui(n: f64) -> String {
    format!("{}", (n * 1e9).round() as i128)
}

pub fn hex(n: u32) -> String {
    format!("0x{n:064x}")
}

fn game(slug: &str, name: &str) -> InvGame {
    InvGame { slug: slug.into(), name: name.into(), logo_url: String::new() }
}

fn item(n: u32, name: &str, kind: &str, coll: (&str, &str), floor: f64, f: impl FnOnce(&mut InvItem)) -> InvItem {
    let mut it = ItemCard {
        id: if n == 412 { POPKIN.into() } else { hex(n) },
        name: name.into(),
        kind: kind.into(),
        image_url: (n == 412).then(|| format!("https://storage.claynosaurz.com/popkins/images/{POPKIN}")),
        collection: Some(CollectionRef { slug: coll.0.into(), name: coll.1.into(), image_url: None, verified: true }),
        owner: Some(OWNER.into()),
        floor_mist: Some(sui(floor)),
        ..Default::default()
    };
    f(&mut it);
    it
}

fn listing(venue: &str, price: f64, all_in: f64, kiosk: Option<String>) -> Option<Listing> {
    Some(Listing { venue: venue.into(), price_mist: sui(price), all_in_mist: Some(sui(all_in)), kiosk_id: kiosk, ..Default::default() })
}

pub fn groups() -> Vec<InvGroup> {
    let hero = ("pawtato-heroes", "Pawtato Heroes");
    let pop = ("popkins", "Popkins");
    let ship = ("warped-ships", "Warped Ships");
    let groups = vec![
        (
            Some(game("pawtato-heroes", "Pawtato Heroes")),
            vec![
                item(101, "Hero #1204", "0xa1::hero::Hero", hero, 4.2, |i| {
                    i.last_sale_mist = Some(sui(4.6));
                    i.top_offer_mist = Some(sui(3.9));
                }),
                item(102, "Hero #88", "0xa1::hero::Hero", hero, 4.2, |i| {
                    i.last_sale_mist = Some(sui(5.1));
                    i.listing = listing("kiosk", 6.0, 6.0, Some(hex(900)));
                }),
                item(103, "Hero #3017", "0xa1::hero::Hero", hero, 4.2, |_| {}),
            ],
        ),
        (
            Some(game("popkins", "Popkins")),
            vec![
                item(412, "Popkins #412", "0xb2::popkin::Popkin", pop, 58.0, |i| {
                    i.last_sale_mist = Some(sui(70.0));
                    i.top_offer_mist = Some(sui(64.0));
                }),
                item(413, "Popkins #1190", "0xb2::popkin::Popkin", pop, 58.0, |i| {
                    i.last_sale_mist = Some(sui(61.5));
                    i.listing = listing("tradeport", 66.0, 67.2, None);
                }),
                item(414, "Popkins #77", "0xb2::popkin::Popkin", pop, 58.0, |i| {
                    i.last_sale_mist = Some(sui(60.0));
                    i.staked = Some(Staked { protocol: Some("0x38f0bad7a60a8c4399a810430b7625b60c93983321b3cd8723f201cc559de5c1::staking_contract".into()), name: Some("staking_contract".into()), via: "staked".into() });
                }),
            ],
        ),
        (
            Some(game("warped-universe", "Warped Universe")),
            vec![item(201, "Scout Ship 07", "0xc3::ship::Ship", ship, 12.5, |i| {
                i.last_sale_mist = Some(sui(11.0));
                i.top_offer_mist = Some(sui(10.2));
            })],
        ),
        (
            None,
            vec![item(301, "Suifrens Cap", "0xd4::art::Piece", ("misc", "Misc art"), 1.1, |i| {
                i.last_sale_mist = Some(sui(1.4));
            })],
        ),
    ];
    groups
        .into_iter()
        .map(|(game, items)| {
            let v: i128 = items.iter().map(|i| i.floor_mist.as_deref().unwrap_or("0").parse::<i128>().unwrap_or(0)).sum();
            InvGroup { game, items, floor_value_mist: v.to_string() }
        })
        .collect()
}

pub fn inventory() -> Inventory {
    let groups = groups();
    let total = groups.iter().map(|g| g.items.len()).sum();
    Inventory { groups, total }
}

/// One item by its fixture number (101, 102, 412...).
pub fn item_no(n: u32) -> InvItem {
    let id = if n == 412 { POPKIN.to_string() } else { hex(n) };
    groups().into_iter().flat_map(|g| g.items).find(|i| i.id == id).expect("fixture item")
}
