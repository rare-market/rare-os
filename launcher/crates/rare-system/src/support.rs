//! Support (Settings > System > Support): the time zone and automatic time (timedated), logs for a
//! bug report (a .tar.gz in ~/Documents/Rare logs), and the factory reset, which the image does at
//! the next start (`os-admin factory-reset`, `rare-factory-reset.service`).

use std::path::{Path, PathBuf};

const ADMIN: &str = "/usr/libexec/rare/os-admin";

// ---------------------------------------------------------------------------- time

/// The regions a time zone is picked from, in the order shown. UTC is a zone of its own.
pub const REGIONS: [&str; 10] = ["Africa", "America", "Antarctica", "Arctic", "Asia", "Atlantic", "Australia", "Europe", "Indian", "Pacific"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeInfo {
    /// "America/New_York".
    pub zone: Option<String>,
    /// Automatic time (NTP) is on.
    pub ntp: Option<bool>,
}

pub fn time_info() -> TimeInfo {
    let out = crate::system::run("timedatectl", &["show", "-p", "Timezone", "-p", "NTP"]).unwrap_or_default();
    let mut t = TimeInfo::default();
    for l in out.lines() {
        match l.split_once('=') {
            Some(("Timezone", v)) if !v.trim().is_empty() => t.zone = Some(v.trim().to_string()),
            Some(("NTP", v)) => t.ntp = Some(v.trim() == "yes"),
            _ => {}
        }
    }
    if t.zone.is_none() {
        // No timedated: the /etc/localtime link names the zone.
        t.zone = std::fs::read_link("/etc/localtime").ok().and_then(|p| zone_from_path(&p));
    }
    t
}

fn zone_from_path(p: &Path) -> Option<String> {
    let s = p.to_string_lossy();
    let i = s.find("zoneinfo/")?;
    Some(s[i + "zoneinfo/".len()..].to_string()).filter(|z| !z.is_empty())
}

/// Every zone a person can pick: `timedatectl list-timezones`, else the zoneinfo files.
pub fn zones() -> Vec<String> {
    let listed = crate::system::run("timedatectl", &["list-timezones", "--no-pager"]).unwrap_or_default();
    let mut v: Vec<String> = listed.lines().map(|l| l.trim().to_string()).filter(|z| pickable(z)).collect();
    if v.is_empty() {
        let root = Path::new("/usr/share/zoneinfo");
        for r in REGIONS {
            walk_zones(&root.join(r), r, &mut v);
        }
        if root.join("UTC").is_file() {
            v.push("UTC".into());
        }
    }
    v.sort();
    v.dedup();
    v
}

fn walk_zones(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path();
        let z = format!("{prefix}/{name}");
        if p.is_dir() {
            walk_zones(&p, &z, out);
        } else if is_tzif(&p) && pickable(&z) {
            out.push(z);
        }
    }
}

fn is_tzif(p: &Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 4];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).is_ok() && &b == b"TZif"
}

/// A zone under one of `REGIONS`, or UTC (not the old aliases: US/Eastern, Etc/GMT+5, EST).
pub fn pickable(z: &str) -> bool {
    if z == "UTC" {
        return true;
    }
    let ok_chars = !z.is_empty() && z.chars().all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c)) && !z.contains("..");
    ok_chars && z.split_once('/').is_some_and(|(r, rest)| REGIONS.contains(&r) && !rest.is_empty())
}

/// The regions that have zones, in `REGIONS` order, then UTC: (name, number of zones).
pub fn regions(zones: &[String]) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = REGIONS.iter().map(|r| (r.to_string(), zones.iter().filter(|z| region_of(z) == *r).count())).filter(|(_, n)| *n > 0).collect();
    if zones.iter().any(|z| z == "UTC") {
        v.push(("UTC".into(), 1));
    }
    v
}

pub fn region_of(zone: &str) -> &str {
    zone.split_once('/').map(|(r, _)| r).unwrap_or(zone)
}

