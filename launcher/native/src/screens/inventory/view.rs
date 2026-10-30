//! The cross-game inventory (launcher/src/inventory/InventoryView.tsx), the Collectibles tab of Rare
//! Wallet: everything the player holds across Sui games, a section per game with its floor value,
//! each item with its price (or Not listed, or Staked), floor and last sale. A opens the item
//! sheet; in select mode A toggles (never a staked item). The data and its optimistic updates
//! are rare-market's `inventory` (WS 11).

use crate::screens::wallet::fmt::{fmt_sui, num_locale};
use crate::screens::wallet::home::lib::load_bar;
use crate::screens::wallet::kit::empty;
use crate::screens::wallet::market::{card_from, item_card, sui};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::thumb;
use rare_market::inventory::{is_staked, total_floor_mist, unstake_line, InvItem, Inventory};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Open(String),
    Toggle(String),
    /// A on a staked item while selecting: nothing
    Nothing,
}

fn mist(s: Option<&str>) -> Option<i128> {
    s.and_then(|v| v.trim().parse::<i128>().ok())
}

/// `inv` is the store's view (base with pending operations applied) for this address, None
/// before it loaded; `error` when the load failed.
pub fn view(inv: Option<&Inventory>, loading: bool, error: bool, selecting: bool, selected: &BTreeSet<String>) -> El<Msg> {
    let mut d = div("pt-4").kid(load_bar(loading));
    if let Some(inv) = inv.filter(|i| i.total > 0) {
        d = d.kid(
            p("mb-4 text-sm text-secondary")
                .kid(text(format!("{} {}, floor value ", num_locale(inv.total as f64, 0, 0), if inv.total == 1 { "item" } else { "items" })))
                .kid(span("tabular-nums text-primary").text(format!("{} SUI", fmt_sui(mist(Some(&total_floor_mist(inv))), 2, false, true)))),
        );
    }
    let mut idx = 0usize;
    for g in inv.map(|i| i.groups.as_slice()).unwrap_or(&[]) {
        let slug = g.game.as_ref().map(|x| x.slug.clone()).unwrap_or_else(|| "other".into());
        let n = g.items.len();
        if n == 0 {
            // No empty game sections (an item can move out while a send is pending).
            continue;
        }
        let mut count = span("text-sm text-secondary").text(format!("{n} {}", if n == 1 { "item" } else { "items" }));
        if mist(Some(&g.floor_value_mist)).unwrap_or(0) > 0 {
            count.text = None;
            count = count.kid(text(format!("{n} {}", if n == 1 { "item" } else { "items" }))).kid(text(", floor value ")).kid(sui(mist(Some(&g.floor_value_mist)), 2, false, true, ""));
        }
        let mut head = div("mb-3 flex items-center gap-3");
        if let Some(game) = &g.game {
            head = head.kid(thumb(if game.logo_url.is_empty() { None } else { Some(&game.logo_url) }, &game.name, "size-8", false));
        }
        head = head.kid(h2("text-base font-semibold").text(g.game.as_ref().map(|x| x.name.as_str()).unwrap_or("Other collectibles"))).kid(count);
        let grid = div("grid grid-cols-6 gap-3").attr("data-nav-group", format!("inv-{slug}")).kids(g.items.iter().map(|it: &InvItem| {
            let on = selected.contains(&it.id);
            let i = idx;
            idx += 1;
            let staked = is_staked(it);
            let msg = if selecting && !staked {
                Msg::Toggle(it.id.clone())
            } else if selecting {
                Msg::Nothing
            } else {
                Msg::Open(it.id.clone())
            };
            let card = card_from(it);
            let mut cell = div("relative min-w-0")
                .key(&it.id)
                .kid(item_card(&card, i, i == 0, false, &cn(&["w-full", when(on, "outline outline-2 -outline-offset-2 outline-primary")]), msg))
                .kid(p("mt-1 truncate px-1 text-label text-secondary").text(format!(
                    "Floor {}",
                    match mist(it.floor_mist.as_deref()) {
                        Some(f) if it.floor_mist.as_deref().is_some_and(|s| !s.is_empty()) => fmt_sui(Some(f), 2, false, true),
                        _ => "--".into(),
                    }
                )));
            if selecting {
                let mark = span(&cn(&[
                    "pointer-events-none absolute left-2 top-2 flex size-6 items-center justify-center rounded-full border",
                    if on { "border-primary bg-primary text-background" } else { "border-border-light bg-background/70" },
                ]));
                cell = cell.kid(if on { mark.kid(icon("check", "").attr("size", "0.95rem").attr("stroke-width", "3")) } else { mark });
            }
            if staked {
                cell = cell.kid(p("px-1 text-label text-secondary").attr("data-testid", "staked-hint").text(unstake_line(it)));
            }
            cell
        }));
        d = d.kid(section("mb-8").key(&slug).kid(head).kid(grid));
    }
    if inv.is_some_and(|i| i.total == 0) {
        d = d.kid(empty("No collectibles in this wallet", Some(text("Items from Sui games show up here, grouped by game.")), None, false));
    }
    if error && inv.is_none() {
        d = d.kid(empty("Could not load your inventory", Some(text("Check the connection and try again.")), None, false));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::inventory::fixtures::inventory;

    #[test]
    fn grouped_by_game() {
        let inv = inventory();
        let v = view(Some(&inv), false, false, false, &BTreeSet::new());
        let t = v.texts();
        assert_eq!(t[0], "8 items, floor value ");
        assert_eq!(t[1], "200.2 SUI");
        assert_eq!(&t[2..4], ["Pawtato Heroes", "3 items"]);
        assert!(t.contains(&"Staked".to_string()));
        assert!(t.contains(&"Unstake on Popkins to list or send".to_string()));
        assert!(t.contains(&"Other collectibles".to_string()));
        assert!(t.contains(&"Floor 4.2".to_string()));
        assert_eq!(v.landing().as_deref().map(|l| l.contains("Hero #1204")), Some(true));
    }

    #[test]
    fn select_mode_skips_staked() {
        let inv = inventory();
        let v = view(Some(&inv), false, false, true, &BTreeSet::from([crate::screens::inventory::fixtures::hex(101)]));
        let staked = inv.groups[1].items.iter().find(|i| i.staked.is_some()).unwrap();
        assert!(v.focusables().iter().any(|(_, n)| n.on == Some(Msg::Nothing)));
        assert!(v.focusables().iter().any(|(_, n)| n.on == Some(Msg::Toggle(crate::screens::inventory::fixtures::hex(101)))));
        assert!(!v.focusables().iter().any(|(_, n)| n.on == Some(Msg::Toggle(staked.id.clone()))));
        assert!(v.dump().contains("outline-primary"));
    }

    #[test]
    fn empty_and_error() {
        let none = Inventory { groups: vec![], total: 0 };
        assert!(view(Some(&none), false, false, false, &BTreeSet::new()).contains_text("No collectibles in this wallet"));
        assert!(view(None, false, true, false, &BTreeSet::new()).contains_text("Could not load your inventory"));
    }
}
