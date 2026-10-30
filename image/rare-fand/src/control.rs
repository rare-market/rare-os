//! The control loop's decisions, free of I/O: what the fan should do for each temperature
//! sample. Rises follow the curve at once; falls wait out a hysteresis band and a delay, then
//! step down slowly, so the fan never hunts around a curve point.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// At or above this edge temperature the fan runs flat out, whatever the preset.
pub const EMERGENCY_C: f64 = 90.0;
/// The emergency ends once the temperature stays below this for `RAMP_DOWN_DELAY`.
pub const EMERGENCY_CLEAR_C: f64 = 85.0;
/// A falling temperature must drop this far below a curve point before the duty falls.
pub const HYSTERESIS_C: f64 = 3.0;
/// How long the duty has to be allowed to fall before it starts falling.
pub const RAMP_DOWN_DELAY: Duration = Duration::from_secs(10);
/// Percent per tick (about a second) while the duty falls.
pub const RAMP_DOWN_STEP: u32 = 2;
/// Consecutive unreadable samples before the fan goes back to the EC.
pub const MAX_READ_FAILURES: u32 = 3;
/// Samples averaged for the curve (the emergency check uses the raw reading).
const AVERAGE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The embedded controller drives the fan (pwm1_enable = 2).
    Ec,
    /// Manual duty in percent (pwm1_enable = 1).
    Duty(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Default preset: the EC's own curve.
    Ec,
    /// Following the preset's curve.
    Curve,
    /// Too hot: 100 %.
    Emergency,
    /// No temperature or the fan could not be written: handed back to the EC.
    Failsafe,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Ec => "ec",
            Mode::Curve => "curve",
            Mode::Emergency => "emergency",
            Mode::Failsafe => "failsafe",
        }
    }
}

pub struct Controller {
    curve: Option<Vec<(u32, u32)>>,
    duty: Option<u32>,
    falling_since: Option<Instant>,
    samples: VecDeque<f64>,
    failures: u32,
    emergency: bool,
    cooling_since: Option<Instant>,
    last: (Target, Mode),
}

impl Controller {
    /// None runs the EC's curve (Default).
    pub fn new(curve: Option<Vec<(u32, u32)>>) -> Self {
        Controller {
            curve,
            duty: None,
            falling_since: None,
            samples: VecDeque::new(),
            failures: 0,
            emergency: false,
            cooling_since: None,
            last: (Target::Ec, Mode::Ec),
        }
    }

    /// A new preset applies straight away (no ramp from the old one).
    pub fn set_curve(&mut self, curve: Option<Vec<(u32, u32)>>) {
        self.curve = curve;
        self.duty = None;
        self.falling_since = None;
    }

    /// One sample, about once a second. `temp` is None when no sensor could be read.
    pub fn step(&mut self, temp: Option<f64>, now: Instant) -> (Target, Mode) {
        let out = self.decide(temp, now);
        self.last = out;
        out
    }

    fn decide(&mut self, temp: Option<f64>, now: Instant) -> (Target, Mode) {
        let Some(t) = temp.filter(|t| t.is_finite()) else {
            self.failures += 1;
            if self.failures >= MAX_READ_FAILURES {
                self.reset();
                return (Target::Ec, Mode::Failsafe);
            }
            // A single bad read: hold what we had.
            return self.last;
        };
        self.failures = 0;

        if t >= EMERGENCY_C {
            self.emergency = true;
            self.cooling_since = None;
        } else if self.emergency {
            if t < EMERGENCY_CLEAR_C {
                let since = *self.cooling_since.get_or_insert(now);
                if now.duration_since(since) >= RAMP_DOWN_DELAY {
                    self.emergency = false;
                    self.cooling_since = None;
                }
            } else {
                self.cooling_since = None;
            }
        }
        if self.emergency {
            // A curve ramps down from full speed once the emergency ends.
            self.duty = Some(100);
            self.falling_since = None;
            self.samples.clear();
            return (Target::Duty(100), Mode::Emergency);
        }

        self.samples.push_back(t);
        while self.samples.len() > AVERAGE {
            self.samples.pop_front();
        }
        let avg = self.samples.iter().sum::<f64>() / self.samples.len() as f64;

        let Some(curve) = self.curve.as_deref() else {
            self.duty = None;
            return (Target::Ec, Mode::Ec);
        };
        let up = crate::curve::interpolate(curve, avg);
        let duty = match self.duty {
            None => up,
            Some(cur) if up >= cur => {
                self.falling_since = None;
                up
            }
            Some(cur) => {
                // Falling: only once the curve a few degrees warmer is below us too.
                let down = crate::curve::interpolate(curve, avg + HYSTERESIS_C);
                if down >= cur {
                    self.falling_since = None;
                    cur
                } else {
                    let since = *self.falling_since.get_or_insert(now);
                    if now.duration_since(since) >= RAMP_DOWN_DELAY {
                        down.max(cur.saturating_sub(RAMP_DOWN_STEP))
                    } else {
                        cur
                    }
                }
            }
        };
        self.duty = Some(duty);
        (Target::Duty(duty), Mode::Curve)
    }

