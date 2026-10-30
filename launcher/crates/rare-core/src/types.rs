//! Small types two or more backend crates (and both front ends) agree on.

use serde::{Deserialize, Serialize};

/// A controller button as the launcher sees it.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Pad {
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
    /// Guide held for GUIDE_HOLD: the launcher offers to close what is running.
    GuideHold,
    /// View + Menu held (the way out of a game) while the launcher already has the screen: the
    /// launcher shows what is running. Over a game the overlay opens instead (overlay::exit_chord).
    ExitHold,
    Qam,
}

impl Pad {
    /// The lowercase name the web UI and the web game bridge use.
    pub fn name(self) -> &'static str {
        match self {
            Pad::Up => "up",
            Pad::Down => "down",
            Pad::Left => "left",
            Pad::Right => "right",
            Pad::A => "a",
            Pad::B => "b",
            Pad::X => "x",
            Pad::Y => "y",
            Pad::Lb => "lb",
            Pad::Rb => "rb",
            Pad::Lt => "lt",
            Pad::Rt => "rt",
            Pad::Start => "start",
            Pad::Select => "select",
            Pad::Guide => "guide",
            Pad::GuideHold => "guidehold",
            Pad::ExitHold => "exithold",
            Pad::Qam => "qam",
        }
    }
}

/// Power profile.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Saved as "battery" before it was renamed.
    #[serde(alias = "battery")]
    Quiet,
    Balanced,
    Performance,
    Custom,
}

/// A browser game the user installed or one that ships with Rare OS.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct WebGame {
    pub id: String,
    pub title: String,
    pub url: String,
    pub cover: Option<String>,
    pub hero: Option<String>,
}
