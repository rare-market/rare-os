//! Badge art for Rare's own trophies, in Rare's flat style: a rounded square in a dark colour
//! picked by the game, one white line icon for the kind, and the hours for the hour ones. The
//! same SVG as rare.market's `GET /v1/trophies/badge/<kind>.svg?game=<id>` (api/src/trophies/badge.ts),
//! so the launcher draws it offline and the website and the chain show the same picture.

use crate::milestones::Kind;

/// Dark, low-saturation backgrounds; white reads on all of them.
pub const PALETTE: [&str; 8] = ["#2b3440", "#3a3026", "#30293c", "#243630", "#3b2a2d", "#28343b", "#36321f", "#2d2d38"];

/// FNV-1a, the same in badge.ts.
pub fn fnv(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn icon(kind: Kind) -> &'static str {
    match kind {
        Kind::First => r#"<path d="M8 5.5v13l10-6.5z"/>"#,
        Kind::Streak7 => r#"<path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.07-2.14-.22-4.05 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.15.43-2.29 1-3a2.5 2.5 0 0 0 2.5 2.5z"/>"#,
        Kind::SuiPlay => r#"<path d="M6 11h4M8 9v4M15 12h.01M18 10h.01"/><path d="M17.32 5H6.68a4 4 0 0 0-3.98 3.59C2.6 9.42 2 14.46 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.41-1.41A2 2 0 0 1 9.83 16h4.34a2 2 0 0 1 1.41.59L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.55-.6-6.58-.69-7.26A4 4 0 0 0 17.32 5z"/>"#,
        Kind::Complete => r#"<path d="M21.8 10A10 10 0 1 1 17 3.34"/><path d="m9 11 3 3L22 4"/>"#,
        _ => r#"<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>"#,
    }
}

/// The badge SVG for a kind in a game (`game` is the game's id, e.g. "steam:10180").
pub fn svg(kind: Kind, game: &str) -> String {
    let bg = PALETTE[(fnv(game) % PALETTE.len() as u32) as usize];
    let hours = kind.hours();
    // The icon on a 24 grid: centred, or up top with the hours under it.
    let (scale, x, y) = if hours.is_some() { (2.0, 40.0, 22.0) } else { (2.6, 32.8, 32.8) };
    let label = hours
        .map(|h| format!(r##"<text x="64" y="106" text-anchor="middle" font-family="Inter, Helvetica, Arial, sans-serif" font-size="24" font-weight="700" fill="#ffffff">{h}h</text>"##))
        .unwrap_or_default();
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><rect width="128" height="128" rx="28" fill="{bg}"/><rect x="1" y="1" width="126" height="126" rx="27" fill="none" stroke="#ffffff" stroke-opacity="0.08" stroke-width="2"/><g transform="translate({x} {y}) scale({scale})" fill="none" stroke="#ffffff" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{}</g>{label}</svg>"##,
        icon(kind)
    )
}

/// The badge as a data URI the image loader draws.
pub fn data_uri(kind: Kind, game: &str) -> String {
    let s = svg(kind, game);
    let mut out = String::with_capacity(s.len() + 32);
    out.push_str("data:image/svg+xml;utf8,");
    for b in s.bytes() {
        match b {
            b'#' => out.push_str("%23"),
            b'%' => out.push_str("%25"),
            b'"' => out.push('\''),
            b'<' => out.push_str("%3C"),
            b'>' => out.push_str("%3E"),
            _ => out.push(b as char),
        }
    }
    out
}

/// rare.market's copy (the website, and the image a pinned trophy keeps on chain).
pub fn url(kind: Kind, game: &str) -> String {
    let enc: String = game.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("https://rare.market/v1/trophies/badge/{}.svg?game={enc}", kind.key())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges() {
        let a = svg(Kind::Hours10, "steam:10180");
        assert!(a.contains(">10h</text>") && a.starts_with("<svg"));
        assert_eq!(a, svg(Kind::Hours10, "steam:10180"));
        assert_ne!(svg(Kind::First, "steam:10180"), svg(Kind::First, "steam:620"));
        assert!(!svg(Kind::Streak7, "x").contains("<text"));
        assert_eq!((fnv(""), fnv("a")), (0x811c_9dc5, 0xe40c_292c));
        let d = data_uri(Kind::Complete, "steam:1");
        assert!(d.starts_with("data:image/svg+xml;utf8,%3Csvg") && !d.contains('#') && !d.contains('"'));
        assert_eq!(url(Kind::First, "ra:1/2"), "https://rare.market/v1/trophies/badge/first.svg?game=ra%3A1%2F2");
    }
}
