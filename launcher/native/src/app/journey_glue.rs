//! The journey probe (rare-journey; WS 16): `tools/journeys` drives the native launcher with the
//! same files and log lines as the web launcher's nav/journey.ts. Off unless the runner's flag
//! exists; while off it costs one file check per pad press, at most every 2 s.

use super::*;
use rare_journey::{Cmd, Focus, Probe, Screen};
use std::sync::Mutex;
use std::time::Instant;

/// The probe and what it has seen.
pub(super) struct Journey {
    pub probe: Probe,
    toast: u64,
    /// The keyboard's title last logged (`osk` lines).
    osk: Option<String>,
    /// The last `topbar` line logged.
    topbar: String,
    /// `quit:` results from the worker, logged on the UI thread.
    quits: Arc<Mutex<Vec<(String, Option<String>)>>>,
    /// The crawler (tools/journeys/crawl.py): every act run and every backend job started.
    pub acts: Vec<String>,
    pub jobs: u64,
}

impl Journey {
    pub fn new() -> Journey {
        Journey { probe: Probe::from_env(), toast: 0, osk: None, topbar: String::new(), quits: Arc::default(), acts: Vec::new(), jobs: 0 }
    }
}

/// The web's tab ids (store.ts TABS).
fn tab_id(t: Tab) -> &'static str {
    match t {
        Tab::Home => "home",
        Tab::Library => "library",
        Tab::Games => "games",
        Tab::Market => "market",
        Tab::Wallet => "wallet",
        Tab::Friends => "friends",
        Tab::Settings => "settings",
    }
}

/// The text inside a node, whitespace folded (nav/debug.ts describe).
fn text_in(scene: &Scene, id: Id, out: &mut String) {
    if out.len() > 80 {
        return;
    }
    if let Some(t) = scene.get(id).text.as_ref() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&t.text);
    }
    for c in scene.children(id) {
        text_in(scene, c, out);
    }
}

impl App {
    fn journey_overlay(&self) -> Option<(String, Option<String>)> {
        if self.legacy.is_some() {
            return Some((if self.switcher_up { "running" } else if self.power_up { "power" } else { "raremenu" }.into(), None));
        }
        Some(match self.st.overlay.as_ref()? {
            Overlay::Game(id) => ("game".into(), Some(id.clone())),
            Overlay::LaunchFail { id, .. } => ("launchfail".into(), Some(id.clone())),
            o => (o.kind().to_string(), None),
        })
    }

    /// Logs the screen and focus lines (when they changed), toasts and quit results.
    pub(super) fn journey_tick(&mut self) {
        if !self.journey.probe.on() {
            return;
        }
        let ov = self.journey_overlay();
        let screen = Screen { tab: tab_id(self.st.tab), overlay: ov.as_ref().map(|(k, id)| (k.as_str(), id.as_deref())), running: self.st.running.len(), launching: self.st.launching.as_deref() };
        self.journey.probe.screen(&screen);
        let (text, rect, group) = self.focus_desc();
        let f = self.focus_of(&text, rect, group.as_deref());
        self.journey.probe.focus(&f);
        for t in self.st.toasts.iter().filter(|t| t.id > self.journey.toast) {
            self.journey.probe.toast(if t.tone == Tone::Error { "error" } else { "info" }, &t.text);
        }
        if let Some(t) = self.st.toasts.iter().map(|t| t.id).max() {
            self.journey.toast = self.journey.toast.max(t);
        }
        // The top bar fits: the tabs end before the right group (profile chip, icons, clock) starts.
        if let (Some(t), Some(r)) = (self.top.view.id("topbar-tabs"), self.top.view.id("topbar-right")) {
            let (t, r) = (self.scene.rect(t), self.scene.rect(r));
            // And no tab is cut: the last tab ends inside the tab row (nav > [LB, tabs, RB]).
            let tabs_id = self.top.view.id("topbar-tabs");
            let strip = tabs_id.and_then(|n| self.scene.children(n).get(1).copied());
            let cut = strip.is_some_and(|row| {
                let rr = self.scene.rect(row);
                self.scene.children(row).last().is_some_and(|l| {
                    let lr = self.scene.rect(*l);
                    lr[0] + lr[2] > rr[0] + rr[2] + 0.5
                })
            });
            let fits = !cut && t[0] + t[2] <= r[0] + 0.5 && r[0] + r[2] <= self.scene.w + 0.5;
            let line = format!("topbar tabs_end={} right_start={} right_end={} fits={fits}", (t[0] + t[2]).round(), r[0].round(), (r[0] + r[2]).round());
            if line != self.journey.topbar {
                self.journey.probe.log(&line);
                self.journey.topbar = line;
            }
        }
        // The keyboard came up (or went): what it is for, so a journey can end on it.
        let osk = self.st.osk.as_ref().map(|o| o.title.clone());
        if osk != self.journey.osk {
            self.journey.probe.log(&format!("osk {}", osk.as_deref().unwrap_or("-")));
            self.journey.osk = osk;
        }
        let quits = std::mem::take(&mut *self.journey.quits.lock().unwrap());
        for (id, err) in quits {
            self.journey.probe.quit_result(&id, err.as_deref());
        }
    }

