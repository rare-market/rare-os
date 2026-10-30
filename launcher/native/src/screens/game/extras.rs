//! The game page under the fold: Rare's screenshots of the game (a full-screen viewer with delete
//! and Share), its news and patch notes (a reader paragraph by paragraph), its DLC (installed,
//! turned off, or in the store; A turns owned DLC on or off) and its manual. Everything is read off
//! the UI thread (`extras_data`) and kept, so a revisit shows the last answer at once. The sections
//! sit below the hero, so nothing above moves when they land.

use super::extras_data::{self as data, Dlc, NewsItem, ParaKind, Remote, Source};
use crate::act::Act;
use crate::data::model::Game;
use crate::kit::{self, s, tok, ArtFit, Btn as B, BtnIcon, Ty, Ui, T};
use crate::page::{Acts, Btn, Feedback, Fx};
use crate::state::{Overlay, State, Tone};
use rare_library::steam_dlc::DlcState;
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::Focusable;
use rare_render::view::{key, El, Motion};
use rare_share::Capture;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// `Act::Local` codes (panels use 1..400, settings.rs 1000..1300).
const SHOT: u32 = 10_000;
const NEWS: u32 = 20_000;
const DLC: u32 = 30_000;
const MANUAL: u32 = 40_000;
const STILL: u32 = 40_001;
const PARA: u32 = 50_000;
pub const FIRST: u32 = SHOT;

#[derive(Default)]
struct Inner {
    src: Option<Source>,
    remote: Option<Remote>,
    dlc: DlcState,
    shots: Vec<Capture>,
    /// The DLC being turned on or off.
    busy: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Page,
    Viewer(usize),
    Reader(usize),
}

impl Default for Mode {
    fn default() -> Mode {
        Mode::Page
    }
}

/// The page's extras: the shared answers plus the viewer or reader on top.
#[derive(Default)]
pub struct Extras {
    inner: Arc<Mutex<Inner>>,
    ver: Arc<AtomicU64>,
    mode: Mode,
    /// X pressed once in the viewer (second press within 3 s deletes).
    armed: Option<f64>,
    /// The tile to focus when the viewer or reader closes (its code).
    back_to: Option<u32>,
    /// That tile's tag in the last build.
    focus_tag: Option<u32>,
}

