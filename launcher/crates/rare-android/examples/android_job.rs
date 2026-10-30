//! Runs one Android job end to end and prints its progress (a check on a test box or the device):
//!   cargo run -p rare-android --example android_job -- setup | install <package> <name> | remove <package> | list | search <words>
use rare_android::jobs::{self, Kind};
use std::time::Duration;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let kind = match a.first().map(String::as_str) {
        Some("setup") => Kind::Setup,
        Some("install") => Kind::Install { package: a[1].clone(), name: a.get(2).cloned().unwrap_or_else(|| a[1].clone()) },
        Some("remove") => Kind::Remove { package: a[1].clone(), name: a[1].clone() },
        Some("list") => {
            println!("status {:?}", rare_android::status());
            for g in rare_android::games() {
                println!("{}\t{}\t{:?}\t{:?}", g.id, g.title, g.size_bytes, g.art.icon);
            }
            println!("total {} bytes", rare_android::total_size());
            return;
        }
        Some("search") => {
            for h in rare_android::fdroid::search(&a[1..].join(" ")).unwrap() {
                println!("{}\t{}\t{}", h.package, h.name, h.summary);
            }
            return;
        }
        _ => {
            eprintln!("usage: android_job setup | install <package> [name] | remove <package> | list | search <words>");
            std::process::exit(2);
        }
    };
    rare_android::ensure_files().unwrap();
    let k = kind.clone();
    let h = std::thread::spawn(move || jobs::run_wait(k));
    let mut last = String::new();
    while !h.is_finished() {
        if let Some(j) = jobs::current() {
            let l = format!("{} {:?}", j.line, j.pct.map(|p| (p * 100.0).round()));
            if l != last {
                println!("{l}");
                last = l;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    println!("{kind:?}: {:?}", h.join().unwrap());
}
