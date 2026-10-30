//! App tiles (launcher/src/apps/tiles.tsx): the console media row on Home and the Library > Apps
//! grid. Same shape and focus as the other square tiles; the icon on the layer colour, the name and
//! how it opens underneath like a game cover.

use super::catalog::{apps, icon_src, run_id, AppEntry, AppStatus};
use super::state::{InstallState, Shared};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::anim::{Ease, Keys, BREATHE};
use rare_render::input::Pad;
use rare_render::taffy::Style;
use rare_render::theme::{motion, tok};
use rare_render::{Fit, Id, ImgReq, Node};

/// The icon as the service ships it, masked to the rounded square it was drawn for
/// (`rounded-[22%] object-contain`). `rem` is the side in rem.
pub fn app_icon(cx: &mut Cx, parent: Id, app: &AppEntry, rem: f32) -> Id {
    let r = cx.r();
    let px = rem * r;
    let st = s().wh(px, px).no_shrink_().done();
    // The wrapper rounds and clips the image (rounded-[22%]).
    let wrap = cx.scene.add(parent, st, Node::default().radius(0.22 * px).clip());
    if let Some(src) = icon_src(&app.icon) {
        let k = cx.img.request(ImgReq { src, w: px.round() as u32, h: px.round() as u32, fit: Fit::Contain });
        cx.scene.add(wrap, s().fill().done(), Node::default().image(k).contain([0.5, 0.5]));
    }
    wrap
}

/// A thin determinate bar on the slider track of Quick settings (layer 04, white fill). `None` is
/// the indeterminate load bar; it only moves when `live` (goldens keep it still). `width` is the
/// bar's width in px when the caller knows it (the sliding piece is a fraction of it).
pub fn install_bar(cx: &mut Cx, parent: Id, style: Style, progress: Option<f32>, live: bool, width: f32, now: f64) -> Id {
    let r = cx.r();
    let h = 0.35 * r;
    let mut st = style;
    st.size.height = rare_render::taffy::prelude::length(h);
    let track = cx.scene.add(parent, st, Node::bg(tok::L4).radius(h / 2.0).clip());
    match progress {
        Some(p) => {
            let p = p.clamp(0.0, 1.0);
            // The percentage is rounded like the web's `Math.round(p * 100)`.
            let pct = (p * 100.0).round() / 100.0;
            cx.scene.add(track, s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(pct).done(), Node::bg(tok::FG).radius(h / 2.0));
        }
        None => {
            // .load-bar::after: a piece 100% wide that slides in scaled to 0.4..0.6, 1.1 s.
            let fill = cx.scene.add(track, s().fill().done(), Node::bg(tok::FG.alpha(0.55)).origin(0.0, 0.5));
            cx.scene.n(fill).scale_x.snap(0.4);
            cx.scene.n(fill).tx.snap(0.3 * width);
            if live {
                let tx: &'static Keys = Box::leak(Box::new(Keys {
                    frames: Box::leak(vec![(0.0, -width), (0.5, 0.3 * width), (1.0, 1.1 * width)].into_boxed_slice()),
                    ease: Ease::STANDARD,
                }));
                static SX: Keys = Keys { frames: &[(0.0, 0.4), (0.5, 0.6), (1.0, 0.4)], ease: Ease::STANDARD };
                cx.scene.n(fill).tx.start_loop(tx, 1.1, now);
                cx.scene.n(fill).scale_x.start_loop(&SX, 1.1, now);
            }
        }
    }
    track
}

/// ui Dot, live: the 0.45rem white marker that breathes (opacity only).
pub fn live_dot(cx: &mut Cx, parent: Id, style: Style, color: rare_render::Color, live_motion: bool, breathe: bool, now: f64) -> Id {
    let r = cx.r();
    let mut st = style;
    st.size.width = rare_render::taffy::prelude::length(0.45 * r);
    st.size.height = rare_render::taffy::prelude::length(0.45 * r);
    let id = cx.scene.add(parent, st, Node::bg(color).radius(0.225 * r));
    if breathe && live_motion {
        cx.scene.n(id).opacity.start_loop(&BREATHE, 2.4, now);
    }
    id
}

/// What a tile shows of one app.
#[derive(Clone, Debug)]
pub struct TileView {
    pub running: bool,
    pub status: Option<AppStatus>,
    pub installing: Option<InstallState>,
}

