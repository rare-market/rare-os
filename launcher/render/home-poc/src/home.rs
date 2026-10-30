//! Home, natively: the top bar and its five tabs, the game strip with real covers, the stage art,
//! the stage lockup with Play and Details, the Your Rare panel and the hint bar. A copy of
//! launcher/src/screens/Home.tsx, Chrome.tsx and app.css (sizes in rem, the same tokens).
//! Library is a cover grid so the tab switch and a long sweep have real images to move.

use crate::data::{self, Game, Live};
use crate::icons;
use crate::ui::s;
use rare_render::focus::{Dir, FocusMap};
use rare_render::input::Pad;
use rare_render::scene::{GradDir, Gradient};
use rare_render::theme::{motion, size, tok};
use rare_render::{Color, Fit, Id, ImageLoader, ImgReq, Node, Scene, Src, TextStyle, TextSys};

const G_STRIP: u32 = 1;
const G_HERO: u32 = 2;
const G_PANEL: u32 = 3;
const G_GRID: u32 = 10;

/// What the app should do after an input.
pub enum Action {
    None,
    /// Focus moved (the `move` sound and rumble would go here).
    Moved,
    Tab,
    Select(String),
}

struct Tile {
    id: Id,
    label: Id,
    game: Option<usize>,
}

struct Btn {
    id: Id,
    text: Id,
    icon: Option<Id>,
    base: Color,
    fg: Color,
}

pub struct Home {
    rem: f32,
    pub games: Vec<Game>,
    tabs: Vec<Id>,
    underline: Id,
    pub tab: usize,
    underline_dirty: bool,
    pages: Vec<Id>,
    backdrop: Id,
    back_layers: [Id; 2],
    back_cur: usize,
    back_game: Option<usize>,
    stage_slots: [Id; 2],
    stage_cur: usize,
    stage_game: Option<usize>,
    pending_stage: Option<(usize, f64)>,
    rail_row: Id,
    rail_scroll: f32,
    tiles: Vec<Tile>,
    buttons: Vec<Btn>,
    grid_scroll_node: Id,
    grid_scroll: f32,
    grid: Vec<(Id, usize)>,
    grid_view_h: f32,
    focus: FocusMap,
    pub cur: Option<Id>,
    page_focus: Vec<Option<Id>>,
    clock: Id,
    battery: Id,
    panel_price: Id,
    panel_change: Id,
    bar_price: Id,
    bar_change: Id,
    bar_tps: Id,
    bar_gas: Id,
    play_label: Id,
}

fn ts(rem: f32, size_rem: f32, line_rem: f32, weight: u16) -> TextStyle {
    TextStyle::new((size_rem * rem).round(), weight).line((line_rem * rem).round())
}

fn tile_px(rem: f32, w_rem: f32, h_rem: f32, k: f32) -> (u32, u32) {
    ((w_rem * rem * k).round() as u32, (h_rem * rem * k).round() as u32)
}

