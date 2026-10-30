//! Device, power and performance controls. Everything goes through the base OS's own
//! services (PowerStation over D-Bus or Handheld Daemon's API for TDP and CPU boost, Rare OS's
//! rare-fand or Handheld Daemon for the fan, gamescope's root-window atoms for FPS, logind for
//! brightness, WirePlumber for volume).

use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;

pub fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = crate::host::env(Command::new(cmd).args(args)).output().map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `run` that gives up after `limit` (the process is killed and the error says so). For tools that
/// can wait forever on a service that is not there: `bluetoothctl` without bluetoothd, `nmcli`
/// without NetworkManager's bus name.
pub fn run_for(cmd: &str, args: &[&str], limit: std::time::Duration) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::Instant;
    let mut child = crate::host::env(Command::new(cmd).args(args)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{cmd}: {e}"))?;
    // Read both pipes on threads so a full pipe never stalls the child.
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(e) => return Err(format!("{cmd}: {e}")),
        }
    };
    let (stdout, stderr) = (t_out.join().unwrap_or_default(), t_err.join().unwrap_or_default());
    match status {
        Some(st) if st.success() => Ok(stdout.trim().to_string()),
        Some(_) => Err(stderr.trim().to_string()),
        None => Err(format!("{cmd} did not answer")),
    }
}

fn read(p: &str) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    /// Full-charge capacity against design (energy_full / energy_full_design), percent.
    pub health: Option<u8>,
    /// Minutes of battery left while discharging, or until full while charging; None when unknown.
    pub minutes: Option<u32>,
}

/// Minutes left while discharging, or to full while charging.
/// `now` and `full` are energy (µWh) or charge (µAh); `rate` is power (µW) or current (µA).
pub fn battery_minutes(status: &str, now: f64, full: f64, rate: f64) -> Option<u32> {
    if rate <= 0.0 || now < 0.0 {
        return None;
    }
    let hours = match status {
        "Discharging" => now / rate,
        "Charging" => (full - now).max(0.0) / rate,
        _ => return None,
    };
    let m = (hours * 60.0).round();
    (m.is_finite() && m < 24.0 * 60.0).then_some(m as u32)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// DMI product name, e.g. "SuiPlay0X1".
    pub product: String,
    pub vendor: String,
    /// Friendly name shown in the UI.
    pub name: String,
    pub is_suiplay: bool,
    pub is_vm: bool,
    pub os_name: String,
    pub os_version: String,
    pub hostname: String,
    pub under_gamescope: bool,
    pub battery: Option<Battery>,
    pub wifi: Option<String>,
    pub platform: String,
}

fn os_release() -> (String, String) {
    let text = read("/etc/os-release").or_else(|| read("/usr/lib/os-release")).unwrap_or_default();
    let get = |k: &str| {
        text.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim_matches('"').to_string())
    };
    (get("NAME").unwrap_or_else(|| "Rare OS".into()), get("VERSION").or_else(|| get("VERSION_ID")).unwrap_or_default())
}

pub fn battery() -> Option<Battery> {
    let rd = fs::read_dir("/sys/class/power_supply").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        // A controller's battery has scope Device; the system battery has none.
        if read(&p.join("type").to_string_lossy()).as_deref() != Some("Battery") || read(&p.join("scope").to_string_lossy()).as_deref() == Some("Device") {
            continue;
        }
        let percent = read(&p.join("capacity").to_string_lossy())?.parse().ok()?;
        let status = read(&p.join("status").to_string_lossy()).unwrap_or_default();
        let num = |f: &str| read(&p.join(f).to_string_lossy()).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0);
        let full = num("energy_full").zip(num("energy_full_design")).or_else(|| num("charge_full").zip(num("charge_full_design")));
        let health = full.map(|(f, d)| (f * 100.0 / d).round().min(100.0) as u8);
        let minutes = num("energy_now")
            .zip(num("energy_full"))
            .zip(num("power_now"))
            .or_else(|| num("charge_now").zip(num("charge_full")).zip(num("current_now")))
            .and_then(|((n, f), r)| battery_minutes(&status, n, f, r));
        return Some(Battery { percent, charging: status == "Charging" || status == "Full", health, minutes });
    }
    None
}

fn wifi() -> Option<String> {
    let out = run("nmcli", &["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]).ok()?;
    out.lines().find_map(|l| l.strip_prefix("yes:").map(String::from)).filter(|s| !s.is_empty())
}

pub fn device_info() -> DeviceInfo {
    let product = read("/sys/class/dmi/id/product_name").unwrap_or_default();
    let vendor = read("/sys/class/dmi/id/sys_vendor").unwrap_or_default();
    let is_suiplay = product == "SuiPlay0X1";
    let is_vm = run("systemd-detect-virt", &["--vm"]).map(|s| s != "none").unwrap_or(false);
    let (os_name, os_version) = os_release();
    let name = if is_suiplay {
        "SuiPlay0X1".to_string()
    } else if product.is_empty() {
        if cfg!(target_os = "macos") { "Mac (dev)".into() } else { "PC".into() }
    } else {
        product.clone()
    };
    DeviceInfo {
        product,
        vendor,
        name,
        is_suiplay,
        is_vm,
        os_name,
        os_version,
        hostname: read("/etc/hostname").unwrap_or_else(|| run("hostname", &[]).unwrap_or_default()),
        under_gamescope: std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some(),
        battery: battery(),
        wifi: wifi(),
        platform: std::env::consts::OS.into(),
    }
}

// ---------- performance ----------
//
// Two backends, tried in this order:
// - PowerStation (org.shadowblip.PowerStation, system bus). Rare OS ships it. Its bus policy lets
//   any user send to it, so the launcher sets properties directly. TDP goes through RyzenAdj on
//   the 7840U (STAPM = TDP, slow PPT = TDP + Boost, fast PPT = 1.25 x slow); CPU boost writes
//   /sys/devices/system/cpu/cpufreq/boost. It has no fan control.
// - Handheld Daemon (hhd), for stock Bazzite: see system/hhd.rs. TDP, TDP boost, CPU boost, EPP
//   and the fan curve. When PowerStation is there too, hhd's TDP plugin is left alone so two
//   daemons never fight over the SMU; hhd's fan curve is used whenever hhd's TDP plugin runs.
// The fan: rare-fand (org.rare.Fan, Rare OS's own root service on ayaneo_ec devices) when it
// runs, otherwise hhd's fan curve. Both take the same Default/Cool/Quiet presets.

mod hhd;
pub mod power_watch;
pub mod wifi;

use serde_json::json;
use std::time::Duration;

/// Power profile (rare-core, shared with the emulator defaults).
pub use rare_core::Profile;

/// Fan presets, as edge-temperature curves (rare-fand has the same ones built in). Default hands
/// the fan back to the EC.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FanMode {
    Default,
    Cool,
    Quiet,
    /// A curve set elsewhere (hhd's own UI, or a custom curve given to rare-fand).
    Custom,
}

/// Edge temperature (C) -> fan %, at hhd's points (40, 45, 50, 55, 60, 65, 70, 80, 90).
/// hhd's own default edge curve is 30 30 40 45 55 60 70 85 100.
const FAN_COOL: [(u32, u32); 9] = [(40, 30), (45, 36), (50, 46), (55, 56), (60, 66), (65, 76), (70, 86), (80, 100), (90, 100)];
const FAN_QUIET: [(u32, u32); 9] = [(40, 20), (45, 20), (50, 20), (55, 22), (60, 26), (65, 30), (70, 50), (80, 80), (90, 100)];

pub fn fan_curve(m: FanMode) -> Option<&'static [(u32, u32)]> {
    match m {
        FanMode::Cool => Some(&FAN_COOL),
        FanMode::Quiet => Some(&FAN_QUIET),
        FanMode::Default | FanMode::Custom => None,
    }
}

