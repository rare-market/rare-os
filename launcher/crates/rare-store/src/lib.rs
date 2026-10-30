//! Rare Store installs: signed download, hash check against the chain, safe unpack into
//! `~/Games/Rare/<slug>/<version>`, `current` pointing at it, and `install.json` beside it.
//!
//! Layout of `~/Games/Rare/<slug>/`:
//! - `.download`          the archive while it downloads (deleted after the install)
//! - `.<version>.partial` the unpack in progress
//! - `<version>/`         the unpacked build, with `rare.json` at its root
//! - `current`            symlink to `<version>` (unix)
//! - `install.json`       what was installed, for whom, and when the licence was last checked

pub mod unpack;
pub mod update;
pub mod verify;

use rare_core::{Ctx, Error, Event};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unpack::Runtime;
pub use update::UpdateDecision;
use verify::{is_hex64, valid_slug, HashingWriter, HASH_MISMATCH};

const API: &str = "https://rare.market/v1/store";
const UA: &str = "RareLauncher/0.1";
const PLATFORMS: [&str; 2] = ["linux-x86_64", "windows-x86_64"];
const CHAIN_MISMATCH: &str = "The download doesn't match the release on chain. Nothing was installed.";

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `~/Games/Rare`.
pub fn rare_root() -> Result<PathBuf, String> {
    dirs::home_dir().map(|h| h.join("Games").join("Rare")).ok_or_else(|| "Couldn't find your home folder.".into())
}

/// Every Rare folder: `~/Games/Rare` first, then `Games/Rare` on mounted drives (an SD card a game
/// was moved to from Settings, Storage). The layout inside each is the same.
pub fn rare_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = rare_root().into_iter().collect();
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    for line in mounts.lines() {
        let Some(m) = line.split_whitespace().nth(1) else { continue };
        let m = m.replace("\\040", " ");
        if ["/run/media/", "/media/", "/mnt/", "/var/mnt/"].iter().any(|b| m.starts_with(b)) {
            let r = Path::new(&m).join("Games").join("Rare");
            if r.is_dir() && !v.contains(&r) {
                v.push(r);
            }
        }
    }
    v
}

/// The Rare folder holding `slug` (where it was installed or moved to), else `~/Games/Rare`.
pub fn root_of(slug: &str) -> Result<PathBuf, String> {
    for r in rare_roots() {
        if r.join(slug).join("install.json").is_file() {
            return Ok(r);
        }
    }
    rare_root()
}

/// Accepts a u64 as a JSON number or a decimal string (Sui JSON often sends strings).
fn de_u64<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Number(n) => n.as_u64().ok_or_else(|| serde::de::Error::custom("expected a whole number")),
        serde_json::Value::String(s) => s.trim().parse().map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom("expected a number")),
    }
}

/// A string or a number, as a string.
fn de_text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Null => Ok(String::new()),
        v => Ok(v.to_string()),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Images {
    pub logo: Option<String>,
    pub banner: Option<String>,
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub slug: String,
    #[serde(default)]
    pub name: String,
    pub platform: String,
    pub address: String,
    pub message: String,
    pub signature: String,
    pub expected_sha256: String,
    #[serde(deserialize_with = "de_u64")]
    pub expected_size: u64,
    #[serde(deserialize_with = "de_u64")]
    pub version: u64,
    #[serde(default, deserialize_with = "de_text")]
    pub label: String,
    #[serde(deserialize_with = "de_u64")]
    pub edition: u64,
    #[serde(default)]
    pub launch_check: bool,
    #[serde(default)]
    pub images: Images,
}

/// `~/Games/Rare/<slug>/install.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    pub slug: String,
    pub name: String,
    pub version: u64,
    #[serde(default)]
    pub label: String,
    pub edition: u64,
    #[serde(default)]
    pub licence_id: String,
    pub platform: String,
    pub sha256: String,
    pub size: u64,
    pub runtime: Runtime,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub launch_check: bool,
    #[serde(default)]
    pub images: Images,
    pub installed_ms: u64,
    pub checked_ms: u64,
    #[serde(default)]
    pub owner: String,
    /// Name of the version folder (what `current` points at; used directly where there are no symlinks).
    pub version_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    #[serde(flatten)]
    pub record: InstallRecord,
    /// "rare:<slug>", the library id.
    pub id: String,
    /// `~/Games/Rare/<slug>`.
    pub dir: String,
}

