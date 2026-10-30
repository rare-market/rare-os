//! Night light and the TV's resolution and refresh, on gamescope's runtime root-window atoms (the
//! same xprop path as upscale.rs and the FPS limit). Saved in ~/.config/rare/screen.json and put back
//! when the launcher starts, every minute for the night light's hours, and when a TV is plugged in.
//!
//!   GAMESCOPE_COLOR_NIGHT_MODE            CARDINAL 32 x3: f32 bits of amount (0 off, 1 on), hue, saturation
//!   GAMESCOPE_DISPLAY_IS_EXTERNAL         CARDINAL 32: 1 when the picture is on an external display
//!   GAMESCOPE_DISPLAY_MODE_LIST_EXTERNAL  STRING: "3840x2160@60 1920x1080@120 ..." (gamescope writes it)
//!   GAMESCOPE_XWAYLAND_MODE_CONTROL       CARDINAL 32 x4: server, width, height, allow larger than the output
//!   GAMESCOPE_DYNAMIC_REFRESH_EXTERNAL    CARDINAL 32: refresh in Hz for the external display, 0 its default
//!
//! gamescope cannot change the TV's own output mode while it runs (no atom reaches drm_set_resolution),
//! so the resolution is the size games and Rare render at, scaled to the TV by gamescope, as Steam's
//! gaming mode does. The refresh does change the TV's mode.

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct NightLight {
    pub on: bool,
    /// 10 (a little warm) to 100 (very warm).
    pub warmth: u8,
    /// Only from 9 PM to 7 AM.
    pub scheduled: bool,
}

impl Default for NightLight {
    fn default() -> Self {
        NightLight { on: false, warmth: 50, scheduled: false }
    }
}

/// The TV's picture: 0 x 0 is the TV's own size, refresh 0 its default.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TvMode {
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ScreenSettings {
    pub night_light: NightLight,
    pub tv: TvMode,
}

pub const SCHEDULE_FROM: u32 = 21;
pub const SCHEDULE_TO: u32 = 7;
/// Orange, as Steam's night mode.
const HUE: f32 = 30.0 / 360.0;
const MAX_SATURATION: f32 = 0.65;

fn path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config/rare/screen.json"))
}

pub fn saved() -> ScreenSettings {
    path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(s: &ScreenSettings) -> Result<(), String> {
    let p = path().ok_or("No home folder")?;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

/// Whether the night light shows at this local hour.
pub fn night_active(n: &NightLight, hour: u32) -> bool {
    n.on && (!n.scheduled || hour >= SCHEDULE_FROM || hour < SCHEDULE_TO)
}

/// The three f32s gamescope reads, as the u32 bits xprop writes. All zero is off.
pub fn night_values(n: &NightLight, hour: u32) -> [u32; 3] {
    if !night_active(n, hour) {
        return [0, 0, 0];
    }
    let amount = (n.warmth.clamp(10, 100) as f32) / 100.0;
    // gamescope multiplies by hsv(hue, saturation * amount, 1): amount 1 and the saturation carry the warmth.
    [1.0f32.to_bits(), HUE.to_bits(), (MAX_SATURATION * amount).to_bits()]
}

fn local_hour() -> u32 {
    #[cfg(unix)]
    unsafe {
        extern "C" {
            fn tzset();
        }
        // A new time zone (Settings > System) is read again.
        tzset();
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&t, &mut tm).is_null() {
            return tm.tm_hour as u32;
        }
    }
    12
}

fn gamescope() -> Result<(), String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("This needs the Rare game mode session".into());
    }
    Ok(())
}

fn xprop(args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new("xprop");
    crate::host::env(cmd.args(args));
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(format!("xprop: {e}")),
    }
}

fn set_atom(atom: &str, vals: &[u32]) -> Result<(), String> {
    xprop(&crate::upscale::xprop_args(atom, vals)).map(|_| ())
}

fn remove_atom(atom: &str) -> Result<(), String> {
    xprop(&["-root".into(), "-remove".into(), atom.into()]).map(|_| ())
}

static TV_MODES: Mutex<Option<Vec<(u32, u32, u32)>>> = Mutex::new(None);

/// The TV's modes as last read by the `start` loop (no process on the caller's thread).
pub fn cached_tv_modes() -> Option<Vec<(u32, u32, u32)>> {
    TV_MODES.lock().ok()?.clone()
}

static APPLIED_NIGHT: Mutex<Option<[u32; 3]>> = Mutex::new(None);

/// Puts the night light in place for now (only touches gamescope when it changes).
pub fn night_apply_now(n: &NightLight) -> Result<(), String> {
    gamescope()?;
    let v = night_values(n, local_hour());
    let mut last = APPLIED_NIGHT.lock().unwrap_or_else(|e| e.into_inner());
    if *last == Some(v) {
        return Ok(());
    }
    set_atom("GAMESCOPE_COLOR_NIGHT_MODE", &v)?;
    *last = Some(v);
    Ok(())
}

pub fn night_set(n: NightLight) -> Result<(), String> {
    let mut s = saved();
    s.night_light = n;
    save(&s)?;
    night_apply_now(&n)
}

// ---------- the TV ----------

/// "3840x2160@60 1920x1080@120" into (width, height, refresh), in gamescope's order.
pub fn parse_modes(list: &str) -> Vec<(u32, u32, u32)> {
    list.split_whitespace()
        .filter_map(|m| {
            let (size, hz) = m.split_once('@')?;
            let (w, h) = size.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?, hz.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()?))
        })
        .collect()
}

