//! Spatial focus for controller navigation, the native port of `launcher/src/nav/focus.ts`.
//!
//! Screens register what can take focus each time their layout changes, in document order: a
//! `Focusable` per `[data-nav]` element, a `Group` per `[data-nav-group]` (a rail, a button row, a
//! grid, a list) and a `Scroller` per `[data-scroll]` page or `[data-rail]` strip. A direction moves
//! to the nearest focusable on that side, weighting distance along the axis far less than drift
//! off it, so rails, grids and lists all behave as expected. Scopes (modals, overlays) trap focus:
//! the topmost wins. Groups remember their last focused child by position, so coming back to a
//! rail lands where you left it, even after the screen was rebuilt. Something always has focus:
//! when the focused node goes away, focus goes back to a saved spot, to the same place in its
//! group, or to the screen's default.
//!
//! Scrolling follows focus exactly as the web launcher does: a rail scrolls just enough to show the
//! focused tile, a page keeps the focused row a little above the middle like a console shelf, and
//! smooth scrolls glide toward an absolute target that held-direction repeats keep retargeting.
//! The glide is evaluated in `tick`; the screen applies `take_scrolls` to its scroll containers.
//!
//! Everything here is pure layout-space geometry: no scene queries on the hot path, unit-tested
//! below against the behaviours the web launcher's journeys rely on.

use crate::scene::{Id, Scene};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub fn horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }
}

/// `[x, y, w, h]` in window space, at the scroll offsets the screen last reported.
pub type Rect = [f32; 4];

fn right(r: &Rect) -> f32 {
    r[0] + r[2]
}
fn bottom(r: &Rect) -> f32 {
    r[1] + r[3]
}
fn cx(r: &Rect) -> f32 {
    r[0] + r[2] / 2.0
}
fn cy(r: &Rect) -> f32 {
    r[1] + r[3] / 2.0
}

/// One `[data-nav]` element.
#[derive(Clone, Debug)]
pub struct Focusable {
    pub id: Id,
    pub rect: Rect,
    /// Index into `Nav::groups` (`[data-nav-group]`, the closest one).
    pub group: Option<usize>,
    /// The `[data-nav-scope]` it sits in: 0 is the page, higher numbers are modals stacked on top.
    pub scope: u32,
    pub disabled: bool,
    /// Under an `[inert]` subtree (the page behind an overlay).
    pub inert: bool,
    /// `[data-nav-default]`: where focus starts on this screen.
    pub default: bool,
    /// The `[data-scroll]` page it scrolls in (`Scroller::key`).
    pub scroll: Option<u64>,
    /// The `[data-rail]` strip it scrolls in sideways (`Scroller::key`).
    pub rail: Option<u64>,
    /// `[data-center]`: always keep it centred in its page, not only when it leaves the view.
    pub center: bool,
    /// `data-hint-a`, `data-hint-x`, `data-hint-y`.
    pub hint_a: Option<String>,
    pub hint_x: Option<String>,
    pub hint_y: Option<String>,
    /// Inside `[data-game]`: Menu opens Options for it.
    pub game: bool,
    /// The screen's own tag (an index into its model).
    pub tag: u32,
    /// What it reads as to a screen reader (a11y.rs); falls back to its text.
    pub label: Option<String>,
}

impl Focusable {
    pub fn new(id: Id, rect: Rect) -> Focusable {
        Focusable {
            id,
            rect,
            group: None,
            scope: 0,
            disabled: false,
            inert: false,
            default: false,
            scroll: None,
            rail: None,
            center: false,
            hint_a: None,
            hint_x: None,
            hint_y: None,
            game: false,
            tag: 0,
            label: None,
        }
    }
    pub fn group(mut self, g: usize) -> Self {
        self.group = Some(g);
        self
    }
    pub fn scope(mut self, s: u32) -> Self {
        self.scope = s;
        self
    }
    pub fn tag(mut self, t: u32) -> Self {
        self.tag = t;
        self
    }
    pub fn default(mut self) -> Self {
        self.default = true;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn scroll(mut self, key: u64) -> Self {
        self.scroll = Some(key);
        self
    }
    pub fn rail(mut self, key: u64) -> Self {
        self.rail = Some(key);
        self
    }
    pub fn center(mut self) -> Self {
        self.center = true;
        self
    }
    pub fn hints(mut self, a: Option<&str>, x: Option<&str>, y: Option<&str>) -> Self {
        self.hint_a = a.map(str::to_string);
        self.hint_x = x.map(str::to_string);
        self.hint_y = y.map(str::to_string);
        self
    }
    pub fn game(mut self) -> Self {
        self.game = true;
        self
    }
    pub fn label(mut self, l: &str) -> Self {
        self.label = Some(l.to_string());
        self
    }
    fn visible(&self) -> bool {
        !self.inert && self.rect[2] > 0.0 && self.rect[3] > 0.0
    }
}

/// One `[data-nav-group]`.
#[derive(Clone, Debug)]
pub struct Group {
    /// The `data-nav-group` value: memory and restore spots are keyed by it.
    pub name: String,
    pub rect: Rect,
    /// `[data-rail]` on the group: it remembers, and LT / RT never page inside it.
    pub rail: bool,
    /// `[data-nav-remember]`: coming back lands on the last item, not the nearest.
    pub remember: bool,
    /// The `<section>` around the group, if any: the page keeps that whole row in view.
    pub section: Option<Rect>,
}

impl Group {
    pub fn new(name: &str, rect: Rect) -> Group {
        Group { name: name.to_string(), rect, rail: false, remember: false, section: None }
    }
    pub fn rail(mut self) -> Self {
        self.rail = true;
        self
    }
    pub fn remember(mut self) -> Self {
        self.remember = true;
        self
    }
    pub fn section(mut self, r: Rect) -> Self {
        self.section = Some(r);
        self
    }
    /// Rails and single rows of buttons go back to their last item; grids pick by position.
    fn remembers(&self) -> bool {
        self.rail || self.remember
    }
}

/// A scroll container: a `[data-scroll]` page or a `[data-rail]` strip.
#[derive(Clone, Debug, Default)]
pub struct Scroller {
    pub key: u64,
    /// The viewport, window space (clientWidth / clientHeight are its size).
    pub rect: Rect,
    /// scrollWidth, scrollHeight.
    pub content: [f32; 2],
    /// scrollLeft, scrollTop, as applied to the rects registered with it.
    pub offset: [f32; 2],
    /// The rail's left padding (a focused tile keeps that much air at the edge).
    pub pad_left: f32,
    /// The lowest offsets [left, top] (0, or below 0 when its padding is thinner than a focus
    /// ring: `Scene::min_scroll`).
    pub start: [f32; 2],
    /// Height of the page's `[data-sticky]` header (0 when none).
    pub sticky: f32,
    /// The scroller of the same axis this one sits in (a sheet's list inside a page, a strip
    /// inside a strip): focus-follow scrolls it too, so the inner one is on screen.
    pub parent: Option<u64>,
}

/// A place to come back to: a group and an index in it, plus the page's scroll offset.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub group: Option<String>,
    pub index: usize,
    pub scroll_top: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
struct Aim {
    left: f32,
    top: f32,
    at: f64,
}

#[derive(Clone, Copy, Debug)]
struct Glide {
    l: f32,
    t: f32,
    x: f32,
    y: f32,
}

/// A smooth scroll aimed less than this long ago is still "where the box is going".
const AIM_FRESH: f64 = 0.45;
/// Scroll glide time constant: about 90 % of the way in 150 ms.
const GLIDE_TAU_MS: f64 = 65.0;
/// How long a restore spot waits for content that is still loading.
const RESTORE_WAIT: f64 = 2.5;
/// Screens that restore focus themselves get this long before the default takes over.
const FALLBACK_DELAY: f64 = 0.06;
/// Focus resting this long preloads its neighbours' art.
const PRELOAD_REST: f64 = 0.12;

/// How a focus placement scrolls.
#[derive(Clone, Copy, Debug)]
pub struct FocusOpts {
    pub scroll: bool,
    pub smooth: bool,
}

impl Default for FocusOpts {
    fn default() -> Self {
        FocusOpts { scroll: true, smooth: true }
    }
}

const INSTANT: FocusOpts = FocusOpts { scroll: true, smooth: false };

/// The focus engine for one window.
#[derive(Default)]
pub struct Nav {
    pub items: Vec<Focusable>,
    pub groups: Vec<Group>,
    pub scrollers: Vec<Scroller>,
    /// innerWidth, innerHeight.
    pub viewport: [f32; 2],
    /// The active scope; None: the highest scope any registered item is in.
    pub scope_top: Option<u32>,
    /// A text field has focus (the on-screen keyboard is typing): focus is left alone.
    pub editing: bool,
    cur: Option<Id>,
    memory: HashMap<String, usize>,
    /// True while focus sits where the engine put it on its own, not where the user moved it:
    /// then a screen's default that shows up late (after data loads) still wins.
    auto: bool,
    pending: Option<(Spot, f64)>,
    last_spot: Option<(String, usize)>,
    fallback_at: Option<f64>,
    aims: HashMap<u64, Aim>,
    glides: HashMap<u64, Glide>,
    glide_prev: f64,
    out: HashMap<u64, [f32; 2]>,
    preload: Option<(Id, f64)>,
    changed: bool,
}

impl Nav {
    pub fn new(w: f32, h: f32) -> Nav {
        Nav { viewport: [w, h], ..Default::default() }
    }

