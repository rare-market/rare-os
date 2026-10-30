//! Apps on this device (launcher/src/apps/state.ts) and the running list's model (sheets.tsx
//! RunningItem, useRunningItems, resumeItem, quitItem). Backend calls go through `Host`, so
//! rare-launch, rare-overlay and rare-stream replace the fixtures later.

use super::catalog::{app_by_id, app_by_run_id, run_id, AppEntry, AppStatus};
use rare_render::Src;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// `failed()` in lib/fail.ts: a sentence and what to do next.
pub fn failed(what: &str, next: &str) -> String {
    if next.is_empty() {
        what.to_string()
    } else {
        format!("{what} {next}")
    }
}

/// "Press the Rare button" or, in guest mode, "Hold View" (lib/guest.ts rareWay).
pub fn rare_way(guest: bool) -> &'static str {
    if guest {
        "Hold View"
    } else {
        "Press the Rare button"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstallState {
    /// 0 to 1, or None while flatpak hasn't said.
    pub progress: Option<f32>,
    pub error: Option<String>,
}

/// A library game as the running list shows it.
#[derive(Clone, Debug)]
pub struct GameRef {
    pub id: String,
    pub title: String,
    /// The icon art, else the cover.
    pub art: Option<Src>,
}

/// A running stream: the host and the app on it.
#[derive(Clone, Debug)]
pub struct StreamRef {
    pub host: String,
    pub app: String,
    pub art: Option<Src>,
}

/// Everything the apps and the running list ask of the backend.
pub trait Host {
    fn open(&mut self, app: &AppEntry, way: Option<usize>) -> Result<(), String>;
    fn resume_app(&mut self, app: &AppEntry) -> Result<(), String>;
    /// native.quit(runId(app.id))
    fn close_app(&mut self, app: &AppEntry) -> Result<(), String>;
    fn install(&mut self, app: &AppEntry) -> Result<(), String>;
    fn toast(&mut self, msg: &str, error: bool);

    fn game_by_id(&self, rid: &str) -> Option<GameRef>;
    /// stream/state.ts streamOf
    fn stream_of(&self, rid: &str) -> Option<StreamRef>;
    /// stream/state.ts streamTitle
    fn stream_title(&self, rid: &str) -> Option<String>;
    fn resume_game(&mut self, game: &GameRef) -> Result<(), String>;
    fn quit_game(&mut self, game: &GameRef) -> Result<(), String>;
    fn resume_stream(&mut self, s: &StreamRef) -> Result<(), String>;
    fn quit_stream(&mut self, s: &StreamRef) -> Result<(), String>;
    /// native.resume / native.quit for an id nothing else knows (a Sui Games web game).
    fn native_resume(&mut self, rid: &str) -> Result<(), String>;
    fn native_quit(&mut self, rid: &str) -> Result<(), String>;
}

/// What A on a tile does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activation {
    Resume,
    Open,
    /// Nothing can open it yet (or an install runs): the app's page.
    Page,
}

pub struct Apps {
    pub status: HashMap<String, AppStatus>,
    pub installing: HashMap<String, InstallState>,
    /// The launcher's running ids ("app:telegram", "steam:730"...), newest last.
    pub running: Vec<String>,
    pub guest: bool,
    /// Looping motion (the breathing dot, the load bar). Goldens turn it off so the scene settles.
    pub live: bool,
    /// Bumped on every change; open screens compare it in tick() and rebuild.
    pub version: u64,
    /// The way each app was last opened with, so the failure sheet's Try again repeats it.
    pub last_way: HashMap<String, Option<usize>>,
    pub host: Box<dyn Host>,
}

pub type Shared = Rc<RefCell<Apps>>;

impl Apps {
    pub fn new(host: Box<dyn Host>) -> Apps {
        Apps { status: HashMap::new(), installing: HashMap::new(), running: Vec::new(), guest: false, live: true, version: 0, last_way: HashMap::new(), host }
    }

