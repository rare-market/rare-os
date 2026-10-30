//! Toasts (notify/Toast.tsx and Chrome.tsx Toasts): notice toasts with art, the sentence and one
//! action on Menu (Start), and the plain app toasts (dock changes, "Press A to play"). They stack
//! downward, newest on top, at the top right under the top bar on the toast layer and pop in
//! (320 ms, from 96 %). Menu takes the newest notice toast only after it has been up for 400 ms.

use super::data::actions::action_label;
use super::data::store::{NotifyStore, Toast, TOAST_MS};
use super::data::types::{now_ms, Notice};
use super::ui::{notice_art, ArtSource};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::input::Pad;
use rare_render::scene::{layer, Shadow};
use rare_render::theme::{motion, tok};
use rare_render::{Id, Node};

/// A press this soon after a toast appears is meant for the page, not the toast.
pub const GUARD_MS: i64 = 400;

/// lib/store.ts toast(): a line with a dot, `error` for the negative tone.
#[derive(Clone, Debug, PartialEq)]
pub struct AppToast {
    pub id: u64,
    pub text: String,
    pub error: bool,
}

struct Row {
    key: String,
    node: Id,
    hint: Option<Id>,
}

/// The toast column on `layer::TOAST`. `sync` makes it show exactly what the store and the app
/// toasts hold, popping in new ones.
pub struct ToastLayer {
    col: Id,
    rows: Vec<Row>,
}

fn pop_in(cx: &mut Cx, id: Id, now: f64) {
    cx.scene.n(id).opacity.restart(0.0, 1.0, now, motion::SLOW);
    cx.scene.n(id).scale.restart(0.96, 1.0, now, motion::SLOW);
}

fn notice_toast(cx: &mut Cx, parent: Id, t: &Toast, art: &dyn ArtSource, hint: bool) -> (Id, Id) {
    let r = cx.r();
    let n = &t.notice;
    let (w, py, pl, pr, side) = if t.small { (22.0, 0.45, 0.45, 0.8, 2.2) } else { (28.0, 0.7, 0.7, 1.0, 3.0) };
    let card = cx.scene.add(
        parent,
        s().row().items_center().gap(0.85 * r).w(w * r).pad(pl * r, py * r, pr * r, py * r).done(),
        Node::bg(tok::L3).radius(0.5 * r).border(1.0, tk::BORDER_LIGHT).shadow(&Shadow::xl(0.3)),
    );
    notice_art(cx, card, n, art.art(n), side);
    let col = cx.scene.add(card, s().col().grow().done(), Node::default());
    text_tr(cx, col, ty::SMALL, 600, tok::FG, &n.title);
    if let Some(b) = n.body.as_deref().filter(|b| !b.is_empty()) {
        text_tr(cx, col, ty::META, 400, tok::MUTED, b);
    }
    // Only the newest toast takes Menu, so only it shows the glyph (the others keep the room).
    let h = cx.scene.add(card, s().row().items_center().gap(0.45 * r).done(), Node::default().opacity(if hint { 1.0 } else { 0.0 }));
    glyph(cx, h, G::Start, false);
    text(cx, h, ty::META, 400, tok::FG.alpha(0.8), action_label(Some(&n.action), Some(n)));
    (card, h)
}

fn app_toast(cx: &mut Cx, parent: Id, t: &AppToast) -> Id {
    let r = cx.r();
    let card = cx.scene.add(parent, s().row().items_center().gap(0.6 * r).px(1.1 * r).py(0.6 * r).done(), Node::bg(tok::L3).radius(0.5 * r).border(1.0, tk::BORDER_LIGHT).shadow(&Shadow::xl(0.3)));
    dot(cx, card, if t.error { tok::NEG } else { tok::FG });
    text(cx, card, ty::SMALL, 400, tok::FG, &t.text);
    card
}

