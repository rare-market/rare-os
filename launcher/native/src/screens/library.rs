//! Library (screens/Library.tsx): every game, art only, the focused
//! game's name over the grid. LT/RT switch the source filter (only filters with games; Streaming
//! always shows), View opens sort and filter in one list. A sticky filter bar under the top bar.
//! The player's collections sit behind one Collections tab (a grid of them; A opens one, B goes
//! back), so the tab row stays the same length however many there are.

use crate::act::Act;
use crate::data::model::{fmt_ago, pretty_path, source_label, Game};
use crate::kit::{self, s, tok, Btn as B, BtnIcon, DotTone, Pb, TabItem, Track, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::screens::game::parts;
use crate::screens::home::{app_tile, pc_tile, APPS};
use crate::state::{OptionItem, OptionsSpec, State};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El};

const SOURCES: [&str; 13] = ["stream", "emu", "media", "cloud", "android", "rare", "steam", "epic", "gog", "itch", "local", "web", "apps"];
/// The Downloads view's "Not installed games" asks the Library to open on that filter.
static WANT_NOT_INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn show_not_installed() {
    WANT_NOT_INSTALLED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// A filter the Library opens on next (the journey probe's `tab:library:<filter>`).
static WANT_FILTER: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn show_filter(f: &str) {
    // The names people (and the journey probe) use for the tabs, then any unknown one is All:
    // a filter that matches nothing showed an empty "No games here yet" under the All tab.
    let f = match f {
        "installed" | "games" => "all",
        "emulation" | "roms" => "emu",
        "favourites" | "favorites" => "favs",
        "collections" => COLS,
        "streaming" => "stream",
        "not-installed" | "notInstalled" => "notinstalled",
        f => f,
    };
    let known = ["all", "favs", "hidden", COLS, "notinstalled"].contains(&f) || SOURCES.contains(&f) || f.starts_with("col:");
    *WANT_FILTER.lock().unwrap() = Some(if known { f } else { "all" }.to_string());
}

const SORTS: [(&str, &str); 3] = [("recent", "Recently played"), ("name", "Name"), ("played", "Most played")];
/// Room the bar's actions take (the Downloads icon and Add), rem.
const BAR_ACTIONS_REM: f32 = 8.4;
/// The Add sheet (under the collection cards' codes, which start at 400).
const ACT_ADD_MENU: u32 = 310;

/// A filter tab's width in rem, estimated from its label and count (Body text, Meta count).
fn tab_rem(t: &TabItem) -> f32 {
    let chars = t.label.chars().count() as f32 * 0.53;
    let count = t.count.map(|c| 0.4 + c.to_string().len() as f32 * 0.5).unwrap_or(0.0);
    chars + count + 1.25
}

/// The run of tabs `[from, to)` that fits in `room` rem and holds `cur`: from the first tab when
/// it reaches `cur`, else as far left as it can, then as many after as fit.
fn tab_window(widths: &[f32], cur: usize, room: f32) -> (usize, usize) {
    if widths.is_empty() {
        return (0, 0);
    }
    let cur = cur.min(widths.len() - 1);
    let mut from = 0;
    while from < cur && widths[from..=cur].iter().sum::<f32>() > room {
        from += 1;
    }
    let mut used: f32 = widths[from..=cur].iter().sum();
    let mut to = cur + 1;
    while to < widths.len() && used + widths[to] <= room {
        used += widths[to];
        to += 1;
    }
    (from, to)
}

/// View sheet codes past the sorts and filters.
const ACT_NEW_COLLECTION: u32 = 300;
const ACT_RENAME: u32 = 301;
const ACT_DELETE: u32 = 302;
const ACT_DELETE_YES: u32 = 303;
/// Collection cards in the Collections tab: 400 + their index.
const ACT_OPEN_COLLECTION: u32 = 400;
/// The Collections tab's filter.
const COLS: &str = "cols";
/// Emulation: system chips 600 + their index, More (every system in a sheet), a system from it.
const ACT_SYSTEM: u32 = 600;
const ACT_SYSTEMS_MORE: u32 = 690;
const ACT_SYSTEM_PICK: u32 = 700;
/// The name and count band under a collection card's covers (rem).
const BAND: f32 = 3.6;

/// A filter's tab label: "all", "favs", "hidden", "col:<name>" or a source.
fn label(f: &str) -> String {
    match f {
        "all" => "All".into(),
        "favs" => "Favourites".into(),
        "hidden" => "Hidden".into(),
        COLS => "Collections".into(),
        "media" => "Apps".into(),
        "stream" => "Streaming".into(),
        "notinstalled" => "Not installed".into(),
        f => match f.strip_prefix("col:") {
            Some(name) => name.to_string(),
            None => source_label(f).to_string(),
        },
    }
}

pub struct Library {
    filter: String,
    sort: &'static str,
    /// The focused game: its name and id, shown over the grid with its played line.
    focused: Option<(String, Option<String>)>,
    /// The filters the open View sheet listed, by its codes.
    menu: Vec<String>,
    /// The focused game's id and the grid's ids at the last build: when the focused game leaves
    /// (uninstalled), focus takes the tile now in its place and the grid stays where it is.
    focused_id: Option<String>,
    shown_ids: Vec<String>,
    want: Option<u32>,
    /// The Collections tab: the cards' collection names by index, the focused one, and the one
    /// to focus when B comes back from it.
    cols: Vec<String>,
    focus_col: Option<String>,
    back_to: Option<String>,
    /// Emulation: the system shown (None: all of them), and the chips' and More sheet's systems.
    system: Option<String>,
    chips: Vec<Option<String>>,
    sys_menu: Vec<String>,
}

impl Default for Library {
    fn default() -> Self {
        Library { filter: "all".into(), sort: "recent", focused: None, menu: Vec::new(), focused_id: None, shown_ids: Vec::new(), want: None, cols: Vec::new(), focus_col: None, back_to: None, system: None, chips: Vec::new(), sys_menu: Vec::new() }
    }
}

impl Library {
    /// Whether a game belongs under filter `f` (hidden games only under Hidden).
    fn fits(st: &State, f: &str, g: &Game) -> bool {
        if f == "hidden" {
            return st.is_hidden(&g.id);
        }
        if st.is_hidden(&g.id) {
            return false;
        }
        match f {
            // Owned Epic and GOG games that are not installed stay under their store. Cloud games
            // are not in `games` at all (State::cloud); ROMs (often hundreds) stay under Emulation. Android's app stores (F-Droid, Aurora) stay under Android.
            "all" => g.installed && g.source != "emu" && !rare_android::is_store(&g.id),
            COLS => false,
            "favs" => st.is_fav(&g.id),
            // Installs in progress here; the owned games that aren't installed join in `games`.
            "notinstalled" => !g.installed,
            f => match f.strip_prefix("col:") {
                Some(name) => st.in_collection(name, &g.id),
                None => g.source == f,
            },
        }
    }

    /// The games filter `f` looks through: the cloud games only under Cloud, Favourites, Hidden
    /// and collections (what the player picked).
    fn pool<'a>(st: &'a State, f: &str) -> impl Iterator<Item = &'a Game> {
        let cloud = f == "cloud" || f == "favs" || f == "hidden" || f.starts_with("col:");
        st.games().iter().chain(st.cloud().iter().filter(move |_| cloud))
    }

    fn count(st: &State, f: &str) -> usize {
        match f {
            "media" => APPS.len(),
            "stream" => st.pc.as_ref().map(|p| p.apps.len()).unwrap_or(0),
            "notinstalled" => st.one_each(st.owned().iter().filter(|g| !st.is_hidden(&g.id)).chain(st.games().iter().filter(|g| Library::fits(st, f, g)))).len(),
            COLS => st.user.collections.len(),
            // One per game: copies of one game count once.
            f => st.one_each(Library::pool(st, f).filter(|g| Library::fits(st, f, g))).len(),
        }
    }

    /// The tabs: All, Favourites, Collections (one tab for all of them), Streaming and the
    /// sources with games, Hidden when anything is hidden. The current tab always stays.
    fn shown(&self, st: &State) -> Vec<String> {
        let mut v = vec!["all".to_string(), "favs".to_string(), COLS.to_string()];
        // Streaming always; Android whenever this system can run it (its setup lives there).
        let android = crate::features::android::status() != rare_android::Status::Unavailable;
        // Emulator apps live inside Emulation (its Emulators section), not in a tab of their own.
        v.extend(SOURCES.iter().filter(|f| **f == "stream" || **f == "emu" || (**f == "android" && android) || **f == self.filter || (**f != "apps" && Library::count(st, f) > 0)).map(|f| f.to_string()));
        if !st.user.hidden.is_empty() || self.filter == "hidden" {
            v.push("hidden".into());
        }
        // Not installed last, so LT from All reaches it in one press (the tabs wrap).
        if self.filter == "notinstalled" || Library::count(st, "notinstalled") > 0 {
            v.push("notinstalled".into());
        }
        v
    }

    /// The tab the filter sits under: a collection is under Collections.
    fn tab(&self) -> &str {
        if self.collection().is_some() {
            COLS
        } else {
            &self.filter
        }
    }

    /// The collection Rename and Delete act on: the open one, or the focused card.
    fn target(&self) -> Option<String> {
        match self.collection() {
            Some(c) => Some(c.to_string()),
            None if self.filter == COLS => self.focus_col.clone(),
            None => None,
        }
    }

    fn set_filter(&mut self, f: String) {
        self.system = None;
        self.filter = f;
        self.focused = None;
        self.focused_id = None;
        self.shown_ids.clear();
    }

    fn games<'a>(&self, st: &'a State) -> Vec<&'a Game> {
        let mut v: Vec<&Game> = Library::pool(st, &self.filter).filter(|g| Library::fits(st, &self.filter, g)).collect();
        if self.filter == "notinstalled" {
            v.extend(st.owned().iter().filter(|g| !st.is_hidden(&g.id)));
        }
        if let Some(sys) = self.system.as_deref().filter(|_| self.filter == "emu") {
            v.retain(|g| rare_emu::roms::system_of(&g.id).is_some_and(|s| s.id == sys));
        }
        // One tile per game: the copy to show of each (data/groups.rs), with the group's art and times.
        let mut v = st.one_each(v);
        match self.sort {
            "name" => v.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
            "played" => v.sort_by(|a, b| b.playtime_minutes.unwrap_or(0).cmp(&a.playtime_minutes.unwrap_or(0)).then_with(|| b.last_played.unwrap_or(0).cmp(&a.last_played.unwrap_or(0)))),
            _ => v.sort_by(|a, b| b.last_played.unwrap_or(0).cmp(&a.last_played.unwrap_or(0)).then_with(|| a.title.cmp(&b.title))),
        }
        // Installed first (stable: the sort above stays within each group).
        v.sort_by_key(|g| !g.installed);
        v
    }

    fn collection(&self) -> Option<&str> {
        self.filter.strip_prefix("col:")
    }

    /// Keeps the filter valid after a collection was renamed or deleted.
    fn settle(&mut self, st: &State) {
        if let Some(f) = self.focus_col.as_ref().filter(|f| st.user.collection(f).is_none()) {
            self.focus_col = match &st.user.renamed {
                Some((old, new)) if old == f => Some(new.clone()),
                _ => None,
            };
        }
        if let Some(name) = self.collection() {
            if st.user.collection(name).is_none() {
                self.filter = match &st.user.renamed {
                    Some((old, new)) if old == name => format!("col:{new}"),
                    _ => COLS.into(),
                };
            }
        }
    }
}

