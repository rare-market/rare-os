//! Notices data and pure logic (notify/{types,kinds,logic,mock,store,actions}.ts, timeAgo of
//! marketplace/format.ts) and the Notifications tab body (overlay/menuTabs.tsx NoticesTab).

use super::{calm, mk_row, G_NEWS, G_NEWS_HEAD};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Id, Node};

// ---- types.ts ----

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Item { item_id: String },
    Collection,
    Position,
    Coin,
    Tx,
    Trader,
    Friends,
    Party,
    OsUpdate,
    Downloads,
    Emulators,
    Trophies,
    Battery,
}

#[derive(Clone, Debug)]
pub struct Notice {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    /// The picture's name for the fallback initial (item name, coin symbol or title).
    pub art_label: String,
    pub action: Action,
    pub item_id: Option<String>,
    pub cp_ts: Option<i64>,
    pub created_at: i64,
}

/// The kinds with a switch in Settings (kinds.ts) are all on by default; `off` lists the ones turned off.
#[derive(Clone, Debug, Default)]
pub struct Prefs {
    pub off: Vec<String>,
}

/// Unknown kinds (a newer server) are shown: only a switched-off kind is hidden.
pub fn kind_on(p: &Prefs, kind: &str) -> bool {
    !p.off.iter().any(|k| k == kind)
}

// ---- logic.ts ----

/// Compares two decimal id strings without losing precision.
pub fn cmp_id(a: &str, b: &str) -> std::cmp::Ordering {
    let x = a.trim_start_matches('0');
    let y = b.trim_start_matches('0');
    x.len().cmp(&y.len()).then_with(|| x.cmp(y))
}

pub fn is_id(v: &str) -> bool {
    !v.is_empty() && v.len() <= 40 && v.bytes().all(|b| b.is_ascii_digit())
}

/// When a notice happened, for ordering, read and clear marks.
pub fn notice_time(n: &Notice) -> i64 {
    if n.created_at != 0 {
        n.created_at
    } else {
        n.cp_ts.unwrap_or(0)
    }
}

/// Newest first, one copy of each id, at most `cap`. Later copies of an id never replace the first.
pub fn merge_notices(existing: &[Notice], incoming: &[Notice], cap: usize) -> Vec<Notice> {
    let mut by: Vec<Notice> = vec![];
    for n in existing.iter().chain(incoming) {
        if is_id(&n.id) && !by.iter().any(|x| x.id == n.id) {
            by.push(n.clone());
        }
    }
    by.sort_by(|a, b| notice_time(b).cmp(&notice_time(a)).then_with(|| cmp_id(&b.id, &a.id)));
    by.truncate(cap);
    by
}

/// The ids in `incoming` not in `existing`.
pub fn new_ids(existing: &[Notice], incoming: &[Notice]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for n in incoming {
        if is_id(&n.id) && !existing.iter().any(|e| e.id == n.id) && !out.contains(&n.id) {
            out.push(n.id.clone());
        }
    }
    out
}

/// The notices the centre shows: kinds that are on, newer than the last Clear.
pub fn visible<'a>(notices: &'a [Notice], prefs: &Prefs, cleared_at: Option<i64>) -> Vec<&'a Notice> {
    notices.iter().filter(|n| kind_on(prefs, &n.kind) && cleared_at.filter(|c| *c != 0).map_or(true, |c| notice_time(n) > c)).collect()
}

/// Read: opened one by one, or at or before the last "Mark all read".
pub fn is_read(n: &Notice, read: &[String], read_at: Option<i64>) -> bool {
    read.contains(&n.id) || read_at.filter(|t| *t != 0).map_or(false, |t| notice_time(n) <= t)
}

pub fn unread_count(notices: &[Notice], prefs: &Prefs, cleared_at: Option<i64>, read: &[String], read_at: Option<i64>) -> usize {
    visible(notices, prefs, cleared_at).into_iter().filter(|n| !is_read(n, read, read_at)).count()
}

// ---- store.ts ----

#[derive(Clone, Debug, Default)]
pub struct NoticeStore {
    pub notices: Vec<Notice>,
    pub read: Vec<String>,
    pub read_at: Option<i64>,
    pub cleared_at: Option<i64>,
    pub prefs: Prefs,
}

