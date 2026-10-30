//! A game's art page: Cover, Hero and Logo (LB/RB), each a grid of choices found by the game's
//! name (SteamGridDB with a key, else Steam's store), a picture from this device (the file
//! picker) and Use the original. A picks; the choice is kept for the game (rare-library
//! custom.rs) and shows at once. B goes back to the game.

use super::art::{Choice, Origin};
use crate::act::Act;
use crate::kit::{self, s, tok, ArtFit, Pb, TabItem, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{Overlay, State, Tone};
use rare_library::custom::{ArtKind, Custom};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El, Motion};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const KINDS: [ArtKind; 3] = [ArtKind::Cover, ArtKind::Hero, ArtKind::Logo];

static TARGET: Mutex<Option<(String, ArtKind)>> = Mutex::new(None);

/// Opens the art page for a game at one kind of picture.
pub fn open(st: &mut State, id: &str, kind: ArtKind) {
    *TARGET.lock().unwrap() = Some((id.to_string(), kind));
    st.open(Some(Overlay::Other(super::ART)));
}

/// Back to the art page (from the file picker), at the kind it was on.
pub fn reopen(st: &mut State, id: &str) {
    let kind = TARGET.lock().unwrap().as_ref().filter(|t| t.0 == id).map(|t| t.1).unwrap_or(ArtKind::Cover);
    open(st, id, kind);
}

pub fn target() -> (String, ArtKind) {
    TARGET.lock().unwrap().clone().unwrap_or((String::new(), ArtKind::Cover))
}

fn what(kind: ArtKind) -> &'static str {
    match kind {
        ArtKind::Cover => "cover",
        ArtKind::Hero => "hero image",
        ArtKind::Logo => "logo",
    }
}

/// Keeps `url` (None: the original) as one of the game's pictures.
pub fn choose(st: &mut State, fx: &mut Fx, id: &str, kind: ArtKind, url: impl Into<Option<String>>) {
    let url: Option<String> = url.into();
    let id = id.to_string();
    let _ = st;
    fx.spawn(move |b| {
        let mut c = b.game_custom(&id);
        *kind.slot(&mut c) = url.clone();
        let r = b.set_game_custom(&id, c.clone());
        Box::new(move |st: &mut State, now| match r {
            Ok(()) => {
                *MINE.lock().unwrap() = Some((id.clone(), c.clone()));
                MINE_VER.fetch_add(1, Ordering::Relaxed);
                if url.is_some() {
                    super::show_custom(st, &id, &c);
                    st.toast(format!("New {} kept for this game", what(kind)), Tone::Info, now);
                } else {
                    // The original comes back with the next scan.
                    st.rescan_at = Some(now);
                    st.toast(format!("The original {} is back", what(kind)), Tone::Info, now);
                }
            }
            Err(e) => {
                log::warn!("art: {e}");
                st.toast("Couldn't keep that picture.", Tone::Error, now);
            }
        })
    });
}