    // ---- registration ----

    /// Starts a new registration pass (the screen's layout changed). Focus, memory and scroll
    /// aims survive; call `ensure` after pushing everything.
    pub fn begin(&mut self) {
        self.items.clear();
        self.groups.clear();
        self.scrollers.clear();
    }

    pub fn add_group(&mut self, g: Group) -> usize {
        self.groups.push(g);
        self.groups.len() - 1
    }

    pub fn add(&mut self, f: Focusable) {
        self.items.push(f);
    }

    pub fn add_scroller(&mut self, s: Scroller) {
        self.scrollers.retain(|o| o.key != s.key);
        self.scrollers.push(s);
    }

    /// Moves every rect registered with scroller `key` to a new scroll offset (the screen scrolled
    /// it without a rebuild).
    pub fn scrolled(&mut self, key: u64, offset: [f32; 2]) {
        let Some(s) = self.scrollers.iter_mut().find(|s| s.key == key) else { return };
        let (dx, dy) = (s.offset[0] - offset[0], s.offset[1] - offset[1]);
        s.offset = offset;
        for it in self.items.iter_mut() {
            if it.rail == Some(key) {
                it.rect[0] += dx;
            }
            if it.scroll == Some(key) {
                it.rect[1] += dy;
            }
        }
        // Groups (and their sections) move with the scrollers their items are in.
        for g in 0..self.groups.len() {
            let first = self.items.iter().find(|i| i.group == Some(g));
            let (sx, sy) = first.map(|i| (i.rail == Some(key), i.scroll == Some(key))).unwrap_or((false, false));
            let grp = &mut self.groups[g];
            if sx {
                grp.rect[0] += dx;
                if let Some(s) = grp.section.as_mut() {
                    s[0] += dx;
                }
            }
            if sy {
                grp.rect[1] += dy;
                if let Some(s) = grp.section.as_mut() {
                    s[1] += dy;
                }
            }
        }
    }

    // ---- queries ----

    fn top_scope(&self) -> u32 {
        self.scope_top.unwrap_or_else(|| self.items.iter().map(|i| i.scope).max().unwrap_or(0))
    }

    fn in_scope(&self, it: &Focusable) -> bool {
        let top = self.top_scope();
        top == 0 || it.scope == top
    }

    /// Visible in the active scope (and enabled, unless `disabled_too`).
    pub fn reachable(&self, it: &Focusable, disabled_too: bool) -> bool {
        it.visible() && self.in_scope(it) && (disabled_too || !it.disabled)
    }

    fn idx(&self, id: Id) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }

    pub fn item(&self, id: Id) -> Option<&Focusable> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Everything that can take focus now, in document order.
    pub fn focusables(&self) -> Vec<usize> {
        (0..self.items.len()).filter(|&i| self.items[i].visible() && !self.items[i].disabled && self.in_scope(&self.items[i])).collect()
    }

    fn group_items(&self, g: usize) -> Vec<usize> {
        (0..self.items.len()).filter(|&i| self.items[i].group == Some(g) && !self.items[i].disabled && self.in_scope(&self.items[i])).collect()
    }

    fn group_by_name(&self, name: &str) -> Option<usize> {
        // The first group of that name in the active scope.
        (0..self.groups.len()).find(|&g| self.groups[g].name == name && (self.top_scope() == 0 || self.items.iter().any(|i| i.group == Some(g) && self.in_scope(i))))
    }

    fn scroller(&self, key: Option<u64>) -> Option<&Scroller> {
        let k = key?;
        self.scrollers.iter().find(|s| s.key == k)
    }

    /// The focused node, if it still exists in the active scope.
    pub fn current(&self) -> Option<Id> {
        let id = self.cur?;
        let it = self.item(id)?;
        (self.in_scope(it) && !it.inert).then_some(id)
    }

    /// The focused node's item.
    pub fn current_item(&self) -> Option<&Focusable> {
        self.current().and_then(|id| self.item(id))
    }

    /// True once after focus changed (the screen restyles the ring, hints and a11y follow).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Where focus is now, to restore later (after an overlay closes or a tab comes back).
    pub fn spot(&self) -> Option<Spot> {
        let cur = self.current()?;
        let i = self.idx(cur)?;
        let it = &self.items[i];
        let (group, index) = match it.group {
            Some(g) => (Some(self.groups[g].name.clone()), self.group_items(g).iter().position(|&x| x == i).unwrap_or(0)),
            None => (None, self.focusables().iter().position(|&x| x == i).unwrap_or(0)),
        };
        Some(Spot { group, index, scroll_top: self.scroller(it.scroll).map(|s| s.offset[1]) })
    }

    /// Asks for focus to go back to `s` as soon as that node exists (content may still be loading).
    pub fn restore_later(&mut self, s: Option<Spot>, now: f64) {
        self.pending = s.map(|s| (s, now + RESTORE_WAIT));
    }

    fn resolve(&self, s: &Spot) -> Option<usize> {
        match &s.group {
            Some(name) => {
                let g = self.group_by_name(name)?;
                let items = self.group_items(g);
                let n = items.len().checked_sub(1)?;
                Some(items[s.index.min(n)])
            }
            None => self.focusables().get(s.index).copied(),
        }
    }

    // ---- placing focus ----

    /// Puts focus on `id` on purpose (wins over a late-loading default).
    pub fn focus(&mut self, id: Id, opts: FocusOpts, now: f64) {
        let Some(i) = self.idx(id) else { return };
        self.focus_idx(i, opts, now);
    }

    fn focus_idx(&mut self, i: usize, opts: FocusOpts, now: f64) {
        self.auto = false;
        let id = self.items[i].id;
        if self.cur != Some(id) {
            self.changed = true;
        }
        self.cur = Some(id);
        match self.items[i].group {
            Some(g) => {
                let index = self.group_items(g).iter().position(|&x| x == i).unwrap_or(0);
                let name = self.groups[g].name.clone();
                self.memory.insert(name.clone(), index);
                self.last_spot = Some((name, index));
            }
            None => self.last_spot = None,
        }
        if opts.scroll {
            self.reveal(i, opts.smooth, now);
        }
        self.preload = Some((id, now));
    }

    pub fn focus_first(&mut self, now: f64) {
        let def = self.items.iter().position(|it| it.default && self.in_scope(it));
        let target = match def {
            Some(d) if self.items[d].visible() && !self.items[d].disabled => Some(d),
            _ => self.focusables().first().copied(),
        };
        if let Some(t) = target {
            self.focus_idx(t, INSTANT, now);
        }
    }

    /// The last focused index a group remembers.
    pub fn remembered(&self, group: &str) -> Option<usize> {
        self.memory.get(group).copied()
    }

    /// Forgets a group's memory (a screen that wants its rail to start over).
    pub fn forget(&mut self, group: &str) {
        self.memory.remove(group);
    }

    // ---- moving ----

