//! Home (screens/Home.tsx): a console home with a trading terminal's live data.
//!
//!   Stage     the focused game full bleed behind the top (the shell's backdrop), its logo and
//!             Play, and Your Rare on the right.
//!   Strip     your games: the focused tile grows 45 % from its corner, the ones after it
//!             slide right, its name shows under it; moving along it changes the stage after 220 ms.
//!   Then      Sui Games, On Sui, your collectibles, Apps, Emulators, On your PC, Top collections
//!             and Latest sales, Rare.

use crate::act::Act;
use crate::data::catalog::SuiGame;
use crate::data::live::{CollectionRow, Sale};
use crate::data::model::Game;
use crate::kit::art::{ArtFit, ArtWant};
use crate::kit::{self, s, tok, Btn as B, BtnIcon, DotTone, NumKind, NumOptions, Track, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Fx, Page};
use crate::screens::game::parts::{self, game_meta, hero_logo, meta_line, Variant};
use crate::state::{start_line, Overlay, State, Tab};
use rare_render::focus::{Focusable, Group};
use rare_render::scene::{GradDir, Gradient};
use rare_render::view::{key, El, Look, Transition};

/// The strip's tile, and how far the tiles after the focused one slide (5.5rem x 0.45).
const TILE: f32 = 5.5;
const SHIFT: f32 = 2.475;
/// The stage switches to the focused game as soon as its art and logo are on the GPU (the
/// neighbours' are preloaded); past this the old game's art fades to the plain background rather
/// than staying up under the wrong title.
const STAGE_WAIT: f64 = 0.12;
/// How many games either side of the focused one have their stage art preloaded.
const PRELOAD: usize = 2;

#[derive(Default)]
pub struct Home {
    /// The game on the stage (None: the fallback, the first on the shelf).
    stage: Option<String>,
    /// The game the strip focus moved to and since when, until the stage has switched to it.
    want: Option<(String, f64)>,
    /// The game whose art is behind the page (None: cleared to the plain background).
    bd: Option<String>,
    bd_seen: bool,
    /// The game whose text is on the stage now (the stage, or the first on the shelf).
    stage_id: Option<String>,
    /// The game whose art is still decoding after the stage moved to it.
    late: Option<String>,
    /// Which strip tile has focus (the tiles after it slide).
    strip_focus: Option<usize>,
    /// The tag of each strip tile, in order.
    strip_tags: Vec<u32>,
    /// The strip has taken its first focus (Home opens on the strip, like a console).
    strip_seen: bool,
    hero_tags: Vec<u32>,
    /// First tag of the friends piece this build (None: it is not showing).
    fr_base: Option<u32>,
    /// The Steam tile's tag (first on the shelf; focus starts on the game after it).
    steam_tag: Option<u32>,
}

/// `Act::Local` codes for the friends piece: `FR_CODE + offset`.
const FR_CODE: u32 = 5000;

/// Emulator frontends first, then the rest by name (Home.tsx sortApps).
pub fn sort_apps(mut apps: Vec<&Game>) -> Vec<&Game> {
    const ORDER: [&str; 4] = ["esde", "esde", "emulationstation", "retroarch"];
    let rank = |g: &Game| {
        let n: String = g.title.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        ORDER.iter().position(|k| n.starts_with(k)).unwrap_or(99)
    };
    apps.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.title.cmp(&b.title)));
    apps
}

