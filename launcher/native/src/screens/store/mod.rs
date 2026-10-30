//! Rare Store (launcher/src/store, hidden behind SHOW_STORE): the store, a game's page and its buy sheet.

pub mod buy;
pub mod game;
pub mod home;
pub mod logic;
pub mod model;
pub mod parts;

use crate::screens::market::host::{Cx, Page, Screen};
use crate::screens::trade::DEMO;
use model::{normalize_config, Account, ChainRelease, Demo, InstalledAt, StoreGame};
use rare_render::Id;

/// The Rare Store is hidden for now: nothing is ready to sell (store/show.ts). While false there
/// is no Store tab, no store game page and no store licences in the Library. The screens stay in
/// place (and the golden fixtures still render); `RARE_SHOW_STORE=1` brings them back for testing.
pub const SHOW_STORE: bool = false;

/// The gate the app checks before showing anything of the store.
pub fn shown() -> bool {
    SHOW_STORE || std::env::var("RARE_SHOW_STORE").is_ok_and(|v| v == "1")
}

/// This area's fixtures (the golden harness shoots these by default).
pub const SHOTS: &[&str] = &[
    "store",
    "store-search",
    "store-search-empty",
    "store-live",
    "store-failed",
    "store-game-link",
    "store-game-coming",
    "store-game-buy",
    "store-game-owned",
    "store-game-installed",
    "store-buy-sheet",
];

fn store_data(d: &Demo, owned: bool) -> home::StoreData {
    let ok = |s: &Vec<String>| Some(Ok(d.list(s)));
    home::StoreData {
        now_ms: d.now,
        featured: ok(&d.shelves.featured),
        fresh: ok(&d.shelves.new),
        top: ok(&d.shelves.top),
        sale: ok(&d.shelves.sale),
        results: None,
        owned: if owned { d.licences.iter().map(|l| l.slug.clone()).collect() } else { Vec::new() },
    }
}

/// The game page's state as the web screenshot has it: `sell` (storedemo=sell: deployed ids),
/// `own` (=both: the demo licence), and the watch-only demo wallet or a signer.
fn game_state(d: &Demo, slug: &str, sell: bool, own: bool, account: Account) -> game::GameState {
    let g: Option<StoreGame> = d.game(slug).cloned();
    let licence = if own { d.licences.iter().find(|l| l.slug == slug).cloned() } else { None };
    let release = licence.as_ref().and(g.as_ref()).and_then(|g| g.latest.as_ref().map(|l| ChainRelease::from_latest(&g.slug, l)));
    game::GameState {
        now_ms: d.now,
        game: Some(g.ok_or(())),
        config: Some(normalize_config(if sell { &d.config_sell } else { &d.config_closed })),
        account,
        licence,
        release,
        sui_e8: Some(d.sui_price.price_e8),
        ..Default::default()
    }
}

/// A page of this area by fixture name, built into `parent` (the shell's main area).
pub fn page(name: &str, cx: &mut Cx, parent: Id, now: f64) -> Option<Box<dyn Page>> {
    if !SHOTS.contains(&name) {
        return None;
    }
    let d = model::demo();
    let watch = || Account::Watch(DEMO.to_string());
    Some(match name {
        "store" => Box::new(home::StorePage::new(cx, parent, store_data(&d, false), None, now)),
        "store-search" | "store-search-empty" => {
            let q = if name == "store-search" { "orchard" } else { "zzz" };
            let mut data = store_data(&d, false);
            data.results = Some(Ok(d.search.get(q).map(|s| d.list(s)).unwrap_or_default()));
            let mut p = home::StorePage::new(cx, parent, data, Some(q.into()), now);
            p.land(cx, now);
            Box::new(p)
        }
        "store-live" | "store-failed" => {
            let v = if name == "store-live" { Some(Ok(Vec::new())) } else { Some(Err(())) };
            let data = home::StoreData { now_ms: d.now, featured: v.clone(), fresh: v.clone(), top: v.clone(), sale: v, ..Default::default() };
            Box::new(home::StorePage::new(cx, parent, data, None, now))
        }
        "store-game-link" => Box::new(game::GamePage::new(cx, parent, game_state(&d, "act", false, false, Account::None), now)),
        "store-game-coming" => Box::new(game::GamePage::new(cx, parent, game_state(&d, "driftline", false, false, Account::None), now)),
        "store-game-buy" => Box::new(game::GamePage::new(cx, parent, game_state(&d, "hollow-keep", true, false, watch()), now)),
        "store-game-owned" => Box::new(game::GamePage::new(cx, parent, game_state(&d, "driftline", true, true, watch()), now)),
        "store-game-installed" => {
            let mut st = game_state(&d, "driftline", true, true, Account::Signer(DEMO.into()));
            st.installed = Some(InstalledAt { edition: 1, version: 2 });
            st.lib_game = true;
            Box::new(game::GamePage::new(cx, parent, st, now))
        }
        "store-buy-sheet" => {
            let st = game_state(&d, "hollow-keep", true, false, Account::Signer(DEMO.into()));
            let mut p = game::GamePage::new(cx, parent, st, now);
            let g = d.game("hollow-keep").cloned().unwrap_or_default();
            p.buying = Some(buy::Buy::new(&g, Some(d.sui_price.price_e8), d.now));
            p.render(cx, now);
            Box::new(p)
        }
        _ => return None,
    })
}

