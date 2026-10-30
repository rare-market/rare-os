//! Rare over a running game (overlay/Overlay.tsx's session, docs/OVERLAY.md). rare-overlay
//! (features/ingame.rs) captures the game's frame, raises our window over the game and takes the
//! pad, then says so with an `overlay` event; this shows what it says: the Rare menu, the Switcher,
//! Quick settings or the power menu over the dimmed still of the game. Resume and B go back to
//! the game, Rare again goes Home (the crate decides and sends `open: false, home: true`).

use super::*;
use std::path::PathBuf;

pub(super) struct Ingame {
    /// The game under the overlay (its run id).
    pub rid: Option<String>,
    pub frame: Option<PathBuf>,
    /// "menu", "switcher", "quick" or "power" (rare_overlay::Mode).
    pub mode: String,
    /// The dimmed still behind everything the session shows (its wrapper in the layer under sheets).
    backdrop: Id,
    /// Going back to the game, or away from it, was asked: nothing reopens until the event.
    closing: bool,
    /// The library knew the game when the menu was built (else it is rebuilt once it does).
    titled: bool,
}

/// The still's image: gamescope's capture, or the fixture frame for shots (`RR_OPEN=ingame:..|..|fixture`).
fn frame_src(p: PathBuf) -> rare_render::Src {
    if p.as_os_str() == "fixture" {
        return rare_render::Src::Static("fixtures/act-hero.jpg", crate::screens::overlay::fixtures::FRAME);
    }
    rare_render::Src::File(p)
}

/// The layer the still sits in: over the page and the top bar, under sheets (Quick settings). The
/// hint bar, which sits at the sheet level, is hidden while the session is up.
const BACKDROP: i16 = layer::SHEET - 1;

impl App {
    /// The open session's game and frame, for the Rare menu and the Switcher.
    pub(super) fn ingame_session(&self) -> Option<(Option<String>, Option<rare_render::Src>)> {
        let ig = self.ingame.as_ref()?;
        Some((ig.rid.clone(), ig.frame.clone().map(frame_src)))
    }

    /// Backend events the in-game layer handles. True when taken.
    pub(super) fn ingame_event(&mut self, ev: &rare_core::Event, now: f64) -> bool {
        let rare_core::Event::Other { name, payload } = ev else { return false };
        match name.as_str() {
            "overlay" => self.on_overlay(payload, now),
            // A button rare-overlay hands back to the launcher's UI (Quick with the launcher in
            // front, View + Menu when the launcher has the screen).
            "pad" => {
                let b = match payload.get("button").and_then(|b| b.as_str()) {
                    Some("qam") => Btn::Qam,
                    Some("exithold") => Btn::ExitHold,
                    Some("guide") => Btn::Guide,
                    _ => return true,
                };
                self.input(b, false);
            }
            "freeze-full" => self.ask_paused(payload),
            _ => return false,
        }
        true
    }

    fn on_overlay(&mut self, p: &serde_json::Value, now: f64) {
        let open = p.get("open").and_then(|v| v.as_bool()).unwrap_or(false);
        if !open {
            let home = p.get("home").and_then(|v| v.as_bool()).unwrap_or(false);
            self.ingame_teardown();
            if home {
                self.st.close();
                self.st.options = None;
                self.st.set_tab(Tab::Home, false);
            }
            self.journey.probe.ingame(None, None);
            return;
        }
        let mode = p.get("mode").and_then(|v| v.as_str()).unwrap_or("menu").to_string();
        let rid = p.get("gameId").and_then(|v| v.as_str()).map(String::from);
        let frame = p.get("frame").and_then(|v| v.as_str()).map(PathBuf::from);
        // An Android game's frame is kept for its touch controls editor.
        crate::features::android::on_overlay(rid.as_deref(), frame.as_deref());
        match self.ingame.as_mut() {
            Some(ig) => {
                ig.mode = mode.clone();
                ig.closing = false;
            }
            None => {
                let host = wrapper(&mut self.scene, BACKDROP);
                let bar = self.footer.view.host();
                self.scene.n(bar).opacity.snap(0.0);
                {
                    let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                    crate::screens::overlay::common::frozen(&mut cx, host, frame.clone().map(frame_src));
                }
                self.ingame = Some(Ingame { rid: rid.clone(), frame, mode: mode.clone(), backdrop: host, closing: false, titled: false });
            }
        }
        log::info!("over the game: {mode} for {}", rid.as_deref().unwrap_or("-"));
        self.show_ingame(now);
        self.journey.probe.ingame(Some(&mode), rid.as_deref());
    }

