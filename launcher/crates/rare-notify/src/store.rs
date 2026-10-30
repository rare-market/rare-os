//! Notification state: the notices (newest first), what has been read or cleared, the toasts on
//! screen, the count that arrived while a game ran, the stream's status and the person's settings.
//! Kept in [`Storage`] under the web launcher's keys: per device, survives updates.
//!
//! Toasts expire by time: the UI calls [`NotifyStore::expire_toasts`] (or `Notifier::tick`) each
//! frame or on a timer instead of the web launcher's per-toast timeouts.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;

use crate::logic::{self, class_of, is_id, merge_notices, notice_time, read_prefs, NoticeClass, Prefs, MERGE_CAP};
use crate::storage::{read_value, write_value, Storage};
use crate::types::{Notice, NoticeKind};

const K_LIST: &str = "rare.notify.list";
const K_READ: &str = "rare.notify.read";
const K_READ_AT: &str = "rare.notify.readAt";
const K_CLEARED_AT: &str = "rare.notify.clearedAt";
const K_PREFS: &str = "rare.notify.prefs";

/// How long a notice toast stays up, ms.
pub const TOAST_MS: i64 = 3000;
const MAX_TOASTS: usize = 3;
/// Notices kept on disk (the list in memory holds up to `MERGE_CAP`).
const STORED: usize = 50;
/// Ids marked read one by one, newest first.
const READ_CAP: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Off,
    Connecting,
    Live,
    Retrying,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Off => "off",
            Status::Connecting => "connecting",
            Status::Live => "live",
            Status::Retrying => "retrying",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub notice: Notice,
    /// When it went up, ms.
    pub at: i64,
    /// Raised over a game (the HUD draws it).
    pub hud: bool,
    /// The small trophy toast.
    pub small: bool,
}

pub struct NotifyStore {
    pub notices: Vec<Notice>,
    /// Opened one by one, newest first.
    pub read: Vec<String>,
    /// "Mark all read" and "Clear" marks, as times (ms): server and local notices share one timeline.
    pub read_at: Option<i64>,
    pub cleared_at: Option<i64>,
    pub prefs: Prefs,
    /// Oldest first, at most three.
    pub toasts: Vec<Toast>,
    /// Arrived while a game ran.
    pub missed: usize,
    pub status: Status,
    storage: Arc<dyn Storage>,
}

fn ms_or_none(v: Option<Value>) -> Option<i64> {
    v.and_then(|v| v.as_f64()).filter(|x| x.is_finite() && *x > 0.0).map(|x| x as i64)
}

/// A mark that covers everything in the list now, even a notice stamped a little ahead of this clock.
fn mark_now(prev: Option<i64>, list: &[Notice], now: i64) -> i64 {
    prev.unwrap_or(0).max(now).max(list.first().map(notice_time).unwrap_or(0))
}

