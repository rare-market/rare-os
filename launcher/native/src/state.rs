//! App state, the native `useApp` (launcher/src/lib/store.ts): the tab, the overlay, the library,
//! what runs, toasts, and the sheets the shell hosts (Options, the keyboard). Screens read it and
//! rebuild when the parts they read change (`State::ver`); handlers change it through methods that
//! bump those versions.

use crate::data::model::{Device, Game, Library};
use crate::data::live::Live;
use std::collections::HashMap;

/// The top tabs plus the two destinations that are not in the bar (store.ts `Tab`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Home,
    Library,
    Games,
    Market,
    Wallet,
    Friends,
    Settings,
}

/// The top tabs (store.ts TABS, plus Friends).
pub const TABS: [(Tab, &str); 6] = [(Tab::Home, "Home"), (Tab::Library, "Library"), (Tab::Games, "Sui Games"), (Tab::Market, "Market"), (Tab::Wallet, "Wallet"), (Tab::Friends, "Friends")];

impl Tab {
    pub fn parse(s: &str) -> Option<Tab> {
        Some(match s {
            "home" => Tab::Home,
            "library" => Tab::Library,
            "games" | "sui" => Tab::Games,
            "market" | "trade" => Tab::Market,
            "wallet" => Tab::Wallet,
            "friends" => Tab::Friends,
            "settings" => Tab::Settings,
            _ => return None,
        })
    }
    pub fn index(self) -> Option<usize> {
        TABS.iter().position(|(t, _)| *t == self)
    }
}

