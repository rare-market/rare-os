//! Deterministic gem-facet identicons (wallet avatars and banners): SVG strings seeded by the
//! wallet address, no network. Ported from launcher/src/marketplace/identicon.ts (rare.market
//! web/lib/identicon). The arithmetic follows JavaScript's (32-bit `Math.imul`, unsigned shifts,
//! `toFixed`) so the same address draws the same gem in both launchers.

use crate::api::enc;
use crate::jsnum::to_fixed;
use std::collections::HashMap;
use std::sync::Mutex;

fn seed_of(s: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for c in s.to_lowercase().encode_utf16() {
        h ^= c as u32;
        h = h.wrapping_mul(16_777_619);
    }
    h ^= h >> 15;
    h = h.wrapping_mul(2_246_822_519);
    h ^= h >> 13;
    h
}

/// mulberry32, as the TS `rng`.
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 1 } else { seed })
    }
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }
}

/// 3 harmonious colours: a base hue plus two neighbours (analogous, sometimes a split-complement
/// accent), as (h, s, l).
fn palette(r: &mut Rng) -> [(f64, f64, f64); 3] {
    let h = r.next() * 360.0;
    let spread = 28.0 + r.next() * 40.0;
    let accent = if r.next() < 0.35 { 150.0 + r.next() * 60.0 } else { spread };
    let c0 = (h, 62.0 + r.next() * 18.0, 52.0 + r.next() * 8.0);
    let c1 = ((h + spread) % 360.0, 58.0 + r.next() * 20.0, 44.0 + r.next() * 10.0);
    let c2 = ((h + accent + 360.0) % 360.0, 55.0 + r.next() * 20.0, 38.0 + r.next() * 12.0);
    [c0, c1, c2]
}

fn mix(p: &[(f64, f64, f64); 3], t: f64, dl: f64) -> String {
    let x = t.clamp(0.0, 0.999) * 2.0;
    let i = x.floor() as usize;
    let f = x - i as f64;
    let (a, b) = (p[i], p[i + 1]);
    let mut dh = b.0 - a.0;
    if dh > 180.0 {
        dh -= 360.0;
    }
    if dh < -180.0 {
        dh += 360.0;
    }
    let h = (a.0 + dh * f + 360.0) % 360.0;
    let s = a.1 + (b.1 - a.1) * f;
    let l = (a.2 + (b.2 - a.2) * f + dl).clamp(18.0, 86.0);
    format!("hsl({} {}% {}%)", to_fixed(h, 0), to_fixed(s, 0), to_fixed(l, 0))
}

fn f1(v: f64) -> String {
    to_fixed(v, 1)
}

#[allow(clippy::too_many_arguments)]
fn mosaic(r: &mut Rng, p: &[(f64, f64, f64); 3], w: f64, h: f64, cols: usize, rows: usize, shine: bool) -> String {
    let (cw, ch) = (w / cols as f64, h / rows as f64);
    let mut pts: Vec<Vec<(f64, f64)>> = Vec::with_capacity(rows + 1);
    for y in 0..=rows {
        let mut row = Vec::with_capacity(cols + 1);
        for x in 0..=cols {
            let edge_x = x == 0 || x == cols;
            let edge_y = y == 0 || y == rows;
            // Points sit slightly outside the box on the edges so facets bleed past the circle crop.
            let px = x as f64 * cw + if edge_x { 0.0 } else { (r.next() - 0.5) * cw * 0.8 };
            let py = y as f64 * ch + if edge_y { 0.0 } else { (r.next() - 0.5) * ch * 0.8 };
            row.push((px, py));
        }
        pts.push(row);
    }
    let ang = r.next() * std::f64::consts::PI * 2.0;
    let (gx, gy) = (ang.cos(), ang.sin());
    // Which way the facets catch light.
    let light = (r.next() - 0.5) * 2.0;
    let mut out = String::new();
    let mut tri = |r: &mut Rng, a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        let cx = (a.0 + b.0 + c.0) / 3.0;
        let cy = (a.1 + b.1 + c.1) / 3.0;
        let t = 0.5 + ((cx / w - 0.5) * gx + (cy / h - 0.5) * gy) * 0.9 + (r.next() - 0.5) * 0.18;
        let dl = (r.next() - 0.5) * 16.0 + (cx / w - cy / h) * light * 8.0;
        out.push_str(&format!("<path fill=\"{}\" d=\"M{} {}L{} {}L{} {}z\"/>", mix(p, t, dl), f1(a.0), f1(a.1), f1(b.0), f1(b.1), f1(c.0), f1(c.1)));
    };
    for y in 0..rows {
        for x in 0..cols {
            let (a, b, c, d) = (pts[y][x], pts[y][x + 1], pts[y + 1][x + 1], pts[y + 1][x]);
            if r.next() < 0.5 {
                tri(r, a, b, c);
                tri(r, a, c, d);
            } else {
                tri(r, a, b, d);
                tri(r, b, c, d);
            }
        }
    }
    if shine {
        // Two soft highlight facets.
        let hx = w * (0.15 + r.next() * 0.4);
        let hy = h * (0.1 + r.next() * 0.3);
        out.push_str(&format!("<path fill=\"#fff\" fill-opacity=\".16\" d=\"M{} {}l{} {}l{} {}z\"/>", f1(hx), f1(hy), f1(w * 0.32), f1(h * 0.06), f1(-w * 0.14), f1(h * 0.3)));
        out.push_str(&format!("<path fill=\"#fff\" fill-opacity=\".08\" d=\"M{} {}l{} {}l0 {}z\"/>", f1(w * 0.6), f1(h * 0.55), f1(w * 0.4), f1(-h * 0.1), f1(h * 0.5)));
    }
    out
}