/// The game's own choices, as last saved (the "In use" mark).
static MINE: Mutex<Option<(String, Custom)>> = Mutex::new(None);
static MINE_VER: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Loaded {
    by_kind: HashMap<KindKey, Result<(Origin, Vec<Choice>), String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct KindKey(u8);

fn kk(k: ArtKind) -> KindKey {
    KindKey(KINDS.iter().position(|x| *x == k).unwrap_or(0) as u8)
}

pub struct ArtPage {
    id: String,
    kind: ArtKind,
    loaded: Arc<Mutex<Loaded>>,
    ver: Arc<AtomicU64>,
    asked: Vec<KindKey>,
}

const ACT_DEVICE: u32 = 0;
const ACT_ORIGINAL: u32 = 1;
const ACT_CHOICE: u32 = 10;

impl ArtPage {
    pub fn new((id, kind): (String, ArtKind)) -> ArtPage {
        ArtPage { id, kind, loaded: Default::default(), ver: Default::default(), asked: vec![] }
    }

    fn ask(&mut self, st: &State, fx: &mut Fx) {
        let k = kk(self.kind);
        if self.asked.contains(&k) {
            return;
        }
        let Some(g) = st.game(&self.id) else { return };
        self.asked.push(k);
        let (title, kind, id) = (g.title.clone(), self.kind, self.id.clone());
        let appid = g.id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok());
        let (loaded, ver) = (self.loaded.clone(), self.ver.clone());
        let first = self.asked.len() == 1;
        fx.spawn(move |b| {
            let r = b.art_choices(&title, appid, kind);
            let mine = first.then(|| b.game_custom(&id));
            Box::new(move |_st: &mut State, _| {
                if let Some(c) = mine {
                    *MINE.lock().unwrap() = Some((id, c));
                }
                loaded.lock().unwrap().by_kind.insert(kk(kind), r);
                ver.fetch_add(1, Ordering::Relaxed);
            })
        });
    }

    fn mine(&self) -> Option<String> {
        let m = MINE.lock().unwrap();
        let (id, c) = m.as_ref()?;
        if *id != self.id {
            return None;
        }
        let mut c = c.clone();
        self.kind.slot(&mut c).clone()
    }

    fn switch(&mut self, d: i32) {
        let i = KINDS.iter().position(|k| *k == self.kind).unwrap_or(0) as i32;
        self.kind = KINDS[(i + d).rem_euclid(3) as usize];
        *TARGET.lock().unwrap() = Some((self.id.clone(), self.kind));
        self.ver.fetch_add(1, Ordering::Relaxed);
    }

    /// Tile size in rem for the kind.
    fn size(&self) -> (f32, f32) {
        match self.kind {
            ArtKind::Cover => (8.0, 12.0),
            ArtKind::Hero => (21.0, 6.75),
            ArtKind::Logo => (14.0, 6.75),
        }
    }
}

