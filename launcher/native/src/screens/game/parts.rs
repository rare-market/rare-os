//! Game pieces several Phase 1 screens share (components/ui.tsx, components/Art.tsx, the HeroLogo of
//! screens/Home.tsx): the 2:3 game tile, an app's icon tile, the generated cover for a game with no
//! art, the logo lockup with its title fallback, and the played/playtime lines.

use crate::act::Act;
use crate::data::model::{fmt_ago, source_label, Game};
use crate::kit::{self, s, tok, ArtFit, DotTone, Ty, Ui, S, T};
use crate::page::Acts;
use crate::state::State;
use rare_render::view::{key, El, Look};

/// Which `<Art variant>`: the size it is drawn at and its fallback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
    Cover,
    Hero,
    Header,
}

/// Art.tsx `Generated`: a game with no art at all, its name on the layer colour (`w` x `h` rem),
/// broken only between words and sized to fit (`kit::title_cover`).
pub fn generated(ui: &mut Ui, title: &str, v: Variant, w: f32, h: f32) -> El {
    match v {
        Variant::Hero => ui.bx(s().fill()).bg(tok::L2),
        Variant::Cover => kit::title_cover(ui, title, w, h, 1.05),
        Variant::Header => kit::title_cover(ui, title, w, h, 0.95),
    }
}

/// `<Art sources title variant>` filling its box, with the generated fallback.
pub fn art(ui: &mut Ui, sources: &[String], title: &str, v: Variant, w: f32, h: f32, fy: f32) -> El {
    let fb = generated(ui, title, v, w, h);
    kit::art(ui, s().fill(), sources, w, h, ArtFit::Cover(fy), Some(fb))
}

/// Drawn as an icon tile: an emulator or app, or an Android app, with no cover art.
pub fn icon_art(g: &Game) -> bool {
    (g.source == "apps" || g.source == "android") && g.art.cover.is_empty()
}

/// components/ui.tsx `AppArt`: an app or emulator with no cover, its icon (or initial) on layer 02.
pub fn app_art(ui: &mut Ui, g: &Game) -> El {
    let initial = ui.text(g.title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(), T::new(Ty::px(2.4)).w(600).c(tok::fg(0.4)));
    // An icon file that is gone or unreadable shows the initial, never an empty tile.
    let inner = if g.art.icon.is_empty() { initial } else { kit::art(ui, s().full(), &g.art.icon, 4.5, 4.5, ArtFit::Contain(0.5, 0.5), Some(ui.bx(s().fill().row().items_center().justify_center()).child(initial))) };
    ui.bx(s().fill().col().items_center().justify_center().gap(0.9).p(1.0)).bg(tok::L2).child(ui.bx(s().size(4.5).row().items_center().justify_center()).child(inner))
}

/// "49 h played" (Home.tsx fmtPlaytime).
pub fn fmt_playtime(m: Option<i64>) -> String {
    match m.filter(|m| *m > 0) {
        None => String::new(),
        Some(m) if m < 60 => format!("{m} min played"),
        Some(m) => format!("{} h played", (m as f64 / 60.0).round()),
    }
}