impl NoticeStore {
    pub fn visible(&self) -> Vec<&Notice> {
        visible(&self.notices, &self.prefs, self.cleared_at)
    }
    pub fn unread(&self) -> usize {
        unread_count(&self.notices, &self.prefs, self.cleared_at, &self.read, self.read_at)
    }
    pub fn mark_read(&mut self, id: &str) {
        if !self.read.iter().any(|r| r == id) {
            self.read.insert(0, id.to_string());
            self.read.truncate(200);
        }
    }
    pub fn mark_all_read(&mut self, now_ms: i64) {
        let newest = self.notices.first().map_or(0, notice_time);
        self.read_at = Some(self.read_at.unwrap_or(0).max(now_ms).max(newest));
        self.read.clear();
    }
}

// ---- actions.ts ----

pub fn is_offer(n: &Notice) -> bool {
    n.kind == "offer_received" && n.item_id.is_some()
}

pub fn action_label(a: &Action, n: Option<&Notice>) -> &'static str {
    if n.map_or(false, is_offer) {
        return "Accept offer";
    }
    match a {
        Action::Item { .. } => "Open item",
        Action::Collection => "Open collection",
        Action::Position => "Open position",
        Action::Coin => "Open coin",
        Action::Tx => "View transaction",
        Action::Trader => "Open trader",
        Action::Friends => "Open friends",
        Action::Party => "Open party",
        Action::OsUpdate => "Restart now",
        Action::Downloads => "Open library",
        Action::Emulators => "Open emulators",
        Action::Trophies => "Open trophies",
        Action::Battery => "Open power",
    }
}

// ---- marketplace/format.ts timeAgo ----

pub fn time_ago(t: Option<i64>, now: i64) -> String {
    let Some(t) = t.filter(|t| *t != 0) else { return "--".into() };
    let s = (((now - t) as f64 / 1000.0).round() as i64).max(0);
    if s < 60 {
        return format!("{s}s");
    }
    let m = s / 60;
    if m < 60 {
        return format!("{m}m");
    }
    let h = m / 60;
    if h < 24 {
        return format!("{h}h");
    }
    let d = h / 24;
    if d < 30 {
        return format!("{d}d");
    }
    let mo = d / 30;
    if mo < 12 {
        return format!("{mo}mo");
    }
    format!("{}y", d / 365)
}

// ---- mock.ts (the samples, stamped ago) ----

pub const NOW_MS: i64 = super::friends::NOW_MS;

fn mk(id: u32, kind: &str, title: &str, body: Option<&str>, art: &str, action: Action, item: Option<&str>, ago_ms: i64) -> Notice {
    let t = NOW_MS - ago_ms;
    Notice { id: id.to_string(), kind: kind.into(), title: title.into(), body: body.map(Into::into), art_label: art.into(), action, item_id: item.map(Into::into), cp_ts: Some(t - 450), created_at: t - 120 }
}

const POPKIN: &str = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";

/// The mock's live and older notices, newest first (two unread once `mock_store` marks the rest).
pub fn mock_notices() -> Vec<Notice> {
    let item = || Action::Item { item_id: POPKIN.into() };
    const M: i64 = 60_000;
    vec![
        mk(1008, "sold", "Popkins #412 sold for 70 SUI", Some("Bought by 0x12ab...9f3c on Tradeport"), "Popkins #412", item(), Some(POPKIN), M),
        mk(1007, "offer_received", "New offer of 64 SUI on Popkins #412", Some("From 0x7c1d...a210, ends in 3 days"), "Popkins #412", item(), Some(POPKIN), 3 * M),
        mk(1006, "price_alert", "SUI is above $1.00", Some("Your alert at $1.00 was hit. Now $1.01."), "SUI", Action::Coin, None, 12 * M),
        mk(1005, "perp_liquidation", "SUI 5x long is near liquidation", Some("Mark $0.87, liquidation at $0.81"), "SUI 5x long is near liquidation", Action::Position, None, 20 * M),
        mk(1004, "coin_received", "You received 25 SUI", Some("From 0x3f9e...11c0"), "SUI", Action::Tx, None, 25 * M),
        mk(1003, "bought", "You bought Popkins #1187 for 58 SUI", Some("From a listing on rare.market"), "Popkins #1187", item(), Some(POPKIN), 3 * 3_600_000),
        mk(1002, "airdrop", "Airdrop: Popkins Pass", Some("Minted to you by 0x55aa...0e12"), "Popkins Pass", Action::Collection, None, 26 * 3_600_000),
        mk(1001, "price_alert", "SUI is below $0.90", Some("Your alert at $0.90 was hit. Now $0.89."), "SUI", Action::Coin, None, 2 * 86_400_000),
    ]
}

