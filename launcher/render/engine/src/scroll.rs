//! Scroll containers on the scene side. A node made with `Node::scrolls()` (or a view element with
//! `scrolls_x` / `scrolls_y`) clips its children and paints them moved by its `scroll_x` /
//! `scroll_y`; Taffy gives the content extent (`Scene::max_scroll`). Focus-follow is
//! `focus::Nav`'s job (rails scroll just enough, pages keep the focused row a little above the
//! middle, held repeats retarget a 65 ms glide): `View::apply_scrolls` hands its offsets to the
//! scene. This module covers everything else that scrolls:
//!
//! - `to`: aim at an offset, gliding (focus.ts's motion) or at once, clamped to the content;
//! - `by`: move by a delta from where it is heading (a stick or a page-down), on a spring so
//!   repeated pushes add up without a stutter;
//! - `reveal`: bring a node into view without Nav (a chat log's last line, an error row);
//! - `clamp`: keep the offset inside content that shrank (views call it after layout).
//!
//! Offsets are always the target ("aim") the container is heading to, so a call during a glide
//! builds on the destination, never on a half-way value.

use crate::anim::Curve;
use crate::scene::{Id, Scene};

/// How `reveal` places a node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reveal {
    /// Scroll as little as possible to show it, keeping `pad` px of air.
    Nearest { pad: f32 },
    /// Put its centre at this fraction of the viewport (0.45: Nav's console shelf).
    At(f32),
    /// Its top at the viewport's top plus `pad` (Settings' `scrollIntoView({block: "start"})`).
    Start { pad: f32 },
    /// Its bottom at the viewport's bottom minus `pad` (a chat log's newest line).
    End { pad: f32 },
}

/// Where the container is heading: [x, y].
pub fn aim(scene: &Scene, id: Id) -> [f32; 2] {
    let n = scene.get(id);
    [n.scroll_x.target(), n.scroll_y.target()]
}

/// Where the container is now: [x, y].
pub fn offset(scene: &Scene, id: Id, now: f64) -> [f32; 2] {
    let n = scene.get(id);
    [n.scroll_x.get(now), n.scroll_y.get(now)]
}

/// Scrolls to `x` / `y` (None leaves that axis), clamped to the content. `smooth` glides there on
/// focus.ts's 65 ms curve; otherwise it jumps.
pub fn to(scene: &mut Scene, id: Id, x: Option<f32>, y: Option<f32>, now: f64, smooth: bool) {
    let (min, max) = (scene.min_scroll(id), scene.max_scroll(id));
    let n = scene.n(id);
    for (a, v, lo, m) in [(&mut n.scroll_x, x, min[0], max[0]), (&mut n.scroll_y, y, min[1], max[1])] {
        if let Some(v) = v {
            let v = v.clamp(lo, m);
            if smooth {
                a.set_curve(v, now, Curve::GLIDE, 0.0);
            } else {
                a.snap(v);
            }
        }
    }
}

/// Scrolls by a delta from the current aim on a spring (velocity carries across repeated calls).
/// Returns the remaining push that hit an edge ([0, 0] when it all fit).
pub fn by(scene: &mut Scene, id: Id, dx: f32, dy: f32, now: f64) -> [f32; 2] {
    let max = scene.max_scroll(id);
    let n = scene.n(id);
    let mut rest = [0.0; 2];
    for (i, (a, d, m)) in [(&mut n.scroll_x, dx, max[0]), (&mut n.scroll_y, dy, max[1])].into_iter().enumerate() {
        if d == 0.0 {
            continue;
        }
        // A push stops at the start (the ring room before it is focus's, not the stick's).
        let want = a.target() + d;
        let v = want.clamp(a.target().min(0.0), m);
        rest[i] = want - v;
        if (v - a.target()).abs() > 1e-3 {
            a.set_curve(v, now, Curve::SPRING, 0.0);
        }
    }
    rest
}

/// Scrolls `viewport` so `target` (a descendant) shows as `how` says, on both axes the viewport
/// can scroll. Uses layout positions, so it is right even mid-glide.
pub fn reveal(scene: &mut Scene, viewport: Id, target: Id, how: Reveal, now: f64, smooth: bool) {
    let Some(o) = scene.offset_in(target, viewport) else { return };
    let [_, _, vw, vh] = scene.rect(viewport);
    let (min, max) = (scene.min_scroll(viewport), scene.max_scroll(viewport));
    let cur = aim(scene, viewport);
    let pick = |pos: f32, size: f32, view: f32, cur: f32| -> f32 {
        match how {
            Reveal::Nearest { pad } => {
                if pos - pad < cur {
                    pos - pad
                } else if pos + size + pad > cur + view {
                    pos + size + pad - view
                } else {
                    cur
                }
            }
            Reveal::At(f) => pos + size / 2.0 - view * f,
            Reveal::Start { pad } => pos - pad,
            Reveal::End { pad } => pos + size + pad - view,
        }
    };
    let x = (max[0] > 0.0 || min[0] < 0.0).then(|| pick(o[0], o[2], vw, cur[0]));
    let y = (max[1] > 0.0 || min[1] < 0.0).then(|| pick(o[1], o[3], vh, cur[1]));
    to(scene, viewport, x, y, now, smooth);
}