fn fan_mode(p: &hhd::Power) -> Option<FanMode> {
    let mode = p.fan_mode.as_deref()?;
    if mode == "disabled" {
        return Some(FanMode::Default);
    }
    let same = |c: &[(u32, u32)]| mode == hhd::FAN_EDGE && c.iter().all(|(t, v)| p.fan_curve.get(t) == Some(v));
    Some(if same(&FAN_COOL) {
        FanMode::Cool
    } else if same(&FAN_QUIET) {
        FanMode::Quiet
    } else {
        FanMode::Custom
    })
}

/// What a profile sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    /// Sustained power limit, watts.
    pub tdp: f64,
    /// CPU boost clocks (up to 5.1 GHz on the 7840U). Off is the big heat win when a game is
    /// GPU-bound: the CPU stops eating the shared power budget.
    pub cpu_boost: bool,
    /// Short-term power above the sustained limit (fast/slow PPT).
    pub tdp_boost: bool,
    /// hhd EPP (cpu_pref).
    pub epp: &'static str,
    /// power-profiles-daemon / tuned-ppd profile (PowerStation backend).
    pub ppd: &'static str,
    /// PowerStation's RyzenAdj power profile.
    pub ryzenadj: &'static str,
}

pub fn preset(p: Profile) -> Option<Preset> {
    match p {
        // Wattages: below ~12 W fixed SoC power eats
        // the budget (the launcher alone averages 6.5 W), 15 to 17 W is the frames-per-watt peak,
        // and 25 to 30 W buys only ~4%. No profile has a slow-PPT gap (TDP boost).
        Profile::Quiet => Some(Preset { tdp: 10.0, cpu_boost: false, tdp_boost: false, epp: "power", ppd: "power-saver", ryzenadj: "power-saving" }),
        Profile::Balanced => Some(Preset { tdp: 15.0, cpu_boost: false, tdp_boost: false, epp: "balance_power", ppd: "balanced", ryzenadj: "power-saving" }),
        Profile::Performance => {
            Some(Preset { tdp: 25.0, cpu_boost: true, tdp_boost: false, epp: "balance_performance", ppd: "performance", ryzenadj: "max-performance" })
        }
        Profile::Custom => None,
    }
}

/// The profile's power limit, clamped to the device range.
pub fn profile_tdp(p: Profile, min: f64, max: f64) -> Option<f64> {
    preset(p).map(|s| s.tdp.clamp(min, max))
}

/// What the user asked for (from the launcher config).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wanted {
    pub profile: Profile,
    /// Slider watts, used with Custom.
    pub tdp: Option<f64>,
    /// CPU boost set by hand, over the profile's.
    pub cpu_boost: Option<bool>,
}

impl Default for Wanted {
    fn default() -> Self {
        Wanted { profile: Profile::Balanced, tdp: None, cpu_boost: None }
    }
}

