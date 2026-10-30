//! Time-based motion evaluated at frame time. A property is an `Anim`: setting a new target starts
//! from wherever the value is now, so interrupted motion never jumps. No timers, no callbacks: the
//! frame loop asks the scene whether anything is still moving and stops drawing when nothing is.
//!
//! Every curve has a closed form in time, so a frame evaluates `get(now)` with no per-frame state:
//! - `Curve::Tween`: a CSS cubic-bezier over a fixed duration (app.css transitions and keyframes);
//! - `Curve::Glide`: exponential approach with a time constant (nav/focus.ts's scroll glide: ~90 %
//!   of the way in 150 ms at tau 65 ms), retargets mid-flight without a stutter;
//! - `Curve::Spring`: a damped spring that keeps its velocity across retargets;
//! - `Curve::Loop`: keyframes that repeat forever (the loading bar, the breathing live dot).

use crate::color::Color;

/// CSS cubic-bezier easing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ease(pub f32, pub f32, pub f32, pub f32);

impl Ease {
    /// The one easing curve: cubic-bezier(0.2, 0.8, 0.2, 1).
    pub const PRO: Ease = Ease(0.2, 0.8, 0.2, 1.0);
    /// CSS `ease`.
    pub const EASE: Ease = Ease(0.25, 0.1, 0.25, 1.0);
    /// CSS `ease-in-out`.
    pub const EASE_IN_OUT: Ease = Ease(0.42, 0.0, 0.58, 1.0);
    /// Tailwind's default `cubic-bezier(0.4, 0, 0.2, 1)` (the loading bar).
    pub const STANDARD: Ease = Ease(0.4, 0.0, 0.2, 1.0);
    pub const LINEAR: Ease = Ease(0.0, 0.0, 1.0, 1.0);

    fn bez(t: f32, p1: f32, p2: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    }

    fn dbez(t: f32, p1: f32, p2: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
    }