impl Page for Library {
    fn stamp(&self, st: &State) -> u64 {
        let want = (WANT_NOT_INSTALLED.load(std::sync::atomic::Ordering::SeqCst), WANT_FILTER.lock().unwrap().is_some());
        let emu = if self.filter == "emu" { crate::features::emulation::ver() } else { 0 };
        hash(((crate::features::apps::shared().map(|a| a.borrow().version).unwrap_or(0), crate::features::android::ver()), st.ver.library, st.ver.running, st.ver.pc, st.ver.favs, st.scanning, &self.filter, self.sort, &self.focused, (st.feat.downloads.ver, want, crate::features::stores::ver(), emu, &self.system)))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        if let Some(f) = WANT_FILTER.lock().unwrap().take() {
            self.set_filter(f);
        }
        if WANT_NOT_INSTALLED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.filter = "notinstalled".into();
            self.focused = None;
        }
        self.settle(st);
        let shown = self.shown(st);
        let value = shown.iter().position(|f| f == self.tab()).unwrap_or(0);
        let all_items: Vec<TabItem> = shown.iter().map(|f| TabItem::new(label(f)).count(Library::count(st, f) as u32)).collect();
        let filter_own = self.filter.clone();
        let filter = filter_own.as_str();
        let lt = kit::glyph(ui, Pb::Lt, kit::GSize::Sm);
        let rt = kit::glyph(ui, Pb::Rt, kit::GSize::Sm);
        // The tabs that fit beside the actions, the current one always among them (LT/RT reach
        // the rest); the row clips whatever an estimate gets wrong, so it never overflows.
        let room = ui.w / ui.rem - 5.0 - 2.0 * 1.6 - 3.0 * 1.1 - BAR_ACTIONS_REM - 1.0;
        let widths: Vec<f32> = all_items.iter().map(tab_rem).collect();
        let (from, to) = tab_window(&widths, value, room);
        let items: Vec<TabItem> = all_items[from..to].to_vec();
        let tabs = ui.bx(s().row().items_stretch().grow().min0()).clip().child(kit::tabs(ui, s().hp(1.0), &items, value - from, true));
        // One Add button (a game from a file, a web game, a PC on Streaming): a sheet, so the bar
        // keeps its room for the filters.
        let add = kit::button(ui, B::new("Add", acts.tag(Act::Local(ACT_ADD_MENU))).sm().icon(BtnIcon::Lucide("plus", 1.0, false)).hint("Add"));
        // Library > Downloads: an icon button so the filter tabs keep their room.
        let downloads = kit::button(ui, B::new("", acts.tag(Act::Open(crate::state::Overlay::Other(crate::features::downloads::KIND)))).sm().gap(0.0).icon(BtnIcon::Lucide("download", 1.05, false)).hint("Downloads"));
        // The sticky filter bar: h-[3.5rem] border-b border-hair bg-bg, flush under the top bar.
        let bar = ui
            .bx(s().row().items_stretch().gap(1.1).h(3.5).mb(1.0).mx(-2.5).px(2.5))
            .look(rare_render::Look::default().bg(tok::BG).z(4))
            .sticky()
            .name(kit::PIN_BAR)
            .child(ui.bx(s().row().items_center()).child(lt))
            .child(tabs)
            .child(ui.bx(s().row().items_center()).child(rt))
            .child(ui.bx(s().row().items_center().gap(0.6).ml_auto()).child(downloads).child(add))
            .child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(1.0)).bg(tok::HAIR));
        let games = self.games(st);
        let count = match filter {
            "media" => format!("{} apps", APPS.len()),
            "stream" => match st.pc.as_ref().map(|p| p.apps.len()).unwrap_or(0) {
                0 => String::new(),
                n => format!("{n} on your PC"),
            },
            "android" => match games.len() {
                0 => String::new(),
                1 => "1 app".into(),
                n => format!("{n} apps"),
            },
            _ if st.scanning => "Scanning".into(),
            _ => format!("{} {}", games.len(), if games.len() == 1 { "game" } else { "games" }),
        };
        let count = match filter {
            COLS => match st.user.collections.len() {
                0 => String::new(),
                1 => "1 collection".into(),
                n => format!("{n} collections"),
            },
            _ => count,
        };
        let games_view = filter != "media" && filter != "stream" && filter != COLS;
        let (title, played) = match self.focused.as_ref() {
            Some((t, id)) if games_view => (t.clone(), id.as_deref().and_then(|id| st.game(id)).map(|g| played_line(st.shown(g))).unwrap_or_default()),
            // A collection card: its name and how many games.
            Some((t, None)) if filter == COLS => (t.clone(), st.user.collection(t).map(|c| games_word(Library::col_games(st, c).len())).unwrap_or_default()),
            _ => (String::new(), String::new()),
        };
        // The open collection's name before the count, cut to fit.
        let crumb = self.collection().map(|c| ui.text_s(s().ml_auto().shrink().min0().max_w(26.0), c.to_string(), T::new(Ty::Small).w(500).c(tok::fg(0.85)).trunc()));
        let count = match (crumb.is_some(), count.is_empty()) {
            (true, false) => format!("\u{b7} {count}"),
            _ => count,
        };
        let count_s = if crumb.is_some() { s() } else { s().ml_auto() };
        // The focused game's name and its played line; one fixed-height row, so nothing moves.
        let head = ui
            .bx(s().row().items_center().gap(0.75).h(1.75).mb(0.75))
            .child(ui.text_s(s().shrink().min0(), title, T::new(Ty::Row).w(600).trunc()))
            .child(ui.text_s(s().shrink().min0(), played, T::new(Ty::Small).c(tok::MUTED).trunc()))
            .maybe(crumb.map(|c| c.key(key("crumb"))))
            .child(ui.text_s(count_s, count, T::new(Ty::Small).c(tok::MUTED)).key(key("count")));
        let mut page = ui.bx(s().col().full().px(2.5).pb(2.5).pt(1.0)).scrolls_y(key("library")).child(bar).child(head);

        let foreign = st.library.as_ref().map(|l| l.foreign_steam_libraries.clone()).unwrap_or_default();
        if !foreign.is_empty() && games_view && filter != "notinstalled" && filter != "android" && filter != "emu" {
            let n = st.games().iter().filter(|g| g.foreign).count();
            let paths = foreign.iter().map(|p| pretty_path(p)).collect::<Vec<_>>().join(", ");
            let btn = kit::button(ui, B::new("Add to Steam", acts.tag(Act::SteamAddLibrary(foreign[0].clone()))));
            let note = kit::panel(ui, s().row().items_center().gap(1.0).px(1.25).py(1.0).mb(1.5))
                .child(kit::icon(ui, "folder", 1.4, tok::ICON2))
                .child(
                    ui.bx(s().col().grow())
                        .child(ui.text("Games found from your previous system", T::new(Ty::Body).w(600)))
                        .child(ui.text(format!("{n} games in {paths}. They play from here; add the folder to Steam so Steam keeps them updated."), T::new(Ty::Small).c(tok::MUTED).trunc())),
                )
                .child(btn);
            page = page.child(note.key(key("foreign")));
        }

        // grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-[1rem]
        let width = ui.w / ui.rem - 5.0;
        let cols = (((width + 1.0) / 10.0).floor() as usize).max(1);
        let tw = (width - (cols as f32 - 1.0)) / cols as f32;
        let grid_cols: Vec<Track> = (0..cols).map(|_| Track::Fr(1.0)).collect();
        let body = match filter {
            "media" => {
                let tiles: Vec<El> = APPS.iter().enumerate().map(|(i, (id, n, icon))| app_tile(ui, acts, id, n, icon, i == 0, st.is_running(&crate::screens::apps::run_id(id)))).collect();
                ui.bx(s().grid(&grid_cols).gap_xy(1.1, 1.4)).group(Group::new("library-media", [0.0; 4])).kids(tiles)
            }
            "stream" => ui.bx(s().col().gap(2.25)).child(match st.pc.as_ref().filter(|p| !p.apps.is_empty()) {
                Some(pc) => {
                    // Every paired PC with its games (stream/tiles.tsx StreamGrid).
                    let (dot, status) = if pc.online {
                        (DotTone::Primary, if pc.current_game.is_some() { "Playing on the PC" } else { "Online" })
                    } else {
                        (DotTone::Muted, if pc.can_wake { "Asleep" } else { "Off" })
                    };
                    let note = ui.bx(s().row().items_center().gap(0.45)).child(kit::dot(ui, s(), dot, false)).child(ui.text(status, T::new(Ty::Small).c(tok::MUTED)));
                    let head = ui.bx(s().row().items_center().gap(0.75).mb(0.9)).child(ui.text(pc.name.clone(), T::new(Ty::Row).w(600).track(-0.01))).child(note);
                    // The first game takes focus when the tab opens (not the bar's Downloads button).
                    let tiles: Vec<El> = pc.apps.iter().enumerate().map(|(i, a)| pc_tile(ui, st, acts, pc, a, tw, i == 0)).collect();
                    let grid = ui.bx(s().grid(&grid_cols).gap_xy(1.1, 1.4)).group(Group::new("library-stream", [0.0; 4])).kids(tiles);
                    ui.bx(s().col()).child(head).child(grid)
                }
                None => {
                    let set = kit::button(ui, B::new("Set up streaming", acts.tag(Act::PcSetup)).primary().def(true).icon(BtnIcon::Lucide("monitor", 1.0, false)).hint("Set up streaming"));
                    kit::empty(ui, s(), "Play the games on your PC", Some("Stream from a gaming PC running Sunshine or GeForce Experience, over your home Wi-Fi. Rare gets Moonlight, the app that streams, the first time."), vec![set])
                }
            })
            .child(more_ways(ui, acts)),
            COLS => self.collections_grid(ui, st, acts, &grid_cols, tw),
            "android" => android_body(ui, st, acts, &games, &grid_cols, tw),
            "emu" => self.emulation(ui, st, acts, &games, &grid_cols, tw),
            "favs" if games.is_empty() => kit::empty(ui, s(), "No favourites yet", Some("Press Menu on a game and pick Add to favourites, or press X on its page."), vec![]),
            "notinstalled" if games.is_empty() && !st.scanning => kit::empty(ui, s(), "Every game you own is installed", Some("Steam games your account owns show here when they are not on this device."), vec![]),
            "hidden" if games.is_empty() => kit::empty(ui, s(), "Nothing hidden", Some("Games you hide leave the library and Home. They show here so you can bring them back."), vec![]),
            f if f.starts_with("col:") && games.is_empty() => kit::empty(ui, s(), "Nothing in this collection yet", Some("Press Menu on a game and pick Add to collection."), vec![]),
            _ if games.is_empty() && !st.scanning => {
                let scan = kit::button(ui, B::new("Scan again", acts.tag(Act::Scan)).primary().def(true));
                kit::empty(ui, s(), "No games here yet", Some("Install a game in Steam, or put a game folder in Games on this device or your SD card."), vec![scan])
            }
            _ => {
                let ids: Vec<String> = games.iter().map(|g| g.id.clone()).collect();
                let gone_at = self.focused_id.as_ref().filter(|f| !ids.contains(f)).and_then(|f| self.shown_ids.iter().position(|x| x == f));
                let tiles: Vec<El> = games.iter().enumerate().map(|(i, g)| parts::game_tile(ui, st, acts, g, i == 0, true, tw, None)).collect();
                if let Some(i) = gone_at.filter(|_| !ids.is_empty()) {
                    let target = &ids[i.min(ids.len() - 1)];
                    self.want = (0..).take_while(|t| acts.get(*t).is_some()).find(|t| acts.game_of(*t) == Some(target.as_str()) && matches!(acts.get(*t), Some(Act::OpenGame(_))));
                    self.focused_id = Some(target.clone());
                }
                self.shown_ids = ids;
                ui.bx(s().grid(&grid_cols).gap(1.0)).group(Group::new(&format!("library-{filter}"), [0.0; 4])).kids(tiles)
            }
        };
        page.child(body.key(key(filter)))
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if st.overlay.is_some() {
            return false;
        }
        match b {
            Btn::Select => {
                self.settle(st);
                let mut items: Vec<OptionItem> = SORTS.iter().map(|(v, l)| OptionItem { label: format!("Sort: {l}"), on: Some(self.sort == *v), act: Act::Local(100 + SORTS.iter().position(|x| x.0 == *v).unwrap() as u32) }).collect();
                self.menu = self.shown(st);
                for (i, f) in self.menu.iter().enumerate() {
                    items.push(OptionItem { label: format!("Show: {}", label(f)), on: Some(self.tab() == f.as_str()), act: Act::Local(200 + i as u32) });
                }
                items.push(OptionItem { label: "New collection".into(), on: None, act: Act::Local(ACT_NEW_COLLECTION) });
                if self.target().is_some() {
                    items.push(OptionItem { label: "Rename collection".into(), on: None, act: Act::Local(ACT_RENAME) });
                    items.push(OptionItem { label: "Delete collection".into(), on: None, act: Act::Local(ACT_DELETE) });
                }
                fx.fb(Feedback::Open);
                st.options = Some(OptionsSpec { title: "Library view".into(), sub: Some("Sort, filter and collections".into()), items });
                true
            }
            Btn::Lt | Btn::Rt => {
                self.settle(st);
                let shown = self.shown(st);
                let i = shown.iter().position(|f| f == self.tab()).unwrap_or(0);
                let n = shown.len();
                self.set_filter(shown[(i + if b == Btn::Rt { 1 } else { n - 1 }) % n].clone());
                fx.fb(Feedback::Move);
                true
            }
            Btn::B if self.collection().is_some() => {
                // Out of a collection: back to the Collections grid, on its card.
                self.back_to = self.collection().map(String::from);
                self.set_filter(COLS.into());
                fx.fb(Feedback::Back);
                true
            }
            _ => false,
        }
    }

    fn act(&mut self, st: &mut State, _fx: &mut Fx, code: u32) {
        match code {
            ACT_ADD_MENU => {
                let mut items = vec![
                    OptionItem { label: "A game from a file or folder".into(), on: None, act: Act::Open(crate::state::Overlay::Other(crate::features::gamemgmt::PICK)) },
                    OptionItem { label: "A web game".into(), on: None, act: Act::Local(0) },
                ];
                items.push(OptionItem { label: "A PC to stream from".into(), on: None, act: Act::PcSetup });
                st.options = Some(OptionsSpec { title: "Add to your library".into(), sub: None, items });
            }
            0 => {
                st.osk = Some(crate::state::OskReq::new("Web game address", "", Some("https://game.example"), crate::state::OskFor::WebGameUrl));
            }
            100..=199 => self.sort = SORTS[(code - 100) as usize].0,
            200..=299 => {
                if let Some(f) = self.menu.get((code - 200) as usize).cloned() {
                    self.set_filter(f);
                }
            }
            ACT_NEW_COLLECTION => crate::state::ask_collection(st, None, None),
            ACT_RENAME => {
                if let Some(name) = self.target() {
                    crate::state::ask_collection(st, Some(name), None);
                }
            }
            ACT_DELETE => {
                if let Some(name) = self.target() {
                    st.options = Some(OptionsSpec {
                        title: format!("Delete {name}?"),
                        sub: Some("The games stay in your library.".into()),
                        items: vec![OptionItem { label: "Delete collection".into(), on: None, act: Act::Local(ACT_DELETE_YES) }, OptionItem { label: "Keep it".into(), on: None, act: Act::None }],
                    });
                }
            }
            ACT_DELETE_YES => {
                if let Some(name) = self.target() {
                    if st.user.delete(&name) {
                        st.user_changed();
                        st.toast(format!("{name} deleted"), crate::state::Tone::Info, rare_render::clock::secs());
                    }
                    self.focus_col = None;
                    self.set_filter(COLS.into());
                }
            }
            ACT_SYSTEMS_MORE => {
                let items = self.sys_menu.iter().enumerate().map(|(i, id)| OptionItem { label: rare_emu::systems::by_id(id).map(|s| s.name).unwrap_or(id).to_string(), on: Some(self.system.as_deref() == Some(id.as_str())), act: Act::Local(ACT_SYSTEM_PICK + i as u32) }).collect();
                st.options = Some(OptionsSpec { title: "Systems".into(), sub: Some("Show one system's games".into()), items });
            }
            c if (ACT_SYSTEM_PICK..ACT_SYSTEM_PICK + 100).contains(&c) => {
                if let Some(id) = self.sys_menu.get((c - ACT_SYSTEM_PICK) as usize).cloned() {
                    self.system = Some(id);
                    self.focused = None;
                }
            }
            c if (ACT_SYSTEM..ACT_SYSTEMS_MORE).contains(&c) => {
                if let Some(id) = self.chips.get((c - ACT_SYSTEM) as usize).cloned() {
                    self.system = id;
                    self.focused = None;
                }
            }
            c if c >= ACT_OPEN_COLLECTION => {
                if let Some(name) = self.cols.get((c - ACT_OPEN_COLLECTION) as usize).cloned() {
                    self.set_filter(format!("col:{name}"));
                }
            }
            _ => {}
        }
    }

    fn tap_text(&mut self, st: &mut State, fx: &mut Fx, text: &str) -> bool {
        // A filter tab's label: that tab, as LT/RT would reach it.
        if st.overlay.is_some() {
            return false;
        }
        self.settle(st);
        let Some(f) = self.shown(st).into_iter().find(|f| label(f) == text) else { return false };
        if f != self.tab() {
            self.set_filter(f);
            fx.fb(Feedback::Move);
        }
        true
    }

    fn hints(&self, st: &State) -> Vec<Hint> {
        if st.overlay.is_some() {
            return vec![];
        }
        let mut v = vec![hint(Key::LtRt, "Sections"), hint(Key::Select, "View")];
        if self.collection().is_some() {
            v.push(hint(Key::B, "Collections"));
        }
        v
    }

    fn take_focus(&mut self, _cur: Option<&Focusable>) -> Option<u32> {
        self.want.take()
    }

    fn focused(&mut self, cur: Option<&Focusable>, acts: &Acts, _now: f64) -> bool {
        if let Some(id) = cur.filter(|c| c.scope == crate::app::SCOPE_PAGE).and_then(|c| acts.game_of(c.tag)) {
            self.focused_id = Some(id.to_string());
        }
        if let Some(Act::Local(c)) = cur.filter(|c| c.scope == crate::app::SCOPE_PAGE).and_then(|c| acts.get(c.tag)) {
            if *c >= ACT_OPEN_COLLECTION {
                self.focus_col = self.cols.get((*c - ACT_OPEN_COLLECTION) as usize).cloned();
            }
        }
        let t = cur.and_then(|c| c.label.clone().map(|l| (l, acts.game_of(c.tag).map(String::from))));
        if t.is_some() && t != self.focused {
            self.focused = t;
            return true;
        }
        false
    }
}

