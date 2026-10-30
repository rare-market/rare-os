//! Screen brightness that feels even, night dim below the backlight minimum, dim after idle.
//!
//! The slider position `pos` is 0..=100.
//!   pos 10..=100  the backlight, raw floor..max, shaped by `raw = floor + (1 - floor) * u^gamma`
//!                 with u = (pos - 10) / 90.
//!   pos 0..=10    backlight stays at the floor; gamescope's G22 look (a .cube that scales output)
//!                 darkens the picture further, down to NIGHT_MIN_GAIN of linear light.
//!
//! About the curve: the amdgpu backlight reports `scale: non-linear` and the firmware applies a
//! "custom brightness curve", so raw is already meant to be perceptual and gamma 1.0 would be
//! right if that curve were exact. It could not be measured from the device over SSH (no lux
//! meter, dmesg is root only), so the default gamma is 1.4, a modest lift of the low end, and it
//! can be tuned without a rebuild with "gamma" in ~/.config/rare/brightness.json once the
//! coordinator has the nits-per-raw table.
//!
//! Night dim does NOT save power: the LCD backlight still burns the same watts at the floor.
//! Brightness keys write the backlight directly; we notice that by comparing the raw value with
//! the one we last wrote, take the key's value as the new position and drop the look.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

pub const POS_HW: f64 = 10.0;
const RAW_FLOOR: f64 = 0.03;
const DEFAULT_GAMMA: f64 = 1.4;
/// Linear-light gain at slider 0.
const NIGHT_MIN_GAIN: f64 = 0.12;
/// Dim after idle goes to this share of the current perceptual position, never above current.
const IDLE_POS: f64 = 30.0;
const ATOM: &str = "GAMESCOPE_COLOR_LOOK_G22";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub pos: f64,
    pub gamma: f64,
    pub dim_idle: bool,
    /// Raw value we last wrote; a different reading means the brightness keys moved it.
    pub last_raw: Option<u32>,
}

impl Default for State {
    fn default() -> Self {
        State { pos: 70.0, gamma: DEFAULT_GAMMA, dim_idle: false, last_raw: None }
    }
}

/// Backlight fraction (0..1 of max) and linear-light gain (1.0 = no dim) for a slider position.
pub fn plan(pos: f64, gamma: f64) -> (f64, f64) {
    let pos = pos.clamp(0.0, 100.0);
    if pos < POS_HW {
        return (RAW_FLOOR, NIGHT_MIN_GAIN.powf(1.0 - pos / POS_HW));
    }
    let u = (pos - POS_HW) / (100.0 - POS_HW);
    (RAW_FLOOR + (1.0 - RAW_FLOOR) * u.powf(gamma), 1.0)
}

/// Inverse of `plan` for the backlight part (used when a key moved the backlight).
pub fn pos_from_frac(frac: f64, gamma: f64) -> f64 {
    if frac <= RAW_FLOOR + 1e-4 {
        return POS_HW;
    }
    let u = ((frac - RAW_FLOOR) / (1.0 - RAW_FLOOR)).clamp(0.0, 1.0).powf(1.0 / gamma);
    POS_HW + (100.0 - POS_HW) * u
}

/// A 5-point 3D LUT (the size gamescope's own looks use) that scales linear light by `gain`,
/// written in the g22 encoded domain.
pub fn cube(gain: f64) -> String {
    let k = gain.clamp(0.0, 1.0).powf(1.0 / 2.2);
    let mut s = String::from("TITLE \"Rare night dim\"\nLUT_3D_SIZE 5\n");
    for b in 0..5 {
        for g in 0..5 {
            for r in 0..5 {
                let f = |i: i32| i as f64 / 4.0 * k;
                s.push_str(&format!("{:.6} {:.6} {:.6}\n", f(r), f(g), f(b)));
            }
        }
    }
    s
}

fn dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".config/rare"))
}

