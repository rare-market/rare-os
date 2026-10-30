//! An app's page (Open or Resume and Close, other ways to open it, Get with progress, the
//! controller layout) and the running list View + Menu (or Guide) held brings up
//! (launcher/src/apps/sheets.tsx). Both are the launcher's standard sheet (trade/parts Sheet).

use super::catalog::{app_by_id, controller_of, controls_for, AppEntry, ControlRow};
use super::state::{quit_item, rare_way, resume_item, running_items, Apps, RunningItem, Shared};
use super::tiles::{app_icon, install_bar, live_dot};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::input::Pad;
use rare_render::scene::Shadow;
use rare_render::theme::tok;
use rare_render::{Fit, Id, ImgReq, Node};

/// Quit asks once more (A again within a few seconds), since unsaved progress goes with it.
pub const ARM_SECS: f64 = 4.0;

/// A disabled button is not focusable (a DOM `disabled`) and drawn at 40 %.
fn dim(cx: &mut Cx, f: &Focusable) {
    cx.scene.n(f.id).opacity.snap(0.4);
}

/// Puts the first / the tagged focusable in focus without animation.
fn focus_tag(cx: &mut Cx, nav: &mut Nav, tag: Option<u32>, def: Option<u32>, now: f64) {
    let pick = tag.and_then(|t| nav.items.iter().find(|f| f.tag == t)).or_else(|| def.and_then(|t| nav.items.iter().find(|f| f.tag == t))).or(nav.items.first()).map(|f| f.id);
    if let Some(id) = pick {
        nav.focus(cx.scene, id, now, false);
    }
}

// ---- Controls ----

fn controls(cx: &mut Cx, parent: Id, rows: &[ControlRow]) {
    let r = cx.r();
    let grid = cx.scene.add(parent, s().col().done(), Node::default());
    for pair in rows.chunks(2) {
        let line = cx.scene.add(grid, s().row().gap(2.0 * r).done(), Node::default());
        for i in 0..2 {
            let cell = cx.scene.add(line, s().row().items_center().gap(0.75 * r).h(2.5 * r).w(1.0).grow().done(), Node::default());
            if let Some(row_) = pair.get(i) {
                let key = cx.scene.add(cell, s().row().items_center().w(6.25 * r).no_shrink_().done(), Node::default());
                if row_.b.contains(' ') || row_.b.contains('-') {
                    text(cx, key, ty::SMALL, 400, tok::MUTED, row_.b);
                } else if let Some(g) = glyph_of(row_.b) {
                    glyph(cx, key, g, true);
                }
                text_tr(cx, cell, ty::SMALL, 400, tok::FG, row_.does);
                edge(cx, cell, 2, tok::HAIR);
            }
        }
    }
}

/// A pad button's name as the catalog writes it.
pub fn glyph_of(b: &str) -> Option<G> {
    Some(match b {
        "a" => G::A,
        "b" => G::B,
        "x" => G::X,
        "y" => G::Y,
        "lb" => G::Lb,
        "rb" => G::Rb,
        "lt" => G::Lt,
        "rt" => G::Rt,
        "start" => G::Start,
        "select" => G::Select,
        "rare" => G::Rare,
        "quick" => G::Quick,
        _ => return None,
    })
}

// ---- The sheet frame with an icon in its title ----

struct Frame {
    card: Id,
    body: Id,
    footer: Option<Id>,
}

