//! PC game streaming: games on the owner's gaming PC (Sunshine or GeForce Experience) played on
//! the SuiPlay through Moonlight-Qt. Moonlight is a separate program (the Flathub app, GPL-3.0)
//! run through its command line, never linked, so Rare keeps its own licence. docs/STREAMING.md
//! has the owner's steps.
//!
//! - cli:      Moonlight's command lines and the SuiPlay presets (pure, tested)
//! - discover: PCs on the network (mDNS `_nvstream._tcp`) and their serverinfo
//! - conf:     what Moonlight knows (Moonlight.conf): paired PCs, MAC addresses
//! - store:    Rare's own file (settings, PCs, app lists with cached box art) and the host list
//! - pair:     a pairing in progress, PIN shown by Rare
//! - wol:      Wake-on-LAN, the packets Moonlight sends
//! - host:     streaming this device: Sunshine as the host, PIN pairing (off by default)
//! - nearby:   other Rare devices on the network (mDNS `_rare._tcp`)
//!
//! Streams run in the launcher's runner as "stream:<host uuid>:<app id>", so Resume, Quit, the
//! running dot and Guide work as they do for games. The pad goes to Moonlight (it reads the
//! controller itself): Rare's pointer mode is never on for a stream.

pub mod cli;
pub mod conf;
pub mod discover;
pub mod host;
pub mod nearby;
pub mod pair;
pub mod store;
pub mod wol;

/// What streaming needs from the program around it (the launcher's runner, the Apps installer,
/// the host environment). The Tauri shell implements it over its modules.
pub trait StreamHost {
    /// A Flatpak app is installed.
    fn flatpak_installed(&self, id: &str) -> bool;
    /// A program is on PATH.
    fn which(&self, program: &str) -> bool;
    /// Sets up the environment a host program runs with.
    fn host_env(&self, cmd: &mut Command);
    /// A command to run `program` as run `run_id`, in its own unit when the session uses them.
    fn host_command_unit(&self, program: &str, args: &[String], run_id: &str) -> (Command, Option<String>);
    /// Flathub is already a remote of the user installation.
    fn user_flathub(&self) -> bool;
    /// The one-line install command for a Flathub app.
    fn install_command(&self, flatpak: &str, user_flathub: bool) -> String;
    /// Run ids the runner has going now.
    fn running(&self) -> Vec<String>;
    fn quit_run(&self, id: &str);
    fn track_child(&self, run_id: &str, child: std::process::Child, unit: Option<String>);
    /// Brings a run to the front, pad to the program (never pointer mode).
    fn to_front(&self, run_id: &str);
    /// The launcher steps aside so the stream shows.
    fn step_aside(&self);
}

use cli::{Decode, Moonlight, Settings};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use store::{HostView, Online, StreamApp};

const PROBE: Duration = Duration::from_millis(1500);

/// The PCs that answered last (by uuid), with the address that answered.
fn online() -> std::sync::MutexGuard<'static, Online> {
    static O: OnceLock<Mutex<Online>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(Online::new())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Rare's stream.json, behind one lock so reads and writes don't race.
fn file() -> std::sync::MutexGuard<'static, store::File> {
    static F: OnceLock<Mutex<store::File>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(store::load())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Moonlight on this device: the Flathub app first, then a program on PATH.
pub fn moonlight(h: &dyn StreamHost) -> Option<Moonlight> {
    if h.flatpak_installed(cli::FLATPAK) {
        return Some(Moonlight::Flatpak);
    }
    ["moonlight", "moonlight-qt"].iter().find(|p| h.which(p)).map(|p| Moonlight::Native(p.to_string()))
}

/// Hardware decode on this device, from `vainfo` (once; None when vainfo isn't there).
pub fn decode(h: &dyn StreamHost) -> Option<Decode> {
    static D: OnceLock<Option<Decode>> = OnceLock::new();
    *D.get_or_init(|| {
        if !h.which("vainfo") {
            return None;
        }
        let mut c = Command::new("vainfo");
        h.host_env(&mut c);
        let out = run_for(c, Duration::from_secs(8)).ok()?;
        cli::parse_vainfo(&format!("{}\n{}", out.1, out.2))
    })
}

