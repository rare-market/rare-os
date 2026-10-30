//! Two small state machines for the power profile (the I/O around them is in system.rs):
//!
//! - The firmware-reset watcher. The SMU firmware puts its own limits back on some resumes,
//!   charger changes and mid-session (seen on other Phoenix handhelds too).
//!   Its signature on the SuiPlay0X1 is
//!   STAPM 15 W, fast PPT 20 W, slow PPT 30 W. That pattern gets the whole profile back. A
//!   slow-PPT gap above STAPM (PowerStation's `Boost`) that the profile didn't ask for gets
//!   `Boost = 0` with the TDP left as it is: Steam's slider (steamos-manager) only sets `TDP`,
//!   and PowerStation's TDP setter keeps the old gap, so Steam's "10 W" would run at 25/31 W.
//!   Any other value (Steam's or the user's wattage with no gap) is left alone.
//! - The game-load CPU boost window. Balanced keeps CPU boost off, which costs about a third of
//!   single-core speed exactly while a game decompresses and parses its data. The launcher turns
//!   boost on at launch and back to the profile's setting 10 s after the game's first frame
//!   (gamescope's focused app becomes the game), never later than 60 s after launch.

use std::time::{Duration, Instant};

/// ryzen_smu's copy of the SMU's power table. World-readable (0444): no root, no D-Bus.
pub const PM_TABLE: &str = "/sys/kernel/ryzen_smu_drv/pm_table";

/// The live package power limits, watts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// STAPM limit (PowerStation's TDP).
    pub stapm: f64,
    /// Fast PPT; unknown when read through PowerStation, which doesn't expose it.
    pub fast: Option<f64>,
    /// Slow PPT (STAPM + PowerStation's Boost).
    pub slow: f64,
}

impl Limits {
    /// PowerStation's Boost: slow PPT above STAPM.
    pub fn gap(&self) -> f64 {
        self.slow - self.stapm
    }
}

/// The first 24 bytes of the PM table (Phoenix, table 0x4C0008, RyzenAdj's offsets): STAPM
/// limit, STAPM value, fast limit, fast value, slow limit, slow value, as little-endian f32.
pub fn parse_pm_table(b: &[u8]) -> Option<Limits> {
    let f = |i: usize| b.get(i * 4..i * 4 + 4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64);
    let l = Limits { stapm: f(0)?, fast: Some(f(2)?), slow: f(4)? };
    let sane = |v: f64| v.is_finite() && v > 0.5 && v < 200.0;
    (sane(l.stapm) && sane(l.slow) && l.fast.is_some_and(sane)).then_some(l)
}

/// The same limits from PowerStation's `TDP` and `Boost` (when the PM table can't be read).
pub fn from_powerstation(tdp: f64, boost: f64) -> Limits {
    Limits { stapm: tdp, fast: None, slow: tdp + boost }
}

/// Values within this many watts count as equal (the table reads 15.000001).
const EPS: f64 = 0.3;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

/// The firmware's own limits on the SuiPlay0X1 (measured on AC): STAPM 15, fast 20, slow 30.
/// Through PowerStation (no fast PPT) that reads TDP 15, Boost 15.
pub fn is_firmware(l: &Limits) -> bool {
    near(l.stapm, 15.0) && near(l.slow, 30.0) && l.fast.is_none_or(|f| near(f, 20.0))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fix {
    None,
    /// The firmware put its limits back: apply the whole profile again.
    Reapply,
    /// A Boost gap nobody here asked for (Steam's slider after a reset, or an old gap): set
    /// Boost to 0 and keep the TDP.
    ZeroBoost,
}

/// What the live limits call for. `gap_ok`: the wish wants a slow-PPT gap (no Rare profile does).
pub fn decide(l: &Limits, gap_ok: bool) -> Fix {
    if is_firmware(l) {
        Fix::Reapply
    } else if !gap_ok && l.gap() > EPS {
        Fix::ZeroBoost
    } else {
        Fix::None
    }
}

/// How long the same fix waits before it is tried again while the limits keep showing it (it
/// didn't take, or something puts the values straight back): never a tight fight.
pub const RETRY: Duration = Duration::from_secs(60);

/// The watcher: `decide` plus a back-off, so a fix that doesn't stick is retried once a minute.
#[derive(Default)]
pub struct Watcher {
    last: Option<(Fix, Instant)>,
}

impl Watcher {
    pub fn tick(&mut self, now: Instant, limits: Option<Limits>, gap_ok: bool) -> Fix {
        let Some(l) = limits else { return Fix::None };
        let fix = decide(&l, gap_ok);
        if fix == Fix::None {
            self.last = None;
            return fix;
        }
        if let Some((f, t)) = self.last {
            if f == fix && now.saturating_duration_since(t) < RETRY {
                return Fix::None;
            }
        }
        self.last = Some((fix, now));
        fix
    }
}

/// How long boost stays on after the game's first frame.
pub const AFTER_FIRST_FRAME: Duration = Duration::from_secs(10);
/// The longest a load window lasts, from launch.
pub const WINDOW_CAP: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Hold,
    End,
}

