//! Frame and input log for the device measurements (RR_STATS=<file>): one line per event, all on
//! CLOCK_MONOTONIC so it lines up with the journeys capture.
//!   start <t>                      process start (main)
//!   first <t> <ms since start>     first frame presented
//!   in <t_kernel> <button> <rep> <t_handled>
//!   f <n> <t_begin> <t_presented> <cpu_us> <acquire_us> <submit_us> <prims> <batches> <layout 0|1> <inputs> <interval_us> <animating 0|1>
//!   mem <t> <rss_kib> <texture_bytes> <glyphs>

use std::fs::File;
use std::io::{BufWriter, Write};

pub struct Stats {
    out: Option<BufWriter<File>>,
    pub frames: u64,
    lines: u32,
}

impl Stats {
    pub fn from_env() -> Stats {
        let out = std::env::var("RR_STATS").ok().filter(|s| !s.is_empty()).and_then(|p| File::create(p).ok()).map(BufWriter::new);
        Stats { out, frames: 0, lines: 0 }
    }

    pub fn on(&self) -> bool {
        self.out.is_some()
    }

    pub fn line(&mut self, s: std::fmt::Arguments) {
        if let Some(o) = self.out.as_mut() {
            let _ = o.write_fmt(s);
            let _ = o.write_all(b"\n");
            self.lines += 1;
        }
    }

    /// Flushed after every idle frame and every 120 lines, so a kill loses little.
    pub fn flush(&mut self) {
        if let Some(o) = self.out.as_mut() {
            let _ = o.flush();
        }
        self.lines = 0;
    }

    pub fn maybe_flush(&mut self) {
        if self.lines >= 120 {
            self.flush();
        }
    }
}

// ---- Frame budget ----------------------------------------------------------------------------
//
// The plan's gates: CPU per frame p99 under 4 ms on
// the device, no layout inside an animation window, at most one layout per input, zero frame
// intervals over 25 ms while focus moves. A frame marks its phases on a `FrameClock`; `Budget`
// accumulates the whole run and writes, with RR_STATS on:
//
//   fb <n> <t_begin> <latch_us> <update_us> <layout_us> <paint_us> <upload_us> <submit_us> <cpu_us>
//      <prims> <batches> <layouts> <inputs> <edits> <animating 0|1>
//   budget <frames> <cpu_p50_us> <cpu_p95_us> <cpu_p99_us> <cpu_max_us> <over_budget>
//      <layouts_in_animation> <max_layouts_per_input> <gaps_over_25ms_moving>
//
// (`f` lines stay as the bench reads them; `fb` adds the breakdown.)

/// The phases of one frame, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Acquire returned (or the frame began, headless).
    Begin,
    /// Inputs latched and handled.
    Latch,
    /// Views updated, focus applied (scene edits).
    Update,
    /// Layout (if it ran).
    Layout,
    /// Scene painted into the draw list.
    Paint,
    /// Glyphs and images uploaded.
    Upload,
    /// Encoded, submitted and presented.
    Submit,
}

const PHASES: usize = 7;

/// Stamps for one frame (CLOCK_MONOTONIC ns). Unmarked phases take the previous stamp.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameClock {
    t: [u64; PHASES],
    marked: [bool; PHASES],
}

impl FrameClock {
    pub fn start() -> FrameClock {
        let mut c = FrameClock::default();
        c.mark(Phase::Begin);
        c
    }

    pub fn mark(&mut self, p: Phase) {
        self.mark_at(p, crate::clock::mono_ns());
    }

    pub fn mark_at(&mut self, p: Phase, t: u64) {
        self.t[p as usize] = t;
        self.marked[p as usize] = true;
    }

    fn at(&self, i: usize) -> u64 {
        (0..=i).rev().find(|j| self.marked[*j]).map(|j| self.t[j]).unwrap_or(0)
    }

    /// Microseconds spent in phase `p` (from the previous phase's stamp).
    pub fn us(&self, p: Phase) -> u32 {
        let i = p as usize;
        if i == 0 || !self.marked[i] {
            return 0;
        }
        (self.at(i).saturating_sub(self.at(i - 1)) / 1000) as u32
    }

    /// From Begin to the last marked phase, in µs.
    pub fn cpu_us(&self) -> u32 {
        (self.at(PHASES - 1).saturating_sub(self.t[0]) / 1000) as u32
    }

    pub fn begin_ns(&self) -> u64 {
        self.t[0]
    }

    pub fn end_ns(&self) -> u64 {
        self.at(PHASES - 1)
    }
}

