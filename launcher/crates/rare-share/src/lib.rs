//! The Share button: screenshots and instant replay clips (the last 15, 30, 60 or 120 seconds),
//! taken with Guide + Y and Guide + Select (see gamepad.rs) or from the UI. Replay is off until
//! it is turned on; saved clips are kept under a size limit, the oldest going first.
//!
//! Screenshots land in ~/Pictures/Rare as WebP, clips in ~/Videos/Rare as MP4 (the XDG user
//! dirs). File names are the capture ids (`shot-20261026-153012-123.webp`); game, size and
//! length are kept in an index in the app data dir (~/.local/share/rare/share/index.json).
//! What each session can capture with is probed at run time (tools.rs); every external tool
//! runs non-interactively, by absolute path, with a timeout.

pub mod clip;
pub mod mp4;
#[cfg(target_os = "linux")]
pub mod portal;
pub mod shot;
pub mod tools;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use rare_core::{Ctx, Event};
use std::sync::Arc;
use tools::{Probe, Session};

const MAX_LIST: usize = 60;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureGame {
    /// Library game id.
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Screenshot,
    Clip,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Screenshot => "shot-",
            Kind::Clip => "clip-",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Kind::Screenshot => "webp",
            Kind::Clip => "mp4",
        }
    }
    fn mime(self) -> &'static str {
        match self {
            Kind::Screenshot => "image/webp",
            Kind::Clip => "video/mp4",
        }
    }
    fn dir(self) -> PathBuf {
        match self {
            Kind::Screenshot => shots_dir(),
            Kind::Clip => clips_dir(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub id: String,
    pub kind: Kind,
    pub path: String,
    pub mime: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
    pub created_ms: u64,
    pub game: Option<CaptureGame>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShotStatus {
    pub available: bool,
    pub method: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClipStatus {
    pub available: bool,
    pub enabled: bool,
    pub recording: bool,
    pub method: Option<String>,
    pub seconds: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShareStatus {
    pub session: Session,
    pub screenshot: ShotStatus,
    pub clips: ClipStatus,
    /// Where screenshots are saved (clips go to the matching Videos folder).
    pub dir: String,
}

#[derive(Serialize, Clone)]
struct ShareError {
    message: String,
}

/// What the index keeps per capture.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
struct Meta {
    width: u32,
    height: u32,
    duration_ms: Option<u64>,
    created_ms: u64,
    game: Option<CaptureGame>,
}

pub fn shots_dir() -> PathBuf {
    dirs::picture_dir().or_else(|| dirs::home_dir().map(|h| h.join("Pictures"))).unwrap_or_else(std::env::temp_dir).join("Rare")
}

pub fn clips_dir() -> PathBuf {
    dirs::video_dir().or_else(|| dirs::home_dir().map(|h| h.join("Videos"))).unwrap_or_else(std::env::temp_dir).join("Rare")
}

fn index_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("rare/share/index.json")
}

static INDEX_LOCK: Mutex<()> = Mutex::new(());

fn index_load() -> HashMap<String, Meta> {
    std::fs::read_to_string(index_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn index_save(m: &HashMap<String, Meta>) {
    let p = index_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("json.tmp");
    if serde_json::to_vec(m).ok().and_then(|b| std::fs::write(&tmp, b).ok()).is_some() {
        let _ = std::fs::rename(tmp, p);
    }
}

fn index_update(f: impl FnOnce(&mut HashMap<String, Meta>)) {
    let _g = INDEX_LOCK.lock().unwrap();
    let mut m = index_load();
    f(&mut m);
    index_save(&m);
}

/// The kind of a capture id, or None for anything that is not one of ours. Ids are
/// "shot-" or "clip-" followed by digits and dashes only, so they can never name a path
/// outside the capture folders.
pub fn parse_id(id: &str) -> Option<Kind> {
    if id.len() > 64 {
        return None;
    }
    let (kind, rest) = match id.strip_prefix("shot-") {
        Some(r) => (Kind::Screenshot, r),
        None => (Kind::Clip, id.strip_prefix("clip-")?),
    };
    let ok = !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || b == b'-') && rest.bytes().next().is_some_and(|b| b.is_ascii_digit());
    ok.then_some(kind)
}

fn file_of(id: &str) -> Option<(Kind, PathBuf)> {
    let kind = parse_id(id)?;
    Some((kind, kind.dir().join(format!("{id}.{}", kind.ext()))))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Local date and time as "YYYYMMDD-HHMMSS-mmm".
fn stamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    #[cfg(unix)]
    {
        // SAFETY: localtime_r writes only into the struct we pass.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let t: libc::time_t = secs as libc::time_t;
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return format!("{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec, ms % 1000);
        }
    }
    // UTC from the civil calendar (days since 1970-01-01).
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{:03}", sod / 3600, sod % 3600 / 60, sod % 60, ms % 1000)
}

/// A fresh id for a capture taken at `ms`, not used by any file yet.
fn new_id(kind: Kind, ms: u64) -> String {
    let base = format!("{}{}", kind.prefix(), stamp(ms));
    let mut id = base.clone();
    let mut n = 2;
    while kind.dir().join(format!("{id}.{}", kind.ext())).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn capture_of(id: &str, kind: Kind, path: &Path, meta: &Meta) -> Capture {
    Capture {
        id: id.to_string(),
        kind,
        path: path.to_string_lossy().into_owned(),
        mime: kind.mime().into(),
        bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        width: meta.width,
        height: meta.height,
        duration_ms: meta.duration_ms,
        created_ms: meta.created_ms,
        game: meta.game.clone(),
    }
}

// ---- probe cache ----

static PROBE: Mutex<Option<(Instant, Probe)>> = Mutex::new(None);
const PROBE_TTL: Duration = Duration::from_secs(60);

fn probe() -> Probe {
    {
        let g = PROBE.lock().unwrap();
        if let Some((at, p)) = g.as_ref() {
            if at.elapsed() < PROBE_TTL {
                return p.clone();
            }
        }
    }
    let t0 = Instant::now();
    let p = tools::probe();
    eprintln!(
        "[share] probe in {} ms: session {:?}, screenshots {:?}, clips {:?}, ffmpeg {}",
        t0.elapsed().as_millis(),
        p.session,
        tools::shot_backends(&p).iter().map(|b| b.name()).collect::<Vec<_>>(),
        tools::clip_backend(&p).map(|b| b.name()),
        p.ffmpeg.is_some()
    );
    *PROBE.lock().unwrap() = Some((Instant::now(), p.clone()));
    p
}

// ---- app state ----

/// What the share code needs from the front end and the rest of the launcher.
pub trait ShareHost: Send + Sync {
    /// Instant replay is switched on in the settings.
    fn clips_enabled(&self) -> bool;
    /// The replay length in seconds (one of `clip::LENGTHS`).
    fn replay_seconds(&self) -> u32 {
        clip::SECONDS
    }
    /// The space saved clips may take; the oldest are deleted past it. None: no limit.
    fn clips_cap_bytes(&self) -> Option<u64> {
        None
    }
    /// Run ids of the games running now.
    fn running(&self) -> Vec<String>;
    /// The library title of a game id.
    fn title_of(&self, id: &str) -> Option<String>;
    /// Rumble on a capture is switched on.
    fn rumble_enabled(&self) -> bool;
    fn rumble(&self);
    /// The launcher itself is the window in front (its own toast shows).
    fn launcher_front(&self) -> bool;
    /// A HUD toast over whatever is in front.
    fn toast(&self, text: &str);
}

type Host<'a> = &'a dyn ShareHost;

static HOST_ENV: std::sync::OnceLock<fn(&mut std::process::Command)> = std::sync::OnceLock::new();

/// The environment external tools run with (the user's session, not the AppImage's); set once at start.
pub fn set_host_env(f: fn(&mut std::process::Command)) {
    let _ = HOST_ENV.set(f);
}

pub(crate) fn host_env(cmd: &mut std::process::Command) {
    if let Some(f) = HOST_ENV.get() {
        f(cmd);
    }
}

fn game_running(host: Host) -> bool {
    !host.running().is_empty()
}

/// The game on screen: the running game (the first by id when several run).
fn current_game(host: Host) -> Option<CaptureGame> {
    let mut ids = host.running();
    ids.sort();
    let id = ids.into_iter().next()?;
    let name = host.title_of(&id).unwrap_or_else(|| id.split_once(':').map(|(_, n)| n.to_string()).unwrap_or_else(|| id.clone()));
    Some(CaptureGame { id, name })
}

pub fn status(host: Host) -> ShareStatus {
    let p = probe();
    let shots = tools::shot_backends(&p);
    let clip = tools::clip_backend(&p);
    let recording = clip::recording();
    ShareStatus {
        session: p.session,
        screenshot: ShotStatus { available: !shots.is_empty(), method: shots.first().map(|b| b.name().to_string()) },
        clips: ClipStatus {
            available: clip.is_some(),
            enabled: host.clips_enabled(),
            recording: recording.is_some(),
            method: recording.or(clip).map(|b| b.name().to_string()),
            seconds: clip::sane_seconds(host.replay_seconds()),
        },
        dir: shots_dir().to_string_lossy().into_owned(),
    }
}

// ---- captures ----

static SHOT_BUSY: AtomicBool = AtomicBool::new(false);
static CLIP_BUSY: AtomicBool = AtomicBool::new(false);

struct Busy(&'static AtomicBool);

impl Busy {
    fn take(flag: &'static AtomicBool) -> Option<Busy> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Busy(flag))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// A picture sent from the owner's phone (PNG, JPEG, WebP, GIF): into the gallery as a screenshot, so Share
/// shows it and can post it like any capture.
pub fn import_picture(data: &[u8]) -> Result<Capture, String> {
    let created_ms = now_ms();
    let enc = shot::to_webp(data, shot::Rotate::None).map_err(|e| format!("That picture could not be read ({e})"))?;
    let dir = shots_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not save the picture ({e})"))?;
    let id = new_id(Kind::Screenshot, created_ms);
    let path = dir.join(format!("{id}.webp"));
    let part = dir.join(format!(".{id}.part"));
    std::fs::write(&part, &enc.webp).and_then(|_| std::fs::rename(&part, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("Could not save the picture ({e})")
    })?;
    let meta = Meta { width: enc.width, height: enc.height, duration_ms: None, created_ms, game: None };
    index_update(|m| {
        m.insert(id.clone(), meta.clone());
    });
    Ok(capture_of(&id, Kind::Screenshot, &path, &meta))
}

pub fn take_screenshot(host: Host) -> Result<Capture, String> {
    let _busy = Busy::take(&SHOT_BUSY).ok_or("Already taking a screenshot")?;
    let p = probe();
    let game = current_game(host);
    let created_ms = now_ms();
    let stage = clip::staging();
    let _ = std::fs::create_dir_all(&stage);
    let tmp = stage.join(format!("grab-{created_ms}.png"));
    let (raw, _) = shot::grab_any(&p, &tmp)?;
    let (w, h) = image::load_from_memory(&raw).map(|i| (i.width(), i.height())).unwrap_or((0, 0));
    let over = std::env::var("RARE_SHARE_ROTATE").ok();
    let rot = shot::rotation_for(p.session, w, h, p.orientation.as_deref(), over.as_deref());
    let enc = shot::to_webp(&raw, rot).map_err(|e| {
        eprintln!("[share] screenshot convert: {e}");
        "Could not take a screenshot".to_string()
    })?;
    let dir = shots_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        eprintln!("[share] {}: {e}", dir.display());
        "Could not save the screenshot".to_string()
    })?;
    let id = new_id(Kind::Screenshot, created_ms);
    let path = dir.join(format!("{id}.webp"));
    let part = dir.join(format!(".{id}.part"));
    std::fs::write(&part, &enc.webp).and_then(|_| std::fs::rename(&part, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        eprintln!("[share] write {}: {e}", path.display());
        "Could not save the screenshot".to_string()
    })?;
    let meta = Meta { width: enc.width, height: enc.height, duration_ms: None, created_ms, game };
    index_update(|m| {
        m.insert(id.clone(), meta.clone());
    });
    eprintln!("[share] saved {} ({}x{}, {} KB, {:?})", path.display(), enc.width, enc.height, enc.webp.len() / 1024, rot);
    Ok(capture_of(&id, Kind::Screenshot, &path, &meta))
}

/// Saves a picture the launcher made itself (a summary card) as a screenshot capture, so Share
/// can post it like any other.
pub fn import_png(png: &[u8], game: Option<CaptureGame>) -> Result<Capture, String> {
    let created_ms = now_ms();
    let enc = shot::to_webp(png, shot::Rotate::None).map_err(|e| {
        eprintln!("[share] import convert: {e}");
        "Could not save the picture".to_string()
    })?;
    let dir = shots_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        eprintln!("[share] {}: {e}", dir.display());
        "Could not save the picture".to_string()
    })?;
    let id = new_id(Kind::Screenshot, created_ms);
    let path = dir.join(format!("{id}.webp"));
    let part = dir.join(format!(".{id}.part"));
    std::fs::write(&part, &enc.webp).and_then(|_| std::fs::rename(&part, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        eprintln!("[share] write {}: {e}", path.display());
        "Could not save the picture".to_string()
    })?;
    let meta = Meta { width: enc.width, height: enc.height, duration_ms: None, created_ms, game };
    index_update(|m| {
        m.insert(id.clone(), meta.clone());
    });
    Ok(capture_of(&id, Kind::Screenshot, &path, &meta))
}

pub fn save_clip(host: Host) -> Result<Capture, String> {
    let p = probe();
    if tools::clip_backend(&p).is_none() {
        return Err("Clips are not available here".into());
    }
    if !host.clips_enabled() {
        return Err(REPLAY_OFF.into());
    }
    if !game_running(host) {
        return Err("No game is running".into());
    }
    let _busy = Busy::take(&CLIP_BUSY).ok_or("Already saving a clip")?;
    let game = current_game(host);
    let created_ms = now_ms();
    let (raw, secs) = clip::save_raw(&p).map_err(|e| {
        eprintln!("[share] clip: {e}");
        if e == clip::NOT_RUNNING || e.starts_with("The clip recorder") { e } else { "Could not save the clip".to_string() }
    })?;
    let dir = clips_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        eprintln!("[share] {}: {e}", dir.display());
        "Could not save the clip".to_string()
    })?;
    let id = new_id(Kind::Clip, created_ms);
    let path = dir.join(format!("{id}.mp4"));
    clip::finish(&raw, &path, secs, &p).map_err(|e| {
        eprintln!("[share] clip finish: {e}");
        "Could not save the clip".to_string()
    })?;
    let mut info = mp4::parse_file(&path);
    // A clip grabbed from the raw portrait scanout in Game Mode is shown turned, by its matrix.
    if let Some(i) = info.as_ref().filter(|i| i.rotation == 0 && i.width > 0) {
        let over = std::env::var("RARE_SHARE_ROTATE").ok();
        let deg = match shot::rotation_for(p.session, i.width, i.height, p.orientation.as_deref(), over.as_deref()) {
            shot::Rotate::Ccw => 270,
            shot::Rotate::Cw => 90,
            shot::Rotate::None => 0,
        };
        if deg != 0 {
            match mp4::set_rotation(&path, deg) {
                Ok(()) => info = mp4::parse_file(&path),
                Err(e) => eprintln!("[share] clip rotate: {e}"),
            }
        }
    }
    let (width, height, duration_ms) = match info.filter(|i| i.width > 0) {
        Some(i) => {
            let (w, h) = i.display_size();
            (w, h, i.duration_ms)
        }
        None => clip::ffprobe_info(&p, &path).unwrap_or((0, 0, None)),
    };
    let meta = Meta { width, height, duration_ms, created_ms, game };
    index_update(|m| {
        m.insert(id.clone(), meta.clone());
    });
    eprintln!("[share] saved {} ({width}x{height}, {:?} ms)", path.display(), duration_ms);
    if let Some(cap) = host.clips_cap_bytes() {
        let gone = prune_clips(cap, &id);
        if !gone.is_empty() {
            eprintln!("[share] over the clip limit: deleted {} older clip(s)", gone.len());
        }
    }
    Ok(capture_of(&id, Kind::Clip, &path, &meta))
}

pub const REPLAY_OFF: &str = "Instant replay is off. Turn it on in Quick settings.";

/// Bytes the saved clips take now.
pub fn clips_bytes() -> u64 {
    clip_files().iter().map(|c| c.1).sum()
}

/// Our clips on disk: (id, bytes, created ms), oldest first.
fn clip_files() -> Vec<(String, u64, u64)> {
    let index = index_load();
    let mut v: Vec<(String, u64, u64)> = std::fs::read_dir(clips_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let path = e.path();
                    if path.extension().and_then(|x| x.to_str()) != Some("mp4") {
                        return None;
                    }
                    let id = path.file_stem()?.to_str()?.to_string();
                    (parse_id(&id) == Some(Kind::Clip)).then_some(())?;
                    let m = e.metadata().ok()?;
                    let created = index.get(&id).map(|x| x.created_ms).filter(|c| *c > 0).unwrap_or_else(|| m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0));
                    Some((id, m.len(), created))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    v
}

/// Which clips go so the rest fit in `cap` bytes: the oldest first, never `keep`.
pub fn over_cap(files: &[(String, u64, u64)], cap: u64, keep: &str) -> Vec<String> {
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let mut out = Vec::new();
    for (id, bytes, _) in files {
        if total <= cap {
            break;
        }
        if id == keep {
            continue;
        }
        total = total.saturating_sub(*bytes);
        out.push(id.clone());
    }
    out
}

/// Deletes the oldest clips until the rest fit in `cap` bytes (never `keep`, the one just
/// saved). Returns the ids deleted.
pub fn prune_clips(cap: u64, keep: &str) -> Vec<String> {
    let gone = over_cap(&clip_files(), cap, keep);
    for id in &gone {
        let _ = delete(id);
    }
    gone
}

/// Captures on disk, newest first, at most 60. Files without an index entry (copied in, or
/// the index was lost) are read for their size.
pub fn list() -> Vec<Capture> {
    let _g = INDEX_LOCK.lock().unwrap();
    let mut index = index_load();
    let mut out = Vec::new();
    for kind in [Kind::Screenshot, Kind::Clip] {
        let Ok(rd) = std::fs::read_dir(kind.dir()) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some(kind.ext()) {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else { continue };
            if parse_id(&id) != Some(kind) {
                continue;
            }
            let meta = index.get(&id).cloned().unwrap_or_else(|| {
                let created_ms = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
                let mut m = Meta { created_ms, ..Default::default() };
                match kind {
                    Kind::Screenshot => {
                        let head = std::fs::read(&path).ok().unwrap_or_default();
                        if let Some((w, h)) = shot::webp_size(&head) {
                            (m.width, m.height) = (w, h);
                        }
                    }
                    Kind::Clip => {
                        if let Some(i) = mp4::parse_file(&path) {
                            (m.width, m.height) = i.display_size();
                            m.duration_ms = i.duration_ms;
                        }
                    }
                }
                m
            });
            out.push(capture_of(&id, kind, &path, &meta));
        }
    }
    let before = index.len();
    index.retain(|id, _| out.iter().any(|c| &c.id == id));
    if index.len() != before {
        index_save(&index);
    }
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then_with(|| b.id.cmp(&a.id)));
    out.truncate(MAX_LIST);
    out
}

pub fn read(id: &str) -> Result<Vec<u8>, String> {
    let (_, path) = file_of(id).ok_or("Unknown capture")?;
    std::fs::read(&path).map_err(|_| "Capture not found".to_string())
}

pub fn delete(id: &str) -> Result<(), String> {
    let (_, path) = file_of(id).ok_or("Unknown capture")?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not delete it".into()),
    }
    index_update(|m| {
        m.remove(id);
    });
    Ok(())
}

// ---- controller chord ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    Screenshot,
    Clip,
}