impl Home {
    pub fn build(scene: &mut Scene, t: &mut TextSys, img: &mut ImageLoader, games: Vec<Game>, live: &Live) -> Home {
        let r = scene.rem;
        let root = scene.root;
        scene.ring_gap_color = tok::BG;
        scene.ring_color = tok::FG;
        let mut root_style = scene.style(root);
        root_style.flex_direction = rare_render::taffy::FlexDirection::Column;
        root_style.position = rare_render::taffy::style::Position::Relative;
        scene.set_style(root, root_style);
        scene.n(root).bg.snap(tok::BG);

        // Favourites lead the shelf in the web launcher; with none, everything by last played.
        let mut shelf: Vec<usize> = (0..games.len()).filter(|i| games[*i].source != "apps").collect();
        shelf.sort_by_key(|i| std::cmp::Reverse(games[*i].last_played.unwrap_or(0)));
        shelf.truncate(16);

        // ---- Backdrop: art behind the top, two layers that cross-fade, and the scrims. ----
        let backdrop = scene.add(root, s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h(40.0 * r).done(), Node::bg(tok::BG).clip());
        let back_layers = [
            scene.add(backdrop, s().fill().done(), Node::default().opacity(0.0)),
            scene.add(backdrop, s().fill().done(), Node::default().opacity(0.0)),
        ];
        let bg = tok::BG;
        scene.add(backdrop, s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h(7.0 * r).done(), Node::default().grad(Gradient { dir: GradDir::ToBottom, stops: [bg.alpha(0.8), bg.alpha(0.4), bg.alpha(0.0)], mid: 0.5 }));
        scene.add(backdrop, s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(0.62).done(), Node::default().grad(Gradient { dir: GradDir::ToRight, stops: [bg, bg.alpha(0.55), bg.alpha(0.0)], mid: 0.5 }));
        scene.add(backdrop, s().abs(None, Some(0.0), Some(0.0), Some(0.0)).hp(0.45).done(), Node::default().grad(Gradient { dir: GradDir::ToTop, stops: [bg, bg.alpha(0.6), bg.alpha(0.0)], mid: 0.5 }));

        // ---- Top bar ----
        let top = scene.add(root, s().row().items_center().h(4.25 * r).px(2.5 * r).done(), Node::default());
        let brand = scene.add(top, s().row().items_center().gap(0.5 * r).done(), Node::default());
        let mark = img.request(ImgReq { src: Src::Static("brand/mark.png", icons::MARK_PNG), w: (1.6 * r) as u32, h: (1.6 * r) as u32, fit: Fit::Contain });
        scene.add(brand, s().wh(1.6 * r, 1.6 * r).done(), Node::default().image(mark).contain([0.5, 0.5]));
        scene.add_text(t, brand, s().done(), "rare", ts(r, size::TITLE, 1.75, 600).tracking(-0.01), tok::FG);
        let tabs_row = scene.add(top, s().row().items_center().gap(1.25 * r).pt(0.0).done(), Node::default());
        let mut st = scene.style(tabs_row);
        st.margin.left = rare_render::taffy::prelude::length(2.0 * r);
        scene.set_style(tabs_row, st);
        chip(scene, t, tabs_row, r, "LB");
        let mut tabs = Vec::new();
        for (i, name) in ["Home", "Library", "Sui", "Market", "Wallet"].iter().enumerate() {
            let id = scene.add_text(t, tabs_row, s().done(), name, ts(r, size::ROW, 1.5, 500), if i == 0 { tok::FG } else { tok::MUTED });
            tabs.push(id);
        }
        chip(scene, t, tabs_row, r, "RB");
        let underline = scene.add(top, s().abs(Some(4.25 * r - 0.3 * r), None, None, Some(0.0)).wh(2.0 * r, 0.1 * r).done(), Node::bg(tok::FG).radius(0.05 * r));
        let right = scene.add(top, s().row().items_center().gap(1.25 * r).ml_auto().done(), Node::default());
        let wifi = img.request(icons::svg_req("wifi", icons::WIFI, 1.15 * r));
        scene.add(right, s().wh(1.15 * r, 1.15 * r).done(), Node::default().icon(wifi, tok::FG));
        let batt_row = scene.add(right, s().row().items_center().gap(0.45 * r).done(), Node::default());
        let pct = rare_render::platform::battery_percent();
        let bat = img.request(icons::svg_req(&format!("battery{}", pct.unwrap_or(100)), &icons::battery(pct.unwrap_or(100)), 1.35 * r));
        scene.add(batt_row, s().wh(1.35 * r, 1.35 * r).done(), Node::default().icon(bat, tok::FG));
        let battery = scene.add_text(t, batt_row, s().done(), &pct.map(|p| format!("{p}%")).unwrap_or_default(), ts(r, size::BODY, 1.375, 400).tabular(), tok::FG.alpha(0.75));
        let clock = scene.add_text(t, right, s().done(), &rare_render::platform::clock_text(), ts(r, size::ROW, 1.5, 500).tabular(), tok::FG);

        // ---- Pages ----
        let content = scene.add(root, s().grow().done(), Node::default());
        let mut pages = Vec::new();
        for i in 0..5 {
            pages.push(scene.add(content, s().fill().col().done(), Node::default().opacity(if i == 0 { 1.0 } else { 0.0 })));
        }
        for i in 1..5 {
            scene.n(pages[i]).hidden = true;
        }

        // Home: the strip.
        let home = pages[0];
        let rail = scene.add(home, s().row().h(14.5 * r).pt(1.0 * r).px(2.5 * r).done(), Node::default().clip());
        let rail_row = scene.add(rail, s().row().items_start().gap(0.75 * r).done(), Node::default());
        let mut tiles = Vec::new();
        let (tw, th) = tile_px(r, 5.5, 5.5, 1.45);
        let step = 5.5 + 0.75;
        for (k, gi) in shelf.iter().copied().chain(std::iter::once(usize::MAX)).enumerate() {
            let game = (gi != usize::MAX).then_some(gi);
            let mut node = Node::bg(if game.is_some() { tok::L3 } else { tok::L2 }).radius(0.5 * r).origin(0.0, 0.0);
            if let Some(g) = game.map(|g| &games[g]) {
                let list = if g.art.cover.is_empty() { &g.art.header } else { &g.art.cover };
                if let Some(src) = data::first(list) {
                    node = node.image(img.request(ImgReq { src, w: tw, h: th, fit: Fit::Cover { fy: 0.28 } }));
                }
            }
            let id = scene.add(rail_row, s().wh(5.5 * r, 5.5 * r).done(), node);
            if game.is_none() {
                let grid = img.request(icons::svg_req("layout-grid", icons::LAYOUT_GRID, 1.5 * 1.45 * r));
                let inner = scene.add(id, s().fill().items_center().justify_center().done(), Node::default());
                scene.add(inner, s().wh(1.5 * r, 1.5 * r).done(), Node::default().icon(grid, tok::FG));
            }
            // The label sits under the grown tile, at the grown size (crisp, not a scaled bitmap).
            let lx = k as f32 * step * r;
            let label = scene.add(rail_row, s().abs(Some(5.5 * 1.45 * r + 0.45 * 1.45 * r), None, None, Some(lx)).col().done(), Node::default().opacity(0.0));
            let (title, sub) = match game {
                Some(g) => (games[g].title.clone(), data::source_label(&games[g].source).to_string()),
                None => ("All games".to_string(), format!("{} in your library", games.len())),
            };
            scene.add_text(t, label, s().done(), &title, ts(r, 0.68 * 1.45, 0.68 * 1.45 * 1.25, 600), tok::FG);
            scene.add_text(t, label, s().done(), &sub, ts(r, 0.55 * 1.45, 0.55 * 1.45 * 1.3, 400), tok::FG.alpha(0.6));
            tiles.push(Tile { id, label, game });
        }

        // Home: stage row (lockup + buttons on the left, Your Rare on the right).
        let stage_row = scene.add(home, s().row().items_end().gap(2.0 * r).px(2.5 * r).h(27.75 * r).done(), Node::default());
        let stage = scene.add(stage_row, s().grow().col().gap(1.0 * r).done(), Node::default());
        let slot_area = scene.add(stage, s().h(13.4 * r).done(), Node::default());
        let stage_slots = [
            scene.add(slot_area, s().fill().col().justify_end().gap(1.0 * r).done(), Node::default().opacity(0.0)),
            scene.add(slot_area, s().fill().col().justify_end().gap(1.0 * r).done(), Node::default().opacity(0.0)),
        ];
        let btn_row = scene.add(stage, s().row().gap(0.75 * r).mt(0.35 * r).done(), Node::default());
        let play_icon = img.request(icons::svg_req("play", icons::PLAY, 1.1 * r));
        let play = button(scene, t, btn_row, r, "Play", tok::BRAND, Some(play_icon), true, Some(10.0 * r));
        let details = button(scene, t, btn_row, r, "Details", Color::rgb(62, 56, 77).alpha(0.6), None, true, None);
        let play_label = play.text;

        let panel = scene.add(stage_row, s().col().w(24.0 * r).p(1.25 * r).gap(1.0 * r).self_end().done(), Node::bg(tok::L1).radius(1.0 * r));
        scene.add_text(t, panel, s().done(), "Your items", ts(r, size::LABEL, 1.0, 400), tok::MUTED);
        scene.add_text(t, panel, s().done(), "Sign in with a passkey to see your items.", ts(r, size::BODY, 1.375, 400).wrap(), tok::MUTED);
        let sign_row = scene.add(panel, s().row().done(), Node::default());
        let signin = button(scene, t, sign_row, r, "Sign in", tok::BRAND, None, false, None);
        let price_row = scene.add(panel, s().row().items_center().gap(0.45 * r).mt(0.9 * r).done(), Node::default());
        let drop = img.request(icons::svg_req("sui", icons::SUI_DROP, 0.9 * r));
        scene.add(price_row, s().wh(0.9 * 0.7823 * r, 0.9 * r).done(), Node::default().image(drop).contain([0.5, 0.5]));
        scene.add_text(t, price_row, s().done(), "SUI", ts(r, size::META, 1.125, 400), tok::MUTED);
        let panel_price = scene.add_text(t, price_row, s().done(), "", ts(r, size::META, 1.125, 400).tabular(), tok::MUTED);
        let panel_change = scene.add_text(t, price_row, s().ml_auto().done(), "", ts(r, size::META, 1.125, 400).tabular(), tok::POS);

        // Library: a cover grid.
        let lib = pages[1];
        let lib_view = scene.add(lib, s().grow().done(), Node::default().clip());
        let grid_scroll_node = scene.add(lib_view, s().col().wp(1.0).px(2.5 * r).pt(1.4 * r).pb(2.0 * r).gap(1.0 * r).done(), Node::default());
        scene.add_text(t, grid_scroll_node, s().done(), "Library", ts(r, size::TITLE, 1.75, 600).tracking(-0.01), tok::FG);
        let grid_box = scene.add(grid_scroll_node, s().row().wrap().gap(1.0 * r).done(), Node::default());
        let mut order: Vec<usize> = (0..games.len()).collect();
        order.sort_by(|a, b| games[*a].title.to_lowercase().cmp(&games[*b].title.to_lowercase()));
        let (cw, ch) = tile_px(r, 9.0, 13.5, 1.06);
        let mut grid = Vec::new();
        for gi in order {
            let g = &games[gi];
            let cell = scene.add(grid_box, s().col().w(9.0 * r).gap(0.5 * r).done(), Node::default());
            let mut node = Node::bg(tok::L3).radius(0.5 * r);
            let src = data::first(&g.art.cover).or_else(|| if g.source == "apps" { data::first(&g.art.icon) } else { None });
            let is_icon = g.art.cover.is_empty();
            if let Some(src) = src {
                node = if is_icon {
                    node.image(img.request(ImgReq { src, w: (4.0 * r) as u32, h: (4.0 * r) as u32, fit: Fit::Contain })).contain([0.5, 0.5])
                } else {
                    node.image(img.request(ImgReq { src, w: cw, h: ch, fit: Fit::Cover { fy: 0.5 } }))
                };
            }
            let cover = scene.add(cell, s().wh(9.0 * r, 13.5 * r).done(), node);
            if src_missing(g) {
                let inner = scene.add(cover, s().fill().items_center().justify_center().px(0.75 * r).done(), Node::default());
                scene.add_text(t, inner, s().done(), &g.title, ts(r, size::BODY, 1.375, 600).wrap(), tok::FG.alpha(0.7));
            }
            scene.add_text(t, cell, s().done(), &g.title, ts(r, size::SMALL, 1.25, 500).truncate(), tok::FG);
            grid.push((cover, gi));
        }

        // Sui, Market, Wallet: titles only in the preview.
        for (i, name) in [(2, "Sui"), (3, "Market"), (4, "Wallet")] {
            let col = scene.add(pages[i], s().col().px(2.5 * r).pt(1.4 * r).gap(0.75 * r).done(), Node::default());
            scene.add_text(t, col, s().done(), name, ts(r, size::TITLE, 1.75, 600).tracking(-0.01), tok::FG);
            scene.add_text(t, col, s().done(), "Not in the native preview yet.", ts(r, size::BODY, 1.375, 400), tok::MUTED);
        }

        // ---- Bottom bar ----
        let bar = scene.add(root, s().row().items_center().h(3.0 * r).done(), Node::bg(tok::BG).border(0.0, tok::HAIR));
        scene.add(bar, s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h(1.0).done(), Node::bg(tok::HAIR));
        let cell = |scene: &mut Scene| scene.add(bar, s().row().items_center().gap(0.45 * r).px(1.05 * r).hp(1.0).done(), Node::default());
        let c1 = cell(scene);
        let pl = scene.style(c1);
        let mut pl2 = pl.clone();
        pl2.padding.left = rare_render::taffy::prelude::length(2.5 * r);
        scene.set_style(c1, pl2);
        let drop2 = img.request(icons::svg_req("sui-bar", icons::SUI_DROP, 0.95 * r));
        scene.add(c1, s().wh(0.95 * 0.7823 * r, 0.95 * r).done(), Node::default().image(drop2).contain([0.5, 0.5]));
        scene.add_text(t, c1, s().done(), "SUI", ts(r, size::SMALL, 1.25, 500), tok::FG);
        let bar_price = scene.add_text(t, c1, s().done(), "", ts(r, size::SMALL, 1.25, 500).tabular(), tok::FG);
        let bar_change = scene.add_text(t, c1, s().done(), "", ts(r, size::SMALL, 1.25, 500).tabular(), tok::POS);
        divider(scene, bar, r);
        let c2 = cell(scene);
        scene.add_text(t, c2, s().done(), "TPS", ts(r, size::SMALL, 1.25, 400), tok::MUTED);
        let bar_tps = scene.add_text(t, c2, s().done(), "", ts(r, size::SMALL, 1.25, 500).tabular(), tok::FG);
        divider(scene, bar, r);
        let c3 = cell(scene);
        scene.add_text(t, c3, s().done(), "Gas", ts(r, size::SMALL, 1.25, 400), tok::MUTED);
        let bar_gas = scene.add_text(t, c3, s().done(), "", ts(r, size::SMALL, 1.25, 500).tabular(), tok::FG);
        divider(scene, bar, r);
        let hints = scene.add(bar, s().row().items_center().gap(1.5 * r).ml_auto().pt(0.0).done(), Node::default());
        let mut hs = scene.style(hints);
        hs.padding.right = rare_render::taffy::prelude::length(2.5 * r);
        scene.set_style(hints, hs);
        for (g, label) in [("A", "Play"), ("B", "Back"), ("X", "Details"), ("Y", "Search")] {
            let h = scene.add(hints, s().row().items_center().gap(0.45 * r).done(), Node::default());
            let c = scene.add(h, s().wh(1.2 * r, 1.2 * r).items_center().justify_center().done(), Node::bg(Color::rgb(220, 218, 226)).radius(0.6 * r));
            scene.add_text(t, c, s().done(), g, ts(r, 0.68, 0.8, 700), tok::BG);
            scene.add_text(t, h, s().done(), label, ts(r, size::SMALL, 1.25, 400), tok::FG);
        }
        let h = scene.add(hints, s().row().items_center().gap(0.3 * r).done(), Node::default());
        chip(scene, t, h, r, "LB");
        chip(scene, t, h, r, "RB");
        scene.add_text(t, h, s().done(), "Tabs", ts(r, size::SMALL, 1.25, 400), tok::FG);
        for (icon, name, label) in [(icons::MENU, "menu", "Options"), (icons::ELLIPSIS, "ellipsis", "Quick settings")] {
            let h = scene.add(hints, s().row().items_center().gap(0.45 * r).done(), Node::default());
            let c = scene.add(h, s().wh(1.2 * r, 1.2 * r).items_center().justify_center().done(), Node::default().radius(0.6 * r).border(0.08 * r, tok::FG.alpha(0.55)));
            let k = img.request(icons::svg_req(name, icon, 0.75 * r));
            scene.add(c, s().wh(0.75 * r, 0.75 * r).done(), Node::default().icon(k, tok::FG));
            scene.add_text(t, h, s().done(), label, ts(r, size::SMALL, 1.25, 400), tok::FG);
        }

        let mut focus = FocusMap::default();
        focus.remember = vec![G_STRIP, G_HERO, G_PANEL];
        let mut home = Home {
            rem: r,
            games,
            tabs,
            underline,
            tab: 0,
            underline_dirty: true,
            pages,
            backdrop,
            back_layers,
            back_cur: 1,
            back_game: None,
            stage_slots,
            stage_cur: 1,
            stage_game: None,
            pending_stage: None,
            rail_row,
            rail_scroll: 0.0,
            tiles,
            buttons: vec![play, details, signin],
            grid_scroll_node,
            grid_scroll: 0.0,
            grid,
            grid_view_h: 0.0,
            focus,
            cur: None,
            page_focus: vec![None; 5],
            clock,
            battery,
            panel_price,
            panel_change,
            bar_price,
            bar_change,
            bar_tps,
            bar_gas,
            play_label,
        };
        home.set_live(scene, t, live);
        home.register_focus();
        home
    }

