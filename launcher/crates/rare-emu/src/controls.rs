//! Controller profiles: every emulator bound to the one pad games should use, with every button.
//!
//! On the SuiPlay0X1 the pad games see is InputPlumber's target, "Microsoft X-Box One Elite 2 pad"
//! (045e:0b00, SDL calls it "Xbox One Elite 2 Controller"). The pad's own device, the "Microsoft
//! X-Box 360 pad" (045e:028e), is grabbed by InputPlumber and never sends anything; an emulator that
//! binds it (SDL lists it first, as index 0) gets no input at all. So:
//!
//! 1. `launch_env` hides the raw pad from SDL for everything Rare starts
//!    (`SDL_GAMECONTROLLER_IGNORE_DEVICES`), which makes the target SDL's pad 0;
//! 2. each profile binds the target by what that emulator binds with: SDL index 0 (DuckStation,
//!    PCSX2, PPSSPP, melonDS), the SDL name (Dolphin), the evdev name (RPCS3) or the SDL GUID
//!    (Azahar, Lime3DS, Cemu, Ryujinx, Citron). RetroArch takes SDL's driver at launch
//!    (retroarch::PAD_CFG) and its own autoconfig profile for the pad, for every core;
//! 3. hotkeys (save and load state, fast forward, the emulator's menu) are never on the pad: they are
//!    keyboard keys the Rare menu sends (`actions.rs`), so Start and Select always reach the game.
//!
//! What Rare writes is only the controller part of each emulator's settings (an INI section, a whole
//! controller file, Ryujinx's player 1 entry). Before writing, each part is classified:
//! - missing, an emulator's own keyboard default, or what Rare wrote last time and nobody changed:
//!   written without asking (`ensure`, at every launch and setup);
//! - the player's own pad setup: kept, and listed by `review` so the setup page can ask; only
//!   `replace` (after the player says yes) writes over it, with a copy of the old file next to it.
//!
//! The profiles are in docs/EMULATION.md, with the build machine proof (tools/emu).

use crate::defaults::Env;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------
// The pad
// ---------------------------------------------------------------------------------------------

/// The pad games should use, as each input layer names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pad {
    /// The evdev name ("Microsoft X-Box One Elite 2 pad"): RPCS3's evdev handler.
    pub evdev: String,
    /// The name SDL gives it as a game controller ("Xbox One Elite 2 Controller"): Dolphin, RetroArch.
    pub sdl: String,
    /// SDL's joystick GUID since SDL 2.26, with the CRC of the name: Cemu.
    pub guid: String,
    /// The same GUID with the CRC cleared, the form the yuzu and Citra forks keep: Azahar, Citron, Ryujinx.
    pub guid0: String,
    pub vendor: u16,
    pub product: u16,
}

/// SDL_crc16 (CRC-16/ARC), which SDL puts in bytes 2 and 3 of a joystick's GUID.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        let mut r = (crc as u8) ^ b;
        let mut c: u16 = 0;
        for _ in 0..8 {
            c = (if (c ^ r as u16) & 1 != 0 { 0xA001 } else { 0 }) ^ (c >> 1);
            r >>= 1;
        }
        crc = c ^ (crc >> 8);
    }
    crc
}

fn le(v: u16) -> String {
    format!("{:02x}{:02x}", v & 0xff, v >> 8)
}

/// SDL's GUID for an evdev pad: bus, name CRC, vendor, product, version, little endian.
pub fn sdl_guid(bus: u16, vendor: u16, product: u16, version: u16, name: &str) -> (String, String) {
    let tail = format!("{}0000{}0000{}0000", le(vendor), le(product), le(version));
    (format!("{}{}{tail}", le(bus), le(crc16(name.as_bytes()))), format!("{}0000{tail}", le(bus)))
}

/// The name SDL's controller database gives a pad by its ids (its evdev name otherwise).
fn sdl_name(vendor: u16, product: u16, evdev: &str) -> String {
    match (vendor, product) {
        (0x045e, 0x0b00) => "Xbox One Elite 2 Controller".into(),
        (0x045e, 0x0b12) => "Xbox Series X Controller".into(),
        (0x045e, 0x028e) => "Xbox 360 Controller".into(),
        (0x28de, 0x1205) => "Steam Deck Controller".into(),
        _ => evdev.into(),
    }
}

impl Pad {
    /// InputPlumber's xbox-elite target as it is on the SuiPlay0X1.
    pub fn suiplay() -> Pad {
        Pad::from_ids(0x0003, 0x045e, 0x0b00, 0x0001, "Microsoft X-Box One Elite 2 pad")
    }

    pub fn from_ids(bus: u16, vendor: u16, product: u16, version: u16, evdev: &str) -> Pad {
        let (guid, guid0) = sdl_guid(bus, vendor, product, version, evdev);
        Pad { evdev: evdev.into(), sdl: sdl_name(vendor, product, evdev), guid, guid0, vendor, product }
    }

    /// The pad on this device: InputPlumber's virtual pad when there is one, else the SuiPlay's.
    pub fn here() -> Pad {
        std::fs::read_to_string("/proc/bus/input/devices").ok().and_then(|t| target(&t)).unwrap_or_else(Pad::suiplay)
    }
}

struct Dev {
    bus: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: String,
    sysfs: String,
    js: bool,
}

fn devices(proc_devices: &str) -> Vec<Dev> {
    let mut out = Vec::new();
    for block in proc_devices.split("\n\n") {
        let mut d = Dev { bus: 0, vendor: 0, product: 0, version: 0, name: String::new(), sysfs: String::new(), js: false };
        for line in block.lines() {
            if let Some(r) = line.strip_prefix("I: ") {
                for kv in r.split_whitespace() {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    let v = u16::from_str_radix(v, 16).unwrap_or(0);
                    match k {
                        "Bus" => d.bus = v,
                        "Vendor" => d.vendor = v,
                        "Product" => d.product = v,
                        "Version" => d.version = v,
                        _ => {}
                    }
                }
            } else if let Some(r) = line.strip_prefix("N: Name=") {
                d.name = r.trim_matches('"').to_string();
            } else if let Some(r) = line.strip_prefix("S: Sysfs=") {
                d.sysfs = r.to_string();
            } else if let Some(r) = line.strip_prefix("H: Handlers=") {
                d.js = r.split_whitespace().any(|h| h.starts_with("js"));
            }
        }
        if d.js {
            out.push(d);
        }
    }
    out
}

/// InputPlumber's virtual pad (a virtual device with a js handler and an Xbox-style name) from
/// /proc/bus/input/devices text.
pub fn target(proc_devices: &str) -> Option<Pad> {
    devices(proc_devices)
        .into_iter()
        .filter(|d| d.sysfs.starts_with("/devices/virtual") && !RAW.contains(&(d.vendor, d.product)))
        .find(|d| {
            let n = d.name.to_ascii_lowercase();
            n.contains("x-box") || n.contains("xbox")
        })
        .map(|d| Pad::from_ids(d.bus, d.vendor, d.product, d.version, &d.name))
}

/// The pads InputPlumber reads on the SuiPlay (its profile's sources): grabbed, so silent to
/// everyone else. (vendor, product).
const RAW: &[(u16, u16)] = &[(0x045e, 0x028e), (0x057e, 0x2009)];

/// Environment for anything Rare starts while InputPlumber's pad is there, whichever target it
/// presents now (the Xbox pad, or the Steam Deck pad while gyro in Steam games is on): SDL leaves
/// the grabbed source pads out, so InputPlumber's pad is SDL's pad 0 and the only one. A source
/// id is never hidden when InputPlumber's own pad carries it (its xbox-360 target is 045e:028e).
pub fn launch_env_in(proc_devices: &str) -> Vec<(String, String)> {
    let live: Vec<(u16, u16)> = devices(proc_devices)
        .iter()
        .filter(|d| d.sysfs.starts_with("/devices/virtual") && {
            let n = d.name.to_ascii_lowercase();
            n.contains("x-box") || n.contains("xbox") || d.vendor == 0x28de
        })
        .map(|d| (d.vendor, d.product))
        .collect();
    if live.is_empty() {
        return vec![];
    }
    // The Elite or Deck pad is there: every source id goes. Only the xbox-360 target: keep its id.
    let all = live.iter().any(|l| !RAW.contains(l));
    let list: Vec<String> = RAW.iter().filter(|r| all || !live.contains(r)).map(|(v, p)| format!("0x{v:04x}/0x{p:04x}")).collect();
    if list.is_empty() {
        return vec![];
    }
    vec![("SDL_GAMECONTROLLER_IGNORE_DEVICES".into(), list.join(","))]
}