    fn reset(&mut self) {
        self.duty = None;
        self.falling_since = None;
        self.samples.clear();
        self.emergency = false;
        self.cooling_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::{COOL, QUIET};

    struct Clock(Instant);
    impl Clock {
        fn tick(&mut self) -> Instant {
            self.0 += Duration::from_secs(1);
            self.0
        }
    }

    fn run(c: &mut Controller, clock: &mut Clock, temp: f64, secs: u32) -> (Target, Mode) {
        let mut out = (Target::Ec, Mode::Ec);
        for _ in 0..secs {
            out = c.step(Some(temp), clock.tick());
        }
        out
    }

    #[test]
    fn default_leaves_the_fan_to_the_ec() {
        let mut c = Controller::new(None);
        let mut k = Clock(Instant::now());
        assert_eq!(run(&mut c, &mut k, 60.0, 5), (Target::Ec, Mode::Ec));
    }

    #[test]
    fn rises_at_once_and_holds_inside_the_band() {
        let mut c = Controller::new(Some(COOL.to_vec()));
        let mut k = Clock(Instant::now());
        assert_eq!(run(&mut c, &mut k, 50.0, 3), (Target::Duty(46), Mode::Curve));
        // Up follows the (averaged) temperature straight away.
        assert_eq!(run(&mut c, &mut k, 60.0, 3), (Target::Duty(66), Mode::Curve));
        // Wobbling 1-2 C under the point never lowers the fan.
        for t in [59.0, 58.0, 59.5, 58.5, 59.0, 58.0, 59.0, 58.2, 59.0, 58.0, 58.5, 59.0, 58.0, 59.0, 58.0] {
            assert_eq!(c.step(Some(t), k.tick()), (Target::Duty(66), Mode::Curve));
        }
    }

    #[test]
    fn falls_after_the_delay_in_small_steps() {
        let mut c = Controller::new(Some(COOL.to_vec()));
        let mut k = Clock(Instant::now());
        run(&mut c, &mut k, 70.0, 3);
        assert_eq!(c.step(Some(70.0), k.tick()).0, Target::Duty(86));
        // Drop to 50 C: the duty holds for the ramp-down delay...
        for _ in 0..RAMP_DOWN_DELAY.as_secs() {
            assert_eq!(c.step(Some(50.0), k.tick()).0, Target::Duty(86));
        }
        // ...then falls 2 % a second...
        assert_eq!(c.step(Some(50.0), k.tick()).0, Target::Duty(84));
        assert_eq!(c.step(Some(50.0), k.tick()).0, Target::Duty(82));
        // ...down to the curve at 50 + 3 C (52 %), not below.
        let out = run(&mut c, &mut k, 50.0, 60);
        assert_eq!(out.0, Target::Duty(52));
        // A warm-up in the middle of a fall stops it and rises with the averaged temperature.
        let mut c = Controller::new(Some(QUIET.to_vec()));
        run(&mut c, &mut k, 80.0, 3);
        run(&mut c, &mut k, 60.0, 12);
        let (Target::Duty(mid), _) = c.step(Some(60.0), k.tick()) else { panic!() };
        assert!(mid < 80 && mid > 30, "{mid}");
        // Quiet at 85 C: halfway between 80 % (80 C) and 100 % (90 C).
        assert_eq!(run(&mut c, &mut k, 85.0, 3).0, Target::Duty(90));
    }

    #[test]
    fn emergency_overrides_every_preset() {
        let quiet = vec![(40, 10), (95, 20)];
        for curve in [None, Some(quiet), Some(QUIET.to_vec())] {
            let mut c = Controller::new(curve.clone());
            let mut k = Clock(Instant::now());
            run(&mut c, &mut k, 60.0, 3);
            assert_eq!(c.step(Some(90.0), k.tick()), (Target::Duty(100), Mode::Emergency));
            // Still hot (above the clear point): stays at 100 however long.
            assert_eq!(run(&mut c, &mut k, 87.0, 30), (Target::Duty(100), Mode::Emergency));
            // Below 85 C it waits out the delay before letting go.
            assert_eq!(run(&mut c, &mut k, 80.0, RAMP_DOWN_DELAY.as_secs() as u32), (Target::Duty(100), Mode::Emergency));
            let out = c.step(Some(80.0), k.tick());
            match curve {
                None => assert_eq!(out, (Target::Ec, Mode::Ec)),
                // A curve ramps down from 100 rather than dropping.
                Some(_) => assert_eq!(out, (Target::Duty(100), Mode::Curve)),
            }
        }
    }

    #[test]
    fn unreadable_sensors_fail_safe() {
        let mut c = Controller::new(Some(COOL.to_vec()));
        let mut k = Clock(Instant::now());
        run(&mut c, &mut k, 60.0, 3);
        // One or two bad reads hold the last duty.
        assert_eq!(c.step(None, k.tick()), (Target::Duty(66), Mode::Curve));
        assert_eq!(c.step(Some(f64::NAN), k.tick()), (Target::Duty(66), Mode::Curve));
        // The third hands the fan back to the EC, and it stays there while reads fail.
        assert_eq!(c.step(None, k.tick()), (Target::Ec, Mode::Failsafe));
        assert_eq!(c.step(None, k.tick()), (Target::Ec, Mode::Failsafe));
        // A good read picks the curve up again, from scratch.
        assert_eq!(c.step(Some(50.0), k.tick()), (Target::Duty(46), Mode::Curve));
        // Bad reads during an emergency also end up with the EC, never stuck at a duty.
        assert_eq!(c.step(Some(95.0), k.tick()).1, Mode::Emergency);
        for _ in 0..MAX_READ_FAILURES {
            c.step(None, k.tick());
        }
        assert_eq!(c.step(None, k.tick()), (Target::Ec, Mode::Failsafe));
    }

    #[test]
    fn new_preset_applies_at_once() {
        let mut c = Controller::new(Some(COOL.to_vec()));
        let mut k = Clock(Instant::now());
        assert_eq!(run(&mut c, &mut k, 60.0, 3).0, Target::Duty(66));
        c.set_curve(Some(QUIET.to_vec()));
        assert_eq!(c.step(Some(60.0), k.tick()).0, Target::Duty(26));
        c.set_curve(None);
        assert_eq!(c.step(Some(60.0), k.tick()), (Target::Ec, Mode::Ec));
    }
}
