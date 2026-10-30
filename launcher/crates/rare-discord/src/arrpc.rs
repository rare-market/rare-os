//! arRPC (OpenAsar, MIT): the open source Discord RPC server, run by Rare while Discord on the web
//! is open in Rare's window. Games with Discord's SDK talk to it on discord-ipc-N as they would to
//! Discord's app; it hands their activities to the page through its bridge (ws://127.0.0.1:1337,
//! `BRIDGE_JS` in the page). Its own process scanning is off: Rare reports what runs (lib.rs).
//!
//! Where it is: `<dir>/arrpc/src/index.js` with Node at `<dir>/bin/node` (or `node` on PATH);
//! `<dir>` is RARE_ARRPC_DIR, `arrpc` next to the launcher (dev installs), or the image's
//! /usr/libexec/rare/arrpc (tools/render/arrpc-tools.sh makes both).

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Runtime {
    pub node: PathBuf,
    pub script: PathBuf,
    /// Discord's detectable games list arRPC ships (name to application id).
    pub detectable: PathBuf,
}

fn runtime_in(dir: &Path) -> Option<Runtime> {
    let script = dir.join("arrpc/src/index.js");
    if !script.is_file() {
        return None;
    }
    let node = Some(dir.join("bin/node")).filter(|n| n.is_file()).or_else(|| std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("node")).find(|n| n.is_file())))?;
    Some(Runtime { node, detectable: dir.join("arrpc/src/process/detectable.json"), script })
}

pub fn dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = std::env::var_os("RARE_ARRPC_DIR") {
        v.push(PathBuf::from(d));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(exe_dir.join("arrpc"));
    }
    v.push("/usr/libexec/rare/arrpc".into());
    v
}

pub fn find() -> Option<Runtime> {
    dirs().iter().find_map(|d| runtime_in(d))
}

/// The socket path from arRPC's "listening at <path>" line (it colours its output).
pub fn listening_path(line: &str) -> Option<PathBuf> {
    let plain = strip_ansi(line);
    let rest = plain.split("listening at ").nth(1)?.trim();
    rest.contains("discord-ipc-").then(|| PathBuf::from(rest))
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

struct Proc {
    child: Child,
    started: Instant,
}

struct Sup {
    proc: Option<Proc>,
    socket: Option<PathBuf>,
    failures: u32,
    next_try: Option<Instant>,
}

static SUP: Mutex<Sup> = Mutex::new(Sup { proc: None, socket: None, failures: 0, next_try: None });

/// The socket arRPC listens on, once it said so.
pub fn socket() -> Option<PathBuf> {
    let s = SUP.lock().unwrap();
    s.proc.as_ref().and(s.socket.clone())
}

pub fn running() -> bool {
    SUP.lock().unwrap().proc.is_some()
}

fn stop(mut p: Proc) {
    eprintln!("[discord] stopping arRPC");
    let pid = p.child.id() as i32;
    // SAFETY: arRPC's own process group (started with setsid).
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(3) {
        if let Ok(Some(_)) = p.child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = p.child.wait();
}

fn start(rt: &Runtime, env: &dyn Fn(&mut Command)) -> Result<Proc, String> {
    let mut c = Command::new(&rt.node);
    c.arg(&rt.script);
    env(&mut c);
    c.env("ARRPC_NO_PROCESS_SCANNING", "1").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid and prctl only, between fork and exec.
        unsafe {
            c.pre_exec(|| {
                libc::setsid();
                // Gone with the launcher (the supervisor thread that starts it never ends).
                #[cfg(target_os = "linux")]
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    let mut child = c.spawn().map_err(|e| format!("arRPC: {e}"))?;
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(out).lines().map_while(Result::ok).take(2000) {
                if let Some(p) = listening_path(&l) {
                    eprintln!("[discord] arRPC listening at {}", p.display());
                    SUP.lock().unwrap().socket = Some(p);
                }
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(err).lines().map_while(Result::ok).take(200) {
                eprintln!("[discord] arRPC: {}", strip_ansi(&l));
            }
        });
    }
    eprintln!("[discord] started arRPC ({})", child.id());
    Ok(Proc { child, started: Instant::now() })
}

/// One supervisor step: arRPC runs while `want`, restarted with a backoff.
pub fn tick(want: bool, env: &dyn Fn(&mut Command)) {
    let mut s = SUP.lock().unwrap();
    if let Some(p) = s.proc.as_mut() {
        if let Ok(Some(st)) = p.child.try_wait() {
            let ran = p.started.elapsed();
            eprintln!("[discord] arRPC exited ({st}) after {} s", ran.as_secs());
            s.proc = None;
            s.socket = None;
            s.failures = if ran < Duration::from_secs(60) { s.failures + 1 } else { 1 };
            s.next_try = Some(Instant::now() + Duration::from_secs((2u64 << s.failures.min(5)).min(60)));
        }
    }
    if !want {
        if let Some(p) = s.proc.take() {
            // Its socket file stays behind when it is killed; a game would find nothing there.
            if let Some(sock) = s.socket.take() {
                let _ = std::fs::remove_file(sock);
            }
            s.failures = 0;
            s.next_try = None;
            drop(s);
            stop(p);
        }
        return;
    }
    if s.proc.is_some() || s.next_try.is_some_and(|t| Instant::now() < t) {
        return;
    }
    let Some(rt) = find() else { return };
    match start(&rt, env) {
        Ok(p) => s.proc = Some(p),
        Err(e) => {
            eprintln!("[discord] {e}");
            s.failures += 1;
            s.next_try = Some(Instant::now() + Duration::from_secs(30));
        }
    }
}

pub fn shutdown() {
    let (p, sock) = {
        let mut s = SUP.lock().unwrap();
        (s.proc.take(), s.socket.take())
    };
    if let Some(p) = p {
        stop(p);
        if let Some(sock) = sock {
            let _ = std::fs::remove_file(sock);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_listening_line() {
        let l = "[\u{1b}[38;2;88;101;242marRPC\u{1b}[0m > \u{1b}[38;2;254;231;92mipc\u{1b}[0m] listening at /run/user/1000/discord-ipc-0";
        assert_eq!(listening_path(l), Some(PathBuf::from("/run/user/1000/discord-ipc-0")));
        assert_eq!(listening_path("[arRPC > bridge] listening on 1337"), None);
        assert_eq!(strip_ansi("\u{1b}[1mx\u{1b}[0m"), "x");
    }

    #[test]
    fn finds_a_runtime_in_a_dir() {
        let d = std::env::temp_dir().join(format!("rare-arrpc-{}", std::process::id()));
        std::fs::create_dir_all(d.join("arrpc/src")).unwrap();
        std::fs::create_dir_all(d.join("bin")).unwrap();
        std::fs::write(d.join("arrpc/src/index.js"), "").unwrap();
        std::fs::write(d.join("bin/node"), "").unwrap();
        let rt = runtime_in(&d).unwrap();
        assert_eq!(rt.node, d.join("bin/node"));
        assert!(rt.detectable.ends_with("arrpc/src/process/detectable.json"));
        assert_eq!(runtime_in(&d.join("nope")), None);
        let _ = std::fs::remove_dir_all(d);
    }
}
