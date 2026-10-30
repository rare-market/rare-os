//! PC streaming sheets (stream/sheets.tsx): StreamSetup (get Moonlight, find PCs, add one, pair
//! with a big PIN) and StreamAppSheet (a PC game: Play, Resume, Quit, wake, stream quality).

use super::logic::*;
use super::state::{rare_way, StreamState};
use super::view::*;
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Id, Node};

fn sheet_layer(cx: &mut Cx, parent: Id, title: &str, sub: Option<&str>, now: f64) -> (Layer, Id, Id) {
    let p = sheet(cx, parent, title, sub, true, true, now);
    (Layer::new(p.card, Some(p.card)), p.body, p.footer.unwrap())
}

/// Moonlight from Flathub: Rare gets it the first time streaming opens, with progress here; the
/// command for Desktop mode only after a failed install.
fn get_moonlight(cx: &mut Cx, parent: Id, st: &StreamState, waiting: Option<&str>) {
    let Some(offer) = &st.offer else { return };
    let r = cx.r();
    let busy = st.installing.as_ref().is_some_and(|i| i.error.is_none());
    let failed = st.installing.as_ref().and_then(|i| i.error.as_ref());
    let col = cx.scene.add(parent, s().col().gap(0.6 * r).mb(1.4 * r).done(), Node::default());
    text(cx, col, ty::BODY, 600, tok::FG, if busy { "Getting Moonlight" } else { "Moonlight" });
    let why = match waiting {
        Some(name) if busy => format!("Rare streams with Moonlight, a free open-source app. It takes a minute; pairing with {name} starts once it's in."),
        _ if busy => "Rare streams with Moonlight, a free open-source app. It takes a minute, and you can look for your PC meanwhile.".to_string(),
        _ => "Rare streams with Moonlight, a free open-source app. It installs from Flathub for you only, no password needed.".to_string(),
    };
    wrap_text(cx, col, ty::SMALL, 400, tok::MUTED, &why);
    // The bar's row keeps its place before, during and after, so nothing below moves.
    let row_ = cx.scene.add(col, s().row().items_center().gap(0.9 * r).h(1.25 * r).done(), Node::default());
    if busy {
        let p = st.installing.as_ref().unwrap().progress;
        install_bar(cx, row_, p);
        let pct = cx.scene.add(row_, s().w(3.0 * r).row().justify_end().done(), Node::default());
        text(cx, pct, ty::SMALL, 400, tok::FG, &p.map(|p| format!("{}%", (p * 100.0).round() as u32)).unwrap_or_default());
    }
    if let Some(e) = failed {
        wrap_text(cx, col, ty::SMALL, 400, tok::NEG, e);
        text(cx, col, ty::LABEL, 400, tok::MUTED, "Or in Desktop mode, in a terminal:");
        let code = cx.scene.add(col, s().col().px(0.9 * r).py(0.6 * r).done(), Node::bg(tok::L2).radius(0.5 * r));
        let ts_ = ts(r, ty::META.0, ty::META.1, 400).mono().wrap();
        cx.scene.add_text(cx.t, code, s().done(), &offer.command, ts_, tok::FG);
    }
}

fn host_row(cx: &mut Cx, layer: &mut Layer, parent: Id, st: &StreamState, host: &Host, def: bool) {
    let r = cx.r();
    let id = cx.scene.add(parent, s().row().items_center().gap(0.9 * r).h(3.75 * r).wp(1.0).px(0.75 * r).done(), Node::default().radius(0.5 * r));
    let circle = cx.scene.add(id, s().wh(2.25 * r, 2.25 * r).items_center().justify_center().done(), Node::bg(tok::L3).radius(1.125 * r));
    let k = cx.img.request(svg_req("monitor", MONITOR, 1.1 * r));
    cx.scene.add(circle, s().wh(1.1 * r, 1.1 * r).done(), Node::default().icon(k, tok::MUTED));
    let col = cx.scene.add(id, s().col().grow().done(), Node::default());
    text_tr(cx, col, ty::BODY, 500, tok::FG, &host.name);
    let sub: Vec<String> = [host.address.clone(), host.software.clone()].into_iter().flatten().collect();
    mono_text(cx, col, ty::META, 400, tok::MUTED, &sub.join(" \u{b7} "), true);
    let right = cx.scene.add(id, s().row().items_center().gap(0.45 * r).no_shrink_().done(), Node::default());
    dot(cx, right, if host.online { tok::FG } else { tok::MUTED });
    let state = if host.paired {
        "Paired"
    } else if host.online {
        "Not paired"
    } else {
        "Not answering"
    };
    text(cx, right, ty::SMALL, 400, tok::MUTED, state);
    let hint = if host.paired { "Show games" } else { "Pair" };
    let t = layer.reg(Focusable { id, kind: Kind::Row, group: 10, hint: hint.into(), tag: 0, sid: host.uuid.clone() }, Act::HostRow(host.uuid.clone()));
    if def {
        layer.default = Some(t);
    }
    let _ = st;
}

