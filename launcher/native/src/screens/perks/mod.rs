//! Rareling perks on screen (launcher/src/perks/ui.tsx): the profile badge, the trader list label,
//! and the profile's perks panel (themes and early access for holders; "Hold a Rareling to
//! unlock" for everyone else). All of it is hidden while Rarelings are off (the shipping
//! launcher, `rarelingsOn()` false): every view takes `on` and returns nothing then. Detection
//! and themes are rare-perks'. Phase 3, workstream 14.

use super::sui::catalog::SuiGame;
use super::sui::tab::{game_card, Data};
use super::wallet::fmt::date_md;
use super::wallet::kit::mimg;
use super::wallet::markup::*;
use super::wallet::ui::{button_el, dot, panel, section_head, ButtonSpec, Tone};

pub const RARELINGS_URL: &str = "https://rare.market/launchpad/rarelings";

/// perks/themes.ts: (id, label, swatch).
pub const THEMES: [(&str, &str, &str); 4] = [("rare", "Rare blue", "61 90 254"), ("royal", "Royal", "122 53 255"), ("midnight", "Midnight", "47 72 217"), ("periwinkle", "Periwinkle", "92 118 255")];

/// A launchpad drop (sui/data.ts `Launch`, rare-perks `launches`).
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub starts_ms: Option<i64>,
}

/// `upcomingLaunches`: not opened yet, soonest first.
pub fn upcoming(list: &[Launch], now: i64) -> Vec<Launch> {
    let mut v: Vec<&Launch> = list.iter().filter(|l| l.starts_ms.is_some_and(|t| t > now)).collect();
    v.sort_by_key(|l| l.starts_ms);
    v.into_iter().cloned().collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    OpenWeb(String),
    Theme(String),
    OpenGame(String),
}

/// The Rareling mark, in the brand colour (it follows the theme).
fn mark<M>(size: &str) -> El<M> {
    icon("rareling", "shrink-0").attr("style", format!("width: {size}; height: {size}")).attr("aria-hidden", "true")
}

/// `RarelingBadge`: only for a holder.
pub fn badge<M>(on: bool, owned: bool, count: u32) -> Option<El<M>> {
    if !on || !owned {
        return None;
    }
    Some(
        span("inline-flex items-center gap-[0.45rem] text-meta font-semibold text-fg")
            .attr("title", "Holds a Rareling")
            .kid(mark("0.9rem"))
            .kid(text("Rareling"))
            .kid_if(count > 1, || span("font-normal text-muted").text(count.to_string())),
    )
}

/// `RarelingLabel`: the trader list's muted word.
pub fn label<M>(on: bool) -> Option<El<M>> {
    on.then(|| span("inline-flex items-center gap-[0.3rem] text-label text-secondary").kid(mark("0.75rem")).kid(text("Rareling")))
}

