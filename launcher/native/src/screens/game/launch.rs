//! "Hades II didn't start." (components/LaunchFail.tsx): one sentence, what happened and the next
//! step, Try again and Close. Try again starts it again the way the player did.

use crate::act::Act;
use crate::kit::{self, s, tok, Btn as B, Ty, Ui, T};
use crate::page::{hash, Acts, Page};
use crate::state::{Overlay, State};
use rare_render::view::El;

pub struct LaunchFail;

/// The sheet's second line (store.ts launchFailText).
pub fn fail_text(reason: &str, code: Option<i32>) -> String {
    let code = match code {
        Some(c) if c != -1 => format!(" (exit code {c})"),
        _ => String::new(),
    };
    match reason {
        "steam" => "Steam didn't open it. Open Steam to see why: an update, a sign-in or an error.".into(),
        "steamui" => "Steam didn't open. Try again, or restart the device if it keeps happening.".into(),
        "never" => "Heroic didn't start it. Open Heroic in Desktop mode to see why.".into(),
        "stuck" => "It's still running but nothing showed up in 45 seconds. Try again closes it and starts it again.".into(),
        "quick" => format!("It closed before showing anything{code}. Try again, or check it in Desktop mode."),
        _ => format!("It closed unexpectedly{code}."),
    }
}

impl Page for LaunchFail {
    fn stamp(&self, st: &State) -> u64 {
        hash((format!("{:?}", st.overlay), st.ver.library))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let Some(Overlay::LaunchFail { id, reason, code }) = &st.overlay else { return ui.bx(s()) };
        let name = st.run_name(id);
        // Apps start from the Apps screen (not wired natively yet), so Try again is for games.
        let retry = st.game(id).is_some() || id.starts_with("app:");
        let mut footer = Vec::new();
        if retry {
            footer.push(kit::button(ui, B::new("Try again", acts.tag(Act::Retry(id.clone()))).primary().def(true).hint("Try again")));
        }
        footer.push(kit::button(ui, B::new("Close", acts.tag(Act::Close)).def(!retry).hint("Close")));
        let body = ui.text_s(s().wp(1.0), fail_text(reason, *code), T::new(Ty::Body).c(tok::MUTED).wrap());
        kit::sheet(ui, &format!("{name} didn't start."), None, vec![body], footer, false, 1)
    }
}
