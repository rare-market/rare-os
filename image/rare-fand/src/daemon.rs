//! The service's state: the fan, the sensors, the controller and the saved preset, plus battery
//! care on the same EC (battery.rs). The bus (bus.rs) and the main loop both go through this;
//! tests drive it against a fake sysfs.

use crate::battery::{self, Battery, Care, Charge, Cycles, Window};
use crate::control::{Controller, Mode, Target};
use crate::curve::{self, Preset};
use crate::hwmon::{Fan, Temps};
use crate::store;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub mode: Option<Mode>,
    /// Celsius, from `sensor`.
    pub temp: Option<f64>,
    pub sensor: &'static str,
    /// What pwm1 reads now, in percent.
    pub duty: Option<u32>,
    pub rpm: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryStatus {
    /// BAT0 has charge_behaviour (ayaneo_ec_suiplay loaded with charge_control=1).
    pub available: bool,
    /// The setting.
    pub care: bool,
    pub state: Care,
    /// energy_full / energy_full_design, percent.
    pub health: Option<f64>,
    /// Inhibit from here up, percent.
    pub limit: u8,
    /// Charge cycles counted here (battery.rs Cycles).
    pub cycles: Option<f64>,
}

impl Default for BatteryStatus {
    fn default() -> Self {
        BatteryStatus { available: false, care: false, state: Care::Unavailable, health: None, limit: battery::HIGH, cycles: None }
    }
}

#[derive(Debug, PartialEq)]
pub enum Stop {
    /// The ayaneo_ec hwmon went away (driver unloaded).
    FanGone,
}

pub struct Daemon {
    state_path: PathBuf,
    fan: Fan,
    temps: Temps,
    ctl: Controller,
    preset: Preset,
    /// What the fan was last set to; None forces a write on the next tick.
    applied: Option<Target>,
    last_error: Option<String>,
    pub status: Status,
    root: PathBuf,
    battery: Option<Battery>,
    care_path: PathBuf,
    care: bool,
    window: Window,
    /// What charge_behaviour was last set to; None forces a check on the next battery tick.
    charge_set: Option<Charge>,
    charge_error: Option<String>,
    log: battery::Log,
    cycles: Cycles,
    pub battery_status: BatteryStatus,
}

impl Daemon {
    /// None when this device has no ayaneo_ec fan.
    pub fn new(root: &Path, state_path: &Path) -> Option<Daemon> {
        let fan = Fan::find(root)?;
        let preset = store::load(state_path);
        eprintln!("rare-fand: fan at {}, preset {}", fan.dir.display(), preset.name());
        // Battery files sit next to the fan preset: /var/lib/rare/battery-care, battery.log.
        let care_path = state_path.with_file_name("battery-care");
        let (care, limit) = battery::load_care(&care_path);
        let battery = Battery::find(root);
        if let Some(b) = &battery {
            let ctl = if b.has_control() { "charge control" } else { "no charge control" };
            eprintln!("rare-fand: battery at {}, {ctl}, care {} at {limit} %", b.dir.display(), if care { "on" } else { "off" });
        }
        Some(Daemon {
            state_path: state_path.to_path_buf(),
            fan,
            temps: Temps::find(root),
            ctl: Controller::new(preset.curve().map(<[_]>::to_vec)),
            preset,
            applied: None,
            last_error: None,
            status: Status::default(),
            root: root.to_path_buf(),
            battery,
            care_path,
            care,
            window: Window::with_limit(limit),
            charge_set: None,
            charge_error: None,
            log: battery::Log::new(state_path.with_file_name("battery.log")),
            cycles: Cycles::load(state_path.with_file_name("battery-cycles")),
            battery_status: BatteryStatus { care, limit, ..BatteryStatus::default() },
        })
    }

    pub fn fan_dir(&self) -> &Path {
        &self.fan.dir
    }

    pub fn preset(&self) -> &Preset {
        &self.preset
    }

