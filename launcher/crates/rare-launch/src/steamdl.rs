//! Steam installs and the download queue, for the launcher's Downloads view and the Install
//! button. Steam does the work; this reads its state and presses its buttons:
//!
//! - Reading: Steam's own download list through its client API (`steamcef`) when the port is up
//!   (queue order, pause state, network speed, time left), merged with the appmanifests every
//!   library writes while it downloads (`rare_library::steam::downloads`), which work without it.
//!   Without Steam's numbers the speed comes from the manifests' byte counts between reads, else
//!   from the network interface while one download runs.
//! - Installing: Steam's install wizard opened, given the chosen library folder and continued in
//!   one go, so its dialog never takes the screen. When Steam's API can't be reached (or the
//!   wizard needs an answer only the player can give, a licence) Steam's own dialog is shown
//!   with the usual hand-off (`apps::steam`), and Home brings Rare back.
//! - Pause, resume, move to the top, cancel: the same calls Steam's Downloads page makes.
//!
//! Steam's API needs the remote debugging flag at Steam's start: `ensure_api` sets it and, when
//! nothing is running, restarts Steam quietly once so the port opens.

use crate::launch::{self, Runner};
use crate::steamcef;
use rare_core::{Ctx, Error};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// One download or update in Steam's queue.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dl {
    pub appid: u32,
    /// From the appmanifest; empty when only Steam's list has it (the caller names it).
    pub title: String,
    pub done: u64,
    pub total: u64,
    pub paused: bool,
    /// Downloading now (not waiting in the queue).
    pub active: bool,
    /// Place in Steam's queue (0 is the running one), -1 when not queued (paused or unscheduled).
    pub queue: i32,
    /// An update of an installed game.
    pub update: bool,
    /// Bytes per second, 0 when unknown.
    pub rate: u64,
    /// Seconds left, 0 when unknown.
    pub eta: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub items: Vec<Dl>,
    /// Steam's own numbers (its API answered).
    pub from_steam: bool,
    /// Steam's downloads are paused as a whole.
    pub all_paused: bool,
    /// App ids Steam reports as finished since the last read (for the notice).
    pub finished: Vec<u32>,
    /// Updates Steam has not scheduled yet (its "unscheduled" list): shown as Waiting, with Update now.
    pub waiting: Vec<Dl>,
}

/// A library folder to install into.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub index: u32,
    pub path: String,
    /// "This device", "SD card", or Steam's label for it.
    pub name: String,
    pub free: Option<u64>,
    pub total: Option<u64>,
    pub sd: bool,
}

/// How an install went.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Started {
    /// Steam took it; it shows in the queue.
    Queued,
    /// Steam's own install dialog has the screen.
    SteamShown,
    /// The game has licence agreements to accept first; Steam's install waits for
    /// `Control::AcceptEula` (or `Control::CancelInstall`).
    NeedsEula(Vec<rare_library::appinfo::Eula>),
}

// ---- reading -------------------------------------------------------------------------------

