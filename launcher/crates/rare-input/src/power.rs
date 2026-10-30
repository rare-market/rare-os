//! The power button, whatever has the screen: a press sleeps, a hold of LONG_PRESS opens the power
//! menu (Sleep, Restart, Turn off) over anything.
//!
//! Deck-style images set logind's HandlePowerKey=ignore and leave the key to steamos-powerbuttond,
//! which only forwards presses to Steam (steam://shortpowerpress). In Rare's session Steam is not
//! the session's UI and may not run at all, so Rare owns the key: this reads the evdev "Power
//! Button" device (read-only, no grab) and the image no longer starts steamos-powerbuttond with
//! Rare's session (image/build/build.sh). Images from before that change still start it; while
//! it runs and Steam runs too, Steam gets the press and this stays out of the way, so there is
//! never a double suspend. evdev rather than a steam:// handler: Steam owns that scheme, and the
//! device is readable by the session user (the pads are read from /dev/input as the same user).

use std::time::Duration;

/// Held at least this long counts as a long press (steamos-powerbuttond uses about a second).
pub const LONG_PRESS: Duration = Duration::from_millis(1000);

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Press {
    Short,
    Long,
}

/// Event nodes (`/dev/input/eventN`) of devices called "Power Button" in /proc/bus/input/devices.
pub fn parse_devices(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let power = block.lines().any(|l| l.starts_with("N: Name=") && l.contains("\"Power Button\""));
        if !power {
            continue;
        }
        for l in block.lines().filter(|l| l.starts_with("H: Handlers=")) {
            out.extend(l.split_whitespace().filter(|w| w.starts_with("event")).map(|w| format!("/dev/input/{w}")));
        }
    }
    out
}

/// Whether logind is told to ignore the power key (so nobody else suspends).
pub fn logind_ignores(confs: &[String]) -> bool {
    let mut ignore = false;
    for c in confs {
        for l in c.lines() {
            if let Some(v) = l.trim().strip_prefix("HandlePowerKey=") {
                ignore = v.trim() == "ignore";
            }
        }
    }
    ignore
}

/// KEY_POWER press/release of one `struct input_event` (24 bytes on 64-bit): Some(true) on press.
pub fn key_power(ev: &[u8]) -> Option<bool> {
    if ev.len() < 24 {
        return None;
    }
    let ty = u16::from_ne_bytes([ev[16], ev[17]]);
    let code = u16::from_ne_bytes([ev[18], ev[19]]);
    let val = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
    (ty == 1 && code == 116 && val != 2).then_some(val == 1)
}

/// Whether Rare acts on the press: always, except on an older image where steamos-powerbuttond
/// still runs and forwards it to a running Steam.
pub fn rare_handles(steam_running: bool, powerbuttond_running: bool) -> bool {
    !(steam_running && powerbuttond_running)
}

#[cfg(target_os = "linux")]
fn powerbuttond_running() -> bool {
    let Ok(rd) = std::fs::read_dir("/proc") else { return false };
    rd.flatten().any(|e| std::fs::read(e.path().join("cmdline")).is_ok_and(|c| String::from_utf8_lossy(&c).contains("steamos-powerbuttond")))
}

/// Short or long, from how long the key was held.
pub fn classify(held: Duration) -> Press {
    if held >= LONG_PRESS { Press::Long } else { Press::Short }
}

