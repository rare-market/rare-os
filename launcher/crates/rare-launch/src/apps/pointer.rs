//! The controller as a mouse and keyboard for apps that don't read a gamepad (Chrome,
//! Telegram Desktop, Discord). Rare already reads the pad itself (gilrs, also while another
//! window has focus), so while such an app is in front it writes mouse and key events to a
//! virtual device of its own through /dev/uinput.
//!
//! Why this and not the alternatives (docs/APPS.md has the details):
//! - Handheld Daemon has no mouse or keyboard output mode for the pad, only controller types.
//! - Steam Input applies one layout to the whole shortcut (Rare included) and only a game
//!   using the Steamworks input API can switch action sets.
//! - InputPlumber can load a mouse profile over D-Bus, but its polkit policy wants an admin
//!   password; the Rare OS image ships a rule for that and uses it (see the doc).
//!
//! /dev/uinput is open to the logged-in user on Bazzite (Steam's udev rule tags it uaccess), so
//! this needs no sudo.
//!
//! Layout: right stick moves the pointer (as in Rare's web games), left stick scrolls, d-pad
//! arrows, A left click, B back, X space, Y Steam's on-screen keyboard, LB/RB Shift+Tab/Tab,
//! LT right click, RT left click, Menu Enter, View Escape. Guide is never taken: it brings Rare
//! back, and holding it offers Close app.

use super::catalog::Back;
use rare_core::Pad;
use std::sync::Mutex;
use std::time::Instant;

pub mod code {
    // The device setup (Linux) is the only user of some of these.
    #![cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub const KEY_ESC: u16 = 1;
    pub const KEY_TAB: u16 = 15;
    pub const KEY_ENTER: u16 = 28;
    pub const KEY_LEFTSHIFT: u16 = 42;
    pub const KEY_LEFTALT: u16 = 56;
    pub const KEY_SPACE: u16 = 57;
    pub const KEY_UP: u16 = 103;
    pub const KEY_LEFT: u16 = 105;
    pub const KEY_RIGHT: u16 = 106;
    pub const KEY_DOWN: u16 = 108;
    pub const BTN_LEFT: u16 = 0x110;
    pub const BTN_RIGHT: u16 = 0x111;
    pub const BTN_MIDDLE: u16 = 0x112;
    pub const REL_X: u16 = 0x00;
    pub const REL_Y: u16 = 0x01;
    pub const REL_HWHEEL: u16 = 0x06;
    pub const REL_WHEEL: u16 = 0x08;
    pub const REL_WHEEL_HI_RES: u16 = 0x0b;
    pub const REL_HWHEEL_HI_RES: u16 = 0x0c;
    pub const KEYS: &[u16] = &[KEY_ESC, KEY_TAB, KEY_ENTER, KEY_LEFTSHIFT, KEY_LEFTALT, KEY_SPACE, KEY_UP, KEY_LEFT, KEY_RIGHT, KEY_DOWN, BTN_LEFT, BTN_RIGHT, BTN_MIDDLE];
    pub const RELS: &[u16] = &[REL_X, REL_Y, REL_HWHEEL, REL_WHEEL, REL_WHEEL_HI_RES, REL_HWHEEL_HI_RES];
}
use code::*;

/// What a button does in pointer mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Keys (or mouse buttons) held together, modifiers first; released in reverse.
    Keys(Vec<u16>),
    /// Steam's on-screen keyboard, which types into the window in front.
    Keyboard,
    /// Not taken: the launcher handles it (Guide, quick access).
    Pass,
}

pub fn action(b: Pad, back: Back) -> Action {
    let k = |c: &[u16]| Action::Keys(c.to_vec());
    match b {
        Pad::A | Pad::Rt => k(&[BTN_LEFT]),
        Pad::Lt => k(&[BTN_RIGHT]),
        Pad::B => match back {
            Back::AltLeft => k(&[KEY_LEFTALT, KEY_LEFT]),
            Back::Escape => k(&[KEY_ESC]),
        },
        Pad::X => k(&[KEY_SPACE]),
        Pad::Y => Action::Keyboard,
        Pad::Lb => k(&[KEY_LEFTSHIFT, KEY_TAB]),
        Pad::Rb => k(&[KEY_TAB]),
        Pad::Up => k(&[KEY_UP]),
        Pad::Down => k(&[KEY_DOWN]),
        Pad::Left => k(&[KEY_LEFT]),
        Pad::Right => k(&[KEY_RIGHT]),
        Pad::Start => k(&[KEY_ENTER]),
        Pad::Select => k(&[KEY_ESC]),
        Pad::Guide | Pad::GuideHold | Pad::ExitHold | Pad::Qam => Action::Pass,
    }
}

