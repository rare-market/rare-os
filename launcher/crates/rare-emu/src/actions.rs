//! Emulator rows for the Rare menu: Save state, Load state and the emulator's own menu.
//!
//! `emu_actions(app_id)` lists the rows an emulator can really do, `emu_do(action)` does one. Each row maps
//! to the emulator's own mechanism, never to a guess:
//!
//! | Emulator    | Save state              | Load state              | Fast forward (toggle)  | Emulator menu                   |
//! |-------------|-------------------------|-------------------------|------------------------|---------------------------------|
//! | DuckStation | F13 SaveSelectedSaveState | F14 LoadSelectedSaveState | F16 ToggleFastForward | F15 OpenPauseMenu             |
//! | PCSX2       | PINE MsgSaveState, slot 1 | PINE MsgLoadState, slot 1 | F16 ToggleTurbo     | F15 OpenPauseMenu               |
//! | PPSSPP      | F2 Save State           | F4 Load State           | ` SpeedToggle          | Esc Pause                       |
//! | Dolphin     | Shift+F1 (slot 1, its default) | F1 (slot 1)      | Tab (speed limit off)  | none (its menus need a pointer) |
//! | Azahar      | Ctrl+C (oldest slot)    | Ctrl+V (newest slot)    | none                   | none (menu bar needs a pointer) |
//! | RPCS3, Cemu, Citron | none: RPCS3's savestate needs a menu click and loads only by booting, Cemu and the Switch have none | | | none |
//!
//! None of these is on the pad: controls.rs keeps every pad button for the game.
//!
//! Hotkeys are keyboard keys sent from a virtual keyboard of Rare's own on /dev/uinput (the same device kind
//! apps/pointer.rs uses). A pad chord cannot be sent that way: emulators bind pads by SDL index, and a second
//! virtual pad would be SDL-1, not the SuiPlay pad SDL-0 the chords are bound to. So `ensure_bindings`
//! adds one extra keyboard binding (F13, F14, F15: keys no real keyboard has) next to the pad chords
//! `defaults.rs` writes, before the emulator starts. It only ever adds a missing line; nothing existing changes.
//! PCSX2's PINE is a unix socket and needs no focus at all.

use crate::defaults::Env;
use rare_core::{Ctx, Error};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const KEY_ESC: u16 = 1;
const KEY_TAB: u16 = 15;
const KEY_LEFTCTRL: u16 = 29;
const KEY_GRAVE: u16 = 41;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_C: u16 = 46;
const KEY_V: u16 = 47;
const KEY_F1: u16 = 59;
const KEY_F2: u16 = 60;
const KEY_F4: u16 = 62;
const KEY_F13: u16 = 183;
const KEY_F14: u16 = 184;
const KEY_F15: u16 = 185;
const KEY_F16: u16 = 186;
/// Every key the virtual keyboard can send.
const KEYS: &[u16] = &[KEY_ESC, KEY_TAB, KEY_LEFTCTRL, KEY_GRAVE, KEY_LEFTSHIFT, KEY_C, KEY_V, KEY_F1, KEY_F2, KEY_F4, KEY_F13, KEY_F14, KEY_F15, KEY_F16];

