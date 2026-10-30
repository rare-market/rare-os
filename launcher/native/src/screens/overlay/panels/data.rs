//! What the panels show for one game (overlay/data.ts): your items with their floors, the cheapest
//! listings, and the trophy objects the game's own packages gave you. Loading is behind
//! `PanelSource`; the pure ordering and picking lives here with the web tests.

use rare_render::Src;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    pub price_mist: i128,
    pub all_in_mist: Option<i128>,
    pub venue: String,
    pub seller: Option<String>,
}

/// An ItemCard from rare.market with the collection's floor (data.ts OverlayItem).
#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub nft_type: Option<String>,
    pub image: Option<Src>,
    pub collection: Option<String>,
    pub listing: Option<Listing>,
    /// The collection's floor (cheapest all-in listing).
    pub floor_mist: Option<i128>,
    pub top_offer_mist: Option<i128>,
    pub last_sale_mist: Option<i128>,
}

impl Item {
    pub fn new(id: &str, name: &str) -> Item {
        Item { id: id.into(), name: name.into(), nft_type: None, image: None, collection: None, listing: None, floor_mist: None, top_offer_mist: None, last_sale_mist: None }
    }
    /// The price a buyer pays: all-in when the API knows it.
    pub fn buy_price(&self) -> Option<i128> {
        self.listing.as_ref().map(|l| l.all_in_mist.unwrap_or(l.price_mist))
    }
}

