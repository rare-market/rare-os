//! The controller while InputPlumber's target is `deck-uhid` (gyro.rs: a Steam game runs).
//! Thin wrapper: the hidraw reader lives in the rare-input crate (launcher/crates/rare-input/src/deck.rs).
pub use rare_input::deck::*;
pub use rare_input::pad::Ev;
