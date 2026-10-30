//! Wi-Fi power save off while a game or stream has the screen, back on when it leaves.
//!
//! Power save adds tens of milliseconds of wake-up latency to incoming packets (iwlwifi on the
//! SuiPlay0X1 has it on), which shows as stutter in streams and online games. It is turned off
//! with `iw dev <if> set power_save off` (nl80211 SET_POWER_SAVE: no reassociation, so SSH over
//! Wi-Fi stays up), through `/usr/libexec/rare/wifi-powersave` run with pkexec; polkit lets the
//! active local session do that one thing without a prompt
//! (image/system_files/usr/share/polkit-1/{actions,rules.d}).
//!
//! NetworkManager can't do it without dropping the link: `nmcli device modify ... wifi.powersave`
//! is refused (NM 1.56's Wi-Fi Reapply only accepts seen-bssids, mtu and wake-on-wlan), and
//! `nmcli connection modify` plus `up` reconnects. NM only writes power save when a connection
//! activates, so a reconnect during play may put the driver default back; the loop checks again
//! every 30 s while playing.

use std::path::PathBuf;
use std::time::{Duration, Instant};

const HELPER: &str = "/usr/libexec/rare/wifi-powersave";

/// Run ids of games and streams (`steam:`, `rare:`, `web:`, `stream:`, ...), not apps (`app:`).
pub fn is_game_or_stream(id: &str) -> bool {
    !id.starts_with("app:")
}

/// Power save should be off: a game or stream that is running has the screen.
pub fn want_off(front: Option<&str>, running: &[String]) -> bool {
    front.is_some_and(|f| is_game_or_stream(f) && running.iter().any(|r| r == f))
}

/// What to do on a tick, from what we want, what the driver says and whether we turned it off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    None,
    Off,
    On,
}

/// `now_on`: the driver's power save state (None when unknown). `ours`: power save is off
/// because we turned it off. Power save that was already off before a game stays as it was.
pub fn act(want_off: bool, now_on: Option<bool>, ours: bool) -> Act {
    match (want_off, now_on, ours) {
        // Playing and it is on (first time, or a reconnect put it back).
        (true, Some(true), _) => Act::Off,
        // Done playing: back on only if we were the ones who turned it off.
        (false, Some(false), true) => Act::On,
        (false, None, true) => Act::On,
        _ => Act::None,
    }
}

fn iface() -> Option<String> {
    let rd = std::fs::read_dir("/sys/class/net").ok()?;
    rd.flatten().find(|e| e.path().join("wireless").is_dir()).map(|e| e.file_name().to_string_lossy().into_owned())
}

/// `Power save: on` from `iw dev <if> get power_save` (no root needed).
pub fn parse_iw(out: &str) -> Option<bool> {
    match out.trim().strip_prefix("Power save:")?.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

fn power_save() -> Option<bool> {
    let dev = iface()?;
    parse_iw(&super::run("iw", &["dev", &dev, "get", "power_save"]).ok()?)
}

fn set(on: bool) -> Result<(), String> {
    super::run("pkexec", &["--disable-internal-agent", HELPER, if on { "on" } else { "off" }]).map(|_| ())
}

/// Present while power save is off because of us, so a launcher crash or restart turns it back on.
fn marker() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("rare-wifi-powersave-off"))
}

/// Starts the loop. `state` gives the run id in front (None while the launcher has the screen)
/// and the running ids.
pub fn start(state: impl Fn() -> (Option<String>, Vec<String>) + Send + 'static) {
    if !std::path::Path::new(HELPER).exists() || iface().is_none() {
        return;
    }
    std::thread::spawn(move || {
        let mut ours = marker().is_some_and(|m| m.exists());
        let mut last_want = None;
        let mut checked = Instant::now();
        loop {
            let (front, running) = state();
            let want = want_off(front.as_deref(), &running);
            // Read the driver when the wish changes, and every 30 s while playing.
            if last_want != Some(want) || (want && checked.elapsed() >= Duration::from_secs(30)) {
                checked = Instant::now();
                match act(want, power_save(), ours) {
                    Act::Off => match set(false) {
                        Ok(()) => {
                            log::info!("wifi power save off ({})", front.as_deref().unwrap_or(""));
                            ours = true;
                            if let Some(m) = marker() {
                                let _ = std::fs::write(m, "");
                            }
                        }
                        Err(e) => log::warn!("wifi power save off: {e}"),
                    },
                    Act::On => match set(true) {
                        Ok(()) => {
                            log::info!("wifi power save back on");
                            ours = false;
                            if let Some(m) = marker() {
                                let _ = std::fs::remove_file(m);
                            }
                        }
                        Err(e) => log::warn!("wifi power save on: {e}"),
                    },
                    Act::None => {
                        if !want && ours {
                            // Already on again (a reconnect); nothing of ours left to undo.
                            ours = false;
                            if let Some(m) = marker() {
                                let _ = std::fs::remove_file(m);
                            }
                        }
                    }
                }
                last_want = Some(want);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_and_streams_in_front() {
        let running = vec!["steam:1145360".to_string(), "stream:abc:1".to_string(), "app:firefox".to_string()];
        assert!(want_off(Some("steam:1145360"), &running));
        assert!(want_off(Some("stream:abc:1"), &running));
        // An app in front, the launcher in front, or a game that already ended.
        assert!(!want_off(Some("app:firefox"), &running));
        assert!(!want_off(None, &running));
        assert!(!want_off(Some("steam:7"), &running));
    }

    #[test]
    fn turns_off_and_restores_only_its_own_change() {
        assert_eq!(act(true, Some(true), false), Act::Off);
        // A reconnect put it back on mid-game.
        assert_eq!(act(true, Some(true), true), Act::Off);
        assert_eq!(act(true, Some(false), true), Act::None);
        // Already off before the game (the user's setting): never touched, never turned on.
        assert_eq!(act(true, Some(false), false), Act::None);
        assert_eq!(act(false, Some(false), false), Act::None);
        // Game over: back on.
        assert_eq!(act(false, Some(false), true), Act::On);
        assert_eq!(act(false, None, true), Act::On);
        assert_eq!(act(false, Some(true), true), Act::None);
        // Unknown state while playing: leave it.
        assert_eq!(act(true, None, false), Act::None);
    }

    #[test]
    fn reads_iw() {
        assert_eq!(parse_iw("Power save: on\n"), Some(true));
        assert_eq!(parse_iw("Power save: off"), Some(false));
        assert_eq!(parse_iw("command failed: No such device (-19)"), None);
    }
}
