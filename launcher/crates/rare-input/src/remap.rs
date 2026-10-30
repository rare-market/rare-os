//! The button layout (Settings > Controls > Button layout): which pad button does what, for every
//! game and the launcher, through an InputPlumber device profile.
//!
//! InputPlumber translates every event with the composite device's profile before anything else
//! sees it (games, Steam, the launcher, and the `ui_*` actions while Rare intercepts). It loads
//! `/usr/share/inputplumber/profiles/default.yaml` when the device starts. Rare writes its own
//! profile, `~/.config/rare/inputplumber/layout.yaml`: that default's mappings (merged, so nothing
//! the default does is lost) plus one mapping per changed button, and loads it with
//! `org.shadowblip.Input.CompositeDevice.LoadProfilePath` (InputPlumber 0.83). Reset loads the
//! default profile again.
//!
//! The Rare, Quick, LC and RC buttons are not in the profile and can never be remapped here: they
//! are D-Bus actions from Rare's capability map (`/etc/inputplumber/capability_maps.d`, see ip.rs),
//! which no profile touches.
//!
//! The layout itself is kept in `~/.config/rare/inputplumber/layout.txt` (`a=b` lines), and put
//! back on launcher start and whenever InputPlumber comes back with another profile (a restart).
//! LoadProfilePath needs the polkit rule in image/system_files (50-rare-inputplumber.rules); on an
//! image without it InputPlumber refuses and the layout waits for the next system update.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// A pad button the layout can change. Guide (Rare) and Quick are never here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Button {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    L3,
    R3,
    View,
    Menu,
    Up,
    Down,
    Left,
    Right,
}

pub const BUTTONS: [Button; 16] = [
    Button::A,
    Button::B,
    Button::X,
    Button::Y,
    Button::Lb,
    Button::Rb,
    Button::Lt,
    Button::Rt,
    Button::L3,
    Button::R3,
    Button::View,
    Button::Menu,
    Button::Up,
    Button::Down,
    Button::Left,
    Button::Right,
];

/// What InputPlumber calls a source or target: a button, or an analog trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cap {
    Button(&'static str),
    Trigger(&'static str),
}

impl Button {
    pub fn label(self) -> &'static str {
        match self {
            Button::A => "A",
            Button::B => "B",
            Button::X => "X",
            Button::Y => "Y",
            Button::Lb => "LB",
            Button::Rb => "RB",
            Button::Lt => "LT",
            Button::Rt => "RT",
            Button::L3 => "L3",
            Button::R3 => "R3",
            Button::View => "View",
            Button::Menu => "Menu",
            Button::Up => "D-pad up",
            Button::Down => "D-pad down",
            Button::Left => "D-pad left",
            Button::Right => "D-pad right",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Button::A => "a",
            Button::B => "b",
            Button::X => "x",
            Button::Y => "y",
            Button::Lb => "lb",
            Button::Rb => "rb",
            Button::Lt => "lt",
            Button::Rt => "rt",
            Button::L3 => "l3",
            Button::R3 => "r3",
            Button::View => "view",
            Button::Menu => "menu",
            Button::Up => "up",
            Button::Down => "down",
            Button::Left => "left",
            Button::Right => "right",
        }
    }

    pub fn from_key(k: &str) -> Option<Button> {
        BUTTONS.iter().copied().find(|b| b.key() == k)
    }

    /// InputPlumber's name. X is North (evdev 307) and Y is West (308), as in ip.rs.
    fn cap(self) -> Cap {
        match self {
            Button::A => Cap::Button("South"),
            Button::B => Cap::Button("East"),
            Button::X => Cap::Button("North"),
            Button::Y => Cap::Button("West"),
            Button::Lb => Cap::Button("LeftBumper"),
            Button::Rb => Cap::Button("RightBumper"),
            Button::Lt => Cap::Trigger("LeftTrigger"),
            Button::Rt => Cap::Trigger("RightTrigger"),
            Button::L3 => Cap::Button("LeftStick"),
            Button::R3 => Cap::Button("RightStick"),
            Button::View => Cap::Button("Select"),
            Button::Menu => Cap::Button("Start"),
            Button::Up => Cap::Button("DPadUp"),
            Button::Down => Cap::Button("DPadDown"),
            Button::Left => Cap::Button("DPadLeft"),
            Button::Right => Cap::Button("DPadRight"),
        }
    }
}

/// What a button does now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Button(Button),
    Nothing,
}

impl Target {
    pub fn label(self) -> &'static str {
        match self {
            Target::Button(b) => b.label(),
            Target::Nothing => "Nothing",
        }
    }
}

