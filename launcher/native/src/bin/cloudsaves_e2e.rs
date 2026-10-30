//! End to end check of automatic cloud saves (features::cloudsync, the same jobs Play and a
//! session's end run) on Walrus TESTNET, in throwaway home folders, with a throwaway account.
//! Never point it at mainnet or a real wallet: it refuses unless RARE_SAVES_NETWORK=testnet.
//!
//!   cloudsaves_e2e writer  <secret hex> <account>          the cloud saves address to fund
//!   cloudsaves_e2e run     <secret hex> <account> <dir>    the whole flow (dir: scratch homes)
//!   cloudsaves_e2e cleanup <secret hex> <account> <to>     delete every backup, send WAL and SUI to <to>
//!                                                          (<to> = the cloud saves address: delete only)
//!
//! RARE_SAVES_EPOCHS=2 makes uploads short so the renewal step has something to renew.
//! Prints each step; exits 1 at the first failure.

use rare_native::data::model::{Game, LaunchSpec};
use rare_native::features::cloudsync::{self as cs, e2e, Keep};
use rare_native::screens::game::saves::when;
use std::path::Path;

fn fail(msg: impl AsRef<str>) -> ! {
    println!("FAIL: {}", msg.as_ref());
    std::process::exit(1);
}

fn ok(msg: impl AsRef<str>) {
    println!("ok: {}", msg.as_ref());
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|_| fail("secret is not hex"))).collect()
}

fn game(id: &str, source: &str, title: &str, dir: Option<&Path>, launch: Option<LaunchSpec>) -> Game {
    Game { id: id.into(), source: source.into(), title: title.into(), installed: true, install_dir: dir.map(|d| d.to_string_lossy().into_owned()), size_bytes: None, last_played: None, playtime_minutes: None, art: Default::default(), launch, origin: String::new(), foreign: false }
}

