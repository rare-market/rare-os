//! The Rare menu's Now playing card: what any player on the device plays (Spotify, a Chrome tab,
//! Twitch), with its art and Previous, Play or Pause, Next, so music keeps going under a game and
//! is one press away. Shown on the Game tab under Resume, and on the Rare tab over the launcher.

use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Fit, Id, ImgReq, Node};
use rare_system::media::{self, Cmd, NowPlaying};

/// The card's focus group (the Game tab uses 1..3).
pub const G_MEDIA: u32 = 4;

/// The control a focusable's sid stands for.
pub fn cmd_of(sid: &str) -> Option<Cmd> {
    match sid {
        "media-prev" => Some(Cmd::Previous),
        "media-play" => Some(Cmd::PlayPause),
        "media-next" => Some(Cmd::Next),
        _ => None,
    }
}

/// Sends a control to the player (off the UI thread; the bus answers in a few ms).
pub fn run(c: Cmd) {
    std::thread::spawn(move || {
        if let Err(e) = media::command(c) {
            log::info!("media: {e}");
        }
    });
}

fn ibtn(cx: &mut Cx, nav: &mut Nav, parent: Id, sid: &str, icon_name: &str, hint: &str, on: bool) {
    let r = cx.r();
    let b = cx.scene.add(parent, s().row().grow().items_center().justify_center().h(2.5 * r).done(), Node::bg(tok::L3).radius(0.25 * r));
    icon(cx, b, &format!("lucide-{icon_name}-f"), &crate::kit::icon::lucide_svg(icon_name, 1.75, true), 1.1, tok::FG);
    if on {
        nav.add(Focusable { id: b, kind: Kind::Tile { scale: 1.05 }, group: G_MEDIA, hint: hint.to_string(), tag: 0, sid: sid.to_string() });
    } else {
        cx.scene.n(b).opacity.snap(0.4);
    }
}

/// The card from the last reading. False (nothing added) when nothing is playing or paused.
pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id) -> bool {
    let Some(n) = media::now() else { return false };
    card(cx, nav, parent, &n);
    true
}

pub fn card(cx: &mut Cx, nav: &mut Nav, parent: Id, n: &NowPlaying) {
    let r = cx.r();
    let wrap = cx.scene.add(parent, s().col().px(0.65 * r).done(), Node::default());
    let c = cx.scene.add(wrap, s().col().gap(0.6 * r).pad(0.75 * r, 0.7 * r, 0.75 * r, 0.75 * r).done(), Node::bg(tok::L2).radius(0.5 * r));
    let head = cx.scene.add(c, s().row().items_center().gap(0.75 * r).done(), Node::default());
    let w = 2.75 * r;
    let a = cx.scene.add(head, s().wh(w, w).items_center().justify_center().no_shrink_().done(), Node::bg(tok::L3).radius(0.35 * r).clip());
    match n.art.as_deref().and_then(|u| crate::data::art::single(u, w)) {
        Some(src) => {
            let k = cx.img.request(ImgReq { src, w: w as u32, h: w as u32, fit: Fit::Cover { fy: 0.5 } });
            cx.scene.add(a, s().fill().done(), Node::default().image(k));
        }
        None => {
            icon(cx, a, "lucide-music", &crate::kit::icon::lucide_svg("music", 1.75, false), 1.2, tok::MUTED);
        }
    }
    let names = cx.scene.add(head, s().col().grow().shrink().done(), Node::default());
    text_tr(cx, names, ty::SMALL, 600, tok::FG, &n.title);
    let sub = if n.artist.is_empty() { n.app.clone() } else { format!("{} · {}", n.artist, n.app) };
    text_tr(cx, names, ty::LABEL, 400, tok::MUTED, &sub);
    let row = cx.scene.add(c, s().row().gap(0.5 * r).done(), Node::default());
    ibtn(cx, nav, row, "media-prev", "skip-back", "Previous", n.can_previous);
    ibtn(cx, nav, row, "media-play", if n.playing { "pause" } else { "play" }, if n.playing { "Pause" } else { "Play" }, n.can_play_pause);
    ibtn(cx, nav, row, "media-next", "skip-forward", "Next", n.can_next);
}
