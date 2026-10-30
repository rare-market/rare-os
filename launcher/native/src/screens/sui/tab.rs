//! The Sui tab (launcher/src/screens/SuiGames.tsx): a spotlight for the first featured game with
//! clean key art, the featured grid, All games and Coming; the store tiles (`SuiGameCard`), the
//! token line, the game logo tile, the "Owned" line (sui/ui/Owned.tsx), and a Sui game's own page
//! (`SuiGameDetail`) with Play, Get it, View on Rare and Your items.

use super::catalog::{clean_art, installed_native, is_active, platform_label, play_url, profile, status_label, unavailable, usable_banner, SuiGame, MIN_BANNER_WIDTH, MIN_LOGO_WIDTH};
use crate::data::model::Game;
use crate::kit::format::{NumKind, NumOptions};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, dot, num, section_head, BSize, BVariant, ButtonSpec, Tone};
use std::collections::BTreeMap;

pub const RARE: &str = "https://rare.market";

/// What the account holds of a game (sui/ownership.ts `GameOwnership`): the count and the
/// collections with rare.market's slug when there is one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Owned {
    pub count: u64,
    pub collections: Vec<(String, Option<String>)>,
}

/// the Trade API's price and 24 h change for a coin (lib/live.ts tokens).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TokenPrice {
    pub price_usd: Option<f64>,
    pub change_24h: Option<f64>,
}

pub struct Data<'a> {
    pub owned: &'a BTreeMap<String, Owned>,
    pub tokens: &'a BTreeMap<String, TokenPrice>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// open the game's page
    Open(String),
    OpenWeb(String),
    /// launch an installed library game (Play, or Play now for a game already in the library)
    Launch(String),
    /// open the web game in its window (Play now)
    PlayWeb { slug: String, name: String, url: String },
    AddToLibrary(String),
    /// the store: Steam by app id, else the store page in a browser
    Store { slug: String, steam: Option<u64>, url: String, store: String },
    /// close the page and open the player's items in this collection in the Marketplace
    YourItems(String),
    /// the game's collections: its one collection in the Market, else the pick sheet (slug)
    Collections(String),
    /// a collection in the Market (its NFT type: rare.market reads a type as the slug)
    Collection(String),
    /// close the pick sheet
    CloseSheet,
}

/// `TokenLine`: symbol, price, 24 h change. Nothing without a token.
pub fn token_line(g: &SuiGame, d: &Data, class: &str) -> Option<El<Msg>> {
    let tok = g.token.as_ref()?;
    let t = d.tokens.get(&tok.coin_type);
    let mut s = span(cn(&["flex items-center gap-[0.45rem] whitespace-nowrap", class])).kid(span("font-medium text-fg").text(format!("${}", tok.symbol)));
    if let Some(p) = t.and_then(|t| t.price_usd) {
        s = s.kid_opt(num(NumKind::Price, Some(p), NumOptions::default(), "text-fg/80", false, true));
    }
    Some(s.kid_opt(num(NumKind::Change, t.and_then(|t| t.change_24h), NumOptions::default(), "", false, true)))
}

/// `GameLogo`: the logo on a rounded tile, the first letter when it fails. A logo narrower than
/// `MIN_LOGO_WIDTH` keeps its own size (the renderer reads `data-min-logo`).
pub fn game_logo(g: &SuiGame, size: &str) -> El<Msg> {
    let mut tile = span("flex shrink-0 items-center justify-center overflow-hidden rounded-card bg-l3 text-[1.25rem] font-semibold text-fg/50").attr("style", format!("width: {size}; height: {size}"));
    if g.logo_url.is_empty() {
        tile = tile.text(g.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default());
    } else {
        tile = tile.kid(img(Some(&g.logo_url), &g.name, "h-full w-full object-cover").attr("data-art", "w512").attr("data-min-logo", MIN_LOGO_WIDTH.to_string()).attr("data-fallback", g.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()));
    }
    tile
}

