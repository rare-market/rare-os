//! Frame pacing: start each frame as late as it can and still make the next refresh, so a frame
//! carries the newest input and its animation time is sampled close to when it is seen.
//!
//! The model is a refresh grid (period + one known vblank time) plus our frame cost:
//!
//! ```text
//!   start = V - budget - margin      V = the first refresh we can still make
//!   budget = p95 of recent (latch -> submit) CPU time      margin = compositor lead + safety
//! ```
//!
//! Where the grid comes from, best first (`RR_PACING`):
//! - `timing`: VK_EXT_present_timing on our own swapchain (Linux, RADV 26.2+): refresh duration
//!   from the swapchain, past presentation times as the vblank anchor, a target time on every
//!   present, and `margin` adapted from misses. Falls back to `estimate` if setup fails.
//! - `estimate`: the period from `RR_REFRESH_HZ` (default 60, the SuiPlay panel) and the anchor
//!   from frames that blocked in acquire (FIFO releases images at vblank).
//! - `off` (default until the device run signs it off): acquire-first as in the POC, with the
//!   cadence guard (`Pacer::cadence_sleep_ns`, RR_CADENCE=0 turns it off): frames of one run
//!   stay a refresh apart instead of bursting into the swapchain's free images after an idle.
//!
//! The app loop asks `next_start(now)` when it wants a frame and sleeps until then; the frame
//! calls `begin` / `end` around its work and the Gpu feeds `on_acquire` / presents.

use crate::clock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pacing {
    Off,
    Estimate,
    PresentTiming,
}

/// Rolling frame cost: the last 120 samples, p95 on demand.
#[derive(Clone, Debug)]
pub struct Budget {
    ring: [u32; 120],
    n: usize,
    i: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Budget { ring: [0; 120], n: 0, i: 0 }
    }
}

impl Budget {
    pub fn push(&mut self, us: u32) {
        self.ring[self.i] = us;
        self.i = (self.i + 1) % self.ring.len();
        self.n = (self.n + 1).min(self.ring.len());
    }

    /// Percentile `p` (0..1) of the recorded samples in microseconds (0 with none).
    pub fn pct(&self, p: f32) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let mut v: Vec<u32> = self.ring[..self.n].to_vec();
        v.sort_unstable();
        v[((self.n as f32 - 1.0) * p).round() as usize]
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

pub struct Pacer {
    pub mode: Pacing,
    /// Refresh period in ns.
    pub refresh_ns: u64,
    /// A vblank (or present) time on CLOCK_MONOTONIC that anchors the grid.
    pub anchor_ns: Option<u64>,
    /// CPU cost of a frame from latch to submit, in µs.
    pub budget: Budget,
    /// Compositor lead plus safety, in ns. gamescope wakes before vblank by its rolling draw time
    /// plus a 1.65 ms red zone; 3 ms is a safe start that `timing` mode tunes down.
    pub margin_ns: u64,
    /// Frames that made / missed the refresh they aimed at (present timing feedback).
    pub hits: u64,
    pub misses: u64,
    /// The refresh the frame in flight aims at.
    pub target_ns: u64,
    frame_begin: u64,
    /// `off` mode's cadence guard (on unless RR_CADENCE=0): see `cadence_sleep_ns`.
    pub cadence: bool,
    last_acquire: u64,
}

/// How long before the next refresh the cadence guard lets a frame acquire: enough for the sleep's
/// overshoot, so a frame that would have blocked in acquire anyway is never made later.
const CADENCE_SLACK: u64 = 2_000_000;

const MIN_MARGIN: u64 = 1_000_000;
const MAX_MARGIN: u64 = 8_000_000;

impl Pacer {
    pub fn new(mode: Pacing, refresh_hz: f32) -> Pacer {
        Pacer {
            mode,
            refresh_ns: (1e9 / refresh_hz.max(1.0)) as u64,
            anchor_ns: None,
            budget: Budget::default(),
            margin_ns: 3_000_000,
            hits: 0,
            misses: 0,
            target_ns: 0,
            frame_begin: 0,
            cadence: true,
            last_acquire: 0,
        }
    }

