//! Art with fallbacks, the native `<Art sources>`, `Thumb` and `MImg`: tries each source in order
//! (a 404 on Steam's CDN falls through to the next), decodes off the UI thread at the size it is
//! drawn, fades in once ready (the scene's image fade), and when every source fails the build gets
//! `None` and draws its fallback (the title on the layer colour, an initial), never a broken image.
//!
//! A source moving on or running out bumps `version`; put it in the view's stamp so the view
//! rebuilds with the next source or the fallback.

use super::ui::{Ui, S};
use crate::data::art::resolve;
use rare_render::view::{Content, El};
use rare_render::{Fit, ImageLoader, ImgReq, ImgState, Src};
use std::collections::HashMap;

/// How an image fills its box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArtFit {
    /// object-cover; `fy` is the vertical focus (`object-[center_25%]` is 0.25).
    Cover(f32),
    /// object-contain, aligned in the box (0,0 top-left .. 1,1 bottom-right).
    Contain(f32, f32),
}

/// An image a page wants ready before it needs it: `sources` at `w` x `h` rem (`w` 0: the window's width).
#[derive(Clone, Debug, PartialEq)]
pub struct ArtWant {
    pub sources: Vec<String>,
    pub w: f32,
    pub h: f32,
    pub fit: ArtFit,
}

struct Slot {
    srcs: Vec<Src>,
    idx: usize,
    w: u32,
    h: u32,
    fit: Fit,
    key: u64,
    failed: bool,
    /// The slot's key across runs (its references, size and fit).
    id: u64,
    /// The link that loaded was noted in [`Winners`].
    noted: bool,
    /// Every failure so far was an answer (a 4xx, bytes that are not a picture), not the network:
    /// only then is the picture remembered as dead.
    hard: bool,
}

/// Which link of a chain loaded last time, per slot, kept next to the image cache: a revisit asks
/// for that link first (straight from the disk cache) instead of walking the chain again (own
/// renditions that were not made yet answer 404 first, which costs a round trip per image).
struct Winners {
    /// Slot id to the hash of the link that loaded, or to [`DEAD`] plus the unix minute when
    /// every link failed (tried again after [`DEAD_FOR`] minutes; no requests until then).
    map: HashMap<u64, u64>,
    dirty: bool,
    saved_at: f64,
}

fn winners_file() -> Option<std::path::PathBuf> {
    crate::data::img_fetch::cache_dir().map(|d| d.join("winners.bin"))
}

fn winners() -> &'static std::sync::Mutex<Winners> {
    static W: std::sync::OnceLock<std::sync::Mutex<Winners>> = std::sync::OnceLock::new();
    W.get_or_init(|| {
        let mut map = HashMap::new();
        if let Some(b) = winners_file().and_then(|p| std::fs::read(p).ok()) {
            for c in b.chunks_exact(16) {
                map.insert(u64::from_le_bytes(c[0..8].try_into().unwrap()), u64::from_le_bytes(c[8..16].try_into().unwrap()));
            }
        }
        std::sync::Mutex::new(Winners { map, dirty: false, saved_at: 0.0 })
    })
}

const DEAD: u64 = 0xdead_0000_0000_0000;
const DEAD_FOR: u64 = 30;

fn unix_min() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 60).unwrap_or(0)
}

/// Writes the winners file (a few KB; at most every 5 s, and at exit through [`flush`]).
fn save_winners(w: &mut Winners) {
    w.dirty = false;
    if w.map.len() > 50_000 {
        w.map.clear();
    }
    let mut b = Vec::with_capacity(w.map.len() * 16);
    for (k, v) in &w.map {
        b.extend_from_slice(&k.to_le_bytes());
        b.extend_from_slice(&v.to_le_bytes());
    }
    if let Some(p) = winners_file() {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }
}

/// Saves what changed since the last write (call before the process exits).
pub fn flush() {
    if remember() {
        let mut w = winners().lock().unwrap();
        if w.dirty {
            save_winners(&mut w);
        }
    }
}

fn src_hash(s: &Src) -> u64 {
    rare_render::view::key(s)
}

/// The fixtures and golden images keep no memory between runs.
fn remember() -> bool {
    std::env::var_os("RR_FIXTURE").is_none() || std::env::var_os("RR_IMG_CACHE").is_some()
}

/// One `Arts`' slots. They live in a per-thread table, not in the `Arts`, so the app's frame
/// polls every page's slots ([`poll_hidden`]): a list refreshed under another page, or a page
/// already closed, still fails over and still has its winning link remembered.
#[derive(Default)]
struct Table {
    slots: HashMap<u64, Slot>,
    /// A slot moved on while the owner was not polling: its next `poll` reports a change.
    changed: bool,
    /// The owner was dropped (when): the slots are polled until settled, then the table goes.
    orphan: Option<f64>,
}

thread_local! {
    static TABLES: std::cell::RefCell<HashMap<u64, Table>> = std::cell::RefCell::new(HashMap::new());
}

