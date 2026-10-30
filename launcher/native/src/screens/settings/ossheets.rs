//! The two confirmations for OS changes, controller only (screens/settings/OsSheets.tsx): Restart
//! to update, and Restore previous system. A confirms (the focused button), B cancels.

use super::update::OsUpdate;
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::input::Pad;
use rare_render::theme::tok;
use rare_render::{Id, Node};

pub struct OsSheet {
    nav: Nav,
    kind: Kind2,
    yes_act: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind2 {
    Restart { staged: bool },
    Restore,
    /// A question with one yes: A runs the act.
    Confirm,
}

fn para(cx: &mut Cx, parent: Id, t: &str, small: bool) {
    let r = cx.r();
    let (sz, lh, c) = if small { (ty::SMALL.0, ty::SMALL.1, tok::MUTED) } else { (ty::BODY.0, ty::BODY.1, tok::FG.alpha(0.8)) };
    cx.scene.add_text(cx.t, parent, s().done(), t, ts(r, sz, lh, 400).wrap(), c);
}

impl OsSheet {
    /// "Restart to update". `running`: a game is running.
    pub fn restart(cx: &mut Cx, update: &OsUpdate, running: bool, busy: bool, now: f64) -> OsSheet {
        let r = cx.r();
        let root = cx.scene.root;
        let (title, sub) = if update.staged { ("Restart to update", update.staged_version.as_ref().map(|v| format!("Version {v}"))) } else { ("Nothing to install", Some("Rare OS is up to date.".to_string())) };
        let sh = sheet(cx, root, title, sub.as_deref(), true, false, now);
        let mut nav = Nav::default();
        let f = sh.footer.unwrap();
        if update.staged {
            let col = cx.scene.add(sh.body, s().col().gap(0.6 * r).done(), Node::default());
            para(cx, col, "The update is downloaded and installs while the device restarts. It takes a minute or two. Your games and files stay as they are.", false);
            if let Some(notes) = &update.notes {
                para(cx, col, &format!("What's new\n{notes}"), false);
            }
            para(cx, col, if running { "A game is running. Close it first so nothing is lost." } else { "Later keeps the update waiting. It installs the next time the device restarts, and the launcher reminds you again next time it opens." }, false);
            para(cx, col, "Hold the power button to switch off only if you have to. A forced power-off can lose a waiting update, though it downloads again by itself.", true);
            nav.add(button(cx, f, if busy { "Restarting" } else { "Restart now" }, if busy { Variant::Dimmed } else { Variant::Primary }, Sz::Md, false).hint("Restart now").tag(1).group(1));
            nav.add(button(cx, f, "Later", Variant::Secondary, Sz::Md, false).hint("Later").tag(2).group(1));
        } else {
            nav.add(button(cx, f, "Close", Variant::Secondary, Sz::Md, false).hint("Close").tag(2).group(1));
        }
        let first = nav.items[0].id;
        nav.focus(cx.scene, first, now, false);
        OsSheet { nav, kind: Kind2::Restart { staged: update.staged }, yes_act: String::new() }
    }

    /// A plain question (remove an SSH key, add keys from GitHub): `body` lines split on \n.
    pub fn confirm(cx: &mut Cx, title: &str, sub: &str, body: &str, yes: &str, act: &str, now: f64) -> OsSheet {
        OsSheet::confirm_with(cx, title, sub, body, yes, "Cancel", act, now)
    }

