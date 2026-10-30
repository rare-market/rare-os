//! The virtual pad for network apps (Settings > Developer > Allow network apps > Send controller
//! input). One uinput device that looks like an Xbox 360 pad, so Rare's own pad readers pick it up
//! (a remote can drive the launcher exactly like a held controller) and games see one more pad.
//!
//! It exists only while the owner has the switch on. A remote that goes quiet or disconnects can't
//! leave a button held: after `IDLE_RELEASE` without input everything is released, and turning the
//! switch off removes the device.

#[cfg(target_os = "linux")]
mod imp {
    use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
    use evdev::{AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, UinputAbsSetup};
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// Held buttons and axes are let go after this long without input.
    pub const IDLE_RELEASE: Duration = Duration::from_secs(5);

    pub const NAME: &str = "Xbox 360 Controller (Rare remote)";

    struct Pad {
        dev: VirtualDevice,
        held: HashSet<&'static str>,
        axes: [i32; 6],
        last: Instant,
    }

    static PAD: Mutex<Option<Pad>> = Mutex::new(None);

    /// The evdev key for a button name (xpad's codes; X is BTN_X = 0x133, Y is BTN_Y = 0x134).
    pub fn key_code(name: &str) -> Option<u16> {
        Some(match name {
            "a" => 0x130,
            "b" => 0x131,
            "x" => 0x133,
            "y" => 0x134,
            "lb" => 0x136,
            "rb" => 0x137,
            "select" => 0x13a,
            "start" => 0x13b,
            "guide" => 0x13c,
            "l3" => 0x13d,
            "r3" => 0x13e,
            _ => return None,
        })
    }

    const ABS_X: u16 = 0x00;
    const ABS_Y: u16 = 0x01;
    const ABS_Z: u16 = 0x02;
    const ABS_RX: u16 = 0x03;
    const ABS_RY: u16 = 0x04;
    const ABS_RZ: u16 = 0x05;
    const ABS_HAT0X: u16 = 0x10;
    const ABS_HAT0Y: u16 = 0x11;
    const STICK: i32 = 32767;
    const TRIGGER: i32 = 255;

    /// (evdev axis, index into `axes`, scale) for a stick or trigger name. Y axes are up positive
    /// in the API and down positive in evdev.
    fn axis_of(name: &str) -> Option<(u16, usize, i32)> {
        Some(match name {
            "left_x" => (ABS_X, 0, STICK),
            "left_y" => (ABS_Y, 1, -STICK),
            "right_x" => (ABS_RX, 2, STICK),
            "right_y" => (ABS_RY, 3, -STICK),
            "lt" => (ABS_Z, 4, TRIGGER),
            "rt" => (ABS_RZ, 5, TRIGGER),
            _ => return None,
        })
    }

    /// The d-pad is the hat: (axis, value).
    fn hat_of(name: &str) -> Option<(u16, i32)> {
        Some(match name {
            "up" => (ABS_HAT0Y, -1),
            "down" => (ABS_HAT0Y, 1),
            "left" => (ABS_HAT0X, -1),
            "right" => (ABS_HAT0X, 1),
            _ => return None,
        })
    }

    fn ev(kind: EventType, code: u16, value: i32) -> InputEvent {
        InputEvent::new(kind.0, code, value)
    }