/// The API's answer to `POST /v1/store/downloads`.
#[derive(Debug, Clone, Deserialize)]
struct DownloadAnswer {
    url: String,
    sha256: String,
    #[serde(deserialize_with = "de_u64")]
    size: u64,
    #[serde(deserialize_with = "de_u64")]
    version: u64,
    #[serde(default, deserialize_with = "de_text")]
    label: String,
    #[serde(deserialize_with = "de_u64")]
    edition: u64,
    #[serde(default, deserialize_with = "de_text")]
    licence_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress<'a> {
    slug: &'a str,
    phase: &'a str,
    done: u64,
    total: u64,
    message: Option<String>,
}

/// One install per slug at a time.
static BUSY: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

struct SlugGuard(String);

impl SlugGuard {
    fn take(slug: &str) -> Result<Self, String> {
        if !BUSY.lock().unwrap().insert(slug.to_string()) {
            return Err("This game is already installing.".into());
        }
        Ok(SlugGuard(slug.to_string()))
    }
}

impl Drop for SlugGuard {
    fn drop(&mut self) {
        BUSY.lock().unwrap().remove(&self.0);
    }
}

fn check_slug(slug: &str) -> Result<(), String> {
    if valid_slug(slug) {
        Ok(())
    } else {
        Err("That isn't a valid game id.".into())
    }
}

fn check_platform(platform: &str) -> Result<(), String> {
    if PLATFORMS.contains(&platform) {
        Ok(())
    } else {
        Err("That platform isn't supported.".into())
    }
}

/// `<root>/<slug>`, refusing a slug folder that is a symlink.
fn slug_dir(root: &Path, slug: &str) -> Result<PathBuf, String> {
    check_slug(slug)?;
    let d = root.join(slug);
    if fs::symlink_metadata(&d).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!("{} is a link, not a folder. Remove it and try again.", d.display()));
    }
    Ok(d)
}

/// The folder the game runs from: `current` with symlinks, the version folder without.
fn run_dir(slug_dir: &Path, rec: &InstallRecord) -> PathBuf {
    if cfg!(unix) {
        slug_dir.join("current")
    } else {
        slug_dir.join(&rec.version_dir)
    }
}

/// "<code>: <message>" from the API's `{"error":{"code","message"}}`.
fn api_error(status: u16, body: &serde_json::Value) -> String {
    let code = body.pointer("/error/code").and_then(|v| v.as_str()).unwrap_or("");
    let msg = body.pointer("/error/message").and_then(|v| v.as_str()).unwrap_or("");
    match (code.is_empty(), msg.is_empty()) {
        (false, false) => format!("{code}: {msg}"),
        (false, true) => code.to_string(),
        (true, false) => msg.to_string(),
        (true, true) => format!("The store answered with HTTP {status}."),
    }
}

fn api_post(path: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let res = ureq::post(&format!("{API}{path}")).set("User-Agent", UA).timeout(Duration::from_secs(30)).send_json(body);
    match res {
        Ok(r) => r.into_json().map_err(|e| format!("The store sent an answer the launcher can't read: {e}")),
        Err(ureq::Error::Status(code, r)) => {
            let body: serde_json::Value = r.into_json().unwrap_or(serde_json::Value::Null);
            Err(api_error(code, &body))
        }
        Err(e) => Err(format!("Couldn't reach rare.market: {e}")),
    }
}

/// The API's download must be exactly the release the UI read on chain.
fn check_answer(req: &InstallRequest, a: &DownloadAnswer) -> Result<(), String> {
    let ok = a.sha256.eq_ignore_ascii_case(&req.expected_sha256)
        && a.size == req.expected_size
        && a.version == req.version
        && a.edition == req.edition
        && a.url.starts_with("https://");
    if ok {
        Ok(())
    } else {
        Err(CHAIN_MISMATCH.into())
    }
}

fn check_request(req: &InstallRequest) -> Result<(), String> {
    check_slug(&req.slug)?;
    check_platform(&req.platform)?;
    if !is_hex64(&req.expected_sha256) {
        return Err("The release on chain has no valid hash.".into());
    }
    Ok(())
}

/// At most ~5 progress events a second.
struct Throttle(Option<Instant>);

impl Throttle {
    fn ready(&mut self) -> bool {
        if self.0.is_some_and(|t| t.elapsed() < Duration::from_millis(200)) {
            return false;
        }
        self.0 = Some(Instant::now());
        true
    }
}