impl Home {
    /// Favourites lead the shelf, then everything by last played.
    fn recent<'a>(st: &'a State) -> Vec<&'a Game> {
        // ROMs (often hundreds) live in Library, Emulation: here only the ones played or favourited.
        let rom_ok = |g: &Game| g.source != "emu" || g.last_played.is_some_and(|t| t > 0) || st.is_fav(&g.id);
        let mut v: Vec<&Game> = st.games().iter().filter(|g| g.source != "apps" && !rare_android::is_store(&g.id) && !st.is_hidden(&g.id) && g.installed && rom_ok(g)).collect();
        // Cloud games only once played here or made a favourite.
        v.extend(st.cloud().iter().filter(|g| (g.last_played.is_some_and(|t| t > 0) || st.is_fav(&g.id)) && !st.is_hidden(&g.id)));
        // One tile per game (data/groups.rs).
        let mut v = st.one_each(v);
        v.sort_by(|a, b| st.is_fav(&b.id).cmp(&st.is_fav(&a.id)).then_with(|| b.last_played.unwrap_or(0).cmp(&a.last_played.unwrap_or(0))));
        v
    }

    fn stage_game<'a>(&self, st: &'a State) -> Option<&'a Game> {
        self.stage.as_deref().and_then(|id| st.game(id)).map(|g| st.shown(g)).or_else(|| Home::recent(st).first().copied())
    }

    /// Moves the stage to the game the strip focus is on once its art is ready (or has waited too
    /// long). Returns when to look again.
    fn settle_want(&mut self, st: &mut State, now: f64) -> Option<f64> {
        let mut wake = None;
        if let Some((id, since)) = self.want.clone() {
            if st.game(&id).is_none() {
                self.want = None;
            } else if st.art_ready.contains(&id) {
                // On the GPU: the text and the art switch in this frame, the art cross-fading.
                self.stage = Some(id.clone());
                self.stage_id = Some(id.clone());
                self.bd = Some(id);
                self.want = None;
            } else if now - since >= STAGE_WAIT {
                // Not ready in time: the text moves on, the old art fades out to the plain
                // background, and the new art arrives when it is decoded.
                self.stage = Some(id.clone());
                self.stage_id = Some(id.clone());
                self.bd = None;
                self.want = None;
                self.late = Some(id);
            } else {
                wake = Some(now + 0.02);
            }
        }
        if let Some(id) = self.late.clone() {
            if self.stage_id.as_deref() != Some(id.as_str()) {
                self.late = None;
            } else if st.art_ready.contains(&id) {
                self.bd = Some(id);
                self.late = None;
            } else {
                wake = Some(now + 0.03);
            }
        }
        wake
    }

    /// The art behind the page follows `bd`.
    fn sync_backdrop(&mut self, st: &mut State) {
        if st.tab == Tab::Home {
            let b = self.bd.as_deref().and_then(|id| st.game(id)).map(|g| (g.id.clone(), backdrop_srcs(g)));
            st.set_backdrop(b);
        }
    }

    fn strip(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts, games: &[&Game]) -> El {
        self.strip_tags.clear();
        let mut tiles = Vec::new();
        // Steam, always first on the shelf: one press left of the last game (focus starts on that
        // game, so A still plays it).
        let logo = kit::steam(ui, 2.1, tok::FG);
        let inner = ui.bx(s().fill().col().items_center().justify_center()).radius(ui.r(0.5)).bg(tok::L2).child(logo);
        let label = strip_label(ui, "Steam", "Store, library and downloads");
        let tag = acts.tag(Act::SteamBigPicture);
        self.strip_tags.push(tag);
        self.steam_tag = Some(tag);
        tiles.push(self.strip_tile(ui, 0, inner, label, tag, "Open Steam", None, false).key(key("steam-tile")));
        let n = games.len().min(16) + 1;
        for (i, g) in games.iter().take(16).enumerate() {
            let i = i + 1;
            let running = st.is_running(&g.id);
            let cover = if g.art.cover.is_empty() { &g.art.header } else { &g.art.cover };
            let pic = if parts::icon_art(g) { parts::app_art(ui, g) } else { parts::art(ui, cover, &g.title, Variant::Cover, TILE, TILE, 0.28) };
            let mut inner = ui.bx(s().fill()).radius(ui.r(0.5)).clip().child(pic);
            if running {
                inner = inner.child(kit::dot(ui, s().abs(Some(0.4), Some(0.4), None, None), DotTone::Primary, true));
            }
            let tag = acts.game(Act::Launch(g.id.clone()), &g.id);
            self.strip_tags.push(tag);
            let label = strip_label(ui, &g.title, if running { "Playing" } else { crate::data::model::source_label(&g.source) });
            tiles.push(self.strip_tile(ui, i, inner, label, tag, if running { "Resume" } else { "Play" }, Some(&g.id), i == 1).key(key(&g.id)));
        }
        // The Library tile at the end.
        let grid = kit::icon_s(ui, "layout-grid", 1.5, tok::FG, 1.75, false);
        let inner = ui.bx(s().fill().col().items_center().justify_center().gap(0.25)).radius(ui.r(0.5)).bg(tok::L2).child(grid);
        let label = strip_label(ui, "All games", &format!("{} in your library", st.one_each(st.games().iter().filter(|g| g.installed && g.source != "emu")).len()));
        let tag = acts.tag(Act::Tab(Tab::Library));
        self.strip_tags.push(tag);
        tiles.push(self.strip_tile(ui, n, inner, label, tag, "Library", None, false).key(key("all-games")));
        // data-rail data-nav-group="strip": h-[13.5rem] gap-[0.75rem] px-[2.5rem], the rail's 1.4rem
        // room above and below.
        ui.bx(s().row().items_start().h(13.5).gap(0.75).px(2.5).py(1.4).my(-1.4)).scrolls_x(key("home.strip")).group(Group::new("strip", [0.0; 4]).rail()).kids(tiles)
    }

    /// `.strip-tile`: grows to 1.45 from its top-left corner while focused; after the focused one,
    /// slides right by the growth (transform 160 ms on the pro curve).
    #[allow(clippy::too_many_arguments)]
    fn strip_tile(&self, ui: &mut Ui, i: usize, inner: El, label: El, tag: u32, a: &str, game: Option<&str>, def: bool) -> El {
        let shift = if self.strip_focus.is_some_and(|f| i > f) { ui.r(SHIFT) } else { 0.0 };
        let look = Look::default().bg(tok::L3).radius(ui.r(0.5)).origin(0.0, 0.0).translate(shift, 0.0);
        let focus = look.clone().scale(1.45);
        let a = a.to_string();
        let is_game = game.is_some();
        let title = game.map(String::from);
        ui.bx(s().size(TILE))
            .look(look)
            .focus_look(focus)
            .transition(Transition::pro(0.16))
            .child(inner)
            .child(label)
            .nav(move |f| {
                let mut f = f.tag(tag).hints(Some(&a), if is_game { Some("Details") } else { None }, None);
                if is_game {
                    f = f.game();
                }
                if let Some(t) = &title {
                    f = f.label(t);
                }
                if def {
                    f = f.default();
                }
                f
            })
    }

    fn stage_body(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts, g: &Game, buttons: bool) -> El {
        let running = st.is_running(&g.id);
        let start = st.starts.get(&g.id);
        let line = if running {
            start_line(&g.title, start, ui.now).unwrap_or_else(|| "Now playing".into())
        } else if g.last_played.unwrap_or(0) > 0 {
            "Continue playing".into()
        } else {
            "In your library".into()
        };
        let mut status = ui.bx(s().row().items_center().gap(0.5));
        if running {
            status = status.child(kit::dot(ui, s(), DotTone::Brand, true));
        }
        status = status.child(ui.text(line, T::new(Ty::Small).w(500).c(tok::fg(0.75))));
        let logo = hero_logo(ui, g, true);
        let meta = meta_line(ui, s(), &game_meta(g), T::new(Ty::Body).c(tok::fg(0.75)));
        let mut body = ui.bx(s().col().justify_end().gap(1.0).grow()).child(status).child(logo).child(meta);
        if buttons {
            let play_label = if running { "Resume" } else if g.source == "apps" { "Open" } else { "Play" };
            let play_tag = acts.game(Act::Launch(g.id.clone()), &g.id);
            let details_tag = acts.game(Act::OpenGame(g.id.clone()), &g.id);
            let play = kit::button(ui, B::new(play_label, play_tag).primary().lg().hint(if running { "Resume" } else { "Play" }).icon(BtnIcon::Lucide("play", 1.1, true)).min_w(10.0).game());
            let details = kit::button(ui, B::new("Details", details_tag).lg().game());
            self.hero_tags.extend([play_tag, details_tag]);
            body = body.child(ui.bx(s().row().gap(0.75).mt(0.35)).group(Group::new("hero", [0.0; 4]).remember()).child(play).child(details));
        }
        body
    }

    fn stage(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let Some(g) = self.stage_game(st) else {
            // Nothing installed yet.
            let steam = kit::button(ui, B::new("Open Steam", acts.tag(Act::SteamBigPicture)).primary().lg().def(true).icon(BtnIcon::Steam(1.15)));
            let browse = kit::button(ui, B::new("Browse Sui games", acts.tag(Act::Tab(Tab::Games))).lg());
            return ui
                .bx(s().col().justify_end().gap(1.0).pb(0.5).grow())
                .child(ui.text("Welcome to Rare", T::new(Ty::Hero).w(700)))
                .child(ui.text_s(s().max_w(32.0), "Your games from Steam, Epic, GOG, itch.io and your folders show up here.", T::new(Ty::Body).c(tok::MUTED).wrap()))
                .child(ui.bx(s().row().gap(0.75)).group(Group::new("hero", [0.0; 4]).remember()).child(steam).child(browse));
        };
        let cell = ui.bx(s().col().justify_end().grow());
        // Title, logo and meta are the stage game's, switching in one frame with the art.
        let cur = self.stage_body(ui, st, acts, g, true).key(key(&g.id));
        cell.child(cur)
    }

    fn your_rare(&self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        if let Some(a) = &st.account {
            return your_items(ui, st, acts, a);
        }
        let sign = kit::button(ui, B::new("Sign in", acts.tag(Act::Open(Overlay::Other("signin")))).primary());
        let price = sui_price(ui, st);
        kit::panel(ui, s().col().w(24.0).gap(1.0).self_end().p(1.25))
            .child(ui.text("Your items", T::new(Ty::Label).c(tok::MUTED)))
            .child(ui.text_s(s().wp(1.0), "Sign in with a passkey to see your items.", T::new(Ty::Body).c(tok::MUTED).wrap()))
            .child(ui.bx(s().row()).group(Group::new("yourrare", [0.0; 4]).remember()).child(sign))
            .child(price)
    }
}

