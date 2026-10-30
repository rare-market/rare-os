//! Share over the launcher (ShareScreen.tsx): the device's captures, taking a screenshot or a clip,
//! deleting, and posting one: opening a capture builds its quilt and prices it, Share uploads it to
//! Walrus (Rare Wallet reviews and signs both approvals, or the sponsor stores it), then the link is
//! copied. Opens from Quick settings ("Open Share") as `Overlay::Other("share")`. The work is in
//! `share_up`.

use super::*;
use crate::screens::share::{device_state, screen::SharePage};
use crate::screens::overlay::Screen;
use rare_render::input::Pad;
use super::share_up::{self, Prepared, Priced};
use crate::screens::share::format::{self, DurationId};
use crate::screens::share::store::{Phase, ShareResult, Step};
use rare_share::{Capture, ShareHost};
use std::sync::mpsc::{channel, Receiver, Sender};

struct NativeShare {
    running: Vec<String>,
    titles: Vec<(String, String)>,
}

impl ShareHost for NativeShare {
    fn clips_enabled(&self) -> bool {
        crate::features::capture::replay_on()
    }
    fn replay_seconds(&self) -> u32 {
        crate::features::capture::replay_seconds()
    }
    fn clips_cap_bytes(&self) -> Option<u64> {
        Some((crate::features::capture::cap_gb() as u64) << 30)
    }
    fn running(&self) -> Vec<String> {
        self.running.clone()
    }
    fn title_of(&self, id: &str) -> Option<String> {
        self.titles.iter().find(|(i, _)| i == id).map(|(_, t)| t.clone())
    }
    fn rumble_enabled(&self) -> bool {
        false
    }
    fn rumble(&self) {}
    fn launcher_front(&self) -> bool {
        true
    }
    fn toast(&self, _text: &str) {}
}

pub(super) enum Done {
    Shot(Result<Capture, String>),
    Clip(Result<Capture, String>),
    /// A capture's quilt is built (or why not).
    Prepared(String, Result<Prepared, String>),
    Priced(String, DurationId, Priced),
    Step(Step),
    Uploaded(Result<ShareResult, (bool, String)>),
    Toast(String, Tone),
}

pub(super) struct ShareSlot {
    pub(super) page: SharePage,
    nodes: Vec<Id>,
    prepared: Option<Prepared>,
    /// Opened from this game's page (its screenshots): B goes back there.
    game: Option<String>,
    tx: Sender<Done>,
    rx: Receiver<Done>,
}

impl App {
    fn share_host(&self) -> NativeShare {
        NativeShare { running: self.st.running.clone(), titles: self.st.games().iter().map(|g| (g.id.clone(), g.title.clone())).collect() }
    }

    /// Mounts Share when the overlay is Share, takes it down when the overlay moved on.
    pub(super) fn share_sync(&mut self, now: f64) {
        let want = matches!(self.st.overlay, Some(Overlay::Other("share")));
        if want && self.share.is_none() {
            let host = self.share_host();
            let address = crate::screens::wallet::service::tx_signer().map(|s| s.account);
            let mut st = device_state(wall_ms() as u64, 0, address);
            st.network = share_up::network();
            format::set_telegram_poster(Some(share_up::post_telegram));
            st.status = Some(rare_share::status(&host));
            let root = self.scene.root;
            let before = self.scene.children(root);
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            let page = SharePage::new(&mut cx, st, false, now);
            let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
            for n in &nodes {
                self.scene.n(*n).z = layer::SHEET;
            }
            let (tx, rx) = channel();
            let from_game = crate::screens::game::extras::take_share_open();
            self.share = Some(ShareSlot { page, nodes, prepared: None, game: from_game.as_ref().map(|f| f.1.clone()), tx, rx });
            // A game page's Share: that capture, opened and priced at once.
            if let Some((id, _)) = from_game {
                let act = self.share.as_mut().and_then(|s| {
                    let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                    s.page.open_capture(&mut cx, &id, now)
                });
                if let Some(a) = act {
                    self.share_run(a, now);
                }
            }
        } else if !want {
            if let Some(s) = self.share.take() {
                for n in s.nodes {
                    self.scene.remove(n);
                }
            }
        }
    }