/// The settings to apply for a wish within a TDP range: (watts, cpu boost, preset).
pub fn resolve(w: &Wanted, min: f64, max: f64) -> (Option<f64>, Option<bool>, Option<Preset>) {
    let p = preset(w.profile);
    let tdp = match w.profile {
        Profile::Custom => w.tdp.map(|t| t.clamp(min, max)),
        _ => profile_tdp(w.profile, min, max),
    };
    (tdp, w.cpu_boost.or(p.map(|p| p.cpu_boost)), p)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Perf {
    pub available: bool,
    pub tdp: Option<f64>,
    pub tdp_min: f64,
    pub tdp_max: f64,
    pub fps_limit: Option<u32>,
    pub profile: Profile,
    /// Why something is unavailable; empty when all is well.
    pub note: String,
    /// "powerstation" or "hhd".
    pub backend: Option<String>,
    /// None when CPU boost can't be changed here.
    pub cpu_boost: Option<bool>,
    /// None when the fan can't be controlled here.
    pub fan: Option<FanMode>,
    /// What each profile sets here, within the device's range (for the Quick settings labels).
    pub profile_watts: ProfileWatts,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct ProfileWatts {
    pub quiet: f64,
    pub balanced: f64,
    pub performance: f64,
}

pub fn profile_watts(min: f64, max: f64) -> ProfileWatts {
    let w = |p| profile_tdp(p, min, max).unwrap_or_default();
    ProfileWatts { quiet: w(Profile::Quiet), balanced: w(Profile::Balanced), performance: w(Profile::Performance) }
}

/// TDP range per device. SuiPlay0X1: 5-28 W (PowerStation's DMI override for the SuiPlay0X1).
fn tdp_range(product: &str) -> (f64, f64) {
    match product {
        "SuiPlay0X1" | "AYANEO 2S" => (5.0, 28.0),
        _ => (5.0, 30.0),
    }
}

/// The device range narrowed by what the backend reports.
fn clamp_range(dev: (f64, f64), backend: (Option<f64>, Option<f64>)) -> (f64, f64) {
    let min = backend.0.map_or(dev.0, |m| m.max(dev.0));
    let max = backend.1.map_or(dev.1, |m| m.min(dev.1));
    if min < max { (min, max) } else { dev }
}

fn device_range() -> (f64, f64) {
    tdp_range(&read("/sys/class/dmi/id/product_name").unwrap_or_default())
}

// ----- PowerStation -----

const PS: &str = "org.shadowblip.PowerStation";
const TDP_IFACE: &str = "org.shadowblip.GPU.Card.TDP";
const PS_CPU: &str = "/org/shadowblip/Performance/CPU";
const CPU_IFACE: &str = "org.shadowblip.CPU";
/// What PowerStation's BoostEnabled reads and writes.
const BOOST_PATH: &str = "/sys/devices/system/cpu/cpufreq/boost";

/// The value part of `busctl get-property` output: `d 15`, `b true`, `s "power-saving"`,
/// `as 2 "a" "b"` (the strings of an array, unquoted).
fn busctl_value(out: &str) -> Option<Vec<String>> {
    let (sig, rest) = out.trim().split_once(' ')?;
    let rest = rest.trim();
    Some(match sig {
        "s" => vec![rest.trim_matches('"').to_string()],
        "as" => rest.split_once(' ').map(|(_, r)| r).unwrap_or("").split('"').skip(1).step_by(2).map(String::from).collect(),
        _ => vec![rest.to_string()],
    })
}

fn ps_get(path: &str, iface: &str, prop: &str) -> Option<Vec<String>> {
    busctl_value(&run("busctl", &["--system", "get-property", PS, path, iface, prop]).ok()?)
}

fn ps_num(path: &str, iface: &str, prop: &str) -> Option<f64> {
    ps_get(path, iface, prop)?.first()?.parse().ok()
}

fn ps_set(path: &str, iface: &str, prop: &str, sig: &str, val: &str) -> Result<(), String> {
    run("busctl", &["--system", "set-property", PS, path, iface, prop, sig, val]).map(|_| ()).map_err(|e| format!("PowerStation {prop}: {e}"))
}

/// A PowerStation GPU card object path. PowerStation 0.8 names them after the DRM node
/// (`.../GPU/card1`, lowercase); older releases used `Card1`.
fn is_ps_card_path(p: &str) -> bool {
    let rest = p.strip_prefix("/org/shadowblip/Performance/GPU/").unwrap_or("");
    let num = rest.strip_prefix("card").or_else(|| rest.strip_prefix("Card")).unwrap_or("");
    !num.is_empty() && num.chars().all(|c| c.is_ascii_digit())
}

/// The PowerStation GPU object that exposes TDP (the integrated card).
fn ps_card() -> Option<String> {
    let tree = run("busctl", &["--system", "tree", "--list", PS]).ok()?;
    tree.lines()
        .map(str::trim)
        .filter(|l| is_ps_card_path(l))
        .find(|p| run("busctl", &["--system", "get-property", PS, p, TDP_IFACE, "TDP"]).is_ok())
        .map(String::from)
}

/// CPU boost through PowerStation: None when the kernel has no global boost switch.
fn ps_cpu_boost() -> Option<bool> {
    if !std::path::Path::new(BOOST_PATH).exists() {
        return None;
    }
    ps_get(PS_CPU, CPU_IFACE, "BoostEnabled")?.first().map(|v| v == "true")
}

fn ps_set_cpu_boost(on: bool) -> Result<(), String> {
    if !std::path::Path::new(BOOST_PATH).exists() {
        return Err("CPU boost can't be changed on this kernel".into());
    }
    ps_set(PS_CPU, CPU_IFACE, "BoostEnabled", "b", if on { "true" } else { "false" })
}

/// PowerStation's range. 0.8 names the properties `MinTdp`/`MaxTdp`; older ones `MinTDP`/`MaxTDP`.
fn ps_range(card: &str) -> (f64, f64) {
    let get = |a: &str, b: &str| ps_num(card, TDP_IFACE, a).or_else(|| ps_num(card, TDP_IFACE, b));
    clamp_range(device_range(), (get("MinTdp", "MinTDP"), get("MaxTdp", "MaxTDP")))
}

fn ps_set_tdp(card: &str, watts: f64) -> Result<(), String> {
    ps_set(card, TDP_IFACE, "TDP", "d", &format!("{watts:.1}"))
}

/// PowerStation's Boost: slow PPT = STAPM + Boost, fast = 1.25 x slow. Leaves STAPM alone.
fn ps_set_boost(card: &str, watts: f64) -> Result<(), String> {
    ps_set(card, TDP_IFACE, "Boost", "d", &format!("{watts:.1}"))
}

/// The live power limits: the SMU's PM table (world-readable, no D-Bus, and PowerStation logs
/// nothing), else PowerStation's TDP and Boost.
fn live_limits(card: &str) -> Option<power_watch::Limits> {
    use std::io::Read;
    let mut b = [0u8; 24];
    let table = fs::File::open(power_watch::PM_TABLE).and_then(|mut f| f.read_exact(&mut b)).ok().and_then(|_| power_watch::parse_pm_table(&b));
    table.or_else(|| Some(power_watch::from_powerstation(ps_num(card, TDP_IFACE, "TDP")?, ps_num(card, TDP_IFACE, "Boost")?)))
}

/// Best effort: the session's power profile (EPP and friends under tuned-ppd / PPD). Polkit lets
/// the active session switch it without a prompt; anything else is skipped quietly.
fn set_ppd(profile: &str) {
    for (bus, path, iface) in [
        ("org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles", "org.freedesktop.UPower.PowerProfiles"),
        ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles", "net.hadess.PowerProfiles"),
    ] {
        if run("busctl", &["--system", "--allow-interactive-authorization=no", "set-property", bus, path, iface, "ActiveProfile", "s", profile]).is_ok() {
            return;
        }
    }
}

/// Writes the energy preference of every CPU. Devices without the tmpfiles rule (not writable by
/// the session user) are skipped quietly.
fn set_epp(epp: &str) {
    let Ok(rd) = fs::read_dir("/sys/devices/system/cpu") else { return };
    for e in rd.flatten() {
        if !is_cpu_dir(&e.file_name().to_string_lossy()) {
            continue;
        }
        let p = e.path().join("cpufreq/energy_performance_preference");
        if let Err(err) = fs::write(&p, epp) {
            if err.kind() != std::io::ErrorKind::PermissionDenied && p.exists() {
                log::warn!("energy preference {}: {err}", p.display());
            }
        }
    }
}

/// `cpu0`, `cpu15`, not `cpufreq` or `cpuidle`.
fn is_cpu_dir(name: &str) -> bool {
    name.strip_prefix("cpu").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn ps_apply(card: &str, w: &Wanted) -> Result<(), String> {
    let (lo, hi) = ps_range(card);
    let (tdp, boost, preset) = resolve(w, lo, hi);
    let mut errs = Vec::new();
    if let Some(p) = preset {
        // Boost first: RyzenAdj's TDP setter keeps the current slow-minus-STAPM gap.
        let tdp_boost = if p.tdp_boost { ps_num(card, TDP_IFACE, "MaxBoost").unwrap_or(0.0) } else { 0.0 };
        if let Err(e) = ps_set_boost(card, tdp_boost) {
            log::warn!("{e}");
        }
        if ps_get(card, TDP_IFACE, "PowerProfilesAvailable").is_some_and(|a| a.iter().any(|x| x == p.ryzenadj)) {
            if let Err(e) = ps_set(card, TDP_IFACE, "PowerProfile", "s", p.ryzenadj) {
                log::warn!("{e}");
            }
        }
        set_ppd(p.ppd);
    }
    if let Some(p) = preset {
        // scx_lavd --autopower follows the CPU energy preference.
        set_epp(p.epp);
    }
    if let Some(t) = tdp {
        ps_set_tdp(card, t).unwrap_or_else(|e| errs.push(e));
    }
    if let Some(b) = boost {
        if std::path::Path::new(BOOST_PATH).exists() {
            ps_set_cpu_boost(b).unwrap_or_else(|e| errs.push(e));
        }
    }
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

// ----- rare-fand -----

const FAND: &str = "org.rare.Fan";
const FAND_PATH: &str = "/org/rare/Fan";

fn fand_preset(name: &str) -> FanMode {
    match name {
        "default" => FanMode::Default,
        "cool" => FanMode::Cool,
        "quiet" => FanMode::Quiet,
        _ => FanMode::Custom,
    }
}

/// rare-fand's preset; None when it isn't running (other devices, stock Bazzite).
fn fand_mode() -> Option<FanMode> {
    let out = run("busctl", &["--system", "get-property", FAND, FAND_PATH, FAND, "Preset"]).ok()?;
    busctl_value(&out)?.first().map(|p| fand_preset(p))
}

/// Polkit lets the active local session through without a prompt (rare-fand checks it).
fn fand_set(mode: FanMode) -> Result<(), String> {
    let name = match mode {
        FanMode::Default => "default",
        FanMode::Cool => "cool",
        FanMode::Quiet => "quiet",
        FanMode::Custom => return Err("Pick Default, Cool or Quiet".into()),
    };
    run("busctl", &["--system", "--allow-interactive-authorization=no", "call", FAND, FAND_PATH, FAND, "SetPreset", "s", name])
        .map(|_| ())
        .map_err(|e| format!("Fan: {e}"))
}

/// The fan as rare-fand reports it, for the curve editor and Settings.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FanStatus {
    /// "default", "cool", "quiet" or "custom".
    pub preset: String,
    /// The curve that runs (edge C, duty %); empty for "default" (the EC's own curve).
    pub curve: Vec<(u32, u32)>,
    /// "ec", "curve", "emergency", "failsafe" or "starting".
    pub mode: String,
    /// C; None when unknown.
    pub temp: Option<f64>,
    /// "edge" or "tctl".
    pub sensor: String,
    pub duty: Option<u32>,
    pub rpm: Option<u32>,
}

/// The fan's points for a preset as the curve editor starts it (hhd's points).
pub fn fan_preset_curve(name: &str) -> Vec<(u32, u32)> {
    match name {
        "quiet" => FAN_QUIET.to_vec(),
        "cool" => FAN_COOL.to_vec(),
        // The EC's own curve is not readable; a middle one between Quiet and Cool.
        _ => FAN_QUIET.iter().zip(FAN_COOL.iter()).map(|(q, c)| (q.0, (q.1 + c.1) / 2)).collect(),
    }
}

/// `a(uu) 3 40 20 60 50 90 100` -> points.
fn parse_points(line: &str) -> Option<Vec<(u32, u32)>> {
    let rest = line.trim().strip_prefix("a(uu)")?.trim();
    let mut nums = rest.split_whitespace().map(|n| n.parse::<u32>().ok());
    let n = nums.next()??;
    let mut out = Vec::new();
    for _ in 0..n {
        out.push((nums.next()??, nums.next()??));
    }
    Some(out)
}

/// `busctl get-property` with Preset Curve Mode Temperature Sensor Duty FanSpeed.
fn parse_fan_status(out: &str) -> Option<FanStatus> {
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let [preset, curve, mode, temp, sensor, duty, rpm] = lines.as_slice() else { return None };
    let one = |l: &str| busctl_value(l).and_then(|v| v.into_iter().next());
    let num = |l: &str| one(l).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v >= 0.0);
    Some(FanStatus {
        preset: one(preset)?,
        curve: parse_points(curve)?,
        mode: one(mode)?,
        temp: num(temp),
        sensor: one(sensor).unwrap_or_default(),
        duty: num(duty).map(|d| d as u32),
        rpm: num(rpm).map(|r| r as u32),
    })
}

/// None when rare-fand isn't running.
pub fn fan_status() -> Option<FanStatus> {
    let out = run("busctl", &["--system", "get-property", FAND, FAND_PATH, FAND, "Preset", "Curve", "Mode", "Temperature", "Sensor", "Duty", "FanSpeed"]).ok()?;
    parse_fan_status(&out)
}

/// A custom curve for rare-fand: 2 to 16 points, temperatures rising within 20..=100 C, duty
/// never falling (rare-fand checks it again).
pub fn set_fan_curve(points: &[(u32, u32)]) -> Result<(), String> {
    let mut args: Vec<String> = ["--system", "--allow-interactive-authorization=no", "call", FAND, FAND_PATH, FAND, "SetCurve", "a(uu)"].iter().map(|s| s.to_string()).collect();
    args.push(points.len().to_string());
    for (t, d) in points {
        args.push(t.to_string());
        args.push(d.to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run("busctl", &refs).map(|_| ()).map_err(|e| format!("Fan: {e}"))
}

/// Battery health for Settings: what it holds against new, and charge cycles (the battery's own
/// count, else rare-fand's count since Rare OS was installed).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatteryHealth {
    /// Full charge against design, percent (one decimal).
    pub health: Option<f64>,
    pub cycles: Option<u32>,
    /// The count is rare-fand's (from the charge it has seen used), not the battery's.
    pub counted: bool,
}

pub fn battery_health() -> Option<BatteryHealth> {
    let rd = fs::read_dir("/sys/class/power_supply").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        let f = |n: &str| read(&p.join(n).to_string_lossy());
        if f("type").as_deref() != Some("Battery") || f("scope").as_deref() == Some("Device") {
            continue;
        }
        let num = |n: &str| f(n).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0);
        let health = num("energy_full").zip(num("energy_full_design")).or_else(|| num("charge_full").zip(num("charge_full_design"))).map(|(a, b)| (a * 1000.0 / b).round() / 10.0);
        let own = num("cycle_count").map(|c| c as u32);
        let counted = if own.is_none() { run("busctl", &["--system", "get-property", FAND, BATTERY_PATH, BATTERY_IFACE, "Cycles"]).ok() } else { None };
        let counted = counted.and_then(|o| busctl_value(&o)?.first()?.parse::<f64>().ok()).filter(|c| *c >= 0.0);
        return Some(BatteryHealth { health, cycles: own.or(counted.map(|c| c.floor() as u32)), counted: own.is_none() && counted.is_some() });
    }
    None
}

/// rare-fand's charge limit (percent) when it can change it; None on an older rare-fand.
pub fn battery_limit() -> Option<u32> {
    let out = run("busctl", &["--system", "get-property", FAND, BATTERY_PATH, BATTERY_IFACE, "Limit"]).ok()?;
    busctl_value(&out)?.first()?.parse().ok()
}

pub fn set_battery_limit(pct: u32) -> Result<(), String> {
    run("busctl", &["--system", "--allow-interactive-authorization=no", "call", FAND, BATTERY_PATH, BATTERY_IFACE, "SetLimit", "u", &pct.to_string()])
        .map(|_| ())
        .map_err(|e| format!("Charge limit: {e}"))
}

// ----- battery care (rare-fand's org.rare.Battery) -----

const BATTERY_PATH: &str = "/org/rare/Battery";
const BATTERY_IFACE: &str = "org.rare.Battery";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatteryCare {
    /// The driver can hold the charge (ayaneo_ec_suiplay with charge_control=1).
    pub available: bool,
    pub on: bool,
    /// "off", "unavailable", "unplugged", "low", "hot", "unknown", "charging" or "holding".
    pub state: String,
}

/// `busctl get-property` with Available Care State: one value per line.
fn parse_battery_care(out: &str) -> Option<BatteryCare> {
    let vals: Vec<String> = out.lines().filter_map(|l| busctl_value(l)?.into_iter().next()).collect();
    match vals.as_slice() {
        [available, on, state] => Some(BatteryCare { available: available == "true", on: on == "true", state: state.clone() }),
        _ => None,
    }
}

/// None when rare-fand (or its battery interface) isn't there.
pub fn battery_care() -> Option<BatteryCare> {
    let out = run("busctl", &["--system", "get-property", FAND, BATTERY_PATH, BATTERY_IFACE, "Available", "Care", "State"]).ok()?;
    parse_battery_care(&out)
}

/// Polkit lets the active local session through without a prompt (rare-fand checks it).
pub fn set_battery_care(on: bool) -> Result<(), String> {
    run(
        "busctl",
        &["--system", "--allow-interactive-authorization=no", "call", FAND, BATTERY_PATH, BATTERY_IFACE, "SetCare", "b", if on { "true" } else { "false" }],
    )
    .map(|_| ())
    .map_err(|e| format!("Battery care: {e}"))
}

// ----- Handheld Daemon -----

/// Waits up to ~8 s for hhd to report `ok` on its state.
fn hhd_wait(ok: impl Fn(&hhd::Power, &hhd::Caps) -> bool) -> Result<(hhd::Power, hhd::Caps), String> {
    let mut last = Err("Handheld Daemon did not answer".to_string());
    for i in 0..16 {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        let p = hhd::decode(&hhd::get_state()?);
        let caps = if p.tdp_ready { hhd::decode_caps(&hhd::get_settings()?) } else { hhd::Caps::default() };
        if ok(&p, &caps) {
            return Ok((p, caps));
        }
        if p.tdp_enabled && !p.tdp_ready {
            if let Some(e) = &p.error {
                return Err(format!("Handheld Daemon can't control TDP: {e}"));
            }
        }
        last = Ok((p, caps));
    }
    match last {
        Ok((p, _)) if !p.tdp_ready => Err("Handheld Daemon did not turn its TDP controls on".into()),
        Ok(v) => Ok(v),
        Err(e) => Err(e),
    }
}

/// hhd with its TDP plugin running; turns the plugin on when it is off.
fn hhd_ready() -> Result<(hhd::Power, hhd::Caps), String> {
    let p = hhd::decode(&hhd::get_state()?);
    if p.tdp_ready {
        let caps = hhd::decode_caps(&hhd::get_settings()?);
        if caps.tdp_max.is_some() {
            return Ok((p, caps));
        }
    } else {
        log::info!("turning on Handheld Daemon's TDP controls");
        hhd::set_state(&hhd::enable_changes())?;
    }
    // The processor settings (CPU boost, EPP) load a loop after TDP; wait for them too, or for
    // hhd to say TuneD/PPD block them. Gives up after ~8 s with what is there.
    hhd_wait(|p, c| p.tdp_ready && c.tdp_max.is_some() && (c.cpu_boost || p.energy_conflict))
}

fn hhd_apply(w: &Wanted) -> Result<(), String> {
    let (p, caps) = hhd_ready()?;
    let (lo, hi) = clamp_range(device_range(), (caps.tdp_min.or(p.tdp_min), caps.tdp_max.or(p.tdp_max)));
    let (tdp, boost, preset) = resolve(w, lo, hi);
    let mut changes = Vec::new();
    if let Some(t) = tdp {
        changes.push((hhd::K_TDP, json!(t.round() as i64)));
    }
    if let Some(p) = preset {
        changes.push((hhd::K_TDP_BOOST, json!(p.tdp_boost)));
    }
    if let (Some(b), true) = (boost, caps.cpu_boost) {
        let epp = preset.map(|p| p.epp).filter(|e| caps.epp.iter().any(|x| x == e));
        changes.extend(hhd::boost_changes(b, epp));
    }
    if changes.is_empty() {
        return Ok(());
    }
    hhd::set_state(&changes).map(|_| ())
}

// ----- backend choice -----

enum Backend {
    PowerStation(String),
    Hhd(hhd::Power),
}

fn backend() -> Option<Backend> {
    if let Some(card) = ps_card() {
        return Some(Backend::PowerStation(card));
    }
    hhd::get_state().ok().map(|s| Backend::Hhd(hhd::decode(&s)))
}

/// hhd's state when its TDP plugin (and so its fan control) runs, whatever does TDP.
fn hhd_running() -> Option<(hhd::Power, hhd::Caps)> {
    let p = hhd::decode(&hhd::get_state().ok()?);
    if !p.tdp_ready {
        return None;
    }
    let caps = hhd::decode_caps(&hhd::get_settings().ok()?);
    Some((p, caps))
}

pub fn perf_get(w: &Wanted) -> Perf {
    let dev = device_range();
    let fand = fand_mode();
    let mut perf = Perf {
        available: false,
        tdp: None,
        tdp_min: dev.0,
        tdp_max: dev.1,
        fps_limit: fps_limit(),
        profile: w.profile,
        note: String::new(),
        backend: None,
        cpu_boost: None,
        fan: None,
        profile_watts: profile_watts(dev.0, dev.1),
    };
    match backend() {
        Some(Backend::PowerStation(card)) => {
            (perf.tdp_min, perf.tdp_max) = ps_range(&card);
            perf.available = true;
            perf.tdp = ps_num(&card, TDP_IFACE, "TDP");
            perf.cpu_boost = ps_cpu_boost();
            perf.backend = Some("powerstation".into());
            // PowerStation has no fan control: rare-fand on Rare OS, else hhd's when its TDP plugin runs.
            perf.fan = if fand.is_some() { fand } else { hhd_running().filter(|(_, c)| c.fan).and_then(|(p, _)| fan_mode(&p)) };
        }
        Some(Backend::Hhd(p)) => {
            perf.backend = Some("hhd".into());
            perf.available = !(p.tdp_enabled && !p.tdp_ready && p.error.is_some());
            if let Some(e) = p.error.as_ref().filter(|_| !perf.available) {
                perf.note = format!("Handheld Daemon can't control TDP: {e}");
            }
            if p.tdp_ready {
                let caps = hhd::get_settings().map(|s| hhd::decode_caps(&s)).unwrap_or_default();
                (perf.tdp_min, perf.tdp_max) = clamp_range(dev, (caps.tdp_min.or(p.tdp_min), caps.tdp_max.or(p.tdp_max)));
                perf.tdp = p.tdp;
                perf.fan = if caps.fan { fan_mode(&p) } else { None };
                perf.cpu_boost = if caps.cpu_boost && p.energy_live {
                    // In auto mode hhd follows the power profile; report what the CPU does.
                    if p.energy_manual { p.cpu_boost } else { sysfs_boost() }
                } else {
                    None
                };
                if perf.cpu_boost.is_none() && !p.energy_live {
                    perf.note = "CPU boost: Handheld Daemon's processor settings are off while TuneD or power-profiles-daemon runs.".into();
                }
            } else if perf.available {
                // Turned on by the first change; until then the firmware's defaults apply.
                perf.cpu_boost = sysfs_boost();
            }
        }
        None => perf.note = "TDP control is not available on this device".into(),
    }
    // rare-fand owns the fan wherever it runs.
    if fand.is_some() {
        perf.fan = fand;
    }
    perf.profile_watts = profile_watts(perf.tdp_min, perf.tdp_max);
    perf
}

/// Whether the CPU may boost right now, from sysfs.
fn sysfs_boost() -> Option<bool> {
    ["/sys/devices/system/cpu/cpufreq/boost", "/sys/devices/system/cpu/cpu0/cpufreq/boost", "/sys/devices/system/cpu/amd_pstate/cpb_boost"]
        .iter()
        .find_map(|p| read(p))
        .map(|v| v == "1" || v == "enabled")
}

/// Applies a whole wish: the profile (or slider watts) and the CPU boost.
pub fn perf_apply(w: &Wanted) -> Result<(), String> {
    match backend().ok_or("TDP control is not available on this device")? {
        Backend::PowerStation(card) => ps_apply(&card, w),
        Backend::Hhd(_) => hhd_apply(w),
    }
}

pub fn set_tdp(watts: f64) -> Result<(), String> {
    match backend().ok_or("TDP control is not available on this device")? {
        Backend::PowerStation(card) => {
            let (lo, hi) = ps_range(&card);
            // Boost first, as in ps_apply: the slider is the sustained limit, with no gap above it.
            if let Err(e) = ps_set_boost(&card, 0.0) {
                log::warn!("{e}");
            }
            ps_set_tdp(&card, watts.clamp(lo, hi))
        }
        Backend::Hhd(_) => {
            let (p, caps) = hhd_ready()?;
            let (lo, hi) = clamp_range(device_range(), (caps.tdp_min.or(p.tdp_min), caps.tdp_max.or(p.tdp_max)));
            hhd::set_state(&[(hhd::K_TDP, json!(watts.clamp(lo, hi).round() as i64))]).map(|_| ())
        }
    }
}

pub fn set_cpu_boost(on: bool) -> Result<(), String> {
    match backend().ok_or("CPU boost can't be changed on this device")? {
        Backend::PowerStation(_) => ps_set_cpu_boost(on),
        Backend::Hhd(_) => {
            let (p, caps) = hhd_ready()?;
            if !caps.cpu_boost || p.energy_conflict {
                return Err("CPU boost: Handheld Daemon's processor settings are off while TuneD or power-profiles-daemon runs".into());
            }
            hhd::set_state(&hhd::boost_changes(on, None)).map(|_| ())
        }
    }
}

pub fn set_fan(mode: FanMode) -> Result<(), String> {
    if mode == FanMode::Custom {
        return Err("Pick Default, Cool or Quiet".into());
    }
    if fand_mode().is_some() {
        return fand_set(mode);
    }
    let (_, caps) = hhd_running().ok_or("Fan control needs rare-fand or Handheld Daemon; PowerStation has none")?;
    if !caps.fan {
        return Err("Handheld Daemon found no fan it can control".into());
    }
    let changes = hhd::fan_changes(fan_curve(mode));
    let changes: Vec<(&str, serde_json::Value)> = changes.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    hhd::set_state(&changes).map(|_| ())
}

type WantedFn = Box<dyn Fn() -> Wanted + Send + Sync>;
static WANTED: std::sync::OnceLock<WantedFn> = std::sync::OnceLock::new();

/// The saved wish, once `keep` runs.
fn wanted_now() -> Option<Wanted> {
    WANTED.get().map(|f| f())
}

/// Keeps the wish applied: at start, after a resume and when the charger comes or goes (the
/// firmware puts its own limits back then, and RyzenAdj settings don't survive), and when the
/// firmware-reset watcher (power_watch) sees the firmware's own limits or a Boost gap nobody
/// here asked for. Any other TDP (Steam's slider) is left as it is.
pub fn keep(wanted: impl Fn() -> Wanted + Send + Sync + 'static) {
    let _ = WANTED.set(Box::new(wanted));
    std::thread::spawn(move || {
        let apply = |why: &str| match wanted_now().map(|w| perf_apply(&w)) {
            Some(Ok(())) => log::info!("power settings applied ({why})"),
            Some(Err(e)) => log::warn!("power settings ({why}): {e}"),
            None => {}
        };
        apply("start");
        let mut asleep = suspended_secs();
        let mut ac = on_ac();
        let mut watcher = power_watch::Watcher::default();
        let mut card: Option<String> = None;
        let mut tick = 0u64;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            tick += 1;
            let now = suspended_secs();
            let resumed = matches!((asleep, now), (Some(a), Some(b)) if b > a + 2);
            asleep = now;
            let plug = on_ac();
            let plugged = plug != ac;
            ac = plug;
            if resumed || plugged {
                // Give the firmware a moment to finish its own changes.
                std::thread::sleep(Duration::from_secs(3));
                apply(if resumed { "resume" } else { "charger" });
                continue;
            }
            // Every 10 s: the live limits, through PowerStation's card (looked up once).
            if tick % 2 != 0 {
                continue;
            }
            if card.is_none() {
                card = ps_card();
            }
            let Some(c) = card.as_deref() else { continue };
            let gap_ok = wanted_now().and_then(|w| preset(w.profile)).is_some_and(|p| p.tdp_boost);
            let limits = live_limits(c);
            match watcher.tick(std::time::Instant::now(), limits, gap_ok) {
                power_watch::Fix::None => {}
                power_watch::Fix::Reapply => {
                    log::info!("firmware power limits back ({limits:?})");
                    apply("firmware reset");
                }
                power_watch::Fix::ZeroBoost => match ps_set_boost(c, 0.0) {
                    Ok(()) => log::info!("power: TDP boost gap removed, TDP kept ({limits:?})"),
                    Err(e) => {
                        log::warn!("{e}");
                        card = None;
                    }
                },
            }
        }
    });
}