/// How long a closed page's slots are still polled for their winners.
const ORPHAN_FOR: f64 = 120.0;

pub struct Arts {
    id: u64,
    /// Bumps when a source fails over or a list runs out.
    pub version: u64,
    /// The data [`Arts::prefetch`] last asked for.
    prefetched: u64,
}

impl Default for Arts {
    fn default() -> Arts {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        TABLES.with(|t| t.borrow_mut().insert(id, Table::default()));
        Arts { id, version: 0, prefetched: 0 }
    }
}

impl Drop for Arts {
    fn drop(&mut self) {
        let now = rare_render::clock::secs();
        let _ = TABLES.try_with(|t| {
            if let Ok(mut t) = t.try_borrow_mut() {
                if let Some(tb) = t.get_mut(&self.id) {
                    tb.orphan = Some(now);
                }
            }
        });
    }
}

/// Polls the slots of every other `Arts` on this thread (pages under the one on top, pages of a
/// tab not on screen, pages already closed): failed links move on to the next and winners are
/// remembered, as the owner's own poll does. The app calls it once a frame.
pub fn poll_hidden(img: &mut ImageLoader, except: &Arts) {
    let now = rare_render::clock::secs();
    let mut won: Vec<(u64, u64)> = Vec::new();
    TABLES.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|id, tb| {
            if *id == except.id {
                return true;
            }
            if poll_slots(&mut tb.slots, img, &mut won) {
                tb.changed = true;
            }
            match tb.orphan {
                Some(at) => now - at < ORPHAN_FOR && tb.slots.values().any(|s| !s.failed && !s.noted),
                None => true,
            }
        });
    });
    note_winners(won);
}

/// Moves failed slots on and collects winners. True when a slot changed.
fn poll_slots(slots: &mut HashMap<u64, Slot>, img: &mut ImageLoader, won: &mut Vec<(u64, u64)>) -> bool {
    let mut any = false;
    for s in slots.values_mut() {
        if s.failed {
            continue;
        }
        // peek: polling is not drawing (drawn images load first).
        match img.peek(s.key) {
            ImgState::Ready { .. } => {
                if !s.noted {
                    s.noted = true;
                    if s.srcs.len() > 1 && matches!(s.srcs[s.idx], Src::Url(_)) {
                        won.push((s.id, src_hash(&s.srcs[s.idx])));
                    }
                }
                continue;
            }
            ImgState::Pending => continue,
            ImgState::Failed => {
                let e = img.error(s.key).unwrap_or("");
                if !(e.starts_with("http status: 4") || e.starts_with("not an image") || e.starts_with("svg") || e.contains("decod") || e.contains("Format")) {
                    s.hard = false;
                }
            }
        }
        s.idx += 1;
        if s.idx >= s.srcs.len() {
            s.failed = true;
            if s.hard && s.srcs.iter().any(|x| matches!(x, Src::Url(_))) {
                won.push((s.id, DEAD | unix_min()));
            }
        } else {
            s.key = img.request(ImgReq { src: s.srcs[s.idx].clone(), w: s.w, h: s.h, fit: s.fit });
        }
        any = true;
    }
    any
}

fn note_winners(won: Vec<(u64, u64)>) {
    if !remember() {
        return;
    }
    let mut w = winners().lock().unwrap();
    for (id, h) in won {
        if w.map.insert(id, h) != Some(h) {
            w.dirty = true;
        }
    }
    let now = rare_render::clock::secs();
    if w.dirty && (w.saved_at == 0.0 || now - w.saved_at > 5.0) {
        w.saved_at = now;
        save_winners(&mut w);
    }
}

fn slot_key(sources: &[String], w: u32, h: u32, fit: Fit) -> u64 {
    rare_render::view::key(&(sources, w, h, format!("{fit:?}")))
}

