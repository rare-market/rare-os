//! External tools for the Share button: PATH lookup, running a program with a timeout, and
//! the runtime probe of what this session can capture with. Every tool is started by absolute
//! path with an argument list (never a shell string) and the host environment.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

/// Finds a program on the host PATH. Entries inside an AppImage mount are skipped, so a
/// bundled copy never wins over the system's.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let appdir = std::env::var("APPDIR").ok().filter(|d| !d.is_empty());
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for extra in ["/usr/bin", "/usr/local/bin", "/bin", "/usr/sbin"] {
        dirs.push(extra.into());
    }
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join(".local/bin"));
    }
    dirs.into_iter()
        .filter(|d| d.is_absolute())
        .filter(|d| appdir.as_deref().map_or(true, |a| !d.starts_with(a)))
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    let Ok(md) = std::fs::metadata(p) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        md.is_file() && md.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        md.is_file()
    }
}

/// Puts a child in its own process group (so it and its helpers can be signalled together)
/// and, on Linux, has the kernel stop it if the launcher dies.
pub fn own_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: prctl is async-signal-safe and touches no memory of the parent.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
}

/// Sends a signal to a child's whole process group.
#[cfg(unix)]
pub fn signal_group(child: &Child, sig: i32) {
    let pid = child.id() as i32;
    if pid > 0 {
        // SAFETY: plain syscall on a process group we created.
        unsafe {
            libc::kill(-pid, sig);
        }
    }
}

/// Stops a child and its group: `soft` first, KILL after `grace`.
pub fn stop_child(child: &mut Child, grace: Duration) {
    #[cfg(unix)]
    {
        signal_group(child, libc::SIGINT);
        let t0 = Instant::now();
        while t0.elapsed() < grace {
            if matches!(child.try_wait(), Ok(Some(_))) {
                // Helpers in the group (gsr-kms-server) go too.
                signal_group(child, libc::SIGTERM);
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        signal_group(child, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub struct Out {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.status.success()
    }
    /// stdout and stderr together (help texts go to either).
    pub fn text(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

/// Runs a program to completion with the host environment, killing its process group after
/// `timeout`. Output is read on helper threads so a chatty program never blocks.
pub fn run(program: &Path, args: &[String], timeout: Duration) -> Result<Out, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    crate::host_env(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    own_group(&mut cmd);
    let name = program.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut child = cmd.spawn().map_err(|e| format!("{name}: {e}"))?;
    let (tx_o, rx_o) = channel();
    let (tx_e, rx_e) = channel();
    if let Some(mut so) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = std::io::Read::read_to_end(&mut so, &mut b);
            let _ = tx_o.send(b);
        });
    }
    if let Some(mut se) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = std::io::Read::read_to_end(&mut se, &mut b);
            let _ = tx_e.send(b);
        });
    }
    let t0 = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if t0.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                stop_child(&mut child, Duration::from_millis(300));
                return Err(format!("{name} timed out"));
            }
            Err(e) => return Err(format!("{name}: {e}")),
        }
    };
    // A helper that daemonized may still hold the pipes open; do not wait on it for long.
    let get = |rx: std::sync::mpsc::Receiver<Vec<u8>>| String::from_utf8_lossy(&rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default()).into_owned();
    Ok(Out { status, stdout: get(rx_o), stderr: get(rx_e) })
}

/// Short form of `run` for tools found by name.
pub fn run_named(name: &str, args: &[&str], timeout: Duration) -> Result<Out, String> {
    let p = find_tool(name).ok_or_else(|| format!("{name} is not installed"))?;
    run(&p, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), timeout)
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Session {
    /// Game Mode: the gamescope session.
    Gamescope,
    /// Desktop Mode: KDE Plasma.
    Kde,
    /// Some other desktop.
    Desktop,
    /// macOS, for development.
    Mac,
    #[default]
    Other,
}

/// Which kind of session this is, from the environment. `gamescope_proc` says whether a
/// gamescope process is running (only used when the environment says nothing).
pub fn session_from(env: &dyn Fn(&str) -> Option<String>, gamescope_proc: bool, mac: bool) -> Session {
    if mac {
        return Session::Mac;
    }
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    let desktop = set("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    if set("GAMESCOPE_WAYLAND_DISPLAY").is_some() || desktop.contains("gamescope") {
        return Session::Gamescope;
    }
    if desktop.split(':').any(|d| d == "kde") {
        return Session::Kde;
    }
    if !desktop.is_empty() {
        return Session::Desktop;
    }
    if gamescope_proc {
        return Session::Gamescope;
    }
    if set("WAYLAND_DISPLAY").is_some() || set("DISPLAY").is_some() {
        return Session::Desktop;
    }
    Session::Other
}