/// Square mosaic (shown inside a circle).
pub fn avatar_svg(address: &str) -> String {
    let mut r = Rng::new(seed_of(address));
    let p = palette(&mut r);
    let body = mosaic(&mut r, &p, 100.0, 100.0, 4, 4, false);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\" shape-rendering=\"geometricPrecision\"><defs><radialGradient id=\"g\" cx=\".3\" cy=\".25\" r=\".9\"><stop offset=\"0\" stop-color=\"#fff\" stop-opacity=\".28\"/><stop offset=\".5\" stop-color=\"#fff\" stop-opacity=\"0\"/><stop offset=\"1\" stop-color=\"#000\" stop-opacity=\".22\"/></radialGradient></defs>{body}<rect width=\"100\" height=\"100\" fill=\"url(#g)\"/></svg>"
    )
}

/// Wide, larger facets, soft blur, darkened toward the bottom; the avatar's palette.
pub fn banner_svg(address: &str) -> String {
    let mut r = Rng::new(seed_of(&format!("{address}#banner")));
    let p = palette(&mut Rng::new(seed_of(address)));
    let body = mosaic(&mut r, &p, 1600.0, 400.0, 7, 3, true);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1600 400\" preserveAspectRatio=\"xMidYMid slice\"><defs><filter id=\"b\" x=\"-5%\" y=\"-5%\" width=\"110%\" height=\"110%\"><feGaussianBlur stdDeviation=\"14\"/></filter><linearGradient id=\"d\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\"><stop offset=\"0\" stop-color=\"#000\" stop-opacity=\".05\"/><stop offset=\"1\" stop-color=\"#000\" stop-opacity=\".45\"/></linearGradient></defs><g filter=\"url(#b)\">{body}</g><rect width=\"1600\" height=\"400\" fill=\"url(#d)\"/></svg>"
    )
}

static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
const CAP: usize = 2000;

fn memo(key: String, make: impl FnOnce() -> String) -> String {
    let mut g = CACHE.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    if let Some(v) = m.get(&key) {
        return v.clone();
    }
    if m.len() >= CAP {
        m.clear();
    }
    let v = make();
    m.insert(key, v.clone());
    v
}

/// The avatar as a `data:image/svg+xml,` URI.
pub fn avatar_uri(address: &str) -> String {
    memo(format!("a{address}"), || format!("data:image/svg+xml,{}", enc(&avatar_svg(address))))
}

pub fn banner_uri(address: &str) -> String {
    memo(format!("b{address}"), || format!("data:image/svg+xml,{}", enc(&banner_svg(address))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gems_match_the_typescript() {
        let g: serde_json::Value = serde_json::from_str(include_str!("../golden/identicon.json")).unwrap();
        for c in g.as_array().unwrap() {
            let a = c["address"].as_str().unwrap();
            assert_eq!(avatar_svg(a), c["avatar"].as_str().unwrap(), "avatar {a}");
            assert_eq!(banner_svg(a), c["banner"].as_str().unwrap(), "banner {a}");
            assert_eq!(avatar_uri(a), c["uri"].as_str().unwrap(), "uri {a}");
        }
    }
}
