//! rare-fand [run|check|release]
//!   run      the service (default)
//!   check    exit 0 when an ayaneo_ec fan is here, 1 otherwise (the unit's ExecCondition)
//!   release  hand every ayaneo_ec fan back to the EC and every battery's charge_behaviour
//!            back to auto (the unit's ExecStopPost)
//! RARE_FAND_ROOT and RARE_FAND_STATE move the sysfs root and the state file (tests, dev).

use rare_fand::battery;
use rare_fand::daemon::{Daemon, Stop};
use rare_fand::hwmon::{self, Fan};
use rare_fand::store::STATE_FILE;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

static STOP: AtomicBool = AtomicBool::new(false);
static FAN_DIR: OnceLock<PathBuf> = OnceLock::new();
static BAT_DIR: OnceLock<PathBuf> = OnceLock::new();

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn main() {
    let root = std::env::var_os("RARE_FAND_ROOT").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    let state = std::env::var_os("RARE_FAND_STATE").map_or_else(|| PathBuf::from(STATE_FILE), PathBuf::from);
    match std::env::args().nth(1).as_deref() {
        None | Some("run") => run(&root, &state),
        Some("check") => std::process::exit(if Fan::find(&root).is_some() { 0 } else { 1 }),
        Some("release") => {
            // Both, even when one fails.
            let fans = hwmon::release_all(&root);
            let bats = battery::release_all(&root);
            match &fans {
                Ok(n) => eprintln!("rare-fand: {n} fan(s) handed back to the EC"),
                Err(e) => eprintln!("rare-fand: release: {e}"),
            }
            match &bats {
                Ok(n) => eprintln!("rare-fand: {n} battery charge control(s) back to auto"),
                Err(e) => eprintln!("rare-fand: release charge: {e}"),
            }
            if fans.is_err() || bats.is_err() {
                std::process::exit(1);
            }
        }
        Some(other) => {
            eprintln!("rare-fand: unknown command {other:?} (run, check or release)");
            std::process::exit(2);
        }
    }
}

fn run(root: &Path, state: &Path) {
    let Some(daemon) = Daemon::new(root, state) else {
        eprintln!("rare-fand: no ayaneo_ec fan here, nothing to do");
        return;
    };
    // A panic anywhere hands the fan back before the process goes (ExecStopPost does too).
    let _ = FAN_DIR.set(daemon.fan_dir().to_path_buf());
    if let Some(dir) = daemon.battery_dir() {
        let _ = BAT_DIR.set(dir.to_path_buf());
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = FAN_DIR.get() {
            let _ = std::fs::write(dir.join("pwm1_enable"), hwmon::PWM_AUTO);
        }
        if let Some(dir) = BAT_DIR.get().map(|d| d.join("charge_behaviour")).filter(|f| f.exists()) {
            let _ = std::fs::write(dir, battery::AUTO);
        }
        prev(info);
    }));
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: the handler only stores to an atomic.
        unsafe { libc::signal(sig, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t) };
    }

    let daemon = Arc::new(Mutex::new(daemon));
    let lock = || daemon.lock().unwrap_or_else(|e| e.into_inner());
    let mut bus = None;
    let mut next_bus = Instant::now();
    let mut next = Instant::now();
    let mut next_battery = Instant::now();
    let code = loop {
        if STOP.load(Ordering::SeqCst) {
            break 0;
        }
        let now = Instant::now();
        if bus.is_none() && now >= next_bus {
            bus = connect(&daemon);
            next_bus = now + Duration::from_secs(30);
        }
        if now >= next {
            if let Err(Stop::FanGone) = lock().tick(now) {
                eprintln!("rare-fand: the ayaneo_ec fan is gone, stopping");
                break 0;
            }
            next += Duration::from_secs(1);
            if next < now {
                // Slept through a suspend: no catching up.
                next = now + Duration::from_secs(1);
            }
        }
        if now >= next_battery {
            lock().battery_tick(SystemTime::now());
            next_battery = now + Duration::from_secs(5);
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    lock().shutdown();
    drop(bus);
    std::process::exit(code);
}

/// The bus is for changing presets; the saved preset runs without it.
#[cfg(target_os = "linux")]
fn connect(d: &Arc<Mutex<Daemon>>) -> Option<zbus::blocking::Connection> {
    match rare_fand::bus::serve(d.clone()) {
        Ok(c) => {
            eprintln!("rare-fand: on the system bus as {}", rare_fand::bus::NAME);
            Some(c)
        }
        Err(e) => {
            eprintln!("rare-fand: system bus: {e}; running the saved preset, retrying in 30 s");
            None
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn connect(_: &Arc<Mutex<Daemon>>) -> Option<()> {
    None
}