/// Your Rare with an account (Home.tsx YourRare): items in your games and their floor value, per
/// game, then your total and today's change. A opens the Wallet.
fn your_items(ui: &mut Ui, st: &State, acts: &mut Acts, a: &crate::data::account::HomeAccount) -> El {
    let muted = T::new(Ty::Meta).c(tok::MUTED);
    let mut p = ui.bx(s().col().w(24.0).self_end().p(1.25)).bg(tok::L1).radius(ui.r(0.75)).clip().child(ui.text("Items in your games", T::new(Ty::Label).c(tok::MUTED)));
    let mut value = ui.bx(s().row().items_end().gap(0.4).mt(0.25).min_h(2.5));
    let mut line = ui.bx(s().min_h(1.25));
    if let Some(g) = &a.in_games {
        value = value.child(ui.text(g.value.clone(), T::new(Ty::Px(2.0, 2.5)).w(600).mono())).child(ui.text("SUI", T::new(Ty::Row).w(500).c(tok::MUTED)));
        let words = if g.items == 0 {
            "No game items yet".to_string()
        } else {
            let it = format!("{} {}", kit::format::grouped(g.items as f64, 0, 0), if g.items == 1 { "item" } else { "items" });
            if g.offers > 0 {
                format!("{it}, {} {}", kit::format::grouped(g.offers as f64, 0, 0), if g.offers == 1 { "offer" } else { "offers" })
            } else {
                it
            }
        };
        line = line.child(ui.text(words, muted));
    }
    p = p.child(value).child(line);
    if let Some(g) = a.in_games.as_ref().filter(|g| !g.games.is_empty()) {
        let mut list = ui.bx(s().col().gap(0.5).mt(0.9));
        for (name, logo, v) in g.games.iter().take(4) {
            let srcs: Vec<String> = if logo.is_empty() { vec![] } else { vec![logo.clone()] };
            let thumb = parts::art(ui, &srcs, name, Variant::Cover, 1.5, 1.5, 0.3);
            list = list.child(
                ui.bx(s().row().items_center().gap(0.6))
                    .child(ui.bx(s().size(1.5)).radius(ui.r(0.3)).clip().child(thumb))
                    .child(ui.bx(s().grow().min0()).child(ui.text(name.clone(), T::new(Ty::Small).w(500).trunc())))
                    .child(ui.text(format!("{v} SUI"), T::new(Ty::Small).c(tok::MUTED).mono())),
            );
        }
        if g.games.len() > 4 {
            list = list.child(ui.text(format!("and {} more", g.games.len() - 4), muted));
        }
        p = p.child(list);
    }
    p = p.child(ui.bx(s().wp(1.0).h_px(1.0).my(1.0)).bg(tok::HAIR));
    let total = kit::num(ui, s(), NumKind::Money, a.total_usd, NumOptions::default(), T::new(Ty::Row).w(600), false).unwrap_or_else(|| ui.bx(s().h(1.6)));
    let mut foot = ui.bx(s().row().items_end().justify_between().gap(1.0)).child(ui.bx(s().col().min0()).child(ui.text("Your total", T::new(Ty::Label).c(tok::MUTED))).child(total));
    // Today's change only when it is a cent or more (an empty wallet reads $0.00 once).
    if let Some(d) = kit::num(ui, s(), NumKind::Pnl, a.change_usd.filter(|c| c.abs() >= 0.005), NumOptions::default(), T::new(Ty::Small).w(500), true) {
        foot = foot.child(d);
    }
    p = p.child(foot).child(sui_price(ui, st));
    let tag = acts.tag(Act::Tab(Tab::Wallet));
    ui.bx(s().row().self_end()).group(Group::new("yourrare", [0.0; 4]).remember()).child(kit::nav(kit::tile(p, 1.02), tag, Some("Open your items"), None, false))
}

/// The name under a strip tile (`.strip-label`): it shows while the tile has focus, scaled with it.
fn strip_label(ui: &mut Ui, title: &str, sub: &str) -> El {
    let l = ui
        .bx(s().abs(Some(TILE + 0.45), None, None, Some(0.0)).col())
        .opacity(0.0)
        .transition(Transition::pro(0.12))
        .child(ui.text(title, T::new(Ty::Px(0.68, 0.68 * 1.25)).w(600)))
        .child(ui.text(sub, T::new(Ty::Px(0.55, 0.55 * 1.25)).c(tok::fg(0.6))));
    let fl = l.look.clone().opacity(1.0);
    l.focus_look(fl)
}

/// The SUI price, small (Home.tsx SuiPrice).
fn sui_price(ui: &mut Ui, st: &State) -> El {
    let meta = T::new(Ty::Meta).c(tok::MUTED);
    let mut row = ui.bx(s().row().items_center().gap(0.45).mt(0.9)).child(kit::sui_logo(ui, 0.9)).child(ui.text("SUI", meta));
    if let Some(p) = kit::num(ui, s(), NumKind::Price, st.live.sui_usd, NumOptions::default(), meta, true) {
        row = row.child(p);
    }
    let ch = kit::num(ui, s(), NumKind::Change, st.live.sui_change, NumOptions::default(), meta, true);
    row.child(ui.bx(s().ml_auto()).maybe(ch))
}

/// A Sui game as a store tile (SuiGames.tsx SuiGameCard): logo, name, studio and genre, status and
/// token.
pub fn sui_game_card(ui: &mut Ui, acts: &mut Acts, g: &SuiGame, def: bool) -> El {
    let logo = game_logo(ui, &g.logo_url, &g.name, 3.5);
    let head = ui
        .bx(s().row().items_center().gap(1.0).min0())
        .child(logo)
        .child(ui.bx(s().col().min0().shrink()).child(ui.text(g.name.clone(), T::new(Ty::Row).w(600).trunc())).child(ui.text([g.studio.as_str(), g.genre.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" \u{b7} "), T::new(Ty::Small).c(tok::MUTED).trunc())));
    let mut status = ui.bx(s().row().items_center().gap(0.45).min0().shrink());
    if g.status.as_deref() == Some("live") {
        status = status.child(kit::dot(ui, s(), DotTone::Primary, false));
    }
    status = status.child(ui.text(g.status_line(), T::new(Ty::Meta).c(tok::MUTED).trunc()));
    let mut foot = ui.bx(s().row().items_center().justify_between().gap(0.75).min0()).child(status);
    if let Some(t) = &g.token {
        foot = foot.child(ui.bx(s().row().items_center().gap(0.45)).child(ui.text(format!("${}", t.symbol), T::new(Ty::Meta).w(500))));
    }
    let tag = acts.tag(Act::SuiGame(g.slug.clone()));
    let card = ui.bx(s().col().justify_between().h(8.0).w(20.0).p(1.0)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR).child(head).child(foot);
    kit::nav(kit::tile(card, 1.06), tag, Some("Details"), None, def).key(key(&g.slug))
}

/// rare.market's GameLogo: the logo on a rounded tile, or the first letter.
pub fn game_logo(ui: &mut Ui, url: &str, name: &str, size: f32) -> El {
    let fb = ui.bx(s().fill().row().items_center().justify_center()).child(ui.text(name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(), T::new(Ty::px(1.25)).w(600).c(tok::fg(0.5))));
    let srcs: Vec<String> = (!url.is_empty()).then(|| url.to_string()).into_iter().collect();
    let img = kit::art(ui, s().fill(), &srcs, size, size, ArtFit::Cover(0.5), Some(fb));
    ui.bx(s().size(size)).bg(tok::L3).radius(ui.r(0.5)).clip().child(img)
}

/// A square tile with words at the bottom (On Sui, Rare): `.tile`, rounded-card, hairline border.
fn word_tile(ui: &mut Ui, acts: &mut Acts, act: Act, hint: &str, w: f32, h: f32, icon: Option<&str>, title: &str, sub: &str, wide: bool) -> El {
    let mut t = ui.bx(s().col().justify_end().gap(0.2).wh(w, h).p(1.0)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR);
    if let Some(i) = icon {
        t = t.child(kit::icon(ui, i, 1.6, tok::ICON2).style(|st| st.margin.bottom = rare_render::taffy::prelude::LengthPercentageAuto::auto()));
    }
    t = t.child(ui.text(title, T::new(Ty::Body).w(700))).child(ui.text(sub, T::new(Ty::Meta).c(tok::MUTED).wrap()));
    let tag = acts.tag(act);
    kit::nav(kit::tile(t, if wide { 1.03 } else { 1.06 }), tag, Some(hint), None, false)
}