    /// Shows the session's surface (the crate switches the mode in place).
    fn show_ingame(&mut self, _now: f64) {
        let Some(mode) = self.ingame.as_ref().map(|i| i.mode.clone()) else { return };
        let titled = self.ingame.as_ref().and_then(|i| i.rid.as_deref()).is_some_and(|r| self.st.game(r).is_some());
        if let Some(ig) = self.ingame.as_mut() {
            ig.titled = titled;
        }
        self.close_legacy();
        self.st.osk = None;
        match mode.as_str() {
            "quick" => {
                self.st.options = None;
                self.st.open(Some(Overlay::Qam));
                self.fx.fb(Feedback::Open);
            }
            "switcher" => {
                self.ingame_drop_sheets();
                self.open_switcher();
            }
            "power" => {
                self.ingame_drop_sheets();
                self.open_power_over();
            }
            // During first run the Rare button over Steam (opened to sign in) is "back to setup",
            // as the Steam step says: Steam stays open under Rare.
            _ if crate::features::firstrun::in_setup() => {
                self.ingame_drop_sheets();
                self.ingame_leave();
                self.st.open(Some(Overlay::Other("firstrun")));
            }
            _ => {
                self.ingame_drop_sheets();
                self.open_rare_menu();
            }
        }
    }

    fn ingame_drop_sheets(&mut self) {
        self.st.options = None;
        if matches!(self.st.overlay, Some(Overlay::Qam)) {
            self.st.open(None);
        }
    }

