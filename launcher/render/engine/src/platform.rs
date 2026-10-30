//! Session glue: the gamescope app id on our X11 window, the clock and the battery.

use winit::window::Window;

/// Rare's gamescope app id (overlay/gamescope.rs RARE_APPID). With `--steam`, gamescope only
/// focuses windows that carry one.
pub const RARE_APPID: u32 = 1380012613;

/// Sets STEAM_GAME on our X11 window, the way every Steam game is tagged. Returns what happened.
pub fn tag_window(window: &Window, appid: u32) -> String {
    #[cfg(target_os = "linux")]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
        use x11rb::wrapper::ConnectionExt as _;
        let xid: u32 = match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Xlib(h)) => h.window as u32,
            Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
            Ok(RawWindowHandle::Wayland(_)) => return "wayland window: app id comes from the cgroup scope".into(),
            _ => return "no window handle".into(),
        };
        let r = (|| -> Result<(), String> {
            let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
            let atom = conn.intern_atom(false, b"STEAM_GAME").map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom;
            conn.change_property32(PropMode::REPLACE, xid, atom, AtomEnum::CARDINAL, &[appid]).map_err(|e| e.to_string())?;
            conn.sync().map_err(|e| e.to_string())?;
            Ok(())
        })();
        match r {
            Ok(()) => format!("x11 window {xid:#x}: STEAM_GAME={appid}"),
            Err(e) => format!("x11 window {xid:#x}: STEAM_GAME failed: {e}"),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window, appid);
        "not linux".into()
    }
}

/// "12:53 PM" in local time.
pub fn clock_text() -> String {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r fills tm from t.
    unsafe { libc::localtime_r(&t, &mut tm) };
    let h = tm.tm_hour % 12;
    format!("{}:{:02} {}", if h == 0 { 12 } else { h }, tm.tm_min, if tm.tm_hour < 12 { "AM" } else { "PM" })
}

/// Seconds until the minute changes (the clock's only wake-up).
pub fn secs_to_next_minute() -> f32 {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    (60 - (t % 60)) as f32 + 0.05
}

pub fn battery_percent() -> Option<u8> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if std::fs::read_to_string(p.join("type")).ok()?.trim() == "Battery" {
            return std::fs::read_to_string(p.join("capacity")).ok()?.trim().parse().ok();
        }
    }
    None
}

/// Resident set size in KiB.
pub fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()))
        .unwrap_or(0)
}
