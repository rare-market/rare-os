//! Check files. A Steam game: Steam's own check (steam://validate/<appid>, which shows Steam's
//! dialog, so Steam gets the screen the way "Open in Steam" does), followed through Steam's
//! content_log ("Verifying", "Verified 809 MB", the download of what was bad, "Fully
//! Installed"). Any other installed game: Rare's own list of the game's files with their SHA-256
//! (~/.local/share/rare/filecheck), saved on the first check and compared on the next.
//!
//! Jobs run on threads; the page polls `status(id)` about once a second.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Waiting for Steam to start the check.
    Waiting,
    Checking,
    /// Steam is downloading what was missing or damaged.
    Fixing,
    Done,
    /// Done, and some files differ from the saved list (Keep as the good copy).
    Differs,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Status {
    pub phase: Phase,
    /// The line the page shows, in plain words.
    pub text: String,
}

impl Status {
    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Waiting | Phase::Checking | Phase::Fixing)
    }
}

fn jobs() -> std::sync::MutexGuard<'static, HashMap<String, Status>> {
    static J: std::sync::OnceLock<Mutex<HashMap<String, Status>>> = std::sync::OnceLock::new();
    J.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn set(id: &str, phase: Phase, text: impl Into<String>) {
    jobs().insert(id.to_string(), Status { phase, text: text.into() });
}

pub fn status(id: &str) -> Option<Status> {
    jobs().get(id).cloned()
}

pub fn forget(id: &str) {
    jobs().remove(id);
}

fn gb(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else {
        format!("{} MB", (b / 1e6).round().max(1.0) as u64)
    }
}

// ---------------------------------------------------------------- Steam

/// What Steam's content log says about one app's check, from the lines written since it began.
/// The lines (Steam client 1792618285, read on the SuiPlay 2026-10-28):
///   Start validating appID 1449850 on BuildID 24462996
///   AppID 1449850 App update changed : Running Update,Verifying Installed,
///   Validation: read 1 files missing and 0 files corrupt for AppID 1449850
///   AppID 1449850 update started : download 0/96, store 0/0, reuse 0/0, delta 0/0, stage 0/49
///   AppID 1449850 App update changed : Running Update,Downloading,Staging,
///   AppID 1449850 update canceled : Priority (Suspended)        (Steam checks its runtimes first)
///   AppID 1449850 scheduler finished : staying in schedule (result Suspended, state 0x42e)
///   AppID 1449850 finished update, 1 mounted depots (BuildID 24462996) : ...
///   AppID 1449850 scheduler finished : removed from schedule (result No Error, state 0xc)
pub fn steam_parse(appid: u32, text: &str, total: u64) -> Status {
    let tag = format!("AppID {appid} ");
    let start = format!("Start validating appID {appid} ");
    let read = format!(" for AppID {appid}");
    let mut seen = false;
    let mut checked: u64 = 0;
    let mut fixing = false;
    let mut download: u64 = 0;
    let mut bad: Option<(u64, u64)> = None;
    let mut done = false;
    let mut failed: Option<String> = None;
    let num = |s: &str| s.trim().split_whitespace().next().and_then(|n| n.parse::<u64>().ok());
    for line in text.lines() {
        let body = line.split_once("] ").map(|(_, b)| b).unwrap_or(line).trim_end();
        if body.starts_with(&start) {
            seen = true;
            continue;
        }
        if let Some(rest) = body.strip_prefix("Verified ") {
            // "Verified 809 MB in '<library>', clean bytes tally is now 952 MB": the check reads on.
            if seen && !fixing {
                checked += num(rest).unwrap_or(0) * 1_000_000;
            }
            continue;
        }
        // "Validation: read 1 files missing and 0 files corrupt for AppID 1449850"
        if let Some(rest) = body.strip_prefix("Validation: read ").and_then(|r| r.strip_suffix(read.as_str())) {
            let missing = num(rest).unwrap_or(0);
            let corrupt = rest.split(" and ").nth(1).and_then(num).unwrap_or(0);
            bad = Some((missing, corrupt));
            continue;
        }
        let Some(rest) = body.strip_prefix(&tag) else { continue };
        let lower = rest.to_lowercase();
        if lower.contains("verifying") || lower.contains("validating") {
            seen = true;
            failed = None;
        }
        if seen && (lower.contains("downloading") || lower.contains("staging") || lower.contains("committing")) {
            fixing = true;
            failed = None;
        }
        if let Some(d) = lower.split("update started : download ").nth(1) {
            download = download.max(d.split([',', ' ']).next().and_then(|x| x.split('/').nth(1)).and_then(|n| n.parse().ok()).unwrap_or(0));
        }
        if let Some(why) = lower.strip_prefix("update canceled : ").map(|_| rest["update canceled : ".len()..].trim().to_string()) {
            // Suspended means Steam checks something else first and comes back to it.
            if seen && !why.contains("Suspended") {
                failed = Some(why);
            }
        }
        if seen && lower.starts_with("finished update") {
            done = true;
        }
        if let Some(r) = rest.strip_prefix("scheduler finished : removed from schedule (result ") {
            if seen {
                let result = r.split(',').next().unwrap_or("").trim();
                if result == "No Error" {
                    done = true;
                } else if !done {
                    failed = Some(result.to_string());
                }
            }
        }
    }
    if let Some(why) = failed.filter(|_| !done) {
        return Status { phase: Phase::Failed, text: format!("Steam stopped the check: {why}") };
    }
    if done {
        let files = |n: u64, what: &str| format!("{n} {what} file{}", if n == 1 { "" } else { "s" });
        let text = match bad {
            Some((m, c)) if m + c > 0 => {
                let parts: Vec<String> = [(m, "missing"), (c, "damaged")].iter().filter(|(n, _)| *n > 0).map(|(n, w)| files(*n, w)).collect();
                format!("Steam replaced {}", parts.join(" and "))
            }
            Some(_) => "All files are fine".to_string(),
            None if fixing || download > 0 => "Steam replaced missing or damaged files".to_string(),
            None => "All files are fine".to_string(),
        };
        return Status { phase: Phase::Done, text };
    }
    if fixing {
        return Status { phase: Phase::Fixing, text: "Steam is replacing missing or damaged files".into() };
    }
    if seen {
        let text = if total > 0 && checked > 0 {
            format!("Checking files: {} of {}", gb(checked.min(total)), gb(total))
        } else {
            "Checking files".to_string()
        };
        return Status { phase: Phase::Checking, text };
    }
    Status { phase: Phase::Waiting, text: "Waiting for Steam to start the check".into() }
}

