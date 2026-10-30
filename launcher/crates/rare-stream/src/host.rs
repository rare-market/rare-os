//! Streaming this device: Sunshine as the host, so Moonlight on a TV, a PC or a phone plays the
//! games on this device. Off until the owner turns it on (Settings > Capture and streaming).
//!
//! Sunshine (LizardByte, GPL-3.0) is a separate program the image installs (/usr/bin/sunshine,
//! with cap_sys_admin for KMS capture under gamescope). Rare runs it as a child process with its
//! own config in ~/.config/rare/sunshine, never linked. Pairing is Moonlight's PIN: the other
//! device shows four digits and the owner types them into Rare, which hands them to Sunshine's
//! local API (https://localhost:47990, reachable from this device only, `origin_web_ui_allowed =
//! pc`) with a password Rare made for itself. The API calls go through curl with the password on
//! stdin, never on a command line.
//!
//! What Moonlight lists is one app, "Rare": the whole screen, so every game starts from Rare.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Sunshine's base port (Moonlight's default); its web API answers on the next one. RARE_SUNSHINE_PORT
/// moves both (the image reserves the default range, see 60-rare-sunshine.conf).
pub const PORT: u16 = 47989;
const USER: &str = "rare";

pub fn port() -> u16 {
    std::env::var("RARE_SUNSHINE_PORT").ok().and_then(|p| p.parse().ok()).filter(|p| *p > 1024 && *p < 65000).unwrap_or(PORT)
}

/// https://127.0.0.1:<port + 1>
pub fn api_base(port: u16) -> String {
    format!("https://127.0.0.1:{}", port + 1)
}

/// Where Rare keeps Sunshine's files.
#[derive(Clone, Debug)]
pub struct Paths {
    pub dir: PathBuf,
    pub conf: PathBuf,
    pub apps: PathBuf,
    pub state: PathBuf,
    pub log: PathBuf,
    /// Rare's password for Sunshine's API (0600).
    pub secret: PathBuf,
}

pub fn paths_in(dir: &Path) -> Paths {
    Paths {
        dir: dir.to_path_buf(),
        conf: dir.join("sunshine.conf"),
        apps: dir.join("apps.json"),
        state: dir.join("sunshine_state.json"),
        log: dir.join("sunshine.log"),
        secret: dir.join("api-secret"),
    }
}

pub fn paths() -> Paths {
    paths_in(&rare_core::paths::config_dir().join("sunshine"))
}

/// Sunshine on this device: RARE_SUNSHINE, the image's, then PATH.
pub fn find() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RARE_SUNSHINE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    for p in ["/usr/bin/sunshine", "/usr/local/bin/sunshine"] {
        if Path::new(p).is_file() {
            return Some(p.into());
        }
    }
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|d| d.join("sunshine")).find(|p| p.is_file()))
}

/// The name other devices see (Moonlight's host list): plain words only.
pub fn clean_name(n: &str) -> String {
    let s: String = n.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.chars().take(40).collect();
    if s.is_empty() {
        "Rare".into()
    } else {
        s
    }
}

/// sunshine.conf. `gamescope`: capture the screen through KMS (what works under gamescope with
/// the image's capability); the encoder is the GPU's (VA-API).
pub fn conf_text(p: &Paths, name: &str, gamescope: bool) -> String {
    conf_text_port(p, name, gamescope, PORT)
}

pub fn conf_text_port(p: &Paths, name: &str, gamescope: bool, port: u16) -> String {
    let mut c = String::new();
    let mut kv = |k: &str, v: &str| {
        c.push_str(k);
        c.push_str(" = ");
        c.push_str(v);
        c.push('\n');
    };
    kv("sunshine_name", &clean_name(name));
    kv("min_log_level", "info");
    kv("log_path", &p.log.to_string_lossy());
    kv("file_apps", &p.apps.to_string_lossy());
    kv("file_state", &p.state.to_string_lossy());
    kv("credentials_file", &p.state.to_string_lossy());
    kv("pkey", &p.dir.join("credentials/cakey.pem").to_string_lossy());
    kv("cert", &p.dir.join("credentials/cacert.pem").to_string_lossy());
    // The web UI and its API answer this device only.
    kv("origin_web_ui_allowed", "pc");
    kv("upnp", "disabled");
    if port != PORT {
        kv("port", &port.to_string());
    }
    kv("system_tray", "disabled");
    kv("encoder", "vaapi");
    if Path::new("/dev/dri/renderD128").exists() {
        kv("adapter_name", "/dev/dri/renderD128");
    }
    if gamescope {
        kv("capture", "kms");
    }
    c
}

