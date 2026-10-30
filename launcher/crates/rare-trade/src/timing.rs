//! Polls and debounces as plain functions of the time the UI passes in (no threads, no timers):
//! the web launcher's `poll` cadences (lib/idle.ts) and the 300 ms settle before a quote or a perps
//! preview is asked for.

/// How present the person is: using the launcher, idle (no input for a minute), or away (another
/// window in front).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Active,
    Idle,
    Away,
}

/// Per-activity interval in ms; None stops the poll in that state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    pub active: u64,
    pub idle: Option<u64>,
    pub away: Option<u64>,
}

impl Cadence {
    /// A screen's poll: as asked while in use, at least a minute apart idle, none while away.
    pub fn slow_when_idle(ms: u64) -> Cadence {
        Cadence { active: ms, idle: Some((ms * 6).max(60_000)), away: None }
    }

    pub fn every(&self, a: Activity) -> Option<u64> {
        match a {
            Activity::Active => Some(self.active),
            Activity::Idle => self.idle,
            Activity::Away => self.away,
        }
    }

    /// Milliseconds until the next run, or None when the activity stops the poll.
    pub fn due_in(&self, last_run_ms: u64, now_ms: u64, a: Activity) -> Option<u64> {
        let every = self.every(a)?;
        Some((last_run_ms + every).saturating_sub(now_ms))
    }

    /// Whether a run is due now. Nothing polls while a game has the screen (`game_running`).
    pub fn due(&self, last_run_ms: Option<u64>, now_ms: u64, a: Activity, game_running: bool) -> bool {
        if game_running {
            return false;
        }
        match last_run_ms {
            None => true,
            Some(last) => self.due_in(last, now_ms, a) == Some(0),
        }
    }
}

/// How long a ticket must stay still before it is quoted or previewed.
pub const SETTLE_MS: u64 = 300;

/// A key that counts as settled once it has not changed for [`SETTLE_MS`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settle {
    key: Option<String>,
    since: u64,
}

impl Settle {
    /// Records the current key at `now_ms`.
    pub fn observe(&mut self, key: Option<String>, now_ms: u64) {
        if key != self.key {
            self.key = key;
            self.since = now_ms;
        }
    }

    /// The key once it has been still for long enough.
    pub fn settled(&self, now_ms: u64) -> Option<&str> {
        let k = self.key.as_deref()?;
        (now_ms >= self.since + SETTLE_MS).then_some(k)
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
}

/// The wall clock in ms since the epoch, for callers without one of their own.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadences() {
        let c = Cadence::slow_when_idle(15_000);
        assert_eq!(c, Cadence { active: 15_000, idle: Some(90_000), away: None });
        assert_eq!(Cadence::slow_when_idle(5_000).idle, Some(60_000));
        assert_eq!(c.due_in(1_000, 10_000, Activity::Active), Some(6_000));
        assert_eq!(c.due_in(1_000, 99_000, Activity::Idle), Some(0));
        assert_eq!(c.due_in(1_000, 99_000, Activity::Away), None);
        assert!(c.due(None, 0, Activity::Active, false));
        assert!(!c.due(None, 0, Activity::Active, true));
        assert!(c.due(Some(0), 15_000, Activity::Active, false));
    }

    #[test]
    fn settles_after_300_ms_still() {
        let mut s = Settle::default();
        s.observe(Some("a".into()), 0);
        assert_eq!(s.settled(299), None);
        s.observe(Some("a".into()), 299);
        assert_eq!(s.settled(300), Some("a"));
        s.observe(Some("b".into()), 350);
        assert_eq!(s.settled(600), None);
        assert_eq!(s.settled(650), Some("b"));
    }
}
