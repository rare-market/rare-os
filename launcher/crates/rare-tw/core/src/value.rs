//! CSS value parsing shared by the class parser, arbitrary values (`w-[2.5rem]`) and app.css rules.

use crate::appcss::sheet;
use crate::model::{Len, Rgba, Shadow, Stop};

pub fn num(s: &str) -> Option<f32> {
    let s = s.trim();
    if s.is_empty() || s.starts_with('+') {
        return None;
    }
    let v: f32 = s.parse().ok()?;
    v.is_finite().then_some(v)
}

/// `0`, `2.5rem`, `3px`, `-0.02em`, `45%`.
pub fn len(s: &str) -> Option<Len> {
    let s = s.trim();
    if s == "0" || s == "0px" {
        return Some(Len::ZERO);
    }
    if let Some(v) = s.strip_suffix("rem") {
        return num(v).map(Len::Rem);
    }
    if let Some(v) = s.strip_suffix("px") {
        return num(v).map(Len::Px);
    }
    if let Some(v) = s.strip_suffix("em") {
        return num(v).map(Len::Em);
    }
    if let Some(v) = s.strip_suffix('%') {
        return num(v).map(Len::Pct);
    }
    None
}

/// `transparent`, `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(r g b)`, `rgb(r g b / a)`, `rgb(r, g, b)`,
/// `rgba(...)`, `rgb(var(--x))`, `rgb(var(--x) / var(--y))` against app.css's :root.
pub fn color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if s == "transparent" {
        return Some(Rgba::TRANSPARENT);
    }
    if let Some(h) = s.strip_prefix('#') {
        let hex = |i: usize, n: usize| u8::from_str_radix(&h[i..i + n], 16).ok();
        return match h.len() {
            3 | 4 => {
                let c = |i| hex(i, 1).map(|v| v * 17);
                let a = if h.len() == 4 { c(3)? as f32 / 255.0 } else { 1.0 };
                Some(Rgba::rgb(c(0)?, c(1)?, c(2)?).alpha(a))
            }
            6 | 8 => {
                let a = if h.len() == 8 { hex(6, 2)? as f32 / 255.0 } else { 1.0 };
                Some(Rgba::rgb(hex(0, 2)?, hex(2, 2)?, hex(4, 2)?).alpha(a))
            }
            _ => None,
        };
    }
    let inner = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb("))?.strip_suffix(')')?;
    let inner = if inner.contains("var(") { sheet().resolve_vars(inner) } else { inner.to_string() };
    let (rgb, a) = match inner.split_once('/') {
        Some((c, a)) => (c.to_string(), Some(a.trim().to_string())),
        None => (inner.clone(), None),
    };
    let parts: Vec<&str> = rgb.split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    let (rgb, a) = match (parts.len(), a) {
        (3, a) => (parts, a),
        (4, None) => (parts[..3].to_vec(), Some(parts[3].to_string())),
        _ => return None,
    };
    let ch = |p: &str| -> Option<f32> { num(p).filter(|v| (0.0..=255.0).contains(v)) };
    let alpha = match a {
        None => 1.0,
        Some(a) => match a.strip_suffix('%') {
            Some(p) => num(p)? / 100.0,
            None => num(&a)?,
        },
    };
    Some(Rgba { r: ch(rgb[0])? / 255.0, g: ch(rgb[1])? / 255.0, b: ch(rgb[2])? / 255.0, a: alpha.clamp(0.0, 1.0) })
}

/// Splits at top-level separators (outside parentheses).
pub fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c)
            }
            ')' => {
                depth -= 1;
                cur.push(c)
            }
            c if c == sep && depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// Splits on top-level whitespace.
