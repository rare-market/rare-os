//! Finding and running legendary, gogdl and umu-run.
//!
//! Rare ships them in `bin/` next to the launcher (the preview in `~/rare-native`) and in
//! `/usr/libexec/rare/stores` (the image). `add_to_path` appends those folders to the process's
//! PATH once at start, so rare-library's `which`, rare-launch and legendary's own `--wrapper umu-run`
//! all find them; a copy the system already has (umu-launcher's rpm) stays first.

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use std::time::{Duration, Instant};

pub const LEGENDARY: &str = "legendary";
pub const GOGDL: &str = "gogdl";
pub const UMU: &str = "umu-run";

/// The folders Rare's copies may be in, in order.
pub fn dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = std::env::var_os("RARE_STORES_BIN") {
        v.push(PathBuf::from(d));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        v.push(exe_dir.join("bin"));
    }
    v.push(PathBuf::from("/usr/libexec/rare/stores"));
    v
}

/// Appends the folders that exist to PATH. Call once, early, before other threads start.
pub fn add_to_path() {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let mut added = false;
    for d in dirs() {
        if d.is_dir() && !paths.contains(&d) {
            paths.push(d);
            added = true;
        }
    }
    if added {
        if let Ok(joined) = std::env::join_paths(paths) {
            // Called at start-up, before the launcher's threads read the environment.
            std::env::set_var("PATH", joined);
        }
    }
}

pub fn have(tool: &str) -> bool {
    rare_core::proc::which(tool)
}

/// What a run ended with.
#[derive(Debug)]
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    /// The last lines the tool logged (stderr), for the error message.
    pub log: Vec<String>,
}

impl Out {
    /// The tool's own last error line, cleaned of its logger prefix.
    pub fn error(&self) -> String {
        let line = self.log.iter().rev().find(|l| l.contains("ERROR") || l.contains("CRITICAL") || l.contains("Error")).or_else(|| self.log.last()).cloned().unwrap_or_default();
        clean(&line)
    }
}

/// "[cli] ERROR: Not enough space" to "Not enough space".
pub fn clean(line: &str) -> String {
    let s = line.trim();
    let s = match s.find("] ") {
        Some(i) if s.starts_with('[') => &s[i + 2..],
        _ => s,
    };
    for p in ["ERROR: ", "CRITICAL: ", "WARNING: ", "INFO: "] {
        if let Some(r) = s.strip_prefix(p) {
            return r.trim().to_string();
        }
    }
    s.to_string()
}

pub fn command(tool: &str, args: &[String]) -> Command {
    let mut c = Command::new(tool);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Python prints logs straight away, so progress arrives as it happens.
    c.env("PYTHONUNBUFFERED", "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: stopping a download stops legendary's worker processes too.
        c.process_group(0);
    }
    c
}

/// Runs `tool` to the end (or `timeout`), every stderr line to `line`. `started` gets the
/// process id (to stop it from another thread).
pub fn run_with(tool: &str, args: &[String], env: &[(String, String)], timeout: Option<Duration>, started: &mut dyn FnMut(u32), line: &mut dyn FnMut(&str)) -> Result<Out, String> {
    if !have(tool) {
        return Err(format!("{tool} is missing. Update Rare and try again."));
    }
    let mut c = command(tool, args);
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c.spawn().map_err(|e| format!("Couldn't start {tool}: {e}"))?;
    started(child.id());
    // Both streams line by line (legendary writes its file check to stdout with \r); stdout is
    // also kept whole for the JSON commands.
    let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
    let pipes: Vec<(bool, Box<dyn Read + Send>)> = [
        child.stdout.take().map(|o| (true, Box::new(o) as Box<dyn Read + Send>)),
        child.stderr.take().map(|e| (false, Box::new(e) as Box<dyn Read + Send>)),
    ]
    .into_iter()
    .flatten()
    .collect();
    for (is_out, pipe) in pipes {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(pipe);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match read_until_any(&mut r, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let s = String::from_utf8_lossy(&buf).trim_end().to_string();
                        if (is_out || !s.is_empty()) && tx.send((is_out, s)).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    drop(tx);
    let mut stdout = String::new();
    let t0 = Instant::now();
    let mut log: Vec<String> = Vec::new();
    let status = loop {
        let mut take = |(is_out, l): (bool, String)| {
            line(&l);
            if is_out {
                stdout.push_str(&l);
                stdout.push('\n');
            } else {
                log.push(l);
                if log.len() > 40 {
                    log.remove(0);
                }
            }
        };
        while let Ok(m) = rx.recv_timeout(Duration::from_millis(200)) {
            take(m);
        }
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            // The rest of the output after the exit (the readers end at EOF).
            while let Ok(m) = rx.recv_timeout(Duration::from_millis(1500)) {
                take(m);
            }
            break st;
        }
        if timeout.is_some_and(|t| t0.elapsed() > t) {
            stop(child.id());
            let _ = child.wait();
            return Err(format!("{tool} took too long. Try again."));
        }
    };
    Ok(Out { ok: status.success(), stdout, log })
}

fn read_until_any(r: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<usize> {
    let mut n = 0;
    loop {
        let avail = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if avail.is_empty() {
            return Ok(n);
        }
        if let Some(i) = avail.iter().position(|b| *b == b'\n' || *b == b'\r') {
            buf.extend_from_slice(&avail[..i]);
            r.consume(i + 1);
            return Ok(n + i + 1);
        }
        let len = avail.len();
        buf.extend_from_slice(avail);
        r.consume(len);
        n += len;
    }
}

/// Runs to the end and returns stdout; Err is the tool's error line.
pub fn run(tool: &str, args: &[&str], env: &[(String, String)], timeout: Duration) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let out = run_with(tool, &args, env, Some(timeout), &mut |_| {}, &mut |_| {})?;
    if out.ok {
        Ok(out.stdout)
    } else {
        let e = out.error();
        log::warn!("{tool} {}: {e}", args.first().map(String::as_str).unwrap_or(""));
        Err(if e.is_empty() { format!("{tool} stopped with an error") } else { e })
    }
}

/// Stops a process group: TERM, then KILL after three seconds.
pub fn stop(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    });
}

/// A child that is waited for on a thread of its own (fire and forget).
pub fn reap(mut c: Child) {
    std::thread::spawn(move || {
        let _ = c.wait();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn cleans_log_lines() {
        assert_eq!(super::clean("[cli] ERROR: Not enough free space"), "Not enough free space");
        assert_eq!(super::clean("[DLManager] INFO: = Progress"), "= Progress");
        assert_eq!(super::clean("plain"), "plain");
    }
}
