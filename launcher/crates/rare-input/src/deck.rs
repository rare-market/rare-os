//! The controller while InputPlumber's target is `deck-uhid` (gyro.rs: a Steam game runs).
//!
//! InputPlumber 0.83 creates that pad over uhid with Valve's vendor id but product 0x12F0
//! (`ProductId::Generic`), which hid-steam does not bind (it lists 0x1205, 0x1142, 0x1102). So
//! hid-generic takes it and the pad exists only as a hidraw node: no evdev device, nothing gilrs
//! can see. Steam reads the hidraw node; seen on the SuiPlay (kernel 7.2): `hid-generic
//! 0003:28DE:12F0.0001: hidraw0: ... [Generic Steam Controller]` and no "Steam Deck" input device.
//! Without Steam nobody reads it at all, which is how the launcher lost Guide.
//!
//! hidraw hands every report to every reader, so the launcher reads the same node next to Steam
//! (udev's 60-steam-input.rules give the session user access to Valve hidraw nodes) and feeds
//! the presses into the pad state machine (pad.rs) as one more pad. The reports are the Steam
//! Deck's (type 0x09, 64 bytes), laid out as hid-steam's `steam_do_deck_input_event` and
//! InputPlumber's `PackedInputDataReport` (msb0 bit numbering) agree.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::pad::{Axis, Btn as Button, Ev};

/// Valve's USB vendor id, as uevent's HID_ID writes it.
const VALVE: &str = "000028DE";

/// A hidraw device is InputPlumber's virtual Deck pad, Valve's vendor id: its `deck-uhid` target
/// (uhid, product 0x12F0) or its `deck` target (a USB/IP device on vhci_hcd with the real Deck's
/// 0x1205; hid-steam binds that one, but drops its evdev pad while Steam holds the hidraw node,
/// which is exactly while a Steam game runs). A real Deck or Steam Controller on a USB port is
/// left to hid-steam's evdev pad. `device_path` is the resolved
/// `/sys/class/hidraw/hidrawN/device` link, `uevent` its uevent.
pub fn is_deck_uhid(device_path: &str, uevent: &str) -> bool {
    if !device_path.contains("/uhid/") && !device_path.contains("vhci_hcd") {
        return false;
    }
    uevent.lines().any(|l| l.strip_prefix("HID_ID=").and_then(|v| v.split(':').nth(1)).is_some_and(|v| v.eq_ignore_ascii_case(VALVE)))
}

/// The `/dev/hidrawN` node of the virtual Deck pad, when there is one.
#[cfg(target_os = "linux")]
pub fn find_node() -> Option<String> {
    let rd = std::fs::read_dir("/sys/class/hidraw").ok()?;
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names.into_iter().find_map(|n| {
        let dev = std::path::Path::new("/sys/class/hidraw").join(&n).join("device");
        let path = std::fs::canonicalize(&dev).ok()?.to_string_lossy().into_owned();
        let uevent = std::fs::read_to_string(dev.join("uevent")).ok()?;
        is_deck_uhid(&path, &uevent).then(|| format!("/dev/{n}"))
    })
}

/// The digital buttons of a Deck report: (byte, mask, button).
const BUTTONS: [(usize, u8, Button); 15] = [
    (8, 0x80, Button::South),
    (8, 0x40, Button::West),
    (8, 0x20, Button::East),
    (8, 0x10, Button::North),
    (8, 0x08, Button::LeftTrigger),
    (8, 0x04, Button::RightTrigger),
    (9, 0x01, Button::DPadUp),
    (9, 0x02, Button::DPadRight),
    (9, 0x04, Button::DPadLeft),
    (9, 0x08, Button::DPadDown),
    (9, 0x10, Button::Select),
    (9, 0x20, Button::Mode),
    (9, 0x40, Button::Start),
    (10, 0x40, Button::LeftThumb),
    (11, 0x04, Button::RightThumb),
];
/// Quick access (the "..." button), byte 14 bit 2.
const QAM: (usize, u8) = (14, 0x04);

/// The parts of a Deck report the launcher uses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    /// Bit i: BUTTONS[i] held.
    pub buttons: u16,
    pub qam: bool,
    /// Left stick, right stick (up positive), -1..=1.
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    /// Triggers, 0..=1.
    pub lt: f32,
    pub rt: f32,
}

fn i16_at(d: &[u8], i: usize) -> f32 {
    i16::from_le_bytes([d[i], d[i + 1]]) as f32 / 32767.0
}

fn u16_at(d: &[u8], i: usize) -> f32 {
    (u16::from_le_bytes([d[i], d[i + 1]]) as f32 / 32767.0).min(1.0)
}