    pub(super) fn share_tick(&mut self, now: f64, wake: &mut Vec<Option<f64>>) {
        let Some(s) = self.share.as_mut() else { return };
        let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
        let waker = self.waker.clone();
        while let Ok(d) = s.rx.try_recv() {
            match d {
                Done::Prepared(id, r) => {
                    if s.page.st.selected.as_deref() == Some(id.as_str()) {
                        match r {
                            Ok(p) => {
                                let (tx, w, size, dur) = (s.tx.clone(), waker.clone(), p.quilt.len() as u64, s.page.st.duration);
                                s.prepared = Some(p);
                                std::thread::spawn(move || {
                                    let _ = tx.send(Done::Priced(id, dur, share_up::price(size, dur)));
                                    w();
                                });
                            }
                            Err(e) => s.page.st.error = Some(e),
                        }
                    }
                    s.page.changed(&mut cx, now);
                }
                Done::Priced(id, dur, p) => {
                    if s.page.st.duration == dur {
                        s.page.st.priced(&id, p.size, p.cost, p.sponsor);
                    }
                    s.page.changed(&mut cx, now);
                }
                Done::Step(step) => {
                    if matches!(s.page.st.phase, Phase::Uploading(_)) {
                        s.page.st.phase = Phase::Uploading(step);
                        s.page.changed(&mut cx, now);
                    }
                }
                Done::Uploaded(r) => {
                    let r = r.map(|res| {
                        let copied = share_up::copy_text(&res.link);
                        (res, copied)
                    });
                    s.page.st.finished(r);
                    s.page.changed(&mut cx, now);
                }
                Done::Toast(t, tone) => self.st.toast(t, tone, now),
                Done::Shot(r) | Done::Clip(r) => {
                    s.page.st.taking = None;
                    match r {
                        Ok(c) => {
                            s.page.st.added(c);
                            self.st.toast("Saved to Share", Tone::Info, now);
                        }
                        Err(e) => {
                            s.page.st.error = Some(e);
                        }
                    }
                    s.page.changed(&mut cx, now);
                }
            }
        }
        s.page.tick(&mut cx, now);
        wake.push(s.page.next_deadline());
    }

    /// "Send to a friend" on a shared capture: straight to the friend Friends picked, else Friends
    /// opens on a picker.
    fn share_send(&mut self) {
        let now = clock::secs();
        let Some(s) = self.share.as_ref() else { return };
        let Phase::Done { result, .. } = &s.page.st.phase else { return };
        let link = result.link.clone();
        let kind = crate::screens::share::format::kind_word(s.page.st.selected_capture().map(|c| c.kind).unwrap_or(rare_share::Kind::Screenshot)).to_string();
        let Some(social) = self.social.clone() else {
            self.st.toast("Sending to friends needs the live launcher.", Tone::Info, now);
            return;
        };
        match crate::screens::share::send_to() {
            Some((addr, _)) => {
                crate::screens::share::set_send_to(None);
                social.send_capture(addr, kind, link);
                self.st.close();
            }
            None => self.show_friends(Some(rare_friends::nav::FriendsTarget::Send { url: link, kind })),
        }
    }

    /// A press while Share is up. Always handled.
    pub(super) fn share_pad(&mut self, b: Btn) {
        use crate::screens::overlay::Out;
        let now = clock::secs();
        let pad = match b {
            Btn::Up => Pad::Up,
            Btn::Down => Pad::Down,
            Btn::Left => Pad::Left,
            Btn::Right => Pad::Right,
            Btn::A => Pad::A,
            Btn::B => Pad::B,
            Btn::X => Pad::X,
            Btn::Guide => {
                self.st.close();
                return;
            }
            _ => return,
        };
        let out = {
            let Some(s) = self.share.as_mut() else { return };
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            s.page.press(&mut cx, pad, now)
        };
        let Out::Act(a) = out else {
            if out == Out::Close {
                let game = self.share.as_ref().and_then(|s| s.game.clone());
                self.st.close();
                // Opened from a game page's screenshots: back to that page.
                match game.as_deref() {
                    Some("overlay:year") => self.st.open(Some(Overlay::Other(crate::features::stats::YEAR))),
                    Some(g) => self.st.open(Some(Overlay::Game(g.to_string()))),
                    None => {}
                }
                self.fx.fb(Feedback::Back);
            }
            return;
        };
        self.share_run(a, now);
    }

