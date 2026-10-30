//! The focus keeper: what the Rare button does wherever it is pressed, the order games come in
//! when you switch, and what happens to the game you leave. Pure logic; overlay/mod.rs and
//! overlay/switcher.rs carry it out. The rule:
//!
//! > The Rare button always means Rare: press it for the Rare menu, press again for Home, hold it
//! > for the Switcher.
//!
//! | Pressed | Nothing open, a game in front | Nothing open, the launcher in front | Rare menu open | Switcher open | Another Rare surface open (quick settings) |
//! |---|---|---|---|---|---|
//! | Rare | Rare menu over the game | the launcher's own Rare menu | Home | Rare menu | Rare menu |
//! | Rare held (or View + Menu held) | Switcher over the game | the launcher's own Switcher | Switcher | nothing | Switcher |
//!
//! The Rare button arrives as `Pad::Guide` (a press, sent on release) and `Pad::GuideHold` (held
//! 0.6 s), from gamepad.rs. In guest mode (Rare as a shortcut inside someone else's Steam Game
//! Mode) Steam owns Guide, so none of this runs there (overlay::route).

use serde::Serialize;

/// A Rare surface over the screen.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Menu,
    Switcher,
    /// Anything else Rare puts over a game (Quick Settings).
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// A press of the Rare button.
    Rare,
    /// The Rare button held, or the View + Menu rescue chord.
    Hold,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Bring the Rare menu up over the game in front.
    OpenMenu,
    /// Bring the Switcher up over the game in front.
    OpenSwitcher,
    /// The open surface shows this instead.
    Show(Surface),
    /// Close the surface and go Home; the game under it keeps its place and is left (frozen
    /// when its decision says so).
    Home,
    /// The launcher has the screen: its own UI takes the press.
    Launcher,
    /// Nothing happens.
    Nothing,
}

pub fn on_press(open: Option<Surface>, game_in_front: bool, press: Press) -> Step {
    match (open, press) {
        (Some(Surface::Menu), Press::Rare) => Step::Home,
        (Some(_), Press::Rare) => Step::Show(Surface::Menu),
        (Some(Surface::Switcher), Press::Hold) => Step::Nothing,
        (Some(_), Press::Hold) => Step::Show(Surface::Switcher),
        (None, Press::Rare) if game_in_front => Step::OpenMenu,
        (None, Press::Hold) if game_in_front => Step::OpenSwitcher,
        (None, _) => Step::Launcher,
    }
}

/// Most recently used first: the Switcher's order after Home.
#[derive(Default, Debug, Clone)]
pub struct Mru {
    list: Vec<String>,
}

impl Mru {
    pub const fn new() -> Self {
        Mru { list: Vec::new() }
    }

    /// `run` had the screen just now.
    pub fn touch(&mut self, run: &str) {
        self.list.retain(|r| r != run);
        self.list.insert(0, run.to_string());
    }

    /// The running games in Switcher order: most recently in front first, then any that never had
    /// the screen (started in the background), in their given order.
    pub fn order(&self, running: &[String]) -> Vec<String> {
        let mut out: Vec<String> = self.list.iter().filter(|r| running.contains(r)).cloned().collect();
        let rest: Vec<String> = running.iter().filter(|r| !out.contains(r)).cloned().collect();
        out.extend(rest);
        out
    }

    /// Forgets games that ended.
    pub fn retain(&mut self, running: &[String]) {
        self.list.retain(|r| running.contains(r));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Thaw before it shows.
    Thaw(String),
    /// Freeze it (freeze.rs decides whether its policy and the two-game limit allow).
    Freeze(String),
}

/// Leaving `from` (what had the screen under the surface; None for the launcher) for `to` (None
/// for Home). The game you go to thaws first, so it is never seen frozen; the one you leave is
/// frozen after (its still frame was taken when the surface opened). Going back to the same game
/// changes nothing.
pub fn leave(from: Option<&str>, to: Option<&str>) -> Vec<Effect> {
    let mut out = Vec::new();
    if let Some(t) = to {
        out.push(Effect::Thaw(t.to_string()));
    }
    if let Some(f) = from {
        if Some(f) != to {
            out.push(Effect::Freeze(f.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rare_button_everywhere() {
        use Step::*;
        // Over a game.
        assert_eq!(on_press(None, true, Press::Rare), OpenMenu);
        assert_eq!(on_press(None, true, Press::Hold), OpenSwitcher);
        // Pressed again in the menu: Home. Held in the menu: the Switcher.
        assert_eq!(on_press(Some(Surface::Menu), true, Press::Rare), Home);
        assert_eq!(on_press(Some(Surface::Menu), true, Press::Hold), Show(Surface::Switcher));
        // In the Switcher: a press goes to the menu, holding again does nothing.
        assert_eq!(on_press(Some(Surface::Switcher), true, Press::Rare), Show(Surface::Menu));
        assert_eq!(on_press(Some(Surface::Switcher), true, Press::Hold), Nothing);
        // Quick settings over a game: the Rare button switches to the menu or the Switcher.
        assert_eq!(on_press(Some(Surface::Other), true, Press::Rare), Show(Surface::Menu));
        assert_eq!(on_press(Some(Surface::Other), true, Press::Hold), Show(Surface::Switcher));
        // The launcher in front: its own UI (which also knows about first run).
        assert_eq!(on_press(None, false, Press::Rare), Launcher);
        assert_eq!(on_press(None, false, Press::Hold), Launcher);
        // A surface open counts, whatever is in front.
        assert_eq!(on_press(Some(Surface::Menu), false, Press::Rare), Home);
    }

    #[test]
    fn rare_then_a_is_always_back_to_the_game() {
        // Open the menu (Resume is focused), A closes it: nothing is left or frozen.
        assert_eq!(on_press(None, true, Press::Rare), Step::OpenMenu);
        assert_eq!(leave(Some("steam:620"), Some("steam:620")), vec![Effect::Thaw("steam:620".into())]);
    }

    #[test]
    fn leaving_freezes_after_the_next_one_thaws() {
        assert_eq!(leave(Some("a"), Some("b")), vec![Effect::Thaw("b".into()), Effect::Freeze("a".into())]);
        assert_eq!(leave(Some("a"), None), vec![Effect::Freeze("a".into())]);
        assert_eq!(leave(None, Some("b")), vec![Effect::Thaw("b".into())]);
        assert_eq!(leave(None, None), vec![]);
    }

    #[test]
    fn switcher_order() {
        let mut m = Mru::new();
        let running: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        // Nothing had the screen yet: as given.
        assert_eq!(m.order(&running), running);
        m.touch("a");
        m.touch("b");
        assert_eq!(m.order(&running), vec!["b", "a", "c"]);
        m.touch("a");
        assert_eq!(m.order(&running), vec!["a", "b", "c"]);
        // b ended.
        let now: Vec<String> = vec!["a".into(), "c".into()];
        m.retain(&now);
        assert_eq!(m.order(&now), vec!["a", "c"]);
        m.touch("c");
        assert_eq!(m.order(&now), vec!["c", "a"]);
    }
}
