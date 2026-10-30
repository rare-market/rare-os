//! PC games as covers (stream/tiles.tsx): the "On your PC" rail on Home and Library > Streaming.

use super::logic::*;
use super::state::StreamState;
use super::view::*;
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Color, Id, Node};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageKind {
    /// Home: the "On your PC" shelf.
    Home,
    /// Library > Streaming.
    Library,
    /// Both stacked (the preview).
    Both,
}

const TILE_W: f32 = 9.0;
const TILE_H: f32 = 13.5;

/// StreamTile: a 9rem cover, name and second line under it. `def` marks the default focus.
fn tile(cx: &mut Cx, layer: &mut Layer, parent: Id, st: &StreamState, host: &Host, app: &StreamApp, def: bool, group: u32, w: Option<f32>) {
    let r = cx.r();
    let state = play_state(host, app, &st.running);
    let cell = cx.scene.add(parent, s().col().w(w.unwrap_or(TILE_W * r)).done(), Node::default());
    let b = cx.scene.add(cell, s().wh(TILE_W * r, TILE_H * r).done(), Node::bg(tok::L3).radius(0.5 * r).clip());
    cover(cx, b, app.art.as_ref(), &app.name, TILE_W * r, TILE_H * r);
    if matches!(state, PlayState::Playing | PlayState::OnPc) {
        let d = 0.45 * r + 4.0;
        let ring = cx.scene.add(b, s().abs(Some(0.6 * r - 2.0), Some(0.6 * r - 2.0), None, None).wh(d, d).items_center().justify_center().done(), Node::bg(Color::BLACK.alpha(0.4)).radius(d / 2.0));
        dot(cx, ring, tok::FG);
    }
    let lines = cx.scene.add(cell, s().col().mt(0.85 * r).px(0.1 * r).done(), Node::default());
    let name = text_tr(cx, lines, ty::META, 500, tok::FG.alpha(0.85), &app.name);
    text_tr(cx, lines, ty::LABEL, 400, tok::MUTED, &tile_line(host, state));
    layer.names.push((b, name));
    let tag = layer.reg(Focusable { id: b, kind: Kind::Tile { scale: 1.06 }, group, hint: "Details".into(), tag: 0, sid: format!("{}:{}", host.uuid, app.id) }, Act::Tile(host.uuid.clone(), app.id));
    if def {
        layer.default = Some(tag);
    }
}

fn section_head(cx: &mut Cx, parent: Id, title: &str, note: impl FnOnce(&mut Cx, Id), right: Option<&str>, px: f32) {
    let r = cx.r();
    let row_ = cx.scene.add(parent, s().row().items_center().gap(0.75 * r).px(px).mb(0.9 * r).done(), Node::default());
    cx.scene.add_text(cx.t, row_, s().done(), title, ts(r, ty::ROW.0, ty::ROW.1, 600).tracking(-0.01), tok::FG);
    let n = cx.scene.add(row_, s().row().items_center().gap(0.45 * r).done(), Node::default());
    note(cx, n);
    if let Some(rt) = right {
        let rr = cx.scene.add(row_, s().row().items_center().gap(0.75 * r).ml_auto().done(), Node::default());
        text(cx, rr, ty::SMALL, 400, tok::MUTED, rt);
    }
}

/// Home: the games on the paired PC that answers, as soon as it does.
fn rail(cx: &mut Cx, layer: &mut Layer, parent: Id, st: &StreamState) -> bool {
    let Some(host) = home_host(&st.hosts).cloned() else { return false };
    let list = st.apps_of(&host.uuid);
    if list.is_empty() {
        return false;
    }
    let r = cx.r();
    let sec = cx.scene.add(parent, s().col().done(), Node::default());
    let name = host.name.clone();
    section_head(cx, sec, "On your PC", |cx, n| {
        text(cx, n, ty::SMALL, 400, tok::MUTED, &name);
    }, None, 2.5 * r);
    let row_ = cx.scene.add(sec, s().row().items_start().gap(1.0 * r).pad(2.5 * r, 1.4 * r, 2.5 * r, 1.4 * r).done(), Node::default().scrolls());
    neg_margin_y(cx, row_, 1.4 * r);
    for a in ordered(list).iter().take(16) {
        tile(cx, layer, row_, st, &host, a, false, 1, None);
    }
    layer.rails.push(row_);
    true
}

