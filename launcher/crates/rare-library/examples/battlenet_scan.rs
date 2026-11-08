//! Device check: what Rare finds of Battle.net here, and the command it would run for each game.
//! `cargo run -p rare-library --example battlenet_scan [prefix ...]` (no arguments: every place
//! Rare looks).

use rare_library::battlenet;
use rare_library::model::LaunchSpec;
use std::path::PathBuf;

fn main() {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let s = if args.is_empty() { battlenet::scan() } else { battlenet::scan_in(&args) };
    match &s.client {
        Some(c) => println!("app: {}", c.display()),
        None => println!("app: not installed"),
    }
    for g in &s.games {
        let LaunchSpec::Exec { program, args, .. } = &g.launch else { continue };
        let env = battlenet::umu_env(std::path::Path::new(program)).unwrap_or_default();
        let env: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
        println!("{}  {}  in {}", g.id, g.title, g.install_dir.as_deref().unwrap_or("?"));
        println!("  {} umu-run {program:?} {args:?}", env.join(" "));
        if let Some(h) = battlenet::gamepad_hint(&g.id, g.install_dir.as_deref()) {
            println!("  hint: {h}");
        }
    }
    for (id, key) in &s.keys {
        println!("key {id} = {key}");
    }
}