impl Page for ArtPage {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.ver.load(Ordering::Relaxed), MINE_VER.load(Ordering::Relaxed), kk(self.kind), st.ver.library))
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, _now: f64) -> Option<f64> {
        self.ask(st, fx);
        None
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let title = st.game(&self.id).map(|g| g.title.clone()).unwrap_or_default();
        let value = KINDS.iter().position(|k| *k == self.kind).unwrap_or(0);
        let items: Vec<TabItem> = KINDS.iter().map(|k| TabItem::new(super::kind_name(*k))).collect();
        let lb = kit::glyph(ui, Pb::Lb, kit::GSize::Sm);
        let rb = kit::glyph(ui, Pb::Rb, kit::GSize::Sm);
        let tabs = ui.bx(s().row().items_center().gap(1.0).h(2.5).mb(0.75)).child(lb).child(kit::tabs(ui, s(), &items, value, true)).child(rb);
        let head = ui.bx(s().col().gap(0.25).mb(1.0)).child(ui.text(format!("{title} art"), T::new(Ty::Title).w(600))).child(ui.text("Pick what shows in your library and on the game's page", T::new(Ty::Small).c(tok::MUTED)));
        let loaded = self.loaded.lock().unwrap();
        let got = loaded.by_kind.get(&kk(self.kind));
        let note = match got {
            None => "Looking for art".to_string(),
            Some(Err(e)) => format!("Couldn't look for art: {e}"),
            Some(Ok((_, v))) if v.is_empty() => "Nothing found for this name. Pick a picture from this device.".into(),
            Some(Ok((Origin::SteamGridDb, _))) => "From SteamGridDB".into(),
            Some(Ok((Origin::Steam, _))) => "From Steam's store. Add a SteamGridDB key in Settings > Storage for more choices.".into(),
        };
        let note = ui.text_s(s().h(1.5).mb(0.75), note, T::new(Ty::Small).c(tok::MUTED).trunc());
        let (w, h) = self.size();
        let mine = self.mine();
        let action = |ui: &mut Ui, acts: &mut Acts, code: u32, icon: &str, label: &str, on: bool| {
            let mut b = ui.bx(s().wh(if self.kind == ArtKind::Cover { w } else { 11.0 }, h).col().items_center().justify_center().gap(0.6).p(0.75)).bg(tok::L2).radius(ui.r(0.5));
            b = b.child(kit::icon(ui, icon, 1.4, tok::ICON2)).child(ui.text(label, T::new(Ty::Small).w(500).center().wrap()));
            if on {
                b = b.child(ui.text("In use", T::new(Ty::Label).c(tok::MUTED)));
            }
            kit::nav(kit::tile(b, 1.04), acts.tag(Act::Local(code)), Some("Pick"), None, code == ACT_DEVICE).key(key(&("gm-art-act", code)))
        };
        let mut tiles = vec![action(ui, acts, ACT_DEVICE, "images", "From this device", false), action(ui, acts, ACT_ORIGINAL, "rotate-ccw", "Use the original", mine.is_none())];
        if let Some(Ok((origin, v))) = got {
            for (i, c) in v.iter().enumerate() {
                let fit = if self.kind == ArtKind::Logo { ArtFit::Contain(0.5, 0.5) } else { ArtFit::Cover(0.5) };
                let fb = ui.bx(s().fill().row().items_center().justify_center().p(0.6)).child(ui.text_s(s().wp(1.0), c.from.clone(), T::new(Ty::Small).c(tok::MUTED).center().wrap()));
                let pic = kit::art(ui, s().fill(), std::slice::from_ref(&c.thumb), w, h, fit, Some(fb));
                let logo = self.kind == ArtKind::Logo;
                let mut b = ui.bx(if logo { s().wh(w, h).p(0.75) } else { s().wh(w, h) }).bg(if logo { tok::L2 } else { tok::L3 }).radius(ui.r(0.5)).clip().child(pic);
                if mine.as_deref() == Some(c.url.as_str()) {
                    b = b.child(ui.bx(s().abs(Some(0.5), None, None, Some(0.5)).px(0.5).py(0.15)).bg(tok::BG.alpha(0.85)).radius(ui.r(0.25)).child(ui.text("In use", T::new(Ty::Label).w(500))));
                }
                let mut col = ui.bx(s().col().gap(0.35)).child(kit::nav(kit::tile(b, 1.04), acts.tag(Act::Local(ACT_CHOICE + i as u32)), Some("Pick"), None, false));
                if *origin == Origin::Steam {
                    col = col.child(ui.text_s(s().w(w), c.from.clone(), T::new(Ty::Label).c(tok::MUTED).trunc()));
                }
                tiles.push(col.key(key(&c.url)));
            }
        }
        drop(loaded);
        let grid = ui.bx(s().row().wrap().gap_xy(1.0, 1.25)).group(Group::new(&format!("gm-art-{}", value), [0.0; 4])).kids(tiles);
        ui.bx(s().col().fill().px(2.5).pt(2.0).pb(4.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key(&("gm-art", value))).child(head).child(tabs).child(note).child(grid)
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        match b {
            Btn::B => {
                fx.fb(Feedback::Back);
                st.open(Some(Overlay::Game(self.id.clone())));
                true
            }
            Btn::Lb | Btn::Rb => {
                fx.fb(Feedback::Tab);
                self.switch(if b == Btn::Lb { -1 } else { 1 });
                self.ask(st, fx);
                true
            }
            _ => false,
        }
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        match code {
            ACT_DEVICE => super::picker::open(st, super::picker::Mode::Image { id: self.id.clone(), kind: self.kind }),
            ACT_ORIGINAL => choose(st, fx, &self.id, self.kind, None),
            c => {
                let url = self.loaded.lock().unwrap().by_kind.get(&kk(self.kind)).and_then(|r| r.as_ref().ok()).and_then(|(_, v)| v.get((c - ACT_CHOICE) as usize)).map(|c| c.url.clone());
                if let Some(url) = url {
                    choose(st, fx, &self.id, self.kind, Some(url));
                }
            }
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        vec![hint(Key::LbRb, "Cover, hero, logo"), hint(Key::B, "Back")]
    }
}
