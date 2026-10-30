//! Retro trophies end to end on a Linux box with Flatpak, Xvfb and xdotool: RetroArch set up the
//! way Rare sets it up, a ROM started the way the launcher starts it (rare.cfg points rcheevos at
//! rare.market with this device's token), the game played with key presses until rare.market
//! records an unlock as the account's Rare trophy.
//!
//!   HOME=/tmp/retro-home RARE_RETRO_TOKEN=rr_... DISPLAY=:77 \
//!     cargo run -p rare-emu --example retro_e2e -- nes/2048.nes
//!
//! The ROM path is under ~/Emulation/roms. Keys pressed: Return, then the arrows in turn (2048's
//! "Four!" comes after a few moves). RETRO_E2E_KEYS changes them; EMU_E2E_APPEND adds RetroArch
//! settings for the test machine (`video_driver = "gl"`). Exits 0 when an unlock arrived.

use rare_core::{Ctx, Event};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let want = std::env::args().nth(1).expect("a ROM path under ~/Emulation/roms");
    let token = std::env::var("RARE_RETRO_TOKEN").expect("RARE_RETRO_TOKEN");
    let ctx = Ctx::new(Arc::new(|_e: Event| {}), Arc::new(rare_core::host::NoHost));
    let t0 = Instant::now();
    println!("setup: {:?} in {} s", rare_emu::setup::run(&ctx, Some(&["retroarch"])), t0.elapsed().as_secs());
    let sign = rare_emu::retro::Sign { user: std::env::var("RARE_RETRO_USER").unwrap_or_else(|_| "Rare player".into()), token, address: String::new(), hardcore: std::env::var("RETRO_E2E_SOFTCORE").is_err() };
    rare_emu::retro::save(Some(&sign)).expect("save sign-in");
    let home = rare_emu::roms::home();
    let roms = rare_emu::roms::scan();
    let rom = roms.iter().find(|r| r.path.ends_with(&want)).unwrap_or_else(|| panic!("no ROM {want} in {:?}", roms.iter().map(|r| &r.id).collect::<Vec<_>>()));
    println!("rom {} hash {:?}", rom.id, rare_emu::retro::rom_hash(rom));
    let before = rare_emu::retro::played(&sign).map(|p| p.len());
    println!("played before: {before:?}");
    let spec = rare_emu::roms::launch_spec(rom, &home, true).expect("launch spec");
    rare_emu::roms::prepare(rom, &home);
    let cfg = rare_emu::retroarch::config_dir(&home).join("rare.cfg");
    let rare_cfg = std::fs::read_to_string(&cfg).unwrap_or_default();
    println!("rare.cfg cheevos lines:\n{}", rare_cfg.lines().filter(|l| l.starts_with("cheevos")).map(|l| if l.starts_with("cheevos_token") { "cheevos_token = <set>" } else { l }).collect::<Vec<_>>().join("\n"));
    let rare_library::model::LaunchSpec::Exec { program, mut args, .. } = spec else { panic!("not an exec launch") };
    if let Ok(extra) = std::env::var("EMU_E2E_APPEND") {
        let f = rare_emu::retroarch::config_dir(&home).join("e2e.cfg");
        std::fs::write(&f, extra.replace("\\n", "\n")).unwrap();
        if let Some(i) = args.iter().position(|a| a == "--appendconfig") {
            args[i + 1] = format!("{}|{}", args[i + 1], f.display());
        }
    }
    args.insert(args.len() - 1, "--verbose".into());
    let log = std::fs::File::create(home.join("retroarch-e2e.log")).unwrap();
    println!("start: {program} {}", args.join(" "));
    let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let mut child = Command::new(&program).args(&args).stdout(Stdio::from(log.try_clone().unwrap())).stderr(Stdio::from(log)).spawn().expect("start");
    let up = (0..90).any(|_| {
        std::thread::sleep(Duration::from_millis(500));
        rare_emu::retroarch::ask("VERSION").map(|v| println!("RetroArch {v}")).is_ok()
    });
    assert!(up, "RetroArch never answered");
    let keys: Vec<String> = std::env::var("RETRO_E2E_KEYS").unwrap_or_else(|_| "Left Right Up Down".into()).split_whitespace().map(String::from).collect();
    let press = |k: &str| {
        let _ = Command::new("xdotool").args(["keydown", k]).status();
        std::thread::sleep(Duration::from_millis(120));
        let _ = Command::new("xdotool").args(["keyup", k]).status();
        std::thread::sleep(Duration::from_millis(250));
    };
    std::thread::sleep(Duration::from_secs(6));
    for _ in 0..3 {
        press("Return");
        std::thread::sleep(Duration::from_millis(800));
    }
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut unlocked = None;
    let mut last_check = Instant::now();
    let mut i = 0;
    while Instant::now() < deadline && unlocked.is_none() {
        press(&keys[i % keys.len()]);
        i += 1;
        if last_check.elapsed() > Duration::from_secs(3) {
            last_check = Instant::now();
            match rare_emu::retro::recent(&sign, started - 5_000) {
                Ok(list) if !list.is_empty() => unlocked = Some(list),
                Ok(_) => {}
                Err(e) => println!("recent: {e}"),
            }
        }
    }
    let _ = rare_emu::retroarch::send("SCREENSHOT");
    std::thread::sleep(Duration::from_secs(1));
    let _ = rare_emu::retroarch::send("QUIT");
    std::thread::sleep(Duration::from_secs(2));
    let _ = rare_emu::retroarch::send("QUIT");
    if !(0..20).any(|_| {
        std::thread::sleep(Duration::from_millis(500));
        matches!(child.try_wait(), Ok(Some(_)))
    }) {
        let _ = child.kill();
    }
    let log = std::fs::read_to_string(home.join("retroarch-e2e.log")).unwrap_or_default();
    println!("RetroArch achievement log:\n{}", log.lines().filter(|l| l.contains("RCHEEVOS") || l.to_lowercase().contains("cheevos")).filter(|l| !l.contains("token")).take(40).collect::<Vec<_>>().join("\n"));
    match unlocked {
        Some(list) => {
            for u in &list {
                println!("UNLOCK {} \"{}\" {} points hardcore={} game {} \"{}\" badge {}", u.id, u.title, u.points, u.hardcore, u.game_id, u.game, u.badge_url);
            }
            if let Ok(p) = rare_emu::retro::played(&sign) {
                for g in p {
                    println!("CASE {} \"{}\" {} of {} (hardcore {})", g.game_id, rare_emu::retro::display_title(&g.title), g.earned, g.total, g.earned_hardcore);
                }
            }
            println!("RETRO E2E PASS after {} key presses", i);
        }
        None => {
            println!("RETRO E2E FAIL: no unlock recorded");
            std::process::exit(1);
        }
    }
}