    fn register_focus(&mut self) {
        self.focus.clear();
        match self.tab {
            0 => {
                for (i, tile) in self.tiles.iter().enumerate() {
                    self.focus.push(tile.id, G_STRIP, i as u32);
                }
                self.focus.push(self.buttons[0].id, G_HERO, 0);
                self.focus.push(self.buttons[1].id, G_HERO, 1);
                self.focus.push(self.buttons[2].id, G_PANEL, 0);
            }
            1 => {
                for (i, (id, _)) in self.grid.iter().enumerate() {
                    self.focus.push(*id, G_GRID, i as u32);
                }
            }
            _ => {}
        }
    }

    /// First frame: the stage on the first game and focus on the first tile (Home opens on the strip).
    pub fn start(&mut self, scene: &mut Scene, t: &mut TextSys, img: &mut ImageLoader, now: f64) {
        if let Some(first) = self.tiles.first().map(|x| x.id) {
            self.set_focus(scene, first, now, false);
        }
        if let Some(g) = self.tiles.first().and_then(|x| x.game) {
            self.show_stage(scene, t, img, g, now, false);
        }
    }

    fn is_tile(&self, id: Id) -> Option<usize> {
        self.tiles.iter().position(|x| x.id == id)
    }

    fn set_focus(&mut self, scene: &mut Scene, id: Id, now: f64, animate: bool) {
        let d = |v: f32| if animate { v } else { 0.0 };
        let r = self.rem;
        if let Some(old) = self.cur.take() {
            scene.n(old).ring.set(0.0, now, 0.0);
            scene.n(old).z = 0;
            if let Some(i) = self.is_tile(old) {
                scene.n(old).scale.set(1.0, now, d(motion::STRIP));
                scene.n(self.tiles[i].label).opacity.set(0.0, now, d(motion::FAST));
            } else if let Some(b) = self.buttons.iter().find(|b| b.id == old) {
                let (base, fg, text, icon) = (b.base, b.fg, b.text, b.icon);
                scene.n(old).bg.set(base, now, d(motion::FOCUS));
                scene.n(old).scale.set(1.0, now, d(motion::FOCUS));
                scene.n(text).text.as_mut().unwrap().color.set(fg, now, d(motion::FOCUS));
                if let Some(ic) = icon {
                    scene.n(ic).img.as_mut().unwrap().tint = Some(fg);
                }
            } else {
                scene.n(old).scale.set(1.0, now, d(motion::FOCUS));
            }
        }
        // The ring itself never animates (DESIGN.md: crisp, not animated).
        scene.n(id).ring.snap(1.0);
        scene.n(id).z = 2;
        if let Some(i) = self.is_tile(id) {
            scene.n(id).scale.set(1.45, now, d(motion::STRIP));
            scene.n(self.tiles[i].label).opacity.set(1.0, now, d(motion::FAST));
            for (k, tile) in self.tiles.iter().enumerate() {
                let shift = if k > i { 5.5 * 0.45 * r } else { 0.0 };
                scene.n(tile.id).tx.set(shift, now, d(motion::STRIP));
                scene.n(tile.label).tx.set(shift, now, d(motion::STRIP));
            }
            // Keep the grown tile inside the rail.
            let step = (5.5 + 0.75) * r;
            let view = scene.w - 5.0 * r;
            let right = i as f32 * step + 5.5 * 1.45 * r;
            let mut scroll = self.rail_scroll;
            if right - scroll > view {
                scroll = right - view;
            }
            if (i as f32 * step) < scroll {
                scroll = i as f32 * step;
            }
            if scroll != self.rail_scroll {
                self.rail_scroll = scroll;
                scene.n(self.rail_row).tx.set(-scroll, now, d(motion::BASE));
            }
        } else if let Some(b) = self.buttons.iter().find(|b| b.id == id) {
            let (text, icon) = (b.text, b.icon);
            scene.n(id).bg.set(tok::FG, now, d(motion::FOCUS));
            scene.n(id).scale.set(1.04, now, d(motion::FOCUS));
            scene.n(text).text.as_mut().unwrap().color.set(tok::BG, now, d(motion::FOCUS));
            if let Some(ic) = icon {
                scene.n(ic).img.as_mut().unwrap().tint = Some(tok::BG);
            }
        } else if let Some(k) = self.grid.iter().position(|(g, _)| *g == id) {
            scene.n(id).scale.set(1.06, now, d(motion::FOCUS));
            // Scroll the grid so the focused row shows.
            let rr = scene.rect(id);
            let top_of_view = scene.rect(self.pages[1])[1];
            let view_h = scene.rect(self.pages[1])[3];
            self.grid_view_h = view_h;
            let y = rr[1] - top_of_view + self.grid_scroll; // layout y within content
            let _ = k;
            let mut sc = self.grid_scroll;
            let pad = 1.4 * r;
            if y - sc < pad {
                sc = (y - pad).max(0.0);
            }
            if y + rr[3] - sc > view_h - pad {
                sc = y + rr[3] - view_h + pad;
            }
            if sc != self.grid_scroll {
                self.grid_scroll = sc;
                scene.n(self.grid_scroll_node).ty.set(-sc, now, d(motion::BASE));
            }
        }
        self.focus.note(id);
        self.cur = Some(id);
        self.page_focus[self.tab] = Some(id);
    }

