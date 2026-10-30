//! The username picker's logic: the live line under the text while typing (rules first, then the
//! server's answer for that name) and the words around it. The on-screen keyboard and toasts are
//! the screen's; saving is `Friends::claim_username`.

use crate::api::Result;
use crate::username::{Availability, Mine};
use crate::username_rules::{normalize_username, username_problem};

pub const TITLE: &str = "Pick a username";
pub const PLACEHOLDER: &str = "3 to 20 letters, numbers, _ or .";
/// Toast when there is no account to name.
pub const NO_ACCOUNT: &str = "Add a Rare Wallet account first";
/// Toast when saving failed without a message.
pub const SAVE_FAILED: &str = "Could not save the username";

/// The line under the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickStatus {
    pub text: String,
    pub ok: bool,
}

/// The line for what is typed now. `check` asks rare.market (`FriendsApi::check_username` with the
/// account's address); it is only called when the rules pass and the name is not already yours.
pub fn status_for(raw: &str, current: Option<&str>, check: impl FnOnce(&str) -> Result<Availability>) -> Option<PickStatus> {
    let u = normalize_username(raw);
    if u.is_empty() {
        return None;
    }
    if let Some(problem) = username_problem(&u) {
        return Some(PickStatus { text: problem, ok: false });
    }
    if current == Some(u.as_str()) {
        return Some(PickStatus { text: "That is your username.".into(), ok: false });
    }
    Some(match check(&u) {
        Ok(a) if a.available => PickStatus { text: format!("{u} is free"), ok: true },
        Ok(a) => PickStatus { text: a.message.unwrap_or_else(|| "That name is not available.".into()), ok: false },
        Err(_) => PickStatus { text: "Could not check the name. Try again.".into(), ok: false },
    })
}

/// The toast once saved.
pub fn saved_toast(m: &Mine) -> String {
    format!("Your username is {}", m.username)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Error;

    fn free(u: &str) -> Result<Availability> {
        Ok(Availability { username: u.into(), available: true, reason: None, message: None, yours: None })
    }

    #[test]
    fn rules_first_then_the_server() {
        assert_eq!(status_for("  ", None, free), None);
        assert_eq!(status_for("ab", None, |_| panic!("no check")).unwrap().text, "Use 3 to 20 characters.");
        assert_eq!(status_for("@Mira", Some("mira"), |_| panic!("no check")).unwrap().text, "That is your username.");
        assert_eq!(status_for("Mira", None, free), Some(PickStatus { text: "mira is free".into(), ok: true }));
        let taken = |u: &str| Ok(Availability { username: u.into(), available: false, reason: None, message: None, yours: None });
        assert_eq!(status_for("mira", None, taken).unwrap().text, "That name is not available.");
        assert_eq!(status_for("mira", None, |_| Err(Error::Network("x".into()))).unwrap().text, "Could not check the name. Try again.");
    }
}