/// Library > Android: setting Android up (one press, progress here), then Find on F-Droid and
/// the installed apps, the stores first.
fn android_body(ui: &mut Ui, st: &State, acts: &mut Acts, games: &[&Game], grid_cols: &[Track], tw: f32) -> El {
    use rare_android::Status;
    rare_android::set_screen(ui.w as u32, ui.h as u32);
    let job = rare_android::jobs::current();
    let setup = job.as_ref().filter(|j| j.kind == rare_android::jobs::Kind::Setup);
    let line = |ui: &mut Ui, j: &rare_android::jobs::Job| -> El {
        let text = if j.line.is_empty() { "Starting".to_string() } else { j.line.clone() };
        let bar = match j.pct {
            Some(p) => crate::features::downloads::bar(ui, None, p, false),
            None => {
                // Busy with no measure: a segment that breathes.
                let name = ui.loop_name(&rare_render::anim::BREATHE, 1.3);
                let seg = ui.bx(s().wp(0.35).h_px(4.0)).bg(tok::fg(0.8)).radius(ui.r(0.15)).name(name);
                ui.bx(s().wp(1.0).h_px(4.0)).bg(tok::fg(0.15)).radius(ui.r(0.15)).clip().child(seg)
            }
        };
        ui.bx(s().col().gap(0.6).wp(1.0).max_w(36.0)).child(ui.text_s(s().wp(1.0).min0(), text, T::new(Ty::Small).c(tok::fg(0.85)).trunc())).child(bar)
    };
    match crate::features::android::status() {
        Status::Unavailable => kit::empty(ui, s(), "Android isn't available here", Some("Android apps need Rare OS on this device."), vec![]),
        Status::NeedsUpdate => kit::empty(ui, s(), "Android comes with the next Rare OS update", Some("Update Rare OS in Settings > System. Then set up Android here with one press."), vec![]),
        Status::NotSetUp => match setup {
            Some(j) => {
                let body = line(ui, j);
                let note = ui.text_s(s().wp(1.0).max_w(36.0).mt(0.9), "Getting Android takes a few minutes. You can keep using Rare.", T::new(Ty::Small).c(tok::MUTED).wrap());
                let head = ui.text_s(s().mb(0.9), "Setting up Android", T::new(Ty::Row).w(600));
                ui.bx(s().col().items_center().py(4.0).wp(1.0)).child(head).child(body).child(note)
            }
            None => {
                let go = kit::button(ui, B::new("Set up Android", acts.tag(Act::Legacy("android:setup".into()))).primary().def(true).icon(BtnIcon::Lucide("download", 1.0, false)).hint("Set up"));
                kit::empty(ui, s(), "Play Android games", Some("Rare gets Android without Google apps (about 1 GB) and adds F-Droid and Aurora Store, so you can install games with no Google account."), vec![go])
            }
        },
        Status::Ready => {
            let find = kit::button(ui, B::new("Find on F-Droid", acts.tag(Act::Legacy("android:find".into()))).sm().icon(BtnIcon::Lucide("search", 1.0, false)).hint("Search F-Droid"));
            let mut top = ui.bx(s().row().items_center().gap(1.5).mb(1.25)).group(Group::new("library-android-top", [0.0; 4])).child(find);
            if let Some(j) = job.as_ref().filter(|j| j.kind != rare_android::jobs::Kind::Setup) {
                top = top.child(ui.bx(s().grow().min0()).child(line(ui, j)));
            }
            // The stores first, then the games in the view's order.
            let mut games: Vec<&Game> = games.to_vec();
            games.sort_by_key(|g| !rare_android::is_store(&g.id));
            let tiles: Vec<El> = games.iter().enumerate().map(|(i, g)| parts::game_tile(ui, st, acts, g, i == 0, true, tw, None)).collect();
            let mut col = ui.bx(s().col()).child(top);
            if games.iter().all(|g| rare_android::is_store(&g.id)) {
                col = col.child(ui.text_s(s().mb(1.0).wp(1.0), "Find a game on F-Droid, or open F-Droid or Aurora Store. Games you install show up here.", T::new(Ty::Small).c(tok::MUTED).wrap()));
            }
            col.child(ui.bx(s().grid(grid_cols).gap(1.0)).group(Group::new("library-android", [0.0; 4])).kids(tiles))
        }
    }
}