    /// The focus as the journeys probe describes it, and whether the ring is hidden (headless
    /// `held:` / `flick:` steps print it).
    pub fn focus_debug(&self) -> String {
        let (text, rect, group) = self.focus_desc();
        format!("focus {text:?} at {:?} group {group:?} ring_hidden {}", rect.map(|(x, y, w, h)| (x as i32, y as i32, w as i32, h as i32)), self.scene.ring_hidden)
    }

    fn focus_desc(&self) -> (String, Option<(f32, f32, f32, f32)>, Option<String>) {
        // A scene-built surface (the Rare menu, the Switcher) has its own focus.
        if let Some((screen, _)) = self.legacy.as_ref() {
            let Some(id) = screen.focus_id() else { return (String::new(), None, None) };
            let mut text = String::new();
            text_in(&self.scene, id, &mut text);
            // A picture tile (the Switcher's cards) has its caption under it, in its parent.
            if text.trim().is_empty() {
                if let Some(p) = self.scene.parent(id) {
                    text_in(&self.scene, p, &mut text);
                }
            }
            let r = self.scene.rect(id);
            return (text, Some((r[0], r[1], r[2], r[3])), None);
        }
        // A scene-built tab page (Friends, Market, Settings) has its own focus: the node whose
        // focus ring is on.
        let page_up = self.legacy_page.is_some() && self.st.overlay.is_none() && self.st.options.is_none() && self.st.osk.is_none();
        let ring = || {
            let mut found = None;
            let mut stack = vec![self.scene.root];
            while let Some(id) = stack.pop() {
                // A hidden subtree (a section not shown) may keep its last ring.
                if self.scene.get(id).hidden || self.scene.get(id).opacity.target() < 0.01 {
                    continue;
                }
                if self.scene.get(id).ring.target() > 0.5 {
                    found = Some(id);
                }
                let mut kids = self.scene.children(id);
                kids.reverse();
                stack.extend(kids);
            }
            found.map(|id| {
                let mut text = String::new();
                text_in(&self.scene, id, &mut text);
                let r = self.scene.rect(id);
                (text, Some((r[0], r[1], r[2], r[3])), None)
            })
        };
        if page_up {
            // The page's own focus first (its Nav), the ring only when it has none.
            if let Some(id) = self.legacy_page.as_ref().and_then(|(_, p, _)| p.focus_id()) {
                let mut text = String::new();
                text_in(&self.scene, id, &mut text);
                let r = self.scene.rect(id);
                return (text, Some((r[0], r[1], r[2], r[3])), None);
            }
            if let Some(f) = ring() {
                return f;
            }
        }
        let Some(it) = self.nav.current_item() else { return ring().unwrap_or((String::new(), None, None)) };
        let mut text = String::new();
        text_in(&self.scene, it.id, &mut text);
        if text.trim().is_empty() {
            text = it.label.clone().unwrap_or_default();
        }
        let r = self.scene.rect(it.id);
        let group = it.group.and_then(|g| self.nav.groups.get(g)).map(|g| g.name.clone()).filter(|n| !n.is_empty());
        (text, Some((r[0], r[1], r[2], r[3])), group)
    }

    fn focus_of<'a>(&self, text: &'a str, rect: Option<(f32, f32, f32, f32)>, group: Option<&'a str>) -> Focus<'a> {
        match rect {
            None => Focus::nothing(),
            Some(rect) => Focus { tag: "button", group, text, rect, nothing: false },
        }
    }

    fn journey_state(&mut self) {
        let ov = self.journey_overlay();
        let screen = Screen { tab: tab_id(self.st.tab), overlay: ov.as_ref().map(|(k, id)| (k.as_str(), id.as_deref())), running: self.st.running.len(), launching: self.st.launching.as_deref() };
        let (text, rect, group) = self.focus_desc();
        let f = self.focus_of(&text, rect, group.as_deref());
        self.journey.probe.after_commands(&screen, &f);
    }

    /// Every pad press the UI handled (turns the probe on when the runner's flag is there).
    pub(super) fn journey_pad(&mut self, b: Btn, repeat: bool) {
        if self.journey.probe.pad(b.name(), repeat, Instant::now()) {
            self.journey_state();
        }
    }

