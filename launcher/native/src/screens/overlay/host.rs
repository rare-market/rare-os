//! Rare over a game (overlay/Overlay.tsx): the Rare menu (press the Rare button), the Switcher
//! (hold it), the power menu (hold the power button) or Quick Settings (the Quick button), as the
//! native overlay session says (rare-overlay, docs/OVERLAY.md). The same surfaces open over the
//! launcher as its own sheets. Always present, so it also holds "Close a paused game?" (quick
//! resume's two-game limit).
//!
//! The host owns what is on screen and routes pad input: the paused-game sheet first, then the
//! open surface. It mounts a surface by building it on the scene root and remembering the nodes
//! that appeared, so closing removes exactly those.

use super::common::*;
use super::fixtures::Session;
use super::menu::{MenuData, RareMenu};
use super::power::PowerMenu;
use super::switcher::{FreezeFull, PausedFull, RecentGame, RunItem, Switcher, SwitcherState};
use rare_render::input::Pad;
use rare_render::{Id, Src};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Menu,
    Quick,
    Power,
    Switcher,
}

/// What a surface needs to open (the data the web hooks fetch).
pub enum Open {
    Menu(Box<MenuData>),
    Power { frame: Option<Src>, game_id: Option<String> },
    Switcher { frame: Option<Src>, game_id: Option<String>, state: SwitcherState, running: Vec<RunItem>, library: Vec<RecentGame> },
    /// Quick Settings over the frozen frame; the screen comes from the shell (components/QuickSettings).
    Quick { frame: Option<Src> },
    /// A game asked for a Rare sheet (Local API): only the dimmed game stays behind it.
    SheetOnly { frame: Option<Src> },
}

struct Mounted {
    mode: Option<Mode>,
    screen: Box<dyn Screen>,
    nodes: Vec<Id>,
}

/// Builds the Quick Settings screen for the frozen-frame overlay (the shell supplies it).
pub type QuickBuilder = Box<dyn Fn(&mut Cx, Option<Src>, f64) -> Box<dyn Screen>>;

pub struct InGameOverlay {
    open: Option<Mounted>,
    paused: Option<Mounted>,
    quick: Option<QuickBuilder>,
}

fn mount(cx: &mut Cx, mode: Option<Mode>, build: impl FnOnce(&mut Cx) -> Box<dyn Screen>) -> Mounted {
    let root = cx.scene.root;
    let before = cx.scene.children(root);
    let screen = build(cx);
    let nodes = cx.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
    Mounted { mode, screen, nodes }
}

fn unmount(cx: &mut Cx, m: Mounted) {
    for n in m.nodes {
        cx.scene.remove(n);
    }
}

impl InGameOverlay {
    pub fn new(quick: Option<QuickBuilder>) -> InGameOverlay {
        InGameOverlay { open: None, paused: None, quick }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some() || self.paused.is_some()
    }

    pub fn mode(&self) -> Option<Mode> {
        self.open.as_ref().and_then(|m| m.mode)
    }

    /// Opens a surface (replacing the one that is up).
    pub fn open(&mut self, cx: &mut Cx, what: Open, now: f64) {
        self.close(cx);
        let m = match what {
            Open::Menu(d) => mount(cx, Some(Mode::Menu), |cx| Box::new(RareMenu::build(cx, *d, now))),
            Open::Power { frame, game_id } => mount(cx, Some(Mode::Power), |cx| Box::new(PowerMenu::build(cx, &Session { game_id, frame, guest: false }, now))),
            Open::Switcher { frame, game_id, state, running, library } => mount(cx, Some(Mode::Switcher), |cx| Box::new(Switcher::build(cx, &Session { game_id, frame, guest: false }, &state, running, &library, now))),
            Open::Quick { frame } => {
                let q = self.quick.as_ref();
                mount(cx, Some(Mode::Quick), |cx| match q {
                    Some(b) => b(cx, frame, now),
                    None => Box::new(FrozenOnly::build(cx, frame)),
                })
            }
            Open::SheetOnly { frame } => mount(cx, None, |cx| Box::new(FrozenOnly::build(cx, frame))),
        };
        self.open = Some(m);
    }

    pub fn close(&mut self, cx: &mut Cx) {
        if let Some(m) = self.open.take() {
            unmount(cx, m);
        }
    }

    /// Leaving a game would make three paused: ask which to close.
    pub fn ask_paused(&mut self, cx: &mut Cx, ask: &FreezeFull, all: &[RunItem], now: f64) {
        if let Some(m) = self.paused.take() {
            unmount(cx, m);
        }
        self.paused = Some(mount(cx, None, |cx| Box::new(PausedFull::build(cx, ask, all, now))));
    }

