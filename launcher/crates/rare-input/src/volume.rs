//! Volume keys, whatever has the screen, and Rare + volume for brightness.
//!
//! On the SuiPlay the volume keys sit on the AT keyboard, which InputPlumber grabs; they come out
//! on its virtual "InputPlumber Keyboard" as KEY_VOLUMEUP / KEY_VOLUMEDOWN (while a Rare surface
//! intercepts the pad they come as D-Bus actions instead, and ip.rs calls `key`). LC
//! and RC are D-Bus actions too (ip.rs, side.rs). In Steam's session
//! Steam handles volume. gamescope sends volume keys only to server 0's focused
//! window, never to the game (wlserver.cpp, "Always send volume+/- to root server only"); in
//! Rare's session that is the launcher's window, and the page does nothing with them. So nobody
//! changed the volume.
//!
//! This reads that keyboard's evdev node (read-only, no grab, found by name and found again when
//! InputPlumber re-creates it) and steps the default sink by STEP percent with wpctl, with an
//! accelerating repeat while held, never above 100; with the Rare button (Guide) held the same
//! keys step the screen brightness. The HUD (rare-system's hud) shows the new level over whatever is in
//! front. Only in Rare OS game mode (Rare is gamescope's session
//! client): in Steam's session Steam handles the keys and on a desktop the desktop does.

use std::time::Duration;

pub const KEY_MUTE: u16 = 113;
pub const KEY_VOLUMEDOWN: u16 = 114;
pub const KEY_VOLUMEUP: u16 = 115;
/// Percent per step.
pub const STEP: u8 = 5;
/// Held this long before the first repeat.
pub const REPEAT_DELAY: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Mute,
}

/// One `struct input_event` (24 bytes on 64-bit): Some((key, pressed)) for a system key press
/// or release; the kernel's own autorepeat (value 2) is ignored, the repeat is timed here.
pub fn key_event(ev: &[u8]) -> Option<(Key, bool)> {
    if ev.len() < 24 {
        return None;
    }
    let ty = u16::from_ne_bytes([ev[16], ev[17]]);
    let code = u16::from_ne_bytes([ev[18], ev[19]]);
    let val = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
    if ty != 1 || val == 2 {
        return None;
    }
    let key = match code {
        KEY_VOLUMEUP => Key::Up,
        KEY_VOLUMEDOWN => Key::Down,
        KEY_MUTE => Key::Mute,
        _ => return None,
    };
    Some((key, val == 1))
}

/// The next level: onto the STEP grid, 0..=100 (a level above 100 comes back to 100).
pub fn step(cur: u8, up: bool) -> u8 {
    let cur = cur.min(100);
    if up {
        ((cur / STEP + 1) * STEP).min(100)
    } else if cur % STEP == 0 {
        cur.saturating_sub(STEP)
    } else {
        cur - cur % STEP
    }
}

/// Time to the next repeat after `n` repeats: 160 ms easing to 60 ms.
pub fn repeat_interval(n: u32) -> Duration {
    Duration::from_millis(160u64.saturating_sub(n as u64 * 20).max(60))
}

/// Event nodes of keyboards that can send volume keys: InputPlumber's virtual keyboard by name.
pub fn parse_devices(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        if !block.lines().any(|l| l.starts_with("N: Name=") && l.contains("\"InputPlumber Keyboard\"")) {
            continue;
        }
        for l in block.lines().filter(|l| l.starts_with("H: Handlers=")) {
            out.extend(l.split_whitespace().filter(|w| w.starts_with("event")).map(|w| format!("/dev/input/{w}")));
        }
    }
    out
}

#[cfg(target_os = "linux")]
mod run {
    use super::*;
    use std::collections::HashMap;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::time::Instant;

    /// The level (percent, may be above 100 when set elsewhere) and mute state as last read or
    /// set, and when.
    struct Level {
        level: u8,
        muted: bool,
        at: Instant,
    }

