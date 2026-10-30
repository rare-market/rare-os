//! Instant replay: a ring buffer that runs while a game is running (and replay is on), and
//! saving its last 15, 30, 60 or 120 seconds (Settings > Capture and streaming).
//!
//! gpu-screen-recorder in replay mode is preferred: it keeps the buffer in RAM and writes an
//! MP4 on SIGUSR1, printing the path on stdout. The fallback, Game Mode only, is GStreamer:
//! gamescope's PipeWire video node, H.264 on the GPU (VA-API) and `splitmuxsink` writing
//! MPEG-TS segments of about 2 s into $XDG_RUNTIME_DIR/rare-share/ring, the oldest deleted as
//! new ones arrive. MPEG-TS can be read while being written and joined byte for byte, so a
//! save takes the newest segments covering 30 s (the one still being written included), joins
//! them and remuxes to MP4.

use super::tools::{self, ClipBackend, Gsr, Gst, Probe, PwNode};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The replay length when nothing is set.
pub const SECONDS: u32 = 30;
/// The lengths the settings offer.
pub const LENGTHS: [u32; 4] = [15, 30, 60, 120];
const VIDEO_KBPS: u32 = 4000;
const AUDIO_KBPS: u32 = 128;
const GST_FPS: u32 = 30;
/// Keyframe every 2 s; splitmuxsink cuts at each one.
const KEYINT: u32 = GST_FPS * 2;
const SEGMENT_NS: u64 = 1_900_000_000;

/// A replay length the settings offer (anything else becomes the nearest one).
pub fn sane_seconds(s: u32) -> u32 {
    *LENGTHS.iter().min_by_key(|l| l.abs_diff(s)).unwrap_or(&SECONDS)
}

/// Segments kept in the ring for `secs`: the length plus about 8 s, so a save while the newest
/// segment is still being written has enough.
pub fn max_segments(secs: u32) -> u32 {
    (secs * 1000).div_ceil((SEGMENT_NS / 1_000_000) as u32) + 4
}

/// Saved clips above this are encoded again at the target bitrate (a quarter over what the
/// bitrate gives for the length, so a clip at the target rate is only remuxed).
pub fn reencode_above(secs: u32) -> u64 {
    secs as u64 * (VIDEO_KBPS + AUDIO_KBPS) as u64 * 1000 / 8 * 5 / 4
}

pub const NOT_RUNNING: &str = "The clip recorder is not running";

/// Scratch space for grabs and the ring buffer (tmpfs on Linux).
pub fn staging() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
    base.join("rare-share")
}

/// gpu-screen-recorder replay arguments for the flags this version has. With `-bm cbr`,
/// `-q` is the bitrate in kbps (about 15 MB per 30 s with audio); older versions only take
/// a quality name.
pub fn gsr_replay_args(g: &Gsr, out_dir: &Path, secs: u32) -> Vec<String> {
    let mut a: Vec<String> = ["-w", "screen", "-c", "mp4", "-k", "h264", "-f", "60", "-r"].iter().map(|s| s.to_string()).collect();
    a.push(secs.to_string());
    if g.bm {
        a.extend(["-bm".into(), "cbr".into(), "-q".into(), VIDEO_KBPS.to_string()]);
    } else {
        a.extend(["-q".into(), "medium".into()]);
    }
    a.extend(["-a".into(), "default_output".into()]);
    if g.ac {
        a.extend(["-ac".into(), "aac".into()]);
    }
    if g.ab {
        a.extend(["-ab".into(), AUDIO_KBPS.to_string()]);
    }
    if g.v {
        a.extend(["-v".into(), "no".into()]);
    }
    a.extend(["-o".into(), out_dir.to_string_lossy().into_owned()]);
    a
}