    /// Applies a preset from the next tick on and saves it. A custom curve is checked first.
    pub fn set_preset(&mut self, p: Preset) -> Result<(), String> {
        if let Preset::Custom(points) = &p {
            curve::validate(points)?;
        }
        if let Err(e) = store::save(&self.state_path, &p) {
            // Still applied; it just won't survive a reboot.
            eprintln!("rare-fand: saving {}: {e}", self.state_path.display());
        }
        eprintln!("rare-fand: preset {}", p.name());
        self.ctl.set_curve(p.curve().map(<[_]>::to_vec));
        self.preset = p;
        Ok(())
    }

    pub fn battery_dir(&self) -> Option<&Path> {
        self.battery.as_ref().map(|b| b.dir.as_path())
    }

    /// Battery care on or off, from the next battery tick on, and saved.
    pub fn set_care(&mut self, on: bool) {
        if let Err(e) = battery::save_care(&self.care_path, on, self.window.high) {
            eprintln!("rare-fand: saving {}: {e}", self.care_path.display());
        }
        eprintln!("rare-fand: battery care {}", if on { "on" } else { "off" });
        self.care = on;
        self.battery_status.care = on;
        // Turning it off writes auto once.
        self.charge_set = None;
    }

    /// The charge limit (battery::LIMITS), from the next battery tick on, and saved.
    pub fn set_limit(&mut self, limit: u8) -> Result<(), String> {
        if !battery::LIMITS.contains(&limit) {
            return Err(format!("limit {limit} % is outside {}..={} %", battery::LIMITS.start(), battery::LIMITS.end()));
        }
        if let Err(e) = battery::save_care(&self.care_path, self.care, limit) {
            eprintln!("rare-fand: saving {}: {e}", self.care_path.display());
        }
        eprintln!("rare-fand: charge limit {limit} %");
        self.window.high = limit;
        self.battery_status.limit = limit;
        Ok(())
    }

    /// Battery care, the health log and the cycle count (every few seconds; the battery changes slowly).
    pub fn battery_tick(&mut self, now: SystemTime) {
        let Some(bat) = &self.battery else { return };
        let available = bat.has_control();
        let reading = bat.reading(&self.root);
        let (want, mut state) = self.window.step(self.care && available, &reading);
        if !available {
            state = Care::Unavailable;
            self.charge_set = None;
        } else if self.care || self.charge_set != Some(Charge::Auto) {
            // On: keep the EC where the window wants it (it may reset 0x1E on resume or replug).
            // Off: auto once, then hands off, so a person testing by hand is not fought.
            let res = if bat.behaviour() == Some(want) { Ok(()) } else { bat.set(want) };
            match res {
                Ok(()) => {
                    if self.charge_set != Some(want) {
                        let cap = reading.capacity.map_or("?".into(), |c| c.to_string());
                        eprintln!("rare-fand: charge {} at {cap} % ({})", want.sysfs(), state.name());
                    }
                    self.charge_set = Some(want);
                    self.charge_error = None;
                }
                Err(e) => {
                    let msg = format!("writing charge_behaviour: {e}");
                    if self.charge_error.as_deref() != Some(&msg) {
                        eprintln!("rare-fand: {msg}");
                    }
                    self.charge_error = Some(msg);
                    let _ = bat.set(Charge::Auto);
                    self.charge_set = None;
                }
            }
        }
        let health = bat.health();
        let status = bat.status();
        if let Some(why) = self.log.tick(now, status.as_deref(), health) {
            eprintln!("rare-fand: battery health {:.1} % logged ({why})", health.map_or(0.0, |h| h.percent()));
        }
        let (level, full) = bat.level();
        self.cycles.step(level, full, matches!(status.as_deref(), Some("Charging" | "Full")), now);
        let cycles = full.and_then(|f| self.cycles.count(f));
        self.battery_status = BatteryStatus { available, care: self.care, state, health: health.map(|h| h.percent()), limit: self.window.high, cycles };
    }