/// The game-load boost window (power_watch::BoostWindow): CPU boost on from launch until 10 s
/// after the game's first frame, at most 60 s, then the profile's setting again. Nothing in
/// Quiet, nor when the wish has boost on anyway. `target`: the app id gamescope will show for
/// the game, when known. A newer launch takes over a window that is still open.
pub fn load_boost(id: String, target: Option<u64>, running: impl Fn(&str) -> bool + Send + 'static) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GEN: AtomicU64 = AtomicU64::new(0);
    let Some(w) = wanted_now() else { return };
    if w.profile == Profile::Quiet || resolve(&w, 0.0, f64::MAX).1 != Some(false) {
        return;
    }
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let focused = || if crate::host::get().gamescope_active() { crate::host::get().gamescope_focused_app() } else { None };
    std::thread::spawn(move || {
        let initial = focused();
        if let Err(e) = set_cpu_boost(true) {
            log::warn!("load boost: {e}");
            return;
        }
        log::info!("load boost on for {id}");
        let mut win = power_watch::BoostWindow::new(std::time::Instant::now(), target, initial);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if GEN.load(Ordering::SeqCst) != gen {
                // A newer launch owns boost now and restores it when its window ends.
                return;
            }
            if win.step(std::time::Instant::now(), focused(), running(&id)) == power_watch::Step::End {
                break;
            }
        }
        // Back to what the wish says now (the profile may have changed meanwhile).
        let restore = wanted_now().and_then(|w| resolve(&w, 0.0, f64::MAX).1);
        if let Some(b) = restore {
            if let Err(e) = set_cpu_boost(b) {
                log::warn!("load boost restore: {e}");
            }
        }
        log::info!("load boost done for {id} (first frame {:?}, boost back to {restore:?})", win.first_frame());
    });
}

