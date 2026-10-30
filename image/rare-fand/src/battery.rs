//! Battery care: hold the charge just under a limit (80 % unless set to 70 or 90; it charges again
//! 5 points under it) on AC with the EC's charge inhibit, a battery health log, and a charge cycle
//! count kept here for batteries that report none (the SuiPlay's cycle_count stays 0).
//! Everything is relative to a root ("/" on the device, a temp dir in tests).
//!
//! ayaneo_ec (ayaneo_ec_suiplay with charge_control=1 on the SuiPlay0X1) adds `charge_behaviour`
//! to BAT0 through a power_supply extension: reads `[auto] inhibit-charge`, takes `auto` (EC
//! register 0x1E = 0xAA) or `inhibit-charge` (0x55). There is no percentage threshold in the
//! EC, so the window is kept here.
//!
//! Always `auto` when care is off, off AC, under 70 %, above 45 C (if the battery reports a
//! temperature), on stop, on a panic and in ExecStopPost (`rare-fand release`).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Inhibit from here up (the default limit).
pub const HIGH: u8 = 80;
/// Charge again from here down (the limit minus 5).
pub const LOW: u8 = 75;
/// Under this, always charge (the limit minus 10: a stuck or slow inhibit never leaves the
/// battery low).
pub const FLOOR: u8 = 70;
/// Limits SetLimit takes.
pub const LIMITS: std::ops::RangeInclusive<u8> = 60..=95;
/// Hand back to the EC above this, until the battery is back at or under `COOL_C`.
pub const HOT_C: f64 = 45.0;
pub const COOL_C: f64 = 42.0;

pub const AUTO: &str = "auto";
pub const INHIBIT: &str = "inhibit-charge";
const DAY: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charge {
    Auto,
    Inhibit,
}

impl Charge {
    pub fn sysfs(self) -> &'static str {
        match self {
            Charge::Auto => AUTO,
            Charge::Inhibit => INHIBIT,
        }
    }
}

/// Why the charge is where it is; the bus's `State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Care {
    /// The setting is off (the EC charges to 100).
    Off,
    /// The driver has no charge_behaviour (module without charge_control=1, other devices).
    Unavailable,
    Unplugged,
    /// Under 70 %: charging whatever the window says.
    Low,
    Hot,
    /// No capacity reading.
    Unknown,
    /// On AC, under the window's top: charging.
    Charging,
    /// On AC, inhibited.
    Holding,
}

impl Care {
    pub fn name(self) -> &'static str {
        match self {
            Care::Off => "off",
            Care::Unavailable => "unavailable",
            Care::Unplugged => "unplugged",
            Care::Low => "low",
            Care::Hot => "hot",
            Care::Unknown => "unknown",
            Care::Charging => "charging",
            Care::Holding => "holding",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// Percent.
    pub capacity: Option<u8>,
    /// Any adapter online.
    pub ac: bool,
    /// Celsius, when the battery reports one.
    pub temp: Option<f64>,
}

/// The window under the limit with its safety exits. Pure; the daemon feeds it a reading every
/// few seconds.
#[derive(Debug)]
pub struct Window {
    inhibit: bool,
    hot: bool,
    /// The limit: inhibit from here up.
    pub high: u8,
}

impl Default for Window {
    fn default() -> Self {
        Window { inhibit: false, hot: false, high: HIGH }
    }
}

impl Window {
    pub fn with_limit(high: u8) -> Window {
        Window { high, ..Window::default() }
    }
    pub fn low(&self) -> u8 {
        self.high.saturating_sub(HIGH - LOW)
    }
    pub fn floor(&self) -> u8 {
        self.high.saturating_sub(HIGH - FLOOR)
    }

