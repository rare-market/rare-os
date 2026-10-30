//! Parts of a library game's page (games/GamePanels.tsx): ES-DE's systems for the emulator, a web
//! game's controls with "Open in <browser>", a Steam game's Proton picker, and the thin bar while
//! a Steam game starts.

use crate::act::Act;
use crate::data::model::{fmt_ago, Emulation, Game, ProtonStatus};
use crate::kit::{self, s, tok, Btn as B, Ty, Ui, T};
use crate::page::{Acts, Fx};
use crate::state::{Start, State, Tone};
use rare_render::focus::{Group};
use rare_render::view::El;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// What the page's workers fill in; `ver` moves whenever it changes so the page rebuilds.
#[derive(Default)]
pub struct Shared {
    inner: Mutex<Inner>,
    ver: AtomicU64,
}

#[derive(Default)]
struct Inner {
    proton: Option<ProtonStatus>,
    browser: Option<String>,
    /// The Proton action in flight ("set:...", "install:ge", "rm:...").
    busy: Option<String>,
}

/// The newest `proton-install` event (phase, done, total); None once it finished or failed.
static PROGRESS: Mutex<Option<(String, u64, u64)>> = Mutex::new(None);
static PROG_VER: AtomicU64 = AtomicU64::new(0);

/// Event::ProtonInstall, on the UI thread (bridge.ts onProtonInstall).
pub fn on_progress(p: &rare_core::events::ProtonProgress) {
    *PROGRESS.lock().unwrap() = if p.phase == "done" || p.phase == "failed" { None } else { Some((p.phase.clone(), p.done, p.total)) };
    PROG_VER.fetch_add(1, Ordering::Relaxed);
}

impl Shared {
    pub fn ver(&self) -> u64 {
        self.ver.load(Ordering::Relaxed) + PROG_VER.load(Ordering::Relaxed)
    }
    fn set(&self, f: impl FnOnce(&mut Inner)) {
        f(&mut self.inner.lock().unwrap());
        self.ver.fetch_add(1, Ordering::Relaxed);
    }
    fn busy(&self) -> Option<String> {
        self.inner.lock().unwrap().busy.clone()
    }
}

const KIND_LABEL: [(&str, &str); 2] = [("ge", "GE-Proton"), ("cachyos", "Proton-CachyOS")];

fn kind_label(k: &str) -> &str {
    KIND_LABEL.iter().find(|(a, _)| *a == k).map(|(_, b)| *b).unwrap_or(k)
}

fn phase_text(phase: &str) -> &'static str {
    match phase {
        "lookup" => "Looking up the release",
        "download" => "Downloading",
        "verify" => "Checking the checksum",
        "unpack" => "Unpacking",
        _ => "",
    }
}

/// One choice in the Proton row.
struct Entry {
    label: String,
    /// The tool name to store; None goes back to Valve's stable build.
    name: Option<String>,
    on: bool,
}

fn entries(st: &ProtonStatus) -> Vec<Entry> {
    let current = st.game.clone().or_else(|| st.global.clone());
    let mut v = Vec::new();
    if let Some(t) = st.tools.iter().find(|t| t.kind == "valve") {
        v.push(Entry { label: t.label.clone(), name: None, on: current.as_deref() == Some(t.name.as_str()) && st.game.is_none() });
    }
    for t in st.tools.iter().filter(|t| t.kind == "experimental" && !t.stale) {
        v.push(Entry { label: "Experimental".into(), name: Some(t.name.clone()), on: st.game.as_deref() == Some(t.name.as_str()) });
    }
    for t in st.tools.iter().filter(|t| matches!(t.kind.as_str(), "ge" | "cachyos" | "other") && !t.stale) {
        v.push(Entry { label: t.label.clone(), name: Some(t.name.clone()), on: st.game.as_deref() == Some(t.name.as_str()) });
    }
    v
}

fn stale(st: &ProtonStatus) -> Vec<(String, String)> {
    st.tools.iter().filter(|t| t.stale).map(|t| (t.name.clone(), t.label.clone())).collect()
}

const ACT_BROWSER: u32 = 1;
const ACT_SET: u32 = 100;
const ACT_INSTALL: u32 = 200;
const ACT_REMOVE: u32 = 300;

fn proton_appid(g: &Game) -> Option<u64> {
    (g.source == "steam" && !g.foreign).then(|| g.id.strip_prefix("steam:")?.parse().ok()).flatten()
}

