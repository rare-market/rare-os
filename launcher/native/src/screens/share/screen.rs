//! share/ShareScreen.tsx: the captures grid and a capture's page (the picture or the playing
//! clip, keep-it-for, the price, Share and Delete, the upload's steps, the link with its QR).
//! A full-screen layer (`fixed inset-0 z-30 bg-bg`) over the shell, so it mounts on the scene
//! root, not the main area; the shell's hint bar stays on top.
//!
//! Pads: directions move focus, A activates, B goes back to the grid (not while uploading) or
//! closes Share, X switches how long to keep it while the page can still be changed.

use super::format::{ago, fmt_bytes, fmt_duration, fmt_sui_mist, fmt_wal, kind_word, telegram_poster, DurationId, DURATIONS};
use super::qr;
use super::store::{clips_note, Payer, Phase, ShareState, Taking, Thumb, STEPS};
use super::video::{self, Player};
use crate::kit::controls::{LOAD_SX, LOAD_X};
use crate::kit::{Btn, DotTone};
use crate::screens::market::host::*;
use rare_render::anim::Keys;
use rare_render::chart::Textures;
use rare_render::gpu::KIND_IMAGE;
use rare_render::input::Pad;
use rare_render::scene::{layer, GradDir, Gradient};
use rare_render::taffy::prelude::length;
use rare_render::view::{key, El, Look};
use rare_render::{Color, DrawList, Fit, Id, ImgReq, ImgState, Prim, Scene, Src, TexId, TextSys};
use rare_share::{Capture, Kind};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

// Focus tags.
pub const T_SHOT: u32 = 10;
pub const T_CLIP: u32 = 11;
pub const T_CLIPS: u32 = 12;
pub const T_DUR: u32 = 100;
pub const T_PAYER: u32 = 110;
pub const T_SHARE: u32 = 120;
pub const T_DELETE: u32 = 121;
pub const T_COPY: u32 = 130;
pub const T_TELEGRAM: u32 = 131;
pub const T_DONE: u32 = 132;
pub const T_SEND: u32 = 133;
pub const T_TILE: u32 = 1000;

/// The named element the clip plays over.
pub const MEDIA: &str = "share-media";
const LOAD_SEG: &str = "share-load-seg";
const LOAD_STRIP: &str = "share-load-strip";

fn kind_label(c: &Capture) -> &'static str {
    match c.kind {
        Kind::Clip => "Clip",
        Kind::Screenshot => "Screenshot",
    }
}

/// What a build asked the image loader for, so failures can turn into "Can't read this file".
type Requested = Vec<(String, u64)>;

/// A capture's picture as an image leaf, or None when there is none (or it failed).
fn picture(ui: &mut Ui, req: &mut Requested, id: &str, thumb: Option<&Thumb>, w: f32, h: f32, fit: Fit, st: S) -> Option<El> {
    let src = match thumb? {
        Thumb::Static(n, b) => Src::Static(n, b),
        Thumb::File(p) => Src::File(p.clone()),
        Thumb::Failed => return None,
    };
    let k = ui.img.request(ImgReq { src, w: w.round().max(1.0) as u32, h: h.round().max(1.0) as u32, fit });
    if matches!(ui.img.peek(k), ImgState::Failed) {
        return None;
    }
    req.push((id.to_string(), k));
    let el = match fit {
        Fit::Contain => kit::art::image(k, kit::ArtFit::Contain(0.5, 0.5)),
        Fit::Cover { fy } => kit::art::image(k, kit::ArtFit::Cover(fy)),
    };
    Some(el.with_style(st.to_style(ui.rem)))
}

/// A hairline across the top of a box (`border-t border-hair`).
fn hair_top(ui: &Ui) -> El {
    ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0)).bg(tok::HAIR)
}

/// The leaf text of a pick button (`btn h-[2.25rem] rounded-chip px-[0.8rem] text-small
/// font-medium`, white when chosen).
fn pick(ui: &mut Ui, label: &str, on: bool, tag: u32, hint: Option<&str>) -> El {
    let look = Look::default().bg(if on { tok::FG } else { tok::L2 }).radius(ui.r(0.25));
    let focus = look.clone().bg(tok::FG).scale(1.04);
    let el = ui
        .bx(s().row().items_center().justify_center().h(2.25).px(0.8))
        .look(look)
        .focus_look(focus)
        .child(ui.text(label, T::new(Ty::Small).w(500).c(if on { tok::BG } else { tok::FG })).focus_color(tok::BG));
    kit::nav(el, tag, hint, None, false)
}

fn flex1(el: El) -> El {
    el.style(|st| {
        st.flex_grow = 1.0;
        st.flex_shrink = 1.0;
        st.flex_basis = length(0.0);
    })
}