/// Find PCs and pair.
pub fn build_setup(cx: &mut Cx, st: &StreamState, parent: Id, now: f64) -> Layer {
    if st.pairing.is_some() {
        return build_pair(cx, st, parent, now);
    }
    let r = cx.r();
    let (mut layer, body, footer) = sheet_layer(cx, parent, "Stream from your PC", None, now);
    let busy = st.installing.as_ref().is_some_and(|i| i.error.is_none());
    let waiting = st.pending_pair.as_deref().and_then(|u| st.host(u)).map(|h| h.name.clone());
    get_moonlight(cx, body, st, waiting.as_deref());
    let list = cx.scene.add(body, s().col().done(), Node::default());
    let head = cx.scene.add(list, s().row().items_center().gap(0.75 * r).mb(0.5 * r).done(), Node::default());
    text(cx, head, ty::BODY, 600, tok::FG, "PCs on your network");
    text(cx, head, ty::SMALL, 400, tok::MUTED, if st.scanning { "Looking" } else { "" });
    load_bar(cx, list, st.scanning);
    let hosts = cx.scene.add(list, s().col().done(), Node::default());
    let unpaired = st.hosts.iter().position(|h| !h.paired && h.online);
    for (i, h) in st.hosts.iter().enumerate() {
        let def = st.offer.is_none() && unpaired.map(|u| i == u).unwrap_or(i == 0);
        host_row(cx, &mut layer, hosts, st, h, def);
    }
    if !st.scanning && st.hosts.is_empty() {
        let p = cx.scene.add(list, s().col().py(0.9 * r).done(), Node::default());
        wrap_text(cx, p, ty::SMALL, 400, tok::MUTED, "No PCs found yet.");
    }
    let note = cx.scene.add(body, s().col().mt(1.1 * r).done(), Node::default());
    rich(
        cx,
        note,
        ty::SMALL,
        tok::MUTED,
        &[
            ("Your PC needs Sunshine (free, for any graphics card) or GeForce Experience, on the same network. Install Sunshine from ", false),
            ("github.com/LizardByte/Sunshine", true),
            (", start it, then look again.", false),
        ],
    );
    // Footer.
    if st.offer.is_some() && !busy {
        // Only after a failed install: Rare starts it on its own when streaming opens.
        let b = btn(cx, footer, "Try again", Variant::Primary, "Get Moonlight", false, None);
        reg_btn(&mut layer, b, Act::GetMoonlight, 11, true, false);
    }
    let b = btn(cx, footer, "Add by address", Variant::Secondary, "Add by address", false, Some(("plus", PLUS, 1.0)));
    reg_btn(&mut layer, b, Act::AddAddress, 11, false, false);
    let b = btn(cx, footer, "Look again", Variant::Secondary, "Look again", st.scanning, None);
    reg_btn(&mut layer, b, Act::LookAgain, 11, false, st.scanning);
    layer
}

