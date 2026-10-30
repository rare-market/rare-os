//! Native charts: the Trade screens' price chart (trade/Chart.tsx on lightweight-charts) and the
//! small lines (ui/charts.tsx `Spark`, `MiniCurve`).
//!
//! Geometry is computed without the GPU (`Model::frame`), in the box's own pixels, with
//! lightweight-charts' measures so the native chart lands on the web one: panes split by stretch
//! factor, a 1 px separator, a 28 px time axis, the price axis as wide as its widest label plus 22 px,
//! ticks at 1/2/5 steps no closer than 2.5 font heights, bar spacing = plot width / bars in view,
//! candle bodies from `optimalCandlestickWidth`, time marks picked by UTC weight (year, month, day,
//! 12h, 6h, 3h, 1h, 30m, 5m, 1m) at least `(font + 4) * 5` px apart and labelled in local time.
//!
//! Painting: candles, volume columns, grid, separators and label boxes are instanced rects (free);
//! the dotted price line and the dashed crosshair are 1 px mask textures made once per length;
//! polylines (the line series, MA 20, MA 50, VWAP, sparklines) are rasterised with tiny-skia into one
//! texture, again only when the lines themselves change (not when the crosshair moves). Text is the
//! engine's glyph atlas. `Chart::prepare` does everything that needs the text system or textures,
//! `Chart::paint` only pushes primitives, so it can run inside a scene paint.

use crate::color::Color;
use crate::gpu::{DrawList, Gpu, Prim, TexId, KIND_GLYPH, KIND_IMAGE, KIND_MASK, KIND_RECT};
use crate::text::{TextStyle, TextSys};
use crate::theme::tok;
use cosmic_text::Buffer;
use resvg::tiny_skia as sk;
use std::collections::HashMap;

// ---------------------------------------------------------------------------------------------
// Data and the pure maths (chartmode.ts and Chart.tsx's helpers)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Candle {
    /// Unix seconds.
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Bars by time, one per time, no gaps in the open price (Chart.tsx `cleanCandles`).
pub fn clean_candles(rows: &[Candle]) -> Vec<Candle> {
    let mut by: Vec<Candle> = Vec::with_capacity(rows.len());
    for r in rows {
        if r.close.is_finite() && r.close > 0.0 {
            by.push(*r);
        }
    }
    // Last row for a time wins, like the Map in the web version.
    by.reverse();
    by.sort_by_key(|c| c.time);
    by.dedup_by_key(|c| c.time);
    for i in 1..by.len() {
        if by[i].open.is_nan() || by[i].open <= 0.0 {
            by[i].open = by[i - 1].close;
        }
    }
    by
}