/// Library > Streaming's other ways to play from another device, each with its setup steps.
const STREAM_APPS: [&str; 2] = ["chiaki", "steamlink"];

/// "Ready", "Installing", "Not installed" for a catalog app (the Apps store's reading).
fn app_state(id: &str) -> &'static str {
    match crate::features::apps::shared() {
        Some(a) => {
            let a = a.borrow();
            if a.installing.get(id).is_some_and(|i| i.error.is_none()) {
                "Installing"
            } else if a.status.get(id).is_some_and(|s| s.way.is_some() && s.offer.is_none()) {
                "Ready"
            } else if a.status.contains_key(id) {
                "Not installed"
            } else {
                "\u{a0}"
            }
        }
        None => "\u{a0}",
    }
}

/// One way to stream: its icon, name and state, and the plain steps. A opens the app's page
/// (Get, then Open), which installs it without leaving Rare.
fn stream_card(ui: &mut Ui, acts: &mut Acts, id: &str) -> Option<El> {
    let app = crate::screens::apps::app_by_id(id)?;
    let state = app_state(id);
    let icon = kit::art(ui, s().size(3.0), std::slice::from_ref(&app.icon), 3.0, 3.0, kit::ArtFit::Contain(0.5, 0.5), None).radius(ui.r(3.0 * 0.22));
    let head = ui
        .bx(s().row().items_center().gap(0.9))
        .child(icon)
        .child(ui.bx(s().col().grow().min0()).child(ui.text(app.name.clone(), T::new(Ty::Body).w(600).trunc())).child(ui.text(state, T::new(Ty::Small).c(tok::MUTED))));
    let mut steps = ui.bx(s().col().gap(0.4));
    for (i, step) in app.steps.iter().enumerate() {
        steps = steps.child(ui.bx(s().row().gap(0.5)).child(ui.text_s(s().w(1.1), format!("{}.", i + 1), T::new(Ty::Small).w(600))).child(ui.text_s(s().grow().min0(), step.clone(), T::new(Ty::Small).c(tok::MUTED).wrap())));
    }
    let card = ui.bx(s().col().gap(1.0).p(1.25)).bg(tok::L2).radius(ui.r(0.5)).child(head).child(steps);
    let hint = if state == "Ready" { "Open" } else { "Get" };
    let fl = card.look.clone().bg(tok::L3).scale(1.02);
    Some(kit::nav(card.focus_look(fl), acts.tag(Act::OpenApp(id.to_string())), Some(hint), None, false).key(key(id)))
}