/// Seconds spent suspended since boot (CLOCK_BOOTTIME - CLOCK_MONOTONIC).
#[cfg(target_os = "linux")]
fn suspended_secs() -> Option<i64> {
    let get = |c| {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: clock_gettime writes into the timespec we own.
        (unsafe { libc::clock_gettime(c, &mut ts) } == 0).then_some(ts.tv_sec as i64)
    };
    Some(get(libc::CLOCK_BOOTTIME)? - get(libc::CLOCK_MONOTONIC)?)
}

#[cfg(not(target_os = "linux"))]
fn suspended_secs() -> Option<i64> {
    None
}

fn on_ac() -> Option<bool> {
    let rd = fs::read_dir("/sys/class/power_supply").ok()?;
    rd.flatten().find_map(|e| {
        let p = e.path();
        (read(&p.join("type").to_string_lossy()).as_deref() == Some("Mains")).then(|| read(&p.join("online").to_string_lossy()).as_deref() == Some("1"))
    })
}

#[cfg(test)]
mod perf_tests {
    #[test]
    fn ps_card_path_matches_both_cases() {
        assert!(super::is_ps_card_path("/org/shadowblip/Performance/GPU/card1"));
        assert!(super::is_ps_card_path("/org/shadowblip/Performance/GPU/Card0"));
        assert!(!super::is_ps_card_path("/org/shadowblip/Performance/GPU/card1/eDP"));
        assert!(!super::is_ps_card_path("/org/shadowblip/Performance/GPU/card"));
        assert!(!super::is_ps_card_path("/org/shadowblip/Performance/GPU"));
    }

