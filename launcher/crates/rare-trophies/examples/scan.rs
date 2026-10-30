//! Prints what the trophy case gets from a Steam folder (default: this device's), with Steam's
//! global percentages and the tiers and points they give.
//!   cargo run -p rare-trophies --example scan [steam root] [appid]
use rare_trophies::{rarity, score, steam};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next();
    let only = args.next().and_then(|a| a.parse().ok());
    let games = match &root {
        Some(r) => steam::scan_root(std::path::Path::new(r), only),
        None => steam::scan(only),
    };
    for mut g in games {
        if let Some(r) = rarity::steam(g.appid) {
            rarity::apply(&mut g, &r);
        }
        let pts: u32 = g.achievements.iter().filter(|a| a.unlocked).map(|a| score::points(score::tier_for_rarity(a.global_percent.map(f64::from)))).sum();
        println!("{} {:?}: {} of {} unlocked, {} points, source {}", g.appid, g.name, g.unlocked, g.total, pts, g.source);
        for a in g.achievements.iter().filter(|a| a.unlocked).take(5) {
            let t = score::tier_for_rarity(a.global_percent.map(f64::from));
            println!("   {} {:?} {:?}% {} at {:?} icon {}", a.api_name, a.name, a.global_percent, score::tier_name(t), a.unlock_time, a.icon_url.is_some());
        }
    }
}