/// One game launch's boost window.
#[derive(Clone, Debug)]
pub struct BoostWindow {
    started: Instant,
    /// The app id gamescope will report for the game, when known (Steam games and shortcuts).
    target: Option<u64>,
    /// What gamescope had in focus at launch (the launcher, usually).
    initial: Option<u64>,
    first_frame: Option<Instant>,
}

impl BoostWindow {
    pub fn new(now: Instant, target: Option<u64>, initial: Option<u64>) -> Self {
        BoostWindow { started: now, target, initial, first_frame: None }
    }

    /// Whether `focused` is the game: its known app id, or (unknown id) anything other than
    /// what was in front at launch.
    fn is_game(&self, focused: Option<u64>) -> bool {
        let Some(f) = focused else { return false };
        match self.target {
            Some(t) if Some(t) != self.initial => f == t,
            _ => Some(f) != self.initial,
        }
    }

    /// Called every half second: the focused app id and whether the game still runs.
    pub fn step(&mut self, now: Instant, focused: Option<u64>, running: bool) -> Step {
        if !running {
            return Step::End;
        }
        if self.first_frame.is_none() && self.is_game(focused) {
            self.first_frame = Some(now);
        }
        let cap = self.started + WINDOW_CAP;
        let end = self.first_frame.map_or(cap, |t| (t + AFTER_FIRST_FRAME).min(cap));
        if now >= end { Step::End } else { Step::Hold }
    }

    pub fn first_frame(&self) -> Option<Duration> {
        self.first_frame.map(|t| t.saturating_duration_since(self.started))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(v: [f32; 6]) -> Vec<u8> {
        let mut b: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
        b.extend([0u8; 16]);
        b
    }

    #[test]
    fn reads_the_pm_table() {
        // As read on the SuiPlay (Balanced set by hand): `od -An -tf4 -N24 pm_table`.
        let l = parse_pm_table(&table([15.000001, 8.387577, 18.75, 5.8679457, 15.000001, 5.9037504])).unwrap();
        assert!(near(l.stapm, 15.0) && near(l.fast.unwrap(), 18.75) && near(l.slow, 15.0));
        assert!(l.gap().abs() < EPS);
        assert_eq!(parse_pm_table(&[0u8; 12]), None);
        assert_eq!(parse_pm_table(&table([0.0; 6])), None);
        assert_eq!(parse_pm_table(&table([f32::NAN, 0.0, 20.0, 0.0, 30.0, 0.0])), None);
    }

    #[test]
    fn firmware_limits_get_the_profile_back() {
        let fw = Limits { stapm: 15.000001, fast: Some(20.0), slow: 30.0 };
        assert_eq!(decide(&fw, false), Fix::Reapply);
        // Even a profile that allows a gap: this is the firmware, not a choice.
        assert_eq!(decide(&fw, true), Fix::Reapply);
        // Through PowerStation: TDP 15, Boost 15.
        assert_eq!(decide(&from_powerstation(15.0, 15.0), false), Fix::Reapply);
    }

    #[test]
    fn rare_profiles_and_steam_values_are_left_alone() {
        for w in [10.0, 15.0, 25.0, 5.0, 28.0] {
            let l = Limits { stapm: w, fast: Some(w * 1.25), slow: w };
            assert_eq!(decide(&l, false), Fix::None, "{w} W, no gap");
        }
        // Steam's slider at 12 W after the launcher zeroed Boost: left at 12.
        assert_eq!(decide(&from_powerstation(12.0, 0.0), false), Fix::None);
    }

    #[test]
    fn steam_slider_with_the_firmware_gap_gets_boost_zeroed() {
        // Steam sets TDP 10 through steamos-manager after a reset: STAPM 10, slow 25, fast 31.25.
        let l = Limits { stapm: 10.0, fast: Some(31.25), slow: 25.0 };
        assert_eq!(decide(&l, false), Fix::ZeroBoost);
        assert_eq!(decide(&from_powerstation(10.0, 15.0), false), Fix::ZeroBoost);
        // Steam at 15 W with the gap still there is not the firmware pattern (fast is 37.5).
        assert_eq!(decide(&Limits { stapm: 15.0, fast: Some(37.5), slow: 30.0 }, false), Fix::ZeroBoost);
        // A profile that asks for a gap keeps it.
        assert_eq!(decide(&l, true), Fix::None);
    }

    #[test]
    fn watcher_backs_off_and_rearms() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let fw = Some(Limits { stapm: 15.0, fast: Some(20.0), slow: 30.0 });
        let ok = Some(Limits { stapm: 15.0, fast: Some(18.75), slow: 15.0 });
        let mut w = Watcher::default();
        assert_eq!(w.tick(s(0), None, false), Fix::None);
        assert_eq!(w.tick(s(0), fw, false), Fix::Reapply);
        // Didn't take yet: no retry for a minute.
        assert_eq!(w.tick(s(10), fw, false), Fix::None);
        assert_eq!(w.tick(s(50), fw, false), Fix::None);
        assert_eq!(w.tick(s(61), fw, false), Fix::Reapply);
        // It took; a later reset is handled at once.
        assert_eq!(w.tick(s(71), ok, false), Fix::None);
        assert_eq!(w.tick(s(81), fw, false), Fix::Reapply);
        // A different fix isn't held back by the last one.
        let gap = Some(Limits { stapm: 10.0, fast: Some(31.25), slow: 25.0 });
        assert_eq!(w.tick(s(91), gap, false), Fix::ZeroBoost);
        assert_eq!(w.tick(s(101), gap, false), Fix::None);
    }