/// An error line: a red dot and the words.
fn error_line(ui: &mut Ui, msg: &str, center: bool) -> El {
    let d = kit::dot(ui, if center { s() } else { s().mt(0.4) }, DotTone::Negative, false);
    let row = if center { s().row().items_center().gap(0.5) } else { s().row().items_start().gap(0.5) };
    ui.bx(row).child(d).child(ui.text_s(s().shrink().min0(), msg, T::new(Ty::Small).wrap()))
}

/// The page (ShareScreen's JSX). `req` collects image requests.
pub fn view(ui: &mut Ui, st: &ShareState, req: &mut Requested) -> El {
    let body = match st.selected_capture() {
        Some(c) => detail(ui, st, c, req),
        None => grid(ui, st, req),
    };
    let scroll = ui.bx(s().fill().col().px(2.5).pb(3.0).pt(1.0)).scrolls_y(key("share-scroll")).child(body);
    // [data-scroll] pads the top with max(1rem, ..) over pt-[2rem] (app.css wins over utilities).
    // LoadBar: 2px at the top, a sliding segment while something loads.
    let on = st.loading();
    let strip = ui.bx(s().fill()).bg(tok::fg(0.55)).name(LOAD_STRIP).look(Look::default().bg(tok::fg(0.55)).origin(0.0, 0.5));
    let seg = ui.bx(s().fill()).name(LOAD_SEG).child(strip);
    let bar = ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(2.0)).look(Look::default().z(30).opacity(if on { 1.0 } else { 0.0 })).clip().child(seg);
    ui.bx(s().fill()).look(Look::default().bg(tok::BG).z(layer::PAGE)).child(scroll).child(bar)
}

fn grid(ui: &mut Ui, st: &ShareState, req: &mut Requested) -> El {
    let status = st.status.as_ref();
    let note = clips_note(status);
    let head = ui
        .bx(s().col())
        .child(ui.text("Share", T::new(Ty::Title).w(600).track(-0.02)))
        .child(ui.text_s(
            s().mt(0.25).max_w(46.0),
            "Shared links open on rare.market for anyone. Files are stored on Walrus and your wallet pays for the storage.",
            T::new(Ty::Small).c(tok::MUTED).wrap(),
        ));

    let taking = st.taking;
    let shot_btn = kit::button(
        ui,
        Btn::new(if taking == Some(Taking::Screenshot) { "Saving" } else { "Take screenshot" }, T_SHOT).def(true).hint("Take a screenshot").disabled(taking.is_some() || status.is_some_and(|s| !s.screenshot.available)),
    );
    let secs = status.map(|s| s.clips.seconds).unwrap_or(rare_share::clip::SECONDS);
    let save = crate::features::capture::save_label(secs);
    let clip_btn = kit::button(
        ui,
        Btn::new(if taking == Some(Taking::Clip) { "Saving" } else { save.as_str() }, T_CLIP).hint("Save the clip").disabled(taking.is_some() || !status.is_some_and(|s| s.clips.recording)),
    );
    let capture_words = ui
        .bx(s().col().grow())
        .child(ui.text("Capture", T::new(Ty::Body).w(600)))
        .child(ui.text(format!("In a game: Guide and Y for a screenshot, Guide and View for {}.", crate::features::capture::last_words(secs)), T::new(Ty::Small).c(tok::MUTED).wrap()));
    let actions = ui.bx(s().row().min_h(4.25).items_center().gap(1.25).px(1.4).py(0.85)).group(kit::group("share-actions", true)).child(capture_words).child(shot_btn).child(clip_btn);

    let enabled = status.is_some_and(|s| s.clips.enabled);
    let available = status.is_some_and(|s| s.clips.available);
    let sub = match status {
        None => String::new(),
        Some(s) if !s.clips.available => note.unwrap_or("").to_string(),
        Some(s) if s.clips.enabled && s.clips.recording => format!("Recording in the background{}. Uses a little battery.", s.clips.method.as_deref().map(|m| format!(" with {m}")).unwrap_or_default()),
        Some(s) if s.clips.enabled => "On. Recording starts with the next game.".into(),
        Some(_) => "Off. Turn it on to save clips.".into(),
    };
    let mut words = ui.bx(s().col().grow()).child(ui.text(format!("Instant replay: keep {} while playing", crate::features::capture::last_words(secs)), T::new(Ty::Body).w(600)));
    if !sub.is_empty() {
        words = words.child(ui.text(sub, T::new(Ty::Small).c(tok::MUTED).wrap()));
    }
    let mut row = ui.bx(s().row().min_h(4.25).wp(1.0).items_center().gap(1.25).px(1.4).py(0.85)).child(words);
    if status.is_some_and(|s| s.clips.recording) {
        let d = kit::dot(ui, s(), DotTone::Brand, true);
        row = row.child(ui.bx(s().row().items_center().gap(0.45)).child(d).child(ui.text("Recording", T::new(Ty::Small).c(tok::MUTED))));
    }
    row = row.child(kit::toggle(ui, enabled)).child(hair_top(ui));
    if !available {
        row = row.opacity(0.6);
    }
    let hint = if enabled { "Turn off" } else { "Turn on" };
    let row = {
        let fl = row.look.clone().bg(tok::L4);
        row.focus_look(fl).nav(|f| f.tag(T_CLIPS).hints(Some(hint), None, None).disabled(!available))
    };
    let mut panel = kit::panel(ui, s().col().mt(1.5)).child(actions).child(row);
    if status.is_some_and(|s| !s.screenshot.available) {
        panel = panel.child(ui.bx(s().px(1.4).py(0.85)).child(hair_top(ui)).child(ui.text("Screenshots aren't available in this session.", T::new(Ty::Small).c(tok::MUTED))));
    }

    let mut page = ui.bx(s().col()).child(head).child(panel);
    if let Some(e) = &st.error {
        page = page.child(ui.bx(s().mt(0.9)).child(error_line(ui, e, true)));
    }
    let count = st.captures.as_ref().map(|c| format!("{} on this device", c.len()));
    page = page.child(kit::section_head(ui, s().mb(0.9).mt(2.0), "Recent", count.as_deref(), None));
    match st.captures.as_deref() {
        Some([]) => page = page.child(kit::empty(ui, s(), "No screenshots or clips yet", Some("Press Guide and Y in a game, or take one here."), vec![])),
        Some(list) => {
            let tw = ((ui.w - ui.r(8.0)) / 4.0).floor();
            let mut tiles = Vec::new();
            for (i, c) in list.iter().enumerate() {
                tiles.push(tile(ui, st, c, i, tw, req));
            }
            page = page.child(ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0)]).gap(1.0)).group(kit::group("share-grid", false)).kids(tiles));
        }
        None => {}
    }
    page
}

