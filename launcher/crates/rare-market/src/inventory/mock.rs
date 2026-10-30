//! Inventory fixtures for golden images and offline runs (launcher/src/inventory/mock.ts): three
//! games, a listed item, items with offers, a staked item. [`net`] answers the reads the
//! inventory makes; [`MockTrade`] sells, lists and cancels against the same data (at once: the web
//! mock's 4 s indexer lag is left out so images are stable).

use super::trade::{Quote, TradeApi};
use super::{InvGame, InvGroup, InvItem};
use crate::api::{CollectionRef, Listing, Offer, OfferItem, Staked};
use crate::net::MockNet;
use std::sync::{Arc, Mutex};

const POPKIN: &str = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";
pub const OWNER: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

fn sui(n: f64) -> String {
    format!("{}", (n * 1e9).round() as i128)
}

fn hex(n: u32) -> String {
    format!("0x{n:064x}")
}

const T_HERO: &str = "0xa1::hero::Hero";
const T_POP: &str = "0xb2::popkin::Popkin";
const T_SHIP: &str = "0xc3::ship::Ship";
const T_MISC: &str = "0xd4::art::Piece";

fn game(slug: &str, name: &str) -> InvGame {
    InvGame { slug: slug.into(), name: name.into(), logo_url: String::new() }
}

fn item(n: u32, name: &str, kind: &str, coll: (&str, &str), floor: f64, f: impl FnOnce(&mut InvItem)) -> InvItem {
    let mut it = InvItem {
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

pub fn initial() -> Vec<InvGroup> {
    let hero = ("pawtato-heroes", "Pawtato Heroes");
    let pop = ("popkins", "Popkins");
    let ship = ("warped-ships", "Warped Ships");
    let groups = vec![
        (
            Some(game("pawtato-heroes", "Pawtato Heroes")),
            vec![
                item(101, "Hero #1204", T_HERO, hero, 4.2, |i| {
                    i.last_sale_mist = Some(sui(4.6));
                    i.top_offer_mist = Some(sui(3.9));
                }),
                item(102, "Hero #88", T_HERO, hero, 4.2, |i| {
                    i.last_sale_mist = Some(sui(5.1));
                    i.listing = listing("kiosk", 6.0, 6.0, Some(hex(900)));
                }),
                item(103, "Hero #3017", T_HERO, hero, 4.2, |_| {}),
            ],
        ),
        (
            Some(game("popkins", "Popkins")),
            vec![
                item(412, "Popkins #412", T_POP, pop, 58.0, |i| {
                    i.last_sale_mist = Some(sui(70.0));
                    i.top_offer_mist = Some(sui(64.0));
                }),
                item(413, "Popkins #1190", T_POP, pop, 58.0, |i| {
                    i.last_sale_mist = Some(sui(61.5));
                    i.listing = listing("tradeport", 66.0, 67.2, None);
                }),
                item(414, "Popkins #77", T_POP, pop, 58.0, |i| {
                    i.last_sale_mist = Some(sui(60.0));
                    i.staked = Some(Staked { protocol: Some("0x38f0bad7a60a8c4399a810430b7625b60c93983321b3cd8723f201cc559de5c1::staking_contract".into()), name: Some("staking_contract".into()), via: "staked".into() });
                }),
            ],
        ),
        (
            Some(game("warped-universe", "Warped Universe")),
            vec![item(201, "Scout Ship 07", T_SHIP, ship, 12.5, |i| {
                i.last_sale_mist = Some(sui(11.0));
                i.top_offer_mist = Some(sui(10.2));
            })],
        ),
        (
            None,
            vec![item(301, "Suifrens Cap", T_MISC, ("misc", "Misc art"), 1.1, |i| {
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

/// The offer an item with a top offer has (an item bid on Tradeport).
pub fn offer_for(it: &InvItem) -> Option<Offer> {
    let price = it.top_offer_mist.clone()?;
    Some(Offer {
        id: format!("o-{}", it.id),
        venue: "tradeport".into(),
        source: "tradeport/bids".into(),
        nft_type: Some(it.kind.clone()),
        fee_mist: "0".into(),
        royalty_mist: "0".into(),
        commission_mist: "0".into(),
        fillable: true,
        kind: "item".into(),
        collection: it.collection.clone().unwrap_or_default(),
        item: Some(OfferItem { id: it.id.clone(), name: it.name.clone(), image_url: it.image_url.clone().unwrap_or_default() }),
        price_mist: price,
        quantity: Some(1),
        filled: Some(0),
        bidder: hex(77),
        created_at: "2026-10-27T00:00:00.000Z".into(),
        status: "open".into(),
        ..Default::default()
    })
}

/// The shared fixture data.
#[derive(Clone)]
pub struct Db(pub Arc<Mutex<Vec<InvGroup>>>);

impl Default for Db {
    fn default() -> Self {
        Db(Arc::new(Mutex::new(initial())))
    }
}

impl Db {
    pub fn all(&self) -> Vec<InvItem> {
        self.0.lock().unwrap().iter().flat_map(|g| g.items.clone()).collect()
    }
    fn edit(&self, id: &str, f: impl Fn(&InvItem) -> Option<InvItem>) {
        for g in self.0.lock().unwrap().iter_mut() {
            g.items = g.items.iter().filter_map(|i| if i.id == id { f(i) } else { Some(i.clone()) }).collect();
        }
    }

    /// A net answering the inventory, each item and its offers from the current data.
    pub fn net(&self) -> MockNet {
        let n = MockNet::new();
        let groups: Vec<InvGroup> = self.0.lock().unwrap().iter().filter(|g| !g.items.is_empty()).cloned().collect();
        let total: usize = groups.iter().map(|g| g.items.len()).sum();
        n.on_read_json(&format!("/v1/accounts/{OWNER}/inventory"), serde_json::json!({"data": groups, "total": total}));
        for it in self.all() {
            n.on_read_json(&format!("/v1/items/{}", it.id), serde_json::to_value(&it).unwrap());
            let offers: Vec<Offer> = offer_for(&it).into_iter().collect();
            n.on_read_json(&format!("/v1/offers?item={}&limit=10", it.id), serde_json::json!({"data": offers, "nextCursor": null}));
        }
        n
    }
}

/// Trades against [`Db`].
#[derive(Clone, Default)]
pub struct MockTrade(pub Db);

impl TradeApi for MockTrade {
    fn quote(&self, item: &InvItem, _sender: &str) -> crate::Result<Option<Quote>> {
        let it = self.0.all().into_iter().find(|x| x.id == item.id).unwrap_or_else(|| item.clone());
        Ok(offer_for(&it).map(|offer| Quote { net_mist: offer.price_mist.parse().unwrap_or(0), offer, blocked: None, sponsored: true }))
    }
    fn sell(&self, item: &InvItem, _offer: &Offer, _sender: &str) -> crate::Result<()> {
        self.0.edit(&item.id, |_| None);
        Ok(())
    }
    fn list(&self, item: &InvItem, price_mist: u64, _sender: &str) -> crate::Result<()> {
        let p = price_mist.to_string();
        self.0.edit(&item.id, |i| Some(InvItem { listing: Some(Listing { venue: "kiosk".into(), price_mist: p.clone(), all_in_mist: Some(p.clone()), kiosk_id: Some(hex(900)), ..Default::default() }), ..i.clone() }));
        Ok(())
    }
    fn cancel(&self, item: &InvItem, _sender: &str) -> crate::Result<()> {
        self.0.edit(&item.id, |i| Some(InvItem { listing: None, ..i.clone() }));
        Ok(())
    }
}