/// What else the frame did, for the gates.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameFacts {
    pub prims: u32,
    pub batches: u32,
    /// Layout passes this frame (0 or 1 normally).
    pub layouts: u32,
    /// Inputs latched this frame.
    pub inputs: u32,
    /// Scene edits from view updates (`view::Edits` summed: created + removed + moved + restyled + retexted + relooked).
    pub edits: u32,
    /// Something was animating when the frame began (scene or Nav scroll).
    pub animating: bool,
    /// New data (a store change, an image, the clock) arrived this frame: a layout is allowed.
    pub data: bool,
}

/// Linear-bucket histogram of µs values (50 µs buckets up to 100 ms).
#[derive(Clone)]
struct Hist {
    b: Vec<u32>,
    n: u64,
    max: u32,
}

impl Default for Hist {
    fn default() -> Self {
        Hist { b: vec![0; 2001], n: 0, max: 0 }
    }
}

impl Hist {
    fn push(&mut self, us: u32) {
        let i = (us / 50).min(2000) as usize;
        self.b[i] += 1;
        self.n += 1;
        self.max = self.max.max(us);
    }

    fn pct(&self, p: f64) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let want = ((self.n as f64) * p).ceil().max(1.0) as u64;
        let mut acc = 0u64;
        for (i, c) in self.b.iter().enumerate() {
            acc += *c as u64;
            if acc >= want {
                return ((i as u32) * 50 + 25).min(self.max);
            }
        }
        self.max
    }
}

/// The run's numbers against the gates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BudgetReport {
    pub frames: u64,
    pub cpu_p50_us: u32,
    pub cpu_p95_us: u32,
    pub cpu_p99_us: u32,
    pub cpu_max_us: u32,
    /// Frames over the CPU budget.
    pub over_budget: u32,
    /// Layouts in frames that only animated (no input, no data).
    pub layouts_in_animation: u32,
    /// The most layouts that one input caused (layouts until the next input).
    pub max_layouts_per_input: u32,
    /// Frame intervals over 25 ms within 500 ms of an input while something moved.
    pub gaps_over_25ms_moving: u32,
}

/// Gate limits; `Gates::PLAN` are the plan's.
#[derive(Clone, Copy, Debug)]
pub struct Gates {
    pub cpu_p99_us: u32,
    pub layouts_in_animation: u32,
    pub layouts_per_input: u32,
    pub gaps_over_25ms_moving: u32,
}

impl Gates {
    pub const PLAN: Gates = Gates { cpu_p99_us: 4000, layouts_in_animation: 0, layouts_per_input: 1, gaps_over_25ms_moving: 0 };
}

impl BudgetReport {
    /// The gates this run breaks, in words (empty: it passes).
    pub fn check(&self, g: &Gates) -> Vec<String> {
        let mut v = Vec::new();
        if self.cpu_p99_us > g.cpu_p99_us {
            v.push(format!("CPU per frame p99 {} us over {} us", self.cpu_p99_us, g.cpu_p99_us));
        }
        if self.layouts_in_animation > g.layouts_in_animation {
            v.push(format!("{} layouts inside animation frames", self.layouts_in_animation));
        }
        if self.max_layouts_per_input > g.layouts_per_input {
            v.push(format!("an input caused {} layouts", self.max_layouts_per_input));
        }
        if self.gaps_over_25ms_moving > g.gaps_over_25ms_moving {
            v.push(format!("{} frame intervals over 25 ms while moving", self.gaps_over_25ms_moving));
        }
        v
    }
}

/// Accumulates frames for the gates and writes `fb` lines.
pub struct Budget {
    cpu: Hist,
    frames: u64,
    over: u32,
    budget_us: u32,
    layouts_in_anim: u32,
    layouts_since_input: u32,
    max_layouts_per_input: u32,
    last_end: u64,
    last_input: u64,
    gaps: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Budget::new(4000)
    }
}

impl Budget {
    /// `budget_us`: a frame over this counts in `over_budget` (4 ms on the device).
    pub fn new(budget_us: u32) -> Budget {
        Budget { cpu: Hist::default(), frames: 0, over: 0, budget_us, layouts_in_anim: 0, layouts_since_input: 0, max_layouts_per_input: 0, last_end: 0, last_input: 0, gaps: 0 }
    }