fn tile(ui: &mut Ui, st: &ShareState, c: &Capture, i: usize, tw: f32, req: &mut Requested) -> El {
    let px = 1.0 / ui.rem;
    let fresh = st.is_fresh(c);
    let (iw, ih) = (tw - 2.0, tw * 9.0 / 16.0 - 2.0);
    let thumb = st.thumbs.get(&c.id);
    let mut inner = ui.bx(s().abs(Some(px), Some(px), Some(px), Some(px))).radius(ui.r(0.5) - 1.0).clip();
    match picture(ui, req, &c.id, thumb, iw, ih, Fit::Cover { fy: 0.5 }, s().fill()) {
        Some(img) => inner = inner.child(img),
        None if thumb.is_some() => inner = inner.child(ui.bx(s().fill().row().items_center().justify_center()).child(ui.text("Can't read this file", T::new(Ty::Small).c(tok::MUTED)))),
        None => {}
    }
    let black = Color::BLACK;
    inner = inner.child(ui.bx(s().abs_pct(None, Some(0.0), Some(0.0), Some(0.0)).hp(0.6)).look(Look::default().grad(Gradient { dir: GradDir::ToTop, stops: [black.alpha(0.9), black.alpha(0.45), black.alpha(0.0)], mid: 0.5 })));
    if fresh {
        inner = inner.child(ui.bx(s().abs_pct(Some(0.0), Some(0.0), None, Some(0.0)).hp(0.4)).look(Look::default().grad(Gradient { dir: GradDir::ToBottom, stops: [black.alpha(0.7), black.alpha(0.35), black.alpha(0.0)], mid: 0.5 })));
        let d = kit::dot(ui, s(), DotTone::Brand, false);
        inner = inner.child(ui.bx(s().abs(Some(0.65), Some(0.85), None, None).row().items_center().gap(0.4)).child(d).child(ui.text("New", T::new(Ty::Label).w(500).c(Color::WHITE.alpha(0.85)))));
    }
    let name = c.game.as_ref().map(|g| g.name.clone()).unwrap_or_else(|| kind_label(c).to_string());
    let words = ui
        .bx(s().col().grow())
        .child(ui.text(name, T::new(Ty::Body).w(700).c(Color::WHITE).trunc()))
        .child(ui.text(format!("{} · {}", kind_label(c), ago(c.created_ms, st.now_ms)), T::new(Ty::Meta).c(Color::WHITE.alpha(0.6)).trunc()));
    let mut foot = ui.bx(s().abs(None, Some(0.85), Some(0.7), Some(0.85)).row().items_end().gap(0.5)).child(words);
    if c.kind == Kind::Clip {
        foot = foot.child(ui.text(fmt_duration(c.duration_ms), T::new(Ty::Meta).tab().c(Color::WHITE.alpha(0.8))));
    }
    inner = inner.child(foot);
    let border = ui.bx(s().fill()).look(Look::default().radius(ui.r(0.5)).border(1.0, tok::HAIR));
    let el = ui.bx(s().wp(1.0).aspect(16.0 / 9.0)).bg(tok::L1).radius(ui.r(0.5)).child(inner).child(border).key(key(&c.id));
    kit::nav(kit::tile(el, 1.06), T_TILE + i as u32, Some("Open"), None, false)
}