/// The zones most people pick, west to east: the time zone picker's first list, so a new device
/// on UTC is a few presses from the right zone instead of scrolling a region of 169 cities.
pub const COMMON: [&str; 39] = [
    "Pacific/Honolulu", "America/Anchorage", "America/Los_Angeles", "America/Phoenix", "America/Denver", "America/Chicago", "America/Mexico_City", "America/New_York",
    "America/Bogota", "America/Halifax", "America/Sao_Paulo", "America/Argentina/Buenos_Aires", "Europe/London", "Europe/Lisbon", "Africa/Lagos", "Europe/Paris",
    "Europe/Berlin", "Europe/Madrid", "Europe/Rome", "Europe/Warsaw", "Africa/Johannesburg", "Africa/Cairo", "Europe/Athens", "Europe/Istanbul", "Europe/Moscow",
    "Asia/Dubai", "Asia/Karachi", "Asia/Kolkata", "Asia/Bangkok", "Asia/Jakarta", "Asia/Singapore", "Asia/Manila", "Asia/Hong_Kong", "Asia/Shanghai", "Asia/Seoul",
    "Asia/Tokyo", "Australia/Perth", "Australia/Sydney", "Pacific/Auckland",
];

/// `COMMON` as far as this system has them, in that order.
pub fn common(zones: &[String]) -> Vec<String> {
    COMMON.iter().filter(|c| zones.iter().any(|z| z == *c)).map(|c| c.to_string()).collect()
}

/// The zones of one region, sorted by the name shown.
pub fn cities(zones: &[String], region: &str) -> Vec<String> {
    let mut v: Vec<String> = zones.iter().filter(|z| region_of(z) == region && z.contains('/')).cloned().collect();
    v.sort_by_key(|z| city_name(z).to_lowercase());
    v
}

/// "America/Argentina/Buenos_Aires" -> "Buenos Aires, Argentina"; "Europe/Berlin" -> "Berlin".
pub fn city_name(zone: &str) -> String {
    let parts: Vec<&str> = zone.split('/').collect();
    if parts.len() < 2 {
        return zone.to_string();
    }
    let nice = |s: &str| s.replace('_', " ");
    let city = nice(parts[parts.len() - 1]);
    if parts.len() > 2 {
        format!("{city}, {}", parts[1..parts.len() - 1].iter().map(|p| nice(p)).collect::<Vec<_>>().join(", "))
    } else {
        city
    }
}

/// Sets the zone (polkit lets the person at the device do it). The clock reads it at once.
pub fn set_zone(zone: &str) -> Result<(), String> {
    if !pickable(zone) {
        return Err("That is not a time zone".into());
    }
    crate::system::run("timedatectl", &["set-timezone", zone]).map_err(clean_polkit)?;
    reload_zone();
    Ok(())
}

pub fn set_ntp(on: bool) -> Result<(), String> {
    crate::system::run("timedatectl", &["set-ntp", if on { "true" } else { "false" }]).map(|_| ()).map_err(clean_polkit)
}

/// Makes this process read /etc/localtime again (glibc keeps the zone it read first).
pub fn reload_zone() {
    #[cfg(unix)]
    {
        extern "C" {
            fn tzset();
        }
        // SAFETY: tzset only re-reads the zone into libc's own state.
        unsafe { tzset() };
    }
}

fn clean_polkit(e: String) -> String {
    let low = e.to_lowercase();
    if low.contains("interactive authentication required") || low.contains("not authorized") || low.contains("access denied") {
        "This needs the next system update".into()
    } else {
        e
    }
}