/// Full-screen pages and sheets over the tab (store.ts `Overlay`). Only the Phase 1 kinds are here;
/// the others join with their screens.
#[derive(Clone, Debug, PartialEq)]
pub enum Overlay {
    Game(String),
    Search,
    Qam,
    LaunchFail { id: String, reason: String, code: Option<i32> },
    /// Overlays other workstreams own (sign-in, first run, the Rare menu, the Switcher...): shown as
    /// a placeholder page until their screens land.
    Other(&'static str),
}

impl Overlay {
    pub fn kind(&self) -> &'static str {
        match self {
            Overlay::Game(_) => "game",
            Overlay::Search => "search",
            Overlay::Qam => "qam",
            Overlay::LaunchFail { .. } => "launchfail",
            Overlay::Other(k) => k,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub tone: Tone,
    /// Seconds (clock::secs) it goes away.
    pub until: f64,
}

/// Launch progress for a run (lib/progress.ts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Steam,
    Game,
    /// Steam waits for the player (a cloud save conflict, a license): what it asks, in plain words.
    Answer(&'static str),
    Playing,
    None,
}

#[derive(Clone, Copy, Debug)]
pub struct Start {
    pub phase: Phase,
    /// Seconds (clock::secs) the run started.
    pub since: f64,
}

/// After this long without a window the stage says "Still starting. B to cancel."
pub const STILL_STARTING: f64 = 15.0;

/// The line the stage shows for a run, or None once it plays (progress.ts startLine).
pub fn start_line(title: &str, s: Option<&Start>, now: f64) -> Option<String> {
    let s = s?;
    if matches!(s.phase, Phase::Playing | Phase::None) {
        return None;
    }
    if let Phase::Answer(q) = s.phase {
        return Some(q.into());
    }
    if now - s.since >= STILL_STARTING {
        return Some("Still starting. B to cancel.".into());
    }
    Some(if s.phase == Phase::Steam { "Starting Steam".into() } else { format!("Starting {title}") })
}

/// The start B would cancel: the newest one still going after STILL_STARTING (progress.ts cancellable).
pub fn cancellable(starts: &HashMap<String, Start>, now: f64) -> Option<String> {
    let mut slow: Vec<(&String, &Start)> = starts.iter().filter(|(_, s)| matches!(s.phase, Phase::Steam | Phase::Game) && now - s.since >= STILL_STARTING).collect();
    slow.sort_by(|a, b| b.1.since.partial_cmp(&a.1.since).unwrap_or(std::cmp::Ordering::Equal));
    slow.first().map(|(id, _)| (*id).clone())
}

/// One row of the Options or View sheet (components/Options.tsx).
#[derive(Clone, Debug)]
pub struct OptionItem {
    pub label: String,
    /// The current choice in a View list (a check in front of it).
    pub on: Option<bool>,
    pub act: crate::act::Act,
}

#[derive(Clone, Debug)]
pub struct OptionsSpec {
    pub title: String,
    pub sub: Option<String>,
    pub items: Vec<OptionItem>,
}

/// What the keyboard's text is for (the web's askText promise, as data).
#[derive(Clone, Debug, PartialEq)]
pub enum OskFor {
    Search,
    WebGameUrl,
    WebGameTitle { url: String },
    /// An answer for the Friends actions (crate::social): the tag says which.
    Social(String),
    /// A text field nothing listens to (the ?osk screenshot state).
    Nothing,
    /// A screen asked (`State::ask_text`); the answer lands in `State::osk_answer` under this key
    /// for that screen to take (`State::take_answer`).
    Screen(String),
    /// A collection's name: a new one (with `game` in it), or `rename` given a new name.
    Collection { rename: Option<String>, game: Option<String> },
    /// Settings asked: the key says which (`query`, `wifi:<ssid>`, `folder`, `gridkey`,
    /// `endpoint:<field>`). The answer waits in `State::settings_input` for the app.
    Settings(String),
    /// Same game as another: a name to look for (the game's id); features/copies.rs.
    SameGame(String),
}

#[derive(Clone, Debug)]
pub struct OskReq {
    pub title: String,
    pub value: String,
    pub placeholder: Option<String>,
    pub secret: bool,
    pub purpose: OskFor,
    /// A live line under the field the asking screen keeps current (Friends' "Found name.sui").
    pub status: Option<String>,
}

impl OskReq {
    pub fn new(title: impl Into<String>, value: impl Into<String>, placeholder: Option<&str>, purpose: OskFor) -> OskReq {
        OskReq { title: title.into(), value: value.into(), placeholder: placeholder.map(String::from), secret: false, purpose, status: None }
    }
}

/// What the Rare menu's Game tab shows besides the game: read on a worker while games run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamePerf {
    /// The running games these were read for.
    pub running: Vec<String>,
    pub perf: Option<crate::data::model::Perf>,
    /// The game whose power settings Quick settings changes, and whether they are kept for it.
    pub power: Option<(String, bool)>,
    /// Emulator rows (Save state, Load state, Emulator menu) of the running games.
    pub emu: Vec<rare_emu::actions::EmuAction>,
}

/// Quick settings' device state (types.ts Perf, Levels, Radios, HudLevel, GamePowerView).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Qs {
    pub perf: Option<crate::data::model::Perf>,
    pub levels: Option<crate::data::model::Levels>,
    pub radios: Option<crate::data::model::Radios>,
    pub hud: String,
    pub game: Option<(String, bool)>,
    pub dnd: bool,
    /// The game in front's frame generation, and what the device can do (None: no game).
    pub fx: Option<(rare_system::gamefx::FrameGen, rare_system::gamefx::Status)>,
}

/// Versions of the parts of the state screens read; a screen rebuilds when one it reads moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ver {
    pub library: u64,
    pub running: u64,
    pub device: u64,
    pub live: u64,
    pub favs: u64,
    pub starts: u64,
    pub search: u64,
    pub catalog: u64,
    pub qs: u64,
    pub pc: u64,
    pub account: u64,
    /// The social service revision the friends data was read at.
    pub friends: u64,
}