fn detail(ui: &mut Ui, st: &ShareState, c: &Capture, req: &mut Requested) -> El {
    // grid-cols-[minmax(0,1fr)_23rem] gap-[1.5rem] inside px-[2.5rem].
    let mw = ui.w - ui.r(5.0) - ui.r(23.0) - ui.r(1.5);
    let mh = mw * 9.0 / 16.0;
    let mut media = ui.bx(s().wp(1.0).aspect(16.0 / 9.0)).bg(tok::L1).radius(ui.r(1.0)).clip().name(MEDIA);
    if let Some(img) = picture(ui, req, &c.id, st.thumbs.get(&c.id), mw, mh, Fit::Contain, s().fill()) {
        media = media.child(img);
    }

    let name = c.game.as_ref().map(|g| g.name.clone()).unwrap_or_else(|| kind_label(c).to_string());
    let mut meta: Vec<String> = vec![kind_label(c).into()];
    if c.kind == Kind::Clip {
        meta.push(fmt_duration(c.duration_ms));
    }
    // `${w} x ${h}`: Inter's contextual alternates draw the x between digits as a multiplication
    // sign in the browser; the native shaper does not, so the sign is written out.
    meta.extend([format!("{} \u{d7} {}", c.width, c.height), fmt_bytes(c.bytes), ago(c.created_ms, st.now_ms)]);
    meta.retain(|m| !m.is_empty());
    let head = ui.bx(s().col().min0()).child(ui.text(name, T::new(Ty::Row).w(600).trunc())).child(ui.text(meta.join(" · "), T::new(Ty::Small).c(tok::MUTED).wrap()));
    let mut panel = kit::panel(ui, s().col().gap(1.0).px(1.25).py(1.2)).child(head);

    let q = st.quote.as_ref();
    let rare = st.sponsored();
    match &st.phase {
        Phase::Idle | Phase::Error(_) => {
            let mut durs = Vec::new();
            for (i, d) in DURATIONS.iter().enumerate() {
                durs.push(pick(ui, d.label, st.duration == d.id, T_DUR + i as u32, Some(&format!("Keep for {}", d.label))));
            }
            let keep = ui
                .bx(s().col().gap(0.5))
                .child(ui.text("Keep it for", T::new(Ty::Label).w(500).c(tok::MUTED)))
                .child(ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5)).group(kit::group("share-duration", false)).kids(durs));
            panel = panel.child(keep);

            if q.and_then(|q| q.sponsor.as_ref()).is_some_and(|s| s.covered) {
                let mut who = Vec::new();
                for (i, (p, label)) in [(Payer::Rare, "Rare covers it"), (Payer::Me, "My wallet")].into_iter().enumerate() {
                    who.push(pick(ui, label, st.payer == p, T_PAYER + i as u32, None));
                }
                panel = panel.child(
                    ui.bx(s().col().gap(0.5))
                        .child(ui.text("Who pays", T::new(Ty::Label).w(500).c(tok::MUTED)))
                        .child(ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5)).group(kit::group("share-payer", false)).kids(who)),
                );
            }

            let mut price = ui.bx(s().col().pt(0.9)).child(hair_top(ui));
            let anyone = "Anyone with the link can see it.";
            match q {
                Some(q) if rare => {
                    let note = q.sponsor.as_ref().and_then(|s| s.note.clone()).unwrap_or_else(|| format!("Rare pays for {} days on Walrus.", q.days));
                    price = price.child(ui.text("Free", T::new(Ty::Body).w(600))).child(ui.text(format!("{note} {anyone}"), T::new(Ty::Small).c(tok::MUTED).wrap()));
                }
                Some(q) if q.cost.is_some() => {
                    let cost = q.cost.unwrap();
                    let line = ui
                        .bx(s().row().wrap())
                        .child(ui.text(format!("About {}", fmt_wal(Some(cost.wal_frost))), T::new(Ty::Body).w(600).tab()))
                        .child(ui.text(format!(" plus {} in fees", fmt_sui_mist(cost.tip_mist + cost.gas_mist)), T::new(Ty::Body).c(tok::MUTED)));
                    let net = if st.network == super::format::Network::Testnet { " testnet" } else { "" };
                    price = price.child(line).child(ui.text(format!("Stored on Walrus{net} for {} days, {}. {anyone}", q.days, fmt_bytes(q.size)), T::new(Ty::Small).c(tok::MUTED).wrap()));
                }
                Some(_) => price = price.child(ui.text("Couldn't get the Walrus price. Try again in a moment.", T::new(Ty::Small).c(tok::MUTED).wrap())),
                None => price = price.child(ui.text("Getting the price", T::new(Ty::Small).c(tok::MUTED))),
            }
            if st.address.is_none() && !rare {
                price = price.child(ui.text_s(s().mt(0.4), "Add a Rare Wallet account to share.", T::new(Ty::Small).c(tok::MUTED)));
            }
            panel = panel.child(price);
            if let Phase::Error(m) = &st.phase {
                panel = panel.child(error_line(ui, m, false));
            }
            if let Some(e) = &st.error {
                panel = panel.child(error_line(ui, e, false));
            }
            let share = kit::button(ui, Btn::new(if matches!(st.phase, Phase::Error(_)) { "Try again" } else { "Share" }, T_SHARE).primary().def(true).hint("Share").disabled(st.share_blocked()));
            let del = kit::button(ui, Btn::new(if st.confirm { "Press again" } else { "Delete" }, T_DELETE).hint(if st.confirm { "Delete" } else { "Delete from this device" }));
            panel = panel.child(ui.bx(s().row().gap(0.5)).group(kit::group("share-go", true)).child(flex1(share)).child(del));
        }
        Phase::Uploading(step) => {
            let cur = STEPS.iter().position(|x| x.0 == *step).unwrap_or(0);
            let mut col = ui.bx(s().col().gap(0.25).pt(0.9)).child(hair_top(ui));
            for (i, (_, label, doing)) in STEPS.iter().enumerate() {
                let (tone, live) = match i.cmp(&cur) {
                    std::cmp::Ordering::Less => (DotTone::Primary, false),
                    std::cmp::Ordering::Equal => (DotTone::Brand, true),
                    std::cmp::Ordering::Greater => (DotTone::Muted, false),
                };
                let d = kit::dot(ui, s(), tone, live);
                let words = if i == cur { *doing } else { *label };
                col = col.child(ui.bx(s().row().h(2.25).items_center().gap(0.65)).child(d).child(ui.text(words, T::new(Ty::Small).c(if i > cur { tok::MUTED } else { tok::FG }))));
            }
            let note = if rare { "Rare is storing it." } else { "Two approvals in Rare Wallet: one pays for the storage, one confirms it." };
            col = col.child(ui.text_s(s().mt(0.4), note, T::new(Ty::Small).c(tok::MUTED).wrap()));
            panel = panel.child(col);
        }
        Phase::Done { result, copied } => {
            let qr_px = ui.r(10.0).round() as u32;
            let mut code = ui.bx(s().size(11.0).p(0.5)).bg(Color::WHITE).radius(ui.r(0.375)).clip();
            if let Some(src) = qr_src(&result.link, qr_px) {
                let k = ui.img.request(ImgReq { src, w: qr_px, h: qr_px, fit: Fit::Cover { fy: 0.5 } });
                code = code.child(El::image(k).with_style(s().wh(10.0, 10.0).to_style(ui.rem)));
            }
            let mut col = ui.bx(s().col().items_center().gap(0.75).pt(1.0)).child(hair_top(ui)).child(code);
            // num w-full break-all text-center: the mono face is a fixed advance, so the lines break
            // at a whole number of characters.
            let text = result.link.trim_start_matches("https://");
            let per = ((ui.r(20.5)) / (Ty::Meta.metrics().0 * ui.rem * 0.6)).floor().max(1.0) as usize;
            let chars: Vec<char> = text.chars().collect();
            let mut lines = ui.bx(s().col().wp(1.0));
            for chunk in chars.chunks(per) {
                lines = lines.child(center_text(ui, s().wp(1.0), chunk.iter().collect::<String>(), T::new(Ty::Meta).mono()));
            }
            col = col.child(lines);
            let d = kit::dot(ui, s(), if *copied { DotTone::Brand } else { DotTone::Muted }, false);
            col = col.child(ui.bx(s().row().items_center().gap(0.45)).child(d).child(ui.text(if *copied { "Copied" } else { "Scan the code or copy the link" }, T::new(Ty::Small).c(tok::MUTED))));
            panel = panel.child(col);
            let mut btns = ui.bx(s().col().gap(0.5)).group(kit::group("share-done", false));
            btns = btns.child(kit::button(ui, Btn::new("Copy link", T_COPY).primary().def(true).hint("Copy link").min_w(0.0)).style(|st| st.size.width = rare_render::taffy::prelude::percent(1.0)));
            let send = match super::send_to() {
                Some((_, name)) => format!("Send to {name}"),
                None => "Send to a friend".to_string(),
            };
            btns = btns.child(kit::button(ui, Btn::new(send, T_SEND).hint("Send to a friend").min_w(0.0)).style(|st| st.size.width = rare_render::taffy::prelude::percent(1.0)));
            if telegram_poster().is_some() {
                btns = btns.child(kit::button(ui, Btn::new("Post to Telegram", T_TELEGRAM).hint("Post to Telegram")).style(|st| st.size.width = rare_render::taffy::prelude::percent(1.0)));
            }
            let done = kit::button(ui, Btn::new("Done", T_DONE).hint("Back to your captures"));
            let del = kit::button(ui, Btn::new(if st.confirm { "Press again" } else { "Delete" }, T_DELETE).hint(if st.confirm { "Delete" } else { "Delete from this device" }));
            btns = btns.child(ui.bx(s().row().gap(0.5)).child(flex1(done)).child(flex1(del)));
            panel = panel.child(btns);
        }
    }
    ui.bx(s().grid(&[Track::Fr(1.0), Track::Rem(23.0)]).items_start().gap(1.5)).child(media).child(panel)
}