const DEADZONE: f32 = 0.15;
/// Pointer speed at full tilt, in pixels per second (the web games' stick pointer: 22 px a frame).
const POINTER_SPEED: f32 = 1400.0;
/// Scrolling at full tilt, in hi-res wheel units per second (120 = one notch).
const SCROLL_SPEED: f32 = 1800.0;

/// Curved response: fine control near the centre, fast at full tilt. 0 inside the deadzone.
fn curve(x: f32, y: f32) -> (f32, f32) {
    let mag = (x * x + y * y).sqrt();
    if mag < DEADZONE {
        return (0.0, 0.0);
    }
    let s = ((mag.min(1.0) - DEADZONE) / (1.0 - DEADZONE)).powf(1.8);
    (x / mag * s, y / mag * s)
}

/// Fractions carried between ticks, so slow movement still moves.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Carry {
    pub x: f32,
    pub y: f32,
    pub wheel: f32,
    pub hwheel: f32,
    /// Hi-res units not yet sent as whole notches (for apps that only read REL_WHEEL).
    pub notch: i32,
    pub hnotch: i32,
}

/// One tick of analog movement: relative events to send. `axes` is left x, left y, right x,
/// right y, down positive.
pub fn motion(axes: &[f32; 4], dt: f32, c: &mut Carry) -> Vec<(u16, i32)> {
    let mut out = Vec::new();
    let (px, py) = curve(axes[2], axes[3]);
    c.x += px * POINTER_SPEED * dt;
    c.y += py * POINTER_SPEED * dt;
    let (dx, dy) = (c.x.trunc() as i32, c.y.trunc() as i32);
    c.x -= dx as f32;
    c.y -= dy as f32;
    if dx != 0 {
        out.push((REL_X, dx));
    }
    if dy != 0 {
        out.push((REL_Y, dy));
    }
    let (sx, sy) = curve(axes[0], axes[1]);
    // Stick down scrolls down: the wheel counts up as away from the user.
    c.wheel += -sy * SCROLL_SPEED * dt;
    c.hwheel += sx * SCROLL_SPEED * dt;
    let (w, h) = (c.wheel.trunc() as i32, c.hwheel.trunc() as i32);
    c.wheel -= w as f32;
    c.hwheel -= h as f32;
    if w != 0 {
        out.push((REL_WHEEL_HI_RES, w));
        c.notch += w;
        let n = c.notch / 120;
        if n != 0 {
            out.push((REL_WHEEL, n));
            c.notch -= n * 120;
        }
    }
    if h != 0 {
        out.push((REL_HWHEEL_HI_RES, h));
        c.hnotch += h;
        let n = c.hnotch / 120;
        if n != 0 {
            out.push((REL_HWHEEL, n));
            c.hnotch -= n * 120;
        }
    }
    if sx == 0.0 && sy == 0.0 {
        c.notch = 0;
        c.hnotch = 0;
    }
    out
}

struct Mapper {
    dev: Option<uinput::Device>,
    failed: bool,
    carry: Carry,
    last: Option<Instant>,
}

static MAPPER: Mutex<Mapper> = Mutex::new(Mapper { dev: None, failed: false, carry: Carry { x: 0.0, y: 0.0, wheel: 0.0, hwheel: 0.0, notch: 0, hnotch: 0 }, last: None });

fn with_dev<F: FnOnce(&mut uinput::Device)>(m: &mut Mapper, f: F) {
    if m.dev.is_none() && !m.failed {
        match uinput::Device::open("Rare controller pointer") {
            Ok(d) => {
                eprintln!("[apps] pointer device ready");
                m.dev = Some(d);
            }
            Err(e) => {
                // Logged once; the app still runs, the pad just doesn't drive it.
                eprintln!("[apps] no pointer device ({e}); is /dev/uinput writable?");
                m.failed = true;
            }
        }
    }
    if let Some(d) = m.dev.as_mut() {
        f(d);
    }
}

/// True while the pad should drive an app as a mouse and keyboard.
pub fn active() -> bool {
    super::front::pointer_active()
}

/// Opens the virtual device ahead of time, so the first press after an app opens isn't lost
/// while the compositor picks the new device up.
pub fn prepare() {
    let mut m = MAPPER.lock().unwrap_or_else(|e| e.into_inner());
    with_dev(&mut m, |_| {});
}

