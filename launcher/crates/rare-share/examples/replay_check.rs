//! Device check for instant replay: runs the recorder supervisor as the launcher does (a game
//! "running", replay on), waits, saves the last REPLAY_SECS (30) seconds and prints the capture.
//! Nothing shows on screen. KEEP=1 keeps the clip; otherwise it is deleted after the check.
//!
//!   REPLAY_WAIT=40 REPLAY_SECS=30 cargo run --example replay_check

use rare_share::{ShareHost, Kind};
use std::sync::Arc;
use std::time::Duration;

struct Host(u32);

impl ShareHost for Host {
    fn clips_enabled(&self) -> bool {
        true
    }
    fn replay_seconds(&self) -> u32 {
        self.0
    }
    fn clips_cap_bytes(&self) -> Option<u64> {
        None
    }
    fn running(&self) -> Vec<String> {
        vec!["check:replay".into()]
    }
    fn title_of(&self, _id: &str) -> Option<String> {
        Some("Replay check".into())
    }
    fn rumble_enabled(&self) -> bool {
        false
    }
    fn rumble(&self) {}
    fn launcher_front(&self) -> bool {
        true
    }
    fn toast(&self, _text: &str) {}
}

fn main() {
    let secs: u32 = std::env::var("REPLAY_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    let wait: u64 = std::env::var("REPLAY_WAIT").ok().and_then(|s| s.parse().ok()).unwrap_or(secs as u64 + 8);
    let host = Arc::new(Host(secs));
    let st = rare_share::status(&*host);
    println!("status: {}", serde_json::to_string(&st).unwrap());
    rare_share::start(host.clone());
    for i in 0..wait {
        std::thread::sleep(Duration::from_secs(1));
        if i % 10 == 9 {
            println!("{}s: recording with {:?}", i + 1, rare_share::clip::recording().map(|b| b.name()));
        }
    }
    let r = rare_share::save_clip(&*host);
    rare_share::shutdown();
    match r {
        Ok(c) => {
            println!("saved: {}", serde_json::to_string(&c).unwrap());
            let listed = rare_share::list().into_iter().find(|x| x.id == c.id);
            println!("in Share: {} (game {:?})", listed.is_some(), listed.and_then(|x| x.game.map(|g| g.name)));
            assert_eq!(c.kind, Kind::Clip);
            if std::env::var_os("KEEP").is_none() {
                let _ = rare_share::delete(&c.id);
                println!("deleted {}", c.id);
            }
        }
        Err(e) => {
            println!("save failed: {e}");
            std::process::exit(1);
        }
    }
}