type Emit<'a> = &'a dyn Fn(&str, u64, u64, Option<String>);

/// Streams `url` to `path`, hashing as it goes. Deletes the file on any failure.
fn download(url: &str, path: &Path, expected_sha256: &str, expected_size: u64, emit: Emit) -> Result<(), String> {
    let r = download_inner(url, path, expected_sha256, expected_size, emit);
    if r.is_err() {
        let _ = fs::remove_file(path);
    }
    r
}

fn download_inner(url: &str, path: &Path, expected_sha256: &str, expected_size: u64, emit: Emit) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(30)).timeout_read(Duration::from_secs(60)).user_agent(UA).build();
    let resp = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("The download failed with HTTP {code}. Try again."),
        e => format!("The download failed: {e}"),
    })?;
    if resp.header("Content-Length").and_then(|v| v.trim().parse::<u64>().ok()).is_some_and(|n| n != expected_size) {
        return Err(HASH_MISMATCH.into());
    }
    let file = File::create(path).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    let mut w = HashingWriter::new(BufWriter::with_capacity(1 << 20, file));
    let mut body = resp.into_reader().take(expected_size.saturating_add(1));
    let mut buf = vec![0u8; 256 * 1024];
    let mut throttle = Throttle(None);
    throttle.ready();
    emit("download", 0, expected_size, None);
    loop {
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("The download stopped: {e}")),
        };
        if w.count() + n as u64 > expected_size {
            return Err(HASH_MISMATCH.into());
        }
        w.write_all(&buf[..n]).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
        if throttle.ready() {
            emit("download", w.count(), expected_size, None);
        }
    }
    let (bw, digest, size) = w.finish();
    let f = bw.into_inner().map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    f.sync_all().map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    emit("download", size, expected_size, None);
    emit("verify", size, expected_size, None);
    verify::check_download(&digest, size, expected_sha256, expected_size)
}

#[cfg(unix)]
fn point_current(slug_dir: &Path, version_dir: &str) -> std::io::Result<()> {
    let tmp = slug_dir.join(".current.tmp");
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(version_dir, &tmp)?;
    let cur = slug_dir.join("current");
    if fs::symlink_metadata(&cur).is_ok_and(|m| m.is_dir()) {
        fs::remove_dir_all(&cur)?;
    }
    fs::rename(&tmp, &cur)
}

#[cfg(not(unix))]
fn point_current(_slug_dir: &Path, _version_dir: &str) -> std::io::Result<()> {
    Ok(())
}

fn write_record(slug_dir: &Path, rec: &InstallRecord) -> Result<(), String> {
    let tmp = slug_dir.join(".install.json.tmp");
    let json = serde_json::to_vec_pretty(rec).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).and_then(|_| fs::rename(&tmp, slug_dir.join("install.json"))).map_err(|e| format!("Couldn't save install.json: {e}"))
}

fn read_record(slug_dir: &Path) -> Option<InstallRecord> {
    serde_json::from_slice(&fs::read(slug_dir.join("install.json")).ok()?).ok()
}

fn to_installed(slug_dir: &Path, record: InstallRecord) -> Installed {
    Installed { id: format!("rare:{}", record.slug), dir: slug_dir.to_string_lossy().into_owned(), record }
}