/// trade/parts Sheet with `title` as icon + name (the app's page).
fn app_frame(cx: &mut Cx, parent: Id, app: &AppEntry, with_footer: bool, animate: bool, now: f64) -> Frame {
    let r = cx.r();
    let page = cx.scene.add(parent, s().abs(Some(0.0), Some(0.0), Some(3.0 * r), Some(0.0)).items_center().justify_center().p(1.5 * r).done(), Node::default());
    cx.scene.add(page, s().fill().done(), Node::bg(tok::BG.alpha(0.8)));
    let card = cx.scene.add(page, s().col().wp(1.0).max_w(44.0 * r).done(), Node::bg(tok::BG).radius(1.25 * r).border(1.0, tk::BORDER_LIGHT).clip().shadow(&Shadow::xl2(0.4)));
    if animate {
        fade_in(cx, card, now);
    }
    let head = cx.scene.add(card, s().col().pad(2.0 * r, 1.5 * r, 2.0 * r, 1.0 * r).done(), Node::default());
    let title = cx.scene.add(head, s().row().items_center().gap(0.9 * r).done(), Node::default());
    app_icon(cx, title, app, 2.75);
    text(cx, title, ty::ROW, 600, tok::FG, &app.name);
    cx.scene.add_text(cx.t, head, s().done(), &app.about, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
    if let Some(plan) = &app.plan {
        cx.scene.add_text(cx.t, head, s().mt(0.5 * r).done(), plan, ts(r, ty::SMALL.0, ty::SMALL.1, 500).wrap(), tok::FG);
    }
    if !app.steps.is_empty() {
        let list = cx.scene.add(head, s().col().gap(0.35 * r).mt(0.75 * r).done(), Node::default());
        for (i, step) in app.steps.iter().enumerate() {
            let line = cx.scene.add(list, s().row().gap(0.5 * r).done(), Node::default());
            let n = cx.scene.add(line, s().w(1.1 * r).no_shrink_().done(), Node::default());
            text(cx, n, ty::SMALL, 600, tok::FG, &format!("{}.", i + 1));
            cx.scene.add_text(cx.t, line, s().grow().shrink().done(), step, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
        }
    }
    edge(cx, head, 2, tok::HAIR);
    let body = cx.scene.add(card, s().col().px(2.0 * r).py(1.25 * r).done(), Node::default());
    let footer = with_footer.then(|| {
        let f = cx.scene.add(card, s().row().gap(0.5 * r).px(2.0 * r).py(1.0 * r).done(), Node::default());
        edge(cx, f, 0, tok::HAIR);
        f
    });
    Frame { card, body, footer }
}

// ---- AppSheet ----

#[derive(Clone, Debug, PartialEq)]
enum Act {
    Resume,
    Close,
    Open(Option<usize>),
    Install,
}

pub struct AppSheet {
    apps: Shared,
    app: &'static AppEntry,
    page: Id,
    nav: Nav,
    acts: Vec<Act>,
    built: u64,
}

impl AppSheet {
    pub fn build(cx: &mut Cx, apps: Shared, id: &str, now: f64) -> Option<AppSheet> {
        let app = app_by_id(id)?;
        let mut me = AppSheet { apps, app, page: cx.scene.root, nav: Nav::default(), acts: Vec::new(), built: 0 };
        me.populate(cx, None, true, now);
        Some(me)
    }

    fn populate(&mut self, cx: &mut Cx, keep: Option<u32>, animate: bool, now: f64) {
        let r = cx.r();
        let app = self.app;
        let apps = self.apps.borrow();
        self.built = apps.version;
        self.nav = Nav::default();
        self.acts.clear();
        let st = apps.status.get(&app.id).cloned();
        let installing = apps.installing.get(&app.id).cloned();
        let running = apps.is_running(&app.id);
        let (live, guest) = (apps.live, apps.guest);
        drop(apps);
        let ready = st.as_ref().is_some_and(|s| s.way.is_some());
        let offer = st.as_ref().and_then(|s| s.offer.clone());
        let busy = installing.as_ref().is_some_and(|i| i.error.is_none());
        let way = match (&st, ready) {
            (Some(s), true) => &app.ways[s.way.unwrap()],
            _ => &app.ways[0],
        };
        let controller = st.as_ref().and_then(|s| s.controller).or(way.controller).unwrap_or(super::catalog::Controller::Pointer);
        let _ = controller_of;
        let line = if running {
            "Running".to_string()
        } else if ready {
            format!("Opens in {}", st.as_ref().and_then(|s| s.opens_in.clone()).unwrap_or_default())
        } else if let Some(o) = &offer {
            format!("Needs {}", o.name)
        } else {
            "Can't open on this device".to_string()
        };
        let root = cx.scene.root;
        let frame = app_frame(cx, root, app, true, animate, now);
        self.page = cx.scene.parent(frame.card).unwrap_or(root);
        let body = frame.body;

        // Footer buttons.
        let mut default_tag = None;
        if let Some(foot) = frame.footer {
            let mut add = |me: &mut AppSheet, cx: &mut Cx, label: &str, v: Variant, act: Act, hint: &str, def: bool, disabled: bool| {
                let tag = me.acts.len() as u32;
                let b = button(cx, foot, label, v, Sz::Md, false).tag(tag).hint(hint);
                me.acts.push(act);
                if disabled {
                    dim(cx, &b);
                } else {
                    me.nav.add(b);
                }
                if def && default_tag.is_none() && !disabled {
                    default_tag = Some(tag);
                }
            };
            if running {
                add(self, cx, "Resume", Variant::Primary, Act::Resume, "Resume", true, false);
                add(self, cx, "Close", Variant::Secondary, Act::Close, "Close app", false, false);
            } else if ready {
                add(self, cx, "Open", Variant::Primary, Act::Open(None), "Open", true, false);
                for &i in &st.as_ref().unwrap().other_ways {
                    add(self, cx, &format!("Open in {}", app.ways[i].label), Variant::Secondary, Act::Open(Some(i)), "", false, false);
                }
            }
            if let Some(o) = &offer {
                let v = if ready || running { Variant::Secondary } else { Variant::Primary };
                let label = if busy { "Installing".to_string() } else { format!("Get {}", o.name) };
                add(self, cx, &label, v, Act::Install, &format!("Get {}", o.name), !ready && !running, busy);
            }
        }

        // Body.
        let status = cx.scene.add(body, s().row().items_center().gap(0.5 * r).done(), Node::default());
        live_dot(cx, status, s().done(), if running { tok::FG } else { tok::MUTED }, live, running, now);
        text(cx, status, ty::SMALL, 400, tok::FG.alpha(0.8), &line);

        if let Some(o) = &offer {
            let blk = cx.scene.add(body, s().col().gap(0.6 * r).mt(1.1 * r).done(), Node::default());
            let msg = if ready {
                format!("{} is the full desktop app. It installs from Flathub for you only, no password needed.", o.name)
            } else {
                format!("{} opens in {}. It installs from Flathub for you only, no password needed.", app.name, o.name)
            };
            cx.scene.add_text(cx.t, blk, s().done(), &msg, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
            if busy {
                let i = installing.as_ref().unwrap();
                let rowb = cx.scene.add(blk, s().row().items_center().gap(0.9 * r).done(), Node::default());
                // 44rem card - 4rem body padding - 0.9rem gap - 3rem percentage.
                install_bar(cx, rowb, s().grow().done(), i.progress, live, 36.1 * r, now);
                let pct = i.progress.map(|p| format!("{}%", (p * 100.0).round() as i32)).unwrap_or_default();
                let w = cx.scene.add(rowb, s().row().justify_end().w(3.0 * r).no_shrink_().done(), Node::default());
                cx.scene.add_text(cx.t, w, s().done(), &pct, ts(r, ty::SMALL.0, ty::SMALL.1, 400).tabular(), tok::FG);
            }
            if let Some(e) = installing.as_ref().and_then(|i| i.error.as_ref()) {
                cx.scene.add_text(cx.t, blk, s().done(), e, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::NEG);
            }
            text(cx, blk, ty::LABEL, 400, tok::MUTED, "Or in Desktop mode, in a terminal:");
            let code = cx.scene.add(blk, s().col().px(0.9 * r).py(0.6 * r).done(), Node::bg(tok::L2).radius(0.5 * r));
            cx.scene.add_text(cx.t, code, s().done(), &o.command, ts(r, ty::META.0, ty::META.1, 400).mono().wrap(), tok::FG);
        }

        let ctl = cx.scene.add(body, s().col().mt(1.4 * r).done(), Node::default());
        cx.scene.add_text(cx.t, ctl, s().mb(0.35 * r).done(), "Controls", ts(r, ty::BODY.0, ty::BODY.1, 600), tok::FG);
        controls(cx, ctl, &controls_for(controller, Some(way)));
        cx.scene.add_text(
            cx.t,
            ctl,
            s().mt(0.8 * r).done(),
            &format!("{} to come back to Rare, then Resume or Close it.", rare_way(guest)),
            ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(),
            tok::MUTED,
        );
        focus_tag(cx, &mut self.nav, keep, default_tag, now);
    }
}

impl Screen for AppSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => {
                let Some(tag) = self.nav.current().map(|f| f.tag) else { return Out::None };
                let app = self.app;
                let id = app.id.clone();
                let mut a = self.apps.borrow_mut();
                match self.acts[tag as usize].clone() {
                    Act::Resume => {
                        a.resume(app);
                        Out::Act(format!("app-resume:{id}"))
                    }
                    Act::Close => {
                        a.close(app);
                        Out::Act(format!("app-close:{id}"))
                    }
                    Act::Open(w) => {
                        a.open(app, w);
                        Out::Act(format!("app-open:{id}"))
                    }
                    Act::Install => {
                        a.install(app);
                        Out::None
                    }
                }
            }
            _ => Out::None,
        }
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.apps.borrow().version != self.built {
            let keep = self.nav.current().map(|f| f.tag);
            cx.scene.remove(self.page);
            self.populate(cx, keep, false, now);
        }
    }
}