    fn show_stage(&mut self, scene: &mut Scene, t: &mut TextSys, img: &mut ImageLoader, gi: usize, now: f64, animate: bool) {
        let r = self.rem;
        let d = |v: f32| if animate { v } else { 0.0 };
        let g = self.games[gi].clone();
        if self.back_game != Some(gi) {
            self.back_game = Some(gi);
            let next = 1 - self.back_cur;
            let list = if g.art.hero.is_empty() { &g.art.header } else { &g.art.hero };
            let layer = self.back_layers[next];
            scene.clear_children(layer);
            if let Some(src) = data::first(list) {
                let key = img.request(ImgReq { src, w: scene.w as u32, h: (40.0 * r) as u32, fit: Fit::Cover { fy: 0.25 } });
                scene.add(layer, s().fill().done(), Node::default().image(key));
            }
            scene.n(layer).z = 1;
            scene.n(self.back_layers[self.back_cur]).z = 0;
            scene.n(layer).opacity.restart(0.0, 1.0, now, d(motion::SLOW));
            if !animate {
                scene.n(layer).opacity.snap(1.0);
            }
            // The outgoing art stays under the incoming one; it is reused for the next change.
            self.back_cur = next;
        }
        if self.stage_game != Some(gi) {
            self.stage_game = Some(gi);
            let next = 1 - self.stage_cur;
            let slot = self.stage_slots[next];
            scene.clear_children(slot);
            let label = if g.last_played.unwrap_or(0) > 0 { "Continue playing" } else { "In your library" };
            scene.add_text(t, slot, s().done(), label, ts(r, size::SMALL, 1.25, 500), tok::FG.alpha(0.75));
            let logo_box = scene.add(slot, s().row().items_end().h(8.5 * r).max_w(34.0 * r).done(), Node::default());
            match data::first(&g.art.logo) {
                Some(src) => {
                    let key = img.request(ImgReq { src, w: (34.0 * r) as u32, h: (8.5 * r) as u32, fit: Fit::Contain });
                    scene.add(logo_box, s().wh(34.0 * r, 8.5 * r).done(), Node::default().image(key).contain([0.0, 1.0]));
                }
                None => {
                    scene.add_text(t, logo_box, s().done(), &g.title, ts(r, 3.0, 3.0, 700).tracking(-0.03), tok::FG);
                }
            }
            let meta: Vec<String> = [Some(data::source_label(&g.source).to_string()).filter(|s| !s.is_empty()), Some(data::fmt_ago(g.last_played)), data::fmt_playtime(g.playtime_minutes)].into_iter().flatten().collect();
            scene.add_text(t, slot, s().done(), &meta.join("  \u{00b7}  "), ts(r, size::BODY, 1.375, 400), tok::FG.alpha(0.75));
            scene.n(slot).z = 1;
            scene.n(self.stage_slots[self.stage_cur]).z = 0;
            scene.n(slot).opacity.restart(0.0, 1.0, now, d(motion::BASE));
            if !animate {
                scene.n(slot).opacity.snap(1.0);
            }
            // The outgoing lockup stays under the incoming one until its fade is done.
            let old = self.stage_slots[self.stage_cur];
            if animate {
                scene.n(old).opacity.set(0.0, now + motion::SLOW as f64, 0.001);
            } else {
                scene.n(old).opacity.snap(0.0);
            }
            self.stage_cur = next;
            let play = if g.source == "apps" { "Open" } else { "Play" };
            scene.set_text(t, self.play_label, play);
        }
    }

