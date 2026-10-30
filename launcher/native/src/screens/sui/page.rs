//! The Sui tab as an app page: the catalog (rare.market's, the bundled copy silently until the live
//! one is read, retried until it is) and a game's page over it. B goes back from a game's page.

use super::catalog::{clean, seed, with_builtins, SuiGame};
use super::tab::{self, Data, Msg};
use crate::act::Act;
use crate::kit::Ui;
use crate::page::{hash, Acts, Btn, Fx, Page};
use crate::screens::wallet::render::{build, Built};
use crate::state::{State, Tab};
use rare_render::focus::Focusable;
use rare_render::view::El;
use std::collections::BTreeMap;

thread_local! {
    static OPEN_GAME: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Opens the Sui tab on this game's page the next time it ticks (Home's cards, Search).
pub fn open_game(slug: String) {
    OPEN_GAME.with(|o| *o.borrow_mut() = Some(slug));
}

pub struct SuiTab {
    games: Vec<SuiGame>,
    /// `games` is rare.market's answer, not the bundled copy
    live: bool,
    /// the game page on top
    detail: Option<String>,
    /// the collections sheet over the game page
    picking: bool,
    built: Built<Msg>,
    ver: u64,
    /// fixtures keep the bundled copy and never fetch
    fixture: bool,
}

/// rare.market's /v1/games, cleaned, with the built-ins. Blocking: on a worker.
fn live_catalog() -> Option<Vec<SuiGame>> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(8))).build().into();
    let v: serde_json::Value = agent.get("https://rare.market/v1/games").header("User-Agent", "RareLauncher/0.1").call().ok()?.body_mut().read_json().ok()?;
    let games = clean(v.get("data")?);
    (!games.is_empty()).then(|| with_builtins(games))
}

/// The live catalog for the whole app: fetched until rare.market answers, then kept, so the tab
/// opens on it every time after the first answer.
#[derive(Default)]
struct Shared {
    games: Option<Vec<SuiGame>>,
    /// the fetch loop is running
    running: bool,
    /// retry now (the tab opened)
    kick: bool,
}

static SHARED: std::sync::Mutex<Shared> = std::sync::Mutex::new(Shared { games: None, running: false, kick: false });

/// Seconds to wait after the nth failed fetch (1-based).
fn backoff(fails: u32) -> u64 {
    [1, 2, 4, 8, 15][(fails.max(1) as usize - 1).min(4)]
}

/// A default route exists (Linux); None where that can't be read.
fn net_up() -> Option<bool> {
    let t = std::fs::read_to_string("/proc/net/route").ok()?;
    Some(t.lines().skip(1).any(|l| l.split_whitespace().nth(1) == Some("00000000")))
}

/// rare.market's catalog once it has answered.
fn live_games() -> Option<Vec<SuiGame>> {
    SHARED.lock().ok()?.games.clone()
}

/// Asks for the live catalog: retries now if the loop is waiting, starts it if it isn't running.
/// The loop tries until an answer arrives: on a short backoff, and at once when the network comes up.
fn want_live() {
    let Ok(mut sh) = SHARED.lock() else { return };
    if sh.games.is_some() {
        return;
    }
    sh.kick = true;
    if sh.running {
        return;
    }
    sh.running = true;
    drop(sh);
    let spawned = std::thread::Builder::new().name("rare-sui-games".into()).spawn(|| {
        let mut fails = 0u32;
        loop {
            if let Ok(mut sh) = SHARED.lock() {
                sh.kick = false;
            }
            if let Some(g) = live_catalog() {
                if let Ok(mut sh) = SHARED.lock() {
                    sh.games = Some(g);
                    sh.running = false;
                }
                return;
            }
            fails += 1;
            let until = std::time::Instant::now() + std::time::Duration::from_secs(backoff(fails));
            let mut was_up = net_up();
            while std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if SHARED.lock().map(|s| s.kick).unwrap_or(false) {
                    break;
                }
                let up = net_up();
                if up == Some(true) && was_up == Some(false) {
                    break;
                }
                was_up = up;
            }
        }
    });
    if spawned.is_err() {
        if let Ok(mut sh) = SHARED.lock() {
            sh.running = false;
        }
    }
}

