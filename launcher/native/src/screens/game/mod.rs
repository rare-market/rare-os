//! The game page (screens/GameDetail.tsx): key art over the top 72 %, plain scrims,
//! the status line, the logo, Play and the store's actions, then activity cards. It rises in over
//! everything but the hint bar. X toggles favourite; Menu opens Options.

pub mod extras;
pub mod extras_data;
pub mod knowledge;
pub mod launch;
pub mod manage;
pub mod panels;
pub mod parts;
pub mod saves;
pub mod steam_cloud;
pub mod settings;
pub mod stores;

use crate::act::Act;
use crate::data::model::{fmt_ago, pretty_path, source_label};
use crate::kit::format::fmt_size;
use crate::kit::{self, s, tok, Btn as B, BtnIcon, DotTone, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{State, Tone};
use parts::{hero_logo, Variant};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::scene::{GradDir, Gradient};
use rare_render::view::{El, Look, Motion};

pub struct GameDetail {
    pub id: String,
    shared: std::sync::Arc<panels::Shared>,
    cloud: steam_cloud::Cloud,
    settings: std::sync::Arc<settings::Shared>,
    /// Screenshots, news, DLC and the manual under the hero (extras.rs).
    extras: extras::Extras,
    asked: bool,
    /// Performance, Check files, Rename and Art (manage.rs).
    manage: manage::Manage,
    /// HowLongToBeat, reviews and Runs on SuiPlay (knowledge.rs).
    know: knowledge::Knowledge,
    /// The copies under "Play on" at the last build (data/groups.rs), by their code's index.
    copy_ids: Vec<String>,
    /// Opened from another copy's "Play on": no rise, and the focus stays on the copies.
    switched: bool,
    rise: bool,
    want: Option<u32>,
    /// The copies' pages this one was reached from through Play on, the last most recent: B goes
    /// back to it.
    back: Vec<String>,
}

/// "Play on" chips: PLAY_ON + the copy's index.
const PLAY_ON: u32 = 80_000;
/// Set by a "Play on" chip (and B back through them): the next page (the other copy) opens in
/// place, with the pages to go back to.
static SWITCHED: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// Opens another copy's page in place, `back` the pages B returns through.
fn switch_to(st: &mut State, id: String, back: Vec<String>) {
    *SWITCHED.lock().unwrap_or_else(|e| e.into_inner()) = Some(back);
    st.open(Some(crate::state::Overlay::Game(id)));
}

impl GameDetail {
    pub fn new(id: String) -> GameDetail {
        let back = SWITCHED.lock().unwrap_or_else(|e| e.into_inner()).take();
        let switched = back.is_some();
        let back = back.unwrap_or_default();
        GameDetail { id, shared: Default::default(), cloud: Default::default(), settings: Default::default(), extras: Default::default(), asked: false, manage: Default::default(), know: Default::default(), copy_ids: Vec::new(), switched, rise: !switched, want: None, back }
    }
}

/// "49.2 h" (GameDetail.tsx fmtPlaytime).
fn playtime(m: Option<i64>) -> String {
    match m.filter(|m| *m > 0) {
        None => String::new(),
        Some(m) if m < 60 => format!("{m} min"),
        Some(m) => format!("{} h", (m as f64 / 6.0).round() / 10.0),
    }
}

impl Page for GameDetail {
    fn stamp(&self, st: &State) -> u64 {
        let slow = st.starts.get(&self.id).is_some_and(|s| rare_render::clock::secs() - s.since >= 15.0);
        hash((&self.id, st.ver.library, st.ver.running, st.ver.favs, st.ver.starts, self.shared.ver(), self.cloud.ver(), self.settings.ver(), self.extras.ver(), (slow, crate::features::cloudsync::ver()), self.manage.stamp(), (st.feat.downloads.ver, crate::features::stores::ver(), self.know.ver(), if self.id.starts_with("emu:") { crate::features::emulation::ver() } else { 0 })))
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, now: f64) -> Option<f64> {
        if !self.asked {
            if let Some(g) = st.game(&self.id) {
                self.asked = true;
                panels::load(&self.shared, fx, g);
                steam_cloud::load(&self.cloud, fx, g);
                settings::load(&self.settings, fx, g);
                self.extras.load(st, fx, g);
                self.know.load(fx, g);
                crate::features::cloudsync::load(st, g);
            }
        }
        let mut manage = None;
        if let Some(g) = st.game(&self.id).cloned() {
            settings::answers(&self.settings, st, fx, &g);
            manage = self.manage.tick(st, fx, &g, now);
        }
        // The Steam start line says "it is updating" after 15 s.
        let start = st.starts.get(&self.id).filter(|s| s.since + 15.0 > now).map(|s| s.since + 15.0);
        match (start, manage) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        if (PLAY_ON..PLAY_ON + 64).contains(&code) {
            // Another copy of the game: its page in place of this one.
            // The lit chip is this page's copy: say so rather than do nothing.
            if self.copy_ids.get((code - PLAY_ON) as usize) == Some(&self.id) {
                let copies = st.copies(&self.id);
                let label = crate::data::groups::copy_labels(&copies).into_iter().nth((code - PLAY_ON) as usize).unwrap_or_default();
                st.toast(format!("This page is the {label} copy. Pick another to switch."), Tone::Info, rare_render::clock::secs());
                return;
            }
            if let Some(id) = self.copy_ids.get((code - PLAY_ON) as usize).filter(|id| **id != self.id).cloned() {
                fx.fb(Feedback::Move);
                // B comes back here; a copy already on the way back ends the trail there.
                let mut back = self.back.clone();
                match back.iter().position(|b| *b == id) {
                    Some(i) => back.truncate(i),
                    None => back.push(self.id.clone()),
                }
                switch_to(st, id, back);
            }
            return;
        }
        if (knowledge::BASE..knowledge::END).contains(&code) {
            if let Some(g) = st.game(&self.id).cloned() {
                self.know.act(st, fx, &g, code);
            }
            return;
        }
        if code == saves::ACT_SAVES {
            if let Some(g) = st.game(&self.id).cloned() {
                saves::open(st, &g);
            }
            return;
        }
        if code >= extras::FIRST {
            if let Some(g) = st.game(&self.id).cloned() {
                self.extras.act(st, fx, &g, code);
            }
            return;
        }
        if code >= manage::BASE {
            if let Some(g) = st.game(&self.id).cloned() {
                self.manage.act(st, fx, &g, code);
            }
            return;
        }
        if code >= settings::FIRST {
            if let Some(g) = st.game(&self.id).cloned() {
                settings::act(&self.settings, st, fx, &g, code);
            }
            return;
        }
        if let Some(g) = st.game(&self.id).cloned() {
            panels::act(&self.shared, st, fx, &g, code);
        }
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let Some(g) = st.game(&self.id) else { return ui.bx(s().fill()).bg(tok::BG) };
        // Copies of one game share art, title, last played and playtime (data/groups.rs).
        let g = st.shown(g);
        let copies = st.copies(&g.id);
        if let Some(cover) = self.extras.cover(ui, acts, g) {
            return cover;
        }
        let running = st.is_running(&g.id);
        let fav = st.is_fav(&g.id);
        let w = ui.w / ui.rem;
        let h = ui.h / ui.rem;
        // The hero, then the header when no hero link loads (a CDN 404).
        let hero_src: Vec<String> = g.art.hero.iter().chain(g.art.header.iter()).cloned().collect();
        let art = parts::art(ui, &hero_src, &g.title, Variant::Hero, w, h * 0.72, 0.25);
        let bg = tok::BG;
        let mut status = ui.bx(s().row().items_center().gap(0.5));
        if running {
            status = status.child(kit::dot(ui, s(), DotTone::Primary, true));
        }
        status = status.child(ui.text(if running { "Now playing" } else { source_label(&g.source) }, T::new(Ty::Small).w(500).c(tok::fg(0.75))));
        if !g.installed && !running {
            status = status.child(ui.text("\u{b7} Not installed", T::new(Ty::Small).w(500).c(tok::fg(0.5))));
        }
        if fav {
            status = status.child(ui.text("\u{b7} Favourite", T::new(Ty::Small).w(500).c(tok::fg(0.5))));
        }
        // Epic and GOG games Rare manages (rare-stores): Install before, Update and the rest after.
        let store = stores::managed(g);
        if store && !g.installed && !running {
            status = status.child(ui.text("\u{b7} Not installed", T::new(Ty::Small).w(500).c(tok::fg(0.5))));
        }
        // Steam Deck and ProtonDB, after everything else on the line so nothing moves when they arrive.
        for t in settings::compat_texts(&self.settings) {
            status = status.child(ui.text(format!("\u{b7} {t}"), T::new(Ty::Small).w(500).c(tok::fg(0.5))));
        }
        let logo = hero_logo(ui, g, true);
        // Play on: each copy of the game (Steam, the cloud, a ROM file), this one lit.
        self.copy_ids = copies.iter().map(|c| c.id.clone()).collect();
        let play_on = (copies.len() > 1).then(|| {
            let labels = crate::data::groups::copy_labels(&copies);
            let chips: Vec<El> = copies
                .iter()
                .zip(labels)
                .enumerate()
                .map(|(i, (c, label))| {
                    let tag = acts.game(Act::Local(PLAY_ON + i as u32), &c.id);
                    let on = c.id == g.id;
                    if on && self.switched {
                        self.want = Some(tag);
                    }
                    let b = B::new(label, tag).sm().hint("Play on");
                    kit::button(ui, if on { b.primary() } else { b }).key(rare_render::view::key(&("copy", &c.id)))
                })
                .collect();
            ui.bx(s().row().items_start().gap(0.75).mt(0.2))
                .child(ui.text_s(s().mt(0.45), "Play on", T::new(Ty::Small).w(500).c(tok::fg(0.6))))
                .child(ui.bx(s().row().wrap().items_center().gap(0.5).shrink().min0()).group(Group::new("game-copies", [0.0; 4]).remember()).kids(chips))
        });
        self.switched = false;
        let play_label = if running { "Resume" } else if g.source == "apps" { "Open" } else { "Play" };
        // A Steam game being installed or updated: Pause or Resume; not installed: Install.
        let main = if running { None } else { crate::features::downloads::game_button(ui, st, acts, g) };
        let mut actions = if store && !g.installed && !running {
            stores::not_installed(ui, acts, g)
        } else {
            vec![main.unwrap_or_else(|| kit::button(ui, B::new(play_label, acts.game(Act::Launch(g.id.clone()), &g.id)).primary().lg().def(true).hint(play_label).icon(BtnIcon::Lucide("play", 1.1, true)).min_w(10.0).game()))]
        };
        if running {
            let quit = if g.source == "apps" { "Close" } else { "Quit game" };
            actions.push(kit::button(ui, B::new(quit, acts.game(Act::QuitGame(g.id.clone()), &g.id)).lg().hint(quit).game()));
        }
        // Managing the game sits right after Play, in the open: Move (to the SD card or back) and
        // Uninstall. Options (Menu) has them too.
        let can_move = g.installed && !running && matches!(g.source.as_str(), "steam" | "epic" | "gog" | "local" | "itch");
        if can_move {
            actions.push(kit::button(ui, B::new("Move", acts.game(Act::Legacy(format!("storage:move-game:{}", g.id)), &g.id)).lg().hint("Move to another drive").game()));
        }
        let uninstall = crate::features::uninstall::can_uninstall(g) && !running;
        if uninstall {
            let label = if g.source == "web" { "Remove".to_string() } else { crate::features::uninstall::label(g) };
            actions.push(kit::button(ui, B::new(label, acts.game(Act::Uninstall(g.id.clone()), &g.id)).lg().game()));
        }
        if !running {
            actions.extend(crate::features::downloads::update_now_button(ui, st, acts, g));
        }
        if crate::features::downloads::of(st, &g.id).is_some() || crate::features::stores::job(&g.id).is_some() {
            actions.push(kit::button(ui, B::new("Downloads", acts.game(Act::Open(crate::state::Overlay::Other(crate::features::downloads::KIND)), &g.id)).lg().game()));
        }
        if store && g.installed {
            actions.extend(stores::installed(ui, acts, g, running));
        }
        if g.foreign {
            let lib = g.origin.trim_start_matches("Steam library on ").to_string();
            actions.push(kit::button(ui, B::new("Add library to Steam", acts.game(Act::SteamAddLibrary(lib), &g.id)).lg().game()));
        }
        // Trophies follow the game: a copy outside Steam shows its Steam copy's; every other game
        // has Rare's own (first launch, hours, ...) with its store's achievements.
        let steam_copy = if g.source != "steam" { copies.iter().find_map(|c| c.id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).filter(|a| *a < 0x8000_0000)) } else { None };
        if let Some(appid) = steam_copy {
            actions.push(kit::button(ui, B::new("Trophies", acts.game(Act::Trophies(Some(appid)), &g.id)).lg().game()));
        } else if crate::features::trophy_case::eligible(g) {
            actions.push(kit::button(ui, B::new("Trophies", acts.game(Act::Trophies(Some(crate::features::trophy_case::case_id(&g.id))), &g.id)).lg().game()));
        }
        if g.source == "steam" && g.id.starts_with("steam:") {
            let appid = g.id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok());
            let open = match appid {
                Some(appid) => Act::SteamGamePage(appid),
                None => Act::SteamBigPicture,
            };
            actions.push(kit::button(ui, B::new("Open in Steam", acts.game(open, &g.id)).lg().game()));
            // Non-Steam shortcuts have big ids that are not store apps.
            if let Some(appid) = appid.filter(|a| *a < 0x8000_0000) {
                actions.push(kit::button(ui, B::new("Store page", acts.game(Act::SteamStore(appid), &g.id)).lg().hint("Open the Steam store page").game()));
            }
        }
        // Android games: pad buttons as screen touches, for games made for touch only.
        if g.source == "android" && !rare_android::is_store(&g.id) {
            let n = rare_android::touch::map(rare_android::package_of(&g.id).unwrap_or_default()).len();
            let label = if n == 0 { "Touch controls".to_string() } else { format!("Touch controls ({n})") };
            actions.push(kit::button(ui, B::new(label, acts.game(Act::Legacy(format!("android:touch:{}", g.id)), &g.id)).lg().hint("Buttons as screen touches").game()));
        }
        let row = ui.bx(s().row().gap(0.75).mt(0.4)).group(Group::new("game-actions", [0.0; 4]).remember()).kids(actions);
        // Activity cards: one fact per card.
        let mut info: Vec<El> = Vec::new();
        if let Some((bad, text)) = crate::screens::sui::catalog::anti_cheat(&g.id) {
            let dot = kit::dot(ui, s().mt(0.45), if bad { DotTone::Attention } else { DotTone::Muted }, false);
            info.push(ui.bx(s().row().gap(0.6).max_w(52.0)).child(dot).child(ui.text(text, T::new(Ty::Small).c(tok::fg(0.8)).wrap())));
        }
        // A cloud game: what playing it needs, in plain words (a subscription, the game in a store).
        let cloud = g.source == "cloud";
        if let Some(plan) = crate::screens::apps::catalog::cloud_app(&g.id).and_then(|a| a.plan.as_deref()) {
            let dot = kit::dot(ui, s().mt(0.45), DotTone::Muted, false);
            info.push(ui.bx(s().row().gap(0.6).max_w(52.0)).child(dot).child(ui.text(plan, T::new(Ty::Small).c(tok::fg(0.8)).wrap())));
        }
        let origin = pretty_path(&g.origin);
        let facts: Vec<(&str, String, Option<String>)> = vec![
            ("Last played", if g.last_played.is_some() { fmt_ago(g.last_played).trim_start_matches("Played ").to_string() } else { "Not yet".into() }, None),
            ("Playtime", { let p = playtime(g.playtime_minutes); if p.is_empty() { "None yet".into() } else { p } }, None),
            (if g.installed { "Size" } else { "Install size" }, { let z = stores::size_text(g).unwrap_or_else(|| fmt_size(g.size_bytes)); if z.is_empty() && (g.source == "web" || cloud) { "Streamed".into() } else { z } }, match g.source.as_str() { "web" => Some("Plays from the web".to_string()), "cloud" => Some("Plays from the cloud".to_string()), _ => None }),
            ("Store", source_label(&g.source).to_string(), (origin != source_label(&g.source)).then_some(origin)),
        ];
        let mut cards: Vec<El> = facts.into_iter().filter(|f| !f.1.is_empty()).map(|(l, v, sub)| kit::info_card(ui, l, &v, sub.as_deref(), None)).collect();
        cards.extend(steam_cloud::card(ui, &self.cloud, g));
        cards.extend(saves::card(ui, acts, g));
        let cards = kit::card_row(ui, s().mt(0.75), "game-cards", cards);
        let steam_game = g.source == "steam";
        let mut extra: Vec<El> = Vec::new();
        if running && steam_game {
            if let Some(bar) = st.starts.get(&g.id).and_then(|s| panels::start_progress(ui, s, rare_render::clock::secs())) {
                extra.push(bar);
            }
        }
        if !running {
            extra.extend(crate::features::downloads::game_panel(ui, st, g));
        }
        if store {
            extra.push(stores::progress(ui, g));
        }
        extra.extend(info);
        if !running {
            extra.extend(panels::proton(ui, &self.shared, acts, g));
            extra.extend(settings::view(ui, &self.settings, acts, g));
        }
        // Performance, Check files, Rename, Art (manage.rs), under the game's settings.
        if g.source != "cloud" {
            extra.push(self.manage.section(ui, st, acts, g));
        }
        if let Some(e) = st.library.as_ref().filter(|l| l.emulation_app.as_deref() == Some(g.id.as_str())).and_then(|l| l.emulation.as_ref()) {
            extra.push(panels::emulation(ui, acts, e));
        }
        if g.source == "web" {
            extra.push(panels::web_info(ui, &self.shared, acts, g));
        }
        if g.source == "emu" {
            extra.extend(crate::features::emulation::game_panel(ui, acts, g));
        }
        let body = ui.bx(s().col().justify_end().gap(1.0).px(2.5).pb(4.75).w(w).h(h)).child(status).child(logo).maybe(play_on.map(|p| p.key(rare_render::view::key("play-on")))).child(row).kids(extra).child(cards);
        // Under the fold: screenshots, news, DLC, the manual (the page scrolls down to them).
        let below = self.extras.sections(ui, st, acts, g);
        // Runs on SuiPlay after the extras (it lands later; nothing above it moves).
        let below = match (below, self.know.section(ui, acts, g)) {
            (b, None) => b,
            (None, Some(k)) => Some(ui.bx(s().col().pt(0.5).pb(4.75)).child(k)),
            // The extras end in 4.75rem of room: the section sits 2.25rem under them.
            (Some(b), Some(k)) => Some(ui.bx(s().col()).child(b).child(ui.bx(s().col().mt(-2.5).pb(4.75)).child(k))),
        };
        let page = ui.bx(s().fill().col()).scrolls_y(rare_render::view::key(&("game-page", &g.id))).child(body).kids(below.map(|b| {
            // A soft edge where the sections meet the art (over the hero's empty bottom padding).
            let fade = ui.bx(s().h(4.0).wp(1.0)).look(Look::default().grad(Gradient { dir: GradDir::ToBottom, stops: [bg.alpha(0.0), bg.alpha(0.6), bg], mid: 0.5 }));
            ui.bx(s().col().mt(-4.0)).child(fade).child(b.bg(bg))
        }));
        let sheet = self.manage.sheet(ui, acts, g).or_else(|| self.know.sheet(ui, acts, g));
        let root = ui.bx(s().fill()).bg(tok::BG);
        // Opened from another copy's Play on: in place, no rise.
        let root = if self.rise { root.enter(Motion::RISE) } else { root };
        root.child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).hp(0.72)).child(art))
            .child(ui.bx(s().abs_pct(Some(0.4), Some(0.0), None, Some(0.0)).hp(0.325)).look(Look::default().grad(Gradient { dir: GradDir::ToBottom, stops: [bg.alpha(0.0), bg.alpha(0.5), bg], mid: 0.5 })))
            .child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(0.6)).look(Look::default().grad(Gradient { dir: GradDir::ToRight, stops: [bg.alpha(0.9), bg.alpha(0.5), bg.alpha(0.0)], mid: 0.5 })))
            .child(page)
            .kids(sheet)
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if self.manage.pad(fx, b) || self.know.pad(fx, b) {
            return true;
        }
        if let Some(g) = st.game(&self.id).cloned() {
            if self.extras.pad(st, fx, &g, b, cur) {
                return true;
            }
        }
        // B from a copy opened through Play on: back to the copy it came from.
        if b == Btn::B {
            let mut back = self.back.clone();
            let Some(prev) = back.pop() else { return false };
            if st.game(&prev).is_none() {
                return false;
            }
            fx.fb(Feedback::Back);
            switch_to(st, prev, back);
            return true;
        }
        if b != Btn::X {
            return false;
        }
        fx.fb(Feedback::Toggle);
        let on = st.toggle_fav(&self.id);
        st.toast(if on { "Added to favourites" } else { "Removed from favourites" }, Tone::Info, rare_render::clock::secs());
        true
    }

    fn take_focus(&mut self, _cur: Option<&Focusable>) -> Option<u32> {
        self.want.take().or_else(|| self.manage.take_focus()).or_else(|| self.extras.take_focus())
    }

    fn hints(&self, st: &State) -> Vec<Hint> {
        if self.manage.sheet_open() {
            return vec![hint(Key::B, "Close")];
        }
        if let Some(h) = self.know.hints() {
            return h;
        }
        if let Some(h) = self.extras.hints() {
            return h;
        }
        vec![hint(Key::X, if st.is_fav(&self.id) { "Unfavourite" } else { "Favourite" }), hint(Key::Start, "Options"), hint(Key::Rare, "Menu")]
    }
}
