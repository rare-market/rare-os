//! Touch controls for touch-only Android games: each pad button can press a spot on the screen.
//! The spots are saved per game (~/.config/rare/android-touch.json, x and y from 0 to 1). While
//! such a game is in front the launcher takes the pad (InputPlumber's intercept mode) and calls
//! `press`, which holds a finger on a virtual multitouch screen ("Rare Android Touch") for as long
//! as the button is held, one finger per button, so a direction and a button work together.
//! Android reads that screen like the pads (Waydroid passes input devices through).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// The buttons a spot can be set for, with their names as the editor shows them.
pub const BUTTONS: [(&str, &str); 14] = [
    ("a", "A"),
    ("b", "B"),
    ("x", "X"),
    ("y", "Y"),
    ("up", "Up"),
    ("down", "Down"),
    ("left", "Left"),
    ("right", "Right"),
    ("lb", "LB"),
    ("rb", "RB"),
    ("lt", "LT"),
    ("rt", "RT"),
    ("start", "Menu"),
    ("select", "View"),
];

/// The virtual screen's name (the image's udev rule keeps the desktop's input stack off it).
pub const DEVICE_NAME: &str = "Rare Android Touch";

/// One game's spots: button -> (x, y), 0..1 across Android's screen.
pub type Map = BTreeMap<String, [f32; 2]>;

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    games: BTreeMap<String, Map>,
}

fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("rare/android-touch.json")
}