/// gamescope's root-window switch that composes every layer into the one primary plane.
/// Without it gamescope scans Rare's window out on an overlay plane, and Sunshine's KMS capture
/// (the primary plane only) streams the empty layer under it: a black picture. Composed, the
/// primary plane is the whole screen, landscape and upright (gamescope turns the portrait panel
/// with the plane's rotation, so the buffer itself is 1920 x 1200).
pub const COMPOSITE_ON: &str = "xprop -root -f GAMESCOPE_COMPOSITE_FORCE 32c -set GAMESCOPE_COMPOSITE_FORCE 1";
pub const COMPOSITE_OFF: &str = "xprop -root -f GAMESCOPE_COMPOSITE_FORCE 32c -set GAMESCOPE_COMPOSITE_FORCE 0";

/// apps.json: one app, the whole screen. Under gamescope a stream forces composition while it
/// runs (Sunshine's prep commands, undone when the stream ends).
pub fn apps_text(gamescope: bool) -> String {
    let prep = if gamescope {
        serde_json::json!([{ "do": format!("sh -c '{COMPOSITE_ON}'"), "undo": format!("sh -c '{COMPOSITE_OFF}'") }])
    } else {
        serde_json::json!([])
    };
    serde_json::json!({
        "env": {},
        "apps": [{ "name": "Rare", "output": "", "cmd": "", "prep-cmd": prep, "exclude-global-prep-cmd": "false", "elevated": "false", "auto-detach": "true", "wait-all": "true", "exit-timeout": "5", "image-path": "" }]
    })
    .to_string()
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o.open(&tmp)?.write_all(text.as_bytes())?;
    }
    std::fs::rename(tmp, path)
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        for (i, x) in b.iter_mut().enumerate() {
            *x = (t >> ((i % 16) * 8)) as u8 ^ (std::process::id() as u8).wrapping_mul(i as u8 + 1);
        }
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Rare's API password, made once.
pub fn secret(p: &Paths) -> Result<String, String> {
    if let Some(s) = std::fs::read_to_string(&p.secret).ok().map(|s| s.trim().to_string()).filter(|s| s.len() >= 16) {
        return Ok(s);
    }
    std::fs::create_dir_all(&p.dir).map_err(|e| e.to_string())?;
    let s = random_hex(16);
    write_private(&p.secret, &s).map_err(|e| e.to_string())?;
    Ok(s)
}

/// Writes the config and sets Sunshine's API login to Rare's (once per password).
fn prepare(bin: &Path, p: &Paths, name: &str, gamescope: bool, env: &dyn Fn(&mut Command)) -> Result<(), String> {
    std::fs::create_dir_all(p.dir.join("credentials")).map_err(|e| e.to_string())?;
    write_private(&p.conf, &conf_text_port(p, name, gamescope, port())).map_err(|e| e.to_string())?;
    // Rare's own file: written every start, so a fix here reaches devices set up before it.
    std::fs::write(&p.apps, apps_text(gamescope)).map_err(|e| e.to_string())?;
    let pw = secret(p)?;
    let marker = p.dir.join(".creds-set");
    if std::fs::read_to_string(&marker).ok().as_deref() != Some(pw.as_str()) {
        // `sunshine <conf> --creds <user> <password>` writes the login and exits. The password
        // shows on this one short-lived command line; it only opens the API on this device.
        let mut c = Command::new(bin);
        c.arg(&p.conf).args(["--creds", USER, &pw]);
        env(&mut c);
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut ch = c.spawn().map_err(|e| format!("sunshine --creds: {e}"))?;
        let t = Instant::now();
        loop {
            match ch.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if t.elapsed() < Duration::from_secs(20) => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = ch.kill();
                    let _ = ch.wait();
                    return Err("sunshine --creds did not finish".into());
                }
            }
        }
        write_private(&marker, &pw).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- the process ----