    /// One control step (about once a second).
    pub fn tick(&mut self, now: Instant) -> Result<(), Stop> {
        if !self.fan.exists() {
            return Err(Stop::FanGone);
        }
        let reading = self.temps.read();
        let (target, mut mode) = self.ctl.step(reading.map(|r| r.0), now);
        let res = match target {
            // Hand over once; never fight the EC (or a person testing by hand) after that.
            Target::Ec if self.applied == Some(Target::Ec) => Ok(()),
            Target::Ec => self.fan.release(),
            Target::Duty(d) => self.fan.set_duty(d),
        };
        match res {
            Ok(()) => {
                self.applied = Some(target);
                self.last_error = None;
            }
            Err(e) => {
                let msg = format!("writing the fan: {e}; handing it back to the EC");
                if self.last_error.as_deref() != Some(&msg) {
                    eprintln!("rare-fand: {msg}");
                }
                self.last_error = Some(msg);
                let _ = self.fan.release();
                self.applied = None;
                mode = Mode::Failsafe;
            }
        }
        let status = Status {
            mode: Some(mode),
            temp: reading.map(|r| r.0),
            sensor: reading.map_or("", |r| r.1),
            duty: self.fan.duty(),
            rpm: self.fan.rpm(),
        };
        if status.mode != self.status.mode {
            let temp = status.temp.map_or("no temperature".into(), |t| format!("{t:.1} C {}", status.sensor));
            let duty = match target {
                Target::Duty(d) if mode != Mode::Failsafe => format!(", {d}%"),
                _ => String::new(),
            };
            eprintln!("rare-fand: {} ({}) at {temp}{duty}", mode.name(), self.preset.name());
        }
        self.status = status;
        Ok(())
    }