/// A Share opened from the game page: the capture to open, and the game to come back to.
static SHARE_FROM_GAME: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Opens Share on a capture made elsewhere (the year card); B comes back to `back` (a game id, or
/// `overlay:year`).
pub fn share_capture(id: String, back: String) {
    *SHARE_FROM_GAME.lock().unwrap() = Some((id, back));
}

/// Share asks once it is up: the capture to open at once and the game page to go back to.
pub fn take_share_open() -> Option<(String, String)> {
    SHARE_FROM_GAME.lock().unwrap().take()
}

fn steam_appid(g: &Game) -> Option<u32> {
    (g.source == "steam").then(|| g.id.strip_prefix("steam:")?.parse().ok()).flatten()
}

/// The rare.market slug for a library game that is a Sui game (a web game from the catalog).
fn sui_slug(st: &State, g: &Game) -> Option<String> {
    let bare = g.id.strip_prefix("web:").unwrap_or(&g.id);
    if st.sui_games.iter().any(|s| s.slug == bare) {
        return Some(bare.to_string());
    }
    let url = g.launch.as_ref().and_then(|l| l.url.clone()).unwrap_or_default();
    rare_library::profiles::for_game(&g.id, &url).map(|(slug, _)| slug).filter(|s| st.sui_games.iter().any(|g| &g.slug == s))
}

fn fixture() -> bool {
    std::env::var("RR_FIXTURE").is_ok()
}

impl Extras {
    pub fn ver(&self) -> u64 {
        crate::page::hash((self.ver.load(Ordering::Relaxed), self.mode == Mode::Page, format!("{:?}", self.mode), self.armed.is_some()))
    }

    fn bump(ver: &AtomicU64) {
        ver.fetch_add(1, Ordering::Relaxed);
    }

    /// Whether the viewer or reader has the screen.
    pub fn open(&self) -> bool {
        self.mode != Mode::Page
    }

    /// First look at the page: the kept answer at once, then the network and the disk.
    pub fn load(&self, st: &State, fx: &mut Fx, g: &Game) {
        let src = match steam_appid(g) {
            Some(a) => Some(Source::Steam(a)),
            _ => sui_slug(st, g).map(Source::Sui),
        };
        {
            let mut i = self.inner.lock().unwrap();
            i.src = src.clone();
            if let Some(r) = data::cached(&g.id) {
                i.remote = Some(r);
            }
        }
        Self::bump(&self.ver);
        // Fixtures stay offline unless a shot asks for the live answers (RR_GAME_EXTRAS=1).
        if fixture() && std::env::var("RR_GAME_EXTRAS").is_err() {
            return;
        }
        let (inner, ver, id) = (self.inner.clone(), self.ver.clone(), g.id.clone());
        let stale = self.inner.lock().unwrap().remote.as_ref().map_or(true, |r| !data::fresh(r));
        fx.spawn(move |_| {
            let shots = shots_of(&id);
            let dlc = match &src {
                Some(Source::Steam(a)) => rare_library::steam_dlc::state(*a),
                _ => DlcState::default(),
            };
            {
                let mut i = inner.lock().unwrap();
                i.shots = shots;
                i.dlc = dlc;
            }
            Self::bump(&ver);
            if let (Some(src), true) = (src, stale) {
                let r = data::fetch(&id, &src);
                inner.lock().unwrap().remote = Some(r);
                Self::bump(&ver);
            }
            Box::new(|_, _| {})
        });
    }

    fn reload_shots(&self, fx: &mut Fx, id: &str) {
        let (inner, ver, id) = (self.inner.clone(), self.ver.clone(), id.to_string());
        fx.spawn(move |_| {
            let shots = shots_of(&id);
            inner.lock().unwrap().shots = shots;
            Self::bump(&ver);
            Box::new(|_, _| {})
        });
    }

    // ------------------------------------------------------------ presses

    pub fn act(&mut self, st: &mut State, fx: &mut Fx, g: &Game, code: u32) {
        let now = rare_render::clock::secs();
        match code {
            c if (SHOT..NEWS).contains(&c) => {
                if (c - SHOT) < self.inner.lock().unwrap().shots.len() as u32 {
                    self.back_to = Some(c);
                    self.mode = Mode::Viewer((c - SHOT) as usize);
                    self.armed = None;
                    fx.fb(Feedback::Open);
                }
            }
            c if (NEWS..DLC).contains(&c) => {
                if self.inner.lock().unwrap().remote.as_ref().is_some_and(|r| ((c - NEWS) as usize) < r.news.len()) {
                    self.back_to = Some(c);
                    self.mode = Mode::Reader((c - NEWS) as usize);
                    fx.fb(Feedback::Open);
                }
            }
            c if (DLC..MANUAL).contains(&c) => self.dlc_press(st, fx, g, (c - DLC) as usize, now),
            MANUAL => {
                let url = self.inner.lock().unwrap().remote.as_ref().and_then(|r| r.manual.clone());
                if let Some(u) = url {
                    st.toast("Opening the manual", Tone::Info, now);
                    fx.run(Act::OpenWeb(u));
                }
            }
            _ => {}
        }
        Self::bump(&self.ver);
    }

    fn dlc_press(&mut self, st: &mut State, fx: &mut Fx, g: &Game, i: usize, now: f64) {
        let Some(appid) = steam_appid(g) else { return };
        let (d, owned, off, busy) = {
            let inn = self.inner.lock().unwrap();
            let Some(d) = inn.remote.as_ref().and_then(|r| r.dlc.get(i)).cloned() else { return };
            (d.clone(), inn.dlc.switchable(d.appid), inn.dlc.disabled.contains(&d.appid), inn.busy)
        };
        if busy.is_some() {
            return;
        }
        if !owned {
            // Not owned, or an unlock with no files to switch: its page in Steam.
            st.handoff_until = Some(now + 3.0);
            let id = d.appid;
            fx.spawn(move |_| {
                let r = rare_launch::api::steam_store(&crate::features::bus::ctx(), crate::data::backend::runner(), id);
                Box::new(move |st: &mut State, now| {
                    if let Err(e) = r {
                        log::warn!("steam store {id}: {e}");
                        st.handoff_until = None;
                        st.toast("Couldn't open Steam. Try again.", Tone::Error, now);
                    }
                })
            });
            return;
        }
        if st.is_running(&g.id) {
            st.toast("Close the game first to change its DLC.", Tone::Info, now);
            return;
        }
        let on = off;
        self.inner.lock().unwrap().busy = Some(d.appid);
        fx.fb(Feedback::Toggle);
        let (inner, ver, name) = (self.inner.clone(), self.ver.clone(), d.name.clone());
        let restart = crate::features::config::get("steamStart").and_then(|v| v.as_str().map(String::from)).as_deref() != Some("demand");
        fx.spawn(move |_| {
            let r = if fixture() { Ok(()) } else { rare_launch::api::dlc_set(crate::data::backend::runner(), appid, d.appid, on, restart).map_err(|e| e.to_string()) };
            let fresh = rare_library::steam_dlc::state(appid);
            {
                let mut i = inner.lock().unwrap();
                i.busy = None;
                if !fixture() {
                    i.dlc = fresh;
                }
            }
            Self::bump(&ver);
            Box::new(move |st: &mut State, now| match r {
                Ok(()) => st.toast(if on { format!("{name} is on") } else { format!("{name} is off") }, Tone::Info, now),
                Err(e) => {
                    log::warn!("dlc {appid}/{}: {e}", d.appid);
                    st.toast(e, Tone::Error, now);
                }
            })
        });
    }

    /// Presses while the viewer or reader is up (all of them), and Y on a screenshot tile.
    pub fn pad(&mut self, st: &mut State, fx: &mut Fx, g: &Game, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        let now = rare_render::clock::secs();
        match self.mode {
            Mode::Page => {
                if b == Btn::Y {
                    if let Some((_, Some(Act::Local(c)))) = cur {
                        if (SHOT..NEWS).contains(c) {
                            self.share(st, fx, g, (*c - SHOT) as usize);
                            return true;
                        }
                    }
                }
                false
            }
            Mode::Viewer(i) => {
                let n = self.inner.lock().unwrap().shots.len();
                match b {
                    Btn::B => self.close(fx),
                    Btn::Left | Btn::Lb if i > 0 => {
                        self.mode = Mode::Viewer(i - 1);
                        self.armed = None;
                        fx.fb(Feedback::Move);
                    }
                    Btn::Right | Btn::Rb if i + 1 < n => {
                        self.mode = Mode::Viewer(i + 1);
                        self.armed = None;
                        fx.fb(Feedback::Move);
                    }
                    Btn::X => {
                        if self.armed.is_some_and(|t| now - t < 3.0) {
                            self.delete(st, fx, g, i);
                        } else {
                            self.armed = Some(now);
                            fx.fb(Feedback::Toggle);
                        }
                    }
                    Btn::Y => self.share(st, fx, g, i),
                    Btn::Guide | Btn::GuideHold | Btn::Qam | Btn::ExitHold => return false,
                    _ => {}
                }
                Self::bump(&self.ver);
                true
            }
            Mode::Reader(_) => match b {
                Btn::B => {
                    self.close(fx);
                    Self::bump(&self.ver);
                    true
                }
                Btn::Y => {
                    if let Some(u) = self.reading().map(|n| n.url).filter(|u| !u.is_empty()) {
                        fx.run(Act::OpenWeb(u));
                    }
                    true
                }
                _ => false,
            },
        }
    }

    fn close(&mut self, fx: &mut Fx) {
        self.mode = Mode::Page;
        self.armed = None;
        fx.fb(Feedback::Back);
    }

    fn reading(&self) -> Option<NewsItem> {
        let Mode::Reader(i) = self.mode else { return None };
        self.inner.lock().unwrap().remote.as_ref()?.news.get(i).cloned()
    }

    fn delete(&mut self, st: &mut State, fx: &mut Fx, g: &Game, i: usize) {
        let now = rare_render::clock::secs();
        self.armed = None;
        let Some(c) = self.inner.lock().unwrap().shots.get(i).cloned() else { return };
        match if fixture() { Ok(()) } else { rare_share::delete(&c.id) } {
            Ok(()) => {
                let left = {
                    let mut inn = self.inner.lock().unwrap();
                    inn.shots.retain(|s| s.id != c.id);
                    inn.shots.len()
                };
                st.toast("Screenshot deleted", Tone::Info, now);
                if left == 0 {
                    self.mode = Mode::Page;
                    self.back_to = None;
                } else {
                    self.mode = Mode::Viewer(i.min(left - 1));
                    self.back_to = Some(SHOT + i.min(left - 1) as u32);
                }
                self.reload_shots(fx, &g.id);
            }
            Err(e) => {
                log::warn!("delete {}: {e}", c.id);
                st.toast("Couldn't delete that screenshot.", Tone::Error, now);
            }
        }
    }

    fn share(&mut self, _st: &mut State, fx: &mut Fx, g: &Game, i: usize) {
        let Some(c) = self.inner.lock().unwrap().shots.get(i).cloned() else { return };
        *SHARE_FROM_GAME.lock().unwrap() = Some((c.id, g.id.clone()));
        self.mode = Mode::Page;
        self.back_to = Some(SHOT + i as u32);
        fx.fb(Feedback::Open);
        fx.run(Act::Open(Overlay::Other("share")));
    }

    pub fn hints(&self) -> Option<Vec<Hint>> {
        match self.mode {
            Mode::Page => None,
            Mode::Viewer(_) => Some(vec![
                hint(Key::Lb, "Previous"),
                hint(Key::Rb, "Next"),
                hint(Key::X, if self.armed.is_some() { "Press again to delete" } else { "Delete" }),
                hint(Key::Y, "Share"),
                hint(Key::B, "Back"),
            ]),
            Mode::Reader(_) => {
                let mut v = Vec::new();
                if self.reading().is_some_and(|n| !n.url.is_empty()) {
                    v.push(hint(Key::Y, "Open in browser"));
                }
                v.push(hint(Key::B, "Back"));
                Some(v)
            }
        }
    }

    /// After a build: back on the tile the viewer or reader was opened from.
    pub fn take_focus(&mut self) -> Option<u32> {
        if self.mode != Mode::Page {
            return None;
        }
        self.back_to.take()?;
        self.focus_tag.take()
    }

    // ------------------------------------------------------------ views

    /// The viewer or reader, when one is up (it takes the whole page).
    pub fn cover(&mut self, ui: &mut Ui, acts: &mut Acts, g: &Game) -> Option<El> {
        match self.mode {
            Mode::Page => None,
            Mode::Viewer(i) => {
                let n = self.inner.lock().unwrap().shots.len();
                let Some(c) = self.inner.lock().unwrap().shots.get(i).cloned() else {
                    self.mode = Mode::Page;
                    return None;
                };
                Some(viewer(ui, acts, g, &c, i, n, self.armed.is_some()))
            }
            Mode::Reader(_) => match self.reading() {
                Some(item) => Some(reader(ui, acts, &item)),
                None => {
                    self.mode = Mode::Page;
                    None
                }
            },
        }
    }

    /// The sections under the hero (None when there is nothing to show).
    pub fn sections(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts, g: &Game) -> Option<El> {
        let inn = self.inner.lock().unwrap();
        let mut col: Vec<El> = Vec::new();
        let mut focus_tag = None;
        let back = self.back_to;
        let mut tag = |acts: &mut Acts, code: u32| {
            let t = acts.game(Act::Local(code), &g.id);
            if back == Some(code) {
                focus_tag = Some(t);
            }
            t
        };
        // Screenshots.
        if !inn.shots.is_empty() {
            let tiles: Vec<El> = inn
                .shots
                .iter()
                .enumerate()
                .take(60)
                .map(|(i, c)| {
                    let t = tag(acts, SHOT + i as u32);
                    let pic = kit::art(ui, s().fill(), &[c.path.clone()], 16.0, 9.0, ArtFit::Cover(0.5), None);
                    let b = ui.bx(s().wh(16.0, 9.0)).bg(tok::L2).radius(ui.r(0.5)).clip().child(pic);
                    kit::nav(kit::tile(b, 1.06), t, Some("View"), None, false).key(key(&c.id))
                })
                .collect();
            let n = inn.shots.len().to_string();
            col.push(kit::rail(ui, s(), "game-shots", Some("Screenshots"), Some(&n), tiles).key(key("game-shots")));
        }
        if let Some(r) = inn.remote.as_ref() {
            // News.
            if !r.news.is_empty() {
                let label = if matches!(inn.src, Some(Source::Sui(_))) { "Updates" } else { "News" };
                let cards: Vec<El> = r.news.iter().enumerate().map(|(i, n)| news_card(ui, acts, &mut tag, i, n)).collect();
                col.push(kit::rail(ui, s(), "game-news", Some(label), None, cards).key(key("game-news")));
            }
            // DLC.
            if !r.dlc.is_empty() && steam_appid(g).is_some() {
                let owned = r.dlc.iter().filter(|d| inn.dlc.owned(d.appid)).count();
                let note = if owned > 0 { format!("{owned} of {} owned", r.dlc.len()) } else { format!("{} in the store", r.dlc.len()) };
                let mut list: Vec<(usize, &Dlc)> = r.dlc.iter().enumerate().collect();
                // Owned first, then the store's order.
                list.sort_by_key(|(i, d)| (!inn.dlc.owned(d.appid), *i));
                let running = st.is_running(&g.id);
                let cards: Vec<El> = list.into_iter().map(|(i, d)| dlc_card(ui, acts, &mut tag, i, d, &inn.dlc, inn.busy, running)).collect();
                col.push(kit::rail(ui, s(), "game-dlc", Some("DLC"), Some(&note), cards).key(key("game-dlc")));
            }
            // Manual.
            if r.manual.is_some() {
                let t = tag(acts, MANUAL);
                let c = ui
                    .bx(s().col().w(20.0).h(9.0).p(1.0).gap(0.4))
                    .bg(tok::L1)
                    .radius(ui.r(0.5))
                    .child(kit::icon(ui, "book-open", 1.4, tok::fg(0.8)))
                    .child(ui.text_s(s().mt_auto(), "Game manual", T::new(Ty::Body).w(600)))
                    .child(ui.text("Opens in the browser", T::new(Ty::Meta).c(tok::MUTED)));
                col.push(kit::rail(ui, s(), "game-guides", Some("Manual"), None, vec![kit::nav(kit::tile(c, 1.03), t, Some("Read"), None, false)]).key(key("game-guides")));
            }
        }
        drop(inn);
        self.focus_tag = focus_tag;
        (!col.is_empty()).then(|| ui.bx(s().col().gap(2.25).pt(0.5).pb(4.75)).kids(col))
    }
}

fn shots_of(id: &str) -> Vec<Capture> {
    let mut v: Vec<Capture> = rare_share::list().into_iter().filter(|c| c.kind == rare_share::Kind::Screenshot && c.game.as_ref().is_some_and(|g| g.id == id)).collect();
    v.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
    v
}

fn news_card(ui: &mut Ui, acts: &mut Acts, tag: &mut impl FnMut(&mut Acts, u32) -> u32, i: usize, n: &NewsItem) -> El {
    let t = tag(acts, NEWS + i as u32);
    let head = format!("{} \u{b7} {}", n.feed, data::date_label(n.date));
    let c = ui
        .bx(s().col().w(22.0).h(9.0).p(1.0).gap(0.35))
        .bg(tok::L1)
        .radius(ui.r(0.5))
        .child(ui.text(head.trim_end_matches(" \u{b7} ").to_string(), T::new(Ty::Label).w(500).c(tok::MUTED).trunc()))
        .child(ui.text_s(s().wp(1.0), n.title.clone(), T::new(Ty::Body).w(600).wrap().clamp(3)));
    kit::nav(kit::tile(c, 1.03), t, Some("Read"), None, false).key(key(&n.id))
}

#[allow(clippy::too_many_arguments)]
fn dlc_card(ui: &mut Ui, acts: &mut Acts, tag: &mut impl FnMut(&mut Acts, u32) -> u32, i: usize, d: &Dlc, st: &DlcState, busy: Option<u32>, running: bool) -> El {
    let t = tag(acts, DLC + i as u32);
    let owned = st.owned(d.appid);
    let off = st.disabled.contains(&d.appid);
    let (line, a) = if busy == Some(d.appid) {
        ("Changing, Steam restarts".to_string(), None)
    } else if off {
        ("Turned off".to_string(), (!running).then_some("Turn on"))
    } else if st.switchable(d.appid) {
        ("Installed".to_string(), (!running).then_some("Turn off"))
    } else if owned {
        ("Owned".to_string(), Some("View in Steam"))
    } else {
        (d.price.clone().unwrap_or_else(|| "In the Steam store".into()), Some("View in Steam"))
    };
    let pic = kit::art(ui, s().fill(), &d.art, 18.0, 8.4, ArtFit::Cover(0.5), Some(ui.bx(s().fill()).bg(tok::L2)));
    let mut art = ui.bx(s().wh(18.0, 8.4)).bg(tok::L2).clip().child(pic);
    if off {
        art = art.child(ui.bx(s().fill()).bg(tok::BG.alpha(0.55)));
    }
    let tone = if owned && !off { tok::fg(0.85) } else { tok::MUTED };
    let c = ui
        .bx(s().col().w(18.0))
        .bg(tok::L1)
        .radius(ui.r(0.5))
        .clip()
        .child(art)
        .child(ui.bx(s().col().px(0.9).py(0.7).gap(0.2)).child(ui.text(d.name.clone(), T::new(Ty::Meta).w(600).trunc())).child(ui.text(line, T::new(Ty::Label).c(tone).trunc())));
    kit::nav(kit::tile(c, 1.03), t, a, None, false).key(key(&d.appid))
}

fn viewer(ui: &mut Ui, acts: &mut Acts, g: &Game, c: &Capture, i: usize, n: usize, armed: bool) -> El {
    let (w, h) = (ui.w / ui.rem, ui.h / ui.rem);
    let pic = kit::art(ui, s().fill(), &[c.path.clone()], w, h - 3.0, ArtFit::Contain(0.5, 0.5), None);
    let when = data::date_label((c.created_ms / 1000) as i64);
    let line = format!("{} \u{b7} {} of {n}{}", g.title, i + 1, if when.is_empty() { String::new() } else { format!(" \u{b7} {when}") });
    let mut cap = ui.bx(s().abs(None, None, Some(0.75), Some(2.5)).row().items_center().gap(0.75).px(1.0).py(0.5)).bg(tok::BG.alpha(0.75)).radius(ui.r(0.5)).child(ui.text(line, T::new(Ty::Small).w(500).c(tok::fg(0.85))));
    if armed {
        cap = cap.child(ui.text("Press X again to delete", T::new(Ty::Small).w(600).c(tok::ATTN)));
    }
    // The caption sits inside the focused picture so it draws over it.
    let t = acts.game(Act::Local(STILL), &g.id);
    let still = kit::nav(ui.bx(s().abs(Some(0.0), Some(0.0), Some(3.0), Some(0.0))).child(pic).child(cap), t, None, None, true);
    ui.bx(s().fill()).bg(tok::BLACK).enter(Motion::FADE).child(still)
}

fn reader(ui: &mut Ui, acts: &mut Acts, n: &NewsItem) -> El {
    let head = format!("{} \u{b7} {}", n.feed, data::date_label(n.date));
    let mut col = ui
        .bx(s().col().gap(0.5).wp(1.0).max_w(52.0))
        .child(ui.text(head.trim_end_matches(" \u{b7} ").to_string(), T::new(Ty::Small).w(500).c(tok::MUTED)))
        .child(ui.text_s(s().wp(1.0).mb(1.0), n.title.clone(), T::new(Ty::Px(2.0, 2.5)).w(700).wrap()));
    if n.body.is_empty() {
        let t = acts.tag(Act::Local(PARA));
        col = col.child(kit::nav(kit::row_look(ui.bx(s().px(0.75).py(0.5)).radius(ui.r(0.5)).child(ui.text_s(s().wp(1.0), "This post has no text here. Y opens it in the browser.", T::new(Ty::Body).c(tok::MUTED).wrap()))), t, None, None, true));
    }
    for (k, p) in n.body.iter().enumerate().take(400) {
        let t = acts.tag(Act::Local(PARA + k as u32));
        let (txt, look) = match p.kind {
            ParaKind::Head => (p.text.clone(), T::new(Ty::Row).w(600).wrap()),
            ParaKind::Bullet => (format!("\u{2022}  {}", p.text), T::new(Ty::Body).c(tok::fg(0.85)).wrap()),
            ParaKind::Text => (p.text.clone(), T::new(Ty::Body).c(tok::fg(0.85)).wrap().lead(1.6)),
        };
        let mt = if p.kind == ParaKind::Head && k > 0 { 0.75 } else { 0.0 };
        let el = ui.bx(s().px(0.75).py(0.4).mt(mt).wp(1.0)).radius(ui.r(0.5)).child(ui.text_s(s().wp(1.0), txt, look));
        col = col.child(kit::nav(kit::row_look(el), t, None, None, k == 0));
    }
    if !n.url.is_empty() {
        let b = kit::button(ui, B::new("Open in browser", acts.tag(Act::OpenWeb(n.url.clone()))).ghost().icon(BtnIcon::Lucide("external-link", 1.0, false)));
        col = col.child(ui.bx(s().row().mt(1.25)).child(b));
    }
    ui.bx(s().fill())
        .bg(tok::BG)
        .enter(Motion::FADE)
        .child(ui.bx(s().fill().col().items_center().px(2.5).pt(2.5).pb(5.0)).scrolls_y(key(&("news-reader", &n.id))).child(col))
}