    /// `confirm` with its own words on the button that leaves things as they are.
    #[allow(clippy::too_many_arguments)]
    pub fn confirm_with(cx: &mut Cx, title: &str, sub: &str, body: &str, yes: &str, no: &str, act: &str, now: f64) -> OsSheet {
        let r = cx.r();
        let root = cx.scene.root;
        let sh = sheet(cx, root, title, if sub.is_empty() { None } else { Some(sub) }, true, false, now);
        let col = cx.scene.add(sh.body, s().col().gap(0.6 * r).done(), Node::default());
        for (i, line) in body.split('\n').enumerate() {
            para(cx, col, line, i > 0);
        }
        let f = sh.footer.unwrap();
        let mut nav = Nav::default();
        nav.add(button(cx, f, yes, Variant::Primary, Sz::Md, false).hint(yes).tag(1).group(1));
        nav.add(button(cx, f, no, Variant::Secondary, Sz::Md, false).hint(no).tag(2).group(1));
        let first = nav.items[1].id;
        nav.focus(cx.scene, first, now, false);
        OsSheet { nav, kind: Kind2::Confirm, yes_act: act.to_string() }
    }

    /// `confirm` with the yes button focused: an offer, not a warning (sign in to a Wi-Fi network).
    pub fn offer(cx: &mut Cx, title: &str, sub: &str, body: &str, yes: &str, act: &str, now: f64) -> OsSheet {
        let mut sh = OsSheet::confirm(cx, title, sub, body, yes, act, now);
        let first = sh.nav.items[0].id;
        sh.nav.focus(cx.scene, first, now, false);
        sh
    }

    pub fn restore(cx: &mut Cx, busy: bool, now: f64) -> OsSheet {
        let r = cx.r();
        let root = cx.scene.root;
        let sh = sheet(cx, root, "Restore previous system", Some("Go back to the system this device ran before Rare OS."), true, false, now);
        let col = cx.scene.add(sh.body, s().col().gap(0.6 * r).done(), Node::default());
        para(cx, col, "The next start is the previous system. Rare OS stays installed, and your files and games are shared by both.", false);
        para(cx, col, "To come back to Rare OS, pick it in the boot menu.", false);
        let f = sh.footer.unwrap();
        let mut nav = Nav::default();
        let v = if busy { Variant::Dimmed } else { Variant::Primary };
        nav.add(button(cx, f, "Restore and restart", v, Sz::Md, false).hint("Restore and restart").tag(1).group(1));
        nav.add(button(cx, f, "Restore, restart later", if busy { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).hint("Restore, restart later").tag(3).group(1));
        nav.add(button(cx, f, "Cancel", Variant::Secondary, Sz::Md, false).hint("Cancel").tag(2).group(1));
        let first = nav.items[0].id;
        nav.focus(cx.scene, first, now, false);
        OsSheet { nav, kind: Kind2::Restore, yes_act: String::new() }
    }
}

impl Screen for OsSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            Pad::A => match (self.kind, self.nav.current().map(|f| f.tag)) {
                (_, Some(2)) => Out::Close,
                (Kind2::Restart { .. }, Some(1)) => Out::Act("power:reboot".into()),
                (Kind2::Restore, Some(1)) => Out::Act("os:restore:reboot".into()),
                (Kind2::Restore, Some(3)) => Out::Act("os:restore:later".into()),
                (Kind2::Confirm, Some(1)) => Out::Act(self.yes_act.clone()),
                _ => Out::None,
            },
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            _ => Out::None,
        }
    }
}

pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let staged = OsUpdate { staged: true, staged_version: Some("43.2".into()), booted_version: Some("43.1".into()), ..Default::default() };
    Some(match name {
        "settings-restart" => Box::new(OsSheet::restart(cx, &staged, false, false, now)),
        "settings-restart-running" => Box::new(OsSheet::restart(cx, &staged, true, false, now)),
        "settings-restart-notes" => {
            let u = OsUpdate { notes: Some("Updates download in the background on battery too.\nIf a new version does not start, the device goes back to the previous one by itself.\nSettings > System can go back to the previous version.".into()), ..staged.clone() };
            Box::new(OsSheet::restart(cx, &u, true, false, now))
        }
        "settings-restart-none" => Box::new(OsSheet::restart(cx, &OsUpdate::default(), false, false, now)),
        "settings-restore" => Box::new(OsSheet::restore(cx, false, now)),
        _ => return None,
    })
}

#[allow(dead_code)]
fn _keep(_: Id) {}
