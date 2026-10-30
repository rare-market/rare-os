//! Editable text for the on-screen keyboard and search fields: a cursor and a selection on
//! grapheme boundaries, word moves, a composition (preedit) the OSK drives itself (dead keys,
//! a CJK candidate row: no system IME runs under gamescope), secret masking, and the view that
//! lays it out, keeps the caret in sight and draws caret, selection and composition underline.
//!
//! The look follows components/Osk.tsx: the text in the field's style, the caret a 2 px bar of
//! the text colour 1 px after the insertion point, pulsing like Tailwind's `animate-pulse`
//! (opacity 1 -> 0.5 -> 1 over 2 s, cubic-bezier(0.4, 0, 0.6, 1)).

use super::{TextPaint, TextStyle, TextSys};
use crate::anim::Ease;
use crate::color::Color;
use crate::gpu::{DrawList, Prim, KIND_RECT};
use cosmic_text::{Buffer, Cursor};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// The secret field's dot (Osk.tsx shows "•" per character).
pub const DOT: char = '•';

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compose {
    pub text: String,
    /// Caret inside the composition (byte offset).
    pub cursor: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditText {
    text: String,
    /// Byte offset, always on a grapheme boundary.
    cursor: usize,
    /// The other end of the selection.
    anchor: Option<usize>,
    compose: Option<Compose>,
    pub secret: bool,
    /// Longest text in characters (None: no limit).
    pub max_chars: Option<usize>,
}

/// What a field draws: the text with the composition in place (dots when secret) and where the
/// caret, selection and composition fall in it (byte ranges of `text`).
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub text: String,
    pub caret: usize,
    pub selection: Option<Range<usize>>,
    pub compose: Option<Range<usize>>,
}

fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].grapheme_indices(true).next_back().map_or(0, |(j, _)| j)
}

fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].grapheme_indices(true).nth(1).map_or(s.len(), |(j, _)| i + j)
}

fn is_word(g: &str) -> bool {
    g.chars().any(char::is_alphanumeric)
}

/// Start of the word before `i` (skipping spaces and punctuation first, like Ctrl+Left).
fn word_left(s: &str, i: usize) -> usize {
    let mut seen_word = false;
    let mut at = i;
    for (j, w) in s[..i].split_word_bound_indices().rev() {
        if is_word(w) {
            seen_word = true;
            at = j;
        } else if seen_word {
            break;
        } else {
            at = j;
        }
    }
    at
}

fn word_right(s: &str, i: usize) -> usize {
    let mut seen_word = false;
    let mut at = i;
    for (j, w) in s[i..].split_word_bound_indices() {
        if is_word(w) {
            seen_word = true;
            at = i + j + w.len();
        } else if seen_word {
            break;
        } else {
            at = i + j + w.len();
        }
    }
    at
}

impl EditText {
    /// Text with the caret at its end (askText opens with the field's value).
    pub fn new(text: &str) -> EditText {
        EditText { text: text.to_string(), cursor: text.len(), ..Default::default() }
    }

    pub fn secret(mut self) -> EditText {
        self.secret = true;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor?;
        (a != self.cursor).then(|| a.min(self.cursor)..a.max(self.cursor))
    }

    pub fn composing(&self) -> Option<&Compose> {
        self.compose.as_ref()
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
        self.anchor = None;
        self.compose = None;
    }

    pub fn clear(&mut self) {
        self.set_text("");
    }