/// PINE: opcodes and the default slot (pcsx2/PINE.cpp, IPCCommand).
const PINE_SAVE_STATE: u8 = 9;
const PINE_LOAD_STATE: u8 = 0x0A;
const PINE_STATE_SLOT: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mech {
    /// A chord on the virtual keyboard (keys pressed in order, released in reverse).
    Keys(&'static [u16]),
    /// A PINE message to PCSX2.
    Pine(u8),
    /// A RetroArch network command (ROM games Rare starts in RetroArch).
    Ra(RaCmd),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaCmd {
    Save(u32),
    Load(u32),
    FastForward,
    Rewind,
    Disc,
    Menu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Save,
    Load,
    FastForward,
    Menu,
}

impl Row {
    const ALL: [Row; 4] = [Row::Save, Row::Load, Row::FastForward, Row::Menu];
    fn id(self) -> &'static str {
        match self {
            Row::Save => "save",
            Row::Load => "load",
            Row::FastForward => "ff",
            Row::Menu => "menu",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Row::Save => "Save state",
            Row::Load => "Load state",
            Row::FastForward => "Fast forward",
            Row::Menu => "Emulator menu",
        }
    }
    fn parse(id: &str) -> Option<Row> {
        Row::ALL.into_iter().find(|r| r.id() == id)
    }
}

/// The emulators with rows. `emu::identify` knows the first four; PPSSPP is only known here (it is not
/// updated by Rare, so it has no entry in `EMULATORS`).
fn emulator_of(program: Option<&str>, title: &str) -> Option<&'static str> {
    let by = |s: &str| {
        let n: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
        if n.contains("ppsspp") {
            return Some("ppsspp");
        }
        if n.contains("dolphin") {
            return Some("dolphin");
        }
        crate::identify(s).map(|e| e.id)
    };
    program.and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()).and_then(by)).or_else(|| by(title))
}

