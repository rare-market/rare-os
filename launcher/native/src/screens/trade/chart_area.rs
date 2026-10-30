//! The chart as a focus target with a chart mode (trade/ChartArea.tsx). A on it (or View) enters
//! chart mode; then the pad drives the chart and nothing else: left/right move the crosshair bar by
//! bar (held: 1, then 3, then 6 bars a step), up/down zoom, the right stick slides time (x) and zooms
//! (y), LT/RT step the timeframe, Y flips candles and line, X shows or hides volume, A cycles
//! MA 20 / MA 50 / VWAP, B leaves. The readout over the chart shows the crosshair bar.
//!
//! `ChartMode` is the pure state machine (tested); `ChartArea` is its panel on the scene plus the
//! engine chart (`rare_render::chart`), painted in the scene's paint slot (`SLOT`) so sheets and toasts sit over it.

use crate::kit::format::grouped;
use crate::screens::market::host::*;
use rare_render::chart::{self, Candle, Chart, ChartType, Indicator, Textures, INDICATOR_STEPS};
use rare_render::input::Pad;
use rare_render::scene::{Shadow, SlotPaint};
use rare_render::view::{El, Look};
use rare_render::{DrawList, Id, TextSys};

/// What a press did in chart mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartOut {
    /// Not for the chart (chart mode is off).
    Pass,
    /// Taken by the chart.
    Took,
    /// LT/RT asked for the timeframe at this index.
    Tf(usize),
    /// Chart mode turned on or off.
    Mode(bool),
}

/// Chart mode's state: everything the pad changes, without the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartMode {
    pub on: bool,
    pub ty: ChartType,
    pub volume: bool,
    /// Index into INDICATOR_STEPS.
    pub ind: usize,
    pub cursor: Option<usize>,
    held: u32,
}

impl Default for ChartMode {
    fn default() -> ChartMode {
        ChartMode { on: false, ty: ChartType::Candles, volume: true, ind: 0, cursor: None, held: 0 }
    }
}

/// The hint bar while chart mode is on (button as hints.ts names it, label).
pub const HINTS: [(&str, &str); 8] = [("left", "Move"), ("up", "Zoom"), ("lt", "Shorter"), ("rt", "Longer"), ("y", "Candles or line"), ("x", "Volume"), ("a", "Indicators"), ("b", "Leave chart")];

impl ChartMode {
    pub fn indicators(&self) -> &'static [Indicator] {
        INDICATOR_STEPS[self.ind % INDICATOR_STEPS.len()]
    }

    pub fn enter(&mut self, n: usize) {
        self.on = true;
        self.cursor = Some(n.saturating_sub(1));
    }

    pub fn leave(&mut self) {
        self.on = false;
        self.cursor = None;
    }

    /// A new timeframe or coin: the cursor goes to the newest bar.
    pub fn refit(&mut self, n: usize) {
        if self.on {
            self.cursor = Some(n.saturating_sub(1));
        }
    }

    /// A press. `n` bars, `tf` the current timeframe of `tfs`. `zoom` gets the zoom factor.
    pub fn press(&mut self, b: Pad, repeat: bool, n: usize, tf: usize, tfs: usize, zoom: &mut dyn FnMut(f64)) -> ChartOut {
        if b == Pad::Select {
            if repeat {
                return ChartOut::Took;
            }
            if self.on {
                self.leave();
            } else {
                self.enter(n);
            }
            return ChartOut::Mode(self.on);
        }
        if !self.on {
            return ChartOut::Pass;
        }
        match b {
            Pad::Start | Pad::Guide => return ChartOut::Pass,
            Pad::Left | Pad::Right => {
                self.held = if repeat { self.held + 1 } else { 0 };
                let step = if self.held > 24 {
                    6
                } else if self.held > 10 {
                    3
                } else {
                    1
                };
                let c = self.cursor.unwrap_or(n.saturating_sub(1)) as i64 + if b == Pad::Right { step } else { -step };
                self.cursor = Some(chart::clamp_index(c as f64, n));
            }
            Pad::Up => zoom(0.8),
            Pad::Down => zoom(1.25),
            Pad::Lt | Pad::Rt => {
                let i = tf as i64 + if b == Pad::Rt { 1 } else { -1 };
                if i >= 0 && (i as usize) < tfs {
                    return ChartOut::Tf(i as usize);
                }
            }
            Pad::Y => {
                if !repeat {
                    self.ty = if self.ty == ChartType::Candles { ChartType::Line } else { ChartType::Candles };
                }
            }
            Pad::X => {
                if !repeat {
                    self.volume = !self.volume;
                }
            }
            Pad::A => {
                if !repeat {
                    self.ind = (self.ind + 1) % INDICATOR_STEPS.len();
                }
            }
            Pad::B => {
                self.leave();
                return ChartOut::Mode(false);
            }
            _ => {}
        }
        ChartOut::Took
    }
}

