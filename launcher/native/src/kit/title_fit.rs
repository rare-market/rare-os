//! Text covers for games with no art (Art.tsx `Generated`): the title centred in its box, broken
//! only at spaces into lines of even length, smaller when a word would not fit, and cut with an
//! ellipsis only when even the smallest size runs out of room. Widths are estimated from Inter
//! SemiBold's advances (the view has no text measurer), with some air kept for the estimate.

use super::{s, tok, Ty, Ui, T};
use rare_render::view::El;

/// Inter SemiBold advance of `c` in em (close enough to place line breaks).
fn adv(c: char) -> f32 {
    match c {
        ' ' => 0.27,
        'i' | 'l' | 'j' | 'I' | '.' | ',' | '\'' | ':' | ';' | '!' | '|' => 0.30,
        'f' | 't' | 'r' | '(' | ')' | '-' => 0.40,
        'm' | 'w' => 0.86,
        'M' | 'W' => 0.96,
        c if c.is_ascii_uppercase() => 0.70,
        c if c.is_ascii_digit() => 0.62,
        c if c.is_ascii_lowercase() => 0.57,
        // CJK and other full-width scripts.
        c if (c as u32) >= 0x2E80 => 1.0,
        _ => 0.64,
    }
}

/// A line's width in em, estimated (SemiBold advances, so a little wide for lighter text).
pub fn width(s: &str) -> f32 {
    s.chars().map(adv).sum()
}

/// Greedy lines no wider than `max` em (a word wider than `max` gets its own line).
fn greedy<'a>(words: &[&'a str], max: f32) -> Vec<Vec<&'a str>> {
    let mut lines: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut w = 0.0;
    for word in words {
        let ww = width(word);
        let add = if cur.is_empty() { ww } else { w + adv(' ') + ww };
        if !cur.is_empty() && add > max {
            lines.push(std::mem::take(&mut cur));
            cur.push(word);
            w = ww;
        } else {
            cur.push(word);
            w = add;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// `s` cut to `max` em with an ellipsis.
fn ellipsize(s: &str, max: f32) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    let room = max - adv('\u{2026}');
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        if w + adv(c) > room {
            break;
        }
        w += adv(c);
        out.push(c);
    }
    format!("{}\u{2026}", out.trim_end())
}

/// Lays `title` out for a box `w` x `h` rem (its inner area): the lines joined with '\n' and the
/// font size in rem, between `base` and `min`. Line height is 1.25 x size.
pub fn fit_title(title: &str, w: f32, h: f32, base: f32, min: f32, max_lines: usize) -> (String, f32) {
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.is_empty() {
        return (String::new(), base);
    }
    // Room kept for the width estimate.
    let w = w * 0.94;
    let mut size = base;
    loop {
        let avail = w / size;
        let last = size <= min + 1e-3;
        let widest = words.iter().map(|x| width(x)).fold(0.0, f32::max);
        let fits_words = widest <= avail;
        let lines = greedy(&words, avail);
        let rows = ((h / (size * 1.25)).floor() as usize).clamp(1, max_lines);
        if fits_words && lines.len() <= rows {
            // Even lines: the narrowest width that still needs no more lines.
            let n = lines.len();
            let (mut lo, mut hi) = (widest, avail);
            for _ in 0..24 {
                let mid = (lo + hi) / 2.0;
                if greedy(&words, mid).len() <= n {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let text = greedy(&words, hi).iter().map(|l| l.join(" ")).collect::<Vec<_>>().join("\n");
            return (text, size);
        }
        if last {
            // The smallest size: cut long words, then the lines past the room, with an ellipsis.
            let cut: Vec<String> = words.iter().map(|x| ellipsize(x, avail)).collect();
            let refs: Vec<&str> = cut.iter().map(String::as_str).collect();
            let mut lines: Vec<String> = greedy(&refs, avail).iter().map(|l| l.join(" ")).collect();
            if lines.len() > rows {
                lines.truncate(rows);
                let l = lines.pop().unwrap_or_default();
                lines.push(ellipsize(&format!("{l}\u{2026}"), avail).replace("\u{2026}\u{2026}", "\u{2026}"));
            }
            return (lines.join("\n"), size);
        }
        size = (size * 0.92).max(min);
        size = (size * 100.0).round() / 100.0;
    }
}

/// A text cover filling its parent (`w` x `h` rem): the title fitted by `fit_title` on layer 03.
pub fn title_cover(ui: &mut Ui, title: &str, w: f32, h: f32, base: f32) -> El {
    let (pw, ph) = (w * 0.1, h * 0.1);
    // Not below 80 % of the base: a word too long even then is cut with an ellipsis, still readable.
    let (text, size) = fit_title(title, w - 2.0 * pw, h - 2.0 * ph, base, (base * 0.8).max(0.7), 5);
    ui.bx(s().fill().row().items_center().justify_center().px(pw).py(ph))
        .bg(tok::L3)
        .clip()
        .child(ui.text_s(s().wp(1.0), text, T::new(Ty::Px(size, size * 1.25)).w(600).c(tok::fg(0.8)).center()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_breaks_inside_a_word() {
        for t in ["SuperTuxKart", "The Witcher 3", "Hyperlightdriftercollectorsedition", "A Short Hike", "Baldur's Gate 3: Deluxe Edition of the Year"] {
            let (text, size) = fit_title(t, 7.2, 10.8, 1.05, 0.63, 4);
            for line in text.lines() {
                for word in line.split(' ') {
                    let w = word.trim_end_matches('\u{2026}');
                    assert!(t.contains(w), "{t:?} -> {text:?} has a broken word {word:?}");
                }
                assert!(width(line) * size <= 7.2 + 1e-3, "{t:?} line {line:?} too wide at {size}");
            }
        }
    }

    #[test]
    fn lines_are_even() {
        let (text, _) = fit_title("The Legend of the Long Title Game", 7.2, 10.8, 1.05, 0.63, 4);
        let ws: Vec<f32> = text.lines().map(width).collect();
        let (lo, hi) = (ws.iter().cloned().fold(f32::MAX, f32::min), ws.iter().cloned().fold(0.0, f32::max));
        assert!(hi - lo < 4.0, "{text:?}");
    }
}