    pub fn set_live(&mut self, scene: &mut Scene, t: &mut TextSys, live: &Live) {
        if let Some(p) = live.sui_usd {
            scene.set_text(t, self.panel_price, &data::fmt_price(p));
            scene.set_text(t, self.bar_price, &data::fmt_price(p));
        }
        if let Some(c) = live.sui_change {
            let col = if c >= 0.0 { tok::POS } else { tok::NEG };
            for id in [self.panel_change, self.bar_change] {
                scene.set_text(t, id, &data::fmt_change(c));
                scene.n(id).text.as_mut().unwrap().color.snap(col);
            }
        }
        if let Some(v) = live.tps {
            scene.set_text(t, self.bar_tps, &v.to_string());
        }
        if let Some(v) = live.gas {
            scene.set_text(t, self.bar_gas, &format!("{v} MIST"));
        }
    }

    pub fn tick_clock(&mut self, scene: &mut Scene, t: &mut TextSys) {
        scene.set_text(t, self.clock, &rare_render::platform::clock_text());
        if let Some(p) = rare_render::platform::battery_percent() {
            scene.set_text(t, self.battery, &format!("{p}%"));
        }
    }

    /// After layout: put the underline under the current tab.
    pub fn after_layout(&mut self, scene: &mut Scene, now: f64, animate: bool) {
        if !self.underline_dirty {
            return;
        }
        self.underline_dirty = false;
        let tr = scene.rect(self.tabs[self.tab]);
        let top = scene.rect(self.underline);
        let mut st = scene.style(self.underline);
        let w = tr[2];
        if (top[2] - w).abs() > 0.5 {
            st.size.width = rare_render::taffy::prelude::length(w);
            scene.set_style(self.underline, st);
        }
        scene.n(self.underline).tx.set(tr[0], now, if animate { motion::BASE } else { 0.0 });
    }