fn emit_error(ctx: &Ctx, message: String) {
    ctx.send(Event::Other { name: "share-error".into(), payload: serde_json::to_value(ShareError { message }).unwrap_or_default() });
}

/// Runs a chord capture on its own thread (never on the pad thread). Success sends
/// `share-captured` and a rumble, since the launcher is behind the game; failure sends
/// `share-error`.
pub fn chord(ctx: &Ctx, host: &Arc<dyn ShareHost>, c: Chord) {
    let busy = match c {
        Chord::Screenshot => &SHOT_BUSY,
        Chord::Clip => &CLIP_BUSY,
    };
    if busy.load(Ordering::Acquire) {
        return;
    }
    let (ctx, host) = (ctx.clone(), host.clone());
    std::thread::spawn(move || {
        let res = match c {
            Chord::Screenshot => take_screenshot(&*host),
            Chord::Clip => save_clip(&*host),
        };
        match res {
            Ok(cap) => {
                if host.rumble_enabled() {
                    host.rumble();
                }
                // Over a game or app the launcher's own toast can't be seen: the HUD says it.
                if !host.launcher_front() {
                    host.toast(if c == Chord::Screenshot { "Screenshot saved" } else { "Clip saved" });
                }
                ctx.send(Event::Other { name: "share-captured".into(), payload: serde_json::to_value(cap).unwrap_or_default() });
            }
            Err(e) => {
                eprintln!("[share] {c:?}: {e}");
                emit_error(&ctx, e);
            }
        }
    });
}

