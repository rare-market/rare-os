//! Process facts read from /proc (Linux) or pgrep (elsewhere), shared by the library scan
//! (is Steam running?) and the runner (is this game still going?).

use std::process::Command;

/// (pid, comm, command line with spaces) of every process. Empty outside Linux.
#[cfg(target_os = "linux")]
pub fn cmdlines() -> Vec<(u32, String, String)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return out };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let cmd = std::fs::read(e.path().join("cmdline")).map(|b| String::from_utf8_lossy(&b).replace('\0', " ")).unwrap_or_default();
        let comm = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default().trim().to_string();
        out.push((pid, comm, cmd));
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn cmdlines() -> Vec<(u32, String, String)> {
    Vec::new()
}

/// Parent pid of every process.
#[cfg(target_os = "linux")]
pub fn parents() -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return out };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let stat = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
        // "pid (comm) S ppid ...": comm may hold spaces, so split after the last ')'.
        let ppid = stat.rsplit_once(')').and_then(|(_, r)| r.split_whitespace().nth(1)).and_then(|p| p.parse().ok()).unwrap_or(0);
        out.push((pid, ppid));
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub fn parents() -> Vec<(u32, u32)> {
    Vec::new()
}

pub fn steam_running() -> bool {
    #[cfg(target_os = "linux")]
    {
        cmdlines().iter().any(|(_, comm, _)| comm == "steam" || comm == "steamwebhelper")
    }
    #[cfg(not(target_os = "linux"))]
    {
        Command::new("pgrep").args(["-x", "steam_osx"]).output().map(|o| o.status.success()).unwrap_or(false)
    }
}

/// Is `bin` an executable file on PATH.
pub fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|p| p.join(bin).is_file()))
        .unwrap_or(false)
}