/// Whose Proton the row sets: Steam's per-game tool, or the one Rare hands umu-run for an Epic or
/// GOG game (rare-library umu). A GOG Linux build (start.sh) has none.
#[derive(Clone)]
enum Target {
    Steam(u64),
    Umu(String),
}

fn target(g: &Game) -> Option<Target> {
    if let Some(a) = proton_appid(g) {
        return Some(Target::Steam(a));
    }
    let umu = g.installed && (g.id.starts_with("epic:") || g.id.starts_with("gog:")) && super::stores::managed(g);
    let linux = g.install_dir.as_deref().is_some_and(|d| std::path::Path::new(d).join("start.sh").is_file());
    (umu && !linux).then(|| Target::Umu(g.id.clone()))
}

fn status_of(b: &dyn crate::data::backend::Backend, t: &Target) -> Option<ProtonStatus> {
    match t {
        Target::Steam(a) => b.proton_status(*a),
        Target::Umu(id) => b.umu_proton_status(id),
    }
}

fn set_tool(b: &dyn crate::data::backend::Backend, t: &Target, name: Option<&str>) -> Result<(), String> {
    match t {
        Target::Steam(a) => b.proton_set_game(*a, name),
        Target::Umu(id) => b.umu_proton_set(id, name),
    }
}

/// First look at a page: asks the backend for what its panels show.
pub fn load(sh: &Arc<Shared>, fx: &mut Fx, g: &Game) {
    let sh = sh.clone();
    let tgt = target(g);
    let web = g.source == "web";
    if tgt.is_none() && !web {
        return;
    }
    fx.spawn(move |b| {
        let proton = tgt.as_ref().and_then(|t| status_of(b, t));
        let browser = if web { b.browser_name() } else { None };
        Box::new(move |_, _| sh.set(|i| {
            i.proton = proton;
            i.browser = browser;
        }))
    });
}

/// A press on one of the panel buttons (`Act::Local` codes above).
pub fn act(sh: &Arc<Shared>, st: &mut State, fx: &mut Fx, g: &Game, code: u32) {
    if code == ACT_BROWSER {
        let url = g.launch.as_ref().and_then(|l| l.url.clone()).unwrap_or_default();
        let id = g.id.clone();
        fx.spawn(move |b| {
            let r = b.webgame_open_browser(&id, &url);
            Box::new(move |st: &mut State, now| match r {
                Ok(name) => st.toast(format!("Opening in {name}"), Tone::Info, now),
                Err(e) => {
                    log::warn!("browser: {e}");
                    st.toast("Couldn't open the browser.", Tone::Error, now)
                }
            })
        });
        return;
    }
    let Some(tgt) = target(g) else { return };
    let Some(status) = sh.inner.lock().unwrap().proton.clone() else { return };
    let now = rare_render::clock::secs();
    if sh.busy().is_some() {
        return st.toast("Wait for the Proton change to finish.", Tone::Info, now);
    }
    if st.is_running(&g.id) {
        return st.toast("Close the game first, then change its Proton.", Tone::Info, now);
    }
    let ents = entries(&status);
    let (key, done, job): (String, String, Box<dyn FnOnce(&dyn crate::data::backend::Backend) -> Result<(), String> + Send>) = match code {
        c if (ACT_SET..ACT_INSTALL).contains(&c) => {
            let Some(e) = ents.get((c - ACT_SET) as usize) else { return };
            if e.on {
                return st.toast(format!("{} is already this game's Proton", e.label), Tone::Info, now);
            }
            let name = e.name.clone();
            let t = tgt.clone();
            (format!("set:{:?}", name), format!("Proton for this game: {}", e.label), Box::new(move |b| set_tool(b, &t, name.as_deref())))
        }
        c if (ACT_INSTALL..ACT_REMOVE).contains(&c) => {
            let Some(kind) = status.installable.get((c - ACT_INSTALL) as usize).cloned() else { return };
            let t = tgt.clone();
            (format!("install:{kind}"), format!("{} installed and selected", kind_label(&kind)), Box::new(move |b| {
                let name = b.proton_install(&kind)?;
                set_tool(b, &t, Some(&name))
            }))
        }
        c => {
            let Some((name, label)) = stale(&status).get((c - ACT_REMOVE) as usize).cloned() else { return };
            (format!("rm:{name}"), format!("{label} removed"), Box::new(move |b| b.proton_remove(&name)))
        }
    };
    *PROGRESS.lock().unwrap() = None;
    sh.set(|i| i.busy = Some(key));
    let sh = sh.clone();
    fx.spawn(move |b| {
        let r = job(b);
        let fresh = status_of(b, &tgt);
        Box::new(move |st: &mut State, now| {
            sh.set(|i| {
                i.busy = None;
                if fresh.is_some() {
                    i.proton = fresh;
                }
            });
            match r {
                Ok(()) => st.toast(done, Tone::Info, now),
                Err(e) => {
                    log::warn!("proton: {e}");
                    st.toast("Couldn't finish that.", Tone::Error, now)
                }
            }
        })
    });
}

