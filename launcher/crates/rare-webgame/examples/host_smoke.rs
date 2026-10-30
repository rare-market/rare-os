//! Drives the real web game host (launcher/native/webhost) the way the native launcher does: opens
//! a page, answers its Rare Wallet requests (connect with a test account, then rejects signing),
//! sends a pad press, closes. Linux, with an X server (Xvfb is enough).
//!
//!   RARE_WEBGAME_HOST=<rare-webgame-host> cargo run --example host_smoke -- <url> [seconds]

use rare_core::{Ctx, Event, NoHost, Pad};
use rare_webgame::process::Spawner;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

struct Launcher;
impl rare_webgame::WebHost for Launcher {
    fn main_exists(&self) -> bool {
        true
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "https://aresrpg.world/".into());
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(15);
    let bin = std::env::var("RARE_WEBGAME_HOST").unwrap_or_else(|_| "rare-webgame-host".into());
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(Into::into).unwrap_or_else(std::env::temp_dir);
    let sink = Arc::new(|e: Event| {
        let Event::Other { name, payload } = e else { return };
        println!("event {name}: {payload}");
        if name != "wallet-request" {
            return;
        }
        let id = payload["id"].as_u64().unwrap_or(0);
        let origin = payload["origin"].as_str().unwrap_or("").to_string();
        std::thread::spawn(move || match payload["method"].as_str() {
            Some("connect") => {
                rare_webgame::wallet_set_approved(vec![origin]);
                rare_webgame::wallet_reply(id, true, json!({ "accounts": [{ "address": format!("0x{}", "ab".repeat(32)), "label": "Test", "publicKey": Value::Null }] }));
            }
            _ => rare_webgame::wallet_reply(id, false, json!("User rejected the request")),
        });
    });
    let ctx = Ctx::new(sink, Arc::new(NoHost));
    rare_webgame::install(ctx, Arc::new(Spawner::new(bin.into(), vec![], dir)), Arc::new(Launcher));
    let t = std::time::Instant::now();
    rare_webgame::open("web:smoke", "Smoke", &url).expect("open");
    println!("opened in {} ms; visible {}", t.elapsed().as_millis(), rare_webgame::visible());
    std::thread::sleep(Duration::from_secs(secs / 2));
    rare_webgame::pad(Pad::A, true);
    rare_webgame::pad(Pad::A, false);
    std::thread::sleep(Duration::from_secs(secs - secs / 2));
    println!("current {:?}", rare_webgame::current_id());
    rare_webgame::close();
    std::thread::sleep(Duration::from_millis(500));
    println!("closed; current {:?}", rare_webgame::current_id());
}
