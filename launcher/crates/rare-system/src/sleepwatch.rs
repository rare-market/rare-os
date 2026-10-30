//! Sleep and wake, seen from the launcher.
//!
//! Resume: CLOCK_BOOTTIME runs through suspend and CLOCK_MONOTONIC does not, so a jump in their
//! difference is a sleep. The page gets a `system-resume` event then and holds its network polling
//! until the network is back (see lib/sleep.ts). Auto-sleep: the page tracks input and decides;
//! this module only answers the two things the page cannot see, whether audio is playing and
//! whether the charger is in.

use serde::Serialize;
use std::time::Duration;

#[derive(Serialize, Clone, Copy, Default)]
#[serde(rename_all = "camelCase")]
pub struct SleepState {
    /// Nothing is charging the device.
    pub on_battery: bool,
    /// A sound card is streaming (an app or a game is playing audio).
    pub audio_playing: bool,
}

/// A sleep happened when the suspended-seconds counter moved by more than the poll jitter.
pub fn resumed(before: Option<i64>, now: Option<i64>) -> Option<i64> {
    match (before, now) {
        (Some(a), Some(b)) if b > a + 2 => Some(b - a),
        _ => None,
    }
}

/// `state: RUNNING` in any /proc/asound/card*/pcm*/sub*/status text. PipeWire closes the card
/// shortly after the last stream stops, so RUNNING means sound is really going out.
pub fn audio_running(statuses: &[String]) -> bool {
    statuses.iter().any(|s| s.lines().any(|l| l.trim() == "state: RUNNING"))
}

#[cfg(target_os = "linux")]
fn suspended_secs() -> Option<i64> {
    let get = |c| {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: clock_gettime writes into the timespec we own.
        (unsafe { libc::clock_gettime(c, &mut ts) } == 0).then_some(ts.tv_sec as i64)
    };
    Some(get(libc::CLOCK_BOOTTIME)? - get(libc::CLOCK_MONOTONIC)?)
}

#[cfg(not(target_os = "linux"))]
fn suspended_secs() -> Option<i64> {
    None
}

fn read(p: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

pub fn state() -> SleepState {
    let mut s = SleepState::default();
    // Mains online anywhere means plugged in; no Mains supply at all means battery only.
    let mut mains = false;
    let mut online = false;
    for e in std::fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten() {
        let p = e.path();
        if read(&p.join("type")).as_deref() == Some("Mains") {
            mains = true;
            online |= read(&p.join("online")).as_deref() == Some("1");
        }
    }
    s.on_battery = !(mains && online);
    let mut texts = Vec::new();
    for card in std::fs::read_dir("/proc/asound").into_iter().flatten().flatten() {
        if !card.file_name().to_string_lossy().starts_with("card") {
            continue;
        }
        for pcm in std::fs::read_dir(card.path()).into_iter().flatten().flatten() {
            if !pcm.file_name().to_string_lossy().starts_with("pcm") {
                continue;
            }
            for sub in std::fs::read_dir(pcm.path()).into_iter().flatten().flatten() {
                if let Some(t) = read(&sub.path().join("status")) {
                    texts.push(t);
                }
            }
        }
    }
    s.audio_playing = audio_running(&texts);
    s
}

pub fn sleep_state() -> SleepState {
    state()
}

pub fn start(sink: std::sync::Arc<dyn rare_core::EventSink>) {
    std::thread::spawn(move || {
        let mut before = suspended_secs();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = suspended_secs();
            if let Some(slept) = resumed(before, now) {
                log::info!("resumed after {slept} s asleep");
                sink.send(rare_core::Event::Other { name: "system-resume".into(), payload: serde_json::json!(slept) });
            }
            before = now;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jump_in_suspended_seconds_is_a_resume() {
        assert_eq!(resumed(Some(10), Some(10)), None);
        assert_eq!(resumed(Some(10), Some(12)), None);
        assert_eq!(resumed(Some(10), Some(75)), Some(65));
        assert_eq!(resumed(None, Some(75)), None);
    }

    #[test]
    fn running_pcm_means_audio() {
        let idle = "closed".to_string();
        let on = "state: RUNNING\nowner_pid   : 812\n".to_string();
        let prep = "state: PREPARED\n".to_string();
        assert!(!audio_running(&[idle.clone(), prep]));
        assert!(audio_running(&[idle, on]));
    }
}