/// rare.market's featured card: square art, a dark fade at the bottom, white title and subline.
fn rare_card(ui: &mut Ui, acts: &mut Acts, act: Act, label: &str, sub: &str, image: Option<&str>, mosaic: &[String], icon: &str) -> El {
    let w = 11.0;
    let mut c = ui.bx(s().size(w)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR).clip();
    let mut art = false;
    if let Some(src) = image {
        if let Some(k) = ui.arts.key(ui.img, &[src.to_string()], ui.r(w), ui.r(w), ArtFit::Cover(0.5)) {
            c = c.child(crate::kit::art::image(k, ArtFit::Cover(0.5)).with_style(s().fill().to_style(ui.rem)));
            art = true;
        }
    }
    if mosaic.len() >= 4 {
        let mut grid = ui.bx(s().fill().grid(&[Track::Fr(1.0), Track::Fr(1.0)]));
        for m in mosaic.iter().take(4) {
            grid = grid.child(kit::thumb(ui, s(), Some(m), "", w / 2.0, w / 2.0, 0.0, false));
        }
        c = c.child(grid);
        art = true;
    }
    if art {
        c = c.child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).hp(0.5)).look(Look::default().grad(Gradient { dir: GradDir::ToTop, stops: [tok::BLACK.alpha(0.95), tok::BLACK.alpha(0.5), tok::BLACK.alpha(0.0)], mid: 0.5 })));
    } else {
        // No art: the card's icon where the Cloud saves tile has its own, so it never reads blank.
        c = c.child(ui.bx(s().abs(Some(1.0), None, None, Some(1.0))).child(kit::icon(ui, icon, 1.6, tok::ICON2)));
    }
    c = c.child(
        ui.bx(s().abs(None, Some(1.0), Some(0.9), Some(1.0)).col())
            .child(ui.text(label, T::new(Ty::Row).w(700).c(if art { tok::WHITE } else { tok::FG }).trunc()))
            .child(ui.text(sub, T::new(Ty::Meta).c(if art { tok::WHITE.alpha(0.6) } else { tok::MUTED }).trunc())),
    );
    let tag = acts.tag(act);
    kit::nav(kit::tile(c, 1.06), tag, Some("Open"), None, false).key(key(label))
}

const COLL_COLS: [Track; 4] = [Track::Rem(1.25), Track::Fr(1.0), Track::Rem(6.5), Track::Rem(4.75)];
const SALE_COLS: [Track; 3] = [Track::Fr(1.0), Track::Rem(6.5), Track::Rem(3.25)];

fn top_collections(ui: &mut Ui, acts: &mut Acts, rows: Option<&Vec<CollectionRow>>) -> El {
    let right = ui.text("rare.market", T::new(Ty::Small).c(tok::MUTED));
    let head = kit::section_head(ui, s().mb(0.5).px(0.75), "Top collections", Some("24h volume"), Some(right));
    let lbl = T::new(Ty::Label).w(500).c(tok::MUTED);
    let hr = kit::head_row(ui, &COLL_COLS, vec![ui.text("#", lbl), ui.text("Collection", lbl), ui.text_s(s().wp(1.0), "Floor", lbl.right()), ui.text_s(s().wp(1.0), "24h", lbl.right())]);
    let mut list = ui.bx(s().col()).group(Group::new("home-collections", [0.0; 4]));
    match rows {
        None => list = list.child(ui.bx(s().h(16.25))),
        Some(rows) => {
            for r in rows.iter().take(5) {
                let small = T::new(Ty::Small);
                let mut name = ui.bx(s().row().items_center().gap(0.65).min0()).child(kit::avatar(ui, s(), r.image_url.as_deref(), &r.name, 2.1, false)).child(ui.text(r.name.clone(), small.w(600).trunc()));
                if r.verified {
                    name = name.child(kit::verified(ui, 0.9));
                }
                let floor = kit::sui(ui, s(), r.floor_mist, 2, small.w(600));
                let delta = kit::delta(ui, s(), r.floor_change_pct, 1, small.w(600));
                let cells = vec![
                    ui.text(r.rank.to_string(), small.w(600).tab().c(tok::MUTED)),
                    name,
                    kit::cell(ui, true).child(floor),
                    kit::cell(ui, true).child(delta),
                ];
                let tag = acts.tag(Act::Market(format!("collection:{}", r.slug)));
                list = list.child(kit::row(ui, &COLL_COLS, tag, Some("Open collection"), cells).key(key(&r.slug)));
            }
        }
    }
    kit::panel(ui, s().col().min0().p(1.0)).child(head).child(hr).child(list)
}

fn latest_sales(ui: &mut Ui, acts: &mut Acts, sales: Option<&Vec<Sale>>, now_ms: f64) -> El {
    let live = ui.bx(s().row().items_center().gap(0.45)).child(kit::dot(ui, s(), DotTone::Brand, true)).child(ui.text("Live", T::new(Ty::Small).c(tok::MUTED)));
    let head = kit::section_head(ui, s().mb(0.5).px(0.75), "Latest sales", None, Some(live));
    let lbl = T::new(Ty::Label).w(500).c(tok::MUTED);
    let hr = kit::head_row(ui, &SALE_COLS, vec![ui.text("Item", lbl), ui.text_s(s().wp(1.0), "Price", lbl.right()), ui.text_s(s().wp(1.0), "Time", lbl.right())]);
    let mut list = ui.bx(s().col()).group(Group::new("home-sales", [0.0; 4]));
    match sales {
        None => list = list.child(ui.bx(s().h(16.25))),
        Some(sales) => {
            for e in sales.iter().take(5) {
                let small = T::new(Ty::Small);
                let name = e.item_name.clone().map(|n| short_name(&n, e.collection_name.as_deref())).or_else(|| e.collection_name.clone()).unwrap_or_default();
                let item = ui
                    .bx(s().row().items_center().gap(0.65).min0())
                    .child(kit::avatar(ui, s(), e.item_image.as_deref().or(e.collection_image.as_deref()), e.item_name.as_deref().unwrap_or(""), 2.1, true))
                    .child(ui.bx(s().col().min0().shrink()).child(ui.text(name, small.w(600).trunc())).child(ui.text(e.collection_name.clone().unwrap_or_default(), T::new(Ty::Meta).c(tok::MUTED).trunc())));
                let price = kit::sui(ui, s(), e.price_mist, 2, small.w(600));
                let cells = vec![item, kit::cell(ui, true).child(price), kit::cell(ui, true).child(ui.text(kit::format::time_ago(e.at_ms, now_ms), T::new(Ty::Meta).c(tok::MUTED)))];
                let target = match (&e.item_id, &e.collection_slug) {
                    (Some(i), _) => Act::Market(format!("item:{i}")),
                    (None, Some(c)) => Act::Market(format!("collection:{c}")),
                    _ => Act::Tab(Tab::Market),
                };
                let tag = acts.tag(target);
                list = list.child(kit::row(ui, &SALE_COLS, tag, Some("View item"), cells).key(key(&e.id)));
            }
        }
    }
    kit::panel(ui, s().col().min0().p(1.0)).child(head).child(hr).child(list)
}

