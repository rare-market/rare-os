//! Settings > Notifications (notify/SettingsSection.tsx): Do not disturb, toasts over a game and
//! desktop notifications; a switch per kind of notice under its class; this device's price alerts
//! (A deletes one). `NotificationsSection` plugs into `settings::SectionView`:
//! `SectionId::Notifications => Box::new(notify::section::NotificationsSection::build(cx, &mut self.nav, col, &cfg, now))`.

use super::backend::{AlertsApi, AlertsState, MemAlerts};
use super::data::kinds::kinds;
use super::data::logic::{class_of, NoticeClass, CLASSES};
use super::data::storage::MemStorage;
use super::data::store::NotifyStore;
use super::data::types::{AlertDir, PriceAlert};
use super::fixtures::SUI;
use super::ui::usd_price;
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use crate::screens::settings::config::Config;
use crate::screens::settings::rows::*;
use crate::screens::settings::{SectionOut, SectionView};
use rare_render::theme::tok;
use rare_render::{Id, Node};
use std::sync::Arc;

pub fn class_note(c: NoticeClass) -> &'static str {
    match c {
        NoticeClass::Money | NoticeClass::Invites | NoticeClass::Messages => "Pops up over a game. Waits in the list with Do not disturb.",
        NoticeClass::Trophies => "A small toast over a game. Waits in the list with Do not disturb.",
        NoticeClass::Social | NoticeClass::System => "Pops up in Rare. Waits in the list over a game and with Do not disturb.",
        NoticeClass::Battery => "Always pops up, even over a game and with Do not disturb.",
    }
}

/// The body line of the "Your price alerts" row.
pub fn alerts_body(list: Option<&[PriceAlert]>, error: Option<&str>) -> &'static str {
    match list {
        Some(l) if l.is_empty() => "No price alerts. Set one from a coin's page in Trade.",
        None if error.is_some() => "Could not load price alerts.",
        _ => "One notice when the price crosses your target, then the alert stops.",
    }
}

/// Active alerts first, then up to five that were hit.
pub fn alert_rows(list: &[PriceAlert]) -> Vec<&PriceAlert> {
    list.iter().filter(|a| a.active).chain(list.iter().filter(|a| !a.active).take(5)).collect()
}

const G_ALERTS: u32 = 20;

pub struct NotificationsSection {
    rows: RowSet,
    pub store: NotifyStore,
    api: Box<dyn AlertsApi>,
    alerts: AlertsState,
    panel: Id,
    base: usize,
}

fn fixture_alerts() -> Vec<PriceAlert> {
    let now = super::data::types::now_ms();
    vec![
        PriceAlert { id: "a1".into(), coin_type: SUI.into(), symbol: "SUI".into(), dir: AlertDir::Above, target: 1.1, created_at: now - 3_600_000, fired_at: None, active: true },
        PriceAlert { id: "a2".into(), coin_type: SUI.into(), symbol: "SUI".into(), dir: AlertDir::Below, target: 0.9, created_at: now - 86_400_000, fired_at: Some(now - 7_200_000), active: false },
    ]
}

impl NotificationsSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, now: f64) -> NotificationsSection {
        let mut store = NotifyStore::load(Arc::new(MemStorage::new()));
        // The live notifier's prefs: the switches here are the ones it routes with.
        if let Some(p) = crate::social::prefs() {
            store.prefs = p;
        } else {
            store.prefs.dnd = crate::social::dnd();
        }
        let api: Box<dyn AlertsApi> = if super::live::live() { Box::new(super::live::LiveAlerts) } else { Box::new(MemAlerts::new(fixture_alerts())) };
        Self::build_with(cx, nav, parent, store, api, now)
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, store: NotifyStore, mut api: Box<dyn AlertsApi>, _now: f64) -> NotificationsSection {
        let r = cx.r();
        let col = cx.scene.add(parent, s().col().wp(1.0).gap(1.25 * r).done(), Node::default());
        let mut rows = RowSet::default();
        let p = panel(cx, col);
        let prefs = store.prefs.clone();
        rows.switch(cx, nav, p, "notif-dnd", "Do not disturb", "Only battery warnings pop up. Everything else waits in the list.", prefs.dnd, true);
        rows.switch(cx, nav, p, "notif-toasts", "Show toasts over a game", "Money, invites, messages and trophies pop up over a game. Off: they wait in the list until it ends.", prefs.game_toasts, false);
        rows.switch(cx, nav, p, "notif-desktop", "Desktop notifications", "When Rare is in the background, notices also show on the desktop.", prefs.desktop, false);
        for c in CLASSES {
            let list: Vec<_> = kinds().into_iter().filter(|k| class_of(&k.kind) == c).collect();
            if list.is_empty() {
                continue;
            }
            let p = panel(cx, col);
            setting_row(cx, p, c.label(), Some(class_note(c)), true);
            for k in list {
                rows.switch(cx, nav, p, &format!("notif-{}", k.kind), k.title, k.body, prefs.kind_on(&k.kind), false);
            }
        }
        let panel_ = panel(cx, col);
        let mut alerts = AlertsState::default();
        alerts.load(api.as_mut());
        let base = nav.len();
        let mut me = NotificationsSection { rows, store, api, alerts, panel: panel_, base };
        me.build_alerts(cx, nav);
        me
    }

    fn build_alerts(&mut self, cx: &mut Cx, nav: &mut Nav) {
        let r = cx.r();
        let body = alerts_body(self.alerts.list.as_deref(), self.alerts.error.as_deref());
        setting_row(cx, self.panel, "Your price alerts", Some(body), true);
        let list = self.alerts.list.clone().unwrap_or_default();
        for (i, a) in alert_rows(&list).into_iter().enumerate() {
            let row_ = cx.scene.add(self.panel, s().row().items_center().gap(1.25 * r).min_h(3.75 * r).wp(1.0).px(1.4 * r).py(0.7 * r).done(), Node::default());
            edge(cx, row_, 0, tok::HAIR);
            let c = cx.scene.add(row_, s().col().grow().done(), Node::default());
            text(cx, c, ty::BODY, 600, tok::FG, &format!("{} {} {}", a.symbol, a.dir.as_str(), usd_price(a.target)));
            let sub = cx.scene.add(c, s().row().items_center().gap(0.45 * r).done(), Node::default());
            dot(cx, sub, if a.active { tok::FG } else { tok::MUTED });
            text(cx, sub, ty::SMALL, 400, tok::MUTED, if a.active { "Watching" } else { "Hit" });
            text(cx, row_, ty::SMALL, 400, tok::MUTED, "Delete");
            nav.add(Focusable { id: row_, kind: Kind::Row, group: G_ALERTS, hint: "Delete alert".into(), tag: i as u32, sid: format!("alert:{}", a.id) });
        }
    }
}