/// Local date and time as "2026-10-27-1532" (file names).
pub fn stamp() -> String {
    #[cfg(unix)]
    {
        let t = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: localtime_r fills tm from t.
        unsafe { libc::localtime_r(&t, &mut tm) };
        format!("{:04}-{:02}-{:02}-{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min)
    }
    #[cfg(not(unix))]
    {
        "now".into()
    }
}

// ---------------------------------------------------------------------------- logs

/// ~/Documents/Rare logs
pub fn logs_dir() -> PathBuf {
    dirs::document_dir().unwrap_or_else(|| rare_core::paths::home().join("Documents")).join("Rare logs")
}

/// Config keys whose values never go in a bundle (matched in lower case, as a part of the key).
const SECRET_KEYS: [&str; 22] = ["key", "token", "secret", "password", "passwd", "pass", "pin", "seed", "mnemonic", "phrase", "private", "auth", "cookie", "session", "wallet", "account", "address", "credential", "signature", "sig", "jwt", "salt"];

/// Removes every value that could be a secret: keys named like one, and strings that look like a
/// key, a token or an address, wherever they are.
pub fn strip_secrets(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            for (k, val) in m.iter_mut() {
                let low = k.to_lowercase();
                if SECRET_KEYS.iter().any(|s| low.contains(s)) && !val.is_boolean() && !val.is_null() {
                    *val = Value::String("[removed]".into());
                } else {
                    strip_secrets(val);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(strip_secrets),
        Value::String(s) if looks_secret(s) => *s = "[removed]".into(),
        _ => {}
    }
}

/// A word that could be a key, a token or an address.
fn looks_secret(w: &str) -> bool {
    let w = w.trim_matches(|c: char| "\"',;()[]{}<>".contains(c));
    if w.starts_with("suiprivkey") || (w.starts_with("eyJ") && w.len() > 30) {
        return true;
    }
    let hex = w.strip_prefix("0x").unwrap_or(w);
    if hex.len() >= 32 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // Long runs of base64 / base58 with digits and both cases.
    w.len() >= 40 && w.chars().all(|c| c.is_ascii_alphanumeric() || "+/=_-".contains(c)) && w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_ascii_uppercase()) && w.chars().any(|c| c.is_ascii_lowercase())
}

/// Log text with anything that looks like a secret replaced: `token=..`, `password: ..`, keys,
/// long hex and base64 words.
pub fn scrub_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        let mut redact_next = false;
        let mut first = true;
        for word in line.split(' ') {
            if !first {
                out.push(' ');
            }
            first = false;
            let low = word.to_lowercase();
            let named = ["token", "password", "passwd", "secret", "mnemonic", "authorization", "cookie", "privkey", "private_key", "api_key", "apikey"].iter().any(|k| low.contains(k));
            if redact_next && !word.trim().is_empty() {
                out.push_str(if word.ends_with('\n') { "[removed]\n" } else { "[removed]" });
                redact_next = false;
                continue;
            }
            if named {
                if let Some(i) = word.find(['=', ':']) {
                    if i + 1 < word.trim_end().len() {
                        out.push_str(&word[..=i]);
                        out.push_str(if word.ends_with('\n') { "[removed]\n" } else { "[removed]" });
                        continue;
                    }
                    redact_next = true;
                }
            }
            if looks_secret(word.trim_end()) {
                out.push_str(if word.ends_with('\n') { "[removed]\n" } else { "[removed]" });
            } else {
                out.push_str(word);
            }
        }
    }
    out
}

/// The tail of a file, at most `max` bytes.
fn tail(p: &Path, max: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(p).ok()?;
    let len = f.metadata().ok()?.len();
    if len > max {
        f.seek(SeekFrom::Start(len - max)).ok()?;
    }
    let mut b = Vec::new();
    f.take(max).read_to_end(&mut b).ok()?;
    Some(String::from_utf8_lossy(&b).into_owned())
}