/// localeCompare(.., { numeric: true }) for plain names: case-insensitive, digit runs by value.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(|c| c.is_ascii_digit()) {
                        s.push(c);
                        it.next();
                    }
                    s.trim_start_matches('0').to_string()
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let o = m.len().cmp(&n.len()).then_with(|| m.cmp(&n));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(p), Some(q)) => {
                let o = p.to_lowercase().cmp(q.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Most sellable first: items with an offer (highest first), then listed, then the rest.
pub fn sort_items(items: &[Item]) -> Vec<Item> {
    let score = |x: &Item| if x.top_offer_mist.is_some_and(|v| v != 0) { 2 } else if x.listing.is_some() { 1 } else { 0 };
    let mut out = items.to_vec();
    out.sort_by(|a, b| score(b).cmp(&score(a)).then(b.top_offer_mist.unwrap_or(0).cmp(&a.top_offer_mist.unwrap_or(0))).then_with(|| natural_cmp(&a.name, &b.name)));
    out
}

/// The cheapest listings across the game's collections (all-in price), a turn per collection, never
/// the player's own. `pages` is one list per collection.
pub fn merge_market(pages: &[Vec<Item>], me: Option<&str>, limit: usize) -> Vec<Item> {
    let me = me.map(super::trade::norm_id);
    let price = |x: &Item| x.buy_price().unwrap_or(0);
    let mut lists: Vec<Vec<Item>> = pages
        .iter()
        .map(|p| {
            let mut l: Vec<Item> = p
                .iter()
                .filter(|x| {
                    x.listing.as_ref().is_some_and(|l| match (&me, &l.seller) {
                        (Some(m), Some(s)) => super::trade::norm_id(s) != *m,
                        _ => true,
                    })
                })
                .cloned()
                .collect();
            l.sort_by_key(|x| price(x));
            l
        })
        .collect();
    let mut out: Vec<Item> = Vec::new();
    let mut i = 0;
    while out.len() < limit && lists.iter().any(|l| l.len() > i) {
        let mut turn: Vec<Item> = lists.iter_mut().filter_map(|l| l.get(i).cloned()).collect();
        turn.sort_by_key(|x| price(x));
        out.extend(turn);
        i += 1;
    }
    out.truncate(limit);
    out
}

// ---------- trophies from the game's own packages ----------

#[derive(Clone, Debug, PartialEq)]
pub struct Trophy {
    pub id: String,
    pub nft_type: String,
    pub name: String,
    pub image_url: Option<String>,
    pub description: Option<String>,
    /// Fixture art (the app resolves image_url through rare-images).
    pub art: Option<Src>,
}

pub struct ChainObject {
    pub object_id: String,
    pub nft_type: String,
    /// Display fields: name, image_url / img_url, description.
    pub display: Vec<(String, String)>,
}

/// canonType's address part: 0x + 64 hex, lower case.
pub fn package_of(t: &str) -> String {
    super::trade::norm_id(t.split("::").next().unwrap_or(t))
}

fn canon_type(t: &str) -> String {
    super::trade::norm_type(t)
}

/// typeMatches: the same struct, generics aside.
fn type_matches(a: &str, b: &str) -> bool {
    let strip = |t: &str| canon_type(t).split('<').next().unwrap_or("").to_string();
    strip(a) == strip(b)
}

/// A struct's own name: `0x..::badges::SeasonOneBadge<..>` is "Season One Badge".
pub fn struct_label(t: &str) -> String {
    let c = canon_type(t);
    let last = c.split('<').next().unwrap_or("").rsplit("::").next().unwrap_or(t).replace('_', " ");
    let mut spaced = String::new();
    let mut prev: Option<char> = None;
    for ch in last.chars() {
        if ch.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
            spaced.push(' ');
        }
        spaced.push(ch);
        prev = Some(ch);
    }
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Objects from the game's own packages that are not one of its tradeable collections.
pub fn pick_trophies(objects: &[ChainObject], packages: &[String], collections: &[String]) -> Vec<Trophy> {
    let pkgs: Vec<String> = packages.iter().map(|p| package_of(p)).collect();
    let get = |o: &ChainObject, k: &str| o.display.iter().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
    objects
        .iter()
        .filter(|o| pkgs.contains(&package_of(&o.nft_type)) && !collections.iter().any(|c| type_matches(&o.nft_type, c)))
        .map(|o| Trophy {
            id: o.object_id.clone(),
            nft_type: o.nft_type.clone(),
            name: get(o, "name").unwrap_or_else(|| struct_label(&o.nft_type)),
            image_url: get(o, "image_url").or_else(|| get(o, "img_url")),
            description: get(o, "description"),
            art: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAND: &str = "0x09afb9a1c63a9bbaba650ef0a6b473b9874882cd63aab9570b99274e8f796f00::pawtato::LAND";
    const ME: &str = "0x7aa0c2206ef649591f9b12db3baa4572e01a30727b949f7749fc50e08f5ad784";
    const OTHER: &str = "0x00000000000000000000000000000000000000000000000000000000000000aa";

    fn it(id: &str) -> Item {
        Item::new(id, id)
    }

    #[test]
    fn puts_the_most_sellable_first() {
        let listed = Item { listing: Some(Listing { price_mist: 1, all_in_mist: None, venue: "kiosk".into(), seller: None }), ..it("listed") };
        let small = Item { top_offer_mist: Some(10), ..it("offer small") };
        let big = Item { top_offer_mist: Some(20), ..it("offer big") };
        let sorted = sort_items(&[it("plain"), listed, small, big]);
        assert_eq!(sorted.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["offer big", "offer small", "listed", "plain"]);
    }

    #[test]
    fn names_order_numerically() {
        let sorted = sort_items(&[it("Sword #10"), it("sword #9"), it("Sword #2")]);
        assert_eq!(sorted.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["Sword #2", "sword #9", "Sword #10"]);
    }

    #[test]
    fn market_cheapest_all_in_first_never_the_players_own() {
        let card = |id: &str, all: i128, seller: &str| Item { listing: Some(Listing { price_mist: all, all_in_mist: Some(all), venue: "tradeport".into(), seller: Some(seller.into()) }), ..it(id) };
        let pages = vec![vec![card("a", 50, OTHER), card("mine", 1, ME)], vec![card("b", 20, OTHER), card("c", 90, OTHER)]];
        let m = merge_market(&pages, Some(ME), 12);
        assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
        assert_eq!(merge_market(&pages, Some(ME), 2).len(), 2);
    }

    #[test]
    fn keeps_other_objects_from_the_games_packages() {
        let pkg = LAND.split("::").next().unwrap();
        let d = |k: &str, v: &str| (k.to_string(), v.to_string());
        let obj = |id: &str, t: &str, display: Vec<(String, String)>| ChainObject { object_id: id.into(), nft_type: t.into(), display };
        let objects = vec![
            obj("0x1", LAND, vec![d("name", "Land #1")]),
            obj("0x2", &format!("{pkg}::badges::SeasonOneBadge"), vec![d("name", "Season one"), d("image_url", "https://img/b.png")]),
            obj("0x3", &format!("{pkg}::quest::Quest_Reward"), vec![]),
            obj("0x4", "0x2::coin::Coin<0x2::sui::SUI>", vec![]),
        ];
        let t = pick_trophies(&objects, &[pkg.to_string()], &[LAND.to_string()]);
        assert_eq!(t.iter().map(|x| (x.id.as_str(), x.name.as_str(), x.image_url.as_deref())).collect::<Vec<_>>(), [("0x2", "Season one", Some("https://img/b.png")), ("0x3", "Quest Reward", None)]);
        assert_eq!(struct_label("0x2::badges::SeasonOneBadge<0x2::sui::SUI>"), "Season One Badge");
    }
}