impl SectionView for NotificationsSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, now: f64) -> SectionOut {
        let Some(cur) = nav.current().cloned() else { return SectionOut::None };
        if let Some(id) = cur.sid.strip_prefix("alert:") {
            let idx = cur.tag as usize;
            self.alerts.remove(self.api.as_mut(), id);
            cx.scene.clear_children(self.panel);
            nav.truncate(self.base);
            self.build_alerts(cx, nav);
            let n = nav.len();
            if n > self.base {
                let to = nav.items[(self.base + idx).min(n - 1)].id;
                nav.focus(cx.scene, to, now, false);
            } else if let Some(last) = nav.items.last().map(|f| f.id) {
                nav.focus(cx.scene, last, now, false);
            }
            return SectionOut::None;
        }
        if let Some((sid, Val::Bool(on))) = self.rows.press(cx, nav, now) {
            let mut p = self.store.prefs.clone();
            match sid.as_str() {
                "notif-dnd" => p.dnd = on,
                "notif-toasts" => p.game_toasts = on,
                "notif-desktop" => p.desktop = on,
                other => {
                    if let Some(k) = other.strip_prefix("notif-") {
                        p.kinds.insert(k.to_string(), on);
                    }
                }
            }
            self.store.set_prefs(p.clone());
            if sid == "notif-dnd" {
                crate::social::set_dnd(on);
            }
            crate::social::set_prefs(p);
        }
        SectionOut::None
    }
}

/// The section on a page of its own (until Settings builds it): the fixture "notify-settings".
pub struct NotificationsPage {
    nav: Nav,
    cfg: Config,
    view: NotificationsSection,
}

impl NotificationsPage {
    pub fn build(cx: &mut Cx, now: f64) -> NotificationsPage {
        let r = cx.r();
        let area = crate::screens::settings::fixture_area(cx);
        let pane = cx.scene.add(area, s().fill().col().pad(22.5 * r, 0.5 * r, 2.5 * r, 1.5 * r).done(), Node::default().scrolls());
        let mut nav = Nav::default();
        nav.scrollers.push(pane);
        let view = NotificationsSection::build(cx, &mut nav, pane, &Config::default(), now);
        let first = nav.items[0].id;
        nav.focus(cx.scene, first, now, false);
        NotificationsPage { nav, cfg: Config::default(), view }
    }
}

impl Screen for NotificationsPage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: rare_render::input::Pad, now: f64) -> Out {
        use rare_render::input::Pad;
        match pad {
            Pad::B => Out::Close,
            Pad::A => {
                self.view.press(cx, &mut self.nav, &mut self.cfg, now);
                Out::None
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            _ => Out::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: &str, active: bool) -> PriceAlert {
        PriceAlert { id: id.into(), active, ..Default::default() }
    }

    #[test]
    fn alert_rows_are_active_first_then_five_hit() {
        let list: Vec<_> = (0..8).map(|i| a(&format!("h{i}"), false)).chain([a("on1", true)]).collect();
        let ids: Vec<_> = alert_rows(&list).iter().map(|a| a.id.clone()).collect();
        assert_eq!(ids, vec!["on1", "h0", "h1", "h2", "h3", "h4"]);
    }

    #[test]
    fn alerts_row_body() {
        assert_eq!(alerts_body(Some(&[]), None), "No price alerts. Set one from a coin's page in Trade.");
        assert_eq!(alerts_body(None, Some("x")), "Could not load price alerts.");
        assert_eq!(alerts_body(Some(&[a("1", true)]), None), "One notice when the price crosses your target, then the alert stops.");
    }

    #[test]
    fn every_class_but_battery_has_switches() {
        for c in CLASSES {
            let n = kinds().into_iter().filter(|k| class_of(&k.kind) == c).count();
            assert_eq!(n == 0, c == NoticeClass::Battery);
        }
    }
}
