//! Desktop notifications, for when the launcher is not the focused window. Whether to send one is
//! [`crate::logic::route`]'s `desktop`; sending it is the host's (`NotifyHost::send_desktop`), and a
//! desktop without a notification service just has the in-app toast. What is decided here is
//! whether the launcher counts as focused and what the notification says.

use crate::types::Notice;

/// The launcher is focused when it is shown and its window has focus. `window_focused` is the
/// window system's own answer when it has one; without it, `has_focus` (the toolkit's) decides.
pub fn launcher_focused(hidden: bool, window_focused: Option<bool>, has_focus: bool) -> bool {
    !hidden && window_focused.unwrap_or(has_focus)
}

/// A desktop notification's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopNote {
    pub title: String,
    pub body: Option<String>,
}

pub fn desktop_note(n: &Notice) -> DesktopNote {
    DesktopNote { title: n.title.clone(), body: n.body.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_is_never_focused() {
        assert!(!launcher_focused(true, Some(true), true));
        assert!(launcher_focused(false, None, true));
        assert!(!launcher_focused(false, Some(false), true));
    }
}