pub struct State {
    /// The live friends data Home's strip reads (None: no service, or fixtures without friends).
    pub friends: Option<std::sync::Arc<crate::screens::friends::model::FriendsData>>,
    pub tab: Tab,
    pub overlay: Option<Overlay>,
    pub library: Option<Library>,
    pub scanning: bool,
    pub device: Option<Device>,
    /// The performance profile and the running game's saved power settings, read with the device
    /// (the Rare menu's Performance line).
    pub game_perf: GamePerf,
    /// Acts a state change asks the app to run (a finished keyboard), run after the input.
    pub acts: Vec<crate::act::Act>,
    /// A tool on the network is sending controller input (top bar icon).
    pub remote_input: bool,
    pub running: Vec<String>,
    pub launching: Option<String>,
    pub toasts: Vec<Toast>,
    pub history: Vec<Tab>,
    pub live: Live,
    pub starts: HashMap<String, Start>,
    pub favs: Vec<String>,
    /// Collections and hidden games (`ver.favs` moves with them).
    pub user: crate::data::userlib::UserLib,
    /// Copies of the same game, shown as one (data/groups.rs); rebuilt with the library and the
    /// player's merges and splits.
    pub groups: crate::data::groups::Groups,
    /// Copies the player renamed (game-custom.json titles): their name names the group.
    pub renamed: std::collections::HashSet<String>,
    /// Rare's own play sessions, merged into the library's playtime and last played.
    pub played: crate::data::played::Played,
    pub options: Option<OptionsSpec>,
    pub osk: Option<OskReq>,
    /// The keyboard's answer for a screen that asked: (key, Some(text) when finished, None when
    /// cancelled). Taken by the asking screen on its next tick; dropped after one update.
    pub osk_answer: Option<(String, Option<String>)>,
    /// A Switcher card was closed: show the Switcher again without it.
    pub reopen_switcher: bool,
    /// A Try again waiting for the old run to close.
    pub retry: Option<String>,
    /// A quiet rescan due at this time (a game closed, a games folder changed).
    pub rescan_at: Option<f64>,
    /// When the library last arrived (clock::secs).
    pub last_scan: f64,
    /// The search the Search page shows (set when the keyboard finishes).
    pub search: String,
    /// The search before the keyboard opened on it (results follow the typing live; B puts it back).
    pub search_before: Option<String>,
    /// What the network found for `search`: Rare Marketplace collections and coins.
    pub search_hits: crate::screens::search::Hits,
    /// What a keyboard Settings asked for returned: (key, text or None when cancelled).
    pub settings_input: Option<(String, Option<String>)>,
    /// Steam is about to take the screen until this time (SteamHandoff).
    pub handoff_until: Option<f64>,
    /// The art behind Home: the last two (key, sources), the newest last (Backdrop.tsx layers).
    pub backdrop: Vec<(String, Vec<String>)>,
    /// Art a page wants decoded and on the GPU before it switches to it (a game id and its images,
    /// the page's own; the app requests them and reports in `art_ready`).
    pub art_want: Vec<(String, Vec<crate::kit::art::ArtWant>)>,
    /// The ids in `art_want` whose images are all on the GPU (or gave up).
    pub art_ready: std::collections::HashSet<String>,
    /// What Quick settings shows (read from the backend when it opens).
    pub qs: Qs,
    /// Home's account: the current Rare Wallet account, its value and items (None: no account).
    pub account: Option<crate::data::account::HomeAccount>,
    /// Home's "On your PC" row (None: no paired PC answers, the row is hidden).
    pub pc: Option<crate::data::model::PcShelf>,
    /// The Sui Games catalog (active games, rare.market's order).
    pub sui_games: Vec<crate::data::catalog::SuiGame>,
    pub ver: Ver,
    toast_seq: u64,
    /// What the features (first run, trophies, saves, Local API sheets) keep.
    pub feat: crate::features::Shared,
}

impl Default for State {
    fn default() -> Self {
        State {
            tab: Tab::Home,
            overlay: None,
            library: None,
            scanning: false,
            device: None,
            game_perf: GamePerf::default(),
            acts: Vec::new(),
            remote_input: false,
            running: Vec::new(),
            launching: None,
            toasts: Vec::new(),
            history: Vec::new(),
            live: Live::default(),
            starts: HashMap::new(),
            favs: Vec::new(),
            user: Default::default(),
            groups: Default::default(),
            renamed: Default::default(),
            played: Default::default(),
            options: None,
            osk: None,
            osk_answer: None,
            retry: None,
            reopen_switcher: false,
            rescan_at: None,
            last_scan: 0.0,
            search: String::new(),
            search_before: None,
            search_hits: Default::default(),
            settings_input: None,
            handoff_until: None,
            backdrop: Vec::new(),
            art_want: Vec::new(),
            art_ready: Default::default(),
            sui_games: Vec::new(),
            pc: None,
            account: None,
            qs: Qs::default(),
            friends: None,
            ver: Ver::default(),
            toast_seq: 0,
            feat: Default::default(),
        }
    }
}