struct Proc {
    child: Child,
    started: Instant,
}

struct Sup {
    proc: Option<Proc>,
    failures: u32,
    next_try: Option<Instant>,
    last_error: Option<String>,
    /// Sunshine logged a pairing request it is waiting on (a PIN to type).
    pin_wanted: bool,
    /// It kept dying at start (no capture or encoder): no more tries until the switch goes off
    /// and on again.
    gave_up: bool,
}

/// Quick deaths in a row before Rare stops restarting it.
const GIVE_UP_AFTER: u32 = 4;

static SUP: Mutex<Sup> = Mutex::new(Sup { proc: None, failures: 0, next_try: None, last_error: None, pin_wanted: false, gave_up: false });

fn backoff(failures: u32) -> Duration {
    Duration::from_secs((2u64 << failures.min(5)).min(60))
}

fn pidfile() -> PathBuf {
    paths().dir.join("sunshine.pid")
}

/// Stops a Sunshine a crashed launcher left behind.
pub fn cleanup_stale() {
    let Some(pid) = std::fs::read_to_string(pidfile()).ok().and_then(|t| t.trim().parse::<i32>().ok()) else { return };
    let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    if pid > 1 && cmd.contains("sunshine") && cmd.contains("rare/sunshine") {
        eprintln!("[stream-host] stopping a Sunshine left from an earlier run ({pid})");
        // SAFETY: plain syscalls on a process group this launcher started before.
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        std::thread::sleep(Duration::from_millis(1500));
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = std::fs::remove_file(pidfile());
}

fn stop_proc(mut p: Proc) {
    eprintln!("[stream-host] stopping Sunshine");
    let pid = p.child.id() as i32;
    // SAFETY: the group Sunshine leads (it was started with setsid).
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if let Ok(Some(_)) = p.child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = p.child.wait();
    let _ = std::fs::remove_file(pidfile());
    // A stream cut short by the stop never ran its undo: composition goes back to normal.
    if std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() {
        let _ = Command::new("sh").args(["-c", COMPOSITE_OFF]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// A pairing request Sunshine is waiting on, from a log line.
pub fn wants_pin(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    (l.contains("pin") && (l.contains("/pin") || l.contains("pin required") || l.contains("enter the pin") || l.contains("pairing pin"))) || l.contains("pairing request")
}

/// The request was answered (paired or refused), from a log line.
fn pin_done(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("paired") || l.contains("pairing failed") || l.contains("pair failed")
}

/// The TCP ports Sunshine listens on for a base port (HTTPS, HTTP, web API, RTSP).
pub fn tcp_ports(base: u16) -> [u16; 4] {
    [base - 5, base, base + 1, base + 21]
}

/// A port another program holds (Sunshine stops at start when it can't listen).
fn port_taken(base: u16) -> Option<u16> {
    tcp_ports(base).into_iter().find(|p| std::net::TcpListener::bind(("0.0.0.0", *p)).is_err())
}

fn start(bin: &Path, name: &str, gamescope: bool, env: &dyn Fn(&mut Command)) -> Result<Proc, String> {
    if let Some(p) = port_taken(port()) {
        return Err(format!("port {p} is in use by another program"));
    }
    let p = paths();
    prepare(bin, &p, name, gamescope, env)?;
    let mut c = Command::new(bin);
    c.arg(&p.conf);
    env(&mut c);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
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
    let mut child = c.spawn().map_err(|e| format!("sunshine: {e}"))?;
    let _ = std::fs::write(pidfile(), child.id().to_string());
    if let Some(se) = child.stderr.take() {
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(se).lines().map_while(Result::ok) {
                note_line(&l);
            }
        });
    }
    // Sunshine writes its log to log_path too; follow it for pairing requests.
    let log = p.log.clone();
    std::thread::spawn(move || follow_log(&log));
    eprintln!("[stream-host] started Sunshine ({})", child.id());
    Ok(Proc { child, started: Instant::now() })
}

fn note_line(l: &str) {
    if wants_pin(l) {
        eprintln!("[stream-host] a device wants to pair");
        SUP.lock().unwrap().pin_wanted = true;
    } else if pin_done(l) {
        SUP.lock().unwrap().pin_wanted = false;
    }
    let low = l.to_ascii_lowercase();
    if low.contains("fatal:") {
        eprintln!("[stream-host] {l}");
        // The first fatal line says why ("Unable to find display or encoder during startup.").
        let mut s = SUP.lock().unwrap();
        if s.last_error.as_deref().map_or(true, |e| !e.starts_with("Fatal")) {
            s.last_error = Some(fatal_text(l));
        }
    } else if low.contains("error:") {
        eprintln!("[stream-host] {l}");
    }
}

fn follow_log(path: &Path) {
    let mut pos = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(700));
        if SUP.lock().unwrap().proc.is_none() {
            return;
        }
        let Ok(mut f) = std::fs::File::open(path) else { continue };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < pos {
            pos = 0;
        }
        if len == pos {
            continue;
        }
        use std::io::Seek;
        if f.seek(std::io::SeekFrom::Start(pos)).is_err() {
            continue;
        }
        let mut buf = String::new();
        let _ = f.take(256 * 1024).read_to_string(&mut buf);
        pos += buf.len() as u64;
        for l in buf.lines() {
            note_line(l);
        }
    }
}

