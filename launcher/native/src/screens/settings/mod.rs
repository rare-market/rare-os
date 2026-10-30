//! Settings (screens/Settings.tsx and screens/settings/*): a section list on the left (with a
//! search field on top), rows on the right. LT and RT move between sections, Y asks for a search,
//! focusing a section shows it. Each section is a `SectionView` built into the right pane.

pub mod about;
pub mod accounts;
pub mod capture;
pub mod ask;
pub mod config;
pub mod controls;
pub mod developer;
pub mod devices;
pub mod display;
pub mod index;
pub mod live;
pub mod network;
pub mod ossheets;
pub mod parental;
pub mod power;
pub mod remap;
pub mod remote;
pub mod rows;
pub mod search;
pub mod sections;
pub mod sound;
pub mod storage;
pub mod support;
pub mod supportsheets;
pub mod system;
pub mod update;
pub mod upscale;

use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use config::Config;
use rare_render::input::Pad;
use rare_render::theme::tok;
use rare_render::{Id, Node};
use search::{search_settings, SectionHits, SettingEntry};
use sections::{SectionId, SECTIONS};

/// What a section reports after A or a nudge.
#[derive(Debug, PartialEq, Clone)]
pub enum SectionOut {
    None,
    /// The config changed; the shell saves it.
    Changed,
    /// Not handled here (a nudge on a row that is not a slider): focus moves instead.
    Pass,
    /// Something only the host can do (switch to Desktop mode, check for updates...).
    Act(String),
}

/// A section's behaviour once built. `press` is A on the focused row, `nudge` is left or right on a
/// focused slider. Both may change `cfg`.
pub trait SectionView {
    fn press(&mut self, _cx: &mut Cx, _nav: &mut Nav, _cfg: &mut Config, _now: f64) -> SectionOut {
        SectionOut::None
    }
    fn nudge(&mut self, _cx: &mut Cx, _nav: &mut Nav, _cfg: &mut Config, _dir: i32, _now: f64) -> SectionOut {
        SectionOut::Pass
    }
}

/// The row a locked section shows (A asks for the parent's PIN).
const LOCKED_ROW: u32 = 9100;

struct NoView;
impl SectionView for NoView {}

thread_local! {
    static OPEN_SECTION: std::cell::Cell<Option<SectionId>> = const { std::cell::Cell::new(None) };
}

/// The section the next live Settings opens on (a notice that leads to one).
pub fn open_section_next(id: SectionId) {
    OPEN_SECTION.with(|o| o.set(Some(id)));
}

pub struct Settings {
    nav: Nav,
    cfg: Config,
    pub section: SectionId,
    nav_ids: Vec<(SectionId, Id)>,
    nav_count: usize,
    pane: Id,
    view: Box<dyn SectionView>,
    query: String,
    hits: Vec<SectionHits>,
    index: Vec<SettingEntry>,
    pub dirty: bool,
    /// The config as last read from or written to the file (only the difference is written).
    saved: Config,
    /// Write changes to the launcher config file (the device, not a fixture).
    persist: bool,
    /// An error for the app to toast (bad input, a save that failed).
    pending_toast: Option<String>,
}

const SEARCH_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>"#;