/// A Deck input report (`01 00 09 ..`), or None for anything else.
pub fn parse(d: &[u8]) -> Option<Report> {
    if d.len() < 56 || d[0] != 0x01 || d[1] != 0x00 || d[2] != 0x09 {
        return None;
    }
    let mut buttons = 0u16;
    for (i, (byte, mask, _)) in BUTTONS.iter().enumerate() {
        if d[*byte] & mask != 0 {
            buttons |= 1 << i;
        }
    }
    Some(Report {
        buttons,
        qam: d[QAM.0] & QAM.1 != 0,
        lt: u16_at(d, 44),
        rt: u16_at(d, 46),
        lx: i16_at(d, 48),
        ly: i16_at(d, 50),
        rx: i16_at(d, 52),
        ry: i16_at(d, 54),
    })
}

/// Axis moves smaller than this are not sent (the report comes at the IMU's rate).
const AXIS_STEP: f32 = 0.03;

/// The events that take `prev` to `next`. Axes go out when they moved at least AXIS_STEP from
/// what was last sent (`sent`, updated) or came back to rest.
pub fn diff(prev: &Report, next: &Report, sent: &mut Report, out: &mut Vec<Ev>) {
    for (i, (_, _, b)) in BUTTONS.iter().enumerate() {
        let (was, now) = (prev.buttons & (1 << i) != 0, next.buttons & (1 << i) != 0);
        if now && !was {
            out.push(Ev::Pressed(*b));
        } else if was && !now {
            out.push(Ev::Released(*b));
        }
    }
    if next.qam != prev.qam {
        out.push(Ev::Qam(next.qam));
    }
    let mut axis = |v: f32, last: &mut f32, ev: Ev| {
        if (v - *last).abs() >= AXIS_STEP || (v == 0.0 && *last != 0.0) {
            *last = v;
            out.push(match ev {
                Ev::Axis(a, _) => Ev::Axis(a, v),
                Ev::Changed(b, _) => Ev::Changed(b, v),
                e => e,
            });
        }
    };
    axis(next.lx, &mut sent.lx, Ev::Axis(Axis::LeftStickX, 0.0));
    axis(next.ly, &mut sent.ly, Ev::Axis(Axis::LeftStickY, 0.0));
    axis(next.rx, &mut sent.rx, Ev::Axis(Axis::RightStickX, 0.0));
    axis(next.ry, &mut sent.ry, Ev::Axis(Axis::RightStickY, 0.0));
    axis(next.lt, &mut sent.lt, Ev::Changed(Button::LeftTrigger2, 0.0));
    axis(next.rt, &mut sent.rt, Ev::Changed(Button::RightTrigger2, 0.0));
}