fn load_all() -> File {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_all(f: &File) -> Result<(), String> {
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(f).unwrap_or_default()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// A game's spots (empty when it has none).
pub fn map(pkg: &str) -> Map {
    load_all().games.remove(pkg).unwrap_or_default()
}

/// Saves a game's spots; an empty map removes them.
pub fn set_map(pkg: &str, m: Map) -> Result<(), String> {
    let mut f = load_all();
    let m: Map = m.into_iter().filter(|(b, _)| BUTTONS.iter().any(|(k, _)| k == b)).map(|(b, [x, y])| (b, [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)])).collect();
    if m.is_empty() {
        f.games.remove(pkg);
    } else {
        f.games.insert(pkg.to_string(), m);
    }
    save_all(&f)?;
    // A game that is on now takes its new spots at once.
    if let Some(l) = live().as_mut().filter(|l| l.pkg == pkg) {
        l.map = f.games.get(pkg).cloned().unwrap_or_default();
        l.fingers.clear();
    }
    crate::bump();
    Ok(())
}

/// The game was uninstalled: its spots go too.
pub fn forget(pkg: &str) {
    let mut f = load_all();
    if f.games.remove(pkg).is_some() {
        let _ = save_all(&f);
    }
}

/// The still the editor draws the spots over: the game's frame from the last time Rare's menu
/// was opened over it.
pub fn frame_path(pkg: &str) -> PathBuf {
    crate::dir().join("frames").join(format!("{pkg}.png"))
}

/// Keeps a frame of the game for the editor (a copy of the overlay's capture).
pub fn keep_frame(pkg: &str, from: &std::path::Path) {
    let to = frame_path(pkg);
    if let Some(d) = to.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::copy(from, to);
}

/// Fingers in use: button -> slot.
struct Live {
    pkg: String,
    map: Map,
    fingers: BTreeMap<String, usize>,
    #[cfg(target_os = "linux")]
    dev: Option<imp::Screen>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn live() -> std::sync::MutexGuard<'static, Option<Live>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Touch controls are on (a mapped game is in front).
pub fn active() -> bool {
    live().as_ref().is_some_and(|l| !l.map.is_empty())
}

/// The game in front now, when its touch controls are on.
pub fn active_for() -> Option<String> {
    live().as_ref().filter(|l| !l.map.is_empty()).map(|l| l.pkg.clone())
}

/// Turns touch controls on for a game (false when it has no spots, or no virtual screen could be
/// made). Called every frame while the game is in front: its spots are read once.
pub fn begin(pkg: &str) -> bool {
    let mut g = live();
    if g.as_ref().map_or(true, |l| l.pkg != pkg) {
        *g = Some(Live {
            pkg: pkg.to_string(),
            map: map(pkg),
            fingers: BTreeMap::new(),
            #[cfg(target_os = "linux")]
            dev: None,
        });
    }
    let Some(l) = g.as_mut() else { return false };
    if l.map.is_empty() {
        return false;
    }
    #[cfg(target_os = "linux")]
    if l.dev.is_none() {
        let (w, h) = crate::screen();
        match imp::Screen::new(w, h) {
            Ok(d) => l.dev = Some(d),
            Err(e) => {
                log::warn!("android touch: {e}");
                // Not again every frame: the spots stay off for this run.
                l.map.clear();
                return false;
            }
        }
    }
    true
}

/// Lifts every finger and removes the virtual screen.
pub fn end() {
    let mut g = live();
    if let Some(l) = g.as_mut() {
        let held: Vec<String> = l.fingers.keys().cloned().collect();
        for b in held {
            lift(l, &b);
        }
    }
    *g = None;
}

/// Lifts every finger but keeps touch controls on (Rare's menu came up over the game).
pub fn lift_all() {
    let mut g = live();
    if let Some(l) = g.as_mut() {
        let held: Vec<String> = l.fingers.keys().cloned().collect();
        for b in held {
            lift(l, &b);
        }
    }
}

fn lift(l: &mut Live, button: &str) {
    if let Some(_slot) = l.fingers.remove(button) {
        #[cfg(target_os = "linux")]
        if let Some(d) = l.dev.as_mut() {
            d.up(_slot, l.fingers.is_empty());
        }
    }
}

/// A pad button went down or up. True when it has a spot (the touch was made, nothing else
/// should act on the button).
pub fn press(button: &str, down: bool) -> bool {
    let mut g = live();
    let Some(l) = g.as_mut() else { return false };
    let Some([x, y]) = l.map.get(button).copied() else { return false };
    if !down {
        lift(l, button);
        return true;
    }
    if l.fingers.contains_key(button) {
        return true;
    }
    let Some(slot) = (0..imp_slots()).find(|s| !l.fingers.values().any(|v| v == s)) else { return true };
    l.fingers.insert(button.to_string(), slot);
    #[cfg(target_os = "linux")]
    if let Some(d) = l.dev.as_mut() {
        d.down(slot, x, y);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (x, y);
    true
}

const SLOTS: usize = 10;

fn imp_slots() -> usize {
    SLOTS
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{DEVICE_NAME, SLOTS};
    use evdev::uinput::VirtualDevice;
    use evdev::{AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, PropType, UinputAbsSetup};

    const ABS_X: u16 = 0x00;
    const ABS_Y: u16 = 0x01;
    const ABS_MT_SLOT: u16 = 0x2f;
    const ABS_MT_POSITION_X: u16 = 0x35;
    const ABS_MT_POSITION_Y: u16 = 0x36;
    const ABS_MT_TRACKING_ID: u16 = 0x39;
    const BTN_TOUCH: u16 = 0x14a;
    const INPUT_PROP_DIRECT: u16 = 0x01;
    const SYN_REPORT: u16 = 0;

    pub struct Screen {
        dev: VirtualDevice,
        w: u32,
        h: u32,
        next_id: i32,
    }

    fn ev(kind: EventType, code: u16, value: i32) -> InputEvent {
        InputEvent::new(kind.0, code, value)
    }

    impl Screen {
        pub fn new(w: u32, h: u32) -> std::io::Result<Screen> {
            let mut keys = AttributeSet::<KeyCode>::new();
            keys.insert(KeyCode(BTN_TOUCH));
            let mut props = AttributeSet::<PropType>::new();
            props.insert(PropType(INPUT_PROP_DIRECT));
            let xi = AbsInfo::new(0, 0, w as i32 - 1, 0, 0, 0);
            let yi = AbsInfo::new(0, 0, h as i32 - 1, 0, 0, 0);
            let dev = VirtualDevice::builder()?
                .name(DEVICE_NAME)
                .input_id(InputId::new(BusType::BUS_VIRTUAL, 0x5241, 0x0001, 1))
                .with_keys(&keys)?
                .with_properties(&props)?
                .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(ABS_X), xi))?
                .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(ABS_Y), yi))?
                .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(ABS_MT_SLOT), AbsInfo::new(0, 0, SLOTS as i32 - 1, 0, 0, 0)))?
                .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(ABS_MT_TRACKING_ID), AbsInfo::new(0, 0, 65535, 0, 0, 0)))?
                .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(ABS_MT_POSITION_X), xi))?
                .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(ABS_MT_POSITION_Y), yi))?
                .build()?;
            Ok(Screen { dev, w, h, next_id: 1 })
        }

        pub fn down(&mut self, slot: usize, x: f32, y: f32) {
            let px = (x * (self.w as f32 - 1.0)).round() as i32;
            let py = (y * (self.h as f32 - 1.0)).round() as i32;
            let id = self.next_id;
            self.next_id = if self.next_id >= 65535 { 1 } else { self.next_id + 1 };
            let evs = [
                ev(EventType::ABSOLUTE, ABS_MT_SLOT, slot as i32),
                ev(EventType::ABSOLUTE, ABS_MT_TRACKING_ID, id),
                ev(EventType::ABSOLUTE, ABS_MT_POSITION_X, px),
                ev(EventType::ABSOLUTE, ABS_MT_POSITION_Y, py),
                ev(EventType::KEY, BTN_TOUCH, 1),
                ev(EventType::ABSOLUTE, ABS_X, px),
                ev(EventType::ABSOLUTE, ABS_Y, py),
                ev(EventType::SYNCHRONIZATION, SYN_REPORT, 0),
            ];
            if let Err(e) = self.dev.emit(&evs) {
                log::warn!("android touch down: {e}");
            }
        }

        pub fn up(&mut self, slot: usize, last: bool) {
            let mut evs = vec![ev(EventType::ABSOLUTE, ABS_MT_SLOT, slot as i32), ev(EventType::ABSOLUTE, ABS_MT_TRACKING_ID, -1)];
            if last {
                evs.push(ev(EventType::KEY, BTN_TOUCH, 0));
            }
            evs.push(ev(EventType::SYNCHRONIZATION, SYN_REPORT, 0));
            if let Err(e) = self.dev.emit(&evs) {
                log::warn!("android touch up: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingers_follow_buttons() {
        let map: Map = [("a".to_string(), [0.5, 0.5]), ("left".to_string(), [0.1, 0.8])].into_iter().collect();
        *live() = Some(Live {
            pkg: "p".into(),
            map,
            fingers: BTreeMap::new(),
            #[cfg(target_os = "linux")]
            dev: None,
        });
        assert!(active());
        assert!(press("a", true));
        assert!(press("left", true));
        assert!(!press("b", true));
        {
            let g = live();
            let f = &g.as_ref().unwrap().fingers;
            assert_eq!(f.get("a"), Some(&0));
            assert_eq!(f.get("left"), Some(&1));
        }
        assert!(press("a", false));
        assert!(!press("x", false));
        assert!(press("a", true));
        assert_eq!(live().as_ref().unwrap().fingers.get("a"), Some(&0));
        end();
        assert!(!active());
    }
}