fn more_ways(ui: &mut Ui, acts: &mut Acts) -> El {
    let cards: Vec<El> = STREAM_APPS.iter().filter_map(|id| stream_card(ui, acts, id)).collect();
    let head = kit::section_head(ui, s().mb(0.9), "More ways to stream", Some("From a PlayStation, or a PC with Steam"), None);
    let grid = ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(1.1)).group(Group::new("library-stream-apps", [0.0; 4])).kids(cards);
    ui.bx(s().col()).child(head).child(grid)
}

impl Library {
    /// Library > Emulation: the setup strip, the system chips (as many as fit, the current one
    /// always among them, the rest behind More), the ROMs, then the emulators themselves.
    fn emulation(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts, games: &[&Game], grid_cols: &[Track], tw: f32) -> El {
        let body = self.emulation_games(ui, st, acts, games, grid_cols, tw);
        match emulators(ui, st, acts, grid_cols, tw) {
            Some(e) => ui.bx(s().col().gap(2.25)).child(body).child(e),
            None => body,
        }
    }

    fn emulation_games(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts, games: &[&Game], grid_cols: &[Track], tw: f32) -> El {
        let all: Vec<&Game> = st.one_each(st.games().iter().filter(|g| g.source == "emu" && !st.is_hidden(&g.id)));
        let mut by_sys: Vec<(&'static rare_emu::systems::System, usize)> = Vec::new();
        for sys in rare_emu::systems::SYSTEMS {
            let n = all.iter().filter(|g| rare_emu::roms::system_of(&g.id).is_some_and(|s| s.id == sys.id)).count();
            if n > 0 {
                by_sys.push((sys, n));
            }
        }
        let empty = all.is_empty() && !st.scanning;
        let mut col = ui.bx(s().col()).child(crate::features::emulation::library_head(ui, acts, empty).key(key("emu-head")));
        if empty {
            let scan = kit::button(ui, B::new("Scan again", acts.tag(Act::Scan)));
            let folders = kit::button(ui, B::new("ROM folders", acts.tag(Act::Open(crate::state::Overlay::Other(crate::features::emulation::SETUP)))).ghost());
            return col.child(kit::empty(ui, s(), "No games here yet", Some("Copy games you own into Emulation/roms on this device or your SD card, in the folder for their system (snes, gba, psx...). Rare finds them on the next scan."), vec![scan, folders]).key(key("emu-empty")));
        }
        // The chips: All, then each system with games.
        let mut chips: Vec<(Option<String>, String)> = vec![(None, format!("All \u{b7} {}", all.len()))];
        chips.extend(by_sys.iter().map(|(sys, n)| (Some(sys.id.to_string()), format!("{} \u{b7} {n}", sys.short))));
        let cur = chips.iter().position(|c| c.0 == self.system).unwrap_or(0);
        let widths: Vec<f32> = chips.iter().map(|c| c.1.chars().count() as f32 * 0.56 + 2.4).collect();
        let more_w = 7.0;
        let room = ui.w / ui.rem - 5.0;
        let (from, to) = if widths.iter().sum::<f32>() <= room { (0, chips.len()) } else { tab_window(&widths, cur, room - more_w) };
        self.chips = chips[from..to].iter().map(|c| c.0.clone()).collect();
        self.sys_menu = by_sys.iter().map(|(s, _)| s.id.to_string()).collect();
        let mut els: Vec<El> = chips[from..to]
            .iter()
            .enumerate()
            .map(|(i, (id, label))| {
                let on = *id == self.system;
                let b = B::new(label.clone(), acts.tag(Act::Local(ACT_SYSTEM + i as u32))).sm().hint("Show");
                kit::button(ui, if on { b.primary() } else { b }).key(key(&("chip", id)))
            })
            .collect();
        if to - from < chips.len() {
            els.push(kit::button(ui, B::new(format!("More {}", chips.len() - (to - from)), acts.tag(Act::Local(ACT_SYSTEMS_MORE))).sm().ghost().icon(BtnIcon::Lucide("list", 1.0, false)).hint("All systems")).key(key("chip-more")));
        }
        col = col.child(ui.bx(s().row().items_center().gap(0.5).mb(1.1).wp(1.0)).clip().group(Group::new("library-emu-systems", [0.0; 4]).remember()).kids(els).key(key("emu-chips")));
        let ids: Vec<String> = games.iter().map(|g| g.id.clone()).collect();
        let tiles: Vec<El> = games.iter().enumerate().map(|(i, g)| parts::game_tile(ui, st, acts, g, i == 0, true, tw, None)).collect();
        self.shown_ids = ids;
        col.child(ui.bx(s().grid(grid_cols).gap(1.0)).group(Group::new(&format!("library-emu-{}", self.system.as_deref().unwrap_or("all")), [0.0; 4])).kids(tiles).key(key(&("emu-grid", &self.system))))
    }

    /// A collection's games that are in the library and not hidden, in the collection's order.
    fn col_games<'a>(st: &'a State, c: &crate::data::userlib::Collection) -> Vec<&'a Game> {
        st.one_each(c.games.iter().filter_map(|id| st.game(id)).filter(|g| !st.is_hidden(&g.id)))
    }

    /// The Collections tab: one card per collection (its first covers, the name cut to fit, how
    /// many games), then New collection. Two game columns per card.
    fn collections_grid(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts, grid_cols: &[Track], tw: f32) -> El {
        self.cols = st.user.collections.iter().map(|c| c.name.clone()).collect();
        let new = acts.tag(Act::Local(ACT_NEW_COLLECTION));
        if self.cols.is_empty() {
            let b = kit::button(ui, B::new("New collection", new).primary().def(true).icon(BtnIcon::Lucide("plus", 1.0, false)));
            return kit::empty(ui, s(), "No collections yet", Some("Press Menu on a game and pick Add to collection."), vec![b]);
        }
        let per = (grid_cols.len() / 2).max(1);
        let gap = 1.0;
        let width = grid_cols.len() as f32 * tw + (grid_cols.len() as f32 - 1.0);
        let cw = (width - gap * (per as f32 - 1.0)) / per as f32;
        let ch = cw * 0.45;
        let back = self.back_to.take();
        let mut cards: Vec<El> = Vec::new();
        for (i, c) in st.user.collections.iter().enumerate() {
            let games = Library::col_games(st, c);
            let tag = acts.tag(Act::Local(ACT_OPEN_COLLECTION + i as u32));
            if back.as_deref() == Some(c.name.as_str()) {
                self.want = Some(tag);
            }
            // Up to three covers side by side, each close to 2:3.
            let pw = cw / 3.0;
            let mut strip = ui.bx(s().fill().row());
            for g in games.iter().take(3) {
                let pic = if parts::icon_art(g) { parts::app_art(ui, g) } else { parts::art(ui, &g.art.cover, &g.title, parts::Variant::Cover, pw, ch, 0.5) };
                strip = strip.child(ui.bx(s().wh(pw, ch)).clip().child(pic).key(key(&g.id)));
            }
            if games.is_empty() {
                strip = ui.bx(s().fill().row().items_center().justify_center()).child(kit::icon(ui, "folder", 2.2, tok::ICON2));
            }
            // The name and count sit inside the card, so the focus ring and the page's scroll
            // take them along.
            let name = ui.text_s(s().wp(1.0).min0(), c.name.clone(), T::new(Ty::Meta).w(500).c(tok::fg(0.9)).trunc());
            let sub = ui.text_s(s(), games_word(games.len()), T::new(Ty::Label).c(tok::MUTED));
            let band = ui.bx(s().col().justify_center().h(BAND).px(1.0).gap(0.2).min0()).child(name).child(sub);
            let btn = ui.bx(s().col().w(cw)).bg(tok::L2).radius(ui.r(0.5)).clip().child(ui.bx(s().wh(cw, ch)).bg(tok::L3).child(strip)).child(band);
            let btn = btn.nav(|f| {
                let f = f.tag(tag).hints(Some("Open"), None, None).label(&c.name);
                if i == 0 {
                    f.default()
                } else {
                    f
                }
            });
            cards.push(kit::tile(btn, 1.03).key(key(&format!("col:{}", c.name))));
        }
        // New collection, last: a card of the same size with a plus.
        let plus = ui.bx(s().wh(cw, ch + BAND).row().items_center().justify_center().gap(0.6)).bg(tok::L2).radius(ui.r(0.5)).child(kit::icon(ui, "plus", 1.4, tok::ICON2)).child(ui.text("New collection", T::new(Ty::Body).w(500).c(tok::fg(0.85))));
        let plus = kit::tile(plus.nav(|f| f.tag(new).hints(Some("Create"), None, None).label("New collection")), 1.03);
        cards.push(plus.key(key("new-collection")));
        let tracks: Vec<Track> = (0..per).map(|_| Track::Fr(1.0)).collect();
        ui.bx(s().grid(&tracks).gap_xy(gap, 1.2)).group(Group::new("library-collections", [0.0; 4])).kids(cards)
    }
}