// ---- The running list ----

/// The buttons of one row, for a screen that wants to drive them.
pub struct ListRow {
    pub rid: String,
    pub resume: Option<Focusable>,
    pub quit: Option<Focusable>,
}

/// Resume and Quit for each running game or app. `armed` is the row waiting for a second A,
/// `quitting` the row being closed. Tags: row * 2 (Resume) and row * 2 + 1 (Quit).
pub fn running_list(cx: &mut Cx, parent: Id, items: &[RunningItem], armed: Option<&str>, quitting: Option<&str>) -> Vec<ListRow> {
    let r = cx.r();
    let list = cx.scene.add(parent, s().col().done(), Node::default());
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let row_ = cx.scene.add(list, s().row().items_center().gap(0.9 * r).py(0.7 * r).done(), Node::default());
        if i + 1 < items.len() {
            edge(cx, row_, 2, tok::HAIR);
        }
        if let Some(app) = it.app {
            app_icon(cx, row_, app, 2.5);
        } else {
            let src = it.stream.as_ref().and_then(|s| s.art.clone()).or_else(|| it.game.as_ref().and_then(|g| g.art.clone()));
            let thumb = cx.scene.add(row_, s().wh(2.5 * r, 2.5 * r).no_shrink_().done(), Node::bg(tok::L3).radius(0.25 * r).clip());
            if let Some(src) = src {
                let px = (2.5 * r).round() as u32;
                let k = cx.img.request(ImgReq { src, w: px, h: px, fit: Fit::Cover { fy: 0.5 } });
                cx.scene.add(thumb, s().fill().done(), Node::default().image(k));
            }
        }
        let info = cx.scene.add(row_, s().col().grow().shrink().done(), Node::default());
        text_tr(cx, info, ty::BODY, 500, tok::FG, &it.name);
        if quitting == Some(it.rid.as_str()) {
            text(cx, info, ty::SMALL, 400, tok::MUTED, "Quitting...");
        } else if armed == Some(it.rid.as_str()) {
            text(cx, info, ty::SMALL, 400, tok::MUTED, "Unsaved progress is lost. Press A again to quit.");
        }
        let disabled = quitting == Some(it.rid.as_str());
        let is_armed = armed == Some(it.rid.as_str());
        let resume = button(cx, row_, "Resume", Variant::Primary, Sz::Sm, false).tag(i as u32 * 2).hint("Resume");
        let label = if is_armed {
            "Quit now"
        } else if it.app.is_some() {
            "Close"
        } else {
            "Quit game"
        };
        let hint = if is_armed {
            "Quit now"
        } else if it.app.is_some() {
            "Close app"
        } else {
            "Quit game"
        };
        let quit = button(cx, row_, label, if is_armed { Variant::Primary } else { Variant::Secondary }, Sz::Sm, false).tag(i as u32 * 2 + 1).hint(hint);
        if disabled {
            dim(cx, &resume);
            dim(cx, &quit);
            out.push(ListRow { rid: it.rid.clone(), resume: None, quit: None });
        } else {
            out.push(ListRow { rid: it.rid.clone(), resume: Some(resume), quit: Some(quit) });
        }
    }
    out
}

