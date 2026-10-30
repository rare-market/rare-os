//! Device check for streaming this device: starts Sunshine (RARE_SUNSHINE) with Rare's config as the
//! launcher does, waits for a pairing request from Moonlight, types PIN (PIN=1234) through Rare's API
//! call, lists the paired devices, then (UNPAIR=1) removes them and stops Sunshine.
//!
//!   RARE_SUNSHINE=/path/sunshine PIN=4821 cargo run --example sunshine_check

use rare_stream::host;
use std::time::{Duration, Instant};

fn main() {
    let pin = std::env::var("PIN").unwrap_or_else(|_| "1234".into());
    let wait = Duration::from_secs(std::env::var("WAIT").ok().and_then(|s| s.parse().ok()).unwrap_or(120));
    let gamescope = std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some();
    let env = |_: &mut std::process::Command| {};
    host::cleanup_stale();
    let t = Instant::now();
    let mut asked = false;
    let mut paired = false;
    while t.elapsed() < wait {
        host::tick(true, "SuiPlay", gamescope, &env);
        host::poll_pending();
        let st = host::status();
        if st.running && st.up_s >= 3 && !asked {
            println!("sunshine up ({} s), clients: {:?}", st.up_s, host::clients());
            asked = true;
            println!("waiting for Moonlight to ask (pin_wanted), then PIN {pin}");
        }
        if asked && !paired && st.pin_wanted {
            println!("waiting: {:?}", host::pending());
            match host::submit_pin(&pin, "Rare Moonlight") {
                Ok(()) => {
                    println!("PIN accepted");
                    paired = true;
                }
                Err(e) => println!("PIN: {e}"),
            }
        }
        if paired {
            std::thread::sleep(Duration::from_secs(3));
            println!("paired devices: {:?}", host::clients());
            break;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    if let Some(s) = std::env::var_os("HOLD") {
        let secs: u64 = s.to_string_lossy().parse().unwrap_or(30);
        println!("holding {secs} s");
        for _ in 0..secs {
            host::tick(true, "SuiPlay", gamescope, &env);
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    if std::env::var_os("UNPAIR").is_some() {
        for c in host::clients().unwrap_or_default() {
            println!("unpair {}: {:?}", c.name, host::unpair(&c.uuid));
        }
        println!("paired devices now: {:?}", host::clients());
    }
    println!("status: {:?}", host::status());
    host::shutdown();
}
