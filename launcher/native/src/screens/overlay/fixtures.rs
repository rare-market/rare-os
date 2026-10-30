//! Fixture state for the overlay screens: the same shapes the web launcher's mocks give.

use rare_render::Src;

/// The frozen frame of the game under the menu (the web mock uses the game's art).
pub static FRAME: &[u8] = include_bytes!("../../../../render/assets/games/act/hero.jpg");

pub struct Session {
    pub game_id: Option<String>,
    pub frame: Option<Src>,
    pub guest: bool,
}

pub fn session() -> Session {
    Session { game_id: Some("web:act".into()), frame: Some(Src::Static("fixtures/act-hero.jpg", FRAME)), guest: false }
}
