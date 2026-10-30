//! PC streaming screens (launcher/src/stream, docs/STREAMING.md): the "On your PC" shelf and the
//! streaming grid, the setup / pairing / game sheets, the pure logic and the state machine, with a
//! fixture backend (the web's mock) behind `StreamApi` until rare-stream is wired.

pub mod device;
pub mod fixtures;
pub mod logic;
pub mod sheets;
pub mod state;
pub mod tiles;
pub mod view;

use crate::screens::overlay::common::*;
use fixtures::{Mode, MockApi, DEN, PC};
use logic::*;
use rare_render::input::Pad;
use state::{StreamApi, StreamState};
pub use tiles::PageKind;
use view::{Act, Layer};

#[derive(Clone, PartialEq, Debug)]
pub enum SheetKind {
    Setup,
    App(String, u32),
}

/// What the sheet's content is, so a rebuild in place does not fade in again.
#[derive(Clone, Copy, PartialEq)]
enum ViewKey {
    Setup,
    Pair,
    App,
}

pub struct StreamUi {
    pub st: StreamState,
    page_kind: Option<PageKind>,
    page: Option<Layer>,
    sheet: Option<(SheetKind, ViewKey, Layer)>,
    /// A Wake and play that closes its sheet once the stream starts.
    sheet_wait: Option<String>,
    /// A Play / Resume / Quit that is running in the background: the sheet closes when it is done.
    close_when_idle: bool,
    /// The sheet closed itself and there is no page behind it: the app can drop this screen.
    done: bool,
    /// Fixtures only: Moonlight's install progress (mock.ts mockInstall).
    sim_install: Option<(f64, u32)>,
    pub sim: bool,
}

impl StreamUi {
    pub fn new(cx: &mut Cx, st: StreamState, page: Option<PageKind>, sheet: Option<SheetKind>, now: f64) -> StreamUi {
        let mut ui = StreamUi { st, page_kind: page, page: None, sheet: None, sheet_wait: None, close_when_idle: false, done: false, sim_install: None, sim: true };
        ui.build_page(cx, None, now);
        if let Some(k) = sheet {
            ui.open_sheet(cx, k, now);
        }
        ui
    }

    /// The streaming grid / shelf over a backend.
    pub fn with_api(cx: &mut Cx, api: Box<dyn StreamApi>, page: PageKind, now: f64) -> StreamUi {
        let mut st = StreamState::new(api);
        st.load();
        let mut ui = StreamUi::new(cx, st, Some(page), None, now);
        ui.sim = false;
        ui
    }

    pub fn has_sheet(&self) -> bool {
        self.sheet.is_some()
    }

    /// Streaming was opened and Moonlight is missing: Rare gets it now, in the background, so the
    /// first stream never waits on a Get button (the sheet shows the progress).
    pub fn need_moonlight(&mut self, cx: &mut Cx, now: f64) {
        if self.st.moonlight.is_none() && self.st.offer.is_some() && self.st.installing.is_none() {
            self.start_install(now);
            self.rebuild(cx, now);
        }
    }

    fn start_install(&mut self, now: f64) {
        self.st.install();
        if self.sim && self.st.installing.is_some() {
            self.sim_install = Some((now + 0.1, 0));
        }
    }