    /// A direction: true when focus moved (the caller plays the move feedback).
    pub fn move_dir(&mut self, dir: Dir, now: f64) -> bool {
        self.pending = None;
        self.auto = false;
        let Some(cur) = self.current().and_then(|c| self.idx(c)) else {
            self.focus_first(now);
            return true;
        };
        let all = self.focusables();
        let cands = all.iter().filter(|&&i| i != cur).map(|&i| (i, self.items[i].rect, self.items[i].group.map(|g| &self.groups[g].name)));
        let cur_group = self.items[cur].group.map(|g| &self.groups[g].name);
        let Some(mut best) = best_in_dir(&self.items[cur].rect, cur_group, cands, dir, self.viewport[0]) else { return false };
        // Entering another rail, button row or side menu: go back to its remembered item.
        if let Some(g) = self.items[best].group {
            let grp = &self.groups[g];
            if Some(&grp.name) != cur_group && grp.remembers() {
                if let Some(&i) = self.memory.get(&grp.name) {
                    if let Some(&it) = self.group_items(g).get(i) {
                        best = it;
                    }
                }
            }
        }
        self.focus_idx(best, FocusOpts::default(), now);
        true
    }

    /// LT / RT: jump a section. Inside a tall grid or list that runs past the screen it pages by a
    /// screen instead; at the end of it the next jump leaves for the next section.
    pub fn jump(&mut self, forward: bool, now: f64) -> bool {
        self.pending = None;
        self.auto = false;
        let Some(cur) = self.current().and_then(|c| self.idx(c)) else {
            self.focus_first(now);
            return true;
        };
        let all = self.focusables();
        let cur_group = self.items[cur].group;
        let a = self.items[cur].rect;
        let view_h = self.scroller(self.items[cur].scroll).map(|s| s.rect[3]).unwrap_or(self.viewport[1]);
        let sign = if forward { 1.0 } else { -1.0 };

        if let Some(g) = cur_group.filter(|&g| !self.groups[g].rail) {
            let gr = self.groups[g].rect;
            let more_there = if forward { bottom(&gr) > bottom(&a) + 4.0 } else { gr[1] < a[1] - 4.0 };
            if more_there && gr[3] > view_h * 0.7 {
                let target_y = cy(&a) + sign * view_h * 0.8;
                let ax = cx(&a);
                let mut best: Option<(f32, usize)> = None;
                for i in self.group_items(g) {
                    let r = self.items[i].rect;
                    let (x, y) = (cx(&r), cy(&r));
                    if if forward { y <= cy(&a) + 1.0 } else { y >= cy(&a) - 1.0 } {
                        continue;
                    }
                    let d = (y - target_y).abs() * 2.0 + (x - ax).abs();
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, i));
                    }
                }
                if let Some((_, b)) = best {
                    self.focus_idx(b, FocusOpts::default(), now);
                    return true;
                }
            }
        }

        // Next or previous section: the nearest group (or ungrouped node) fully past this one.
        let edge = cur_group.map(|g| self.groups[g].rect).unwrap_or(a);
        let mut target: Option<usize> = None;
        let mut best_y = if forward { f32::INFINITY } else { f32::NEG_INFINITY };
        for &i in &all {
            let g = self.items[i].group;
            if g.is_some() && g == cur_group {
                continue;
            }
            let r = g.map(|g| self.groups[g].rect).unwrap_or(self.items[i].rect);
            let hit = if forward { r[1] >= bottom(&edge) - 2.0 && r[1] < best_y } else { bottom(&r) <= edge[1] + 2.0 && r[1] > best_y };
            if hit {
                best_y = r[1];
                target = Some(i);
            }
        }
        let mut target = match target {
            Some(t) => t,
            None => {
                // Nothing further: the very first or last node.
                let t = if forward { all.last() } else { all.first() };
                match t {
                    Some(&t) if t != cur => t,
                    _ => return false,
                }
            }
        };
        if let Some(g) = self.items[target].group {
            let items = self.group_items(g);
            let grp = &self.groups[g];
            let mem = self.memory.get(&grp.name).and_then(|&i| items.get(i)).filter(|_| grp.remembers());
            target = mem.or(items.first()).copied().unwrap_or(target);
        }
        self.focus_idx(target, FocusOpts::default(), now);
        true
    }

    /// A: the node to activate (the caller plays select and runs its action), or None after
    /// putting focus somewhere first.
    pub fn activate(&mut self, now: f64) -> Option<Id> {
        self.auto = false;
        match self.current() {
            Some(c) => Some(c),
            None => {
                self.focus_first(now);
                None
            }
        }
    }

    /// A tap at a window point: the topmost focusable under it (touch = focus + A).
    pub fn hit(&self, x: f32, y: f32) -> Option<Id> {
        self.focusables().into_iter().rev().map(|i| &self.items[i]).find(|it| x >= it.rect[0] && x < right(&it.rect) && y >= it.rect[1] && y < bottom(&it.rect)).map(|it| it.id)
    }

    // ---- keeping focus alive ----

    /// Puts focus somewhere sensible when nothing in the active scope has it. Call after every
    /// registration pass and whenever `next_wake` comes due.
    pub fn ensure(&mut self, now: f64) {
        if self.editing {
            return;
        }
        if let Some((spot, until)) = self.pending.clone() {
            if now > until {
                self.pending = None;
            } else {
                if let Some(i) = self.resolve(&spot) {
                    self.pending = None;
                    if let (Some(top), Some(key)) = (spot.scroll_top, self.items[i].scroll) {
                        self.set_scroll(key, None, Some(top));
                    }
                    self.focus_idx(i, INSTANT, now);
                    self.auto = false;
                    return;
                }
                // The saved spot isn't there yet; keep waiting, but don't leave focus nowhere.
                if self.current().is_some() {
                    return;
                }
            }
        }
        if let Some(cur) = self.current() {
            if self.auto {
                let def = self.items.iter().position(|it| it.default && self.in_scope(it));
                if let Some(d) = def {
                    let it = &self.items[d];
                    if it.id != cur && it.visible() && !it.disabled {
                        self.focus_idx(d, INSTANT, now);
                        self.auto = true;
                    }
                }
            }
            return;
        }
        if let Some((g, index)) = self.last_spot.clone() {
            if let Some(i) = self.resolve(&Spot { group: Some(g), index, scroll_top: None }) {
                self.focus_idx(i, INSTANT, now);
                return;
            }
        }
        // Last resort: the screen's default, after screens that restore focus themselves had a
        // couple of frames to do it.
        match self.fallback_at {
            None => self.fallback_at = Some(now + FALLBACK_DELAY),
            Some(t) if now >= t => {
                self.fallback_at = None;
                if !self.focusables().is_empty() {
                    self.focus_first(now);
                    self.auto = true;
                }
            }
            Some(_) => {}
        }
    }

    /// When the loop must wake for focus work (the default fallback, a preload).
    pub fn next_wake(&self) -> Option<f64> {
        let p = self.preload.map(|(_, t)| t + PRELOAD_REST);
        match (self.fallback_at, p) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Once focus rested: the focused node and the neighbours whose art should be in memory
    /// before focus gets there (the next 8 in its rail or grid, then the 4 before).
    pub fn take_preload(&mut self, now: f64) -> Option<(Id, Vec<Id>)> {
        let (id, at) = self.preload?;
        if now < at + PRELOAD_REST {
            return None;
        }
        self.preload = None;
        let i = self.idx(id)?;
        let near = match self.items[i].group {
            Some(g) => {
                let items = self.group_items(g);
                let p = items.iter().position(|&x| x == i).unwrap_or(0);
                items[p + 1..(p + 9).min(items.len())].iter().chain(items[p.saturating_sub(4)..p].iter()).map(|&x| self.items[x].id).collect()
            }
            None => Vec::new(),
        };
        Some((id, near))
    }

    // ---- scrolling ----

    /// Offset of a rect inside a scroller, independent of the scroller's current scroll.
    fn offset_in(r: &Rect, s: &Scroller) -> Rect {
        [r[0] - s.rect[0] + s.offset[0], r[1] - s.rect[1] + s.offset[1], r[2], r[3]]
    }

    fn aimed(&self, s: &Scroller, left: bool, now: f64) -> f32 {
        match self.aims.get(&s.key) {
            Some(a) if now - a.at < AIM_FRESH => {
                if left {
                    a.left
                } else {
                    a.top
                }
            }
            _ => s.offset[if left { 0 } else { 1 }],
        }
    }

    fn scroll_target(&mut self, key: u64, left: Option<f32>, top: Option<f32>, smooth: bool, now: f64) {
        let Some(s) = self.scroller(Some(key)).cloned() else { return };
        let fresh = self.aims.get(&key).filter(|a| now - a.at < AIM_FRESH).copied();
        let l = left.unwrap_or(fresh.map(|a| a.left).unwrap_or(s.offset[0]));
        let t = top.unwrap_or(fresh.map(|a| a.top).unwrap_or(s.offset[1]));
        let max_l = s.content[0] - s.rect[2];
        let max_t = s.content[1] - s.rect[3];
        let cl = l.min(max_l).max(s.start[0].min(0.0));
        let ct = t.min(max_t).max(s.start[1].min(0.0));
        self.aims.insert(key, Aim { left: cl, top: ct, at: now });
        if smooth {
            let from = self.out.get(&key).copied().unwrap_or(s.offset);
            if self.glides.is_empty() {
                self.glide_prev = now;
            }
            let g = self.glides.entry(key).or_insert(Glide { l: cl, t: ct, x: from[0], y: from[1] });
            g.l = cl;
            g.t = ct;
        } else {
            self.glides.remove(&key);
            self.out.insert(key, [cl, ct]);
        }
    }

    /// Scrolls at once (a restore spot's page offset): the registered rects move with it, so what
    /// follows measures against the new offset as the DOM would.
    fn set_scroll(&mut self, key: u64, left: Option<f32>, top: Option<f32>) {
        let Some(s) = self.scroller(Some(key)) else { return };
        let max = [(s.content[0] - s.rect[2]).max(0.0), (s.content[1] - s.rect[3]).max(0.0)];
        let o = [left.unwrap_or(s.offset[0]).clamp(s.start[0].min(0.0), max[0]), top.unwrap_or(s.offset[1]).clamp(s.start[1].min(0.0), max[1])];
        self.glides.remove(&key);
        self.aims.remove(&key);
        self.scrolled(key, o);
        self.out.insert(key, o);
    }

    /// Scrolls the item into view inside its scrollers, sideways and vertically, and every
    /// scroller those sit in (nested lists: each outer one measures the item where the inner one
    /// is taking it).
    fn reveal(&mut self, i: usize, smooth: bool, now: f64) {
        let it = self.items[i].clone();
        // Sideways: just enough, keeping the strip's left padding as air.
        let mut x = it.rect[0];
        let mut key = it.rail;
        let mut hops = 0;
        while let Some(rail) = self.scroller(key).cloned() {
            hops += 1;
            if hops > 8 {
                break;
            }
            key = rail.parent;
            if rail.content[0] <= rail.rect[2] {
                continue;
            }
            let o = x - rail.rect[0] + rail.offset[0];
            let view = self.aimed(&rail, true, now);
            let pad = rail.pad_left;
            let want = if o - pad < view {
                Some(o - pad)
            } else if o + it.rect[2] + pad > view + rail.rect[2] {
                Some(o + it.rect[2] + pad - rail.rect[2])
            } else {
                None
            };
            let to = want.map(|w| w.clamp(rail.start[0].min(0.0), (rail.content[0] - rail.rect[2]).max(0.0))).unwrap_or(view);
            if want.is_some() {
                self.scroll_target(rail.key, Some(to), None, smooth, now);
            }
            // Where the item lands in window space once this strip gets there.
            x -= to - rail.offset[0];
        }
        // Up and down: keep the focused row a little above the middle, like a console shelf.
        let mut key = it.scroll;
        let mut first = true;
        let mut dy = 0.0;
        let mut hops = 0;
        while let Some(page) = self.scroller(key).cloned() {
            hops += 1;
            if hops > 8 {
                break;
            }
            key = page.parent;
            let at_first = std::mem::take(&mut first);
            if page.content[1] <= page.rect[3] {
                continue;
            }
            let row = if at_first { it.group.and_then(|g| self.groups[g].section).filter(|r| r[3] < page.rect[3] * 0.8) } else { None };
            let mut r = row.unwrap_or(it.rect);
            r[1] -= dy;
            let o = Self::offset_in(&r, &page);
            let view = self.aimed(&page, false, now);
            let top_pad = page.sticky + 12.0;
            let outside = o[1] - top_pad < view || o[1] + o[3] + 12.0 > view + page.rect[3];
            let mut to = view;
            if outside || (it.center && at_first) {
                let target = o[1] + o[3] / 2.0 - page.rect[3] * 0.45;
                let t = if o[1] - top_pad < target { o[1] - top_pad } else { target };
                to = t.clamp(page.start[1].min(0.0), (page.content[1] - page.rect[3]).max(0.0));
                self.scroll_target(page.key, None, Some(t), smooth, now);
            }
            dy += to - page.offset[1];
        }
    }

    /// Advances the scroll glides to `now`; true while any is still moving.
    pub fn tick(&mut self, now: f64) -> bool {
        if self.glides.is_empty() {
            return false;
        }
        let dt = ((now - self.glide_prev) * 1000.0).clamp(0.0, 64.0);
        self.glide_prev = now;
        let k = (1.0 - (-dt / GLIDE_TAU_MS).exp()) as f32;
        let mut done = Vec::new();
        for (key, s) in self.glides.iter_mut() {
            s.x += (s.l - s.x) * k;
            s.y += (s.t - s.y) * k;
            let dx = (s.l - s.x).abs() < 0.5;
            let dy = (s.t - s.y).abs() < 0.5;
            if dx {
                s.x = s.l;
            }
            if dy {
                s.y = s.t;
            }
            self.out.insert(*key, [s.x, s.y]);
            if dx && dy {
                done.push(*key);
            }
        }
        for k in done {
            self.glides.remove(&k);
        }
        !self.glides.is_empty()
    }

    pub fn scrolling(&self) -> bool {
        !self.glides.is_empty()
    }

    /// A finger took the scrolling over: every glide and aim stops where it is, so focus-follow
    /// never pulls a dragged page back. The next registration reads the offsets the drag left.
    pub fn stop_scrolls(&mut self) {
        self.glides.clear();
        self.aims.clear();
        self.out.clear();
    }

    /// The ids a touch can reach now: visible, enabled and in the active scope.
    pub fn touchable(&self) -> Vec<Id> {
        self.focusables().into_iter().map(|i| self.items[i].id).collect()
    }

    /// Scroll offsets to apply: (scroller key, [left, top]). The screen applies them and reports
    /// the new offsets (and rects) on its next registration, or through `scrolled`.
    pub fn take_scrolls(&mut self) -> Vec<(u64, [f32; 2])> {
        let mut v: Vec<_> = self.out.drain().collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }

    /// What the focused node says about its buttons (hints.ts `useFocusedHints`).
    pub fn focused_hints(&self) -> hints::Focused {
        match self.current_item() {
            Some(it) => hints::Focused {
                a: it.hint_a.clone(),
                x: it.hint_x.clone(),
                y: it.hint_y.clone(),
                menu: it.game.then(|| "Options".to_string()),
                focused: true,
            },
            None => hints::Focused::default(),
        }
    }
}

