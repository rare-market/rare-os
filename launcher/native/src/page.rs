//! What a Phase 1 screen is to the app: a view over `State` (`view` builds an `El` tree, rebuilt
//! only when `stamp` moves; the engine's `View` diffs it into the scene), plus the handlers for the
//! presses it owns. A press on a focused element comes back as the `Act` its tag was given while
//! building (`Acts::tag`), so screens never keep node ids.

use crate::act::Act;
use crate::data::backend::Backend;
use crate::kit::Ui;
use crate::state::State;
use rare_render::focus::hints::Hint;
use rare_render::focus::Focusable;
use rare_render::view::El;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// The app's buttons: the engine's pad plus the SuiPlay's Quick key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Btn {
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
    /// The Rare button.
    Guide,
    /// The Quick button.
    Qam,
    /// The Rare button held (the Switcher).
    GuideHold,
    /// View + Menu held (the Switcher, silent rescue).
    ExitHold,
}

impl Btn {
    pub fn from_pad(p: rare_render::input::Pad) -> Btn {
        use rare_render::input::Pad;
        match p {
            Pad::Up => Btn::Up,
            Pad::Down => Btn::Down,
            Pad::Left => Btn::Left,
            Pad::Right => Btn::Right,
            Pad::A => Btn::A,
            Pad::B => Btn::B,
            Pad::X => Btn::X,
            Pad::Y => Btn::Y,
            Pad::Lb => Btn::Lb,
            Pad::Rb => Btn::Rb,
            Pad::Lt => Btn::Lt,
            Pad::Rt => Btn::Rt,
            Pad::Start => Btn::Start,
            Pad::Select => Btn::Select,
            Pad::Guide => Btn::Guide,
            Pad::Qam => Btn::Qam,
            Pad::GuideHold => Btn::GuideHold,
            Pad::ExitHold => Btn::ExitHold,
        }
    }

    pub fn parse(s: &str) -> Option<Btn> {
        Some(match s {
            "up" => Btn::Up,
            "down" => Btn::Down,
            "left" => Btn::Left,
            "right" => Btn::Right,
            "a" => Btn::A,
            "b" => Btn::B,
            "x" => Btn::X,
            "y" => Btn::Y,
            "lb" => Btn::Lb,
            "rb" => Btn::Rb,
            "lt" => Btn::Lt,
            "rt" => Btn::Rt,
            "start" | "menu" => Btn::Start,
            "select" | "view" => Btn::Select,
            "guide" | "rare" => Btn::Guide,
            "qam" | "quick" => Btn::Qam,
            "guidehold" => Btn::GuideHold,
            "exithold" => Btn::ExitHold,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Btn::Up => "up",
            Btn::Down => "down",
            Btn::Left => "left",
            Btn::Right => "right",
            Btn::A => "a",
            Btn::B => "b",
            Btn::X => "x",
            Btn::Y => "y",
            Btn::Lb => "lb",
            Btn::Rb => "rb",
            Btn::Lt => "lt",
            Btn::Rt => "rt",
            Btn::Start => "start",
            Btn::Select => "select",
            Btn::Guide => "guide",
            Btn::Qam => "qam",
            Btn::GuideHold => "guidehold",
            Btn::ExitHold => "exithold",
        }
    }
}

/// Sounds and rumble (nav/feedback.ts kinds). The input workstream plays them; the app only names
/// them (and never makes a sound on a development Mac).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feedback {
    Move,
    Select,
    Back,
    Tab,
    Open,
    Close,
    Launch,
    Toggle,
    Error,
    Boot,
}

/// A result applied on the UI thread.
pub type Done = Box<dyn FnOnce(&mut State, f64) + Send>;
pub type Job = Box<dyn FnOnce(&dyn Backend) -> Done + Send>;

/// What handlers ask of the app besides changing state: backend work off the UI thread, feedback,
/// and shared acts to run next.
pub struct Fx {
    pub backend: Arc<dyn Backend>,
    pub jobs: Vec<Job>,
    pub feedback: Vec<Feedback>,
    pub acts: Vec<Act>,
}

