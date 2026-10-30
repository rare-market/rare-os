//! Spark and MiniCurve (ui/charts.tsx, the Trade API's): a table sparkline with no fill, and a card curve
//! with a soft area ending in a dot. Drawn in real pixels as SVG and rasterised once per data
//! change by the image workers (the engine's chart module draws the big Trade charts). Green when
//! the line ends at or above where it started, red below: these lines are money.

use super::icon::svg_key;
use super::ui::{Ui, S};
use rare_render::view::{Content, El};

/// (t, v)
pub type Point = (f64, f64);

pub struct Geometry {
    pub xy: Vec<(f32, f32)>,
    pub line: String,
    pub area: String,
}

fn r1(n: f32) -> f32 {
    (n * 10.0).round() / 10.0
}

/// Points placed by time across the width and by value down the height (lineGeometry).
pub fn line_geometry(points: &[Point], w: f32, h: f32, pad_y: f32) -> Option<Geometry> {
    if points.len() < 2 || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let t0 = points[0].0;
    let t1 = points[points.len() - 1].0;
    let mut lo = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let mut hi = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    if hi == lo {
        hi += 1.0;
        lo -= 1.0;
    }
    let plot_h = (h - pad_y * 2.0).max(1.0);
    let x = |t: f64| if t1 == t0 { w } else { ((t - t0) / (t1 - t0)) as f32 * w };
    let y = |v: f64| pad_y + ((hi - v) / (hi - lo)) as f32 * plot_h;
    let xy: Vec<(f32, f32)> = points.iter().map(|p| (r1(x(p.0)), r1(y(p.1)))).collect();
    let line = xy.iter().enumerate().map(|(i, p)| format!("{}{} {}", if i > 0 { "L" } else { "M" }, p.0, p.1)).collect::<Vec<_>>().join(" ");
    let floor = r1(pad_y + plot_h);
    let area = format!("{line} L{} {floor} L{} {floor} Z", xy[xy.len() - 1].0, xy[0].0);
    Some(Geometry { xy, line, area })
}

/// The last point at or above the first.
pub fn rising(points: &[Point]) -> bool {
    points.last().map(|p| p.1).unwrap_or(0.0) >= points.first().map(|p| p.1).unwrap_or(0.0)
}

const UP: &str = "rgb(42,223,114)";
const DOWN: &str = "rgb(234,65,65)";

fn drawn(ui: &mut Ui, st: S, w: f32, h: f32, name: &str, svg: Option<String>) -> El {
    let el = ui.bx(st.wh(w, h));
    match svg {
        Some(svg) => {
            let k = svg_key(ui, name, svg, w, h);
            El { content: Content::Image { key: k, tint: None, contain: Some([0.0, 0.0]) }, ..el }
        }
        None => el,
    }
}

/// A tiny line with no fill, for a row (`h-[1.5rem] w-[5.5rem]` by default).
pub fn spark(ui: &mut Ui, st: S, points: &[Point], w: f32, h: f32) -> El {
    let (pw, ph) = (ui.r(w).round(), ui.r(h).round());
    let svg = line_geometry(points, pw, ph, 2.0).map(|g| {
        let stroke = (ph / 16.0).max(1.5);
        let c = if rising(points) { UP } else { DOWN };
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pw}" height="{ph}" viewBox="0 0 {pw} {ph}"><path d="{}" fill="none" stroke="{c}" stroke-width="{stroke}" stroke-linejoin="round" stroke-linecap="round"/></svg>"#, g.line)
    });
    drawn(ui, st, w, h, "spark", svg)
}

/// A small filled line ending in a dot, for a card. `tone_up` overrides the direction colour.
pub fn mini_curve(ui: &mut Ui, st: S, points: &[Point], w: f32, h: f32, tone_up: Option<bool>) -> El {
    let (pw, ph) = (ui.r(w).round(), ui.r(h).round());
    let color = if tone_up.unwrap_or_else(|| rising(points)) { UP } else { DOWN };
    let svg = line_geometry(points, pw, ph, (ph / 16.0).max(3.0)).map(|g| {
        let stroke = (ph / 36.0).max(1.75);
        let end = g.xy[g.xy.len() - 1];
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pw}" height="{ph}" viewBox="0 0 {pw} {ph}"><defs><linearGradient id="g" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="{color}" stop-opacity="0.22"/><stop offset="1" stop-color="{color}" stop-opacity="0"/></linearGradient></defs><path d="{}" fill="url(#g)"/><path d="{}" fill="none" stroke="{color}" stroke-width="{stroke}" stroke-linejoin="round" stroke-linecap="round"/><circle cx="{}" cy="{}" r="{}" fill="{color}"/></svg>"#,
            g.area,
            g.line,
            end.0,
            end.1,
            stroke * 1.7
        )
    });
    drawn(ui, st, w, h, "curve", svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry() {
        let g = line_geometry(&[(0.0, 1.0), (1.0, 2.0)], 100.0, 20.0, 2.0).unwrap();
        assert_eq!(g.xy, vec![(0.0, 18.0), (100.0, 2.0)]);
        assert!(g.area.ends_with('Z'));
        assert!(line_geometry(&[(0.0, 1.0)], 10.0, 10.0, 1.0).is_none());
        assert!(rising(&[(0.0, 1.0), (1.0, 1.0)]));
    }
}