/// The node `dir` moves to from `a` (focus.ts `move`): the smallest score of edge distance along
/// the move plus 2.5x the drift across it. Left and right stay in the same rail or grid row while
/// they can; up and down prefer what is on screen over things scrolled off sideways. Ties go to
/// the first in document order.
pub fn best_in_dir<'a, K: Copy, G: PartialEq + 'a>(a: &Rect, cur_group: Option<G>, cands: impl Iterator<Item = (K, Rect, Option<G>)>, dir: Dir, inner_width: f32) -> Option<K> {
    let (ax, ay) = (cx(a), cy(a));
    let horizontal = dir.horizontal();
    let mut best: Option<(f32, K)> = None;
    for (k, b, g) in cands {
        let (bx, by) = (cx(&b), cy(&b));
        let overlap_x = b[0].max(a[0]) < right(&b).min(right(a));
        let overlap_y = b[1].max(a[1]) < bottom(&b).min(bottom(a));
        let (main, cross) = match dir {
            Dir::Right => {
                if bx <= ax + 1.0 || b[0] < a[0] + 1.0 {
                    continue;
                }
                (b[0] - right(a), if overlap_y { 0.0 } else { (by - ay).abs() })
            }
            Dir::Left => {
                if bx >= ax - 1.0 || right(&b) > right(a) - 1.0 {
                    continue;
                }
                (a[0] - right(&b), if overlap_y { 0.0 } else { (by - ay).abs() })
            }
            Dir::Down => {
                if by <= ay + 1.0 || b[1] < a[1] + 1.0 {
                    continue;
                }
                (b[1] - bottom(a), if overlap_x { (bx - ax).abs() * 0.1 } else { (bx - ax).abs() })
            }
            Dir::Up => {
                if by >= ay - 1.0 || bottom(&b) > bottom(a) - 1.0 {
                    continue;
                }
                (a[1] - bottom(&b), if overlap_x { (bx - ax).abs() * 0.1 } else { (bx - ax).abs() })
            }
        };
        let group_penalty = if horizontal && cur_group.is_some() && g != cur_group { 5000.0 } else { 0.0 };
        let offscreen = if !horizontal && (right(&b) < 0.0 || b[0] > inner_width) { 3000.0 } else { 0.0 };
        let score = main.max(0.0) + cross * 2.5 + group_penalty + offscreen;
        if best.as_ref().is_none_or(|(s, _)| score < *s) {
            best = Some((score, k));
        }
    }
    best.map(|(_, k)| k)
}