impl NotifyStore {
    /// The state saved in `storage`, with defaults for anything missing or broken.
    pub fn load(storage: Arc<dyn Storage>) -> Self {
        let s = storage.as_ref();
        let list: Vec<Notice> = match read_value(s, K_LIST) {
            Some(Value::Array(a)) => a.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect(),
            _ => Vec::new(),
        };
        let read = match read_value(s, K_READ) {
            Some(Value::Array(a)) => a.into_iter().filter_map(|v| v.as_str().filter(|x| is_id(x)).map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        NotifyStore {
            notices: merge_notices(&[], &list, STORED),
            read,
            read_at: ms_or_none(read_value(s, K_READ_AT)),
            cleared_at: ms_or_none(read_value(s, K_CLEARED_AT)),
            prefs: read_prefs(&read_value(s, K_PREFS).unwrap_or(Value::Null)),
            toasts: Vec::new(),
            missed: 0,
            status: Status::Off,
            storage,
        }
    }

    pub fn add(&mut self, list: &[Notice]) {
        self.notices = merge_notices(&self.notices, list, MERGE_CAP);
        let keep = &self.notices[..self.notices.len().min(STORED)];
        write_value(self.storage.as_ref(), K_LIST, Some(keep));
    }

    pub fn has(&self, id: &str) -> bool {
        self.notices.iter().any(|n| n.id == id)
    }

    pub fn show_toast(&mut self, notice: &Notice, hud: bool, small: bool, now: i64) {
        if self.toasts.iter().any(|t| t.notice.id == notice.id) {
            return;
        }
        // Newest last (under the older ones), at most three; the oldest goes first.
        self.toasts.push(Toast { notice: notice.clone(), at: now, hud, small });
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.drain(..self.toasts.len() - MAX_TOASTS);
        }
    }

    pub fn dismiss_toast(&mut self, id: &str) {
        self.toasts.retain(|t| t.notice.id != id);
    }

    /// Takes down toasts that have been up for [`TOAST_MS`]; true when any went.
    pub fn expire_toasts(&mut self, now: i64) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| now - t.at < TOAST_MS);
        self.toasts.len() != before
    }

    pub fn mark_read(&mut self, id: &str) {
        if self.read.iter().any(|x| x == id) {
            return;
        }
        self.read.insert(0, id.to_string());
        self.read.truncate(READ_CAP);
        write_value(self.storage.as_ref(), K_READ, Some(&self.read));
    }

    pub fn mark_all_read(&mut self, now: i64) {
        let read_at = mark_now(self.read_at, &self.notices, now);
        self.read_at = Some(read_at);
        self.read.clear();
        write_value(self.storage.as_ref(), K_READ_AT, Some(&read_at));
        write_value(self.storage.as_ref(), K_READ, Some(&self.read));
    }

    pub fn clear(&mut self, now: i64) {
        let cleared_at = mark_now(self.cleared_at, &self.notices, now);
        let read_at = mark_now(self.read_at, &self.notices, now);
        self.cleared_at = Some(cleared_at);
        self.read_at = Some(read_at);
        self.read.clear();
        let s = self.storage.as_ref();
        write_value(s, K_CLEARED_AT, Some(&cleared_at));
        write_value(s, K_READ_AT, Some(&read_at));
        write_value(s, K_READ, Some(&self.read));
    }

    pub fn set_kind(&mut self, kind: &NoticeKind, on: bool) {
        let mut p = self.prefs.clone();
        p.set_kind(kind, on);
        self.set_prefs(p);
    }

    pub fn set_prefs(&mut self, prefs: Prefs) {
        self.prefs = prefs;
        write_value(self.storage.as_ref(), K_PREFS, Some(&self.prefs));
    }

    /// The centre's list: kinds that are on, newer than the last Clear.
    pub fn visible(&self) -> Vec<&Notice> {
        logic::visible(&self.notices, &self.prefs, self.cleared_at)
    }

    /// The centre's list for one class (the Rare menu's Notifications tab filters with this).
    pub fn by_class(&self, cls: Option<NoticeClass>) -> Vec<&Notice> {
        self.visible().into_iter().filter(|n| cls.map_or(true, |c| class_of(&n.kind) == c)).collect()
    }

    fn read_set(&self) -> HashSet<String> {
        self.read.iter().cloned().collect()
    }

    pub fn unread(&self) -> usize {
        logic::unread_count(&self.notices, &self.prefs, self.cleared_at, &self.read_set(), self.read_at)
    }

    pub fn is_read(&self, n: &Notice) -> bool {
        self.read.contains(&n.id) || logic::is_read(n, &HashSet::new(), self.read_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemStorage;

    fn n(id: &str, at: i64) -> Notice {
        Notice { id: id.into(), kind: NoticeKind::Sold, created_at: at, ..Default::default() }
    }

    #[test]
    fn persists_and_reloads() {
        let s: Arc<dyn Storage> = Arc::new(MemStorage::new());
        let mut st = NotifyStore::load(s.clone());
        st.add(&[n("1", 100), n("2", 200)]);
        st.mark_read("1");
        st.set_kind(&NoticeKind::Bought, false);
        let back = NotifyStore::load(s.clone());
        assert_eq!(back.notices.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
        assert_eq!(back.read, ["1"]);
        assert!(!back.prefs.kind_on(&NoticeKind::Bought));
        assert_eq!(back.unread(), 1);
    }

    #[test]
    fn marks_cover_notices_stamped_ahead_and_clear_empties_the_centre() {
        let mut st = NotifyStore::load(Arc::new(MemStorage::new()));
        st.add(&[n("1", 100), n("2", 5000)]);
        st.mark_all_read(1000);
        assert_eq!(st.read_at, Some(5000));
        assert_eq!(st.unread(), 0);
        st.add(&[n("3", 6000)]);
        assert_eq!(st.unread(), 1);
        st.clear(7000);
        assert!(st.visible().is_empty());
        assert_eq!(st.unread(), 0);
    }

    #[test]
    fn toasts_stack_three_and_expire() {
        let mut st = NotifyStore::load(Arc::new(MemStorage::new()));
        for (i, id) in ["1", "2", "3", "4"].iter().enumerate() {
            st.show_toast(&n(id, 0), false, false, i as i64 * 1000);
        }
        st.show_toast(&n("4", 0), false, false, 0);
        assert_eq!(st.toasts.iter().map(|t| t.notice.id.as_str()).collect::<Vec<_>>(), ["2", "3", "4"]);
        assert!(st.expire_toasts(4100));
        assert_eq!(st.toasts.len(), 2);
        st.dismiss_toast("4");
        assert_eq!(st.toasts.len(), 1);
    }
}