    pub fn shared(self) -> Shared {
        Rc::new(RefCell::new(self))
    }

    pub fn touch(&mut self) {
        self.version += 1;
    }

    pub fn set_status(&mut self, list: Vec<AppStatus>) {
        self.status = list.into_iter().map(|s| (s.id.clone(), s)).collect();
        self.touch();
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.contains(&run_id(id))
    }

    /// Home's "n running" note.
    pub fn running_apps(&self) -> usize {
        self.running.iter().filter(|r| r.starts_with("app:")).count()
    }

    pub fn busy(&self, id: &str) -> bool {
        self.installing.get(id).is_some_and(|i| i.error.is_none())
    }

    /// A on a tile: resume, open, or (nothing can open it yet) the app's page. Does the call.
    pub fn activate(&mut self, app: &AppEntry) -> Activation {
        let s = self.status.get(&app.id);
        if self.is_running(&app.id) {
            self.resume(app);
            return Activation::Resume;
        }
        if s.is_none_or(|s| s.way.is_none()) || self.installing.contains_key(&app.id) {
            return Activation::Page;
        }
        // A streaming app with setup steps whose own app is not in yet (Steam Link opens Steam's
        // library meanwhile): its page first, with the steps and Get.
        if !app.steps.is_empty() && s.is_some_and(|s| s.offer.is_some()) {
            return Activation::Page;
        }
        self.open(app, None);
        Activation::Open
    }

    pub fn open(&mut self, app: &AppEntry, way: Option<usize>) {
        let status_ctrl = self.status.get(&app.id).and_then(|s| s.controller);
        let ctrl = match way {
            Some(i) => app.ways.get(i).and_then(|w| w.controller),
            None => status_ctrl,
        };
        self.last_way.insert(app.id.clone(), way);
        match self.host.open(app, way) {
            Ok(()) => {
                let msg = if ctrl == Some(super::catalog::Controller::Pointer) {
                    format!("{}: the right stick moves the pointer. {} to come back to Rare.", app.name, rare_way(self.guest))
                } else {
                    format!("Opening {}", app.name)
                };
                self.host.toast(&msg, false);
            }
            Err(e) => {
                let m = failed(&format!("{} didn't start.", app.name), "Try again.");
                let _ = e;
                self.host.toast(&m, true);
            }
        }
    }

    /// Try again on a failed start: the same app, the same way. False when `rid` is no app.
    pub fn reopen(&mut self, rid: &str) -> bool {
        let Some(app) = app_by_run_id(rid) else { return false };
        let way = self.last_way.get(&app.id).copied().flatten();
        self.open(app, way);
        true
    }

    pub fn resume(&mut self, app: &AppEntry) {
        if self.host.resume_app(app).is_err() {
            let m = failed(&format!("Couldn't switch back to {}.", app.name), "Try again.");
            self.host.toast(&m, true);
        }
    }

    pub fn close(&mut self, app: &AppEntry) {
        match self.host.close_app(app) {
            Ok(()) => self.host.toast(&format!("Closing {}", app.name), false),
            Err(_) => {
                let m = failed(&format!("Couldn't close {}.", app.name), "Try again.");
                self.host.toast(&m, true);
            }
        }
    }

    pub fn install(&mut self, app: &AppEntry) {
        self.installing.insert(app.id.clone(), InstallState { progress: None, error: None });
        self.touch();
        if self.host.install(app).is_err() {
            let m = failed(&format!("Couldn't install {}.", app.name), "Check your Wi-Fi and try again.");
            self.finish(&app.id, Some(m));
        }
    }

    /// The "app-install" event: progress, or done (with an error or not).
    pub fn install_event(&mut self, id: &str, progress: Option<f32>, done: bool, error: Option<String>) {
        if done {
            return self.finish(id, error);
        }
        self.installing.insert(id.to_string(), InstallState { progress, error: None });
        self.touch();
    }