/// ProtonPanel: Valve's stable build by default, others on request.
pub fn proton(ui: &mut Ui, sh: &Shared, acts: &mut Acts, g: &Game) -> Option<El> {
    let steam = matches!(target(g)?, Target::Steam(_));
    let (status, busy) = {
        let i = sh.inner.lock().unwrap();
        (i.proton.clone()?, i.busy.clone())
    };
    if status.tools.is_empty() {
        return None;
    }
    let off = busy.is_some();
    let mut kids = vec![ui.text_s(s().mr(0.4), "Proton", T::new(Ty::Small).w(600))];
    for (i, e) in entries(&status).iter().enumerate() {
        let b = B::new(e.label.clone(), acts.tag(Act::Local(ACT_SET + i as u32))).disabled(off);
        kids.push(kit::button(ui, if e.on { b.primary() } else { b.ghost() }));
    }
    for (i, k) in status.installable.iter().enumerate() {
        kids.push(kit::button(ui, B::new(format!("Install {}", kind_label(k)), acts.tag(Act::Local(ACT_INSTALL + i as u32))).ghost().disabled(off)));
    }
    for (i, (_, label)) in stale(&status).into_iter().enumerate() {
        kids.push(kit::button(ui, B::new(format!("Remove old {label}"), acts.tag(Act::Local(ACT_REMOVE + i as u32))).ghost().disabled(off)));
    }
    if off {
        let text = match PROGRESS.lock().unwrap().as_ref() {
            Some((phase, done, total)) => {
                let pct = if phase == "download" && *total > 0 { format!(" {}%", (*done as f64 / *total as f64 * 100.0).round()) } else { String::new() };
                format!("{}{pct}", phase_text(phase))
            }
            None if steam => "Restarting Steam".into(),
            None => "Saving".into(),
        };
        kids.push(ui.text(text, T::new(Ty::Meta).c(tok::MUTED)));
    }
    Some(ui.bx(s().row().wrap().items_center().gap(0.5).max_w(75.0)).group(Group::new("proton", [0.0; 4])).kids(kids))
}

/// "12 min" or "1.5 h" (GamePanels.tsx fmtHours).
fn fmt_hours(secs: u64) -> String {
    if secs < 3600 {
        format!("{} min", ((secs as f64 / 60.0).round() as u64).max(1))
    } else {
        format!("{} h", (secs as f64 / 360.0).round() / 10.0)
    }
}