    fn open_power_over(&mut self) {
        let now = clock::secs();
        let Some((rid, frame)) = self.ingame_session() else { return };
        let session = crate::screens::overlay::fixtures::Session { game_id: rid, frame, guest: false };
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(crate::screens::overlay::power::PowerMenu::build(&mut cx, &session, now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
        self.power_up = true;
    }

    /// Fixtures (shots, UX journeys) have no rare-overlay to see the Rare button over a game
    /// first: its focus keeper's decision, carried out the way its event would. True when taken.
    pub(super) fn ingame_fixture_press(&mut self, b: Btn) -> bool {
        use rare_overlay::focus::{on_press, Press, Step, Surface};
        if !self.fixture || !matches!(b, Btn::Guide | Btn::GuideHold) {
            return false;
        }
        let Some(ig) = self.ingame.as_ref() else { return false };
        let open = Some(match ig.mode.as_str() {
            "menu" => Surface::Menu,
            "switcher" => Surface::Switcher,
            _ => Surface::Other,
        });
        let now = clock::secs();
        match on_press(open, true, if b == Btn::Guide { Press::Rare } else { Press::Hold }) {
            Step::Home => self.on_overlay(&serde_json::json!({ "open": false, "home": true }), now),
            Step::Show(s) => {
                let mode = if s == Surface::Switcher { "switcher" } else { "menu" };
                let (rid, frame) = (ig.rid.clone(), ig.frame.clone());
                self.on_overlay(&serde_json::json!({ "open": true, "gameId": rid, "frame": frame, "mode": mode }), now);
            }
            _ => {}
        }
        true
    }

    /// Leaves the game for the launcher (it stays running, frozen when its decision says so):
    /// what a Rare menu row that lives in the launcher does over a game. False when no game is
    /// under the menu.
    pub(super) fn ingame_leave(&mut self) -> bool {
        if self.ingame.is_none() {
            return false;
        }
        self.ingame_closing();
        self.close_legacy();
        std::thread::spawn(|| {
            let from = rare_overlay::under();
            let _ = rare_overlay::close_for(false);
            rare_overlay::switcher::apply(from.as_deref(), None);
        });
        true
    }

    /// The power sheet (Sleep, Restart, Turn off) over a game or over the launcher.
    pub(super) fn open_power_sheet(&mut self) {
        let now = clock::secs();
        let (rid, frame) = self.ingame_session().unwrap_or((None, None));
        let session = crate::screens::overlay::fixtures::Session { game_id: rid, frame, guest: false };
        self.close_legacy();
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(crate::screens::overlay::power::PowerMenu::build(&mut cx, &session, now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
        self.power_up = true;
    }

    /// The session ended (back to the game, or Home): everything it showed goes.
    fn ingame_teardown(&mut self) {
        let Some(ig) = self.ingame.take() else { return };
        self.close_legacy();
        self.ingame_drop_sheets();
        self.scene.remove(ig.backdrop);
        let bar = self.footer.view.host();
        self.scene.n(bar).opacity.snap(1.0);
    }

    /// Back to the game (Resume, B): the crate gives the screen and the pad back, then sends the
    /// close. The surface stays up until then, so the launcher never shows in between.
    pub(super) fn ingame_resume(&mut self) {
        if let Some(ig) = self.ingame.as_mut() {
            ig.closing = true;
        }
        self.fx.fb(Feedback::Back);
        std::thread::spawn(|| {
            if let Err(e) = rare_overlay::close() {
                log::warn!("overlay close: {e}");
            }
        });
    }

    pub(super) fn ingame_closing(&mut self) {
        if let Some(ig) = self.ingame.as_mut() {
            ig.closing = true;
        }
    }

    /// Every update: Quick settings opened from the menu closed (back to the menu), or Quick
    /// settings over the game closed (back to the game).
    pub(super) fn ingame_tick(&mut self) {
        let Some(ig) = self.ingame.as_ref() else { return };
        // The library arrived after the menu was built: the Game tab gets the game's card.
        if !ig.closing && !ig.titled && (ig.mode == "menu" || ig.mode == "switcher") && self.legacy.is_some() && !self.paused_up && ig.rid.as_deref().is_some_and(|r| self.st.game(r).is_some()) {
            self.show_ingame(clock::secs());
            return;
        }
        // A sheet Ask Rare opened (install, an options sheet, the keyboard, a review): the menu comes
        // back when it is gone.
        if ig.closing || self.legacy.is_some() || self.st.overlay.is_some() || self.share.is_some() || self.st.options.is_some() || self.st.osk.is_some() || crate::screens::wallet::sign::host::active() {
            return;
        }
        if ig.mode == "menu" {
            self.open_rare_menu();
        } else {
            self.ingame_resume();
        }
    }

    /// A press on the Rare menu or the Switcher over a game. True when it was the session's.
    pub(super) fn ingame_out(&mut self, out: &crate::screens::overlay::Out) -> bool {
        use crate::screens::overlay::Out;
        if self.ingame.is_none() {
            return false;
        }
        match out {
            Out::Close => {
                self.ingame_resume();
                true
            }
            Out::Act(a) => self.ingame_act(a),
            _ => false,
        }
    }

    pub(super) fn ingame_act(&mut self, a: &str) -> bool {
        if self.journey.probe.on() {
            self.journey_act(&format!("ingame:{a}"));
        }
        let rid = self.ingame.as_ref().and_then(|i| i.rid.clone());
        match a {
            "resume" => self.ingame_resume(),
            "quit" => {
                let Some(rid) = rid else { return false };
                self.ingame_closing();
                let name = self.st.run_name(&rid);
                self.fx.fb(Feedback::Back);
                self.fx.spawn(move |b| {
                    let r = b.quit(&rid);
                    // The game is gone: the launcher keeps the screen.
                    let _ = rare_overlay::close();
                    let running = b.running();
                    Box::new(move |st: &mut State, now| {
                        st.set_running(running);
                        if let Err(e) = r {
                            log::warn!("quit {rid}: {e}");
                            st.toast(format!("Couldn't close {name}."), Tone::Error, now);
                        }
                    })
                });
            }
            // A Market tab row: the purchase of that listing, reviewed and signed in Rare Wallet.
            a if a.starts_with("market:") => self.menu_market_act(a),
            "details" => {
                let Some(rid) = rid else { return false };
                // Leaves the game for its page (it is frozen when its decision says so).
                self.ingame_closing();
                self.close_legacy();
                std::thread::spawn(|| {
                    let from = rare_overlay::under();
                    let _ = rare_overlay::close_for(false);
                    rare_overlay::switcher::apply(from.as_deref(), None);
                });
                self.st.open(Some(Overlay::Game(rid)));
            }
            "performance" => {
                // Quick settings inside the menu; B comes back to the menu (ingame_tick).
                self.close_legacy();
                self.st.open(Some(Overlay::Qam));
                self.fx.fb(Feedback::Open);
            }
            "screenshot" => {
                // A screenshot of the game, not of the menu: back to it first.
                self.ingame_resume();
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(350));
                    crate::side::capture(false);
                });
            }
            "save-clip" => crate::side::capture(true),
            "discord-mute" | "discord-deafen" => {
                let a = if a == "discord-mute" { "discord:mute" } else { "discord:deafen" };
                if let Some((msg, err)) = crate::features::capture::act(a, None).filter(|m| !m.0.is_empty()) {
                    self.st.toast(msg, if err { Tone::Error } else { Tone::Info }, clock::secs());
                }
            }
            a if a.starts_with("emu:") => {
                let Some(rid) = rid else { return false };
                let id = &a["emu:".len()..];
                let Some(action) = self.st.game_perf.emu.iter().find(|e| e.app_id == rid && e.id == id).cloned() else { return false };
                // Keyboard rows need the game in front; rare-emu closes the overlay first.
                self.ingame_closing();
                self.fx.spawn(move |b| {
                    let r = b.emu_do(&action);
                    let _ = rare_overlay::close();
                    Box::new(move |st: &mut State, now| match r {
                        Ok(()) => {
                            let done = match action.id.split(':').next().unwrap_or("") {
                                "save" => Some("State saved"),
                                "load" => Some("State loaded"),
                                "disc" => Some("Next disc in"),
                                _ => None,
                            };
                            if let Some(t) = done {
                                st.toast(t, Tone::Info, now);
                            }
                        }
                        Err(e) => {
                            log::warn!("emu {}: {e}", action.id);
                            st.toast(crate::screens::settings::live::failed("Couldn't do that.", &e, None), Tone::Error, now);
                        }
                    })
                });
            }
            // A trophy row is for reading (the web's rows have no action either).
            a if a.starts_with("trophies:") => {}
            "steam-menu" => {
                self.ingame_closing();
                std::thread::spawn(|| {
                    if let Err(e) = rare_overlay::steam_menu() {
                        log::warn!("steam menu: {e}");
                        crate::screens::settings::live::toast_pub(crate::screens::settings::live::failed("Couldn't open Steam's overlay.", &e, None), true);
                    }
                });
            }
            a if a.starts_with("steam-page:") => {
                let page = a["steam-page:".len()..].to_string();
                std::thread::spawn(move || {
                    if let Err(e) = rare_overlay::steampages::open_page(&page) {
                        log::warn!("steam page {page}: {e}");
                        crate::screens::settings::live::toast_pub(crate::screens::settings::live::failed("Couldn't open that Steam page.", &e, None), true);
                    }
                });
            }
            "suspend" | "reboot" | "poweroff" => {
                let action = a.to_string();
                self.ingame_closing();
                self.fx.spawn(move |b| {
                    let _ = rare_overlay::close();
                    let r = b.power(&action);
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(e, Tone::Error, now);
                        }
                    })
                });
            }
            // The Switcher over a game (switcher_switch closes the surface and leaves the game
            // under it, frozen when its decision says so).
            "home" => {
                self.ingame_closing();
                std::thread::spawn(|| {
                    let _ = rare_overlay::switcher::switcher_switch(None);
                });
            }
            a if a.starts_with("switch:") => {
                let to = a["switch:".len()..].to_string();
                self.ingame_closing();
                self.fx.fb(Feedback::Launch);
                self.fx.spawn(move |b| {
                    let r = rare_overlay::switcher::switcher_switch(Some(to.clone())).and_then(|()| b.launch(&to));
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            log::warn!("switch: {e}");
                            st.toast("Couldn't switch to it. Try again.", Tone::Error, now);
                        }
                    })
                });
            }
            a if a.starts_with("launch:") => {
                let id = a["launch:".len()..].to_string();
                self.ingame_closing();
                self.fx.spawn(move |_| {
                    let _ = rare_overlay::switcher::switcher_switch(None);
                    Box::new(move |st: &mut State, _| st.retry = Some(id))
                });
            }
            _ => return false,
        }
        true
    }

    /// Two games are paused already and a third was left: which paused game to close
    /// (Switcher.tsx "Close a paused game?").
    fn ask_paused(&mut self, p: &serde_json::Value) {
        use crate::screens::overlay::switcher::model::{FreezeFull, RunItem};
        let pending = p.get("pending").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let frozen: Vec<String> = p.get("frozen").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        if pending.is_empty() || frozen.is_empty() {
            return;
        }
        let ask = FreezeFull { pending, frozen };
        let all: Vec<RunItem> = self.st.running.iter().map(|rid| RunItem { rid: rid.clone(), name: self.st.run_name(rid), app: crate::screens::apps::catalog::app_by_run_id(rid).is_some(), art: None, icon: None }).collect();
        let now = clock::secs();
        self.close_legacy();
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(crate::screens::overlay::switcher::PausedFull::build(&mut cx, &ask, &all, now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
        self.paused_up = true;
    }

    /// "Close a paused game?": keep it running, or close one of the paused ones.
    pub(super) fn paused_act(&mut self, a: &str) -> bool {
        if a.starts_with("keep:") {
            rare_overlay::switcher::freeze_keep_pending();
            return true;
        }
        if let Some(rid) = a.strip_prefix("quit:") {
            let rid = rid.to_string();
            self.fx.spawn(move |b| {
                let r = b.quit(&rid);
                let running = b.running();
                Box::new(move |st: &mut State, now| {
                    st.set_running(running);
                    if let Err(e) = r {
                        st.toast(e, Tone::Error, now);
                    }
                })
            });
            return true;
        }
        false
    }
}

impl App {
    /// The Game tab's card for a run: title, banner and logo, whether it is an app, whether the
    /// library has it (Game details).
    pub(super) fn menu_game(&self, rid: &str) -> (crate::screens::overlay::menu::GameInfo, bool, bool) {
        use crate::screens::overlay::menu::GameInfo;
        let first = |v: &[String]| v.iter().find_map(|s| crate::data::art::src_of(s));
        let app = crate::screens::apps::catalog::app_by_run_id(rid);
        match self.st.game(rid) {
            Some(g) => (GameInfo { title: g.title.clone(), banner: first(&g.art.hero).or_else(|| first(&g.art.header)), logo: first(&g.art.logo) }, app.is_some(), true),
            None => (GameInfo { title: self.st.run_name(rid), banner: app.and_then(|a| crate::data::art::src_of(&a.icon)), logo: None }, app.is_some(), false),
        }
    }
}

impl App {
    /// `market:<row>` from the Market tab.
    pub(super) fn menu_market_act(&mut self, a: &str) {
        let id = a["market:".len()..].parse::<usize>().ok().and_then(|i| crate::screens::overlay::panels::data().market.and_then(|m| m.get(i).map(|x| x.id.clone())));
        if let Some(id) = id {
            self.menu_buy(id);
        }
    }

    /// Buys a listed item from the Market tab: builds it from the item's listing and hands it to
    /// Rare Wallet's review. The menu stays up; the result comes back as a toast.
    pub(super) fn menu_buy(&mut self, id: String) {
        use rare_market::api::{self, ItemDetail};
        let Some((address, _)) = (!self.fixture).then(crate::data::account::current).flatten() else {
            self.st.toast(crate::sign::NO_SIGNER, Tone::Info, clock::secs());
            return;
        };
        self.st.toast("Preparing the purchase...", Tone::Info, clock::secs());
        self.fx.spawn(move |_| {
            let net = crate::screens::wallet::live::market_net();
            let r: Result<String, String> = (|| {
                let d = api::get::<ItemDetail>(net, &api::paths::item(&id)).map_err(|_| "Couldn't read that item. Try again.".to_string())?;
                let listing = d.card.listing.clone().ok_or_else(|| "This item is not listed anymore.".to_string())?;
                let tx = rare_market::buy::build_buy_transaction(net, &Default::default(), &d.card, &listing, &address, None, None).map_err(|e| if e.0.is_empty() { "This item can't be bought here.".to_string() } else { e.0 })?;
                crate::sign::sign_and_execute(tx, &address, &format!("Buy {}", d.card.name)).map(|_| d.card.name)
            })();
            Box::new(move |st: &mut State, now| match r {
                Ok(name) => {
                    st.toast(format!("Bought {name}."), Tone::Info, now);
                    crate::screens::overlay::menu::live::request_reload();
                }
                Err(e) if e == rare_market::inventory::notice::SIGN_REJECTED => {}
                Err(e) => st.toast(e, Tone::Error, now),
            })
        });
    }

    /// The Game tab's live parts for a run: how long it has played, and a Sui game's collections
    /// (its Items and Market tabs).
    pub(super) fn menu_game_data(&self, mut d: crate::screens::overlay::menu::MenuData) -> crate::screens::overlay::menu::MenuData {
        let Some(rid) = d.rid.clone().filter(|_| d.over_game) else { return d };
        if !self.fixture {
            d.started_ms = crate::data::backend::runner().elapsed(&rid).map(|e| d.now_ms - e.as_millis() as f64);
            d.colls = crate::screens::overlay::menu::live::collections_of(&rid, self.st.game(&rid), &self.st.sui_games);
        }
        d
    }

    /// The Game tab's Performance line and emulator rows, and a Steam game's Trophies tab (its
    /// achievements on this device, as the trophy case reads them).
    pub(super) fn menu_game_extras(&mut self, mut d: crate::screens::overlay::menu::MenuData) -> crate::screens::overlay::menu::MenuData {
        use crate::screens::overlay::menu::{EmuAction, Perf};
        use crate::screens::overlay::panels;
        panels::set_pending(None);
        let Some(rid) = d.rid.clone().filter(|_| d.over_game && !self.fixture) else { return d };
        let gp = &self.st.game_perf;
        d.perf = gp.perf.as_ref().map(|p| Perf { profile: p.profile.clone(), tdp: p.tdp });
        d.power_saved = gp.power.as_ref().is_some_and(|(g, saved)| *saved && *g == rid);
        d.emu = gp
            .emu
            .iter()
            .filter(|a| a.app_id == rid)
            .map(|a| EmuAction {
                id: a.id.clone(),
                label: a.label.clone(),
                ready: a.ready,
                note: a.note.clone(),
                thumb: a.thumb.as_ref().map(|t| rare_render::Src::File(t.into())),
                sub: a.saved.map(crate::features::emulation::ago),
            })
            .collect();
        // Every running game (not a Sui game's items menu): its whole trophy case, the store's
        // achievements and Rare's own trophies.
        let eligible = self.st.game(&rid).is_some_and(crate::features::trophy_case::eligible) || rid.starts_with("steam:");
        if eligible && d.colls.is_empty() {
            let title = d.game.title.clone();
            d.trophies = true;
            match crate::features::trophies::case_rows(&self.st, &rid) {
                Some(rows) => {
                    let mut pd = panels::live_data(&title, panels::Account::SignedOut, None, None);
                    pd.rare = (!rows.is_empty()).then(|| menu_trophies(&rows));
                    d.trophies_count = pd.rare.as_ref().map(|r| r.sum.unlocked as u32).unwrap_or(0);
                    panels::set_data(pd);
                    panels::set_pending(None);
                }
                None => {
                    // Still reading the Steam achievements: the tab fills in when they land, with
                    // Rare's own trophies after them.
                    let ms = crate::features::trophies::milestone_rows(&self.st, &rid);
                    let appid = rid.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).unwrap_or(0);
                    let data = move || {
                        let rows = crate::features::trophies::rows_for(appid)?;
                        let all: Vec<_> = rows.unwrap_or_default().into_iter().chain(ms.iter().cloned()).collect();
                        let mut pd = panels::live_data(&title, panels::Account::SignedOut, None, None);
                        pd.rare = (!all.is_empty()).then(|| menu_trophies(&all));
                        Some(pd)
                    };
                    let mut pd = panels::live_data(&d.game.title, panels::Account::SignedOut, None, None);
                    pd.trophies_ready = false;
                    panels::set_data(pd);
                    panels::set_pending(Some(Box::new(data)));
                }
            }
        }
        d
    }
}