pub fn detect_session() -> Session {
    let env = |k: &str| std::env::var(k).ok();
    let mac = cfg!(target_os = "macos");
    let lazy_proc = !mac && std::env::var("XDG_CURRENT_DESKTOP").map(|v| v.is_empty()).unwrap_or(true) && std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_none();
    session_from(&env, lazy_proc && gamescope_args().is_some(), mac)
}

/// The command line of the running gamescope, if any (Linux).
pub fn gamescope_args() -> Option<Vec<String>> {
    #[cfg(target_os = "linux")]
    {
        let rd = std::fs::read_dir("/proc").ok()?;
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().parse::<u32>().is_err() {
                continue;
            }
            let comm = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default();
            let comm = comm.trim();
            if comm == "gamescope" || comm == "gamescope-wl" {
                let raw = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
                return Some(raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect());
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// gamescope's `--force-orientation` value from its arguments.
pub fn orientation_arg(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix("--force-orientation=") {
            return Some(v.to_string());
        }
        if a == "--force-orientation" {
            return it.next().cloned();
        }
    }
    None
}

/// What gpu-screen-recorder offers, from `--version` and `--help`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gsr {
    pub path: PathBuf,
    pub version: (u32, u32, u32),
    /// `-bm cbr` (bitrate mode; with it, `-q` is a bitrate in kbps).
    pub bm: bool,
    /// `-ab` audio bitrate.
    pub ab: bool,
    /// `-ac` audio codec.
    pub ac: bool,
    /// `-v yes|no` (fps log).
    pub v: bool,
}

impl Gsr {
    pub fn from_texts(path: PathBuf, version: &str, help: &str) -> Gsr {
        let has = |flag: &str| help.split(|c: char| c.is_whitespace() || c == '[' || c == ']' || c == ',').any(|t| t == flag);
        Gsr { path, version: parse_version(version).unwrap_or((0, 0, 0)), bm: has("-bm"), ab: has("-ab"), ac: has("-ac"), v: has("-v") }
    }
    /// Screenshot mode (`-o file.png`) arrived in version 5.
    pub fn screenshots(&self) -> bool {
        self.version.0 >= 5
    }
}

/// The first "a.b.c" (or "a.b") in a text.
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.')).find_map(|tok| {
        let parts: Vec<&str> = tok.split('.').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return None;
        }
        let n = |i: usize| parts.get(i).and_then(|p| p.parse::<u32>().ok());
        Some((n(0)?, n(1)?, n(2).unwrap_or(0)))
    })
}

/// The GStreamer elements the fallback paths need.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gst {
    pub launch: PathBuf,
    pub pipewiresrc: bool,
    /// pipewiresrc takes `target-object` (newer); else `path=<node id>`.
    pub target_object: bool,
    pub keepalive: bool,
    /// "vah264enc", "vaapih264enc" or "x264enc".
    pub h264: Option<String>,
    pub vapostproc: bool,
    pub vaapipostproc: bool,
    pub h264parse: bool,
    pub splitmuxsink: bool,
    pub mpegtsmux: bool,
    pub tsdemux: bool,
    pub mp4mux: bool,
    pub pngenc: bool,
    pub pulsesrc: bool,
    /// "avenc_aac", "fdkaacenc" or "voaacenc".
    pub aac: Option<String>,
    pub aacparse: bool,
}

impl Gst {
    /// Everything the ring buffer needs.
    pub fn can_record(&self) -> bool {
        self.pipewiresrc && self.h264.is_some() && self.h264parse && self.splitmuxsink && self.mpegtsmux && self.tsdemux && self.mp4mux
    }
    pub fn can_snap(&self) -> bool {
        self.pipewiresrc && self.pngenc
    }
    pub fn audio(&self) -> bool {
        self.pulsesrc && self.aac.is_some() && self.aacparse
    }
}

/// gamescope's PipeWire video node.
#[derive(Clone, Debug, PartialEq)]
pub struct PwNode {
    pub id: u64,
    pub serial: Option<u64>,
    pub name: String,
}