static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True while the launcher reads the virtual Deck pad.
pub fn active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Reads the virtual Deck pad whenever there is one and sends its events to `tx`. Looks for the
/// node twice a second (gyro.rs gives a switch 2 s to show a working pad); a node that goes away
/// (target back to the Xbox pad) ends the read with every held button released.
#[cfg(target_os = "linux")]
pub fn start(tx: std::sync::mpsc::Sender<Ev>) {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    std::thread::spawn(move || {
        let mut said_denied = false;
        loop {
            let Some(node) = find_node() else {
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            };
            let mut f = match std::fs::File::open(&node) {
                Ok(f) => f,
                Err(e) => {
                    if !said_denied {
                        said_denied = true;
                        crate::log(&format!("[deckpad] can't read {node}: {e}"));
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }
            };
            said_denied = false;
            crate::log(&format!("[deckpad] reading the Deck pad at {node}"));
            ACTIVE.store(true, Ordering::Relaxed);
            let _ = tx.send(Ev::Connected);
            let (mut prev, mut sent) = (Report::default(), Report::default());
            let mut buf = [0u8; 64];
            let mut out = Vec::new();
            loop {
                match f.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if let Some(r) = parse(&buf[..n]) {
                            diff(&prev, &r, &mut sent, &mut out);
                            prev = r;
                            for e in out.drain(..) {
                                let _ = tx.send(e);
                            }
                        }
                    }
                    _ => break,
                }
            }
            // Gone: nothing stays held.
            diff(&prev, &Report::default(), &mut sent, &mut out);
            for e in out.drain(..) {
                let _ = tx.send(e);
            }
            let _ = tx.send(Ev::Disconnected);
            ACTIVE.store(false, Ordering::Relaxed);
            crate::log(&format!("[deckpad] {node} went away"));
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn start(_tx: std::sync::mpsc::Sender<Ev>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> [u8; 64] {
        let mut d = [0u8; 64];
        d[0] = 0x01;
        d[2] = 0x09;
        d[3] = 64;
        d
    }

    #[test]
    fn finds_inputplumbers_uhid_pad_only() {
        let ue = "DRIVER=hid-generic\nHID_ID=0003:000028DE:000012F0\nHID_NAME=Steam Controller\n";
        assert!(is_deck_uhid("/sys/devices/virtual/misc/uhid/0003:28DE:12F0.0001", ue));
        // InputPlumber's `deck` target: USB/IP on vhci_hcd with the real Deck's id.
        assert!(is_deck_uhid("/sys/devices/platform/vhci_hcd.0/usb3/3-1/3-1:1.2/0003:28DE:1205.0004", "HID_ID=0003:000028DE:00001205\n"));
        // A real Deck or Steam Controller on USB: hid-steam's evdev covers it already.
        assert!(!is_deck_uhid("/sys/devices/pci0000:00/0000:00:08.1/usb1/1-3/1-3:1.2/0003:28DE:1205.0002", ue));
        // Another vendor over uhid (a DualSense target).
        assert!(!is_deck_uhid("/sys/devices/virtual/misc/uhid/0003:054C:0CE6.0003", "HID_ID=0003:0000054C:00000CE6\n"));
    }

    #[test]
    fn reads_buttons_as_hid_steam_maps_them() {
        let mut d = report();
        d[8] = 0x80; // A
        d[9] = 0x20 | 0x01; // Steam (Guide), d-pad up
        d[14] = 0x04; // quick access
        let r = parse(&d).unwrap();
        assert!(r.qam);
        let mut out = Vec::new();
        diff(&Report::default(), &r, &mut Report::default(), &mut out);
        assert_eq!(out, vec![Ev::Pressed(Button::South), Ev::Pressed(Button::DPadUp), Ev::Pressed(Button::Mode), Ev::Qam(true)]);
        // Every mapped bit lands on the button hid-steam reports for it.
        let mut d = report();
        d[8] = 0x40 | 0x20 | 0x10 | 0x08 | 0x04;
        d[9] = 0x02 | 0x04 | 0x08 | 0x10 | 0x40;
        d[10] = 0x40;
        d[11] = 0x04;
        let mut out = Vec::new();
        diff(&Report::default(), &parse(&d).unwrap(), &mut Report::default(), &mut out);
        let pressed: Vec<Button> = out.iter().filter_map(|e| if let Ev::Pressed(b) = e { Some(*b) } else { None }).collect();
        assert_eq!(
            pressed,
            vec![
                Button::West,
                Button::East,
                Button::North,
                Button::LeftTrigger,
                Button::RightTrigger,
                Button::DPadRight,
                Button::DPadLeft,
                Button::DPadDown,
                Button::Select,
                Button::Start,
                Button::LeftThumb,
                Button::RightThumb
            ]
        );
    }

    #[test]
    fn releases_and_sticks() {
        let mut d = report();
        d[9] = 0x20;
        d[48..50].copy_from_slice(&32767i16.to_le_bytes());
        d[50..52].copy_from_slice(&(-16384i16).to_le_bytes());
        d[44..46].copy_from_slice(&32767u16.to_le_bytes());
        let a = parse(&d).unwrap();
        assert!((a.lx - 1.0).abs() < 1e-4 && (a.ly + 0.5).abs() < 1e-3 && (a.lt - 1.0).abs() < 1e-4);
        let mut sent = Report::default();
        let mut out = Vec::new();
        diff(&Report::default(), &a, &mut sent, &mut out);
        assert!(out.contains(&Ev::Pressed(Button::Mode)));
        assert!(out.contains(&Ev::Axis(Axis::LeftStickX, a.lx)));
        assert!(out.contains(&Ev::Changed(Button::LeftTrigger2, a.lt)));
        // A tiny wobble sends nothing; letting go releases and centres.
        let mut b = a;
        b.lx -= 0.01;
        out.clear();
        diff(&a, &b, &mut sent, &mut out);
        assert!(out.is_empty());
        out.clear();
        diff(&b, &Report::default(), &mut sent, &mut out);
        assert!(out.contains(&Ev::Released(Button::Mode)));
        assert!(out.contains(&Ev::Axis(Axis::LeftStickX, 0.0)));
        assert!(out.contains(&Ev::Changed(Button::LeftTrigger2, 0.0)));
    }

    #[test]
    fn ignores_other_reports() {
        let mut d = report();
        d[2] = 0x01; // Steam Controller state
        assert_eq!(parse(&d), None);
        assert_eq!(parse(&[0x01, 0x00, 0x09]), None);
    }
}
