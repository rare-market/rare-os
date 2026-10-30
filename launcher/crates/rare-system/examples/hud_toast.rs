//! Draws one HUD toast over whatever has the screen, as the launcher does for a notice over a game
//! (device check): `hud_toast [display] [title] [body]`, then waits for it to go.

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
    let a: Vec<String> = std::env::args().collect();
    let display = a.get(1).cloned().unwrap_or_else(|| ":0".into());
    rare_system::host::set(H(display));
    rare_system::hud::toast(a.get(2).map(String::as_str).unwrap_or("bee: live?"), Some(a.get(3).map(String::as_str).unwrap_or("Message")));
    std::thread::sleep(std::time::Duration::from_secs(7));
}