fn pw_target(g: &Gst, node: &PwNode) -> String {
    if g.target_object {
        format!("target-object={}", node.serial.map(|s| s.to_string()).unwrap_or_else(|| node.name.clone()))
    } else {
        format!("path={}", node.id)
    }
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// gst-launch arguments for the ring buffer.
pub fn gst_record_args(g: &Gst, node: &PwNode, dir: &Path, secs: u32) -> Vec<String> {
    let mut a = strs(&["-e", "-q", "splitmuxsink", "name=sm", "muxer-factory=mpegtsmux", "send-keyframe-requests=true"]);
    a.push(format!("location={}", dir.join("seg%05d.ts").display()));
    a.push(format!("max-size-time={SEGMENT_NS}"));
    a.push(format!("max-files={}", max_segments(secs)));
    a.extend(strs(&["pipewiresrc", &pw_target(g, node), "do-timestamp=true"]));
    if g.keepalive {
        // gamescope only sends frames when the picture changes; repeat the last one.
        a.push("keepalive-time=100".into());
    }
    a.extend(strs(&["!", "videorate", "!", &format!("video/x-raw,framerate={GST_FPS}/1")]));
    let kbps = format!("bitrate={VIDEO_KBPS}");
    match g.h264.as_deref() {
        Some("vah264enc") => {
            if g.vapostproc {
                a.extend(strs(&["!", "vapostproc", "!", "video/x-raw(memory:VAMemory),format=NV12"]));
            } else {
                a.extend(strs(&["!", "videoconvert", "!", "video/x-raw,format=NV12"]));
            }
            a.extend(strs(&["!", "vah264enc", &kbps, "rate-control=cbr", &format!("key-int-max={KEYINT}")]));
        }
        Some("vaapih264enc") => {
            if g.vaapipostproc {
                a.extend(strs(&["!", "vaapipostproc"]));
            } else {
                a.extend(strs(&["!", "videoconvert", "!", "video/x-raw,format=NV12"]));
            }
            a.extend(strs(&["!", "vaapih264enc", &kbps, "rate-control=cbr", &format!("keyframe-period={KEYINT}")]));
        }
        _ => {
            a.extend(strs(&["!", "videoconvert", "!", "video/x-raw,format=I420", "!", "x264enc", &kbps, "speed-preset=ultrafast", "tune=zerolatency", &format!("key-int-max={KEYINT}")]));
        }
    }
    a.extend(strs(&["!", "h264parse", "config-interval=-1", "!", "queue", "!", "sm.video"]));
    if g.audio() {
        let enc = g.aac.as_deref().unwrap_or("avenc_aac");
        a.extend(strs(&[
            "pulsesrc",
            "device=@DEFAULT_MONITOR@",
            "do-timestamp=true",
            "!",
            "audioconvert",
            "!",
            "audioresample",
            "!",
            "audio/x-raw,rate=48000,channels=2",
            "!",
            enc,
            &format!("bitrate={}", AUDIO_KBPS * 1000),
            "!",
            "aacparse",
            "!",
            "queue",
            "!",
            "sm.audio_0",
        ]));
    }
    a
}

/// gst-launch arguments for one PNG frame from gamescope's node.
pub fn gst_snap_args(g: &Gst, node: &PwNode, out: &Path) -> Vec<String> {
    let mut a = strs(&["-q", "pipewiresrc", &pw_target(g, node), "!", "videoconvert", "!", "pngenc", "snapshot=true", "!", "filesink"]);
    a.push(format!("location={}", out.display()));
    a
}

/// gst-launch arguments that remux joined MPEG-TS into a faststart MP4 (no ffmpeg).
fn gst_remux_args(ts: &Path, out: &Path, audio: bool) -> Vec<String> {
    let mut a = strs(&["-q", "filesrc"]);
    a.push(format!("location={}", ts.display()));
    a.extend(strs(&["!", "tsdemux", "name=d", "mp4mux", "name=m", "faststart=true", "!", "filesink"]));
    a.push(format!("location={}", out.display()));
    a.extend(strs(&["d.", "!", "queue", "!", "h264parse", "!", "m."]));
    if audio {
        a.extend(strs(&["d.", "!", "queue", "!", "aacparse", "!", "m."]));
    }
    a
}

/// The newest segments that cover `want_ms`. `segs` holds each segment's path and the time
/// it was last written (its end). A segment starts where the one before it ended, so the
/// span of segments k..n is end[n] - end[k-1]; one older segment than strictly needed is
/// never required. With too little recorded, everything is returned.
pub fn pick_segments(mut segs: Vec<(PathBuf, u64)>, want_ms: u64) -> Vec<PathBuf> {
    segs.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let Some(last) = segs.last().map(|s| s.1) else { return vec![] };
    let mut k = 0;
    for i in (1..segs.len()).rev() {
        if last.saturating_sub(segs[i - 1].1) >= want_ms {
            k = i;
            break;
        }
    }
    segs.into_iter().skip(k).map(|s| s.0).collect()
}

fn ms_of(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct RecProc {
    kind: ClipBackend,
    secs: u32,
    child: Child,
    lines: Option<Receiver<String>>,
    started: Instant,
    dir: PathBuf,
    audio: bool,
}

struct Recorder {
    proc: Option<RecProc>,
    failures: u32,
    next_try: Option<Instant>,
}

static REC: Mutex<Recorder> = Mutex::new(Recorder { proc: None, failures: 0, next_try: None });

fn backoff(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.min(6)).min(60))
}

