//! The item sheet (launcher/src/inventory/ItemSheet.tsx): price and floor, best offer, last sale, and
//! the four things to do with an item you hold: sell into the best offer, list or change the
//! price, cancel the listing, send. A staked item says where to unstake instead (muted, no
//! actions). Every action shows on screen at once and rolls back if it fails (rare-market's
//! `InventoryStore::run_optimistic`).

use crate::screens::wallet::fmt::fmt_sui;
use crate::screens::wallet::home::lib::load_bar;
use crate::screens::wallet::kit::{btn_full, mimg, sheet, SheetSpec, Size, Variant};
use crate::screens::wallet::market::sui;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::onboard::passkey_page::{add_passkey_view, Need};
use crate::screens::wallet::ui::stat_line;
use rare_market::format::venue_name;
use rare_market::inventory::trade::Quote;
use rare_market::inventory::{is_staked, unstake_line, InvItem};

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Working(String),
    Error(String),
    Done(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub phase: Phase,
    /// the price picked to list at (starts at the floor)
    pub price: Option<i128>,
}

impl Default for State {
    fn default() -> Self {
        State { phase: Phase::Idle, price: None }
    }
}

pub struct Props<'a> {
    /// None while loading, Some(None) when it could not load
    pub item: Option<Option<&'a InvItem>>,
    /// the best offer's quote: None while looking, Some(None) when there is none
    pub quote: Option<Option<&'a Quote>>,
    pub can_sign: bool,
    /// an account exists (watch only) or not at all (AddPasskey's line)
    pub signed_out: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Close,
    Sell,
    Price(i128),
    OtherPrice,
    List,
    Cancel,
    Send,
    AddPasskey,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Close,
    /// sell into the quote's offer; done: "Sold <name> for <net> SUI"
    Sell { done: String },
    List { price: i128, done: String },
    Cancel { done: String },
    /// close, then open Send collectibles with this item
    Send(String),
    /// the price keyboard (overlay/price.ts `askPrice`) from this value
    AskPrice(i128),
    AddPasskey,
}

fn mist(s: Option<&str>) -> Option<i128> {
    s.filter(|v| !v.is_empty()).and_then(|v| v.trim().parse::<i128>().ok())
}

fn trim3(m: i128) -> String {
    fmt_sui(Some(m), 3, false, true)
}