/// `PerksSection`: locked (hold one to unlock) or the unlocked perks.
pub fn section_view(on: bool, owned: bool, theme: &str, drops: &[Launch], games: &[SuiGame], d: &Data, tz_min: i32) -> Option<El<Msg>> {
    if !on {
        return None;
    }
    type SM = Msg;
    if !owned {
        return Some(panel(
            "mx-[2.5rem] flex items-center gap-[1.5rem] px-[1.5rem] py-[1.25rem]",
            vec![
                mark("2.25rem"),
                div("min-w-0 flex-1").kid(div("text-row font-semibold").text("Hold a Rareling to unlock")).kid(div("text-small text-muted").text(
                    "A Rareling badge on your profile, launcher themes, early access to new Sui games and drops, and a Rareling label in Top traders.",
                )),
                div("").attr("data-nav-group", "perks-locked").kid(button_el(ButtonSpec::new("See Rarelings").icon("external-link", ""), Some(SM::OpenWeb(RARELINGS_URL.into())))),
            ],
        ));
    }
    let themes = div("mt-[0.9rem] flex flex-wrap gap-[0.75rem]").attr("data-nav-group", "perks-themes").attr("data-nav-remember", "").kids(THEMES.iter().map(|(id, l, sw)| {
        let on_ = *id == theme;
        button(cn(&["flex h-[2.85rem] items-center gap-[0.6rem] rounded-chip px-[1rem] text-body font-medium", if on_ { "bg-button-secondary-active text-fg" } else { "bg-button-secondary text-secondary" }]), Some(SM::Theme(id.to_string())))
            .hint_a(if on_ { "In use" } else { "Use this theme" })
            .attr("aria-pressed", on_.to_string())
            .key(*id)
            .kid(span("size-[1rem] shrink-0 rounded-full").attr("style", format!("background: rgb({sw})")).attr("aria-hidden", "true"))
            .kid(text(*l))
            .kid_if(on_, || icon("check", "").attr("size", "1rem"))
    }));
    let early: Vec<El<SM>> = drops
        .iter()
        .take(6)
        .map(|dr| {
            button("tile flex w-[20rem] shrink-0 items-center gap-[1rem] rounded-card border border-hair bg-l1 p-[1rem] text-left", Some(SM::OpenWeb(format!("https://rare.market/launchpad/{}", dr.slug))))
                .hint_a("Open")
                .key(format!("d:{}", dr.slug))
                .kid(span("relative h-[3.5rem] w-[3.5rem] shrink-0 overflow-hidden rounded-card bg-l3").kid(mimg(dr.image_url.as_deref(), &dr.name, 160, "absolute inset-0 h-full w-full")))
                .kid(span("min-w-0").kid(span("block truncate text-row font-semibold").text(&dr.name)).kid(span("block truncate text-small text-muted").text(format!("Drop, opens {}", dr.starts_ms.map(|t| date_md(t, tz_min, false)).unwrap_or_default()))))
        })
        .chain(games.iter().take(8).map(|g| {
            game_card(g, d, false, false).map(&|m| match m {
                super::sui::tab::Msg::Open(slug) => Msg::OpenGame(slug),
                super::sui::tab::Msg::OpenWeb(u) => Msg::OpenWeb(u),
                _ => Msg::OpenGame(g.slug.clone()),
            })
        }))
        .collect();
    let early_el = if early.is_empty() {
        p("px-[2.5rem] text-small text-muted").text("Nothing new on the way right now. New games and drops show here first.")
    } else {
        div("flex gap-[1rem] overflow-x-auto px-[2.5rem]").attr("data-rail", "").attr("data-nav-group", "perks-early").kids(early)
    };
    Some(
        section("flex flex-col gap-[1.25rem]")
            .kid(section_head(text("Rareling perks"), Some(span("inline-flex items-center gap-[0.4rem]").kid(dot(Tone::Primary, false, "")).kid(text("Unlocked"))), None, "px-[2.5rem]"))
            .kid(panel(
                "mx-[2.5rem] px-[1.5rem] py-[1.25rem]",
                vec![div("text-body font-semibold").text("Launcher theme"), div("text-small text-muted").text("Accent colours from rare.market. Only Rareling holders see them."), themes],
            ))
            .kid(div("").kid(section_head(text("Early access"), None, None, "mb-[0.9rem] px-[2.5rem]")).kid(early_el)),
    )
}

/// The early-access games: upcoming only (announced or coming), never paused or offline ones.
pub fn early_games(games: &[SuiGame]) -> Vec<SuiGame> {
    games.iter().filter(|g| !super::sui::catalog::is_active(g) && matches!(g.status.as_deref(), Some("announced" | "coming"))).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::sui::catalog::seed;
    use std::collections::BTreeMap;

    #[test]
    fn hidden_when_off() {
        let (o, t) = (BTreeMap::new(), BTreeMap::new());
        let d = Data { owned: &o, tokens: &t };
        assert!(badge::<()>(false, true, 3).is_none());
        assert!(label::<()>(false).is_none());
        assert!(section_view(false, true, "rare", &[], &[], &d, 0).is_none());
    }

    #[test]
    fn on_for_testing() {
        let (o, t) = (BTreeMap::new(), BTreeMap::new());
        let d = Data { owned: &o, tokens: &t };
        let b: El<()> = badge(true, true, 3).unwrap();
        assert_eq!(b.texts(), ["Rareling", "3"]);
        let locked = section_view(true, false, "rare", &[], &[], &d, 0).unwrap();
        assert!(locked.contains_text("Hold a Rareling to unlock"));
        let now = 1_793_113_620_000;
        let drops = upcoming(&[Launch { slug: "a".into(), name: "Later".into(), image_url: None, starts_ms: Some(now + 2 * 86_400_000) }, Launch { slug: "b".into(), name: "Past".into(), image_url: None, starts_ms: Some(now - 1) }], now);
        assert_eq!(drops.len(), 1);
        let games = early_games(&seed());
        assert!(games.iter().all(|g| g.status.as_deref() == Some("announced") || g.status.as_deref() == Some("coming")));
        let v = section_view(true, true, "royal", &drops, &games, &d, 0).unwrap();
        let t = v.texts();
        assert!(t.contains(&"Rareling perks".to_string()));
        assert!(t.contains(&"Royal".to_string()));
        assert!(t.contains(&"Drop, opens Oct 29".to_string()));
        let none = section_view(true, true, "rare", &[], &[], &d, 0).unwrap();
        assert!(none.contains_text("Nothing new on the way right now. New games and drops show here first."));
    }
}