    pub fn step(&mut self, enabled: bool, r: &Reading) -> (Charge, Care) {
        self.hot = match r.temp {
            Some(t) if t > HOT_C => true,
            Some(t) if t <= COOL_C => false,
            Some(_) => self.hot,
            None => false,
        };
        let exit = if !enabled {
            Some(Care::Off)
        } else if !r.ac {
            Some(Care::Unplugged)
        } else if r.capacity.is_none() {
            Some(Care::Unknown)
        } else if r.capacity < Some(self.floor()) {
            Some(Care::Low)
        } else if self.hot {
            Some(Care::Hot)
        } else {
            None
        };
        if let Some(why) = exit {
            self.inhibit = false;
            return (Charge::Auto, why);
        }
        let cap = r.capacity.unwrap_or(0);
        if cap >= self.high {
            self.inhibit = true;
        } else if cap <= self.low() {
            self.inhibit = false;
        }
        if self.inhibit {
            (Charge::Inhibit, Care::Holding)
        } else {
            (Charge::Auto, Care::Charging)
        }
    }
}

fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn read_num(p: &Path) -> Option<i64> {
    read_trim(p)?.parse().ok()
}

/// The selected value of a sysfs choice list: `[auto] inhibit-charge` -> `auto`.
pub fn selected(list: &str) -> Option<&str> {
    let start = list.find('[')? + 1;
    let end = start + list[start..].find(']')?;
    Some(&list[start..end])
}

fn supplies(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(root.join("sys/class/power_supply"))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// System batteries (not a controller's: those have scope Device).
fn batteries(root: &Path) -> Vec<PathBuf> {
    supplies(root)
        .into_iter()
        .filter(|d| read_trim(&d.join("type")).as_deref() == Some("Battery") && read_trim(&d.join("scope")).as_deref() != Some("Device"))
        .collect()
}

/// An adapter (Mains, or USB-C/PD) reports online.
pub fn ac_online(root: &Path) -> bool {
    supplies(root).iter().any(|d| {
        let ty = read_trim(&d.join("type")).unwrap_or_default();
        (ty == "Mains" || ty.starts_with("USB")) && read_trim(&d.join("scope")).as_deref() != Some("Device") && read_num(&d.join("online")) == Some(1)
    })
}

/// Every battery's charge_behaviour back to auto (ExecStopPost, panics). Returns how many.
pub fn release_all(root: &Path) -> io::Result<usize> {
    let mut n = 0;
    let mut err = None;
    for d in batteries(root) {
        let f = d.join("charge_behaviour");
        if f.exists() {
            match fs::write(&f, AUTO) {
                Ok(()) => n += 1,
                Err(e) => err = Some(e),
            }
        }
    }
    match err {
        Some(e) => Err(e),
        None => Ok(n),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Health {
    /// The EC's learnt full capacity and the design capacity, in µWh (energy_*) or µAh (charge_*).
    pub full: u64,
    pub design: u64,
    pub energy: bool,
    pub cycles: Option<u64>,
}

impl Health {
    pub fn percent(&self) -> f64 {
        self.full as f64 * 100.0 / self.design as f64
    }

    /// One line of /var/lib/rare/battery.log.
    pub fn line(&self, at: SystemTime, reason: &str) -> String {
        let (f, d) = if self.energy { ("energy_full", "energy_full_design") } else { ("charge_full", "charge_full_design") };
        let cycles = self.cycles.map_or("-".to_string(), |c| c.to_string());
        format!("{} {f}={} {d}={} cycle_count={cycles} health={:.1} reason={reason}\n", iso_utc(at), self.full, self.design, self.percent())
    }
}

pub struct Battery {
    pub dir: PathBuf,
}

impl Battery {
    /// BAT0 if there is one, else the first system battery.
    pub fn find(root: &Path) -> Option<Battery> {
        let all = batteries(root);
        let dir = all.iter().find(|d| d.file_name().is_some_and(|n| n == "BAT0")).or(all.first())?.clone();
        Some(Battery { dir })
    }

    fn file(&self, f: &str) -> PathBuf {
        self.dir.join(f)
    }

    /// The driver offers auto and inhibit-charge here. The kernel always lists every choice; a
    /// plain file (tests) holds only the last value written.
    pub fn has_control(&self) -> bool {
        read_trim(&self.file("charge_behaviour"))
            .is_some_and(|s| s == AUTO || s.split_whitespace().any(|w| w.trim_matches(['[', ']']) == INHIBIT))
    }

    pub fn behaviour(&self) -> Option<Charge> {
        let s = read_trim(&self.file("charge_behaviour"))?;
        match selected(&s).unwrap_or(&s) {
            AUTO => Some(Charge::Auto),
            INHIBIT => Some(Charge::Inhibit),
            _ => None,
        }
    }

    pub fn set(&self, c: Charge) -> io::Result<()> {
        fs::write(self.file("charge_behaviour"), c.sysfs())
    }

    pub fn status(&self) -> Option<String> {
        read_trim(&self.file("status"))
    }

    pub fn reading(&self, root: &Path) -> Reading {
        Reading {
            capacity: read_num(&self.file("capacity")).filter(|c| (0..=100).contains(c)).map(|c| c as u8),
            ac: ac_online(root),
            // power_supply temp is in tenths of a degree.
            temp: read_num(&self.file("temp")).map(|t| t as f64 / 10.0).filter(|t| (-40.0..=120.0).contains(t)),
        }
    }

    /// (energy_now, energy_full), or the charge_* pair: what the cycle count needs.
    pub fn level(&self) -> (Option<u64>, Option<u64>) {
        let n = |f: &str| read_num(&self.file(f)).filter(|v| *v >= 0).map(|v| v as u64);
        match n("energy_now") {
            Some(e) => (Some(e), n("energy_full")),
            None => (n("charge_now"), n("charge_full")),
        }
    }

    pub fn health(&self) -> Option<Health> {
        let pick = |a: &str, b: &str| Some((read_num(&self.file(a))?, read_num(&self.file(b))?));
        let (energy, (full, design)) = match pick("energy_full", "energy_full_design") {
            Some(v) => (true, v),
            None => (false, pick("charge_full", "charge_full_design")?),
        };
        if full <= 0 || design <= 0 {
            return None;
        }
        let cycles = read_num(&self.file("cycle_count")).filter(|c| *c >= 0).map(|c| c as u64);
        Some(Health { full: full as u64, design: design as u64, energy, cycles })
    }
}

/// The health log: one line when a charge reaches Full, and at least one a day.
pub struct Log {
    pub path: PathBuf,
    next: Option<SystemTime>,
    was_full: Option<bool>,
}

impl Log {
    pub fn new(path: PathBuf) -> Log {
        Log { path, next: None, was_full: None }
    }

    /// Writes a line when one is due; returns the reason it wrote.
    pub fn tick(&mut self, now: SystemTime, status: Option<&str>, health: Option<Health>) -> Option<&'static str> {
        let next = *self.next.get_or_insert_with(|| {
            // The last line's time is the file's mtime; a clock that went back logs today.
            match fs::metadata(&self.path).and_then(|m| m.modified()) {
                Ok(t) if t <= now => t + DAY,
                _ => now,
            }
        });
        let full = status == Some("Full");
        let reached_full = full && self.was_full == Some(false);
        if status.is_some() {
            self.was_full = Some(full);
        }
        let reason = if reached_full {
            "full"
        } else if now >= next {
            "daily"
        } else {
            return None;
        };
        let h = health?;
        if let Err(e) = append(&self.path, &h.line(now, reason)) {
            eprintln!("rare-fand: {}: {e}", self.path.display());
            // Try again in an hour, not every tick.
            self.next = Some(now + Duration::from_secs(3600));
            return None;
        }
        self.next = Some(now + DAY);
        Some(reason)
    }
}

fn append(path: &Path, line: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
}

/// The care setting, kept in /var/lib/rare/battery-care ("on" or "off", then the limit when it
/// is not 80: "on 90"); off at 80 when missing.
pub fn load_care(path: &Path) -> (bool, u8) {
    let text = read_trim(path).unwrap_or_default();
    let mut w = text.split_whitespace();
    let on = w.next() == Some("on");
    let limit = w.next().and_then(|l| l.parse::<u8>().ok()).filter(|l| LIMITS.contains(l)).unwrap_or(HIGH);
    (on, limit)
}

pub fn save_care(path: &Path, on: bool, limit: u8) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let word = if on { "on" } else { "off" };
    let text = if limit == HIGH { format!("{word}\n") } else { format!("{word} {limit}\n") };
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// Charge cycles counted from what the battery gives out: every drop of energy_now (or
/// charge_now) while not charging adds up, and one full battery's worth (energy_full) is one
/// cycle. Kept in /var/lib/rare/battery-cycles as the total given out (µWh or µAh), saved at
/// most every 10 minutes and when the daemon stops.
pub struct Cycles {
    pub path: PathBuf,
    /// The total given out.
    pub used: u64,
    last: Option<u64>,
    saved: u64,
    saved_at: Option<SystemTime>,
}

impl Cycles {
    pub fn load(path: PathBuf) -> Cycles {
        let used = read_trim(&path).and_then(|t| t.split_whitespace().next()?.parse().ok()).unwrap_or(0);
        Cycles { path, used, last: None, saved: used, saved_at: None }
    }

    /// One reading: `now` is energy_now (or charge_now), `full` energy_full, `charging` from
    /// status. A drop bigger than half a battery between two readings (a swapped or recalibrated
    /// battery) is not counted.
    pub fn step(&mut self, now: Option<u64>, full: Option<u64>, charging: bool, at: SystemTime) {
        if let (Some(prev), Some(n)) = (self.last, now) {
            let drop = prev.saturating_sub(n);
            if !charging && drop > 0 && full.is_none_or(|f| drop <= f / 2) {
                self.used += drop;
            }
        }
        self.last = now;
        let due = self.saved_at.is_none_or(|t| at.duration_since(t).unwrap_or_default() >= Duration::from_secs(600));
        if self.used != self.saved && due {
            self.save(at);
        }
    }

    pub fn save(&mut self, at: SystemTime) {
        if self.used == self.saved {
            return;
        }
        let r = (|| {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            let tmp = self.path.with_extension("tmp");
            fs::write(&tmp, format!("{}\n", self.used))?;
            fs::rename(&tmp, &self.path)
        })();
        match r {
            Ok(()) => {
                self.saved = self.used;
                self.saved_at = Some(at);
            }
            Err(e) => {
                eprintln!("rare-fand: {}: {e}", self.path.display());
                self.saved_at = Some(at);
            }
        }
    }

    /// Cycles so far for a battery that holds `full` when full.
    pub fn count(&self, full: u64) -> Option<f64> {
        (full > 0).then(|| self.used as f64 / full as f64)
    }
}

/// `2026-10-26T14:03:07Z`.
pub fn iso_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(capacity: u8, ac: bool, temp: Option<f64>) -> Reading {
        Reading { capacity: Some(capacity), ac, temp }
    }

    /// Steps the window through capacities on AC at 30 C, returning what it asks for each time.
    fn run(w: &mut Window, caps: &[u8]) -> Vec<Charge> {
        caps.iter().map(|&c| w.step(true, &r(c, true, Some(30.0))).0).collect()
    }

    use Charge::{Auto as A, Inhibit as I};

    #[test]
    fn holds_between_75_and_80() {
        let mut w = Window::default();
        // Charging up from 72: auto until 80, inhibit from 80.
        assert_eq!(run(&mut w, &[72, 75, 78, 79, 80, 81]), [A, A, A, A, I, I]);
        // Draining under inhibit (no bypass): held until 75, then charge again.
        assert_eq!(run(&mut w, &[80, 79, 77, 76, 75]), [I, I, I, I, A]);
        // Back up through the band: still charging until 80.
        assert_eq!(run(&mut w, &[76, 77, 79, 80]), [A, A, A, I]);
        // Plugged in already above the top (100 %): inhibit right away, never charge.
        let mut w = Window::default();
        assert_eq!(run(&mut w, &[100, 95, 85, 80, 78, 75, 74]), [I, I, I, I, I, A, A]);
        assert_eq!(w.step(true, &r(78, true, None)), (A, Care::Charging));
        assert_eq!(w.step(true, &r(80, true, None)), (I, Care::Holding));
    }

    #[test]
    fn every_exit_goes_back_to_auto() {
        let mut w = Window::default();
        assert_eq!(w.step(true, &r(85, true, Some(30.0))), (I, Care::Holding));
        // Setting off.
        assert_eq!(w.step(false, &r(85, true, Some(30.0))), (A, Care::Off));
        assert_eq!(w.step(true, &r(85, true, Some(30.0))), (I, Care::Holding));
        // Unplugged: auto; plugged back in the band, it charges to 80 first (the hold reset).
        assert_eq!(w.step(true, &r(85, false, Some(30.0))), (A, Care::Unplugged));
        assert_eq!(w.step(true, &r(78, true, Some(30.0))), (A, Care::Charging));
        // Under 70 %: auto whatever happened before.
        assert_eq!(w.step(true, &r(82, true, Some(30.0))), (I, Care::Holding));
        assert_eq!(w.step(true, &r(69, true, Some(30.0))), (A, Care::Low));
        assert_eq!(w.step(true, &r(70, true, Some(30.0))), (A, Care::Charging));
        // No capacity: auto.
        assert_eq!(w.step(true, &Reading { capacity: None, ac: true, temp: None }), (A, Care::Unknown));
    }

    #[test]
    fn hot_battery_hands_back_until_cool() {
        let mut w = Window::default();
        assert_eq!(w.step(true, &r(85, true, Some(44.9))), (I, Care::Holding));
        assert_eq!(w.step(true, &r(85, true, Some(45.0))), (I, Care::Holding));
        assert_eq!(w.step(true, &r(85, true, Some(45.1))), (A, Care::Hot));
        // Between 42 and 45: still hot (no flapping at the edge).
        assert_eq!(w.step(true, &r(85, true, Some(44.0))), (A, Care::Hot));
        assert_eq!(w.step(true, &r(85, true, Some(42.1))), (A, Care::Hot));
        assert_eq!(w.step(true, &r(85, true, Some(42.0))), (I, Care::Holding));
        // No temperature at all: the window runs on capacity alone.
        assert_eq!(w.step(true, &r(85, true, Some(50.0))), (A, Care::Hot));
        assert_eq!(w.step(true, &r(85, true, None)), (I, Care::Holding));
    }

    #[test]
    fn other_limits_move_the_whole_window() {
        let mut w = Window { high: 90, ..Window::default() };
        assert_eq!((w.low(), w.floor()), (85, 80));
        assert_eq!(run(&mut w, &[84, 89, 90, 87, 85, 86]), [A, A, I, I, A, A]);
        assert_eq!(w.step(true, &r(79, true, Some(30.0))), (A, Care::Low));
        let mut w = Window { high: 70, ..Window::default() };
        assert_eq!(run(&mut w, &[72, 68, 65, 66]), [I, I, A, A]);
    }

    #[test]
    fn care_file_keeps_the_limit() {
        let t = std::env::temp_dir().join(format!("rare-fand-care-{}", std::process::id()));
        let f = t.join("battery-care");
        assert_eq!(load_care(&f), (false, HIGH));
        save_care(&f, true, HIGH).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "on\n");
        assert_eq!(load_care(&f), (true, 80));
        save_care(&f, true, 90).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "on 90\n");
        assert_eq!(load_care(&f), (true, 90));
        fs::write(&f, "on 12\n").unwrap();
        assert_eq!(load_care(&f), (true, 80));
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn cycles_add_up_what_the_battery_gives_out() {
        let t = std::env::temp_dir().join(format!("rare-fand-cycles-{}", std::process::id()));
        let f = t.join("battery-cycles");
        let t0 = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut c = Cycles::load(f.clone());
        let full = 45_000_000;
        // Draining 45 -> 30 Wh in steps, a charge back up (not counted), then 30 -> 15.
        for (i, e) in [45, 40, 35, 30].iter().enumerate() {
            c.step(Some(e * 1_000_000), Some(full), false, t0 + Duration::from_secs(i as u64 * 5));
        }
        for e in [35, 45] {
            c.step(Some(e * 1_000_000), Some(full), true, t0);
        }
        c.step(Some(30_000_000), Some(full), false, t0);
        assert_eq!(c.used, 30_000_000);
        assert!((c.count(full).unwrap() - 0.6667).abs() < 0.001);
        // A jump of more than half a battery is a new battery, not use.
        c.step(Some(1_000_000), Some(full), false, t0);
        assert_eq!(c.used, 30_000_000);
        // Saved on the first change, then no more than every 10 minutes, and on stop.
        assert_eq!(fs::read_to_string(&f).unwrap(), "5000000\n");
        c.save(t0);
        assert_eq!(Cycles::load(f.clone()).used, 30_000_000);
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn reads_choice_lists_and_dates() {
        assert_eq!(selected("[auto] inhibit-charge"), Some("auto"));
        assert_eq!(selected("auto [inhibit-charge]"), Some("inhibit-charge"));
        assert_eq!(selected("auto"), None);
        assert_eq!(iso_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(UNIX_EPOCH + Duration::from_secs(1_792_929_787)), "2026-10-25T12:03:07Z");
        assert_eq!(iso_utc(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00Z");
        let h = Health { full: 45_183_000, design: 49_087_000, energy: true, cycles: Some(12) };
        assert_eq!(
            h.line(UNIX_EPOCH + Duration::from_secs(951_782_400), "full"),
            "2000-02-29T00:00:00Z energy_full=45183000 energy_full_design=49087000 cycle_count=12 health=92.0 reason=full\n"
        );
    }
}