    fn build() -> std::io::Result<VirtualDevice> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for n in ["a", "b", "x", "y", "lb", "rb", "select", "start", "guide", "l3", "r3"] {
            keys.insert(KeyCode(key_code(n).unwrap()));
        }
        let stick = AbsInfo::new(0, -STICK - 1, STICK, 16, 128, 0);
        let trig = AbsInfo::new(0, 0, TRIGGER, 0, 0, 0);
        let hat = AbsInfo::new(0, -1, 1, 0, 0, 0);
        let mut b: VirtualDeviceBuilder = VirtualDevice::builder()?
            .name(NAME)
            .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0114))
            .with_keys(&keys)?;
        for (code, info) in [(ABS_X, stick), (ABS_Y, stick), (ABS_RX, stick), (ABS_RY, stick), (ABS_Z, trig), (ABS_RZ, trig), (ABS_HAT0X, hat), (ABS_HAT0Y, hat)] {
            b = b.with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(code), info))?;
        }
        b.build()
    }

    /// Makes the virtual pad (`on`) or removes it. Err: why it couldn't be made (no /dev/uinput).
    pub fn ready(on: bool) -> Result<(), String> {
        let mut g = PAD.lock().unwrap_or_else(|e| e.into_inner());
        if !on {
            *g = None;
            return Ok(());
        }
        if g.is_some() {
            return Ok(());
        }
        let dev = build().map_err(|e| format!("No virtual controller ({e}); is /dev/uinput writable?"))?;
        *g = Some(Pad { dev, held: HashSet::new(), axes: [0; 6], last: Instant::now() });
        drop(g);
        std::thread::Builder::new()
            .name("rare-remote-pad".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_millis(500));
                let mut g = PAD.lock().unwrap_or_else(|e| e.into_inner());
                let Some(p) = g.as_mut() else { return };
                if p.last.elapsed() >= IDLE_RELEASE && (!p.held.is_empty() || p.axes.iter().any(|a| *a != 0)) {
                    release_locked(p);
                }
            })
            .ok();
        Ok(())
    }

    pub fn available() -> bool {
        PAD.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    fn release_locked(p: &mut Pad) {
        let mut evs = Vec::new();
        for n in p.held.drain().collect::<Vec<_>>() {
            if let Some(c) = key_code(n) {
                evs.push(ev(EventType::KEY, c, 0));
            } else if let Some((a, _)) = hat_of(n) {
                evs.push(ev(EventType::ABSOLUTE, a, 0));
            }
        }
        for (i, name) in ["left_x", "left_y", "right_x", "right_y", "lt", "rt"].iter().enumerate() {
            if p.axes[i] != 0 {
                p.axes[i] = 0;
                evs.push(ev(EventType::ABSOLUTE, axis_of(name).unwrap().0, 0));
            }
        }
        if !evs.is_empty() {
            let _ = p.dev.emit(&evs);
        }
    }

    /// Lets go of everything.
    pub fn release_all() {
        if let Some(p) = PAD.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            release_locked(p);
        }
    }

    const BUTTONS: [&str; 17] = ["a", "b", "x", "y", "lb", "rb", "lt", "rt", "up", "down", "left", "right", "start", "select", "guide", "l3", "r3"];

    /// Presses or releases a button by name.
    pub fn button(name: &str, down: bool) -> Result<(), String> {
        let name: &'static str = BUTTONS.iter().copied().find(|b| *b == name).ok_or("Unknown button")?;
        let mut g = PAD.lock().unwrap_or_else(|e| e.into_inner());
        let p = g.as_mut().ok_or("The virtual controller isn't running")?;
        p.last = Instant::now();
        let e = if let Some(c) = key_code(name) {
            ev(EventType::KEY, c, down as i32)
        } else if let Some((a, v)) = hat_of(name) {
            ev(EventType::ABSOLUTE, a, if down { v } else { 0 })
        } else {
            let (a, i, scale) = axis_of(name).ok_or("Unknown button")?;
            p.axes[i] = if down { scale } else { 0 };
            ev(EventType::ABSOLUTE, a, p.axes[i])
        };
        if down {
            p.held.insert(name);
        } else {
            p.held.remove(name);
        }
        p.dev.emit(&[e]).map_err(|e| e.to_string())
    }

    /// Moves a stick (-1..1, up positive) or a trigger (0..1).
    pub fn axis(name: &str, value: f32) -> Result<(), String> {
        let (a, i, scale) = axis_of(name).ok_or("Unknown axis")?;
        let lo = if i >= 4 { 0.0 } else { -1.0 };
        let v = (value.clamp(lo, 1.0) * scale as f32).round() as i32;
        let mut g = PAD.lock().unwrap_or_else(|e| e.into_inner());
        let p = g.as_mut().ok_or("The virtual controller isn't running")?;
        p.last = Instant::now();
        p.axes[i] = v;
        p.dev.emit(&[ev(EventType::ABSOLUTE, a, v)]).map_err(|e| e.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_button_has_a_code() {
            for b in BUTTONS {
                assert!(key_code(b).is_some() || hat_of(b).is_some() || axis_of(b).is_some(), "{b}");
            }
            assert_eq!(key_code("a"), Some(0x130));
            assert_eq!(hat_of("up"), Some((ABS_HAT0Y, -1)));
            assert_eq!(axis_of("left_y"), Some((ABS_Y, 1, -STICK)));
            assert!(key_code("turbo").is_none());
        }

        #[test]
        fn names_match_what_the_pad_reader_expects() {
            // The pad reader swaps X and Y for a device called Xbox, so 0x133 is X.
            assert!(crate::pad::xy_swapped(NAME, Some(0x045e)));
            assert_eq!(crate::evdev::button(key_code("x").unwrap()), Some(crate::pad::Btn::North));
            assert_eq!(crate::pad::map_button_for(crate::pad::Btn::North, true), Some(rare_core::Pad::X));
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(not(target_os = "linux"))]
mod stub {
    pub fn ready(_on: bool) -> Result<(), String> {
        Err("Controller input needs Linux".into())
    }
    pub fn available() -> bool {
        false
    }
    pub fn release_all() {}
    pub fn button(_name: &str, _down: bool) -> Result<(), String> {
        Err("Controller input needs Linux".into())
    }
    pub fn axis(_name: &str, _value: f32) -> Result<(), String> {
        Err("Controller input needs Linux".into())
    }
}
#[cfg(not(target_os = "linux"))]
pub use stub::*;