    fn build_page(&mut self, cx: &mut Cx, want: Option<Act>, now: f64) {
        let Some(kind) = self.page_kind else { return };
        let (sy, sx) = self.page.take().map(|l| {
            let v = (cx.scene.get(l.root).scroll_y.target(), l.rails.iter().map(|&r| cx.scene.get(r).scroll_x.target()).collect::<Vec<_>>());
            cx.scene.remove(l.root);
            v
        }).unwrap_or((0.0, vec![]));
        let root = cx.scene.root;
        let mut l = tiles::build_page(cx, &self.st, kind, root, now);
        cx.scene.layout(cx.t);
        cx.scene.n(l.root).scroll_y.snap(sy);
        for (i, &r) in l.rails.iter().enumerate() {
            cx.scene.n(r).scroll_x.snap(sx.get(i).copied().unwrap_or(0.0));
        }
        l.focus_initial(cx, want.as_ref(), now);
        // The page sits under any open sheet.
        if let Some((_, _, sl)) = &self.sheet {
            let (sr, pr) = (sl.root, l.root);
            let kids = cx.scene.children(cx.scene.root);
            let mut order: Vec<_> = kids.into_iter().filter(|k| *k != sr && *k != pr).collect();
            order.push(pr);
            order.push(sr);
            cx.scene.set_children(cx.scene.root, &order);
        }
        self.page = Some(l);
    }

    fn build_sheet_layer(&mut self, cx: &mut Cx, kind: &SheetKind, now: f64) -> Option<(ViewKey, Layer)> {
        let root = cx.scene.root;
        match kind {
            SheetKind::Setup => {
                let vk = if self.st.pairing.is_some() { ViewKey::Pair } else { ViewKey::Setup };
                Some((vk, sheets::build_setup(cx, &self.st, root, now)))
            }
            SheetKind::App(u, a) => sheets::build_app(cx, &self.st, u, *a, root, now).map(|l| (ViewKey::App, l)),
        }
    }

    pub fn open_sheet(&mut self, cx: &mut Cx, kind: SheetKind, now: f64) -> bool {
        if kind == SheetKind::Setup {
            if !self.st.loaded {
                self.st.load();
            }
            // Finding PCs is Rare's own mDNS; it doesn't need Moonlight.
            self.st.scan();
        } else if !self.st.loaded {
            self.st.load();
        } else {
            self.st.refresh();
        }
        self.drop_sheet(cx);
        let Some((vk, mut l)) = self.build_sheet_layer(cx, &kind, now) else { return false };
        cx.scene.layout(cx.t);
        l.focus_initial(cx, None, now);
        self.sheet = Some((kind, vk, l));
        true
    }

    fn drop_sheet(&mut self, cx: &mut Cx) {
        if let Some((_, _, l)) = self.sheet.take() {
            cx.scene.remove(l.root);
        }
        self.sheet_wait = None;
    }

    /// Shows what changed in the state: the page and the sheet are rebuilt in place.
    pub fn rebuild(&mut self, cx: &mut Cx, now: f64) {
        let want_page = self.page.as_ref().and_then(|l| l.current_act());
        self.build_page(cx, want_page, now);
        let Some((kind, old_vk, old)) = self.sheet.take() else { return };
        let want = old.current_act();
        cx.scene.remove(old.root);
        match self.build_sheet_layer(cx, &kind, now) {
            Some((vk, mut l)) => {
                if vk == old_vk {
                    if let Some(c) = l.card {
                        cx.scene.n(c).opacity.snap(1.0);
                        cx.scene.n(c).ty.snap(0.0);
                    }
                }
                cx.scene.layout(cx.t);
                l.focus_initial(cx, want.as_ref(), now);
                self.sheet = Some((kind, vk, l));
                // Keep the page under the sheet.
                self.build_page_order(cx);
            }
            None => self.sheet_wait = None,
        }
    }

    fn build_page_order(&mut self, cx: &mut Cx) {
        if let (Some(p), Some((_, _, sl))) = (&self.page, &self.sheet) {
            let (sr, pr) = (sl.root, p.root);
            let mut order: Vec<_> = cx.scene.children(cx.scene.root).into_iter().filter(|k| *k != sr && *k != pr).collect();
            order.push(pr);
            order.push(sr);
            cx.scene.set_children(cx.scene.root, &order);
        }
    }

    /// The sheet is gone: back to the page, or out when there is none.
    fn sheet_closed(&mut self, cx: &mut Cx, now: f64) -> Out {
        self.drop_sheet(cx);
        if self.page.is_some() {
            self.rebuild(cx, now);
            Out::Moved
        } else {
            Out::Close
        }
    }