/// Steam's list and overview, normalised. The typings (`downloaded_bytes`, `total_bytes`,
/// `update_seconds_remaining`) and the client on the SuiPlay (2026-10: items grouped per client in
/// `item_data`, bytes per content type and phase in `update_type_info[].progress[]`,
/// `overall_estimated_time_remaining_sec`) differ, so both are read.
const READ_JS: &str = r#"(async () => {
  const D = SteamClient.Downloads;
  const r = await new Promise((res) => {
    let items = null, ov = null, h1 = null, h2 = null, fin = false;
    const end = () => { if (fin) return; fin = true; try { h1 && h1.unregister(); } catch (e) {} try { h2 && h2.unregister(); } catch (e) {} res({ items: items || [], ov }); };
    h1 = D.RegisterForDownloadItems((on, list) => { items = list || []; if (ov) end(); });
    h2 = D.RegisterForDownloadOverview((o) => { ov = o; if (items) end(); });
    setTimeout(end, 1200);
  });
  const flat = [];
  for (const g of r.items) {
    if (g && Array.isArray(g.item_data)) { if (String(g.remote_client_id || '0') === '0') flat.push(...g.item_data); }
    else if (g) flat.push(g);
  }
  const bytes = (i) => {
    if (typeof i.total_bytes === 'number') return [i.downloaded_bytes || 0, i.total_bytes || 0];
    let best = [0, 0];
    for (const u of (i.update_type_info || [])) {
      if (!u.has_update) continue;
      for (const p of (u.progress || [])) if ((p.bytes_total || 0) > best[1]) best = [p.bytes_in_progress || 0, p.bytes_total || 0];
    }
    return best;
  };
  const o = r.ov;
  const eta = o ? (o.overall_estimated_time_remaining_sec > 0 ? o.overall_estimated_time_remaining_sec : (o.update_seconds_remaining || 0)) : 0;
  return {
    items: flat.map((i) => { const b = bytes(i); return { appid: i.appid, active: !!i.active, paused: !!i.paused, queue: (typeof i.queue_index === 'number' ? i.queue_index : -1), done: b[0], total: b[1], completed: !!i.completed, buildid: i.buildid || 0 }; }),
    ov: o ? { appid: o.update_appid || 0, rate: o.update_network_bytes_per_second || 0, eta: eta > 0 ? eta : 0, paused: !!o.paused } : null,
  };
})()"#;

#[derive(serde::Deserialize, Default)]
struct JsItem {
    appid: u32,
    active: bool,
    paused: bool,
    queue: i32,
    done: f64,
    total: f64,
    completed: bool,
    buildid: u64,
}

#[derive(serde::Deserialize, Default)]
struct JsOv {
    appid: u32,
    rate: f64,
    eta: f64,
    paused: bool,
}

#[derive(serde::Deserialize, Default)]
struct JsRead {
    #[serde(default)]
    items: Vec<JsItem>,
    #[serde(default)]
    ov: Option<JsOv>,
}

/// Byte counts seen per app (for a speed without Steam's numbers), the network counter, and the
/// completed apps already reported.
#[derive(Default)]
struct Memory {
    seen: HashMap<u32, VecDeque<(Instant, u64)>>,
    net: Option<(Instant, u64)>,
    net_rate: u64,
    reported: Vec<u32>,
    primed: bool,
    /// Names from the app info for apps without a manifest, read once each.
    names: HashMap<u32, String>,
}

static MEM: Mutex<Option<Memory>> = Mutex::new(None);

/// Received bytes on every interface but loopback (/proc/net/dev).
fn rx_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/net/dev").ok()?;
    Some(
        text.lines()
            .skip(2)
            .filter_map(|l| l.split_once(':'))
            .filter(|(name, _)| name.trim() != "lo")
            .filter_map(|(_, rest)| rest.split_whitespace().next()?.parse::<u64>().ok())
            .sum(),
    )
}

/// Bytes per second over the last 20 s of readings of `done` (0 until there are two).
fn rate_of(q: &VecDeque<(Instant, u64)>) -> u64 {
    let (Some(a), Some(b)) = (q.front(), q.back()) else { return 0 };
    let dt = b.0.duration_since(a.0).as_secs_f64();
    if dt < 1.0 || b.1 <= a.1 {
        return 0;
    }
    ((b.1 - a.1) as f64 / dt) as u64
}