/// "[2026-10-28 02:50:22.849]: Fatal: Unable to find display or encoder during startup." ->
/// "Fatal: Unable to find display or encoder during startup."
pub fn fatal_text(line: &str) -> String {
    let l = line.trim();
    match l.find("Fatal:") {
        Some(i) => l[i..].to_string(),
        None => l.to_string(),
    }
}

/// One supervisor step: Sunshine runs while `want` (and it is installed), restarted with a
/// backoff when it dies, and left stopped after it died at start a few times in a row.
pub fn tick(want: bool, name: &str, gamescope: bool, env: &dyn Fn(&mut Command)) {
    let mut s = SUP.lock().unwrap();
    if let Some(p) = s.proc.as_mut() {
        if let Ok(Some(st)) = p.child.try_wait() {
            let ran = p.started.elapsed();
            eprintln!("[stream-host] Sunshine exited ({st}) after {} s", ran.as_secs());
            s.proc = None;
            s.failures = if ran < Duration::from_secs(60) { s.failures + 1 } else { 1 };
            s.next_try = Some(Instant::now() + backoff(s.failures));
            if s.failures >= GIVE_UP_AFTER {
                eprintln!("[stream-host] Sunshine keeps stopping at start; not trying again until streaming is switched on again");
                s.gave_up = true;
            }
            let _ = std::fs::remove_file(pidfile());
        }
    }
    if !want {
        s.gave_up = false;
        s.failures = 0;
        s.next_try = None;
        if let Some(p) = s.proc.take() {
            drop(s);
            stop_proc(p);
            let mut s = SUP.lock().unwrap();
            s.failures = 0;
            s.next_try = None;
            s.pin_wanted = false;
        }
        return;
    }
    if s.proc.is_some() || s.gave_up || s.next_try.is_some_and(|t| Instant::now() < t) {
        return;
    }
    let Some(bin) = find() else { return };
    match start(&bin, name, gamescope, env) {
        Ok(p) => {
            s.proc = Some(p);
            s.last_error = None;
        }
        Err(e) => {
            eprintln!("[stream-host] could not start Sunshine: {e}");
            s.failures += 1;
            s.next_try = Some(Instant::now() + backoff(s.failures));
            s.last_error = Some(e);
        }
    }
}