    #[test]
    fn boost_window_ends_ten_seconds_after_the_first_frame() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let launcher = Some(769);
        let mut w = BoostWindow::new(t0, Some(1145360), launcher);
        assert_eq!(w.step(s(1), launcher, true), Step::Hold);
        // Steam's own overlay or a launcher window of another app is not the game.
        assert_eq!(w.step(s(5), Some(7), true), Step::Hold);
        assert_eq!(w.step(s(12), Some(1145360), true), Step::Hold);
        assert_eq!(w.first_frame(), Some(Duration::from_secs(12)));
        assert_eq!(w.step(s(21), launcher, true), Step::Hold);
        assert_eq!(w.step(s(22), Some(1145360), true), Step::End);
    }

    #[test]
    fn boost_window_is_capped_at_sixty_seconds() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        // No first frame at all.
        let mut w = BoostWindow::new(t0, Some(10), Some(769));
        assert_eq!(w.step(s(59), Some(769), true), Step::Hold);
        assert_eq!(w.step(s(60), Some(769), true), Step::End);
        // A first frame at 55 s still ends at 60, not 65.
        let mut w = BoostWindow::new(t0, Some(10), Some(769));
        assert_eq!(w.step(s(55), Some(10), true), Step::Hold);
        assert_eq!(w.step(s(60), Some(10), true), Step::End);
    }

    #[test]
    fn boost_window_ends_when_the_game_does() {
        let t0 = Instant::now();
        let mut w = BoostWindow::new(t0, None, Some(769));
        assert_eq!(w.step(t0 + Duration::from_secs(3), Some(769), false), Step::End);
    }

    #[test]
    fn boost_window_without_a_known_app_id_waits_for_focus_to_change() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut w = BoostWindow::new(t0, None, Some(769));
        assert_eq!(w.step(s(1), Some(769), true), Step::Hold);
        assert_eq!(w.step(s(2), None, true), Step::Hold);
        assert_eq!(w.step(s(8), Some(4242), true), Step::Hold);
        assert_eq!(w.step(s(18), Some(4242), true), Step::End);
        // The game's id was already in front at launch (a relaunch): falls back to "focus changed",
        // then the cap.
        let mut w = BoostWindow::new(t0, Some(10), Some(10));
        assert_eq!(w.step(s(5), Some(10), true), Step::Hold);
        assert_eq!(w.step(s(60), Some(10), true), Step::End);
        // Outside gamescope there is no focus at all: the cap.
        let mut w = BoostWindow::new(t0, Some(10), None);
        assert_eq!(w.step(s(30), None, true), Step::Hold);
        assert_eq!(w.step(s(60), None, true), Step::End);
    }
}