/// The readout's words for one bar (ChartArea's top-left line).
pub fn readout_texts(bars: &[Candle], i: usize, fmt: &dyn Fn(f64) -> String, volume: bool, tz: i64) -> Vec<(String, Option<bool>)> {
    let Some(b) = bars.get(i) else { return Vec::new() };
    let r = chart::readout(b, i.checked_sub(1).and_then(|j| bars.get(j)));
    let mut out = vec![
        (chart::readout_time(r.time, tz), None),
        (format!("O {}", fmt(r.open)), Some(false)),
        (format!("H {}", fmt(r.high)), Some(false)),
        (format!("L {}", fmt(r.low)), Some(false)),
        (format!("C {}", fmt(r.close)), Some(false)),
    ];
    let pct = format!("{}{:.2}%", if r.change_pct >= 0.0 { "+" } else { "" }, r.change_pct);
    out.push((pct, Some(r.change_pct >= 0.0)));
    if volume {
        let v = if r.volume != 0.0 { grouped(super::fmt::precision_num(r.volume, 3), 0, 3) } else { "0".into() };
        out.push((format!("Vol {v}"), Some(false)));
    }
    out
}

/// Chart mode plus the engine chart; the page puts `ChartArea::el` in its tree and calls
/// `prepare` before the scene paints and `paint_slot` from the scene's slot callback.
pub struct ChartArea {
    pub chart: Chart,
    pub mode: ChartMode,
    pub tfs: usize,
    pub tf: usize,
    /// UTC offset for labels and the readout (None: the system's).
    pub tz: Option<i64>,
    stick: (f32, f32),
    last: f64,
    dirty: bool,
}

/// The element names the page's view gives the panel and the chart's rect.
pub const PANEL: &str = "chart-panel";
pub const AREA: &str = "chart-area";
/// The paint slot of the chart's rect.
pub const SLOT: u32 = 0x4348;

impl ChartArea {
    pub fn new(fmt: Box<chart::Fmt>, tfs: usize, tf: usize) -> ChartArea {
        let chart = Chart::new(chart::Look::default(), fmt, Box::new(|v| if v > 0.0 { super::fmt::count(Some(v)) } else { String::new() }));
        ChartArea { chart, mode: ChartMode::default(), tfs, tf, tz: None, stick: (0.0, 0.0), last: 0.0, dirty: true }
    }

    pub fn set_rows(&mut self, rows: &[Candle], fit_key: &str) {
        let refit = self.chart.m.fitted() != fit_key;
        self.chart.m.set_rows(rows, fit_key);
        if refit {
            self.mode.refit(self.chart.m.bars.len());
        } else if let Some(c) = self.mode.cursor {
            if c + 1 > self.chart.m.bars.len() {
                self.mode.cursor = Some(self.chart.m.bars.len().saturating_sub(1));
            }
        }
        self.sync();
    }

    /// Copies chart mode into the engine chart.
    fn sync(&mut self) {
        let m = &mut self.chart.m;
        m.ty = self.mode.ty;
        m.volume = self.mode.volume;
        m.indicators = self.mode.indicators().to_vec();
        m.tz = self.tz;
        let want = self.mode.cursor.map(|c| c as f64);
        if want != m.cursor.map(|c| c as f64) {
            m.set_cursor(want);
        }
        self.dirty = true;
    }

    /// A press on the page while this chart shows. `focused`: the chart box has focus.
    pub fn press(&mut self, b: Pad, repeat: bool, focused: bool) -> ChartOut {
        if !self.mode.on && b == Pad::A && focused {
            self.mode.enter(self.chart.m.bars.len());
            self.sync();
            return ChartOut::Mode(true);
        }
        let n = self.chart.m.bars.len();
        let mut zf = None;
        let out = self.mode.press(b, repeat, n, self.tf, self.tfs, &mut |f| zf = Some(f));
        if let Some(f) = zf {
            self.chart.m.zoom(f);
        }
        if out != ChartOut::Pass {
            self.sync();
        }
        out
    }