/// The layout: only the buttons that differ from the pad's own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    map: BTreeMap<Button, Target>,
}

impl Layout {
    pub fn target(&self, b: Button) -> Target {
        self.map.get(&b).copied().unwrap_or(Target::Button(b))
    }

    pub fn set(&mut self, b: Button, t: Target) {
        if t == Target::Button(b) {
            self.map.remove(&b);
        } else {
            self.map.insert(b, t);
        }
    }

    pub fn is_default(&self) -> bool {
        self.map.is_empty()
    }

    pub fn changed(&self) -> usize {
        self.map.len()
    }

    pub fn swapped(&self, a: Button, b: Button) -> bool {
        self.target(a) == Target::Button(b) && self.target(b) == Target::Button(a)
    }

    /// The quick presets: swaps two buttons, or puts them back when they are swapped.
    pub fn toggle_swap(&mut self, a: Button, b: Button) {
        if self.swapped(a, b) {
            self.set(a, Target::Button(a));
            self.set(b, Target::Button(b));
        } else {
            self.set(a, Target::Button(b));
            self.set(b, Target::Button(a));
        }
    }

    /// Some button gives `t`.
    pub fn produces(&self, t: Button) -> bool {
        BUTTONS.iter().any(|&b| self.target(b) == Target::Button(t))
    }

    /// A layout that would lock the person out of the launcher is refused.
    pub fn check(&self) -> Result<(), &'static str> {
        if !self.produces(Button::A) || !self.produces(Button::B) {
            return Err("Keep one button for A and one for B, so you can always get back here.");
        }
        Ok(())
    }

    /// One short line for the Controls row.
    pub fn summary(&self) -> String {
        if self.is_default() {
            return "Standard.".into();
        }
        let mut parts = Vec::new();
        let mut rest = self.changed();
        for (a, b) in [(Button::A, Button::B), (Button::X, Button::Y)] {
            if self.swapped(a, b) {
                parts.push(format!("{} and {} swapped", a.label(), b.label()));
                rest -= 2;
            }
        }
        if rest > 0 {
            parts.push(match (parts.is_empty(), rest) {
                (true, 1) => "1 button changed".to_string(),
                (true, n) => format!("{n} buttons changed"),
                (false, n) => format!("{n} more changed"),
            });
        }
        format!("{}.", parts.join(", "))
    }

    /// `a=b` lines (`a=none` for Nothing).
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for (b, t) in &self.map {
            let v = match t {
                Target::Button(x) => x.key(),
                Target::Nothing => "none",
            };
            s.push_str(&format!("{}={}\n", b.key(), v));
        }
        s
    }

    pub fn from_text(s: &str) -> Layout {
        let mut l = Layout::default();
        for line in s.lines() {
            let Some((k, v)) = line.trim().split_once('=') else { continue };
            let Some(b) = Button::from_key(k.trim()) else { continue };
            let t = match v.trim() {
                "none" => Target::Nothing,
                v => match Button::from_key(v) {
                    Some(x) => Target::Button(x),
                    None => continue,
                },
            };
            l.set(b, t);
        }
        l
    }

    /// The InputPlumber device profile: the default profile's mappings, then one per changed
    /// button. An unmapped button passes through as itself; an empty target list drops it.
    pub fn profile_yaml(&self) -> String {
        let mut y = String::new();
        y.push_str("# Written by the Rare launcher (Settings > Controls > Button layout). Changes here are replaced.\n");
        y.push_str("version: 1\nkind: DeviceProfile\nname: Rare button layout\n\nmapping:\n");
        y.push_str(DEFAULT_MAPPINGS);
        for (b, t) in &self.map {
            y.push_str(&format!("  - name: Rare {}\n", b.label()));
            y.push_str("    source_event:\n");
            y.push_str(&cap_yaml(b.cap(), 6));
            match t {
                Target::Nothing => y.push_str("    target_events: []\n"),
                Target::Button(x) => {
                    y.push_str("    target_events:\n");
                    let c = cap_yaml(x.cap(), 8);
                    // The first line of the item carries the list dash.
                    y.push_str(&format!("      - {}", &c[8..]));
                }
            }
        }
        y
    }
}

fn cap_yaml(c: Cap, indent: usize) -> String {
    let p = " ".repeat(indent);
    match c {
        Cap::Button(n) => format!("{p}gamepad:\n{p}  button: {n}\n"),
        Cap::Trigger(n) => format!("{p}gamepad:\n{p}  trigger:\n{p}    name: {n}\n"),
    }
}