/// Finds gamescope's video node in `pw-dump` output.
pub fn find_gamescope_node(json: &str) -> Option<PwNode> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut best: Option<(bool, PwNode)> = None;
    for o in v.as_array()? {
        if o["type"].as_str() != Some("PipeWire:Interface:Node") {
            continue;
        }
        let props = &o["info"]["props"];
        let name = props["node.name"].as_str().unwrap_or_default();
        if !name.to_lowercase().contains("gamescope") {
            continue;
        }
        let class = props["media.class"].as_str().unwrap_or_default();
        if !class.is_empty() && !class.starts_with("Video") {
            continue;
        }
        let Some(id) = o["id"].as_u64() else { continue };
        let node = PwNode { id, serial: props["object.serial"].as_u64(), name: name.to_string() };
        let source = class == "Video/Source";
        if best.as_ref().map_or(true, |(s, _)| source && !s) {
            best = Some((source, node));
        }
    }
    best.map(|(_, n)| n)
}

pub fn pw_gamescope_node() -> Option<PwNode> {
    let out = run_named("pw-dump", &[], Duration::from_secs(4)).ok()?;
    find_gamescope_node(&out.stdout)
}

/// What this session can capture with. Built by `probe()`; the backend choices below are
/// pure functions of it.
#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub session: Session,
    /// gamescopectl, when its command list has `screenshot`.
    pub gamescopectl: Option<PathBuf>,
    /// gamescope's Xwayland display (for the screenshot root window property).
    pub x_display: Option<String>,
    pub gsr: Option<Gsr>,
    pub gst: Option<Gst>,
    pub pw_node: Option<PwNode>,
    pub spectacle: Option<PathBuf>,
    /// xdg-desktop-portal is on the session bus.
    pub portal: bool,
    pub screencapture: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    /// gamescope's `--force-orientation`.
    pub orientation: Option<String>,
}

pub fn probe() -> Probe {
    let session = detect_session();
    let mut p = Probe { session, ffmpeg: find_tool("ffmpeg"), ffprobe: find_tool("ffprobe"), ..Default::default() };
    match session {
        Session::Mac => {
            p.screencapture = find_tool("screencapture").or_else(|| Some(PathBuf::from("/usr/sbin/screencapture")).filter(|x| x.exists()));
        }
        Session::Other => {}
        _ => {
            p.gsr = find_tool("gpu-screen-recorder").map(|path| {
                let ver = run(&path, &["--version".into()], Duration::from_secs(3)).map(|o| o.text()).unwrap_or_default();
                let help = run(&path, &["--help".into()], Duration::from_secs(3)).map(|o| o.text()).unwrap_or_default();
                Gsr::from_texts(path, &ver, &help)
            });
            if session == Session::Gamescope {
                p.gamescopectl = find_tool("gamescopectl").filter(|ctl| {
                    run(ctl, &["help".into()], Duration::from_secs(3)).map(|o| o.text().split_whitespace().any(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '_') == "screenshot")).unwrap_or(false)
                });
                p.x_display = std::env::var("DISPLAY").ok().filter(|d| !d.is_empty()).or_else(|| Some(":0".into()));
                p.gst = probe_gst();
                p.pw_node = p.gst.as_ref().filter(|g| g.pipewiresrc).and_then(|_| pw_gamescope_node());
                p.orientation = gamescope_args().as_deref().and_then(orientation_arg);
            } else {
                p.spectacle = find_tool("spectacle");
                p.portal = portal_present();
            }
        }
    }
    p
}

fn probe_gst() -> Option<Gst> {
    let launch = find_tool("gst-launch-1.0")?;
    let inspect = find_tool("gst-inspect-1.0")?;
    let has = |el: &str| run(&inspect, &["--exists".into(), el.into()], Duration::from_secs(4)).map(|o| o.ok()).unwrap_or(false);
    let mut g = Gst { launch, pipewiresrc: has("pipewiresrc"), ..Default::default() };
    if g.pipewiresrc {
        let text = run(&inspect, &["pipewiresrc".into()], Duration::from_secs(4)).map(|o| o.stdout).unwrap_or_default();
        g.target_object = text.contains("target-object");
        g.keepalive = text.contains("keepalive-time");
    }
    g.h264 = ["vah264enc", "vaapih264enc", "x264enc"].into_iter().find(|e| has(e)).map(String::from);
    g.vapostproc = has("vapostproc");
    g.vaapipostproc = has("vaapipostproc");
    g.h264parse = has("h264parse");
    g.splitmuxsink = has("splitmuxsink");
    g.mpegtsmux = has("mpegtsmux");
    g.tsdemux = has("tsdemux");
    g.mp4mux = has("mp4mux");
    g.pngenc = has("pngenc");
    g.pulsesrc = has("pulsesrc");
    g.aac = ["avenc_aac", "fdkaacenc", "voaacenc"].into_iter().find(|e| has(e)).map(String::from);
    g.aacparse = has("aacparse");
    Some(g)
}

