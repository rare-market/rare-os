//! A game's own settings on its page: launch options (Steam's own for Steam games; arguments and
//! environment for games Rare starts itself), LC and RC for this game on top of Settings >
//! Controls, and the Steam Deck / ProtonDB line.

use crate::act::Act;
use crate::data::compat::Compat;
use crate::data::gameset;
use crate::data::model::Game;
use crate::kit::{self, s, tok, Btn as B, Ty, Ui, T};
use crate::page::{Acts, Feedback, Fx};
use crate::screens::settings::controls::ACTIONS;
use crate::state::{OptionItem, OptionsSpec, State, Tone};
use rare_render::focus::Group;
use rare_render::view::El;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// `Act::Local` codes on the game page (panels.rs uses 1..399).
pub const FIRST: u32 = 1000;
const STEAM_OPTS: u32 = 1000;
const ARGS: u32 = 1001;
const ENV: u32 = 1002;
const LC: u32 = 1010;
const RC: u32 = 1011;
/// + 0 follows Settings, + 1.. is ACTIONS[i - 1].
const LC_PICK: u32 = 1100;
const RC_PICK: u32 = 1200;

const ASK_STEAM: &str = "game:steam-opts";
const ASK_ARGS: &str = "game:args";
const ASK_ENV: &str = "game:env";

#[derive(Default)]
pub struct Shared {
    inner: Mutex<Inner>,
    ver: AtomicU64,
}

#[derive(Default)]
struct Inner {
    compat: Option<Compat>,
    /// Steam's launch options for this game (None: not read yet or no Steam).
    steam: Option<String>,
    /// The first read finished.
    loaded: bool,
    saving: bool,
}

impl Shared {
    pub fn ver(&self) -> u64 {
        self.ver.load(Ordering::Relaxed) + gameset::version()
    }
    fn set(&self, f: impl FnOnce(&mut Inner)) {
        f(&mut self.inner.lock().unwrap());
        self.ver.fetch_add(1, Ordering::Relaxed);
    }
}

fn steam_appid(g: &Game) -> Option<u64> {
    (g.source == "steam" && !g.foreign).then(|| g.id.strip_prefix("steam:")?.parse().ok()).flatten()
}

/// Games Rare starts itself take arguments and environment (a program, or legendary).
fn own_launch(g: &Game) -> bool {
    // Android apps start inside Android: arguments and environment never reach them.
    g.source != "android" && g.launch.as_ref().is_some_and(|l| l.kind == "exec" || l.kind == "legendary")
}

/// First look at the page: the ratings and Steam's launch options.
pub fn load(sh: &Arc<Shared>, fx: &mut Fx, g: &Game) {
    let Some(appid) = steam_appid(g) else { return };
    let sh = sh.clone();
    fx.spawn(move |b| {
        let steam = b.steam_launch_options(appid);
        let compat = b.compat(appid);
        Box::new(move |_, _| {
            sh.set(|i| {
                i.steam = steam;
                i.loaded = true;
                i.compat = Some(compat);
            })
        })
    });
}

/// The ratings for the status line ("Steam Deck Verified", "ProtonDB Gold").
pub fn compat_texts(sh: &Shared) -> Vec<String> {
    let i = sh.inner.lock().unwrap();
    let Some(c) = i.compat.as_ref() else { return vec![] };
    c.deck_text().map(String::from).into_iter().chain(c.protondb_text()).collect()
}

fn action_label(a: Option<&str>) -> &'static str {
    a.and_then(|a| ACTIONS.iter().find(|(k, _)| *k == a)).map(|(_, l)| *l).unwrap_or("Capture")
}

/// What Settings > Controls has for LC or RC.
fn global(lc: bool) -> String {
    let c = crate::screens::settings::config::load();
    if lc {
        c.lc_action.unwrap_or_else(|| "capture".into())
    } else {
        c.rc_action.unwrap_or_else(|| "keyboard".into())
    }
}

fn short(v: &str) -> String {
    let v = v.trim();
    if v.is_empty() {
        return "None".into();
    }
    if v.chars().count() > 26 {
        format!("{}\u{2026}", v.chars().take(25).collect::<String>())
    } else {
        v.to_string()
    }
}