/// Stops Sunshine (launcher exit, or the switch off).
pub fn shutdown() {
    let p = SUP.lock().unwrap().proc.take();
    if let Some(p) = p {
        stop_proc(p);
    }
}

// ---- status and the API ----

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub name: String,
    pub uuid: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    /// Sunshine is installed.
    pub available: bool,
    pub running: bool,
    /// It kept stopping at start and Rare stopped trying (`error` says why).
    pub stopped: bool,
    /// Seconds since it started (the API answers after a few).
    pub up_s: u64,
    pub pin_wanted: bool,
    pub error: Option<String>,
}

pub fn status() -> HostStatus {
    let s = SUP.lock().unwrap();
    HostStatus {
        available: find().is_some(),
        running: s.proc.is_some(),
        stopped: s.gave_up,
        up_s: s.proc.as_ref().map(|p| p.started.elapsed().as_secs()).unwrap_or(0),
        pin_wanted: s.pin_wanted,
        error: s.last_error.clone(),
    }
}

/// curl's config for one API call (read from stdin with `-K -`, so the password is never on a
/// command line).
pub fn curl_config(path: &str, body: Option<&str>, pw: &str) -> String {
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut c = format!("url = \"{}{}\"\nuser = \"{}:{}\"\ninsecure\nsilent\nshow-error\nmax-time = 10\nwrite-out = \"\\n%{{http_code}}\"\n", api_base(port()), q(path), USER, q(pw));
    if let Some(b) = body {
        c.push_str("header = \"Content-Type: application/json\"\n");
        c.push_str(&format!("data = \"{}\"\n", q(b)));
    }
    c
}

/// Splits curl's output into the body and the HTTP status (the last line, from `write-out`).
pub fn split_status(out: &str) -> (String, u16) {
    match out.rsplit_once('\n') {
        Some((body, code)) => (body.to_string(), code.trim().parse().unwrap_or(0)),
        None => (String::new(), out.trim().parse().unwrap_or(0)),
    }
}

fn api(path: &str, body: Option<&str>) -> Result<serde_json::Value, String> {
    let pw = secret(&paths())?;
    let mut c = Command::new("curl");
    c.args(["-K", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut ch = c.spawn().map_err(|e| format!("curl: {e}"))?;
    if let Some(mut i) = ch.stdin.take() {
        let _ = i.write_all(curl_config(path, body, &pw).as_bytes());
    }
    let o = ch.wait_with_output().map_err(|e| e.to_string())?;
    let (text, code) = split_status(&String::from_utf8_lossy(&o.stdout));
    match code {
        200 => serde_json::from_str(&text).map_err(|_| "Sunshine answered something unexpected".to_string()),
        401 => Err("Sunshine refused Rare's login".into()),
        0 => Err("Sunshine isn't answering yet".into()),
        c => Err(format!("Sunshine answered {c}")),
    }
}

fn ok_of(v: &serde_json::Value) -> bool {
    match &v["status"] {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => s == "true",
        _ => false,
    }
}

/// A PIN of four digits.
pub fn valid_pin(pin: &str) -> bool {
    pin.len() == 4 && pin.bytes().all(|b| b.is_ascii_digit())
}

/// A device waiting for its PIN (Sunshine's `GET /api/pin`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub id: String,
    /// What the device calls itself (Moonlight sends its host name).
    pub name: String,
    pub address: String,
}

pub fn parse_pending(v: &serde_json::Value) -> Vec<Pending> {
    v["pairings"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| Some(Pending { id: p["id"].as_str()?.to_string(), name: p["name"].as_str().unwrap_or("").to_string(), address: p["address"].as_str().unwrap_or("").to_string() })).collect())
        .unwrap_or_default()
}

/// The devices waiting for a PIN now. Older Sunshine has no list (Err).
pub fn pending() -> Result<Vec<Pending>, String> {
    api("/api/pin", None).map(|v| parse_pending(&v))
}

/// Asks Sunshine who waits for a PIN (the supervisor calls it every few seconds while it runs).
pub fn poll_pending() {
    if SUP.lock().unwrap().proc.is_none() {
        return;
    }
    if let Ok(p) = pending() {
        SUP.lock().unwrap().pin_wanted = !p.is_empty();
    }
}