    /// The runner's setup commands. Returns the next time to poll while the probe is on.
    pub(super) fn journey_poll(&mut self) -> Option<f64> {
        if !self.journey.probe.on() {
            return None;
        }
        let cmds = self.journey.probe.poll(Instant::now());
        let mut applied = false;
        for c in cmds {
            applied = true;
            match c {
                Cmd::Tab(t) => {
                    self.close_legacy();
                    self.st.close();
                    // tab:library:<filter> opens the Library on that filter (android, steam, ...).
                    if let Some(f) = t.strip_prefix("library:") {
                        crate::screens::library::show_filter(f);
                        self.st.set_tab(Tab::Library, false);
                        continue;
                    }
                    if let Some(t) = Tab::parse(&t) {
                        self.st.set_tab(t, false);
                    }
                }
                Cmd::OpenGame(id) => {
                    self.close_legacy();
                    self.st.open(Some(Overlay::Game(id)));
                }
                Cmd::OpenApp(id) => {
                    self.close_legacy();
                    self.open_app(&id, true);
                }
                Cmd::Open(k) => match k.as_str() {
                    "running" => self.open_switcher(),
                    "downloads" => {
                        self.close_legacy();
                        self.st.open(Some(Overlay::Other(crate::features::downloads::KIND)));
                    }
                    "qam" => {
                        self.close_legacy();
                        self.st.open(Some(Overlay::Qam));
                    }
                    "fan-curve" => {
                        self.close_legacy();
                        self.st.open(Some(Overlay::Other(crate::features::fancurve::KIND)));
                    }
                    "search" => {
                        self.close_legacy();
                        self.st.open(Some(Overlay::Search));
                        self.ask_search();
                    }
                    "profile" => {
                        self.close_legacy();
                        self.st.open(Some(Overlay::Other("profile")));
                    }
                    _ => {}
                },
                Cmd::Close => {
                    self.close_legacy();
                    self.st.close();
                    self.st.options = None;
                    self.st.osk = None;
                }
                Cmd::Quit(id) => {
                    let ids = if id == "all" { self.st.running.clone() } else { vec![id] };
                    let out = self.journey.quits.clone();
                    self.fx.spawn(move |b| {
                        for id in ids {
                            let r = b.quit(&id);
                            out.lock().unwrap().push((id, r.err()));
                        }
                        let running = b.running();
                        Box::new(move |st: &mut State, _| st.set_running(running))
                    });
                }
                Cmd::State | Cmd::Echo(_) | Cmd::Unknown(_) => {}
            }
        }
        if applied {
            // Lay the change out first so the focus line reports the new screen.
            let now = clock::secs();
            self.reconcile(now);
            self.build_all(now, false);
            self.settle(now);
            self.journey_state();
        }
        Some(clock::secs() + 0.25)
    }

    /// Headless journeys (tools/journeys/ux.py): the probe on from the first frame, so the
    /// screen and focus before the first press are in the log too.
    pub fn journey_begin(&mut self) {
        if self.journey.probe.pad("begin", false, Instant::now()) {
            self.journey_state();
        }
    }

    /// What a press finally did (a shared act or a Rare menu row): `act <what>`.
    pub(super) fn journey_act(&mut self, what: &str) {
        if self.journey.probe.on() {
            self.journey.acts.push(what.to_string());
        }
        self.journey.probe.log(&format!("act {what}"));
    }

    /// The crawler's view of the app (RR_DRIVE): screen, focus, keyboard, toasts, acts and jobs
    /// so far, as one JSON object.
    pub fn crawl_state(&self) -> String {
        let ov = self.journey_overlay();
        let (text, rect, group) = self.focus_desc();
        let toasts: Vec<_> = self.st.toasts.iter().map(|t| serde_json::json!({"id": t.id, "text": t.text, "error": t.tone == Tone::Error})).collect();
        serde_json::json!({
            "tab": tab_id(self.st.tab),
            "overlay": ov.as_ref().map(|o| o.0.clone()),
            "overlay_id": ov.as_ref().and_then(|o| o.1.clone()),
            "legacy": self.legacy.is_some(),
            "page": self.legacy_page.is_some(),
            "options": self.st.options.is_some(),
            "share": self.share.is_some(),
            "osk": self.st.osk.as_ref().map(|o| o.title.clone()),
            "focus": {"text": text.trim(), "rect": rect.map(|r| [r.0.round(), r.1.round(), r.2.round(), r.3.round()]), "group": group},
            "toasts": toasts,
            "acts": self.journey.acts,
            "jobs": self.journey.jobs,
            "running": self.st.running,
            "launching": self.st.launching,
        })
        .to_string()
    }

    /// After each present (main.rs): `painted` after a screen change.
    pub fn presented(&mut self) {
        self.journey.probe.presented();
    }
}