/// The settings rows (hidden while the game runs, like the Proton row: they apply at the next start).
pub fn view(ui: &mut Ui, sh: &Shared, acts: &mut Acts, g: &Game) -> Option<El> {
    let steam = steam_appid(g);
    let own = own_launch(g);
    if steam.is_none() && !own {
        return None;
    }
    let set = gameset::get(&g.id);
    let (steam_opts, loaded, saving) = {
        let i = sh.inner.lock().unwrap();
        (i.steam.clone(), i.loaded, i.saving)
    };
    let label = |ui: &mut Ui, t: &str| ui.text_s(s().mr(0.4).w(7.5), t.to_string(), T::new(Ty::Small).w(600));
    let mut launch = vec![label(ui, "Launch options")];
    if steam.is_some() {
        // Always a button, so the row keeps its height while Steam's file is read.
        let b = match (&steam_opts, loaded) {
            (Some(v), _) => Some(B::new(short(v), acts.tag(Act::Local(STEAM_OPTS))).disabled(saving).hint("Edit")),
            (None, false) => Some(B::new("Reading Steam", acts.tag(Act::None)).disabled(true)),
            // No Steam to edit: say so, no button that can't be pressed.
            (None, true) => None,
        };
        match b {
            Some(b) => launch.push(kit::button(ui, b.ghost())),
            None => launch.push(ui.text("Steam isn't set up on this device.", T::new(Ty::Small).c(tok::MUTED))),
        }
        if saving {
            launch.push(ui.text("Restarting Steam", T::new(Ty::Meta).c(tok::MUTED)));
        }
    } else {
        launch.push(kit::button(ui, B::new(format!("Arguments: {}", short(&set.args)), acts.tag(Act::Local(ARGS))).ghost().hint("Edit")));
        launch.push(kit::button(ui, B::new(format!("Environment: {}", short(&set.env)), acts.tag(Act::Local(ENV))).ghost().hint("Edit")));
    }
    let side = |lc: bool, v: &Option<String>| match v {
        Some(a) => action_label(Some(a)).to_string(),
        None => format!("{} (Controls)", action_label(Some(&global(lc)))),
    };
    let controls = vec![
        label(ui, "Controls"),
        kit::button(ui, B::new(format!("LC: {}", side(true, &set.lc)), acts.tag(Act::Local(LC))).ghost().hint("Change")),
        kit::button(ui, B::new(format!("RC: {}", side(false, &set.rc)), acts.tag(Act::Local(RC))).ghost().hint("Change")),
    ];
    let row = |ui: &mut Ui, kids: Vec<El>| ui.bx(s().row().wrap().items_center().gap(0.5)).kids(kids);
    let a = row(ui, launch);
    let b = row(ui, controls);
    Some(ui.bx(s().col().gap(0.5).max_w(75.0)).group(Group::new("game-settings", [0.0; 4])).child(a).child(b))
}

/// The keyboard answers for this page's questions (from the page's tick).
pub fn answers(sh: &Arc<Shared>, st: &mut State, fx: &mut Fx, g: &Game) {
    let now = rare_render::clock::secs();
    if let Some(Some(v)) = st.take_answer(ASK_ARGS) {
        gameset::update(&g.id, |s| s.args = v.trim().to_string());
        st.toast("Saved. Used the next time the game starts.", Tone::Info, now);
    }
    if let Some(Some(v)) = st.take_answer(ASK_ENV) {
        match gameset::parse_env(&v) {
            Ok(_) => {
                gameset::update(&g.id, |s| s.env = v.trim().to_string());
                st.toast("Saved. Used the next time the game starts.", Tone::Info, now);
            }
            Err(bad) => st.toast(format!("{bad} is not NAME=value. Nothing was saved."), Tone::Error, now),
        }
    }
    if let Some(Some(v)) = st.take_answer(ASK_STEAM) {
        let Some(appid) = steam_appid(g) else { return };
        if st.is_running(&g.id) || !st.running.is_empty() {
            st.toast("Close the running game first.", Tone::Error, now);
            return;
        }
        let v = v.trim().to_string();
        sh.set(|i| i.saving = true);
        let sh = sh.clone();
        fx.spawn(move |b| {
            let r = b.steam_set_launch_options(appid, &v);
            let fresh = b.steam_launch_options(appid);
            Box::new(move |st: &mut State, now| {
                sh.set(|i| {
                    i.saving = false;
                    if fresh.is_some() {
                        i.steam = fresh;
                    }
                });
                match r {
                    Ok(()) => st.toast("Launch options saved in Steam", Tone::Info, now),
                    Err(e) => {
                        log::warn!("launch options: {e}");
                        st.toast(e, Tone::Error, now)
                    }
                }
            })
        });
    }
}

/// A press on one of the rows, or a choice in the LC/RC sheet.
pub fn act(sh: &Shared, st: &mut State, fx: &mut Fx, g: &Game, code: u32) {
    match code {
        STEAM_OPTS => {
            let cur = sh.inner.lock().unwrap().steam.clone().unwrap_or_default();
            st.ask_text(ASK_STEAM, "Steam launch options", cur, Some("DXVK_HUD=fps %command% -dx11"));
        }
        ARGS => st.ask_text(ASK_ARGS, "Arguments for this game", gameset::get(&g.id).args, Some("-windowed -skipintro")),
        ENV => st.ask_text(ASK_ENV, "Environment for this game", gameset::get(&g.id).env, Some("DXVK_HUD=fps PROTON_USE_WINED3D=1")),
        LC | RC => {
            let lc = code == LC;
            let set = gameset::get(&g.id);
            let cur = if lc { set.lc } else { set.rc };
            let base = if lc { LC_PICK } else { RC_PICK };
            let mut items = vec![OptionItem { label: format!("Same as Controls ({})", action_label(Some(&global(lc)))), on: Some(cur.is_none()), act: Act::Local(base) }];
            for (i, (k, l)) in ACTIONS.iter().enumerate() {
                items.push(OptionItem { label: l.to_string(), on: Some(cur.as_deref() == Some(*k)), act: Act::Local(base + 1 + i as u32) });
            }
            fx.fb(Feedback::Open);
            st.options = Some(OptionsSpec { title: format!("{} in {}", if lc { "LC" } else { "RC" }, g.title), sub: Some("Used while this game runs".into()), items });
        }
        c if (LC_PICK..LC_PICK + 1 + ACTIONS.len() as u32).contains(&c) || (RC_PICK..RC_PICK + 1 + ACTIONS.len() as u32).contains(&c) => {
            let lc = c < RC_PICK;
            let i = (c - if lc { LC_PICK } else { RC_PICK }) as usize;
            let v = if i == 0 { None } else { Some(ACTIONS[i - 1].0.to_string()) };
            gameset::update(&g.id, |s| if lc { s.lc = v } else { s.rc = v });
        }
        _ => {}
    }
}