/// "#7409" from "Popkins #7409" (marketplace/format.ts shortName).
fn short_name(name: &str, collection: Option<&str>) -> String {
    match collection {
        Some(c) if name.starts_with(&format!("{c} ")) => name[c.len() + 1..].to_string(),
        _ => name.to_string(),
    }
}

/// The games a paired PC serves (the stream mock, standing in until WS 8/13's stream data).
/// The Apps row's entries (apps/catalog.json): id, name, icon.
/// Game apps first (cloud gaming, Steam), then the rest.
pub const APPS: [(&str, &str, &str); 11] = [
    ("xcloud", "Xbox Cloud", "apps/xcloud.png"),
    ("geforcenow", "GeForce NOW", "apps/geforcenow.png"),
    ("steam", "Steam", "apps/steam.png"),
    ("chrome", "Google Chrome", "apps/chrome.png"),
    ("youtube", "YouTube", "apps/youtube.png"),
    ("netflix", "Netflix", "apps/netflix.png"),
    ("twitch", "Twitch", "apps/twitch.png"),
    ("spotify", "Spotify", "apps/spotify.png"),
    ("discord", "Discord", "apps/discord.png"),
    ("telegram", "Telegram", "apps/telegram.png"),
    ("x", "X", "apps/x.png"),
];

/// A PC game's cover (stream/tiles.tsx StreamTile): opens the PC game sheet.
pub fn pc_tile(ui: &mut Ui, st: &State, acts: &mut Acts, pc: &crate::data::model::PcShelf, a: &crate::data::model::PcApp, w: f32, def: bool) -> El {
    let srcs: Vec<String> = a.art.iter().cloned().collect();
    let pic = parts::art(ui, &srcs, &a.name, Variant::Cover, 9.0, 13.5, 0.5);
    let tag = acts.tag(Act::PcSheet(pc.uuid.clone(), a.id));
    let mut b = ui.bx(s().wh(9.0, 13.5)).bg(tok::L3).radius(ui.r(0.5)).clip().child(pic);
    let line = pc.tile_line(a.id, &st.running);
    if line == "Playing" || line == "Running on your PC" {
        b = b.child(kit::dot(ui, s().abs(Some(0.6), Some(0.6), None, None), DotTone::Primary, line == "Playing"));
    }
    ui.bx(s().col().w(w))
        .child(kit::nav(kit::tile(b, 1.06), tag, Some("Open"), None, def))
        .child(ui.text_s(s().mt(0.85).px(0.1), a.name.clone(), T::new(Ty::Meta).w(500).c(tok::fg(0.85)).trunc()))
        .child(ui.text_s(s().px(0.1), line, T::new(Ty::Label).c(tok::MUTED).trunc()))
        .key(key(&a.id))
}

/// A square app tile (apps/tiles.tsx AppTile): the app's own icon on layer 02, its name under it.
pub fn app_tile(ui: &mut Ui, acts: &mut Acts, id: &str, name: &str, icon: &str, def: bool, running: bool) -> El {
    // apps/tiles.tsx AppTile: Installing with a bar while flatpak runs, Not installed when no way opens it.
    let (installing, ready, known) = match crate::features::apps::shared() {
        Some(a) => {
            let a = a.borrow();
            (a.installing.get(id).filter(|i| i.error.is_none()).cloned(), a.status.get(id).is_some_and(|s| s.way.is_some()), a.status.contains_key(id))
        }
        None => (None, true, false),
    };
    let sub = if installing.is_some() {
        "Installing"
    } else if running {
        "Running"
    } else if !ready && known {
        "Not installed"
    } else {
        "\u{a0}"
    };
    let ic = kit::art(ui, s().size(4.5), &[icon.to_string()], 4.5, 4.5, ArtFit::Contain(0.5, 0.5), None).radius(ui.r(4.5 * 0.22));
    let tag = acts.tag(Act::OpenApp(id.to_string()));
    let mut b = ui.bx(s().row().items_center().justify_center().size(9.0)).bg(tok::L2).radius(ui.r(0.5)).clip().child(ic);
    if running {
        b = b.child(kit::dot(ui, s().abs(Some(0.6), Some(0.6), None, None), DotTone::Primary, true));
    }
    if let Some(i) = &installing {
        let w = i.progress.map(|p| p.clamp(0.0, 1.0)).unwrap_or(0.4);
        let fill = ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(w)).bg(tok::FG.alpha(if i.progress.is_some() { 1.0 } else { 0.55 })).radius(ui.r(0.175));
        let track = ui.bx(s().abs(None, Some(0.75), Some(0.75), Some(0.75)).h(0.35)).bg(tok::L4).radius(ui.r(0.175)).clip().child(fill);
        b = b.child(track);
    }
    let hint = if running { "Resume" } else if ready || !known { "Open" } else { "Get" };
    // Named for the focus line (journeys) and screen readers: the name sits under the tile.
    let label = name.to_string();
    let b = kit::tile(b, 1.06).nav(move |f| {
        let f = f.tag(tag).hints(Some(hint), Some("Details"), None).label(&label);
        if def {
            f.default()
        } else {
            f
        }
    });
    ui.bx(s().col().w(9.0))
        .child(b)
        .child(ui.text_s(s().mt(0.85).px(0.1), name, T::new(Ty::Meta).w(500).c(tok::fg(0.85)).trunc()))
        .child(ui.text_s(s().px(0.1), sub, T::new(Ty::Label).c(tok::MUTED)))
        .key(key(id))
}