impl State {
    pub fn game(&self, id: &str) -> Option<&Game> {
        let l = self.library.as_ref()?;
        l.games.iter().find(|g| g.id == id).or_else(|| l.owned.iter().find(|g| g.id == id)).or_else(|| l.cloud.iter().find(|g| g.id == id))
    }

    /// Cloud games (Library > Cloud): never in `games`, so Home, All, counts and the profile leave
    /// them out.
    pub fn cloud(&self) -> &[Game] {
        self.library.as_ref().map(|l| l.cloud.as_slice()).unwrap_or(&[])
    }

    /// Owned Steam games that are not installed (not in `games`).
    pub fn owned(&self) -> &[Game] {
        self.library.as_ref().map(|l| l.owned.as_slice()).unwrap_or(&[])
    }

    pub fn games(&self) -> &[Game] {
        self.library.as_ref().map(|l| l.games.as_slice()).unwrap_or(&[])
    }

    /// What a run id is called (store.ts runName): an app, a stream, a game.
    pub fn run_name(&self, id: &str) -> String {
        if id.starts_with("app:") {
            return crate::screens::apps::catalog::app_by_run_id(id).map(|a| a.name.clone()).unwrap_or_else(|| "The app".into());
        }
        if id.starts_with("stream:") {
            return "The stream".into();
        }
        // A web game the library doesn't know (added by address): its window's title.
        let web = || (rare_webgame::current_id().as_deref() == Some(id)).then(rare_webgame::current_title).flatten();
        self.game(id).map(|g| g.title.clone()).or_else(web).unwrap_or_else(|| "The game".into())
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.iter().any(|r| r == id)
    }

    /// A favourite: any copy of the game is (favourites follow the group).
    pub fn is_fav(&self, id: &str) -> bool {
        self.copy_ids(id).iter().any(|c| self.favs.iter().any(|f| f == c))
    }

    /// The ids of every copy of the game `id` is a copy of, the default first ([id] alone).
    pub fn copy_ids(&self, id: &str) -> Vec<String> {
        self.groups.ids(id)
    }

    /// Every copy of the game, the default (installed, played last) first.
    pub fn copies(&self, id: &str) -> Vec<&Game> {
        self.copy_ids(id).iter().filter_map(|c| self.game(c)).collect()
    }

    /// The copy as the views show it: the group's art, title, last played and playtime.
    pub fn shown<'a>(&'a self, g: &'a Game) -> &'a Game {
        self.groups.shown(g)
    }

    /// One entry per game out of `v` (data/groups.rs `one_each`).
    pub fn one_each<'a>(&'a self, v: impl IntoIterator<Item = &'a Game>) -> Vec<&'a Game> {
        self.groups.one_each(v)
    }

    /// In the collection: any copy of the game is.
    pub fn in_collection(&self, name: &str, id: &str) -> bool {
        self.copy_ids(id).iter().any(|c| self.user.in_collection(name, c))
    }

    /// Puts the game in the collection or takes every copy out; true when it is in now.
    pub fn toggle_in(&mut self, name: &str, id: &str) -> bool {
        if self.in_collection(name, id) {
            for c in self.copy_ids(id) {
                if self.user.in_collection(name, &c) {
                    self.user.toggle_in(name, &c);
                }
            }
            false
        } else {
            self.user.toggle_in(name, id)
        }
    }

    /// Hides the game (every copy) or brings it back; true when hidden now.
    pub fn toggle_hidden(&mut self, id: &str) -> bool {
        if self.is_hidden(id) {
            let ids = self.copy_ids(id);
            self.user.hidden.retain(|h| !ids.contains(h));
            false
        } else {
            self.user.toggle_hidden(id)
        }
    }

    /// Groups the library's copies again (after a scan, a play session, a merge or split).
    pub fn regroup(&mut self) {
        self.groups = match self.library.as_ref() {
            Some(l) => crate::data::groups::Groups::build(l, &self.user, &self.renamed),
            None => Default::default(),
        };
    }