    /// The right stick (x slides time, y zooms) while chart mode is on.
    pub fn stick(&mut self, x: f32, y: f32, now: f64) {
        if !self.mode.on {
            return;
        }
        if self.stick == (0.0, 0.0) {
            self.last = now;
        }
        self.stick = (x, y);
    }

    /// Applies the stick for the time since the last frame; true while it is held.
    pub fn tick(&mut self, now: f64) -> bool {
        let (x, y) = self.stick;
        if x == 0.0 && y == 0.0 {
            return false;
        }
        let dt = (now - self.last).clamp(0.0, 0.05);
        self.last = now;
        if x != 0.0 {
            self.chart.m.scroll_by(chart::stick_rate(x as f64, 60.0) * dt);
        }
        if y != 0.0 {
            self.chart.m.zoom(1.8f64.powf(y.signum() as f64 * (y * y) as f64 * dt * 3.0));
        }
        self.dirty = true;
        true
    }

    /// The panel (`Panel relative` with ChartArea's class, e.g. `mt-[1rem] h-[19rem]`): the
    /// focusable box with the chart's rect inside its `px-[0.5rem] pt-[1rem]`, then the readout and
    /// chart mode's labels over it. `tag` is the box's focus tag.
    pub fn el(&self, ui: &mut Ui, st: S, tag: u32, focused: bool) -> El {
        let mut panel = ui.bx(st).bg(tok::L1).radius(ui.r(1.0)).name(PANEL);
        if self.mode.on {
            panel = panel.shadow(&[Shadow::new(0.0, 0.0, 0.0, 1.0, tok::BORDER_LIGHT)]);
        }
        let area = ui.bx(s().abs(Some(1.0), Some(0.5), Some(0.0), Some(0.5))).name(AREA).slot(SLOT);
        let boxn = fill_nav(ui.bx(s().fill()).radius(ui.r(0.5)).child(area), tag, Some("Control chart"), None, false);
        panel = panel.child(boxn);
        let mut over = ui.bx(s().fill()).look(Look::default().z(3));
        if let Some(i) = self.mode.cursor {
            let tz = self.tz.unwrap_or_else(|| chart::local_offset(self.chart.m.bars.get(i).map(|b| b.time).unwrap_or(0)));
            let words = readout_texts(&self.chart.m.bars, i, &*self.chart.fmt, self.mode.volume, tz);
            let mut row = ui.bx(s().abs(Some(0.4), None, None, Some(0.75)).row().wrap().items_end().gap_xy(0.9, 0.0));
            for (w, up) in words {
                let col = match up {
                    None => tok::FG,
                    Some(u) if w.ends_with('%') => {
                        if u {
                            tok::POS
                        } else {
                            tok::NEG
                        }
                    }
                    Some(_) => tok::MUTED,
                };
                row = row.child(ui.text(w, T::new(Ty::Label).mono().tab().c(col)));
            }
            over = over.child(row);
        }
        if self.mode.on {
            let mut row = ui.bx(s().abs(Some(0.4), Some(4.5), None, None).row().items_center().gap(0.75));
            let ind = self.mode.indicators();
            if !ind.is_empty() {
                let l: Vec<&str> = ind.iter().map(|k| k.label()).collect();
                row = row.child(ui.text(l.join(", "), T::new(Ty::Label).c(tok::MUTED)));
            }
            row = row.child(ui.text(if self.mode.ty == ChartType::Candles { "Candles" } else { "Line" }, T::new(Ty::Label).c(tok::MUTED)));
            let g = kit::glyph(ui, kit::Pb::B, kit::GSize::Sm);
            row = row.child(ui.bx(s().row().items_center().gap(0.35)).child(g).child(ui.text("Leave", T::new(Ty::Label).c(tok::MUTED))));
            over = over.child(row);
        } else {
            let g = kit::glyph(ui, kit::Pb::A, kit::GSize::Sm);
            // `opacity-0 [[data-area=chart]:focus_&]:opacity-100`: shows while the box has focus.
            let row = ui.bx(s().abs(Some(0.5), Some(4.5), None, None).row().items_center().gap(0.35)).opacity(if focused { 1.0 } else { 0.0 }).child(g).child(ui.text("Control chart", T::new(Ty::Label).c(tok::MUTED)));
            over = over.child(row);
        }
        panel.child(over)
    }