/// Merges Steam's list (when it answered) with the manifests and fills in speed and time left.
fn merge(steam: Option<JsRead>, manifests: Vec<rare_library::steam::Download>, pending: Vec<rare_library::steam::Download>, mem: &mut Memory, now: Instant) -> Snapshot {
    let appid = |id: &str| id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).unwrap_or(0);
    let mut out = Snapshot::default();
    let mut ov_rate = (0u32, 0u64, 0u64);
    // Every app Steam's list has, kept or not: its manifest doesn't add it back.
    let mut listed: Vec<u32> = Vec::new();
    if let Some(r) = steam {
        listed = r.items.iter().map(|i| i.appid).collect();
        out.from_steam = true;
        if let Some(ov) = &r.ov {
            out.all_paused = ov.paused;
            ov_rate = (ov.appid, ov.rate.max(0.0) as u64, ov.eta.max(0.0) as u64);
        }
        // What Steam had already finished before Rare's first read is not news.
        if !mem.primed {
            mem.primed = true;
            mem.reported = r.items.iter().filter(|i| i.completed).map(|i| i.appid).collect();
        }
        for i in r.items {
            let m = manifests.iter().find(|d| appid(&d.id) == i.appid).or_else(|| pending.iter().find(|d| appid(&d.id) == i.appid));
            // Updates Steam holds back for later (unscheduled): Waiting, not in the queue.
            if !i.completed && !i.active && !i.paused && i.queue < 0 {
                let title = m.map(|d| d.title.clone()).unwrap_or_default();
                if !title.is_empty() && !rare_library::steam::is_tool(i.appid, &title) {
                    let (done, total) = match m.filter(|d| d.total > 0) {
                        Some(d) => (d.done, d.total),
                        None => (i.done.max(0.0) as u64, i.total.max(0.0) as u64),
                    };
                    out.waiting.push(Dl { appid: i.appid, title, done, total, paused: false, active: false, queue: -1, update: true, rate: 0, eta: 0 });
                }
                continue;
            }
            if i.completed {
                if !mem.reported.contains(&i.appid) {
                    mem.reported.push(i.appid);
                    out.finished.push(i.appid);
                }
                continue;
            }
            mem.reported.retain(|a| *a != i.appid);
            // The manifest's byte counts are the download's own; Steam's list is the fallback.
            let (done, total) = match m.filter(|d| d.total > 0) {
                Some(d) => (d.done, d.total),
                None => (i.done.max(0.0) as u64, i.total.max(0.0) as u64),
            };
            out.items.push(Dl {
                appid: i.appid,
                title: m.map(|d| d.title.clone()).unwrap_or_default(),
                done,
                total,
                paused: i.paused,
                active: i.active,
                queue: i.queue,
                update: i.buildid != 0 || m.is_some_and(|d| d.update),
                rate: 0,
                eta: 0,
            });
        }
    }
    for d in &manifests {
        let a = appid(&d.id);
        if a == 0 || listed.contains(&a) || out.items.iter().any(|x| x.appid == a) {
            continue;
        }
        out.items.push(Dl { appid: a, title: d.title.clone(), done: d.done, total: d.total, paused: d.paused, active: !d.paused, queue: -1, update: d.update, rate: 0, eta: 0 });
    }
    // Without Steam's list, the manifests' pending updates are the Waiting list.
    if !out.from_steam {
        for d in &pending {
            let a = appid(&d.id);
            if a != 0 && !out.items.iter().any(|x| x.appid == a) {
                out.waiting.push(Dl { appid: a, title: d.title.clone(), done: d.done, total: d.total, paused: false, active: false, queue: -1, update: true, rate: 0, eta: 0 });
            }
        }
    }
    out.waiting.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    // Speed: Steam's for the running one, else the byte counts between reads, else the network.
    if let Some(rx) = rx_bytes() {
        if let Some((t, b)) = mem.net {
            let dt = now.duration_since(t).as_secs_f64();
            if dt >= 1.0 {
                mem.net_rate = (rx.saturating_sub(b) as f64 / dt) as u64;
                mem.net = Some((now, rx));
            }
        } else {
            mem.net = Some((now, rx));
        }
    }
    let ids: Vec<u32> = out.items.iter().map(|d| d.appid).collect();
    mem.seen.retain(|a, _| ids.contains(a));
    let running = out.items.iter().filter(|d| d.active && !d.paused).count();
    for d in out.items.iter_mut() {
        let q = mem.seen.entry(d.appid).or_default();
        q.push_back((now, d.done));
        while q.front().is_some_and(|f| now.duration_since(f.0) > Duration::from_secs(20)) && q.len() > 2 {
            q.pop_front();
        }
        if d.paused || !d.active {
            continue;
        }
        let (rate, eta) = if ov_rate.0 == d.appid && ov_rate.1 > 0 {
            (ov_rate.1, ov_rate.2)
        } else {
            let r = rate_of(q);
            let r = if r == 0 && running == 1 { mem.net_rate } else { r };
            (r, 0)
        };
        d.rate = rate;
        d.eta = if eta > 0 { eta } else if rate > 0 { d.total.saturating_sub(d.done) / rate } else { 0 };
    }
    // The running one first, then Steam's queue order, then the rest.
    out.items.sort_by_key(|d| (!(d.active && !d.paused), if d.queue < 0 { i32::MAX } else { d.queue }, d.paused));
    out
}