    pub fn dismiss_paused(&mut self, cx: &mut Cx) {
        if let Some(m) = self.paused.take() {
            unmount(cx, m);
        }
    }

    /// Routes a press: the paused-game sheet has it first, then the open surface. A close from the
    /// surface closes it here; every action comes back for the app to run.
    pub fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        if let Some(p) = self.paused.as_mut() {
            let out = p.screen.pad(cx, pad, now);
            if matches!(out, Out::Close) || matches!(&out, Out::Act(a) if a.starts_with("keep:") || a.starts_with("quit:")) {
                self.dismiss_paused(cx);
            }
            return out;
        }
        let Some(m) = self.open.as_mut() else { return Out::None };
        let out = m.screen.pad(cx, pad, now);
        if out == Out::Close {
            self.close(cx);
        }
        out
    }

    pub fn tick(&mut self, cx: &mut Cx, now: f64) {
        if let Some(m) = self.open.as_mut() {
            m.screen.tick(cx, now);
        }
        if let Some(m) = self.paused.as_mut() {
            m.screen.tick(cx, now);
        }
    }

    pub fn next_deadline(&self) -> Option<f64> {
        [self.open.as_ref(), self.paused.as_ref()].into_iter().flatten().filter_map(|m| m.screen.next_deadline()).fold(None, |a: Option<f64>, d| Some(a.map_or(d, |x| x.min(d))))
    }
}

/// The dimmed frame alone (a game's own Rare sheet is up over it, or Quick Settings is not wired).
pub struct FrozenOnly;

impl FrozenOnly {
    pub fn build(cx: &mut Cx, frame: Option<Src>) -> FrozenOnly {
        let root = cx.scene.root;
        let page = cx.scene.add(root, super::lay::s().fill().done(), rare_render::Node::default());
        frozen(cx, page, frame);
        FrozenOnly
    }
}

impl Screen for FrozenOnly {
    fn pad(&mut self, _cx: &mut Cx, pad: Pad, _now: f64) -> Out {
        // Only what must not reach the launcher underneath stops here; B and Quick close it.
        match pad {
            Pad::B => Out::Close,
            _ => Out::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;
    use rare_render::{ImageLoader, Scene, TextSys};
    use std::sync::Arc;

    static FONTS: [&[u8]; 1] = [include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf")];

    fn with_cx(f: impl FnOnce(&mut Cx)) {
        let mut text = TextSys::new(&FONTS, false, 512);
        let mut images = ImageLoader::new(1, Arc::new(|| {}), None);
        let mut scene = Scene::new(1280.0, 800.0, 16.0);
        let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
        f(&mut cx);
    }

    #[test]
    fn open_replaces_and_close_removes_exactly_its_nodes() {
        with_cx(|cx| {
            let root = cx.scene.root;
            let base = cx.scene.children(root).len();
            let mut o = InGameOverlay::new(None);
            let s = fixtures::session();
            o.open(cx, Open::Power { frame: s.frame.clone(), game_id: s.game_id.clone() }, 0.0);
            assert_eq!(o.mode(), Some(Mode::Power));
            assert_eq!(cx.scene.children(root).len(), base + 1);
            o.open(cx, Open::SheetOnly { frame: None }, 0.0);
            assert_eq!(o.mode(), None);
            assert_eq!(cx.scene.children(root).len(), base + 1);
            o.close(cx);
            assert!(!o.is_open());
            assert_eq!(cx.scene.children(root).len(), base);
        });
    }

    #[test]
    fn b_closes_the_power_menu_and_a_reports_the_action() {
        with_cx(|cx| {
            let mut o = InGameOverlay::new(None);
            o.open(cx, Open::Power { frame: None, game_id: None }, 0.0);
            assert_eq!(o.pad(cx, Pad::A, 0.1), Out::Act("suspend".into()));
            assert!(o.is_open());
            assert_eq!(o.pad(cx, Pad::B, 0.2), Out::Close);
            assert!(!o.is_open());
        });
    }

    #[test]
    fn the_paused_sheet_takes_presses_first() {
        with_cx(|cx| {
            let mut o = InGameOverlay::new(None);
            o.open(cx, Open::Power { frame: None, game_id: None }, 0.0);
            o.ask_paused(cx, &super::super::switcher::model::fixture_freeze_full(), &super::super::switcher::model::fixture_running(), 0.0);
            assert_eq!(o.pad(cx, Pad::B, 0.1), Out::Act("keep:app:telegram".into()));
            // The sheet is gone; the power menu underneath gets the next press.
            assert_eq!(o.pad(cx, Pad::A, 0.2), Out::Act("suspend".into()));
        });
    }
}