/// A press or release while pointer mode is on. Returns false for buttons it doesn't take.
pub fn button(b: Pad, down: bool, back: Back) -> bool {
    match action(b, back) {
        Action::Pass => false,
        Action::Keyboard => {
            if down {
                // Steam's keyboard types into whatever window is in front (Game Mode).
                let _ = crate::launch::steam_url("steam://open/keyboard");
            }
            true
        }
        Action::Keys(keys) => {
            let mut m = MAPPER.lock().unwrap_or_else(|e| e.into_inner());
            with_dev(&mut m, |d| {
                if down {
                    for k in &keys {
                        d.key(*k, true);
                    }
                } else {
                    for k in keys.iter().rev() {
                        d.key(*k, false);
                    }
                }
                d.syn();
            });
            true
        }
    }
}

/// Presses `keys` together and lets go (in reverse), on the virtual keyboard, whether or not
/// pointer mode is on. Used for Steam's in-game menu (Shift+Tab, overlay::steam_menu).
pub fn combo(keys: &[u16]) {
    let mut m = MAPPER.lock().unwrap_or_else(|e| e.into_inner());
    with_dev(&mut m, |d| {
        for k in keys {
            d.key(*k, true);
        }
        d.syn();
        for k in keys.iter().rev() {
            d.key(*k, false);
        }
        d.syn();
    });
}

/// Called by the input thread every pass (about 60 times a second) while pointer mode is on.
pub fn analog(axes: &[f32; 4]) {
    let mut m = MAPPER.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let dt = m.last.map(|l| now.duration_since(l).as_secs_f32()).unwrap_or(0.0).min(0.05);
    m.last = Some(now);
    let mut carry = std::mem::take(&mut m.carry);
    let evs = motion(axes, dt, &mut carry);
    m.carry = carry;
    if evs.is_empty() {
        return;
    }
    with_dev(&mut m, |d| {
        for (c, v) in &evs {
            d.rel(*c, *v);
        }
        d.syn();
    });
}

/// Pointer mode ended: forget timing, so the next start doesn't jump.
pub fn stop() {
    let mut m = MAPPER.lock().unwrap_or_else(|e| e.into_inner());
    m.last = None;
    m.carry = Carry::default();
}

#[cfg(target_os = "linux")]
pub mod uinput {
    //! The kernel's uinput interface (linux/uinput.h), with the legacy-free UI_DEV_SETUP.
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    const EV_SYN: u16 = 0;
    const EV_KEY: u16 = 1;
    const EV_REL: u16 = 2;
    // _IOW('U', 100, int), _IOW('U', 101, int), _IOW('U', 102, int), _IOW('U', 3, uinput_setup), _IO('U', 1), _IO('U', 2)
    const UI_SET_EVBIT: u64 = 0x4004_5564;
    const UI_SET_KEYBIT: u64 = 0x4004_5565;
    const UI_SET_RELBIT: u64 = 0x4004_5566;
    const UI_DEV_SETUP: u64 = 0x405c_5503;
    const UI_DEV_CREATE: u64 = 0x5501;
    const UI_DEV_DESTROY: u64 = 0x5502;
    const BUS_VIRTUAL: u16 = 0x06;

    #[repr(C)]
    struct InputId {
        bustype: u16,
        vendor: u16,
        product: u16,
        version: u16,
    }

    #[repr(C)]
    struct UinputSetup {
        id: InputId,
        name: [u8; 80],
        ff_effects_max: u32,
    }

    #[repr(C)]
    struct InputEvent {
        time: libc::timeval,
        kind: u16,
        code: u16,
        value: i32,
    }

    pub struct Device {
        f: File,
    }