/// Steam's downloads now. Never restarts Steam; reads its API only when the port is already up.
pub fn snapshot() -> Snapshot {
    if let Some(root) = crate::steamcfg::primary_root() {
        // Rare's next Steam start opens the port.
        steamcef::enable(&root);
    }
    let steam = if steamcef::up() { steamcef::eval(READ_JS, Duration::from_secs(4)).ok().and_then(|v| serde_json::from_value::<JsRead>(v).ok()) } else { None };
    let manifests = rare_library::steam::downloads();
    let pending = rare_library::steam::pending_updates();
    let mut g = MEM.lock().unwrap();
    let mem = g.get_or_insert_with(Memory::default);
    let mut snap = merge(steam, manifests, pending, mem, Instant::now());
    // A dependency Steam fetches first (a runtime) has no manifest yet: its name from the app info.
    let missing: std::collections::HashSet<u32> = snap.items.iter().chain(snap.waiting.iter()).filter(|d| d.title.is_empty() && !mem.names.contains_key(&d.appid)).map(|d| d.appid).collect();
    if !missing.is_empty() {
        if let Some(root) = crate::steamcfg::primary_root() {
            let found = rare_library::appinfo::read(&root, &missing);
            for a in missing {
                mem.names.insert(a, found.get(&a).map(|m| m.name.clone()).unwrap_or_default());
            }
        }
    }
    for d in snap.items.iter_mut().chain(snap.waiting.iter_mut()) {
        if d.title.is_empty() {
            d.title = mem.names.get(&d.appid).cloned().unwrap_or_default();
        }
    }
    snap
}

// ---- Steam's API, made ready -----------------------------------------------------------------

