//! The notify state and rules are the rare-notify crate's (ported from launcher/src/notify); the
//! screens read them through these paths.
#![allow(dead_code)]

pub use rare_notify::{actions, battery, downloads, kinds, latency, logic, storage, store, types, LocalNotice};