    use super::*;

    #[test]
    fn battery_time_left_and_to_full() {
        // 30 Wh left at 15 W is 2 h; charging 20 Wh to go at 40 W is 30 min.
        assert_eq!(battery_minutes("Discharging", 30e6, 50e6, 15e6), Some(120));
        assert_eq!(battery_minutes("Charging", 30e6, 50e6, 40e6), Some(30));
        assert_eq!(battery_minutes("Full", 50e6, 50e6, 1e6), None);
        assert_eq!(battery_minutes("Discharging", 30e6, 50e6, 0.0), None);
    }

    #[test]
    fn reads_battery_care() {
        assert_eq!(
            parse_battery_care("b true\nb false\ns \"off\"\n"),
            Some(BatteryCare { available: true, on: false, state: "off".into() })
        );
        assert_eq!(parse_battery_care("b true\nb true\ns \"holding\""), Some(BatteryCare { available: true, on: true, state: "holding".into() }));
        assert_eq!(parse_battery_care("b false\n"), None);
    }

    #[test]
    fn profiles() {
        assert_eq!(profile_tdp(Profile::Quiet, 5.0, 28.0), Some(10.0));
        assert_eq!(profile_tdp(Profile::Balanced, 5.0, 28.0), Some(15.0));
        assert_eq!(profile_tdp(Profile::Performance, 5.0, 28.0), Some(25.0));
        assert_eq!(profile_tdp(Profile::Custom, 5.0, 28.0), None);
        // Clamped to the device.
        assert_eq!(profile_tdp(Profile::Performance, 5.0, 18.0), Some(18.0));
        assert_eq!(profile_tdp(Profile::Quiet, 12.0, 28.0), Some(12.0));
        let q = preset(Profile::Quiet).unwrap();
        let b = preset(Profile::Balanced).unwrap();
        let p = preset(Profile::Performance).unwrap();
        assert!(!q.cpu_boost && !b.cpu_boost && p.cpu_boost);
        // Boost 0 everywhere: sustained power is capped at STAPM through the 5 s slow filter.
        assert!(!q.tdp_boost && !b.tdp_boost && !p.tdp_boost);
        assert_eq!((q.epp, b.epp, p.epp), ("power", "balance_power", "balance_performance"));
        assert_eq!(Wanted::default().profile, Profile::Balanced);
    }

