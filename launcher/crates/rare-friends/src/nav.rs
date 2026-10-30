//! Opening Friends at a person or a party (notices, Home's "Friends playing" row, the game page).
//! The host closes whatever is open and switches to the Friends tab; the screen shows the target
//! and clears it.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FriendsTarget {
    Friend { address: String },
    Party { id: String },
    Add,
    /// Pick a friend to send a shared capture to (the Share screen's "Send to a friend").
    Send {
        url: String,
        #[serde(rename = "captureKind")]
        kind: String,
    },
}

/// What the Friends screen should show next. `seq` changes on every open, so opening the same
/// target twice still moves the screen.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FriendsNav {
    pub target: Option<FriendsTarget>,
    pub seq: u64,
}

impl FriendsNav {
    pub fn open(&mut self, target: Option<FriendsTarget>) {
        self.target = target;
        self.seq += 1;
    }

    /// The screen took the target.
    pub fn take(&mut self) -> Option<FriendsTarget> {
        self.target.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_open_moves_the_screen() {
        let mut n = FriendsNav::default();
        n.open(Some(FriendsTarget::Party { id: "p1".into() }));
        n.open(Some(FriendsTarget::Party { id: "p1".into() }));
        assert_eq!(n.seq, 2);
        assert_eq!(n.take(), Some(FriendsTarget::Party { id: "p1".into() }));
        assert_eq!(n.take(), None);
    }
}