impl Settings {
    /// `area` is the page area under the top bar and above the hint bar.
    pub fn build(cx: &mut Cx, area: Id, cfg: Config, section: SectionId, query: &str, now: f64) -> Settings {
        let r = cx.r();
        let root = cx.scene.add(area, s().fill().row().gap(2.0 * r).pad(2.5 * r, 0.5 * r, 2.5 * r, 1.5 * r).done(), Node::default());
        // The section list.
        let nav_col = cx.scene.add(root, s().col().w(17.5 * r).no_shrink_().gap(0.1 * r).pad(0.5 * r, 1.0 * r, 0.5 * r, 0.5 * r).ml(-1.5 * r).done(), Node::default().scrolls());
        let h1 = cx.scene.add(nav_col, s().px(1.0 * r).mb(0.5 * r).done(), Node::default());
        text(cx, h1, ty::TITLE, 600, tok::FG, "Settings");
        let mut label_style = s().row().items_center().gap(0.6 * r).h(2.75 * r).px(0.85 * r).mb(0.5 * r).done();
        // The web's label shrinks to its content when the section list overflows (flex-shrink 1).
        label_style.flex_shrink = 1.0;
        label_style.border = rare_render::taffy::Rect { left: rare_render::taffy::style::LengthPercentage::length(1.0), right: rare_render::taffy::style::LengthPercentage::length(1.0), top: rare_render::taffy::style::LengthPercentage::length(1.0), bottom: rare_render::taffy::style::LengthPercentage::length(1.0) };
        let label = cx.scene.add(nav_col, label_style, Node::bg(tok::L1).radius(0.5 * r).border(1.0, tk::BORDER_LIGHT));
        icon(cx, label, "search", SEARCH_ICON, 1.1, tok::MUTED);
        let field = cx.scene.add(label, s().row().items_center().grow().done(), Node::default().radius(0.25 * r));
        let shown = if query.is_empty() { "Search settings" } else { query };
        let field_text = cx.scene.add_text(cx.t, field, s().done(), shown, ts(r, ty::BODY.0, ty::BODY.1, 400).truncate(), if query.is_empty() { tok::MUTED } else { tok::FG });
        let _ = field_text;
        let mut nav = Nav::default();
        nav.add(Focusable { id: field, kind: Kind::Row, group: 0, hint: "Search".into(), tag: 9000, sid: "search".into() });
        let mut nav_ids = Vec::new();
        for (i, (id, name)) in SECTIONS.iter().enumerate() {
            let b = cx.scene.add(nav_col, s().row().items_center().px(1.0 * r).py(0.6 * r).done(), Node::default().radius(0.5 * r));
            let t = cx.scene.add_text(cx.t, b, s().done(), name, ts(r, ty::BODY.0, ty::BODY.1, 500), tok::FG.alpha(0.7));
            let _ = t;
            let bar = cx.scene.add(b, s().abs(Some(0.6 * r), None, Some(0.6 * r), Some(0.0)).w(3.0).done(), Node::bg(tok::FG).radius(1.5));
            cx.scene.n(bar).hidden = true;
            nav.add(Focusable { id: b, kind: Kind::Row, group: 1, hint: String::new(), tag: i as u32, sid: id.key().into() });
            nav_ids.push((*id, b));
        }
        // The pane.
        let pane = cx.scene.add(root, s().col().grow().pad(0.0, 1.0 * r, 0.0, 1.0 * r).done(), Node::default().scrolls());
        nav.scrollers.push(pane);
        let nav_count = nav.len();
        let mut me = Settings { nav, cfg, section, nav_ids, nav_count, pane, view: Box::new(NoView), query: query.to_string(), hits: vec![], index: index::settings_index(), dirty: false, saved: Config::default(), persist: false, pending_toast: None };
        me.saved = me.cfg.clone();
        me.refresh_nav_look(cx, now, false);
        me.show(cx, now);
        let start = me.nav_ids.iter().find(|(id, _)| *id == section).map(|(_, b)| *b).unwrap();
        me.nav.focus(cx.scene, start, now, false);
        me
    }

    /// Settings on fixtures (shots, UX journeys), opened on the section asked for last
    /// (`open_section_next`), else System.
    pub fn fixture_open(cx: &mut Cx, now: f64) -> Settings {
        let section = OPEN_SECTION.with(|o| o.take()).unwrap_or(SectionId::System);
        let area = fixture_area(cx);
        Settings::build(cx, area, config::fixture(), section, "", now)
    }