/// InputPlumber 0.83.1's rootfs/usr/share/inputplumber/profiles/default.yaml mappings, the
/// profile every composite device starts with. Kept in Rare's profile so loading it changes only
/// the buttons the person changed. None of these sources is a Button above.
const DEFAULT_MAPPINGS: &str = "  - name: LeftTop
    source_event:
      gamepad:
        button: LeftTop
    target_events:
      - gamepad:
          button: LeftPaddle1
  - name: RightTop
    source_event:
      gamepad:
        button: RightTop
    target_events:
      - gamepad:
          button: RightPaddle1
  - name: Keyboard
    source_event:
      gamepad:
        button: Keyboard
    target_events:
      - gamepad:
          button: Guide
      - gamepad:
          button: North
  - name: QuickAccess2
    source_event:
      gamepad:
        button: QuickAccess2
    target_events:
      - gamepad:
          button: Screenshot
  - name: Left Dial Counter-clockwise
    source_event:
      gamepad:
        dial:
          name: LeftStickDial
          direction: counter-clockwise
    target_events:
      - keyboard: KeyVolumeDown
  - name: Left Dial Clockwise
    source_event:
      gamepad:
        dial:
          name: LeftStickDial
          direction: clockwise
    target_events:
      - keyboard: KeyVolumeUp
  - name: Right Dial Counter-clockwise
    source_event:
      gamepad:
        dial:
          name: RightStickDial
          direction: counter-clockwise
    target_events:
      - keyboard: KeyBrightnessDown
  - name: Right Dial Clockwise
    source_event:
      gamepad:
        dial:
          name: RightStickDial
          direction: clockwise
    target_events:
      - keyboard: KeyBrightnessUp
";

/// What the last apply did.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Nothing tried yet.
    Idle,
    /// InputPlumber has the layout (or the default profile, after a reset).
    Applied,
    /// The system refused: the polkit rule comes with the next system update.
    NeedsUpdate,
    /// InputPlumber has no LoadProfilePath.
    TooOld,
    /// No InputPlumber, or no built-in controller.
    NoDevice,
    Failed(String),
}

impl Status {
    /// The plain line the layout sheet shows, if any.
    pub fn line(&self) -> Option<String> {
        match self {
            Status::Idle | Status::Applied => None,
            Status::NeedsUpdate => Some("Saved. This system can't change buttons yet, so it takes effect after the next system update.".into()),
            Status::TooOld => Some("Saved. InputPlumber on this system is too old to change buttons.".into()),
            Status::NoDevice => Some("Saved. InputPlumber is not running, so the buttons can't be changed right now.".into()),
            Status::Failed(e) => Some(format!("Saved, but InputPlumber refused it: {e}")),
        }
    }
}

static STATUS: Mutex<Status> = Mutex::new(Status::Idle);

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or(Status::Idle)
}

fn set_status(s: Status) {
    if let Ok(mut g) = STATUS.lock() {
        *g = s;
    }
}

/// ~/.config/rare/inputplumber
pub fn dir() -> PathBuf {
    rare_core::paths::config_dir().join("inputplumber")
}

pub fn profile_path() -> PathBuf {
    dir().join("layout.yaml")
}

fn layout_path() -> PathBuf {
    dir().join("layout.txt")
}

/// The saved layout (the standard one when there is none).
pub fn load() -> Layout {
    std::fs::read_to_string(layout_path()).map(|s| Layout::from_text(&s)).unwrap_or_default()
}

/// Saves the layout and its profile, then loads it into InputPlumber (blocking: call it off the
/// UI thread). A standard layout loads InputPlumber's default profile again.
pub fn apply(l: &Layout) -> Status {
    let st = match save(l) {
        Err(e) => Status::Failed(e),
        Ok(()) => imp::load(l),
    };
    if st != Status::Applied {
        crate::log(&format!("[remap] {st:?}"));
    }
    set_status(st.clone());
    st
}

fn save(l: &Layout) -> Result<(), String> {
    std::fs::create_dir_all(dir()).map_err(|e| format!("{}: {e}", dir().display()))?;
    std::fs::write(layout_path(), l.to_text()).map_err(|e| format!("layout: {e}"))?;
    std::fs::write(profile_path(), l.profile_yaml()).map_err(|e| format!("profile: {e}"))?;
    Ok(())
}

