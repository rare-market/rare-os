//! Quick settings' Now playing card: what any player on the device plays (Spotify, a Chrome tab,
//! Twitch), with its art and Previous, Play or Pause, Next. Shown only while something has a
//! title; the reading comes from rare_system::media, which keeps it fresh while the sheet is up.

use crate::act::Act;
use crate::kit::{self, s, tok, ArtFit, Track, Ty, Ui, T};
use crate::page::{Acts, Feedback, Fx};
use crate::state::{State, Tone};
use rare_render::focus::Group;
use rare_render::view::{key, El};
use rare_system::media::{self, Cmd, NowPlaying};

/// Act codes 700..709.
const PREV: u32 = 700;
const PLAY: u32 = 701;
const NEXT: u32 = 702;

pub fn owns(code: u32) -> bool {
    (700..710).contains(&code)
}

/// A square icon button in the card's row (Previous, Play or Pause, Next).
fn ibtn(ui: &mut Ui, id: &str, icon: &str, fill: bool, tag: u32, hint: &str, on: bool) -> El {
    let b = ui.bx(s().row().items_center().justify_center().h(2.6)).bg(tok::L3).radius(ui.r(0.25)).child(kit::focus_icon(ui, icon, 1.15, fill, tok::FG, tok::BG));
    let fl = b.look.clone().bg(tok::FG).scale(1.04);
    let el = kit::nav(b.focus_look(fl), tag, Some(hint), None, false).key(key(id));
    if on {
        el
    } else {
        el.opacity(0.4).nav(|f| f.disabled(true))
    }
}

/// The art (or a music note on layer 03 while it loads or when there is none).
pub fn art(ui: &mut Ui, n: &NowPlaying, size: f32) -> El {
    let fb = ui.bx(s().fill().row().items_center().justify_center()).child(kit::icon(ui, "music", size * 0.42, tok::MUTED));
    let srcs: Vec<String> = n.art.iter().cloned().collect();
    let img = kit::art(ui, s().fill(), &srcs, size, size, ArtFit::Cover(0.5), Some(fb));
    ui.bx(s().size(size)).bg(tok::L3).radius(ui.r(0.4)).clip().child(img)
}

/// The card, or None when nothing is playing or paused with a title.
pub fn view(ui: &mut Ui, acts: &mut Acts) -> Option<El> {
    let n = media::now()?;
    let names = ui
        .bx(s().col().grow().min0())
        .child(ui.text(n.title.clone(), T::new(Ty::Small).w(600).trunc()))
        .child(ui.text(n.sub(), T::new(Ty::Label).c(tok::MUTED).trunc()));
    let head = ui.bx(s().row().items_center().gap(0.8)).child(art(ui, &n, 3.0)).child(names);
    let btns = vec![
        ibtn(ui, "media-prev", "skip-back", true, acts.tag(Act::Local(PREV)), "Previous", n.can_previous),
        ibtn(ui, "media-play", if n.playing { "pause" } else { "play" }, true, acts.tag(Act::Local(PLAY)), if n.playing { "Pause" } else { "Play" }, n.can_play_pause),
        ibtn(ui, "media-next", "skip-forward", true, acts.tag(Act::Local(NEXT)), "Next", n.can_next),
    ];
    let row = ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5)).group(Group::new("qam-media", [0.0; 4])).kids(btns);
    Some(ui.bx(s().col().gap(0.7).px(0.95).py(0.8)).bg(tok::L2).radius(ui.r(0.5)).child(head).child(row).key(key("qam-media-card")))
}

/// Runs a control off the UI thread; a failure is a toast.
pub fn act(st: &mut State, fx: &mut Fx, code: u32) {
    let c = match code {
        PREV => Cmd::Previous,
        NEXT => Cmd::Next,
        _ => Cmd::PlayPause,
    };
    fx.fb(Feedback::Toggle);
    fx.spawn(move |_| {
        let r = media::command(c);
        Box::new(move |st: &mut State, now| {
            if let Err(e) = r {
                log::info!("media: {e}");
                st.toast("The player didn't respond.", Tone::Error, now);
            }
            st.ver.qs += 1;
        })
    });
    st.ver.qs += 1;
}
