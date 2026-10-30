//! The notification centre (notify/Centre.tsx): recent notices newest first with how long ago and a
//! dot for the unread, per-kind action hints, Mark all read and Clear in the head, Steam downloads
//! in progress above the list, and a calm empty line. Plus the small counts the shell shows.

use super::data::actions::action_label;
use super::data::downloads::{download_fraction, Download};
use super::data::store::NotifyStore;
use super::data::types::now_ms;
use super::ui::{notice_art, time_ago, ArtSource};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::input::Pad;
use rare_render::scene::{layer, Shadow};
use rare_render::theme::tok;
use rare_render::{Color, Id, Node};

/// How many notices the list shows.
pub const SHOW: usize = 8;
/// "3m" moves on every 30 s while the sheet is open.
pub const TICK_SECS: f64 = 30.0;

const G_HEAD: u32 = 1;
const G_DOWNLOADS: u32 = 2;
const G_LIST: u32 = 3;
const T_MARK: u32 = 1000;
const T_CLEAR: u32 = 1001;

pub struct Centre {
    section: Id,
    pub nav: Nav,
    pub store: NotifyStore,
    pub downloads: Vec<Download>,
    art: Box<dyn ArtSource>,
    tick_at: f64,
}

/// "Downloading", "Paused" or the percentage (Centre.tsx DownloadRows).
pub fn download_status(d: &Download) -> String {
    if d.paused {
        return "Paused".into();
    }
    match download_fraction(d) {
        None => "Downloading".into(),
        Some(f) => format!("{}%", (f * 100.0).round() as i64),
    }
}

/// The unread count beside the Quick settings hint: a plain number, nothing when zero.
pub fn unread_label(store: &NotifyStore) -> Option<String> {
    let n = store.unread();
    (n > 0).then(|| n.to_string())
}

/// What came in while a game runs, in the bottom bar's Playing cell: "N new".
pub fn missed_label(store: &NotifyStore) -> Option<String> {
    (store.missed > 0).then(|| format!("{} new", store.missed))
}

fn chip_button(cx: &mut Cx, parent: Id, label: &str) -> Focusable {
    let r = cx.r();
    let id = cx.scene.add(parent, s().row().items_center().justify_center().h(1.9 * r).px(0.7 * r).done(), Node::bg(tok::L2).radius(0.25 * r));
    let t = cx.scene.add_text(cx.t, id, s().done(), label, ts(r, ty::META.0, ty::META.1, 500), tok::FG);
    Focusable { id, kind: Kind::Btn { base: tok::L2, fg: tok::FG, text: t }, group: 0, hint: String::new(), tag: 0, sid: String::new() }
}

impl Centre {
    /// The centre's section, built into `parent` (it is the content of the Quick settings aside).
    pub fn build(cx: &mut Cx, parent: Id, store: NotifyStore, downloads: Vec<Download>, art: Box<dyn ArtSource>, now: f64) -> Centre {
        let r = cx.r();
        let section = cx.scene.add(parent, s().col().wp(1.0).gap(0.5 * r).done(), Node::default());
        let mut me = Centre { section, nav: Nav::default(), store, downloads, art, tick_at: now + TICK_SECS };
        me.rebuild(cx, now, None);
        me
    }

