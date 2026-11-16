//! "Hades II didn't start." (components/LaunchFail.tsx): one sentence, what happened and the next
//! step, Try again and Close. Try again starts it again the way the player did.

use crate::act::Act;
use rare_i18n::{tr, trf};
use crate::kit::{self, s, tok, Btn as B, Ty, Ui, T};
use crate::page::{hash, Acts, Page};
use crate::state::{Overlay, State};
use rare_render::view::El;

/// `detail`: for Epic and GOG games, why from the game's log and where the log is (read once per
/// sheet: the id it was read for, the reason, the log's path).
#[derive(Default)]
pub struct LaunchFail {
    detail: Option<(String, Option<String>, String)>,
}

/// An Epic or GOG game's start writes its log (rare-library umu `log_path`); other runs have none.
fn store_detail(id: &str) -> Option<(Option<String>, String)> {
    rare_stores::Store::of(id)?;
    let path = rare_library::umu::log_path(id);
    path.is_file().then(|| (rare_stores::launch_problem(id).map(problem_text), rare_library::umu::show_path(&path)))
}

/// rare-stores words a store game's start problem in English (`launch_problem`): the ones it
/// words itself in the UI language; a tool's own error line stays as it came.
fn problem_text(p: String) -> String {
    let t = &tr().game;
    match p.as_str() {
        "Epic can't be reached: no connection. Check Wi-Fi and try again." => t.store_fail_offline.into(),
        "Epic needs you to sign in again: Settings > Accounts." => t.store_fail_sign_in.into(),
        "legendary doesn't have it as installed. Check files in the game's Manage row, or install it again." => t.store_fail_not_installed.into(),
        "It needs an update first. Update it from its page." => t.store_fail_out_of_date.into(),
        _ => p,
    }
}

/// The sheet's second line (store.ts launchFailText).
pub fn fail_text(reason: &str, code: Option<i32>) -> String {
    let code = match code {
        Some(c) if c != -1 => trf!(tr().game.exit_code, code = c),
        _ => String::new(),
    };
    match reason {
        "steam" => tr().game.fail_steam.into(),
        "steamui" => tr().game.fail_steamui.into(),
        "never" => tr().game.fail_never.into(),
        "stuck" => tr().game.fail_stuck.into(),
        "quick" => trf!(tr().game.fail_quick, code = code),
        _ => trf!(tr().game.fail_closed, code = code),
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
            footer.push(kit::button(ui, B::new(tr().common.try_again, acts.tag(Act::Retry(id.clone()))).primary().def(true).hint(tr().common.try_again)));
        }
        footer.push(kit::button(ui, B::new(tr().common.close, acts.tag(Act::Close)).def(!retry).hint(tr().common.close)));
        let mut body = vec![ui.text_s(s().wp(1.0), fail_text(reason, *code), T::new(Ty::Body).c(tok::MUTED).wrap())];
        if self.detail.as_ref().is_none_or(|(for_id, _, _)| for_id != id) {
            self.detail = store_detail(id).map(|(why, path)| (id.clone(), why, path));
        }
        if let Some((_, why, path)) = self.detail.as_ref().filter(|(for_id, _, path)| for_id == id && !path.is_empty()) {
            if let Some(why) = why {
                body.push(ui.text_s(s().wp(1.0).mt(0.6), why.clone(), T::new(Ty::Body).wrap()));
            }
            body.push(ui.text_s(s().wp(1.0).mt(0.6), trf!(tr().game.fail_log, path = path), T::new(Ty::Small).c(tok::MUTED).wrap()));
        }
        kit::sheet(ui, &trf!(tr().game.fail_title, name = name), None, body, footer, false, 1)
    }
}
