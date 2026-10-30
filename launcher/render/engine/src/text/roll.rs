//! Rolling digits for Num (launcher/src/ui/Num.tsx, app.css `num-roll-*` and `tint-*`).
//!
//! When a live number changes, the characters that moved (compared from the right, so a new
//! leading digit counts as moved and the rest line up) slide in from 45 % of the line height
//! below (value went up) or above (down) while fading in, over 200 ms on the pro curve; the
//! whole number's background flashes the positive or negative tint at 16 % and fades out over
//! 600 ms. A new change restarts both. Nothing rolls when the number first appears, when either
//! value is unknown, or while the launcher is idle or away (the caller passes `animate: false`).
//!
//! Draw with `TextSys::paint_fx(buf, .., |_, g| roll.fx(char_index_of(g), line, now))`, or
//! `TextSys::paint_roll`, which does that for a one-line buffer.

use super::{GlyphFx, TextPaint, TextSys};
use crate::anim::Ease;
use crate::color::Color;
use crate::gpu::{DrawList, Prim, KIND_RECT};
use cosmic_text::Buffer;

/// --dur-base: the roll.
pub const ROLL_S: f32 = 0.2;
/// --dur-tint: the background flash.
pub const TINT_S: f32 = 0.6;
/// translateY(±45%) of the character's own box (an inline-block one line high).
pub const OFFSET: f32 = 0.45;
/// --tint-up / --tint-down alpha.
pub const TINT_ALPHA: f32 = 0.16;