fn load() -> State {
    dir().and_then(|d| std::fs::read_to_string(d.join("brightness.json")).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(s: &State) {
    if let Some(d) = dir() {
        let _ = std::fs::create_dir_all(&d);
        if let Ok(j) = serde_json::to_string(s) {
            let _ = std::fs::write(d.join("brightness.json"), j);
        }
    }
}

/// Look applied right now, to skip redundant xprop calls.
static LOOK: Mutex<Option<u32>> = Mutex::new(None);
/// Raw value from before an idle dim, while one is active.
static IDLE: Mutex<Option<u32>> = Mutex::new(None);

fn set_look(gain: f64) {
    if std::env::var_os("DISPLAY").is_none() {
        return;
    }
    let key = (gain * 1000.0).round() as u32;
    let mut cur = LOOK.lock().unwrap();
    if *cur == Some(key) {
        return;
    }
    let ok = if key >= 1000 {
        let _ = Command::new("xprop").args(["-root", "-remove", ATOM]).output();
        true
    } else {
        let Some(home) = std::env::var_os("HOME") else { return };
        let p = PathBuf::from(home).join(".cache/rare");
        let _ = std::fs::create_dir_all(&p);
        let f = p.join(format!("night-{key}.cube"));
        let _ = std::fs::write(&f, cube(key as f64 / 1000.0));
        crate::host::env(Command::new("xprop").args(["-root", "-f", ATOM, "8s", "-set", ATOM, &f.to_string_lossy()])).output().map(|o| o.status.success()).unwrap_or(false)
    };
    if ok {
        *cur = Some(key);
    }
}

fn write_raw(raw: u32) -> Result<(), String> {
    let (dev, _, _) = crate::system::backlight().ok_or("No adjustable screen")?;
    crate::system::run(
        "busctl",
        &["call", "org.freedesktop.login1", "/org/freedesktop/login1/session/auto", "org.freedesktop.login1.Session", "SetBrightness", "ssu", "backlight", &dev, &raw.to_string()],
    )
    .map(|_| ())
}

/// Slider position now. A raw value that is not the one we wrote means a brightness key moved it.
pub fn current() -> Option<u8> {
    let (_, cur, max) = crate::system::backlight()?;
    let mut st = load();
    if IDLE.lock().unwrap().is_some() {
        return Some(st.pos.round() as u8);
    }
    if let Some(last) = st.last_raw {
        if last != cur {
            st.pos = pos_from_frac(cur as f64 / max.max(1) as f64, st.gamma);
            st.last_raw = Some(cur);
            save(&st);
            set_look(1.0);
        }
    } else if st.last_raw.is_none() {
        st.pos = pos_from_frac(cur as f64 / max.max(1) as f64, st.gamma).max(POS_HW);
    }
    Some(st.pos.round() as u8)
}

pub fn set(pos: u8) -> Result<(), String> {
    let (_, _, max) = crate::system::backlight().ok_or("No adjustable screen")?;
    let mut st = load();
    st.pos = pos.min(100) as f64;
    let (frac, gain) = plan(st.pos, st.gamma);
    let raw = (frac * max as f64).round() as u32;
    *IDLE.lock().unwrap() = None;
    write_raw(raw)?;
    set_look(gain);
    st.last_raw = Some(raw);
    save(&st);
    Ok(())
}

/// Reapply the saved position (look included) at launcher start; the backlight itself is
/// restored by systemd-backlight, but the look is lost on a gamescope restart.
pub fn apply_saved() {
    let st = load();
    if st.last_raw.is_some() {
        let _ = set(st.pos.round() as u8);
    }
}

/// Idle dim on or off. Never raises brightness, and restoring is the exact previous raw value.
pub fn idle(dim: bool) -> Result<(), String> {
    let st = load();
    let mut held = IDLE.lock().unwrap();
    if dim {
        if held.is_some() || !st.dim_idle {
            return Ok(());
        }
        let (_, cur, max) = crate::system::backlight().ok_or("No adjustable screen")?;
        let target = (plan(IDLE_POS, st.gamma).0 * max as f64).round() as u32;
        if target < cur {
            *held = Some(cur);
            drop(held);
            write_raw(target)?;
        }
        Ok(())
    } else if let Some(raw) = held.take() {
        drop(held);
        write_raw(raw)
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub pos: Option<u8>,
    pub dim_idle: bool,
}

pub fn brightness_info() -> Info {
    Info { pos: current(), dim_idle: load().dim_idle }
}

pub fn brightness_dim_idle(on: bool) {
    let mut st = load();
    st.dim_idle = on;
    save(&st);
    if !on {
        let _ = idle(false);
    }
}

pub fn brightness_idle(dim: bool) -> Result<(), String> {
    idle(dim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        assert_eq!(plan(100.0, 1.4), (1.0, 1.0));
        let (f, g) = plan(10.0, 1.4);
        assert!((f - RAW_FLOOR).abs() < 1e-9 && g == 1.0);
        let (f, g) = plan(0.0, 1.4);
        assert!((f - RAW_FLOOR).abs() < 1e-9 && (g - NIGHT_MIN_GAIN).abs() < 1e-9);
    }

    #[test]
    fn monotonic_and_even_low_end() {
        let mut prev = (0.0, 0.0);
        for p in 0..=100 {
            let (f, g) = plan(p as f64, 1.4);
            // Backlight and gain never go down as the slider goes up, and one of them moves each step.
            assert!(f >= prev.0 && g >= prev.1 - 1e-12, "pos {p}");
            assert!(f > prev.0 || g > prev.1 || p == 0, "stuck at {p}");
            prev = (f, g);
        }
        // Gamma above 1 makes the first steps above the floor smaller than the last ones.
        let lo = plan(20.0, 1.4).0 - plan(10.0, 1.4).0;
        let hi = plan(100.0, 1.4).0 - plan(90.0, 1.4).0;
        assert!(lo < hi);
    }

    #[test]
    fn night_zone_steps_are_equal_in_log_light() {
        let r1 = plan(5.0, 1.4).1 / plan(0.0, 1.4).1;
        let r2 = plan(10.0, 1.4).1 / plan(5.0, 1.4).1;
        assert!((r1 - r2).abs() < 1e-9);
    }

    #[test]
    fn inverse_round_trips() {
        for p in (10..=100).step_by(5) {
            let (f, _) = plan(p as f64, 1.4);
            assert!((pos_from_frac(f, 1.4) - p as f64).abs() < 0.01, "pos {p}");
        }
        assert_eq!(pos_from_frac(0.0, 1.4), POS_HW);
    }

    #[test]
    fn cube_scales_white() {
        let c = cube(0.5);
        assert!(c.contains("LUT_3D_SIZE 5"));
        let last = c.lines().last().unwrap();
        let v: f64 = last.split(' ').next().unwrap().parse().unwrap();
        assert!((v.powf(2.2) - 0.5).abs() < 0.01);
        assert_eq!(c.lines().count(), 2 + 125);
    }
}