/// The replay length the recorder runs with now.
pub fn recording_seconds() -> Option<u32> {
    REC.lock().unwrap().proc.as_ref().map(|p| p.secs)
}

/// The backend recording right now.
pub fn recording() -> Option<ClipBackend> {
    REC.lock().unwrap().proc.as_ref().map(|p| p.kind)
}

fn pidfile() -> PathBuf {
    staging().join("recorder.pid")
}

/// Stops a recorder a crashed launcher left behind.
pub fn cleanup_stale() {
    #[cfg(target_os = "linux")]
    {
        let Some(pid) = std::fs::read_to_string(pidfile()).ok().and_then(|t| t.trim().parse::<i32>().ok()) else { return };
        let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        if pid > 1 && (cmd.contains("gpu-screen-recorder") || cmd.contains("gst-launch")) {
            eprintln!("[share] stopping a recorder left from an earlier run ({pid})");
            // SAFETY: plain syscalls on a process group this launcher created before.
            unsafe {
                libc::kill(-pid, libc::SIGINT);
                std::thread::sleep(Duration::from_millis(800));
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = std::fs::remove_file(pidfile());
}

fn clear_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::create_dir_all(dir);
}

fn spawn(program: &Path, args: &[String], tag: &'static str, want_stdout: bool, env: &[(&str, std::ffi::OsString)]) -> Result<(Child, Option<Receiver<String>>), String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    crate::host_env(&mut cmd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null()).stdout(if want_stdout { Stdio::piped() } else { Stdio::null() }).stderr(Stdio::piped());
    tools::own_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("{tag}: {e}"))?;
    let lines = child.stdout.take().map(|so| {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(so).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        rx
    });
    if let Some(se) = child.stderr.take() {
        std::thread::spawn(move || {
            for l in std::io::BufReader::new(se).lines().map_while(Result::ok).take(200) {
                eprintln!("[share] {tag}: {l}");
            }
        });
    }
    let _ = std::fs::create_dir_all(staging());
    let _ = std::fs::write(pidfile(), child.id().to_string());
    Ok((child, lines))
}

fn start(kind: ClipBackend, secs: u32, p: &Probe) -> Result<RecProc, String> {
    match kind {
        ClipBackend::Gsr => {
            let g = p.gsr.as_ref().ok_or("gpu-screen-recorder is missing")?;
            let dir = staging().join("gsr");
            clear_dir(&dir);
            let args = gsr_replay_args(g, &dir, secs);
            eprintln!("[share] starting {} {}", g.path.display(), args.join(" "));
            // Under gamescope's rootless Xwayland it talks Wayland, to gamescope's socket.
            let wl: Vec<(&str, std::ffi::OsString)> = match (std::env::var_os("WAYLAND_DISPLAY"), std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY")) {
                (None, Some(w)) => vec![("WAYLAND_DISPLAY", w)],
                _ => vec![],
            };
            let (child, lines) = spawn(&g.path, &args, "gsr", true, &wl)?;
            Ok(RecProc { kind, secs, child, lines, started: Instant::now(), dir, audio: true })
        }
        ClipBackend::Gst => {
            let g = p.gst.as_ref().ok_or("gstreamer is missing")?;
            let node = tools::pw_gamescope_node().ok_or("no gamescope video node")?;
            let dir = staging().join("ring");
            clear_dir(&dir);
            let args = gst_record_args(g, &node, &dir, secs);
            eprintln!("[share] starting {} {}", g.launch.display(), args.join(" "));
            let (child, _) = spawn(&g.launch, &args, "gst", false, &[])?;
            Ok(RecProc { kind, secs, child, lines: None, started: Instant::now(), dir, audio: g.audio() })
        }
    }
}

fn stop(mut p: RecProc) {
    eprintln!("[share] stopping {}", p.kind.name());
    tools::stop_child(&mut p.child, Duration::from_secs(3));
    let _ = std::fs::remove_file(pidfile());
    let _ = std::fs::remove_dir_all(&p.dir);
}

/// One supervisor step: start, restart (with backoff) or stop the recorder so it matches
/// `want` (the backend and the replay length).
pub fn tick(want: Option<(ClipBackend, u32)>, p: &Probe) {
    let mut r = REC.lock().unwrap();
    let mut died = None;
    if let Some(rp) = r.proc.as_mut() {
        if let Ok(Some(st)) = rp.child.try_wait() {
            died = Some((rp.kind, st, rp.started.elapsed()));
        }
    }
    if let Some((kind, st, ran)) = died {
        eprintln!("[share] {} exited ({st}) after {} s", kind.name(), ran.as_secs());
        if let Some(rp) = r.proc.take() {
            let _ = std::fs::remove_dir_all(&rp.dir);
        }
        r.failures = if ran < Duration::from_secs(60) { r.failures + 1 } else { 1 };
        r.next_try = Some(Instant::now() + backoff(r.failures));
    }
    let Some((kind, secs)) = want else {
        if let Some(rp) = r.proc.take() {
            stop(rp);
        }
        r.failures = 0;
        r.next_try = None;
        return;
    };
    if r.proc.as_ref().is_some_and(|rp| rp.kind != kind || rp.secs != secs) {
        if let Some(rp) = r.proc.take() {
            stop(rp);
        }
    }
    if r.proc.is_none() && r.next_try.map_or(true, |t| Instant::now() >= t) {
        match start(kind, secs, p) {
            Ok(rp) => r.proc = Some(rp),
            Err(e) => {
                r.failures += 1;
                r.next_try = Some(Instant::now() + backoff(r.failures));
                eprintln!("[share] could not start {}: {e}", kind.name());
            }
        }
    }
}

/// Stops the recorder (launcher exit).
pub fn shutdown() {
    if let Some(rp) = REC.lock().unwrap().proc.take() {
        stop(rp);
    }
}

/// Waits until a file stops growing.
fn settle(path: &Path) {
    let mut last = u64::MAX;
    for _ in 0..25 {
        let n = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if n > 0 && n == last {
            return;
        }
        last = n;
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Saves the ring buffer's last seconds (the replay length) to an MP4 in the staging dir and
/// returns it with that length.
pub fn save_raw(p: &Probe) -> Result<(PathBuf, u32), String> {
    let mut r = REC.lock().unwrap();
    let rp = r.proc.as_mut().ok_or(NOT_RUNNING)?;
    if rp.started.elapsed() < Duration::from_secs(2) {
        return Err("The clip recorder is starting".into());
    }
    let secs = rp.secs;
    match rp.kind {
        ClipBackend::Gsr => {
            let lines = rp.lines.as_ref().ok_or(NOT_RUNNING)?;
            while lines.try_recv().is_ok() {}
            #[cfg(unix)]
            // SAFETY: plain syscall. Only gsr itself gets SIGUSR1, not its helpers.
            unsafe {
                libc::kill(rp.child.id() as i32, libc::SIGUSR1);
            }
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(20) {
                match lines.recv_timeout(Duration::from_millis(250)) {
                    Ok(l) => {
                        let path = PathBuf::from(l.trim());
                        if path.is_absolute() && path.is_file() {
                            settle(&path);
                            return Ok((path, secs));
                        }
                        eprintln!("[share] gsr: {l}");
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
            Err("gpu-screen-recorder saved nothing".into())
        }
        ClipBackend::Gst => {
            let (dir, audio) = (rp.dir.clone(), rp.audio);
            drop(r);
            join_segments(&dir, audio, secs, p).map(|f| (f, secs))
        }
    }
}

fn join_segments(dir: &Path, audio: bool, secs: u32, p: &Probe) -> Result<PathBuf, String> {
    let segs: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "ts"))
        .filter_map(|e| {
            let m = e.metadata().ok().filter(|m| m.len() > 0)?;
            Some((e.path(), ms_of(m.modified().ok()?)))
        })
        .collect();
    let pick = pick_segments(segs, secs as u64 * 1000);
    if pick.is_empty() {
        return Err("Nothing recorded yet".into());
    }
    let stamp = ms_of(SystemTime::now());
    let ts = staging().join(format!("join-{stamp}.ts"));
    let out = staging().join(format!("join-{stamp}.mp4"));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&ts).map_err(|e| e.to_string())?;
        for s in &pick {
            let mut b = std::fs::read(s).unwrap_or_default();
            // The newest segment may stop mid-packet.
            b.truncate(b.len() / 188 * 188);
            f.write_all(&b).map_err(|e| e.to_string())?;
        }
    }
    let res = if let Some(ff) = &p.ffmpeg {
        let args: Vec<String> = vec![
            "-y".into(),
            "-v".into(),
            "error".into(),
            "-i".into(),
            ts.to_string_lossy().into_owned(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "0:a?".into(),
            "-c".into(),
            "copy".into(),
            "-bsf:a".into(),
            "aac_adtstoasc".into(),
            "-movflags".into(),
            "+faststart".into(),
            out.to_string_lossy().into_owned(),
        ];
        tools::run(ff, &args, Duration::from_secs(60))
    } else {
        let g = p.gst.as_ref().ok_or("gstreamer is missing")?;
        tools::run(&g.launch, &gst_remux_args(&ts, &out, audio), Duration::from_secs(60))
    };
    let _ = std::fs::remove_file(&ts);
    match res {
        Ok(o) if o.ok() && out.is_file() => Ok(out),
        Ok(o) => Err(format!("remux failed: {}", o.stderr.trim())),
        Err(e) => Err(e),
    }
}

fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Puts a saved clip at `out`: re-encoded down when it is well over the target bitrate for its
/// length, else remuxed with the index up front (plays while loading). Without ffmpeg the file
/// is moved as it is.
pub fn finish(raw: &Path, out: &Path, secs: u32, p: &Probe) -> Result<(), String> {
    let s = |x: &str| x.to_string();
    let rawp = raw.to_string_lossy().into_owned();
    let outp = out.to_string_lossy().into_owned();
    if let Some(ff) = &p.ffmpeg {
        let size = std::fs::metadata(raw).map(|m| m.len()).unwrap_or(0);
        let rate = format!("{VIDEO_KBPS}k");
        let audio = ["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"].map(s);
        let mut attempts: Vec<Vec<String>> = Vec::new();
        if size > reencode_above(secs) {
            if Path::new("/dev/dri/renderD128").exists() {
                let mut a = ["-y", "-v", "error", "-vaapi_device", "/dev/dri/renderD128", "-i"].map(s).to_vec();
                a.push(rawp.clone());
                a.extend(["-vf", "format=nv12,hwupload", "-c:v", "h264_vaapi", "-b:v", &rate, "-maxrate", &rate].map(s));
                a.extend(audio.clone());
                a.push(outp.clone());
                attempts.push(a);
            }
            let mut a = ["-y", "-v", "error", "-i"].map(s).to_vec();
            a.push(rawp.clone());
            a.extend(["-c:v", "libx264", "-preset", "veryfast", "-b:v", &rate, "-maxrate", &rate, "-bufsize", "8000k"].map(s));
            a.extend(audio);
            a.push(outp.clone());
            attempts.push(a);
        }
        let mut a = ["-y", "-v", "error", "-i"].map(s).to_vec();
        a.push(rawp.clone());
        a.extend(["-map", "0", "-c", "copy", "-movflags", "+faststart"].map(s));
        a.push(outp.clone());
        attempts.push(a);
        for args in attempts {
            match tools::run(ff, &args, Duration::from_secs(180)) {
                Ok(o) if o.ok() && std::fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false) => {
                    let _ = std::fs::remove_file(raw);
                    return Ok(());
                }
                Ok(o) => eprintln!("[share] ffmpeg failed: {}", o.stderr.trim()),
                Err(e) => eprintln!("[share] ffmpeg: {e}"),
            }
            let _ = std::fs::remove_file(out);
        }
    }
    move_file(raw, out)
}

/// Clip size and length through ffprobe, when the MP4 reader could not tell.
pub fn ffprobe_info(p: &Probe, path: &Path) -> Option<(u32, u32, Option<u64>)> {
    let fp = p.ffprobe.as_ref()?;
    let args: Vec<String> = ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height:format=duration", "-of", "json"].iter().map(|x| x.to_string()).chain([path.to_string_lossy().into_owned()]).collect();
    let o = tools::run(fp, &args, Duration::from_secs(10)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&o.stdout).ok()?;
    let w = v["streams"][0]["width"].as_u64()? as u32;
    let h = v["streams"][0]["height"].as_u64()? as u32;
    let d = v["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).map(|d| (d * 1000.0) as u64);
    Some((w, h, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsr_args_follow_the_version() {
        let new = Gsr { path: "/usr/bin/gpu-screen-recorder".into(), version: (5, 3, 0), bm: true, ab: true, ac: true, v: true };
        let a = gsr_replay_args(&new, Path::new("/run/user/1000/rare-share/gsr"), 30).join(" ");
        assert_eq!(a, "-w screen -c mp4 -k h264 -f 60 -r 30 -bm cbr -q 4000 -a default_output -ac aac -ab 128 -v no -o /run/user/1000/rare-share/gsr");
        let old = Gsr { path: "/usr/bin/gpu-screen-recorder".into(), version: (4, 1, 0), bm: false, ab: false, ac: true, v: false };
        let a = gsr_replay_args(&old, Path::new("/x"), 120).join(" ");
        assert_eq!(a, "-w screen -c mp4 -k h264 -f 60 -r 120 -q medium -a default_output -ac aac -o /x");
    }

    fn gst() -> Gst {
        Gst {
            launch: "/usr/bin/gst-launch-1.0".into(),
            pipewiresrc: true,
            target_object: true,
            keepalive: true,
            h264: Some("vah264enc".into()),
            vapostproc: true,
            h264parse: true,
            splitmuxsink: true,
            mpegtsmux: true,
            tsdemux: true,
            mp4mux: true,
            pngenc: true,
            pulsesrc: true,
            aac: Some("avenc_aac".into()),
            aacparse: true,
            ..Default::default()
        }
    }

    #[test]
    fn gst_pipelines() {
        let node = PwNode { id: 77, serial: Some(912), name: "gamescope".into() };
        let a = gst_record_args(&gst(), &node, Path::new("/r"), 30).join(" ");
        assert!(a.starts_with("-e -q splitmuxsink name=sm muxer-factory=mpegtsmux"));
        assert!(a.contains("location=/r/seg%05d.ts max-size-time=1900000000 max-files=20"));
        assert!(a.contains("pipewiresrc target-object=912 do-timestamp=true keepalive-time=100 ! videorate ! video/x-raw,framerate=30/1 ! vapostproc"));
        assert!(a.contains("vah264enc bitrate=4000 rate-control=cbr key-int-max=60 ! h264parse config-interval=-1 ! queue ! sm.video"));
        assert!(a.contains("avenc_aac bitrate=128000 ! aacparse ! queue ! sm.audio_0"));
        let mut old = gst();
        old.target_object = false;
        old.h264 = Some("x264enc".into());
        old.aac = None;
        let a = gst_record_args(&old, &node, Path::new("/r"), 120).join(" ");
        assert!(a.contains("max-files=68"));
        assert!(a.contains("pipewiresrc path=77"));
        assert!(a.contains("x264enc bitrate=4000 speed-preset=ultrafast"));
        assert!(!a.contains("pulsesrc"));
        let s = gst_snap_args(&gst(), &node, Path::new("/t/a.png")).join(" ");
        assert_eq!(s, "-q pipewiresrc target-object=912 ! videoconvert ! pngenc snapshot=true ! filesink location=/t/a.png");
        let r = gst_remux_args(Path::new("/t/j.ts"), Path::new("/t/j.mp4"), false).join(" ");
        assert_eq!(r, "-q filesrc location=/t/j.ts ! tsdemux name=d mp4mux name=m faststart=true ! filesink location=/t/j.mp4 d. ! queue ! h264parse ! m.");
    }

    #[test]
    fn picks_newest_30_seconds() {
        let seg = |i: u32, end: u64| (PathBuf::from(format!("seg{i:05}.ts")), end);
        // 19 segments of 2 s, ending at 2 s, 4 s, ... 38 s; the ring wrapped so names are out of order.
        let segs: Vec<_> = (0..19).map(|i| seg((i + 7) % 19, 2000 * (i as u64 + 1))).collect();
        let pick = pick_segments(segs, 30_000);
        // 38 s - 8 s = 30 s: the segments ending at 10 s .. 38 s.
        assert_eq!(pick.len(), 15);
        assert_eq!(pick.first().unwrap(), &PathBuf::from(format!("seg{:05}.ts", 11)));
        assert_eq!(pick.last().unwrap(), &PathBuf::from(format!("seg{:05}.ts", 25 % 19)));
        // Uneven segments: still at least 30 s.
        let segs = vec![seg(0, 1000), seg(1, 5000), seg(2, 9000), seg(3, 20_000), seg(4, 36_500)];
        assert_eq!(pick_segments(segs, 30_000), vec![PathBuf::from("seg00002.ts"), PathBuf::from("seg00003.ts"), PathBuf::from("seg00004.ts")]);
        // Not enough recorded: all of it.
        let segs = vec![seg(0, 2000), seg(1, 4000)];
        assert_eq!(pick_segments(segs, 30_000).len(), 2);
        assert!(pick_segments(vec![], 30_000).is_empty());
    }

    #[test]
    fn lengths_and_limits() {
        assert_eq!(sane_seconds(30), 30);
        assert_eq!(sane_seconds(0), 15);
        assert_eq!(sane_seconds(100), 120);
        assert_eq!(sane_seconds(9999), 120);
        assert_eq!(max_segments(30), 20);
        assert_eq!(max_segments(15), 12);
        // 30 s at 4128 kbps is 15.5 MB; a quarter more is about 19.3 MB.
        assert_eq!(reencode_above(30), 19_350_000);
        assert!(reencode_above(120) > 4 * 19_000_000);
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(10), Duration::from_secs(60));
    }
}