    /// Draws the whole section again from the store, keeping focus where it was by position.
    pub fn rebuild(&mut self, cx: &mut Cx, now: f64, keep: Option<usize>) {
        let r = cx.r();
        cx.scene.clear_children(self.section);
        self.nav.clear();
        let t_ms = now_ms();
        let list: Vec<_> = self.store.visible().into_iter().cloned().collect();
        let unread = self.store.unread();
        // Head.
        let head = cx.scene.add(self.section, s().row().items_center().gap(0.5 * r).done(), Node::default());
        text(cx, head, ty::LABEL, 500, tok::MUTED, "Notifications");
        if unread > 0 {
            cx.scene.add_text(cx.t, head, s().done(), &format!("{unread} new"), ts(r, ty::LABEL.0, ty::LABEL.1, 400).tabular(), tok::FG);
        }
        if !list.is_empty() {
            let btns = cx.scene.add(head, s().row().gap(0.4 * r).ml_auto().done(), Node::default());
            if unread > 0 {
                let b = chip_button(cx, btns, "Mark all read").group(G_HEAD).hint("Mark all read").tag(T_MARK);
                self.nav.add(b);
            }
            let b = chip_button(cx, btns, "Clear").group(G_HEAD).hint("Clear all").tag(T_CLEAR);
            self.nav.add(b);
        }
        // Downloads in progress.
        if !self.downloads.is_empty() {
            let col = cx.scene.add(self.section, s().col().done(), Node::default());
            for (i, d) in self.downloads.clone().iter().enumerate() {
                let row_ = cx.scene.add(col, s().col().justify_center().gap(0.4 * r).min_h(3.5 * r).wp(1.0).px(0.6 * r).py(0.45 * r).done(), Node::default().radius(0.5 * r));
                let top = cx.scene.add(row_, s().row().items_center().gap(0.5 * r).done(), Node::default());
                cx.scene.add_text(cx.t, top, s().grow().done(), &d.title, ts(r, ty::SMALL.0, ty::SMALL.1, 600).truncate(), tok::FG);
                let t = cx.scene.add_text(cx.t, top, s().done(), &download_status(d), ts(r, ty::LABEL.0, ty::LABEL.1, 400).tabular(), tok::MUTED);
                let _ = t;
                let bar = cx.scene.add(row_, s().h(3.0).wp(1.0).done(), Node::bg(tok::L2).radius(1.5).clip());
                let f = download_fraction(d).unwrap_or(0.0);
                cx.scene.add(bar, s().hp(1.0).wp(((f * 100.0).round() / 100.0) as f32).done(), Node::bg(tok::FG).radius(1.5));
                self.nav.add(Focusable { id: row_, kind: Kind::Row, group: G_DOWNLOADS, hint: "Open library".into(), tag: 2000 + i as u32, sid: String::new() });
            }
        }
        // The list.
        if list.is_empty() {
            let b = cx.scene.add(self.section, s().px(0.95 * r).py(0.8 * r).done(), Node::default().radius(0.5 * r).border(1.0, tok::HAIR));
            text(cx, b, ty::SMALL, 400, tok::MUTED, "No notifications yet.");
        } else {
            let col = cx.scene.add(self.section, s().col().done(), Node::default());
            for (i, n) in list.iter().take(SHOW).enumerate() {
                let read = self.store.is_read(n);
                let row_ = cx.scene.add(col, s().row().items_center().gap(0.75 * r).min_h(3.5 * r).wp(1.0).px(0.6 * r).py(0.45 * r).done(), Node::default().radius(0.5 * r));
                notice_art(cx, row_, n, self.art.art(n), 2.5);
                let c = cx.scene.add(row_, s().col().grow().done(), Node::default());
                let (w, col_) = if read { (400, tok::FG.alpha(0.8)) } else { (600, tok::FG) };
                text_tr(cx, c, ty::SMALL, w, col_, &n.title);
                let sub = [Some(time_ago(Some(n.created_at), t_ms)), n.body.clone()].into_iter().flatten().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" \u{b7} ");
                text_tr(cx, c, ty::LABEL, 400, tok::MUTED, &sub);
                if !read {
                    dot(cx, row_, tok::FG);
                }
                self.nav.add(Focusable { id: row_, kind: Kind::Row, group: G_LIST, hint: action_label(Some(&n.action), Some(n)).into(), tag: i as u32, sid: n.id.clone() });
            }
        }
        // Focus.
        if let Some(i) = keep {
            if !self.nav.is_empty() {
                let id = self.nav.items[i.min(self.nav.len() - 1)].id;
                self.nav.focus(cx.scene, id, now, false);
            }
        }
        self.tick_at = now + TICK_SECS;
    }

    fn cur_index(&self) -> Option<usize> {
        let c = self.nav.cur?;
        self.nav.items.iter().position(|f| f.id == c)
    }

    /// Focuses the first row of the list (or the first thing there is).
    pub fn focus_first(&mut self, cx: &mut Cx, now: f64) {
        let first = self.nav.items.iter().find(|f| f.group == G_LIST).or(self.nav.items.first()).map(|f| f.id);
        if let Some(id) = first {
            self.nav.focus(cx.scene, id, now, false);
        }
    }
}

impl Screen for Centre {
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
                let Some(f) = self.nav.current().cloned() else { return Out::None };
                let keep = self.cur_index();
                match (f.group, f.tag) {
                    (G_HEAD, T_MARK) => {
                        self.store.mark_all_read(now_ms());
                        self.rebuild(cx, now, keep);
                        Out::None
                    }
                    (G_HEAD, T_CLEAR) => {
                        self.store.clear(now_ms());
                        self.rebuild(cx, now, None);
                        Out::None
                    }
                    (G_DOWNLOADS, _) => Out::Act("downloads".into()),
                    (G_LIST, _) => {
                        self.store.mark_read(&f.sid);
                        self.rebuild(cx, now, keep);
                        Out::Act(format!("notice:{}", f.sid))
                    }
                    _ => Out::None,
                }
            }
            _ => Out::None,
        }
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if now >= self.tick_at {
            let keep = self.cur_index();
            self.rebuild(cx, now, keep);
        }
    }

    fn next_deadline(&self) -> Option<f64> {
        Some(self.tick_at)
    }
}

/// The Quick settings aside the centre sits in (w 27rem, bg-l1, border-l, the edge shadow), on the
/// sheet layer.
pub struct CentreSheet {
    pub centre: Centre,
}

impl CentreSheet {
    pub fn build(cx: &mut Cx, store: NotifyStore, downloads: Vec<Download>, art: Box<dyn ArtSource>, now: f64) -> CentreSheet {
        let r = cx.r();
        let layer = cx.scene.layer(layer::SHEET);
        let aside = cx.scene.add(
            layer,
            s().abs(Some(0.0), Some(0.0), Some(3.0 * r), None).col().w(27.0 * r).p(1.4 * r).done(),
            Node::bg(tok::L1).shadow(&[Shadow::rem(-2.0, 0.0, 4.0, 0.45, r)]),
        );
        edge(cx, aside, 3, tk::BORDER_LIGHT);
        let _: Color = tok::L1;
        let mut centre = Centre::build(cx, aside, store, downloads, art, now);
        centre.focus_first(cx, now);
        CentreSheet { centre }
    }
}

impl Screen for CentreSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.centre.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.centre.pad(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        self.centre.tick(cx, now)
    }
    fn next_deadline(&self) -> Option<f64> {
        self.centre.next_deadline()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_status_text() {
        let d = |done, total, paused| Download { id: "1".into(), title: "x".into(), done, total, paused };
        assert_eq!(download_status(&d(1.0, 4.0, false)), "25%");
        assert_eq!(download_status(&d(1.0, 4.0, true)), "Paused");
        assert_eq!(download_status(&d(1.0, 0.0, false)), "Downloading");
    }
}