/// The body for `POST /api/pin`: with the waiting request's id (Sunshine 2026 and later), else the
/// older shape without one.
pub fn pin_body(pin: &str, name: &str, pairing_id: Option<&str>) -> String {
    match pairing_id {
        Some(id) => serde_json::json!({ "pairing_id": id, "pin": pin, "name": clean_name(name) }).to_string(),
        None => serde_json::json!({ "pin": pin, "name": clean_name(name) }).to_string(),
    }
}

/// Hands the PIN another device shows to Sunshine, for the device that asked last. `name` is what
/// the paired list calls it when the device sent no name of its own.
pub fn submit_pin(pin: &str, name: &str) -> Result<(), String> {
    let pin = pin.trim();
    if !valid_pin(pin) {
        return Err("The PIN is the four digits Moonlight shows".into());
    }
    let body = match pending() {
        Ok(list) => match list.last() {
            Some(p) => pin_body(pin, if p.name.trim().is_empty() { name } else { &p.name }, Some(&p.id)),
            None => return Err("No device is waiting. Pick this device in Moonlight first, then type the PIN it shows.".into()),
        },
        Err(_) => pin_body(pin, name, None),
    };
    let v = api("/api/pin", Some(&body))?;
    if ok_of(&v) {
        SUP.lock().unwrap().pin_wanted = false;
        Ok(())
    } else {
        Err("That PIN didn't match. Start pairing in Moonlight again and type the new one.".into())
    }
}

pub fn parse_clients(v: &serde_json::Value) -> Vec<Client> {
    v["named_certs"].as_array().map(|a| a.iter().filter_map(|c| Some(Client { name: c["name"].as_str().unwrap_or("").to_string(), uuid: c["uuid"].as_str()?.to_string() })).collect()).unwrap_or_default()
}

/// The devices paired with this one.
pub fn clients() -> Result<Vec<Client>, String> {
    api("/api/clients/list", None).map(|v| parse_clients(&v))
}