/// Builds a screen by fixture name (the WS 13 golden harness and tests).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let main = crate::screens::trade::main_area(cx);
    page(name, cx, main, now).map(|p| p as Box<dyn Screen>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::overlay::common::Out;
    use rare_render::input::Pad;
    use rare_render::theme::rem_for;
    use rare_render::{ImageLoader, Scene, TextSys};
    use std::sync::Arc;

    static FONTS: [&[u8]; 2] = [include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf"), include_bytes!("../../../../render/assets/fonts/FiraCode-Medium.ttf")];

    /// Builds fixture `name` headless and plays `pads`, returning what each press said.
    fn play(name: &str, pads: &[Pad]) -> (Vec<Out>, Vec<(&'static str, String)>) {
        let mut scene = Scene::new(1920.0, 1200.0, rem_for(1920.0, 1200.0));
        let mut text = TextSys::new(&FONTS, false, 1024);
        let mut img = ImageLoader::new(1, Arc::new(|| {}), None);
        let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut img };
        let main = crate::screens::trade::main_area(&mut cx);
        let mut p = page(name, &mut cx, main, 0.0).expect(name);
        let outs = pads.iter().map(|b| p.pad(&mut cx, *b, 0.0)).collect();
        (outs, p.hints())
    }

    fn act(o: &Out) -> &str {
        match o {
            Out::Act(s) => s,
            _ => "",
        }
    }

    #[test]
    fn store_tiles_open_their_game_and_x_searches() {
        let (o, h) = play("store", &[Pad::A, Pad::X, Pad::Down, Pad::Down, Pad::Right, Pad::A]);
        assert_eq!(act(&o[0]), "store:game:driftline");
        assert_eq!(act(&o[1]), "store:search");
        // Down twice from the first featured tile lands on New, right moves along the rail.
        assert_eq!(act(&o[5]), "store:game:driftline");
        assert_eq!(h[0], ("a", "Details".to_string()));
    }

    #[test]
    fn b_leaves_the_results_first() {
        let (o, _) = play("store-search", &[Pad::A, Pad::B, Pad::B]);
        assert_eq!(act(&o[0]), "store:game:pocket-orchard");
        assert_eq!(o[1], Out::Moved);
        assert_eq!(o[2], Out::Close);
    }

    #[test]
    fn game_pages_hand_their_actions_to_the_app() {
        let (o, _) = play("store-game-link", &[Pad::A, Pad::Right, Pad::A]);
        assert_eq!(act(&o[0]), "store:play:act");
        assert_eq!(act(&o[2]), "store:website:act");
        // Not on sale: no buy button, nothing to press.
        let (o, h) = play("store-game-coming", &[Pad::A]);
        assert_eq!(o[0], Out::None);
        assert_eq!(h, vec![("b", "Back".to_string())]);
        // Watch-only: Buy says so instead of opening the sheet.
        let (o, _) = play("store-game-buy", &[Pad::A]);
        assert_eq!(o[0], Out::Moved);
        let (o, _) = play("store-game-installed", &[Pad::A, Pad::Right, Pad::A]);
        assert_eq!(act(&o[0]), "store:install:driftline");
    }

    #[test]
    fn the_buy_sheet_pays_and_b_closes_it() {
        let (o, _) = play("store-buy-sheet", &[Pad::Right, Pad::A, Pad::B]);
        assert_eq!(act(&o[1]), "store:buy:hollow-keep:sui");
        // Waiting on Rare Wallet: B does not close it.
        assert_eq!(o[2], Out::Moved);
        let (o, _) = play("store-buy-sheet", &[Pad::A]);
        assert_eq!(act(&o[0]), "store:buy:hollow-keep:usdc");
    }
}