    #[test]
    fn quick_settings_watts() {
        assert_eq!(profile_watts(5.0, 28.0), ProfileWatts { quiet: 10.0, balanced: 15.0, performance: 25.0 });
        // A device or PowerStation range that is narrower shows what is really set.
        assert_eq!(profile_watts(12.0, 20.0), ProfileWatts { quiet: 12.0, balanced: 15.0, performance: 20.0 });
        let json = serde_json::to_value(profile_watts(5.0, 28.0)).unwrap();
        assert_eq!(json["quiet"], 10.0);
    }

    #[test]
    fn energy_preference_per_profile() {
        let epp = |p| preset(p).map(|s| s.epp);
        assert_eq!(epp(Profile::Quiet), Some("power"));
        assert_eq!(epp(Profile::Balanced), Some("balance_power"));
        assert_eq!(epp(Profile::Performance), Some("balance_performance"));
        assert_eq!(epp(Profile::Custom), None);
        assert!(is_cpu_dir("cpu0") && is_cpu_dir("cpu15"));
        assert!(!is_cpu_dir("cpufreq") && !is_cpu_dir("cpuidle") && !is_cpu_dir("cpu"));
    }

    #[test]
    fn resolves_wishes() {
        let w = Wanted::default();
        assert_eq!(resolve(&w, 5.0, 28.0).0, Some(15.0));
        assert_eq!(resolve(&w, 5.0, 28.0).1, Some(false));
        // A hand-set boost wins over the profile's.
        let w = Wanted { cpu_boost: Some(true), ..w };
        assert_eq!(resolve(&w, 5.0, 28.0).1, Some(true));
        // Custom uses the slider, clamped; no slider value leaves TDP alone.
        let c = Wanted { profile: Profile::Custom, tdp: Some(40.0), cpu_boost: None };
        assert_eq!(resolve(&c, 5.0, 28.0), (Some(28.0), None, None));
        assert_eq!(resolve(&Wanted { tdp: None, ..c }, 5.0, 28.0).0, None);
    }

    #[test]
    fn old_battery_profile_loads_as_quiet() {
        assert_eq!(serde_json::from_str::<Profile>("\"battery\"").unwrap(), Profile::Quiet);
        assert_eq!(serde_json::to_string(&Profile::Quiet).unwrap(), "\"quiet\"");
        assert_eq!(serde_json::to_string(&FanMode::Cool).unwrap(), "\"cool\"");
    }

    #[test]
    fn ranges() {
        assert_eq!(clamp_range((5.0, 28.0), (Some(4.0), Some(30.0))), (5.0, 28.0));
        assert_eq!(clamp_range((5.0, 28.0), (Some(6.0), Some(25.0))), (6.0, 25.0));
        assert_eq!(clamp_range((5.0, 28.0), (None, None)), (5.0, 28.0));
        assert_eq!(clamp_range((5.0, 28.0), (Some(30.0), Some(35.0))), (5.0, 28.0));
    }

