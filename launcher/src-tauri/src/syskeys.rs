//! Volume keys, whatever has the screen, and Rare + volume for brightness. Thin wrapper: the
//! reader and the steps live in the rare-input crate (launcher/crates/rare-input/src/volume.rs).

pub use rare_input::volume::key;

/// Starts the reader in Rare OS game mode (Linux only): in Steam's session Steam handles the
/// keys and on a desktop the desktop does.
pub fn start() {
    #[cfg(target_os = "linux")]
    {
        if crate::overlay::gamescope::session_client() {
            rare_input::volume::start();
        } else {
            log::info!("volume keys: not Rare's own gamescope session, left to Steam or the desktop");
        }
    }
}