/// The QR as a static PNG source, made once per link and size.
fn qr_src(link: &str, px: u32) -> Option<Src> {
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<(String, u32), (&'static str, &'static [u8])>>> = Mutex::new(None);
    let mut g = CACHE.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);
    if let Some((n, b)) = map.get(&(link.to_string(), px)) {
        return Some(Src::Static(n, b));
    }
    let png = qr::png(link, px)?;
    let n: &'static str = Box::leak(format!("share-qr-{px}-{link}.png").into_boxed_str());
    let b: &'static [u8] = Box::leak(png.into_boxed_slice());
    map.insert((link.to_string(), px), (n, b));
    Some(Src::Static(n, b))
}

/// The Share screen.
pub struct SharePage {
    pub st: ShareState,
    pub host: Host,
    /// Fixtures and goldens: no looping animation, no playback.
    pub still: bool,
    player: Option<Player>,
    tex: Option<(TexId, u32, u32)>,
    requested: Requested,
    /// The tile to land on when coming back to the grid.
    last_tile: Option<String>,
    view_key: String,
    auto_focus: Option<Id>,
    loops: HashMap<&'static str, Id>,
    load_on: bool,
    posters: Option<Receiver<(String, Result<std::path::PathBuf, String>)>>,
}

impl SharePage {
    /// Builds Share on the scene root (it covers the shell like the web's fixed layer).
    pub fn new(cx: &mut Cx, st: ShareState, still: bool, now: f64) -> SharePage {
        let root = cx.scene.root;
        let mut p = SharePage {
            st,
            host: Host::new(cx, root),
            still,
            player: None,
            tex: None,
            requested: Vec::new(),
            last_tile: None,
            view_key: String::new(),
            auto_focus: None,
            loops: HashMap::new(),
            load_on: false,
            posters: None,
        };
        if p.st.selected_capture().is_some() {
            p.open_player();
        }
        p.make_posters();
        p.render(cx, now);
        p
    }