    /// Records a finished frame and writes its `fb` line.
    pub fn frame(&mut self, c: &FrameClock, f: &FrameFacts, stats: &mut Stats) {
        self.frames += 1;
        let cpu = c.cpu_us();
        self.cpu.push(cpu);
        if cpu > self.budget_us {
            self.over += 1;
        }
        let end = c.end_ns();
        if f.inputs > 0 {
            self.layouts_since_input = 0;
            self.last_input = end;
        }
        if f.layouts > 0 {
            if f.inputs == 0 && !f.data && f.animating {
                self.layouts_in_anim += f.layouts;
            }
            if self.last_input > 0 && !f.data {
                self.layouts_since_input += f.layouts;
                self.max_layouts_per_input = self.max_layouts_per_input.max(self.layouts_since_input);
            }
        }
        if self.last_end > 0 && f.animating && self.last_input > 0 && end.saturating_sub(self.last_input) < 500_000_000 && end.saturating_sub(self.last_end) > 25_000_000 {
            self.gaps += 1;
        }
        self.last_end = end;
        if stats.on() {
            stats.line(format_args!(
                "fb {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
                self.frames,
                c.begin_ns(),
                c.us(Phase::Latch),
                c.us(Phase::Update),
                c.us(Phase::Layout),
                c.us(Phase::Paint),
                c.us(Phase::Upload),
                c.us(Phase::Submit),
                cpu,
                f.prims,
                f.batches,
                f.layouts,
                f.inputs,
                f.edits,
                f.animating as u8
            ));
        }
    }

    /// The frame was idle long enough that the next one starts a new motion window.
    pub fn idle(&mut self) {
        self.last_end = 0;
    }

    pub fn report(&self) -> BudgetReport {
        BudgetReport {
            frames: self.frames,
            cpu_p50_us: self.cpu.pct(0.50),
            cpu_p95_us: self.cpu.pct(0.95),
            cpu_p99_us: self.cpu.pct(0.99),
            cpu_max_us: self.cpu.max,
            over_budget: self.over,
            layouts_in_animation: self.layouts_in_anim,
            max_layouts_per_input: self.max_layouts_per_input,
            gaps_over_25ms_moving: self.gaps,
        }
    }

    /// Writes the `budget` summary line (call on exit and every few seconds).
    pub fn summary(&self, stats: &mut Stats) {
        let r = self.report();
        stats.line(format_args!(
            "budget {} {} {} {} {} {} {} {} {}",
            r.frames, r.cpu_p50_us, r.cpu_p95_us, r.cpu_p99_us, r.cpu_max_us, r.over_budget, r.layouts_in_animation, r.max_layouts_per_input, r.gaps_over_25ms_moving
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(t0: u64, cpu_us: u64) -> FrameClock {
        let mut c = FrameClock::default();
        c.mark_at(Phase::Begin, t0);
        c.mark_at(Phase::Latch, t0 + cpu_us * 200);
        c.mark_at(Phase::Paint, t0 + cpu_us * 700);
        c.mark_at(Phase::Submit, t0 + cpu_us * 1000);
        c
    }

    #[test]
    fn phases_and_cpu() {
        let c = clock(1_000_000, 1000);
        assert_eq!(c.cpu_us(), 1000);
        assert_eq!(c.us(Phase::Latch), 200);
        assert_eq!(c.us(Phase::Update), 0, "unmarked");
        assert_eq!(c.us(Phase::Paint), 500);
        assert_eq!(c.us(Phase::Submit), 300);
    }

    #[test]
    fn gates() {
        let mut b = Budget::new(4000);
        let mut s = Stats::from_env();
        let mut t = 1_000_000_000u64;
        // An input with one layout, then an animation that never lays out: passes.
        b.frame(&clock(t, 1500), &FrameFacts { inputs: 1, layouts: 1, ..Default::default() }, &mut s);
        for _ in 0..20 {
            t += 16_666_667;
            b.frame(&clock(t, 800), &FrameFacts { animating: true, ..Default::default() }, &mut s);
        }
        let r = b.report();
        assert!(r.check(&Gates::PLAN).is_empty(), "{:?}", r.check(&Gates::PLAN));
        assert_eq!(r.frames, 21);
        assert!(r.cpu_p50_us >= 800 && r.cpu_p50_us <= 850);
        // A layout in an animation frame, a 40 ms gap while moving, a slow frame.
        t += 40_000_000;
        b.frame(&clock(t, 5000), &FrameFacts { animating: true, layouts: 1, ..Default::default() }, &mut s);
        let r = b.report();
        assert_eq!(r.layouts_in_animation, 1);
        assert_eq!(r.gaps_over_25ms_moving, 1);
        assert_eq!(r.over_budget, 1);
        assert_eq!(r.max_layouts_per_input, 2);
        // p99 is the 5 ms frame too.
        assert_eq!(r.check(&Gates::PLAN).len(), 4);
    }
}