impl TileView {
    pub fn of(apps: &super::state::Apps, app: &AppEntry) -> TileView {
        TileView { running: apps.running.contains(&run_id(&app.id)), status: apps.status.get(&app.id).cloned(), installing: apps.installing.get(&app.id).cloned() }
    }

    pub fn ready(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.way.is_some())
    }

    /// Only a state worth reading: how an app opens (which browser) is detail for its page.
    pub fn sub(&self) -> &'static str {
        if self.installing.as_ref().is_some_and(|i| i.error.is_none()) {
            "Installing"
        } else if self.running {
            "Running"
        } else if !self.ready() && self.status.is_some() {
            "Not installed"
        } else {
            ""
        }
    }

    /// data-hint-a: what A does on this tile.
    pub fn hint_a(&self) -> &'static str {
        if self.running {
            "Resume"
        } else if self.ready() {
            "Open"
        } else {
            "Get"
        }
    }
}

pub struct TileParts {
    pub f: Focusable,
    pub label: Id,
}

/// One AppTile: `w-[9rem]`, the square button and the name and state under it. `details` adds
/// the X hint.
pub fn app_tile(cx: &mut Cx, parent: Id, app: &AppEntry, v: &TileView, live: bool, details: bool, now: f64) -> TileParts {
    let r = cx.r();
    let wrap = cx.scene.add(parent, s().col().w(9.0 * r).no_shrink_().done(), Node::default());
    let btn = cx.scene.add(wrap, s().wh(9.0 * r, 9.0 * r).items_center().justify_center().done(), Node::bg(tok::L2).radius(0.5 * r).clip());
    app_icon(cx, btn, app, 4.5);
    if v.running {
        live_dot(cx, btn, s().abs(Some(0.6 * r), Some(0.6 * r), None, None).done(), tok::FG, live, true, now);
    }
    if let Some(i) = v.installing.as_ref().filter(|i| i.error.is_none()) {
        install_bar(cx, btn, s().abs(None, Some(0.75 * r), Some(0.75 * r), Some(0.75 * r)).done(), i.progress, live, 7.5 * r, now);
    }
    let label = cx.scene.add_text(cx.t, wrap, s().mt(0.85 * r).px(0.1 * r).done(), &app.name, ts(r, ty::META.0, ty::META.1, 500).truncate(), tok::FG.alpha(0.85));
    let sub = v.sub();
    cx.scene.add_text(cx.t, wrap, s().px(0.1 * r).done(), if sub.is_empty() { "\u{a0}" } else { sub }, ts(r, ty::LABEL.0, ty::LABEL.1, 400).truncate(), tok::MUTED);
    let mut hint = v.hint_a().to_string();
    if details {
        hint.push_str("|X:Details");
    }
    let f = Focusable { id: btn, kind: Kind::Tile { scale: 1.06 }, group: 0, hint, tag: 0, sid: app.id.clone() };
    TileParts { f, label }
}

/// `.focus-text`: the name goes from fg/85 to full fg while its tile has focus.
pub fn sync_labels(cx: &mut Cx, nav: &Nav, labels: &[(Id, Id)], now: f64, animate: bool) {
    for (btn, label) in labels {
        let c = if nav.cur == Some(*btn) { tok::FG } else { tok::FG.alpha(0.85) };
        if let Some(t) = cx.scene.n(*label).text.as_mut() {
            t.color.set(c, now, if animate { motion::FOCUS } else { 0.0 });
        }
    }
}

// ---- The two screens the tiles live in ----

/// Home's Apps row (`AppsRail`) or Library > Apps (`AppsGrid`), as a screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Layout {
    Rail,
    Grid,
}

pub struct AppsTiles {
    apps: Shared,
    layout: Layout,
    root: Id,
    nav: Nav,
    labels: Vec<(Id, Id)>,
    built: u64,
    /// Which tile had focus before a rebuild.
    focus_sid: Option<String>,
}

impl AppsTiles {
    pub fn build(cx: &mut Cx, apps: Shared, layout: Layout, focus_sid: Option<&str>, now: f64) -> AppsTiles {
        let mut me = AppsTiles { apps, layout, root: cx.scene.root, nav: Nav::default(), labels: Vec::new(), built: 0, focus_sid: focus_sid.map(String::from) };
        me.populate(cx, now);
        me
    }