/// Makes Steam's API reachable: sets the flag and, when Steam runs without the port and nothing
/// is running, restarts it quietly (as the Proton picker does). Blocks up to two minutes.
pub fn ensure_api(runner: &Runner) -> Result<(), Error> {
    if steamcef::up() {
        return Ok(());
    }
    let root = crate::steamcfg::primary_root().ok_or("Steam is not installed.")?;
    steamcef::enable(&root);
    if launch::steam_running() {
        if !runner.running().is_empty() {
            return Err(Error::new("busy"));
        }
        let _ = launch::host_command("steam", &["-shutdown".to_string()], "steam").status();
        for _ in 0..60 {
            if !launch::steam_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if launch::steam_running() {
            return Err(Error::new("Steam did not close."));
        }
    }
    crate::steamcfg::prepare_start();
    let mut child = launch::host_command("steam", &["-silent".to_string()], "steam").spawn().map_err(|e| Error::new(format!("Could not start Steam: {e}")))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(120) {
        std::thread::sleep(Duration::from_secs(1));
        if steamcef::up() && steamcef::eval("typeof SteamClient === 'object' && !!SteamClient.Downloads", Duration::from_secs(3)).ok().and_then(|v| v.as_bool()) == Some(true) {
            return Ok(());
        }
    }
    Err(Error::new("Steam is taking long to start."))
}

/// A JS number literal list of paths, for matching Steam's folder list.
fn js_str(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// Opens Steam's wizard for `appid`, picks the folder, continues. "ok", or what stopped it.
fn wizard_js(appid: u32, paths: &[String], fallback_index: u32) -> String {
    let list = paths.iter().map(|p| js_str(p.trim_end_matches('/'))).collect::<Vec<_>>().join(",");
    format!(
        r#"(async () => {{
  const want = [{list}];
  let idx = {fallback_index};
  try {{
    const fs = await SteamClient.InstallFolder.GetInstallFolders();
    const f = (fs || []).find((x) => want.includes(String(x.strFolderPath || '').replace(/\/+$/, '')));
    if (f) idx = f.nFolderIndex;
  }} catch (e) {{}}
  const I = SteamClient.Installs;
  // The last argument of Steam's calls is the client ("0": this device); older clients ignore it.
  I.OpenInstallWizard([{appid}], '0');
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  // A player's answer (licence, key, password, disc): Steam's own dialog takes it from here.
  const ask = (s) => s === 3 || s === 4 || s === 6 || s === 8 || s === 11 || s === 13;
  // The wizard's last run lingers (Canceled, Complete) until this one has started.
  let fresh = false;
  for (let i = 0; i < 300; i++) {{
    const info = await I.GetInstallManagerInfo();
    const ours = info && ((info.rgApps || info.rgAppIDs || []).some((a) => (a && a.nAppID !== undefined ? a.nAppID : a) === {appid}) || info.currentAppID === {appid});
    const s = ours ? info.eInstallState : -1;
    if (s >= 1 && s <= 13) fresh = true;
    if (s === 7) {{
      await I.SetInstallFolder(idx);
      await sleep(150);
      await I.ContinueInstall();
      for (let j = 0; j < 100; j++) {{
        await sleep(100);
        const n = await I.GetInstallManagerInfo();
        const t = n ? n.eInstallState : 0;
        if (t === 14 || t === 0) return 'ok';
        if (t === 15) return 'failed:' + (n.eAppError || 0);
        if (t === 16) return 'canceled';
        if (t === 8) return 'eula';
        if (ask(t)) {{ try {{ I.CancelInstall(); }} catch (e) {{}} return 'ask:' + t; }}
      }}
      return 'ok';
    }}
    if (fresh && s === 14) return 'ok';
    if (fresh && s === 15) return 'failed:' + (info.eAppError || 0);
    if (fresh && s === 16) return 'canceled';
    if (s === 8) return 'eula';
    if (ask(s)) {{ try {{ I.CancelInstall(); }} catch (e) {{}} return 'ask:' + s; }}
    await sleep(100);
  }}
  try {{ I.CancelInstall(); }} catch (e) {{}}
  return 'timeout';
}})()"#
    )
}

/// Installs a Steam game into the library folder at `folder` (a path from `folders`).
pub fn install(ctx: &Ctx, runner: &Runner, appid: u32, folder: &str) -> Result<Started, Error> {
    let shown = |ctx: &Ctx| crate::apps::steam(ctx, runner, &format!("steam://install/{appid}")).map(|_| Started::SteamShown);
    if let Err(e) = ensure_api(runner) {
        eprintln!("[steamdl] install {appid}: Steam's API not ready ({e}); showing Steam's dialog");
        return shown(ctx);
    }
    let folders = rare_library::steam::install_folders();
    let f = folders.iter().find(|f| same_path(&f.path, folder));
    let mut paths = vec![folder.to_string()];
    if let Some(f) = f {
        paths.push(f.path.clone());
    }
    if let Ok(c) = std::fs::canonicalize(folder) {
        paths.push(c.to_string_lossy().into_owned());
    }
    let js = wizard_js(appid, &paths, f.map(|f| f.index).unwrap_or(0));
    match steamcef::eval(&js, Duration::from_secs(45)) {
        Ok(v) if v.as_str() == Some("ok") => Ok(Started::Queued),
        // Licence agreements: the player accepts them in Rare; the wizard waits meanwhile.
        Ok(v) if v.as_str() == Some("eula") => {
            let eulas = eulas_of(appid);
            if eulas.is_empty() {
                let _ = steamcef::eval("SteamClient.Installs.CancelInstall(); true", Duration::from_secs(4));
                return shown(ctx);
            }
            Ok(Started::NeedsEula(eulas))
        }
        // A licence or key only the player can answer: Steam's own install dialog, on screen.
        Ok(v) if v.as_str().is_some_and(|s| s.starts_with("ask:")) => shown(ctx),
        Ok(v) if v.as_str() == Some("canceled") => Err(Error::new("The install was cancelled.")),
        Ok(v) if v.as_str().is_some_and(|s| s.starts_with("failed:")) => Err(Error::new(install_error(v.as_str().unwrap_or_default()))),
        Ok(v) => {
            eprintln!("[steamdl] install {appid}: wizard said {v}; showing Steam's dialog");
            shown(ctx)
        }
        Err(e) => {
            eprintln!("[steamdl] install {appid}: {e}; showing Steam's dialog");
            shown(ctx)
        }
    }
}