    /// The bash stopgap the owner ran before this existed; while it runs, it does the stepping.
    fn stopgap_running() -> bool {
        let Ok(rd) = std::fs::read_dir("/proc") else { return false };
        rd.flatten().any(|e| std::fs::read(e.path().join("cmdline")).is_ok_and(|c| String::from_utf8_lossy(&c).contains("rare-volkeys")))
    }

    fn read_level() -> Option<Level> {
        let l = rare_system::system::levels();
        // Not capped: a level above 100 is brought back to 100 by the next step.
        Some(Level { level: l.volume?, muted: l.muted, at: Instant::now() })
    }

    fn set_mute(on: bool) {
        let _ = rare_system::system::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", if on { "1" } else { "0" }]);
    }

    /// One volume state for both sources (this reader and ip.rs while intercepting).
    static VOL: std::sync::Mutex<Volume> = std::sync::Mutex::new(Volume { cached: None, verify_at: None, stopgap: None });

    fn vol() -> std::sync::MutexGuard<'static, Volume> {
        VOL.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One press from another source (ip.rs while intercepting).
    pub fn key(k: Key) {
        vol().press(k);
    }

    struct Volume {
        cached: Option<Level>,
        /// After a step: when to check that nobody else (Steam) stepped on the same key.
        verify_at: Option<Instant>,
        stopgap: Option<(bool, Instant)>,
    }

    impl Volume {
        fn current(&mut self) -> Option<&mut Level> {
            // Within a burst of presses use what was set (no wpctl read per step); otherwise read,
            // the level may have changed from quick settings or an app.
            if !self.cached.as_ref().is_some_and(|c| c.at.elapsed() <= Duration::from_secs(3)) {
                self.cached = read_level();
            }
            self.cached.as_mut()
        }

        fn stopgap(&mut self) -> bool {
            match self.stopgap {
                Some((on, at)) if at.elapsed() < Duration::from_secs(10) => on,
                _ => {
                    let on = stopgap_running();
                    if on {
                        crate::log("[volume] rare-volkeys is running: it handles the keys, the launcher only shows the level");
                    }
                    self.stopgap = Some((on, Instant::now()));
                    on
                }
            }
        }

        /// Rare + volume: the screen brightness, on the same grid.
        fn brightness(&mut self, up: bool) {
            let Some(cur) = rare_system::brightness::current() else {
                crate::log("[volume] no adjustable screen");
                return;
            };
            let next = step(cur, up);
            if let Err(e) = rare_system::brightness::set(next) {
                crate::log(&format!("[volume] brightness {next}%: {e}"));
            }
            rare_system::hud::bar(rare_system::hud::Bar::Brightness { level: next });
        }

        fn press(&mut self, key: Key) {
            if matches!(key, Key::Up | Key::Down) && crate::chords::guide_held() {
                crate::chords::guide_chord_used();
                self.brightness(key == Key::Up);
                return;
            }
            if self.stopgap() {
                // Show what the stopgap set, a moment later.
                std::thread::sleep(Duration::from_millis(60));
                self.cached = None;
                if let Some(l) = self.current() {
                    rare_system::hud::bar(rare_system::hud::Bar::Volume { level: l.level.min(100), muted: l.muted });
                }
                return;
            }
            let Some(l) = self.current() else {
                crate::log("[volume] no default sink (wpctl get-volume failed)");
                return;
            };
            match key {
                Key::Mute => {
                    l.muted = !l.muted;
                    set_mute(l.muted);
                }
                Key::Up | Key::Down => {
                    let up = key == Key::Up;
                    let next = step(l.level, up);
                    if up && l.muted {
                        l.muted = false;
                        set_mute(false);
                    }
                    if next != l.level {
                        if let Err(e) = rare_system::system::set_volume(next) {
                            crate::log(&format!("[volume] set {next}%: {e}"));
                        }
                    }
                    l.level = next;
                }
            }
            l.at = Instant::now();
            let (level, muted) = (l.level, l.muted);
            rare_system::hud::bar(rare_system::hud::Bar::Volume { level: level.min(100), muted });
            self.verify_at = Some(Instant::now() + Duration::from_millis(350));
        }

