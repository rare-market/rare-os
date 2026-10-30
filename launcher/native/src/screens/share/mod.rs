//! Share (launcher/src/share): the captures grid, a capture's page with video playback, posting.
//!
//! `screen` is ShareScreen.tsx, `store` the state of share/store.ts, `format` share/format.ts with
//! the UI half of upload.ts, sponsor.ts and post.ts, `entry` entry.tsx, `video` the GStreamer
//! player and poster.ts, `qr` the link's QR. Captures are WS 8's `rare_share` types. Share is a
//! full-screen layer over the shell (`fixed inset-0 z-30`), so its pages mount on the scene root;
//! the shell's hint bar (bottom 3 rem) stays above it.

pub mod entry;
pub mod format;
pub mod qr;
pub mod screen;
pub mod store;
pub mod video;

use crate::screens::market::host::{Cx, Page, Screen};
use format::CostEstimate;
use rare_render::Id;
use rare_share::{Capture, CaptureGame, ClipStatus, Kind, ShareStatus, ShotStatus};
use screen::SharePage;
use serde::Deserialize;
use store::{Phase, ShareResult, ShareState, Step, Thumb};

/// This area's fixtures (the golden harness shoots these by default).
pub const SHOTS: &[&str] = &["share-grid", "share-detail", "share-clip", "share-uploading", "share-done", "share-clips-off", "share-empty"];

/// A page of this area by fixture name. Share covers the screen, so `_parent` (the shell's main
/// area) is not used: the page mounts on the scene root.
/// The friend a capture goes to (Friends' "Send a capture"): address and name, with when it was
/// set. It lapses after 10 minutes so a later Share is not still aimed at them.
static SEND_TO: std::sync::Mutex<Option<(String, String, std::time::Instant)>> = std::sync::Mutex::new(None);

pub fn set_send_to(to: Option<(String, String)>) {
    if let Ok(mut s) = SEND_TO.lock() {
        *s = to.map(|(a, l)| (a, l, std::time::Instant::now()));
    }
}

/// The friend Share sends to, when one was picked in the last 10 minutes.
pub fn send_to() -> Option<(String, String)> {
    let s = SEND_TO.lock().ok()?;
    s.as_ref().filter(|(_, _, t)| t.elapsed().as_secs() < 600).map(|(a, l, _)| (a.clone(), l.clone()))
}

pub fn page(name: &str, cx: &mut Cx, _parent: Id, now: f64) -> Option<Box<dyn Page>> {
    let st = fixture_state(name)?;
    Some(Box::new(SharePage::new(cx, st, true, now)))
}

/// Builds a screen by fixture name (the WS 13 golden harness and tests).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let main = crate::screens::trade::main_area(cx);
    page(name, cx, main, now).map(|p| p as Box<dyn Screen>)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fx {
    now_ms: u64,
    seen_age_h: f64,
    status: FxStatus,
    captures: Vec<FxCapture>,
    quotes: FxQuotes,
    address: String,
    done_quilt_id: String,
}

#[derive(Deserialize)]
struct FxStatus {
    screenshot: FxShot,
    clips: FxClips,
    dir: String,
}

#[derive(Deserialize)]
struct FxShot {
    available: bool,
    method: Option<String>,
}