pub fn launch_env() -> Vec<(String, String)> {
    std::fs::read_to_string("/proc/bus/input/devices").map(|t| launch_env_in(&t)).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Pieces of settings Rare owns
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A whole file (a controller profile file of its own).
    File,
    /// One `[section]` of an INI or TOML file; the rest of the file is the player's.
    Section(String),
    /// Ryujinx's Config.json: the `input_config` entry for Player1.
    RyujinxPlayer1,
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub path: PathBuf,
    pub kind: Kind,
    /// The section's lines (no header), the whole file, or the JSON entry.
    pub body: String,
}

impl Piece {
    fn file(path: PathBuf, body: String) -> Piece {
        Piece { path, kind: Kind::File, body }
    }
    fn section(path: PathBuf, name: &str, body: String) -> Piece {
        Piece { path, kind: Kind::Section(name.into()), body }
    }
    fn key(&self) -> String {
        match &self.kind {
            Kind::File => self.path.display().to_string(),
            Kind::Section(s) => format!("{}#{s}", self.path.display()),
            Kind::RyujinxPlayer1 => format!("{}#Player1", self.path.display()),
        }
    }
}

/// What is on disk for a piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Exactly Rare's profile.
    Current,
    /// Not there yet.
    Missing,
    /// The emulator's own default: no pad in it (keyboard keys only).
    Default,
    /// Rare's own earlier profile, unchanged since.
    Rare,
    /// The player's own pad setup.
    Own,
}

impl State {
    /// Rare may write it without asking.
    pub fn free(self) -> bool {
        matches!(self, State::Missing | State::Default | State::Rare)
    }
}

// ---- INI sections (line based; works for DuckStation, PCSX2, Dolphin, Qt's INI and TOML tables)

fn header(line: &str) -> Option<&str> {
    // PPSSPP starts its files with a byte order mark.
    let t = line.trim().trim_start_matches('\u{feff}');
    t.strip_prefix('[').and_then(|r| r.strip_suffix(']'))
}

/// The lines of `[name]`, without the header and trailing blank lines. None when absent.
pub fn ini_section(text: &str, name: &str) -> Option<String> {
    let mut inside = false;
    let mut found = false;
    let mut out: Vec<&str> = Vec::new();
    for l in text.lines() {
        if let Some(h) = header(l) {
            if inside {
                break;
            }
            inside = h == name;
            found |= inside;
            continue;
        }
        if inside {
            out.push(l);
        }
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    found.then(|| out.iter().map(|l| format!("{l}\n")).collect())
}

/// `text` with `[name]` holding `body` (replaced in place, or appended).
pub fn ini_replace(text: &str, name: &str, body: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    let mut done = false;
    for l in text.lines() {
        if let Some(h) = header(l) {
            if inside {
                inside = false;
                out.push('\n');
            }
            if h == name && !done {
                out.push_str(&format!("[{name}]\n{body}"));
                inside = true;
                done = true;
                continue;
            }
        }
        if !inside {
            out.push_str(l);
            out.push('\n');
        }
    }
    if !done {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&format!("[{name}]\n{body}"));
    }
    out
}

fn norm(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    v.sort();
    v
}

/// Words that mean a gamepad is bound (SDL, evdev, a GUID, a joystick, InputPlumber's name).
const PAD_WORDS: &[&str] = &["sdl", "evdev", "guid", "joystick", "gamepad", "xinput/", "x-box", "xbox", "controller\""];

fn binds_a_pad(text: &str, words: &[&str]) -> bool {
    let l = text.to_ascii_lowercase();
    words.iter().any(|w| l.contains(w)) && !l.trim().is_empty()
}

/// Text that binds the SuiPlay's grabbed source pad (by SDL name, evdev name or GUID) or the
/// Steam Deck pad InputPlumber shows Steam (emulators always run with its Xbox pad).
pub fn bound_to_a_dead_pad(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    ["xbox 360 controller", "x-box 360 pad", "5e0400008e02", "steam deck controller", "de28000005120000", "de2800000512"].iter().any(|w| l.contains(w))
}

/// What tells Rare's own and the emulator's stock settings from the player's.
#[derive(Clone, Debug, Default)]
pub struct Known {
    /// Rare's earlier profiles for these pieces (before the manifest).
    pub legacy: Vec<String>,
    /// The emulator's own stock mappings, which bind the pad but are not the player's doing.
    pub stock: Vec<String>,
    /// Text that means a pad is bound in this emulator's format (PAD_WORDS when empty).
    pub pad_words: Vec<&'static str>,
    /// Or a test of the text, for formats where a pad binding is only a number.
    pub binds: Option<fn(&str) -> bool>,
}

fn subset_of(lines: &[String], known: &str) -> bool {
    let k = norm(known);
    lines.iter().all(|x| k.contains(x))
}

// ---- What Rare wrote

fn manifest_path(home: &Path) -> PathBuf {
    home.join(".local/share/rare/emu-controls.json")
}

