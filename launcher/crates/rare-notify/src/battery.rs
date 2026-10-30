//! Low-battery warnings: which one to show for a reading, and the watcher that turns readings into
//! Battery notices (they show over Do not disturb and over a game).

use crate::notifier::LocalNotice;
use crate::types::{NoticeAction, NoticeKind};

/// The battery levels that warn, highest first.
pub const LOW_BATTERY: [u8; 2] = [15, 5];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatteryReading {
    pub percent: f64,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LowBattery {
    /// The level to warn about now.
    pub warn: Option<u8>,
    /// The lowest level warned about since the last charge, to pass back next time.
    pub warned: Option<u8>,
}

/// Which low-battery warning to show now. `warned` is the lowest level already warned about since
/// the last charge: a warning shows once per level when the battery is discharging, and charging
/// (or climbing back above the level) re-arms it. Dropping straight to 4 % shows the 5 % warning
/// only.
pub fn low_battery(warned: Option<u8>, b: Option<BatteryReading>) -> LowBattery {
    let Some(b) = b else { return LowBattery { warn: None, warned } };
    if b.charging {
        return LowBattery { warn: None, warned: None };
    }
    let hit = LOW_BATTERY.iter().rev().copied().find(|l| b.percent <= *l as f64);
    let Some(hit) = hit else {
        // Back to 20 % without charging (a reading that jumped) re-arms; 16 % does not, so 15 %
        // does not repeat.
        return LowBattery { warn: None, warned: if b.percent >= 20.0 { None } else { warned } };
    };
    if warned.is_some_and(|w| w <= hit) {
        return LowBattery { warn: None, warned };
    }
    LowBattery { warn: Some(hit), warned: Some(hit) }
}

/// The sentence for a warning.
pub fn low_battery_text(level: u8) -> &'static str {
    if level <= 5 {
        "Battery at 5%. Plug in soon."
    } else {
        "Battery at 15%."
    }
}

/// Keeps the warned level between readings (the web launcher's BatteryNotice).
#[derive(Clone, Debug, Default)]
pub struct BatteryWatch {
    warned: Option<u8>,
}

impl BatteryWatch {
    /// A new reading (none without a battery); the notice to post, if any.
    pub fn reading(&mut self, b: Option<BatteryReading>) -> Option<LocalNotice> {
        let r = low_battery(self.warned, b);
        self.warned = r.warned;
        r.warn.map(|level| LocalNotice::new(NoticeKind::Battery, low_battery_text(level), NoticeAction::Battery))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dis(percent: f64) -> Option<BatteryReading> {
        Some(BatteryReading { percent, charging: false })
    }

    #[test]
    fn warns_once_at_15_and_once_at_5() {
        let mut w = None;
        let got: Vec<Option<u8>> = [30.0, 16.0, 15.0, 14.0, 13.0, 6.0, 5.0, 4.0, 3.0]
            .iter()
            .map(|p| {
                let r = low_battery(w, dis(*p));
                w = r.warned;
                r.warn
            })
            .collect();
        assert_eq!(got, [None, None, Some(15), None, None, None, Some(5), None, None]);
    }

    #[test]
    fn jumping_below_5_warns_for_5_only_and_charging_rearms() {
        let a = low_battery(None, dis(4.0));
        assert_eq!(a, LowBattery { warn: Some(5), warned: Some(5) });
        assert_eq!(low_battery(a.warned, Some(BatteryReading { percent: 10.0, charging: true })), LowBattery { warn: None, warned: None });
        assert_eq!(low_battery(None, dis(14.0)).warn, Some(15));
    }

    #[test]
    fn does_not_repeat_at_15_when_the_reading_wobbles_and_says_nothing_without_a_battery() {
        assert_eq!(low_battery(Some(15), dis(16.0)), LowBattery { warn: None, warned: Some(15) });
        assert_eq!(low_battery(Some(15), dis(15.0)).warn, None);
        assert_eq!(low_battery(Some(15), dis(20.0)), LowBattery { warn: None, warned: None });
        assert_eq!(low_battery(Some(15), None), LowBattery { warn: None, warned: Some(15) });
    }

    #[test]
    fn the_watch_posts_battery_notices() {
        let mut w = BatteryWatch::default();
        assert!(w.reading(dis(40.0)).is_none());
        let n = w.reading(dis(15.0)).unwrap();
        assert_eq!((n.kind, n.title.as_str(), n.action), (NoticeKind::Battery, "Battery at 15%.", NoticeAction::Battery));
        assert!(w.reading(dis(14.0)).is_none());
        assert_eq!(w.reading(dis(5.0)).unwrap().title, "Battery at 5%. Plug in soon.");
    }
}