/// Air a focused row keeps from a vertical scroller's edges (focus.ts: `topPad` and the 12 px under).
pub const FOLLOW_PAD: f32 = 12.0;
/// Where a row that left the view lands: its centre at this fraction of the viewport (a console shelf).
pub const FOLLOW_AT: f32 = 0.45;

/// The scroll containers above `id`, innermost first.
pub fn scrollers_above(scene: &Scene, id: Id) -> Vec<Id> {
    let mut out = Vec::new();
    let mut p = scene.parent(id);
    while let Some(a) = p {
        if scene.try_get(a).is_some_and(|n| n.scroll) {
            out.push(a);
        }
        p = scene.parent(a);
    }
    out
}

/// Focus-follow for every scroll container above `target`, with nothing to register: what
/// `focus::Nav` does for a view's `[data-rail]` / `[data-scroll]` containers (focus.ts `reveal`),
/// for any `Node::scrolls()` box a screen built. Sideways, a strip scrolls just enough to show the
/// target with its left padding as air; vertically, a list leaves the row alone while it is in view
/// (12 px of air at both edges) and otherwise brings it a little above the middle (0.45), clamped
/// to the ends. Nested containers each follow: an outer one measures the target where the inner
/// ones are heading. `smooth` glides on focus.ts's 65 ms curve, retargeting a glide in flight.
pub fn follow(scene: &mut Scene, target: Id, now: f64, smooth: bool) {
    let chain = scrollers_above(scene, target);
    for (i, &c) in chain.iter().enumerate() {
        let Some(mut o) = scene.offset_in(target, c) else { continue };
        // Inner containers move the target by where they are heading.
        for &inner in &chain[..i] {
            let a = aim(scene, inner);
            o[0] -= a[0];
            o[1] -= a[1];
        }
        let [_, _, vw, vh] = scene.rect(c);
        let (min, max) = (scene.min_scroll(c), scene.max_scroll(c));
        let cur = aim(scene, c);
        // A row scrolls sideways (a strip), anything else up and down (a list or a page).
        let row = matches!(scene.style(c).flex_direction, taffy::style::FlexDirection::Row | taffy::style::FlexDirection::RowReverse);
        let x = (row && (max[0] > 0.5 || min[0] < -0.5)).then(|| {
            let pad = scene.padding(c)[3];
            if o[0] - pad < cur[0] {
                o[0] - pad
            } else if o[0] + o[2] + pad > cur[0] + vw {
                o[0] + o[2] + pad - vw
            } else {
                cur[0]
            }
        });
        let y = (!row && (max[1] > 0.5 || min[1] < -0.5)).then(|| {
            let outside = o[1] - FOLLOW_PAD < cur[1] || o[1] + o[3] + FOLLOW_PAD > cur[1] + vh;
            if !outside {
                return cur[1];
            }
            let t = o[1] + o[3] / 2.0 - vh * FOLLOW_AT;
            if o[1] - FOLLOW_PAD < t {
                o[1] - FOLLOW_PAD
            } else {
                t
            }
        });
        let x = x.filter(|v| (v.clamp(min[0], max[0]) - cur[0]).abs() > 0.25);
        let y = y.filter(|v| (v.clamp(min[1], max[1]) - cur[1]).abs() > 0.25);
        if x.is_some() || y.is_some() {
            to(scene, c, x, y, now, smooth);
        }
    }
}