/// A flat range (one candle, or no trades in view) opened up around its price.
pub fn pad_flat_range(min: f64, max: f64, pad: f64) -> (f64, f64) {
    let mid = (min + max) / 2.0;
    let need = (mid.abs() * pad * 2.0).max(1e-12 * 100.0);
    if max - min >= need / 1e4 {
        return (min, max);
    }
    (mid - need / 2.0, mid + need / 2.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub from: f64,
    pub to: f64,
}

pub const OPEN_BARS: f64 = 150.0;
pub const RIGHT_PAD: f64 = 4.0;
pub const MIN_BARS: f64 = 12.0;

/// The range a chart opens on: the newest 150 bars and 4 empty ones on the right.
pub fn open_range(n: usize) -> Range {
    let n = n as f64;
    Range { from: (n - OPEN_BARS).max(-RIGHT_PAD), to: n - 1.0 + RIGHT_PAD }
}

pub fn clamp_index(i: f64, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    i.round().clamp(0.0, (n - 1) as f64) as usize
}

/// The range moved the least that shows bar `i` with `margin` bars to spare.
pub fn show_bar(r: Range, i: f64, margin: f64) -> Range {
    if i < r.from + margin {
        return Range { from: i - margin, to: r.to - (r.from - (i - margin)) };
    }
    if i > r.to - margin {
        return Range { from: r.from + (i + margin - r.to), to: i + margin };
    }
    r
}

/// Zoom about the right-hand end (the newest bars stay put): factor < 1 zooms in.
pub fn zoom_range(r: Range, factor: f64, n: usize) -> Range {
    let n = n as f64;
    let width = ((r.to - r.from) * factor).min(MIN_BARS.max(n + RIGHT_PAD * 2.0)).max(MIN_BARS);
    Range { from: r.to - width, to: r.to }
}

/// Slide the range by `bars`, never past the first bar or more than RIGHT_PAD past the last.
pub fn scroll_range(r: Range, bars: f64, n: usize) -> Range {
    let width = r.to - r.from;
    let from = (r.from + bars).min(n as f64 - 1.0 + RIGHT_PAD - width).max(-RIGHT_PAD);
    Range { from, to: from + width }
}

/// Bars per second to move for a stick deflection: slow near the centre, quick at the edge.
pub fn stick_rate(v: f64, max_bars_per_sec: f64) -> f64 {
    v.signum() * v * v * max_bars_per_sec
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readout {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub change_pct: f64,
}

/// What the readout over the chart says for one bar.
pub fn readout(b: &Candle, prev: Option<&Candle>) -> Readout {
    let r = prev.map(|p| p.close).unwrap_or(b.open);
    Readout { time: b.time, open: b.open, high: b.high, low: b.low, close: b.close, volume: b.volume, change_pct: if r > 0.0 { (b.close - r) / r * 100.0 } else { 0.0 } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChartType {
    Candles,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Indicator {
    Ma20,
    Ma50,
    Vwap,
}

impl Indicator {
    pub fn label(self) -> &'static str {
        match self {
            Indicator::Ma20 => "MA 20",
            Indicator::Ma50 => "MA 50",
            Indicator::Vwap => "VWAP",
        }
    }
}

/// A cycles through these: none, MA 20, MA 20 + MA 50, all three.
pub const INDICATOR_STEPS: [&[Indicator]; 4] = [&[], &[Indicator::Ma20], &[Indicator::Ma20, Indicator::Ma50], &[Indicator::Ma20, Indicator::Ma50, Indicator::Vwap]];

/// Simple moving average of the closes; `None` for the first n-1 bars.
pub fn sma(bars: &[Candle], n: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    let mut sum = 0.0;
    for i in 0..bars.len() {
        sum += bars[i].close;
        if i >= n {
            sum -= bars[i - n].close;
        }
        if i + 1 >= n {
            out[i] = Some(sum / n as f64);
        }
    }
    out
}

/// Volume weighted average price, restarting each UTC day.
pub fn vwap(bars: &[Candle]) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    let (mut day, mut pv, mut v) = (i64::MIN, 0.0, 0.0);
    for (i, b) in bars.iter().enumerate() {
        let d = b.time.div_euclid(86_400);
        if d != day {
            day = d;
            pv = 0.0;
            v = 0.0;
        }
        let typical = (b.high + b.low + b.close) / 3.0;
        pv += typical * b.volume;
        v += b.volume;
        if v > 0.0 {
            out[i] = Some(pv / v);
        }
    }
    out
}

pub fn indicator_line(k: Indicator, bars: &[Candle]) -> Vec<Option<f64>> {
    match k {
        Indicator::Ma20 => sma(bars, 20),
        Indicator::Ma50 => sma(bars, 50),
        Indicator::Vwap => vwap(bars),
    }
}

// ---------------------------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------------------------

/// Civil date and time of a unix second count (proleptic Gregorian, no time zone applied).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    /// 1..=12
    pub month: u32,
    /// 1..=31
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn civil(t: i64) -> Civil {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    // Howard Hinnant's days_from_civil, inverted.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    Civil { year, month, day, hour: (secs / 3600) as u32, minute: (secs / 60 % 60) as u32, second: (secs % 60) as u32 }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub fn month_name(m: u32) -> &'static str {
    MONTHS[(m.clamp(1, 12) - 1) as usize]
}

/// The local time zone's offset from UTC at `t`, in seconds (the web chart labels in local time).
pub fn local_offset(t: i64) -> i64 {
    #[cfg(unix)]
    unsafe {
        let tt = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&tt, &mut tm).is_null() {
            return tm.tm_gmtoff as i64;
        }
    }
    let _ = t;
    0
}

/// lightweight-charts' tick mark types, as the web's `tickMarkFormatter` gets them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Year,
    Month,
    Day,
    Time,
}

/// the Trade API's time axis labels: year, month, day, then hh:mm, in local time (`off` = UTC offset).
pub fn time_label(t: i64, kind: MarkKind, off: i64) -> String {
    let c = civil(t + off);
    match kind {
        MarkKind::Year => c.year.to_string(),
        MarkKind::Month => month_name(c.month).to_string(),
        MarkKind::Day => format!("{} {}", month_name(c.month), c.day),
        MarkKind::Time => format!("{:02}:{:02}", c.hour, c.minute),
    }
}

/// The crosshair's time label: lightweight-charts' default `dd MMM 'yy` and `HH:mm`, which it
/// formats in UTC (the web chart only localises the tick marks, so the two differ; kept for parity).
pub fn crosshair_time(t: i64) -> String {
    let c = civil(t);
    format!("{:02} {} '{:02}   {:02}:{:02}", c.day, month_name(c.month), c.year.rem_euclid(100), c.hour, c.minute)
}

/// ChartArea's readout time: "Oct 14, 07:00".
pub fn readout_time(t: i64, off: i64) -> String {
    let c = civil(t + off);
    format!("{} {}, {:02}:{:02}", month_name(c.month), c.day, c.hour, c.minute)
}

/// lightweight-charts' weight of a bar's time against the bar before it (computed in UTC).
pub fn mark_weight(t: i64, prev: Option<i64>) -> u8 {
    let Some(p) = prev else { return 70 };
    let (a, b) = (civil(t), civil(p));
    if a.year != b.year {
        70
    } else if a.month != b.month {
        60
    } else if a.day != b.day {
        50
    } else if a.hour / 12 != b.hour / 12 {
        33
    } else if a.hour / 6 != b.hour / 6 {
        32
    } else if a.hour / 3 != b.hour / 3 {
        31
    } else if a.hour != b.hour {
        30
    } else if a.minute / 30 != b.minute / 30 {
        22
    } else if a.minute / 5 != b.minute / 5 {
        21
    } else if a.minute != b.minute {
        20
    } else {
        10
    }
}

pub fn mark_kind(weight: u8) -> MarkKind {
    match weight {
        70.. => MarkKind::Year,
        60..=69 => MarkKind::Month,
        50..=59 => MarkKind::Day,
        _ => MarkKind::Time,
    }
}

/// The bars that get a time label: heaviest first, no two closer than `min_bars`.
pub fn time_marks(bars: &[Candle], lo: usize, hi: usize, min_bars: usize) -> Vec<(usize, MarkKind)> {
    if bars.is_empty() || lo > hi {
        return Vec::new();
    }
    let hi = hi.min(bars.len() - 1);
    let mut cand: Vec<(usize, u8)> = (lo..=hi).map(|i| (i, mark_weight(bars[i].time, i.checked_sub(1).map(|j| bars[j].time)))).collect();
    // Stable: equal weights keep time order, heavier ones are placed first.
    cand.sort_by_key(|c| std::cmp::Reverse(c.1));
    let mut taken: Vec<usize> = Vec::new();
    for (i, w) in cand {
        if taken.iter().all(|&t| t.abs_diff(i) >= min_bars) {
            taken.push(i);
            let _ = w;
        }
    }
    taken.sort_unstable();
    taken.into_iter().map(|i| (i, mark_kind(mark_weight(bars[i].time, i.checked_sub(1).map(|j| bars[j].time))))).collect()
}

// ---------------------------------------------------------------------------------------------
// Scales
// ---------------------------------------------------------------------------------------------

fn ge(a: f64, b: f64) -> bool {
    a - b >= -1e-14
}

/// lightweight-charts' `PriceTickSpanCalculator.tickSpan` for one divider order.
fn span_with(high: f64, low: f64, max_span: f64, min_move: f64, dividers: [f64; 3]) -> f64 {
    let mut span = 10f64.powf((high - low).log10().ceil().max(0.0));
    let mut i = 0;
    let mut c = dividers[0];
    loop {
        let go = ge(span, min_move) && span > min_move + 1e-14 && ge(span, max_span * c) && ge(span, 1.0);
        if !go {
            break;
        }
        span /= c;
        i += 1;
        c = dividers[i % 3];
    }
    if span <= min_move + 1e-14 {
        span = min_move;
    }
    span = span.max(1.0);
    // A decimal base (every minMove here is 10^-n) steps below 1 by 2, 2.5, 2.
    if (span - 1.0).abs() < 1e-14 {
        let frac = [2.0, 2.5, 2.0];
        let mut i = 0;
        let mut c = frac[0];
        while ge(span, max_span * c) && span > min_move + 1e-14 {
            span /= c;
            i += 1;
            c = frac[i % 3];
        }
    }
    span
}

/// The tick spacing for a scale showing `low..high` (PriceTickMarkBuilder.tickSpan): the smallest of
/// three divider orders, so steps run 1, 2, 2.5, 4, 5 times a power of ten.
pub fn tick_span(high: f64, low: f64, max_span: f64, min_move: f64) -> f64 {
    [[2.0, 2.5, 2.0], [2.0, 2.0, 2.5], [2.5, 2.0, 2.0]].iter().map(|d| span_with(high, low, max_span, min_move, *d)).fold(f64::INFINITY, f64::min)
}

/// The price marks of a scale (PriceTickMarkBuilder.rebuildTickMarks): marks no closer than
/// ceil(2.5 font heights), from the top down, only where they land inside the pane.
pub fn price_marks(s: &Scale, font: f32, min_move: f64) -> Vec<f64> {
    if !(s.hi > s.lo) || s.h <= 1.0 {
        return Vec::new();
    }
    let (high, low) = (s.price_at(0.0), s.price_at(s.h - 1.0));
    if !(high > low) {
        return Vec::new();
    }
    let mark_h = (font * 2.5).ceil() as f64;
    let span = tick_span(high, low, (high - low) * mark_h / s.h as f64, min_move);
    let mut m = high % span;
    if m < 0.0 {
        m += span;
    }
    let mut out = Vec::new();
    let mut v = high - m;
    while v > low && out.len() < 64 {
        let y = s.y_of(v) - s.y;
        if y >= 0.0 && y <= s.h - 1.0 {
            out.push((v / span).round() * span);
        }
        v -= span;
    }
    out
}

/// A vertical price scale: data `lo..hi` inside `y..y+h` with margins as fractions of `h`
/// (lightweight-charts' PriceScale: coordinates run over `h - margins - 1` pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub lo: f64,
    pub hi: f64,
    pub y: f32,
    pub h: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Scale {
    fn inner(&self) -> f64 {
        ((self.h * (1.0 - self.top - self.bottom)) as f64 - 1.0).max(1.0)
    }
    /// The pane coordinate of a price (window y: plus `self.y`).
    pub fn y_of(&self, p: f64) -> f32 {
        let mb = (self.bottom * self.h) as f64;
        let inv = mb + self.inner() * (p - self.lo) / (self.hi - self.lo);
        self.y + (self.h as f64 - 1.0 - inv) as f32
    }
    /// The price at pane coordinate `y` (0 = the pane's top).
    pub fn price_at(&self, y: f32) -> f64 {
        let mb = (self.bottom * self.h) as f64;
        let inv = self.h as f64 - 1.0 - y as f64;
        self.lo + (inv - mb) / self.inner() * (self.hi - self.lo)
    }
}

/// lightweight-charts' `optimalCandlestickWidth`, then made odd so the 1 px wick sits centred.
pub fn candle_width(spacing: f32, dpr: f32) -> f32 {
    let w = if (2.5..=4.0).contains(&spacing) {
        (3.0 * dpr).floor()
    } else {
        let coeff = 1.0 - 0.2 * (spacing.max(4.0) - 4.0).atan() / std::f32::consts::FRAC_PI_2;
        (spacing * coeff * dpr).floor().min((spacing * dpr).floor()).max(dpr.floor())
    };
    let wick = dpr.floor().max(1.0);
    if w >= 2.0 && (wick as i32 % 2) != (w as i32 % 2) {
        w - 1.0
    } else {
        w
    }
}

/// lightweight-charts' histogram column: the bar spacing less a 1 px gap.
pub fn column_width(spacing: f32, dpr: f32) -> f32 {
    let gap = if (spacing * dpr).ceil() <= 1.0 { 0.0 } else { dpr.floor().max(1.0) };
    ((spacing * dpr).round() - gap).max(1.0)
}

// ---------------------------------------------------------------------------------------------
// Small lines (ui/charts.tsx)
// ---------------------------------------------------------------------------------------------

/// Points placed by time across the width and by value down the height; None under two points.
pub fn line_geometry(points: &[(f64, f64)], w: f32, h: f32, pad_y: f32) -> Option<Vec<[f32; 2]>> {
    if points.len() < 2 || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let (t0, t1) = (points[0].0, points[points.len() - 1].0);
    let mut lo = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let mut hi = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    if hi == lo {
        hi += 1.0;
        lo -= 1.0;
    }
    let plot_h = (h - pad_y * 2.0).max(1.0) as f64;
    let r1 = |v: f64| ((v * 10.0).round() / 10.0) as f32;
    Some(
        points
            .iter()
            .map(|&(t, v)| {
                let x = if t1 == t0 { w as f64 } else { (t - t0) / (t1 - t0) * w as f64 };
                [r1(x), r1(pad_y as f64 + (hi - v) / (hi - lo) * plot_h)]
            })
            .collect(),
    )
}

/// Direction of a series: the last point against the first.
pub fn rising(points: &[(f64, f64)]) -> bool {
    points.last().map(|p| p.1).unwrap_or(0.0) >= points.first().map(|p| p.1).unwrap_or(0.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    pub pts: Vec<[f32; 2]>,
    pub color: Color,
    pub width: f32,
}

/// A soft area under a line, `alpha_top` at the top fading to nothing at `floor`.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub pts: Vec<[f32; 2]>,
    pub floor: f32,
    pub color: Color,
    pub alpha_top: f32,
}

/// Rasterises lines (round joins and caps, anti-aliased) into premultiplied RGBA, `w` x `h` px.
/// Coordinates are in px of the pixmap.
pub fn raster_lines(w: u32, h: u32, areas: &[Area], lines: &[Polyline], dots: &[([f32; 2], f32, Color)]) -> Vec<u8> {
    let Some(mut pm) = sk::Pixmap::new(w.max(1), h.max(1)) else { return vec![0; (w.max(1) * h.max(1) * 4) as usize] };
    let c8 = |c: Color, a: f32| sk::Color::from_rgba8((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8, (c.a * a * 255.0).round().clamp(0.0, 255.0) as u8);
    for a in areas {
        if a.pts.len() < 2 {
            continue;
        }
        let mut pb = sk::PathBuilder::new();
        pb.move_to(a.pts[0][0], a.pts[0][1]);
        for p in &a.pts[1..] {
            pb.line_to(p[0], p[1]);
        }
        pb.line_to(a.pts[a.pts.len() - 1][0], a.floor);
        pb.line_to(a.pts[0][0], a.floor);
        pb.close();
        let Some(path) = pb.finish() else { continue };
        let top = a.pts.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let stops = vec![sk::GradientStop::new(0.0, c8(a.color, a.alpha_top)), sk::GradientStop::new(1.0, c8(a.color, 0.0))];
        // The SVG gradient runs over the path's bounding box (objectBoundingBox): top of the line to the floor.
        let Some(shader) = sk::LinearGradient::new(sk::Point::from_xy(0.0, top), sk::Point::from_xy(0.0, a.floor.max(top + 1.0)), stops, sk::SpreadMode::Pad, sk::Transform::identity()) else { continue };
        let paint = sk::Paint { shader, anti_alias: true, ..Default::default() };
        pm.fill_path(&path, &paint, sk::FillRule::Winding, sk::Transform::identity(), None);
    }
    for l in lines {
        if l.pts.len() < 2 {
            continue;
        }
        let mut pb = sk::PathBuilder::new();
        pb.move_to(l.pts[0][0], l.pts[0][1]);
        for p in &l.pts[1..] {
            pb.line_to(p[0], p[1]);
        }
        let Some(path) = pb.finish() else { continue };
        let mut paint = sk::Paint::default();
        paint.set_color(c8(l.color, 1.0));
        paint.anti_alias = true;
        let stroke = sk::Stroke { width: l.width, line_join: sk::LineJoin::Round, line_cap: sk::LineCap::Round, ..Default::default() };
        pm.stroke_path(&path, &paint, &stroke, sk::Transform::identity(), None);
    }
    for &(c, r, col) in dots {
        if let Some(path) = sk::PathBuilder::from_circle(c[0], c[1], r) {
            let mut paint = sk::Paint::default();
            paint.set_color(c8(col, 1.0));
            paint.anti_alias = true;
            pm.fill_path(&path, &paint, sk::FillRule::Winding, sk::Transform::identity(), None);
        }
    }
    pm.take()
}

// ---------------------------------------------------------------------------------------------
// The look
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub up: Color,
    pub down: Color,
    /// Axis text, crosshair and the price line (`--text-secondary`).
    pub dim: Color,
    /// Crosshair label boxes (`--layer-04`).
    pub raised: Color,
    /// Pane separator and time axis border: rgba(203,208,235,0.08).
    pub hair: Color,
    /// Horizontal grid: rgba(203,208,235,0.045).
    pub grid: Color,
    pub ma20: Color,
    pub ma50: Color,
    pub vwap: Color,
    /// Axis font size in px (11, not rem-scaled: the web chart sets 11 px too).
    pub font: f32,
    pub dpr: f32,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            up: tok::POS,
            down: tok::NEG,
            dim: tok::MUTED,
            raised: tok::L4,
            hair: Color::rgb(203, 208, 235).alpha(0.08),
            grid: Color::rgb(203, 208, 235).alpha(0.045),
            ma20: Color::rgb(0xf5, 0xc5, 0x42),
            ma50: Color::rgb(0x5b, 0x8c, 0xff),
            vwap: Color::rgb(0xc7, 0x7d, 0xff),
            font: 11.0,
            dpr: 1.0,
        }
    }
}