    fn finish(&mut self, id: &str, error: Option<String>) {
        let app = app_by_id(id);
        self.installing.remove(id);
        if let Some(e) = &error {
            self.installing.insert(id.to_string(), InstallState { progress: None, error: Some(e.clone()) });
            self.host.toast(e, true);
        } else if let Some(app) = app {
            let name = self.status.get(id).and_then(|s| s.offer.as_ref()).map(|o| o.name.clone()).unwrap_or_else(|| app.name.clone());
            self.host.toast(&format!("{name} is installed"), false);
        }
        self.touch();
    }
}

// ---- The running list ----

#[derive(Clone, Debug)]
pub struct RunningItem {
    pub rid: String,
    pub name: String,
    pub app: Option<&'static AppEntry>,
    pub game: Option<GameRef>,
    pub stream: Option<StreamRef>,
}

/// Everything running, newest last. `title` names what the library doesn't (a Sui Games web game).
pub fn running_items(apps: &Apps, title: Option<&dyn Fn(&str) -> Option<String>>) -> Vec<RunningItem> {
    apps.running
        .iter()
        .map(|rid| {
            if let Some(app) = app_by_run_id(rid) {
                return RunningItem { rid: rid.clone(), name: app.name.clone(), app: Some(app), game: None, stream: None };
            }
            let stream = apps.host.stream_of(rid);
            if stream.is_some() || rid.starts_with("stream:") {
                let name = apps.host.stream_title(rid).unwrap_or_else(|| "PC game".into());
                return RunningItem { rid: rid.clone(), name, app: None, game: None, stream };
            }
            let game = apps.host.game_by_id(rid);
            let name = game.as_ref().map(|g| g.title.clone()).or_else(|| title.and_then(|t| t(rid))).unwrap_or_else(|| "Game".into());
            RunningItem { rid: rid.clone(), name, app: None, game, stream: None }
        })
        .collect()
}

/// Puts a running game or app back on screen.
pub fn resume_item(apps: &mut Apps, it: &RunningItem) {
    if let Some(app) = it.app {
        apps.resume(app);
    } else if let Some(s) = &it.stream {
        let _ = apps.host.resume_stream(s);
    } else if let Some(g) = &it.game {
        let _ = apps.host.resume_game(g);
    } else if apps.host.native_resume(&it.rid).is_err() {
        let m = failed("Couldn't switch back.", "Try again.");
        apps.host.toast(&m, true);
    }
}

/// Ends a running game or app.
pub fn quit_item(apps: &mut Apps, it: &RunningItem) {
    if let Some(app) = it.app {
        apps.close(app);
    } else if let Some(s) = &it.stream {
        let _ = apps.host.quit_stream(s);
    } else if let Some(g) = &it.game {
        let _ = apps.host.quit_game(g);
    } else if apps.host.native_quit(&it.rid).is_err() {
        let m = failed("Couldn't close it.", "Try again.");
        apps.host.toast(&m, true);
    }
}

// ---- Fixtures and tests: a recording host ----

/// Records every call; `games`/`streams` answer the lookups. Fixtures and tests use it.
#[derive(Default)]
pub struct MockHost {
    pub calls: Rc<RefCell<Vec<String>>>,
    pub toasts: Rc<RefCell<Vec<(String, bool)>>>,
    pub games: Vec<GameRef>,
    pub streams: Vec<(String, StreamRef, String)>,
    pub fail_native: bool,
}

impl MockHost {
    fn log(&self, s: String) {
        self.calls.borrow_mut().push(s);
    }
}