fn mech(emu: &str, row: Row) -> Option<Mech> {
    Some(match (emu, row) {
        ("duckstation", Row::Save) => Mech::Keys(&[KEY_F13]),
        ("duckstation", Row::Load) => Mech::Keys(&[KEY_F14]),
        ("duckstation", Row::FastForward) => Mech::Keys(&[KEY_F16]),
        ("duckstation", Row::Menu) => Mech::Keys(&[KEY_F15]),
        ("pcsx2", Row::Save) => Mech::Pine(PINE_SAVE_STATE),
        ("pcsx2", Row::Load) => Mech::Pine(PINE_LOAD_STATE),
        ("pcsx2", Row::FastForward) => Mech::Keys(&[KEY_F16]),
        ("pcsx2", Row::Menu) => Mech::Keys(&[KEY_F15]),
        ("ppsspp", Row::Save) => Mech::Keys(&[KEY_F2]),
        ("ppsspp", Row::Load) => Mech::Keys(&[KEY_F4]),
        ("ppsspp", Row::FastForward) => Mech::Keys(&[KEY_GRAVE]),
        ("ppsspp", Row::Menu) => Mech::Keys(&[KEY_ESC]),
        ("dolphin", Row::Save) => Mech::Keys(&[KEY_LEFTSHIFT, KEY_F1]),
        ("dolphin", Row::Load) => Mech::Keys(&[KEY_F1]),
        ("dolphin", Row::FastForward) => Mech::Keys(&[KEY_TAB]),
        ("azahar", Row::Save) => Mech::Keys(&[KEY_LEFTCTRL, KEY_C]),
        ("azahar", Row::Load) => Mech::Keys(&[KEY_LEFTCTRL, KEY_V]),
        // RPCS3, Cemu, Citron: see the module table.
        _ => return None,
    })
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmuAction {
    /// The library id of the running game.
    pub app_id: String,
    /// "save", "load" or "menu".
    pub id: String,
    pub label: String,
    /// Emulator id ("duckstation", "pcsx2", "ppsspp", "azahar").
    pub emulator: String,
    /// False when the emulator's config lacks the binding this row needs (the reason is in `note`).
    pub ready: bool,
    pub note: Option<String>,
    /// A state slot's picture (RetroArch's thumbnail next to the state file).
    #[serde(default)]
    pub thumb: Option<String>,
    /// Unix seconds a state slot was saved.
    #[serde(default)]
    pub saved: Option<i64>,
}

// ---------------------------------------------------------------------------------------------
// Config: one added binding per row
// ---------------------------------------------------------------------------------------------

/// Where the emulator keeps its settings file and what each row needs in it: (section, key, value, multi).
/// `multi` bindings sit next to the chord already there (DuckStation and PCSX2 take several lines per
/// hotkey); the others are single settings set to the value.
struct Need {
    file: PathBuf,
    lines: Vec<(Row, &'static str, &'static str, &'static str, bool)>,
    /// Created only when the section is already in the file (a partial file would unbind everything).
    needs_section: Option<&'static str>,
}

fn need(env: &Env, emu: &str) -> Option<Need> {
    Some(match emu {
        "duckstation" => Need {
            file: env.data("duckstation/settings.ini"),
            lines: vec![
                (Row::Save, "Hotkeys", "SaveSelectedSaveState", "Keyboard/F13", true),
                (Row::Load, "Hotkeys", "LoadSelectedSaveState", "Keyboard/F14", true),
                (Row::FastForward, "Hotkeys", "ToggleFastForward", "Keyboard/F16", true),
                (Row::Menu, "Hotkeys", "OpenPauseMenu", "Keyboard/F15", true),
            ],
            needs_section: None,
        },
        "pcsx2" => Need {
            file: env.config("PCSX2/inis/PCSX2.ini"),
            lines: vec![
                (Row::Save, "EmuCore", "EnablePINE", "true", false),
                (Row::Load, "EmuCore", "EnablePINE", "true", false),
                (Row::FastForward, "Hotkeys", "ToggleTurbo", "Keyboard/F16", true),
                (Row::Menu, "Hotkeys", "OpenPauseMenu", "Keyboard/F15", true),
            ],
            needs_section: None,
        },
        // PPSSPP numbers keys as Android does: F2 = 132, F4 = 134, on the keyboard device (1).
        "ppsspp" => Need {
            file: env.config("ppsspp/PSP/SYSTEM/controls.ini"),
            lines: vec![
                (Row::Save, "ControlMapping", "Save State", "1-132", false),
                (Row::Load, "ControlMapping", "Load State", "1-134", false),
                (Row::FastForward, "ControlMapping", "SpeedToggle", "1-68", false),
            ],
            needs_section: Some("ControlMapping"),
        },
        _ => return None,
    })
}

fn section_name(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[').and_then(|r| r.strip_suffix(']'))
}

fn kv(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    Some((k.trim(), v.trim()))
}

/// True when `section` holds `key = value` (any of its lines for a multi key).
pub fn has_line(text: &str, section: &str, key: &str, value: &str) -> bool {
    let mut inside = false;
    for l in text.lines() {
        if let Some(s) = section_name(l) {
            inside = s == section;
        } else if inside && kv(l) == Some((key, value)) {
            return true;
        }
    }
    false
}

fn has_section(text: &str, section: &str) -> bool {
    text.lines().any(|l| section_name(l) == Some(section))
}

/// `text` with `key = value` in `section`. A multi key gets one more line next to the others; a single key
/// is replaced in place, or added. Missing sections are appended. Idempotent.
pub fn upsert(text: &str, section: &str, key: &str, value: &str, multi: bool) -> String {
    if has_line(text, section, key, value) && (multi || text.lines().filter(|l| kv(l).is_some_and(|(k, _)| k == key)).count() == 1) {
        return text.to_string();
    }
    let new_line = format!("{key} = {value}");
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    let mut found_section = false;
    let mut done = false;
    let mut last_in_section = None; // index in `out` after the section's last non-empty line
    for l in &lines {
        if let Some(s) = section_name(l) {
            if inside && !done {
                // Leaving the section without having placed it.
                let at = last_in_section.unwrap_or(out.len());
                out.insert(at, new_line.clone());
                done = true;
            }
            inside = s == section;
            found_section |= inside;
            out.push(l.to_string());
            if inside {
                last_in_section = Some(out.len());
            }
            continue;
        }
        if inside {
            if !multi && !done && kv(l).is_some_and(|(k, _)| k == key) {
                out.push(new_line.clone());
                done = true;
                last_in_section = Some(out.len());
                continue;
            }
            if !multi && done && kv(l).is_some_and(|(k, _)| k == key) {
                continue; // drop a duplicate of a single key
            }
            out.push(l.to_string());
            if !l.trim().is_empty() {
                last_in_section = Some(out.len());
            }
        } else {
            out.push(l.to_string());
        }
    }
    if inside && !done {
        let at = last_in_section.unwrap_or(out.len());
        out.insert(at, new_line.clone());
        done = true;
    }
    if !found_section {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(format!("[{section}]"));
        out.push(new_line);
    } else if !done {
        out.push(new_line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

fn ready(env: &Env, emu: &str, row: Row) -> (bool, Option<String>) {
    let Some(n) = need(env, emu) else { return (true, None) };
    let Some(&(_, section, key, value, _)) = n.lines.iter().find(|l| l.0 == row) else { return (true, None) };
    let Ok(text) = std::fs::read_to_string(&n.file) else {
        return (false, Some("The emulator has no settings file yet. Start it once from Rare first.".into()));
    };
    if has_line(&text, section, key, value) {
        return (true, None);
    }
    let note = match emu {
        "ppsspp" => format!("Bind {key} to {} once in PPSSPP, Settings, Controls.", match row {
            Row::Save => "F2",
            Row::Load => "F4",
            _ => "`",
        }),
        _ => "Rare adds this binding the next time it starts the game.".into(),
    };
    (false, Some(note))
}

/// Adds the missing bindings to the emulator's own settings, before it starts (it rewrites its file on exit).
/// Returns the file when it changed.
pub fn ensure_bindings(env: &Env, emu: &str) -> std::io::Result<Option<PathBuf>> {
    let Some(n) = need(env, emu) else { return Ok(None) };
    let Ok(old) = std::fs::read_to_string(&n.file) else { return Ok(None) };
    if n.needs_section.is_some_and(|s| !has_section(&old, s)) {
        return Ok(None);
    }
    let mut text = old.clone();
    for &(_, section, key, value, multi) in &n.lines {
        // A single setting the user already set to something else is left alone, except PINE (off is useless here).
        if !multi && key != "EnablePINE" && text.lines().any(|l| kv(l).is_some_and(|(k, _)| k == key)) {
            continue;
        }
        text = upsert(&text, section, key, value, multi);
    }
    if text == old {
        return Ok(None);
    }
    let tmp = n.file.with_extension("rare-tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &n.file)?;
    Ok(Some(n.file))
}

/// Called by `prepare_launch` for any program or title; does nothing unless it is one of the emulators here.
pub fn prepare(program: Option<&str>, title: &str) {
    let Some(emu) = emulator_of(program, title) else { return };
    match ensure_bindings(&Env::real(), emu) {
        Ok(Some(f)) => eprintln!("[emu] {emu}: added Rare's hotkey bindings to {}", f.display()),
        Ok(None) => {}
        Err(e) => eprintln!("[emu] {emu}: hotkey bindings: {e}"),
    }
}

// ---------------------------------------------------------------------------------------------
// The rows
// ---------------------------------------------------------------------------------------------

/// The rows for one game, in menu order: Save state, Load state, Emulator menu. Empty for anything that
/// is not one of the emulators above.
pub fn list(env: &Env, app_id: &str, program: Option<&str>, title: &str) -> Vec<EmuAction> {
    let Some(emu) = emulator_of(program, title) else { return vec![] };
    Row::ALL
        .into_iter()
        .filter(|r| mech(emu, *r).is_some())
        .map(|r| {
            let (ready, note) = ready(env, emu, r);
            EmuAction { app_id: app_id.into(), id: r.id().into(), label: r.label().into(), emulator: emu.into(), ready, note, thumb: None, saved: None }
        })
        .collect()
}

pub fn mechanism(action: &EmuAction) -> Option<Mech> {
    if action.emulator == "retroarch" {
        return ra_cmd(&action.id).map(Mech::Ra);
    }
    mech(&action.emulator, Row::parse(&action.id)?)
}

fn ra_cmd(id: &str) -> Option<RaCmd> {
    Some(match id {
        "ff" => RaCmd::FastForward,
        "rewind" => RaCmd::Rewind,
        "disc" => RaCmd::Disc,
        "menu" => RaCmd::Menu,
        _ => match id.split_once(':')? {
            ("save", n) => RaCmd::Save(n.parse().ok()?),
            ("load", n) => RaCmd::Load(n.parse().ok()?),
            _ => return None,
        },
    })
}

/// The Rare menu's rows for a ROM game Rare started (`emu:` ids): RetroArch's states with their
/// pictures, fast forward, rewind, the next disc and its menu; a standalone emulator's rows by
/// its Flatpak's or AppImage's settings.
pub fn rom_rows(env: &Env, app_id: &str) -> Vec<EmuAction> {
    let Some(rom) = crate::roms::find(app_id) else { return vec![] };
    let (emu, inst) = crate::roms::runner_of(rom.system, &env.home);
    if emu.id != "retroarch" {
        let env = match inst {
            Some(crate::catalog::Install::Flatpak(app)) => env.flatpak(app),
            _ => env.clone(),
        };
        return list(&env, app_id, None, emu.name);
    }
    let row = |id: String, label: &str| EmuAction { app_id: app_id.into(), id, label: label.into(), emulator: "retroarch".into(), ready: true, note: None, thumb: None, saved: None };
    let p = crate::retroarch::paths(&env.home);
    let slots = crate::retroarch::slots(&p.states, &rom.path);
    let next = crate::retroarch::next_slot(&slots);
    // Hardcore retro trophies: RetroArch loads no states and doesn't rewind in a game with achievements.
    let hardcore = crate::retro::sign().is_some_and(|s| s.hardcore) && crate::retro::known_game(&rom.id).is_some();
    let mut v = vec![row(format!("save:{next}"), "Save state")];
    for sl in slots.iter().filter(|s| s.saved.is_some()) {
        let r = EmuAction { thumb: sl.thumb.clone(), saved: sl.saved, ..row(format!("load:{}", sl.slot), &format!("Slot {}", sl.slot)) };
        v.push(if hardcore { EmuAction { ready: false, note: Some("Off in hardcore".into()), ..r } } else { r });
    }
    v.push(row("ff".into(), "Fast forward"));
    if rom.system.rewind && !hardcore {
        v.push(row("rewind".into(), "Rewind 5 seconds"));
    }
    if rom.discs > 1 {
        v.push(row("disc".into(), "Next disc"));
    }
    v.push(row("menu".into(), "Emulator settings"));
    v
}

/// The PINE request for a state message: u32 length (counting itself), opcode, slot.
pub fn pine_frame(op: u8, slot: u8) -> Vec<u8> {
    let mut v = 6u32.to_le_bytes().to_vec();
    v.push(op);
    v.push(slot);
    v
}

/// Reads a PINE reply: u32 length then a status byte, 0 = ok.
pub fn pine_ok(reply: &[u8]) -> bool {
    reply.len() >= 5 && reply[4] == 0
}

fn pine_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join("pcsx2.sock")
}

#[cfg(unix)]
fn pine_send(path: &Path, op: u8, slot: u8) -> Result<(), String> {
    use std::io::{Read, Write};
    let mut s = std::os::unix::net::UnixStream::connect(path).map_err(|e| format!("PCSX2's PINE socket is not there ({e}). Is PINE on in PCSX2, Settings, Advanced?"))?;
    let t = Some(Duration::from_secs(3));
    let _ = s.set_read_timeout(t);
    let _ = s.set_write_timeout(t);
    s.write_all(&pine_frame(op, slot)).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 5];
    s.read_exact(&mut buf).map_err(|e| e.to_string())?;
    if pine_ok(&buf) {
        Ok(())
    } else {
        Err("PCSX2 refused the state request".into())
    }
}

#[cfg(not(unix))]
fn pine_send(_path: &Path, _op: u8, _slot: u8) -> Result<(), String> {
    Err("PINE needs Linux".into())
}

static KEYBOARD: Mutex<Option<rare_launch::apps::pointer::uinput::Device>> = Mutex::new(None);

/// Creates the virtual keyboard now so it is known to the compositor before the first press.
pub fn warm_up() {
    let mut k = KEYBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if k.is_none() {
        match rare_launch::apps::pointer::uinput::Device::open_keys("Rare emulator hotkeys", KEYS) {
            Ok(d) => *k = Some(d),
            Err(e) => eprintln!("[emu] no virtual keyboard ({e}); is /dev/uinput writable?"),
        }
    }
}

fn press(keys: &[u16]) -> Result<(), String> {
    warm_up();
    let mut k = KEYBOARD.lock().unwrap_or_else(|e| e.into_inner());
    let d = k.as_mut().ok_or("No virtual keyboard: /dev/uinput is not writable")?;
    for key in keys {
        d.key(*key, true);
        d.syn();
        std::thread::sleep(Duration::from_millis(15));
    }
    std::thread::sleep(Duration::from_millis(70));
    for key in keys.iter().rev() {
        d.key(*key, false);
        d.syn();
        std::thread::sleep(Duration::from_millis(15));
    }
    Ok(())
}

/// Does one row. Keyboard rows need the game in front, so the overlay closes first (the game gets focus back).
pub fn run(ctx: &Ctx, action: &EmuAction) -> Result<(), Error> {
    let m = mechanism(action).ok_or_else(|| Error::new("That emulator has no such action"))?;
    match m {
        Mech::Pine(op) => Ok(pine_send(&pine_path(), op, PINE_STATE_SLOT)?),
        Mech::Ra(c) => {
            // RetroArch takes commands while it runs: the game comes back first (a state's
            // picture is the game, not the menu).
            if ctx.host.overlay_is_open() {
                ctx.host.overlay_close()?;
                std::thread::sleep(Duration::from_millis(400));
            }
            use crate::retroarch as r;
            Ok(match c {
                RaCmd::Save(n) => r::save_slot(n)?,
                RaCmd::Load(n) => r::load_slot(n)?,
                RaCmd::FastForward => r::send("FAST_FORWARD")?,
                RaCmd::Rewind => r::rewind(5.0)?,
                RaCmd::Disc => r::swap_disc()?,
                RaCmd::Menu => r::send("MENU_TOGGLE")?,
            })
        }
        Mech::Keys(keys) => {
            if ctx.host.overlay_is_open() {
                ctx.host.overlay_close()?;
                // Focus goes back to the game window; a key sent before that would reach nothing.
                std::thread::sleep(Duration::from_millis(400));
            }
            Ok(press(keys)?)
        }
    }
}

/// The rows for a game in the library (`program` is the game's executable when it is a program
/// on disk). Warms the keyboard when any row needs it.
pub fn emu_actions(app_id: &str, program: Option<&str>, title: &str) -> Vec<EmuAction> {
    let rows = if app_id.starts_with("emu:") { rom_rows(&Env::real(), app_id) } else { list(&Env::real(), app_id, program, title) };
    if rows.iter().any(|r| matches!(mechanism(r), Some(Mech::Keys(_)))) {
        std::thread::spawn(warm_up);
    }
    rows
}

/// Does one row for a running game (the caller has checked the game is running).
pub fn emu_do(ctx: &Ctx, action: &EmuAction) -> Result<(), Error> {
    run(ctx, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> (tempfile::TempDir, Env) {
        let t = tempfile::tempdir().unwrap();
        let env = Env { home: t.path().to_path_buf(), pad_guid: None, pad_name: None, xdg: None };
        (t, env)
    }

    #[test]
    fn rows_per_emulator() {
        let ids = |p: &str| -> Vec<String> { list(&env().1, "x", Some(p), "t").into_iter().map(|a| a.id).collect() };
        assert_eq!(ids("/h/Applications/DuckStation-x64.AppImage"), ["save", "load", "ff", "menu"]);
        assert_eq!(ids("/h/Applications/pcsx2-v2.AppImage"), ["save", "load", "ff", "menu"]);
        assert_eq!(ids("/h/Applications/PPSSPP.AppImage"), ["save", "load", "ff", "menu"]);
        assert_eq!(ids("/h/dolphin-emu"), ["save", "load", "ff"]);
        assert_eq!(ids("/h/Applications/azahar.AppImage"), ["save", "load"]);
        for none in ["rpcs3.AppImage", "Cemu.AppImage", "Citron-0.12.AppImage", "shadps4-qt.AppImage", "ES-DE.AppImage", "hades"] {
            assert!(ids(&format!("/h/{none}")).is_empty(), "{none}");
        }
        // A Steam shortcut knows only its title.
        assert_eq!(list(&env().1, "x", None, "DuckStation").len(), 4);
    }

    #[test]
    fn pine_bytes() {
        assert_eq!(pine_frame(PINE_SAVE_STATE, 1), [6, 0, 0, 0, 9, 1]);
        assert_eq!(pine_frame(PINE_LOAD_STATE, 1), [6, 0, 0, 0, 0x0A, 1]);
        assert!(pine_ok(&[5, 0, 0, 0, 0]));
        assert!(!pine_ok(&[5, 0, 0, 0, 0xFF]));
        assert!(!pine_ok(&[5, 0]));
    }

    #[cfg(unix)]
    #[test]
    fn pine_talks_to_a_socket() {
        use std::io::{Read, Write};
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("pcsx2.sock");
        let l = std::os::unix::net::UnixListener::bind(&p).unwrap();
        let h = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut b = [0u8; 6];
            c.read_exact(&mut b).unwrap();
            c.write_all(&[5, 0, 0, 0, 0]).unwrap();
            b
        });
        pine_send(&p, PINE_LOAD_STATE, 1).unwrap();
        assert_eq!(h.join().unwrap(), [6, 0, 0, 0, 0x0A, 1]);
        assert!(pine_send(&t.path().join("none.sock"), 9, 1).is_err());
    }

    #[test]
    fn upsert_adds_beside_the_chord_once() {
        let ini = "[Main]\nA = 1\n\n[Hotkeys]\nSaveSelectedSaveState = SDL-0/Back & SDL-0/RightShoulder\nFastForward = x\n\n[Other]\nB = 2\n";
        let a = upsert(ini, "Hotkeys", "SaveSelectedSaveState", "Keyboard/F13", true);
        assert!(a.contains("FastForward = x\nSaveSelectedSaveState = Keyboard/F13\n\n[Other]"), "{a}");
        assert!(a.contains("SDL-0/RightShoulder"));
        assert_eq!(upsert(&a, "Hotkeys", "SaveSelectedSaveState", "Keyboard/F13", true), a);
        // A missing section is appended.
        let b = upsert("[Main]\nA = 1\n", "Hotkeys", "OpenPauseMenu", "Keyboard/F15", true);
        assert_eq!(b, "[Main]\nA = 1\n\n[Hotkeys]\nOpenPauseMenu = Keyboard/F15\n");
        // A single key is replaced in place.
        let c = upsert("[EmuCore]\nEnablePINE = false\nX = 1\n", "EmuCore", "EnablePINE", "true", false);
        assert_eq!(c, "[EmuCore]\nEnablePINE = true\nX = 1\n");
        assert_eq!(upsert(&c, "EmuCore", "EnablePINE", "true", false), c);
        // Added at the end of the last section.
        let d = upsert("[EmuCore]\nX = 1\n", "EmuCore", "EnablePINE", "true", false);
        assert_eq!(d, "[EmuCore]\nX = 1\nEnablePINE = true\n");
    }

    #[test]
    fn duckstation_and_pcsx2_get_bindings_before_launch() {
        let (_t, env) = env();
        // No settings file: nothing is created (a partial file would hide the tuned defaults).
        assert_eq!(ensure_bindings(&env, "duckstation").unwrap(), None);
        let f = env.home.join(".local/share/duckstation/settings.ini");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "[Hotkeys]\nSaveSelectedSaveState = SDL-0/Back & SDL-0/RightShoulder\n").unwrap();
        let rows = list(&env, "g", Some("DuckStation.AppImage"), "t");
        assert!(rows.iter().all(|r| !r.ready));
        assert!(ensure_bindings(&env, "duckstation").unwrap().is_some());
        let s = std::fs::read_to_string(&f).unwrap();
        assert!(s.contains("SDL-0/RightShoulder") && s.contains("LoadSelectedSaveState = Keyboard/F14") && s.contains("OpenPauseMenu = Keyboard/F15") && s.contains("ToggleFastForward = Keyboard/F16"));
        assert!(list(&env, "g", Some("DuckStation.AppImage"), "t").iter().all(|r| r.ready));
        assert_eq!(ensure_bindings(&env, "duckstation").unwrap(), None);

        let p = env.home.join(".config/PCSX2/inis/PCSX2.ini");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "[EmuCore]\nEnableCheats = false\n[Hotkeys]\nSaveStateToSlot = x\n").unwrap();
        ensure_bindings(&env, "pcsx2").unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("EnablePINE = true") && s.contains("OpenPauseMenu = Keyboard/F15") && s.contains("SaveStateToSlot = x"));
        assert!(list(&env, "g", Some("pcsx2.AppImage"), "t").iter().all(|r| r.ready));
    }

    #[test]
    fn ppsspp_never_gets_a_partial_controls_file() {
        let (_t, env) = env();
        let f = env.home.join(".config/ppsspp/PSP/SYSTEM/controls.ini");
        assert_eq!(ensure_bindings(&env, "ppsspp").unwrap(), None);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "[Other]\nA = 1\n").unwrap();
        assert_eq!(ensure_bindings(&env, "ppsspp").unwrap(), None);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[Other]\nA = 1\n");
        std::fs::write(&f, "[ControlMapping]\nCross = 1-45\nSave State = 1-70\n").unwrap();
        ensure_bindings(&env, "ppsspp").unwrap();
        let s = std::fs::read_to_string(&f).unwrap();
        // The user's own Save State binding stays; Load State is added.
        assert!(s.contains("Save State = 1-70") && !s.contains("Save State = 1-132") && s.contains("Load State = 1-134"));
        let rows = list(&env, "g", Some("PPSSPP.AppImage"), "t");
        assert!(!rows[0].ready && rows[0].note.as_deref().unwrap().contains("F2") && rows[1].ready && rows[2].ready && rows[3].ready);
    }

    #[test]
    fn mechanisms() {
        let a = |emu: &str, id: &str| EmuAction { app_id: "x".into(), id: id.into(), label: String::new(), emulator: emu.into(), ready: true, note: None, thumb: None, saved: None };
        assert_eq!(mechanism(&a("retroarch", "save:3")), Some(Mech::Ra(RaCmd::Save(3))));
        assert_eq!(mechanism(&a("retroarch", "load:1")), Some(Mech::Ra(RaCmd::Load(1))));
        assert_eq!(mechanism(&a("retroarch", "rewind")), Some(Mech::Ra(RaCmd::Rewind)));
        assert_eq!(mechanism(&a("retroarch", "load:x")), None);
        assert_eq!(mechanism(&a("pcsx2", "save")), Some(Mech::Pine(9)));
        assert_eq!(mechanism(&a("azahar", "load")), Some(Mech::Keys(&[KEY_LEFTCTRL, KEY_V])));
        assert_eq!(mechanism(&a("rpcs3", "save")), None);
        assert_eq!(mechanism(&a("azahar", "menu")), None);
        assert_eq!(mechanism(&a("duckstation", "nope")), None);
        // Every key a chord sends is on the device.
        for e in ["duckstation", "pcsx2", "ppsspp", "azahar", "dolphin"] {
            for r in Row::ALL {
                if let Some(Mech::Keys(k)) = mech(e, r) {
                    assert!(k.iter().all(|c| KEYS.contains(c)));
                }
            }
        }
    }
}