    #[test]
    fn fan_presets() {
        for c in [&FAN_COOL, &FAN_QUIET] {
            assert_eq!(c.map(|(t, _)| t), hhd::FAN_POINTS);
            assert!(c.windows(2).all(|w| w[0].1 <= w[1].1), "curves only rise");
            assert!(c.iter().all(|(_, v)| *v <= 100 && v % 2 == 0), "hhd steps by 2%");
        }
        // Quiet holds low until about 65 C; Cool is never quieter than Quiet.
        assert!(FAN_QUIET.iter().filter(|(t, _)| *t <= 65).all(|(_, v)| *v <= 30));
        assert!(FAN_COOL.iter().zip(FAN_QUIET.iter()).all(|(c, q)| c.1 >= q.1));
        let p = |mode: &str, c: &[(u32, u32)]| hhd::Power { fan_mode: Some(mode.into()), fan_curve: c.iter().copied().collect(), ..Default::default() };
        assert_eq!(fan_mode(&p("disabled", &[])), Some(FanMode::Default));
        assert_eq!(fan_mode(&p("manual_edge", &FAN_COOL)), Some(FanMode::Cool));
        assert_eq!(fan_mode(&p("manual_edge", &FAN_QUIET)), Some(FanMode::Quiet));
        assert_eq!(fan_mode(&p("manual_edge", &[(40, 30)])), Some(FanMode::Custom));
        assert_eq!(fan_mode(&p("manual_junction", &FAN_COOL)), Some(FanMode::Custom));
        assert_eq!(fan_mode(&hhd::Power::default()), None);
    }

    #[test]
    fn fand_presets() {
        // What `busctl get-property org.rare.Fan /org/rare/Fan org.rare.Fan Preset` prints.
        let p = |out: &str| busctl_value(out).and_then(|v| v.first().map(|s| fand_preset(s)));
        assert_eq!(p("s \"cool\""), Some(FanMode::Cool));
        assert_eq!(p("s \"quiet\""), Some(FanMode::Quiet));
        assert_eq!(p("s \"default\""), Some(FanMode::Default));
        assert_eq!(p("s \"custom\""), Some(FanMode::Custom));
        assert!(fand_set(FanMode::Custom).is_err());
        // rare-fand's built-in Cool and Quiet are these curves (image/rare-fand/src/curve.rs).
        let fand_cool = [30, 36, 46, 56, 66, 76, 86, 100, 100];
        let fand_quiet = [20, 20, 20, 22, 26, 30, 50, 80, 100];
        assert_eq!(FAN_COOL.map(|(_, v)| v), fand_cool);
        assert_eq!(FAN_QUIET.map(|(_, v)| v), fand_quiet);
    }

    #[test]
    fn reads_busctl() {
        assert_eq!(busctl_value("d 15").unwrap(), vec!["15"]);
        assert_eq!(busctl_value("b true\n").unwrap(), vec!["true"]);
        assert_eq!(busctl_value("s \"power-saving\"").unwrap(), vec!["power-saving"]);
        assert_eq!(busctl_value("as 2 \"max-performance\" \"power-saving\"").unwrap(), vec!["max-performance", "power-saving"]);
        assert_eq!(busctl_value("as 0").unwrap(), Vec::<String>::new());
        assert!(busctl_value("").is_none());
    }
}

fn fps_limit() -> Option<u32> {
    let out = run("xprop", &["-root", "GAMESCOPE_FPS_LIMIT"]).ok()?;
    out.rsplit('=').next()?.trim().parse().ok().filter(|v| *v > 0)
}

/// 0 removes the limit.
pub fn set_fps_limit(fps: u32) -> Result<(), String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("FPS limit needs the gamescope session".into());
    }
    if fps == 0 {
        run("xprop", &["-root", "-remove", "GAMESCOPE_FPS_LIMIT"]).map(|_| ())
    } else {
        run("xprop", &["-root", "-f", "GAMESCOPE_FPS_LIMIT", "32c", "-set", "GAMESCOPE_FPS_LIMIT", &fps.to_string()]).map(|_| ())
    }
}

// ---------- screen and sound ----------

pub fn backlight() -> Option<(String, u32, u32)> {
    let rd = fs::read_dir("/sys/class/backlight").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        let max: u32 = read(&p.join("max_brightness").to_string_lossy())?.parse().ok()?;
        let cur: u32 = read(&p.join("brightness").to_string_lossy())?.parse().ok()?;
        return Some((e.file_name().to_string_lossy().into_owned(), cur, max));
    }
    None
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Levels {
    /// 0..=100, None when there is nothing to control.
    pub brightness: Option<u8>,
    pub volume: Option<u8>,
    pub muted: bool,
}

pub fn levels() -> Levels {
    let brightness = crate::brightness::current();
    let vol = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]).ok();
    let volume = vol.as_ref().and_then(|v| v.split_whitespace().nth(1)?.parse::<f64>().ok()).map(|v| (v * 100.0).round().min(150.0) as u8);
    Levels { brightness, volume, muted: vol.map(|v| v.contains("MUTED")).unwrap_or(false) }
}

pub fn set_brightness(pos: u8) -> Result<(), String> {
    crate::brightness::set(pos)
}

pub fn set_volume(percent: u8) -> Result<(), String> {
    run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{}%", percent.min(100))]).map(|_| ())
}

pub fn power(action: &str) -> Result<(), String> {
    match action {
        "suspend" | "reboot" | "poweroff" => run("systemctl", &[action]).map(|_| ()),
        // Bazzite's session switcher (steamos-manager); desktop mode is the KDE session.
        "desktop" => run("steamosctl", &["switch-to-desktop-mode"]).map(|_| ()),
        _ => Err(format!("unknown power action {action}")),
    }
}

#[cfg(test)]
mod fan_tests {
    use super::*;

    #[test]
    fn reads_rare_fand_status() {
        let out = "s \"custom\"\na(uu) 3 40 20 60 50 90 100\ns \"curve\"\nd 54.5\ns \"edge\"\ni 41\ni 2898\n";
        let f = parse_fan_status(out).unwrap();
        assert_eq!(f.preset, "custom");
        assert_eq!(f.curve, vec![(40, 20), (60, 50), (90, 100)]);
        assert_eq!((f.mode.as_str(), f.temp, f.duty, f.rpm), ("curve", Some(54.5), Some(41), Some(2898)));
        let ec = parse_fan_status("s \"default\"\na(uu) 0\ns \"ec\"\nd -1\ns \"edge\"\ni -1\ni -1").unwrap();
        assert_eq!((ec.curve.len(), ec.temp, ec.duty, ec.rpm), (0, None, None, None));
        assert!(parse_fan_status("s \"default\"").is_none());
        let mid = fan_preset_curve("default");
        assert_eq!(mid.len(), 9);
        assert!(mid.windows(2).all(|w| w[1].1 >= w[0].1 && w[1].0 > w[0].0));
    }
}