    /// LB/RB, a tile or a link: switch tabs, remembering where we were for B (store.ts setTab).
    pub fn set_tab(&mut self, tab: Tab, back: bool) {
        if tab == self.tab {
            return;
        }
        let cur = self.tab;
        if back {
            self.history.pop();
        } else {
            self.history.retain(|t| *t != tab);
            self.history.push(cur);
            if self.history.len() > 8 {
                self.history.remove(0);
            }
        }
        self.tab = tab;
    }

    pub fn open(&mut self, o: Option<Overlay>) {
        self.overlay = o;
    }

    pub fn close(&mut self) {
        self.overlay = None;
    }

    /// B: close the overlay, else go back a tab, else Home. False when there is nowhere to go.
    pub fn back(&mut self) -> bool {
        if self.overlay.is_some() {
            self.close();
            return true;
        }
        if let Some(prev) = self.history.last().copied() {
            self.set_tab(prev, true);
            return true;
        }
        if self.tab != Tab::Home {
            self.set_tab(Tab::Home, true);
            return true;
        }
        false
    }

    pub fn toast(&mut self, text: impl Into<String>, tone: Tone, now: f64) {
        self.toast_for(text, tone, now, 3.0);
    }

    /// A toast that leaves after `secs`, at most 3 s (toasts never linger; fixtures may hold one
    /// for a shot with a long time).
    pub fn toast_for(&mut self, text: impl Into<String>, tone: Tone, now: f64, secs: f64) {
        let secs = if secs >= 600.0 { secs } else { secs.min(3.0) };
        self.toast_seq += 1;
        self.toasts.push(Toast { id: self.toast_seq, text: text.into(), tone, until: now + secs });
    }

    pub fn set_library(&mut self, mut lib: Library) {
        lib.split_cloud();
        // A zero or placeholder time (before 2001) is not a real "last played": unknown, so no
        // date shows for it anywhere.
        for g in lib.games.iter_mut().chain(lib.owned.iter_mut()).chain(lib.cloud.iter_mut()) {
            if g.last_played.is_some_and(|t| t < crate::data::model::REAL_TIME) {
                g.last_played = None;
            }
        }
        self.played.apply(&mut lib, crate::data::model::now_secs());
        self.library = Some(lib);
        self.regroup();
        self.ver.library += 1;
        self.last_scan = rare_render::clock::secs();
    }

    pub fn set_running(&mut self, running: Vec<String>) {
        if running != self.running {
            crate::features::localapi::note_running(&running, self.library.as_ref());
            crate::data::live::set_game_running(!running.is_empty());
            self.running = running;
            self.ver.running += 1;
            self.note_played();
        }
    }

    /// Opens and closes Rare's play sessions for what runs now, and puts the new numbers into
    /// the library at once.
    pub fn note_played(&mut self) {
        let now = crate::data::model::now_secs();
        if self.played.note_running(&self.running, now) {
            if let Some(lib) = self.library.as_mut() {
                self.played.apply(lib, now);
                self.regroup();
                self.ver.library += 1;
            }
        }
    }

    /// Hidden by the player (only the Library's Hidden view shows it).
    pub fn is_hidden(&self, id: &str) -> bool {
        self.copy_ids(id).iter().any(|c| self.user.is_hidden(c))
    }

    /// Collections or hidden games changed: save them and rebuild what shows them.
    pub fn user_changed(&mut self) {
        self.user.save();
        self.regroup();
        self.ver.favs += 1;
    }

    /// Shows a game's art behind Home (Backdrop.tsx setBackdrop); None clears it.
    pub fn set_backdrop(&mut self, b: Option<(String, Vec<String>)>) {
        match b {
            None => self.backdrop.clear(),
            Some(b) => {
                if self.backdrop.last().map(|l| &l.0) != Some(&b.0) {
                    if self.backdrop.len() > 1 {
                        self.backdrop.remove(0);
                    }
                    self.backdrop.push(b);
                }
            }
        }
    }

    /// Opens the keyboard for a screen (the web's askText). `key` names the question; the answer
    /// comes back through `take_answer(key)`.
    pub fn ask_text(&mut self, key: impl Into<String>, title: impl Into<String>, value: impl Into<String>, placeholder: Option<&str>) {
        self.osk = Some(OskReq::new(title, value, placeholder, OskFor::Screen(key.into())));
    }