/// Every launcher log there is: the native launcher's (and its rotated ones), the preview's in
/// ~/rare-native, panics, the debug and journey logs in the runtime dir.
pub fn log_files() -> Vec<PathBuf> {
    let home = rare_core::paths::home();
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/state")).join("rare");
    let mut v = Vec::new();
    for dir in [state, home.join("rare-native")] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut l: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().contains(".log"))).collect();
            l.sort();
            v.extend(l);
        }
    }
    v.push(rare_core::paths::cache_dir().join("panic.log"));
    for n in ["rare-launcher-debug.log", "rare-launcher-panic.log", "rare-journey.log"] {
        if let Some(p) = rare_core::paths::runtime_file(n) {
            v.push(p);
        }
    }
    v.into_iter().filter(|p| p.is_file()).collect()
}

/// The launcher config with every secret taken out.
pub fn config_stripped() -> String {
    let Ok(b) = std::fs::read(rare_core::paths::config_file()) else { return "no launcher.json\n".into() };
    match serde_json::from_slice::<serde_json::Value>(&b) {
        Ok(mut v) => {
            strip_secrets(&mut v);
            serde_json::to_string_pretty(&v).unwrap_or_default()
        }
        Err(_) => "launcher.json could not be read\n".into(),
    }
}

fn cmd_text(cmd: &str, args: &[&str]) -> String {
    match std::process::Command::new(cmd).args(args).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            let e = String::from_utf8_lossy(&o.stderr);
            if !e.trim().is_empty() {
                s.push_str("\n--- stderr\n");
                s.push_str(&e);
            }
            s
        }
        Err(e) => format!("{cmd}: {e}\n"),
    }
}

/// Builds the bundle in ~/Documents/Rare logs and returns its path. `device` is the device info
/// as text. Nothing from wallet storage goes in, and the config and logs are scrubbed.
pub fn save_logs(device: &str) -> Result<PathBuf, String> {
    let dir = logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    let name = format!("rare-logs-{}", stamp());
    let work = rare_core::paths::cache_dir().join("logs-work").join(&name);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("launcher")).map_err(|e| e.to_string())?;
    let put = |file: &str, text: &str| std::fs::write(work.join(file), scrub_text(text));
    let mut seen = std::collections::HashSet::new();
    for p in log_files() {
        let mut n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if p.starts_with(rare_core::paths::home().join("rare-native")) {
            n = format!("preview-{n}");
        }
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(t) = tail(&p, 8 * 1024 * 1024) {
            let _ = put(&format!("launcher/{n}"), &t);
        }
    }
    let _ = put("journal-user.txt", &cmd_text("journalctl", &["--user", "-b", "--no-pager", "-n", "5000"]));
    let _ = put("journal-system-warnings.txt", &cmd_text("journalctl", &["-b", "-p", "warning", "--no-pager", "-n", "3000"]));
    let mut os = cmd_text("rpm-ostree", &["status"]);
    os.push_str("\n--- bootc status\n");
    os.push_str(&cmd_text("bootc", &["status"]));
    let _ = put("os-status.txt", &os);
    let mut dev = device.to_string();
    dev.push_str("\n--- uname -a\n");
    dev.push_str(&cmd_text("uname", &["-a"]));
    dev.push_str("\n--- timedatectl\n");
    dev.push_str(&cmd_text("timedatectl", &[]));
    dev.push_str("\n--- df -h\n");
    dev.push_str(&cmd_text("df", &["-h"]));
    let _ = put("device.txt", &dev);
    let _ = std::fs::write(work.join("launcher-config.json"), config_stripped());
    let out = dir.join(format!("{name}.tar.gz"));
    let parent = work.parent().unwrap_or(&work).to_path_buf();
    let r = crate::system::run("tar", &["-czf", &out.to_string_lossy(), "-C", &parent.to_string_lossy(), &name]);
    let _ = std::fs::remove_dir_all(&work);
    r.map_err(|e| format!("Couldn't pack the logs. {e}"))?;
    Ok(out)
}