/// The library game tile (components/ui.tsx `GameTile`): a 2:3 cover, `.tile` focus, title and
/// store under it (`bare`: art only, the Library grid). A opens the game page unless `act` says.
pub fn game_tile(ui: &mut Ui, st: &State, acts: &mut Acts, g: &Game, def: bool, bare: bool, w: f32, act: Option<Act>) -> El {
    let running = st.is_running(&g.id);
    let h = w * 1.5;
    let pic = if icon_art(g) { app_art(ui, g) } else { art(ui, &g.art.cover, &g.title, Variant::Cover, w, h, 0.5) };
    let mut btn = ui.bx(s().wh(w, h)).bg(tok::L3).radius(ui.r(0.5)).clip().child(pic);
    if running {
        btn = btn.child(kit::dot(ui, s().abs(Some(0.6), Some(0.6), None, None), DotTone::Primary, true));
    }
    if g.foreign {
        btn = btn.child(kit::dot(ui, s().abs(Some(0.5), Some(0.5), None, None), DotTone::Attention, false));
    }
    // A Steam download or install in progress: a bar over the bottom of the art.
    if let Some(bar) = crate::features::downloads::tile_overlay(ui, st, &g.id) {
        btn = btn.child(bar);
    }
    // Owned but not installed: a download mark. An Epic or GOG download: its bar.
    if !g.installed {
        let mark = ui.bx(s().abs(None, Some(0.55), Some(0.55), None).size(1.9).row().items_center().justify_center()).bg(tok::BG.alpha(0.72)).radius(ui.r(0.45)).child(kit::icon(ui, "download", 1.0, tok::fg(0.9)));
        btn = btn.child(mark);
    }
    if let Some(j) = crate::features::stores::job(&g.id) {
        let dim = j.phase != rare_stores::jobs::Phase::Running;
        btn = btn.child(ui.bx(s().abs(None, Some(0.5), Some(0.5), Some(0.5))).child(crate::features::downloads::bar(ui, None, j.pct / 100.0, dim)));
    }
    // Owned and not installed (nothing downloading for it yet): A installs it (the drive sheet);
    // the game page is on Menu, Details.
    let busy = crate::features::downloads::of(st, &g.id).is_some() || crate::features::stores::job(&g.id).is_some();
    let install = match act {
        None if !g.installed && !running && !busy && g.source == "steam" && g.id.starts_with("steam:") => Some(Act::Install(g.id.clone())),
        None if !g.installed && !running && !busy && crate::screens::game::stores::managed(g) => Some(Act::Legacy(format!("stores:install:{}", g.id))),
        _ => None,
    };
    let a_hint = if install.is_some() { "Install" } else { "Details" };
    let tag = acts.game(install.or(act).unwrap_or_else(|| Act::OpenGame(g.id.clone())), &g.id);
    let btn = btn.nav(|f| {
        let f = f.tag(tag).hints(Some(a_hint), None, None).game().label(&g.title);
        if def {
            f.default()
        } else {
            f
        }
    });
    let btn = kit::tile(btn, 1.06);
    let mut col = ui.bx(s().col().w(w)).child(btn);
    if !bare {
        let name = ui.text_s(s().mt(0.85).px(0.1), g.title.clone(), T::new(Ty::Meta).w(500).c(tok::fg(0.85)).trunc());
        let name = name.clone().focus_color(tok::FG);
        let sub = if running {
            "Playing".to_string()
        } else if let Some(j) = crate::features::stores::job(&g.id) {
            j.status()
        } else if !g.installed {
            format!("{}, not installed", source_label(&g.source))
        } else {
            source_label(&g.source).to_string()
        };
        col = col.child(name).child(ui.text_s(s().px(0.1), sub, T::new(Ty::Label).c(tok::MUTED).trunc()));
    }
    col.key(key(&g.id))
}

/// Home.tsx `HeroLogo`: the logo art bottom-left in its box, or the title when there is none.
pub fn hero_logo(ui: &mut Ui, g: &Game, big: bool) -> El {
    let (h, mw) = if big { (8.5, 34.0) } else { (6.0, 26.0) };
    // The title when there is no logo: at most two lines broken between words, smaller for a
    // long one, cut with an ellipsis only past that.
    let (base, lead) = if big { (3.0, 1.08) } else { (2.75, 3.0 / 2.75) };
    let (text, size) = kit::fit_title(&g.title, mw, h, base, base * 0.55, 2);
    let title = ui.text(text, T::new(Ty::Px(size, size * lead)).w(700).track(-0.03));
    let inner = if g.art.logo.is_empty() { title } else { kit::art(ui, s().wh(mw, h), &g.art.logo, mw, h, ArtFit::Contain(0.0, 1.0), Some(title)) };
    ui.bx(s().row().items_end().h(h).max_w(mw)).child(inner)
}

/// The "Web · Played 1 h ago · 49 h played" line.
pub fn meta_line(ui: &mut Ui, st: S, parts: &[String], t: T) -> El {
    let mut row = ui.bx(st.row().items_center().gap(0.6));
    for (i, m) in parts.iter().filter(|m| !m.is_empty()).enumerate() {
        let mut cell = ui.bx(s().row().items_center().gap(0.6));
        if i > 0 {
            cell = cell.child(ui.text("\u{b7}", t.c(tok::fg(0.3))));
        }
        row = row.child(cell.child(ui.text(m.clone(), t)).key(key(m)));
    }
    row
}

/// The line under a game: store, last played, playtime.
pub fn game_meta(g: &Game) -> Vec<String> {
    vec![source_label(&g.source).to_string(), fmt_ago(g.last_played), fmt_playtime(g.playtime_minutes)]
}

/// No look (for `focus_look` copies).
pub fn plain() -> Look {
    Look::default()
}