/// The POC's registration API (home-poc): items with numeric groups, rects read from the scene.
/// Moves score exactly like `Nav` (focus.ts).
#[derive(Clone, Debug)]
pub struct Item {
    pub id: Id,
    pub group: u32,
    /// App tag (index into the screen's model).
    pub tag: u32,
}

#[derive(Default)]
pub struct FocusMap {
    pub items: Vec<Item>,
    memory: HashMap<u32, Id>,
    /// Groups that remember (data-nav-remember).
    pub remember: Vec<u32>,
}

impl FocusMap {
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn push(&mut self, id: Id, group: u32, tag: u32) {
        self.items.push(Item { id, group, tag });
    }

    pub fn item(&self, id: Id) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn note(&mut self, id: Id) {
        if let Some(g) = self.item(id).map(|i| i.group) {
            self.memory.insert(g, id);
        }
    }

    /// The next focus from `cur` in `dir`, or None at an edge.
    pub fn step(&self, scene: &Scene, cur: Id, dir: Dir) -> Option<Id> {
        let from = self.item(cur)?;
        let cands = self.items.iter().filter(|i| i.id != cur).map(|i| (i, scene.rect(i.id), Some(i.group)));
        let target = best_in_dir(&scene.rect(cur), Some(from.group), cands, dir, scene.w)?;
        if target.group != from.group && self.remember.contains(&target.group) {
            if let Some(m) = self.memory.get(&target.group) {
                if self.items.iter().any(|i| i.id == *m) {
                    return Some(*m);
                }
            }
        }
        Some(target.id)
    }
}

