//! The Rare overlay screens (launcher/src/overlay): the power menu, the Switcher and the Rare menu,
//! as native scenes with fixtures until rare-overlay lands.

pub mod background;
pub mod common;
pub mod fixtures;
pub mod host;
pub mod lay;
pub mod menu;
pub mod panels;
pub mod power;
pub mod switcher;
pub mod tabs;

pub use common::{Cx, Out, Screen};

/// Builds a screen by fixture name (the golden-image harness and the tests use this).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    if name == "power" {
        return Some(Box::new(power::PowerMenu::build(cx, &fixtures::session(), now)));
    }
    if name == "game-background" || name == "game-background-app" {
        let r = cx.r();
        let root = cx.scene.root;
        let col = cx.scene.add(root, super::overlay::lay::s().col().pad(2.5 * r, 2.5 * r, 0.0, 0.0).done(), rare_render::Node::default());
        let mut nav = common::Nav::default();
        let why = if name == "game-background" { background::BackgroundWhy::Online } else { background::BackgroundWhy::App };
        background::BackgroundSwitch::build(cx, &mut nav, col, Some(background::Background { pause: false, why, can_change: true }));
        let first = nav.items[0].id;
        nav.focus(cx.scene, first, now, false);
        return Some(Box::new(common::NavScreen { nav }));
    }
    menu::fixture(name, cx, now).or_else(|| switcher::fixture(name, cx, now))
}
