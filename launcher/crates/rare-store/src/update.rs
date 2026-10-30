//! Pure decisions: install or update, and the offline launch grace.

use serde::Serialize;

/// How long a positive licence check lets a `launch_check` game start offline.
pub const LAUNCH_GRACE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateDecision {
    /// Not installed, and the chain has a release for the licence's edition.
    Install,
    /// Installed, and the chain's release for that edition is newer.
    Update,
    /// Installed, and nothing newer is on chain.
    UpToDate,
    /// Not installed, and there is no release to install.
    Unavailable,
}

/// `installed` is the installed (edition, version); `chain_version` is the on-chain release
/// version for that edition (the licence's edition when nothing is installed).
pub fn update_decision(installed: Option<(u64, u64)>, chain_version: Option<u64>) -> UpdateDecision {
    match (installed, chain_version) {
        (None, Some(_)) => UpdateDecision::Install,
        (None, None) => UpdateDecision::Unavailable,
        (Some((_, have)), Some(chain)) if chain > have => UpdateDecision::Update,
        (Some(_), _) => UpdateDecision::UpToDate,
    }
}

/// True when the game may start: no launch check, or the last positive check is recent enough.
/// `checked_ms` 0 means the last check failed or never happened.
pub fn launch_allowed(launch_check: bool, checked_ms: u64, now_ms: u64) -> bool {
    !launch_check || (checked_ms > 0 && now_ms.saturating_sub(checked_ms) <= LAUNCH_GRACE_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use UpdateDecision::*;

    #[test]
    fn decides_updates() {
        assert_eq!(update_decision(None, Some(3)), Install);
        assert_eq!(update_decision(None, None), Unavailable);
        assert_eq!(update_decision(Some((0, 3)), Some(4)), Update);
        assert_eq!(update_decision(Some((0, 3)), Some(3)), UpToDate);
        assert_eq!(update_decision(Some((1, 3)), Some(2)), UpToDate);
        assert_eq!(update_decision(Some((1, 3)), None), UpToDate);
        assert_eq!(serde_json::to_value(UpToDate).unwrap(), "upToDate");
    }

    #[test]
    fn launch_grace() {
        let now = 100 * LAUNCH_GRACE_MS;
        assert!(launch_allowed(false, 0, now));
        assert!(launch_allowed(true, now, now));
        assert!(launch_allowed(true, now - LAUNCH_GRACE_MS, now));
        assert!(!launch_allowed(true, now - LAUNCH_GRACE_MS - 1, now));
        assert!(!launch_allowed(true, 0, now));
        // A check stamped slightly in the future (clock change) still counts.
        assert!(launch_allowed(true, now + 1000, now));
    }
}