/// `plain`: rare.market being unreachable reads as such; other errors as they are.
pub fn plain(message: &str) -> String {
    let m = message.to_lowercase();
    let unreachable = (m.starts_with("rare.market ") && m[12..].chars().next().is_some_and(|c| c.is_ascii_digit())) || m.contains("failed to fetch") || m.contains("networkerror") || m.contains("load failed");
    if unreachable {
        return "Couldn't reach rare.market. Check the connection and try again.".into();
    }
    if message.is_empty() {
        "That didn't go through. Try again.".into()
    } else {
        message.to_string()
    }
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let spec = |title: String, sub: Option<String>, width: &'static str| SheetSpec { title, subtitle: sub, icon: None, footer: None, close: Some(Msg::Close), width };
    let item = match pr.item {
        Some(None) => return sheet(spec("Item".into(), None, "52rem"), vec![p("text-sm text-secondary").text("Couldn't load this item.")]),
        None => return sheet(spec("Item".into(), None, "52rem"), vec![load_bar(true)]),
        Some(Some(i)) => i,
    };
    let listing = item.listing.as_ref();
    let staked = is_staked(item);
    let native = listing.is_some_and(|l| l.venue == "kiosk");
    let elsewhere = listing.filter(|_| !native).map(|l| venue_name(Some(&l.venue)));
    let floor = mist(item.floor_mist.as_deref());
    let price = st.price.or(floor);
    let quote = pr.quote.flatten();
    let busy = matches!(st.phase, Phase::Working(_));

    let status = if let Some(l) = listing {
        format!("Listed on {}", venue_name(Some(&l.venue)))
    } else if staked {
        "Staked".into()
    } else {
        "Not listed".into()
    };
    let mut info = div("min-w-0 flex-1").kid(p("text-sm text-secondary").text(status));
    if let Some(l) = listing {
        info = info.kid(sui(mist(l.all_in_mist.as_deref()).or(mist(Some(&l.price_mist))), 3, false, true, "text-xl font-semibold"));
    }
    let best = match quote {
        Some(q) => mist(Some(&q.offer.price_mist)),
        None => mist(item.top_offer_mist.as_deref()),
    };
    info = info.kid_opt(stat_line(
        vec![
            ("Floor".into(), floor.map(|f| sui(Some(f), 3, false, true, ""))),
            ("Best offer".into(), best.map(|b| sui(Some(b), 3, false, true, ""))),
            ("Last sale".into(), mist(item.last_sale_mist.as_deref()).map(|m| sui(Some(m), 3, false, true, ""))),
        ],
        true,
        false,
        "mt-3",
    ));
    let label = item.collection.as_ref().map(|c| c.name.as_str()).unwrap_or(&item.name);
    let mut body = vec![
        load_bar(pr.quote.is_none() && pr.can_sign),
        div("flex gap-5").kid(div("relative size-[8rem] shrink-0 overflow-hidden rounded-lg bg-layer-03").kid(mimg(item.image_url.as_deref(), label, 400, "absolute inset-0 size-full"))).kid(info),
    ];
    if staked {
        body.push(p("mt-5 text-sm text-secondary").attr("data-testid", "staked-hint").text(unstake_line(item)));
    }
    if !staked && !pr.can_sign {
        body.push(add_passkey_view(Need::Sell, pr.signed_out, "mt-5").map(&|_| Msg::AddPasskey));
    }
    if !staked && pr.can_sign && !matches!(st.phase, Phase::Done(_)) {
        let sell = match pr.quote {
            None => vec![p("text-sm text-secondary").text("Looking for offers...")],
            Some(Some(q)) if q.blocked.is_some() => vec![p("text-sm text-secondary").text(format!("Best offer {} SUI. {}", trim3(mist(Some(&q.offer.price_mist)).unwrap_or(0)), q.blocked.clone().unwrap_or_default()))],
            Some(Some(q)) => {
                let offer = mist(Some(&q.offer.price_mist)).unwrap_or(0);
                let venue = venue_name(Some(&q.offer.venue));
                vec![
                    btn_full(&format!("Sell for {} SUI", trim3(q.net_mist)), Variant::Primary, Size::Md, false, "", Some(Msg::Sell)).def(true),
                    p("text-label text-secondary").text(format!(
                        "{}{}",
                        if q.sponsored { "Network fee paid by Rare. " } else { "" },
                        if q.net_mist < offer {
                            format!("Best offer {} SUI on {venue}. You get this after fees and royalty.", trim3(offer))
                        } else {
                            format!("Best offer on {venue}. You get the full price.")
                        }
                    )),
                ]
            }
            Some(None) => vec![p("text-sm text-secondary").text("No offers on this item right now.")],
        };
        let mut actions = div("mt-6 flex flex-col gap-5").kid(section("flex flex-col gap-2").kid(h3("text-sm font-medium text-secondary").text("Sell now")).kids(sell));
        if let Some(v) = &elsewhere {
            actions = actions.kid(p("text-sm text-secondary").text(format!("Listed on {v}. Cancel it there to list or send it here.")));
        } else {
            let mut choices = Vec::new();
            if let Some(f) = floor {
                choices.push(("Floor", f));
                choices.push(("Floor +10%", f * 110 / 100));
            }
            let mut row = div("flex flex-wrap gap-2").attr("data-nav-group", "inv-price");
            for (l, m) in &choices {
                row = row.kid(btn_full(&format!("{l} {}", fmt_sui(Some(*m), 2, false, true)), if price == Some(*m) { Variant::Primary } else { Variant::Secondary }, Size::Md, false, "", Some(Msg::Price(*m))).key(*l));
            }
            row = row.kid(btn_full("Other price", Variant::Secondary, Size::Md, false, "", Some(Msg::OtherPrice)));
            let list_label = match price {
                Some(p_) => format!("{} for {} SUI", if listing.is_some() { "Relist" } else { "List" }, trim3(p_)),
                None => "Pick a price".into(),
            };
            actions = actions.kid(
                section("flex flex-col gap-2")
                    .kid(h3("text-sm font-medium text-secondary").text(if listing.is_some() { "Change the price" } else { "List for sale" }))
                    .kid(row)
                    .kid(btn_full(&list_label, Variant::Secondary, Size::Md, false, "", Some(Msg::List))),
            );
        }
        let mut last = div("flex flex-wrap gap-2 border-t border-border-primary pt-4");
        if native {
            last = last.kid(btn_full("Cancel listing", Variant::Secondary, Size::Md, false, "", Some(Msg::Cancel)));
        }
        if listing.is_none() {
            last = last.kid(btn_full("Send", Variant::Secondary, Size::Md, false, "", Some(Msg::Send)));
        }
        body.push(actions.kid(last));
    }
    let status = match &st.phase {
        Phase::Idle => None,
        Phase::Working(t) => Some(("text-secondary", t.clone())),
        Phase::Error(m) => Some(("text-negative", m.clone())),
        Phase::Done(t) => Some(("text-primary", t.clone())),
    };
    if let Some((c, t)) = status {
        body.push(p(cn(&["mt-4 text-sm", c])).role("status").text(t));
    }
    let _ = busy;
    sheet(spec(item.name.clone(), item.collection.as_ref().map(|c| c.name.clone()), "40rem"), body).attr("data-ready", if pr.quote.is_none() { "0" } else { "1" })
}