/// Keeps the offset inside the content (after the content shrank). True when it moved.
pub fn clamp(scene: &mut Scene, id: Id) -> bool {
    let Some(n) = scene.try_get(id) else { return false };
    if !n.scroll {
        return false;
    }
    let (min, max) = (scene.min_scroll(id), scene.max_scroll(id));
    let n = scene.n(id);
    let mut moved = false;
    for (a, lo, m) in [(&mut n.scroll_x, min[0], max[0]), (&mut n.scroll_y, min[1], max[1])] {
        if a.target() > m + 0.01 || a.target() < lo - 0.01 {
            a.snap(a.target().clamp(lo, m));
            moved = true;
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Node;
    use crate::text::TextSys;
    use taffy::prelude::*;

    fn page() -> (Scene, Id, Vec<Id>) {
        let mut s = Scene::new(200.0, 100.0, 16.0);
        let mut ts = TextSys::new(&[], false, 64);
        let st = Style { flex_direction: FlexDirection::Column, size: Size { width: length(200.0), height: length(100.0) }, ..Default::default() };
        let v = s.add(s.root, st, Node::default().scrolls());
        let rows = (0..10).map(|_| s.add(v, Style { size: Size { width: length(200.0), height: length(40.0) }, flex_shrink: 0.0, ..Default::default() }, Node::default())).collect();
        s.layout(&mut ts);
        (s, v, rows)
    }

    #[test]
    fn to_clamps_and_glides() {
        let (mut s, v, _) = page();
        // The content's end plus room for a focus ring (0.3125 rem = 5 px), and as much before
        // the start (no padding there).
        assert_eq!(s.max_scroll(v), [0.0, 305.0]);
        assert_eq!(s.min_scroll(v), [0.0, -5.0]);
        to(&mut s, v, None, Some(1000.0), 0.0, true);
        assert_eq!(aim(&s, v), [0.0, 305.0]);
        let mid = offset(&s, v, 0.15)[1];
        assert!(mid > 250.0 && mid < 305.0, "{mid}");
        assert_eq!(offset(&s, v, 2.0)[1], 305.0);
        to(&mut s, v, None, Some(-50.0), 2.0, false);
        assert_eq!(offset(&s, v, 2.0)[1], -5.0);
    }

    #[test]
    fn by_adds_up_and_reports_the_edge() {
        let (mut s, v, _) = page();
        assert_eq!(by(&mut s, v, 0.0, 120.0, 0.0), [0.0, 0.0]);
        assert_eq!(by(&mut s, v, 0.0, 120.0, 0.01), [0.0, 0.0]);
        assert_eq!(aim(&s, v)[1], 240.0);
        assert_eq!(by(&mut s, v, 0.0, 120.0, 0.02), [0.0, 55.0]);
        assert_eq!(aim(&s, v)[1], 305.0);
        // A push back stops at the start, not in the ring room before it.
        by(&mut s, v, 0.0, -1000.0, 0.03);
        assert_eq!(aim(&s, v)[1], 0.0);
    }

    #[test]
    fn reveal_modes() {
        let (mut s, v, rows) = page();
        reveal(&mut s, v, rows[5], Reveal::Nearest { pad: 10.0 }, 0.0, false);
        assert_eq!(aim(&s, v)[1], 200.0 + 40.0 + 10.0 - 100.0);
        reveal(&mut s, v, rows[5], Reveal::Nearest { pad: 10.0 }, 0.0, false);
        assert_eq!(aim(&s, v)[1], 150.0, "already in view: no move");
        reveal(&mut s, v, rows[5], Reveal::At(0.5), 0.0, false);
        assert_eq!(aim(&s, v)[1], 220.0 - 50.0);
        reveal(&mut s, v, rows[9], Reveal::End { pad: 0.0 }, 0.0, false);
        assert_eq!(aim(&s, v)[1], 300.0);
        reveal(&mut s, v, rows[1], Reveal::Start { pad: 0.0 }, 0.0, false);
        assert_eq!(aim(&s, v)[1], 40.0);
    }

    #[test]
    fn follow_keeps_the_row_in_view_without_registering() {
        let (mut s, v, rows) = page();
        // In view: no move.
        follow(&mut s, rows[1], 0.0, false);
        assert_eq!(aim(&s, v)[1], 0.0);
        // Row 4 (160..200) runs past the 100 px view: its centre goes to 45 %, clamped.
        follow(&mut s, rows[4], 0.0, false);
        assert_eq!(aim(&s, v)[1], 180.0 - 45.0);
        // The last row: the end, with its ring's room under it.
        follow(&mut s, rows[9], 0.0, true);
        assert_eq!(aim(&s, v)[1], 305.0);
        assert!(offset(&s, v, 0.05)[1] < 305.0, "glides");
        // Back up to the first: 12 px of air wins over the shelf, clamped to the ring's room.
        follow(&mut s, rows[0], 1.0, false);
        assert_eq!(aim(&s, v)[1], -5.0);
    }

    #[test]
    fn follow_scrolls_nested_containers() {
        let mut s = Scene::new(200.0, 100.0, 16.0);
        let mut ts = TextSys::new(&[], false, 64);
        let col = |h: f32| Style { flex_direction: FlexDirection::Column, size: Size { width: length(200.0), height: length(h) }, flex_shrink: 0.0, ..Default::default() };
        let outer = s.add(s.root, col(100.0), Node::default().scrolls());
        let head = s.add(outer, col(80.0), Node::default());
        let inner = s.add(outer, col(60.0), Node::default().scrolls());
        let rows: Vec<Id> = (0..5).map(|_| s.add(inner, col(30.0), Node::default())).collect();
        let _ = head;
        s.layout(&mut ts);
        follow(&mut s, rows[4], 0.0, false);
        assert_eq!(aim(&s, inner)[1], 95.0, "the inner list to its end (and the ring's room)");
        // The inner list sits at 80..140 in the outer one and shows the row at its bottom: the
        // outer one scrolls to its end (40, and the ring's 5) so the row is on screen.
        assert_eq!(aim(&s, outer)[1], 45.0);
    }

    #[test]
    fn clamp_after_shrink() {
        let (mut s, v, rows) = page();
        to(&mut s, v, None, Some(300.0), 0.0, false);
        for r in &rows[3..] {
            s.remove(*r);
        }
        let mut ts = TextSys::new(&[], false, 64);
        s.layout(&mut ts);
        assert!(clamp(&mut s, v));
        assert_eq!(aim(&s, v)[1], 25.0);
    }
}
