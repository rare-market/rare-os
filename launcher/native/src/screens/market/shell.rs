//! The Marketplace tab's page stack (marketplace/Marketplace.tsx, nav.ts, page.ts). Every page in
//! the stack stays built (the ones underneath are hidden and get no input), so B returns instantly
//! to the same scroll position with focus on the element that opened the page (each page keeps its
//! own focus engine). The page on top is asked about every press first; what it leaves goes to the
//! stack: B pops (the front page hands it to the launcher), Y opens search, directions move focus.

use super::data::Answers;
use super::host::*;
use rare_market::nav::{MarketNav, View};
use rare_render::input::Pad;
use rare_render::{Id, Node};
use std::rc::Rc;

/// What a page did with a press.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Not this page's: the stack handles it.
    Pass,
    /// Taken, nothing to report.
    None,
    Moved,
    /// Open a page on top.
    Push(View),
    /// Close the page (its own Back, a sheet's Done).
    Pop,
    /// Up to the launcher (section switch, open on rare.market, the keyboard).
    Out(Out),
}

/// One page of the stack: a view function on its own host.
pub trait MkPage {
    fn host(&mut self) -> &mut Host;
    fn render(&mut self, cx: &mut Cx, now: f64);
    fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Act;
    fn hints(&self) -> Vec<(&'static str, String)>;
    /// The search page answers Y itself (Edit search).
    fn is_search(&self) -> bool {
        false
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.host().tick(cx, now) {
            self.render(cx, now);
        }
    }
    /// The rare.market paths this page reads now (the next page of a list once focus nears its
    /// end): the app fetches the missing ones and hands the answers back with `set_answers`.
    fn wants(&self) -> Vec<String>;
    /// When the page wants a tick to ask again (a focus rest), if ever.
    fn deadline(&self) -> Option<f64> {
        None
    }
    fn set_answers(&mut self, a: Rc<Answers>);
    /// The keyboard's answer for the search page (None: cancelled).
    fn set_query(&mut self, _cx: &mut Cx, _q: Option<&str>, _now: f64) -> bool {
        false
    }
    /// The purchase moved on (the item page's buy sheet).
    fn set_buy_phase(&mut self, _cx: &mut Cx, _phase: super::buy::Phase, _now: f64) -> bool {
        false
    }
}

struct Frame {
    node: Id,
    page: Box<dyn MkPage>,
    /// The person moved focus on this page: late data no longer moves it (useLandingFocus).
    moved: bool,
}

impl Frame {
    /// useLandingFocus: the page's default element, again when its data arrives, as long as the
    /// person has not moved away from where focus first landed.
    fn land(&mut self, cx: &mut Cx, now: f64) {
        if self.moved {
            return;
        }
        let h = self.page.host();
        h.nav.focus_first(now);
        h.view.set_focus(cx.scene, cx.t, h.nav.current(), now);
    }
}

pub struct Marketplace {
    pub a: Rc<Answers>,
    parent: Id,
    pub nav: MarketNav,
    frames: Vec<Frame>,
    /// MarketLoadBar: the 2px bar along the top while the page on top waits for an answer.
    bar: Id,
    loading: bool,
}

/// Builds the page for a view in `node`.
pub fn make(cx: &mut Cx, node: Id, a: &Rc<Answers>, view: &View, frame: u64, now: f64) -> Box<dyn MkPage> {
    match view {
        View::Home => Box::new(super::home::HomePage::new(cx, node, a.clone(), frame, now)),
        View::Collection { slug } => Box::new(super::collection::CollectionPage::new(cx, node, a.clone(), slug, frame, now)),
        View::Item { id, card, thumb } => Box::new(super::item::ItemPage::new(cx, node, a.clone(), id, card.as_deref().cloned(), *thumb, frame, now)),
        View::Search { q } => Box::new(super::search::SearchPage::new(cx, node, a.clone(), q, frame, now)),
        View::Account { address, label } => Box::new(super::account::AccountPage::new(cx, node, a.clone(), address, label, frame, now)),
    }
}

impl Marketplace {
    /// The front page, with `views` opened over it (`?mk=collection/popkins,item/0x...`).
    pub fn new(cx: &mut Cx, parent: Id, a: Rc<Answers>, views: Vec<View>, now: f64) -> Marketplace {
        let r = cx.r();
        let bar = cx.scene.add(parent, s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(2.0).to_style(r), Node::default().z(30).clip().opacity(0.0));
        let seg = cx.scene.add(bar, s().fill().to_style(r), Node::default());
        cx.scene.add(seg, s().abs(Some(-2.0 / r), None, None, Some(0.0)).wp(1.0).h_px(6.0).to_style(r), Node::bg(tok::fg(0.55)).origin(0.0, 0.5));
        let mut m = Marketplace { a, parent, nav: MarketNav::default(), frames: Vec::new(), bar, loading: false };
        m.nav.reset(views);
        let stack = m.nav.stack.clone();
        for (i, f) in stack.iter().enumerate() {
            let node = m.mount(cx);
            let page = make(cx, node, &m.a, &f.view, f.key, now);
            if i + 1 < stack.len() {
                cx.scene.n(node).hidden = true;
            }
            m.frames.push(Frame { node, page, moved: false });
        }
        cx.scene.mark_dirty();
        m
    }