/// What the bottom hint bar shows (`nav/hints.ts` and `components/Chrome.tsx` `HintBar`). It
/// always matches what the buttons do right now:
///   A  the focused node's action (`hint_a`, else "Select")
///   B  "Back" whenever B has somewhere to go
///   X  the screen's or focused node's extra action
///   Y  "Search" unless a screen claims it
/// Screens and overlays push their extra hints on the stack; the topmost wins.
pub mod hints {
    /// A glyph in the hint bar (`PadButton` in lib/types.ts).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Key {
        Up,
        Down,
        Left,
        Right,
        A,
        B,
        X,
        Y,
        Lb,
        Rb,
        Lt,
        Rt,
        Start,
        Select,
        Guide,
        Qam,
        Rare,
        Quick,
        Lc,
        Rc,
        GuideHold,
        ExitHold,
        /// LB / RB (the launcher's tabs) as one hint.
        LbRb,
        /// LT / RT (the screen's sections) as one hint.
        LtRt,
    }

    impl Key {
        pub fn name(self) -> &'static str {
            match self {
                Key::Up => "up",
                Key::Down => "down",
                Key::Left => "left",
                Key::Right => "right",
                Key::A => "a",
                Key::B => "b",
                Key::X => "x",
                Key::Y => "y",
                Key::Lb => "lb",
                Key::Rb => "rb",
                Key::Lt => "lt",
                Key::Rt => "rt",
                Key::Start => "start",
                Key::Select => "select",
                Key::Guide => "guide",
                Key::Qam => "qam",
                Key::Rare => "rare",
                Key::Quick => "quick",
                Key::Lc => "lc",
                Key::Rc => "rc",
                Key::GuideHold => "guidehold",
                Key::ExitHold => "exithold",
                Key::LbRb => "lbrb",
                Key::LtRt => "ltrt",
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Hint {
        pub b: Key,
        pub label: String,
    }

    pub fn hint(b: Key, label: &str) -> Hint {
        Hint { b, label: label.to_string() }
    }

    /// The order the bar shows hints in, on every screen.
    pub const ORDER: [Key; 13] = [Key::A, Key::B, Key::X, Key::Y, Key::LbRb, Key::LtRt, Key::Lt, Key::Rt, Key::Select, Key::Start, Key::Quick, Key::Rare, Key::ExitHold];

    fn rank(b: Key) -> usize {
        // Unknown keys sort last, as `(indexOf + 100) % 100` does.
        ORDER.iter().position(|&o| o == b).unwrap_or(99)
    }

    /// On game and app pages: how to reach Rare from inside the game. On Rare OS that is the Rare
    /// button ("Menu"); in guest mode Steam keeps it, so the hint is the View hold.
    pub fn exit_hint(guest: bool) -> Hint {
        if guest {
            hint(Key::Select, "Hold View for the Rare menu")
        } else {
            hint(Key::Rare, "Menu")
        }
    }

    /// Extra hints registered by mounted screens and overlays (`useHints`).
    #[derive(Default)]
    pub struct Stack {
        seq: u64,
        stack: Vec<(u64, Vec<Hint>)>,
    }

    impl Stack {
        pub fn push(&mut self, hints: Vec<Hint>) -> u64 {
            self.seq += 1;
            self.stack.push((self.seq, hints));
            self.seq
        }
        pub fn remove(&mut self, id: u64) {
            self.stack.retain(|(i, _)| *i != id);
        }
        pub fn entries(&self) -> impl Iterator<Item = &Vec<Hint>> {
            self.stack.iter().map(|(_, h)| h)
        }
    }

    /// The focused node's own hints.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Focused {
        pub a: Option<String>,
        pub x: Option<String>,
        pub y: Option<String>,
        pub menu: Option<String>,
        pub focused: bool,
    }

    /// The bar: `over` (a screen's full list) replaces everything but Quick, Menu and Y from
    /// `base`; otherwise `base`, the stack, the focused node and Back are merged.
    pub fn compose(over: Option<&[Hint]>, base: &[Hint], stack: &Stack, f: &Focused, can_back: bool) -> Vec<Hint> {
        if let Some(over) = over {
            let mut hints: Vec<Hint> = over.to_vec();
            let sys = |b: Key| matches!(b, Key::Quick | Key::Start | Key::Qam);
            for h in base {
                if !matches!(h.b, Key::Quick | Key::Start | Key::Y) {
                    continue;
                }
                if over.iter().any(|o| o.b == h.b || (h.b != Key::Y && sys(o.b))) {
                    continue;
                }
                hints.push(h.clone());
            }
            hints.sort_by_key(|h| rank(h.b));
            return hints;
        }
        let mut by: Vec<(Key, String)> = Vec::new();
        let set = |by: &mut Vec<(Key, String)>, b: Key, l: String| match by.iter_mut().find(|(k, _)| *k == b) {
            Some(e) => e.1 = l,
            None => by.push((b, l)),
        };
        for h in base {
            set(&mut by, h.b, h.label.clone());
        }
        for e in stack.entries() {
            for h in e {
                set(&mut by, h.b, h.label.clone());
            }
        }
        let get = |by: &Vec<(Key, String)>, b: Key| by.iter().find(|(k, _)| *k == b).map(|(_, l)| l.clone());
        if let Some(x) = &f.x {
            set(&mut by, Key::X, x.clone());
        }
        if let Some(y) = &f.y {
            set(&mut by, Key::Y, y.clone());
        }
        if let Some(m) = &f.menu {
            if get(&by, Key::Start).is_none() {
                set(&mut by, Key::Start, m.clone());
            }
        }
        if f.focused {
            let a = f.a.clone().or_else(|| get(&by, Key::A)).unwrap_or_else(|| "Select".into());
            set(&mut by, Key::A, a);
        }
        if can_back && get(&by, Key::B).is_none() {
            set(&mut by, Key::B, "Back".into());
        }
        ORDER.iter().filter_map(|&b| get(&by, b).filter(|l| !l.is_empty()).map(|label| Hint { b, label })).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::hints::{compose, exit_hint, hint, Focused, Key, Stack};
    use super::*;
    use taffy::NodeId;

    fn id(n: u64) -> Id {
        NodeId::from(n)
    }

    /// A screen like Home: a rail of 6 tiles (group "strip", remembers), a row of two buttons
    /// below (group "hero"), and a lone button to the right.
    fn home() -> Nav {
        let mut n = Nav::new(1920.0, 1200.0);
        let strip = n.add_group(Group::new("strip", [100.0, 100.0, 1200.0, 200.0]).rail());
        let hero = n.add_group(Group::new("hero", [100.0, 500.0, 500.0, 80.0]).remember());
        for i in 0..6 {
            n.add(Focusable::new(id(i), [100.0 + i as f32 * 200.0, 100.0, 180.0, 200.0]).group(strip).tag(i as u32));
        }
        n.add(Focusable::new(id(10), [100.0, 500.0, 200.0, 80.0]).group(hero).default());
        n.add(Focusable::new(id(11), [320.0, 500.0, 200.0, 80.0]).group(hero));
        n.add(Focusable::new(id(20), [1500.0, 500.0, 300.0, 80.0]));
        n
    }

    #[test]
    fn rail_moves_and_stops_at_its_ends() {
        let mut n = home();
        n.focus(id(0), FocusOpts::default(), 0.0);
        assert!(n.move_dir(Dir::Right, 0.0));
        assert_eq!(n.current(), Some(id(1)));
        assert!(!n.move_dir(Dir::Up, 0.0), "nothing above the strip");
        n.focus(id(5), FocusOpts::default(), 0.0);
        // Right from the last tile: the lone button is down-right, a different group: penalised
        // but still the only thing to the right.
        assert!(n.move_dir(Dir::Right, 0.0));
        assert_eq!(n.current(), Some(id(20)));
    }

    #[test]
    fn left_right_stay_in_the_group_while_they_can() {
        let mut n = Nav::new(1920.0, 1200.0);
        let a = n.add_group(Group::new("a", [0.0, 0.0, 1000.0, 100.0]));
        let b = n.add_group(Group::new("b", [0.0, 110.0, 1000.0, 100.0]));
        n.add(Focusable::new(id(1), [0.0, 0.0, 100.0, 100.0]).group(a));
        // Far along the same row...
        n.add(Focusable::new(id(2), [800.0, 0.0, 100.0, 100.0]).group(a));
        // ...versus right next door, one row down, in another group.
        n.add(Focusable::new(id(3), [120.0, 110.0, 100.0, 100.0]).group(b));
        n.focus(id(1), FocusOpts::default(), 0.0);
        n.move_dir(Dir::Right, 0.0);
        assert_eq!(n.current(), Some(id(2)));
    }

    #[test]
    fn down_prefers_overlap_and_rail_memory_brings_you_back() {
        let mut n = home();
        n.focus(id(3), FocusOpts::default(), 0.0);
        n.move_dir(Dir::Down, 0.0);
        // Tile 3 is at x 700-880: the nearest below by drift is the second hero button (320-520),
        // the first is farther off.
        assert_eq!(n.current(), Some(id(11)));
        n.move_dir(Dir::Up, 0.0);
        // The strip remembers tile 3, whatever is geometrically nearest to the button.
        assert_eq!(n.current(), Some(id(3)));
        // A remembered group (hero) also brings you back to its last item.
        n.focus(id(10), FocusOpts::default(), 0.0);
        n.focus(id(0), FocusOpts::default(), 0.0);
        n.move_dir(Dir::Down, 0.0);
        assert_eq!(n.current(), Some(id(10)));
    }

    #[test]
    fn grids_pick_by_position_not_memory() {
        let mut n = Nav::new(1920.0, 1200.0);
        let top = n.add_group(Group::new("top", [0.0, 0.0, 600.0, 100.0]));
        let grid = n.add_group(Group::new("grid", [0.0, 200.0, 600.0, 300.0]));
        for i in 0..3 {
            n.add(Focusable::new(id(i), [i as f32 * 200.0, 0.0, 180.0, 100.0]).group(top));
        }
        for i in 0..6 {
            let (c, r) = (i % 3, i / 3);
            n.add(Focusable::new(id(100 + i), [c as f32 * 200.0, 200.0 + r as f32 * 150.0, 180.0, 130.0]).group(grid));
        }
        n.focus(id(105), FocusOpts::default(), 0.0); // bottom right of the grid
        n.focus(id(0), FocusOpts::default(), 0.0);
        n.move_dir(Dir::Down, 0.0);
        assert_eq!(n.current(), Some(id(100)), "a grid without data-nav-remember picks the nearest");
    }

    #[test]
    fn up_down_prefer_what_is_on_screen() {
        let mut n = Nav::new(1000.0, 800.0);
        n.add(Focusable::new(id(1), [400.0, 0.0, 200.0, 100.0]));
        // Directly below but scrolled off to the right of the window.
        n.add(Focusable::new(id(2), [1100.0, 200.0, 200.0, 100.0]));
        // Below and well off to the side, but on screen.
        n.add(Focusable::new(id(3), [0.0, 400.0, 100.0, 100.0]));
        n.focus(id(1), FocusOpts::default(), 0.0);
        n.move_dir(Dir::Down, 0.0);
        assert_eq!(n.current(), Some(id(3)));
    }

    #[test]
    fn scopes_trap_focus_and_the_topmost_wins() {
        let mut n = home();
        n.focus(id(0), FocusOpts::default(), 0.0);
        // A modal opens with two buttons.
        n.add(Focusable::new(id(50), [600.0, 600.0, 200.0, 60.0]).scope(1));
        n.add(Focusable::new(id(51), [600.0, 680.0, 200.0, 60.0]).scope(1).default());
        assert_eq!(n.current(), None, "the page's focus is outside the active scope");
        n.ensure(0.0);
        assert_eq!(n.current(), None, "screens get a moment to restore focus themselves");
        n.ensure(0.07);
        assert_eq!(n.current(), Some(id(51)), "then the scope's default");
        assert!(!n.move_dir(Dir::Right, 0.1), "nothing in the page can be reached");
        n.move_dir(Dir::Up, 0.1);
        assert_eq!(n.current(), Some(id(50)));
    }

    #[test]
    fn restore_waits_for_content_and_rebuilds_keep_the_spot() {
        let mut n = home();
        n.focus(id(4), FocusOpts::default(), 0.0);
        let spot = n.spot().unwrap();
        assert_eq!(spot, Spot { group: Some("strip".into()), index: 4, scroll_top: None });
        // A rebuild replaces every node (new ids): focus goes back to the same index in the strip.
        let mut m = Nav::new(1920.0, 1200.0);
        std::mem::swap(&mut m.memory, &mut n.memory);
        m.last_spot = n.last_spot.clone();
        let g = m.add_group(Group::new("strip", [100.0, 100.0, 1200.0, 200.0]).rail());
        for i in 0..6 {
            m.add(Focusable::new(id(200 + i), [100.0 + i as f32 * 200.0, 100.0, 180.0, 200.0]).group(g));
        }
        m.ensure(0.0);
        assert_eq!(m.current(), Some(id(204)));
        // A restore spot that is not there yet waits (with focus somewhere meanwhile)...
        let mut n = home();
        n.focus(id(0), FocusOpts::default(), 0.0);
        n.restore_later(Some(Spot { group: Some("later".into()), index: 1, scroll_top: None }), 0.0);
        n.ensure(0.1);
        assert_eq!(n.current(), Some(id(0)));
        // ...and lands when its group shows up, clamped to what exists.
        let g = n.add_group(Group::new("later", [0.0, 900.0, 400.0, 100.0]));
        n.add(Focusable::new(id(30), [0.0, 900.0, 100.0, 100.0]).group(g));
        n.ensure(0.2);
        assert_eq!(n.current(), Some(id(30)));
        // A spot older than the wait is dropped.
        n.restore_later(Some(Spot { group: Some("strip".into()), index: 2, scroll_top: None }), 0.0);
        n.ensure(3.0);
        assert_eq!(n.current(), Some(id(30)));
    }

    #[test]
    fn a_late_default_wins_over_automatic_focus_only() {
        let mut n = Nav::new(1920.0, 1200.0);
        n.add(Focusable::new(id(1), [0.0, 0.0, 100.0, 100.0]));
        n.ensure(0.0);
        n.ensure(0.1);
        assert_eq!(n.current(), Some(id(1)), "the fallback put focus on the first node");
        // Data loads; the screen's default appears: it takes over the automatic focus.
        n.add(Focusable::new(id(2), [0.0, 200.0, 100.0, 100.0]).default());
        n.ensure(0.2);
        assert_eq!(n.current(), Some(id(2)));
        // Once the user moved, a default never steals focus again.
        n.move_dir(Dir::Up, 0.3);
        n.add(Focusable::new(id(3), [0.0, 400.0, 100.0, 100.0]).default());
        n.items[1].default = false;
        n.ensure(0.4);
        assert_eq!(n.current(), Some(id(1)));
    }

    #[test]
    fn editing_leaves_focus_alone() {
        let mut n = home();
        n.editing = true;
        n.ensure(0.0);
        n.ensure(1.0);
        assert_eq!(n.current(), None);
    }

    #[test]
    fn first_focus_without_any_goes_to_the_default() {
        let mut n = home();
        assert!(n.move_dir(Dir::Down, 0.0));
        assert_eq!(n.current(), Some(id(10)));
        let mut n = home();
        n.items[6].disabled = true;
        assert!(n.move_dir(Dir::Down, 0.0));
        assert_eq!(n.current(), Some(id(0)), "a disabled default falls back to the first node");
    }

    fn tall_page() -> Nav {
        // A page: a top row, a tall grid of 4 columns x 12 rows (rows 150 px), a bottom row.
        let mut n = Nav::new(1920.0, 1000.0);
        n.add_scroller(Scroller { key: 1, rect: [0.0, 0.0, 1920.0, 1000.0], content: [1920.0, 2400.0], ..Default::default() });
        let top = n.add_group(Group::new("top", [0.0, 0.0, 800.0, 100.0]).remember());
        let grid = n.add_group(Group::new("grid", [0.0, 150.0, 800.0, 1800.0]));
        let end = n.add_group(Group::new("end", [0.0, 2000.0, 800.0, 100.0]).remember());
        for i in 0..2 {
            n.add(Focusable::new(id(i), [i as f32 * 200.0, 0.0, 180.0, 100.0]).group(top).scroll(1));
        }
        for i in 0..48 {
            let (c, r) = (i % 4, i / 4);
            n.add(Focusable::new(id(100 + i), [c as f32 * 200.0, 150.0 + r as f32 * 150.0, 180.0, 130.0]).group(grid).scroll(1));
        }
        for i in 0..2 {
            n.add(Focusable::new(id(300 + i), [i as f32 * 200.0, 2000.0, 180.0, 100.0]).group(end).scroll(1));
        }
        n
    }

    #[test]
    fn triggers_page_through_a_tall_grid_then_leave_it() {
        let mut n = tall_page();
        n.focus(id(101), FocusOpts::default(), 0.0); // row 0, column 1
        assert!(n.jump(true, 0.0));
        // A page is 0.8 of the 1000 px view: 800 px down, the nearest row centre (row 5 at 215+750).
        assert_eq!(n.current(), Some(id(100 + 5 * 4 + 1)));
        n.jump(true, 0.0);
        n.jump(true, 0.0);
        assert_eq!(n.current(), Some(id(100 + 11 * 4 + 1)), "the last row");
        n.jump(true, 0.0);
        assert_eq!(n.current(), Some(id(300)), "past the end of the grid: the next section");
        n.jump(false, 0.0);
        // Back up: the grid's nearest-section item (its first, grids don't remember)...
        assert_eq!(n.current(), Some(id(100)));
        n.jump(false, 0.0);
        assert_eq!(n.current(), Some(id(0)));
        assert!(!n.jump(false, 0.0), "nothing above the first section");
    }

    #[test]
    fn triggers_remember_rows_and_end_at_the_last_node() {
        let mut n = tall_page();
        n.focus(id(1), FocusOpts::default(), 0.0);
        n.focus(id(301), FocusOpts::default(), 0.0);
        n.jump(false, 0.0);
        assert!(n.current().is_some_and(|c| (100..148).contains(&usize::from(c))), "into the grid");
        n.focus(id(147), FocusOpts::default(), 0.0);
        n.jump(false, 0.0);
        // From the grid's bottom row it pages up first.
        assert!(n.current().is_some_and(|c| (100..148).contains(&usize::from(c))));
        n.focus(id(100), FocusOpts::default(), 0.0);
        n.jump(false, 0.0);
        assert_eq!(n.current(), Some(id(1)), "the top row remembers its last item");
    }

    #[test]
    fn a_list_inside_a_page_scrolls_both() {
        // A page (0..1000 on screen, 2400 tall) with a sheet's list at 1600..2000 in it (400 on
        // screen, rows of 100, 1000 tall): the last row needs the list and the page to move.
        let mut n = Nav::new(1920.0, 1000.0);
        n.add_scroller(Scroller { key: 1, rect: [0.0, 0.0, 1920.0, 1000.0], content: [1920.0, 2400.0], ..Default::default() });
        n.add_scroller(Scroller { key: 2, rect: [0.0, 1600.0, 800.0, 400.0], content: [800.0, 1000.0], parent: Some(1), ..Default::default() });
        for i in 0..10 {
            n.add(Focusable::new(id(i), [0.0, 1600.0 + i as f32 * 100.0, 800.0, 100.0]).scroll(2));
        }
        n.focus(id(9), INSTANT, 0.0);
        let s: HashMap<u64, [f32; 2]> = n.take_scrolls().into_iter().collect();
        assert_eq!(s[&2][1], 600.0, "the list to its end");
        // The row lands at 2500 - 600 = 1900..2000 on the page: the page brings it in view.
        let page = s[&1][1];
        assert!(1900.0 - page >= 0.0 && 2000.0 - page <= 1000.0, "page at {page}");
    }

    #[test]
    fn pages_keep_the_focused_row_a_little_above_the_middle() {
        let mut n = tall_page();
        n.focus(id(100 + 8 * 4), INSTANT, 0.0); // row 8: y 1350..1480
        let s = n.take_scrolls();
        // Target: centre (1415) - 0.45 * 1000 = 965, but the row's top minus the pad (1338) is
        // lower... the row is below the target? 1350 - 12 = 1338 > 965 so the target wins.
        assert_eq!(s, vec![(1, [0.0, 965.0])]);
        // Something already in view does not scroll.
        let mut n = tall_page();
        n.focus(id(104), INSTANT, 0.0);
        assert!(n.take_scrolls().is_empty());
        // Clamped to the content.
        let mut n = tall_page();
        n.focus(id(300), INSTANT, 0.0);
        assert_eq!(n.take_scrolls(), vec![(1, [0.0, 1400.0])]);
    }

    #[test]
    fn rails_scroll_just_enough_with_their_padding() {
        let mut n = Nav::new(1000.0, 800.0);
        n.add_scroller(Scroller { key: 7, rect: [0.0, 0.0, 1000.0, 300.0], content: [2440.0, 300.0], pad_left: 40.0, ..Default::default() });
        let g = n.add_group(Group::new("rail", [0.0, 0.0, 2440.0, 300.0]).rail());
        for i in 0..12 {
            n.add(Focusable::new(id(i), [40.0 + i as f32 * 200.0, 0.0, 180.0, 300.0]).group(g).rail(7));
        }
        n.focus(id(4), INSTANT, 0.0); // 840..1020: past the right edge
        assert_eq!(n.take_scrolls(), vec![(7, [1020.0 + 40.0 - 1000.0, 0.0])]);
    }

    #[test]
    fn smooth_scrolls_glide_and_repeats_aim_from_the_last_target() {
        let mut n = tall_page();
        n.focus(id(100 + 8 * 4), FocusOpts::default(), 0.0);
        assert!(n.scrolling());
        assert!(n.tick(0.016));
        let first = n.take_scrolls()[0].1[1];
        assert!(first > 0.0 && first < 965.0, "{first}");
        // A held direction moves again before the glide arrives: the next aim builds on 965, not on
        // where the box is now, so fast repeats never undershoot.
        // Row 11 (1800..1930) is already inside the view the glide is heading for (965..1965).
        n.focus(id(100 + 11 * 4), FocusOpts::default(), 0.1);
        assert!(n.take_scrolls().is_empty());
        n.focus(id(300), FocusOpts::default(), 0.1);
        let mut t = 0.1;
        while n.tick(t) {
            t += 0.016;
        }
        let end = n.take_scrolls()[0].1[1];
        assert_eq!(end, 1400.0);
        assert!(t < 0.8, "settles within a few hundred ms ({t})");
    }

    #[test]
    fn restore_puts_the_page_back_where_it_was() {
        let mut n = tall_page();
        n.restore_later(Some(Spot { group: Some("grid".into()), index: 20, scroll_top: Some(700.0) }), 0.0);
        n.ensure(0.0);
        assert_eq!(n.current(), Some(id(120)));
        assert_eq!(n.take_scrolls(), vec![(1, [0.0, 700.0])]);
    }

    #[test]
    fn taps_hit_the_topmost_focusable_in_scope() {
        let mut n = home();
        assert_eq!(n.hit(150.0, 150.0), Some(id(0)));
        assert_eq!(n.hit(10.0, 10.0), None);
        n.add(Focusable::new(id(50), [0.0, 0.0, 400.0, 400.0]).scope(1));
        assert_eq!(n.hit(150.0, 150.0), Some(id(50)));
        assert_eq!(n.hit(1600.0, 520.0), None, "the page under a modal takes no taps");
    }

    #[test]
    fn preload_neighbours_after_focus_rests() {
        let mut n = home();
        n.focus(id(2), FocusOpts::default(), 1.0);
        assert!(n.take_preload(1.05).is_none());
        let (f, near) = n.take_preload(1.2).unwrap();
        assert_eq!(f, id(2));
        assert_eq!(near, vec![id(3), id(4), id(5), id(0), id(1)]);
        assert!(n.take_preload(2.0).is_none());
    }

    #[test]
    fn focus_map_scores_like_nav() {
        // The POC API answers the same as Nav for the same geometry.
        let n = home();
        for from in [0u64, 3, 5, 10, 11, 20] {
            for d in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
                let a = n.item(id(from)).unwrap();
                let cands = n.items.iter().filter(|i| i.id != a.id).map(|i| (i.id, i.rect, i.group));
                let x = best_in_dir(&a.rect, a.group, cands, d, 1920.0);
                let mut m = Nav::new(1920.0, 1200.0);
                m.items = n.items.clone();
                m.groups = n.groups.clone();
                m.cur = Some(id(from));
                let moved = m.move_dir(d, 0.0);
                assert_eq!(moved, x.is_some());
                if let Some(x) = x {
                    // Nav also applies group memory; with none set they agree.
                    assert_eq!(m.current(), Some(x), "from {from} {d:?}");
                }
            }
        }
    }

    // ---- hints (hints.ts, Chrome.tsx HintBar) ----

    fn labels(h: &[super::hints::Hint]) -> Vec<(&'static str, &str)> {
        h.iter().map(|h| (h.b.name(), h.label.as_str())).collect()
    }

    #[test]
    fn hint_bar_on_home() {
        let base = vec![hint(Key::Y, "Search"), hint(Key::LbRb, "Tabs"), hint(Key::Quick, "Quick settings")];
        let st = Stack::default();
        let f = Focused { a: Some("Play".into()), menu: Some("Options".into()), focused: true, ..Default::default() };
        assert_eq!(labels(&compose(None, &base, &st, &f, false)), vec![("a", "Play"), ("y", "Search"), ("lbrb", "Tabs"), ("start", "Options"), ("quick", "Quick settings")]);
        // Off Home, B goes back.
        let f = Focused { focused: true, ..Default::default() };
        assert_eq!(labels(&compose(None, &base, &st, &f, true)), vec![("a", "Select"), ("b", "Back"), ("y", "Search"), ("lbrb", "Tabs"), ("quick", "Quick settings")]);
    }

    #[test]
    fn the_topmost_hints_win_and_go_away_with_their_screen() {
        let base = vec![hint(Key::Y, "Search"), hint(Key::LbRb, "Tabs")];
        let mut st = Stack::default();
        let a = st.push(vec![hint(Key::X, "Refresh"), hint(Key::LtRt, "Sections")]);
        let b = st.push(vec![hint(Key::X, "Favourite"), hint(Key::B, "Close")]);
        let f = Focused::default();
        assert_eq!(labels(&compose(None, &base, &st, &f, true)), vec![("b", "Close"), ("x", "Favourite"), ("y", "Search"), ("lbrb", "Tabs"), ("ltrt", "Sections")]);
        st.remove(b);
        assert_eq!(labels(&compose(None, &base, &st, &f, true)), vec![("b", "Back"), ("x", "Refresh"), ("y", "Search"), ("lbrb", "Tabs"), ("ltrt", "Sections")]);
        st.remove(a);
        // The focused node's own X and Y beat the screen's.
        let f = Focused { x: Some("Details".into()), y: Some("Sell".into()), focused: true, ..Default::default() };
        assert_eq!(labels(&compose(None, &base, &st, &f, false)), vec![("a", "Select"), ("x", "Details"), ("y", "Sell"), ("lbrb", "Tabs")]);
        // A screen's Menu hint beats the focused game's Options.
        let mut st = Stack::default();
        st.push(vec![hint(Key::Start, "Sort")]);
        let f = Focused { menu: Some("Options".into()), ..Default::default() };
        assert_eq!(labels(&compose(None, &[], &st, &f, false)), vec![("start", "Sort")]);
    }

    #[test]
    fn a_screens_own_list_keeps_quick_menu_and_search() {
        let base = vec![hint(Key::Y, "Search"), hint(Key::LbRb, "Tabs"), hint(Key::Quick, "Quick settings")];
        let over = vec![hint(Key::LtRt, "Sections"), hint(Key::A, "Open"), hint(Key::LbRb, "Tabs")];
        let st = Stack::default();
        let f = Focused::default();
        assert_eq!(labels(&compose(Some(&over), &base, &st, &f, true)), vec![("a", "Open"), ("y", "Search"), ("lbrb", "Tabs"), ("ltrt", "Sections"), ("quick", "Quick settings")]);
        // A list with its own system hint drops Quick from the base, but keeps Search.
        let over = vec![hint(Key::Qam, "Close"), hint(Key::A, "Open")];
        assert_eq!(labels(&compose(Some(&over), &base, &st, &f, true)), vec![("a", "Open"), ("y", "Search"), ("qam", "Close")]);
    }

    #[test]
    fn exit_hint_follows_the_mode() {
        assert_eq!(exit_hint(false), hint(Key::Rare, "Menu"));
        assert_eq!(exit_hint(true), hint(Key::Select, "Hold View for the Rare menu"));
    }

    #[test]
    fn focused_hints_come_from_the_focused_node() {
        let mut n = home();
        n.items[0] = Focusable::new(id(0), [100.0, 100.0, 180.0, 200.0]).group(0).hints(Some("Play"), Some("Details"), None).game();
        assert_eq!(n.focused_hints(), Focused::default());
        n.focus(id(0), FocusOpts::default(), 0.0);
        assert_eq!(n.focused_hints(), Focused { a: Some("Play".into()), x: Some("Details".into()), y: None, menu: Some("Options".into()), focused: true });
    }
}
