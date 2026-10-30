//! End to end on a Linux box with Flatpak and a display (Xvfb works): the one-press setup for the
//! given emulators, a scan of ~/Emulation/roms, then each ROM found is started the way the
//! launcher starts it and driven over RetroArch's command port the way the Rare menu does: save
//! state to slot 1 (its picture must appear), load it, fast forward, open RetroArch's menu, quit.
//!
//!   HOME=/tmp/emu-home cargo run -p rare-emu --example emu_e2e -- retroarch
//!
//! Run it with a throwaway HOME: it installs into that user's Flatpak and writes its configs.
//! EMU_E2E_APPEND: extra RetroArch settings for the test machine (`video_driver = "gl"`).

use rare_core::{Ctx, Event};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let only: Vec<String> = std::env::args().skip(1).collect();
    let ctx = Ctx::new(
        Arc::new(|e: Event| {
            if let Event::Other { name, payload } = e {
                println!("event {name}: {payload}");
            }
        }),
        Arc::new(rare_core::host::NoHost),
    );
    let t0 = Instant::now();
    let ids: Vec<&str> = only.iter().map(String::as_str).collect();
    let r = rare_emu::setup::run(&ctx, if ids.is_empty() { None } else { Some(&ids) });
    println!("setup: {r:?} in {} s", t0.elapsed().as_secs());
    let st = rare_emu::setup::status();
    println!("status: {}", serde_json::to_string_pretty(&st).unwrap());
    let home = rare_emu::roms::home();
    let roms = rare_emu::roms::scan();
    println!("roms: {:?}", roms.iter().map(|r| (&r.id, &r.title)).collect::<Vec<_>>());
    let mut failed = 0;
    for rom in &roms {
        let spec = match rare_emu::roms::launch_spec(rom, &home, true) {
            Ok(s) => s,
            Err(e) => {
                println!("{}: {e}", rom.title);
                continue;
            }
        };
        rare_emu::roms::prepare(rom, &home);
        let rare_library::model::LaunchSpec::Exec { program, mut args, .. } = spec else { continue };
        if let Ok(extra) = std::env::var("EMU_E2E_APPEND") {
            let f = rare_emu::retroarch::config_dir(&home).join("e2e.cfg");
            std::fs::write(&f, extra.replace("\\n", "\n")).unwrap();
            if let Some(i) = args.iter().position(|a| a == "--appendconfig") {
                args[i + 1] = format!("{}|{}", args[i + 1], f.display());
            }
        }
        println!("start: {program} {}", args.join(" "));
        let mut child = Command::new(&program).args(&args).spawn().expect("start");
        // RetroArch answers on its command port once it runs.
        let up = (0..60).any(|_| {
            std::thread::sleep(Duration::from_millis(500));
            rare_emu::retroarch::ask("VERSION").map(|v| println!("RetroArch {v}")).is_ok()
        });
        if !up {
            println!("{}: RetroArch never answered", rom.title);
            failed += 1;
            let _ = child.kill();
            continue;
        }
        std::thread::sleep(Duration::from_secs(2));
        let states = rare_emu::retroarch::paths(&home).states;
        let step = |name: &str, r: Result<(), String>| {
            println!("{name}: {r:?}");
            std::thread::sleep(Duration::from_millis(1500));
        };
        for n in [1, 3] {
            step(&format!("save slot {n}"), rare_emu::retroarch::save_slot(n));
        }
        let slots = rare_emu::retroarch::slots(&states, &rom.path);
        println!("slots in {}: {slots:?}", states.display());
        for n in [1usize, 3] {
            if slots[n - 1].saved.is_none() || slots[n - 1].thumb.is_none() {
                println!("{}: slot {n} has no state or no picture", rom.title);
                failed += 1;
            }
        }
        if slots[1].saved.is_some() {
            println!("{}: slot 2 should be empty", rom.title);
            failed += 1;
        }
        println!("next slot: {}", rare_emu::retroarch::next_slot(&slots));
        step("load slot 1", rare_emu::retroarch::load_slot(1));
        step("fast forward on", rare_emu::retroarch::send("FAST_FORWARD"));
        step("fast forward off", rare_emu::retroarch::send("FAST_FORWARD"));
        if rom.system.rewind {
            step("rewind", rare_emu::retroarch::rewind(2.0));
        }
        step("screenshot", rare_emu::retroarch::send("SCREENSHOT"));
        step("menu", rare_emu::retroarch::send("MENU_TOGGLE"));
        step("menu off", rare_emu::retroarch::send("MENU_TOGGLE"));
        let _ = rare_emu::retroarch::send("QUIT");
        std::thread::sleep(Duration::from_secs(2));
        let _ = rare_emu::retroarch::send("QUIT");
        let gone = (0..20).any(|_| {
            std::thread::sleep(Duration::from_millis(500));
            matches!(child.try_wait(), Ok(Some(_)))
        });
        if !gone {
            let _ = child.kill();
        }
        println!("{}: done (exited by itself: {gone})", rom.title);
    }
    println!("E2E {}", if failed == 0 && !roms.is_empty() { "PASS" } else { "FAIL" });
    std::process::exit(if failed == 0 && !roms.is_empty() { 0 } else { 1 });
}
