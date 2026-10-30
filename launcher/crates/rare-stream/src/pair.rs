//! A pairing in progress: Rare picks the PIN and shows it big, `moonlight pair <host> --pin`
//! runs with no window, and the PC is paired once Moonlight has saved its certificate (read back
//! from Moonlight.conf). `next` decides the phase from what was seen (pure, tested).

use serde::Serialize;
use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the PIN stays up before Rare gives up.
pub const TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Paired,
    Failed(String),
}

/// The phase after one look: the certificate in Moonlight.conf wins; then a failure Moonlight
/// logged; then Moonlight having exited; then the timeout.
pub fn next(paired: bool, exited: bool, elapsed: Duration, log: &str) -> Phase {
    if paired {
        return Phase::Paired;
    }
    let l = log.to_lowercase();
    if l.contains("didn't match") || l.contains("incorrect pin") || l.contains("pin was incorrect") {
        return Phase::Failed("The PIN didn't match. Try again.".into());
    }
    if l.contains("previous session is still running") {
        return Phase::Failed("A game is still streaming from your PC. Quit it on the PC, then try again.".into());
    }
    if l.contains("failed to connect") {
        return Phase::Failed("Rare couldn't reach your PC. Check it's on and on the same network.".into());
    }
    if exited {
        return Phase::Failed("Pairing didn't finish. Try again.".into());
    }
    if elapsed >= TIMEOUT {
        return Phase::Failed("Your PC didn't get the PIN in time. Try again.".into());
    }
    Phase::Waiting
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairView {
    pub uuid: String,
    pub name: String,
    pub address: String,
    pub pin: String,
    /// "waiting", "paired" or "failed".
    pub phase: String,
    pub error: Option<String>,
    pub software: String,
}

struct Session {
    view: PairView,
    child: Option<Child>,
    started: Instant,
    log: Arc<Mutex<String>>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// A random 4 digit PIN.
pub fn new_pin() -> String {
    let mut b = [0u8; 2];
    let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_ok();
    let n = if ok { u16::from_le_bytes(b) } else { (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(4242) % 10_000) as u16 };
    super::cli::pin_from(n)
}

/// Ends Moonlight gently (SIGTERM, so it saves), then for sure.
pub fn stop_child(mut c: Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(c.id() as i32, libc::SIGTERM);
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(3) {
        if matches!(c.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = c.kill();
    let _ = c.wait();
}

/// Starts pairing with `info`'s PC at `address`. Already paired in Moonlight: done at once.
pub fn start(h: &dyn super::StreamHost, m: &super::cli::Moonlight, address: &str, info: &super::discover::ServerInfo) -> Result<PairView, String> {
    cancel();
    let pin = new_pin();
    let mut view = PairView { uuid: info.uuid.clone(), name: info.name.clone(), address: address.to_string(), pin: pin.clone(), phase: "waiting".into(), error: None, software: info.software.clone() };
    if paired_now(&info.uuid) {
        view.phase = "paired".into();
        return Ok(view);
    }
    let (prog, args, env) = m.command(&super::cli::pair_args(address, &pin), true);
    let mut cmd = std::process::Command::new(&prog);
    cmd.args(&args);
    h.host_env(&mut cmd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Could not start Moonlight: {e}"))?;
    let log = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let log = log.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut l = log.lock().unwrap_or_else(|e| e.into_inner());
                l.push_str(&String::from_utf8_lossy(&buf[..n]));
                // Keep the tail only.
                if l.len() > 16_384 {
                    let cut = l.len() - 8192;
                    let cut = (cut..l.len()).find(|i| l.is_char_boundary(*i)).unwrap_or(l.len());
                    l.drain(..cut);
                }
            }
        });
    }
    *lock() = Some(Session { view: view.clone(), child: Some(child), started: Instant::now(), log });
    let watch_pin = pin;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(800));
        if !tick(&watch_pin) {
            break;
        }
    });
    Ok(view)
}

fn paired_now(uuid: &str) -> bool {
    super::conf::load().iter().any(|k| k.uuid == uuid && k.paired)
}

/// One look at the session with this PIN. False once it is over.
fn tick(pin: &str) -> bool {
    let mut g = lock();
    let Some(s) = g.as_mut().filter(|s| s.view.pin == pin) else { return false };
    if s.view.phase != "waiting" {
        return false;
    }
    let exited = s.child.as_mut().map(|c| matches!(c.try_wait(), Ok(Some(_)))).unwrap_or(true);
    let log = s.log.lock().map(|l| l.clone()).unwrap_or_default();
    let phase = next(paired_now(&s.view.uuid), exited, s.started.elapsed(), &log);
    match phase {
        Phase::Waiting => true,
        Phase::Paired | Phase::Failed(_) => {
            if let Phase::Failed(e) = &phase {
                s.view.error = Some(e.clone());
                s.view.phase = "failed".into();
            } else {
                s.view.phase = "paired".into();
            }
            // Moonlight waits on its own "paired" or error dialog: end it.
            if let Some(c) = s.child.take() {
                std::thread::spawn(move || stop_child(c));
            }
            false
        }
    }
}

pub fn poll() -> Option<PairView> {
    lock().as_ref().map(|s| s.view.clone())
}

pub fn cancel() {
    if let Some(mut s) = lock().take() {
        if let Some(c) = s.child.take() {
            std::thread::spawn(move || stop_child(c));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases() {
        let s = Duration::from_secs(5);
        assert_eq!(next(false, false, s, ""), Phase::Waiting);
        assert_eq!(next(true, true, s, "Failed to connect"), Phase::Paired);
        assert!(matches!(next(false, false, s, "Pairing: The PIN from the PC didn't match. Please try again."), Phase::Failed(e) if e.contains("PIN")));
        assert!(matches!(next(false, false, s, "Failed to connect to 192.168.0.20"), Phase::Failed(e) if e.contains("reach")));
        assert!(matches!(next(false, true, s, ""), Phase::Failed(_)));
        assert!(matches!(next(false, false, TIMEOUT, ""), Phase::Failed(e) if e.contains("in time")));
        let p = new_pin();
        assert_eq!(p.len(), 4);
        assert!(p.chars().all(|c| c.is_ascii_digit()));
    }
}