/// The PIN, big, and where to type it on the PC.
fn build_pair(cx: &mut Cx, st: &StreamState, parent: Id, now: f64) -> Layer {
    let r = cx.r();
    let p = st.pairing.clone().unwrap();
    let title = format!("Pair with {}", p.name);
    let (mut layer, body, footer) = sheet_layer(cx, parent, &title, Some("Pairing happens once. After that your PC's games show up in your Library."), now);
    let gfe = p.software == "GeForce Experience";
    let top = cx.scene.add(body, s().row().items_center().gap(2.5 * r).done(), Node::default());
    let pin = cx.scene.add(top, s().row().gap(0.6 * r).no_shrink_().done(), Node::default());
    for d in p.pin.chars() {
        let b = cx.scene.add(pin, s().wh(4.25 * r, 5.5 * r).items_center().justify_center().done(), Node::bg(tok::L2).radius(0.5 * r));
        cx.scene.add_text(cx.t, b, s().done(), &d.to_string(), ts(r, 2.75, 3.25, 600).tabular(), tok::FG);
    }
    let ol = cx.scene.add(top, s().col().grow().gap(0.7 * r).done(), Node::default());
    let steps: Vec<Vec<(String, bool)>> = if gfe {
        vec![
            vec![(format!("GeForce Experience shows a box on {}.", p.name), false)],
            vec![("Type this PIN in it.".into(), false)],
            vec![("Rare finishes on its own.".into(), false)],
        ]
    } else {
        vec![
            vec![(format!("On {}, open Sunshine at ", p.name), false), ("https://localhost:47990".into(), true)],
            vec![("Go to PIN. Enter this PIN and a name for this SuiPlay.".into(), false)],
            vec![("Select Send. Rare finishes on its own.".into(), false)],
        ]
    };
    for (i, segs) in steps.iter().enumerate() {
        let li = cx.scene.add(ol, s().row().gap(0.75 * r).done(), Node::default());
        let n = cx.scene.add(li, s().w(1.0 * r).no_shrink_().done(), Node::default());
        cx.scene.add_text(cx.t, n, s().done(), &(i + 1).to_string(), ts(r, ty::BODY.0, ty::BODY.1, 400).tabular(), tok::MUTED);
        let txt = cx.scene.add(li, s().col().grow().done(), Node::default());
        let segs: Vec<(&str, bool)> = segs.iter().map(|(t, c)| (t.as_str(), *c)).collect();
        rich(cx, txt, ty::BODY, tok::FG, &segs);
    }
    let bottom = cx.scene.add(body, s().col().mt(1.6 * r).pt(1.0 * r).done(), Node::default());
    edge(cx, bottom, 0, tok::HAIR);
    load_bar(cx, bottom, p.phase == PairPhase::Waiting);
    let row_ = cx.scene.add(bottom, s().row().items_center().gap(0.5 * r).done(), Node::default());
    let (tone, msg, col) = match p.phase {
        PairPhase::Paired => (Tone::Primary, format!("Paired with {}", p.name), tok::FG.alpha(0.8)),
        PairPhase::Failed => (Tone::Negative, p.error.clone().unwrap_or_default(), tok::NEG),
        PairPhase::Waiting => (Tone::Muted, format!("Waiting for {}", p.name), tok::FG.alpha(0.8)),
    };
    status_dot(cx, row_, tone);
    text(cx, row_, ty::SMALL, 400, col, &msg);
    match p.phase {
        PairPhase::Paired => {
            let b = btn(cx, footer, "Show games", Variant::Primary, "Show games", false, None);
            reg_btn(&mut layer, b, Act::ShowGames, 11, true, false);
        }
        PairPhase::Failed => {
            let b = btn(cx, footer, "Try again", Variant::Primary, "Try again", false, None);
            reg_btn(&mut layer, b, Act::TryAgain(p.uuid.clone()), 11, true, false);
            let b = btn(cx, footer, "Back", Variant::Secondary, "", false, None);
            reg_btn(&mut layer, b, Act::BackFromPair, 11, false, false);
        }
        PairPhase::Waiting => {
            let b = btn(cx, footer, "Cancel", Variant::Secondary, "Cancel", false, None);
            reg_btn(&mut layer, b, Act::CancelPair, 11, true, false);
        }
    }
    layer
}

/// A focusable setting row: A moves to the next value.
fn choice(cx: &mut Cx, layer: &mut Layer, parent: Id, label: &str, items: &[String], on: usize, act: Act, def: bool) {
    let r = cx.r();
    let id = cx.scene.add(parent, s().row().items_center().gap(1.0 * r).h(3.25 * r).wp(1.0).px(0.75 * r).done(), Node::default().radius(0.5 * r));
    let l = cx.scene.add(id, s().grow().done(), Node::default());
    text(cx, l, ty::BODY, 400, tok::FG, label);
    chips(cx, id, items, on);
    let t = layer.reg(Focusable { id, kind: Kind::Row, group: 12, hint: "Change".into(), tag: 0, sid: label.to_lowercase() }, act);
    if def {
        layer.default = Some(t);
    }
}

/// What the sheet's status line says for each state.
pub fn app_line(host: &Host, state: PlayState, waking: bool, other: &str) -> String {
    match state {
        PlayState::Playing => "Streaming to this SuiPlay".into(),
        PlayState::OnPc => format!("Running on {}. Resume picks it up where you left it.", host.name),
        PlayState::Busy => format!("{other} is running on {}. Play quits it first.", host.name),
        PlayState::Asleep => {
            if waking {
                format!("Waking {}", host.name)
            } else {
                format!("{} is asleep. Play wakes it first.", host.name)
            }
        }
        PlayState::Offline => format!("{} isn't answering. Turn it on, then look again.", host.name),
        PlayState::Ready => format!("Ready on {}", host.name),
    }
}