const WORKING: &str = "Review and approve in Rare Wallet.";

pub fn update(pr: &Props, st: &mut State, msg: Msg) -> Option<Effect> {
    let item = pr.item.flatten()?;
    let busy = matches!(st.phase, Phase::Working(_));
    match msg {
        Msg::Close => Some(Effect::Close),
        Msg::Price(m) => {
            st.price = Some(m);
            None
        }
        Msg::OtherPrice => Some(Effect::AskPrice(st.price.or(mist(item.floor_mist.as_deref())).unwrap_or(0))),
        Msg::Sell => {
            let q = pr.quote.flatten()?;
            if busy {
                return None;
            }
            st.phase = Phase::Working(WORKING.into());
            Some(Effect::Sell { done: format!("Sold {} for {} SUI", item.name, trim3(q.net_mist)) })
        }
        Msg::List => {
            let price = st.price.or(mist(item.floor_mist.as_deref()))?;
            if busy {
                return None;
            }
            st.phase = Phase::Working(WORKING.into());
            Some(Effect::List { price, done: format!("Listed {} for {} SUI", item.name, trim3(price)) })
        }
        Msg::Cancel => {
            if busy {
                return None;
            }
            st.phase = Phase::Working(WORKING.into());
            Some(Effect::Cancel { done: format!("Cancelled the listing of {}", item.name) })
        }
        Msg::Send => Some(Effect::Send(item.id.clone())),
        Msg::AddPasskey => Some(Effect::AddPasskey),
    }
}