#[derive(Deserialize)]
struct FxClips {
    available: bool,
    enabled: bool,
    recording: bool,
    method: Option<String>,
    seconds: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FxCapture {
    kind: Kind,
    age_h: f64,
    bytes: u64,
    image: String,
    game: Option<CaptureGame>,
}

#[derive(Deserialize)]
struct FxQuotes {
    screenshot: FxQuote,
    clip: FxQuote,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FxQuote {
    size: u64,
    wal_frost: String,
    tip_mist: String,
    gas_mist: String,
}

impl FxQuote {
    fn cost(&self) -> CostEstimate {
        CostEstimate { wal_frost: self.wal_frost.parse().unwrap_or(0), tip_mist: self.tip_mist.parse().unwrap_or(0), gas_mist: self.gas_mist.parse().unwrap_or(0) }
    }
}

/// The bundled pictures (the web mock's canvas scenes; clips show the recorded clip's poster).
fn image(name: &str) -> Option<Thumb> {
    let b: &'static [u8] = match name {
        "hades.webp" => include_bytes!("fixtures/hades.webp"),
        "balatro.webp" => include_bytes!("fixtures/balatro.webp"),
        "act.webp" => include_bytes!("fixtures/act.webp"),
        "celeste.webp" => include_bytes!("fixtures/celeste.webp"),
        "wordgame.webp" => include_bytes!("fixtures/wordgame.webp"),
        "clip-poster.jpg" => include_bytes!("fixtures/clip-poster.jpg"),
        _ => return None,
    };
    let n: &'static str = match name {
        "hades.webp" => "share-fx-hades.webp",
        "balatro.webp" => "share-fx-balatro.webp",
        "act.webp" => "share-fx-act.webp",
        "celeste.webp" => "share-fx-celeste.webp",
        "wordgame.webp" => "share-fx-wordgame.webp",
        _ => "share-fx-clip-poster.jpg",
    };
    Some(Thumb::Static(n, b))
}

/// The prices and link the fixture's selected states show.
pub struct FixtureExtra {
    pub shot: (u64, CostEstimate),
    pub clip: (u64, CostEstimate),
    pub quilt_id: String,
}

/// The mock's state (share/mock.ts in the browser) for the shots in shots.mjs.
pub fn fixture_base() -> (ShareState, FixtureExtra) {
    let fx: Fx = serde_json::from_str(include_str!("fixtures/captures.json")).expect("share fixture");
    let h = 3_600_000.0;
    let mut st = ShareState { now_ms: fx.now_ms, seen_at: fx.now_ms - (fx.seen_age_h * h) as u64, address: Some(fx.address), ..Default::default() };
    st.status = Some(ShareStatus {
        session: Default::default(),
        screenshot: ShotStatus { available: fx.status.screenshot.available, method: fx.status.screenshot.method },
        clips: ClipStatus { available: fx.status.clips.available, enabled: fx.status.clips.enabled, recording: fx.status.clips.recording, method: fx.status.clips.method, seconds: fx.status.clips.seconds },
        dir: fx.status.dir.clone(),
    });
    let mut list = Vec::new();
    for (i, c) in fx.captures.iter().enumerate() {
        let created = fx.now_ms - (c.age_h * h) as u64;
        let clip = c.kind == Kind::Clip;
        let id = format!("{}-{}-{}", if clip { "clip" } else { "shot" }, created, i + 1);
        if let Some(t) = image(&c.image) {
            st.thumbs.insert(id.clone(), t);
        }
        list.push(Capture {
            path: format!("{}/{id}.{}", fx.status.dir, if clip { "mp4" } else { "webp" }),
            id,
            kind: c.kind,
            mime: if clip { "video/mp4" } else { "image/webp" }.into(),
            bytes: c.bytes,
            width: 1920,
            height: 1080,
            duration_ms: clip.then_some(30_000),
            created_ms: created,
            game: c.game.clone(),
        });
    }
    st.captures = Some(list);
    let extra = FixtureExtra { shot: (fx.quotes.screenshot.size, fx.quotes.screenshot.cost()), clip: (fx.quotes.clip.size, fx.quotes.clip.cost()), quilt_id: fx.done_quilt_id };
    (st, extra)
}

/// Opens the first capture of `kind` with its price in.
fn select(st: &mut ShareState, x: &FixtureExtra, kind: Kind) {
    let id = st.captures.as_ref().and_then(|l| l.iter().find(|c| c.kind == kind)).map(|c| c.id.clone()).unwrap_or_default();
    st.select(Some(&id));
    let (size, cost) = if kind == Kind::Clip { x.clip } else { x.shot };
    st.priced(&id, size, Some(cost), None);
}