impl Page for Home {
    fn stamp(&self, st: &State) -> u64 {
        hash((crate::features::apps::shared().map(|a| a.borrow().version).unwrap_or(0), st.ver.library, st.ver.running, st.ver.favs, st.ver.starts, st.ver.live, st.ver.catalog, st.ver.pc, st.ver.account, st.ver.friends, &self.stage, (self.strip_focus, crate::data::live::PIC_VER.load(std::sync::atomic::Ordering::Relaxed), crate::data::knowledge::free_ver(), crate::data::wishlist::ver())))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let recent = Home::recent(st);
        self.hero_tags.clear();
        // The first screen: the strip on top, the lockup and Your Rare at the bottom, art behind it all.
        let mut hero = ui.bx(s().col().h(42.75).pb(1.5));
        if !recent.is_empty() {
            hero = hero.child(self.strip(ui, st, acts, &recent));
        }
        let stage = self.stage(ui, st, acts);
        let yr = self.your_rare(ui, st, acts);
        hero = hero.child(ui.bx(s().row().items_end().gap(2.0).px(2.5).grow()).child(stage).child(yr));

        let mut below = ui.bx(s().col().gap(2.25).mt(1.0));
        // Apps right under the stage (Down, Down from the shelf): Xbox Cloud, GeForce NOW, Steam...
        let n_apps = st.running.iter().filter(|r| r.starts_with("app:")).count();
        let apps: Vec<El> = APPS.iter().map(|(id, name, icon)| app_tile(ui, acts, id, name, icon, false, st.is_running(&crate::screens::apps::run_id(id)))).collect();
        below = below.child(kit::rail(ui, s(), "media-apps", Some("Apps"), (n_apps > 0).then(|| format!("{n_apps} running")).as_deref(), apps).key(key("media-apps")));
        if !st.sui_games.is_empty() {
            let cards: Vec<El> = st.sui_games.iter().take(10).map(|g| sui_game_card(ui, acts, g, false)).collect();
            below = below.child(kit::rail(ui, s(), "suigames", Some("Sui Games"), Some("from Rare"), cards).key(key("suigames")));
        }
        // Free to claim: Epic's weekly games, GOG and Steam giveaways, Steam free weekends (rare.market).
        if let Some(free) = crate::data::knowledge::free_games().filter(|f| !f.is_empty()) {
            let now = crate::data::model::now_secs();
            let n = free.iter().filter(|f| !f.upcoming).count();
            let cards: Vec<El> = free.iter().take(12).map(|f| free_card(ui, acts, f, now)).collect();
            let note = if n > 0 { format!("{n} free now") } else { "Coming up".to_string() };
            below = below.child(kit::rail(ui, s(), "free-games", Some("Free to claim"), Some(&note), cards).key(key("free-games")));
        }
        // Stats and wishlist: the pages, then wishlist games on sale.
        below = below.child(play_rail(ui, acts).key(key("play")));
        // On Sui (sui/ui/HomeSui.tsx): the profile card for the current account, else the way in.
        let mut on_sui = vec![
            match &st.account {
                Some(a) => {
                    let label = T::new(Ty::Label).c(tok::MUTED);
                    // rare-sui's profile (name, picture, games owned) when it has answered.
                    let pl = st.live.profile.as_ref().filter(|p| p.address.eq_ignore_ascii_case(&a.address));
                    let name = crate::data::account::shown_name(a, pl);
                    let total = pl.and_then(|p| p.total_usd).or(a.total_usd);
                    let change = pl.and_then(|p| p.change_usd).or(a.change_usd).filter(|c| c.abs() >= 0.005);
                    let owned = pl.and_then(|p| p.owned_games).filter(|n| *n > 0).map(|n| format!("{n} Sui {} owned", if n == 1 { "game" } else { "games" })).unwrap_or_else(|| "Your profile".into());
                    // The pick on this device first (it shows the moment it is made), then rare.market's.
                    let pic = match crate::data::live::picked(&a.address).map(|p| p.image_url).filter(|u| !u.is_empty()).or_else(|| pl.and_then(|p| p.avatar.clone())).as_deref() {
                        Some(src) => kit::avatar(ui, s(), Some(src), &name, 2.75, false),
                        None => crate::screens::market::parts::account_avatar(ui, s(), &a.address, 2.75),
                    };
                    let value = kit::num(ui, s(), NumKind::Money, total, NumOptions::default(), T::new(Ty::Row).w(600), false).unwrap_or_else(|| ui.text("--", T::new(Ty::Row).c(tok::MUTED)));
                    let mut bottom = ui.bx(s().row().items_end().justify_between().gap(1.0)).child(ui.bx(s().col().shrink()).child(ui.text("Total value", label)).child(value));
                    if let Some(d) = kit::num(ui, s(), NumKind::Pnl, change, NumOptions::default(), T::new(Ty::Small).w(500), true) {
                        bottom = bottom.child(ui.bx(s().col().items_end()).child(ui.text("Today", label)).child(d));
                    }
                    let head = ui
                        .bx(s().row().items_center().gap(0.75))
                        .child(pic)
                        .child(ui.bx(s().col().shrink()).child(ui.text(name, T::new(Ty::Body).w(600).trunc())).child(ui.text(owned, T::new(Ty::Meta).c(tok::MUTED).trunc())));
                    let t = ui.bx(s().col().justify_between().gap(0.75).wh(19.0, 12.0).p(1.1)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR).child(head).child(bottom);
                    let tag = acts.tag(Act::Open(Overlay::Other("profile")));
                    kit::nav(kit::tile(t, 1.03), tag, Some("Profile"), None, false)
                }
                None => {
                    let t = ui
                        .bx(s().col().justify_between().gap(0.75).wh(19.0, 12.0).p(1.1))
                        .bg(tok::L1)
                        .radius(ui.r(0.5))
                        .border(1.0, tok::HAIR)
                        .child(ui.text("Your Sui profile", T::new(Ty::Body).w(600)))
                        .child(ui.text_s(s().wp(1.0), "Add a Rare Wallet account to see your name, picture, games you own and cloud saves.", T::new(Ty::Small).c(tok::MUTED).wrap()));
                    let tag = acts.tag(Act::Tab(Tab::Wallet));
                    kit::nav(kit::tile(t, 1.03), tag, Some("Open Wallet"), None, false)
                }
            },
            word_tile(ui, acts, Act::Open(Overlay::Other("saves")), "Cloud saves", 12.0, 12.0, Some("cloud"), "Cloud saves", "Emulator saves on Walrus", false),
            word_tile(ui, acts, Act::Open(Overlay::Other("share")), "Share", 12.0, 12.0, Some("images"), "Share", "Screenshots and clips", false),
        ];
        on_sui.extend(st.live.mints.iter().flatten().map(|m| mint_card(ui, acts, m)));
        on_sui.push(rare_card_sq(ui, acts, st.live.mints.as_ref().map(|m| m.is_empty())));
        below = below.child(kit::rail(ui, s(), "sui-shelf", Some("On Sui"), st.live.mints.as_ref().filter(|m| !m.is_empty()).map(|m| format!("{} minting now", m.len())).as_deref(), on_sui).key(key("sui-shelf")));
        // Friends (friends/ui/HomeFriends.tsx): the party, then who is playing; nothing unless the
        // service is ready and there is something to show.
        self.fr_base = None;
        if let Some(d) = st.friends.as_deref().filter(|d| d.st.status == rare_friends::FriendsStatus::Ready) {
            use crate::screens::friends::home as fh;
            let base = acts.tag(Act::Local(FR_CODE));
            for i in 1..fh::H_SPAN {
                acts.tag(Act::Local(FR_CODE + i));
            }
            let parts = fh::home_friends(ui, d, base);
            if !parts.is_empty() {
                self.fr_base = Some(base);
            }
            for el in parts {
                below = below.child(el);
            }
        }
        // Your collectibles: what the account holds, straight from Rare Marketplace.
        let items: Vec<El> = st
            .account
            .as_ref()
            .and_then(|a| a.items.as_ref())
            .map(|list| {
                list.iter()
                    .take(16)
                    .map(|it| {
                        let srcs: Vec<String> = it.image.iter().cloned().collect();
                        let pic = parts::art(ui, &srcs, &it.name, Variant::Cover, 9.0, 9.0, 0.5);
                        let mut words = ui.bx(s().abs(None, Some(0.5), Some(0.6), Some(0.6)).col()).child(ui.text(it.name.clone(), T::new(Ty::Meta).w(600).c(tok::WHITE).trunc()));
                        if let Some(c) = &it.collection {
                            words = words.child(ui.text(c.clone(), T::new(Ty::Label).c(tok::WHITE.alpha(0.6)).trunc()));
                        }
                        let b = ui
                            .bx(s().size(9.0))
                            .bg(tok::L3)
                            .radius(ui.r(0.5))
                            .border(1.0, tok::HAIR)
                            .clip()
                            .child(pic)
                            .child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).hp(0.5)).look(Look::default().grad(Gradient { dir: GradDir::ToTop, stops: [tok::BLACK.alpha(0.95), tok::BLACK.alpha(0.5), tok::BLACK.alpha(0.0)], mid: 0.5 })))
                            .child(words);
                        let tag = acts.tag(Act::Market(format!("item:{}", it.id)));
                        kit::nav(kit::tile(b, 1.06), tag, Some("View"), None, false).key(key(&it.id))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Hidden while empty or loading: no empty owned shelf (the Your Rare panel asks to sign in).
        if !items.is_empty() {
            let note = st.account.as_ref().map(|a| a.label.clone());
            below = below.child(kit::rail(ui, s(), "collectibles", Some("Your collectibles"), note.as_deref(), items).key(key("collectibles")));
        }
        // Emulators.
        let emus = sort_apps(st.games().iter().filter(|g| g.source == "apps").collect());
        if !emus.is_empty() {
            let n = emus.len().to_string();
            let tiles: Vec<El> = emus.iter().take(16).map(|g| parts::game_tile(ui, st, acts, g, false, false, 9.0, None)).collect();
            below = below.child(kit::rail(ui, s(), "apps", Some("Emulators"), Some(&n), tiles).key(key("apps")));
        }
        // On your PC: the games on the paired PC that answers, once it does (stream/tiles.tsx).
        if let Some(pc) = st.pc.as_ref().filter(|p| !p.apps.is_empty()) {
            let tiles: Vec<El> = pc.apps.iter().take(16).map(|a| pc_tile(ui, st, acts, pc, a, 9.0, false)).collect();
            below = below.child(kit::rail(ui, s(), "stream-pc", Some("On your PC"), Some(&pc.name), tiles).key(key("stream-pc")));
        }
        // Market today.
        let now_ms = crate::app::wall_ms();
        let tc = top_collections(ui, acts, st.live.collections.as_ref());
        let ls = latest_sales(ui, acts, st.live.sales.as_ref(), now_ms);
        below = below.child(ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(1.25).px(2.5)).child(tc).child(ls).key(key("market")));
        // Rare.
        let logos: Vec<String> = st.sui_games.iter().filter(|g| !g.logo_url.is_empty()).take(4).map(|g| g.logo_url.clone()).collect();
        let links = vec![
            rare_card(ui, acts, Act::Tab(Tab::Market), "Marketplace", "Collections and coins", None, &[], "store"),
            rare_card(ui, acts, Act::Tab(Tab::Games), "Sui Games", "Games and collections", None, &logos, "gamepad-2"),
            rare_card(ui, acts, Act::OpenWeb("https://rare.market/launchpad".into()), "Launchpad", "New drops", None, &[], "rocket"),
        ];
        below = below.child(kit::rail(ui, s(), "rare", Some("Rare"), None, links).key(key("rare")));

        // The page scrolls (`[data-scroll]`): pb-[3rem], the rest of the shelves under the fold.
        ui.bx(s().col().full().pt(1.0).pb(3.0)).scrolls_y(key("home")).child(hero.key(key("hero"))).child(below.key(key("below")))
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        // X on a friend card: join their game.
        if b == Btn::X {
            if let (Some(base), Some((f, _)), Some(d)) = (self.fr_base, cur, st.friends.as_deref()) {
                if let Some(a) = crate::screens::friends::home::home_press(d, base, f.tag, rare_render::input::Pad::X) {
                    fx.run(Act::HomeFriends(a));
                    return true;
                }
            }
        }
        // X on a strip tile: the game page.
        if b == Btn::X {
            if let Some((f, Some(Act::Launch(id)))) = cur {
                if self.strip_tags.contains(&f.tag) {
                    fx.fb(crate::page::Feedback::Open);
                    st.open(Some(Overlay::Game(id.clone())));
                    return true;
                }
            }
        }
        false
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        let (Some(base), Some(d)) = (self.fr_base, st.friends.as_deref()) else { return };
        if let Some(off) = code.checked_sub(FR_CODE) {
            if let Some(a) = crate::screens::friends::home::home_press(d, base, base + off, rare_render::input::Pad::A) {
                fx.run(Act::HomeFriends(a));
            }
        }
    }

