//! Upscaling through gamescope's runtime root-window atoms (gamescope 3.17 as in the ogc build).
//!
//! Runtime (no restart, set with xprop on the gamescope X display, same as the FPS limit):
//!   GAMESCOPE_SCALING_FILTER  CARDINAL 32  0 linear, 1 nearest, 2 FSR, 3 NIS, 4 pixel
//!   GAMESCOPE_SHARPNESS       CARDINAL 32  0..20 (older alias GAMESCOPE_FSR_SHARPNESS); for FSR 0 is sharpest
//!   GAMESCOPE_XWAYLAND_MODE_CONTROL  CARDINAL 32 x4: server index, width, height, flags (render resolution)
//! Launch-only: -w/-h/-W/-H, -F/-S/--filter and the output size. The session owns those, so the
//! launcher cannot change them; it re-applies the saved atoms whenever a game starts instead.
//!
//! `Upscale` is the plain per-game record: the profile store can keep one per game and call `apply`.

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Off,
    Fsr,
    Nis,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", default)]
pub struct Upscale {
    pub mode: Mode,
    /// 0 (soft) to 20 (sharp).
    pub sharpness: u32,
    /// Render width and height; 0 x 0 is the native 1920x1200.
    pub width: u32,
    pub height: u32,
}

impl Default for Upscale {
    fn default() -> Self {
        Upscale { mode: Mode::Off, sharpness: 10, width: 0, height: 0 }
    }
}

const NATIVE: (u32, u32) = (1920, 1200);

fn filter_value(m: Mode) -> u32 {
    match m {
        Mode::Off => 0,
        Mode::Fsr => 2,
        Mode::Nis => 3,
    }
}

/// The (atom, values) pairs for one setting. Sharpness is inverted: gamescope reads 0 as sharpest.
pub fn atoms(u: &Upscale) -> Vec<(&'static str, Vec<u32>)> {
    atoms_for(u, NATIVE)
}

/// `atoms` with the size "native" means (the TV's chosen size when docked, screen.rs).
pub fn atoms_for(u: &Upscale, native: (u32, u32)) -> Vec<(&'static str, Vec<u32>)> {
    let (w, h) = if u.width == 0 || u.height == 0 { native } else { (u.width, u.height) };
    vec![
        ("GAMESCOPE_SCALING_FILTER", vec![filter_value(u.mode)]),
        ("GAMESCOPE_SHARPNESS", vec![20 - u.sharpness.min(20)]),
        ("GAMESCOPE_XWAYLAND_MODE_CONTROL", vec![0, w, h, 0]),
    ]
}

/// The argv for `xprop` that sets one atom.
pub fn xprop_args(atom: &str, vals: &[u32]) -> Vec<String> {
    let mut a: Vec<String> = ["-root", "-f", atom, "32c", "-set", atom].iter().map(|s| s.to_string()).collect();
    a.push(vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
    a
}

pub fn apply(u: &Upscale) -> Result<(), String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("Upscaling needs the gamescope session".into());
    }
    let mut errs = Vec::new();
    for (atom, vals) in atoms_for(u, crate::screen::native_size()) {
        let mut cmd = Command::new("xprop");
        crate::host::env(cmd.args(xprop_args(atom, &vals)));
        match cmd.output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => errs.push(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => errs.push(format!("xprop: {e}")),
        }
    }
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

/// The refresh rate atoms: the built-in screen's and an external one's (gamescope switches when
/// the screen has a mode at that rate). 0 removes them: the screen's own rate.
pub fn refresh_args(hz: u32) -> Vec<Vec<String>> {
    ["GAMESCOPE_DYNAMIC_REFRESH", "GAMESCOPE_DYNAMIC_REFRESH_EXTERNAL"]
        .iter()
        .map(|atom| if hz == 0 { vec!["-root".to_string(), "-remove".into(), atom.to_string()] } else { xprop_args(atom, &[hz]) })
        .collect()
}

pub fn set_refresh(hz: u32) -> Result<(), String> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("The refresh rate needs the gamescope session".into());
    }
    let mut errs = Vec::new();
    for args in refresh_args(hz) {
        let mut cmd = Command::new("xprop");
        crate::host::env(cmd.args(&args));
        match cmd.output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => errs.push(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => errs.push(format!("xprop: {e}")),
        }
    }
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

fn path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config/rare/upscale.json"))
}

pub fn saved() -> Upscale {
    path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Called when a game starts, so the filter and render size are in place for it.
pub fn apply_saved() {
    let u = saved();
    if u != Upscale::default() {
        let _ = apply(&u);
    }
}

pub fn upscale_get() -> Upscale {
    saved()
}

pub fn upscale_set(value: &Upscale) -> Result<(), String> {
    if let Some(p) = path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        std::fs::write(p, serde_json::to_string(value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    apply(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_atoms() {
        let u = Upscale { mode: Mode::Fsr, sharpness: 15, width: 1280, height: 800 };
        let a = atoms(&u);
        assert_eq!(a[0], ("GAMESCOPE_SCALING_FILTER", vec![2]));
        assert_eq!(a[1], ("GAMESCOPE_SHARPNESS", vec![5]));
        assert_eq!(a[2], ("GAMESCOPE_XWAYLAND_MODE_CONTROL", vec![0, 1280, 800, 0]));
    }

    #[test]
    fn off_native_defaults() {
        let a = atoms(&Upscale::default());
        assert_eq!(a[0].1, vec![0]);
        assert_eq!(a[2].1, vec![0, 1920, 1200, 0]);
        assert_eq!(atoms(&Upscale { mode: Mode::Nis, sharpness: 99, ..Default::default() })[1].1, vec![0]);
    }

    #[test]
    fn xprop_argv() {
        assert_eq!(xprop_args("X", &[0, 1280, 800, 0]).join(" "), "-root -f X 32c -set X 0, 1280, 800, 0");
    }

    #[test]
    fn refresh_atoms() {
        assert_eq!(refresh_args(40)[0].join(" "), "-root -f GAMESCOPE_DYNAMIC_REFRESH 32c -set GAMESCOPE_DYNAMIC_REFRESH 40");
        assert_eq!(refresh_args(0)[1].join(" "), "-root -remove GAMESCOPE_DYNAMIC_REFRESH_EXTERNAL");
    }

    #[test]
    fn serde_roundtrip() {
        let u: Upscale = serde_json::from_str(r#"{"mode":"nis","sharpness":4}"#).unwrap();
        assert_eq!(u.mode, Mode::Nis);
        assert_eq!(u.width, 0);
    }
}
