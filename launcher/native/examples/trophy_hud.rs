//! Device check for trophy pop-ups over a game: draws one unlock on the HUD as the launcher does
//! (rare_system::hud with Rare's badge or a store icon), asks gamescope for a full-composition
//! screenshot while it is up, and waits for it to go.
//!   trophy_hud <out.png> [title] [meta] [icon: badge kind or https url]
use std::time::{Duration, SystemTime};

struct H(String);

impl rare_system::host::Host for H {
    fn gamescope_active(&self) -> bool {
        true
    }
    fn gamescope_base_display(&self) -> String {
        self.0.clone()
    }
    fn launcher_front(&self) -> bool {
        false
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let display = rare_overlay::gamescope::base_display();
    rare_system::host::set(H(display.clone()));
    let title = a.get(1).cloned().unwrap_or_else(|| "A week of Hades II".into());
    let meta = a.get(2).cloned().unwrap_or_else(|| "Hades II \u{b7} Silver \u{b7} 30 points".into());
    let src = match a.get(3).map(String::as_str) {
        Some(u) if u.starts_with("https://") => u.to_string(),
        k => rare_trophies::badge::data_uri(rare_trophies::milestones::Kind::from_key(k.unwrap_or("streak7")).unwrap_or(rare_trophies::milestones::Kind::Streak7), "steam:1145350"),
    };
    let icon = rare_native::features::trophies::hud_icon(&src);
    println!("icon {}", if icon.is_some() { "ok" } else { "missing" });
    rare_system::hud::toast_icon(&title, Some(&meta), icon);
    std::thread::sleep(Duration::from_millis(900));
    let file = std::path::Path::new(rare_overlay::gamescope::SCREENSHOT_FILE);
    let before = std::fs::metadata(file).and_then(|m| m.modified()).ok();
    let asked = SystemTime::now();
    // 3 = full composition: the game and every overlay, the HUD included.
    rare_overlay::gamescope::set(&display, rare_overlay::gamescope::SCREENSHOT, 3).expect("screenshot request");
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(50));
        let m = std::fs::metadata(file).and_then(|m| m.modified()).ok();
        if m.is_some() && m != before && m.unwrap() >= asked && rare_overlay::gamescope::get(&display, rare_overlay::gamescope::SCREENSHOT).is_empty() {
            std::thread::sleep(Duration::from_millis(300));
            if let Some(out) = a.first() {
                std::fs::copy(file, out).expect("copy");
                println!("screenshot {out}");
            }
            break;
        }
    }
    let _ = rare_overlay::gamescope::remove(&display, rare_overlay::gamescope::SCREENSHOT);
    std::thread::sleep(Duration::from_secs(5));
}
