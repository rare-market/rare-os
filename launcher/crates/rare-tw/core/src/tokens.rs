//! Design tokens: app.css's `@theme` (read at build time from launcher/src/styles/app.css) on top
//! of the Tailwind v4 defaults the launcher uses. Lookups by Tailwind name (`bg`, `title`, `card`).

use crate::appcss::sheet;
use crate::model::{Family, Len, LineHeight, Rgba};
use crate::value;

/// A colour token: app.css `--color-<name>`, then Tailwind's black and white.
pub fn color(name: &str) -> Option<Rgba> {
    if let Some(v) = sheet().theme(&format!("color-{name}")) {
        return value::color(v);
    }
    match name {
        "black" => Some(Rgba::rgb(0, 0, 0)),
        "white" => Some(Rgba::rgb(255, 255, 255)),
        "transparent" => Some(Rgba::TRANSPARENT),
        _ => None,
    }
}

/// Every `--color-*` in app.css's @theme, resolved, in source order.
pub fn colors() -> Vec<(String, Rgba)> {
    sheet().theme.iter().filter_map(|(k, v)| Some((k.strip_prefix("color-")?.to_string(), value::color(v)?))).collect()
}

/// A type-scale step: size, its line height and letter spacing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextTok {
    pub size: Len,
    pub line: Option<LineHeight>,
    pub tracking: Option<Len>,
}

fn line_height(v: &str) -> Option<LineHeight> {
    if let Some(n) = value::num(v) {
        return Some(LineHeight::Num(n));
    }
    if let Some(l) = value::len(v) {
        return Some(LineHeight::Len(l));
    }
    // calc(a / b)
    let inner = v.strip_prefix("calc(")?.strip_suffix(')')?;
    let (a, b) = inner.split_once('/')?;
    Some(LineHeight::Num(value::num(a)? / value::num(b)?))
}

/// `text-<name>`: app.css `--text-<name>` (with `--line-height` / `--letter-spacing`), then
/// Tailwind's xs..9xl.
pub fn text(name: &str) -> Option<TextTok> {
    let sh = sheet();
    if let Some(v) = sh.theme(&format!("text-{name}")) {
        return Some(TextTok {
            size: value::len(v)?,
            line: sh.theme(&format!("text-{name}--line-height")).and_then(line_height),
            tracking: sh.theme(&format!("text-{name}--letter-spacing")).and_then(value::len),
        });
    }
    let (size, line) = match name {
        "xs" => (0.75, 1.0 / 0.75),
        "sm" => (0.875, 1.25 / 0.875),
        "base" => (1.0, 1.5),
        "lg" => (1.125, 1.75 / 1.125),
        "xl" => (1.25, 1.75 / 1.25),
        "2xl" => (1.5, 2.0 / 1.5),
        "3xl" => (1.875, 2.25 / 1.875),
        "4xl" => (2.25, 2.5 / 2.25),
        "5xl" => (3.0, 1.0),
        "6xl" => (3.75, 1.0),
        "7xl" => (4.5, 1.0),
        "8xl" => (6.0, 1.0),
        "9xl" => (8.0, 1.0),
        _ => return None,
    };
    Some(TextTok { size: Len::Rem(size), line: Some(LineHeight::Num(line)), tracking: None })
}

/// Every `--text-*` size in app.css's @theme, in source order.
pub fn texts() -> Vec<(String, TextTok)> {
    sheet()
        .theme
        .iter()
        .filter_map(|(k, _)| {
            let n = k.strip_prefix("text-")?;
            (!n.contains("--")).then(|| Some((n.to_string(), text(n)?))).flatten()
        })
        .collect()
}

/// `rounded-<name>`: app.css `--radius-<name>`, then Tailwind's xs..4xl.
pub fn radius(name: &str) -> Option<Len> {
    if let Some(v) = sheet().theme(&format!("radius-{name}")) {
        return value::len(v);
    }
    Some(Len::Rem(match name {
        "xs" => 0.125,
        "sm" => 0.25,
        "md" => 0.375,
        "lg" => 0.5,
        "xl" => 0.75,
        "2xl" => 1.0,
        "3xl" => 1.5,
        "4xl" => 2.0,
        _ => return None,
    }))
}

/// Every `--radius-*` in app.css's @theme.
pub fn radii() -> Vec<(String, Len)> {
    sheet().theme.iter().filter_map(|(k, v)| Some((k.strip_prefix("radius-")?.to_string(), value::len(v)?))).collect()
}

pub fn family(name: &str) -> Option<Family> {
    match name {
        "sans" => Some(Family::Sans),
        "mono" => Some(Family::Mono),
        _ => None,
    }
}

/// The CSS font stack of a family (app.css `--font-*`).
pub fn family_stack(f: Family) -> String {
    let key = match f {
        Family::Sans => "font-sans",
        Family::Mono => "font-mono",
    };
    sheet().theme(key).unwrap_or_default().to_string()
}

pub fn weight(name: &str) -> Option<u16> {
    Some(match name {
        "thin" => 100,
        "extralight" => 200,
        "light" => 300,
        "normal" => 400,
        "medium" => 500,
        "semibold" => 600,
        "bold" => 700,
        "extrabold" => 800,
        "black" => 900,
        _ => return None,
    })
}

pub fn tracking(name: &str) -> Option<Len> {
    Some(Len::Em(match name {
        "tighter" => -0.05,
        "tight" => -0.025,
        "normal" => 0.0,
        "wide" => 0.025,
        "wider" => 0.05,
        "widest" => 0.1,
        _ => return None,
    }))
}

pub fn leading(name: &str) -> Option<f32> {
    Some(match name {
        "tight" => 1.25,
        "snug" => 1.375,
        "normal" => 1.5,
        "relaxed" => 1.625,
        "loose" => 2.0,
        _ => return None,
    })
}

/// `max-w-<name>` container sizes.
pub fn container(name: &str) -> Option<Len> {
    Some(Len::Rem(match name {
        "3xs" => 16.0,
        "2xs" => 18.0,
        "xs" => 20.0,
        "sm" => 24.0,
        "md" => 28.0,
        "lg" => 32.0,
        "xl" => 36.0,
        "2xl" => 42.0,
        "3xl" => 48.0,
        "4xl" => 56.0,
        "5xl" => 64.0,
        "6xl" => 72.0,
        "7xl" => 80.0,
        _ => return None,
    }))
}

/// `ease-<name>`: app.css `--ease-*`, then Tailwind's.
pub fn ease(name: &str) -> Option<[f32; 4]> {
    if let Some(v) = sheet().theme(&format!("ease-{name}")) {
        return value::ease(v);
    }
    Some(match name {
        "linear" => [0.0, 0.0, 1.0, 1.0],
        "in" => [0.4, 0.0, 1.0, 1.0],
        "out" => [0.0, 0.0, 0.2, 1.0],
        "in-out" => [0.4, 0.0, 0.2, 1.0],
        _ => return None,
    })
}

/// Tailwind's transition defaults (`--default-transition-*`).
pub const DEFAULT_TRANSITION_MS: f32 = 150.0;
pub const DEFAULT_TRANSITION_EASE: [f32; 4] = [0.4, 0.0, 0.2, 1.0];
/// Tailwind's spacing step (`--spacing`).
pub const SPACING_REM: f32 = 0.25;

/// A `:root` duration (`--dur-base` -> ms).
pub fn duration(name: &str) -> Option<f32> {
    value::time_ms(sheet().root.get(&format!("dur-{name}"))?)
}