/// Removable drives mounted under /run/media (USB sticks, SD cards).
pub fn usb_drives() -> Vec<PathBuf> {
    let m = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    m.lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .map(|p| p.replace("\\040", " "))
        .filter(|p| p.starts_with("/run/media/"))
        .map(PathBuf::from)
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| !m.permissions().readonly()))
        .collect()
}

/// Copies the bundle to the top of a drive; returns where it went.
pub fn copy_to(bundle: &Path, drive: &Path) -> Result<PathBuf, String> {
    if !drive.starts_with("/run/media/") {
        return Err("That is not a removable drive".into());
    }
    let to = drive.join(bundle.file_name().ok_or("No file")?);
    std::fs::copy(bundle, &to).map_err(|e| e.to_string())?;
    let _ = crate::system::run("sync", &[]);
    Ok(to)
}

// ---------------------------------------------------------------------------- factory reset

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    /// Rare's settings, accounts, wallets, friends, notices, Wi-Fi and Bluetooth go; games stay.
    KeepGames,
    /// All user data.
    EraseAll,
}

impl ResetMode {
    pub fn arg(self) -> &'static str {
        match self {
            ResetMode::KeepGames => "keep-games",
            ResetMode::EraseAll => "erase-all",
        }
    }
    pub fn parse(s: &str) -> Option<ResetMode> {
        match s {
            "keep-games" => Some(ResetMode::KeepGames),
            "erase-all" => Some(ResetMode::EraseAll),
            _ => None,
        }
    }
}

/// The installed system can do a reset (its os-admin has the action).
pub fn reset_supported() -> bool {
    std::fs::read_to_string(ADMIN).is_ok_and(|s| s.contains("factory-reset") && s.contains("--later"))
}

/// Books the reset for the next start (the caller restarts once the session's part is done).
pub fn factory_reset(mode: ResetMode) -> Result<(), String> {
    if !reset_supported() {
        return Err("Needs the next system update.".into());
    }
    crate::system::run("pkexec", &["--disable-internal-agent", ADMIN, "factory-reset", mode.arg(), "--later"]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_group_by_region() {
        let z: Vec<String> = ["America/New_York", "America/Argentina/Buenos_Aires", "Europe/Berlin", "UTC", "US/Eastern", "Etc/GMT+5", "EST"].iter().map(|s| s.to_string()).filter(|z| pickable(z)).collect();
        assert_eq!(z.len(), 4);
        assert_eq!(regions(&z), vec![("America".to_string(), 2), ("Europe".to_string(), 1), ("UTC".to_string(), 1)]);
        assert_eq!(cities(&z, "America"), vec!["America/Argentina/Buenos_Aires".to_string(), "America/New_York".to_string()]);
        assert_eq!(city_name("America/Argentina/Buenos_Aires"), "Buenos Aires, Argentina");
        assert_eq!(city_name("Europe/Berlin"), "Berlin");
        assert!(!pickable("Europe/../etc/passwd"));
    }

    #[test]
    fn secrets_are_stripped() {
        let mut v = serde_json::json!({
            "uiScale": "auto", "steamgriddbKey": "abc", "rumble": true,
            "net": {"sponsorToken": "t", "rpc": "https://fullnode.mainnet.sui.io"},
            "selected": "0x2b4c3d1f0e9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c",
            "gamePower": {}
        });
        strip_secrets(&mut v);
        assert_eq!(v["steamgriddbKey"], "[removed]");
        assert_eq!(v["net"]["sponsorToken"], "[removed]");
        assert_eq!(v["net"]["rpc"], "https://fullnode.mainnet.sui.io");
        assert_eq!(v["selected"], "[removed]");
        assert_eq!(v["rumble"], true);
        assert_eq!(v["uiScale"], "auto");
        let t = scrub_text("joined wifi password=hunter2 ok\nkey suiprivkey1qabc then token: abc123\nplain line\n");
        assert_eq!(t, "joined wifi password=[removed] ok\nkey [removed] then token: [removed]\nplain line\n");
    }
}