    /// The cadence guard of acquire-first pacing (`off`). After an idle the swapchain has two free
    /// images, so the first two frames of a motion acquired at once and rendered ~2 ms apart: the
    /// second showed the animation only ~2 ms on, and the third then blocked until the compositor
    /// released the first, two refreshes later (the 26-37 ms intervals the device budget gate
    /// failed on, 2026-10-27 bench). The guard keeps frames of a continuous run at least a
    /// refresh apart (minus `CADENCE_SLACK`): the first frame after an idle (input) still starts
    /// at once, and in steady state the acquire blocks to vblank either way. How long to sleep
    /// before acquiring at `now` (0: go).
    pub fn cadence_sleep_ns(&self, now: u64) -> u64 {
        if self.mode != Pacing::Off || !self.cadence || self.last_acquire == 0 || now <= self.last_acquire {
            return 0;
        }
        let since = now - self.last_acquire;
        let spacing = self.refresh_ns.saturating_sub(CADENCE_SLACK);
        if since >= spacing || since >= 2 * self.refresh_ns {
            0
        } else {
            spacing - since
        }
    }

    /// `RR_PACING=off|estimate|timing`, `RR_REFRESH_HZ` (default 60), `RR_PACE_MARGIN_US`.
    pub fn from_env() -> Pacer {
        let mode = match std::env::var("RR_PACING").unwrap_or_default().to_ascii_lowercase().as_str() {
            "estimate" => Pacing::Estimate,
            "timing" | "present_timing" | "on" => Pacing::PresentTiming,
            _ => Pacing::Off,
        };
        let hz = std::env::var("RR_REFRESH_HZ").ok().and_then(|v| v.parse().ok()).unwrap_or(60.0);
        let mut p = Pacer::new(mode, hz);
        if let Some(m) = std::env::var("RR_PACE_MARGIN_US").ok().and_then(|v| v.parse::<u64>().ok()) {
            p.margin_ns = m * 1000;
        }
        p.cadence = std::env::var("RR_CADENCE").map_or(true, |v| v != "0");
        p
    }

    pub fn describe(&self) -> String {
        match self.mode {
            Pacing::Off => "off".into(),
            m => format!("{m:?} {:.2} Hz margin {:.2} ms", 1e9 / self.refresh_ns as f64, self.margin_ns as f64 / 1e6),
        }
    }

    pub fn on(&self) -> bool {
        self.mode != Pacing::Off
    }

    /// The first refresh at or after `t` on the grid (None without an anchor).
    pub fn refresh_after(&self, t: u64) -> Option<u64> {
        let a = self.anchor_ns?;
        let p = self.refresh_ns.max(1);
        Some(if t <= a { a - ((a - t) / p) * p } else { a + (t - a).div_ceil(p) * p })
    }

    /// The cost we plan for: p95 of recent frames plus 20 %, at least 0.5 ms.
    pub fn plan_ns(&self) -> u64 {
        (self.budget.pct(0.95) as u64 * 1200).max(500_000)
    }

    /// When the next frame should start (latch input) to make the earliest refresh it still can.
    /// None: start now (pacing off, or no grid yet).
    pub fn next_start(&self, now: u64) -> Option<u64> {
        if self.mode == Pacing::Off {
            return None;
        }
        let lead = self.plan_ns() + self.margin_ns;
        let v = self.refresh_after(now + lead)?;
        Some(v - lead)
    }

    /// Sleeps until `next_start` (for loops that block in place of a timed wake-up).
    pub fn wait(&self) {
        let now = clock::mono_ns();
        if let Some(t) = self.next_start(now) {
            if t > now {
                std::thread::sleep(std::time::Duration::from_nanos(t - now));
            }
        }
    }

    /// Call when the frame starts its work (after acquire, before latching input).
    pub fn begin(&mut self, now: u64) {
        self.frame_begin = now;
        let lead = self.plan_ns() + self.margin_ns;
        self.target_ns = self.refresh_after(now + lead).unwrap_or(0);
    }

    /// Call after submit: records the frame's CPU cost.
    pub fn end(&mut self, now: u64) {
        if self.frame_begin > 0 && now > self.frame_begin {
            self.budget.push(((now - self.frame_begin) / 1000).min(u32::MAX as u64) as u32);
        }
    }

    /// An acquire that blocked ended at `t` after `blocked_ns`: under FIFO that is a vblank.
    pub fn on_acquire(&mut self, t: u64, blocked_ns: u64) {
        self.last_acquire = t;
        if self.mode == Pacing::Estimate && blocked_ns > 1_000_000 {
            self.anchor_ns = Some(match self.anchor_ns {
                // Nudge the phase toward the new sample (a quarter of the error), robust to jitter.
                Some(a) => {
                    let p = self.refresh_ns as i64;
                    let err = ((t as i64 - a as i64) % p + p + p / 2) % p - p / 2;
                    (a as i64 + err / 4) as u64
                }
                None => t,
            });
        }
    }