/// Holding Guide, or View + Menu: everything running, each with Resume and Quit.
pub struct RunningSheet {
    apps: Shared,
    page: Id,
    nav: Nav,
    items: Vec<RunningItem>,
    armed: Option<(String, f64)>,
    quitting: Option<String>,
    built: u64,
    dirty: bool,
}

impl RunningSheet {
    pub fn build(cx: &mut Cx, apps: Shared, armed: Option<&str>, quitting: Option<&str>, now: f64) -> RunningSheet {
        let mut me = RunningSheet {
            apps,
            page: cx.scene.root,
            nav: Nav::default(),
            items: Vec::new(),
            armed: armed.map(|a| (a.to_string(), now + ARM_SECS)),
            quitting: quitting.map(String::from),
            built: 0,
            dirty: false,
        };
        me.populate(cx, None, true, now);
        me
    }

    fn populate(&mut self, cx: &mut Cx, keep: Option<u32>, animate: bool, now: f64) {
        let r = cx.r();
        let apps = self.apps.borrow();
        self.built = apps.version;
        self.items = running_items(&apps, None);
        self.nav = Nav::default();
        let root = cx.scene.root;
        let sub = if self.items.is_empty() { None } else { Some("Resume one, or quit it.") };
        let parts = sheet(cx, root, "Running", sub, false, false, now);
        if !animate {
            cx.scene.n(parts.card).opacity.snap(1.0);
            cx.scene.n(parts.card).ty.snap(0.0);
        }
        self.page = cx.scene.parent(parts.card).unwrap_or(root);
        if self.items.is_empty() {
            text(cx, parts.body, ty::SMALL, 400, tok::MUTED, "Nothing is running.");
        }
        let rows = running_list(cx, parts.body, &self.items, self.armed.as_ref().map(|a| a.0.as_str()), self.quitting.as_deref());
        for row_ in rows {
            for f in [row_.resume, row_.quit].into_iter().flatten() {
                self.nav.add(f);
            }
        }
        let line = cx.scene.add(parts.body, s().row().items_center().gap(0.45 * r).mt(0.9 * r).done(), Node::default());
        if apps.guest {
            text(cx, line, ty::SMALL, 400, tok::MUTED, "Hold");
            glyph(cx, line, G::Select, true);
        } else {
            text(cx, line, ty::SMALL, 400, tok::MUTED, "Press");
            glyph(cx, line, G::Rare, true);
        }
        text(cx, line, ty::SMALL, 400, tok::MUTED, "in any game to come back here.");
        drop(apps);
        focus_tag(cx, &mut self.nav, keep, Some(0), now);
    }