/// A game's trophy rows (store achievements and Rare's own) as the Rare menu's Trophies tab draws them.
fn menu_trophies(rows: &[crate::screens::trophies::model::TrophyRow]) -> crate::screens::overlay::panels::RareGame {
    use crate::screens::overlay::panels::trophies as pt;
    use crate::screens::trophies::model as tm;
    let rows: Vec<pt::TrophyRow> = rows
        .iter()
        .map(|r| pt::TrophyRow {
            key: r.key.clone(),
            name: r.name.clone(),
            description: r.description.clone(),
            icon_url: r.icon_url.clone(),
            icon_gray_url: r.icon_gray_url.clone(),
            masked: r.masked,
            state: match r.state {
                tm::RowState::Minted => pt::RowState::Minted,
                tm::RowState::Unlocked => pt::RowState::Unlocked,
                tm::RowState::Locked => pt::RowState::Locked,
            },
            at: r.at,
            tier: r.tier,
            points: r.points,
            rarity: r.rarity,
            rarity_from: r.rarity_from.map(|f| match f {
                tm::RarityFrom::Rare => pt::RarityFrom::Rare,
                tm::RarityFrom::Steam => pt::RarityFrom::Steam,
            }),
            has_def: r.def.is_some(),
            source: None,
        })
        .collect();
    let art = rows
        .iter()
        .filter_map(|r| {
            let url = if r.state == pt::RowState::Locked { r.icon_gray_url.as_ref().or(r.icon_url.as_ref()) } else { r.icon_url.as_ref() }?;
            Some((r.key.clone(), rare_render::Src::Url(url.clone())))
        })
        .collect();
    crate::screens::overlay::panels::RareGame { sum: pt::summarize(&rows), rows, art }
}

impl App {
    /// A on an app tile (apps/tiles.tsx): resume it, open it through rare-launch, or (nothing can
    /// open it yet, or `page`: Menu on the tile) the app's page with its ways, install and Close.
    pub(super) fn open_app(&mut self, id: &str, page: bool) {
        use crate::screens::apps::state::Activation;
        let (Some(apps), Some(app)) = (crate::features::apps::shared(), crate::screens::apps::app_by_id(id)) else { return };
        let act = if page { Activation::Page } else { apps.borrow_mut().activate(app) };
        match act {
            Activation::Resume | Activation::Open => self.fx.fb(Feedback::Launch),
            Activation::Page => {
                let now = clock::secs();
                self.close_legacy();
                let root = self.scene.root;
                let before = self.scene.children(root);
                let screen = {
                    let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                    crate::screens::apps::AppSheet::build(&mut cx, apps.clone(), id, now)
                };
                let Some(screen) = screen else { return };
                let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
                for n in &nodes {
                    self.scene.n(*n).z = layer::SHEET + 5;
                }
                self.fx.fb(Feedback::Open);
                self.legacy = Some((Box::new(screen), nodes));
            }
        }
    }
}
