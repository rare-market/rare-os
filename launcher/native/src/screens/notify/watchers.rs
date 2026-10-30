//! The launcher-level watchers that render nothing: BatteryNotice, OsUpdateNotice and DockWatch.
//! Each turns a reading from its source into a notice (or a plain toast) for the shell to post
//! with `Notifier::notify_local` / the app toast list.

use super::backend::BatterySource;
use super::data::battery::BatteryWatch;
use super::data::downloads::os_update_notice_with;
use super::data::LocalNotice;
use super::toast::AppToast;
use crate::screens::settings::display::{dock_change, DisplayState};
use crate::screens::settings::update::{to_announce, OsUpdate};

/// How often OsUpdateNotice reads the update state while the launcher is shown.
pub const OS_POLL_SECS: f64 = 120.0;
/// How often DockWatch reads the display state, and how long its toast stays up.
pub const DOCK_POLL_SECS: f64 = 3.0;
pub const DOCK_TOAST_MS: u64 = 5000;

/// BatteryNotice.tsx: a toast at 15 % and at 5 % while discharging.
#[derive(Default)]
pub struct BatteryNotice {
    watch: BatteryWatch,
}

impl BatteryNotice {
    pub fn poll(&mut self, src: &mut dyn BatterySource) -> Option<LocalNotice> {
        self.watch.reading(src.reading())
    }
}

/// OsUpdateNotice.tsx: once per staged version per launch, "Rare OS x is ready".
#[derive(Default)]
pub struct OsUpdateNotice {
    announced: Option<String>,
}

impl OsUpdateNotice {
    pub fn on_update(&mut self, u: &OsUpdate) -> Option<LocalNotice> {
        let key = to_announce(u, self.announced.as_deref())?;
        self.announced = Some(key);
        Some(os_update_notice_with(u.staged_version.as_deref(), u.notes.as_deref()))
    }
}

/// DockWatch.tsx: a toast when a TV or monitor is plugged in or out or changes mode.
#[derive(Default)]
pub struct DockWatch {
    prev: Option<DisplayState>,
    seq: u64,
}

impl DockWatch {
    pub fn on_state(&mut self, d: DisplayState) -> Option<AppToast> {
        let msg = dock_change(self.prev.as_ref(), &d);
        self.prev = Some(d);
        msg.map(|text| {
            self.seq += 1;
            AppToast { id: self.seq, text, error: false }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::notify::data::battery::BatteryReading;
    use crate::screens::notify::data::types::NoticeKind;

    struct Seq(Vec<Option<BatteryReading>>);
    impl BatterySource for Seq {
        fn reading(&mut self) -> Option<BatteryReading> {
            self.0.remove(0)
        }
    }

    fn b(p: f64, c: bool) -> Option<BatteryReading> {
        Some(BatteryReading { percent: p, charging: c })
    }

    #[test]
    fn battery_warns_once_per_level() {
        let mut w = BatteryNotice::default();
        let mut src = Seq(vec![b(40.0, false), b(15.0, false), b(14.0, false), b(5.0, false), b(4.0, false), b(30.0, true), b(15.0, false)]);
        assert!(w.poll(&mut src).is_none());
        let n = w.poll(&mut src).unwrap();
        assert_eq!(n.kind, NoticeKind::Battery);
        assert_eq!(n.title, "Battery at 15%.");
        assert!(w.poll(&mut src).is_none());
        assert_eq!(w.poll(&mut src).unwrap().title, "Battery at 5%. Plug in soon.");
        assert!(w.poll(&mut src).is_none());
        assert!(w.poll(&mut src).is_none());
        assert!(w.poll(&mut src).is_some());
    }

    #[test]
    fn os_update_is_announced_once_per_version() {
        let mut w = OsUpdateNotice::default();
        let none = OsUpdate::default();
        assert!(w.on_update(&none).is_none());
        let staged = OsUpdate { staged: true, staged_version: Some("43.2".into()), ..Default::default() };
        let n = w.on_update(&staged).unwrap();
        assert_eq!(n.title, "Rare OS 43.2 is ready");
        assert_eq!(n.body.as_deref(), Some("Restart to update."));
        assert!(w.on_update(&staged).is_none());
        let newer = OsUpdate { staged: true, staged_version: Some("43.3".into()), ..Default::default() };
        assert!(w.on_update(&newer).is_some());
    }

    #[test]
    fn dock_toasts_on_change_only() {
        let mut w = DockWatch::default();
        let tv = DisplayState { docked: true, dock_name: Some("LG TV".into()), dock_mode: Some("3840x2160".into()), ..Default::default() };
        let own = DisplayState::default();
        assert!(w.on_state(tv.clone()).is_none());
        assert_eq!(w.on_state(own.clone()).unwrap().text, "Back on the built-in screen");
        assert_eq!(w.on_state(tv).unwrap().text, "Docked to LG TV 3840x2160");
        assert!(w.on_state(own.clone()).is_some());
        assert!(w.on_state(own).is_none());
    }
}