/// Unpacks a verified archive into `<slug_dir>/<version>`, points `current` at it, writes
/// `install.json` and removes the download and older versions.
fn place_build(slug_dir: &Path, archive: &Path, req: &InstallRequest, licence_id: &str, now: u64) -> Result<InstallRecord, String> {
    let version_dir = req.version.to_string();
    let partial = slug_dir.join(format!(".{version_dir}.partial"));
    let _ = fs::remove_dir_all(&partial);
    unpack::unpack_tar_zst(archive, &partial)?;
    let (rj, _) = match unpack::read_rare_json(&partial) {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&partial);
            return Err(e);
        }
    };
    let target = slug_dir.join(&version_dir);
    let old = slug_dir.join(format!(".{version_dir}.old"));
    if fs::symlink_metadata(&target).is_ok() {
        let _ = fs::remove_dir_all(&old);
        fs::rename(&target, &old).map_err(|e| format!("Couldn't replace {}: {e}", target.display()))?;
    }
    fs::rename(&partial, &target).map_err(|e| format!("Couldn't move the build into place: {e}"))?;
    point_current(slug_dir, &version_dir).map_err(|e| format!("Couldn't point current at the new build: {e}"))?;
    let rec = InstallRecord {
        slug: req.slug.clone(),
        name: if req.name.trim().is_empty() { rj.name.clone() } else { req.name.clone() },
        version: req.version,
        label: req.label.clone(),
        edition: req.edition,
        licence_id: licence_id.to_string(),
        platform: req.platform.clone(),
        sha256: req.expected_sha256.to_ascii_lowercase(),
        size: req.expected_size,
        runtime: rj.runtime,
        exec: rj.exec,
        args: rj.args,
        launch_check: req.launch_check,
        images: req.images.clone(),
        installed_ms: now,
        checked_ms: now,
        owner: req.address.clone(),
        version_dir: version_dir.clone(),
    };
    write_record(slug_dir, &rec)?;
    // The download, leftovers and other versions go.
    if let Ok(rd) = fs::read_dir(slug_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let stale = name.starts_with('.') || (name != version_dir && !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()));
            if !stale {
                continue;
            }
            let p = e.path();
            if fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) {
                let _ = fs::remove_dir_all(&p);
            } else {
                let _ = fs::remove_file(&p);
            }
        }
    }
    Ok(rec)
}

fn install_blocking(req: InstallRequest, emit: Emit) -> Result<Installed, String> {
    check_request(&req)?;
    let _guard = SlugGuard::take(&req.slug)?;
    let answer: DownloadAnswer = serde_json::from_value(api_post(
        "/downloads",
        serde_json::json!({
            "address": req.address,
            "slug": req.slug,
            "platform": req.platform,
            "message": req.message,
            "signature": req.signature,
        }),
    )?)
    .map_err(|e| format!("The store sent an answer the launcher can't read: {e}"))?;
    check_answer(&req, &answer)?;
    let mut req = req;
    if req.label.trim().is_empty() {
        req.label = answer.label.clone();
    }
    let root = root_of(&req.slug)?;
    let dir = slug_dir(&root, &req.slug)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let archive = dir.join(".download");
    download(&answer.url, &archive, &req.expected_sha256, req.expected_size, emit)?;
    emit("unpack", 0, req.expected_size, None);
    let placed = place_build(&dir, &archive, &req, &answer.licence_id, now_ms());
    let _ = fs::remove_file(&archive);
    let rec = placed?;
    emit("done", req.expected_size, req.expected_size, None);
    Ok(to_installed(&dir, rec))
}

fn installed_in(root: &Path) -> Vec<Installed> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !valid_slug(&name) || !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = e.path();
        if let Some(rec) = read_record(&dir) {
            if rec.slug == name && run_dir(&dir, &rec).is_dir() {
                out.push(to_installed(&dir, rec));
            }
        }
    }
    out.sort_by(|a, b| a.record.name.to_lowercase().cmp(&b.record.name.to_lowercase()));
    out
}

fn mark_checked_in(root: &Path, slug: &str, ok: bool, now: u64) -> Result<(), String> {
    let dir = slug_dir(root, slug)?;
    let mut rec = read_record(&dir).ok_or("This game isn't installed.")?;
    rec.checked_ms = if ok { now } else { 0 };
    write_record(&dir, &rec)
}

fn launch_gate_in(root: &Path, slug: &str, now: u64) -> Result<(), String> {
    let dir = slug_dir(root, slug)?;
    let rec = read_record(&dir).ok_or("This game isn't installed.")?;
    if update::launch_allowed(rec.launch_check, rec.checked_ms, now) {
        Ok(())
    } else {
        Err(format!("Connect to the internet to check your licence for {}.", rec.name))
    }
}

fn uninstall_in(root: &Path, slug: &str) -> Result<(), String> {
    check_slug(slug)?;
    if BUSY.lock().unwrap().contains(slug) {
        return Err("Wait for the install to finish.".into());
    }
    let p = root.join(slug);
    match fs::symlink_metadata(&p) {
        Err(_) => Ok(()),
        Ok(m) if m.is_dir() => fs::remove_dir_all(&p),
        Ok(_) => fs::remove_file(&p),
    }
    .map_err(|e| format!("Couldn't remove {}: {e}", p.display()))
}