    fn ioctl(fd: i32, req: u64, arg: libc::c_ulong) -> std::io::Result<()> {
        // SAFETY: plain uinput ioctls on our own open fd; pointer arguments point to live memory.
        let r = unsafe { libc::ioctl(fd, req as _, arg) };
        if r < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    impl Device {
        pub fn open(name: &str) -> std::io::Result<Self> {
            Self::open_with(name, super::code::KEYS, super::code::RELS)
        }

        /// A keyboard-only device that can send exactly `keys` (the emulator rows' hotkeys).
        pub fn open_keys(name: &str, keys: &[u16]) -> std::io::Result<Self> {
            Self::open_with(name, keys, &[])
        }

        fn open_with(name: &str, keys: &[u16], rels: &[u16]) -> std::io::Result<Self> {
            let f = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open("/dev/uinput")?;
            let fd = f.as_raw_fd();
            for ev in [EV_KEY, EV_REL] {
                ioctl(fd, UI_SET_EVBIT, ev as libc::c_ulong)?;
            }
            for k in keys {
                ioctl(fd, UI_SET_KEYBIT, *k as libc::c_ulong)?;
            }
            for r in rels {
                ioctl(fd, UI_SET_RELBIT, *r as libc::c_ulong)?;
            }
            let mut setup = UinputSetup { id: InputId { bustype: BUS_VIRTUAL, vendor: 0x5241, product: 0x0001, version: 1 }, name: [0; 80], ff_effects_max: 0 };
            let n = name.as_bytes();
            setup.name[..n.len().min(79)].copy_from_slice(&n[..n.len().min(79)]);
            ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup as libc::c_ulong)?;
            ioctl(fd, UI_DEV_CREATE, 0)?;
            Ok(Device { f })
        }

        fn emit(&mut self, kind: u16, code: u16, value: i32) {
            let ev = InputEvent { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, kind, code, value };
            // SAFETY: InputEvent is repr(C) plain data; the slice covers exactly that struct.
            let bytes = unsafe { std::slice::from_raw_parts(&ev as *const InputEvent as *const u8, std::mem::size_of::<InputEvent>()) };
            let _ = self.f.write_all(bytes);
        }

        pub fn key(&mut self, code: u16, down: bool) {
            self.emit(EV_KEY, code, down as i32);
        }

        pub fn rel(&mut self, code: u16, value: i32) {
            self.emit(EV_REL, code, value);
        }

        pub fn syn(&mut self) {
            self.emit(EV_SYN, 0, 0);
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            let _ = ioctl(self.f.as_raw_fd(), UI_DEV_DESTROY, 0);
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub mod uinput {
    //! No virtual input devices outside Linux; pointer mode does nothing there.
    pub struct Device;
    impl Device {
        pub fn open(_name: &str) -> std::io::Result<Self> {
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "pointer mode needs Linux"))
        }
        pub fn open_keys(_name: &str, _keys: &[u16]) -> std::io::Result<Self> {
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "virtual keys need Linux"))
        }
        pub fn key(&mut self, _code: u16, _down: bool) {}
        pub fn rel(&mut self, _code: u16, _value: i32) {}
        pub fn syn(&mut self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_layout() {
        assert_eq!(action(Pad::A, Back::Escape), Action::Keys(vec![BTN_LEFT]));
        assert_eq!(action(Pad::Lt, Back::Escape), Action::Keys(vec![BTN_RIGHT]));
        assert_eq!(action(Pad::B, Back::AltLeft), Action::Keys(vec![KEY_LEFTALT, KEY_LEFT]));
        assert_eq!(action(Pad::B, Back::Escape), Action::Keys(vec![KEY_ESC]));
        assert_eq!(action(Pad::Lb, Back::Escape), Action::Keys(vec![KEY_LEFTSHIFT, KEY_TAB]));
        assert_eq!(action(Pad::Y, Back::Escape), Action::Keyboard);
        // Guide always reaches the launcher.
        for b in [Pad::Guide, Pad::GuideHold, Pad::Qam] {
            assert_eq!(action(b, Back::Escape), Action::Pass);
        }
        // Every key a layout can send is enabled on the device.
        for b in [Pad::A, Pad::B, Pad::X, Pad::Lb, Pad::Rb, Pad::Lt, Pad::Rt, Pad::Up, Pad::Down, Pad::Left, Pad::Right, Pad::Start, Pad::Select] {
            for back in [Back::Escape, Back::AltLeft] {
                if let Action::Keys(ks) = action(b, back) {
                    assert!(ks.iter().all(|k| KEYS.contains(k)), "{b:?}");
                }
            }
        }
    }

    #[test]
    fn stick_motion() {
        let mut c = Carry::default();
        // Resting sticks, and drift inside the deadzone, move nothing.
        assert!(motion(&[0.1, -0.1, 0.12, 0.05], 0.016, &mut c).is_empty());
        // Right stick fully right for one 16 ms frame: about 22 px.
        let ev = motion(&[0.0, 0.0, 1.0, 0.0], 0.016, &mut c);
        assert_eq!(ev, vec![(REL_X, 22)]);
        // A slow push still moves once the fractions add up.
        let mut c = Carry::default();
        let mut moved = 0;
        for _ in 0..60 {
            moved += motion(&[0.0, 0.0, 0.0, 0.25], 0.016, &mut c).iter().filter(|(k, _)| *k == REL_Y).map(|(_, v)| v).sum::<i32>();
        }
        assert!(moved > 0 && moved < 40, "{moved}");
        // Left stick down scrolls down: negative wheel, whole notches as they add up.
        let mut c = Carry::default();
        let mut hi = 0;
        let mut notches = 0;
        for _ in 0..30 {
            for (k, v) in motion(&[0.0, 1.0, 0.0, 0.0], 0.016, &mut c) {
                if k == REL_WHEEL_HI_RES {
                    hi += v;
                }
                if k == REL_WHEEL {
                    notches += v;
                }
            }
        }
        assert!(hi < -800, "{hi}");
        assert_eq!(notches, hi / 120);
    }
}
