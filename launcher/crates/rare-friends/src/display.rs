//! How a Rare player is shown, everywhere: username first. A SuiNS name the player also owns is a
//! quiet second line (muted text, no pill). With neither, a short address. Pure.

use crate::types::Person;
use serde::Serialize;

/// The identity the display rule reads: an address, what rare.market sends to show (`name`: the
/// username, else the SuiNS name), the username and the SuiNS name.
pub type Identity = Person;

pub fn short_addr(a: &str) -> String {
    let head: String = a.chars().take(6).collect();
    let n = a.chars().count();
    let tail: String = a.chars().skip(n.saturating_sub(4)).collect();
    format!("{head}...{tail}")
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShownKind {
    Username,
    Suins,
    Address,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub primary: String,
    /// The SuiNS name, only when a username leads.
    pub secondary: Option<String>,
    pub kind: ShownKind,
}

fn is_suins(name: &str) -> bool {
    name.to_lowercase().ends_with(".sui")
}

pub fn shown(p: Option<&Identity>, fallback: &str) -> Shown {
    let Some(p) = p else {
        return Shown { primary: fallback.to_string(), secondary: None, kind: ShownKind::Address };
    };
    let username = p.username.as_deref().filter(|s| !s.is_empty());
    let suins = p.suins.as_deref().filter(|s| !s.is_empty());
    if let Some(u) = username {
        let secondary = suins.filter(|s| *s != u).map(str::to_string);
        return Shown { primary: u.to_string(), secondary, kind: ShownKind::Username };
    }
    // Older answers carry only `name`; it is a SuiNS name when it ends in .sui, else a username.
    let name = p.name.as_deref().filter(|s| !s.is_empty());
    if let Some(n) = name {
        if suins.is_none() && !is_suins(n) {
            return Shown { primary: n.to_string(), secondary: None, kind: ShownKind::Username };
        }
    }
    if let Some(s) = suins.or(name) {
        return Shown { primary: s.to_string(), secondary: None, kind: ShownKind::Suins };
    }
    Shown { primary: short_addr(&p.address), secondary: None, kind: ShownKind::Address }
}

/// What to call someone in one line ("Someone" when unknown).
pub fn name_of(p: Option<&Identity>, fallback: &str) -> String {
    shown(p, fallback).primary
}

/// [`name_of`] with the usual fallback (the store's `label`).
pub fn label(p: Option<&Identity>) -> String {
    name_of(p, "Someone")
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

    fn id(name: Option<&str>, username: Option<&str>, suins: Option<&str>) -> Identity {
        Identity { address: A.into(), name: name.map(Into::into), username: username.map(Into::into), suins: suins.map(Into::into), avatar_url: None }
    }

    #[test]
    fn username_leads_suins_is_the_quiet_second_line() {
        assert_eq!(shown(Some(&id(Some("mira"), Some("mira"), Some("mira.sui"))), "Someone"), Shown { primary: "mira".into(), secondary: Some("mira.sui".into()), kind: ShownKind::Username });
    }

    #[test]
    fn a_username_alone_has_no_second_line() {
        assert_eq!(shown(Some(&id(None, Some("mira"), None)), "Someone"), Shown { primary: "mira".into(), secondary: None, kind: ShownKind::Username });
    }

    #[test]
    fn suins_alone_leads_when_there_is_no_username() {
        assert_eq!(shown(Some(&id(Some("mira.sui"), None, Some("mira.sui"))), "Someone"), Shown { primary: "mira.sui".into(), secondary: None, kind: ShownKind::Suins });
    }

    #[test]
    fn falls_back_to_a_short_address() {
        assert_eq!(shown(Some(&id(None, None, None)), "Someone"), Shown { primary: "0xa049...887e".into(), secondary: None, kind: ShownKind::Address });
        assert_eq!(name_of(None, "Someone"), "Someone");
    }

    #[test]
    fn answers_that_only_carry_name_still_work() {
        assert_eq!(shown(Some(&id(Some("mira"), None, None)), "Someone").kind, ShownKind::Username);
        assert_eq!(shown(Some(&id(Some("mira.sui"), None, None)), "Someone").kind, ShownKind::Suins);
    }
}
