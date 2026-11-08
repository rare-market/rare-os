//! Getting Battle.net (Apps > Battle.net > Get Battle.net): Blizzard's installer, downloaded from
//! Blizzard, runs through umu-run in Rare's prefix for the app (rare-library battlenet), the way
//! GOG's Windows builds run. Its window takes the screen as the app `app:battlenet`, with the pad
//! as a mouse; the player clicks Continue and then signs in, in Blizzard's own window. Progress
//! goes out as `app-install` events: the download's percent, then a plain loading bar until the
//! app's program is in the prefix.

use super::catalog::{self, Back};
use rare_core::events::InstallEvent;
use rare_core::{Ctx, Error, Event};
use rare_library::battlenet;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static BUSY: Mutex<bool> = Mutex::new(false);

/// How long the installer may take before Rare stops waiting for it (a slow connection).
const WAIT: Duration = Duration::from_secs(45 * 60);

fn setup_path() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".cache")).join("rare/battlenet/Battle.net-Setup.exe")
}

/// Starts it; progress and the end arrive as `app-install` events for `battlenet`.
pub fn start(ctx: Ctx, runner: crate::launch::Runner) -> Result<(), Error> {
    if !rare_library::steam::which("umu-run") {
        return Err("Battle.net needs umu-run (Proton). Update Rare and try again.".into());
    }
    {
        let mut b = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        if *b {
            return Ok(());
        }
        *b = true;
    }
    std::thread::spawn(move || {
        let result = run(&ctx, &runner);
        *BUSY.lock().unwrap_or_else(|e| e.into_inner()) = false;
        ctx.send(Event::AppInstall(InstallEvent { id: battlenet::APP_ID.into(), progress: result.is_ok().then_some(1.0), done: true, error: result.err() }));
    });
    Ok(())
}

fn emit(ctx: &Ctx, progress: Option<f32>) {
    ctx.send(Event::AppInstall(InstallEvent { id: battlenet::APP_ID.into(), progress, done: false, error: None }));
}

/// Blizzard's installer into the cache (kept for a second try), with the download's percent.
fn download(ctx: &Ctx, to: &PathBuf) -> Result<(), String> {
    let _ = std::fs::create_dir_all(to.parent().unwrap_or(to));
    let resp = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).build().get(battlenet::SETUP_URL).set("User-Agent", "RareLauncher/1.0").call().map_err(|e| {
        rare_core::host::global().log(&format!("[battlenet] download: {e}"));
        "Couldn't download Battle.net. Check Wi-Fi and try again.".to_string()
    })?;
    let total: Option<u64> = resp.header("Content-Length").and_then(|v| v.parse().ok());
    let tmp = to.with_extension("part");
    let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut r = resp.into_reader();
    let (mut buf, mut done, mut last) = (vec![0u8; 64 * 1024], 0u64, -1i32);
    loop {
        let n = r.read(&mut buf).map_err(|_| "The download stopped. Try again.".to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if let Some(t) = total.filter(|t| *t > 0) {
            let pct = (done * 100 / t) as i32;
            if pct != last {
                last = pct;
                emit(ctx, Some(pct as f32 / 100.0));
            }
        }
    }
    drop(out);
    // A Windows program starts with MZ; anything else is an error page.
    let mut head = [0u8; 2];
    std::fs::File::open(&tmp).and_then(|mut f| f.read_exact(&mut head)).map_err(|e| e.to_string())?;
    if &head != b"MZ" {
        let _ = std::fs::remove_file(&tmp);
        return Err("Blizzard sent something that isn't the installer. Try again later.".into());
    }
    std::fs::rename(&tmp, to).map_err(|e| e.to_string())
}

fn run(ctx: &Ctx, runner: &crate::launch::Runner) -> Result<(), String> {
    let prefix = battlenet::rare_prefix();
    if battlenet::client_in(&prefix).is_some() {
        return Ok(());
    }
    emit(ctx, Some(0.0));
    let setup = setup_path();
    if !setup.is_file() {
        download(ctx, &setup)?;
    }
    emit(ctx, None);
    std::fs::create_dir_all(&prefix).map_err(|e| e.to_string())?;
    let run_id = catalog::run_id(battlenet::APP_ID);
    let mut args = vec![setup.to_string_lossy().into_owned()];
    args.extend(battlenet::setup_args(&std::env::var("LANG").unwrap_or_default()));
    let (mut cmd, unit) = crate::launch::host_command_unit("umu-run", &args, &run_id);
    cmd.envs(battlenet::prefix_env(&prefix));
    super::pointer::prepare();
    let child = cmd.spawn().map_err(|e| format!("Could not start Battle.net's installer: {e}"))?;
    runner.track_child(&run_id, child, unit);
    super::front::set_pointer(&run_id, Some(Back::Escape));
    super::front::to_front(&run_id);
    super::step_aside(ctx);
    // Done once the app's program is there; the installer then opens the app's sign-in itself,
    // which stays the running app.
    let t0 = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if battlenet::client_in(&prefix).is_some() {
            return Ok(());
        }
        // Proton's first start sets the prefix up before the installer shows: give it a minute.
        if t0.elapsed() > Duration::from_secs(60) && !runner.running().contains(&run_id) {
            return Err("Battle.net's installer closed before it finished. Try again.".into());
        }
        if t0.elapsed() > WAIT {
            return Err("Battle.net's installer is taking too long. Open Battle.net again to finish.".into());
        }
    }
}
