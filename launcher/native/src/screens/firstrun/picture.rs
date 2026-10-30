//! First run: your picture. Shown after an account is added (passed over without one): Rare's
//! default pictures, the account's own default marked; A uses one and moves on, Continue keeps the
//! marked one. NFTs the account holds can be picked later from the profile's Change picture.

use super::kit::step;
use crate::screens::sui::profile::default_tile;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, BSize, ButtonSpec, BVariant};

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Use(String),
    Continue,
    Back,
}

/// `current`: the account's picture (a default id; anything else marks none).
pub fn view(current: &str) -> El<Msg> {
    let grid = div("grid grid-cols-10 gap-[1rem]").attr("data-nav-group", "firstrun-pictures").attr("data-nav-remember", "").kids(crate::avatars::all().map(|d| default_tile(d.id, d.name, d.id == current, d.id == current, Msg::Use(d.id.to_string()))));
    let content = vec![
        div("mb-[0.75rem] text-label text-muted").text("Default"),
        grid,
        div("mt-[1.25rem] text-small text-muted").text("An NFT you hold can be your picture too. Pick it from Change picture on your profile."),
    ];
    step(text("Pick a picture"), Some(text("Friends see it next to your name. You can change it any time.")), content, vec![button_el(ButtonSpec::new("Continue").v(BVariant::Primary).size(BSize::Lg).class("min-w-[10rem]"), Some(Msg::Continue))], true).on_pad("b", Msg::Back)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture() {
        let v = view("owl");
        assert_eq!(v.texts()[..3], ["Pick a picture", "Friends see it next to your name. You can change it any time.", "Default"]);
        assert_eq!(v.find_by_attr("data-nav-group", "firstrun-pictures").unwrap().kids.len(), crate::avatars::SET.len());
        assert!(v.dump().contains("avatars/512/owl.png"));
        assert_eq!(v.pad("b"), Some(&Msg::Back));
    }
}