    /// Removes the selection; true when there was one.
    fn take_selection(&mut self) -> bool {
        match self.selection() {
            Some(r) => {
                self.text.replace_range(r.clone(), "");
                self.cursor = r.start;
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    /// Types `s` at the caret, replacing the selection (a running composition is committed first).
    pub fn insert(&mut self, s: &str) {
        self.commit();
        self.take_selection();
        let room = self.max_chars.map_or(usize::MAX, |m| m.saturating_sub(self.text.chars().count()));
        let s: String = s.chars().filter(|c| !c.is_control()).take(room).collect();
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }

    /// Delete key on the OSK (X): the selection, else the grapheme before the caret.
    pub fn backspace(&mut self) {
        if self.compose.is_some() {
            let c = self.compose.as_mut().unwrap();
            if c.cursor > 0 {
                let p = prev_boundary(&c.text, c.cursor);
                c.text.replace_range(p..c.cursor, "");
                c.cursor = p;
            }
            if c.text.is_empty() {
                self.compose = None;
            }
            return;
        }
        if !self.take_selection() && self.cursor > 0 {
            let p = prev_boundary(&self.text, self.cursor);
            self.text.replace_range(p..self.cursor, "");
            self.cursor = p;
        }
    }

    /// Forward delete.
    pub fn delete(&mut self) {
        if self.compose.is_some() {
            return;
        }
        if !self.take_selection() && self.cursor < self.text.len() {
            let n = next_boundary(&self.text, self.cursor);
            self.text.replace_range(self.cursor..n, "");
        }
    }

    fn go(&mut self, to: usize, select: bool) {
        self.commit();
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = to;
    }

    /// LB on the OSK. Without `select`, a selection collapses to its start.
    pub fn left(&mut self, select: bool) {
        let to = match (select, self.selection()) {
            (false, Some(r)) => r.start,
            _ => prev_boundary(&self.text, self.cursor),
        };
        self.go(to, select);
    }

    /// RB on the OSK.
    pub fn right(&mut self, select: bool) {
        let to = match (select, self.selection()) {
            (false, Some(r)) => r.end,
            _ => next_boundary(&self.text, self.cursor),
        };
        self.go(to, select);
    }

    pub fn word_left(&mut self, select: bool) {
        self.go(word_left(&self.text, self.cursor), select);
    }

    pub fn word_right(&mut self, select: bool) {
        self.go(word_right(&self.text, self.cursor), select);
    }

    pub fn home(&mut self, select: bool) {
        self.go(0, select);
    }

    pub fn end(&mut self, select: bool) {
        self.go(self.text.len(), select);
    }

    pub fn select_all(&mut self) {
        self.commit();
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// Puts the caret at byte `i` (a tap), snapped to a grapheme boundary.
    pub fn place(&mut self, i: usize, select: bool) {
        let mut i = i.min(self.text.len());
        while !self.text.is_char_boundary(i) {
            i -= 1;
        }
        let snapped = self.text.grapheme_indices(true).map(|(j, _)| j).take_while(|&j| j <= i).last().unwrap_or(0);
        let snapped = if i == self.text.len() { i } else { snapped };
        self.go(snapped, select);
    }

    /// Sets the composition shown at the caret (the selection is replaced when it commits).
    pub fn compose(&mut self, text: &str, cursor: usize) {
        if text.is_empty() {
            self.compose = None;
            return;
        }
        let cursor = cursor.min(text.len());
        self.compose = Some(Compose { text: text.to_string(), cursor });
    }

    /// Commits the composition as typed text.
    pub fn commit(&mut self) {
        if let Some(c) = self.compose.take() {
            self.insert(&c.text);
        }
    }

    pub fn cancel_compose(&mut self) {
        self.compose = None;
    }

    /// The text as drawn.
    pub fn shown(&self) -> Shown {
        let mask = |s: &str| -> String {
            if self.secret {
                s.chars().map(|_| DOT).collect()
            } else {
                s.to_string()
            }
        };
        let (before, after) = self.text.split_at(self.cursor);
        let (before, after) = (mask(before), mask(after));
        let map = |i: usize| -> usize {
            if self.secret {
                self.text[..i].chars().count() * DOT.len_utf8()
            } else {
                i
            }
        };
        match &self.compose {
            Some(c) => {
                let start = before.len();
                let text = format!("{before}{}{after}", c.text);
                Shown { caret: start + c.cursor, selection: None, compose: Some(start..start + c.text.len()), text }
            }
            None => Shown { caret: before.len(), selection: self.selection().map(|r| map(r.start)..map(r.end)), compose: None, text: format!("{before}{after}") },
        }
    }
}

/// Tailwind `animate-pulse` opacity at time `t` seconds.
pub fn pulse(t: f64) -> f32 {
    const CURVE: Ease = Ease(0.4, 0.0, 0.6, 1.0);
    let phase = (t.rem_euclid(2.0) / 2.0) as f32;
    if phase < 0.5 {
        1.0 - 0.5 * CURVE.y(phase * 2.0)
    } else {
        0.5 + 0.5 * CURVE.y((phase - 0.5) * 2.0)
    }
}

/// How the field draws its decorations.
#[derive(Clone, Copy, Debug)]
pub struct EditLook {
    pub caret: Color,
    /// Caret height (Osk: 1.5rem), centred on the line.
    pub caret_h: f32,
    pub caret_w: f32,
    /// Gap between the insertion point and the caret (Osk: ml-[1px]).
    pub caret_gap: f32,
    pub selection: Color,
    /// Composition underline thickness.
    pub underline: f32,
    /// Room kept between the caret and the field's edges when scrolling.
    pub margin: f32,
}

impl EditLook {
    pub fn osk(rem: f32, fg: Color) -> EditLook {
        EditLook { caret: fg, caret_h: 1.5 * rem, caret_w: 2.0, caret_gap: 1.0, selection: fg.alpha(0.25), underline: (rem / 16.0).max(1.0).round(), margin: rem }
    }
}

/// A field's laid-out text: one line, scrolled sideways so the caret stays in view.
pub struct EditView {
    pub buf: Buffer,
    pub style: TextStyle,
    shown: Option<Shown>,
    /// How far the text is scrolled left, px.
    pub scroll: f32,
}

impl EditView {
    pub fn new(ts: &mut TextSys, style: TextStyle) -> EditView {
        let style = TextStyle { wrap: false, truncate: false, clamp: 0, ..style };
        EditView { buf: ts.buffer("", &style), style, shown: None, scroll: 0.0 }
    }

    /// Lays out `edit` if it changed and scrolls the caret into a field `width` wide.
    pub fn sync(&mut self, ts: &mut TextSys, edit: &EditText, width: f32, look: &EditLook) {
        let shown = edit.shown();
        if self.shown.as_ref().map(|s| &s.text) != Some(&shown.text) {
            ts.set(&mut self.buf, &shown.text, &self.style);
            ts.measure(&mut self.buf, None);
        }
        self.shown = Some(shown);
        let x = self.x_of(self.shown.as_ref().unwrap().caret);
        let w = self.width();
        let room = (width - look.caret_gap - look.caret_w).max(0.0);
        if x - self.scroll > room - look.margin.min(room / 2.0) {
            self.scroll = x - room + look.margin.min(room / 2.0);
        }
        if x - self.scroll < look.margin.min(room / 2.0) {
            self.scroll = x - look.margin.min(room / 2.0);
        }
        // Never scroll past the text's end or before its start.
        self.scroll = self.scroll.min((w - room).max(0.0)).max(0.0);
    }

    pub fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    pub fn width(&self) -> f32 {
        self.buf.layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
    }

    /// X of byte `i` of the shown text, in text space.
    pub fn x_of(&self, i: usize) -> f32 {
        let Some(run) = self.buf.layout_runs().next() else { return 0.0 };
        if i >= run.text.len() {
            return run.line_w;
        }
        run.cursor_position(&Cursor::new(0, i)).unwrap_or_else(|| run.glyphs.iter().find(|g| g.start >= i).map_or(run.line_w, |g| g.x))
    }

    /// Byte offset of the edit text under field-space x (taps). Secret fields map dots back.
    pub fn hit(&self, edit: &EditText, x: f32) -> usize {
        let line = self.buf.metrics().line_height;
        let i = self.buf.hit(x + self.scroll, line / 2.0).map_or(0, |c| c.index);
        if edit.secret {
            let n = self.shown.as_ref().map_or("", |s| &s.text[..i.min(s.text.len())]).chars().count();
            edit.text().char_indices().nth(n).map_or(edit.text().len(), |(j, _)| j)
        } else {
            i.min(edit.text().len())
        }
    }

    /// Draws selection, text, composition underline and caret. `tp` is the text box's top-left
    /// (the field's padding already applied); the text is clipped to `tp.clip`.
    pub fn paint(&self, ts: &mut TextSys, tp: &TextPaint, look: &EditLook, focused: bool, now: f64, out: &mut DrawList) {
        let Some(shown) = &self.shown else { return };
        let (s, sy) = (tp.scale, tp.scale_y);
        let line = self.buf.metrics().line_height;
        let x0 = tp.x - self.scroll * s;
        let rect = |out: &mut DrawList, x: f32, y: f32, w: f32, h: f32, c: Color| {
            out.push(Prim { rect: [x, y, w, h], clip: tp.clip, c0: c.arr(), p: [0.0, 0.0, KIND_RECT, tp.opacity], ..Default::default() }, None);
        };
        if let Some(r) = &shown.selection {
            let (a, b) = (self.x_of(r.start), self.x_of(r.end));
            rect(out, x0 + a * s, tp.y, (b - a) * s, line * sy, look.selection);
        }
        ts.paint(&self.buf, &TextPaint { x: x0, ..*tp }, out);
        if let Some(r) = &shown.compose {
            let (a, b) = (self.x_of(r.start), self.x_of(r.end));
            let base = tp.y + (line * 0.5 + self.style.size * 0.45) * sy;
            rect(out, x0 + a * s, base.round(), (b - a) * s, look.underline * sy, tp.color);
        }
        if focused {
            let x = x0 + (self.x_of(shown.caret) + look.caret_gap) * s;
            let y = tp.y + (line - look.caret_h) / 2.0 * sy;
            rect(out, x.round(), y.round(), look.caret_w * s, look.caret_h * sy, look.caret.alpha(pulse(now)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_deleting() {
        let mut e = EditText::new("ha");
        e.insert("lo");
        assert_eq!(e.text(), "halo");
        e.backspace();
        e.backspace();
        assert_eq!((e.text(), e.cursor()), ("ha", 2));
        e.left(false);
        e.insert("e");
        assert_eq!(e.text(), "hea");
        e.delete();
        assert_eq!(e.text(), "he");
    }

    #[test]
    fn graphemes_move_and_delete_whole() {
        let mut e = EditText::new("a👍🏽e\u{301}");
        e.backspace();
        assert_eq!(e.text(), "a👍🏽");
        e.left(false);
        assert_eq!(e.cursor(), 1);
        e.right(false);
        assert_eq!(e.cursor(), e.text().len());
        e.backspace();
        assert_eq!(e.text(), "a");
    }

    #[test]
    fn selection_replace_and_collapse() {
        let mut e = EditText::new("Hollow Keep");
        e.word_left(true);
        assert_eq!(e.selection(), Some(7..11));
        e.insert("Hill");
        assert_eq!(e.text(), "Hollow Hill");
        e.select_all();
        e.left(false);
        assert_eq!((e.cursor(), e.selection()), (0, None));
        e.end(false);
        e.left(true);
        e.left(true);
        assert_eq!(e.selection(), Some(9..11));
        e.right(false);
        assert_eq!((e.cursor(), e.selection()), (11, None));
    }

    #[test]
    fn words() {
        let mut e = EditText::new("0xabcd::coin::SUN  next");
        e.word_left(false);
        assert_eq!(&e.text()[e.cursor()..], "next");
        e.word_left(false);
        assert_eq!(&e.text()[e.cursor()..], "SUN  next");
        e.home(false);
        e.word_right(false);
        assert_eq!(&e.text()[..e.cursor()], "0xabcd");
    }

    #[test]
    fn composition() {
        let mut e = EditText::new("ab");
        e.left(false);
        e.compose("か", 3);
        let s = e.shown();
        assert_eq!(s.text, "aかb");
        assert_eq!((s.caret, s.compose), (4, Some(1..4)));
        e.compose("かん", 6);
        e.backspace();
        assert_eq!(e.composing().unwrap().text, "か");
        e.compose("漢", 3);
        e.commit();
        assert_eq!((e.text(), e.cursor()), ("a漢b", 4));
        e.compose("x", 1);
        e.cancel_compose();
        assert_eq!(e.text(), "a漢b");
        // Moving commits.
        e.compose("y", 1);
        e.end(false);
        assert_eq!(e.text(), "a漢yb");
    }

    #[test]
    fn secret_masks_and_maps() {
        let mut e = EditText::new("pässwörd").secret();
        let s = e.shown();
        assert_eq!(s.text, "••••••••");
        assert_eq!(s.caret, s.text.len());
        e.home(false);
        e.right(true);
        e.right(true);
        assert_eq!(e.shown().selection, Some(0..2 * DOT.len_utf8()));
    }

    #[test]
    fn max_chars() {
        let mut e = EditText { max_chars: Some(4), ..EditText::new("abc") };
        e.insert("def");
        assert_eq!(e.text(), "abcd");
        e.insert("\n\t");
        assert_eq!(e.text(), "abcd");
    }

    #[test]
    fn pulse_curve() {
        assert_eq!(pulse(0.0), 1.0);
        assert!((pulse(1.0) - 0.5).abs() < 1e-4);
        assert!((pulse(2.0) - 1.0).abs() < 1e-4);
        assert!(pulse(0.5) < 1.0 && pulse(0.5) > 0.5);
    }

    #[test]
    fn view_scrolls_caret_into_sight_and_hits() {
        let mut ts = TextSys::new(&[], false, 1024);
        let st = TextStyle::new(17.0, 400).line(24.0).mono();
        let look = EditLook::osk(16.0, Color::WHITE);
        let mut v = EditView::new(&mut ts, st);
        let mut e = EditText::new("0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e");
        v.sync(&mut ts, &e, 200.0, &look);
        let caret = v.x_of(v.shown().unwrap().caret);
        assert!(caret - v.scroll <= 200.0 && caret - v.scroll > 150.0, "{caret} {}", v.scroll);
        e.home(false);
        v.sync(&mut ts, &e, 200.0, &look);
        assert_eq!(v.scroll, 0.0);
        // Fira Code: every character the same advance.
        let adv = v.x_of(1);
        let i = v.hit(&e, adv * 5.0 + adv * 0.2);
        assert_eq!(i, 5);
        let mut out = DrawList::default();
        v.paint(&mut ts, &TextPaint::at(10.0, 10.0, Color::WHITE), &look, true, 0.0, &mut out);
        let caret = out.prims.last().unwrap();
        assert_eq!(caret.p[2], KIND_RECT);
        assert_eq!(caret.rect, [11.0, 10.0, 2.0, 24.0]);
    }

    #[test]
    fn view_draws_selection_under_text() {
        let mut ts = TextSys::new(&[], false, 1024);
        let look = EditLook::osk(16.0, Color::WHITE);
        let mut v = EditView::new(&mut ts, TextStyle::new(17.0, 400).line(24.0));
        let mut e = EditText::new("Hollow Keep");
        e.word_left(true);
        v.sync(&mut ts, &e, 400.0, &look);
        let mut out = DrawList::default();
        v.paint(&mut ts, &TextPaint::at(0.0, 0.0, Color::WHITE), &look, false, 0.0, &mut out);
        let sel = out.prims[0];
        assert_eq!(sel.p[2], KIND_RECT);
        assert!((sel.rect[0] - v.x_of(7)).abs() < 1e-3 && sel.rect[2] > 20.0);
    }
}
