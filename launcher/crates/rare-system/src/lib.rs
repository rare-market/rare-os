//! Rare's device layer: everything the launcher does to the machine it runs on, with no UI in it.
//! Extracted from the Tauri launcher's src-tauri/src (system, power, network, radios, brightness,
//! display, osupdate, sleepwatch, upscale, hud, perfhud, freeze, devmode, render, instant); the
//! Tauri crate keeps one-line wrappers so the web UI works unchanged.

pub mod devmode;
pub mod audio;
pub mod brightness;
pub mod display;
pub mod freeze;
pub mod gamefx;
pub mod hud;
pub mod host;
pub mod instant;
pub mod media;
pub mod netctl;
pub mod network;
pub mod osupdate;
pub mod perfhud;
pub mod portal;
pub mod power;
pub mod radios;
pub mod render;
pub mod screen;
pub mod sleepwatch;
pub mod speakers;
pub mod ssh;
pub mod support;
pub mod system;
pub mod upscale;