    /// Before the scene paints: lays the chart out for its rect (labels shaped, textures made).
    /// `area` is the node the view named `AREA`.
    pub fn prepare(&mut self, scene: &rare_render::Scene, area: Id, ts: &mut TextSys, tex: &mut dyn Textures) {
        let rr = scene.settled_rect(area);
        let (w, h) = (rr[2], rr[3]);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        if self.dirty || self.chart.frame().w != w || self.chart.frame().h != h {
            self.chart.prepare(w, h, ts, tex);
            self.dirty = false;
        }
    }

    /// From the scene's slot callback: pushes the chart at its slot, under the clip the scene
    /// gives it.
    pub fn paint_slot(&mut self, sp: &SlotPaint, ts: &mut TextSys, out: &mut DrawList) {
        if sp.slot == SLOT && self.chart.frame().w > 0.0 {
            self.chart.paint(sp.rect[0], sp.rect[1], sp.clip, sp.opacity, ts, out);
        }
    }
}

/// The tag the chart box carries in a page's focus specs.
pub const CHART_TAG: u32 = 9_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_enters_moves_and_leaves() {
        let mut m = ChartMode::default();
        let mut z = |_f: f64| {};
        assert_eq!(m.press(Pad::Left, false, 100, 1, 5, &mut z), ChartOut::Pass);
        assert_eq!(m.press(Pad::Select, false, 100, 1, 5, &mut z), ChartOut::Mode(true));
        assert_eq!(m.cursor, Some(99));
        m.press(Pad::Left, false, 100, 1, 5, &mut z);
        assert_eq!(m.cursor, Some(98));
        for _ in 0..12 {
            m.press(Pad::Left, true, 100, 1, 5, &mut z);
        }
        // 10 single steps, then 2 of 3.
        assert_eq!(m.cursor, Some(98 - 10 - 6));
        assert_eq!(m.press(Pad::Rt, false, 100, 1, 5, &mut z), ChartOut::Tf(2));
        assert_eq!(m.press(Pad::Lt, false, 100, 0, 5, &mut z), ChartOut::Took);
        m.press(Pad::Y, false, 100, 1, 5, &mut z);
        assert_eq!(m.ty, ChartType::Line);
        m.press(Pad::X, false, 100, 1, 5, &mut z);
        assert!(!m.volume);
        m.press(Pad::A, false, 100, 1, 5, &mut z);
        assert_eq!(m.indicators(), &[Indicator::Ma20]);
        assert_eq!(m.press(Pad::B, false, 100, 1, 5, &mut z), ChartOut::Mode(false));
        assert_eq!(m.cursor, None);
    }

    #[test]
    fn zoom_goes_to_the_chart() {
        let mut m = ChartMode::default();
        m.enter(10);
        let mut got = Vec::new();
        m.press(Pad::Up, false, 10, 0, 1, &mut |f| got.push(f));
        m.press(Pad::Down, false, 10, 0, 1, &mut |f| got.push(f));
        assert_eq!(got, vec![0.8, 1.25]);
    }

    #[test]
    fn readout_words() {
        let bars = vec![
            Candle { time: 1_792_505_100, open: 1.0, high: 1.1, low: 0.9, close: 1.02, volume: 24_123.0 },
            Candle { time: 1_792_506_000, open: 1.02, high: 1.05, low: 1.0, close: 1.0, volume: 0.0 },
        ];
        let f = |v: f64| format!("${v:.3}");
        let w = readout_texts(&bars, 0, &f, true, 0);
        assert_eq!(w[0].0, "Oct 20, 14:05");
        assert_eq!(w[1].0, "O $1.000");
        assert_eq!(w[5].0, "+2.00%");
        assert_eq!(w[6].0, "Vol 24,100");
        let w2 = readout_texts(&bars, 1, &f, false, 0);
        assert_eq!(w2[5].0, "-1.96%");
        assert_eq!(w2.len(), 6);
    }
}