        /// Steam, if its window had the key too, steps on its own: put ours back.
        fn verify(&mut self) {
            self.verify_at = None;
            let Some(want) = self.cached.as_ref().map(|c| c.level) else { return };
            if let Some(now) = read_level() {
                if now.level != want && !now.muted {
                    crate::log(&format!("[volume] something else also changed the volume ({want}% -> {}%); keeping {want}%", now.level));
                    let _ = rare_system::system::set_volume(want);
                    rare_system::hud::bar(rare_system::hud::Bar::Volume { level: want.min(100), muted: now.muted });
                }
            }
        }
    }

    pub fn start() {
        std::thread::spawn(move || {
            let mut open: HashMap<String, std::fs::File> = HashMap::new();
            // The key held, since when, its repeats so far and when the next is due.
            let mut held: Option<(Key, u32, Instant)> = None;
            let mut said: Vec<String> = Vec::new();
            let mut scanned: Option<Instant> = None;
            loop {
                // InputPlumber re-creates its keyboard (a target change, a restart): look again.
                if !scanned.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
                    scanned = Some(Instant::now());
                    let nodes = parse_devices(&std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default());
                    open.retain(|p, _| nodes.contains(p));
                    for n in nodes {
                        if open.contains_key(&n) {
                            continue;
                        }
                        match std::fs::File::open(&n) {
                            Ok(f) => {
                                crate::log(&format!("[syskeys] reading system keys from {n}"));
                                open.insert(n, f);
                            }
                            Err(e) if !said.contains(&n) => {
                                crate::log(&format!("[syskeys] can't read {n}: {e}"));
                                said.push(n);
                            }
                            Err(_) => {}
                        }
                    }
                }
                let now = Instant::now();
                let mut due = now + Duration::from_secs(2);
                if let Some((_, _, at)) = held {
                    due = due.min(at);
                }
                if let Some(t) = vol().verify_at {
                    due = due.min(t);
                }
                let paths: Vec<String> = open.keys().cloned().collect();
                let mut fds: Vec<libc::pollfd> = paths.iter().map(|p| libc::pollfd { fd: open[p].as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
                let timeout = due.saturating_duration_since(now).as_millis() as i32;
                // SAFETY: fds is a valid array of pollfd for the duration of the call.
                let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
                if n > 0 {
                    for (i, pfd) in fds.iter().enumerate() {
                        if pfd.revents == 0 {
                            continue;
                        }
                        let mut buf = [0u8; 24 * 16];
                        let got = open.get_mut(&paths[i]).map(|f| f.read(&mut buf));
                        match got {
                            Some(Ok(len)) if len >= 24 => {
                                for ev in buf[..len].chunks_exact(24) {
                                    match key_event(ev) {
                                        Some((k, true)) => {
                                            vol().press(k);
                                            held = (k != Key::Mute).then(|| (k, 0, Instant::now() + REPEAT_DELAY));
                                        }
                                        Some((k, false)) => {
                                            if held.is_some_and(|(h, _, _)| h == k) {
                                                held = None;
                                            }
                                        }
                                        None => {}
                                    }
                                }
                            }
                            _ => {
                                // Gone (ENODEV): drop it, the next scan finds its successor.
                                open.remove(&paths[i]);
                                held = None;
                                scanned = None;
                            }
                        }
                    }
                } else if n < 0 && open.is_empty() {
                    std::thread::sleep(Duration::from_millis(500));
                }
                let now = Instant::now();
                if let Some((k, reps, at)) = held {
                    if now >= at {
                        vol().press(k);
                        held = Some((k, reps + 1, Instant::now() + repeat_interval(reps + 1)));
                    }
                }
                let due = vol().verify_at.is_some_and(|t| now >= t);
                if held.is_none() && due {
                    vol().verify();
                }
            }
        });
    }
}

/// Starts the reader (Linux only). Only in Rare OS game mode (Rare is gamescope's session
/// client): in Steam's session Steam handles the keys and on a desktop the desktop does; the front
/// end checks that before calling.
pub fn start() {
    #[cfg(target_os = "linux")]
    run::start();
}

/// A volume key press from another source (ip.rs while it intercepts the pad): the
/// same steps, brightness chord and HUD bar as the keyboard node.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn key(k: Key) {
    #[cfg(target_os = "linux")]
    run::key(k);
    #[cfg(not(target_os = "linux"))]
    let _ = k;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ty: u16, code: u16, val: i32) -> [u8; 24] {
        let mut b = [0u8; 24];
        b[16..18].copy_from_slice(&ty.to_ne_bytes());
        b[18..20].copy_from_slice(&code.to_ne_bytes());
        b[20..24].copy_from_slice(&val.to_ne_bytes());
        b
    }

    #[test]
    fn decodes_volume_keys() {
        assert_eq!(key_event(&ev(1, 115, 1)), Some((Key::Up, true)));
        assert_eq!(key_event(&ev(1, 115, 0)), Some((Key::Up, false)));
        assert_eq!(key_event(&ev(1, 114, 1)), Some((Key::Down, true)));
        assert_eq!(key_event(&ev(1, 113, 1)), Some((Key::Mute, true)));
        assert_eq!(key_event(&ev(1, 185, 1)), None, "LC is a D-Bus action, not a key");
        assert_eq!(key_event(&ev(1, 115, 2)), None, "kernel autorepeat is ignored");
        assert_eq!(key_event(&ev(1, 30, 1)), None);
        assert_eq!(key_event(&ev(0, 0, 0)), None);
        assert_eq!(key_event(&[0u8; 8]), None);
    }

    #[test]
    fn steps_on_a_five_percent_grid_within_0_and_100() {
        assert_eq!(step(50, true), 55);
        assert_eq!(step(47, true), 50);
        assert_eq!(step(97, true), 100);
        assert_eq!(step(100, true), 100);
        assert_eq!(step(130, true), 100, "never above 100");
        assert_eq!(step(50, false), 45);
        assert_eq!(step(47, false), 45);
        assert_eq!(step(3, false), 0);
        assert_eq!(step(0, false), 0);
        assert_eq!(step(130, false), 95);
    }

    #[test]
    fn repeat_accelerates() {
        assert_eq!(repeat_interval(0), Duration::from_millis(160));
        assert_eq!(repeat_interval(2), Duration::from_millis(120));
        assert_eq!(repeat_interval(5), Duration::from_millis(60));
        assert_eq!(repeat_interval(50), Duration::from_millis(60));
        // Holding from 0 reaches 100 in about two seconds: 20 steps.
        let mut t = REPEAT_DELAY;
        for n in 1..20 {
            t += repeat_interval(n);
        }
        assert!(t < Duration::from_millis(2000), "{t:?}");
    }

    #[test]
    fn finds_the_inputplumber_keyboard() {
        let t = "I: Bus=0011 Vendor=0001 Product=0001 Version=ab83\nN: Name=\"AT Translated Set 2 keyboard\"\nH: Handlers=sysrq kbd leds event1 \n\nI: Bus=0003 Vendor=1234 Product=5678 Version=0111\nN: Name=\"InputPlumber Keyboard\"\nH: Handlers=sysrq kbd event10 \n\nN: Name=\"InputPlumber Mouse\"\nH: Handlers=mouse1 event12 \n";
        assert_eq!(parse_devices(t), vec!["/dev/input/event10"]);
        assert!(parse_devices("N: Name=\"Power Button\"\nH: Handlers=kbd event0\n").is_empty());
    }
}