// ---- recorder supervisor ----

/// Starts the thread that keeps the clip recorder running exactly while a game runs and instant
/// replay is on (the recorder costs battery, so it never runs on the home screen).
pub fn start(host: Arc<dyn ShareHost>) {
    std::thread::spawn(move || {
        clip::cleanup_stale();
        let _ = std::fs::create_dir_all(clip::staging());
        loop {
            let secs = clip::sane_seconds(host.replay_seconds());
            let want = if host.clips_enabled() && game_running(&*host) { tools::clip_backend(&probe()).map(|b| (b, secs)) } else { None };
            clip::tick(want, &probe_cached_or_default());
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
}

/// The cached probe without probing again (the supervisor ticks often).
fn probe_cached_or_default() -> Probe {
    PROBE.lock().unwrap().as_ref().map(|(_, p)| p.clone()).unwrap_or_default()
}

/// Stops the recorder; called when the launcher exits.
pub fn shutdown() {
    clip::shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_validated() {
        assert_eq!(parse_id("shot-20261026-153012-123"), Some(Kind::Screenshot));
        assert_eq!(parse_id("clip-20261026-153012-123-2"), Some(Kind::Clip));
        for bad in ["", "shot-", "shot--1", "shot-../../etc/passwd", "shot-1/2", "clip-1.mp4", "other-123", "SHOT-1", "shot-1\0", "clip-1 2", &format!("shot-{}", "1".repeat(70))] {
            assert_eq!(parse_id(bad), None, "{bad:?}");
        }
        let (k, p) = file_of("clip-20261026-153012-123").unwrap();
        assert_eq!(k, Kind::Clip);
        assert!(p.ends_with("Rare/clip-20261026-153012-123.mp4"));
        assert!(file_of("shot-../x").is_none());
        assert!(read("../../etc/passwd").is_err());
        assert!(delete("shot-/etc").is_err());
    }

    #[test]
    fn stamps_look_right() {
        let s = stamp(1_793_000_000_123);
        assert_eq!(s.len(), "20261026-153012-123".len());
        assert!(s.ends_with("-123"));
        assert!(parse_id(&format!("shot-{s}")).is_some());
    }

    #[test]
    fn the_oldest_clips_go_first() {
        let f = |id: &str, mb: u64, t: u64| (id.to_string(), mb << 20, t);
        let files = vec![f("clip-1", 20, 1), f("clip-2", 20, 2), f("clip-3", 20, 3), f("clip-4", 20, 4)];
        assert!(over_cap(&files, 80 << 20, "clip-4").is_empty());
        assert_eq!(over_cap(&files, 50 << 20, "clip-4"), vec!["clip-1", "clip-2"]);
        // The one just saved stays even when it alone is over the limit.
        assert_eq!(over_cap(&files, 10 << 20, "clip-4"), vec!["clip-1", "clip-2", "clip-3"]);
        assert_eq!(over_cap(&[f("clip-9", 50, 1)], 10 << 20, "clip-9"), Vec::<String>::new());
    }

    #[test]
    fn capture_json_shape() {
        let meta = Meta { width: 1920, height: 1200, duration_ms: Some(30_000), created_ms: 5, game: Some(CaptureGame { id: "steam:1".into(), name: "Game".into() }) };
        let c = capture_of("clip-1", Kind::Clip, Path::new("/nonexistent/clip-1.mp4"), &meta);
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["kind"], "clip");
        assert_eq!(v["mime"], "video/mp4");
        assert_eq!(v["durationMs"], 30_000);
        assert_eq!(v["createdMs"], 5);
        assert_eq!(v["game"]["name"], "Game");
        let st = ShareStatus {
            session: Session::Gamescope,
            screenshot: ShotStatus { available: true, method: Some("gamescopectl".into()) },
            clips: ClipStatus { available: true, enabled: true, recording: false, method: None, seconds: 30 },
            dir: "/x".into(),
        };
        let v = serde_json::to_value(&st).unwrap();
        assert_eq!(v["session"], "gamescope");
        assert_eq!(v["clips"]["seconds"], 30);
        assert_eq!(v["screenshot"]["method"], "gamescopectl");
    }
}