/// The licence agreements in Steam's app info for `appid`.
fn eulas_of(appid: u32) -> Vec<rare_library::appinfo::Eula> {
    let Some(root) = crate::steamcfg::primary_root() else { return Vec::new() };
    rare_library::appinfo::read(&root, &std::collections::HashSet::from([appid])).remove(&appid).map(|m| m.eulas).unwrap_or_default()
}

/// Accepts the game's licence agreements in the open install wizard and continues it.
fn accept_eulas_js(appid: u32, eulas: &[rare_library::appinfo::Eula]) -> String {
    let marks = eulas.iter().map(|e| format!("SteamClient.Apps.MarkEulaAccepted({appid}, {}, {});", js_str(&e.id), e.version)).collect::<String>();
    format!(
        r#"(async () => {{
  const I = SteamClient.Installs;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  {marks}
  await sleep(200);
  await I.ContinueInstall();
  for (let j = 0; j < 100; j++) {{
    await sleep(100);
    const n = await I.GetInstallManagerInfo();
    const t = n ? n.eInstallState : 0;
    if (t === 14 || t === 0) return 'ok';
    if (t === 15) return 'failed:' + (n.eAppError || 0);
    if (t === 16) return 'canceled';
    if (t === 8 && j > 20) return 'eula';
  }}
  return 'ok';
}})()"#
    )
}

/// Steam's EAppUpdateError as a sentence (the common ones).
fn install_error(s: &str) -> String {
    // EAppUpdateError.
    match s.trim_start_matches("failed:").parse::<i32>().unwrap_or(0) {
        12 => "Steam says there isn't enough space on that drive.".into(),
        11 | 15 => "Steam couldn't write to that drive.".into(),
        5 => "Steam has no licence for this game on this account.".into(),
        16 => "Steam won't install while a game is running. Close it and try again.".into(),
        _ => "Steam couldn't start the install. Try again.".into(),
    }
}

fn same_path(a: &str, b: &str) -> bool {
    let n = |p: &str| std::fs::canonicalize(p).map(|c| c.to_string_lossy().into_owned()).unwrap_or_else(|_| p.trim_end_matches('/').to_string());
    a.trim_end_matches('/') == b.trim_end_matches('/') || n(a) == n(b)
}

/// What a row in the Downloads view asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Pause,
    Resume,
    Top,
    Cancel,
    /// A waiting (unscheduled) update: into the queue, first.
    UpdateNow,
    /// The install waiting on its licence agreements: accepted, it goes on.
    AcceptEula,
    /// That install, cancelled.
    CancelInstall,
}