fn put(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
    // Distinct mtimes between steps, as real play gives.
    std::thread::sleep(std::time::Duration::from_millis(1100));
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn device(home: &Path) {
    std::fs::create_dir_all(home).unwrap();
    std::env::set_var("HOME", home);
    std::env::remove_var("XDG_DATA_HOME");
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn newest_fp(set: &str) -> Option<String> {
    e2e::cloud().into_iter().filter(|b| b.meta.id == set).max_by_key(|b| b.meta.created).and_then(|b| b.meta.fp)
}

struct Log;
impl log::Log for Log {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            println!("   {}: {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

fn main() {
    let _ = log::set_logger(&Log).map(|()| log::set_max_level(log::LevelFilter::Info));
    if std::env::var("RARE_SAVES_NETWORK").as_deref() != Ok("testnet") {
        fail("testnet only: set RARE_SAVES_NETWORK=testnet");
    }
    let a: Vec<String> = std::env::args().collect();
    let (cmd, secret, account) = match a.as_slice() {
        [_, c, s, acc, ..] => (c.as_str(), hex(s), acc.clone()),
        _ => fail("usage: cloudsaves_e2e writer|run|cleanup <secret hex> <account> [dir|to]"),
    };
    match cmd {
        "writer" => {
            let tmp = std::env::temp_dir().join(format!("cs-e2e-writer-{}", std::process::id()));
            device(&tmp);
            println!("{}", cs::install(&secret, &account).unwrap_or_else(|e| fail(e)));
            let _ = std::fs::remove_dir_all(tmp);
        }
        "run" => run(&secret, &account, Path::new(a.get(4).unwrap_or_else(|| fail("dir")))),
        "cleanup" => cleanup(&secret, &account, a.get(4).unwrap_or_else(|| fail("to"))),
        // After `run`: device 2 uploads a newer save and device 1 changes its own, so the app on
        // device 1's home shows the conflict sheet on Play (the UI check).
        "conflict" => {
            let dir = Path::new(a.get(4).unwrap_or_else(|| fail("dir")));
            let ra = game("apps:retroarch", "apps", "RetroArch", None, None);
            device(&dir.join("device2"));
            cs::e2e::switch_device(&secret, &account).unwrap_or_else(|e| fail(e));
            e2e::before(&ra, false);
            put(&dir.join("device2/.config/retroarch/saves/zelda.srm"), "device 2, a longer save after the boss fight");
            e2e::after(&ra, false);
            device(&dir.join("device1"));
            put(&dir.join("device1/.config/retroarch/saves/zelda.srm"), "device 1 offline");
            ok("conflict ready for device 1");
        }
        _ => fail("unknown command"),
    }
}

fn run(secret: &[u8], account: &str, dir: &Path) {
    let (home1, home2) = (dir.join("device1"), dir.join("device2"));
    let _ = std::fs::remove_dir_all(dir);
    device(&home1);
    let writer = cs::install(secret, account).unwrap_or_else(|e| fail(e));
    ok(format!("turned on, cloud saves address {writer}"));
    if !e2e::index_blocking() {
        fail("Ludusavi index did not load");
    }
    ok("Ludusavi index loaded");

    // The games: RetroArch (an emulator), a Linux PC game the manifest knows, a Steam shortcut.
    let ra = game("apps:retroarch", "apps", "RetroArch", None, None);
    let hk_dir = home1.join("Games/Hollow Knight");
    let hk = game("local:hk", "local", "Hollow Knight", Some(&hk_dir), Some(LaunchSpec { kind: "exec".into(), ..Default::default() }));
    let ra_file = |h: &Path| h.join(".config/retroarch/saves/zelda.srm");
    let hk_file = |h: &Path| h.join(".config/unity3d/Team Cherry/Hollow Knight/user1.dat");
    put(&ra_file(&home1), "v1");
    put(&hk_file(&home1), "hk v1");

    let t = e2e::targets(&ra, false);
    if !t.iter().any(|t| t.id() == "retroarch-saves") {
        fail(format!("RetroArch targets: {:?}", t.iter().map(|t| t.id().to_string()).collect::<Vec<_>>()));
    }
    let t = e2e::targets(&hk, false);
    if !t.iter().any(|t| t.id() == "game:local:hk") {
        fail("Hollow Knight has no save place from the manifest");
    }
    ok("save places: RetroArch saves, Hollow Knight (Ludusavi, ~/.config/unity3d)");

    // Steam shortcut through Proton: its compatdata prefix.
    let appid: u64 = 3_000_000_123;
    let pfx = home1.join(format!(".local/share/Steam/steamapps/compatdata/{appid}/pfx"));
    put(&pfx.join("drive_c/users/steamuser/AppData/LocalLow/Team Cherry/Hollow Knight/user1.dat"), "sc");
    let sc = game("apps:sc", "apps", "Hollow Knight", None, Some(LaunchSpec { kind: "steamShortcut".into(), gameid: Some((appid << 32) | 0x0200_0000), ..Default::default() }));
    let t = e2e::targets(&sc, false);
    let in_pfx = t.iter().any(|t| matches!(t, rare_sui::saves::sync::Target::Game(g) if g.roots.iter().any(|r| r.base.starts_with(&pfx))));
    let scan = t.first().and_then(|t| t.scan().ok()).unwrap_or_default();
    if !in_pfx || scan.files != 1 {
        fail(format!("Steam shortcut saves not found in its compatdata prefix ({} files)", scan.files));
    }
    ok("Steam shortcut: saves found in steamapps/compatdata/<appid>/pfx");

    // 1. Play on device 1: nothing in the cloud yet, it starts; the session's end uploads.
    let o = e2e::before(&ra, false);
    if !o.iter().any(|x| x == "launch:apps:retroarch") || e2e::conflict().is_some() {
        fail(format!("first launch: {o:?}"));
    }
    ok("Play with an empty cloud starts the game");
    put(&ra_file(&home1), "v2 after playing");
    let o = e2e::after(&ra, false);
    if let Some(e) = e2e::error("apps:retroarch") {
        fail(format!("upload after the session: {e} {o:?}"));
    }
    let fp = newest_fp("retroarch-saves").unwrap_or_else(|| fail("no cloud copy after the session"));
    ok(format!("session end uploaded RetroArch saves (fingerprint {fp})"));
    e2e::after(&hk, false);
    if newest_fp("game:local:hk").is_none() {
        fail(format!("Hollow Knight upload: {:?}", e2e::error("local:hk")));
    }
    ok("session end uploaded Hollow Knight (a PC game's Ludusavi save place)");

    // 2. A second device: nothing there, Play brings the cloud copy down first.
    device(&home2);
    cs::e2e::switch_device(secret, account).unwrap_or_else(|e| fail(e));
    e2e::index_blocking();
    let o = e2e::before(&ra, false);
    if read(&ra_file(&home2)) != "v2 after playing" || !o.iter().any(|x| x.starts_with("launch:")) {
        fail(format!("second device download: {:?} {o:?}", read(&ra_file(&home2))));
    }
    ok("new device: Play downloaded the cloud save before starting");
    put(&ra_file(&home2), "v3 from device 2");
    e2e::after(&ra, false);
    ok("device 2 played and uploaded");

    // 3. Back on device 1, changed there too: the conflict sheet.
    device(&home1);
    cs::e2e::switch_device(secret, account).unwrap_or_else(|e| fail(e));
    put(&ra_file(&home1), "v2 plus offline play on device 1");
    let o = e2e::before(&ra, false);
    let c = e2e::conflict().unwrap_or_else(|| fail(format!("no conflict: {o:?}")));
    let s = &c.sets[0];
    let now = now_ms();
    let line = format!("This device: {} / Cloud: {}", when(s.local.modified.unwrap_or(now), now), when(s.cloud.meta.modified.unwrap_or(s.cloud.meta.created), now));
    if !o.iter().any(|x| x == "conflict") || o.iter().any(|x| x.starts_with("launch:")) {
        fail(format!("conflict should hold the launch: {o:?}"));
    }
    ok(format!("conflict sheet: \"{line}\", {} B here, {} B in the cloud; the game waits", s.local.bytes, s.cloud.meta.raw.unwrap_or(s.cloud.meta.bytes)));

    // 4. Pick the cloud: the device copy is kept first.
    let o = e2e::resolve(&ra, false, Keep::Cloud);
    if read(&ra_file(&home1)) != "v3 from device 2" {
        fail(format!("use cloud: file is {:?} {o:?}", read(&ra_file(&home1))));
    }
    let v = e2e::view(&ra, false);
    let kept = v.versions.iter().find(|r| r.local.is_some() && r.reason == "download").cloned().unwrap_or_else(|| fail(format!("device copy not kept: {:?}", v.versions)));
    ok(format!("chose Cloud: saves replaced, device copy kept ({} versions listed)", v.versions.len()));

    // 5. Restore that kept version from the Saves sheet.
    let o = e2e::restore(&ra, false, &kept);
    if read(&ra_file(&home1)) != "v2 plus offline play on device 1" || !o.iter().any(|x| x.contains("Restored")) {
        fail(format!("restore: {:?} {o:?}", read(&ra_file(&home1))));
    }
    let restored_fp = e2e::view(&ra, false).versions.iter().find(|r| r.current).map(|r| r.reason.clone());
    ok(format!("restored the kept version; it is current ({restored_fp:?}) and was uploaded"));
    let here = e2e::targets(&ra, false).first().and_then(|t| t.scan().ok()).map(|s| s.fp);
    if newest_fp("retroarch-saves") != here {
        fail("restore did not upload");
    }

    // 6. A failed upload is retried later, not only after the next session.
    put(&hk_file(&home1), "hk v2");
    std::env::set_var("RARE_SAVES_E2E_OFFLINE", "1");
    e2e::after(&hk, false);
    let r = e2e::retry("local:hk").unwrap_or_else(|| fail("no retry scheduled"));
    ok(format!("offline upload failed; retry {} scheduled in {:.0} s ({:?})", r.0, r.1, e2e::error("local:hk")));
    std::env::remove_var("RARE_SAVES_E2E_OFFLINE");
    e2e::run_retries();
    if e2e::retry("local:hk").is_some() || e2e::error("local:hk").is_some() {
        fail("retry did not upload");
    }
    ok("network back: the retry uploaded it");

    // 7. Renewal: the newest copies were bought short (RARE_SAVES_EPOCHS) and get extended.
    let before: Vec<(String, Option<u64>)> = e2e::cloud().iter().map(|b| (b.blob_object_id.clone(), b.end_epoch)).collect();
    let r = e2e::renew();
    if r.is_empty() {
        fail(format!("nothing renewed: {before:?}"));
    }
    let after = e2e::cloud();
    let grew = after.iter().filter(|b| before.iter().any(|(id, e)| id == &b.blob_object_id && b.end_epoch > *e)).count();
    if grew == 0 {
        fail("end epochs did not move");
    }
    ok(format!("renewed {r:?}; {grew} copies now end later"));
    println!("PASS");
}

fn cleanup(secret: &[u8], account: &str, to: &str) {
    let tmp = std::env::temp_dir().join(format!("cs-e2e-clean-{}", std::process::id()));
    device(&tmp);
    cs::install(secret, account).unwrap_or_else(|e| fail(e));
    let w = e2e::writer().unwrap_or_else(|| fail("no writer"));
    let sui = cs::sui().unwrap_or_else(|| fail("no network"));
    let rt = rare_native::screens::wallet::live::rt();
    let list = e2e::cloud();
    for b in list.iter().filter(|b| b.meta.how.as_deref() == Some("auto")) {
        match rt.block_on(rare_sui::saves::delete(sui, w.as_ref(), &b.blob_object_id)) {
            Ok(_) => ok(format!("deleted {}", b.blob_object_id)),
            Err(e) => println!("   could not delete {}: {e}", b.blob_object_id),
        }
    }
    use rare_sui::grpc::ChainRead;
    let addr = w.address();
    let wal_t = rare_sui::saves::store::wal_type(sui.network());
    let wal = rt.block_on(sui.balance(&addr, wal_t)).map(|b| b.balance).unwrap_or(0);
    let gas = rt.block_on(sui.balance(&addr, "0x2::sui::SUI")).map(|b| b.balance).unwrap_or(0);
    let mut tx = rare_sui::ptb::Tx::new();
    let mut give = Vec::new();
    if wal > 0 {
        give.push(tx.coin_with_balance(wal_t, wal));
    }
    if gas > 30_000_000 {
        give.push(tx.split_u64(rare_sui::ptb::Arg::Gas, gas - 30_000_000));
    }
    if !give.is_empty() && to != addr {
        tx.transfer_to(&give, to);
        match rt.block_on(w.sign_and_execute(tx, "return")) {
            Ok(d) => ok(format!("returned {wal} FROST and {} MIST to {to} ({d})", gas.saturating_sub(30_000_000))),
            Err(e) => println!("   return failed: {e}"),
        }
    }
    let _ = std::fs::remove_dir_all(tmp);
    println!("CLEAN");
}