pub fn unpair(uuid: &str) -> Result<(), String> {
    let body = serde_json::json!({ "uuid": uuid }).to_string();
    let v = api("/api/clients/unpair", Some(&body))?;
    if ok_of(&v) {
        Ok(())
    } else {
        Err("Sunshine kept it".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_local_and_private() {
        let p = paths_in(Path::new("/home/u/.config/rare/sunshine"));
        let c = conf_text(&p, "SuiPlay; rm -rf", true);
        assert!(c.contains("sunshine_name = SuiPlay rm -rf\n"));
        assert!(c.contains("origin_web_ui_allowed = pc\n"));
        assert!(c.contains("upnp = disabled\n"));
        assert!(c.contains("capture = kms\n"));
        assert!(c.contains("encoder = vaapi\n"));
        assert!(c.contains("file_apps = /home/u/.config/rare/sunshine/apps.json\n"));
        assert!(!conf_text(&p, "x", false).contains("capture ="));
        assert!(!c.contains("port ="));
        assert!(conf_text_port(&p, "x", false, 49989).contains("port = 49989\n"));
        assert_eq!(api_base(49989), "https://127.0.0.1:49990");
        assert_eq!(tcp_ports(PORT), [47984, 47989, 47990, 48010]);
        let a: serde_json::Value = serde_json::from_str(&apps_text(true)).unwrap();
        assert_eq!(a["apps"][0]["name"], "Rare");
        assert_eq!(a["apps"].as_array().unwrap().len(), 1);
        assert!(a["apps"][0]["prep-cmd"][0]["do"].as_str().unwrap().contains("GAMESCOPE_COMPOSITE_FORCE 1"));
        assert!(a["apps"][0]["prep-cmd"][0]["undo"].as_str().unwrap().contains("GAMESCOPE_COMPOSITE_FORCE 0"));
        let d: serde_json::Value = serde_json::from_str(&apps_text(false)).unwrap();
        assert!(d["apps"][0]["prep-cmd"].as_array().unwrap().is_empty());
    }

    #[test]
    fn names_and_pins() {
        assert_eq!(clean_name("  Living   room TV "), "Living room TV");
        assert_eq!(clean_name("\"; drop"), "drop");
        assert_eq!(clean_name(""), "Rare");
        assert_eq!(clean_name(&"x".repeat(80)).len(), 40);
        assert!(valid_pin("0427"));
        assert!(!valid_pin("427"));
        assert!(!valid_pin("04a7"));
        assert!(!valid_pin("12345"));
    }

    #[test]
    fn curl_config_keeps_the_password_off_argv() {
        let c = curl_config("/api/pin", Some(r#"{"pin":"1234","name":"TV"}"#), "ab\"c");
        assert!(c.contains("url = \"https://127.0.0.1:47990/api/pin\"\n"));
        assert!(c.contains("user = \"rare:ab\\\"c\"\n"));
        assert!(c.contains("data = \"{\\\"pin\\\":\\\"1234\\\",\\\"name\\\":\\\"TV\\\"}\"\n"));
        assert!(c.contains("insecure\n"));
        assert!(!curl_config("/api/clients/list", None, "x").contains("data ="));
        assert_eq!(split_status("{\"status\":true}\n200"), ("{\"status\":true}".to_string(), 200));
        assert_eq!(split_status("401"), (String::new(), 401));
    }

    #[test]
    fn reads_the_api() {
        let v: serde_json::Value = serde_json::from_str(r#"{"named_certs":[{"name":"TV","uuid":"A1"},{"name":"","uuid":"B2"},{"name":"x"}],"status":true}"#).unwrap();
        assert_eq!(parse_clients(&v), vec![Client { name: "TV".into(), uuid: "A1".into() }, Client { name: "".into(), uuid: "B2".into() }]);
        assert!(ok_of(&v));
        assert!(ok_of(&serde_json::json!({"status": "true"})));
        assert!(!ok_of(&serde_json::json!({"status": false})));
    }

    #[test]
    fn pending_requests_and_the_pin_body() {
        let v: serde_json::Value = serde_json::from_str(r#"{"pairings":[{"address":"192.168.1.50","id":"143dd63935addaa3ae4ab51e50b2df67","name":"roth"},{"name":"no id"}]}"#).unwrap();
        assert_eq!(parse_pending(&v), vec![Pending { id: "143dd63935addaa3ae4ab51e50b2df67".into(), name: "roth".into(), address: "192.168.1.50".into() }]);
        assert!(parse_pending(&serde_json::json!({})).is_empty());
        let b: serde_json::Value = serde_json::from_str(&pin_body("4821", "TV", Some("ab"))).unwrap();
        assert_eq!((b["pairing_id"].as_str(), b["pin"].as_str(), b["name"].as_str()), (Some("ab"), Some("4821"), Some("TV")));
        assert!(serde_json::from_str::<serde_json::Value>(&pin_body("4821", "TV", None)).unwrap().get("pairing_id").is_none());
    }

    #[test]
    fn fatal_lines() {
        assert_eq!(fatal_text("[2026-10-28 02:50:22.849]: Fatal: Unable to find display or encoder during startup."), "Fatal: Unable to find display or encoder during startup.");
        assert_eq!(fatal_text(" other "), "other");
    }

    #[test]
    fn pairing_log_lines() {
        assert!(wants_pin("[2026-10-27 12:00:01]: Info: Please navigate to the web UI to enter the PIN: /pin"));
        assert!(wants_pin("Warning: Pairing PIN required"));
        assert!(!wants_pin("Info: Starting stream"));
        assert!(pin_done("Info: Client paired"));
    }

    #[test]
    fn secret_is_made_once_and_private() {
        let d = std::env::temp_dir().join(format!("rare-sunshine-{}", std::process::id()));
        let p = paths_in(&d);
        let a = secret(&p).unwrap();
        assert_eq!(a.len(), 32);
        assert_eq!(secret(&p).unwrap(), a);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p.secret).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(d);
    }
}