fn status_line(g: &SuiGame) -> String {
    let st = g.status.as_deref().map(status_label).unwrap_or_default();
    let plats = g.platforms.iter().map(|p| platform_label(p)).collect::<Vec<_>>().join(", ");
    [st, plats].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

fn studio_genre(g: &SuiGame) -> String {
    [g.studio.as_str(), g.genre.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// `OwnedLine`: a white dot and "Owned · 3 items". Nothing when not owned.
pub fn owned_line(slug: &str, d: &Data, class: &str) -> Option<El<Msg>> {
    let own = d.owned.get(slug)?;
    Some(span(cn(&["flex items-center gap-[0.45rem] text-fg", class])).kid(dot(Tone::Primary, false, "")).kid(text(format!(
        "Owned · {} {}",
        crate::screens::wallet::fmt::num_locale(own.count as f64, 0, 0),
        if own.count == 1 { "item" } else { "items" }
    ))))
}

/// `OwnedActions`: Your items, when a held collection has a rare.market slug.
pub fn owned_actions(slug: &str, d: &Data) -> Option<El<Msg>> {
    let own = d.owned.get(slug)?;
    let col = own.collections.iter().find_map(|(_, s)| s.clone())?;
    Some(button_el(ButtonSpec::new("Your items").hint("Your items"), Some(Msg::YourItems(col))))
}

/// `SuiGameCard`: a store tile, logo first; every card the same shape, never promo art.
pub fn game_card(g: &SuiGame, d: &Data, wide: bool, def: bool) -> El<Msg> {
    let left = if d.owned.contains_key(&g.slug) {
        owned_line(&g.slug, d, "").unwrap_or_else(|| span(""))
    } else {
        let mut s = span("flex min-w-0 items-center gap-[0.45rem]");
        if g.status.as_deref() == Some("live") {
            s = s.kid(dot(Tone::Primary, false, ""));
        }
        s.kid(span("truncate").text(status_line(g)))
    };
    button(cn(&["tile flex h-[8rem] shrink-0 flex-col justify-between rounded-card border border-hair bg-l1 p-[1rem] text-left", if wide { "tile-wide w-full" } else { "w-[20rem]" }]), Some(Msg::Open(g.slug.clone())))
        .hint_a("Details")
        .def(def)
        .key(&g.slug)
        .kid(
            span("flex min-w-0 items-center gap-[1rem]")
                .kid(game_logo(g, "3.5rem"))
                .kid(span("min-w-0").kid(span("block truncate text-row font-semibold text-fg").text(&g.name)).kid(span("block truncate text-small text-muted").text(studio_genre(g)))),
        )
        .kid(span("flex min-w-0 items-center justify-between gap-[0.75rem] text-meta text-muted").kid(left).kid_opt(token_line(g, d, "")))
}

/// `ArtImg`: art that fades in once loaded and hides itself when it fails (or is narrower than `min_width`).
fn art_img(src: &str, class: &str, min_width: u32, w: u32, h: u32) -> El<Msg> {
    img(Some(src), "", cn(&[class, "object-cover"])).attr("data-width", w.to_string()).attr("data-height", h.to_string()).attr("data-art", "hero").attr("data-min-width", min_width.to_string()).attr("data-hide-on-fail", "true")
}

/// `Spotlight`: store-front size, art on the right, words on the dark left.
pub fn spotlight(g: &SuiGame, art: &str, d: &Data) -> El<Msg> {
    let mut meta = span("flex items-center gap-[0.5rem] text-small text-fg/70");
    if g.status.as_deref() == Some("live") {
        meta = meta.kid(dot(Tone::Primary, false, ""));
    }
    let words = [g.status.as_deref().map(status_label).unwrap_or_default(), g.studio.clone(), g.genre.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    meta = meta.kid(text(words));
    let mut col = span("relative flex max-w-[30rem] flex-col gap-[0.5rem] px-[2rem]").kid(meta).kid(span("text-display font-semibold").text(&g.name));
    if !g.description.is_empty() {
        col = col.kid(span("line-clamp-2 text-small text-fg/70").text(&g.description));
    }
    col = col.kid_opt(token_line(g, d, "mt-[0.25rem] text-small"));
    button("tile tile-wide relative flex h-[16rem] w-full shrink-0 items-center overflow-hidden rounded-panel bg-l1 text-left", Some(Msg::Open(g.slug.clone())))
        .hint_a("Details")
        .def(true)
        .kid(
            span("absolute inset-y-0 left-[30%] right-0")
                .kid(art_img(art, "absolute inset-0 h-full w-full object-[80%_center] mix-blend-lighten", 0, 1280, 256))
                .kid(span("absolute inset-y-0 left-0 w-1/2 bg-gradient-to-r from-l1 to-transparent")),
        )
        .kid(col)
}

/// The tab. `games` is None before the first catalog answer. The bundled copy shows silently
/// until rare.market answers.
pub fn view(games: Option<&[SuiGame]>, d: &Data) -> El<Msg> {
    let all = games.unwrap_or(&[]);
    let active: Vec<&SuiGame> = all.iter().filter(|g| is_active(g)).collect();
    let featured: Vec<&SuiGame> = active.iter().copied().filter(|g| g.featured).collect();
    let rest: Vec<&SuiGame> = active.iter().copied().filter(|g| !g.featured).collect();
    let coming: Vec<&SuiGame> = all.iter().filter(|g| !is_active(g)).collect();
    let spot = featured.iter().copied().find(|g| clean_art(g).is_some());
    let more: Vec<&SuiGame> = featured.iter().copied().filter(|g| Some(g.slug.as_str()) != spot.map(|s| s.slug.as_str())).collect();

    let mut root = div("h-full overflow-y-auto px-[2.5rem] pb-[2.5rem] pt-[0.5rem]").attr("data-scroll", "").kid(
        div("mb-[1.25rem] flex items-end gap-[1rem]")
            .kid(
                div("")
                    .kid(h1("text-title font-semibold").text("Sui Games"))
                    .kid(p("text-small text-muted").text(if games.is_some() { format!("{} games", active.len()) } else { String::new() })),
            )
            .kid(div("ml-auto").kid(button_el(ButtonSpec::new("Open on Rare").size(BSize::Sm), Some(Msg::OpenWeb(format!("{RARE}/games")))))),
    );
    // The spotlight's slot is always there, so nothing above the grid moves when the live catalog
    // replaces the bundled one.
    root = root.kid(match spot {
        Some(s) => div("mb-[1rem]").attr("data-nav-group", "suigames-spot").kid(spotlight(s, &clean_art(s).unwrap_or_default(), d)),
        None => div("mb-[1rem] h-[16rem] shrink-0"),
    });
    root = root.kid(div("grid grid-cols-3 gap-[1rem]").attr("data-nav-group", "suigames-featured").kids(more.iter().map(|g| game_card(g, d, true, false))));
    if !rest.is_empty() {
        root = root
            .kid(section_head(text("All games"), Some(text(rest.len().to_string())), None, "mb-[0.9rem] mt-[2rem]"))
            .kid(div("grid grid-cols-4 gap-[1rem]").attr("data-nav-group", "suigames-all").kids(rest.iter().map(|g| game_card(g, d, true, false))));
    }
    if !coming.is_empty() {
        root = root
            .kid(section_head(text("Coming"), Some(text(coming.len().to_string())), None, "mb-[0.4rem] mt-[2rem]"))
            .kid(p("mb-[0.9rem] text-small text-muted").text("Announced, paused or offline for now. They move up when they're live."))
            .kid(div("grid grid-cols-4 gap-[1rem]").attr("data-nav-group", "suigames-coming").kids(coming.iter().map(|g| game_card(g, d, true, false))));
    }
    root
}

/// The sheet that picks one of a game's collections to open in the Market.
pub fn collections_sheet(g: &SuiGame) -> El<Msg> {
    use crate::screens::wallet::kit::{sheet, SheetSpec};
    let rows = g.collections.iter().enumerate().map(|(i, c)| {
        button("tile flex w-full items-center justify-between gap-[1rem] rounded-card bg-l1 px-[1.25rem] py-[1rem] text-left", Some(Msg::Collection(c.nft_type.clone())))
            .hint_a("Open")
            .def(i == 0)
            .key(&c.nft_type)
            .kid(span("min-w-0 truncate text-row font-semibold text-fg").text(&c.name))
            .kid(icon("chevron-right", "shrink-0 text-muted").attr("size", "1.25rem"))
    });
    sheet(
        SheetSpec { title: "Collections".into(), subtitle: Some(g.name.clone()), icon: None, footer: None, close: Some(Msg::CloseSheet), width: "40rem" },
        vec![div("flex flex-col gap-[0.75rem]").attr("data-nav-group", "sui-collections").kids(rows)],
    )
}

/// A Sui game's page (`SuiGameBody`): Play (an installed native build or the web game), Get it,
/// View on Rare, Your items, Collections. It starts under the top bar. `picking`: the collections
/// sheet is up.
pub fn detail(g: &SuiGame, d: &Data, library: &[Game], picking: bool) -> El<Msg> {
    let url = play_url(g);
    let playable = url.is_some();
    let why = unavailable(&g.slug);
    let in_library = library.iter().find(|x| x.id == format!("web:{}", g.slug));
    let nat = profile(&g.slug).and_then(|p| p.native);
    let installed = if nat.is_some() || g.platforms.iter().any(|p| p == "pc" || p == "suiplay") { installed_native(&g.slug, &g.name, library) } else { None };
    let steam = nat.as_ref().and_then(|n| n.steam_app_ids.first().copied());
    let art = clean_art(g);
    let banner = if art.is_none() && usable_banner(g) { Some(g.banner_url.clone()) } else { None };

    let mut root = div("rise fixed inset-x-0 bottom-0 top-[4.25rem] z-30 overflow-hidden bg-bg");
    if let Some(a) = &art {
        root = root.kid(
            div("absolute inset-y-0 left-[30%] right-0 overflow-hidden")
                // Native cover crops about the centre only, so the art keeps its own aspect at the
                // full height and sits right of centre (object-[80%_30%] on the web).
                .kid(art_img(a, "absolute -right-[8%] top-0 h-full mix-blend-lighten", MIN_BANNER_WIDTH, 1920, 620))
                .kid(div("absolute inset-y-0 left-0 w-1/2 bg-gradient-to-r from-bg to-transparent"))
                .kid(div("absolute inset-x-0 top-0 h-[3rem] bg-gradient-to-b from-bg to-transparent"))
                .kid(div("absolute inset-x-0 bottom-0 h-[60%] bg-gradient-to-t from-bg via-bg/85 to-transparent")),
        );
    }
    if let Some(b) = &banner {
        root = root.kid(
            div("absolute inset-x-0 top-0 h-[52%]")
                .kid(art_img(b, "absolute inset-0 h-full w-full object-[center_30%]", MIN_BANNER_WIDTH, 1920, 580))
                .kid(div("absolute inset-x-0 top-0 h-[3rem] bg-gradient-to-b from-bg/70 to-transparent"))
                .kid(div("absolute inset-x-0 bottom-0 h-[70%] bg-gradient-to-t from-bg to-transparent")),
        );
    }
    let mut body = div("relative flex h-full flex-col justify-end gap-[1rem] px-[2.5rem] pb-[4.5rem]").kid(
        div("flex items-center gap-[1rem]")
            .kid(game_logo(g, "4.5rem"))
            .kid(div("flex flex-col gap-[0.25rem]").kid(div("text-small font-medium text-fg/70").text(studio_genre(g))).kid(h1("text-[3rem] font-semibold leading-none tracking-[-0.035em]").text(&g.name))),
    );
    if !g.description.is_empty() {
        body = body.kid(p("max-w-[44rem] text-body leading-relaxed text-fg/80").text(&g.description));
    }
    let mut facts = div("flex flex-wrap items-center gap-[1.25rem] text-small text-fg/75");
    if let Some(s) = &g.status {
        let mut f = span("flex items-center gap-[0.45rem]");
        if s == "live" {
            f = f.kid(dot(Tone::Primary, false, ""));
        }
        facts = facts.kid(f.kid(text(status_label(s))));
    }
    if !g.platforms.is_empty() {
        facts = facts.kid(span("").text(g.platforms.iter().map(|p| platform_label(p)).collect::<Vec<_>>().join(", ")));
    }
    facts = facts.kid_opt(token_line(g, d, "")).kid_opt(owned_line(&g.slug, d, ""));
    if !g.collections.is_empty() {
        facts = facts.kid(span("").text(g.collections.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")));
    }
    body = body.kid(facts);
    if let Some(w) = &why {
        body = body.kid(p("max-w-[44rem] text-small text-fg/80").text(w));
    }
    if let (None, Some(n)) = (installed, &nat) {
        body = body.kid(p("max-w-[44rem] text-small text-fg/80").text(&n.how));
    }
    let mut acts = div("flex gap-[0.75rem]");
    if let Some(i) = installed {
        acts = acts.kid(button_el(ButtonSpec::new("Play").v(BVariant::Primary).def(true), Some(Msg::Launch(i.id.clone()))));
    }
    if installed.is_none() {
        if let Some(u) = &url {
            let play = match in_library {
                Some(l) => Msg::Launch(l.id.clone()),
                None => Msg::PlayWeb { slug: g.slug.clone(), name: g.name.clone(), url: u.clone() },
            };
            acts = acts.kid(button_el(ButtonSpec::new("Play now").v(BVariant::Primary).def(true), Some(play)));
            if in_library.is_none() {
                acts = acts.kid(button_el(ButtonSpec::new("Add to library"), Some(Msg::AddToLibrary(g.slug.clone()))));
            }
        }
        if let Some(n) = &nat {
            acts = acts.kid(button_el(
                ButtonSpec::new(&format!("Get it on {}", n.store)).v(if url.is_some() { BVariant::Secondary } else { BVariant::Primary }).def(url.is_none()),
                Some(Msg::Store { slug: g.slug.clone(), steam, url: n.store_url.clone(), store: n.store.clone() }),
            ));
        }
    }
    let any = playable || installed.is_some() || nat.is_some();
    acts = acts.kid(button_el(ButtonSpec::new("View on Rare").v(if any { BVariant::Secondary } else { BVariant::Primary }).def(!any), Some(Msg::OpenWeb(format!("{RARE}/games/{}", g.slug)))));
    acts = acts.kid_opt(owned_actions(&g.slug, d));
    if !g.collections.is_empty() {
        acts = acts.kid(button_el(ButtonSpec::new("Collections"), Some(Msg::Collections(g.slug.clone()))));
    }
    root = root.kid(body.kid(acts));
    if picking && g.collections.len() > 1 {
        root = root.kid(collections_sheet(g));
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::sui::catalog::seed;

    fn data<'a>(owned: &'a BTreeMap<String, Owned>, tokens: &'a BTreeMap<String, TokenPrice>) -> Data<'a> {
        Data { owned, tokens }
    }

    #[test]
    fn tab_from_the_seed() {
        let games = seed();
        let (o, t) = (BTreeMap::new(), BTreeMap::new());
        let d = data(&o, &t);
        let v = view(Some(&games), &d);
        let txt = v.texts();
        let n_active = games.iter().filter(|g| is_active(g)).count();
        assert_eq!(txt[0], "Sui Games");
        assert_eq!(txt[1], format!("{n_active} games"));
        assert_eq!(txt[2], "Open on Rare");
        // the bundled copy shows without a notice
        assert!(!txt.iter().any(|t| t.contains("bundled")));
        // ACT has clean art: the spotlight, focused first
        assert_eq!(v.landing().as_deref().map(|l| l.starts_with("Live · Advanced Combat Tactics · Co-op shooterACT")), Some(true));
        assert!(txt.contains(&"All games".to_string()));
        assert!(txt.contains(&"Announced, paused or offline for now. They move up when they're live.".to_string()));
        assert!(txt.contains(&"Offline · Web".to_string()) || txt.iter().any(|t| t.starts_with("Offline")));
        let none = view(None, &d);
        assert!(!none.texts().iter().any(|t| t.ends_with(" games")));
        // a catalog without a spotlight keeps its slot, so the swap moves nothing above the grid
        let plain: Vec<SuiGame> = games.iter().cloned().map(|mut g| { g.featured = false; g }).collect();
        let after = view(Some(&plain), &d);
        assert_eq!(after.kids.len(), v.kids.len());
        assert_eq!(after.kids[0].class, v.kids[0].class);
        assert!(after.kids[1].class.contains("h-[16rem]"));
        assert!(none.kids[1].class.contains("h-[16rem]"));
    }

    #[test]
    fn cards_and_owned() {
        let games = seed();
        let pawtato = games.iter().find(|g| g.slug == "pawtato-land").unwrap();
        let mut o = BTreeMap::new();
        o.insert("pawtato-land".to_string(), Owned { count: 3, collections: vec![("Pawtato Land".into(), Some("pawtato-land".into()))] });
        let t = BTreeMap::new();
        let d = data(&o, &t);
        let c = game_card(pawtato, &d, true, false);
        assert!(c.contains_text("Owned · 3 items"));
        let p = detail(pawtato, &d, &[], false);
        assert!(p.find_nav("Your items").is_some());
        assert!(p.find_nav("Play now").is_some());
        assert_eq!(p.landing().as_deref(), Some("Play now"));
    }

    #[test]
    fn native_game_page() {
        let games = seed();
        let eve = games.iter().find(|g| g.slug == "eve-frontier").unwrap();
        let (o, t) = (BTreeMap::new(), BTreeMap::new());
        let d = data(&o, &t);
        let p = detail(eve, &d, &[], false);
        let n = profile("eve-frontier").unwrap().native.unwrap();
        assert!(p.contains_text(&n.how));
        let get = format!("Get it on {}", n.store);
        assert!(p.find_nav(&get).is_some());
        let lib: Vec<Game> = vec![serde_json::from_value(serde_json::json!({ "id": "steam:999", "source": "steam", "title": eve.name })).unwrap()];
        let p = detail(eve, &d, &lib, false);
        assert_eq!(p.landing().as_deref(), Some("Play"));
        assert!(p.find_nav(&get).is_none());
    }

    #[test]
    fn token_lines() {
        let g = SuiGame { slug: "x".into(), name: "X".into(), token: Some(crate::screens::sui::catalog::Token { symbol: "DEEP".into(), coin_type: "0xdeep::deep::DEEP".into() }), ..Default::default() };
        let o = BTreeMap::new();
        let mut t = BTreeMap::new();
        t.insert("0xdeep::deep::DEEP".to_string(), TokenPrice { price_usd: Some(0.1234), change_24h: Some(-1.5) });
        let d = data(&o, &t);
        let l = token_line(&g, &d, "").unwrap();
        assert_eq!(l.texts()[0], "$DEEP");
        assert_eq!(l.content(), format!("$DEEP{}{}", crate::kit::format::num_text(NumKind::Price, Some(0.1234), NumOptions::default()), crate::kit::format::num_text(NumKind::Change, Some(-1.5), NumOptions::default())));
    }
}