    /// Like `ask_text`, the typed text shown as dots.
    pub fn ask_secret(&mut self, key: impl Into<String>, title: impl Into<String>, placeholder: Option<&str>) {
        let mut r = OskReq::new(title, "", placeholder, OskFor::Screen(key.into()));
        r.secret = true;
        self.osk = Some(r);
    }

    /// The keyboard's answer to `key`: Some(Some(text)) when finished, Some(None) when cancelled,
    /// None while it is still up (or nothing was asked).
    pub fn take_answer(&mut self, key: &str) -> Option<Option<String>> {
        if self.osk_answer.as_ref().is_some_and(|(k, _)| k == key) {
            return self.osk_answer.take().map(|(_, v)| v);
        }
        None
    }

    /// The text typed so far when the keyboard is up for `key` (for live status lines).
    pub fn asking(&self, key: &str) -> Option<&str> {
        let o = self.osk.as_ref()?;
        matches!(&o.purpose, OskFor::Screen(k) if k == key).then_some(o.value.as_str())
    }

    /// Sets the line under the keyboard's field while it is up for `key`.
    pub fn set_osk_status(&mut self, key: &str, status: Option<String>) {
        if let Some(o) = self.osk.as_mut() {
            if matches!(&o.purpose, OskFor::Screen(k) if k == key) && o.status != status {
                o.status = status;
            }
        }
    }

    pub fn toggle_fav(&mut self, id: &str) -> bool {
        let on = if self.is_fav(id) {
            let ids = self.copy_ids(id);
            self.favs.retain(|f| !ids.contains(f));
            false
        } else {
            self.favs.push(id.to_string());
            true
        };
        self.ver.favs += 1;
        // Every way to favourite (X on the game page, Options) keeps it over a restart.
        crate::app::save_favs(&self.favs);
        on
    }
}

/// Asks for a collection name: a new collection (with `game` in it) or a new name for `rename`.
pub fn ask_collection(st: &mut State, rename: Option<String>, game: Option<String>) {
    let (title, value) = match &rename {
        Some(old) => ("New name for the collection", old.clone()),
        None => ("Name for the new collection", String::new()),
    };
    st.osk = Some(OskReq::new(title, value, Some("Co-op nights"), OskFor::Collection { rename, game }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_answers_the_asking_screen() {
        let mut st = State::default();
        st.ask_text("wallet:watch", "Watch", "", Some("0x..."));
        assert_eq!(st.asking("wallet:watch"), Some(""));
        st.osk.as_mut().unwrap().value.push_str("rare.sui");
        st.set_osk_status("wallet:watch", Some("Found".into()));
        assert_eq!(st.osk.as_ref().unwrap().status.as_deref(), Some("Found"));
        let req = st.osk.take().unwrap();
        crate::screens::osk::finish(&mut st, req, Some("rare.sui".into()));
        assert_eq!(st.take_answer("other"), None);
        assert_eq!(st.take_answer("wallet:watch"), Some(Some("rare.sui".into())));
        assert_eq!(st.take_answer("wallet:watch"), None);
        st.ask_secret("pin", "PIN", None);
        let req = st.osk.take().unwrap();
        crate::screens::osk::finish(&mut st, req, None);
        assert_eq!(st.take_answer("pin"), Some(None));
    }

    #[test]
    fn launch_failed_opens_the_sheet_and_crash_toasts() {
        use rare_core::events::LaunchFailed;
        let mut st = State::default();
        crate::app::on_event(&mut st, rare_core::Event::LaunchFailed(LaunchFailed { id: "steam:1".into(), reason: "quick", code: Some(1) }), 1.0);
        assert_eq!(st.overlay, Some(Overlay::LaunchFail { id: "steam:1".into(), reason: "quick".into(), code: Some(1) }));
        st.close();
        crate::app::on_event(&mut st, rare_core::Event::LaunchFailed(LaunchFailed { id: "steam:1".into(), reason: "crash", code: None }), 1.0);
        assert!(st.overlay.is_none());
        assert_eq!(st.toasts.last().unwrap().text, "The game closed unexpectedly.");
    }
}