    /// Settings on the device: the real config file, saved as it changes.
    pub fn live(cx: &mut Cx, area: Id, section: SectionId, now: f64) -> Settings {
        let section = OPEN_SECTION.with(|o| o.take()).unwrap_or(section);
        let mut me = Settings::build(cx, area, config::load(), section, "", now);
        me.persist = true;
        me
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// The current section's row: fill l2, text full, and the 3 px bar (only when not searching).
    fn refresh_nav_look(&mut self, cx: &mut Cx, now: f64, animate: bool) {
        let searching = self.searching();
        for (id, b) in self.nav_ids.clone() {
            let on = !searching && id == self.section;
            self.nav.row_base.insert(b, if on { tok::L2 } else { rare_render::Color::TRANSPARENT });
            if self.nav.cur != Some(b) {
                cx.scene.n(b).bg.set(if on { tok::L2 } else { rare_render::Color::TRANSPARENT }, now, if animate { 0.11 } else { 0.0 });
            }
            let kids = cx.scene.children(b);
            if let Some(t) = kids.first() {
                cx.scene.n(*t).text.as_mut().unwrap().color.snap(if on { tok::FG } else { tok::FG.alpha(0.7) });
            }
            if let Some(bar) = kids.get(1) {
                cx.scene.n(*bar).hidden = !on;
            }
        }
    }

    fn show(&mut self, cx: &mut Cx, now: f64) {
        self.show_with(cx, now, true);
    }

    fn show_with(&mut self, cx: &mut Cx, now: f64, fade: bool) {
        let r = cx.r();
        self.nav.truncate(self.nav_count);
        cx.scene.clear_children(self.pane);
        if fade {
            cx.scene.n(self.pane).scroll_y.snap(0.0);
        }
        let col = cx.scene.add(self.pane, s().col().wp(1.0).gap(1.25 * r).done(), Node::default());
        if fade {
            fade_in_quick(cx, col, now);
        }
        if self.searching() {
            self.hits = search_settings(&self.query, &self.index);
            self.view = Box::new(NoView);
            if self.hits.is_empty() {
                let p = cx.scene.add(col, s().px(0.5 * r).done(), Node::default());
                let msg = format!("No settings match \"{}\".", self.query.trim());
                cx.scene.add_text(cx.t, p, s().done(), &msg, ts(r, ty::BODY.0, ty::BODY.1, 400).wrap(), tok::MUTED);
            } else {
                for (gi, g) in self.hits.clone().iter().enumerate() {
                    let sec = cx.scene.add(col, s().col().done(), Node::default());
                    let lab = cx.scene.add(sec, s().pad(0.25 * r, 0.0, 0.0, 0.5 * r).done(), Node::default());
                    text(cx, lab, ty::LABEL, 500, tok::MUTED, g.label);
                    let panel = rows::panel(cx, sec);
                    for (i, e) in g.entries.iter().enumerate() {
                        let row_ = cx.scene.add(panel, s().row().items_center().gap(1.0 * r).min_h(3.5 * r).wp(1.0).px(1.4 * r).py(0.7 * r).done(), Node::default());
                        if i > 0 {
                            edge(cx, row_, 0, tok::HAIR);
                        }
                        cx.scene.add_text(cx.t, row_, s().grow().done(), &e.title, ts(r, ty::BODY.0, ty::BODY.1, 600).truncate(), tok::FG);
                        self.nav.add(Focusable { id: row_, kind: Kind::Row, group: 2, hint: "Go to setting".into(), tag: 1000 + (gi * 100 + i) as u32, sid: e.id.clone() });
                    }
                }
            }
            return;
        }
        // Parental controls: a locked section shows nothing until the parent's PIN opens it.
        if self.persist && crate::features::parental::section_locked(self.section.key(), now) {
            let panel = rows::panel(cx, col);
            let row_ = cx.scene.add(panel, s().row().items_center().gap(1.0 * r).min_h(4.5 * r).wp(1.0).px(1.4 * r).py(0.9 * r).done(), Node::default());
            let words = cx.scene.add(row_, s().col().grow().gap(0.2 * r).done(), Node::default());
            cx.scene.add_text(cx.t, words, s().done(), "Locked by parental controls", ts(r, ty::BODY.0, ty::BODY.1, 600).truncate(), tok::FG);
            let msg = format!("Enter the parent's PIN to open {}.", self.section.label());
            cx.scene.add_text(cx.t, words, s().done(), &msg, ts(r, ty::BODY.0, ty::BODY.1, 400).wrap(), tok::MUTED);
            self.nav.add(Focusable { id: row_, kind: Kind::Row, group: 2, hint: "Enter PIN".into(), tag: LOCKED_ROW, sid: String::new() });
            self.view = Box::new(NoView);
            return;
        }
        let cfg = self.cfg.clone();
        self.view = match self.section {
            SectionId::System => Box::new(system::SystemSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Display => Box::new(display::DisplaySection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Sound => Box::new(sound::SoundSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Controls => Box::new(controls::ControlsSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Power => Box::new(power::PowerSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Network => Box::new(network::NetworkSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Accounts => Box::new(accounts::AccountsSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Devices => Box::new(devices::DevicesSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Parental => Box::new(parental::ParentalSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Storage => Box::new(storage::StorageSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Capture => {
                // Entering it reads the device again (and looks for Rare devices on the network).
                if fade && self.persist {
                    crate::features::capture::refresh(true);
                }
                Box::new(capture::CaptureSection::build(cx, &mut self.nav, col, &cfg, now))
            }
            SectionId::About => Box::new(about::AboutSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Developer => Box::new(developer::DeveloperSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Ask => Box::new(ask::AskSection::build(cx, &mut self.nav, col, &cfg, now)),
            SectionId::Licences => {
                about::build_licences(cx, &mut self.nav, col);
                Box::new(about::LicencesView)
            }
            SectionId::Emulators => Box::new(crate::screens::emulators::EmulatorsSection::build(cx, &mut self.nav, col, &cfg, now)),
            // Owned by the notify module.
            SectionId::Notifications => Box::new(crate::screens::notify::section::NotificationsSection::build(cx, &mut self.nav, col, &cfg, now)),
        };
    }

    fn set_section(&mut self, cx: &mut Cx, id: SectionId, now: f64) {
        if self.section == id && !self.searching() {
            return;
        }
        self.section = id;
        self.refresh_nav_look(cx, now, true);
        self.show(cx, now);
    }

    pub fn set_query(&mut self, cx: &mut Cx, q: &str, now: f64) {
        self.query = q.to_string();
        let field_node = self.nav.items[0].id;
        let kids = cx.scene.children(field_node);
        if let Some(t) = kids.first() {
            let shown = if q.is_empty() { "Search settings" } else { q };
            cx.scene.set_text(cx.t, *t, shown);
            cx.scene.n(*t).text.as_mut().unwrap().color.snap(if q.is_empty() { tok::MUTED } else { tok::FG });
        }
        self.refresh_nav_look(cx, now, true);
        self.show(cx, now);
    }

    fn section_of_focus(&self) -> Option<SectionId> {
        let c = self.nav.cur?;
        self.nav_ids.iter().find(|(_, b)| *b == c).map(|(id, _)| *id)
    }
}

/// `.fade`: a short opacity fade.
fn fade_in_quick(cx: &mut Cx, id: Id, now: f64) {
    cx.scene.n(id).opacity.restart(0.0, 1.0, now, 0.2);
}

impl Screen for Settings {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        if !self.nav.focus_at(cx.scene, x, y, now) {
            return false;
        }
        // A section in the list shows it, as moving onto it does.
        if let Some(id) = self.section_of_focus().filter(|_| !self.searching()) {
            self.set_section(cx, id, now);
        }
        true
    }

    fn touch_is_slider(&self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        let ids: Vec<Id> = self.nav.items.iter().map(|f| f.id).collect();
        cx.scene.hit_among(x, y, now, &ids).is_some_and(|row| rows::steps_to(cx.scene, row, x, now).is_some())
    }

    fn touch_slide(&mut self, cx: &mut Cx, x: f32, now: f64) -> Option<crate::screens::overlay::Out> {
        let row = self.nav.cur?;
        let n = rows::steps_to(cx.scene, row, x, now)?;
        let mut cfg = self.cfg.clone();
        let mut last = SectionOut::None;
        for _ in 0..n.unsigned_abs() {
            match self.view.nudge(cx, &mut self.nav, &mut cfg, n.signum(), now) {
                SectionOut::Pass | SectionOut::None => {}
                o => last = o,
            }
        }
        Some(self.apply(cfg, last))
    }
    fn command(&mut self, cx: &mut Cx, cmd: &str, arg: &str, now: f64) {
        self.on_command(cx, cmd, arg, now);
    }

    fn bar(&self) -> Option<Vec<rare_render::focus::hints::Hint>> {
        Some(self.bar_hints())
    }

    fn take_message(&mut self) -> Option<String> {
        self.pending_toast.take()
    }

    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        let searching = self.searching();
        match pad {
            Pad::Y => return Out::Act("settings:search".into()),
            Pad::B if searching => {
                self.set_query(cx, "", now);
                return Out::None;
            }
            Pad::Lt | Pad::Rt if !searching => {
                let next = sections::step(self.section, pad == Pad::Rt);
                let in_nav = self.section_of_focus().is_some();
                self.set_section(cx, next, now);
                if in_nav {
                    let b = self.nav_ids.iter().find(|(id, _)| *id == next).map(|(_, b)| *b).unwrap();
                    self.nav.focus(cx.scene, b, now, true);
                } else if self.nav.len() > self.nav_count {
                    let first = self.nav.items[self.nav_count].id;
                    self.nav.focus(cx.scene, first, now, true);
                }
                return Out::Moved;
            }
            // Right on a section goes into it, like A (the nearest thing to the right was the
            // search field above the list, seen in the first-install walk).
            Pad::Right if !searching && self.section_of_focus().is_some() && self.nav.len() > self.nav_count => {
                let first = self.nav.items[self.nav_count].id;
                self.nav.focus(cx.scene, first, now, true);
                return Out::Moved;
            }
            Pad::Left | Pad::Right => {
                let in_pane = !searching && self.nav.current().is_some_and(|f| self.nav.items.iter().position(|g| g.id == f.id).is_some_and(|i| i >= self.nav_count));
                if in_pane {
                    let dir = if pad == Pad::Left { -1 } else { 1 };
                    let mut cfg = self.cfg.clone();
                    let out = self.view.nudge(cx, &mut self.nav, &mut cfg, dir, now);
                    if out != SectionOut::Pass {
                        return self.apply(cfg, out);
                    }
                }
            }
            Pad::A => {
                let Some(f) = self.nav.current().cloned() else { return Out::None };
                if f.tag == 9000 {
                    return Out::Act("settings:search".into());
                }
                if f.tag == LOCKED_ROW {
                    return Out::Act(format!("parental:unlock:{}", self.section.key()));
                }
                if f.tag >= 1000 && f.tag < 9000 {
                    if let Some(e) = self.hit_entry(f.tag - 1000) {
                        self.go(cx, e, now);
                    }
                    return Out::Moved;
                }
                if self.section_of_focus().is_some() {
                    // A on a section moves into its first row.
                    if self.nav.len() > self.nav_count {
                        let first = self.nav.items[self.nav_count].id;
                        self.nav.focus(cx.scene, first, now, true);
                    }
                    return Out::Moved;
                }
                // Parental controls: these sections need the parent's PIN to change.
                if self.persist && crate::features::parental::section_locked(self.section.key(), now) {
                    return Out::Act(format!("parental:unlock:{}", self.section.key()));
                }
                let mut cfg = self.cfg.clone();
                let out = self.view.press(cx, &mut self.nav, &mut cfg, now);
                let changed = out == SectionOut::Changed;
                let res = self.apply(cfg, out);
                // The section draws again from the new config, so the picked choice and the lines
                // that describe it show the change (Testnet, Sleep when idle, Model provider...).
                if changed {
                    self.refresh(cx, now);
                }
                return res;
            }
            Pad::B => return Out::Close,
            _ => {}
        }
        if matches!(pad, Pad::Up | Pad::Down | Pad::Left | Pad::Right) {
            // The section list (search field and sections) and the pane are two columns: up and
            // down stay in the column, right goes into the pane, left at the pane's edge goes back
            // to the open section. (A spatial step across both jumped to a far section when either
            // column was scrolled, and a fast scroll then lost the focus off screen.)
            let n = self.nav_count;
            let in_pane = self.nav.cur.and_then(|c| self.nav.items.iter().position(|f| f.id == c)).is_some_and(|i| i >= n);
            let moved = match (in_pane, pad) {
                (true, Pad::Left) => {
                    // Left stays in the pane only for a button on the same line (Save | Get a link).
                    let cur = cx.scene.rect(self.nav.cur.unwrap());
                    let mid = cur[1] + cur[3] / 2.0;
                    let same_line: Vec<bool> = self.nav.items.iter().map(|f| {
                        let r = cx.scene.rect(f.id);
                        r[1] <= mid && mid <= r[1] + r[3] && r[0] + r[2] <= cur[0] + 1.0
                    }).collect();
                    let m = self.nav.step_in(cx.scene, pad, now, |i| i >= n && same_line[i]);
                    if !m {
                        let row = if searching { None } else { self.nav_ids.iter().find(|(id, _)| *id == self.section).map(|(_, b)| *b) };
                        if let Some(b) = row.or_else(|| self.nav.items.first().map(|f| f.id)) {
                            self.nav.focus(cx.scene, b, now, true);
                            return Out::Moved;
                        }
                    }
                    m
                }
                (true, _) => self.nav.step_in(cx.scene, pad, now, |i| i >= n),
                (false, Pad::Right) => {
                    // Into the pane at its first row (the list may be scrolled, the pane is at its top).
                    if self.nav.len() > n {
                        let first = self.nav.items[n].id;
                        self.nav.focus(cx.scene, first, now, true);
                        return Out::Moved;
                    }
                    false
                }
                (false, _) => self.nav.step_in(cx.scene, pad, now, |i| i < n),
            };
            if moved && !searching {
                // Focusing a section shows it.
                if let Some(id) = self.section_of_focus() {
                    self.set_section(cx, id, now);
                }
            }
            return if moved { Out::Moved } else { Out::None };
        }
        Out::None
    }
}

impl Settings {
    fn apply(&mut self, cfg: Config, out: SectionOut) -> Out {
        match out {
            SectionOut::None | SectionOut::Pass => Out::None,
            SectionOut::Changed => {
                self.cfg = cfg;
                self.dirty = true;
                if self.persist {
                    match config::save_diff(&self.saved, &self.cfg) {
                        Ok(()) => {
                            let ui_scale_changed = self.saved.ui_scale != self.cfg.ui_scale;
                            self.saved = self.cfg.clone();
                            return Out::Act(if ui_scale_changed { "settings:saved:ui-scale".into() } else { "settings:saved".into() });
                        }
                        Err(e) => return Out::Act(format!("toast:error:{}", live::failed("Couldn't save that setting.", &e, None))),
                    }
                }
                Out::Act("settings:save".into())
            }
            SectionOut::Act(a) => Out::Act(a),
        }
    }

    fn hit_entry(&self, code: u32) -> Option<SettingEntry> {
        let (g, i) = ((code / 100) as usize, (code % 100) as usize);
        self.hits.get(g).and_then(|h| h.entries.get(i)).cloned()
    }

    /// A search result picked: leave search, open the section, land on the row.
    fn go(&mut self, cx: &mut Cx, e: SettingEntry, now: f64) {
        self.set_query(cx, "", now);
        self.section = e.section;
        self.refresh_nav_look(cx, now, true);
        self.show(cx, now);
        let target = self.nav.items.iter().skip(self.nav_count).find(|f| f.sid == e.id).map(|f| f.id);
        let to = target.or_else(|| self.nav.items.get(self.nav_count).map(|f| f.id)).or_else(|| self.nav_ids.iter().find(|(id, _)| *id == e.section).map(|(_, b)| *b));
        if let Some(t) = to {
            self.nav.focus(cx.scene, t, now, true);
        }
    }
}

/// The page area under the top bar (4.25rem) and above the hint bar (3rem), for a standalone fixture.
pub fn fixture_area(cx: &mut Cx) -> Id {
    let r = cx.r();
    let root = cx.scene.root;
    cx.scene.add(root, s().abs(Some(4.25 * r), Some(0.0), Some(3.0 * r), Some(0.0)).done(), Node::default())
}

/// Fixture screens: `settings-<section key>` (that section, focus on its list row), and
/// `settings-search` with a query.
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    if let Some(f) = ossheets::fixture(name, cx, now) {
        return Some(f);
    }
    if let Some(f) = remap::fixture(name, cx, now) {
        return Some(f);
    }
    if let Some(f) = supportsheets::fixture(name, cx, now) {
        return Some(f);
    }
    if name == "upscale-rows" {
        let r = cx.r();
        let area = fixture_area(cx);
        let col = cx.scene.add(area, s().col().gap(0.9 * r).w(30.0 * r).pad(2.5 * r, 2.0 * r, 0.0, 0.0).done(), Node::default());
        let mut nav = Nav::default();
        upscale::UpscaleRows::build(cx, &mut nav, col, upscale::Upscale { mode: upscale::Mode::Fsr, ..Default::default() });
        let first = nav.items[0].id;
        nav.focus(cx.scene, first, now, false);
        return Some(Box::new(NavScreen { nav }));
    }
    if let Some(v) = name.strip_prefix("settings-storage-") {
        let v: &'static str = match v {
            "nocard" => "nocard",
            "notready" => "notready",
            "empty" => "empty",
            "many" => "many",
            "moving" => "moving",
            "format" => "format",
            _ => return None,
        };
        storage::set_fixture(v);
        let area = fixture_area(cx);
        return Some(Box::new(Settings::build(cx, area, config::fixture(), SectionId::Storage, "", now)));
    }
    // settings-capture[-off|-many|-unavailable]: Capture and streaming on fixture readings.
    if let Some(kind) = name.strip_prefix("settings-capture") {
        crate::features::capture::use_fixture(kind.trim_start_matches('-'));
        let area = fixture_area(cx);
        return Some(Box::new(Settings::build(cx, area, config::fixture(), SectionId::Capture, "", now)));
    }
    let rest = name.strip_prefix("settings-")?;
    let area = fixture_area(cx);
    let cfg = config::fixture();
    if rest == "search" {
        return Some(Box::new(Settings::build(cx, area, cfg, SectionId::System, "bright", now)));
    }
    if rest == "search-empty" {
        return Some(Box::new(Settings::build(cx, area, cfg, SectionId::System, "zzzz", now)));
    }
    // settings-update-<state>: Settings > System in one update state, focus on its last button.
    if let Some(state) = rest.strip_prefix("update-") {
        if !update::set_fixture_state(state) {
            return None;
        }
        let mut me = Settings::build(cx, area, cfg, SectionId::System, "", now);
        let last = me.nav.items.iter().filter(|f| f.sid == "rollback" || f.sid == "updates").last().map(|f| f.id);
        if let Some(id) = last {
            me.nav.focus(cx.scene, id, now, false);
        }
        return Some(Box::new(me));
    }
    let sec = SectionId::from_key(rest)?;
    Some(Box::new(Settings::build(cx, area, cfg, sec, "", now)))
}

impl Settings {
    /// The Emulators section in its states, for the emulators module's fixtures.
    pub fn fixture_with_emulators(cx: &mut Cx, name: &str, now: f64) -> Settings {
        use crate::screens::emulators::{fixture_list, EmuUpdateInfo, EmulatorsSection};
        let area = fixture_area(cx);
        let mut me = Settings::build(cx, area, config::fixture(), SectionId::Emulators, "", now);
        me.nav.truncate(me.nav_count);
        cx.scene.clear_children(me.pane);
        let r = cx.r();
        let col = cx.scene.add(me.pane, s().col().wp(1.0).gap(1.25 * r).done(), Node::default());
        let (list, found, busy) = match name {
            "emulators-update" => (Some(fixture_list()), vec![("rpcs3".to_string(), EmuUpdateInfo { available: true, tag: "v0.0.35".into(), verify: "checksum".into() })], None),
            "emulators-empty" => (Some(vec![]), vec![], None),
            _ => (Some(fixture_list()), vec![], None),
        };
        me.view = Box::new(EmulatorsSection::build_with(cx, &mut me.nav, col, list, &found, busy));
        me
    }
}

impl Settings {
    /// Draws the open section again from the latest readings, keeping the focused row and scroll.
    fn refresh(&mut self, cx: &mut Cx, now: f64) {
        if self.searching() {
            return;
        }
        let keep = self.nav.current().filter(|f| self.nav.items.iter().position(|g| g.id == f.id).is_some_and(|i| i >= self.nav_count)).map(|f| (f.sid.clone(), f.tag));
        // Where the pane is gliding to (a refresh mid-scroll kept the halfway point and left the
        // focused row off screen).
        let sy = cx.scene.n(self.pane).scroll_y.target();
        let keep_i = self.nav.cur.and_then(|c| self.nav.items.iter().position(|f| f.id == c)).filter(|i| *i >= self.nav_count);
        self.show_with(cx, now, false);
        cx.scene.n(self.pane).scroll_y.snap(sy);
        if let Some((sid, tag)) = keep {
            let t = self.nav.items.iter().skip(self.nav_count).find(|f| f.sid == sid && f.tag == tag && !(sid.is_empty() && tag == 0)).or_else(|| self.nav.items.iter().skip(self.nav_count).find(|f| f.tag == tag && tag != 0)).map(|f| f.id);
            // A row with no name: the same place in the rebuilt pane, else its last row, else the section.
            let t = t.or_else(|| keep_i.and_then(|i| self.nav.items.get(i).or_else(|| self.nav.items.last().filter(|_| self.nav.len() > self.nav_count)).map(|f| f.id)));
            let t = t.or_else(|| self.nav_ids.iter().find(|(id, _)| *id == self.section).map(|(_, b)| *b));
            if let Some(t) = t {
                self.nav.focus(cx.scene, t, now, false);
                self.nav.reveal(cx.scene, t, now);
            }
        }
    }
}

impl Settings {
    fn on_command(&mut self, cx: &mut Cx, cmd: &str, arg: &str, now: f64) {
        match cmd {
            "query" => self.set_query(cx, arg, now),
            "goto" => {
                if let Some(id) = SectionId::from_key(arg) {
                    self.set_query(cx, "", now);
                    self.set_section(cx, id, now);
                    if let Some((_, b)) = self.nav_ids.iter().find(|(i, _)| *i == id) {
                        let b = *b;
                        self.nav.focus(cx.scene, b, now, true);
                    }
                }
            }
            "refresh" => self.refresh(cx, now),
            "add-folder" | "grid-key" | "ask-model" | "ask-url" | "endpoint-testnetRpc" | "endpoint-testnetApi" | "endpoint-testnetSponsor" => {
                let v = arg.trim();
                let mut cfg = self.cfg.clone();
                match cmd {
                    "add-folder" => {
                        if v.is_empty() {
                            return;
                        }
                        cfg.game_folders.retain(|x| x != v);
                        cfg.game_folders.push(v.to_string());
                    }
                    "grid-key" => cfg.steamgriddb_key = if v.is_empty() { None } else { Some(v.to_string()) },
                    "ask-model" => cfg.agent_model = (!v.is_empty()).then(|| v.to_string()),
                    "ask-url" => {
                        let Some(u) = ask::clean_url(v) else {
                            self.pending_toast = Some("That address needs to start with http:// or https://.".into());
                            return;
                        };
                        cfg.agent_url = u;
                    }
                    _ => {
                        let Some(clean) = developer::clean_endpoint(v) else {
                            self.pending_toast = Some("That address needs to start with https://.".into());
                            return;
                        };
                        match cmd {
                            "endpoint-testnetRpc" => cfg.testnet_rpc = clean,
                            "endpoint-testnetApi" => cfg.testnet_api = clean,
                            _ => cfg.testnet_sponsor = clean,
                        }
                    }
                }
                if self.cfg != cfg {
                    self.cfg = cfg;
                    self.dirty = true;
                    if self.persist {
                        if let Err(e) = config::save_diff(&self.saved, &self.cfg) {
                            self.pending_toast = Some(live::failed("Couldn't save that setting.", &e, None));
                            return;
                        }
                        self.saved = self.cfg.clone();
                    }
                    self.refresh(cx, now);
                }
            }
            "reload" => {
                let cfg = config::load();
                self.saved = cfg.clone();
                self.cfg = cfg;
                self.refresh(cx, now);
            }
            _ => {}
        }
    }

    fn bar_hints(&self) -> Vec<rare_render::focus::hints::Hint> {
        use rare_render::focus::hints::{hint, Key};
        let a = self.nav.current().map(|f| f.hint.clone()).filter(|h| !h.is_empty()).unwrap_or_else(|| "Select".into());
        let mut v = vec![hint(Key::A, &a)];
        v.extend([hint(Key::B, if self.searching() { "Clear search" } else { "Back" }), hint(Key::Y, "Search"), hint(Key::LbRb, "Tabs")]);
        if !self.searching() {
            v.push(hint(Key::LtRt, "Sections"));
        }
        v
    }
}

/// Presses A on every row of every section (fixture config) and prints what each one asked for:
/// `cargo test -p rare-launcher-native settings::crawl -- --nocapture --ignored`.
#[cfg(test)]
mod crawl {
    use super::*;
    use rare_render::{ImageLoader, Scene, TextSys};
    use std::sync::Arc;

    static FONTS: [&[u8]; 1] = [include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf")];

    fn texts(scene: &rare_render::Scene, id: Id, out: &mut Vec<String>) {
        if let Some(t) = scene.get(id).text.as_ref() {
            out.push(t.text.clone());
        }
        for c in scene.children(id) {
            texts(scene, c, out);
        }
    }

    #[test]
    #[ignore]
    fn crawl_every_row() {
        let mut text = TextSys::new(&FONTS, false, 512);
        let mut images = ImageLoader::new(1, Arc::new(|| {}), None);
        for (sec, name) in SECTIONS {
            let mut scene = Scene::new(1280.0, 800.0, 16.0);
            let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
            let area = fixture_area(&mut cx);
            let probe = Settings::build(&mut cx, area, config::fixture(), sec, "", 0.0);
            let n = probe.nav.len() - probe.nav_count;
            println!("== {name} ({n} rows)");
            for i in 0..n {
                let mut scene = Scene::new(1280.0, 800.0, 16.0);
                let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
                let area = fixture_area(&mut cx);
                let mut me = Settings::build(&mut cx, area, config::fixture(), sec, "", 0.0);
                let f = me.nav.items[me.nav_count + i].clone();
                me.nav.focus(cx.scene, f.id, 0.0, false);
                let mut words = vec![];
                texts(cx.scene, f.id, &mut words);
                let before = me.nav.len();
                let out = me.pad(&mut cx, Pad::A, 1.0);
                let after = me.nav.len();
                let label = words.join(" | ");
                let label: String = label.chars().take(90).collect();
                println!("  [{i}] sid={} hint={:?} -> {:?} nav {}->{} dirty={} :: {}", f.sid, f.hint, out, before, after, me.dirty, label);
            }
        }
    }
}