/// ui/format.ts `changedChars`: for each character of `next`, whether it differs from the
/// character the same distance from the end of `prev`.
pub fn changed_chars(prev: &str, next: &str) -> Vec<bool> {
    let p: Vec<char> = prev.chars().collect();
    let n: Vec<char> = next.chars().collect();
    (0..n.len())
        .map(|i| {
            let from_end = n.len() - 1 - i;
            let before = (p.len() as isize - 1 - from_end as isize).try_into().ok().and_then(|j: usize| p.get(j));
            before != Some(&n[i])
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Roll {
    /// Frame time the change was seen.
    pub start: f64,
    pub up: bool,
    /// One per character of the shown text (body then dimmed cents).
    pub moved: Vec<bool>,
    /// Bumps per change (Num's `key={fx.version}`).
    pub version: u32,
}

/// Num's `useRoll` state for one number.
#[derive(Clone, Debug, Default)]
pub struct NumRoll {
    text: Option<String>,
    value: Option<f64>,
    roll: Option<Roll>,
}

impl NumRoll {
    pub fn new() -> NumRoll {
        NumRoll::default()
    }

    /// Feeds the shown text (None when hidden or not live) and the value. True when a roll
    /// started. `animate` false (idle, away, reduced motion) takes the new value without one.
    pub fn update(&mut self, text: Option<&str>, value: Option<f64>, now: f64, animate: bool) -> bool {
        if text == self.text.as_deref() {
            if text.is_none() {
                self.roll = None;
            }
            return false;
        }
        let prev = std::mem::replace(&mut self.text, text.map(str::to_string));
        let prev_value = std::mem::replace(&mut self.value, value);
        let (Some(t), Some(p), Some(v), Some(pv)) = (text, prev, value, prev_value) else {
            if text.is_none() {
                self.roll = None;
            }
            return false;
        };
        if !animate {
            self.roll = None;
            return false;
        }
        let version = self.roll.as_ref().map_or(0, |r| r.version) + 1;
        self.roll = Some(Roll { start: now, up: v >= pv, moved: changed_chars(&p, t), version });
        true
    }

    pub fn roll(&self) -> Option<&Roll> {
        self.roll.as_ref()
    }

    /// Still drawing (roll or tint in progress).
    pub fn active(&self, now: f64) -> bool {
        self.roll.as_ref().is_some_and(|r| ((now - r.start) as f32) < TINT_S)
    }

    /// Offset and fade of character `index` (of the shown text) in a line `line_px` high.
    pub fn fx(&self, index: usize, line_px: f32, now: f64) -> GlyphFx {
        let Some(r) = &self.roll else { return GlyphFx::NONE };
        if !r.moved.get(index).copied().unwrap_or(false) {
            return GlyphFx::NONE;
        }
        let t = ((now - r.start) as f32 / ROLL_S).clamp(0.0, 1.0);
        if t >= 1.0 {
            return GlyphFx::NONE;
        }
        let e = Ease::PRO.y(t);
        let dir = if r.up { 1.0 } else { -1.0 };
        GlyphFx { dx: 0.0, dy: dir * OFFSET * line_px * (1.0 - e), alpha: e }
    }

    /// The background flash: (up, alpha) while it shows.
    pub fn tint(&self, now: f64) -> Option<(bool, f32)> {
        let r = self.roll.as_ref()?;
        let t = ((now - r.start) as f32 / TINT_S).clamp(0.0, 1.0);
        (t < 1.0).then(|| (r.up, TINT_ALPHA * (1.0 - Ease::PRO.y(t))))
    }
}

/// Character index of a byte offset in `text`.
pub fn char_index(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or(0, |s| s.chars().count())
}

impl TextSys {
    /// Paints a one-line Num buffer with its roll and tint. `tint_colors` are the positive and
    /// negative text colours (the flash uses them at 16 %); the flash covers `box_wh` with
    /// `radius` (Num's `rounded-[0.25rem]`).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_roll(&mut self, buf: &Buffer, tp: &TextPaint, roll: &NumRoll, now: f64, box_wh: [f32; 2], radius: f32, tint_colors: [Color; 2], out: &mut DrawList) {
        if let Some((up, a)) = roll.tint(now) {
            let c = if up { tint_colors[0] } else { tint_colors[1] };
            out.push(
                Prim {
                    rect: [tp.x, tp.y, box_wh[0] * tp.scale, box_wh[1] * tp.scale_y],
                    clip: tp.clip,
                    c0: c.alpha(a).arr(),
                    p: [radius * tp.scale, 0.0, KIND_RECT, tp.opacity],
                    ..Default::default()
                },
                None,
            );
        }
        let line = buf.metrics().line_height * tp.scale_y;
        let mut offsets = Vec::new();
        let mut acc = 0;
        for l in &buf.lines {
            offsets.push(acc);
            acc += l.text().chars().count();
        }
        self.paint_fx(buf, tp, out, |li, g| {
            let text = buf.lines.get(li).map_or("", |l| l.text());
            roll.fx(offsets.get(li).copied().unwrap_or(0) + char_index(text, g.start), line, now)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_chars_matches_format_ts() {
        // Values from the TS: compared from the right.
        assert_eq!(changed_chars("$1.23", "$1.24"), vec![false, false, false, false, true]);
        assert_eq!(changed_chars("$9.99", "$10.00"), vec![true, true, true, false, true, true]);
        assert_eq!(changed_chars("", "12"), vec![true, true]);
        assert_eq!(changed_chars("1,234", "1,234"), vec![false; 5]);
        assert_eq!(changed_chars("0.0₄12", "0.0₅12"), vec![false, false, false, true, false, false]);
    }

    #[test]
    fn first_value_and_unknowns_do_not_roll() {
        let mut r = NumRoll::new();
        assert!(!r.update(Some("$1.00"), Some(1.0), 0.0, true));
        assert!(!r.update(Some("$2.00"), None, 1.0, true));
        assert!(!r.update(Some("$3.00"), Some(3.0), 2.0, true), "the previous value was unknown");
        assert!(r.update(Some("$2.50"), Some(2.5), 3.0, true));
        let roll = r.roll().unwrap();
        assert!(!roll.up);
        assert_eq!(roll.moved, vec![false, true, false, true, false]);
    }

    #[test]
    fn idle_takes_the_value_without_a_roll() {
        let mut r = NumRoll::new();
        r.update(Some("1"), Some(1.0), 0.0, true);
        assert!(!r.update(Some("2"), Some(2.0), 1.0, false));
        assert!(r.roll().is_none());
    }

    #[test]
    fn roll_curve() {
        let mut r = NumRoll::new();
        r.update(Some("10"), Some(10.0), 0.0, true);
        r.update(Some("11"), Some(11.0), 1.0, true);
        // Unmoved characters never move.
        assert_eq!(r.fx(0, 20.0, 1.05), GlyphFx::NONE);
        // Moved: starts 45 % of the line below, invisible; lands at 200 ms.
        let a = r.fx(1, 20.0, 1.0);
        assert!((a.dy - 9.0).abs() < 1e-3 && a.alpha == 0.0, "{a:?}");
        let m = r.fx(1, 20.0, 1.1);
        assert!(m.dy > 0.0 && m.dy < 2.0 && m.alpha > 0.8, "{m:?}");
        assert_eq!(r.fx(1, 20.0, 1.2), GlyphFx::NONE);
        // Tint at 16 %, gone at 600 ms.
        let (up, t0) = r.tint(1.0).unwrap();
        assert!(up && (t0 - 0.16).abs() < 1e-6);
        assert!(r.tint(1.6).is_none());
        assert!(r.active(1.5) && !r.active(1.61));
        // Down rolls from above.
        r.update(Some("9"), Some(9.0), 2.0, true);
        assert!(r.fx(0, 20.0, 2.0).dy < 0.0);
        assert_eq!(r.roll().unwrap().version, 2);
    }

    #[test]
    fn paints_moved_glyphs_offset() {
        let mut t = TextSys::new(&[], false, 512);
        let st = crate::text::TextStyle::new(20.0, 400).line(24.0).tabular();
        let buf = t.buffer("$1.24", &st);
        let mut r = NumRoll::new();
        r.update(Some("$1.23"), Some(1.23), 0.0, true);
        r.update(Some("$1.24"), Some(1.24), 1.0, true);
        let mut still = DrawList::default();
        let mut rolling = DrawList::default();
        let tp = TextPaint::at(0.0, 0.0, Color::WHITE);
        t.paint(&buf, &tp, &mut still);
        let tints = [Color::rgb(42, 223, 114), Color::rgb(234, 65, 65)];
        t.paint_roll(&buf, &tp, &r, 1.05, [60.0, 24.0], 4.0, tints, &mut rolling);
        // Tint rect first, then five glyphs; only the last glyph moved.
        assert_eq!(rolling.prims.len(), 6);
        assert_eq!(rolling.prims[0].p[2], KIND_RECT);
        for i in 0..4 {
            assert_eq!(still.prims[i].rect, rolling.prims[i + 1].rect);
        }
        assert!(rolling.prims[5].rect[1] > still.prims[4].rect[1]);
        assert!(rolling.prims[5].c0[3] < 1.0);
    }
}