    /// Runs one of Share's acts: a press, or a capture a game page asked to share.
    pub(super) fn share_run(&mut self, a: String, now: f64) {
        if a == "share:send" {
            self.share_send();
            return;
        }
        let host = self.share_host();
        let Some(s) = self.share.as_mut() else { return };
        let tx = s.tx.clone();
        let waker = self.waker.clone();
        let (running, titles) = (host.running.clone(), host.titles.clone());
        let mk = move || NativeShare { running, titles };
        let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
        match a.as_str() {
            "share:screenshot" => {
                std::thread::spawn(move || {
                    let _ = tx.send(Done::Shot(rare_share::take_screenshot(&mk())));
                    waker();
                });
            }
            "share:clip" => {
                std::thread::spawn(move || {
                    let _ = tx.send(Done::Clip(rare_share::save_clip(&mk())));
                    waker();
                });
            }
            "share:clips:on" | "share:clips:off" => {
                let msg = crate::features::capture::set_replay(a == "share:clips:on");
                s.page.st.status = Some(rare_share::status(&host));
                s.page.changed(&mut cx, now);
                self.st.toast(&msg, Tone::Info, now);
            }
            a if a.starts_with("share:open:") => {
                let id = a["share:open:".len()..].to_string();
                s.prepared = None;
                let Some(c) = s.page.st.captures.as_ref().and_then(|l| l.iter().find(|c| c.id == id)).cloned() else { return };
                let catalog: Vec<_> = self.st.sui_games.iter().map(|g| format::SuiGame { slug: g.slug.clone(), name: g.name.clone() }).collect();
                let lib: Vec<_> = self.st.games().iter().map(|g| format::LibGame { id: g.id.clone(), title: g.title.clone(), source: g.source.clone(), installed: g.installed, url: None }).collect();
                let slug = format::slug_for(c.game.as_ref(), &catalog, Some(&lib));
                std::thread::spawn(move || {
                    let _ = tx.send(Done::Prepared(id, share_up::prepare(&c, slug)));
                    waker();
                });
            }
            a if a.starts_with("share:duration:") => {
                let Some(p) = s.prepared.as_ref().filter(|p| s.page.st.selected.as_deref() == Some(p.id.as_str())) else { return };
                let dur = s.page.st.duration;
                let (id, size) = (p.id.clone(), p.quilt.len() as u64);
                std::thread::spawn(move || {
                    let _ = tx.send(Done::Priced(id, dur, share_up::price(size, dur)));
                    waker();
                });
            }
            "share:go" => {
                let Some(p) = s.prepared.clone().filter(|p| s.page.st.selected.as_deref() == Some(p.id.as_str())) else {
                    s.page.st.finished(Err((false, "This capture is not ready to share yet. Try again.".into())));
                    s.page.changed(&mut cx, now);
                    return;
                };
                let kind = s.page.st.selected_capture().map(|c| c.kind).unwrap_or(rare_share::Kind::Screenshot);
                let (dur, sponsored) = (s.page.st.duration, s.page.st.sponsored());
                let t2 = tx.clone();
                let w2 = waker.clone();
                std::thread::spawn(move || {
                    let on_step = move |st: Step| {
                        let _ = t2.send(Done::Step(st));
                        w2();
                    };
                    let r = share_up::upload(&p, kind, dur, sponsored, &on_step);
                    let _ = tx.send(Done::Uploaded(r));
                    waker();
                });
                s.page.changed(&mut cx, now);
            }
            "share:copy" => {
                if let Phase::Done { result, .. } = &s.page.st.phase {
                    let ok = share_up::copy_text(&result.link);
                    s.page.st.copied(ok);
                    s.page.changed(&mut cx, now);
                }
            }
            "share:telegram" => {
                if let (Phase::Done { result, .. }, Some(post)) = (&s.page.st.phase, format::telegram_poster()) {
                    let link = result.link.clone();
                    std::thread::spawn(move || {
                        if post(&link).is_err() {
                            let _ = tx.send(Done::Toast("Couldn't open Telegram.".into(), Tone::Error));
                            waker();
                        }
                    });
                }
            }
            a if a.starts_with("share:delete:") => {
                let id = &a["share:delete:".len()..];
                match rare_share::delete(id) {
                    Ok(()) => s.page.st.removed(id),
                    Err(e) => s.page.st.error = Some(e),
                }
                s.page.changed(&mut cx, now);
            }
            _ => {}
        }
    }
}
