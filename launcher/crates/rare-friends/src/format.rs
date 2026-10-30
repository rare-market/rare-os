//! Presence in words. Pure; the clock is passed in.

use crate::types::{Presence, PresenceState};

/// JavaScript's Math.round (halves go up), for the same wording as the web launcher.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

fn mins(ms: i64) -> String {
    let m = js_round(ms as f64 / 60_000.0).max(1.0) as i64;
    if m < 60 {
        return format!("{m} min");
    }
    let rest = if m % 60 != 0 { format!("{} min", m % 60) } else { String::new() };
    format!("{} h {rest}", m / 60).trim().to_string()
}

/// "just now", "12 min ago", "3 h ago", "yesterday", "30 days ago".
pub fn ago(t: i64, now: i64) -> String {
    let s = ((now - t) as f64 / 1000.0).max(0.0);
    if s < 120.0 {
        return "just now".into();
    }
    if s < 3600.0 {
        return format!("{} min ago", js_round(s / 60.0));
    }
    if s < 86400.0 {
        return format!("{} h ago", js_round(s / 3600.0));
    }
    if s < 86400.0 * 2.0 {
        return "yesterday".into();
    }
    format!("{} days ago", js_round(s / 86400.0))
}

/// "Playing ACT", "Online", "Away", "Last seen 3 h ago", "Offline". `long` adds how long.
pub fn presence_text(p: &Presence, long: bool, now: i64) -> String {
    match p.state {
        PresenceState::Offline => match p.last_seen {
            Some(t) if t != 0 => format!("Last seen {}", ago(t, now)),
            _ => "Offline".into(),
        },
        PresenceState::Away => match &p.game {
            Some(g) => format!("Away, in {}", g.title),
            None => "Away".into(),
        },
        PresenceState::Online => match (&p.game, p.since) {
            (Some(g), Some(since)) if long && since != 0 => format!("Playing {} for {}", g.title, mins(now - since)),
            (Some(g), _) => format!("Playing {}", g.title),
            (None, _) => "Online".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GameRef;

    const NOW: i64 = 1_800_000_000_000;

    fn act() -> Option<GameRef> {
        Some(GameRef { id: "web:act".into(), title: "ACT".into(), kind: crate::types::GameKind::Web, ..GameRef::default() })
    }
    fn p(state: PresenceState, game: Option<GameRef>, since: Option<i64>, last_seen: Option<i64>) -> Presence {
        Presence { state, game, since, last_seen }
    }

    #[test]
    fn says_what_a_friend_is_doing() {
        use PresenceState::*;
        assert_eq!(presence_text(&p(Online, act(), Some(NOW - 42 * 60_000), Some(NOW)), false, NOW), "Playing ACT");
        assert_eq!(presence_text(&p(Online, act(), Some(NOW - 42 * 60_000), Some(NOW)), true, NOW), "Playing ACT for 42 min");
        assert_eq!(presence_text(&p(Online, act(), Some(NOW - 75 * 60_000), Some(NOW)), true, NOW), "Playing ACT for 1 h 15 min");
        assert_eq!(presence_text(&p(Online, None, Some(NOW), Some(NOW)), false, NOW), "Online");
        assert_eq!(presence_text(&p(Away, None, Some(NOW), Some(NOW)), false, NOW), "Away");
    }

    #[test]
    fn says_when_an_offline_friend_was_last_seen_if_they_share_it() {
        use PresenceState::*;
        assert_eq!(presence_text(&p(Offline, None, None, Some(NOW - 3 * 3_600_000)), false, NOW), "Last seen 3 h ago");
        assert_eq!(presence_text(&p(Offline, None, None, Some(NOW - 30 * 86_400_000)), false, NOW), "Last seen 30 days ago");
        assert_eq!(presence_text(&p(Offline, None, None, None), false, NOW), "Offline");
    }
}
