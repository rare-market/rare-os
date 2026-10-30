//! The notification system (launcher/src/notify and components/DockWatch.tsx) as native screens:
//! the notification centre, toasts, the price alert sheet, the watchers (battery, OS update, dock)
//! and the Settings > Notifications section. `data` holds the ported state and rules with their
//! tests; `backend` the traits rare-notify implements later.
//!
//! Fixtures: notify-centre, notify-centre-empty, notify-centre-unread, notify-toast,
//! notify-toast-invite, notify-alert-sheet, notify-battery-low, notify-battery-critical,
//! notify-osupdate, notify-dock-docked, notify-dock-undocked, notify-settings (the Notifications
//! section on a page of its own; Settings shows the same thing once it builds the section).

pub mod alert;
pub mod backend;
pub mod centre;
pub mod data;
pub mod fixtures;
pub mod live;
pub mod section;
pub mod toast;
pub mod ui;
pub mod watchers;

use crate::screens::overlay::common::{Cx, Screen};
use crate::screens::settings::display::DisplayState;
use crate::screens::settings::update::OsUpdate;
use data::storage::MemStorage;
use data::store::NotifyStore;
use data::types::{now_ms, Notice};
use fixtures::FixtureArt;
use std::sync::Arc;
use toast::{AppToast, NoticeToasts};

fn store_with(list: &[Notice], read: &[&Notice]) -> NotifyStore {
    let mut st = NotifyStore::load(Arc::new(MemStorage::new()));
    st.add(list);
    for n in read {
        st.mark_read(&n.id);
    }
    st
}

fn toasts(cx: &mut Cx, list: Vec<(Notice, bool)>, app: Vec<AppToast>, now: f64) -> Box<dyn Screen> {
    let mut st = NotifyStore::load(Arc::new(MemStorage::new()));
    let t = now_ms();
    for (n, small) in &list {
        st.show_toast(n, false, *small, t);
    }
    Box::new(NoticeToasts::build(cx, st, app, Box::new(FixtureArt), now))
}

/// Fixture screens by name: "notify-<state>".
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let t = now_ms();
    Some(match name {
        "notify-centre" | "notify-centre-unread" | "notify-centre-empty" => {
            let history = fixtures::history(t);
            let live = fixtures::live(t);
            let all: Vec<Notice> = history.iter().chain(live.iter()).cloned().collect();
            let (store, downloads) = match name {
                "notify-centre-empty" => (store_with(&[], &[]), vec![]),
                "notify-centre-unread" => (store_with(&all, &[]), vec![]),
                _ => (store_with(&all, &history.iter().chain(live.iter().take(2)).collect::<Vec<_>>()), fixtures::downloads()),
            };
            Box::new(centre::CentreSheet::build(cx, store, downloads, Box::new(FixtureArt), now))
        }
        "notify-toast" => toasts(cx, vec![(fixtures::sold(t, 0), false)], vec![], now),
        "notify-toast-invite" => toasts(cx, vec![(fixtures::friend_request(t, 0), false), (fixtures::party_invite(t), false)], vec![], now),
        "notify-alert-sheet" => Box::new(alert::AlertSheet::build(cx, "SUI", fixtures::SUI, 1.01, None, Box::new(backend::MemAlerts::new(vec![])), now)),
        "notify-battery-low" => toasts(cx, vec![(fixtures::battery(15, t), false)], vec![], now),
        "notify-battery-critical" => toasts(cx, vec![(fixtures::battery(5, t), false)], vec![], now),
        "notify-osupdate" => {
            // The watcher decides there is something to say; the notice is what the toast shows.
            let mut w = watchers::OsUpdateNotice::default();
            let u = OsUpdate { staged: true, staged_version: Some("43.2".into()), ..Default::default() };
            w.on_update(&u)?;
            toasts(cx, vec![(fixtures::os_update(t), false)], vec![], now)
        }
        "notify-dock-docked" | "notify-dock-undocked" => {
            let tv = DisplayState { docked: true, dock_name: Some("LG TV".into()), dock_mode: Some("3840x2160".into()), ..Default::default() };
            let own = DisplayState::default();
            let mut w = watchers::DockWatch::default();
            let (first, second) = if name == "notify-dock-docked" { (own, tv) } else { (tv, own) };
            w.on_state(first);
            let toast = w.on_state(second)?;
            toasts(cx, vec![], vec![toast], now)
        }
        "notify-settings" | "settings-notifications" => Box::new(section::NotificationsPage::build(cx, now)),
        _ => return None,
    })
}