/// Installed Rare Store games (the library maps them to its own game type).
pub fn installed() -> Vec<Installed> {
    let mut out: Vec<Installed> = Vec::new();
    for r in rare_roots() {
        for i in installed_in(&r) {
            if !out.iter().any(|o| o.record.slug == i.record.slug) {
                out.push(i);
            }
        }
    }
    out.sort_by(|a, b| a.record.name.to_lowercase().cmp(&b.record.name.to_lowercase()));
    out
}

/// Where an installed game's files run from (`current`, or the version folder without symlinks).
pub fn run_dir_of(i: &Installed) -> PathBuf {
    run_dir(Path::new(&i.dir), &i.record)
}

/// Called before launching a `rare:<slug>` game: refuses when the game wants a licence check
/// and the last positive one is older than the offline grace.
pub fn launch_gate(slug: &str) -> Result<(), String> {
    launch_gate_in(&root_of(slug)?, slug, now_ms())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Platforms {
    pub platforms: Vec<String>,
    pub proton: bool,
}

/// Platforms this device can install: Linux builds natively, Windows builds when umu-run (Proton) is there.
/// `proton` is whether umu-run was found; the caller knows how to look (rare-launch).
pub fn platforms(proton: bool) -> Platforms {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let mut platforms = vec!["linux-x86_64".to_string()];
        if proton {
            platforms.push("windows-x86_64".into());
        }
        Platforms { platforms, proton }
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = proton;
        Platforms { platforms: vec![], proton: false }
    }
}

/// The message the wallet signs before a download: `{ message, expires_ms }`. Blocking.
pub fn challenge(address: &str, slug: &str, platform: &str) -> Result<serde_json::Value, String> {
    check_slug(slug)?;
    check_platform(platform)?;
    api_post("/downloads/challenge", serde_json::json!({ "address": address, "slug": slug, "platform": platform }))
}

/// Downloads, verifies and installs a build (blocking). Progress goes out as `store-progress`
/// events, and a failure as a final `error` phase.
pub fn install(ctx: &Ctx, req: InstallRequest) -> Result<Installed, Error> {
    let slug = req.slug.clone();
    let progress = |phase: &str, done: u64, total: u64, message: Option<String>| {
        if let Ok(v) = serde_json::to_value(Progress { slug: &slug, phase, done, total, message }) {
            ctx.send(Event::Other { name: "store-progress".into(), payload: v });
        }
    };
    let res = install_blocking(req, &progress);
    if let Err(e) = &res {
        progress("error", 0, 0, Some(e.clone()));
    }
    res.map_err(Error::from)
}

/// After the UI's online licence check: `ok` stamps now, a failed check clears the stamp.
pub fn mark_checked(slug: &str, ok: bool) -> Result<(), String> {
    mark_checked_in(&root_of(slug)?, slug, ok, now_ms())
}

pub fn uninstall(slug: &str) -> Result<(), String> {
    uninstall_in(&root_of(slug)?, slug)
}