impl Fx {
    pub fn new(backend: Arc<dyn Backend>) -> Fx {
        Fx { backend, jobs: Vec::new(), feedback: Vec::new(), acts: Vec::new() }
    }

    /// Runs `f` on a worker with the backend; what it returns is applied to the state after.
    pub fn spawn(&mut self, f: impl FnOnce(&dyn Backend) -> Done + Send + 'static) {
        self.jobs.push(Box::new(f));
    }

    pub fn fb(&mut self, k: Feedback) {
        self.feedback.push(k);
    }

    pub fn run(&mut self, a: Act) {
        self.acts.push(a);
    }
}

/// The acts a build gave its focusables, by tag; the game a tag belongs to (Menu's Options).
#[derive(Default, Clone)]
pub struct Acts {
    v: Vec<(Act, Option<String>)>,
}

impl Acts {
    pub fn clear(&mut self) {
        self.v.clear();
    }
    pub fn tag(&mut self, a: Act) -> u32 {
        self.v.push((a, None));
        (self.v.len() - 1) as u32
    }
    /// A tag inside `[data-game]`.
    pub fn game(&mut self, a: Act, game: &str) -> u32 {
        self.v.push((a, Some(game.to_string())));
        (self.v.len() - 1) as u32
    }
    pub fn get(&self, tag: u32) -> Option<&Act> {
        self.v.get(tag as usize).map(|(a, _)| a)
    }
    pub fn game_of(&self, tag: u32) -> Option<&str> {
        self.v.get(tag as usize).and_then(|(_, g)| g.as_deref())
    }
}

pub trait Page {
    /// What the view reads from the state; a change rebuilds it.
    fn stamp(&self, st: &State) -> u64;
    /// The view. Tags for focusables come from `acts` (cleared before each build).
    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El;
    /// A press before the shell's own handling; true when used. `cur` is the focused element
    /// (and its act).
    fn pad(&mut self, _st: &mut State, _fx: &mut Fx, _b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        false
    }
    /// Touch: a finger on the focused element at `frac` (0 to 1) across it, tapped or dragged
    /// along it. True when it is a slider and its value follows the finger.
    fn slide(&mut self, _st: &mut State, _fx: &mut Fx, _cur: (&Focusable, Option<&Act>), _frac: f32) -> bool {
        false
    }
    /// Touch: whether this element is a slider (a sideways drag on it moves its value, not the page).
    fn is_slider(&self, _cur: (&Focusable, Option<&Act>)) -> bool {
        false
    }
    /// Touch: a tap on words that are not a focusable element (a filter tab's label in a tab row
    /// that LT/RT drive). True when the screen used it.
    fn tap_text(&mut self, _st: &mut State, _fx: &mut Fx, _text: &str) -> bool {
        false
    }
    /// An `Act::Local` from one of its elements.
    fn act(&mut self, _st: &mut State, _fx: &mut Fx, _code: u32) {}
    /// Timers. Returns the next time it needs a frame.
    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, _now: f64) -> Option<f64> {
        None
    }
    /// Extra hints while it is up (useHints).
    fn hints(&self, _st: &State) -> Vec<Hint> {
        Vec::new()
    }
    /// Focus moved; true when the view should rebuild (looks that depend on which element has
    /// focus beyond its own focus look, like Home's strip).
    fn focused(&mut self, _cur: Option<&Focusable>, _acts: &Acts, _now: f64) -> bool {
        false
    }
    /// After `focused`, with the state: a page that switches something on focus (Home's stage) does
    /// it in this frame. True when it changed and the views should rebuild.
    fn focus_moved(&mut self, _st: &mut State, _now: f64) -> bool {
        false
    }
    /// After a build: a tag the screen wants focus on now (a late-arriving default, like Home's
    /// strip taking focus from the stage buttons when the library lands).
    fn take_focus(&mut self, _cur: Option<&Focusable>) -> Option<u32> {
        None
    }
}

pub fn hash<T: Hash>(v: T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}