    /// The fan back to the EC: on stop, on exit and on errors.
    pub fn shutdown(&mut self) {
        match self.fan.release() {
            Ok(()) => eprintln!("rare-fand: fan handed back to the EC"),
            Err(e) => eprintln!("rare-fand: handing the fan back to the EC: {e}"),
        }
        self.applied = None;
        if let Some(bat) = self.battery.as_ref().filter(|b| b.has_control()) {
            match bat.set(Charge::Auto) {
                Ok(()) => eprintln!("rare-fand: charge back to auto"),
                Err(e) => eprintln!("rare-fand: charge back to auto: {e}"),
            }
        }
        self.charge_set = None;
        self.cycles.save(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battery::{self, Care};
    use crate::hwmon::{pct_to_pwm, release_all, FAN_HWMON};
    use std::fs;
    use std::time::Duration;

    /// A fake /sys/class/hwmon: ayaneo_ec, amdgpu (edge) and k10temp (Tctl).
    struct Sys {
        root: PathBuf,
        now: Instant,
    }

    impl Sys {
        fn new(name: &str) -> Sys {
            let root = std::env::temp_dir().join(format!("rare-fand-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let hw = root.join("sys/class/hwmon");
            for (dir, files) in [
                ("hwmon3", vec![("name", "nvme"), ("temp1_label", "Composite"), ("temp1_input", "40000")]),
                ("hwmon5", vec![("name", FAN_HWMON), ("pwm1_enable", "2"), ("pwm1", "102"), ("fan1_input", "2400")]),
                ("hwmon6", vec![("name", "amdgpu"), ("temp1_label", "edge"), ("temp1_input", "50000")]),
                ("hwmon7", vec![("name", "k10temp"), ("temp1_label", "Tctl"), ("temp1_input", "58000")]),
            ] {
                fs::create_dir_all(hw.join(dir)).unwrap();
                for (f, v) in files {
                    fs::write(hw.join(dir).join(f), format!("{v}\n")).unwrap();
                }
            }
            Sys { root, now: Instant::now() }
        }
        fn file(&self, path: &str) -> PathBuf {
            self.root.join("sys/class/hwmon").join(path)
        }
        fn get(&self, path: &str) -> String {
            fs::read_to_string(self.file(path)).unwrap().trim().to_string()
        }
        fn set(&self, path: &str, v: &str) {
            fs::write(self.file(path), v).unwrap();
        }
        fn edge(&self, c: u32) {
            self.set("hwmon6/temp1_input", &(c * 1000).to_string());
        }
        fn state(&self) -> PathBuf {
            self.root.join("var/lib/rare/fan")
        }
        fn daemon(&self) -> Daemon {
            Daemon::new(&self.root, &self.state()).expect("fan found")
        }
        fn ticks(&mut self, d: &mut Daemon, n: u32) {
            for _ in 0..n {
                self.now += Duration::from_secs(1);
                d.tick(self.now).unwrap();
            }
        }
    }

    impl Drop for Sys {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn default_hands_the_fan_to_the_ec_once() {
        let mut s = Sys::new("default");
        s.set("hwmon5/pwm1_enable", "1"); // left in manual by a crash
        let mut d = s.daemon();
        assert_eq!(d.preset(), &Preset::Default);
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
        assert_eq!(d.status.mode, Some(Mode::Ec));
        assert_eq!(d.status.rpm, Some(2400));
        assert_eq!(d.status.sensor, "edge");
        // Someone sets the fan by hand while on Default: left alone.
        s.set("hwmon5/pwm1_enable", "1");
        s.ticks(&mut d, 3);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
    }

    #[test]
    fn runs_a_preset_and_keeps_it() {
        let mut s = Sys::new("preset");
        let mut d = s.daemon();
        d.set_preset(Preset::Cool).unwrap();
        assert_eq!(fs::read_to_string(s.state()).unwrap(), "cool\n");
        s.edge(55);
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        assert_eq!(s.get("hwmon5/pwm1"), pct_to_pwm(56).to_string());
        assert_eq!(d.status.mode, Some(Mode::Curve));
        assert_eq!(d.status.duty, Some(56));

        // After a resume the EC has the fan again: taken back, duty rewritten.
        s.set("hwmon5/pwm1_enable", "2");
        s.set("hwmon5/pwm1", "0");
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        assert_eq!(s.get("hwmon5/pwm1"), pct_to_pwm(56).to_string());

        // A restart picks the saved preset up.
        drop(d);
        let d = s.daemon();
        assert_eq!(d.preset(), &Preset::Cool);

        // Custom curves are checked before anything is saved.
        let mut d = d;
        assert!(d.set_preset(Preset::Custom(vec![(60, 50), (50, 60)])).is_err());
        assert_eq!(d.preset(), &Preset::Cool);
        d.set_preset(Preset::Custom(vec![(40, 25), (80, 75)])).unwrap();
        s.ticks(&mut d, 1);
        // 55 C on 40->80 C / 25->75 %: 43.75 -> 44 %.
        assert_eq!(s.get("hwmon5/pwm1"), pct_to_pwm(44).to_string());
        assert_eq!(fs::read_to_string(s.state()).unwrap(), "custom\n40 25\n80 75\n");

        // Back to Default: the EC gets the fan.
        d.set_preset(Preset::Default).unwrap();
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
    }

    #[test]
    fn too_hot_runs_flat_out_on_any_preset() {
        let mut s = Sys::new("hot");
        let mut d = s.daemon();
        for p in [Preset::Quiet, Preset::Default] {
            d.set_preset(p).unwrap();
            s.edge(60);
            s.ticks(&mut d, 3);
            s.edge(91);
            s.ticks(&mut d, 1);
            assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
            assert_eq!(s.get("hwmon5/pwm1"), "255");
            assert_eq!(d.status.mode, Some(Mode::Emergency));
            s.edge(70);
            s.ticks(&mut d, 15);
            assert_ne!(d.status.mode, Some(Mode::Emergency));
        }
        // Default after the emergency: the EC has it again.
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
    }

    #[test]
    fn lost_sensors_fail_safe() {
        let mut s = Sys::new("sensors");
        let mut d = s.daemon();
        d.set_preset(Preset::Quiet).unwrap();
        s.edge(70);
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        // No edge sensor: Tctl (58 C) takes over.
        fs::remove_file(s.file("hwmon6/temp1_input")).unwrap();
        s.ticks(&mut d, 1);
        assert_eq!(d.status.sensor, "tctl");
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        // No sensors at all: back to the EC after a few seconds.
        fs::remove_file(s.file("hwmon7/temp1_input")).unwrap();
        s.ticks(&mut d, 2);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
        assert_eq!(d.status.mode, Some(Mode::Failsafe));
        // A reading that makes no sense counts as none.
        s.set("hwmon7/temp1_input", "-273000");
        s.ticks(&mut d, 2);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
        // Sensors back (amdgpu reloaded): the curve resumes.
        s.set("hwmon6/temp1_input", "70000");
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        assert_eq!(s.get("hwmon5/pwm1"), pct_to_pwm(50).to_string());
    }

    #[test]
    fn write_errors_and_stop_release_the_fan() {
        let mut s = Sys::new("errors");
        let mut d = s.daemon();
        d.set_preset(Preset::Cool).unwrap();
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        // pwm1 can't be written (an EC error): the fan goes back to the EC.
        fs::remove_file(s.file("hwmon5/pwm1")).unwrap();
        fs::create_dir(s.file("hwmon5/pwm1")).unwrap();
        s.edge(65);
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
        assert_eq!(d.status.mode, Some(Mode::Failsafe));
        // Fixed: next tick takes it again.
        fs::remove_dir(s.file("hwmon5/pwm1")).unwrap();
        s.ticks(&mut d, 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "1");
        // 50, 65, 65 C averaged: 60 C, 66 %.
        assert_eq!(s.get("hwmon5/pwm1"), pct_to_pwm(66).to_string());
        // Stop.
        d.shutdown();
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
        // ExecStopPost / panic path.
        s.set("hwmon5/pwm1_enable", "1");
        assert_eq!(release_all(&s.root).unwrap(), 1);
        assert_eq!(s.get("hwmon5/pwm1_enable"), "2");
        // Driver gone: the loop stops.
        fs::remove_dir_all(s.file("hwmon5")).unwrap();
        s.now += Duration::from_secs(1);
        assert_eq!(d.tick(s.now), Err(Stop::FanGone));
    }

    /// BAT0 (85 %, 30 C, charge_behaviour unless `control` is false) and an online adapter.
    fn battery(s: &Sys, control: bool) {
        let ps = s.root.join("sys/class/power_supply");
        for (dir, files) in [
            ("ADP1", vec![("type", "Mains"), ("online", "1")]),
            ("hid-controller-battery", vec![("type", "Battery"), ("scope", "Device"), ("capacity", "50")]),
            (
                "BAT0",
                vec![
                    ("type", "Battery"),
                    ("status", "Charging"),
                    ("capacity", "85"),
                    ("temp", "300"),
                    ("energy_full", "45183000"),
                    ("energy_full_design", "49087000"),
                    ("cycle_count", "12"),
                ],
            ),
        ] {
            fs::create_dir_all(ps.join(dir)).unwrap();
            for (f, v) in files {
                fs::write(ps.join(dir).join(f), format!("{v}\n")).unwrap();
            }
        }
        if control {
            fs::write(ps.join("BAT0/charge_behaviour"), "[auto] inhibit-charge\n").unwrap();
        }
    }

    fn bat(s: &Sys, f: &str) -> String {
        fs::read_to_string(s.root.join("sys/class/power_supply").join(f)).unwrap().trim().to_string()
    }

    fn set_bat(s: &Sys, f: &str, v: &str) {
        fs::write(s.root.join("sys/class/power_supply").join(f), v).unwrap();
    }

    #[test]
    fn battery_care_holds_the_window_and_always_falls_back_to_auto() {
        let s = Sys::new("care");
        battery(&s, true);
        let t0 = std::time::UNIX_EPOCH + Duration::from_secs(1_792_929_787);
        // Left inhibited by a crash, care off: auto once, then hands off.
        set_bat(&s, "BAT0/charge_behaviour", "inhibit-charge");
        let mut d = s.daemon();
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        assert_eq!(d.battery_status.state, Care::Off);
        assert!(d.battery_status.available);
        set_bat(&s, "BAT0/charge_behaviour", "inhibit-charge");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");

        // On at 85 %: held.
        d.set_care(true);
        assert_eq!(fs::read_to_string(s.root.join("var/lib/rare/battery-care")).unwrap(), "on\n");
        set_bat(&s, "BAT0/charge_behaviour", "[auto] inhibit-charge");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");
        assert_eq!(d.battery_status.state, Care::Holding);
        // The EC reset it (resume, replug): set again.
        set_bat(&s, "BAT0/charge_behaviour", "[auto] inhibit-charge");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");
        // Drained to 75: charge; to 80: hold.
        set_bat(&s, "BAT0/capacity", "75");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        set_bat(&s, "BAT0/capacity", "80");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");
        // Hot battery (46 C): auto.
        set_bat(&s, "BAT0/temp", "460");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        assert_eq!(d.battery_status.state, Care::Hot);
        set_bat(&s, "BAT0/temp", "300");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");
        // Unplugged: auto.
        set_bat(&s, "ADP1/online", "0");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        assert_eq!(d.battery_status.state, Care::Unplugged);
        set_bat(&s, "ADP1/online", "1");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");
        // Under 70: auto.
        set_bat(&s, "BAT0/capacity", "65");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        assert_eq!(d.battery_status.state, Care::Low);
        set_bat(&s, "BAT0/capacity", "90");
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");
        // Setting off: auto.
        d.set_care(false);
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        // Stop: auto.
        d.set_care(true);
        d.battery_tick(t0);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "inhibit-charge");
        d.shutdown();
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        // ExecStopPost / panic: auto, controller batteries untouched.
        set_bat(&s, "BAT0/charge_behaviour", "inhibit-charge");
        assert_eq!(battery::release_all(&s.root).unwrap(), 1);
        assert_eq!(bat(&s, "BAT0/charge_behaviour"), "auto");
        // A restart keeps the setting.
        drop(d);
        assert!(s.daemon().battery_status.care);
    }

    #[test]
    fn no_charge_control_touches_nothing() {
        let s = Sys::new("nocare");
        battery(&s, false);
        fs::create_dir_all(s.root.join("var/lib/rare")).unwrap();
        fs::write(s.root.join("var/lib/rare/battery-care"), "on\n").unwrap();
        let mut d = s.daemon();
        d.battery_tick(std::time::UNIX_EPOCH + Duration::from_secs(1_792_929_787));
        d.shutdown();
        assert!(!s.root.join("sys/class/power_supply/BAT0/charge_behaviour").exists());
        assert_eq!(d.battery_status.state, Care::Unavailable);
        assert_eq!(battery::release_all(&s.root).unwrap(), 0);
        // Health still reads.
        assert_eq!(d.battery_status.health.map(|h| (h * 10.0).round() / 10.0), Some(92.0));
    }

    #[test]
    fn health_log_once_a_day_and_on_full() {
        let s = Sys::new("healthlog");
        battery(&s, true);
        let log = s.root.join("var/lib/rare/battery.log");
        let lines = || fs::read_to_string(&log).map_or(0, |t| t.lines().count());
        let t0 = std::time::UNIX_EPOCH + Duration::from_secs(1_792_929_787);
        let mut d = s.daemon();
        // No log yet: one now.
        d.battery_tick(t0);
        assert_eq!(lines(), 1);
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            "2026-10-25T12:03:07Z energy_full=45183000 energy_full_design=49087000 cycle_count=12 health=92.0 reason=daily\n"
        );
        d.battery_tick(t0 + Duration::from_secs(5));
        assert_eq!(lines(), 1);
        // A charge reaches Full: one line, once.
        set_bat(&s, "BAT0/status", "Full");
        d.battery_tick(t0 + Duration::from_secs(10));
        d.battery_tick(t0 + Duration::from_secs(15));
        assert_eq!(lines(), 2);
        assert!(fs::read_to_string(&log).unwrap().ends_with("reason=full\n"));
        // A day later: daily.
        d.battery_tick(t0 + Duration::from_secs(10 + 24 * 3600));
        assert_eq!(lines(), 3);
        // A restart within the day (the log's mtime is now): nothing new until a day passes.
        drop(d);
        let mut d = s.daemon();
        let now = std::time::SystemTime::now();
        d.battery_tick(now);
        assert_eq!(lines(), 3);
        d.battery_tick(now + Duration::from_secs(24 * 3600 + 1));
        assert_eq!(lines(), 4);
    }

    #[test]
    fn other_devices_are_left_alone() {
        let s = Sys::new("nofan");
        fs::remove_dir_all(s.file("hwmon5")).unwrap();
        assert!(Daemon::new(&s.root, &s.state()).is_none());
        assert_eq!(release_all(&s.root).unwrap(), 0);
        assert!(!s.state().exists());
    }
}