/// Reads the power button on its own threads (Linux only). A press sleeps; a hold calls
/// `power_menu` (the power menu over whatever is in front). Nothing when logind handles the key.
#[cfg(target_os = "linux")]
pub fn start(power_menu: impl Fn() + Send + Sync + Clone + 'static) {
    use std::io::Read;
    use std::time::Instant;

    let handle = move |p: Press| {
        if !rare_handles(rare_core::proc::steam_running(), powerbuttond_running()) {
            crate::log("[power] steamos-powerbuttond and Steam have this press");
            return;
        }
        match p {
            Press::Short => {
                crate::log("[power] press: sleep");
                if let Err(e) = rare_system::system::power("suspend") {
                    log::warn!("power button: suspend failed: {e}");
                }
            }
            Press::Long => power_menu(),
        }
    };
    std::thread::spawn(move || {
        let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
        let mut confs = vec![read("/usr/lib/systemd/logind.conf")];
        for dir in ["/usr/lib/systemd/logind.conf.d", "/etc/systemd/logind.conf.d"] {
            let mut names: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
            names.sort();
            confs.extend(names.iter().map(|p| read(&p.to_string_lossy())));
        }
        if !logind_ignores(&confs) {
            log::info!("power button: logind handles it, nothing to do");
            return;
        }
        let devs = parse_devices(&read("/proc/bus/input/devices"));
        for path in devs {
            let handle = handle.clone();
            std::thread::spawn(move || {
                let Ok(mut f) = std::fs::File::open(&path) else {
                    log::warn!("power button: can't read {path}");
                    return;
                };
                let fd = std::os::fd::AsRawFd::as_raw_fd(&f);
                let mut down: Option<Instant> = None;
                let mut long_sent = false;
                let mut buf = [0u8; 24];
                loop {
                    // While held, wake at the long-press mark to fire it without waiting for the release.
                    let timeout = match down {
                        Some(t) if !long_sent => LONG_PRESS.saturating_sub(t.elapsed()).as_millis() as i32 + 1,
                        _ => -1,
                    };
                    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
                    // SAFETY: one valid pollfd.
                    let n = unsafe { libc::poll(&mut pfd, 1, timeout) };
                    if n == 0 {
                        long_sent = true;
                        handle(Press::Long);
                        continue;
                    }
                    if n < 0 {
                        continue;
                    }
                    if f.read_exact(&mut buf).is_err() {
                        return;
                    }
                    match key_power(&buf) {
                        Some(true) => {
                            down = Some(Instant::now());
                            long_sent = false;
                        }
                        Some(false) => {
                            if let Some(t) = down.take() {
                                if !long_sent {
                                    handle(classify(t.elapsed()));
                                }
                            }
                        }
                        None => {}
                    }
                }
            });
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn start(_power_menu: impl Fn() + Send + Sync + Clone + 'static) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_power_button_nodes() {
        let t = "I: Bus=0019 Vendor=0000 Product=0001 Version=0000\nN: Name=\"Power Button\"\nH: Handlers=kbd event0 \nB: EV=3\n\nI: Bus=0011\nN: Name=\"AT Translated Set 2 keyboard\"\nH: Handlers=sysrq kbd event3 leds\n\nI: Bus=0019\nN: Name=\"Power Button\"\nH: Handlers=kbd event1 \n";
        assert_eq!(parse_devices(t), vec!["/dev/input/event0", "/dev/input/event1"]);
    }

    #[test]
    fn logind_override_order() {
        assert!(logind_ignores(&["[Login]\nHandlePowerKey=ignore\n".into()]));
        assert!(!logind_ignores(&["HandlePowerKey=ignore".into(), "HandlePowerKey=poweroff".into()]));
        assert!(!logind_ignores(&["#HandlePowerKey=poweroff".into()]));
    }

    #[test]
    fn decodes_events() {
        let mk = |ty: u16, code: u16, val: i32| {
            let mut b = [0u8; 24];
            b[16..18].copy_from_slice(&ty.to_ne_bytes());
            b[18..20].copy_from_slice(&code.to_ne_bytes());
            b[20..24].copy_from_slice(&val.to_ne_bytes());
            b
        };
        assert_eq!(key_power(&mk(1, 116, 1)), Some(true));
        assert_eq!(key_power(&mk(1, 116, 0)), Some(false));
        assert_eq!(key_power(&mk(1, 116, 2)), None);
        assert_eq!(key_power(&mk(1, 30, 1)), None);
        assert_eq!(key_power(&mk(0, 0, 0)), None);
    }

    #[test]
    fn rare_owns_the_key_unless_an_old_image_hands_it_to_steam() {
        assert!(rare_handles(false, false));
        assert!(rare_handles(true, false), "Steam running, no powerbuttond: Rare sleeps");
        assert!(rare_handles(false, true), "powerbuttond with no Steam forwards to nobody");
        assert!(!rare_handles(true, true));
    }

    #[test]
    fn short_and_long() {
        assert_eq!(classify(Duration::from_millis(200)), Press::Short);
        assert_eq!(classify(Duration::from_millis(1000)), Press::Long);
    }
}