fn portal_present() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::portal::available()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotBackend {
    GamescopeCtl,
    GamescopeProp,
    Gsr,
    Gst,
    Spectacle,
    Portal,
    ScreenCapture,
}

impl ShotBackend {
    pub fn name(self) -> &'static str {
        match self {
            ShotBackend::GamescopeCtl => "gamescopectl",
            ShotBackend::GamescopeProp => "gamescope",
            ShotBackend::Gsr => "gpu-screen-recorder",
            ShotBackend::Gst => "gstreamer",
            ShotBackend::Spectacle => "spectacle",
            ShotBackend::Portal => "portal",
            ShotBackend::ScreenCapture => "screencapture",
        }
    }
}

/// Screenshot backends to try, in order.
pub fn shot_backends(p: &Probe) -> Vec<ShotBackend> {
    let mut v = Vec::new();
    match p.session {
        Session::Gamescope => {
            if p.gamescopectl.is_some() {
                v.push(ShotBackend::GamescopeCtl);
            }
            if p.x_display.is_some() {
                v.push(ShotBackend::GamescopeProp);
            }
            if p.gsr.as_ref().is_some_and(|g| g.screenshots()) {
                v.push(ShotBackend::Gsr);
            }
            if p.gst.as_ref().is_some_and(|g| g.can_snap()) && p.pw_node.is_some() {
                v.push(ShotBackend::Gst);
            }
        }
        Session::Kde | Session::Desktop => {
            if p.spectacle.is_some() {
                v.push(ShotBackend::Spectacle);
            }
            if p.portal {
                v.push(ShotBackend::Portal);
            }
        }
        Session::Mac => {
            if p.screencapture.is_some() {
                v.push(ShotBackend::ScreenCapture);
            }
        }
        Session::Other => {}
    }
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipBackend {
    Gsr,
    Gst,
}

impl ClipBackend {
    pub fn name(self) -> &'static str {
        match self {
            ClipBackend::Gsr => "gpu-screen-recorder",
            ClipBackend::Gst => "gstreamer",
        }
    }
}

