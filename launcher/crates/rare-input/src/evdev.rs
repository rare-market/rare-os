//! Pads read straight from evdev, for the native launcher: one blocking reader thread per pad, so
//! a press reaches the pad state machine the moment the kernel delivers it (the Tauri shell polls
//! gilrs every 16 ms instead). The Deck pad (hidraw, deck.rs) feeds the same channel.
//!
//! Each event carries its kernel timestamp on CLOCK_MONOTONIC, the clock the journeys harness and
//! Rare Render's stats use, so input-to-photon can be measured from the kernel event on.

use crate::pad::{Axis, Btn, Ev, PadNow, Probe, Src, DECK};
use evdev::{Device, EventType, KeyCode};
use std::collections::{HashMap, HashSet};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

/// One pad event as read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Raw {
    pub src: Src,
    pub ev: Ev,
    /// The pad reports X and Y swapped (`pad::xy_swapped`).
    pub swaps: bool,
    /// CLOCK_MONOTONIC ns of the kernel event.
    pub t_ns: u64,
}

fn clock(id: libc::clockid_t) -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes into the timespec we pass.
    unsafe { libc::clock_gettime(id, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

pub fn mono_ns() -> u64 {
    clock(libc::CLOCK_MONOTONIC)
}

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

/// The standard button for an evdev key code.
pub fn button(code: u16) -> Option<Btn> {
    Some(match code {
        0x130 => Btn::South,         // BTN_SOUTH (A)
        0x131 => Btn::East,          // BTN_EAST (B)
        0x133 => Btn::North,         // BTN_NORTH = BTN_X: X on xpad, put back by the swap
        0x134 => Btn::West,          // BTN_WEST = BTN_Y
        0x136 => Btn::LeftTrigger,   // BTN_TL
        0x137 => Btn::RightTrigger,  // BTN_TR
        0x138 => Btn::LeftTrigger2,  // BTN_TL2
        0x139 => Btn::RightTrigger2, // BTN_TR2
        0x13a => Btn::Select,
        0x13b => Btn::Start,
        0x13c => Btn::Mode,
        0x13d => Btn::LeftThumb,
        0x13e => Btn::RightThumb,
        0x220 => Btn::DPadUp,
        0x221 => Btn::DPadDown,
        0x222 => Btn::DPadLeft,
        0x223 => Btn::DPadRight,
        _ => return None,
    })
}

/// The hat's d-pad edges: from `old` to `new` (-1, 0, 1) on an axis.
pub fn hat(old: i32, new: i32, neg: Btn, pos: Btn, out: &mut Vec<Ev>) {
    if old == new {
        return;
    }
    match old {
        -1 => out.push(Ev::Released(neg)),
        1 => out.push(Ev::Released(pos)),
        _ => {}
    }
    match new.signum() {
        -1 => out.push(Ev::Pressed(neg)),
        1 => out.push(Ev::Pressed(pos)),
        _ => {}
    }
}

/// An axis value into -1..1 (sticks) or 0..1 (triggers), from the device's range.
pub fn norm(v: i32, lo: i32, hi: i32, trigger: bool) -> f32 {
    if hi <= lo {
        return 0.0;
    }
    let f = (v - lo) as f32 / (hi - lo) as f32;
    if trigger {
        f.clamp(0.0, 1.0)
    } else {
        (f * 2.0 - 1.0).clamp(-1.0, 1.0)
    }
}

/// A pad being read: a second handle for state reads, and its axis ranges.
struct Open {
    file: std::fs::File,
    ranges: [(i32, i32); 6],
}

static OPEN: Mutex<Option<HashMap<Src, Open>>> = Mutex::new(None);

fn abs_ranges(dev: &Device) -> [(i32, i32); 6] {
    let absinfo = dev.get_abs_state().ok();
    let range = |code: u16| -> (i32, i32) { absinfo.as_ref().map(|a| (a[code as usize].minimum, a[code as usize].maximum)).filter(|(lo, hi)| hi > lo).unwrap_or((-32768, 32767)) };
    [range(ABS_X), range(ABS_Y), range(ABS_Z), range(ABS_RX), range(ABS_RY), range(ABS_RZ)]
}

// EVIOCGABS(abs) = _IOR('E', 0x40 + abs, struct input_absinfo) and EVIOCGKEY(len) =
// _IOC(_IOC_READ, 'E', 0x18, len), as in linux/input.h.
const IOC_READ: libc::c_ulong = 2;
fn ioc(nr: libc::c_ulong, size: libc::c_ulong) -> libc::c_ulong {
    (IOC_READ << 30) | (size << 16) | ((b'E' as libc::c_ulong) << 8) | nr
}

fn abs_value(fd: i32, code: u16) -> Option<i32> {
    let mut info = [0i32; 6];
    // SAFETY: EVIOCGABS writes one struct input_absinfo (six i32) into the buffer we pass.
    let r = unsafe { libc::ioctl(fd, ioc(0x40 + code as libc::c_ulong, 24) as _, info.as_mut_ptr()) };
    (r >= 0).then_some(info[0])
}

fn key_down(keys: &[u8; 96], code: u16) -> bool {
    keys.get(code as usize / 8).is_some_and(|b| b & (1 << (code % 8)) != 0)
}

/// The kernel's state of the pad `src` now (EVIOCGABS, EVIOCGKEY): what a held direction checks
/// before it repeats from an old event. The Deck pad (hidraw) can't be read this way.
pub fn probe(src: Src) -> Probe {
    if src == DECK {
        return Probe::Unknown;
    }
    let g = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(o) = g.as_ref().and_then(|m| m.get(&src)) else { return Probe::Gone };
    let fd = o.file.as_raw_fd();
    let mut keys = [0u8; 96];
    // SAFETY: EVIOCGKEY(96) writes at most 96 bytes of key bits into the buffer we pass.
    let r = unsafe { libc::ioctl(fd, ioc(0x18, keys.len() as libc::c_ulong) as _, keys.as_mut_ptr()) };
    let Some(x) = abs_value(fd, ABS_X) else { return if r < 0 { Probe::Gone } else { Probe::Unknown } };
    let axis = |code: u16, trigger: bool| abs_value(fd, code).map(|v| norm(v, o.ranges[code as usize].0, o.ranges[code as usize].1, trigger)).unwrap_or(0.0);
    let (hx, hy) = (abs_value(fd, ABS_HAT0X).unwrap_or(0), abs_value(fd, ABS_HAT0Y).unwrap_or(0));
    let k = |code: u16| r >= 0 && key_down(&keys, code);
    Probe::Now(PadNow {
        lx: norm(x, o.ranges[0].0, o.ranges[0].1, false),
        ly: -axis(ABS_Y, false),
        rx: axis(ABS_RX, false),
        ry: -axis(ABS_RY, false),
        lt: axis(ABS_Z, true),
        rt: axis(ABS_RZ, true),
        dpad: [hy < 0 || k(0x220), hy > 0 || k(0x221), hx < 0 || k(0x222), hx > 0 || k(0x223)],
    })
}

/// Every pad being read and its state now, for a reset that re-reads them all.
pub fn probe_all() -> Vec<(Src, Probe)> {
    let srcs: Vec<Src> = OPEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|m| m.keys().copied().collect()).unwrap_or_default();
    srcs.into_iter().map(|s| (s, probe(s))).collect()
}

/// Starts the readers (rescanned every 2 s for new pads) and the Deck pad reader. Every event
/// goes to `tx`.
pub fn spawn(tx: Sender<Raw>) {
    // The Deck pad over hidraw: it has no evdev device.
    let (dtx, drx) = std::sync::mpsc::channel::<Ev>();
    crate::deck::start(dtx);
    let deck_tx = tx.clone();
    std::thread::Builder::new()
        .name("rare-pad-deck".into())
        .spawn(move || {
            while let Ok(ev) = drx.recv() {
                if deck_tx.send(Raw { src: DECK, ev, swaps: false, t_ns: mono_ns() }).is_err() {
                    return;
                }
            }
        })
        .ok();
    let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
    std::thread::Builder::new()
        .name("rare-pad-scan".into())
        .spawn(move || {
            let mut next: Src = 1;
            loop {
                for (path, dev) in evdev::enumerate() {
                    if open.lock().unwrap().contains(&path) {
                        continue;
                    }
                    if !dev.supported_keys().is_some_and(|k| k.contains(KeyCode::BTN_SOUTH)) {
                        continue;
                    }
                    let name = dev.name().unwrap_or("?").to_string();
                    let swaps = crate::pad::xy_swapped(&name, Some(dev.input_id().vendor()));
                    crate::log(&format!("[pad] {} ({name}){}", path.display(), if swaps { ", X and Y swapped" } else { "" }));
                    open.lock().unwrap().insert(path.clone());
                    let (tx, open, src) = (tx.clone(), open.clone(), next);
                    next += 1;
                    if let Ok(file) = std::fs::File::open(&path) {
                        OPEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(src, Open { file, ranges: abs_ranges(&dev) });
                    }
                    std::thread::Builder::new()
                        .name("rare-pad-read".into())
                        .spawn(move || {
                            read(dev, src, swaps, &tx);
                            if let Some(m) = OPEN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                                m.remove(&src);
                            }
                            let _ = tx.send(Raw { src, ev: Ev::Disconnected, swaps, t_ns: mono_ns() });
                            open.lock().unwrap().remove(&path);
                        })
                        .ok();
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .ok();
}

fn read(mut dev: Device, src: Src, swaps: bool, tx: &Sender<Raw>) {
    let ranges = abs_ranges(&dev);
    // Kernel timestamps are CLOCK_REALTIME: map them onto CLOCK_MONOTONIC.
    let offset = clock(libc::CLOCK_REALTIME) as i128 - mono_ns() as i128;
    let _ = tx.send(Raw { src, ev: Ev::Connected, swaps, t_ns: mono_ns() });
    let mut hat_state = [0i32; 2];
    let mut evs = Vec::new();
    loop {
        let Ok(events) = dev.fetch_events() else { return };
        for e in events {
            let rt = e.timestamp().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as i128).unwrap_or(0);
            let t_ns = (rt - offset).max(0) as u64;
            let (code, v) = (e.code(), e.value());
            match e.event_type() {
                EventType::KEY => {
                    if let Some(b) = button(code) {
                        match v {
                            1 => evs.push(Ev::Pressed(b)),
                            0 => evs.push(Ev::Released(b)),
                            _ => {} // kernel autorepeat: the state machine times repeats
                        }
                    }
                }
                EventType::ABSOLUTE => match code {
                    ABS_HAT0X => {
                        hat(hat_state[0], v, Btn::DPadLeft, Btn::DPadRight, &mut evs);
                        hat_state[0] = v.signum();
                    }
                    ABS_HAT0Y => {
                        hat(hat_state[1], v, Btn::DPadUp, Btn::DPadDown, &mut evs);
                        hat_state[1] = v.signum();
                    }
                    ABS_X | ABS_Y | ABS_Z | ABS_RX | ABS_RY | ABS_RZ => {
                        let (lo, hi) = ranges[code as usize];
                        let ev = match code {
                            ABS_X => Ev::Axis(Axis::LeftStickX, norm(v, lo, hi, false)),
                            // evdev's down is positive; the state machine's up is.
                            ABS_Y => Ev::Axis(Axis::LeftStickY, -norm(v, lo, hi, false)),
                            ABS_RX => Ev::Axis(Axis::RightStickX, norm(v, lo, hi, false)),
                            ABS_RY => Ev::Axis(Axis::RightStickY, -norm(v, lo, hi, false)),
                            ABS_Z => Ev::Axis(Axis::LeftZ, norm(v, lo, hi, true)),
                            _ => Ev::Axis(Axis::RightZ, norm(v, lo, hi, true)),
                        };
                        evs.push(ev);
                    }
                    _ => {}
                },
                _ => {}
            }
            for ev in evs.drain(..) {
                if tx.send(Raw { src, ev, swaps, t_ns }).is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_edges() {
        let mut out = Vec::new();
        hat(0, -1, Btn::DPadUp, Btn::DPadDown, &mut out);
        hat(-1, 1, Btn::DPadUp, Btn::DPadDown, &mut out);
        hat(1, 0, Btn::DPadUp, Btn::DPadDown, &mut out);
        hat(0, 0, Btn::DPadUp, Btn::DPadDown, &mut out);
        assert_eq!(out, vec![Ev::Pressed(Btn::DPadUp), Ev::Released(Btn::DPadUp), Ev::Pressed(Btn::DPadDown), Ev::Released(Btn::DPadDown)]);
    }

    #[test]
    fn axes_normalise() {
        assert_eq!(norm(-32768, -32768, 32767, false), -1.0);
        assert_eq!(norm(32767, -32768, 32767, false), 1.0);
        assert!(norm(0, -32768, 32767, false).abs() < 1e-4);
        assert_eq!(norm(1023, 0, 1023, true), 1.0);
        assert_eq!(norm(0, 0, 1023, true), 0.0);
        assert_eq!(norm(5, 5, 5, true), 0.0);
    }

    #[test]
    fn codes() {
        assert_eq!(button(0x130), Some(Btn::South));
        assert_eq!(button(0x133), Some(Btn::North));
        assert_eq!(button(0x13c), Some(Btn::Mode));
        assert_eq!(button(0x221), Some(Btn::DPadDown));
        assert_eq!(button(0x100), None);
    }
}