    /// Clip posters for tiles that have none yet (device builds with GStreamer), one at a time
    /// off the UI thread, newest first like makeThumbs.
    fn make_posters(&mut self) {
        if self.still || !cfg!(all(target_os = "linux", feature = "share-video")) {
            return;
        }
        let todo: Vec<(String, String)> = self.st.captures.iter().flatten().take(36).filter(|c| c.kind == Kind::Clip && !self.st.thumbs.contains_key(&c.id)).map(|c| (c.id.clone(), c.path.clone())).collect();
        if todo.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for (id, path) in todo {
                let r = video::poster_thumb(std::path::Path::new(&path), &id);
                if tx.send((id, r)).is_err() {
                    break;
                }
            }
        });
        self.posters = Some(rx);
    }

    /// Opens a capture as if its tile was pressed (Share from a game page); the act to run.
    pub fn open_capture(&mut self, cx: &mut Cx, id: &str, now: f64) -> Option<String> {
        self.st.captures.as_ref()?.iter().find(|c| c.id == id)?;
        self.last_tile = Some(id.to_string());
        self.st.select(Some(id));
        self.open_player();
        self.render(cx, now);
        Some(format!("share:open:{id}"))
    }

    fn open_player(&mut self) {
        self.player = None;
        if self.still {
            return;
        }
        let Some(c) = self.st.selected_capture().filter(|c| c.kind == Kind::Clip) else { return };
        let w = (self.host.w - self.host.rem * 29.5).max(1.0) as u32;
        match Player::open(std::path::Path::new(&c.path), Some(w)) {
            Ok(p) => self.player = Some(p),
            Err(e) => log::info!("share: no playback ({e}), showing the poster"),
        }
    }

    fn key_of(&self) -> String {
        match self.st.selected_capture() {
            Some(_) => {
                let at = match self.st.phase {
                    Phase::Idle => "idle",
                    Phase::Uploading(_) => "uploading",
                    Phase::Done { .. } => "done",
                    Phase::Error(_) => "error",
                };
                format!("detail-{at}-{}", self.st.quote.is_some())
            }
            None => format!("grid-{}", self.st.captures.is_none()),
        }
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let mut req = Vec::new();
        let mut loops = Vec::new();
        {
            let st = &self.st;
            self.host.render(cx, now, |ui| {
                let el = view(ui, st, &mut req);
                loops = std::mem::take(&mut ui.loops);
                el
            });
        }
        self.requested = req;
        if !self.still {
            let mut next = HashMap::new();
            for (name, keys, period) in &loops {
                if let Some(id) = self.host.id(name) {
                    if self.loops.get(name) != Some(&id) {
                        cx.scene.n(id).opacity.start_loop(keys, *period, now);
                    }
                    next.insert(*name, id);
                }
            }
            self.loops = next;
            self.load_loop(cx, now);
        }
        // Lands focus on the view's default (Share once the price is in, the last tile when
        // coming back), unless the player already moved it.
        let k = self.key_of();
        if k != self.view_key {
            let moved = self.host.nav.current().is_some() && self.host.nav.current() != self.auto_focus;
            let keep = moved && k.starts_with("detail-idle-true") && self.view_key.starts_with("detail-");
            if !keep {
                let back = if self.st.selected.is_none() { self.last_tile.as_ref().and_then(|id| self.st.captures.as_ref()?.iter().position(|c| &c.id == id)) } else { None };
                if !back.is_some_and(|i| self.host.focus_tag(cx, T_TILE + i as u32, now)) {
                    self.host.nav.focus_first(now);
                    self.host.view.set_focus(cx.scene, cx.t, self.host.nav.current(), now);
                }
                self.auto_focus = self.host.nav.current();
            }
            self.view_key = k;
        }
    }

    fn load_loop(&mut self, cx: &mut Cx, now: f64) {
        let on = self.st.loading();
        if on == self.load_on {
            return;
        }
        self.load_on = on;
        let (Some(seg), Some(strip)) = (self.host.id(LOAD_SEG), self.host.id(LOAD_STRIP)) else { return };
        if on {
            let w = cx.scene.w;
            let xs: &'static [(f32, f32)] = Box::leak(LOAD_X.frames.iter().map(|(t, v)| (*t, v * w)).collect::<Vec<_>>().into_boxed_slice());
            let kx: &'static Keys = Box::leak(Box::new(Keys { frames: xs, ease: LOAD_X.ease }));
            cx.scene.n(seg).tx.start_loop(kx, 1.1, now);
            cx.scene.n(strip).scale.start_loop(&LOAD_SX, 1.1, now);
        } else {
            cx.scene.n(seg).tx.snap(0.0);
            cx.scene.n(strip).scale.snap(1.0);
        }
    }

    /// The hint bar: A says what the focused control does, B goes back, X switches the duration.
    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        let mut h = Vec::new();
        if let Some(f) = self.host.nav.current_item() {
            if !f.disabled {
                h.push(("a", f.hint_a.clone().unwrap_or_else(|| "Select".into())));
            }
        }
        h.push(("b", "Back".into()));
        if self.st.selected.is_some() && self.st.phase.editable() {
            let other = DURATIONS.iter().find(|d| d.id != self.st.duration).map(|d| d.label).unwrap_or("");
            h.push(("x", format!("Keep for {other}")));
        }
        h
    }

    fn act_duration(&mut self, d: DurationId) -> Out {
        if self.st.set_duration(d) {
            Out::Act(format!("share:duration:{}", if d == DurationId::Year { "year" } else { "month" }))
        } else {
            Out::Moved
        }
    }

    /// A press; returns what the app should do. Acts: `share:screenshot`, `share:clip`,
    /// `share:clips:on|off`, `share:open:<id>` (prepare and price it), `share:duration:month|year`
    /// (price again), `share:go` (upload; report steps through `st.phase`), `share:copy`,
    /// `share:telegram`, `share:delete:<id>`.
    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        let out = match b {
            Pad::B if self.st.selected.is_some() => {
                if !self.st.select(None) {
                    return Out::None;
                }
                self.player = None;
                Out::Moved
            }
            Pad::B => return Out::Close,
            Pad::X if self.st.selected.is_some() && self.st.phase.editable() => {
                let d = self.st.next_duration();
                self.act_duration(d)
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                return if self.host.step(cx, b, now) { Out::Moved } else { Out::None };
            }
            Pad::A => self.activate(),
            _ => return Out::None,
        };
        if out != Out::None {
            self.render(cx, now);
        }
        out
    }

    fn activate(&mut self) -> Out {
        let Some(tag) = self.host.tag() else { return Out::None };
        if self.host.nav.current_item().is_some_and(|f| f.disabled) {
            return Out::None;
        }
        let id = self.st.selected.clone();
        match tag {
            T_SHOT => {
                self.st.taking = Some(Taking::Screenshot);
                self.st.error = None;
                Out::Act("share:screenshot".into())
            }
            T_CLIP => {
                self.st.taking = Some(Taking::Clip);
                self.st.error = None;
                Out::Act("share:clip".into())
            }
            T_CLIPS => Out::Act(format!("share:clips:{}", if self.st.status.as_ref().is_some_and(|s| s.clips.enabled) { "off" } else { "on" })),
            t if t >= T_TILE => {
                let Some(c) = self.st.captures.as_ref().and_then(|l| l.get((t - T_TILE) as usize)).map(|c| c.id.clone()) else { return Out::None };
                self.last_tile = Some(c.clone());
                self.st.select(Some(&c));
                self.open_player();
                Out::Act(format!("share:open:{c}"))
            }
            t if (T_DUR..T_DUR + 2).contains(&t) => self.act_duration(DURATIONS[(t - T_DUR) as usize].id),
            t if (T_PAYER..T_PAYER + 2).contains(&t) => {
                self.st.payer = if t == T_PAYER { Payer::Rare } else { Payer::Me };
                Out::Moved
            }
            T_SHARE => {
                if self.st.share_blocked() {
                    return Out::None;
                }
                if self.st.begin_share() {
                    Out::Act("share:go".into())
                } else {
                    Out::Moved
                }
            }
            T_DELETE => {
                if !self.st.confirm {
                    self.st.confirm = true;
                    return Out::Moved;
                }
                self.st.confirm = false;
                match id {
                    Some(id) => Out::Act(format!("share:delete:{id}")),
                    None => Out::Moved,
                }
            }
            T_COPY => Out::Act("share:copy".into()),
            T_TELEGRAM => Out::Act("share:telegram".into()),
            T_SEND => Out::Act("share:send".into()),
            T_DONE => {
                self.st.select(None);
                self.player = None;
                Out::Moved
            }
            _ => Out::None,
        }
    }

    /// The app changed `st` (an answer arrived): rebuild.
    pub fn changed(&mut self, cx: &mut Cx, now: f64) {
        if self.st.selected.is_none() {
            self.player = None;
        }
        self.make_posters();
        self.render(cx, now);
    }

    /// The deleted capture's toast (`<Kind> deleted`).
    pub fn deleted_toast(c: &Capture) -> String {
        format!("{} deleted", kind_label(c))
    }

    pub fn kind_of(c: &Capture) -> &'static str {
        kind_word(c.kind)
    }
}

