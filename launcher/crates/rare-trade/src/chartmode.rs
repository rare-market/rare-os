//! The pure part of chart mode: the cursor bar, the visible range, zoom, and the Trade API's indicator lines
//! (MA 20, MA 50, VWAP), plus the bar clean-up the chart does before drawing. Free of any chart
//! library so it is tested on its own.

use crate::api::Candle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartType {
    Candles,
    Line,
}
pub const CHART_TYPES: [ChartType; 2] = [ChartType::Candles, ChartType::Line];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndicatorKey {
    Ma20,
    Ma50,
    Vwap,
}

/// A cycles through these: none, MA 20, MA 20 + MA 50, all three.
pub const INDICATOR_STEPS: [&[IndicatorKey]; 4] = [&[], &[IndicatorKey::Ma20], &[IndicatorKey::Ma20, IndicatorKey::Ma50], &[IndicatorKey::Ma20, IndicatorKey::Ma50, IndicatorKey::Vwap]];

impl IndicatorKey {
    pub fn color(&self) -> &'static str {
        match self {
            IndicatorKey::Ma20 => "#f5c542",
            IndicatorKey::Ma50 => "#5b8cff",
            IndicatorKey::Vwap => "#c77dff",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            IndicatorKey::Ma20 => "MA 20",
            IndicatorKey::Ma50 => "MA 50",
            IndicatorKey::Vwap => "VWAP",
        }
    }
}

/// A visible range in bar indexes (fractional, may run past either end).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinePoint {
    pub time: f64,
    pub value: f64,
}

pub const MIN_BARS: f64 = 12.0;
pub const RIGHT_PAD: f64 = 4.0;

/// Simple moving average of the closes; the first n-1 bars have no value.
pub fn sma(bars: &[Candle], n: usize) -> Vec<LinePoint> {
    let mut out = Vec::new();
    let mut sum = 0.0;
    for i in 0..bars.len() {
        sum += bars[i].close;
        if i >= n {
            sum -= bars[i - n].close;
        }
        if i + 1 >= n {
            out.push(LinePoint { time: bars[i].time, value: sum / n as f64 });
        }
    }
    out
}

/// Volume weighted average price, restarting each UTC day.
pub fn vwap(bars: &[Candle]) -> Vec<LinePoint> {
    let mut out = Vec::new();
    let mut day = f64::NAN;
    let (mut pv, mut v) = (0.0, 0.0);
    for b in bars {
        let d = (b.time / 86_400.0).floor();
        if d != day {
            day = d;
            pv = 0.0;
            v = 0.0;
        }
        let typical = (b.high + b.low + b.close) / 3.0;
        let w = b.volume.unwrap_or(0.0);
        pv += typical * w;
        v += w;
        if v > 0.0 {
            out.push(LinePoint { time: b.time, value: pv / v });
        }
    }
    out
}

pub fn indicator_line(key: IndicatorKey, bars: &[Candle]) -> Vec<LinePoint> {
    match key {
        IndicatorKey::Ma20 => sma(bars, 20),
        IndicatorKey::Ma50 => sma(bars, 50),
        IndicatorKey::Vwap => vwap(bars),
    }
}

pub fn clamp_index(i: f64, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    crate::jsnum::js_round(i).clamp(0.0, (n - 1) as f64) as usize
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
    let width = MIN_BARS.max((MIN_BARS.max(n as f64 + RIGHT_PAD * 2.0)).min((r.to - r.from) * factor));
    Range { from: r.to - width, to: r.to }
}

/// Slide the range by `bars`, never past the first bar or more than RIGHT_PAD past the last.
pub fn scroll_range(r: Range, bars: f64, n: usize) -> Range {
    let width = r.to - r.from;
    let from = (-RIGHT_PAD).max((r.from + bars).min(n as f64 - 1.0 + RIGHT_PAD - width));
    Range { from, to: from + width }
}

/// What the readout under the chart says for one bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readout {
    pub time: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub change_pct: f64,
}

pub fn readout(b: &Candle, prev: Option<&Candle>) -> Readout {
    let r = prev.map(|p| p.close).unwrap_or(b.open);
    Readout { time: b.time, open: b.open, high: b.high, low: b.low, close: b.close, volume: b.volume.unwrap_or(0.0), change_pct: if r > 0.0 { (b.close - r) / r * 100.0 } else { 0.0 } }
}

