//! rare-pin-guard: the root side of Rare Wallet's PIN accounts (rules in
//! launcher/crates/rare-wallet/src/pin_guard/rules.rs, shared with the wallet's in-process
//! fallback).
//!
//! Listens on /run/rare-pin-guard.sock (one JSON request line, one answer line per connection,
//! one at a time). The caller's uid (SO_PEERCRED) picks its record,
//! /var/lib/rare-pin-guard/<uid>.cred, sealed by `systemd-creds encrypt` (host key + TPM2 where
//! there is one) and rewritten before any answer that changed it. The launcher's user can't read,
//! edit, delete or roll it back, and reinstalling the launcher leaves it alone, so five wrong PINs
//! stay five. PINs and keys never reach logs or argv: systemd-creds gets them on stdin.

#[path = "../../../launcher/crates/rare-wallet/src/pin_guard/rules.rs"]
mod rules;

use rules::{Guard, Record, Request, Store};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

const SOCKET: &str = "/run/rare-pin-guard.sock";
const STATE: &str = "/var/lib/rare-pin-guard";

struct Sealed {
    path: PathBuf,
    name: String,
}

impl Store for Sealed {
    fn load(&self) -> Result<Option<Record>, String> {
        if !self.path.exists() {
            return Ok(None);
        }
        let o = Command::new("systemd-creds").args(["decrypt", &format!("--name={}", self.name)]).arg(&self.path).arg("-").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output().map_err(|e| e.kind().to_string())?;
        let out = Zeroizing::new(o.stdout);
        if !o.status.success() {
            eprintln!("rare-pin-guard: a record did not open ({})", String::from_utf8_lossy(&o.stderr).trim().chars().take(160).collect::<String>());
            return Err("sealed".into());
        }
        serde_json::from_slice(&out).map(Some).map_err(|_| "damaged".to_string())
    }
    fn save(&self, r: &Record) -> Result<(), String> {
        let raw = Zeroizing::new(serde_json::to_vec(r).map_err(|_| "json".to_string())?);
        let tmp = self.path.with_extension("tmp");
        let mut child = Command::new("systemd-creds").args(["encrypt", &format!("--name={}", self.name), "-"]).arg(&tmp).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.kind().to_string())?;
        child.stdin.take().ok_or("stdin")?.write_all(&raw).map_err(|e| e.kind().to_string())?;
        let o = child.wait_with_output().map_err(|e| e.kind().to_string())?;
        if !o.status.success() {
            eprintln!("rare-pin-guard: sealing failed ({})", String::from_utf8_lossy(&o.stderr).trim().chars().take(160).collect::<String>());
            let _ = std::fs::remove_file(&tmp);
            return Err("seal".into());
        }
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        std::fs::rename(&tmp, &self.path).map_err(|e| e.kind().to_string())?;
        // the rename is on disk before the answer goes out
        if let Ok(d) = std::fs::File::open(STATE) {
            let _ = d.sync_all();
        }
        Ok(())
    }
    fn remove(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.kind().to_string()),
        }
    }
}

fn peer_uid(s: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len) };
    (r == 0).then_some(cred.uid)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn serve(guards: &mut HashMap<u32, Guard<Sealed>>, s: UnixStream) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let Some(uid) = peer_uid(&s) else { return };
    let mut line = Zeroizing::new(String::new());
    if BufReader::new((&s).take(64 * 1024)).read_line(&mut line).is_err() {
        return;
    }
    let answer = match serde_json::from_str::<Request>(&line) {
        Ok(req) => {
            let g = guards.entry(uid).or_insert_with(|| Guard::new(Sealed { path: PathBuf::from(STATE).join(format!("{uid}.cred")), name: format!("rare-pin-guard-{uid}") }));
            g.handle(req, now_ms())
        }
        Err(_) => serde_json::json!({ "ok": false, "code": "error", "message": "Bad request" }),
    };
    let out = Zeroizing::new(format!("{answer}\n"));
    let mut s = s;
    let _ = s.write_all(out.as_bytes());
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("check") {
        // ExecCondition: systemd-creds is there
        std::process::exit(if Command::new("systemd-creds").arg("--version").stdout(Stdio::null()).status().is_ok_and(|s| s.success()) { 0 } else { 1 });
    }
    std::fs::create_dir_all(STATE).expect("state dir");
    let _ = std::fs::set_permissions(STATE, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_file(SOCKET);
    let l = UnixListener::bind(SOCKET).expect("bind");
    std::fs::set_permissions(SOCKET, std::fs::Permissions::from_mode(0o666)).expect("socket mode");
    eprintln!("rare-pin-guard: listening");
    let mut guards: HashMap<u32, Guard<Sealed>> = HashMap::new();
    for s in l.incoming().flatten() {
        serve(&mut guards, s);
    }
}