    /// When a strip move has settled, the stage follows (a held direction does not thrash the art).
    pub fn next_deadline(&self) -> Option<f64> {
        self.pending_stage.map(|(_, at)| at)
    }

    pub fn tick(&mut self, scene: &mut Scene, t: &mut TextSys, img: &mut ImageLoader, now: f64) -> bool {
        if let Some((gi, at)) = self.pending_stage {
            if now >= at {
                self.pending_stage = None;
                self.show_stage(scene, t, img, gi, now, true);
                return true;
            }
        }
        false
    }

    pub fn on_pad(&mut self, scene: &mut Scene, t: &mut TextSys, img: &mut ImageLoader, pad: Pad, now: f64) -> Action {
        let _ = (t, img);
        match pad {
            Pad::Lb | Pad::Rb => {
                let next = if pad == Pad::Rb { (self.tab + 1) % 5 } else { (self.tab + 4) % 5 };
                self.switch_tab(scene, next, now);
                Action::Tab
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                let Some(cur) = self.cur else { return Action::None };
                let dir = match pad {
                    Pad::Up => Dir::Up,
                    Pad::Down => Dir::Down,
                    Pad::Left => Dir::Left,
                    _ => Dir::Right,
                };
                let Some(next) = self.focus.step(scene, cur, dir) else { return Action::None };
                self.set_focus(scene, next, now, true);
                if let Some(i) = self.is_tile(next) {
                    if let Some(g) = self.tiles[i].game {
                        self.pending_stage = Some((g, now + 0.14));
                    }
                }
                Action::Moved
            }
            Pad::A => match self.cur.and_then(|c| self.is_tile(c)).and_then(|i| self.tiles[i].game) {
                Some(g) => Action::Select(self.games[g].title.clone()),
                None => Action::None,
            },
            _ => Action::None,
        }
    }