    fn focused(&mut self, cur: Option<&Focusable>, acts: &Acts, now: f64) -> bool {
        let idx = cur.and_then(|c| self.strip_tags.iter().position(|t| *t == c.tag));
        if let Some(i) = idx {
            if let Some(Act::Launch(id)) = cur.and_then(|c| acts.get(c.tag)) {
                if self.stage_id.as_deref() != Some(id) {
                    if self.want.as_ref().map(|w| &w.0) != Some(id) {
                        self.want = Some((id.clone(), now));
                    }
                } else {
                    self.want = None;
                }
            }
            let _ = i;
        }
        if idx != self.strip_focus {
            self.strip_focus = idx;
            return true;
        }
        false
    }

    fn take_focus(&mut self, cur: Option<&Focusable>) -> Option<u32> {
        // Unless the player already moved, the first tile takes focus when the strip appears
        // (the library can land after the stage's buttons took the default).
        if self.strip_seen || self.strip_tags.is_empty() {
            return None;
        }
        self.strip_seen = true;
        let first = self.strip_tags.iter().copied().find(|t| Some(*t) != self.steam_tag);
        match cur {
            None => first,
            Some(c) if self.hero_tags.contains(&c.tag) => first,
            _ => None,
        }
    }

    fn focus_moved(&mut self, st: &mut State, now: f64) -> bool {
        // The art of a game on the shelf is already on the GPU: the stage switches in the very frame
        // focus moves, not one later.
        let before = (self.stage.clone(), self.bd.clone());
        self.settle_want(st, now);
        self.sync_backdrop(st);
        before != (self.stage.clone(), self.bd.clone())
    }

    fn tick(&mut self, st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        let first = Home::recent(st).first().map(|g| g.id.clone());
        self.stage_id = self.stage.clone().or(first);
        let wake = self.settle_want(st, now);
        let recent = Home::recent(st);
        // The first stage has nothing before it: its art simply fades in. Only once there is a
        // stage (the library may land after the first tick; latching before it left Home with no
        // art behind the focused game until focus moved to another game).
        if !self.bd_seen && self.stage_id.is_some() {
            self.bd_seen = true;
            self.bd = self.stage_id.clone();
        }
        // Preload the stage art and logo of the focused game and its neighbours (the engine keeps
        // the decoded textures of recent ones, so going back is instant too).
        let at = self.stage_id.as_ref().and_then(|id| recent.iter().position(|g| &g.id == id));
        let f = self.strip_focus.filter(|f| *f < recent.len()).or(at).unwrap_or(0).min(recent.len().saturating_sub(1));
        let lo = f.saturating_sub(PRELOAD);
        let mut want: Vec<(String, Vec<ArtWant>)> = Vec::new();
        let mut ids: Vec<&Game> = recent[lo..(f + PRELOAD + 1).min(recent.len())].to_vec();
        if let Some(g) = at.and_then(|i| recent.get(i)).filter(|g| !ids.iter().any(|x| x.id == g.id)) {
            ids.push(g);
        }
        // Focus first, then the nearer ones, so the loader's queue serves them in that order.
        ids.sort_by_key(|g| recent.iter().position(|x| x.id == g.id).map_or(99, |i| i.abs_diff(f)));
        for g in ids {
            want.push((g.id.clone(), stage_art(g)));
        }
        if st.art_want != want {
            st.art_want = want;
        }
        self.sync_backdrop(st);
        wake
    }
}

/// The sources of the art behind Home for a game.
fn backdrop_srcs(g: &Game) -> Vec<String> {
    // The hero, then the header when no hero link loads (a CDN 404).
    g.art.hero.iter().chain(g.art.header.iter()).cloned().collect()
}

/// What the stage draws for a game: the backdrop (window wide, 40rem) and the lockup's logo.
fn stage_art(g: &Game) -> Vec<ArtWant> {
    let mut v = vec![ArtWant { sources: backdrop_srcs(g), w: 0.0, h: 40.0, fit: ArtFit::Cover(0.25) }];
    if !g.art.logo.is_empty() {
        v.push(ArtWant { sources: g.art.logo.clone(), w: 34.0, h: 8.5, fit: ArtFit::Contain(0.0, 1.0) });
    }
    v
}