impl Default for SuiTab {
    fn default() -> Self {
        let fixture = std::env::var("RR_FIXTURE").is_ok();
        let live = if fixture { None } else { live_games() };
        if !fixture && live.is_none() {
            want_live();
        }
        SuiTab { live: live.is_some(), games: live.unwrap_or_else(seed), detail: None, picking: false, built: Built::default(), ver: 0, fixture }
    }
}

impl Page for SuiTab {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.ver, self.detail.clone(), self.picking, st.library.as_ref().map(|l| l.games.len())))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let (o, t) = (BTreeMap::new(), BTreeMap::new());
        let d = Data { owned: &o, tokens: &t };
        let lib = st.library.as_ref().map(|l| l.games.as_slice()).unwrap_or(&[]);
        let m = match self.detail.as_ref().and_then(|s| self.games.iter().find(|g| &g.slug == s)) {
            Some(g) => tab::detail(g, &d, lib, self.picking),
            None => tab::view(Some(&self.games), &d),
        };
        let (el, built) = build(ui, acts, &m);
        self.built = built;
        el
    }

    fn pad(&mut self, _st: &mut State, _fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if b == Btn::B && self.picking {
            self.picking = false;
            self.ver += 1;
            return true;
        }
        if b == Btn::B && self.detail.is_some() {
            self.detail = None;
            self.ver += 1;
            return true;
        }
        false
    }

    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        if let Some(slug) = OPEN_GAME.with(|o| o.borrow_mut().take()) {
            self.detail = Some(slug);
            self.picking = false;
            self.ver += 1;
        }
        if self.live || self.fixture {
            return None;
        }
        if let Some(games) = live_games() {
            self.games = games;
            self.live = true;
            self.ver += 1;
            return None;
        }
        Some(now + 0.5)
    }

    fn act(&mut self, _st: &mut State, fx: &mut Fx, code: u32) {
        let Some(m) = self.built.msg(code) else { return };
        match m {
            Msg::Open(slug) => {
                self.detail = Some(slug);
                self.picking = false;
                self.ver += 1;
            }
            Msg::Collections(slug) => {
                let Some(g) = self.games.iter().find(|g| g.slug == slug) else { return };
                match g.collections.as_slice() {
                    [] => {}
                    [one] => fx.run(Act::Market(format!("collection:{}", one.nft_type))),
                    _ => {
                        self.picking = true;
                        self.ver += 1;
                    }
                }
            }
            Msg::Collection(t) => {
                self.picking = false;
                self.ver += 1;
                fx.run(Act::Market(format!("collection:{t}")));
            }
            Msg::CloseSheet => {
                self.picking = false;
                self.ver += 1;
            }
            Msg::OpenWeb(u) => fx.run(Act::OpenWeb(u)),
            Msg::Launch(id) => fx.run(Act::Launch(id)),
            Msg::PlayWeb { slug, name, url } => fx.run(Act::PlayWeb { id: format!("web:{slug}"), title: name, url }),
            // Steam's own store page in Steam (SuiGames.tsx native.steamStore), else the store's
            // page in the web window.
            Msg::Store { url, steam, .. } => fx.run(match steam.and_then(|id| u32::try_from(id).ok()) {
                Some(id) => Act::SteamStore(id),
                None => Act::OpenWeb(url),
            }),
            Msg::YourItems(_) => fx.run(Act::Tab(Tab::Market)),
            Msg::AddToLibrary(slug) => {
                let Some(g) = self.games.iter().find(|g| g.slug == slug) else { return };
                let Some(url) = super::catalog::play_url(g) else { return };
                let art = |s: &str| (!s.is_empty()).then(|| s.to_string());
                fx.run(Act::AddWebGame { id: Some(g.slug.clone()), title: g.name.clone(), url, cover: art(&g.logo_url), hero: art(&g.banner_url) });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn backoff_is_short_and_capped() {
        assert_eq!((1..=7).map(super::backoff).collect::<Vec<_>>(), vec![1, 2, 4, 8, 15, 15, 15]);
    }
}