/// Bars per second to move for a stick deflection: slow near the centre, quick at the edge.
pub fn stick_rate(v: f64, max_bars_per_sec: f64) -> f64 {
    let a = v.abs();
    let sign = if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    };
    sign * a * a * max_bars_per_sec
}

/// A flat range (one candle, or no trades in view) opened up around its price.
pub fn pad_flat_range(min_value: f64, max_value: f64, pad: f64) -> (f64, f64) {
    let mid = (min_value + max_value) / 2.0;
    let need = (mid.abs() * pad * 2.0).max(1e-12 * 100.0);
    if max_value - min_value >= need / 1e4 {
        return (min_value, max_value);
    }
    (mid - need / 2.0, mid + need / 2.0)
}

/// Bars by time, one per time, no gaps in the open price.
pub fn clean_candles(rows: &[Candle]) -> Vec<Candle> {
    let mut by: Vec<Candle> = Vec::new();
    let mut at: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for r in rows {
        if r.time.is_finite() && r.close.is_finite() && r.close > 0.0 {
            match at.get(&r.time.to_bits()) {
                Some(&i) => by[i] = *r,
                None => {
                    at.insert(r.time.to_bits(), by.len());
                    by.push(*r);
                }
            }
        }
    }
    by.sort_by(|a, b| a.time.total_cmp(&b.time));
    for i in 1..by.len() {
        if by[i].open.is_nan() || by[i].open <= 0.0 {
            by[i].open = by[i - 1].close;
        }
    }
    by
}

#[cfg(test)]
mod tests {
    //! chartmode.test.ts
    use super::*;

    fn bar(time: f64, close: f64, volume: f64) -> Candle {
        Candle { time, open: close, high: close + 1.0, low: close - 1.0, close, volume: Some(volume) }
    }

    #[test]
    fn sma_starts_at_the_nth_bar() {
        let bars: Vec<Candle> = [1.0, 2.0, 3.0, 4.0, 5.0].iter().enumerate().map(|(i, c)| bar(i as f64, *c, 1.0)).collect();
        assert_eq!(sma(&bars, 3).iter().map(|p| p.value).collect::<Vec<_>>(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn vwap_weights_by_volume_and_restarts_each_day() {
        let a = vec![bar(0.0, 10.0, 1.0), bar(60.0, 20.0, 3.0)];
        assert!((vwap(&a)[1].value - (10.0 * 1.0 + 20.0 * 3.0) / 4.0).abs() < 1e-6);
        let mut b = a.clone();
        b.push(bar(86_400.0, 50.0, 2.0));
        assert!((vwap(&b)[2].value - 50.0).abs() < 1e-6);
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
    fn clamps_indexes() {
        assert_eq!(clamp_index(-3.0, 10), 0);
        assert_eq!(clamp_index(99.0, 10), 9);
        assert_eq!(clamp_index(2.6, 10), 3);
    }

    #[test]
    fn stick_rate_is_quadratic_and_signed() {
        assert_eq!(stick_rate(0.5, 40.0), 10.0);
        assert_eq!(stick_rate(-1.0, 40.0), -40.0);
    }

    #[test]
    fn readout_change_is_against_the_bar_before() {
        assert!((readout(&bar(1.0, 110.0, 1.0), Some(&bar(0.0, 100.0, 1.0))).change_pct - 10.0).abs() < 1e-6);
    }

    #[test]
    fn cleans_and_pads() {
        let mut gap = bar(120.0, 7.0, 1.0);
        gap.open = 0.0;
        let rows = vec![bar(60.0, 5.0, 1.0), bar(0.0, 4.0, 1.0), bar(60.0, 6.0, 1.0), gap, bar(180.0, 0.0, 1.0)];
        let c = clean_candles(&rows);
        assert_eq!(c.iter().map(|b| (b.time, b.close)).collect::<Vec<_>>(), vec![(0.0, 4.0), (60.0, 6.0), (120.0, 7.0)]);
        assert_eq!(c[2].open, 6.0);
        assert_eq!(pad_flat_range(10.0, 10.0, 0.01), (9.9, 10.1));
        assert_eq!(pad_flat_range(9.0, 11.0, 0.01), (9.0, 11.0));
    }
}
