//! Rare's launch path, extracted from the Tauri launcher's src-tauri/src (launch.rs, steamcfg.rs,
//! apps/*): the runner that starts and tracks games, Steam's client defaults and Proton tools, and
//! the Apps tiles. The Tauri crate keeps thin wrappers; the native launcher calls this directly.

pub mod api;
pub mod apps;
pub mod launch;
pub mod steamcef;
pub mod steamcfg;
pub mod steamdl;
pub mod steamlog;