    fn mount(&mut self, cx: &mut Cx) -> Id {
        let st = s().fill().to_style(cx.r());
        cx.scene.add(self.parent, st, Node::default())
    }

    fn drop_frame(cx: &mut Cx, mut f: Frame) {
        f.page.host().view.clear(cx.scene);
        cx.scene.remove(f.node);
    }

    pub fn top(&mut self) -> &mut dyn MkPage {
        self.frames.last_mut().expect("home").page.as_mut()
    }

    /// Opens a page on top, keeping what had focus underneath.
    pub fn push(&mut self, cx: &mut Cx, view: View, now: f64) {
        let focused = self.frames.last_mut().and_then(|f| f.page.host().tag()).map(|t| t.to_string());
        self.nav.push(view.clone(), focused);
        if let Some(f) = self.frames.last() {
            cx.scene.n(f.node).hidden = true;
        }
        // Deep chains drop the oldest pages above home, as the stack did.
        while self.frames.len() >= self.nav.stack.len() {
            let f = self.frames.remove(1);
            Self::drop_frame(cx, f);
        }
        let key = self.nav.top().key;
        let node = self.mount(cx);
        let page = make(cx, node, &self.a, &view, key, now);
        self.frames.push(Frame { node, page, moved: false });
        cx.scene.mark_dirty();
    }