/// Presses one of Steam's Downloads buttons. When Steam's API can't be reached, Steam's own
/// Downloads page is shown instead (`Ok(false)`).
pub fn control(ctx: &Ctx, runner: &Runner, appid: u32, c: Control) -> Result<bool, Error> {
    match c {
        Control::AcceptEula => {
            let v = steamcef::eval(&accept_eulas_js(appid, &eulas_of(appid)), Duration::from_secs(20)).map_err(|e| Error::new(format!("Steam didn't take that: {e}")))?;
            return match v.as_str() {
                Some("ok") => Ok(true),
                Some(s) if s.starts_with("failed:") => Err(Error::new(install_error(s))),
                Some("canceled") => Err(Error::new("The install was cancelled.")),
                _ => Err(Error::new("Steam still waits for the licence agreement. Try again.")),
            };
        }
        Control::CancelInstall => {
            let _ = steamcef::eval("SteamClient.Installs.CancelInstall(); true", Duration::from_secs(4));
            return Ok(true);
        }
        _ => {}
    }
    if ensure_api(runner).is_err() {
        crate::apps::steam(ctx, runner, "steam://open/downloads")?;
        return Ok(false);
    }
    let update = rare_library::steam::downloads().iter().any(|d| d.id == format!("steam:{appid}") && d.update);
    let js = match c {
        Control::Pause => format!("SteamClient.Downloads.PauseAppUpdate({appid}, '0'); true"),
        Control::Resume => format!(
            "(async () => {{ const D = SteamClient.Downloads; const ov = await new Promise((res) => {{ let h = null; h = D.RegisterForDownloadOverview((o) => {{ try {{ h && h.unregister(); }} catch (e) {{}} res(o); }}); setTimeout(() => res(null), 800); }}); if (ov && ov.paused) D.EnableAllDownloads(true, '0'); D.ResumeAppUpdate({appid}, '0'); return true; }})()"
        ),
        Control::Top => format!("SteamClient.Downloads.SetQueueIndex({appid}, 0, '0'); SteamClient.Downloads.ResumeAppUpdate({appid}, '0'); true"),
        // An update leaves the queue (it waits for the next start); a first install is removed.
        Control::Cancel if update => format!("SteamClient.Downloads.RemoveFromDownloadList({appid}, '0'); true"),
        Control::Cancel => format!("SteamClient.Installs.OpenUninstallWizard([{appid}], '0', true); true"),
        Control::AcceptEula | Control::CancelInstall => unreachable!(),
        Control::UpdateNow => format!("SteamClient.Downloads.QueueAppUpdate({appid}, '0'); SteamClient.Downloads.SetQueueIndex({appid}, 0, '0'); SteamClient.Downloads.ResumeAppUpdate({appid}, '0'); true"),
    };
    steamcef::eval(&js, Duration::from_secs(6)).map_err(|e| Error::new(format!("Steam didn't take that: {e}")))?;
    Ok(true)
}

/// Uninstalls through Steam's own uninstall with its prompt off (the caller has asked the player
/// already). None when Steam's API can't be reached; Some(Err) with Steam's reason when it refuses
/// (it reports a refusal through RegisterForShowFailedUninstall within a moment).
pub fn uninstall_quiet(appid: u32) -> Option<Result<(), String>> {
    if !steamcef::up() {
        return None;
    }
    let js = format!(
        "(async () => {{ const I = SteamClient.Installs; let fail = null, h = null; try {{ h = I.RegisterForShowFailedUninstall((a, r) => {{ if (a === {appid}) fail = r; }}); }} catch (e) {{}} I.OpenUninstallWizard([{appid}], '0', true); await new Promise((r) => setTimeout(r, 2500)); try {{ h && h.unregister(); }} catch (e) {{}} return fail === null ? 'ok' : 'failed:' + fail; }})()"
    );
    match steamcef::eval(&js, Duration::from_secs(10)) {
        Ok(v) if v.as_str() == Some("ok") => Some(Ok(())),
        Ok(v) => Some(Err(uninstall_error(v.as_str().unwrap_or_default()))),
        Err(e) => {
            eprintln!("[steamdl] uninstall {appid}: {e}");
            None
        }
    }
}

/// Steam's reason for refusing an uninstall, as a sentence.
fn uninstall_error(s: &str) -> String {
    match s.trim_start_matches("failed:").parse::<i32>().unwrap_or(-1) {
        16 => "Steam won't uninstall while a game is running. Close it and try again.".into(),
        20 => "Steam is busy with this game. Try again in a moment.".into(),
        18 => "Steam says it isn't installed.".into(),
        r => format!("Steam couldn't uninstall it (reason {r}). Try again."),
    }
}

// ---- where to install ------------------------------------------------------------------------

#[cfg(unix)]
fn space(path: &str) -> (Option<u64>, Option<u64>) {
    let Ok(c) = std::ffi::CString::new(path) else { return (None, None) };
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return (None, None);
    }
    let unit = s.f_frsize as u64;
    (Some(s.f_bavail as u64 * unit), Some(s.f_blocks as u64 * unit))
}

#[cfg(not(unix))]
fn space(_path: &str) -> (Option<u64>, Option<u64>) {
    (None, None)
}