    fn top(&mut self) -> Option<&mut Layer> {
        match self.sheet.as_mut() {
            Some((_, _, l)) => Some(l),
            None => self.page.as_mut(),
        }
    }

    fn sheet_app(&self) -> Option<(Host, StreamApp)> {
        let (SheetKind::App(u, a), _, _) = self.sheet.as_ref()? else { return None };
        let h = self.st.host(u)?.clone();
        let app = self.st.apps_of(u).iter().find(|x| x.id == *a)?.clone();
        Some((h, app))
    }

    /// The address typed on the keyboard for "Add by address".
    pub fn address_entered(&mut self, cx: &mut Cx, address: &str, now: f64) {
        if address.trim().is_empty() {
            return;
        }
        if !valid_address(address) {
            self.st.toasts.push(state::Toast { text: "That doesn't look like an address".into(), error: true });
            return;
        }
        if let Some(h) = self.st.add(address.trim()) {
            if !h.paired && self.st.moonlight.is_some() {
                self.st.pair(&h, now);
            }
        }
        self.rebuild(cx, now);
    }

    /// Focuses a specific item on the top layer (journeys, fixtures).
    pub fn focus_act(&mut self, cx: &mut Cx, act: &Act, now: f64) {
        if let Some(l) = self.top() {
            if let Some(t) = l.acts.iter().position(|a| a == act) {
                if let Some(id) = l.nav.items.iter().find(|f| f.tag == t as u32).map(|f| f.id) {
                    l.nav.focus(cx.scene, id, now, false);
                    l.sync(cx, now);
                }
            }
        }
    }