    pub fn y(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        if *self == Ease::LINEAR {
            return x;
        }
        // Newton on x(t) = x, then bisection if the slope is too flat.
        let mut t = x;
        for _ in 0..8 {
            let err = Self::bez(t, self.0, self.2) - x;
            if err.abs() < 1e-5 {
                return Self::bez(t, self.1, self.3);
            }
            let d = Self::dbez(t, self.0, self.2);
            if d.abs() < 1e-6 {
                break;
            }
            t -= err / d;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        t = x;
        for _ in 0..24 {
            let v = Self::bez(t, self.0, self.2);
            if (v - x).abs() < 1e-5 {
                break;
            }
            if v < x {
                lo = t
            } else {
                hi = t
            }
            t = (lo + hi) * 0.5;
        }
        Self::bez(t, self.1, self.3)
    }
}

/// Keyframes for `Curve::Loop`: (offset 0..1, value) pairs in order, eased per segment like a CSS
/// animation's timing function. The first and last values should match for a seamless loop.
#[derive(Debug, PartialEq)]
pub struct Keys {
    pub frames: &'static [(f32, f32)],
    pub ease: Ease,
}

impl Keys {
    pub fn at(&self, phase: f32) -> f32 {
        let f = self.frames;
        match f.len() {
            0 => 0.0,
            1 => f[0].1,
            _ => {
                if phase <= f[0].0 {
                    return f[0].1;
                }
                for w in f.windows(2) {
                    let ((a, va), (b, vb)) = (w[0], w[1]);
                    if phase <= b {
                        let x = if b > a { (phase - a) / (b - a) } else { 1.0 };
                        return va + (vb - va) * self.ease.y(x);
                    }
                }
                f[f.len() - 1].1
            }
        }
    }
}

/// app.css `.breathe`: opacity 1 -> 0.35 -> 1 over 2.4 s, ease-in-out.
pub static BREATHE: Keys = Keys { frames: &[(0.0, 1.0), (0.5, 0.35), (1.0, 1.0)], ease: Ease::EASE_IN_OUT };

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Tween(Ease),
    /// Exponential approach with time constant `tau` seconds.
    Glide { tau: f32 },
    /// Damped spring: natural angular frequency `omega` (rad/s) and damping ratio `zeta`
    /// (1 = critically damped, below 1 overshoots).
    Spring { omega: f32, zeta: f32 },
    /// Repeats `Keys` every `dur` seconds until the next `set`/`snap`.
    Loop(&'static Keys),
}

impl Curve {
    /// nav/focus.ts's scroll glide: time constant 65 ms.
    pub const GLIDE: Curve = Curve::Glide { tau: 0.065 };
    /// A calm critically damped spring that settles in about 250 ms.
    pub const SPRING: Curve = Curve::Spring { omega: 28.0, zeta: 1.0 };
}

#[derive(Clone, Copy, Debug)]
pub struct Anim {
    from: f32,
    to: f32,
    t0: f64,
    /// Tween length, or the settle time of a glide or spring (after it, the value is `to`).
    dur: f32,
    curve: Curve,
    /// Velocity at `t0` (springs).
    v0: f32,
}

/// Displacement and velocity of a damped spring at `t`, relative to its rest point.
fn spring_at(x0: f32, v0: f32, omega: f32, zeta: f32, t: f32) -> (f32, f32) {
    let w = omega.max(1e-3);
    if (zeta - 1.0).abs() < 1e-3 {
        let e = (-w * t).exp();
        let b = v0 + w * x0;
        let x = e * (x0 + b * t);
        let v = e * (b - w * (x0 + b * t));
        (x, v)
    } else if zeta < 1.0 {
        let wd = w * (1.0 - zeta * zeta).sqrt();
        let e = (-zeta * w * t).exp();
        let b = (v0 + zeta * w * x0) / wd;
        let (s, c) = (wd * t).sin_cos();
        let x = e * (x0 * c + b * s);
        let v = e * ((b * wd - zeta * w * x0) * c - (x0 * wd + zeta * w * b) * s);
        (x, v)
    } else {
        let r = (zeta * zeta - 1.0).sqrt();
        let (r1, r2) = (-w * (zeta - r), -w * (zeta + r));
        let c1 = (v0 - r2 * x0) / (r1 - r2);
        let c2 = x0 - c1;
        let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
        (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
    }
}

/// How close counts as arrived: half a pixel for big moves, proportionally less for unit values.
fn eps(delta: f32) -> f32 {
    (delta.abs() * 0.002).clamp(1e-4, 0.5)
}

impl Anim {
    pub const fn new(v: f32) -> Anim {
        Anim { from: v, to: v, t0: 0.0, dur: 0.0, curve: Curve::Tween(Ease::PRO), v0: 0.0 }
    }

    pub fn get(&self, now: f64) -> f32 {
        if self.dur <= 0.0 {
            return self.to;
        }
        let t = (now - self.t0).max(0.0) as f32;
        match self.curve {
            Curve::Loop(k) => {
                let p = (t / self.dur).fract();
                k.at(p)
            }
            _ if t >= self.dur => self.to,
            Curve::Tween(e) => self.from + (self.to - self.from) * e.y(t / self.dur),
            Curve::Glide { tau } => self.to + (self.from - self.to) * (-t / tau.max(1e-4)).exp(),
            Curve::Spring { omega, zeta } => self.to + spring_at(self.from - self.to, self.v0, omega, zeta, t).0,
        }
    }

    /// Rate of change in units per second at `now`.
    pub fn velocity(&self, now: f64) -> f32 {
        if self.dur <= 0.0 {
            return 0.0;
        }
        let t = (now - self.t0).max(0.0) as f32;
        if t >= self.dur && !matches!(self.curve, Curve::Loop(_)) {
            return 0.0;
        }
        match self.curve {
            Curve::Glide { tau } => -(self.from - self.to) / tau * (-t / tau.max(1e-4)).exp(),
            Curve::Spring { omega, zeta } => spring_at(self.from - self.to, self.v0, omega, zeta, t).1,
            _ => {
                let h = 0.001;
                (self.get(now + h) - self.get(now - h)) / (2.0 * h as f32)
            }
        }
    }

    /// The value it is heading to (the scroll "aim").
    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn curve(&self) -> Curve {
        self.curve
    }

    /// Moves to `v` over `dur` seconds on the pro curve, from the current value.
    pub fn set(&mut self, v: f32, now: f64, dur: f32) {
        self.set_ease(v, now, dur, Ease::PRO)
    }

    pub fn set_ease(&mut self, v: f32, now: f64, dur: f32, ease: Ease) {
        if (v - self.to).abs() < 1e-6 && !self.looping() {
            return;
        }
        self.from = self.get(now);
        self.to = v;
        self.t0 = now;
        self.dur = dur;
        self.curve = Curve::Tween(ease);
        self.v0 = 0.0;
        if dur <= 0.0 {
            self.from = v;
        }
    }

    /// Moves to `v` on any curve. `dur` is used by `Curve::Tween` (length) and `Curve::Loop`
    /// (period, with `v` the value it rests on once stopped); glides and springs settle on their own.
    pub fn set_curve(&mut self, v: f32, now: f64, curve: Curve, dur: f32) {
        match curve {
            Curve::Tween(e) => self.set_ease(v, now, dur, e),
            Curve::Loop(_) => {
                self.from = v;
                self.to = v;
                self.t0 = now;
                self.dur = dur.max(1e-3);
                self.curve = curve;
                self.v0 = 0.0;
            }
            Curve::Glide { tau } => {
                if (v - self.to).abs() < 1e-6 && !self.looping() {
                    return;
                }
                let x = self.get(now);
                self.from = x;
                self.to = v;
                self.t0 = now;
                self.v0 = 0.0;
                self.curve = curve;
                let d = (x - v).abs();
                self.dur = if d <= eps(d) { 0.0 } else { tau * (d / eps(d)).ln() };
                if self.dur <= 0.0 {
                    self.from = v;
                }
            }
            Curve::Spring { omega, zeta } => {
                if (v - self.to).abs() < 1e-6 && !self.looping() {
                    return;
                }
                let (x, vel) = (self.get(now), self.velocity(now));
                self.from = x;
                self.to = v;
                self.t0 = now;
                self.v0 = vel;
                self.curve = curve;
                let e = eps(x - v);
                let mut t = 0.0f32;
                // Settle time: first moment after which it stays within eps (checked in 4 ms steps).
                let mut last_out = 0.0f32;
                while t < 5.0 {
                    let (dx, dv) = spring_at(x - v, vel, omega, zeta, t);
                    if dx.abs() > e || dv.abs() > e * omega {
                        last_out = t;
                    } else if t - last_out > 0.1 {
                        break;
                    }
                    t += 0.004;
                }
                self.dur = last_out + 0.004;
            }
        }
    }

    /// Springs to `to` from `from`, moving at `vel` units/s now (a fling that hit an end bounces).
    pub fn spring_from(&mut self, from: f32, vel: f32, to: f32, now: f64) {
        let (omega, zeta) = (28.0, 1.0);
        self.from = from;
        self.to = to;
        self.t0 = now;
        self.v0 = vel;
        self.curve = Curve::Spring { omega, zeta };
        let e = eps((from - to).abs().max(vel.abs() / omega));
        let (mut t, mut last_out) = (0.0f32, 0.0f32);
        while t < 5.0 {
            let (dx, dv) = spring_at(from - to, vel, omega, zeta, t);
            if dx.abs() > e || dv.abs() > e * omega {
                last_out = t;
            } else if t - last_out > 0.1 {
                break;
            }
            t += 0.004;
        }
        self.dur = last_out + 0.004;
    }

    /// Glides to `v` (nav/focus.ts's scroll motion).
    pub fn glide(&mut self, v: f32, now: f64) {
        self.set_curve(v, now, Curve::GLIDE, 0.0)
    }

    /// Springs to `v`, keeping the current velocity.
    pub fn spring(&mut self, v: f32, now: f64) {
        self.set_curve(v, now, Curve::SPRING, 0.0)
    }

    /// Starts a forever loop of `keys` with `period` seconds (stop it with `set` or `snap`).
    pub fn looping(&self) -> bool {
        matches!(self.curve, Curve::Loop(_)) && self.dur > 0.0
    }

    pub fn start_loop(&mut self, keys: &'static Keys, period: f32, now: f64) {
        let rest = keys.frames.first().map(|f| f.1).unwrap_or(self.to);
        self.set_curve(rest, now, Curve::Loop(keys), period)
    }

    pub fn snap(&mut self, v: f32) {
        self.from = v;
        self.to = v;
        self.dur = 0.0;
        self.v0 = 0.0;
        self.curve = Curve::Tween(Ease::PRO);
    }

    /// Restart a `from` -> `to` run (fades of new content, CSS keyframe `from` states).
    pub fn restart(&mut self, from: f32, to: f32, now: f64, dur: f32) {
        self.restart_ease(from, to, now, dur, Ease::PRO)
    }

    pub fn restart_ease(&mut self, from: f32, to: f32, now: f64, dur: f32, ease: Ease) {
        self.from = from;
        self.to = to;
        self.t0 = now;
        self.dur = dur;
        self.curve = Curve::Tween(ease);
        self.v0 = 0.0;
    }

    pub fn active(&self, now: f64) -> bool {
        self.dur > 0.0 && (self.looping() || now < self.t0 + self.dur as f64)
    }

    /// When it comes to rest (None while looping or already at rest).
    pub fn ends_at(&self) -> Option<f64> {
        (self.dur > 0.0 && !self.looping()).then(|| self.t0 + self.dur as f64)
    }
}

impl Default for Anim {
    fn default() -> Self {
        Anim::new(0.0)
    }
}

/// A colour that eases between values (background and text colour on focus, 110 ms).
#[derive(Clone, Copy, Debug)]
pub struct CAnim {
    from: Color,
    to: Color,
    t: Anim,
}

impl CAnim {
    pub const fn new(c: Color) -> CAnim {
        CAnim { from: c, to: c, t: Anim::new(1.0) }
    }

    pub fn get(&self, now: f64) -> Color {
        let k = self.t.get(now);
        if k >= 1.0 {
            self.to
        } else {
            Color::lerp(self.from, self.to, k)
        }
    }

    pub fn target(&self) -> Color {
        self.to
    }

    pub fn set(&mut self, c: Color, now: f64, dur: f32) {
        self.set_ease(c, now, dur, Ease::PRO)
    }

    pub fn set_ease(&mut self, c: Color, now: f64, dur: f32, ease: Ease) {
        if c == self.to {
            return;
        }
        self.from = self.get(now);
        self.to = c;
        self.t.restart_ease(0.0, 1.0, now, dur, ease);
        if dur <= 0.0 {
            self.t.snap(1.0);
        }
    }

    /// Runs `from` -> `to` from the start (app.css `tint-up`: a flash that fades to transparent).
    pub fn flash(&mut self, from: Color, to: Color, now: f64, dur: f32) {
        self.from = from;
        self.to = to;
        self.t.restart(0.0, 1.0, now, dur);
    }

    pub fn snap(&mut self, c: Color) {
        self.from = c;
        self.to = c;
        self.t.snap(1.0);
    }

    pub fn active(&self, now: f64) -> bool {
        self.t.active(now)
    }

    pub fn ends_at(&self) -> Option<f64> {
        self.t.ends_at()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pro_curve_endpoints_and_shape() {
        assert_eq!(Ease::PRO.y(0.0), 0.0);
        assert_eq!(Ease::PRO.y(1.0), 1.0);
        // A strong ease-out: well past half way at a quarter of the time.
        assert!(Ease::PRO.y(0.25) > 0.6);
        let mut prev = 0.0;
        for i in 0..=100 {
            let v = Ease::PRO.y(i as f32 / 100.0);
            assert!(v + 1e-4 >= prev, "monotonic");
            prev = v;
        }
    }

    #[test]
    fn interrupt_continues_from_current_value() {
        let mut a = Anim::new(0.0);
        a.set(1.0, 0.0, 0.2);
        let mid = a.get(0.1);
        a.set(0.0, 0.1, 0.2);
        assert!((a.get(0.1) - mid).abs() < 1e-5);
        assert!(!a.active(0.31));
        assert_eq!(a.get(0.31), 0.0);
    }

    #[test]
    fn glide_matches_focus_ts() {
        // focus.ts: k = 1 - exp(-dt / 65 ms) per frame, ~90 % of the way in 150 ms.
        let mut a = Anim::new(0.0);
        a.glide(1000.0, 0.0);
        let at150 = a.get(0.150);
        assert!((at150 - 1000.0 * (1.0 - (-150.0f32 / 65.0).exp())).abs() < 0.5, "{at150}");
        assert!(at150 > 880.0 && at150 < 920.0);
        // Settles within half a pixel, then reports rest.
        let end = a.ends_at().unwrap();
        assert!((a.get(end - 1e-4) - 1000.0).abs() <= 0.51);
        assert!(!a.active(end + 1e-3));
        assert_eq!(a.get(end + 1e-3), 1000.0);
        // Retarget mid-flight: continuous.
        let x = a.get(0.05);
        a.glide(500.0, 0.05);
        assert!((a.get(0.05) - x).abs() < 1e-3);
    }

    #[test]
    fn spring_keeps_velocity_and_settles() {
        let mut a = Anim::new(0.0);
        a.spring(100.0, 0.0);
        let v = a.velocity(0.05);
        assert!(v > 0.0);
        let x = a.get(0.05);
        a.spring(200.0, 0.05);
        assert!((a.get(0.05) - x).abs() < 1e-3);
        assert!((a.velocity(0.05) - v).abs() < v.abs() * 0.05 + 1.0, "velocity carried: {} vs {v}", a.velocity(0.05));
        let end = a.ends_at().unwrap();
        assert!(end < 2.0);
        assert!((a.get(end) - 200.0).abs() < 0.5);
        // Critically damped: no overshoot.
        let mut t = 0.05;
        while t < end {
            assert!(a.get(t) <= 200.0 + 0.5);
            t += 0.005;
        }
        // Underdamped overshoots, still settles.
        let mut b = Anim::new(0.0);
        b.set_curve(1.0, 0.0, Curve::Spring { omega: 30.0, zeta: 0.4 }, 0.0);
        let peak = (0..200).map(|i| b.get(i as f64 * 0.005)).fold(0.0f32, f32::max);
        assert!(peak > 1.05);
        assert!((b.get(b.ends_at().unwrap() + 0.01) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn loops_repeat_and_stop() {
        let mut a = Anim::new(1.0);
        a.start_loop(&BREATHE, 2.4, 0.0);
        assert!(a.active(100.0));
        assert!((a.get(1.2) - 0.35).abs() < 1e-4);
        assert!((a.get(2.4 + 1.2) - 0.35).abs() < 1e-3);
        assert!(a.get(0.6) < 1.0 && a.get(0.6) > 0.35);
        a.set(1.0, 3.0, 0.2);
        assert!(!a.active(3.3));
        assert_eq!(a.get(3.3), 1.0);
    }

    #[test]
    fn colour_flash_runs_from_the_start() {
        let mut c = CAnim::new(Color::TRANSPARENT);
        let tint = Color::rgb(42, 223, 114).alpha(0.25);
        c.flash(tint, Color::TRANSPARENT, 0.0, 0.6);
        assert_eq!(c.get(0.0), tint);
        assert!(c.active(0.3));
        assert_eq!(c.get(0.7), Color::TRANSPARENT);
    }
}
