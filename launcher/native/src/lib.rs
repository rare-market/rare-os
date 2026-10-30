//! The native Rare launcher (launcher/native): the kit, the app shell and every screen.
//!
//! - `kit`: launcher/src/ui one to one, as view elements.
//! - `screens`: one module per screen area.
//! - `app`, `state`, `act`, `data`: the shell's state, the input stack and the data the Phase 1
//!   screens read.

pub mod act;
pub mod app;
pub mod avatars;
pub mod banners;
pub mod data;
pub mod features;
pub mod kit;
pub mod page;
pub mod side;
pub mod screens;
pub mod sign;
pub mod social;
pub mod state;