    fn run(&mut self, cx: &mut Cx, act: Act, now: f64) -> Out {
        match act {
            Act::Tile(u, a) => {
                if self.open_sheet(cx, SheetKind::App(u, a), now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Act::OpenSetup => {
                self.open_sheet(cx, SheetKind::Setup, now);
                Out::Moved
            }
            Act::SetupFromApp => {
                self.open_sheet(cx, SheetKind::Setup, now);
                Out::Moved
            }
            Act::ListAgain(u) => {
                self.st.list_apps(&u);
                self.rebuild(cx, now);
                Out::Moved
            }
            Act::Wake(u) => {
                if let Some(h) = self.st.host(&u).cloned() {
                    self.st.wake_start(&h, None, now);
                }
                self.rebuild(cx, now);
                Out::Moved
            }
            Act::GetMoonlight => {
                self.start_install(now);
                self.rebuild(cx, now);
                Out::Moved
            }
            Act::AddAddress => Out::Act("stream:ask-address".into()),
            Act::LookAgain => {
                self.st.scan();
                self.rebuild(cx, now);
                Out::Moved
            }
            Act::HostRow(u) => {
                let Some(h) = self.st.host(&u).cloned() else { return Out::None };
                if self.st.moonlight.is_none() {
                    // Pairing waits for Moonlight, which Rare is getting (or starts getting now).
                    if self.st.offer.is_none() {
                        return Out::None;
                    }
                    if !self.st.installing.as_ref().is_some_and(|i| i.error.is_none()) {
                        self.start_install(now);
                    }
                    self.st.pending_pair = Some(u);
                    self.rebuild(cx, now);
                    return Out::Moved;
                }
                if !h.paired {
                    self.st.pair(&h, now);
                    self.rebuild(cx, now);
                    return Out::Moved;
                }
                self.st.list_apps(&u);
                self.sheet_closed(cx, now);
                Out::Act("stream:show-library".into())
            }
            Act::CancelPair | Act::BackFromPair => {
                self.st.cancel_pair();
                self.rebuild(cx, now);
                Out::Moved
            }
            Act::ShowGames => {
                self.st.cancel_pair();
                self.sheet_closed(cx, now);
                Out::Act("stream:show-library".into())
            }
            Act::TryAgain(u) => {
                if let Some(h) = self.st.host(&u).cloned() {
                    self.st.pair(&h, now);
                }
                self.rebuild(cx, now);
                Out::Moved
            }
            Act::ChooseResolution | Act::ChooseBitrate | Act::ChooseCodec => {
                let mut n = self.st.settings.clone();
                match act {
                    Act::ChooseResolution => n.resolution = cycle(&RESOLUTIONS.map(|r| r.0.to_string()), &n.resolution),
                    Act::ChooseBitrate => n.bitrate_mbps = cycle(&BITRATES, &n.bitrate_mbps),
                    _ => n.codec = cycle(&CODECS, &n.codec),
                }
                self.st.set_settings(n);
                self.rebuild(cx, now);
                Out::Moved
            }
            Act::Play | Act::Resume | Act::QuitGame | Act::QuitAndPlay => {
                let Some((h, a)) = self.sheet_app() else { return Out::None };
                match act {
                    Act::QuitGame => self.st.quit(&h),
                    Act::QuitAndPlay => self.st.play(&h, &a, true, now),
                    _ => self.st.play(&h, &a, false, now),
                }
                if self.st.busy.is_some() {
                    // The call runs on a worker; the sheet shows it and closes when it lands.
                    self.close_when_idle = true;
                    self.rebuild(cx, now);
                    return Out::Moved;
                }
                self.sheet_closed(cx, now)
            }
            Act::WakeAndPlay => {
                let Some((h, a)) = self.sheet_app() else { return Out::None };
                self.st.play(&h, &a, false, now);
                if self.st.waking.is_some() {
                    self.sheet_wait = Some(run_id(&h.uuid, a.id));
                    self.rebuild(cx, now);
                    Out::Moved
                } else {
                    self.sheet_closed(cx, now)
                }
            }
            Act::LookAgainApp => {
                self.st.refresh();
                self.rebuild(cx, now);
                Out::Moved
            }
        }
    }
}

impl Screen for StreamUi {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        let Some(l) = self.top() else { return false };
        let hit = l.nav.focus_at(cx.scene, x, y, now);
        if hit {
            l.sync(cx, now);
        }
        hit
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                let Some(l) = self.top() else { return Out::None };
                if l.nav.step(cx.scene, pad, now) {
                    l.sync(cx, now);
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => match self.top().and_then(|l| l.current_act()) {
                Some(a) => self.run(cx, a, now),
                None => Out::None,
            },
            Pad::B => {
                if self.sheet.is_some() {
                    // Leaving the sheet stops a pairing that hasn't finished.
                    self.st.leave_setup();
                    self.sheet_closed(cx, now)
                } else {
                    Out::Close
                }
            }
            _ => Out::None,
        }
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        let mut changed = self.st.tick(now);
        while let Some((next, p)) = self.sim_install {
            if now < next {
                break;
            }
            let p = p + 5;
            self.st.install_progress(Some(p as f32 / 100.0));
            changed = true;
            if p >= 100 {
                self.st.install_done(None);
                self.sim_install = None;
            } else {
                self.sim_install = Some((next + 0.1, p));
            }
        }
        if self.st.moonlight.is_some() {
            if let Some(h) = self.st.pending_pair.take().and_then(|u| self.st.host(&u).cloned()) {
                if !h.paired && self.st.pairing.is_none() {
                    self.st.pair(&h, now);
                    changed = true;
                }
            }
        } else if self.st.installing.as_ref().is_some_and(|i| i.error.is_some()) {
            self.st.pending_pair = None;
        }
        if self.close_when_idle && self.st.busy.is_none() {
            self.close_when_idle = false;
            if matches!(self.sheet_closed(cx, now), Out::Close) {
                self.done = true;
            }
            return;
        }
        if let Some(rid) = self.sheet_wait.clone() {
            if self.st.running.contains(&rid) {
                self.sheet_closed(cx, now);
                return;
            }
        }
        if changed {
            self.rebuild(cx, now);
        }
    }

    fn finished(&self) -> bool {
        self.done
    }

    fn take_message(&mut self) -> Option<String> {
        let i = self.st.toasts.iter().position(|t| t.error)?;
        Some(self.st.toasts.remove(i).text)
    }

    fn answer(&mut self, cx: &mut Cx, key: &str, value: Option<String>, now: f64) -> Out {
        if let (true, Some(v)) = (key == "stream:ask-address", value) {
            self.address_entered(cx, &v, now);
        }
        Out::None
    }

    fn next_deadline(&self) -> Option<f64> {
        [self.st.next_deadline(), self.sim_install.map(|(t, _)| t)].into_iter().flatten().reduce(f64::min)
    }
}

fn load(mode: Mode, run: &[&str]) -> StreamState {
    let api = MockApi::new(mode).with_running(run.iter().map(|s| s.to_string()).collect());
    let mut st = StreamState::new(Box::new(api));
    st.load();
    st
}

fn setup(cx: &mut Cx, st: StreamState, now: f64) -> Option<Box<dyn Screen>> {
    Some(Box::new(StreamUi::new(cx, st, None, Some(SheetKind::Setup), now)))
}

fn app_sheet(cx: &mut Cx, st: StreamState, app: u32, now: f64) -> Option<Box<dyn Screen>> {
    Some(Box::new(StreamUi::new(cx, st, None, Some(SheetKind::App(PC.into(), app)), now)))
}

fn page(cx: &mut Cx, st: StreamState, kind: PageKind, now: f64) -> Box<dyn Screen> {
    Box::new(StreamUi::new(cx, st, Some(kind), None, now))
}

const ELDEN: u32 = 1245620;
const CYBER: u32 = 1091500;
const DESKTOP: u32 = 881448767;

/// Fixture screens by name: "stream-<state>".
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let rid = |a: u32| run_id(PC, a);
    Some(match name {
        // Tiles: Home's shelf and the streaming grid.
        "stream-tiles" => page(cx, load(Mode::Normal, &[]), PageKind::Both, now),
        "stream-tiles-focus" => {
            let mut ui = StreamUi::new(cx, load(Mode::Busy, &[&rid(CYBER)]), Some(PageKind::Both), None, now);
            ui.focus_act(cx, &Act::Tile(PC.into(), ELDEN), now);
            Box::new(ui)
        }
        "stream-rail" => page(cx, load(Mode::Normal, &[]), PageKind::Home, now),
        "stream-grid" => page(cx, load(Mode::Normal, &[]), PageKind::Library, now),
        "stream-grid-busy" => page(cx, load(Mode::Busy, &[&rid(CYBER)]), PageKind::Library, now),
        "stream-grid-asleep" => page(cx, load(Mode::Asleep, &[]), PageKind::Library, now),
        "stream-grid-empty" => page(cx, load(Mode::NoPcs, &[]), PageKind::Library, now),
        "stream-grid-nomoonlight" => page(cx, load(Mode::NoMoonlight, &[]), PageKind::Library, now),
        "stream-grid-nogames" => page(cx, load(Mode::NoGames, &[]), PageKind::Library, now),
        "stream-grid-listing" => {
            let mut st = load(Mode::NoGames, &[]);
            st.listing = Some(PC.into());
            page(cx, st, PageKind::Library, now)
        }
        "stream-grid-wake" => {
            let mut st = load(Mode::Asleep, &[]);
            st.apps.clear();
            page(cx, st, PageKind::Library, now)
        }
        "stream-grid-waking" => {
            let mut st = load(Mode::Asleep, &[]);
            st.apps.clear();
            st.waking = Some(PC.into());
            page(cx, st, PageKind::Library, now)
        }
        "stream-grid-off" => {
            let mut st = load(Mode::Offline, &[]);
            st.apps.clear();
            page(cx, st, PageKind::Library, now)
        }
        "stream-grid-loading" => page(cx, StreamState::new(Box::new(MockApi::new(Mode::Normal))), PageKind::Library, now),
        // Setup sheet.
        "stream-sheet-setup" => {
            let mut ui = StreamUi::new(cx, load(Mode::Normal, &[]), None, Some(SheetKind::Setup), now);
            ui.st.scanning = false;
            Box::new(ui)
        }
        "stream-sheet-setup-scanning" => {
            let mut st = load(Mode::Normal, &[]);
            st.scanning = true;
            let mut ui = StreamUi::new(cx, st, None, None, now);
            ui.sheet_scanning(cx, now);
            Box::new(ui)
        }
        "stream-sheet-setup-nopcs" => {
            let mut ui = StreamUi::new(cx, load(Mode::NoPcs, &[]), None, Some(SheetKind::Setup), now);
            ui.st.hosts.clear();
            ui.rebuild(cx, now);
            Box::new(ui)
        }
        "stream-sheet-setup-nomoonlight" => return setup(cx, load(Mode::NoMoonlight, &[]), now),
        "stream-sheet-setup-installing" | "stream-sheet-setup-installing-wait" | "stream-sheet-setup-install-error" => {
            let mut ui = StreamUi::new(cx, load(Mode::NoMoonlight, &[]), None, Some(SheetKind::Setup), now);
            match name {
                "stream-sheet-setup-installing" => ui.st.install_progress(Some(0.45)),
                "stream-sheet-setup-installing-wait" => ui.st.install_progress(None),
                _ => {
                    ui.st.installing = Some(state::Installing { progress: None, error: Some("Couldn't set up streaming. Check your Wi-Fi and try again.".into()) });
                }
            }
            ui.rebuild(cx, now);
            Box::new(ui)
        }
        // Pairing.
        "stream-sheet-pair" | "stream-sheet-pair-gfe" | "stream-sheet-pair-paired" | "stream-sheet-pair-failed" => {
            let mode = match name {
                "stream-sheet-pair-gfe" => Mode::Gfe,
                "stream-sheet-pair-failed" => Mode::PairFail,
                "stream-sheet-pair-paired" => Mode::Normal,
                _ => Mode::Pairing,
            };
            let mut ui = StreamUi::new(cx, load(mode, &[]), None, Some(SheetKind::Setup), now);
            let den = ui.st.hosts.iter().find(|h| h.uuid == DEN).cloned()?;
            ui.st.pair(&den, now);
            if name.ends_with("paired") || name.ends_with("failed") {
                for i in 1..=10 {
                    ui.st.tick(now + i as f64);
                }
            }
            ui.rebuild(cx, now);
            Box::new(ui)
        }
        // A PC game's sheet.
        "stream-sheet-app-ready" => return app_sheet(cx, load(Mode::Normal, &[]), CYBER, now),
        "stream-sheet-app-playing" => return app_sheet(cx, load(Mode::Normal, &[&rid(CYBER)]), CYBER, now),
        "stream-sheet-app-on-pc" => return app_sheet(cx, load(Mode::Busy, &[]), ELDEN, now),
        "stream-sheet-app-busy" => return app_sheet(cx, load(Mode::Busy, &[]), CYBER, now),
        "stream-sheet-app-asleep" => return app_sheet(cx, load(Mode::Asleep, &[]), CYBER, now),
        "stream-sheet-app-waking" => {
            let mut st = load(Mode::Asleep, &[]);
            st.waking = Some(PC.into());
            return app_sheet(cx, st, CYBER, now);
        }
        "stream-sheet-app-offline" => return app_sheet(cx, load(Mode::Offline, &[]), CYBER, now),
        "stream-sheet-app-nomoonlight" => return app_sheet(cx, load(Mode::NoMoonlight, &[]), CYBER, now),
        "stream-sheet-app-noart" => return app_sheet(cx, load(Mode::Normal, &[]), DESKTOP, now),
        "stream-sheet-app-1280" => {
            let mut st = load(Mode::Normal, &[]);
            st.settings = StreamSettings { resolution: "1280x800".into(), bitrate_mbps: 50, codec: Codec::Hevc, ..default_settings() };
            return app_sheet(cx, st, CYBER, now);
        }
        _ => return None,
    })
}

