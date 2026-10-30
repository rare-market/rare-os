//! On-device check of Settings > Storage's real code paths (features::storage, the same calls the
//! Storage rows make), against a test card: an image file on a loop device named by
//! RARE_SD_DEVICE (never the internal disk; rare-library refuses anything but /dev/loop* and
//! /dev/mmcblk*). Steps: detect the card, Format (or Use for games), move a Steam game to the card,
//! start it through Steam from there and quit it, move it back, then the same both ways for a
//! Games-folder game it makes and removes itself.
//!
//!   RARE_SD_DEVICE=/dev/loop0 storage_e2e steam:<appid>
//!
//! Prints each step and exits 1 at the first failure.

use rare_library::storage::{self as lst, SdCard};
use rare_native::data::backend::{Backend, DeviceBackend};
use rare_native::features::storage as fst;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fail(msg: impl AsRef<str>) -> ! {
    println!("FAIL: {}", msg.as_ref());
    std::process::exit(1);
}

fn ok(msg: impl AsRef<str>) {
    println!("ok: {}", msg.as_ref());
}

fn drain() -> fst::Out {
    let o = fst::take_out();
    for (t, e) in &o.toasts {
        println!("   toast{}: {t}", if *e { " (error)" } else { "" });
    }
    o
}

/// Waits for the current job to end; returns the toasts it left.
fn wait_job(what: &str, limit: u64) -> Vec<(String, bool)> {
    let t0 = Instant::now();
    let mut last = String::new();
    std::thread::sleep(Duration::from_millis(300));
    while let Some(j) = fst::snapshot().job {
        let now = format!("{j:?}");
        if now.split('{').next() != last.split('{').next() {
            println!("   {what}: {now}");
        }
        last = now;
        if t0.elapsed().as_secs() > limit {
            fail(format!("{what} still running after {limit} s"));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("   {what}: done in {:.1} s", t0.elapsed().as_secs_f32());
    drain().toasts
}

fn measure(w: &fst::Waker, b: &DeviceBackend) {
    b.scan().unwrap_or_else(|e| fail(format!("library scan: {e}")));
    fst::refresh(w, false);
    std::thread::sleep(Duration::from_millis(200));
    while fst::snapshot().measuring {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn item_path(key: &str) -> PathBuf {
    let s = fst::snapshot();
    let it = s.items.unwrap_or_default().into_iter().find(|i| i.key == key).unwrap_or_else(|| fail(format!("{key} is not in Storage's list")));
    it.path.unwrap_or_default()
}

fn on_card(p: &Path, mount: &Path) -> bool {
    std::fs::canonicalize(p).unwrap_or(p.to_path_buf()).starts_with(mount)
}

fn errors(t: &[(String, bool)]) -> Option<String> {
    t.iter().find(|(_, e)| *e).map(|(m, _)| m.clone())
}

fn main() {
    let steam_id = std::env::args().nth(1).unwrap_or_else(|| fail("usage: storage_e2e steam:<appid>"));
    let dev = lst::sd_device();
    if !dev.starts_with("/dev/loop") {
        fail("RARE_SD_DEVICE must name the test loop device");
    }
    let w: fst::Waker = Arc::new(|| {});
    let b = DeviceBackend::new();

    // 1. Detect.
    let sd = lst::sd_card();
    println!("1. detect {dev}: {sd:?}");
    let drives = lst::drives();
    for d in &drives {
        println!("   drive {:?} {} at {} ({}), {} free of {}", d.kind, d.label, d.mount.display(), d.fs, d.free, d.total);
    }

    // 2. Format, which also sets it up for games.
    match sd {
        SdCard::None => fail("no test card seen"),
        SdCard::NotReady { .. } => {
            println!("2. format");
            fst::start_format(&w);
            let t = wait_job("format", 600);
            if let Some(e) = errors(&t) {
                fail(e);
            }
        }
        SdCard::Ready { .. } => {
            println!("2. already formatted: Use for games");
            fst::start_set_up(&w);
            let t = wait_job("set up", 120);
            if let Some(e) = errors(&t) {
                fail(e);
            }
        }
    }
    let SdCard::Ready { mount, .. } = lst::sd_card() else { fail("the card is not mounted after format") };
    println!("   mounted at {}", mount.display());
    if !rare_library::sdcard::is_set_up(&mount) {
        fail("the card is not a Steam library and Games folder");
    }
    ok("card is a Steam library (libraryfolders.vdf) and has a Games folder");

    // 3. A Steam game to the card and back, started from the card in between.
    measure(&w, &b);
    let before = item_path(&steam_id);
    println!("3. move {steam_id} ({}) to the card", before.display());
    fst::start_move(&w, &steam_id);
    let t = wait_job("move", 1800);
    if let Some(e) = errors(&t) {
        fail(e);
    }
    measure(&w, &b);
    let after = item_path(&steam_id);
    if !on_card(&after, &mount) || before.exists() {
        fail(format!("after the move it is at {} and the old folder exists: {}", after.display(), before.exists()));
    }
    let appid = steam_id.trim_start_matches("steam:");
    let manifest = mount.join(format!("SteamLibrary/steamapps/appmanifest_{appid}.acf"));
    if !manifest.is_file() {
        fail(format!("{} is missing", manifest.display()));
    }
    ok(format!("{steam_id} on the card at {}", after.display()));

    println!("4. start {steam_id} through Steam from the card");
    b.launch(&steam_id).unwrap_or_else(|e| fail(format!("launch: {e}")));
    let t0 = Instant::now();
    let mut seen = None;
    while t0.elapsed().as_secs() < 180 && seen.is_none() {
        std::thread::sleep(Duration::from_secs(2));
        seen = rare_core::proc::cmdlines().into_iter().find(|(_, comm, cmd)| comm != "reaper" && cmd.contains(&after.to_string_lossy().to_string()) && !cmd.contains("storage_e2e"));
    }
    match seen {
        Some((pid, comm, cmd)) => ok(format!("game process {pid} ({comm}) runs from the card after {:.0} s: {}", t0.elapsed().as_secs_f32(), cmd.chars().take(160).collect::<String>())),
        None => {
            let _ = b.quit(&steam_id);
            fail("no process from the card within 180 s");
        }
    }
    std::thread::sleep(Duration::from_secs(8));
    let _ = b.quit(&steam_id);
    let t0 = Instant::now();
    while t0.elapsed().as_secs() < 60 && rare_core::proc::cmdlines().iter().any(|(_, _, c)| c.contains(&after.to_string_lossy().to_string()) && !c.contains("storage_e2e")) {
        std::thread::sleep(Duration::from_secs(1));
    }
    ok("quit");
    std::thread::sleep(Duration::from_secs(5));

    println!("5. move {steam_id} back");
    fst::start_move(&w, &steam_id);
    let t = wait_job("move back", 1800);
    if let Some(e) = errors(&t) {
        fail(e);
    }
    measure(&w, &b);
    let back = item_path(&steam_id);
    if on_card(&back, &mount) || after.exists() || manifest.exists() {
        fail(format!("after moving back it is at {}; card copy left: {}", back.display(), after.exists()));
    }
    ok(format!("{steam_id} back at {}", back.display()));

    // 6. A Games-folder game, made here.
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let g = home.join("Games/Rare Move Test");
    std::fs::create_dir_all(&g).unwrap();
    std::fs::write(g.join("start.sh"), "#!/bin/sh\necho hi\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(g.join("start.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(g.join("data.bin"), vec![42u8; 24 << 20]).unwrap();
    measure(&w, &b);
    let key = fst::snapshot().items.unwrap_or_default().into_iter().find(|i| i.path.as_deref().is_some_and(|p| p.ends_with("Rare Move Test"))).map(|i| i.key).unwrap_or_else(|| fail("the test game is not in Storage's list"));
    println!("6. move the Games-folder game {key} to the card");
    fst::start_move(&w, &key);
    let t = wait_job("move", 300);
    if let Some(e) = errors(&t) {
        fail(e);
    }
    let o = drain();
    let _ = o;
    let on = mount.join("Games/Rare Move Test");
    if !on.join("data.bin").is_file() || g.exists() {
        fail("the Games-folder game did not land on the card");
    }
    if std::fs::read(on.join("data.bin")).unwrap() != vec![42u8; 24 << 20] {
        fail("the copy differs");
    }
    ok(format!("on the card at {}", on.display()));
    measure(&w, &b);
    let key = fst::snapshot().items.unwrap_or_default().into_iter().find(|i| i.path.as_deref().is_some_and(|p| p.ends_with("Rare Move Test"))).map(|i| i.key).unwrap_or_else(|| fail("the moved game is not in the list after a scan"));
    println!("7. move it back ({key})");
    fst::start_move(&w, &key);
    let t = wait_job("move back", 300);
    if let Some(e) = errors(&t) {
        fail(e);
    }
    if !g.join("data.bin").is_file() || on.exists() {
        fail("the Games-folder game did not come back");
    }
    ok("back in ~/Games");
    std::fs::remove_dir_all(&g).unwrap();
    let _ = b.scan();
    println!("ALL OK");
}
