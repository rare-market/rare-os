//! Writes or shows Rare's controller profiles in a home folder, as setup and launch do.
//!
//!   emu_controls <home> ensure|replace|review|print <emulator ids...>
//!
//! Folders: an emulator's Flatpak folder when <home>/.var/app/<its app id> exists, else the native
//! ones. The pad is this machine's InputPlumber pad when there is one, else the SuiPlay's (the build machine's
//! virtual pad from tools/emu/vpad.py reports exactly the same). Used by the build machine proof in
//! docs/EMULATION.md.
use rare_emu::controls::{self, Pad};
use rare_emu::defaults::Env;
use std::path::PathBuf;

fn env_for(id: &str, home: &PathBuf) -> Env {
    let base = Env { home: home.clone(), pad_guid: None, pad_name: None, xdg: None };
    let app = rare_emu::catalog::by_id(id)
        .and_then(|e| e.flatpak)
        .or_else(|| rare_emu::catalog::ALTERNATES.iter().find(|(_, e)| e.id == id).and_then(|(_, e)| e.flatpak));
    match app {
        Some(a) if home.join(".var/app").join(a).is_dir() => base.flatpak(a),
        _ => base,
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: emu_controls <home> ensure|replace|review|print <emulator ids...>");
        std::process::exit(2);
    }
    let home = PathBuf::from(&a[1]);
    let ids: Vec<String> = if a.len() > 3 { a[3..].to_vec() } else { controls::emulators().iter().map(|s| s.to_string()).collect() };
    let pad = Pad::here();
    println!("pad: {} / SDL {} / GUID {} ({})", pad.evdev, pad.sdl, pad.guid, pad.guid0);
    for id in &ids {
        let env = env_for(id, &home);
        match a[2].as_str() {
            "ensure" => println!("{id}: wrote {:?}", controls::ensure_in(id, &env, &pad)),
            "replace" => println!("{id}: wrote {:?}", controls::replace_in(id, &env, &pad)),
            "review" | "print" => {
                let Some(p) = controls::profile(id, &env, &pad) else { continue };
                for x in &p.pieces {
                    println!("{id}: {:?} {} {:?}", controls::classify(&home, x, &p.known), x.path.display(), x.kind);
                    if a[2] == "print" {
                        println!("{}", x.body);
                    }
                }
            }
            m => {
                eprintln!("unknown mode {m}");
                std::process::exit(2);
            }
        }
    }
}