impl Arts {
    /// The loader key to draw for `sources` at `w` x `h` px, requesting it on first use; None when
    /// there is no usable source or every one failed.
    pub fn key(&mut self, img: &mut ImageLoader, sources: &[String], w: f32, h: f32, fit: ArtFit) -> Option<u64> {
        let (w, h) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
        let fit = match fit {
            ArtFit::Cover(fy) => Fit::Cover { fy },
            ArtFit::Contain(..) => Fit::Contain,
        };
        let k = slot_key(sources, w, h, fit);
        TABLES.with(|t| {
        let mut t = t.borrow_mut();
        let slots = &mut t.entry(self.id).or_default().slots;
        let slot = slots.entry(k).or_insert_with(|| {
            // Each reference expands to its chain (rare.market's rendition at this size first, the
            // originals after); a later reference (a collection's logo behind an item) follows.
            let aspect = w as f32 / h as f32;
            let inside = matches!(fit, Fit::Contain) || !(0.8..=1.25).contains(&aspect);
            let mut srcs: Vec<Src> = Vec::new();
            for s in sources.iter().filter(|s| !s.is_empty()) {
                for src in resolve(s, w.max(h) as f32, inside) {
                    if !srcs.contains(&src) {
                        srcs.push(src);
                    }
                }
            }
            // The link that loaded last time goes first; a picture none of whose links loaded a
            // little while ago shows its fallback at once.
            if remember() && srcs.iter().any(|s| matches!(s, Src::Url(_))) {
                if let Some(win) = winners().lock().unwrap().map.get(&k).copied() {
                    if win & 0xffff_0000_0000_0000 == DEAD && unix_min().saturating_sub(win & 0xffff_ffff_ffff) < DEAD_FOR {
                        srcs.clear();
                    } else if let Some(i) = srcs.iter().position(|s| src_hash(s) == win) {
                        let s = srcs.remove(i);
                        srcs.insert(0, s);
                    }
                }
            }
            let mut s = Slot { failed: srcs.is_empty(), srcs, idx: 0, w, h, fit, key: 0, id: k, noted: false, hard: true };
            if !s.failed {
                s.key = img.request(ImgReq { src: s.srcs[0].clone(), w, h, fit });
            }
            s
        });
        (!slot.failed).then_some(slot.key)
        })
    }

    /// Requests every wanted image (the same keys the views draw, so they are shared) and returns
    /// the ids whose images are all ready; a source list that ran out counts as ready.
    pub fn ready(&mut self, img: &mut ImageLoader, rem: f32, vw: f32, want: &[(String, Vec<ArtWant>)]) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        for (id, asks) in want {
            let mut ok = true;
            for a in asks {
                if let Some(k) = self.key(img, &a.sources, if a.w <= 0.0 { vw } else { a.w * rem }, a.h * rem, a.fit) {
                    ok &= !matches!(img.peek(k), ImgState::Pending);
                }
            }
            if ok {
                out.insert(id.clone());
            }
        }
        out
    }

    /// Pictures a screen will draw, asked for when their data arrives rather than when the screen
    /// opens: `(reference, size in rem)` square cover slots, the same keys the views use (kit
    /// avatars and MImg), so opening the screen finds them on the GPU. `stamp` names the data;
    /// nothing is asked again until it changes.
    pub fn prefetch(&mut self, img: &mut ImageLoader, rem: f32, stamp: u64, asks: impl FnOnce() -> Vec<(String, f32)>) {
        if stamp == self.prefetched {
            return;
        }
        self.prefetched = stamp;
        for (src, size) in asks() {
            if !src.is_empty() {
                let px = size * rem;
                self.key(img, std::slice::from_ref(&src), px, px, ArtFit::Cover(0.5));
            }
        }
    }

    /// After images land: failed sources move on to the next, exhausted lists give up. True when
    /// anything changed (the views using art should rebuild).
    pub fn poll(&mut self, img: &mut ImageLoader) -> bool {
        let mut won: Vec<(u64, u64)> = Vec::new();
        let any = TABLES.with(|t| {
            let mut t = t.borrow_mut();
            let tb = t.entry(self.id).or_default();
            let moved = poll_slots(&mut tb.slots, img, &mut won);
            // A slot [`poll_hidden`] moved on while this page was not polling counts too.
            moved | std::mem::take(&mut tb.changed)
        });
        if any {
            self.version += 1;
        }
        note_winners(won);
        any
    }

    /// Art still loading.
    pub fn pending(&self, img: &ImageLoader) -> usize {
        TABLES.with(|t| t.borrow().get(&self.id).map(|tb| tb.slots.values().filter(|s| !s.failed && matches!(img.peek(s.key), ImgState::Pending)).count()).unwrap_or(0))
    }
}

/// An image leaf for `key`: cover-fit pixels, or contained and aligned.
pub fn image(key: u64, fit: ArtFit) -> El {
    match fit {
        ArtFit::Cover(_) => El::image(key),
        ArtFit::Contain(x, y) => El { content: Content::Image { key, tint: None, contain: Some([x, y]) }, ..Default::default() },
    }
}

/// `<Art>`: the first source that loads, filling `s` (sized `w` x `h` rem), or `fallback`.
pub fn art(ui: &mut Ui, s: S, sources: &[String], w: f32, h: f32, fit: ArtFit, fallback: Option<El>) -> El {
    // A Rare default picture: drawn from its SVG, never fetched.
    if let Some(d) = sources.first().and_then(|s| crate::avatars::id_of_src(s)).and_then(crate::avatars::get) {
        let k = crate::kit::icon::svg_key(ui, &format!("avatar-{}", d.id), d.svg.to_string(), w, h);
        return image(k, fit).with_style(s.to_style(ui.rem));
    }
    let (pw, ph) = (ui.r(w), ui.r(h));
    match ui.arts.key(ui.img, sources, pw, ph, fit) {
        Some(k) => image(k, fit).with_style(s.to_style(ui.rem)),
        None => fallback.unwrap_or_else(|| ui.bx(s)),
    }
}