    fn rebuild(&mut self, cx: &mut Cx, now: f64) {
        let keep = self.nav.current().map(|f| f.tag);
        cx.scene.remove(self.page);
        self.populate(cx, keep, false, now);
        self.dirty = false;
    }
}

impl Screen for RunningSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => {
                let Some(tag) = self.nav.current().map(|f| f.tag) else { return Out::None };
                let Some(it) = self.items.get(tag as usize / 2).cloned() else { return Out::None };
                if tag % 2 == 0 {
                    // The overlay closes first, then the item comes back on screen.
                    resume_item(&mut self.apps.borrow_mut(), &it);
                    return Out::Act(format!("resume:{}", it.rid));
                }
                if self.armed.as_ref().map(|a| a.0.as_str()) != Some(it.rid.as_str()) {
                    self.armed = Some((it.rid.clone(), now + ARM_SECS));
                } else {
                    self.armed = None;
                    quit_item(&mut self.apps.borrow_mut(), &it);
                }
                self.rebuild(cx, now);
                Out::Moved
            }
            _ => Out::None,
        }
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.armed.as_ref().is_some_and(|a| now >= a.1) {
            self.armed = None;
            self.dirty = true;
        }
        if self.dirty || self.apps.borrow().version != self.built {
            self.rebuild(cx, now);
        }
    }

    fn next_deadline(&self) -> Option<f64> {
        self.armed.as_ref().map(|a| a.1)
    }
}

/// For callers that hold an `Apps` and want the running list's items.
pub fn items_of(apps: &Apps) -> Vec<RunningItem> {
    running_items(apps, None)
}