fn content_log() -> Option<PathBuf> {
    rare_launch::steamcfg::primary_root().map(|r| r.join("logs/content_log.txt"))
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Asks Steam to check one game and follows its log until it is done (two hours at most).
pub fn steam_start(ctx: &rare_core::Ctx, runner: &rare_launch::launch::Runner, id: &str, appid: u32, total: u64) -> Result<(), String> {
    if status(id).is_some_and(|s| s.busy()) {
        return Ok(());
    }
    let log = content_log().ok_or("Steam is not installed.")?;
    let mut from = file_len(&log);
    rare_launch::apps::steam(ctx, runner, &format!("steam://validate/{appid}")).map_err(|e| e.to_string())?;
    set(id, Phase::Waiting, "Waiting for Steam to start the check");
    let id = id.to_string();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let mut buf = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(800));
            let len = file_len(&log);
            if len < from {
                // Steam started and began a new log.
                from = 0;
                buf.clear();
            }
            if len > from {
                if let Ok(mut f) = std::fs::File::open(&log) {
                    use std::io::Seek;
                    let _ = f.seek(std::io::SeekFrom::Start(from));
                    let mut more = String::new();
                    let _ = f.take(len - from).read_to_string(&mut more);
                    buf.push_str(&more);
                }
                from = len;
            }
            let s = steam_parse(appid, &buf, total);
            let end = !s.busy();
            if end {
                log::info!("verify {id}: {}", s.text);
            }
            let waited_long = s.phase == Phase::Waiting && t0.elapsed() > Duration::from_secs(180);
            jobs().insert(id.clone(), if waited_long { Status { phase: Phase::Failed, text: "Steam didn't start the check. Try again, or use Open in Steam.".into() } } else { s });
            if end || waited_long || t0.elapsed() > Duration::from_secs(7200) {
                break;
            }
        }
    });
    Ok(())
}

// ---------------------------------------------------------------- Rare's own file list

#[derive(Serialize, Deserialize, Default)]
struct List {
    dir: String,
    /// Relative path -> (size, sha256 hex).
    files: BTreeMap<String, (u64, String)>,
}

