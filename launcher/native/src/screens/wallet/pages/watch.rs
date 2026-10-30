//! Watch an address (launcher/src/wallet/ui/pages/Watch.tsx): a view-only account from a 0x address
//! or a SuiNS name.

use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::kit::{btn, field, page, FieldSpec, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::onboard::flow::{h, plain_error, Failure};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub input: String,
    pub name: String,
    pub busy: bool,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// A on the address field: keyboard "Address or SuiNS name"
    TypeAddress,
    /// A on the name field: keyboard "Name (optional)"
    TypeName,
    Address(String),
    Name(String),
    Add,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Ask { prompt: &'static str, value: String, placeholder: &'static str },
    /// `watch(input, name)`; answer with `done`, then `finishAdd(address)`
    Watch { input: String, name: Option<String> },
}

pub fn view(st: &State) -> El<Msg> {
    let off = st.input.trim().is_empty() || st.busy;
    page(
        Some("Watch an address"),
        Some(text("Follow any wallet's coins and collectibles. You cannot send from it.")),
        None,
        true,
        "",
        vec![div("flex flex-col gap-5")
            .kid(field(
                FieldSpec { label: Some("Address or SuiNS name"), value: &st.input, placeholder: Some("0x... or name.sui"), error: (!st.error.is_empty()).then_some(st.error.as_str()), def: true, ..Default::default() },
                Msg::TypeAddress,
            ))
            .kid(field(FieldSpec { label: Some("Name (optional)"), value: &st.name, placeholder: Some("Watch 1"), ..Default::default() }, Msg::TypeName))
            .kid(btn(if st.busy { "Adding" } else { "Start watching" }, Variant::Primary, &format!("w-full {}", if off { "pointer-events-none opacity-40" } else { "" }), Some(Msg::Add)))],
    )
}

pub fn update(st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::TypeAddress => Some(Effect::Ask { prompt: "Address or SuiNS name", value: st.input.clone(), placeholder: "0x... or name.sui" }),
        Msg::TypeName => Some(Effect::Ask { prompt: "Name (optional)", value: st.name.clone(), placeholder: "Watch 1" }),
        Msg::Address(v) => {
            st.input = v;
            None
        }
        Msg::Name(v) => {
            st.name = v;
            None
        }
        Msg::Add => {
            if st.input.trim().is_empty() || st.busy {
                return None;
            }
            st.busy = true;
            st.error.clear();
            let name = st.name.trim();
            Some(Effect::Watch { input: st.input.trim().into(), name: (!name.is_empty()).then(|| name.to_string()) })
        }
    }
}

pub fn done(st: &mut State, res: Result<(), Failure>) {
    st.busy = false;
    if let Err(e) = res {
        st.error = plain_error(&e, "Could not add that address");
    }
}

pub fn hints() -> Vec<Hint> {
    vec![h::select(), h::back()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch() {
        let mut st = State::default();
        let v = view(&st);
        assert_eq!(v.texts(), ["Watch an address", "Follow any wallet's coins and collectibles. You cannot send from it.", "Address or SuiNS name", "0x... or name.sui", "Name (optional)", "Watch 1", "Start watching"]);
        assert!(v.find_nav("Start watching").is_some());
        assert_eq!(update(&mut st, Msg::Add), None);
        update(&mut st, Msg::Address(" alice.sui ".into()));
        assert_eq!(update(&mut st, Msg::Add), Some(Effect::Watch { input: "alice.sui".into(), name: None }));
        assert!(view(&st).find_nav("Adding").is_some());
        done(&mut st, Err(Failure::msg("Name not found")));
        assert!(view(&st).contains_text("Name not found"));
    }
}