impl Page for SharePage {
    fn paint_extra(&mut self, scene: &Scene, _ts: &mut TextSys, tex: &mut dyn Textures, out: &mut DrawList) {
        let frame = self.player.as_mut().and_then(|p| p.take_frame());
        if self.player.is_none() {
            if let Some((t, ..)) = self.tex.take() {
                tex.free(t);
            }
            return;
        }
        if let Some(f) = frame.filter(|f| f.w > 0 && f.h > 0) {
            if let Some((t, ..)) = self.tex.take() {
                tex.free(t);
            }
            self.tex = Some((tex.create(f.w, f.h, &f.rgba), f.w, f.h));
        }
        let (Some((t, tw, th)), Some(media)) = (self.tex, self.host.id(MEDIA)) else { return };
        let r = scene.settled_rect(media);
        let v = scene.settled_rect(self.host.view.host());
        if r[2] <= 0.0 || r[3] <= 0.0 {
            return;
        }
        // object-contain, centred, clipped to the rounded box and the screen.
        let k = (r[2] / tw as f32).min(r[3] / th as f32);
        let (dw, dh) = (tw as f32 * k, th as f32 * k);
        let rect = [r[0] + (r[2] - dw) / 2.0, r[1] + (r[3] - dh) / 2.0, dw, dh];
        let clip = [r[0].max(v[0]), r[1].max(v[1]), (r[0] + r[2]).min(v[0] + v[2]), (r[1] + r[3]).min(v[1] + v[3])];
        let radius = self.host.rem;
        out.push(
            Prim { rect, clip, c0: [1.0; 4], uv: [0.0, 0.0, 1.0, 1.0], p: [0.0, 0.0, KIND_IMAGE, 1.0], rclip: [r[0], r[1], r[0] + r[2], r[1] + r[3]], cr: [radius, 0.0, 0.0, 0.0], ..Default::default() },
            Some(t),
        );
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for SharePage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        let mut dirty = self.host.tick(cx, now);
        if let Some(p) = &mut self.player {
            p.poll();
        }
        if let Some(rx) = &self.posters {
            while let Ok((id, r)) = rx.try_recv() {
                self.st.thumbs.insert(id, r.map(Thumb::File).unwrap_or(Thumb::Failed));
                dirty = true;
            }
        }
        // A picture that can't be decoded says so on its tile.
        for (id, k) in &self.requested {
            if matches!(cx.img.peek(*k), ImgState::Failed) && self.st.thumbs.get(id) != Some(&Thumb::Failed) {
                self.st.thumbs.insert(id.clone(), Thumb::Failed);
                dirty = true;
            }
        }
        if dirty {
            self.render(cx, now);
        }
    }
    fn next_deadline(&self) -> Option<f64> {
        // A playing clip wants every frame.
        self.player.as_ref().map(|_| rare_render::clock::secs() + 1.0 / 60.0)
    }
}