/// The sample notices with the newest two unread.
pub fn mock_store() -> NoticeStore {
    let notices = mock_notices();
    let read_at = Some(notice_time(&notices[2]));
    NoticeStore { notices, read: vec![], read_at, cleared_at: None, prefs: Prefs::default() }
}

// ---- the tab ----

/// NoticeArt: a 2.5rem rounded-card on layer-03; the web's fallback is the first letter at 1.6rem
/// in secondary/60 (the picture itself arrives with rare-images).
fn art(cx: &mut Cx, parent: Id, label: &str) {
    let r = cx.r();
    let b = cx.scene.add(parent, s().wh(2.5 * r, 2.5 * r).no_shrink_().items_center().justify_center().done(), Node::bg(tok::L3).radius(0.5 * r).clip());
    if let Some(ch) = label.trim().chars().find(|c| c.is_alphanumeric()) {
        let up: String = ch.to_uppercase().collect();
        cx.scene.add_text(cx.t, b, s().done(), &up, ts(r, 1.6, 2.0, 600), tok::MUTED.alpha(0.6));
    }
}

pub fn notices_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, st: &NoticeStore, now_ms: i64) {
    let r = cx.r();
    let list = st.visible();
    if list.is_empty() {
        return calm(cx, parent, "Nothing new. Sales, offers, invites and trophies show up here.");
    }
    let unread = st.unread();
    let col = cx.scene.add(parent, s().col().done(), Node::default());
    if unread > 0 {
        let head = cx.scene.add(col, s().row().items_center().pad(0.75 * r, 0.4 * r, 0.75 * r, 0.3 * r).done(), Node::default());
        let t = text(cx, head, ty::LABEL, 400, tok::MUTED, &format!("{unread} new"));
        if let Some(ts_) = cx.scene.n(t).text.as_mut() {
            ts_.style.tabular = true;
        }
        let b = cx.scene.add(head, s().row().items_center().justify_center().ml_auto().h(1.9 * r).px(0.7 * r).done(), Node::bg(tok::L2).radius(0.25 * r));
        let tid = text(cx, b, ty::META, 500, tok::FG, "Mark all read");
        nav.add(Focusable { id: b, kind: Kind::Btn { base: tok::L2, fg: tok::FG, text: tid }, group: G_NEWS_HEAD, hint: "Mark all read".into(), tag: 0, sid: "mark-all-read".into() });
    }
    for (i, n) in list.iter().enumerate() {
        let read = is_read(n, &st.read, st.read_at);
        let fo = mk_row(cx, col, 3.5);
        art(cx, fo.id, &n.art_label);
        let c = cx.scene.add(fo.id, s().col().grow().done(), Node::default());
        if read {
            text_tr(cx, c, ty::SMALL, 400, tok::FG.alpha(0.8), &n.title);
        } else {
            text_tr(cx, c, ty::SMALL, 600, tok::FG, &n.title);
        }
        let ago = time_ago(Some(notice_time(n)), now_ms);
        let sub = [Some(ago), n.body.clone()].into_iter().flatten().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" \u{b7} ");
        text_tr(cx, c, ty::LABEL, 400, tok::MUTED, &sub);
        if !read {
            dot(cx, fo.id, tok::FG);
        }
        nav.add(fo.group(G_NEWS).hint(action_label(&n.action, Some(n))).tag(i as u32).sid(&format!("notice:{}", n.id)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    const NOW: i64 = 1_800_000_000_000;
    fn n(id: &str, kind: &str, age: i64) -> Notice {
        Notice { id: id.into(), kind: kind.into(), title: format!("Notice {id}"), body: None, art_label: String::new(), action: Action::Tx, item_id: None, cp_ts: Some(NOW - age - 300), created_at: NOW - age }
    }
    fn ids(v: &[Notice]) -> Vec<&str> {
        v.iter().map(|x| x.id.as_str()).collect()
    }

    #[test]
    fn compares_decimal_strings_past_2_pow_53() {
        assert_eq!(cmp_id("9", "10"), Ordering::Less);
        assert_eq!(cmp_id("9007199254740993", "9007199254740992"), Ordering::Greater);
        assert_eq!(cmp_id("0012", "12"), Ordering::Equal);
    }

    #[test]
    fn merges_newest_first_with_one_copy_per_id() {
        let a = merge_notices(&[n("3", "sold", 0), n("1", "sold", 0)], &[n("2", "sold", 0), n("3", "sold", 0), n("4", "sold", 0)], 100);
        assert_eq!(ids(&a), ["4", "3", "2", "1"]);
    }
    #[test]
    fn keeps_the_first_copy_of_an_id() {
        let mut first = n("7", "sold", 0);
        first.title = "first".into();
        let mut second = n("7", "sold", 0);
        second.title = "second".into();
        assert_eq!(merge_notices(&[first], &[second], 100)[0].title, "first");
    }
    #[test]
    fn caps_the_list() {
        let many: Vec<Notice> = (0..30).map(|i| n(&(i + 1).to_string(), "sold", 0)).collect();
        let m = merge_notices(&[], &many, 10);
        assert_eq!(m[0].id, "30");
        assert_eq!(m.len(), 10);
    }
    #[test]
    fn orders_server_and_local_notices_by_time_not_id() {
        let local = n("1793000000000001", "followed_trade", 5000);
        let m = merge_notices(&[local], &[n("42", "sold", 0), n("41", "sold", 9000)], 100);
        assert_eq!(ids(&m), ["42", "1793000000000001", "41"]);
    }
    #[test]
    fn reports_only_ids_it_has_not_seen() {
        assert_eq!(new_ids(&[n("1", "sold", 0), n("2", "sold", 0)], &[n("2", "sold", 0), n("3", "sold", 0)]), ["3"]);
    }

    #[test]
    fn hides_switched_off_kinds_and_cleared_notices() {
        let prefs = Prefs { off: vec!["offer_received".into()] };
        let list = vec![n("5", "sold", 0), n("4", "offer_received", 1000), n("3", "bought", 2000), n("2", "sold", 3000)];
        let v: Vec<&str> = visible(&list, &prefs, Some(NOW - 3000)).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(v, ["5", "3"]);
    }
    #[test]
    fn counts_unread_from_the_read_set_and_the_read_mark() {
        let list = vec![n("5", "sold", 0), n("4", "sold", 1000), n("3", "sold", 2000), n("2", "sold", 3000)];
        assert_eq!(unread_count(&list, &Prefs::default(), None, &["5".to_string()], Some(NOW - 2000)), 1);
    }

    #[test]
    fn mark_all_read_covers_everything_and_open_marks_one() {
        let mut st = mock_store();
        assert_eq!(st.unread(), 2);
        st.mark_read(&st.notices[0].id.clone());
        assert_eq!(st.unread(), 1);
        st.mark_all_read(NOW_MS);
        assert_eq!(st.unread(), 0);
    }

    #[test]
    fn time_ago_units() {
        let at = |ms: i64| time_ago(Some(NOW - ms), NOW);
        assert_eq!(at(42_000), "42s");
        assert_eq!(at(25 * 60_000), "25m");
        assert_eq!(at(3 * 3_600_000), "3h");
        assert_eq!(at(2 * 86_400_000), "2d");
        assert_eq!(at(70 * 86_400_000), "2mo");
        assert_eq!(at(800 * 86_400_000), "2y");
        assert_eq!(time_ago(None, NOW), "--");
    }

    #[test]
    fn action_labels_follow_the_action_and_offers() {
        assert_eq!(action_label(&Action::Tx, None), "View transaction");
        assert_eq!(action_label(&Action::Item { item_id: "x".into() }, None), "Open item");
        let mut o = n("1", "offer_received", 0);
        o.item_id = Some("x".into());
        assert_eq!(action_label(&o.action, Some(&o)), "Accept offer");
        let mock = mock_notices();
        assert!(is_offer(&mock[1]) && !is_offer(&mock[0]));
    }
}