/// EmulationPanel: ES-DE's systems with game counts, and the ROMs played last.
pub fn emulation(ui: &mut Ui, acts: &mut Acts, e: &Emulation) -> El {
    let total: u32 = e.systems.iter().map(|x| x.count).sum();
    let sys_name = |id: &str| e.systems.iter().find(|x| x.id == id).map(|x| x.name.clone()).unwrap_or_else(|| id.to_uppercase());
    let head = ui.bx(s().row().items_end().gap(0.6).mb(0.6)).child(ui.text("Systems", T::new(Ty::Small).w(600))).child(ui.text(format!("{total} games"), T::new(Ty::Meta).c(tok::MUTED).tab()));
    let mut rows = Vec::new();
    for chunk in e.systems.iter().take(12).collect::<Vec<_>>().chunks(3) {
        let mut cells: Vec<El> = chunk
            .iter()
            .map(|x| {
                ui.bx(s().row().items_end().justify_between().gap(0.5).flex(1.0).min0())
                    .child(ui.text_s(s().min0(), x.name.clone(), T::new(Ty::Small).trunc()))
                    .child(ui.text_s(s().shrink(), x.count.to_string(), T::new(Ty::Small).c(tok::MUTED).tab()))
            })
            .collect();
        // A short last row keeps the columns of the rows above (grid-cols-3).
        while cells.len() < 3 {
            cells.push(ui.bx(s().flex(1.0).min0()));
        }
        rows.push(ui.bx(s().row().gap(1.2)).kids(cells));
    }
    let left = ui.bx(s().col().flex(3.0).min0().px(1.25).py(1.0).gap(0.35)).bg(tok::L1).radius(ui.r(1.0)).child(head).kids(rows);
    let mut row = ui.bx(s().row().gap(1.0).max_w(75.0)).child(left);
    if !e.recent.is_empty() {
        let mut col = vec![ui.text_s(s().mb(0.25), "Played last", T::new(Ty::Small).w(600))];
        for g in e.recent.iter().take(4) {
            let name = g.name.split(['[', '(']).next().unwrap_or(&g.name).trim_end();
            let name = if name.is_empty() { g.name.as_str() } else { name };
            let when = if g.playtime > 0 { fmt_hours(g.playtime) } else { fmt_ago(g.last_played) };
            let mut line = ui
                .bx(s().row().items_center().gap(0.6))
                .child(ui.text_s(s().flex(1.0).min0(), name.to_string(), T::new(Ty::Small).trunc()))
                .child(ui.text_s(s().shrink(), format!("{} \u{b7} {when}", sys_name(&g.system)), T::new(Ty::Meta).c(tok::MUTED).trunc()));
            // Delete: the ROM file leaves the system's folder (features/uninstall.rs).
            if let Some(id) = g.uninstall_id() {
                line = line.child(kit::button(ui, B::new("Delete", acts.tag(Act::Uninstall(id))).sm().ghost().hint("Delete")));
            }
            col.push(line);
        }
        row = row.child(ui.bx(s().col().flex(2.0).min0().px(1.25).py(1.0).gap(0.35)).bg(tok::L1).radius(ui.r(1.0)).group(Group::new("roms", [0.0; 4])).kids(col));
    }
    row
}

/// WebGameInfo: how the game takes the controller, and Open in <browser> for pages WebKit can't run.
pub fn web_info(ui: &mut Ui, sh: &Shared, acts: &mut Acts, g: &Game) -> El {
    let url = g.launch.as_ref().and_then(|l| l.url.as_deref());
    let (label, text) = crate::screens::sui::catalog::web_controls(&g.id, url);
    let col = ui.bx(s().col().flex(1.0).min0().gap(0.25)).child(ui.text(label, T::new(Ty::Label).w(500).c(tok::fg(0.6)))).child(ui.text(text, T::new(Ty::Small).c(tok::fg(0.8)).wrap()));
    let mut row = ui.bx(s().row().items_end().gap(1.5).max_w(60.0)).child(col);
    let browser = sh.inner.lock().unwrap().browser.clone();
    if let (Some(_), Some(name)) = (url, browser) {
        row = row.child(kit::button(ui, B::new(format!("Open in {name}"), acts.tag(Act::Local(ACT_BROWSER))).sm().ghost().hint(format!("Open in {name}"))));
    }
    row
}

/// StartProgress: a thin bar while a Steam game starts, Steam first and then the game; what Steam
/// asks the player when it waits for an answer.
pub fn start_progress(ui: &mut Ui, start: &Start, now: f64) -> Option<El> {
    let (steam, asked) = match start.phase {
        crate::state::Phase::Steam => (true, None),
        crate::state::Phase::Game => (false, None),
        crate::state::Phase::Answer(q) => (false, Some(q)),
        _ => return None,
    };
    let secs = now - start.since;
    let mut text = match asked {
        Some(q) => q.to_string(),
        None if steam => "Starting Steam".to_string(),
        None => "Starting the game".to_string(),
    };
    if steam && secs >= 15.0 {
        text.push_str(", it is updating");
    }
    let name = ui.loop_name(&rare_render::anim::BREATHE, 1.3);
    let seg = ui.bx(s().wp(0.35).h_px(3.0)).bg(tok::fg(0.8)).radius(ui.r(0.15)).name(name);
    let track = ui.bx(s().wp(1.0).h_px(3.0)).bg(tok::fg(0.15)).radius(ui.r(0.15)).clip().child(seg);
    Some(ui.bx(s().col().w(26.0)).child(track).child(ui.text_s(s().mt(0.35), text, T::new(Ty::Small).c(tok::MUTED))))
}