    fn switch_tab(&mut self, scene: &mut Scene, next: usize, now: f64) {
        if next == self.tab {
            return;
        }
        let prev = self.tab;
        // The old page goes at once; the new one is drawn in the same frame and fades in (.fade).
        scene.n(self.pages[prev]).hidden = true;
        scene.n(self.pages[prev]).opacity.snap(0.0);
        scene.n(self.pages[next]).hidden = false;
        scene.n(self.pages[next]).opacity.restart(0.0, 1.0, now, motion::BASE);
        scene.n(self.backdrop).hidden = next != 0;
        scene.n(self.tabs[prev]).text.as_mut().unwrap().color.set(tok::MUTED, now, motion::FOCUS);
        scene.n(self.tabs[next]).text.as_mut().unwrap().color.set(tok::FG, now, motion::FOCUS);
        if let Some(c) = self.cur.take() {
            // Keep the page's look as it was (focus returns to it).
            self.cur = Some(c);
        }
        self.tab = next;
        self.underline_dirty = true;
        self.register_focus();
        let target = self.page_focus[next].or_else(|| self.focus.items.first().map(|i| i.id));
        let old_cur = self.cur;
        // The old page keeps its focused look underneath (hidden); clear the pointer only.
        self.cur = None;
        if let Some(id) = target {
            self.set_focus(scene, id, now, false);
        }
        let _ = old_cur;
    }
}