/// The clip recorder for this session. In Game Mode, GStreamer from gamescope's PipeWire node
/// first: it is the composed, upright picture, encoded on the GPU (VA-API). gpu-screen-recorder's
/// KMS capture there grabbed a single plane of the portrait scanout on the SuiPlay (a grey band,
/// 1200 x 1920), so it is only the fallback. In a desktop session gpu-screen-recorder (GStreamer
/// would need a portal dialog).
pub fn clip_backend(p: &Probe) -> Option<ClipBackend> {
    match p.session {
        Session::Mac | Session::Other => None,
        Session::Gamescope if p.gst.as_ref().is_some_and(|g| g.can_record()) && p.pw_node.is_some() => Some(ClipBackend::Gst),
        _ if p.gsr.is_some() => Some(ClipBackend::Gsr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| pairs.iter().find(|(x, _)| *x == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn sessions() {
        assert_eq!(session_from(&env_of(&[("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0")]), false, false), Session::Gamescope);
        assert_eq!(session_from(&env_of(&[("XDG_CURRENT_DESKTOP", "gamescope")]), false, false), Session::Gamescope);
        assert_eq!(session_from(&env_of(&[("XDG_CURRENT_DESKTOP", "KDE"), ("WAYLAND_DISPLAY", "wayland-0")]), true, false), Session::Kde);
        assert_eq!(session_from(&env_of(&[("XDG_CURRENT_DESKTOP", "GNOME")]), false, false), Session::Desktop);
        assert_eq!(session_from(&env_of(&[]), true, false), Session::Gamescope);
        assert_eq!(session_from(&env_of(&[]), false, false), Session::Other);
        assert_eq!(session_from(&env_of(&[("GAMESCOPE_WAYLAND_DISPLAY", "x")]), false, true), Session::Mac);
    }

    #[test]
    fn versions_and_gsr_flags() {
        assert_eq!(parse_version("gpu-screen-recorder 5.6.1\n"), Some((5, 6, 1)));
        assert_eq!(parse_version("4.3"), Some((4, 3, 0)));
        assert_eq!(parse_version("nothing"), None);
        let help = "usage: gpu-screen-recorder -w <window_id|monitor|focused|portal> [-c <container_format>] [-q <quality>] [-r <replay_buffer_size_sec>] [-ac aac|opus|flac] [-ab <bitrate>] [-bm auto|qp|vbr|cbr] [-v yes|no] [-o <output_file>]";
        let g = Gsr::from_texts("/usr/bin/gpu-screen-recorder".into(), "5.1.0", help);
        assert!(g.bm && g.ab && g.ac && g.v && g.screenshots());
        let old = Gsr::from_texts("/usr/bin/gpu-screen-recorder".into(), "4.2.3", "usage: gpu-screen-recorder -w <window_id> [-q <quality>] [-ac aac|opus|flac] [-o <output_file>]");
        assert!(!old.bm && !old.ab && old.ac && !old.screenshots());
    }

    #[test]
    fn orientation_from_args() {
        let a: Vec<String> = ["gamescope", "-e", "--force-orientation", "right", "--", "steam"].iter().map(|s| s.to_string()).collect();
        assert_eq!(orientation_arg(&a).as_deref(), Some("right"));
        let b: Vec<String> = ["gamescope", "--force-orientation=left"].iter().map(|s| s.to_string()).collect();
        assert_eq!(orientation_arg(&b).as_deref(), Some("left"));
        assert_eq!(orientation_arg(&[]), None);
    }

    #[test]
    fn finds_gamescope_node() {
        let dump = r#"[
          {"id": 30, "type": "PipeWire:Interface:Node", "info": {"props": {"node.name": "alsa_output.pci", "media.class": "Audio/Sink", "object.serial": 31}}},
          {"id": 77, "type": "PipeWire:Interface:Node", "info": {"props": {"node.name": "gamescope", "media.class": "Video/Source", "object.serial": 912}}},
          {"id": 78, "type": "PipeWire:Interface:Port", "info": {"props": {"node.name": "gamescope"}}}
        ]"#;
        assert_eq!(find_gamescope_node(dump), Some(PwNode { id: 77, serial: Some(912), name: "gamescope".into() }));
        assert_eq!(find_gamescope_node("[]"), None);
        assert_eq!(find_gamescope_node("not json"), None);
    }

    fn gst_full() -> Gst {
        Gst {
            launch: "/usr/bin/gst-launch-1.0".into(),
            pipewiresrc: true,
            target_object: true,
            keepalive: true,
            h264: Some("vah264enc".into()),
            vapostproc: true,
            vaapipostproc: false,
            h264parse: true,
            splitmuxsink: true,
            mpegtsmux: true,
            tsdemux: true,
            mp4mux: true,
            pngenc: true,
            pulsesrc: true,
            aac: Some("avenc_aac".into()),
            aacparse: true,
        }
    }

    #[test]
    fn backend_selection() {
        let node = Some(PwNode { id: 1, serial: Some(2), name: "gamescope".into() });
        let gsr5 = Some(Gsr { path: "/usr/bin/gpu-screen-recorder".into(), version: (5, 0, 0), ..Default::default() });
        let mut p = Probe { session: Session::Gamescope, gamescopectl: Some("/usr/bin/gamescopectl".into()), x_display: Some(":0".into()), gsr: gsr5.clone(), gst: Some(gst_full()), pw_node: node.clone(), ..Default::default() };
        assert_eq!(shot_backends(&p), vec![ShotBackend::GamescopeCtl, ShotBackend::GamescopeProp, ShotBackend::Gsr, ShotBackend::Gst]);
        assert_eq!(clip_backend(&p), Some(ClipBackend::Gst));
        let no_node = Probe { pw_node: None, ..p.clone() };
        assert_eq!(clip_backend(&no_node), Some(ClipBackend::Gsr));
        p.gsr = None;
        assert_eq!(clip_backend(&p), Some(ClipBackend::Gst));
        p.pw_node = None;
        assert_eq!(clip_backend(&p), None);
        assert_eq!(shot_backends(&p), vec![ShotBackend::GamescopeCtl, ShotBackend::GamescopeProp]);

        let kde = Probe { session: Session::Kde, spectacle: Some("/usr/bin/spectacle".into()), portal: true, gst: Some(gst_full()), pw_node: node, ..Default::default() };
        assert_eq!(shot_backends(&kde), vec![ShotBackend::Spectacle, ShotBackend::Portal]);
        // No portal dialog for clips: GStreamer is Game Mode only.
        assert_eq!(clip_backend(&kde), None);
        assert_eq!(clip_backend(&Probe { gsr: gsr5, ..kde }), Some(ClipBackend::Gsr));

        let mac = Probe { session: Session::Mac, screencapture: Some("/usr/sbin/screencapture".into()), ..Default::default() };
        assert_eq!(shot_backends(&mac), vec![ShotBackend::ScreenCapture]);
        assert_eq!(clip_backend(&mac), None);
        assert!(shot_backends(&Probe::default()).is_empty());
    }
}