/// The state behind a fixture name (shots.mjs's share-* shots plus the empty grid).
pub fn fixture_state(name: &str) -> Option<ShareState> {
    let (mut st, x) = fixture_base();
    match name {
        "share-grid" => {}
        "share-detail" => select(&mut st, &x, Kind::Screenshot),
        "share-clip" => select(&mut st, &x, Kind::Clip),
        "share-uploading" => {
            select(&mut st, &x, Kind::Screenshot);
            st.phase = Phase::Uploading(Step::Upload);
        }
        "share-done" => {
            select(&mut st, &x, Kind::Screenshot);
            // A made-up quilt id: this state is only for the layout.
            let link = format::share_link(&x.quilt_id, st.network);
            st.phase = Phase::Done { result: ShareResult { quilt_id: x.quilt_id.clone(), blob_object_id: None, link }, copied: true };
        }
        "share-clips-off" => {
            if let Some(s) = &mut st.status {
                s.clips = ClipStatus { available: false, enabled: false, recording: false, method: None, seconds: 30 };
            }
        }
        "share-empty" => {
            st.captures = Some(Vec::new());
            st.thumbs.clear();
        }
        _ => return None,
    }
    Some(st)
}

/// Share on this device: the captures rare-share keeps (newest first), screenshots as their own
/// pictures; clip posters are made by the page (GStreamer builds). The status needs rare-share's
/// host and comes from the app (`st.status`).
pub fn device_state(now_ms: u64, seen_at: u64, address: Option<String>) -> ShareState {
    let list = rare_share::list();
    let mut st = ShareState { now_ms, seen_at, address, ..Default::default() };
    for c in &list {
        match c.kind {
            Kind::Screenshot => {
                st.thumbs.insert(c.id.clone(), Thumb::File(c.path.clone().into()));
            }
            Kind::Clip => {
                let p = video::poster_cache(&c.id);
                if p.exists() {
                    st.thumbs.insert(c.id.clone(), Thumb::File(p));
                }
            }
        }
    }
    st.captures = Some(list);
    st
}

#[cfg(test)]
mod tests {
    use super::screen::*;
    use super::*;
    use crate::screens::market::host::Out;
    use rare_render::input::Pad;
    use rare_render::{ImageLoader, Scene, TextSys};
    use std::sync::Arc;

    static FONTS: [&[u8]; 4] = [
        include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf"),
        include_bytes!("../../../../render/assets/fonts/Inter-Medium.ttf"),
        include_bytes!("../../../../render/assets/fonts/Inter-SemiBold.ttf"),
        include_bytes!("../../../../render/assets/fonts/FiraCode-Medium.ttf"),
    ];

    fn env() -> (Scene, TextSys, ImageLoader) {
        (Scene::new(1920.0, 1200.0, 24.0), TextSys::new(&FONTS, false, 1024), ImageLoader::new(1, Arc::new(|| {}), None))
    }

    #[test]
    fn every_fixture_builds() {
        for n in SHOTS {
            let (mut sc, mut t, mut img) = env();
            let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
            assert!(fixture(n, &mut cx, 0.0).is_some(), "{n}");
        }
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        assert!(fixture("share-nope", &mut cx, 0.0).is_none());
    }

    #[test]
    fn grid_words_match_the_mock() {
        let st = fixture_state("share-grid").unwrap();
        let l = st.captures.as_ref().unwrap();
        assert_eq!(l.len(), 7);
        assert_eq!(store::new_count(Some(l), st.seen_at), 2);
        assert_eq!(format::ago(l[2].created_ms, st.now_ms), "3 h ago");
        assert_eq!(format::ago(l[4].created_ms, st.now_ms), "yesterday");
        assert_eq!(format::ago(l[6].created_ms, st.now_ms), "3 days ago");
    }

    fn page_of(name: &str, cx: &mut Cx) -> SharePage {
        SharePage::new(cx, fixture_state(name).unwrap(), true, 0.0)
    }