impl Host for MockHost {
    fn open(&mut self, app: &AppEntry, way: Option<usize>) -> Result<(), String> {
        self.log(format!("open {} {:?}", app.id, way));
        Ok(())
    }
    fn resume_app(&mut self, app: &AppEntry) -> Result<(), String> {
        self.log(format!("resume_app {}", app.id));
        Ok(())
    }
    fn close_app(&mut self, app: &AppEntry) -> Result<(), String> {
        self.log(format!("close_app {}", app.id));
        Ok(())
    }
    fn install(&mut self, app: &AppEntry) -> Result<(), String> {
        self.log(format!("install {}", app.id));
        Ok(())
    }
    fn toast(&mut self, msg: &str, error: bool) {
        self.toasts.borrow_mut().push((msg.to_string(), error));
    }
    fn game_by_id(&self, rid: &str) -> Option<GameRef> {
        self.games.iter().find(|g| g.id == rid).cloned()
    }
    fn stream_of(&self, rid: &str) -> Option<StreamRef> {
        self.streams.iter().find(|s| s.0 == rid).map(|s| s.1.clone())
    }
    fn stream_title(&self, rid: &str) -> Option<String> {
        self.streams.iter().find(|s| s.0 == rid).map(|s| s.2.clone())
    }
    fn resume_game(&mut self, g: &GameRef) -> Result<(), String> {
        self.log(format!("resume_game {}", g.id));
        Ok(())
    }
    fn quit_game(&mut self, g: &GameRef) -> Result<(), String> {
        self.log(format!("quit_game {}", g.id));
        Ok(())
    }
    fn resume_stream(&mut self, s: &StreamRef) -> Result<(), String> {
        self.log(format!("resume_stream {}", s.host));
        Ok(())
    }
    fn quit_stream(&mut self, s: &StreamRef) -> Result<(), String> {
        self.log(format!("quit_stream {}", s.host));
        Ok(())
    }
    fn native_resume(&mut self, rid: &str) -> Result<(), String> {
        self.log(format!("native_resume {rid}"));
        if self.fail_native {
            Err("no".into())
        } else {
            Ok(())
        }
    }
    fn native_quit(&mut self, rid: &str) -> Result<(), String> {
        self.log(format!("native_quit {rid}"));
        if self.fail_native {
            Err("no".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::catalog::{apps, status_for};
    use super::*;

    fn model(installed: &[&str], running: &[&str]) -> (Apps, Rc<RefCell<Vec<String>>>, Rc<RefCell<Vec<(String, bool)>>>) {
        let host = MockHost {
            games: vec![GameRef { id: "steam:730".into(), title: "Counter-Strike 2".into(), art: None }],
            streams: vec![("stream:pc:1".into(), StreamRef { host: "PC".into(), app: "Desktop".into(), art: None }, "Desktop".into())],
            ..Default::default()
        };
        let (calls, toasts) = (host.calls.clone(), host.toasts.clone());
        let mut a = Apps::new(Box::new(host));
        a.set_status(apps().iter().map(|x| status_for(x, installed, true)).collect());
        a.running = running.iter().map(|s| s.to_string()).collect();
        (a, calls, toasts)
    }

    fn app(id: &str) -> &'static AppEntry {
        app_by_id(id).unwrap()
    }

    #[test]
    fn activate_resumes_opens_or_shows_the_page() {
        let (mut a, calls, _) = model(&["com.google.Chrome"], &["app:x"]);
        assert_eq!(a.activate(app("x")), Activation::Resume);
        assert_eq!(a.activate(app("netflix")), Activation::Open);
        // Nothing can open Netflix without Chrome: its page.
        let (mut b, _, _) = model(&[], &[]);
        assert_eq!(b.activate(app("netflix")), Activation::Page);
        assert_eq!(calls.borrow().as_slice(), ["resume_app x", "open netflix None"]);
        // An install in progress sends A to the page even when something could open it.
        a.installing.insert("netflix".into(), InstallState { progress: Some(0.2), error: None });
        assert_eq!(a.activate(app("netflix")), Activation::Page);
    }

    #[test]
    fn try_again_repeats_the_same_way() {
        let (mut a, calls, _) = model(&["com.google.Chrome"], &[]);
        a.open(app("x"), Some(1));
        assert!(a.reopen("app:x"));
        assert!(!a.reopen("steam:730"));
        assert_eq!(calls.borrow().as_slice(), ["open x Some(1)", "open x Some(1)"]);
    }

    #[test]
    fn open_toasts_the_pointer_hint_for_pointer_apps() {
        let (mut a, _, toasts) = model(&["com.google.Chrome"], &[]);
        a.open(app("netflix"), None);
        a.open(app("x"), Some(0));
        a.guest = true;
        a.open(app("netflix"), None);
        let t = toasts.borrow();
        assert_eq!(t[0].0, "Netflix: the right stick moves the pointer. Press the Rare button to come back to Rare.");
        assert_eq!(t[1].0, "Opening X");
        assert_eq!(t[2].0, "Netflix: the right stick moves the pointer. Hold View to come back to Rare.");
    }

    #[test]
    fn install_runs_to_done_and_to_an_error() {
        let (mut a, calls, toasts) = model(&[], &[]);
        a.install(app("discord"));
        assert!(a.busy("discord"));
        assert_eq!(calls.borrow().as_slice(), ["install discord"]);
        a.install_event("discord", Some(0.42), false, None);
        assert_eq!(a.installing["discord"].progress, Some(0.42));
        a.install_event("discord", None, true, None);
        assert!(!a.installing.contains_key("discord"));
        assert_eq!(toasts.borrow().last().unwrap().0, "Discord is installed");
        a.install(app("discord"));
        a.install_event("discord", None, true, Some("Couldn't install Discord. Check your Wi-Fi and try again.".into()));
        assert!(!a.busy("discord"));
        assert!(a.installing["discord"].error.is_some());
        assert_eq!(toasts.borrow().last().unwrap(), &("Couldn't install Discord. Check your Wi-Fi and try again.".to_string(), true));
    }

    #[test]
    fn running_items_name_what_they_are() {
        let (a, _, _) = model(&[], &["steam:730", "app:telegram", "stream:pc:1", "stream:gone", "web:wordgame", "mystery"]);
        let title = |rid: &str| (rid == "web:wordgame").then(|| "Word Game".to_string());
        let items = running_items(&a, Some(&title));
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Counter-Strike 2", "Telegram", "Desktop", "PC game", "Word Game", "Game"]);
        assert!(items[0].game.is_some() && items[1].app.is_some() && items[2].stream.is_some());
        assert!(items[3].stream.is_none() && items[4].game.is_none());
        // Newest last: the model keeps the launcher's order.
        assert_eq!(items.iter().map(|i| i.rid.as_str()).collect::<Vec<_>>()[1], "app:telegram");
    }

    #[test]
    fn resume_and_quit_go_to_the_right_backend() {
        let (mut a, calls, toasts) = model(&[], &["steam:730", "app:telegram", "stream:pc:1", "web:wordgame"]);
        for it in running_items(&a, None) {
            resume_item(&mut a, &it);
            quit_item(&mut a, &it);
        }
        assert_eq!(
            calls.borrow().as_slice(),
            [
                "resume_game steam:730",
                "quit_game steam:730",
                "resume_app telegram",
                "close_app telegram",
                "resume_stream PC",
                "quit_stream PC",
                "native_resume web:wordgame",
                "native_quit web:wordgame"
            ]
        );
        assert_eq!(toasts.borrow().as_slice(), [("Closing Telegram".to_string(), false)]);
    }

    #[test]
    fn a_failed_native_call_toasts() {
        let host = MockHost { fail_native: true, ..Default::default() };
        let toasts = host.toasts.clone();
        let mut a = Apps::new(Box::new(host));
        a.running = vec!["web:x".into()];
        let it = running_items(&a, None).remove(0);
        resume_item(&mut a, &it);
        quit_item(&mut a, &it);
        assert_eq!(toasts.borrow().as_slice(), [("Couldn't switch back. Try again.".to_string(), true), ("Couldn't close it. Try again.".to_string(), true)]);
    }
}
