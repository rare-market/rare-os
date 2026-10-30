//! The store sign-in window (Settings > Accounts, features/stores.rs) against the real web host:
//! opens Epic's code page signed out, where the login script must turn the page into the
//! `rare.invalid/store-login` navigation the watcher takes; then GOG's login page, logging what it
//! navigates to. Linux, with an X server (Xvfb is enough).
//!
//!   RARE_WEBGAME_HOST=<rare-webgame-host> cargo run --example sign_in_smoke

use rare_core::{Ctx, Event, NoHost};
use rare_stores::{epic, gog, login_code, Store};
use rare_webgame::process::Spawner;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Launcher;
impl rare_webgame::WebHost for Launcher {
    fn main_exists(&self) -> bool {
        true
    }
}

fn watch(store: Store, url: &str, script: &str, secs: u64) -> bool {
    let got = Arc::new(AtomicBool::new(false));
    let g = got.clone();
    let nav = Arc::new(move |u: &str| -> bool {
        println!("{:?} navigation: {}", store, &u[..u.len().min(140)]);
        match login_code(store, u) {
            Some(code) => {
                println!("{store:?}: code taken ({} chars)", code.len());
                g.store(true, Ordering::Relaxed);
                false
            }
            None => true,
        }
    });
    rare_webgame::open_sign_in("Sign in", url, script, nav, Arc::new(|| println!("closed"))).expect("open");
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) && !got.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
    }
    rare_webgame::close_external();
    std::thread::sleep(Duration::from_millis(500));
    got.load(Ordering::Relaxed)
}

fn main() {
    let bin = std::env::var("RARE_WEBGAME_HOST").unwrap_or_else(|_| "rare-webgame-host".into());
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(Into::into).unwrap_or_else(std::env::temp_dir);
    let ctx = Ctx::new(Arc::new(|_e: Event| {}), Arc::new(NoHost));
    rare_webgame::install(ctx, Arc::new(Spawner::new(bin.into(), vec![], dir)), Arc::new(Launcher));
    // Signed out, Epic's code page answers with a null code: the script still sends the navigation.
    let redirect = "https://www.epicgames.com/id/api/redirect?clientId=34a02cf8f4414e29b15921876da36f9a&responseType=code";
    let epic_ok = watch(Store::Epic, redirect, epic::LOGIN_SCRIPT, 25);
    println!("epic script navigation taken: {epic_ok}");
    let _ = watch(Store::Gog, gog::LOGIN_URL, "", 12);
    let _ = watch(Store::Epic, epic::LOGIN_URL, epic::LOGIN_SCRIPT, 12);
}