    #[test]
    fn the_grid_opens_a_capture_and_b_comes_back_to_its_tile() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut p = page_of("share-grid", &mut cx);
        assert_eq!(p.host.tag(), Some(T_SHOT), "Take screenshot is the default");
        assert_eq!(p.page_hints()[0], ("a", "Take a screenshot".to_string()));
        // Down to the clips row, down into the grid.
        assert_eq!(p.press(&mut cx, Pad::Down, 0.1), Out::Moved);
        assert_eq!(p.host.tag(), Some(T_CLIPS));
        assert_eq!(p.page_hints()[0], ("a", "Turn off".to_string()));
        p.press(&mut cx, Pad::Down, 0.2);
        let tile = p.host.tag().unwrap();
        assert!(tile >= T_TILE, "{tile}");
        let id = p.st.captures.as_ref().unwrap()[(tile - T_TILE) as usize].id.clone();
        assert_eq!(p.press(&mut cx, Pad::A, 0.3), Out::Act(format!("share:open:{id}")));
        assert_eq!(p.st.selected.as_deref(), Some(id.as_str()));
        assert_eq!(p.press(&mut cx, Pad::B, 0.4), Out::Moved);
        assert_eq!(p.st.selected, None);
        assert_eq!(p.host.tag(), Some(tile), "back on the tile it came from");
        assert_eq!(p.press(&mut cx, Pad::B, 0.5), Out::Close);
    }

    #[test]
    fn the_detail_page_shares_deletes_and_switches_duration() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut p = page_of("share-detail", &mut cx);
        assert_eq!(p.host.tag(), Some(T_SHARE));
        assert_eq!(p.page_hints(), vec![("a", "Share".to_string()), ("b", "Back".to_string()), ("x", "Keep for 1 year".to_string())]);
        assert_eq!(p.press(&mut cx, Pad::X, 0.1), Out::Act("share:duration:year".into()));
        assert!(p.st.quote.is_none(), "priced again");
        p.st.priced(&p.st.selected.clone().unwrap(), 446_000, Some(CostEstimate { wal_frost: 1, tip_mist: 1, gas_mist: 1 }), None);
        p.changed(&mut cx, 0.2);
        assert_eq!(p.press(&mut cx, Pad::Right, 0.3), Out::Moved);
        assert_eq!(p.host.tag(), Some(T_DELETE));
        assert_eq!(p.press(&mut cx, Pad::A, 0.4), Out::Moved);
        assert!(p.st.confirm, "Press again");
        let id = p.st.selected.clone().unwrap();
        assert_eq!(p.press(&mut cx, Pad::A, 0.5), Out::Act(format!("share:delete:{id}")));
        p.press(&mut cx, Pad::Left, 0.6);
        assert_eq!(p.host.tag(), Some(T_SHARE));
        assert_eq!(p.press(&mut cx, Pad::A, 0.7), Out::Act("share:go".into()));
        assert_eq!(p.st.phase, Phase::Uploading(Step::Encode));
        assert_eq!(p.press(&mut cx, Pad::B, 0.8), Out::None, "no way out while uploading");
        assert_eq!(p.page_hints(), vec![("b", "Back".to_string())]);
    }

    #[test]
    fn done_offers_the_link_then_goes_back() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut p = page_of("share-done", &mut cx);
        assert_eq!(p.host.tag(), Some(T_COPY));
        assert_eq!(p.press(&mut cx, Pad::A, 0.1), Out::Act("share:copy".into()));
        p.press(&mut cx, Pad::Down, 0.2);
        assert_eq!(p.host.tag(), Some(T_SEND));
        assert_eq!(p.press(&mut cx, Pad::A, 0.25), Out::Act("share:send".into()));
        p.press(&mut cx, Pad::Down, 0.27);
        assert_eq!(p.host.tag(), Some(T_DONE));
        assert_eq!(p.press(&mut cx, Pad::A, 0.3), Out::Moved);
        assert_eq!(p.st.selected, None);
    }

    #[test]
    fn clips_off_disables_the_row_and_save_clip() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut p = page_of("share-clips-off", &mut cx);
        p.press(&mut cx, Pad::Right, 0.1);
        assert_ne!(p.host.tag(), Some(T_CLIP), "Save last 30 seconds is disabled");
        p.press(&mut cx, Pad::Down, 0.2);
        assert_ne!(p.host.tag(), Some(T_CLIPS), "the clips row is disabled");
        assert_eq!(store::clips_note(p.st.status.as_ref()), Some("Clips need gpu-screen-recorder or GStreamer with VA-API"));
    }
}