/// Emulation's Emulators section: the emulator apps (ES-DE and RetroArch first), each opening as
/// on Home. None when there are none.
fn emulators(ui: &mut Ui, st: &State, acts: &mut Acts, grid_cols: &[Track], tw: f32) -> Option<El> {
    let apps = crate::screens::home::sort_apps(st.games().iter().filter(|g| g.source == "apps" && !st.is_hidden(&g.id)).collect());
    if apps.is_empty() {
        return None;
    }
    let head = kit::section_head(ui, s().mb(0.9), "Emulators", Some(&apps.len().to_string()), None);
    let tiles: Vec<El> = apps.iter().map(|g| parts::game_tile(ui, st, acts, g, false, true, tw, None)).collect();
    let grid = ui.bx(s().grid(grid_cols).gap(1.0)).group(Group::new("library-emu-apps", [0.0; 4])).kids(tiles);
    Some(ui.bx(s().col()).child(head).child(grid).key(key("emu-apps")))
}

fn games_word(n: usize) -> String {
    format!("{n} {}", if n == 1 { "game" } else { "games" })
}

/// "Played 2 h ago · 49 h played", or "Not played yet".
fn played_line(g: &Game) -> String {
    if g.source == "apps" {
        return String::new();
    }
    let ago = fmt_ago(g.last_played);
    match parts::fmt_playtime(g.playtime_minutes) {
        p if p.is_empty() => if g.last_played.is_some() { ago } else { "Not played yet".into() },
        p => format!("{ago} \u{b7} {p}"),
    }
}

#[cfg(test)]
mod bar_tests {
    use super::tab_window;

    #[test]
    fn the_current_tab_always_shows() {
        let w = [3.0; 10];
        assert_eq!(tab_window(&w, 0, 10.0), (0, 3));
        assert_eq!(tab_window(&w, 8, 10.0), (6, 9));
        assert_eq!(tab_window(&w, 9, 100.0), (0, 10));
        assert_eq!(tab_window(&w, 4, 1.0), (4, 5));
        assert_eq!(tab_window(&[], 0, 10.0), (0, 0));
    }
}