impl ToastLayer {
    pub fn build(cx: &mut Cx) -> ToastLayer {
        let r = cx.r();
        let layer = cx.scene.layer(layer::TOAST);
        // Top right, under the top bar, like every toast (the app's own stack under these).
        let col = cx.scene.add(layer, s().abs(Some(4.75 * r), Some(2.5 * r), None, None).col().items_end().gap(0.5 * r).done(), Node::default());
        ToastLayer { col, rows: vec![] }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The column's height as last laid out (0 when empty): the app's toasts sit under it.
    pub fn height(&self, scene: &rare_render::Scene) -> f32 {
        if self.rows.is_empty() {
            0.0
        } else {
            scene.rect(self.col)[3]
        }
    }

    pub fn sync(&mut self, cx: &mut Cx, store: &NotifyStore, app: &[AppToast], art: &dyn ArtSource, now: f64) {
        let keys: Vec<String> = store.toasts.iter().map(|t| format!("n{}", t.notice.id)).chain(app.iter().map(|t| format!("a{}", t.id))).collect();
        // Gone ones leave.
        let mut i = 0;
        while i < self.rows.len() {
            if keys.contains(&self.rows[i].key) {
                i += 1;
            } else {
                let row = self.rows.remove(i);
                cx.scene.remove(row.node);
            }
        }
        // New ones pop in.
        let newest = store.toasts.last().map(|t| format!("n{}", t.notice.id));
        for (idx, key) in keys.iter().enumerate() {
            if self.rows.iter().any(|r| &r.key == key) {
                continue;
            }
            let (node, hint) = if idx < store.toasts.len() {
                let (c, h) = notice_toast(cx, self.col, &store.toasts[idx], art, newest.as_ref() == Some(key));
                (c, Some(h))
            } else {
                (app_toast(cx, self.col, &app[idx - store.toasts.len()]), None)
            };
            pop_in(cx, node, now);
            self.rows.push(Row { key: key.clone(), node, hint });
        }
        // Only the newest shows the Menu hint.
        for row in &self.rows {
            if let Some(h) = row.hint {
                let on = newest.as_ref() == Some(&row.key);
                cx.scene.n(h).opacity.set(if on { 1.0 } else { 0.0 }, now, motion::FAST);
            }
        }
        // Newest on top, older ones pushed down.
        let order: Vec<Id> = keys.iter().rev().filter_map(|k| self.rows.iter().find(|r| &r.key == k).map(|r| r.node)).collect();
        cx.scene.set_children(self.col, &order);
    }
}

/// Opening a notice from its toast: dismiss, mark read, report the action. The shell maps the
/// `notice:<id>` act back to the notice (data::actions::notice_target) and navigates.
pub fn open_newest(store: &mut NotifyStore, now_ms_: i64) -> Option<Notice> {
    let t = store.toasts.last()?;
    if now_ms_ - t.at < GUARD_MS {
        return None;
    }
    let n = t.notice.clone();
    store.dismiss_toast(&n.id);
    store.mark_read(&n.id);
    Some(n)
}

/// The toasts as a screen: owns a store (fixtures) and answers Start. The shell uses
/// `ToastLayer` with its own store instead.
pub struct NoticeToasts {
    pub store: NotifyStore,
    pub app: Vec<AppToast>,
    layer: ToastLayer,
    art: Box<dyn ArtSource>,
    deadline: Option<f64>,
}

impl NoticeToasts {
    pub fn build(cx: &mut Cx, store: NotifyStore, app: Vec<AppToast>, art: Box<dyn ArtSource>, now: f64) -> NoticeToasts {
        let mut me = NoticeToasts { store, app, layer: ToastLayer::build(cx), art, deadline: None };
        me.sync(cx, now);
        me
    }

    fn sync(&mut self, cx: &mut Cx, now: f64) {
        self.layer.sync(cx, &self.store, &self.app, self.art.as_ref(), now);
        let t = now_ms();
        self.deadline = self.store.toasts.iter().map(|x| now + ((x.at + TOAST_MS - t) as f64 / 1000.0).max(0.0)).fold(None, |a: Option<f64>, d| Some(a.map_or(d, |a| a.min(d))));
    }
}

impl Screen for NoticeToasts {
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        if pad != Pad::Start {
            return Out::None;
        }
        match open_newest(&mut self.store, now_ms()) {
            Some(n) => {
                self.sync(cx, now);
                Out::Act(format!("notice:{}", n.id))
            }
            None => Out::None,
        }
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.store.expire_toasts(now_ms()) {
            self.sync(cx, now);
        }
    }

    fn next_deadline(&self) -> Option<f64> {
        self.deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::notify::data::storage::MemStorage;
    use crate::screens::notify::fixtures;
    use std::sync::Arc;

    fn store() -> NotifyStore {
        NotifyStore::load(Arc::new(MemStorage::new()))
    }

    #[test]
    fn menu_waits_out_the_guard_then_opens_and_reads() {
        let mut st = store();
        let n = fixtures::sold(1_000_000, 0);
        st.add(&[n.clone()]);
        st.show_toast(&n, false, false, 1_000_000);
        assert!(open_newest(&mut st, 1_000_100).is_none());
        assert_eq!(st.toasts.len(), 1);
        let got = open_newest(&mut st, 1_000_500).unwrap();
        assert_eq!(got.id, n.id);
        assert!(st.toasts.is_empty());
        assert!(st.read.contains(&n.id));
    }

    #[test]
    fn the_newest_toast_is_the_one_menu_takes() {
        let mut st = store();
        let (a, b) = (fixtures::sold(0, 0), fixtures::offer(0, 0));
        st.show_toast(&a, false, false, 0);
        st.show_toast(&b, false, false, 10);
        assert_eq!(open_newest(&mut st, 10_000).unwrap().id, b.id);
    }
}