impl StreamUi {
    /// A setup sheet drawn while a scan is going (no scan is started).
    fn sheet_scanning(&mut self, cx: &mut Cx, now: f64) {
        let kind = SheetKind::Setup;
        self.drop_sheet(cx);
        if let Some((vk, mut l)) = self.build_sheet_layer(cx, &kind, now) {
            cx.scene.layout(cx.t);
            l.focus_initial(cx, None, now);
            self.sheet = Some((kind, vk, l));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_render::{ImageLoader, Scene, TextSys};
    use std::sync::Arc;

    static FONTS: [&[u8]; 5] = [
        include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf"),
        include_bytes!("../../../../render/assets/fonts/Inter-Medium.ttf"),
        include_bytes!("../../../../render/assets/fonts/Inter-SemiBold.ttf"),
        include_bytes!("../../../../render/assets/fonts/Inter-Bold.ttf"),
        include_bytes!("../../../../render/assets/fonts/FiraCode-Medium.ttf"),
    ];

    const NAMES: &[&str] = &[
        "stream-tiles", "stream-tiles-focus", "stream-rail", "stream-grid", "stream-grid-busy", "stream-grid-asleep", "stream-grid-empty",
        "stream-grid-nomoonlight", "stream-grid-nogames", "stream-grid-listing", "stream-grid-wake", "stream-grid-waking", "stream-grid-off",
        "stream-grid-loading", "stream-sheet-setup", "stream-sheet-setup-scanning", "stream-sheet-setup-nopcs", "stream-sheet-setup-nomoonlight",
        "stream-sheet-setup-installing", "stream-sheet-setup-installing-wait", "stream-sheet-setup-install-error", "stream-sheet-pair",
        "stream-sheet-pair-gfe", "stream-sheet-pair-paired", "stream-sheet-pair-failed", "stream-sheet-app-ready", "stream-sheet-app-playing",
        "stream-sheet-app-on-pc", "stream-sheet-app-busy", "stream-sheet-app-asleep", "stream-sheet-app-waking", "stream-sheet-app-offline",
        "stream-sheet-app-nomoonlight", "stream-sheet-app-noart", "stream-sheet-app-1280",
    ];

    fn env() -> (Scene, TextSys, ImageLoader) {
        (Scene::new(1920.0, 1200.0, 24.0), TextSys::new(&FONTS, false, 1024), ImageLoader::new(1, Arc::new(|| {}), None))
    }

    #[test]
    fn every_fixture_builds() {
        for n in NAMES {
            let (mut sc, mut t, mut img) = env();
            let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
            assert!(fixture(n, &mut cx, 0.0).is_some(), "{n}");
        }
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        assert!(fixture("stream-nope", &mut cx, 0.0).is_none());
    }

    #[test]
    fn the_flow_from_a_cover_to_a_stream() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut ui = StreamUi::new(&mut cx, load(Mode::Normal, &[]), Some(PageKind::Library), None, 0.0);
        // A opens the focused game's sheet, A on Play streams it and closes the sheet.
        assert_eq!(ui.pad(&mut cx, Pad::A, 0.1), Out::Moved);
        assert!(ui.sheet.is_some());
        assert_eq!(ui.pad(&mut cx, Pad::A, 0.2), Out::Moved);
        assert!(ui.sheet.is_none());
        assert_eq!(ui.st.running.len(), 1);
        assert!(ui.st.toasts[0].text.starts_with("Streaming "));
        assert_eq!(ui.pad(&mut cx, Pad::B, 0.3), Out::Close);
    }

    #[test]
    fn setup_pairs_and_shows_games() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut ui = StreamUi::new(&mut cx, load(Mode::Normal, &[]), None, Some(SheetKind::Setup), 0.0);
        // The unpaired PC found on the network is the default row.
        assert_eq!(ui.top().unwrap().current_act(), Some(Act::HostRow(DEN.into())));
        ui.pad(&mut cx, Pad::A, 0.0);
        assert!(ui.st.pairing.is_some());
        for i in 1..=10 {
            ui.tick(&mut cx, i as f64);
        }
        assert_eq!(ui.st.pairing.as_ref().unwrap().phase, PairPhase::Paired);
        assert_eq!(ui.top().unwrap().current_act(), Some(Act::ShowGames));
        assert_eq!(ui.pad(&mut cx, Pad::A, 11.0), Out::Act("stream:show-library".into()));
    }