/// Puts the saved layout back on launcher start, and again whenever InputPlumber comes back with
/// another profile (it restarted, or re-created the device). Its own thread.
pub fn start() {
    imp::start();
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::Connection;
    use std::time::Duration;

    const DEST: &str = "org.shadowblip.InputPlumber";
    const DEVICE_IFACE: &str = "org.shadowblip.Input.CompositeDevice";
    const TIMEOUT: Duration = Duration::from_secs(10);
    /// InputPlumber's own default profile (its get_profiles_path on the image).
    const DEFAULT_PROFILE: &str = "/usr/share/inputplumber/profiles/default.yaml";

    /// The built-in controller's composite device: the SuiPlay, else the first pad InputPlumber
    /// lists. Looked up every time; the index changes when InputPlumber re-creates it.
    fn device(conn: &Connection) -> Option<String> {
        let mgr = conn.with_proxy(DEST, "/org/shadowblip/InputPlumber/Manager", TIMEOUT);
        let mut paths: Vec<String> = mgr.get("org.shadowblip.InputManager", "GamepadOrder").unwrap_or_default();
        if paths.is_empty() {
            paths = (0..4).map(|i| format!("/org/shadowblip/InputPlumber/CompositeDevice{i}")).collect();
        }
        let mut first = None;
        for p in paths {
            let name: Result<String, _> = conn.with_proxy(DEST, p.as_str(), TIMEOUT).get(DEVICE_IFACE, "Name");
            match name {
                Ok(n) if n.starts_with("SuiPlay") => return Some(p),
                Ok(_) if first.is_none() => first = Some(p),
                _ => {}
            }
        }
        first
    }

    fn classify(e: &dbus::Error) -> Status {
        let name = e.name().unwrap_or("");
        let msg = e.message().unwrap_or("");
        if name.ends_with("UnknownMethod") {
            Status::TooOld
        } else if name.ends_with("InteractiveAuthorizationRequired") || name.ends_with("AccessDenied") || msg.contains("Not authorized") || msg.contains("Authentication required") {
            Status::NeedsUpdate
        } else if name.ends_with("ServiceUnknown") || name.ends_with("NameHasNoOwner") {
            Status::NoDevice
        } else {
            Status::Failed(msg.to_string())
        }
    }

    pub fn load(l: &Layout) -> Status {
        let Ok(conn) = Connection::new_system() else { return Status::NoDevice };
        let Some(dev) = device(&conn) else { return Status::NoDevice };
        let path = if l.is_default() { DEFAULT_PROFILE.to_string() } else { profile_path().to_string_lossy().to_string() };
        let r: Result<(), dbus::Error> = conn.with_proxy(DEST, dev.as_str(), TIMEOUT).method_call(DEVICE_IFACE, "LoadProfilePath", (path.clone(),));
        match r {
            Ok(()) => {
                crate::log(&format!("[remap] loaded {path} on {dev}"));
                Status::Applied
            }
            Err(e) => classify(&e),
        }
    }

    fn loaded_path(conn: &Connection) -> Option<String> {
        let dev = device(conn)?;
        conn.with_proxy(DEST, dev.as_str(), TIMEOUT).get(DEVICE_IFACE, "ProfilePath").ok()
    }

    pub fn start() {
        std::thread::spawn(|| {
            let ours = profile_path().to_string_lossy().to_string();
            loop {
                let l = super::load();
                if !l.is_default() {
                    let wrong = match Connection::new_system() {
                        Ok(c) => loaded_path(&c).map(|p| p != ours),
                        Err(_) => None,
                    };
                    // Retry while InputPlumber is not up yet; give up on a refusal (it needs
                    // the next system update) until the person changes the layout again.
                    let st = status();
                    let retry = matches!(st, Status::Idle | Status::Applied | Status::NoDevice);
                    if wrong == Some(true) && retry {
                        apply(&l);
                    } else if wrong.is_none() && st == Status::Idle {
                        set_status(Status::NoDevice);
                    }
                }
                std::thread::sleep(Duration::from_secs(10));
            }
        });
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;

    pub fn load(_l: &Layout) -> Status {
        Status::NoDevice
    }

    pub fn start() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_presets_toggle() {
        let mut l = Layout::default();
        assert_eq!(l.summary(), "Standard.");
        l.toggle_swap(Button::A, Button::B);
        assert_eq!(l.target(Button::A), Target::Button(Button::B));
        assert_eq!(l.target(Button::B), Target::Button(Button::A));
        assert_eq!(l.summary(), "A and B swapped.");
        l.toggle_swap(Button::X, Button::Y);
        l.set(Button::L3, Target::Nothing);
        assert_eq!(l.summary(), "A and B swapped, X and Y swapped, 1 more changed.");
        l.toggle_swap(Button::A, Button::B);
        assert_eq!(l.target(Button::A), Target::Button(Button::A));
        assert_eq!(l.changed(), 3);
        assert_eq!(l.summary(), "X and Y swapped, 1 more changed.");
        // Setting a button to itself is the default again.
        l.set(Button::L3, Target::Button(Button::L3));
        l.toggle_swap(Button::X, Button::Y);
        assert!(l.is_default());
    }

    #[test]
    fn lockout_refused() {
        let mut l = Layout::default();
        l.set(Button::A, Target::Nothing);
        assert!(l.check().is_err());
        // Another button gives A: fine.
        l.set(Button::Menu, Target::Button(Button::A));
        assert!(l.check().is_ok());
        l.set(Button::B, Target::Button(Button::X));
        assert!(l.check().is_err());
    }

    #[test]
    fn text_round_trip() {
        let mut l = Layout::default();
        l.toggle_swap(Button::A, Button::B);
        l.set(Button::Lt, Target::Button(Button::Rb));
        l.set(Button::View, Target::Nothing);
        let t = l.to_text();
        assert_eq!(Layout::from_text(&t), l);
        assert_eq!(Layout::from_text("junk\nq=a\na=zz\n"), Layout::default());
    }

    #[test]
    fn profile_yaml() {
        let mut l = Layout::default();
        l.toggle_swap(Button::A, Button::B);
        l.set(Button::Lt, Target::Button(Button::Rb));
        l.set(Button::Up, Target::Button(Button::Rt));
        l.set(Button::View, Target::Nothing);
        let y = l.profile_yaml();
        assert!(y.contains("version: 1\nkind: DeviceProfile\nname: Rare button layout\n\nmapping:\n"));
        // The default profile's mappings are kept.
        assert!(y.contains("  - name: LeftTop\n    source_event:\n      gamepad:\n        button: LeftTop\n"));
        assert!(y.contains("          button: RightPaddle1\n"));
        assert!(y.contains(
            "  - name: Rare A
    source_event:
      gamepad:
        button: South
    target_events:
      - gamepad:
          button: East
"
        ));
        assert!(y.contains(
            "  - name: Rare B
    source_event:
      gamepad:
        button: East
    target_events:
      - gamepad:
          button: South
"
        ));
        // A trigger as the source, a button as the target.
        assert!(y.contains(
            "  - name: Rare LT
    source_event:
      gamepad:
        trigger:
          name: LeftTrigger
    target_events:
      - gamepad:
          button: RightBumper
"
        ));
        // A button as the source, a trigger as the target.
        assert!(y.contains(
            "  - name: Rare D-pad up
    source_event:
      gamepad:
        button: DPadUp
    target_events:
      - gamepad:
          trigger:
            name: RightTrigger
"
        ));
        assert!(y.contains("  - name: Rare View\n    source_event:\n      gamepad:\n        button: Select\n    target_events: []\n"));
        // Unchanged buttons are not in the profile; Guide never is.
        assert!(!y.contains("button: North\n    target"));
        assert!(!y.contains("Guide\n    target"));
        // Each mapping is a list item at two spaces, nothing else is.
        let items = y.lines().filter(|l| l.starts_with("  - name: ")).count();
        assert_eq!(items, 8 + 5);
        assert!(y.lines().all(|l| !l.contains('\t')));

        // It parses, in InputPlumber's DeviceProfile shape (config/mod.rs ProfileMapping:
        // source_event, target_events), and an unknown field would be refused like upstream does.
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Profile {
            version: u32,
            kind: String,
            name: String,
            mapping: Vec<Mapping>,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Mapping {
            name: String,
            source_event: serde_yaml::Value,
            target_events: Vec<serde_yaml::Value>,
        }
        let p: Profile = serde_yaml::from_str(&y).expect("valid yaml");
        assert_eq!((p.version, p.kind.as_str(), p.name.as_str()), (1, "DeviceProfile", "Rare button layout"));
        let view = p.mapping.iter().find(|m| m.name == "Rare View").unwrap();
        assert!(view.target_events.is_empty());
        let lt = p.mapping.iter().find(|m| m.name == "Rare LT").unwrap();
        assert_eq!(lt.source_event["gamepad"]["trigger"]["name"].as_str(), Some("LeftTrigger"));
        assert_eq!(lt.target_events[0]["gamepad"]["button"].as_str(), Some("RightBumper"));
        let keyboard = p.mapping.iter().find(|m| m.name == "Keyboard").unwrap();
        assert_eq!(keyboard.target_events.len(), 2);
    }
}