/// Runs a command to the end (killed after `limit`): (success, stdout, stderr).
fn run_for(mut c: Command, limit: Duration) -> Result<(bool, String, String), String> {
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let mut o = child.stdout.take();
    let mut e = child.stderr.take();
    let to = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = o.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let te = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = e.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let t = Instant::now();
    let ok = loop {
        if let Ok(Some(st)) = child.try_wait() {
            break st.success();
        }
        if t.elapsed() > limit {
            pair::stop_child(child);
            return Err("Moonlight didn't answer in time".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    Ok((ok, to.join().unwrap_or_default(), te.join().unwrap_or_default()))
}

/// Runs a Moonlight action with no window.
fn headless(h: &dyn StreamHost, m: &Moonlight, action: Vec<String>, limit: Duration) -> Result<(bool, String, String), String> {
    let (prog, args, env) = m.command(&action, true);
    let mut c = Command::new(prog);
    c.args(args);
    h.host_env(&mut c);
    for (k, v) in env {
        c.env(k, v);
    }
    run_for(c, limit)
}

fn last_line(s: &str) -> Option<String> {
    s.lines().map(str::trim).rfind(|l| !l.is_empty()).map(String::from)
}

/// Something to install first: a Flatpak and the one-line command that installs it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub flatpak: String,
    pub name: String,
    pub command: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// "flatpak" or "native"; None when Moonlight isn't installed.
    pub moonlight: Option<String>,
    /// Moonlight from Flathub, when it isn't installed.
    pub offer: Option<Offer>,
    pub decode: Option<Decode>,
    pub settings: Settings,
    pub hosts: Vec<HostView>,
    pub apps: BTreeMap<String, Vec<StreamApp>>,
}

fn hosts_now() -> Vec<HostView> {
    let known = conf::load();
    let f = file();
    store::merge(&known, &f.hosts, &online())
}

fn apps_now(hosts: &[HostView]) -> BTreeMap<String, Vec<StreamApp>> {
    let known = conf::load();
    let f = file();
    hosts.iter().filter(|h| h.paired).map(|h| (h.uuid.clone(), store::apps_of(&h.uuid, &f, &known))).collect()
}

pub fn status(h: &dyn StreamHost) -> Status {
    let m = moonlight(h);
    let user_flathub = h.user_flathub();
    let hosts = hosts_now();
    Status {
        moonlight: m.as_ref().map(|m| if *m == Moonlight::Flatpak { "flatpak" } else { "native" }.to_string()),
        offer: m.is_none().then(|| Offer { flatpak: cli::FLATPAK.into(), name: "Moonlight".into(), command: h.install_command(cli::FLATPAK, user_flathub) }),
        decode: decode(h),
        settings: file().settings.clone().sane(),
        apps: apps_now(&hosts),
        hosts,
    }
}

/// Asks every known PC for its serverinfo (in parallel) and keeps who answered.
pub fn refresh() -> Vec<HostView> {
    let known = conf::load();
    let (saved, prev) = (file().hosts.clone(), online().clone());
    let ids: Vec<String> = store::merge(&known, &saved, &prev).into_iter().map(|h| h.uuid).collect();
    let jobs: Vec<_> = ids
        .into_iter()
        .map(|id| {
            let addrs = store::addresses(&id, &known, &saved, &prev);
            std::thread::spawn(move || addrs.into_iter().find_map(|a| discover::probe(&a, PROBE).ok().filter(|i| i.uuid == id).map(|i| (id.clone(), (a, i)))))
        })
        .collect();
    let now: Online = jobs.into_iter().filter_map(|j| j.join().ok().flatten()).collect();
    remember_macs(&now);
    *online() = now;
    hosts_now()
}

/// Keeps MAC addresses of saved PCs current, for Wake-on-LAN while they sleep.
fn remember_macs(now: &Online) {
    let mut f = file();
    let mut changed = false;
    for (id, (_, i)) in now {
        if let Some(h) = f.hosts.iter_mut().find(|h| &h.uuid == id) {
            if i.mac.is_some() && h.mac != i.mac {
                h.mac = i.mac.clone();
                changed = true;
            }
        }
    }
    if changed {
        let _ = store::save(&f);
    }
}

/// Looks for PCs on the network for `ms`, then asks each for its serverinfo.
pub fn scan(ms: u64) -> Vec<HostView> {
    let found = discover::browse(Duration::from_millis(ms.clamp(500, 8000)));
    let jobs: Vec<_> = found.into_iter().map(|f| std::thread::spawn(move || discover::probe(&f.address, PROBE).ok().map(|i| (i.uuid.clone(), (f.address, i))))).collect();
    let answers: Vec<(String, (String, discover::ServerInfo))> = jobs.into_iter().filter_map(|j| j.join().ok().flatten()).collect();
    {
        let mut o = online();
        for (id, v) in answers {
            o.insert(id, v);
        }
    }
    hosts_now()
}

/// A PC typed in by address.
pub fn add(address: &str) -> Result<HostView, String> {
    let address = address.trim();
    if !discover::valid_address(address) {
        return Err("That doesn't look like an address".into());
    }
    let info = discover::probe(address, Duration::from_secs(4)).map_err(|_| format!("No PC answered at {address}. Check Sunshine is running on it."))?;
    online().insert(info.uuid.clone(), (address.to_string(), info.clone()));
    {
        let mut f = file();
        store::remember(&mut f, store::Saved { uuid: info.uuid.clone(), name: info.name.clone(), address: address.to_string(), mac: info.mac.clone() });
        store::save(&f)?;
    }
    hosts_now().into_iter().find(|h| h.uuid == info.uuid).ok_or_else(|| "PC not found".into())
}

pub fn pair_start(h: &dyn StreamHost, address: &str) -> Result<pair::PairView, String> {
    let m = moonlight(h).ok_or("Install Moonlight first")?;
    let info = discover::probe(address, Duration::from_secs(4)).map_err(|_| format!("No PC answered at {address}"))?;
    {
        let mut f = file();
        store::remember(&mut f, store::Saved { uuid: info.uuid.clone(), name: info.name.clone(), address: address.to_string(), mac: info.mac.clone() });
        let _ = store::save(&f);
    }
    online().insert(info.uuid.clone(), (address.to_string(), info.clone()));
    pair::start(h, &m, address, &info)
}

/// Copies Moonlight's box art for an app into Rare's cache; returns Rare's copy.
fn cache_art(uuid: &str, l: &cli::Listed) -> Option<String> {
    let safe: String = uuid.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let dst = dirs::cache_dir()?.join("rare/stream").join(&safe).join(format!("{}.png", l.id));
    let src = l
        .boxart
        .as_ref()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| conf::boxart_dir(uuid).into_iter().map(|d| d.join(format!("{}.png", l.id))).find(|p| p.is_file()));
    if let Some(src) = src {
        let fresh = std::fs::metadata(&src).ok().map(|m| m.len()) != std::fs::metadata(&dst).ok().map(|m| m.len());
        if fresh {
            let _ = std::fs::create_dir_all(dst.parent()?);
            let _ = std::fs::copy(&src, &dst);
        }
    }
    dst.is_file().then(|| dst.to_string_lossy().into_owned())
}

/// The PC's games and apps (`moonlight list --csv`), box art cached, saved for the Library.
pub fn apps(h: &dyn StreamHost, uuid: &str) -> Result<Vec<StreamApp>, String> {
    let m = moonlight(h).ok_or("Install Moonlight first")?;
    let host = hosts_now().into_iter().find(|h| h.uuid == uuid).ok_or("PC not found")?;
    if !host.paired {
        return Err(format!("Pair {} first", host.name));
    }
    let address = host.address.clone().ok_or("No address for this PC")?;
    // Moonlight downloads the box art it lacks before it exits.
    let (ok, out, err) = headless(h, &m, cli::list_args(&address), Duration::from_secs(90))?;
    let listed = cli::parse_list_csv(&out);
    if !ok && listed.is_empty() {
        return Err(last_line(&err).unwrap_or_else(|| format!("Couldn't list the games on {}", host.name)));
    }
    let list: Vec<StreamApp> = listed.iter().filter(|l| !l.hidden).map(|l| StreamApp { id: l.id, name: l.name.clone(), art: cache_art(uuid, l), hdr: l.hdr }).collect();
    let mut f = file();
    f.apps.insert(uuid.to_string(), list.clone());
    store::save(&f)?;
    Ok(list)
}

fn running_streams(h: &dyn StreamHost) -> Vec<String> {
    h.running().into_iter().filter(|r| r.starts_with("stream:")).collect()
}

/// Stops Moonlight for these runs and waits for them to go (up to 6 s).
fn stop_runs(h: &dyn StreamHost, ids: &[String]) {
    for id in ids {
        h.quit_run(id);
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(6) && h.running().iter().any(|r| ids.contains(r)) {
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Starts streaming `app_id` from the PC. `replace`: quit what the PC is running first.
pub fn launch(h: &dyn StreamHost, uuid: &str, app_id: u32, replace: bool) -> Result<(), String> {
    let m = moonlight(h).ok_or("Install Moonlight first")?;
    let run_id = store::run_id(uuid, app_id);
    if running_streams(h).contains(&run_id) {
        return resume(h, &run_id);
    }
    let host = hosts_now().into_iter().find(|h| h.uuid == uuid).ok_or("PC not found")?;
    if !host.paired {
        return Err(format!("Pair {} first", host.name));
    }
    let address = host.address.clone().filter(|_| host.online).ok_or_else(|| format!("{} isn't answering. Wake it, or check it's on.", host.name))?;
    let name = {
        let known = conf::load();
        store::apps_of(uuid, &file(), &known).into_iter().find(|a| a.id == app_id).map(|a| a.name).ok_or("That game isn't on this PC any more")?
    };
    // One stream at a time.
    let others = running_streams(h);
    if !others.is_empty() {
        stop_runs(h, &others);
    }
    if replace {
        let _ = headless(h, &m, cli::quit_args(&address), Duration::from_secs(30));
    }
    let settings = file().settings.clone().sane();
    let codec = cli::pick_codec(settings.codec, decode(h), host.codecs);
    let (prog, args, env) = m.command(&cli::stream_args(&address, &name, &settings, codec), false);
    let (mut cmd, unit) = h.host_command_unit(&prog, &args, &run_id);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let child = cmd.spawn().map_err(|e| format!("Could not start Moonlight: {e}"))?;
    h.track_child(&run_id, child, unit);
    h.to_front(&run_id);
    h.step_aside();
    Ok(())
}

/// Back to a running stream.
pub fn resume(h: &dyn StreamHost, run_id: &str) -> Result<(), String> {
    if !running_streams(h).iter().any(|r| r == run_id) {
        return Err("That stream isn't running".into());
    }
    h.to_front(run_id);
    h.step_aside();
    Ok(())
}

/// Ends the stream (if one runs) and quits the game on the PC.
pub fn quit(h: &dyn StreamHost, uuid: &str) -> Result<(), String> {
    let prefix = format!("stream:{uuid}:");
    let ids: Vec<String> = running_streams(h).into_iter().filter(|r| r.starts_with(&prefix)).collect();
    stop_runs(h, &ids);
    let m = moonlight(h).ok_or("Install Moonlight first")?;
    let host = hosts_now().into_iter().find(|h| h.uuid == uuid).ok_or("PC not found")?;
    let Some(address) = host.address.clone() else { return Ok(()) };
    let (ok, _, err) = headless(h, &m, cli::quit_args(&address), Duration::from_secs(30))?;
    if !ok {
        return Err(last_line(&err).unwrap_or_else(|| "The PC didn't quit the game".into()));
    }
    if let Some((_, i)) = online().get_mut(uuid) {
        i.current_game = None;
    }
    Ok(())
}

pub fn wake(uuid: &str) -> Result<(), String> {
    let known = conf::load();
    let saved = file().hosts.clone();
    let o = online().clone();
    let mac = store::mac_of(uuid, &known, &saved, &o).ok_or("Rare doesn't know this PC's network card yet. Wake it by hand once.")?;
    wol::wake(mac, &store::addresses(uuid, &known, &saved, &o))
}

pub fn set_settings(s: Settings) -> Result<(), String> {
    let mut f = file();
    f.settings = s.sane();
    store::save(&f)
}