impl Look {
    pub fn indicator(&self, k: Indicator) -> Color {
        match k {
            Indicator::Ma20 => self.ma20,
            Indicator::Ma50 => self.ma50,
            Indicator::Vwap => self.vwap,
        }
    }
    /// The time axis: border, tick, font, 3/12 font above and below, 4/12 font under the labels.
    pub fn axis_h(&self) -> f32 {
        (1.0 + 5.0 + self.font * (1.0 + 6.0 / 12.0 + 4.0 / 12.0)).ceil()
    }
    /// A price label box: the font plus 2.5/12 font above and below, odd (it centres on a 1 px tick).
    pub fn label_h(&self) -> f32 {
        let h = (self.font * (1.0 + 5.0 / 12.0)).round();
        if h as i32 % 2 == 0 {
            h + 1.0
        } else {
            h
        }
    }
    /// The price axis' inner and outer padding (5/12 font each).
    pub fn pad(&self) -> f32 {
        self.font / 12.0 * 5.0
    }
}

/// lightweight-charts' label text colour on a filled box: black on light, white on dark.
pub fn contrast(bg: Color) -> Color {
    let g = 0.199 * bg.r * 255.0 + 0.687 * bg.g * 255.0 + 0.114 * bg.b * 255.0;
    if g > 160.0 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

// ---------------------------------------------------------------------------------------------
// One frame's geometry
// ---------------------------------------------------------------------------------------------

/// Clip rectangles as [x0, y0, x1, y1] in the box's pixels.
pub type Clip = [f32; 4];

#[derive(Clone, Debug, PartialEq)]
pub struct RectShape {
    pub r: [f32; 4],
    pub color: Color,
    pub radius: f32,
    pub clip: Clip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dash {
    /// 1 on, 1 off (LineStyle.Dotted, the last price line).
    Dotted,
    /// 6 on, 6 off (LineStyle.LargeDashed, the crosshair).
    Large,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DashShape {
    pub x: f32,
    pub y: f32,
    pub len: f32,
    pub vertical: bool,
    pub dash: Dash,
    pub color: Color,
    pub clip: Clip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// x is the left edge.
    Left,
    /// x is the centre.
    Center,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: f32,
    /// Vertical centre of the text line.
    pub y: f32,
    pub anchor: Anchor,
    pub color: Color,
    pub clip: Clip,
    /// Drawn over the label boxes (the last value and crosshair labels); tick labels are under them.
    pub top: bool,
}

/// Everything one frame of the chart draws, in paint order within each list: rects (grid,
/// separators, volume, candles), then the line texture over `plot`, then dashes, then `over`
/// rects (label boxes, the marker), then labels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub w: f32,
    pub h: f32,
    pub rects: Vec<RectShape>,
    /// The main pane's plot area (where `lines` are drawn): [x, y, w, h].
    pub plot: [f32; 4],
    pub lines: Vec<Polyline>,
    pub dots: Vec<([f32; 2], f32, Color)>,
    pub dashes: Vec<DashShape>,
    pub over: Vec<RectShape>,
    pub labels: Vec<Label>,
    /// Derived numbers, for tests and the screens.
    pub spacing: f32,
    pub axis_w: f32,
    pub main: Option<Scale>,
    pub vol: Option<Scale>,
    pub vol_h: f32,
}

/// The chart's state: what the pad and the data change. Pure; no GPU, no fonts.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub bars: Vec<Candle>,
    pub range: Range,
    pub ty: ChartType,
    pub volume: bool,
    pub indicators: Vec<Indicator>,
    /// The bar the crosshair sits on; None hides it.
    pub cursor: Option<usize>,
    /// Height the volume pane's stretch factor is computed from, beyond the chart's own height
    /// (Chart.tsx reads the box's clientHeight, which includes its 1 rem top padding).
    pub extra_h: f32,
    /// UTC offset for labels (seconds); None asks the system at each label.
    pub tz: Option<i64>,
    fitted: String,
}

impl Default for Model {
    fn default() -> Model {
        Model { bars: Vec::new(), range: open_range(0), ty: ChartType::Candles, volume: true, indicators: Vec::new(), cursor: None, extra_h: 0.0, tz: None, fitted: String::new() }
    }
}

pub type Fmt = dyn Fn(f64) -> String;

impl Model {
    /// New rows; the range opens on the newest bars when `fit_key` changes (new coin or timeframe).
    pub fn set_rows(&mut self, rows: &[Candle], fit_key: &str) {
        self.bars = clean_candles(rows);
        if !self.bars.is_empty() && self.fitted != fit_key {
            self.fitted = fit_key.to_string();
            self.range = open_range(self.bars.len());
        }
        if let Some(c) = self.cursor {
            self.cursor = Some(c.min(self.bars.len().saturating_sub(1)));
        }
    }

    /// The fit key the range last opened on.
    pub fn fitted(&self) -> &str {
        &self.fitted
    }

    pub fn scroll_by(&mut self, bars: f64) {
        self.range = scroll_range(self.range, bars, self.bars.len());
    }

    pub fn zoom(&mut self, factor: f64) {
        self.range = zoom_range(self.range, factor, self.bars.len());
    }

    /// Puts the crosshair on bar `i` (clamped) and scrolls it into view; None hides it.
    pub fn set_cursor(&mut self, i: Option<f64>) {
        match i {
            Some(i) if !self.bars.is_empty() => {
                let i = clamp_index(i, self.bars.len());
                self.cursor = Some(i);
                self.range = show_bar(self.range, i as f64, 2.0);
            }
            _ => self.cursor = None,
        }
    }

    fn off(&self, t: i64) -> i64 {
        self.tz.unwrap_or_else(|| local_offset(t))
    }

    fn visible(&self) -> (usize, usize) {
        let n = self.bars.len();
        if n == 0 {
            return (1, 0);
        }
        let lo = self.range.from.floor().max(0.0) as usize;
        let hi = (self.range.to.ceil().max(0.0) as usize).min(n - 1);
        (lo, hi)
    }

    /// The frame for a `w` x `h` box. `measure` gives a label's width in px.
    pub fn frame(&self, look: &Look, w: f32, h: f32, fmt: &Fmt, fmt_vol: &Fmt, measure: &mut dyn FnMut(&str) -> f32) -> Frame {
        let mut f = Frame { w, h, ..Default::default() };
        let box_clip = [0.0, 0.0, w, h];
        let font = look.font;
        let axis_h = look.axis_h();
        let n = self.bars.len();
        let (lo_i, hi_i) = self.visible();
        let ind_lines: Vec<(Indicator, Vec<Option<f64>>)> = self.indicators.iter().map(|&k| (k, indicator_line(k, &self.bars))).collect();

        // Price range in view (all series on the right scale, as autoscale does).
        let (mut plo, mut phi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut vmax: f64 = 0.0;
        for i in lo_i..=hi_i.min(n.saturating_sub(1)) {
            if n == 0 {
                break;
            }
            let b = &self.bars[i];
            match self.ty {
                ChartType::Candles => {
                    plo = plo.min(b.low);
                    phi = phi.max(b.high);
                }
                ChartType::Line => {
                    plo = plo.min(b.close);
                    phi = phi.max(b.close);
                }
            }
            for (_, l) in &ind_lines {
                if let Some(v) = l[i] {
                    plo = plo.min(v);
                    phi = phi.max(v);
                }
            }
            vmax = vmax.max(b.volume);
        }
        let has_data = plo.is_finite() && phi.is_finite();
        if has_data {
            (plo, phi) = pad_flat_range(plo, phi, 0.01);
        }

        // Panes: price over volume by stretch factor, a 1 px separator, the time axis takes the rest.
        let full = h + self.extra_h;
        let v_stretch = if self.volume { (full * 0.18).clamp(48.0, 96.0).round() } else { 1.0 };
        let avail = (h - axis_h - 1.0).max(2.0);
        let main_h = (avail * (full - v_stretch) / full.max(1.0)).floor();
        let vol_h = (avail * v_stretch / full.max(1.0)).floor();
        let sep_y = main_h;
        let vol_y = main_h + 1.0;
        let axis_y = vol_y + vol_h;
        f.vol_h = vol_h;

        let main = Scale { lo: plo, hi: phi, y: 0.0, h: main_h, top: 0.08, bottom: 0.06 };
        let (vlo, vhi) = pad_flat_range(0.0, vmax, 0.01);
        let vol = Scale { lo: vlo, hi: vhi, y: vol_y, h: vol_h, top: 0.2, bottom: 0.0 };

        // Marks and the price axis width: the widest of the first and last mark, the last value and
        // the crosshair's widest label, plus border, tick, both paddings and 5 px; made even.
        let main_marks = if has_data { price_marks(&main, font, 1e-12) } else { Vec::new() };
        let vol_marks = if has_data && self.volume && vmax > 0.0 { price_marks(&vol, font, 0.01) } else { Vec::new() };
        let main_txt: Vec<String> = main_marks.iter().map(|&p| fmt(p)).collect();
        let vol_txt: Vec<String> = vol_marks.iter().map(|&v| fmt_vol(v)).collect();
        let last = if n > 0 { Some(self.bars[n - 1]) } else { None };
        let last_txt = last.map(|b| fmt(b.close));
        let mut widest: f32 = 0.0;
        let ends = |v: &Vec<String>| v.first().into_iter().chain(v.last()).cloned().collect::<Vec<String>>();
        let mut probe: Vec<String> = ends(&main_txt).into_iter().chain(ends(&vol_txt)).chain(last_txt.clone()).collect();
        if has_data {
            let (top, bottom) = (main.price_at(1.0), main.price_at(main_h - 2.0));
            probe.push(fmt(top.min(bottom).floor() + 0.111_111_111_111_11));
            probe.push(fmt(top.max(bottom).ceil() - 0.111_111_111_111_11));
        }
        for t in &probe {
            widest = widest.max(measure(t));
        }
        let pad = look.pad();
        let axis_w = if has_data {
            let w = (1.0 + 5.0 + 2.0 * pad + 5.0 + widest).ceil();
            w + (w as i32 % 2) as f32
        } else {
            0.0
        };
        let plot_w = (w - axis_w).max(1.0);
        let axis_x = plot_w;
        f.axis_w = axis_w;
        let bars_in_view = (self.range.to - self.range.from + 1.0).max(1.0);
        let spacing = plot_w / bars_in_view as f32;
        f.spacing = spacing;
        // lightweight-charts' indexToCoordinate: width - (right - i + 0.5) * spacing - 1.
        let x_of = |i: f64| ((i - self.range.from + 0.5) as f32) * spacing - 1.0;
        let plot_clip = [0.0, 0.0, plot_w, main_h];
        let vol_clip = [0.0, vol_y, plot_w, vol_y + vol_h];
        f.plot = [0.0, 0.0, plot_w, main_h];

        // Grid (price marks only; vertical grid is off) and separators.
        for &p in &main_marks {
            let y = main.y_of(p).round();
            f.rects.push(RectShape { r: [0.0, y, plot_w, 1.0], color: look.grid, radius: 0.0, clip: plot_clip });
        }
        for &v in &vol_marks {
            let y = vol.y_of(v).round();
            f.rects.push(RectShape { r: [0.0, y, plot_w, 1.0], color: look.grid, radius: 0.0, clip: vol_clip });
        }
        f.rects.push(RectShape { r: [0.0, sep_y, w, 1.0], color: look.hair, radius: 0.0, clip: box_clip });
        f.rects.push(RectShape { r: [0.0, axis_y, plot_w + 1.0, 1.0], color: look.hair, radius: 0.0, clip: box_clip });

        if has_data {
            let cw = candle_width(spacing, look.dpr);
            let vw = column_width(spacing, look.dpr);
            let wick = look.dpr.floor().max(1.0);
            for i in lo_i..=hi_i {
                let b = &self.bars[i];
                let xc = x_of(i as f64).round();
                let col = if b.close >= b.open { look.up } else { look.down };
                if self.volume && b.volume > 0.0 {
                    let top = vol.y_of(b.volume).round();
                    let bottom = vol.y_of(0.0).round();
                    // The column runs from its value down through the 1 px base line (PaneRendererHistogram).
                    f.rects.push(RectShape { r: [xc - (vw / 2.0).floor(), top, vw, (bottom - top + 1.0).max(1.0)], color: col.alpha(0.45), radius: 0.0, clip: vol_clip });
                }
                if self.ty == ChartType::Candles {
                    let (yh, yl) = (main.y_of(b.high).round(), main.y_of(b.low).round());
                    let (yo, yc) = (main.y_of(b.open).round(), main.y_of(b.close).round());
                    f.rects.push(RectShape { r: [xc - (wick / 2.0).floor(), yh, wick, (yl - yh + 1.0).max(1.0)], color: col, radius: 0.0, clip: plot_clip });
                    let top = yo.min(yc);
                    let bh = (yo.max(yc) - top + 1.0).max(1.0);
                    f.rects.push(RectShape { r: [xc - (cw / 2.0).floor(), top, cw, bh], color: col, radius: 0.0, clip: plot_clip });
                }
            }

            // Lines: the line series and the indicators, from one bar outside the view on each side.
            let a = lo_i.saturating_sub(1);
            let z = (hi_i + 1).min(n - 1);
            if self.ty == ChartType::Line {
                let pts = (a..=z).map(|i| [x_of(i as f64), main.y_of(self.bars[i].close)]).collect();
                f.lines.push(Polyline { pts, color: look.up, width: 2.0 * look.dpr });
            }
            for (k, l) in &ind_lines {
                let pts: Vec<[f32; 2]> = (a..=z).filter_map(|i| l[i].map(|v| [x_of(i as f64), main.y_of(v)])).collect();
                if pts.len() >= 2 {
                    f.lines.push(Polyline { pts, color: look.indicator(*k), width: look.dpr });
                }
            }

            // The last price: a dotted line across the plot and its label on the axis.
            if let (Some(b), Some(_)) = (last, last_txt.as_ref()) {
                let y = main.y_of(b.close).round();
                if y >= 0.0 && y < main_h {
                    let lc = look.dim.alpha(0.7);
                    f.dashes.push(DashShape { x: 0.0, y, len: plot_w, vertical: false, dash: Dash::Dotted, color: lc, clip: plot_clip });
                }
            }

            // Axis labels.
            let label_clip_main = [axis_x, 0.0, w, main_h];
            let label_clip_vol = [axis_x, vol_y, w, vol_y + vol_h];
            let tick_x = axis_x + 5.0 + pad;
            let box_x = axis_x + 1.0 + 5.0 + pad;
            for (p, t) in main_marks.iter().zip(main_txt) {
                f.labels.push(Label { text: t, x: tick_x, y: main.y_of(*p).round() + 0.5, anchor: Anchor::Left, color: look.dim, clip: label_clip_main, top: false });
            }
            for (v, t) in vol_marks.iter().zip(vol_txt) {
                if !t.is_empty() {
                    f.labels.push(Label { text: t, x: tick_x, y: vol.y_of(*v).round() + 0.5, anchor: Anchor::Left, color: look.dim, clip: label_clip_vol, top: false });
                }
            }
            let min_bars = (((font + 4.0) * 5.0) / spacing).ceil().max(1.0) as usize;
            let label_y = axis_y + 1.0 + 5.0 + font * 3.0 / 12.0 + font / 2.0;
            let lh = look.label_h();
            // A price label box: border, both paddings, the text, the tick; centred on its price.
            let price_box = |y: f32, tw: f32| -> [f32; 4] { [axis_x, (y + 0.5 - lh / 2.0).floor(), (1.0 + 2.0 * pad + tw.ceil() + 5.0).round(), lh] };
            // Marks are picked with the bars just off the left edge in play, so a mark there keeps
            // its neighbours out as it does on the web.
            for (i, kind) in time_marks(&self.bars, lo_i.saturating_sub(min_bars * 2), hi_i, min_bars) {
                if i < lo_i {
                    continue;
                }
                let t = self.bars[i].time;
                let text = time_label(t, kind, self.off(t));
                let tw = measure(&text);
                let x = x_of(i as f64).round();
                if x < 0.0 || x > plot_w {
                    continue;
                }
                // Labels near the edges are pulled in, as lightweight-charts does.
                let x = x.clamp(tw / 2.0, (plot_w - tw / 2.0).max(tw / 2.0));
                f.labels.push(Label { text, x, y: label_y, anchor: Anchor::Center, color: look.dim, clip: [0.0, axis_y + 1.0, plot_w + 1.0, h], top: false });
            }

            // Last value label (lightweight-charts colours it with the price line colour).
            if let (Some(b), Some(t)) = (last, last_txt) {
                let y = main.y_of(b.close).round();
                if y >= 0.0 && y < main_h {
                    let bg = look.dim;
                    let r = price_box(y, measure(&t));
                    f.over.push(RectShape { r, color: bg, radius: 2.0, clip: label_clip_main });
                    f.labels.push(Label { text: t, x: box_x, y: y + 0.5, anchor: Anchor::Left, color: contrast(bg), clip: label_clip_main, top: true });
                }
            }

            // Crosshair on the cursor bar: dashed lines, the marker on a line, labels on both axes.
            if let Some(ci) = self.cursor.filter(|&c| c < n) {
                let b = &self.bars[ci];
                let x = x_of(ci as f64).round();
                let y = main.y_of(b.close).round();
                let cc = look.dim.alpha(0.35);
                if x >= 0.0 && x < plot_w {
                    f.dashes.push(DashShape { x, y: 0.0, len: axis_y, vertical: true, dash: Dash::Large, color: cc, clip: [0.0, 0.0, plot_w, axis_y] });
                }
                if y >= 0.0 && y < main_h {
                    f.dashes.push(DashShape { x: 0.0, y, len: plot_w, vertical: false, dash: Dash::Large, color: cc, clip: plot_clip });
                    if self.ty == ChartType::Line {
                        f.dots.push(([x, y + 0.5], 4.0 * look.dpr, look.up));
                    }
                    let t = fmt(b.close);
                    let r = price_box(y, measure(&t));
                    f.over.push(RectShape { r, color: look.raised, radius: 2.0, clip: label_clip_main });
                    f.labels.push(Label { text: t, x: box_x, y: y + 0.5, anchor: Anchor::Left, color: contrast(look.raised), clip: label_clip_main, top: true });
                }
                if x >= 0.0 && x < plot_w {
                    let t = crosshair_time(b.time);
                    let tw = measure(&t);
                    // Text plus 9/12 font each side, clamped into the scale, the axis' full height.
                    let bw = tw.round() + 2.0 * font * 9.0 / 12.0;
                    let bx = (x - bw / 2.0).floor().clamp(0.0, (plot_w - bw).max(0.0));
                    f.over.push(RectShape { r: [bx, axis_y, bw, axis_h], color: look.raised, radius: 0.0, clip: [0.0, axis_y, plot_w + 1.0, h] });
                    f.labels.push(Label { text: t, x: bx + bw / 2.0, y: label_y, anchor: Anchor::Center, color: contrast(look.raised), clip: [0.0, axis_y + 1.0, plot_w + 1.0, h], top: true });
                }
            }
        }
        f.main = has_data.then_some(main);
        f.vol = (has_data && self.volume).then_some(vol);
        f
    }
}

// ---------------------------------------------------------------------------------------------
// Textures and painting
// ---------------------------------------------------------------------------------------------

/// Where the chart's textures live (the renderer, or a headless harness).
pub trait Textures {
    fn create(&mut self, w: u32, h: u32, rgba: &[u8]) -> TexId;
    fn free(&mut self, id: TexId);
}

impl Textures for Gpu {
    fn create(&mut self, w: u32, h: u32, rgba: &[u8]) -> TexId {
        self.create_texture_rgba(w, h, rgba)
    }
    fn free(&mut self, id: TexId) {
        self.free_texture(id)
    }
}

/// A 1 px dash pattern as a white mask texture, `len` px long.
pub fn dash_mask(dash: Dash, len: u32) -> Vec<u8> {
    let (on, off) = match dash {
        Dash::Dotted => (1, 1),
        Dash::Large => (6, 6),
    };
    let mut px = vec![0u8; len as usize * 4];
    for i in 0..len as usize {
        if i % (on + off) < on {
            px[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    px
}

struct Tex {
    id: TexId,
    w: u32,
    h: u32,
}

struct LabelBuf {
    buf: Buffer,
    w: f32,
    used: u64,
}

/// A chart on screen: the model, its look, its formatters and its GPU leftovers.
pub struct Chart {
    pub m: Model,
    pub look: Look,
    pub fmt: Box<Fmt>,
    pub fmt_vol: Box<Fmt>,
    frame: Frame,
    lines_key: u64,
    lines_tex: Option<Tex>,
    dashes: HashMap<(Dash, bool, u32), Tex>,
    labels: HashMap<String, LabelBuf>,
    gen: u64,
}

/// `$1,234.56` style default formatter (screens pass their own).
pub fn default_price(v: f64) -> String {
    let decimals = if v.abs() >= 1.0 || v == 0.0 { 2 } else { (2.0 - v.abs().log10().floor()).clamp(2.0, 12.0) as usize };
    let s = format!("{:.*}", decimals, v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut g = String::new();
    for (k, ch) in int.chars().enumerate() {
        if k > 0 && (int.len() - k) % 3 == 0 {
            g.push(',');
        }
        g.push(ch);
    }
    format!("{}${}{}{}", if v < 0.0 { "-" } else { "" }, g, if frac.is_empty() { "" } else { "." }, frac)
}

/// trade/format.ts `count`: 1.2K, 3.4M, 5.6B.
pub fn default_count(v: f64) -> String {
    if v.is_nan() || v <= 0.0 {
        return String::new();
    }
    let (d, s) = if v >= 1e9 { (1e9, "B") } else if v >= 1e6 { (1e6, "M") } else if v >= 1e3 { (1e3, "K") } else { (1.0, "") };
    let x = v / d;
    let t = if x >= 100.0 || s.is_empty() && x.fract() == 0.0 { format!("{x:.0}") } else if x >= 10.0 { format!("{x:.1}") } else { format!("{x:.2}") };
    let t = if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t };
    format!("{t}{s}")
}

impl Default for Chart {
    fn default() -> Chart {
        Chart::new(Look::default(), Box::new(default_price), Box::new(default_count))
    }
}

fn hash_lines(lines: &[Polyline], dots: &[([f32; 2], f32, Color)], w: u32, h: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut s = std::collections::hash_map::DefaultHasher::new();
    (w, h).hash(&mut s);
    for l in lines {
        l.width.to_bits().hash(&mut s);
        l.color.arr().map(f32::to_bits).hash(&mut s);
        for p in &l.pts {
            p[0].to_bits().hash(&mut s);
            p[1].to_bits().hash(&mut s);
        }
    }
    for d in dots {
        d.0[0].to_bits().hash(&mut s);
        d.0[1].to_bits().hash(&mut s);
        d.1.to_bits().hash(&mut s);
    }
    s.finish()
}

impl Chart {
    pub fn new(look: Look, fmt: Box<Fmt>, fmt_vol: Box<Fmt>) -> Chart {
        Chart { m: Model::default(), look, fmt, fmt_vol, frame: Frame::default(), lines_key: 0, lines_tex: None, dashes: HashMap::new(), labels: HashMap::new(), gen: 0 }
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    fn style(&self) -> TextStyle {
        TextStyle::new(self.look.font * self.look.dpr, 400)
    }

    /// Lays the chart out for a `w` x `h` box: labels shaped, the line texture redrawn if the lines
    /// changed, dash masks made. Call when the model or the size changed, before `paint`.
    pub fn prepare(&mut self, w: f32, h: f32, ts: &mut TextSys, tex: &mut dyn Textures) {
        self.gen += 1;
        let gen = self.gen;
        let st = self.style();
        let labels = &mut self.labels;
        let mut measure = |t: &str| -> f32 {
            if let Some(l) = labels.get_mut(t) {
                l.used = gen;
                return l.w;
            }
            let mut buf = ts.buffer(t, &st);
            ts.measure(&mut buf, None);
            // The unrounded advance, so centred labels land where the web centres them.
            let w = buf.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
            labels.insert(t.to_string(), LabelBuf { buf, w, used: gen });
            w
        };
        let frame = self.m.frame(&self.look, w, h, &*self.fmt, &*self.fmt_vol, &mut measure);
        // Every label paints from a shaped buffer, not only the ones the axis width was measured on.
        for l in &frame.labels {
            measure(&l.text);
        }
        // Drop label buffers not used for a while (time labels change as the view scrolls).
        if self.labels.len() > 256 {
            self.labels.retain(|_, l| gen - l.used < 4);
        }

        // Lines and the marker dot, rasterised at device pixels over the plot.
        let (pw, ph) = (frame.plot[2].ceil().max(1.0) as u32, frame.plot[3].ceil().max(1.0) as u32);
        if frame.lines.is_empty() && frame.dots.is_empty() {
            if let Some(t) = self.lines_tex.take() {
                tex.free(t.id);
            }
            self.lines_key = 0;
        } else {
            let key = hash_lines(&frame.lines, &frame.dots, pw, ph);
            if key != self.lines_key || self.lines_tex.is_none() {
                let px = raster_lines(pw, ph, &[], &frame.lines, &frame.dots);
                if let Some(t) = self.lines_tex.take() {
                    tex.free(t.id);
                }
                self.lines_tex = Some(Tex { id: tex.create(pw, ph, &px), w: pw, h: ph });
                self.lines_key = key;
            }
        }

        // Dash masks, one per pattern, direction and length.
        let want: Vec<(Dash, bool, u32)> = frame.dashes.iter().map(|d| (d.dash, d.vertical, d.len.ceil().max(1.0) as u32)).collect();
        let stale: Vec<_> = self.dashes.keys().filter(|k| !want.contains(k)).copied().collect();
        for k in stale {
            if let Some(t) = self.dashes.remove(&k) {
                tex.free(t.id);
            }
        }
        for k in want {
            self.dashes.entry(k).or_insert_with(|| {
                let (w, h) = if k.1 { (1, k.2) } else { (k.2, 1) };
                Tex { id: tex.create(w, h, &dash_mask(k.0, k.2)), w, h }
            });
        }
        self.frame = frame;
    }

    /// Frees the chart's textures.
    pub fn release(&mut self, tex: &mut dyn Textures) {
        if let Some(t) = self.lines_tex.take() {
            tex.free(t.id);
        }
        for (_, t) in self.dashes.drain() {
            tex.free(t.id);
        }
        self.lines_key = 0;
    }

    /// Pushes the prepared frame at `(x, y)` (window pixels), inside `clip` ([x0, y0, x1, y1]).
    pub fn paint(&self, x: f32, y: f32, clip: [f32; 4], opacity: f32, ts: &mut TextSys, out: &mut DrawList) {
        let (x, y) = (x.round(), y.round());
        let f = &self.frame;
        let cl = |c: Clip| -> [f32; 4] { [(c[0] + x).max(clip[0]), (c[1] + y).max(clip[1]), (c[2] + x).min(clip[2]), (c[3] + y).min(clip[3])] };
        let rect = |out: &mut DrawList, s: &RectShape| {
            out.push(Prim { rect: [s.r[0] + x, s.r[1] + y, s.r[2], s.r[3]], clip: cl(s.clip), c0: s.color.arr(), p: [s.radius, 0.0, KIND_RECT, opacity], ..Default::default() }, None);
        };
        for s in &f.rects {
            rect(out, s);
        }
        if let Some(t) = &self.lines_tex {
            let p = f.plot;
            out.push(
                Prim { rect: [p[0] + x, p[1] + y, t.w as f32, t.h as f32], clip: cl([p[0], p[1], p[0] + p[2], p[1] + p[3]]), c0: [1.0; 4], uv: [0.0, 0.0, 1.0, 1.0], p: [0.0, 0.0, KIND_IMAGE, opacity], ..Default::default() },
                Some(t.id),
            );
        }
        for d in &f.dashes {
            let len = d.len.ceil().max(1.0) as u32;
            let Some(t) = self.dashes.get(&(d.dash, d.vertical, len)) else { continue };
            out.push(
                Prim { rect: [d.x + x, d.y + y, t.w as f32, t.h as f32], clip: cl(d.clip), c0: d.color.arr(), uv: [0.0, 0.0, 1.0, 1.0], p: [0.0, 0.0, KIND_MASK, opacity], ..Default::default() },
                Some(t.id),
            );
        }
        let a = ts.atlas_size as f32;
        let labels = f.labels.iter().filter(|l| !l.top);
        let tops = f.labels.iter().filter(|l| l.top);
        self.paint_labels(labels, x, y, &cl, opacity, a, ts, out);
        for s in &f.over {
            rect(out, s);
        }
        self.paint_labels(tops, x, y, &cl, opacity, a, ts, out);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_labels<'a>(&self, labels: impl Iterator<Item = &'a Label>, x: f32, y: f32, cl: &dyn Fn(Clip) -> [f32; 4], opacity: f32, a: f32, ts: &mut TextSys, out: &mut DrawList) {
        let line = self.style().line;
        for l in labels {
            let Some(lb) = self.labels.get(&l.text) else { continue };
            let lx = match l.anchor {
                Anchor::Left => l.x,
                Anchor::Center => l.x - lb.w / 2.0,
            };
            let (ox, oy) = ((lx + x).round(), (l.y - line / 2.0 + y).round());
            let c = cl(l.clip);
            for run in lb.buf.layout_runs() {
                let base_y = run.line_y.round() as i32;
                for g in run.glyphs {
                    let p = g.physical((0.0, 0.0), 1.0);
                    let Some(sl) = ts.glyph(p.cache_key) else { continue };
                    out.push(
                        Prim {
                            rect: [ox + (p.x + sl.left) as f32, oy + (base_y + p.y - sl.top) as f32, sl.w as f32, sl.h as f32],
                            clip: c,
                            c0: l.color.arr(),
                            uv: [sl.x as f32 / a, sl.y as f32 / a, (sl.x + sl.w) as f32 / a, (sl.y + sl.h) as f32 / a],
                            p: [0.0, 0.0, KIND_GLYPH, opacity],
                            ..Default::default()
                        },
                        None,
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Sparklines (ui/charts.tsx), as textures
// ---------------------------------------------------------------------------------------------

/// `Spark`: a line with no fill, green when it ends at or above where it started. RGBA, w x h px.
pub fn spark(points: &[(f64, f64)], w: u32, h: u32, look: &Look) -> Option<Vec<u8>> {
    let g = line_geometry(points, w as f32, h as f32, 2.0)?;
    let color = if rising(points) { look.up } else { look.down };
    Some(raster_lines(w, h, &[], &[Polyline { pts: g, color, width: (h as f32 / 16.0).max(1.5) }], &[]))
}

/// `MiniCurve`: a line over a soft area (22 % to nothing) ending in a dot. RGBA, w x h px.
pub fn mini_curve(points: &[(f64, f64)], w: u32, h: u32, up: Option<bool>, look: &Look) -> Option<Vec<u8>> {
    let pad = (h as f32 / 16.0).max(3.0);
    let g = line_geometry(points, w as f32, h as f32, pad)?;
    let color = if up.unwrap_or_else(|| rising(points)) { look.up } else { look.down };
    let stroke = (h as f32 / 36.0).max(1.75);
    let floor = ((pad + (h as f32 - pad * 2.0).max(1.0)) * 10.0).round() / 10.0;
    let end = *g.last()?;
    Some(raster_lines(w, h, &[Area { pts: g.clone(), floor, color, alpha_top: 0.22 }], &[Polyline { pts: g, color, width: stroke }], &[(end, stroke * 1.7, color)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(time: i64, close: f64, volume: f64) -> Candle {
        Candle { time, open: close, high: close + 1.0, low: close - 1.0, close, volume }
    }

    #[test]
    fn sma_starts_at_the_nth_bar() {
        let bars: Vec<_> = [1.0, 2.0, 3.0, 4.0, 5.0].iter().enumerate().map(|(i, &c)| bar(i as i64, c, 1.0)).collect();
        let v: Vec<f64> = sma(&bars, 3).into_iter().flatten().collect();
        assert_eq!(v, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn vwap_weights_by_volume_and_restarts_each_day() {
        let a = vec![bar(0, 10.0, 1.0), bar(60, 20.0, 3.0)];
        assert!((vwap(&a)[1].unwrap() - (10.0 * 1.0 + 20.0 * 3.0) / 4.0).abs() < 1e-6);
        let mut b = a.clone();
        b.push(bar(86_400, 50.0, 2.0));
        assert!((vwap(&b)[2].unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn keeps_the_cursor_on_screen() {
        let r = Range { from: 10.0, to: 50.0 };
        assert_eq!(show_bar(r, 30.0, 2.0), r);
        assert_eq!(show_bar(r, 9.0, 2.0), Range { from: 7.0, to: 47.0 });
        assert_eq!(show_bar(r, 60.0, 2.0), Range { from: 22.0, to: 62.0 });
    }

    #[test]
    fn zooms_about_the_newest_end_and_stops_at_the_limits() {
        assert_eq!(zoom_range(Range { from: 100.0, to: 200.0 }, 0.5, 300), Range { from: 150.0, to: 200.0 });
        assert_eq!(zoom_range(Range { from: 100.0, to: 120.0 }, 0.1, 300).to - 108.0, 12.0);
        let wide = zoom_range(Range { from: 0.0, to: 300.0 }, 10.0, 300);
        assert_eq!(wide.to - wide.from, 308.0);
    }

    #[test]
    fn scrolls_inside_the_data() {
        assert_eq!(scroll_range(Range { from: 0.0, to: 50.0 }, -20.0, 300).from, -4.0);
        assert_eq!(scroll_range(Range { from: 240.0, to: 290.0 }, 100.0, 300).to, 303.0);
    }

    #[test]
    fn clamps_indexes_and_stick_rate() {
        assert_eq!(clamp_index(-3.0, 10), 0);
        assert_eq!(clamp_index(99.0, 10), 9);
        assert_eq!(clamp_index(2.6, 10), 3);
        assert_eq!(stick_rate(0.5, 40.0), 10.0);
        assert_eq!(stick_rate(-1.0, 40.0), -40.0);
    }

    #[test]
    fn readout_change_is_against_the_bar_before() {
        assert!((readout(&bar(1, 110.0, 1.0), Some(&bar(0, 100.0, 1.0))).change_pct - 10.0).abs() < 1e-6);
    }

    #[test]
    fn clean_candles_dedups_sorts_and_fills_opens() {
        let rows = vec![
            Candle { time: 120, open: 0.0, high: 3.0, low: 1.0, close: 2.0, volume: 0.0 },
            Candle { time: 60, open: 1.0, high: 2.0, low: 1.0, close: 1.5, volume: 0.0 },
            Candle { time: 60, open: 1.0, high: 2.0, low: 1.0, close: 1.7, volume: 0.0 },
            Candle { time: 180, open: 1.0, high: 2.0, low: 1.0, close: f64::NAN, volume: 0.0 },
        ];
        let c = clean_candles(&rows);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].close, 1.7);
        assert_eq!(c[1].open, 1.7);
    }

    #[test]
    fn pads_flat_ranges() {
        assert_eq!(pad_flat_range(1.0, 2.0, 0.01), (1.0, 2.0));
        let (a, b) = pad_flat_range(100.0, 100.0, 0.01);
        assert!((a - 99.0).abs() < 1e-9 && (b - 101.0).abs() < 1e-9);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), Civil { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 });
        // 2026-10-20 14:05:09 UTC
        assert_eq!(civil(1_792_505_109), Civil { year: 2026, month: 10, day: 20, hour: 14, minute: 5, second: 9 });
        assert_eq!(time_label(1_792_505_109, MarkKind::Day, 0), "Oct 20");
        assert_eq!(time_label(1_792_505_109, MarkKind::Time, -7 * 3600), "07:05");
        assert_eq!(time_label(1_792_505_109, MarkKind::Month, 0), "Oct");
        assert_eq!(time_label(1_792_505_109, MarkKind::Year, 0), "2026");
        assert_eq!(readout_time(1_792_505_109, 0), "Oct 20, 14:05");
    }

    #[test]
    fn tick_spans_match_lightweight_charts() {
        // trade-perp: a ~1,700 wide price pane of 331 px marks every 200.
        assert_eq!(tick_span(80_900.0, 79_200.0, 1700.0 * 28.0 / 331.0, 1e-12), 200.0);
        // Its 72 px volume pane (0..~68K) marks every 40K: 2.5 then 2 then 2 from 100K.
        assert_eq!(tick_span(68_750.0, 0.0, 68_750.0 * 28.0 / 72.0, 0.01), 40_000.0);
        // Under 1: the fractional dividers.
        assert_eq!(tick_span(1.06, 0.95, 0.11 * 28.0 / 300.0, 1e-12), 0.02);
        let s = Scale { lo: 0.0, hi: 55_000.0, y: 0.0, h: 72.0, top: 0.2, bottom: 0.0 };
        assert_eq!(price_marks(&s, 11.0, 0.01), vec![40_000.0]);
    }

    #[test]
    fn candle_and_column_widths_match_lightweight_charts() {
        // trade-perp-1920x1200.png: 154 bars over 1082 px, 5 px bodies, 6 px volume columns.
        let s = 1082.0 / 154.0;
        assert_eq!(candle_width(s, 1.0), 5.0);
        assert_eq!(column_width(s, 1.0), 6.0);
        assert_eq!(candle_width(3.0, 1.0), 3.0);
        assert_eq!(candle_width(1.0, 1.0), 1.0);
        assert_eq!(candle_width(20.0, 1.0) as i32 % 2, 1);
    }

    #[test]
    fn time_marks_take_the_heaviest_first() {
        // 15 minute bars from 2026-10-24 12:00 UTC.
        let t0 = 1_792_843_200; // 2026-10-24 12:00 UTC
        let bars: Vec<_> = (0..200).map(|i| bar(t0 + i * 900, 100.0, 1.0)).collect();
        let marks = time_marks(&bars, 0, 199, 11);
        // Midnight UTC is a day mark; 3 h steps between.
        let day = marks.iter().find(|m| m.1 == MarkKind::Day).unwrap();
        assert_eq!(civil(bars[day.0].time).hour, 0);
        for w in marks.windows(2) {
            assert!(w[1].0 - w[0].0 >= 11);
        }
        let hours: Vec<u32> = marks.iter().filter(|m| m.1 == MarkKind::Time).map(|m| civil(bars[m.0].time).hour).collect();
        assert!(hours.iter().all(|h| h % 3 == 0), "{hours:?}");
    }

    /// The panes and the axis land where the web chart puts them (trade-perp-1920x1200.png: box
    /// 1164 x 432 inside 1 rem of top padding, main 331, separator, volume 72, axis 28).
    #[test]
    fn frame_matches_the_web_geometry() {
        let t0 = 1_792_843_200;
        let rows: Vec<_> = (0..400).map(|i| Candle { time: t0 + i * 900, open: 79_500.0 + (i % 7) as f64 * 40.0, high: 80_900.0, low: 79_400.0, close: 79_520.0 + (i % 5) as f64 * 50.0, volume: 10_000.0 + i as f64 }).collect();
        let mut m = Model { extra_h: 24.0, tz: Some(0), ..Default::default() };
        m.set_rows(&rows, "BTC-15m");
        // "$81,000.00" is about 61.5 px at 11 px Inter in Chrome.
        let mut measure = |t: &str| t.len() as f32 * 6.15;
        let f = m.frame(&Look::default(), 1164.0, 432.0, &default_price, &default_count, &mut measure);
        assert_eq!(f.main.unwrap().h, 331.0);
        assert_eq!(f.vol_h, 72.0);
        assert_eq!(f.axis_w, 82.0);
        assert!((f.spacing - 1082.0 / 154.0).abs() < 1e-3);
        assert_eq!(f.plot, [0.0, 0.0, 1082.0, 331.0]);
        // Ticks every 200 in a ~1,700 wide pane.
        let s = f.main.unwrap();
        let marks = price_marks(&s, 11.0, 1e-12);
        assert!((marks[0] - marks[1] - 200.0).abs() < 1e-6, "{marks:?}");
        // Bodies and columns.
        assert!(f.rects.iter().any(|r| r.r[2] == 5.0 && r.color == tok::POS || r.color == tok::NEG));
        assert!(f.rects.iter().any(|r| r.r[2] == 6.0));
    }

    #[test]
    fn line_mode_and_crosshair() {
        let t0 = 1_792_843_200;
        let rows: Vec<_> = (0..60).map(|i| bar(t0 + i * 3600, 1.0 + (i as f64 * 0.3).sin() * 0.05, 5.0)).collect();
        let mut m = Model { ty: ChartType::Line, indicators: vec![Indicator::Ma20], tz: Some(0), ..Default::default() };
        m.set_rows(&rows, "SUI-1h");
        m.set_cursor(Some(10.0));
        let mut measure = |t: &str| t.len() as f32 * 6.0;
        let f = m.frame(&Look::default(), 800.0, 400.0, &default_price, &default_count, &mut measure);
        assert_eq!(f.lines.len(), 2);
        assert_eq!(f.dots.len(), 1);
        assert_eq!(f.dashes.iter().filter(|d| d.dash == Dash::Large).count(), 2);
        assert!(f.labels.iter().any(|l| l.text.contains("Oct")));
        // No candles in line mode.
        assert!(!f.rects.iter().any(|r| r.color == tok::POS && r.r[2] > 1.0 && r.clip[3] == f.plot[3]));
    }

    #[test]
    fn empty_chart_draws_only_its_frame() {
        let m = Model::default();
        let mut measure = |_: &str| 10.0;
        let f = m.frame(&Look::default(), 400.0, 300.0, &default_price, &default_count, &mut measure);
        assert!(f.labels.is_empty() && f.lines.is_empty());
        assert_eq!(f.rects.len(), 2);
    }

    #[test]
    fn spark_geometry_and_raster() {
        let pts = [(0.0, 1.0), (1.0, 2.0), (2.0, 1.5)];
        let g = line_geometry(&pts, 88.0, 24.0, 2.0).unwrap();
        assert_eq!(g[0], [0.0, 22.0]);
        assert_eq!(g[1], [44.0, 2.0]);
        assert!(rising(&pts));
        let px = spark(&pts, 88, 24, &Look::default()).unwrap();
        assert_eq!(px.len(), 88 * 24 * 4);
        assert!(px.chunks(4).any(|p| p[3] > 200));
        let mc = mini_curve(&pts, 200, 64, None, &Look::default()).unwrap();
        assert!(mc.chunks(4).any(|p| p[3] > 0 && p[3] < 80));
    }

    #[test]
    fn formatters() {
        assert_eq!(default_price(81_000.0), "$81,000.00");
        assert_eq!(default_price(0.917), "$0.917");
        assert_eq!(default_count(40_000.0), "40K");
        assert_eq!(default_count(1_250_000.0), "1.25M");
        assert_eq!(default_count(0.0), "");
        assert_eq!(dash_mask(Dash::Dotted, 4), vec![255, 255, 255, 255, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0]);
    }
}