fn is_removable(path: &str) -> bool {
    ["/run/media/", "/media/", "/mnt/", "/var/mnt/"].iter().any(|p| path.starts_with(p))
}

/// The library folders Steam installs into, with free space (missing drives left out).
pub fn folders() -> Vec<Folder> {
    let mut out: Vec<Folder> = Vec::new();
    for f in rare_library::steam::install_folders() {
        if !std::path::Path::new(&f.path).is_dir() {
            continue;
        }
        let (free, total) = space(&f.path);
        let sd = is_removable(&f.path);
        let name = if !f.label.is_empty() {
            f.label.clone()
        } else if sd {
            "SD card".into()
        } else {
            "This device".into()
        };
        out.push(Folder { index: f.index, path: f.path, name, free, total, sd });
    }
    // Two folders with the same name get their folder's name after it.
    let names: Vec<String> = out.iter().map(|f| f.name.clone()).collect();
    for f in out.iter_mut() {
        if names.iter().filter(|n| **n == f.name).count() > 1 {
            let last = std::path::Path::new(&f.path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            f.name = format!("{} ({last})", f.name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn man(id: u32, done: u64, total: u64, paused: bool, update: bool) -> rare_library::steam::Download {
        rare_library::steam::Download { id: format!("steam:{id}"), title: format!("Game {id}"), done, total, paused, update }
    }

    #[test]
    fn steam_list_leads_and_manifests_fill_in() {
        let mut mem = Memory::default();
        let steam = JsRead {
            items: vec![
                JsItem { appid: 2, active: false, paused: false, queue: 1, done: 0.0, total: 500.0, ..Default::default() },
                JsItem { appid: 1, active: true, paused: false, queue: 0, done: 100.0, total: 1000.0, buildid: 7, ..Default::default() },
                JsItem { appid: 9, completed: true, ..Default::default() },
            ],
            ov: Some(JsOv { appid: 1, rate: 50.0, eta: 18.0, paused: false }),
        };
        mem.primed = true;
        let s = merge(Some(steam), vec![man(1, 90, 1000, false, true), man(3, 5, 10, true, false)], vec![], &mut mem, Instant::now());
        assert_eq!(s.items.iter().map(|d| d.appid).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!((s.items[0].title.as_str(), s.items[0].done, s.items[0].rate, s.items[0].eta, s.items[0].update), ("Game 1", 90, 50, 18, true));
        assert!(s.items[2].paused);
        assert_eq!(s.finished, [9]);
        // Reported once.
        let again = JsRead { items: vec![JsItem { appid: 9, completed: true, ..Default::default() }], ov: None };
        assert!(merge(Some(again), vec![], vec![], &mut mem, Instant::now()).finished.is_empty());
    }

    #[test]
    fn speed_from_byte_counts_without_steam() {
        let mut mem = Memory::default();
        let t0 = Instant::now();
        merge(None, vec![man(5, 0, 10_000, false, false)], vec![], &mut mem, t0);
        let s = merge(None, vec![man(5, 4_000, 10_000, false, false)], vec![], &mut mem, t0 + Duration::from_secs(4));
        assert_eq!((s.items[0].rate, s.items[0].eta), (1000, 6));
    }

    #[test]
    fn wizard_script_names_the_app_and_folder() {
        let js = wizard_js(620, &["/run/media/sd/SteamLibrary/".into()], 1);
        assert!(js.contains("OpenInstallWizard([620], '0')") && js.contains("\"/run/media/sd/SteamLibrary\"") && js.contains("let idx = 1;"));
    }

    #[test]
    fn first_read_primes_completed_and_skips_unscheduled() {
        let mut mem = Memory::default();
        let r = JsRead { items: vec![JsItem { appid: 4, completed: true, ..Default::default() }, JsItem { appid: 5, queue: -1, total: 30.0, ..Default::default() }], ov: None };
        let s = merge(Some(r), vec![], vec![man(5, 0, 30, false, true)], &mut mem, Instant::now());
        assert!(s.finished.is_empty() && s.items.is_empty());
        assert_eq!((s.waiting[0].appid, s.waiting[0].title.as_str(), s.waiting[0].total), (5, "Game 5", 30));
    }
}