/// A free game (Free to claim): the store's art, the title, until when and where. A opens the
/// store page in the browser, where it is claimed (Rare never claims for you).
fn free_card(ui: &mut Ui, acts: &mut Acts, f: &crate::data::knowledge::FreeGame, now: i64) -> El {
    let srcs: Vec<String> = f.image.iter().cloned().collect();
    let pic = parts::art(ui, &srcs, &f.title, Variant::Hero, 18.0, 8.4, 0.5);
    let art = ui.bx(s().wh(18.0, 8.4)).bg(tok::L3).clip().child(pic);
    let line = format!("{} \u{b7} {}", crate::data::knowledge::free_line(f, now), f.store_name());
    let c = ui
        .bx(s().col().w(18.0))
        .bg(tok::L1)
        .radius(ui.r(0.5))
        .clip()
        .child(art)
        .child(ui.bx(s().col().px(0.9).py(0.7).gap(0.2)).child(ui.text(f.title.clone(), T::new(Ty::Meta).w(600).trunc())).child(ui.text(line, T::new(Ty::Label).c(tok::MUTED).trunc())));
    let hint = if f.upcoming { format!("Open on {}", f.store_name()) } else { format!("Claim on {}", f.store_name()) };
    let tag = acts.tag(Act::OpenWeb(f.url.clone()));
    kit::nav(kit::tile(c, 1.03), tag, Some(&hint), None, false).key(key(&f.id))
}

/// Your stats, your year and the wishlist, then wishlist games on sale (biggest cut first).
fn play_rail(ui: &mut Ui, acts: &mut Acts) -> El {
    use crate::data::wishlist::{self, money};
    let (n, sale, deals) = wishlist::with(|w| {
        let shown = w.shown();
        let deals: Vec<(String, String, Option<u32>, crate::data::knowledge::Price)> = shown.iter().filter_map(|i| w.prices.get(&i.id).filter(|p| p.cut > 0 && p.price.is_some()).map(|p| (i.id.clone(), i.title.clone(), i.steam, p.clone()))).take(8).collect();
        (shown.len(), w.on_sale(), deals)
    });
    let year = crate::features::stats::civil((crate::data::model::now_secs() + crate::features::stats::utc_offset(crate::data::model::now_secs())).div_euclid(86_400)).0;
    let wl_sub = match (n, sale) {
        (0, _) => "Games to keep an eye on".to_string(),
        (n, 0) => format!("{n} {}", if n == 1 { "game" } else { "games" }),
        (n, s) => format!("{n} {}, {s} on sale", if n == 1 { "game" } else { "games" }),
    };
    let mut tiles = vec![
        word_tile(ui, acts, Act::Open(Overlay::Other(crate::features::stats::STATS)), "Open", 12.0, 12.0, Some("chart-line"), "Your stats", "Playtime and streaks", false).key(key("pl-stats")),
        word_tile(ui, acts, Act::Open(Overlay::Other(crate::features::stats::YEAR)), "Open", 12.0, 12.0, Some("sparkles"), "Your year", &format!("Your {year} on Rare so far"), false).key(key("pl-year")),
        word_tile(ui, acts, Act::Open(Overlay::Other(crate::features::wishlist_page::KIND)), "Open", 12.0, 12.0, Some("heart"), "Wishlist", &wl_sub, false).key(key("pl-wish")),
    ];
    for (id, title, steam, p) in deals {
        let mut srcs: Vec<String> = steam.or(p.steam_app_id).map(|a| format!("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{a}/header.jpg")).into_iter().collect();
        srcs.extend(p.thumb.clone());
        let pic = parts::art(ui, &srcs, &title, Variant::Hero, 18.0, 8.4, 0.5);
        let art = ui.bx(s().wh(18.0, 8.4)).bg(tok::L3).clip().child(pic);
        let line = format!("{} \u{b7} -{}%{}", money(p.price.unwrap_or(0.0)), p.cut, p.store.as_ref().map(|s| format!(" at {s}")).unwrap_or_default());
        let c = ui
            .bx(s().col().w(18.0))
            .bg(tok::L1)
            .radius(ui.r(0.5))
            .clip()
            .child(art)
            .child(ui.bx(s().col().px(0.9).py(0.7).gap(0.2)).child(ui.text(title.clone(), T::new(Ty::Meta).w(600).trunc())).child(ui.text(line, T::new(Ty::Label).c(tok::MUTED).trunc())));
        let tag = acts.tag(Act::Open(Overlay::Other(crate::features::wishlist_page::KIND)));
        tiles.push(kit::nav(kit::tile(c, 1.03), tag, Some("Wishlist"), None, false).key(key(&id)));
    }
    let note = (sale > 0).then(|| format!("{sale} on sale"));
    kit::rail(ui, s(), "play", Some("Stats and wishlist"), note.as_deref(), tiles)
}

/// The Launchpad tile at the end of On Sui: the rocket on top like Cloud saves, the words at the bottom.
fn rare_card_sq(ui: &mut Ui, acts: &mut Acts, none_live: Option<bool>) -> El {
    let rocket = kit::icon(ui, "rocket", 1.6, tok::ICON2).style(|st| st.margin.bottom = rare_render::taffy::prelude::LengthPercentageAuto::auto());
    let c = ui
        .bx(s().col().justify_end().gap(0.2).size(12.0).p(1.0))
        .bg(tok::L1)
        .radius(ui.r(0.5))
        .border(1.0, tok::HAIR)
        .child(rocket)
        .child(ui.text("Launchpad", T::new(Ty::Body).w(700).trunc()))
        .child(ui.text(if none_live == Some(true) { "No live mints" } else { "New drops" }, T::new(Ty::Meta).c(tok::MUTED).trunc()));
    let tag = acts.tag(Act::OpenWeb("https://rare.market/launchpad".into()));
    kit::nav(kit::tile(c, 1.06), tag, Some("Open"), None, false)
}

/// One minting-now tile (HomeSui.tsx MintCard): art, dark fade, name and the supply line.
fn mint_card(ui: &mut Ui, acts: &mut Acts, m: &rare_sui::live::Mint) -> El {
    let srcs: Vec<String> = m.image_url.iter().cloned().collect();
    let pic = parts::art(ui, &srcs, &m.name, Variant::Cover, 12.0, 12.0, 0.5);
    let c = ui
        .bx(s().size(12.0))
        .bg(tok::L3)
        .radius(ui.r(0.5))
        .border(1.0, tok::HAIR)
        .clip()
        .child(pic)
        .child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).hp(0.5)).look(Look::default().grad(Gradient { dir: GradDir::ToTop, stops: [tok::BLACK.alpha(0.95), tok::BLACK.alpha(0.5), tok::BLACK.alpha(0.0)], mid: 0.5 })))
        .child(ui.bx(s().abs(None, Some(1.0), Some(0.9), Some(1.0)).col()).child(ui.text(m.name.clone(), T::new(Ty::Body).w(700).c(tok::WHITE).trunc())).child(ui.text(m.sub.clone(), T::new(Ty::Meta).c(tok::WHITE.alpha(0.6)).trunc())));
    let tag = acts.tag(Act::OpenWeb(m.href.clone()));
    kit::nav(kit::tile(c, 1.06), tag, Some("Mint"), None, false).key(key(&m.key))
}