    fn populate(&mut self, cx: &mut Cx, now: f64) {
        let r = cx.r();
        let root = cx.scene.root;
        let apps = self.apps.borrow();
        self.built = apps.version;
        let live = apps.live;
        self.nav = Nav::default();
        self.labels.clear();
        let page = cx.scene.add(root, s().fill().col().pt(3.0 * r).done(), Node::default());
        self.root = page;
        let catalog = apps_list();
        match self.layout {
            Layout::Rail => {
                // Rail > SectionHead: mb 0.9rem px 2.5rem; the note is "n running".
                let n = apps.running_apps();
                let head = cx.scene.add(page, s().row().items_center().gap(0.75 * r).px(2.5 * r).mb(0.9 * r).done(), Node::default());
                cx.scene.add_text(cx.t, head, s().done(), "Apps", ts(r, ty::ROW.0, ty::ROW.1, 600).tracking(-0.01), tok::FG);
                if n > 0 {
                    text(cx, head, ty::SMALL, 400, tok::MUTED, &format!("{n} running"));
                }
                let rail = cx.scene.add(page, s().row().items_start().gap(1.0 * r).px(2.5 * r).done(), Node::default());
                for a in catalog {
                    let v = TileView::of(&apps, a);
                    let t = app_tile(cx, rail, a, &v, live, true, now);
                    self.labels.push((t.f.id, t.label));
                    self.nav.add(t.f.group(1));
                }
            }
            Layout::Grid => {
                // grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-x-[1.1rem] gap-y-[1.4rem]
                let avail = cx.scene.w - 5.0 * r;
                let (tile, gap) = (9.0 * r, 1.1 * r);
                let n = (((avail + gap) / (tile + gap)).floor() as usize).max(1);
                let cell = (avail - gap * (n as f32 - 1.0)) / n as f32;
                let grid = cx.scene.add(page, s().col().gap(1.4 * r).px(2.5 * r).done(), Node::default());
                for chunk in catalog.chunks(n) {
                    let row_ = cx.scene.add(grid, s().row().gap(gap).done(), Node::default());
                    for a in chunk.iter() {
                        let slot = cx.scene.add(row_, s().w(cell).no_shrink_().done(), Node::default());
                        let v = TileView::of(&apps, a);
                        let t = app_tile(cx, slot, a, &v, live, false, now);
                        self.labels.push((t.f.id, t.label));
                        self.nav.add(t.f.group(1));
                    }
                }
            }
        }
        drop(apps);
        let want = self.focus_sid.clone();
        let start = self.nav.items.iter().find(|f| Some(&f.sid) == want.as_ref()).or(self.nav.items.first()).map(|f| f.id);
        if let Some(id) = start {
            self.nav.focus(cx.scene, id, now, false);
            sync_labels(cx, &self.nav, &self.labels, now, false);
        }
    }
}

fn apps_list() -> &'static [AppEntry] {
    apps()
}

impl Screen for AppsTiles {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        let hit = self.nav.focus_at(cx.scene, x, y, now);
        if hit {
            sync_labels(cx, &self.nav, &self.labels, now, true);
        }
        hit
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    sync_labels(cx, &self.nav, &self.labels, now, true);
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => {
                let Some(sid) = self.nav.current().map(|f| f.sid.clone()) else { return Out::None };
                let Some(app) = super::catalog::app_by_id(&sid) else { return Out::None };
                use super::state::Activation::*;
                let act = self.apps.borrow_mut().activate(app);
                Out::Act(match act {
                    Resume => format!("app-resume:{sid}"),
                    Open => format!("app-open:{sid}"),
                    Page => format!("app-sheet:{sid}"),
                })
            }
            // X on a focused app: its page (Resume and Close, other ways to open it, the controls).
            Pad::X if self.layout == Layout::Rail => match self.nav.current() {
                Some(f) => Out::Act(format!("app-sheet:{}", f.sid)),
                None => Out::None,
            },
            _ => Out::None,
        }
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.apps.borrow().version != self.built {
            self.focus_sid = self.nav.current().map(|f| f.sid.clone());
            cx.scene.remove(self.root);
            self.populate(cx, now);
        }
    }
}