fn host_head(cx: &mut Cx, parent: Id, st: &StreamState, host: &Host) {
    let waking = st.waking.as_deref() == Some(host.uuid.as_str());
    let status = if host.online {
        if host.current_game.is_some() {
            "Playing on the PC"
        } else {
            "Online"
        }
    } else if waking {
        "Waking up"
    } else if host.can_wake {
        "Asleep"
    } else {
        "Off"
    };
    let online = host.online;
    section_head(cx, parent, &host.name, |cx, n| {
        dot(cx, n, if online { tok::FG } else { tok::MUTED });
        text(cx, n, ty::SMALL, 400, tok::MUTED, status);
    }, host.software.as_deref(), 0.0);
}

/// Library > Streaming: every paired PC with its games, or how to set one up.
fn grid(cx: &mut Cx, layer: &mut Layer, parent: Id, st: &StreamState) {
    let r = cx.r();
    let wrap = cx.scene.add(parent, s().col().gap(2.25 * r).px(2.5 * r).done(), Node::default());
    if !st.loaded {
        cx.scene.add(wrap, s().h(16.0 * r).done(), Node::default());
        return;
    }
    let paired: Vec<Host> = st.hosts.iter().filter(|h| h.paired).cloned().collect();
    if st.moonlight.is_none() || paired.is_empty() {
        let act = empty_card(cx, wrap, "Play the games on your PC", Some("Stream from a gaming PC running Sunshine or GeForce Experience, over your home Wi-Fi."));
        let b = btn(cx, act, "Set up streaming", Variant::Primary, "Set up streaming", false, Some(("monitor", MONITOR, 1.1)));
        reg_btn(layer, b, Act::OpenSetup, 2, true, false);
        return;
    }
    load_bar(cx, wrap, st.listing.is_some());
    let avail = cx.scene.w - 5.0 * r;
    let gap = 1.1 * r;
    let cols = ((avail + gap) / (TILE_W * r + gap)).floor().max(1.0);
    let cw = (avail - gap * (cols - 1.0)) / cols;
    let mut first = true;
    for (hi, host) in paired.iter().enumerate() {
        let list = ordered(st.apps_of(&host.uuid));
        let sec = cx.scene.add(wrap, s().col().done(), Node::default());
        host_head(cx, sec, st, host);
        let group = 2 + hi as u32;
        if list.is_empty() {
            if st.listing.as_deref() == Some(host.uuid.as_str()) {
                cx.scene.add(sec, s().h(13.5 * r).done(), Node::default());
            } else {
                let (title, body) = if host.online {
                    (format!("No games listed on {}", host.name), "Add games in Sunshine's Applications tab, then look again.")
                } else {
                    (format!("{} isn't answering", host.name), "Turn it on, or wake it from here if Wake-on-LAN is set up.")
                };
                let act = empty_card(cx, sec, &title, Some(body));
                if host.online {
                    let b = btn(cx, act, "Look again", Variant::Secondary, "", false, None);
                    reg_btn(layer, b, Act::ListAgain(host.uuid.clone()), group, first, false);
                    first = false;
                } else if host.can_wake {
                    let b = btn(cx, act, &format!("Wake {}", host.name), Variant::Primary, "", false, None);
                    reg_btn(layer, b, Act::Wake(host.uuid.clone()), group, first, false);
                    first = false;
                }
            }
        } else {
            let g = cx.scene.add(sec, s().row().wrap().gap_xy(gap, 1.4 * r).done(), Node::default());
            for a in &list {
                tile(cx, layer, g, st, host, a, first, group, Some(cw));
                first = false;
            }
        }
    }
}

pub fn build_page(cx: &mut Cx, st: &StreamState, kind: PageKind, parent: Id, now: f64) -> Layer {
    let r = cx.r();
    let root = cx.scene.add(parent, s().fill().col().done(), Node::default().scrolls());
    let mut layer = Layer::new(root, None);
    let body = cx.scene.add(root, s().col().gap(2.25 * r).pt(2.5 * r).pb(4.0 * r).done(), Node::default());
    if matches!(kind, PageKind::Home | PageKind::Both) {
        rail(cx, &mut layer, body, st);
    }
    if matches!(kind, PageKind::Library | PageKind::Both) {
        grid(cx, &mut layer, body, st);
    }
    layer.nav.scrollers.push(root);
    let _ = now;
    layer
}