/// The action's answer: done (toast it, close after 900 ms), declined in the wallet (back to idle),
/// or failed (the plain error).
pub fn finished(st: &mut State, done: &str, res: Result<(), Option<String>>) {
    st.phase = match res {
        Ok(()) => Phase::Done(done.into()),
        Err(None) => Phase::Idle,
        Err(Some(m)) => Phase::Error(plain(&m)),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::inventory::fixtures::{item_no, sui as mist_of};
    use rare_market::api::Offer;

    fn quote(net: f64, offer: f64) -> Quote {
        Quote { offer: Offer { venue: "tradeport".into(), price_mist: mist_of(offer), ..Default::default() }, net_mist: (net * 1e9) as i128, blocked: None, sponsored: false }
    }

    #[test]
    fn hold_and_sell() {
        let it = item_no(101);
        let q = quote(3.7, 3.9);
        let pr = Props { item: Some(Some(&it)), quote: Some(Some(&q)), can_sign: true, signed_out: false };
        let mut st = State::default();
        let v = view(&pr, &st);
        let t = v.texts();
        assert_eq!(&t[..3], ["Hero #1204", "Pawtato Heroes", "Not listed"]);
        assert!(t.contains(&"Sell for 3.7 SUI".to_string()));
        assert!(t.contains(&"Best offer 3.9 SUI on Tradeport. You get this after fees and royalty.".to_string()));
        assert!(t.contains(&"Floor 4.2".to_string()));
        assert!(t.contains(&"Floor +10% 4.62".to_string()));
        assert!(t.contains(&"List for 4.2 SUI".to_string()));
        assert!(t.contains(&"Send".to_string()));
        assert_eq!(v.landing().as_deref(), Some("Sell for 3.7 SUI"));
        assert_eq!(update(&pr, &mut st, Msg::Sell), Some(Effect::Sell { done: "Sold Hero #1204 for 3.7 SUI".into() }));
        assert!(view(&pr, &st).contains_text("Review and approve in Rare Wallet."));
        finished(&mut st, "Sold Hero #1204 for 3.7 SUI", Ok(()));
        let v = view(&pr, &st);
        assert!(v.contains_text("Sold Hero #1204 for 3.7 SUI"));
        assert!(!v.contains_text("Sell now"));
    }

    #[test]
    fn listed_and_staked() {
        let it = item_no(102);
        let pr = Props { item: Some(Some(&it)), quote: Some(None), can_sign: true, signed_out: false };
        let v = view(&pr, &State::default());
        let t = v.texts();
        assert!(t.contains(&"Listed on Kiosk".to_string()));
        assert!(t.contains(&"No offers on this item right now.".to_string()));
        assert!(t.contains(&"Change the price".to_string()));
        assert!(t.contains(&"Relist for 4.2 SUI".to_string()));
        assert!(t.contains(&"Cancel listing".to_string()));
        assert!(!t.contains(&"Send".to_string()));
        let tp = item_no(413);
        let v = view(&Props { item: Some(Some(&tp)), ..pr }, &State::default());
        assert!(v.contains_text("Listed on Tradeport. Cancel it there to list or send it here."));
        let st = item_no(414);
        let v = view(&Props { item: Some(Some(&st)), quote: Some(None), can_sign: true, signed_out: false }, &State::default());
        let t = v.texts();
        assert_eq!(t[2], "Staked");
        assert!(t.contains(&"Unstake on Popkins to list or send".to_string()));
        assert!(!t.contains(&"Sell now".to_string()));
        assert!(v.find_by_attr("data-testid", "staked-hint").is_some());
    }

    #[test]
    fn watch_only_and_errors() {
        let it = item_no(103);
        let pr = Props { item: Some(Some(&it)), quote: Some(None), can_sign: false, signed_out: false };
        let v = view(&pr, &State::default());
        assert!(v.contains_text("Add a passkey to sell."));
        assert!(view(&Props { item: Some(None), ..pr }, &State::default()).contains_text("Couldn't load this item."));
        assert_eq!(plain("rare.market 503: down"), "Couldn't reach rare.market. Check the connection and try again.");
        assert_eq!(plain("Offer filled"), "Offer filled");
        let mut st = State::default();
        finished(&mut st, "x", Err(None));
        assert_eq!(st.phase, Phase::Idle);
    }
}
