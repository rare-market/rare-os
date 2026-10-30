//! Small shared pieces of the notify screens: timeAgo, the price labels, NoticeArt (Toast.tsx).

use super::data::types::Notice;
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Fit, Id, ImgReq, Node, Src};

/// marketplace/format.ts timeAgo for an epoch time in ms: "12s", "3m", "2h", "4d", "1mo", "1y".
pub fn time_ago(t: Option<i64>, now: i64) -> String {
    let Some(t) = t.filter(|t| *t != 0) else { return "\u{2014}".into() };
    let s = ((now - t) as f64 / 1000.0).round().max(0.0) as i64;
    if s < 60 {
        return format!("{s}s");
    }
    let m = s / 60;
    if m < 60 {
        return format!("{m}m");
    }
    let h = m / 60;
    if h < 24 {
        return format!("{h}h");
    }
    let d = h / 24;
    if d < 30 {
        return format!("{d}d");
    }
    let mo = d / 30;
    if mo < 12 {
        return format!("{mo}mo");
    }
    format!("{}y", d / 365)
}

fn group(int: u64) -> String {
    let s = int.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn to_precision(v: f64, p: i32) -> String {
    let e = v.abs().log10().floor() as i32;
    let dec = (p - 1 - e).max(0) as usize;
    format!("{v:.dec$}")
}

const SUB: [char; 10] = ['\u{2080}', '\u{2081}', '\u{2082}', '\u{2083}', '\u{2084}', '\u{2085}', '\u{2086}', '\u{2087}', '\u{2088}', '\u{2089}'];

/// ui/format.ts usdPrice: 3 significant digits, subscript zeros for tiny prices; "" when unknown.
pub fn usd_price(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return String::new();
    }
    if value >= 1000.0 {
        let cents = (value * 100.0).round() as u64;
        return format!("${}.{:02}", group(cents / 100), cents % 100);
    }
    if value >= 1.0 {
        return format!("${:.*}", if value >= 100.0 { 2 } else { 3 }, value);
    }
    if value >= 0.001 {
        return format!("${}", to_precision(value, 3));
    }
    let zeros = (-value.log10()).ceil() as i32 - 1;
    let digits = (value * 10f64.powi(zeros + 4)).round() as u64;
    let d = digits.to_string();
    let d = d.trim_end_matches('0');
    let sub: String = zeros.to_string().chars().map(|c| SUB[c.to_digit(10).unwrap_or(0) as usize]).collect();
    format!("$0.0{sub}{}", if d.is_empty() { "0" } else { d })
}

fn trim_zeros(t: &str) -> String {
    if !t.contains('.') {
        return t.to_string();
    }
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// numText("amount", abs) for the sizes a percent takes here.
pub fn amount_text(abs: f64) -> String {
    if abs >= 1000.0 {
        return format!("{}", group(abs.round() as u64));
    }
    if abs >= 1.0 {
        return trim_zeros(&format!("{abs:.2}"));
    }
    if abs == 0.0 {
        return "0".into();
    }
    trim_zeros(&to_precision(abs, 4))
}

/// Where a notice's art comes from: the host resolves a url to bytes (rare-images later); the
/// fixtures hand back bundled art.
pub trait ArtSource {
    fn art(&self, n: &Notice) -> Option<Src>;
}

/// No art: every notice gets its letter tile.
pub struct NoArt;
impl ArtSource for NoArt {
    fn art(&self, _n: &Notice) -> Option<Src> {
        None
    }
}

/// Toast.tsx NoticeArt: a rounded-card tile, bg-l3, the image covering it, or the label's first
/// letter (MImg's fallback) when there is none.
pub fn notice_art(cx: &mut Cx, parent: Id, n: &Notice, src: Option<Src>, side_rem: f32) -> Id {
    let r = cx.r();
    let px = side_rem * r;
    let tile = cx.scene.add(parent, s().wh(px, px).items_center().justify_center().done(), Node::bg(tok::L3).radius(0.5 * r).clip());
    if let Some(src) = src {
        let k = cx.img.request(ImgReq { src, w: (px * 2.0) as u32, h: (px * 2.0) as u32, fit: Fit::Cover { fy: 0.5 } });
        cx.scene.add(tile, s().fill().done(), Node::default().image(k));
    } else {
        let label = n.item.as_ref().map(|i| i.name.clone()).filter(|x| !x.is_empty()).or_else(|| n.amount.as_ref().map(|a| a.symbol.clone()).filter(|x| !x.is_empty())).unwrap_or_else(|| n.title.clone());
        if let Some(c) = label.trim().chars().find(|c| c.is_alphanumeric()) {
            let up: String = c.to_uppercase().collect();
            cx.scene.add_text(cx.t, tile, s().done(), &up, ts(r, 1.6, 2.0, 600), tok::MUTED.alpha(0.6));
        }
    }
    tile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_ago_steps() {
        let now = 10_000_000_000;
        assert_eq!(time_ago(Some(now - 5_000), now), "5s");
        assert_eq!(time_ago(Some(now - 3 * 60_000), now), "3m");
        assert_eq!(time_ago(Some(now - 26 * 3_600_000), now), "1d");
        assert_eq!(time_ago(Some(now - 2 * 86_400_000), now), "2d");
        assert_eq!(time_ago(Some(now - 40 * 86_400_000), now), "1mo");
        assert_eq!(time_ago(None, now), "\u{2014}");
        assert_eq!(time_ago(Some(now + 5_000), now), "0s");
    }

    #[test]
    fn usd_price_labels() {
        assert_eq!(usd_price(1.1), "$1.100");
        assert_eq!(usd_price(0.87), "$0.870");
        assert_eq!(usd_price(1234.5), "$1,234.50");
        assert_eq!(usd_price(150.0), "$150.00");
        assert_eq!(usd_price(0.00004123), "$0.0\u{2084}4123");
        assert_eq!(usd_price(0.0), "");
    }

    #[test]
    fn amount_text_trims() {
        assert_eq!(amount_text(10.0), "10");
        assert_eq!(amount_text(2.5), "2.5");
        assert_eq!(amount_text(0.0), "0");
        assert_eq!(amount_text(0.5), "0.5");
    }
}