fn list_path(id: &str) -> PathBuf {
    let name: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    rare_core::paths::data_dir().join("filecheck").join(format!("{name}.json"))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&e.path(), out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

fn sha256(p: &Path, hashed: &mut u64, total: u64, id: &str, verb: &str, last: &mut Instant) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        *hashed += n as u64;
        if last.elapsed() > Duration::from_millis(400) {
            *last = Instant::now();
            set(id, Phase::Checking, format!("{verb}: {} of {}", gb(*hashed), gb(total)));
        }
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The files in a folder with their sizes and hashes (`verb` says what the status line does).
fn read_list(id: &str, dir: &Path, verb: &str) -> List {
    let mut files = Vec::new();
    walk(dir, &mut files);
    let total: u64 = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    let mut hashed = 0;
    let mut last = Instant::now();
    let mut out = List { dir: dir.to_string_lossy().into_owned(), files: BTreeMap::new() };
    for f in files {
        let rel = f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().into_owned();
        let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        if let Some(h) = sha256(&f, &mut hashed, total, id, verb, &mut last) {
            out.files.insert(rel, (size, h));
        }
    }
    out
}

fn save_list(id: &str, l: &List) -> Result<(), String> {
    let p = list_path(id);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_vec(l).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn n_files(n: usize) -> String {
    if n == 1 {
        "1 file".into()
    } else {
        format!("{} files", crate::kit::format::grouped(n as f64, 0, 0))
    }
}

/// Compares a fresh list with the saved one: the line to show and whether they differ.
pub fn compare(saved: &BTreeMap<String, (u64, String)>, now: &BTreeMap<String, (u64, String)>) -> (bool, String) {
    let changed: Vec<&String> = saved.iter().filter(|(k, v)| now.get(*k).is_some_and(|n| n != *v)).map(|(k, _)| k).collect();
    let missing: Vec<&String> = saved.keys().filter(|k| !now.contains_key(*k)).collect();
    if changed.is_empty() && missing.is_empty() {
        return (false, format!("All {} match the saved list", n_files(saved.len())));
    }
    let mut parts = Vec::new();
    if !changed.is_empty() {
        parts.push(format!("{} changed", n_files(changed.len())));
    }
    if !missing.is_empty() {
        parts.push(format!("{} missing", n_files(missing.len())));
    }
    let names: Vec<&str> = missing.iter().chain(changed.iter()).take(2).map(|p| p.rsplit('/').next().unwrap_or(p)).collect();
    (true, format!("{} ({}). Reinstall or copy the game again to repair it", parts.join(", "), names.join(", ")))
}

/// Checks a game outside the stores against its saved list, or saves the list the first time.
/// `keep`: save the files as they are now as the good copy.
pub fn local_start(id: &str, dir: &str, keep: bool) -> Result<(), String> {
    if status(id).is_some_and(|s| s.busy()) {
        return Ok(());
    }
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        return Err("The game's folder isn't there any more.".into());
    }
    set(id, Phase::Checking, "Checking files");
    let id = id.to_string();
    std::thread::spawn(move || {
        let saved: Option<List> = if keep { None } else { std::fs::read(list_path(&id)).ok().and_then(|b| serde_json::from_slice(&b).ok()) };
        let verb = if saved.is_some() { "Checking files" } else { "Making a list of the files" };
        let now = read_list(&id, &dir, verb);
        let s = match saved {
            Some(saved) => {
                let (differs, text) = compare(&saved.files, &now.files);
                Status { phase: if differs { Phase::Differs } else { Phase::Done }, text }
            }
            None => match save_list(&id, &now) {
                Ok(()) if keep => Status { phase: Phase::Done, text: format!("Saved {} as the good copy", n_files(now.files.len())) },
                Ok(()) => Status { phase: Phase::Done, text: format!("Saved a list of {}. The next check compares against it", n_files(now.files.len())) },
                Err(e) => Status { phase: Phase::Failed, text: format!("Couldn't save the file list: {e}") },
            },
        };
        log::info!("verify {id}: {}", s.text);
        jobs().insert(id, s);
    });
    Ok(())
}

/// Fixture runs (the browser-mode backend): a check that takes a few seconds.
pub fn mock_start(id: &str, steam: bool) {
    set(id, Phase::Checking, "Checking files");
    let id = id.to_string();
    std::thread::spawn(move || {
        for i in 1..=4u64 {
            std::thread::sleep(Duration::from_millis(700));
            set(&id, Phase::Checking, format!("Checking files: {} of 9.8 GB", gb(i * 2_400_000_000)));
        }
        set(&id, Phase::Done, if steam { "All files are fine" } else { "Saved a list of 1,204 files. The next check compares against it" });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steam's log of a real check on the SuiPlay (2026-10-28): masterduel_Data/app.info deleted
    /// first; Steam suspends the game to check two runtimes, comes back and puts the file back.
    const REAL: &str = include_str!("testdata/validate-1449850.log");

    #[test]
    fn a_real_check_reads_as_plain_words() {
        let lines: Vec<&str> = REAL.lines().collect();
        let upto = |needle: &str| lines.iter().position(|l| l.contains(needle)).map(|i| lines[..=i].join("\n")).unwrap();
        assert_eq!(steam_parse(1449850, "", 0).phase, Phase::Waiting);
        assert_eq!(steam_parse(1449850, &upto("Running Update,Verifying Installed"), 0), Status { phase: Phase::Checking, text: "Checking files".into() });
        assert_eq!(steam_parse(1449850, &upto("Downloading,Staging"), 0).phase, Phase::Fixing);
        // Suspended for the runtimes' checks: not a failure, not done.
        let paused = upto("staying in schedule");
        assert_eq!(steam_parse(1449850, &paused, 0).phase, Phase::Fixing);
        assert_eq!(steam_parse(1449850, REAL, 0), Status { phase: Phase::Done, text: "Steam replaced 1 missing file".into() });
        // The runtime Steam checked on the way has its own lines and result.
        assert_eq!(steam_parse(4628710, REAL, 0), Status { phase: Phase::Done, text: "All files are fine".into() });
        assert_eq!(steam_parse(10, REAL, 0).phase, Phase::Waiting);
    }

    /// Hades II on the SuiPlay (2026-10-28), started from the game page: one voice-line file
    /// deleted, a 27 s scan of 11.77 GB, suspended once for Proton 11, then put back.
    #[test]
    fn a_real_check_from_the_game_page() {
        let log = include_str!("testdata/validate-1145350.log");
        let lines: Vec<&str> = log.lines().collect();
        let upto = |needle: &str| lines.iter().position(|l| l.contains(needle)).map(|i| lines[..=i].join("\n")).unwrap();
        assert_eq!(steam_parse(1145350, &upto("full scan"), 11_770_000_000).text, "Checking files");
        assert_eq!(steam_parse(1145350, &upto("staying in schedule"), 0).phase, Phase::Fixing);
        assert_eq!(steam_parse(1145350, log, 0), Status { phase: Phase::Done, text: "Steam replaced 1 missing file".into() });
    }

    #[test]
    fn clean_checks_progress_and_failures() {
        let ok = "[t] Start validating appID 7 on BuildID 1\n[t] AppID 7 App update changed : Running Update,Verifying Installed,\n[t] Verified 809 MB in '/x', clean bytes tally is now 952 MB\n";
        assert_eq!(steam_parse(7, ok, 9_800_000_000).text, "Checking files: 809 MB of 9.8 GB");
        let done = format!("{ok}[t] Validation: read 0 files missing and 0 files corrupt for AppID 7\n[t] AppID 7 App update changed : None\n[t] AppID 7 scheduler finished : removed from schedule (result No Error, state 0xc)\n");
        assert_eq!(steam_parse(7, &done, 0), Status { phase: Phase::Done, text: "All files are fine".into() });
        let two = done.replace("read 0 files missing and 0 files corrupt", "read 0 files missing and 2 files corrupt");
        assert_eq!(steam_parse(7, &two, 0).text, "Steam replaced 2 damaged files");
        let failed = format!("{ok}[t] AppID 7 update canceled : Disk write failure\n[t] AppID 7 scheduler finished : removed from schedule (result Disk write failure, state 0x6)\n");
        assert_eq!(steam_parse(7, &failed, 0), Status { phase: Phase::Failed, text: "Steam stopped the check: Disk write failure".into() });
    }

    #[test]
    fn file_lists_compare() {
        let mut a = BTreeMap::new();
        a.insert("Game.exe".to_string(), (10, "aa".to_string()));
        a.insert("data/level1.pak".to_string(), (20, "bb".to_string()));
        assert_eq!(compare(&a, &a), (false, "All 2 files match the saved list".into()));
        let mut b = a.clone();
        b.remove("data/level1.pak");
        b.insert("Game.exe".to_string(), (10, "cc".to_string()));
        b.insert("new.txt".to_string(), (1, "dd".to_string()));
        let (differs, text) = compare(&a, &b);
        assert!(differs);
        assert_eq!(text, "1 file changed, 1 file missing (level1.pak, Game.exe). Reinstall or copy the game again to repair it");
    }

    #[test]
    fn a_local_check_saves_then_compares() {
        let d = tempfile_dir();
        std::fs::write(d.join("a.bin"), b"hello").unwrap();
        let mut last = Instant::now();
        let mut n = 0;
        assert_eq!(sha256(&d.join("a.bin"), &mut n, 5, "t", "x", &mut last).unwrap(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        let l = read_list("t", &d, "x");
        assert_eq!(l.files.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rare-filecheck-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
