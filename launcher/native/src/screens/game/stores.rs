//! An Epic or GOG game's page parts (features/stores.rs): Install, Pause, Resume and Cancel while
//! it downloads, Update and Sync saves once installed, and the progress line under the
//! buttons (its space is always kept, so nothing moves when a download starts or ends). Check
//! files for them sits in the page's Manage row (manage.rs) and runs the store's own repair.

use crate::act::Act;
use crate::data::model::Game;
use crate::features::stores;
use crate::kit::{self, s, tok, Btn as B, BtnIcon, Ty, Ui, T};
use crate::page::Acts;
use rare_render::view::El;
use rare_stores::jobs::Phase;

/// An Epic or GOG game Rare manages (not one Heroic installed and keeps).
pub fn managed(g: &Game) -> bool {
    stores::is_store(&g.id) && !g.origin.contains("Heroic")
}

fn act(verb: &str, id: &str) -> Act {
    Act::Legacy(format!("stores:{verb}:{id}"))
}

/// The buttons while a job runs, is queued, paused or failed; None when there is none.
fn job_buttons(ui: &mut Ui, acts: &mut Acts, g: &Game, primary: bool) -> Option<Vec<El>> {
    let j = stores::job(&g.id)?;
    if j.setup {
        // The runtimes going into the prefix can't be paused; Play waits for them anyway.
        if !primary {
            return Some(vec![]);
        }
        return Some(vec![kit::button(ui, B::new("Setting up", acts.game(Act::None, &g.id)).primary().lg().def(true).min_w(10.0).game())]);
    }
    let (label, verb, icon) = match j.phase {
        Phase::Queued | Phase::Running => ("Pause", "pause", Some("pause")),
        Phase::Paused => ("Resume", "resume", Some("play")),
        Phase::Failed(_) => ("Try again", "resume", Some("refresh-cw")),
    };
    let mut main = B::new(label, acts.game(act(verb, &g.id), &g.id)).lg().min_w(10.0).game();
    if let Some(i) = icon {
        main = main.icon(BtnIcon::Lucide(i, 1.1, i != "refresh-cw"));
    }
    if primary {
        main = main.primary().def(true);
    } else {
        main = main.ghost();
    }
    Some(vec![kit::button(ui, main), kit::button(ui, B::new("Cancel", acts.game(act("cancel", &g.id), &g.id)).lg().ghost().game())])
}

/// A game that is not installed: Install (or the download's own buttons). Replaces Play.
pub fn not_installed(ui: &mut Ui, acts: &mut Acts, g: &Game) -> Vec<El> {
    // Its size is asked in the background for the Size card and the drive sheet.
    let _ = stores::size(&g.id);
    if let Some(b) = job_buttons(ui, acts, g, true) {
        return b;
    }
    vec![kit::button(ui, B::new("Install", acts.game(act("install", &g.id), &g.id)).primary().lg().def(true).hint("Install").icon(BtnIcon::Lucide("download", 1.1, false)).min_w(10.0).game())]
}

/// After Play on an installed game: Update, Sync saves, or the running job's buttons.
pub fn installed(ui: &mut Ui, acts: &mut Acts, g: &Game, running: bool) -> Vec<El> {
    if let Some(b) = job_buttons(ui, acts, g, false) {
        return b;
    }
    let mut v = Vec::new();
    if running {
        return v;
    }
    if stores::has_update(&g.id) {
        v.push(kit::button(ui, B::new("Update", acts.game(act("update", &g.id), &g.id)).lg().ghost().icon(BtnIcon::Lucide("download", 1.1, false)).game()));
    }
    if stores::cloud(&g.id) {
        v.push(kit::button(ui, B::new("Sync saves", acts.game(act("sync", &g.id), &g.id)).lg().ghost().icon(BtnIcon::Lucide("cloud", 1.1, false)).game()));
    }
    v
}

/// The progress line and bar of the game's download, in a space that is always there.
pub fn progress(ui: &mut Ui, g: &Game) -> El {
    let mut col = ui.bx(s().col().gap(0.5).h(2.1).max_w(36.0).wp(0.5));
    let Some(j) = stores::job(&g.id) else { return col };
    let failed = matches!(j.phase, Phase::Failed(_));
    col = col.child(ui.text(stores::job_line(&j), T::new(Ty::Small).w(500).c(if failed { tok::NEG } else { tok::fg(0.8) }).trunc()));
    if !failed {
        col = col.child(crate::features::downloads::bar(ui, None, j.pct / 100.0, j.phase != Phase::Running));
    }
    col
}

/// The Size card: the install size before it is installed.
pub fn size_text(g: &Game) -> Option<String> {
    if g.installed || !managed(g) {
        return None;
    }
    Some(stores::size(&g.id).map(|s| rare_stores::fmt_bytes(s.1)).unwrap_or_else(|| "Checking".into()))
}