/// Install, update or nothing, given the on-chain release version for the licence's edition.
pub fn update_check(slug: &str, chain_version: Option<u64>) -> Result<UpdateDecision, String> {
    let root = root_of(slug)?;
    let dir = slug_dir(&root, slug)?;
    let installed = read_record(&dir).filter(|r| run_dir(&dir, r).is_dir()).map(|r| (r.edition, r.version));
    Ok(update::update_decision(installed, chain_version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(version: u64, sha: &str, size: u64) -> InstallRequest {
        serde_json::from_value(serde_json::json!({
            "slug": "test-game", "name": "Test Game", "platform": "linux-x86_64", "address": "0xabc",
            "message": "m", "signature": "s", "expectedSha256": sha, "expectedSize": size.to_string(),
            "version": version, "label": "1.0", "edition": 0, "launchCheck": true,
            "images": { "cover": "https://x/cover.png", "banner": "https://x/banner.png" }
        }))
        .unwrap()
    }

    fn answer(r: &InstallRequest) -> DownloadAnswer {
        serde_json::from_value(serde_json::json!({
            "url": "https://cdn.example/b.tar.zst", "expires_ms": 1, "sha256": r.expected_sha256.to_uppercase(),
            "size": r.expected_size, "version": r.version.to_string(), "label": "1.0", "edition": r.edition, "licence_id": "0x1"
        }))
        .unwrap()
    }

    #[test]
    fn checks_requests_and_answers() {
        let sha = "ab".repeat(32);
        let r = req(2, &sha, 10);
        assert!(check_request(&r).is_ok());
        let a = answer(&r);
        assert!(check_answer(&r, &a).is_ok());
        for bad in [
            DownloadAnswer { sha256: "cd".repeat(32), ..a.clone() },
            DownloadAnswer { version: 1, ..a.clone() },
            DownloadAnswer { edition: 1, ..a.clone() },
            DownloadAnswer { size: 11, ..a.clone() },
            DownloadAnswer { url: "http://cdn.example/b".into(), ..a.clone() },
        ] {
            assert_eq!(check_answer(&r, &bad).unwrap_err(), CHAIN_MISMATCH);
        }
        assert!(check_request(&InstallRequest { slug: "../x".into(), ..r.clone() }).is_err());
        assert!(check_request(&InstallRequest { platform: "mac".into(), ..r.clone() }).is_err());
        assert!(check_request(&InstallRequest { expected_sha256: "ab".into(), ..r }).is_err());
    }

    #[test]
    fn maps_api_errors() {
        let body = serde_json::json!({ "error": { "code": "uploads_closed", "message": "Uploads open soon" } });
        assert_eq!(api_error(503, &body), "uploads_closed: Uploads open soon");
        assert_eq!(api_error(500, &serde_json::Value::Null), "The store answered with HTTP 500.");
    }

    #[test]
    fn throttles_progress() {
        let mut t = Throttle(None);
        assert!(t.ready());
        assert!(!t.ready());
    }

    #[test]
    fn installs_updates_and_uninstalls() {
        let root = std::env::temp_dir().join(format!("rare-store-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("test-game");
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join(".download");
        let bytes = unpack::tests::zst(&unpack::tests::good_tar());
        let sha = "ab".repeat(32);
        for v in [1u64, 2] {
            fs::write(&archive, &bytes).unwrap();
            let rec = place_build(&dir, &archive, &req(v, &sha, bytes.len() as u64), "0x1", 1000).unwrap();
            assert_eq!((rec.version, rec.runtime, rec.exec.as_str(), rec.checked_ms), (v, Runtime::Native, "bin/game", 1000));
        }
        let mut names: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        let want: Vec<&str> = if cfg!(unix) { vec!["2", "current", "install.json"] } else { vec!["2", "install.json"] };
        assert_eq!(names, want);
        let list = installed_in(&root);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].id.as_str(), list[0].record.licence_id.as_str()), ("rare:test-game", "0x1"));
        assert_eq!(list[0].record.images.cover.as_deref(), Some("https://x/cover.png"));
        assert!(run_dir_of(&list[0]).join("bin/game").is_file() && list[0].record.runtime == Runtime::Native);
        // Launch grace.
        assert!(launch_gate_in(&root, "test-game", 1000 + update::LAUNCH_GRACE_MS).is_ok());
        assert_eq!(launch_gate_in(&root, "test-game", 1001 + update::LAUNCH_GRACE_MS).unwrap_err(), "Connect to the internet to check your licence for Test Game.");
        mark_checked_in(&root, "test-game", true, 5000).unwrap();
        assert!(launch_gate_in(&root, "test-game", 5000 + update::LAUNCH_GRACE_MS).is_ok());
        mark_checked_in(&root, "test-game", false, 6000).unwrap();
        assert!(launch_gate_in(&root, "test-game", 6000).is_err());
        assert!(uninstall_in(&root, "..").is_err());
        uninstall_in(&root, "test-game").unwrap();
        assert!(!dir.exists() && installed_in(&root).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_bad_build_leaves_the_old_one() {
        let root = std::env::temp_dir().join(format!("rare-store-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("test-game");
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join(".download");
        let sha = "ab".repeat(32);
        let good = unpack::tests::zst(&unpack::tests::good_tar());
        fs::write(&archive, &good).unwrap();
        place_build(&dir, &archive, &req(1, &sha, good.len() as u64), "0x1", 1).unwrap();
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(2);
        b.append_data(&mut h, "rare.json", &b"{}"[..]).unwrap();
        let bad = unpack::tests::zst(&b.into_inner().unwrap());
        fs::write(&archive, &bad).unwrap();
        assert!(place_build(&dir, &archive, &req(2, &sha, bad.len() as u64), "0x1", 2).is_err());
        assert!(!dir.join(".2.partial").exists() && !dir.join("2").exists());
        assert_eq!(installed_in(&root)[0].record.version, 1);
        fs::remove_dir_all(&root).unwrap();
    }
}
