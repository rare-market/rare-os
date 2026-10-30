//! A GOG game's runtimes into its prefix, end to end (gogdl redist from GOG's public store, then
//! each installer through umu-run with the game's Proton), without a GOG account:
//!
//!   redist_smoke <game id> <dependency>...      e.g. redist_smoke 1 openAL MSVC2015_x64
//!
//! Point XDG_CONFIG_HOME and XDG_DATA_HOME at a scratch folder to keep the prefix out of the home.

fn main() {
    rare_stores::tools::add_to_path();
    let mut args = std::env::args().skip(1);
    let game = args.next().unwrap_or_else(|| "1".into());
    let deps: Vec<String> = args.collect();
    rare_stores::redist::save_deps(&game, deps);
    let id = format!("gog:{game}");
    println!("prefix {}", rare_library::umu::prefix(&id).display());
    println!("proton {:?}", rare_library::umu::protonpath(&id));
    let t = std::time::Instant::now();
    let r = rare_stores::redist::ensure(&game, None, &mut |dep, i, n| println!("installing {dep} ({}/{n}) at {} s", i + 1, t.elapsed().as_secs()));
    println!("result {r:?} after {} s", t.elapsed().as_secs());
    println!("in the prefix now: {:?}", rare_stores::redist::installed(&game));
    println!("still needed: {:?}", rare_stores::redist::needed(&game, &rare_stores::redist::deps_of(&game)));
}