    #[test]
    fn b_in_setup_cancels_the_pairing() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut ui = StreamUi::new(&mut cx, load(Mode::Pairing, &[]), None, Some(SheetKind::Setup), 0.0);
        ui.pad(&mut cx, Pad::A, 0.0);
        assert!(ui.st.pairing.is_some());
        assert_eq!(ui.pad(&mut cx, Pad::B, 0.1), Out::Close);
        assert!(ui.st.pairing.is_none());
    }

    #[test]
    fn get_moonlight_installs_with_progress() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut ui = StreamUi::new(&mut cx, load(Mode::NoMoonlight, &[]), None, Some(SheetKind::Setup), 0.0);
        assert_eq!(ui.top().unwrap().current_act(), Some(Act::GetMoonlight));
        ui.pad(&mut cx, Pad::A, 0.0);
        for i in 1..=30 {
            ui.tick(&mut cx, i as f64 * 0.1);
        }
        assert!(ui.st.installing.is_none() && ui.st.moonlight.is_some());
    }

    #[test]
    fn moonlight_comes_by_itself_and_pairing_waits_for_it() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut ui = StreamUi::new(&mut cx, load(Mode::NoMoonlight, &[]), None, Some(SheetKind::Setup), 0.0);
        ui.need_moonlight(&mut cx, 0.0);
        assert!(ui.st.installing.is_some(), "streaming opened: Rare gets Moonlight without a Get press");
        ui.tick(&mut cx, 0.1);
        if let Some(h) = ui.st.hosts.iter().find(|h| !h.paired).cloned() {
            assert_eq!(ui.run(&mut cx, Act::HostRow(h.uuid.clone()), 0.15), Out::Moved);
            assert_eq!(ui.st.pending_pair.as_deref(), Some(h.uuid.as_str()));
        }
        for i in 2..=40 {
            ui.tick(&mut cx, i as f64 * 0.1);
        }
        assert!(ui.st.installing.is_none() && ui.st.moonlight.is_some());
        assert!(ui.st.pending_pair.is_none());
    }

    #[test]
    fn quality_rows_cycle_and_save() {
        let (mut sc, mut t, mut img) = env();
        let mut cx = Cx { scene: &mut sc, t: &mut t, img: &mut img };
        let mut ui = StreamUi::new(&mut cx, load(Mode::Normal, &[]), None, Some(SheetKind::App(PC.into(), CYBER)), 0.0);
        ui.focus_act(&mut cx, &Act::ChooseBitrate, 0.0);
        ui.pad(&mut cx, Pad::A, 0.1);
        assert_eq!(ui.st.settings.bitrate_mbps, 30);
        ui.focus_act(&mut cx, &Act::ChooseResolution, 0.0);
        ui.pad(&mut cx, Pad::A, 0.2);
        assert_eq!(ui.st.settings.resolution, "1280x800");
    }
}