    /// Back one page; false on the front page.
    pub fn pop(&mut self, cx: &mut Cx, now: f64) -> bool {
        if !self.nav.pop() {
            return false;
        }
        if let Some(f) = self.frames.pop() {
            Self::drop_frame(cx, f);
        }
        if let Some(f) = self.frames.last_mut() {
            cx.scene.n(f.node).hidden = false;
            f.page.render(cx, now);
        }
        cx.scene.mark_dirty();
        true
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// The paths the page on top reads that have no answer yet: the app fetches these.
    /// The top page's own wake-up (see [`MkPage::deadline`]).
    pub fn deadline(&self) -> Option<f64> {
        self.frames.last().and_then(|f| f.page.deadline())
    }

    pub fn missing(&self) -> Vec<String> {
        self.frames.last().map(|f| f.page.wants()).unwrap_or_default().into_iter().filter(|p| !self.a.has(p) && !self.a.failed(p)).collect()
    }

    /// New answers (a fetch landed, a refresh): every page redraws from them.
    pub fn set_answers(&mut self, cx: &mut Cx, a: Rc<Answers>, now: f64) {
        self.a = a;
        for f in self.frames.iter_mut() {
            f.page.set_answers(self.a.clone());
            f.page.render(cx, now);
            f.land(cx, now);
        }
    }

    /// The keyboard's answer to the search page's `osk:` act. Cancelled on a page opened without a
    /// term closes it, as the web does.
    pub fn search_answer(&mut self, cx: &mut Cx, q: Option<&str>, now: f64) -> Out {
        let empty = matches!(&self.nav.top().view, View::Search { q } if q.is_empty());
        if q.is_none() && empty && self.top().is_search() {
            self.pop(cx, now);
            return Out::Moved;
        }
        if let (Some(q), View::Search { .. }) = (q, &self.nav.top().view) {
            let key = self.nav.top().key;
            if let Some(f) = self.nav.stack.iter_mut().find(|f| f.key == key) {
                f.view = View::Search { q: q.trim().to_string() };
            }
        }
        if self.top().set_query(cx, q, now) {
            if let Some(f) = self.frames.last_mut() {
                f.land(cx, now);
            }
            Out::Moved
        } else {
            Out::None
        }
    }

    /// The purchase the item page asked for (`market:buy:<id>`) moved on.
    pub fn buy_phase(&mut self, cx: &mut Cx, phase: super::buy::Phase, now: f64) -> bool {
        self.top().set_buy_phase(cx, phase, now)
    }

    /// Shows or hides the loading bar (nothing loops while it is hidden).
    fn load_bar(&mut self, cx: &mut Cx, now: f64) {
        let on = !self.missing().is_empty();
        if on == self.loading {
            return;
        }
        self.loading = on;
        cx.scene.n(self.bar).opacity.set(if on { 1.0 } else { 0.0 }, now, 0.2);
        let Some(seg) = cx.scene.children(self.bar).first().copied() else { return };
        let Some(strip) = cx.scene.children(seg).first().copied() else { return };
        if on {
            use crate::kit::controls::{LOAD_SX, LOAD_X};
            use rare_render::anim::Keys;
            let w = cx.scene.w;
            let xs: &'static [(f32, f32)] = Box::leak(LOAD_X.frames.iter().map(|(t, v)| (*t, v * w)).collect::<Vec<_>>().into_boxed_slice());
            let kx: &'static Keys = Box::leak(Box::new(Keys { frames: xs, ease: LOAD_X.ease }));
            cx.scene.n(seg).tx.start_loop(kx, 1.1, now);
            cx.scene.n(strip).scale.start_loop(&LOAD_SX, 1.1, now);
        } else {
            cx.scene.n(seg).tx.snap(0.0);
            cx.scene.n(strip).scale.snap(1.0);
        }
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        let act = self.top().press(cx, b, now);
        let act = if act == Act::Pass {
            match b {
                Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                    if self.top().host().step(cx, b, now) {
                        if let Some(f) = self.frames.last_mut() {
                            f.moved = true;
                        }
                        Act::Moved
                    } else {
                        Act::None
                    }
                }
                Pad::B => Act::Pop,
                Pad::Y if !self.top().is_search() => Act::Push(View::Search { q: String::new() }),
                _ => Act::None,
            }
        } else {
            // The page did something with the press (a tab switch on LT/RT, a sheet): late data
            // no longer moves focus back to where the page landed.
            if act == Act::Moved {
                if let Some(f) = self.frames.last_mut() {
                    f.moved = true;
                }
            }
            act
        };
        match act {
            Act::Pass | Act::None => Out::None,
            Act::Moved => Out::Moved,
            Act::Push(v) => {
                // A search opened without a term asks for one at once (SearchPage's first edit).
                let ask = matches!(&v, View::Search { q } if q.is_empty());
                self.push(cx, v, now);
                if ask {
                    Out::Act(super::search::ASK.into())
                } else {
                    Out::Moved
                }
            }
            Act::Pop => {
                if self.pop(cx, now) {
                    Out::Moved
                } else {
                    Out::Close
                }
            }
            Act::Out(o) => o,
        }
    }
}

impl Page for Marketplace {
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.frames.last().map(|f| f.page.hints()).unwrap_or_default()
    }
}

impl Screen for Marketplace {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.top().host().touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        for f in self.frames.iter_mut() {
            f.page.tick(cx, now);
        }
        self.load_bar(cx, now);
    }
}