    /// Present timing feedback: the frame that aimed at `target` was seen at `actual`.
    pub fn on_presented(&mut self, target: u64, actual: u64, refresh_ns: Option<u64>) {
        if let Some(r) = refresh_ns.filter(|r| *r > 1_000_000) {
            self.refresh_ns = r;
        }
        self.anchor_ns = Some(actual);
        if target == 0 {
            return;
        }
        if actual > target + self.refresh_ns / 2 {
            // Missed: back off fast.
            self.misses += 1;
            self.margin_ns = (self.margin_ns + 500_000).min(MAX_MARGIN);
        } else {
            // Made it: creep closer, 20 µs a frame.
            self.hits += 1;
            self.margin_ns = self.margin_ns.saturating_sub(20_000).max(MIN_MARGIN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_and_start() {
        let mut p = Pacer::new(Pacing::Estimate, 60.0);
        assert_eq!(p.next_start(0), None, "no anchor yet");
        p.anchor_ns = Some(1_000_000_000);
        let r = p.refresh_ns;
        assert_eq!(p.refresh_after(1_000_000_000), Some(1_000_000_000));
        assert_eq!(p.refresh_after(1_000_000_001), Some(1_000_000_000 + r));
        assert_eq!(p.refresh_after(1_000_000_000 - r + 5), Some(1_000_000_000));
        for _ in 0..50 {
            p.budget.push(1000);
        }
        // 1 ms p95 -> 1.2 ms planned + 3 ms margin = 4.2 ms lead.
        let now = 1_000_000_000 + 2_000_000;
        let s = p.next_start(now).unwrap();
        assert_eq!(s, 1_000_000_000 + r - 4_200_000);
        // Too late for that refresh: aim at the one after.
        let late = 1_000_000_000 + r - 1_000_000;
        assert_eq!(p.next_start(late).unwrap(), 1_000_000_000 + 2 * r - 4_200_000);
    }

    #[test]
    fn feedback_adapts_margin() {
        let mut p = Pacer::new(Pacing::PresentTiming, 60.0);
        let m0 = p.margin_ns;
        p.on_presented(100_000_000, 100_000_000 + p.refresh_ns, None);
        assert_eq!(p.misses, 1);
        assert!(p.margin_ns > m0);
        for i in 0..1000 {
            p.on_presented(200_000_000 + i, 200_000_000 + i, Some(16_666_667));
        }
        assert_eq!(p.margin_ns, MIN_MARGIN);
        assert_eq!(p.refresh_ns, 16_666_667);
    }

    #[test]
    fn acquire_phase_estimate() {
        let mut p = Pacer::new(Pacing::Estimate, 60.0);
        p.on_acquire(5_000_000, 6_000_000);
        assert_eq!(p.anchor_ns, Some(5_000_000));
        // A sample one period later plus 400 µs of jitter moves the anchor by a quarter of it.
        p.on_acquire(5_000_000 + p.refresh_ns + 400_000, 6_000_000);
        assert_eq!(p.anchor_ns, Some(5_100_000));
        // Short blocks are not vblanks.
        p.on_acquire(9_000_000, 10_000);
        assert_eq!(p.anchor_ns, Some(5_100_000));
    }

    #[test]
    fn cadence_spaces_a_burst_but_never_the_first_frame() {
        let mut p = Pacer::new(Pacing::Off, 60.0);
        let r = p.refresh_ns;
        assert_eq!(p.cadence_sleep_ns(1_000_000_000), 0, "nothing acquired yet");
        // Input after an idle: the acquire returns at once (a free image).
        p.on_acquire(1_000_000_000, 100_000);
        // The next frame 2 ms later would burst: it waits until a refresh (minus slack) has passed.
        assert_eq!(p.cadence_sleep_ns(1_002_000_000), r - CADENCE_SLACK - 2_000_000);
        // Steady state, or a frame long after: no wait.
        assert_eq!(p.cadence_sleep_ns(1_000_000_000 + r), 0);
        assert_eq!(p.cadence_sleep_ns(1_000_000_000 + 3 * r), 0);
        p.cadence = false;
        assert_eq!(p.cadence_sleep_ns(1_002_000_000), 0);
        let mut e = Pacer::new(Pacing::Estimate, 60.0);
        e.on_acquire(1_000_000_000, 100_000);
        assert_eq!(e.cadence_sleep_ns(1_002_000_000), 0, "estimate mode paces itself");
    }

    #[test]
    fn budget_percentile() {
        let mut b = Budget::default();
        for i in 1..=100 {
            b.push(i);
        }
        assert_eq!(b.pct(0.95), 95);
        assert_eq!(b.pct(0.0), 1);
        for _ in 0..200 {
            b.push(7);
        }
        assert_eq!(b.len(), 120);
        assert_eq!(b.pct(0.99), 7);
    }
}