fn src_missing(g: &Game) -> bool {
    data::first(&g.art.cover).is_none() && (g.source != "apps" || data::first(&g.art.icon).is_none())
}

fn chip(scene: &mut Scene, t: &mut TextSys, parent: Id, r: f32, label: &str) -> Id {
    let c = scene.add(parent, s().row().items_center().justify_center().h(1.15 * r).px(0.4 * r).done(), Node::bg(tok::L4).radius(0.25 * r));
    scene.add_text(t, c, s().done(), label, ts(r, 0.62, 0.8, 600), tok::FG.alpha(0.85));
    c
}

fn divider(scene: &mut Scene, parent: Id, r: f32) {
    let _ = r;
    scene.add(parent, s().w(1.0).hp(1.0).done(), Node::bg(tok::HAIR));
}

#[allow(clippy::too_many_arguments)]
fn button(scene: &mut Scene, t: &mut TextSys, parent: Id, r: f32, label: &str, base: Color, icon: Option<u64>, large: bool, min_w: Option<f32>) -> Btn {
    let (h, px, size_rem, line, gap) = if large { (3.25, 1.75, size::ROW, 1.5, 0.6) } else { (2.85, 1.35, size::BODY, 1.375, 0.55) };
    let mut st = s().row().items_center().justify_center().h(h * r).px(px * r).gap(gap * r);
    if let Some(w) = min_w {
        st = st.min_w(w);
    }
    let weight = if base == tok::BRAND { 600 } else { 500 };
    let id = scene.add(parent, st.done(), Node::bg(base).radius(0.25 * r));
    let icon = icon.map(|k| scene.add(id, s().wh(1.1 * r, 1.1 * r).done(), Node::default().icon(k, tok::FG)));
    let text = scene.add_text(t, id, s().done(), label, ts(r, size_rem, line, weight), tok::FG);
    Btn { id, text, icon, base, fg: tok::FG }
}