/// A PC game's page. None when the PC or the game isn't known.
pub fn build_app(cx: &mut Cx, st: &StreamState, uuid: &str, app_id: u32, parent: Id, now: f64) -> Option<Layer> {
    let host = st.host(uuid)?.clone();
    let app = st.apps_of(uuid).iter().find(|a| a.id == app_id)?.clone();
    let r = cx.r();
    let state = play_state(&host, &app, &st.running);
    let other = host.current_game.map(|g| st.apps_of(uuid).iter().find(|a| a.id == g).map(|a| a.name.clone()).unwrap_or_else(|| "Another game".into())).unwrap_or_default();
    let waking = st.waking.as_deref() == Some(uuid);
    let sub = format!("On {}{}", host.name, host.software.as_ref().map(|s| format!(" \u{b7} {s}")).unwrap_or_default());
    let (mut layer, body, footer) = sheet_layer(cx, parent, &app.name, Some(&sub), now);
    let top = cx.scene.add(body, s().row().gap(1.75 * r).done(), Node::default());
    let c = cx.scene.add(top, s().wh(9.0 * r, 13.5 * r).no_shrink_().done(), Node::bg(tok::L3).radius(0.5 * r).clip());
    cover(cx, c, app.art.as_ref(), &app.name, 9.0 * r, 13.5 * r);
    let col = cx.scene.add(top, s().col().grow().done(), Node::default());
    let line = cx.scene.add(col, s().row().items_center().gap(0.5 * r).done(), Node::default());
    dot(cx, line, if matches!(state, PlayState::Playing | PlayState::OnPc) { tok::FG } else { tok::MUTED });
    text(cx, line, ty::SMALL, 400, tok::FG.alpha(0.8), &app_line(&host, state, waking, &other));
    let q = cx.scene.add(col, s().col().mt(1.2 * r).mb(0.35 * r).done(), Node::default());
    text(cx, q, ty::BODY, 600, tok::FG, "Stream quality");
    let set = st.settings.clone();
    let auto = auto_codec(st.decode, host.codecs);
    let list = cx.scene.add(col, s().col().done(), Node::default());
    let res: Vec<String> = RESOLUTIONS.iter().map(|r| r.1.to_string()).collect();
    choice(cx, &mut layer, list, "Resolution", &res, RESOLUTIONS.iter().position(|r| r.0 == set.resolution).unwrap_or(0), Act::ChooseResolution, false);
    let br: Vec<String> = BITRATES.iter().map(|m| format!("{m} Mbps")).collect();
    choice(cx, &mut layer, list, "Bitrate", &br, BITRATES.iter().position(|m| *m == set.bitrate_mbps).unwrap_or(0), Act::ChooseBitrate, false);
    let codecs: Vec<String> = CODECS.iter().map(|c| if *c == Codec::Auto && auto.is_some() { format!("Auto ({})", codec_label(auto.unwrap())) } else { codec_label(*c).to_string() }).collect();
    choice(cx, &mut layer, list, "Video", &codecs, CODECS.iter().position(|c| *c == set.codec).unwrap_or(0), Act::ChooseCodec, false);
    let note = cx.scene.add(col, s().col().mt(0.8 * r).done(), Node::default());
    wrap_text(
        cx,
        note,
        ty::SMALL,
        400,
        tok::MUTED,
        &format!("For every PC game, at 60 fps. Lower the bitrate if the picture stutters on busy Wi-Fi. While you play, Moonlight reads your controller. {} to come back to Rare and resume or quit.", rare_way(st.guest)),
    );
    // Footer, per state.
    let add = |cx: &mut Cx, layer: &mut Layer, label: &str, v: Variant, hint: &str, act: Act, def: bool, disabled: bool| {
        let b = btn(cx, footer, label, v, hint, disabled, None);
        reg_btn(layer, b, act, 11, def, disabled);
    };
    if let Some(b) = st.busy.clone() {
        add(cx, &mut layer, &b, Variant::Primary, &b, Act::Play, true, true);
    } else if st.moonlight.is_none() {
        if st.installing.as_ref().is_some_and(|i| i.error.is_none()) {
            add(cx, &mut layer, "Getting Moonlight", Variant::Primary, "Getting Moonlight", Act::Play, true, true);
        } else {
            add(cx, &mut layer, "Set up streaming", Variant::Primary, "Set up streaming", Act::SetupFromApp, true, false);
        }
    } else {
        match state {
            PlayState::Playing | PlayState::OnPc => {
                add(cx, &mut layer, "Resume", Variant::Primary, "Resume", Act::Resume, true, false);
                add(cx, &mut layer, "Quit game", Variant::Secondary, "Quit game", Act::QuitGame, false, false);
            }
            PlayState::Busy => {
                let l = format!("Quit {other} and play");
                add(cx, &mut layer, &l, Variant::Primary, &l, Act::QuitAndPlay, true, false);
            }
            PlayState::Asleep => {
                let l = if waking { format!("Waking {}", host.name) } else { "Wake and play".to_string() };
                add(cx, &mut layer, &l, Variant::Primary, "Wake and play", Act::WakeAndPlay, true, waking);
            }
            PlayState::Offline => add(cx, &mut layer, "Look again", Variant::Secondary, "Look again", Act::LookAgainApp, true, false),
            PlayState::Ready => add(cx, &mut layer, "Play", Variant::Primary, "Play", Act::Play, true, false),
        }
    }
    // The quality rows sit above the footer in focus order only by position; default is the footer.
    Some(layer)
}
