//! Rare's input stack, with no UI in it. The Tauri shell (src-tauri's gamepad, deckpad,
//! inputplumber, gyro, sysbuttons, syskeys and powerbtn modules, now thin wrappers) and the
//! native launcher (Rare Render's engine input) run the same code:
//!
//! - [`pad`]: the controller state machine. Raw button, axis and connect events from any pad
//!   (gilrs in the Tauri shell, evdev in the native launcher, the Deck pad over hidraw) become
//!   launcher presses: d-pad and left stick with hysteresis and the 320 ms then 120 to 40 ms
//!   repeat, analog triggers, bounce and double-A filtering, one pad at a time, Guide press and
//!   hold, Guide + A for quick access, the share chords and the View + Menu way out.
//! - [`deck`]: the virtual Steam Deck pad InputPlumber creates while a Steam game runs.
//! - [`ip`]: the SuiPlay's Rare, Quick, LC and RC buttons as InputPlumber D-Bus actions, and the
//!   intercept mode while a Rare surface is over a game.
//! - [`side`]: what LC and RC do on press and hold.
//! - [`volume`]: volume keys, Rare + volume for brightness.
//! - [`power`]: the power button (press sleeps, hold opens the power menu).
//! - [`remap`]: the button layout, an InputPlumber device profile Rare writes and loads.
//! - [`gyro`]: switching InputPlumber's pad target so Steam games get motion.
//! - [`remote`]: the virtual pad network apps drive (Settings > Developer > Allow network apps).
//! - [`rumble`]: the short UI rumbles, and an evdev force-feedback player that never re-uploads
//!   an idle effect (no ioctl spam to InputPlumber).
//! - [`evdev`]: blocking evdev readers for the native launcher, one thread per pad, and a fresh
//!   read of a pad's state (`probe`) before a held direction repeats.
//! - [`reset`]: input changing hands (intercept mode, pad target, focus, overlay, resume): every
//!   state machine lets go of what a fresh read doesn't show held.

pub mod chords;
pub mod deck;
#[cfg(target_os = "linux")]
pub mod evdev;
pub mod gyro;
pub mod ip;
pub mod log;
pub mod pad;
pub mod power;
pub mod remap;
pub mod remote;
pub mod reset;
pub mod rumble;
pub mod side;
pub mod volume;

pub use log::{debug_input, log};

pub use rare_core::Pad;

/// A d-pad direction.
pub fn is_dir(p: Pad) -> bool {
    matches!(p, Pad::Up | Pad::Down | Pad::Left | Pad::Right)
}