pub fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c)
            }
            ')' => {
                depth -= 1;
                cur.push(c)
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// One shadow layer: `x y [blur [spread]] color` (Tailwind's arbitrary shadows, text-shadow).
pub fn shadow(s: &str) -> Option<Shadow> {
    let mut lens = Vec::new();
    let mut col = None;
    for w in words(s) {
        if let Some(l) = len(&w) {
            lens.push(l);
        } else if w == "inset" {
            return None;
        } else {
            col = Some(color(&w)?);
        }
    }
    if !(2..=4).contains(&lens.len()) {
        return None;
    }
    let z = Len::ZERO;
    Some(Shadow { x: lens[0], y: lens[1], blur: *lens.get(2).unwrap_or(&z), spread: *lens.get(3).unwrap_or(&z), color: col.unwrap_or(Rgba::rgb(0, 0, 0)) })
}

pub fn shadows(s: &str) -> Option<Vec<Shadow>> {
    split_top(s, ',').iter().map(|l| shadow(l)).collect()
}

/// `linear-gradient(105deg, transparent 30%, rgb(...) 50%, ...)` -> (angle, stops).
pub fn linear_gradient(s: &str) -> Option<(f32, Vec<Stop>)> {
    let inner = s.trim().strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let parts = split_top(inner, ',');
    let mut it = parts.iter().peekable();
    let mut angle = 180.0;
    if let Some(first) = it.peek() {
        if let Some(a) = first.strip_suffix("deg") {
            angle = num(a)?;
            it.next();
        } else if let Some(dir) = first.strip_prefix("to ") {
            angle = side_angle(dir)?;
            it.next();
        }
    }
    let mut stops = Vec::new();
    for p in it {
        let w = words(p);
        let color = color(w.first()?)?;
        let pos = match w.get(1) {
            Some(v) => Some(v.strip_suffix('%').and_then(num)?),
            None => None,
        };
        stops.push(Stop { color, pos });
    }
    (stops.len() >= 2).then_some((angle, stops))
}

/// `to top` and friends as CSS angles.
pub fn side_angle(dir: &str) -> Option<f32> {
    Some(match dir.trim() {
        "top" | "t" => 0.0,
        "right" | "r" => 90.0,
        "bottom" | "b" => 180.0,
        "left" | "l" => 270.0,
        "top right" | "tr" => 45.0,
        "bottom right" | "br" => 135.0,
        "bottom left" | "bl" => 225.0,
        "top left" | "tl" => 315.0,
        _ => return None,
    })
}

/// A position pair (`center`, `left bottom`, `80% 30%`, `center 25%`): x, y.
pub fn position(s: &str) -> Option<[Len; 2]> {
    let w = words(s);
    let kw = |k: &str| -> Option<(Option<bool>, Len)> {
        // Some(true) = horizontal keyword, Some(false) = vertical, None = either.
        Some(match k {
            "left" => (Some(true), Len::Pct(0.0)),
            "right" => (Some(true), Len::Pct(100.0)),
            "top" => (Some(false), Len::Pct(0.0)),
            "bottom" => (Some(false), Len::Pct(100.0)),
            "center" => (None, Len::Pct(50.0)),
            _ => (None, len(k)?),
        })
    };
    match w.len() {
        1 => {
            let (axis, v) = kw(&w[0])?;
            Some(match axis {
                Some(false) => [Len::Pct(50.0), v],
                _ => [v, Len::Pct(50.0)],
            })
        }
        2 => {
            let (a0, v0) = kw(&w[0])?;
            let (a1, v1) = kw(&w[1])?;
            if a0 == Some(false) || a1 == Some(true) {
                Some([v1, v0])
            } else {
                Some([v0, v1])
            }
        }
        _ => None,
    }
}

/// `cubic-bezier(a,b,c,d)` or a timing keyword.
pub fn ease(s: &str) -> Option<[f32; 4]> {
    let s = s.trim();
    Some(match s {
        "linear" => [0.0, 0.0, 1.0, 1.0],
        "ease" => [0.25, 0.1, 0.25, 1.0],
        "ease-in" => [0.42, 0.0, 1.0, 1.0],
        "ease-out" => [0.0, 0.0, 0.58, 1.0],
        "ease-in-out" => [0.42, 0.0, 0.58, 1.0],
        _ => {
            let inner = s.strip_prefix("cubic-bezier(")?.strip_suffix(')')?;
            let v: Vec<f32> = inner.split(',').map(num).collect::<Option<_>>()?;
            v.try_into().ok()?
        }
    })
}

/// `200ms`, `0.2s`, `2.4s` -> ms.
pub fn time_ms(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("ms") {
        return num(v);
    }
    s.strip_suffix('s').and_then(num).map(|v| v * 1000.0)
}