/// The xprop line `NAME(STRING) = "..."` to its string.
fn xprop_string(out: &str) -> Option<String> {
    let v = out.split_once('=')?.1.trim();
    Some(v.trim_matches('"').to_string())
}

/// The picture is on an external display (gamescope says so).
pub fn is_external() -> bool {
    if gamescope().is_err() {
        return false;
    }
    xprop(&["-root".into(), "GAMESCOPE_DISPLAY_IS_EXTERNAL".into()]).ok().and_then(|o| o.rsplit('=').next().map(|v| v.trim() == "1")).unwrap_or(false)
}

/// The modes the TV offers, or None when not on a TV.
pub fn tv_modes() -> Option<Vec<(u32, u32, u32)>> {
    if !is_external() {
        return None;
    }
    let out = xprop(&["-root".into(), "GAMESCOPE_DISPLAY_MODE_LIST_EXTERNAL".into()]).ok()?;
    let m = parse_modes(&xprop_string(&out)?);
    (!m.is_empty()).then_some(m)
}

/// Sizes worth offering: the common ones the TV has, biggest first, at most four.
pub fn tv_sizes(modes: &[(u32, u32, u32)]) -> Vec<(u32, u32)> {
    const COMMON: [(u32, u32); 6] = [(3840, 2160), (2560, 1440), (1920, 1200), (1920, 1080), (1600, 900), (1280, 720)];
    COMMON.iter().copied().filter(|(w, h)| modes.iter().any(|m| m.0 == *w && m.1 == *h)).take(4).collect()
}

/// Refresh rates the TV offers at its own size (the output mode gamescope runs), 120 Hz first.
pub fn tv_refreshes(modes: &[(u32, u32, u32)]) -> Vec<u32> {
    let Some(first) = modes.first() else { return vec![] };
    let mut v: Vec<u32> = modes.iter().filter(|m| m.0 == first.0 && m.1 == first.1).map(|m| m.2).filter(|r| [50, 60, 100, 120, 144].contains(r)).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v
}

/// The size games render at now: the TV choice on a TV, else the built-in panel's.
pub fn native_size() -> (u32, u32) {
    let tv = saved().tv;
    if tv.width > 0 && tv.height > 0 && is_external() {
        return (tv.width, tv.height);
    }
    (1920, 1200)
}

/// Puts the saved TV mode in place (when on a TV).
pub fn tv_apply_now(t: &TvMode) -> Result<(), String> {
    gamescope()?;
    let Some(modes) = tv_modes() else { return Ok(()) };
    let (w, h) = if t.width > 0 && t.height > 0 { (t.width, t.height) } else { modes.first().map(|m| (m.0, m.1)).unwrap_or((1920, 1080)) };
    set_atom("GAMESCOPE_XWAYLAND_MODE_CONTROL", &[0, w, h, 1])?;
    if t.refresh > 0 {
        set_atom("GAMESCOPE_DYNAMIC_REFRESH_EXTERNAL", &[t.refresh])
    } else {
        remove_atom("GAMESCOPE_DYNAMIC_REFRESH_EXTERNAL").or(Ok(()))
    }
}

pub fn tv_set(t: TvMode) -> Result<(), String> {
    let mut s = saved();
    s.tv = t;
    save(&s)?;
    tv_apply_now(&t)
}

/// At launcher start: puts both back, then keeps the night light's hours and follows a TV being
/// plugged in. One cheap check a minute.
pub fn start() {
    std::thread::Builder::new()
        .name("rare-screen".into())
        .spawn(|| {
            if gamescope().is_err() {
                return;
            }
            let mut was_external = None;
            loop {
                let s = saved();
                let _ = night_apply_now(&s.night_light);
                let ext = is_external();
                let modes = if ext { tv_modes() } else { None };
                if let Ok(mut m) = TV_MODES.lock() {
                    *m = modes;
                }
                if was_external != Some(ext) {
                    if ext && s.tv != TvMode::default() {
                        let _ = tv_apply_now(&s.tv);
                    }
                    was_external = Some(ext);
                }
                std::thread::sleep(std::time::Duration::from_secs(if ext { 20 } else { 60 }));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_light_hours_and_values() {
        let n = NightLight { on: true, warmth: 100, scheduled: true };
        assert!(night_active(&n, 22));
        assert!(night_active(&n, 3));
        assert!(!night_active(&n, 12));
        assert_eq!(night_values(&n, 12), [0, 0, 0]);
        let v = night_values(&n, 23);
        assert_eq!(f32::from_bits(v[0]), 1.0);
        assert!((f32::from_bits(v[2]) - MAX_SATURATION).abs() < 1e-6);
        let off = NightLight { on: false, ..n };
        assert_eq!(night_values(&off, 23), [0, 0, 0]);
    }

    #[test]
    fn tv_modes_parse() {
        let out = "GAMESCOPE_DISPLAY_MODE_LIST_EXTERNAL(STRING) = \"3840x2160@60 3840x2160@30 2560x1440@120 1920x1080@120 1920x1080@60 1280x720@60\"";
        let m = parse_modes(&xprop_string(out).unwrap());
        assert_eq!(m.len(), 6);
        assert_eq!(m[0], (3840, 2160, 60));
        assert_eq!(tv_sizes(&m), vec![(3840, 2160), (2560, 1440), (1920, 1080), (1280, 720)]);
        assert_eq!(tv_refreshes(&m), vec![60]);
    }
}