fn hash(s: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(norm(s).join("\n").as_bytes());
    d.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn manifest(home: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(manifest_path(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_manifest(home: &Path, m: &BTreeMap<String, String>) {
    let p = manifest_path(home);
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new("/")));
    let _ = std::fs::write(p, serde_json::to_string_pretty(m).unwrap_or_default());
}

/// The piece's current text on disk (None when the file or the part is not there).
fn current(p: &Piece) -> Option<String> {
    let text = std::fs::read_to_string(&p.path).ok()?;
    match &p.kind {
        Kind::File => Some(text),
        Kind::Section(s) => ini_section(&text, s),
        Kind::RyujinxPlayer1 => {
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let e = v.get("input_config")?.as_array()?.iter().find(|e| e.get("player_index").and_then(|x| x.as_str()) == Some("Player1"))?;
            Some(serde_json::to_string_pretty(e).ok()? + "\n")
        }
    }
}

/// `v` cut down to the keys `shape` has, numbers as floats: Ryujinx saves its file again with
/// keys of its own and 1 for 1.0, which is still Rare's entry.
fn project(v: &serde_json::Value, shape: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value as V;
    match (v, shape) {
        (V::Object(o), V::Object(s)) => V::Object(s.keys().map(|k| (k.clone(), o.get(k).map(|x| project(x, &s[k])).unwrap_or(V::Null))).collect()),
        (V::Number(n), _) => n.as_f64().and_then(serde_json::Number::from_f64).map(V::Number).unwrap_or(V::Null),
        _ => v.clone(),
    }
}

/// The text a piece is compared and hashed by.
fn canon(p: &Piece, text: &str) -> String {
    match p.kind {
        Kind::RyujinxPlayer1 => match (serde_json::from_str::<serde_json::Value>(text), serde_json::from_str::<serde_json::Value>(&p.body)) {
            (Ok(v), Ok(shape)) => serde_json::to_string(&project(&v, &shape)).unwrap_or_default(),
            _ => text.to_string(),
        },
        _ => text.to_string(),
    }
}

/// How the piece on disk relates to Rare's profile.
pub fn classify(home: &Path, p: &Piece, known: &Known) -> State {
    let Some(cur) = current(p) else { return State::Missing };
    if norm(&canon(p, &cur)) == norm(&canon(p, &p.body)) {
        return State::Current;
    }
    if manifest(home).get(&p.key()) == Some(&hash(&canon(p, &cur))) {
        return State::Rare;
    }
    // Rare's earlier profiles (before the manifest), and what the Rare menu's hotkey lines added.
    let lines = norm(&cur);
    if known.legacy.iter().any(|l| subset_of(&lines, l)) {
        return State::Rare;
    }
    if known.stock.iter().any(|l| subset_of(&lines, l)) {
        return State::Default;
    }
    if p.kind == Kind::RyujinxPlayer1 {
        let v: serde_json::Value = serde_json::from_str(&cur).unwrap_or_default();
        let backend = v.get("backend").and_then(|b| b.as_str()).unwrap_or_default();
        return if backend.to_ascii_lowercase().contains("keyboard") { State::Default } else { State::Own };
    }
    // Bound to the silent source pad (Dolphin's own auto setup picks it as SDL/0, "Xbox 360
    // Controller"), or to the Steam Deck pad emulators never run with: nothing the player could
    // use, so not theirs to keep.
    if bound_to_a_dead_pad(&cur) {
        return State::Default;
    }
    let words: &[&str] = if known.pad_words.is_empty() { PAD_WORDS } else { &known.pad_words };
    let bound = match known.binds {
        Some(f) => f(&cur),
        None => binds_a_pad(&cur, words),
    };
    if bound {
        State::Own
    } else {
        State::Default
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("rare-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Writes one piece (and records what Rare wrote).
fn put(home: &Path, p: &Piece, backup: bool) -> std::io::Result<()> {
    let old = std::fs::read_to_string(&p.path).ok();
    if backup {
        if let Some(o) = &old {
            let name = format!("{}.before-rare", p.path.file_name().and_then(|n| n.to_str()).unwrap_or("settings"));
            let b = p.path.with_file_name(name);
            if !b.exists() {
                std::fs::write(b, o)?;
            }
        }
    }
    let text = match &p.kind {
        Kind::File => p.body.clone(),
        Kind::Section(s) => {
            let mut base = old.clone().unwrap_or_default();
            // melonDS's [Instance0.Joystick] needs its [Instance0] table with a key in it: without the
            // table melonDS cannot save its file, and an empty one makes it drop the bindings.
            if let Some((parent, _)) = s.rsplit_once('.') {
                if p.path.extension().is_some_and(|e| e == "toml") && ini_section(&base, parent).is_none() {
                    base = ini_replace(&base, parent, "JoystickID = 0\n");
                }
            }
            ini_replace(&base, s, &p.body)
        }
        Kind::RyujinxPlayer1 => {
            let mut v: serde_json::Value = old.as_deref().and_then(|t| serde_json::from_str(t).ok()).unwrap_or_else(|| serde_json::json!({}));
            let entry: serde_json::Value = serde_json::from_str(&p.body).map_err(std::io::Error::other)?;
            let list = v.as_object_mut().ok_or_else(|| std::io::Error::other("Config.json is not an object"))?.entry("input_config").or_insert_with(|| serde_json::json!([]));
            if !list.is_array() {
                *list = serde_json::json!([]);
            }
            let arr = list.as_array_mut().expect("array");
            arr.retain(|e| e.get("player_index").and_then(|x| x.as_str()) != Some("Player1"));
            arr.insert(0, entry);
            serde_json::to_string_pretty(&v).map_err(std::io::Error::other)? + "\n"
        }
    };
    write_atomic(&p.path, &text)?;
    let mut m = manifest(home);
    m.insert(p.key(), hash(&canon(p, &p.body)));
    save_manifest(home, &m);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The profiles
// ---------------------------------------------------------------------------------------------

/// Every emulator Rare has a controller profile for, with its settings folders.
pub struct Profile {
    pub emu: &'static str,
    pub name: &'static str,
    pub pieces: Vec<Piece>,
    pub known: Known,
}

/// The one in-game button map on the SuiPlay0X1: what each of its buttons is in every game,
/// whatever the system. Every profile here follows it; Settings > Controls > In games shows it and
/// docs/EMULATION.md has it. (button, what it does)
pub const STANDARD: &[(&str, &str)] = &[
    ("A, B, X, Y", "The console's face buttons by where they sit. On Nintendo systems the console's A is the right button (B) and its B the bottom one (A)."),
    ("Menu", "Start, or plus on Switch and Wii U"),
    ("View", "Select, Back, or minus on Switch and Wii U"),
    ("LB, RB", "The console's shoulder buttons"),
    ("LT, RT", "The console's triggers, analog where the console had them"),
    ("Left stick", "The console's stick"),
    ("Right stick", "Its second stick, or what the console used instead: C-stick, C buttons, the Wii Remote's pointer"),
    ("L3, R3", "Stick clicks"),
    ("D-pad", "D-pad"),
    ("Rare", "The Rare menu: save and load states, fast forward, the emulator's menu, quit. Never sent to the game."),
    ("Quick", "Quick settings. Never sent to the game."),
    ("LC, RC", "Nothing in games unless you pick an action in Settings > Controls or on the game's page"),
    ("View + Menu, held", "Rare's way out of the game (the Switcher). The game sees both presses."),
];

/// Per system: what each SuiPlay button is on that console. (system, [(SuiPlay button, console)])
pub const CONSOLES: &[(&str, &[(&str, &str)])] = &[
    ("NES, Famicom", &[("B", "A"), ("A", "B"), ("View", "Select"), ("Menu", "Start"), ("D-pad", "D-pad")]),
    ("Super Nintendo", &[("B", "A"), ("A", "B"), ("Y", "X"), ("X", "Y"), ("LB, RB", "L, R"), ("View", "Select"), ("Menu", "Start")]),
    ("Game Boy, Game Boy Color, Game Boy Advance", &[("B", "A"), ("A", "B"), ("LB, RB", "L, R (Advance)"), ("View", "Select"), ("Menu", "Start")]),
    ("Nintendo 64", &[("A", "A"), ("X", "B"), ("LT", "Z"), ("LB, RB", "L, R"), ("Menu", "Start"), ("Left stick", "Stick"), ("Right stick", "C buttons (or hold RT with A, B, X, Y)")]),
    ("Genesis, Mega Drive", &[("X", "A"), ("A", "B"), ("B", "C"), ("LB", "X"), ("Y", "Y"), ("RB", "Z"), ("View", "Mode"), ("Menu", "Start")]),
    ("PlayStation, PlayStation 2, PlayStation 3", &[("A", "Cross"), ("B", "Circle"), ("X", "Square"), ("Y", "Triangle"), ("LB, RB", "L1, R1"), ("LT, RT", "L2, R2 (pressure on PS2)"), ("L3, R3", "L3, R3"), ("View", "Select"), ("Menu", "Start"), ("Sticks", "Left and right sticks"), ("PS button", "None: the Rare menu")]),
    ("PSP", &[("A", "Cross"), ("B", "Circle"), ("X", "Square"), ("Y", "Triangle"), ("LB or LT", "L"), ("RB or RT", "R"), ("View", "Select"), ("Menu", "Start"), ("Left stick", "Analog nub")]),
    ("GameCube", &[("A", "A"), ("X", "B"), ("B", "X"), ("Y", "Y"), ("RB", "Z"), ("LT, RT", "L, R, analog"), ("Menu", "Start"), ("Left stick", "Control stick"), ("Right stick", "C-stick")]),
    ("Wii (Wii Remote and Nunchuk)", &[("A", "A"), ("RT", "B"), ("X", "1"), ("Y", "2"), ("View", "Minus"), ("Menu", "Plus"), ("R3", "Home"), ("B", "Shake"), ("Right stick", "Pointer"), ("D-pad", "D-pad"), ("Left stick", "Nunchuk stick"), ("LB", "Nunchuk C"), ("LT", "Nunchuk Z"), ("RB", "Nunchuk shake")]),
    ("Wii U, Switch", &[("B", "A"), ("A", "B"), ("Y", "X"), ("X", "Y"), ("LB, RB", "L, R"), ("LT, RT", "ZL, ZR"), ("View", "Minus"), ("Menu", "Plus"), ("L3, R3", "Stick clicks"), ("Sticks", "Left and right sticks"), ("Home, Capture", "None: the Rare menu")]),
    ("Nintendo 3DS", &[("B", "A"), ("A", "B"), ("Y", "X"), ("X", "Y"), ("LB, RB", "L, R"), ("LT, RT", "ZL, ZR"), ("View", "Select"), ("Menu", "Start"), ("Left stick", "Circle Pad"), ("Right stick", "C-Stick"), ("Touch screen", "Bottom screen")]),
    ("Nintendo DS", &[("B", "A"), ("A", "B"), ("Y", "X"), ("X", "Y"), ("LB, RB", "L, R"), ("View", "Select"), ("Menu", "Start"), ("D-pad", "D-pad"), ("Touch screen", "Bottom screen")]),
    ("Xbox", &[("A, B, X, Y", "A, B, X, Y"), ("LB", "White"), ("RB", "Black"), ("LT, RT", "Triggers"), ("View", "Back"), ("Menu", "Start"), ("L3, R3", "Stick clicks"), ("Sticks", "Left and right sticks")]),
    ("Dreamcast", &[("A, B, X, Y", "A, B, X, Y"), ("LT, RT", "Triggers, analog"), ("Menu", "Start"), ("Left stick", "Stick")]),
];

/// Rare's menu keys (actions.rs sends them from its virtual keyboard): F13 save state, F14 load
/// state, F15 the emulator's menu, F16 fast forward (toggle). No real keyboard has F13-F16.
pub const KEY_SAVE: &str = "F13";
pub const KEY_LOAD: &str = "F14";
pub const KEY_MENU: &str = "F15";
pub const KEY_FF: &str = "F16";

fn lines(pairs: &[(&str, String)], sep: &str) -> String {
    pairs.iter().map(|(k, v)| format!("{k}{sep}{v}\n")).collect()
}

/// DuckStation [Pad1]: an analog controller on SDL pad 0 with both motors.
fn duckstation_pad() -> String {
    let p = |k: &'static str, v: &str| (k, format!("SDL-0/{v}"));
    let mut v = vec![("Type", "AnalogController".to_string())];
    v.extend([
        p("Up", "DPadUp"), p("Right", "DPadRight"), p("Down", "DPadDown"), p("Left", "DPadLeft"),
        p("Triangle", "Y"), p("Circle", "B"), p("Cross", "A"), p("Square", "X"),
        p("Select", "Back"), p("Start", "Start"),
        p("L1", "LeftShoulder"), p("R1", "RightShoulder"), p("L2", "+LeftTrigger"), p("R2", "+RightTrigger"), p("L3", "LeftStick"), p("R3", "RightStick"),
        p("LLeft", "-LeftX"), p("LRight", "+LeftX"), p("LDown", "+LeftY"), p("LUp", "-LeftY"),
        p("RLeft", "-RightX"), p("RRight", "+RightX"), p("RDown", "+RightY"), p("RUp", "-RightY"),
        p("SmallMotor", "SmallMotor"), p("LargeMotor", "LargeMotor"),
    ]);
    lines(&v, " = ")
}

fn duckstation_hotkeys() -> String {
    let k = |name: &'static str, key: &str| (name, format!("Keyboard/{key}"));
    lines(&[k("SaveSelectedSaveState", KEY_SAVE), k("LoadSelectedSaveState", KEY_LOAD), k("OpenPauseMenu", KEY_MENU), k("ToggleFastForward", KEY_FF)], " = ")
}

/// PCSX2 [Pad1]: a DualShock 2 on SDL pad 0, pressure triggers, both motors.
fn pcsx2_pad() -> String {
    let p = |k: &'static str, v: &str| (k, format!("SDL-0/{v}"));
    let mut v = vec![("Type", "DualShock2".to_string()), ("Deadzone", "0.000000".into()), ("AxisScale", "1.330000".into())];
    v.extend([
        p("Up", "DPadUp"), p("Right", "DPadRight"), p("Down", "DPadDown"), p("Left", "DPadLeft"),
        p("Triangle", "FaceNorth"), p("Circle", "FaceEast"), p("Cross", "FaceSouth"), p("Square", "FaceWest"),
        p("Select", "Back"), p("Start", "Start"),
        p("L1", "LeftShoulder"), p("R1", "RightShoulder"), p("L2", "+LeftTrigger"), p("R2", "+RightTrigger"), p("L3", "LeftStick"), p("R3", "RightStick"),
        p("LUp", "-LeftY"), p("LRight", "+LeftX"), p("LDown", "+LeftY"), p("LLeft", "-LeftX"),
        p("RUp", "-RightY"), p("RRight", "+RightX"), p("RDown", "+RightY"), p("RLeft", "-RightX"),
        p("SmallMotor", "SmallMotor"), p("LargeMotor", "LargeMotor"),
    ]);
    lines(&v, " = ")
}

fn pcsx2_hotkeys() -> String {
    let k = |name: &'static str, key: &str| (name, format!("Keyboard/{key}"));
    lines(&[k("SaveStateToSlot", KEY_SAVE), k("LoadStateFromSlot", KEY_LOAD), k("OpenPauseMenu", KEY_MENU), k("ToggleTurbo", KEY_FF)], " = ")
}

/// Dolphin's GameCube pad: positional (B left of A, X right of A, as on the GameCube), Z on RB,
/// analog L and R on the triggers, rumble.
pub fn dolphin_gc(pad: &Pad) -> String {
    let v: Vec<(&str, String)> = vec![
        ("Device", format!("SDL/0/{}", pad.sdl)),
        ("Buttons/A", "`Button S`".into()),
        ("Buttons/B", "`Button W`".into()),
        ("Buttons/X", "`Button E`".into()),
        ("Buttons/Y", "`Button N`".into()),
        ("Buttons/Z", "`Shoulder R`".into()),
        ("Buttons/Start", "Start".into()),
        ("Main Stick/Up", "`Left Y+`".into()),
        ("Main Stick/Down", "`Left Y-`".into()),
        ("Main Stick/Left", "`Left X-`".into()),
        ("Main Stick/Right", "`Left X+`".into()),
        ("C-Stick/Up", "`Right Y+`".into()),
        ("C-Stick/Down", "`Right Y-`".into()),
        ("C-Stick/Left", "`Right X-`".into()),
        ("C-Stick/Right", "`Right X+`".into()),
        ("Triggers/L", "`Trigger L`".into()),
        ("Triggers/R", "`Trigger R`".into()),
        ("Triggers/L-Analog", "`Trigger L`".into()),
        ("Triggers/R-Analog", "`Trigger R`".into()),
        ("D-Pad/Up", "`Pad N`".into()),
        ("D-Pad/Down", "`Pad S`".into()),
        ("D-Pad/Left", "`Pad W`".into()),
        ("D-Pad/Right", "`Pad E`".into()),
        ("Rumble/Motor", "Motor".into()),
    ];
    lines(&v, " = ")
}

/// Dolphin's emulated Wii Remote with a Nunchuk on the same pad: A on A, B on RT, 1 and 2 on X
/// and Y, minus and plus on View and Menu, Home on the right stick press, shake on B, the pointer
/// on the right stick, the Nunchuk's stick on the left stick with C on LB and Z on LT.
pub fn dolphin_wiimote(pad: &Pad) -> String {
    let v: Vec<(&str, String)> = vec![
        ("Device", format!("SDL/0/{}", pad.sdl)),
        ("Source", "1".into()),
        ("Extension", "Nunchuk".into()),
        ("Buttons/A", "`Button S`".into()),
        ("Buttons/B", "`Trigger R`".into()),
        ("Buttons/1", "`Button W`".into()),
        ("Buttons/2", "`Button N`".into()),
        ("Buttons/-", "Back".into()),
        ("Buttons/+", "Start".into()),
        ("Buttons/Home", "`Thumb R`".into()),
        ("D-Pad/Up", "`Pad N`".into()),
        ("D-Pad/Down", "`Pad S`".into()),
        ("D-Pad/Left", "`Pad W`".into()),
        ("D-Pad/Right", "`Pad E`".into()),
        ("IR/Up", "`Right Y+`".into()),
        ("IR/Down", "`Right Y-`".into()),
        ("IR/Left", "`Right X-`".into()),
        ("IR/Right", "`Right X+`".into()),
        ("Shake/X", "`Button E`".into()),
        ("Shake/Y", "`Button E`".into()),
        ("Shake/Z", "`Button E`".into()),
        ("Rumble/Motor", "Motor".into()),
        ("Nunchuk/Buttons/C", "`Shoulder L`".into()),
        ("Nunchuk/Buttons/Z", "`Trigger L`".into()),
        ("Nunchuk/Stick/Up", "`Left Y+`".into()),
        ("Nunchuk/Stick/Down", "`Left Y-`".into()),
        ("Nunchuk/Stick/Left", "`Left X-`".into()),
        ("Nunchuk/Stick/Right", "`Left X+`".into()),
        ("Nunchuk/Shake/X", "`Shoulder R`".into()),
        ("Nunchuk/Shake/Y", "`Shoulder R`".into()),
        ("Nunchuk/Shake/Z", "`Shoulder R`".into()),
    ];
    lines(&v, " = ")
}

/// PPSSPP's [ControlMapping]: PSP buttons on the pad (device 10 is SDL's pad 0, codes are
/// PPSSPP's: buttons 188-197 for the face, shoulder, View and Menu buttons, 4000 + axis * 2 for an
/// axis), L and R on both the bumpers and the triggers, and the Rare menu's keys (keyboard 1).
pub fn ppsspp_mapping() -> String {
    let v: Vec<(&str, String)> = vec![
        ("Up", "10-19".into()),
        ("Down", "10-20".into()),
        ("Left", "10-21".into()),
        ("Right", "10-22".into()),
        ("Circle", "10-190".into()),
        ("Cross", "10-189".into()),
        ("Square", "10-191".into()),
        ("Triangle", "10-188".into()),
        ("Start", "10-197".into()),
        ("Select", "10-196".into()),
        ("L", "10-193,10-4008".into()),
        ("R", "10-192,10-4010".into()),
        ("An.Up", "10-4003".into()),
        ("An.Down", "10-4002".into()),
        ("An.Left", "10-4001".into()),
        ("An.Right", "10-4000".into()),
        // Rare's menu: F2 save, F4 load, Escape the pause screen, ` fast forward (toggle).
        ("Save State", "1-132".into()),
        ("Load State", "1-134".into()),
        ("Pause", "1-111".into()),
        ("SpeedToggle", "1-68".into()),
    ];
    lines(&v, " = ")
}

/// What actions.rs adds to PPSSPP's mapping for the Rare menu.
const PPSSPP_KEYS_ADDED: &str = "Save State = 1-132\nLoad State = 1-134\nSpeedToggle = 1-68\n";

/// PPSSPP 1.20's stock mapping (keyboard plus its SDL pad defaults, whose L and R are buttons the
/// SDL build never sends, so the PSP's shoulders did nothing, and whose LT opens the pause screen).
pub const PPSSPP_STOCK: &str = include_str!("ppsspp-stock.ini");

/// Azahar and Lime3DS (Citra's Qt INI): profile 1 on the pad by its full SDL GUID (with the CRC:
/// Rare's earlier profile had it cleared, which Azahar never matched). 3DS A/B/X/Y are where the
/// Nintendo layout has them (east = A), the Circle Pad on the left stick, the C-Stick on the right.
/// Buttons are SDL's joystick numbers for this pad (A 0, B 1, X 2, Y 3, LB 4, RB 5, View 6, Menu 7).
pub fn citra_controls(pad: &Pad) -> String {
    citra_controls_for(&pad.guid)
}

fn citra_controls_for(guid: &str) -> String {
    let mut s = String::new();
    let q = |s: &mut String, k: &str, v: &str| s.push_str(&format!("{k}\\default=false\n{k}={v}\n"));
    q(&mut s, "profile", "0");
    q(&mut s, "profiles\\size", "1");
    s.push_str("profiles\\1\\name=default\nprofiles\\1\\name\\default=false\n");
    let tail = format!("engine:sdl,guid:{guid},port:0");
    for (k, b) in [("a", 1), ("b", 0), ("x", 3), ("y", 2), ("l", 4), ("r", 5), ("select", 6), ("start", 7)] {
        q(&mut s, &format!("profiles\\1\\button_{k}"), &format!("\"button:{b},{tail}\""));
    }
    for (k, a) in [("zl", 2), ("zr", 5)] {
        q(&mut s, &format!("profiles\\1\\button_{k}"), &format!("\"axis:{a},direction:+,{tail},threshold:0.5\""));
    }
    for d in ["up", "down", "left", "right"] {
        q(&mut s, &format!("profiles\\1\\button_{d}"), &format!("\"direction:{d},{tail},hat:0\""));
    }
    q(&mut s, "profiles\\1\\button_home", "\"\"");
    q(&mut s, "profiles\\1\\circle_pad", &format!("\"axis_x:0,axis_y:1,deadzone:0.100000,{tail}\""));
    q(&mut s, "profiles\\1\\c_stick", &format!("\"axis_x:3,axis_y:4,deadzone:0.100000,{tail}\""));
    s
}

/// Citron (yuzu's Qt INI): player 1 is a Pro Controller on the pad by GUID, Nintendo layout,
/// ZL and ZR on the triggers, both sticks, rumble.
pub fn citron_controls(pad: &Pad) -> String {
    let mut s = String::new();
    let q = |s: &mut String, k: &str, v: &str| s.push_str(&format!("{k}\\default=false\n{k}={v}\n"));
    q(&mut s, "player_0_type", "0");
    q(&mut s, "player_0_connected", "true");
    q(&mut s, "player_0_vibration_enabled", "true");
    q(&mut s, "vibration_enabled", "true");
    let tail = format!("engine:sdl,guid:{},port:0,pad:0", pad.guid0);
    for (k, b) in [("a", 1), ("b", 0), ("x", 3), ("y", 2), ("l", 4), ("r", 5), ("minus", 6), ("plus", 7), ("lstick", 9), ("rstick", 10)] {
        q(&mut s, &format!("player_0_button_{k}"), &format!("\"{tail},button:{b}\""));
    }
    for (k, a) in [("zl", 2), ("zr", 5)] {
        q(&mut s, &format!("player_0_button_{k}"), &format!("\"{tail},axis:{a},threshold:0.5,direction:+\""));
    }
    for d in ["up", "down", "left", "right"] {
        q(&mut s, &format!("player_0_button_d{d}"), &format!("\"{tail},direction:{d},hat:0\""));
    }
    for k in ["home", "screenshot", "slleft", "srleft", "slright", "srright"] {
        q(&mut s, &format!("player_0_button_{k}"), "\"\"");
    }
    q(&mut s, "player_0_lstick", &format!("\"{tail},axis_x:0,axis_y:1,threshold:0.5,deadzone:0.100000,range:0.950000\""));
    q(&mut s, "player_0_rstick", &format!("\"{tail},axis_x:3,axis_y:4,threshold:0.5,deadzone:0.100000,range:0.950000\""));
    s
}

/// Cemu: the Wii U GamePad on the pad by SDL GUID (its SDLController ids are SDL's game
/// controller buttons; 38-47 are the stick directions, 42 and 43 the triggers), Nintendo layout.
pub fn cemu_controller(pad: &Pad) -> String {
    const MAP: &[(u32, u32)] = &[
        (1, 1), (2, 0), (3, 3), (4, 2), (5, 9), (6, 10), (7, 42), (8, 43), (9, 6), (10, 4), (11, 11), (12, 12), (13, 13), (14, 14),
        (15, 7), (16, 8), (17, 45), (18, 39), (19, 44), (20, 38), (21, 47), (22, 41), (23, 46), (24, 40),
    ];
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<emulated_controller>\n\t<type>Wii U GamePad</type>\n\t<controller>\n");
    s.push_str(&format!("\t\t<api>SDLController</api>\n\t\t<uuid>0_{}</uuid>\n\t\t<display_name>{}</display_name>\n\t\t<rumble>1</rumble>\n", pad.guid, pad.sdl));
    for t in ["axis", "rotation", "trigger"] {
        s.push_str(&format!("\t\t<{t}>\n\t\t\t<deadzone>0.15</deadzone>\n\t\t\t<range>1</range>\n\t\t</{t}>\n"));
    }
    s.push_str("\t\t<mappings>\n");
    for (m, b) in MAP {
        s.push_str(&format!("\t\t\t<entry>\n\t\t\t\t<mapping>{m}</mapping>\n\t\t\t\t<button>{b}</button>\n\t\t\t</entry>\n"));
    }
    s.push_str("\t\t</mappings>\n\t</controller>\n</emulated_controller>\n");
    s
}

/// RPCS3: player 1 on the pad through RPCS3's SDL handler (the Flatpak has no evdev handler, so
/// Rare's earlier evdev profile left player 1 with nothing), a DualShock 3 with both motors. The PS
/// button is not on the pad: RPCS3's default puts it on View + Menu, which is Rare's way out.
pub fn rpcs3_pad(pad: &Pad) -> String {
    format!(
        "\
Player 1 Input:
  Handler: SDL
  Device: {} 1
  Config:
    Left Stick Left: LS X-
    Left Stick Down: LS Y-
    Left Stick Right: LS X+
    Left Stick Up: LS Y+
    Right Stick Left: RS X-
    Right Stick Down: RS Y-
    Right Stick Right: RS X+
    Right Stick Up: RS Y+
    Start: Start
    Select: Back
    PS Button: \"\"
    Square: West
    Cross: South
    Circle: East
    Triangle: North
    Left: Left
    Down: Down
    Right: Right
    Up: Up
    R1: RB
    R2: RT
    R3: RS
    L1: LB
    L2: LT
    L3: LS
    Enable Large Vibration Motor: true
    Enable Small Vibration Motor: true
    Switch Vibration Motors: false
",
        pad.sdl
    )
}

/// Ryujinx: player 1 is a Pro Controller on the pad (SDL by GUID), Nintendo layout, rumble.
pub fn ryujinx_player1(pad: &Pad) -> String {
    let g = &pad.guid0;
    // Ryujinx's id is the GUID as .NET prints it: the first 4 bytes, then 2, 2, 2 and 6, the first three little endian.
    let b: Vec<&str> = (0..16).map(|i| &g[i * 2..i * 2 + 2]).collect();
    let id = format!("0-{}{}{}{}-{}{}-{}{}-{}{}-{}", b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10..].concat());
    let v = serde_json::json!({
        "left_joycon_stick": { "joystick": "Left", "invert_stick_x": false, "invert_stick_y": false, "rotate90_cw": false, "stick_button": "LeftStick" },
        "right_joycon_stick": { "joystick": "Right", "invert_stick_x": false, "invert_stick_y": false, "rotate90_cw": false, "stick_button": "RightStick" },
        "deadzone_left": 0.1,
        "deadzone_right": 0.1,
        "range_left": 1.0,
        "range_right": 1.0,
        "trigger_threshold": 0.5,
        "motion": { "motion_backend": "GamepadDriver", "sensitivity": 100, "gyro_deadzone": 1, "enable_motion": true },
        "rumble": { "strong_rumble": 1.0, "weak_rumble": 1.0, "enable_rumble": true },
        "left_joycon": { "button_minus": "Minus", "button_l": "LeftShoulder", "button_zl": "LeftTrigger", "button_sl": "Unbound", "button_sr": "Unbound", "dpad_up": "DpadUp", "dpad_down": "DpadDown", "dpad_left": "DpadLeft", "dpad_right": "DpadRight" },
        "right_joycon": { "button_plus": "Plus", "button_r": "RightShoulder", "button_zr": "RightTrigger", "button_sl": "Unbound", "button_sr": "Unbound", "button_x": "Y", "button_b": "A", "button_y": "X", "button_a": "B" },
        "version": 1,
        "backend": "GamepadSDL2",
        "id": id,
        "name": pad.sdl,
        "controller_type": "ProController",
        "player_index": "Player1"
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

/// The profile for one emulator, with its files in `env`'s folders (a Flatpak's or the native ones).
pub fn profile(emu: &str, env: &Env, pad: &Pad) -> Option<Profile> {
    let (name, pieces, legacy): (&'static str, Vec<Piece>, Vec<String>) = match emu {
        "duckstation" => {
            let f = env.data("duckstation/settings.ini");
            (
                "DuckStation",
                vec![Piece::section(f.clone(), "Pad1", duckstation_pad()), Piece::section(f, "Hotkeys", duckstation_hotkeys())],
                vec![crate::defaults::LEGACY_DUCKSTATION_PAD.into(), format!("{}{}", crate::defaults::LEGACY_DUCKSTATION_HOTKEYS, duckstation_hotkeys())],
            )
        }
        "pcsx2" => {
            let f = env.config("PCSX2/inis/PCSX2.ini");
            (
                "PCSX2",
                vec![Piece::section(f.clone(), "Pad1", pcsx2_pad()), Piece::section(f, "Hotkeys", pcsx2_hotkeys())],
                vec![crate::defaults::LEGACY_PCSX2_PAD.into(), format!("{}{}", crate::defaults::LEGACY_PCSX2_HOTKEYS, pcsx2_hotkeys())],
            )
        }
        "dolphin" => {
            let d = env.config("dolphin-emu");
            (
                "Dolphin",
                vec![
                    Piece::section(d.join("GCPadNew.ini"), "GCPad1", dolphin_gc(pad)),
                    Piece::section(d.join("WiimoteNew.ini"), "Wiimote1", dolphin_wiimote(pad)),
                    // The pad works while Rare's menu is not over the game (InputPlumber holds it then).
                    Piece::section(d.join("Dolphin.ini"), "Input", "BackgroundInput = True\n".into()),
                ],
                vec![],
            )
        }
        "ppsspp" => ("PPSSPP", vec![Piece::section(env.config("ppsspp/PSP/SYSTEM/controls.ini"), "ControlMapping", ppsspp_mapping())], vec![format!("{}{}", ppsspp_mapping(), PPSSPP_KEYS_ADDED)]),
        "melonds" => ("melonDS", vec![Piece::section(env.config("melonDS/melonDS.toml"), "Instance0.Joystick", melonds_joystick())], vec![]),
        "azahar" => ("Azahar", vec![Piece::section(env.config("azahar-emu/qt-config.ini"), "Controls", citra_controls(pad))], vec![citra_controls_for(&pad.guid0)]),
        "lime3ds" => ("Lime3DS", vec![Piece::section(env.config("lime3ds-emu/qt-config.ini"), "Controls", citra_controls(pad))], vec![citra_controls_for(&pad.guid0)]),
        "citron" => ("Citron", vec![Piece::section(env.config("citron/qt-config.ini"), "Controls", citron_controls(pad))], vec![]),
        "cemu" => (
            "Cemu",
            vec![Piece::file(env.config("Cemu/controllerProfiles/controller0.xml"), cemu_controller(pad))],
            vec![crate::defaults::legacy_cemu_pad(&pad.guid0)],
        ),
        "rpcs3" => (
            "RPCS3",
            vec![
                Piece::file(env.config("rpcs3/input_configs/global/Default.yml"), rpcs3_pad(pad)),
                Piece::file(env.config("rpcs3/input_configs/active_profiles.yml"), "Active Profiles:\n  global: Default\n".into()),
            ],
            vec![crate::defaults::legacy_rpcs3_pad(&pad.evdev), crate::defaults::legacy_rpcs3_pad("Microsoft X-Box 360 pad")],
        ),
        "ryujinx" => ("Ryujinx", vec![Piece { path: env.config("Ryujinx/Config.json"), kind: Kind::RyujinxPlayer1, body: ryujinx_player1(pad) }], vec![]),
        // xemu binds the first game controller to port 1 by itself and maps it as an Xbox pad;
        // with the raw pad hidden (launch_env) that is the SuiPlay's.
        _ => return None,
    };
    let known = match emu {
        "ppsspp" => Known { legacy, stock: vec![PPSSPP_STOCK.into(), format!("{PPSSPP_STOCK}{PPSSPP_KEYS_ADDED}")], pad_words: vec!["10-"], binds: None },
        // melonDS writes -1 for every key nobody bound.
        "melonds" => Known { legacy, binds: Some(|t| t.lines().filter_map(|l| l.split_once('=')).any(|(_, v)| v.trim() != "-1")), ..Default::default() },
        _ => Known { legacy, ..Default::default() },
    };
    Some(Profile { emu: by_static(emu)?, name, pieces, known })
}

/// melonDS: DS buttons on SDL joystick 0 (its joystick numbers: buttons as SDL numbers them,
/// 0x100 | hat direction for the d-pad, 0x10000 | axis << 24 | direction << 20 for an axis).
pub fn melonds_joystick() -> String {
    let hat = |d: u32| 0x100 | d;
    let v: Vec<(&str, String)> = vec![
        ("A", "1".into()),
        ("B", "0".into()),
        ("X", "3".into()),
        ("Y", "2".into()),
        ("L", "4".into()),
        ("R", "5".into()),
        ("Select", "6".into()),
        ("Start", "7".into()),
        ("Up", hat(1).to_string()),
        ("Right", hat(2).to_string()),
        ("Down", hat(4).to_string()),
        ("Left", hat(8).to_string()),
    ];
    lines(&v, " = ")
}

const IDS: &[&str] = &["duckstation", "pcsx2", "dolphin", "ppsspp", "melonds", "azahar", "lime3ds", "citron", "cemu", "rpcs3", "ryujinx"];

fn by_static(id: &str) -> Option<&'static str> {
    IDS.iter().copied().find(|i| *i == id)
}

/// The emulators with a profile.
pub fn emulators() -> &'static [&'static str] {
    IDS
}

/// The settings folders of an installed emulator: its Flatpak's, or the native ones.
pub fn env_for(emu: &str, home: &Path) -> Option<Env> {
    let base = Env { home: home.to_path_buf(), pad_guid: None, pad_name: None, xdg: None };
    if emu == "lime3ds" || emu == "citron" {
        let alt = crate::catalog::ALTERNATES.iter().find(|(_, e)| e.id == emu).map(|(_, e)| e)?;
        return Some(match crate::catalog::installed(alt, home)? {
            crate::catalog::Install::Flatpak(app) => base.flatpak(app),
            crate::catalog::Install::AppImage(_) => base,
        });
    }
    let e = crate::catalog::by_id(emu)?;
    Some(match crate::catalog::installed(e, home)? {
        crate::catalog::Install::Flatpak(app) => base.flatpak(app),
        crate::catalog::Install::AppImage(_) => base,
    })
}

// ---------------------------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------------------------

/// One emulator's controller settings as they are.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: String,
    pub name: String,
    /// The worst piece: Own when any part is the player's own setup.
    pub state: State,
    /// Files that hold the player's own setup.
    pub own: Vec<String>,
}

fn review_one(home: &Path, p: &Profile) -> Review {
    let states: Vec<(State, &Piece)> = p.pieces.iter().map(|x| (classify(home, x, &p.known), x)).collect();
    let own: Vec<String> = states.iter().filter(|(s, _)| *s == State::Own).map(|(_, x)| x.path.display().to_string()).collect();
    let state = if !own.is_empty() {
        State::Own
    } else if states.iter().all(|(s, _)| *s == State::Current) {
        State::Current
    } else if states.iter().any(|(s, _)| *s == State::Rare) {
        State::Rare
    } else if states.iter().any(|(s, _)| *s == State::Default) {
        State::Default
    } else {
        State::Missing
    };
    Review { id: p.emu.into(), name: p.name.into(), state, own }
}

/// Writes what Rare may write without asking for one emulator (in `env`'s folders). Returns the
/// files written; the player's own pad setups are left as they are.
pub fn ensure_in(emu: &str, env: &Env, pad: &Pad) -> Vec<String> {
    let Some(p) = profile(emu, env, pad) else { return vec![] };
    let mut written = Vec::new();
    for x in &p.pieces {
        if classify(&env.home, x, &p.known).free() {
            match put(&env.home, x, false) {
                Ok(()) => written.push(x.path.display().to_string()),
                Err(e) => eprintln!("[emu] {} controls: {}: {e}", p.name, x.path.display()),
            }
        }
    }
    written
}

/// `ensure_in` for an installed emulator, with this device's pad.
pub fn ensure(emu: &str, home: &Path) -> Vec<String> {
    match env_for(emu, home) {
        Some(env) => ensure_in(emu, &env, &Pad::here()),
        None => vec![],
    }
}

/// Every installed emulator's controller settings, for the setup page.
pub fn review(home: &Path) -> Vec<Review> {
    let pad = Pad::here();
    IDS.iter().filter_map(|id| Some(review_one(home, &profile(id, &env_for(id, home)?, &pad)?))).collect()
}

/// Replaces the player's own setup with Rare's for these emulators (after they said yes). The old
/// file stays next to it as `<name>.before-rare`.
pub fn replace(home: &Path, ids: &[String]) -> Vec<String> {
    let pad = Pad::here();
    ids.iter().filter_map(|id| Some(replace_in(id, &env_for(id, home)?, &pad))).flatten().collect()
}

/// `replace` for one emulator in `env`'s folders.
pub fn replace_in(emu: &str, env: &Env, pad: &Pad) -> Vec<String> {
    let Some(p) = profile(emu, env, pad) else { return vec![] };
    let mut written = Vec::new();
    for x in &p.pieces {
        let st = classify(&env.home, x, &p.known);
        if st == State::Current {
            continue;
        }
        match put(&env.home, x, st == State::Own) {
            Ok(()) => written.push(x.path.display().to_string()),
            Err(e) => eprintln!("[emu] {} controls: {e}", p.name),
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC: &str = "\
I: Bus=0003 Vendor=045e Product=028e Version=0200
N: Name=\"Microsoft X-Box 360 pad\"
S: Sysfs=/devices/pci0000:00/usb1/1-4/input/input6
H: Handlers=event5 js0

I: Bus=0003 Vendor=045e Product=0b00 Version=0001
N: Name=\"Microsoft X-Box One Elite 2 pad\"
S: Sysfs=/devices/virtual/input/input18
H: Handlers=kbd event10 js1
";

    fn env() -> (tempfile::TempDir, Env) {
        let t = tempfile::tempdir().unwrap();
        let e = Env { home: t.path().to_path_buf(), pad_guid: None, pad_name: None, xdg: None };
        (t, e)
    }

    #[test]
    fn guid_matches_sdl() {
        // What SDL 2.30 reports for InputPlumber's pad on the build machine (tools/emu/padprobe.c).
        let p = target(PROC).unwrap();
        assert_eq!(p.guid, "030010f85e040000000b000001000000");
        assert_eq!(p.guid0, "030000005e040000000b000001000000");
        assert_eq!(p.sdl, "Xbox One Elite 2 Controller");
        assert_eq!(p, Pad::suiplay());
        assert_eq!(sdl_guid(3, 0x045e, 0x028e, 0x0200, "Microsoft X-Box 360 pad").0, "030081b85e0400008e02000000020000");
        // The raw pad is hidden only where InputPlumber's pad is.
        let hide = vec![("SDL_GAMECONTROLLER_IGNORE_DEVICES".to_string(), "0x045e/0x028e,0x057e/0x2009".to_string())];
        assert_eq!(launch_env_in(PROC), hide);
        assert!(launch_env_in(PROC.split("\n\n").next().unwrap()).is_empty());
        // InputPlumber showing the Steam Deck pad (gyro in Steam games): the source is still hidden.
        let deck = PROC.replace("Bus=0003 Vendor=045e Product=0b00 Version=0001\nN: Name=\"Microsoft X-Box One Elite 2 pad\"\nS: Sysfs=/devices/virtual/input/input18", "Bus=0003 Vendor=28de Product=1205 Version=0111\nN: Name=\"Steam Deck\"\nS: Sysfs=/devices/virtual/misc/uhid/0003:28DE:1205.0007/input/input40");
        assert!(deck.contains("Steam Deck"));
        assert_eq!(launch_env_in(&deck), hide);
        // InputPlumber's xbox-360 target has the source's ids: nothing is hidden then but the other source.
        let x360 = PROC.replace("Vendor=045e Product=0b00 Version=0001\nN: Name=\"Microsoft X-Box One Elite 2 pad\"", "Vendor=045e Product=028e Version=0001\nN: Name=\"Microsoft X-Box 360 pad\"");
        assert_eq!(launch_env_in(&x360), vec![("SDL_GAMECONTROLLER_IGNORE_DEVICES".to_string(), "0x057e/0x2009".to_string())]);
        // A virtual copy of the source pad (the build machine's test pad makes one) is never the target.
        let rig = PROC.replace("/devices/pci0000:00/usb1/1-4/input/input6", "/devices/virtual/input/input30");
        assert_eq!(target(&rig), Some(Pad::suiplay()));
    }

    #[test]
    fn sections() {
        let t = "[A]\nx = 1\n\n[Pad1]\nUp = k\n\n[B]\ny = 2\n";
        assert_eq!(ini_section(t, "Pad1").as_deref(), Some("Up = k\n"));
        assert_eq!(ini_section(t, "C"), None);
        let r = ini_replace(t, "Pad1", "Up = SDL-0/DPadUp\nDown = SDL-0/DPadDown\n");
        assert_eq!(r, "[A]\nx = 1\n\n[Pad1]\nUp = SDL-0/DPadUp\nDown = SDL-0/DPadDown\n\n[B]\ny = 2\n");
        assert_eq!(ini_replace("[A]\nx = 1\n", "Pad1", "Up = k\n"), "[A]\nx = 1\n\n[Pad1]\nUp = k\n");
        assert_eq!(ini_replace("", "Pad1", "Up = k\n"), "[Pad1]\nUp = k\n");
    }

    #[test]
    fn writes_free_parts_and_never_the_players_own() {
        let (_t, env) = env();
        let pad = Pad::suiplay();
        let d = env.config("dolphin-emu");
        std::fs::create_dir_all(&d).unwrap();
        // Dolphin's own keyboard default (it writes one at its first start): replaced.
        std::fs::write(d.join("GCPadNew.ini"), "[GCPad1]\nDevice = XInput2/0/Virtual core pointer\nButtons/Start = `Return`\n[GCPad2]\nDevice = XInput2/0/Virtual core pointer\n").unwrap();
        // The player's own Wii Remote on the pad: kept.
        let own = "[Wiimote1]\nDevice = SDL/0/Xbox One Elite 2 Controller\nButtons/A = `Button E`\n";
        std::fs::write(d.join("WiimoteNew.ini"), own).unwrap();
        let w = ensure_in("dolphin", &env, &pad);
        assert_eq!(w.len(), 2, "{w:?}");
        let gc = std::fs::read_to_string(d.join("GCPadNew.ini")).unwrap();
        assert!(gc.contains("Buttons/Start = Start") && gc.contains("Device = SDL/0/Xbox One Elite 2 Controller") && gc.contains("[GCPad2]\nDevice = XInput2"));
        assert_eq!(std::fs::read_to_string(d.join("WiimoteNew.ini")).unwrap(), own);
        assert!(std::fs::read_to_string(d.join("Dolphin.ini")).unwrap().contains("[Input]\nBackgroundInput = True"));
        let r = review_one(&env.home, &profile("dolphin", &env, &pad).unwrap());
        assert_eq!(r.state, State::Own);
        assert_eq!(r.own.len(), 1);
        let p = profile("dolphin", &env, &pad).unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &Known::default()), State::Current);
    }

    #[test]
    fn players_change_is_asked_about_then_replaced_with_a_backup() {
        let (_t, env) = env();
        let pad = Pad::suiplay();
        let p = profile("pcsx2", &env, &pad).unwrap();
        assert_eq!(ensure_in("pcsx2", &env, &pad).len(), 2);
        let f = env.config("PCSX2/inis/PCSX2.ini");
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("Start = SDL-0/Start") && text.contains("OpenPauseMenu = Keyboard/F15"));
        // No pad chord is on Start or View (Select).
        assert!(!text.lines().any(|l| l.contains('&')));
        // Rare's profile changes later: Rare's unchanged file follows.
        let edited = text.replace("Deadzone = 0.000000", "Deadzone = 0.100000");
        std::fs::write(&f, &edited).unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &p.known), State::Own);
        assert!(ensure_in("pcsx2", &env, &pad).is_empty());
        assert!(std::fs::read_to_string(&f).unwrap().contains("Deadzone = 0.100000"));
        // The player says yes: Rare's again, the player's file kept beside it.
        assert_eq!(replace_in("pcsx2", &env, &pad).len(), 1);
        assert!(std::fs::read_to_string(&f).unwrap().contains("Deadzone = 0.000000"));
        assert!(std::fs::read_to_string(f.with_file_name("PCSX2.ini.before-rare")).unwrap().contains("Deadzone = 0.100000"));
        assert!(replace_in("pcsx2", &env, &pad).is_empty());
    }

    #[test]
    fn dolphins_own_setup_on_the_dead_pad_is_replaced() {
        // What the SuiPlay had: Dolphin's own setup on SDL/0, the grabbed source pad.
        let (_t, env) = env();
        let d = env.config("dolphin-emu");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("GCPadNew.ini"), "[GCPad1]\nDevice = SDL/0/Xbox 360 Controller\nButtons/A = `Button S`\nButtons/Start = Start\n[GCPad2]\nDevice = SDL/1/Steam Deck Controller\n").unwrap();
        let p = profile("dolphin", &env, &Pad::suiplay()).unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &p.known), State::Default);
        ensure_in("dolphin", &env, &Pad::suiplay());
        let t = std::fs::read_to_string(d.join("GCPadNew.ini")).unwrap();
        assert!(t.contains("[GCPad1]\nDevice = SDL/0/Xbox One Elite 2 Controller") && t.contains("Buttons/Start = Start"), "{t}");
    }

    #[test]
    fn earlier_rare_profiles_count_as_rares() {
        let (_t, env) = env();
        let pad = Pad::suiplay();
        let f = env.data("duckstation/settings.ini");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let old = format!("[Main]\nSettingsVersion = 3\n\n[Pad1]\n{}\n[Hotkeys]\n{}OpenPauseMenu = Keyboard/F15\n", crate::defaults::LEGACY_DUCKSTATION_PAD, crate::defaults::LEGACY_DUCKSTATION_HOTKEYS);
        std::fs::write(&f, old).unwrap();
        let p = profile("duckstation", &env, &pad).unwrap();
        // The pad section has not changed; the hotkeys had pad chords and are replaced.
        assert_eq!(classify(&env.home, &p.pieces[0], &p.known), State::Current);
        assert_eq!(classify(&env.home, &p.pieces[1], &p.known), State::Rare);
        ensure_in("duckstation", &env, &pad);
        let t = std::fs::read_to_string(&f).unwrap();
        assert!(t.contains("[Main]\nSettingsVersion = 3") && t.contains("ToggleFastForward = Keyboard/F16"));
        assert!(!t.contains("Back & SDL-0/RightShoulder"), "{t}");
    }

    #[test]
    fn melonds_gets_its_table() {
        let (_t, env) = env();
        ensure_in("melonds", &env, &Pad::suiplay());
        let t = std::fs::read_to_string(env.config("melonDS/melonDS.toml")).unwrap();
        assert!(t.starts_with("[Instance0]\nJoystickID = 0\n\n[Instance0.Joystick]\nA = 1\n"), "{t}");
        // melonDS's stock file (every key -1) is not the player's.
        let f = env.config("melonDS/melonDS.toml");
        std::fs::write(&f, "[Instance0]\nJoystickID = 0\n\n[Instance0.Joystick]\nA = -1\nB = -1\n").unwrap();
        let p = profile("melonds", &env, &Pad::suiplay()).unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &p.known), State::Default);
        std::fs::write(&f, "[Instance0]\nJoystickID = 0\n\n[Instance0.Joystick]\nA = 0\nB = -1\n").unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &p.known), State::Own);
    }

    #[test]
    fn ppsspp_stock_with_its_bom_is_replaced() {
        let (_t, env) = env();
        let f = env.config("ppsspp/PSP/SYSTEM/controls.ini");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, format!("\u{feff}[ControlMapping]\n{PPSSPP_STOCK}")).unwrap();
        let p = profile("ppsspp", &env, &Pad::suiplay()).unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &p.known), State::Default);
        ensure_in("ppsspp", &env, &Pad::suiplay());
        let t = std::fs::read_to_string(&f).unwrap();
        assert_eq!(t.matches("ControlMapping").count(), 1, "{t}");
        assert!(t.contains("L = 10-193,10-4008"));
    }

    #[test]
    fn ryujinx_entry_and_id() {
        let pad = Pad::suiplay();
        let e: serde_json::Value = serde_json::from_str(&ryujinx_player1(&pad)).unwrap();
        assert_eq!(e["id"], "0-00000003-045e-0000-000b-000001000000");
        assert_eq!(e["right_joycon"]["button_a"], "B");
        let (_t, env) = env();
        let f = env.config("Ryujinx/Config.json");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, r#"{"version": 70, "input_config": [{"backend": "WindowKeyboard", "player_index": "Player1", "id": "0"}], "x": 1}"#).unwrap();
        assert_eq!(ensure_in("ryujinx", &env, &pad).len(), 1);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["x"], 1);
        assert_eq!(v["input_config"].as_array().unwrap().len(), 1);
        assert_eq!(v["input_config"][0]["backend"], "GamepadSDL2");
        let p = profile("ryujinx", &env, &pad).unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &Known::default()), State::Current);
        // Ryujinx saves it again with keys of its own and 1 for 1.0: still Rare's.
        let saved = std::fs::read_to_string(&f).unwrap().replace("1.0", "1").replace("\"version\": 1,", "\"version\": 1, \"led\": null,");
        std::fs::write(&f, saved).unwrap();
        assert_eq!(classify(&env.home, &p.pieces[0], &Known::default()), State::Current);
        // An earlier Rare entry Ryujinx saved again is Rare's and follows Rare's profile.
        let older = std::fs::read_to_string(&f).unwrap().replace("\"Minus\"", "\"Back\"");
        std::fs::write(&f, older).unwrap();
        let mut m = manifest(&env.home);
        let body = p.pieces[0].body.replace("\"Minus\"", "\"Back\"");
        let old_piece = Piece { body, ..p.pieces[0].clone() };
        m.insert(p.pieces[0].key(), hash(&canon(&old_piece, &old_piece.body)));
        save_manifest(&env.home, &m);
        assert_eq!(classify(&env.home, &p.pieces[0], &Known::default()), State::Rare);
    }

    #[test]
    fn the_standard_map_holds_in_every_profile() {
        let pad = Pad::suiplay();
        // Menu is Start, View is Select (or plus and minus on Nintendo's newer systems).
        assert!(duckstation_pad().contains("Start = SDL-0/Start") && duckstation_pad().contains("Select = SDL-0/Back"));
        assert!(pcsx2_pad().contains("Start = SDL-0/Start") && pcsx2_pad().contains("Select = SDL-0/Back"));
        assert!(dolphin_gc(&pad).contains("Buttons/Start = Start"));
        assert!(dolphin_wiimote(&pad).contains("Buttons/+ = Start") && dolphin_wiimote(&pad).contains("Buttons/- = Back"));
        assert!(ppsspp_mapping().contains("Start = 10-197") && ppsspp_mapping().contains("Select = 10-196"));
        assert!(melonds_joystick().contains("Start = 7") && melonds_joystick().contains("Select = 6"));
        assert!(citra_controls(&pad).contains("button_start=\"button:7") && citra_controls(&pad).contains("button_select=\"button:6"));
        assert!(citron_controls(&pad).contains("player_0_button_plus=\"engine:sdl") && citron_controls(&pad).contains(",button:7\""));
        assert!(cemu_controller(&pad).contains("<mapping>9</mapping>\n\t\t\t\t<button>6</button>") && cemu_controller(&pad).contains("<mapping>10</mapping>\n\t\t\t\t<button>4</button>"));
        assert!(rpcs3_pad(&pad).contains("Start: Start") && rpcs3_pad(&pad).contains("Select: Back") && rpcs3_pad(&pad).contains("PS Button: \"\""));
        let r: serde_json::Value = serde_json::from_str(&ryujinx_player1(&pad)).unwrap();
        assert_eq!((r["right_joycon"]["button_plus"].as_str(), r["left_joycon"]["button_minus"].as_str()), (Some("Plus"), Some("Minus")));
        // Nintendo layouts by position: the console's A is the right button (SDL button 1).
        assert!(citra_controls(&pad).contains("button_a=\"button:1,") && citron_controls(&pad).contains("player_0_button_a=\"engine:sdl,guid:030000005e040000000b000001000000,port:0,pad:0,button:1\""));
        assert!(melonds_joystick().starts_with("A = 1\nB = 0\n"));
        // No hotkey anywhere on a pad button.
        assert!(!duckstation_hotkeys().contains("SDL") && !pcsx2_hotkeys().contains("SDL"));
        assert!(crate::retroarch::PAD_CFG.contains("input_menu_toggle_gamepad_combo = \"0\"") && crate::retroarch::PAD_CFG.contains("input_enable_hotkey_btn = \"nul\""));
        assert!(STANDARD.iter().any(|(b, _)| *b == "Menu") && CONSOLES.len() >= 14);
    }

    #[test]
    fn every_profile_builds_and_keeps_start_and_select_for_the_game() {
        let (_t, env) = env();
        let pad = Pad::suiplay();
        for id in emulators() {
            let p = profile(id, &env, &pad).unwrap_or_else(|| panic!("{id}"));
            for x in &p.pieces {
                assert!(!x.body.is_empty(), "{id}");
                // Hotkey chords never use the pad.
                assert!(!x.body.contains(" & SDL"), "{id}");
            }
        }
    }
}
