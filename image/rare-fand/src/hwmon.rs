//! sysfs: the ayaneo_ec fan and the temperature sensors. Everything is relative to a root
//! ("/" on the device, a temp dir in tests).
//!
//! ayaneo_ec (drivers/platform/x86/ayaneo-ec.c, OGC kernel 7.2 and mainline):
//! - pwm1_enable: write 1 = manual (EC register 0x4A = 0x01), 2 = automatic, the EC's own curve
//!   (0x4A = 0x00). Anything else is -EINVAL; reads give 1 or 2.
//! - pwm1: 0..=255, stored as `val * 100 / 255` percent in register 0x4B.
//! - fan1_input: rpm.
//! - Hibernation: the driver hands the fan to the EC on freeze and takes it back on the next
//!   pwm1 write. Suspend to idle has no hook, so the loop re-checks pwm1_enable every tick.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const FAN_HWMON: &str = "ayaneo_ec";
pub const PWM_MANUAL: &str = "1";
pub const PWM_AUTO: &str = "2";

fn read_trim(p: &Path) -> io::Result<String> {
    Ok(fs::read_to_string(p)?.trim().to_string())
}

/// hwmon directories under the root with this `name`.
pub fn find_hwmon(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root.join("sys/class/hwmon")) else { return out };
    for e in rd.flatten() {
        let dir = e.path();
        if read_trim(&dir.join("name")).ok().as_deref() == Some(name) {
            out.push(dir);
        }
    }
    out.sort();
    out
}

/// Percent to the pwm1 value that the driver stores back as exactly that percent
/// (it truncates `val * 100 / 255`).
pub fn pct_to_pwm(pct: u32) -> u32 {
    (pct.min(100) * 255).div_ceil(100)
}

pub fn pwm_to_pct(pwm: u32) -> u32 {
    pwm.min(255) * 100 / 255
}

pub struct Fan {
    pub dir: PathBuf,
    /// The duty last written while in manual mode.
    written: Option<u32>,
}

impl Fan {
    pub fn find(root: &Path) -> Option<Fan> {
        let dir = find_hwmon(root, FAN_HWMON).into_iter().find(|d| d.join("pwm1_enable").exists() && d.join("pwm1").exists())?;
        Some(Fan { dir, written: None })
    }

    pub fn exists(&self) -> bool {
        self.dir.join("pwm1_enable").exists()
    }

    fn write(&self, file: &str, val: &str) -> io::Result<()> {
        fs::write(self.dir.join(file), val)
    }

    /// pwm1_enable as the driver reports it: 1 manual, 2 automatic.
    pub fn enable(&self) -> io::Result<String> {
        read_trim(&self.dir.join("pwm1_enable"))
    }

    /// Hands the fan back to the EC's curve.
    pub fn release(&mut self) -> io::Result<()> {
        self.written = None;
        self.write("pwm1_enable", PWM_AUTO)
    }

    /// Manual mode at this duty. Re-takes manual control when something (a resume, the
    /// driver's hibernation hook, another tool) gave it back to the EC.
    pub fn set_duty(&mut self, pct: u32) -> io::Result<()> {
        if self.enable()? != PWM_MANUAL {
            self.write("pwm1_enable", PWM_MANUAL)?;
            self.written = None;
        }
        if self.written != Some(pct) {
            self.write("pwm1", &pct_to_pwm(pct).to_string())?;
            self.written = Some(pct);
        }
        Ok(())
    }

    pub fn rpm(&self) -> Option<u32> {
        read_trim(&self.dir.join("fan1_input")).ok()?.parse().ok()
    }

    /// The duty the EC register holds now, in percent.
    pub fn duty(&self) -> Option<u32> {
        read_trim(&self.dir.join("pwm1")).ok()?.parse().ok().map(pwm_to_pct)
    }
}

/// Releases the fan in whatever ayaneo_ec hwmon is under the root. For the unit's ExecStopPost
/// and the panic hook: no state, nothing that can fail first.
pub fn release_all(root: &Path) -> io::Result<usize> {
    let mut n = 0;
    let mut err = None;
    for d in find_hwmon(root, FAN_HWMON) {
        match fs::write(d.join("pwm1_enable"), PWM_AUTO) {
            Ok(()) => n += 1,
            Err(e) => err = Some(e),
        }
    }
    match err {
        Some(e) if n == 0 => Err(e),
        _ => Ok(n),
    }
}

/// The temperature the curves use: the amdgpu `edge` sensor, or k10temp's Tctl when there is
/// no edge sensor.
pub struct Temps {
    root: PathBuf,
    edge: Option<PathBuf>,
    tctl: Option<PathBuf>,
}

/// The `temp*_input` whose `temp*_label` is this label, in any hwmon with this name.
fn labelled(root: &Path, name: &str, label: &str) -> Option<PathBuf> {
    for dir in find_hwmon(root, name) {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut labels: Vec<PathBuf> =
            rd.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with("temp") && f.ends_with("_label"))).collect();
        labels.sort();
        for l in labels {
            if read_trim(&l).ok().as_deref() == Some(label) {
                let file = l.file_name().and_then(|f| f.to_str()).unwrap_or_default().replace("_label", "_input");
                let input = l.with_file_name(file);
                if input.exists() {
                    return Some(input);
                }
            }
        }
    }
    None
}

impl Temps {
    pub fn find(root: &Path) -> Temps {
        let mut t = Temps { root: root.to_path_buf(), edge: None, tctl: None };
        t.rescan();
        t
    }

    fn rescan(&mut self) {
        self.edge = labelled(&self.root, "amdgpu", "edge");
        self.tctl = labelled(&self.root, "k10temp", "Tctl");
    }

    fn read_c(p: &Path) -> Option<f64> {
        let milli: f64 = read_trim(p).ok()?.parse().ok()?;
        let c = milli / 1000.0;
        // A disconnected or broken sensor reads 0, -273 or garbage.
        (c > 0.0 && c < 150.0).then_some(c)
    }

    /// (celsius, sensor) or None when nothing could be read. Looks for the sensors again when
    /// they are missing (amdgpu can load after this service starts).
    pub fn read(&mut self) -> Option<(f64, &'static str)> {
        if self.edge.is_none() {
            self.rescan();
        }
        self.read_found().or_else(|| {
            // Paths can change when a driver reloads.
            self.rescan();
            self.read_found()
        })
    }

    fn read_found(&self) -> Option<(f64, &'static str)> {
        if let Some(c) = self.edge.as_deref().and_then(Self::read_c) {
            return Some((c, "edge"));
        }
        self.tctl.as_deref().and_then(Self::read_c).map(|c| (c, "tctl"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_round_trips_through_the_driver() {
        for pct in 0..=100 {
            let pwm = pct_to_pwm(pct);
            assert!(pwm <= 255);
            // What ayaneo_ec stores: val * 100 / 255, truncated.
            assert_eq!(pwm * 100 / 255, pct, "{pct}% -> {pwm}");
            assert_eq!(pwm_to_pct(pwm), pct);
        }
        assert_eq!(pct_to_pwm(100), 255);
        assert_eq!(pct_to_pwm(0), 0);
        assert_eq!(pct_to_pwm(150), 255);
    }
}
