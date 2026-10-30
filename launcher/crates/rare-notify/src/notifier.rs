//! What happens to each arriving notice, on the UI thread: into the store, then routed
//! (`logic::route`) to a toast with the notify sound, a desktop notification when the launcher is in
//! the background, held quietly while a game runs, or straight into the centre for old ones. Also
//! notices made in the launcher (`notify_local`: followed trades, friends, downloads, battery, OS
//! updates), the wake summary after a resume and the summary when a game ends.
//!
//! Everything only the front end can do (sound, the HUD toast, desktop notifications, the launcher's
//! own toast line, focus) goes through [`NotifyHost`]. The stream client (`stream.rs`) runs on its
//! own thread; the UI drains its events into [`Notifier::handle`].

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::latency::Latency;
use crate::logic::{is_fresh, is_id, route, NoticeClass, PlayQueue, Route, RouteCtx, Routing, WakeSummary};
use crate::storage::Storage;
use crate::store::NotifyStore;
use crate::stream::{SocialEvent, StreamEvent};
use crate::types::{now_ms, Notice, NoticeAction, NoticeKind};

/// After a resume, for this long notices are counted into one summary toast instead of popping up
/// one by one.
pub const WAKE_MS: i64 = 10_000;
/// One bell for a burst, not one per notice.
const SOUND_GAP_MS: i64 = 1500;

/// What the front end does for notifications. Every method has a harmless default.
pub trait NotifyHost {
    fn now_ms(&self) -> i64 {
        now_ms()
    }
    /// The launcher window is in front and focused (desktop.rs `launcher_focused`).
    fn launcher_focused(&self) -> bool {
        true
    }
    /// The notify sound (nav feedback "notify").
    fn play_sound(&self) {}
    /// Over a game or app the launcher's toast is behind it: the HUD draws it.
    fn hud_toast(&self, _title: &str, _body: Option<&str>) {}
    /// A desktop notification (the launcher is in the background).
    fn send_desktop(&self, _n: &Notice) {}
    /// The launcher's own one-line toast (summaries after a game or a wake).
    fn app_toast(&self, _line: &str) {}
    /// A diagnostic line (delivery latency).
    fn log(&self, _line: &str) {}
}

/// A host that does nothing, by the system clock.
pub struct NoNotifyHost;
impl NotifyHost for NoNotifyHost {}

/// A notice made in the launcher (no server).
#[derive(Clone, Debug, PartialEq)]
pub struct LocalNotice {
    pub kind: NoticeKind,
    pub title: String,
    pub body: Option<String>,
    pub image_url: Option<String>,
    pub action: NoticeAction,
    pub digest: Option<String>,
    /// When it happened on chain, ms.
    pub at: Option<i64>,
}

impl LocalNotice {
    pub fn new(kind: NoticeKind, title: impl Into<String>, action: NoticeAction) -> Self {
        LocalNotice { kind, title: title.into(), body: None, image_url: None, action, digest: None, at: None }
    }
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

/// The parts of a followed trader's trade a notice needs (trade/traders/feed.ts's
/// FollowedTradeEvent, field names as the Trade API's).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FollowedTrade {
    pub trader: TraderRef,
    pub trade: TraderTrade,
    pub token: Option<TokenChip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TraderRef {
    pub address: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TraderTrade {
    pub id: String,
    pub digest: String,
    pub time_ms: i64,
    pub buy: bool,
    pub usd: f64,
    pub coin: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenChip {
    pub coin_type: String,
    pub symbol: String,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
}

/// The kinds of number `ui::numText` formats here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumKind {
    Money,
    Price,
}

/// One plain sentence for a followed trade. A local copy of trade/traders/feed.ts `tradeSentence`
/// (rare-trade owns the original), with the UI's number formatter passed in.
pub fn trade_sentence(e: &FollowedTrade, num_text: &dyn Fn(NumKind, f64) -> String) -> String {
    let sym = match &e.token {
        Some(t) => t.symbol.clone(),
        None => e.trade.coin.rsplit("::").next().filter(|s| !s.is_empty()).unwrap_or("a coin").to_string(),
    };
    format!("{} {} {} of {sym}", e.trader.name, if e.trade.buy { "bought" } else { "sold" }, num_text(NumKind::Money, e.trade.usd))
}

/// What one ingest did, for the caller's own bookkeeping and tests.
pub type Ingested = Option<Routing>;

pub struct Notifier {
    pub store: NotifyStore,
    pub latency: Latency,
    playing: PlayQueue,
    wake: WakeSummary,
    waking_until: Option<i64>,
    sound_at: i64,
    local_seq: u32,
    game_running: bool,
}

impl Notifier {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Notifier {
            store: NotifyStore::load(storage),
            latency: Latency::default(),
            playing: PlayQueue::default(),
            wake: WakeSummary::default(),
            waking_until: None,
            sound_at: i64::MIN / 2,
            local_seq: 0,
            game_running: false,
        }
    }

    pub fn game_running(&self) -> bool {
        self.game_running
    }

    /// One event from the stream thread. Friend events are returned for the friends module.
    pub fn handle(&mut self, ev: StreamEvent, host: &dyn NotifyHost) -> Option<SocialEvent> {
        match ev {
            StreamEvent::Status(s) => self.store.status = s,
            StreamEvent::Hello(h) => self.latency.set_skew(h.now, host.now_ms()),
            StreamEvent::Notice(n) => {
                self.ingest(*n, true, false, host);
            }
            StreamEvent::Recent(list) => self.ingest_recent(list, host),
            StreamEvent::Social(s) => return Some(s),
        }
        None
    }

    /// `recent` answered: oldest first into the centre, quietly.
    pub fn ingest_recent(&mut self, list: Vec<Notice>, host: &dyn NotifyHost) {
        for n in list.into_iter().rev() {
            self.ingest(n, false, false, host);
        }
    }

    /// One notice. `live`: from the stream now (not a backfill); `local`: made in the launcher.
    pub fn ingest(&mut self, n: Notice, live: bool, local: bool, host: &dyn NotifyHost) -> Ingested {
        if !is_id(&n.id) || self.store.has(&n.id) {
            return None;
        }
        let now = host.now_ms();
        // Latency only for live deliveries from the stream (a replay after a restart is minutes or days old).
        if live && !local && is_fresh(&n, now) {
            if let Some(line) = self.latency.record_receipt(&n, now) {
                host.log(&line);
            }
        }
        self.store.add(std::slice::from_ref(&n));
        let r = route(&n, &RouteCtx { prefs: &self.store.prefs, live, now, playing: self.game_running, focused: host.launcher_focused() });
        let waking = self.waking_until.is_some_and(|t| now < t);
        if waking && r.route != Route::Drop && r.cls != NoticeClass::Battery {
            self.wake.add(&n.id);
        } else if r.route == Route::Queue {
            self.playing.add(&n.id);
            self.store.missed = self.playing.count();
        } else if r.route == Route::Toast {
            self.store.show_toast(&n, r.hud, r.small, now);
            if r.hud {
                host.hud_toast(&n.title, n.body.as_deref());
            }
            if now - self.sound_at > SOUND_GAP_MS {
                self.sound_at = now;
                host.play_sound();
            }
        }
        if r.desktop {
            host.send_desktop(&n);
        }
        Some(r)
    }

    /// A notice made in the launcher, through the same path as the stream's: toast and sound, the
    /// centre, the kind's switch in Settings, the queue while a game runs, and desktop
    /// notifications. Ids are decimal like the server's and never collide with them (much longer).
    pub fn notify_local(&mut self, input: LocalNotice, host: &dyn NotifyHost) -> Notice {
        let now = host.now_ms();
        let n = Notice {
            id: format!("{now}{:03}", self.local_seq % 1000),
            kind: input.kind,
            addr: None,
            title: input.title,
            body: input.body,
            image_url: input.image_url,
            amount: None,
            item: None,
            action: input.action,
            digest: input.digest,
            cp_ts: input.at,
            created_at: now,
            sent_at: now,
        };
        self.local_seq = self.local_seq.wrapping_add(1);
        self.ingest(n.clone(), true, true, host);
        n
    }

    /// A followed trader's trade (the trade feed's event) as a notice.
    pub fn followed_trade(&mut self, e: &FollowedTrade, num_text: &dyn Fn(NumKind, f64) -> String, host: &dyn NotifyHost) -> Notice {
        let price = e.token.as_ref().and_then(|t| t.price_usd.filter(|p| *p != 0.0 && !p.is_nan()).map(|p| (t, p)));
        self.notify_local(
            LocalNotice {
                kind: NoticeKind::FollowedTrade,
                title: trade_sentence(e, num_text),
                body: price.map(|(t, p)| format!("{} at {}", t.symbol, num_text(NumKind::Price, p))),
                image_url: e.token.as_ref().and_then(|t| t.logo.clone()),
                action: NoticeAction::Trader { address: e.trader.address.clone() },
                digest: (!e.trade.digest.is_empty()).then(|| e.trade.digest.clone()),
                at: Some(e.trade.time_ms),
            },
            host,
        )
    }

    /// The device woke: what arrives in the next [`WAKE_MS`] becomes one summary toast. The host
    /// also asks the stream to reconnect and backfill (`NotifyStream::refresh`, `load_recent`).
    pub fn resume(&mut self, host: &dyn NotifyHost) {
        self.waking_until = Some(host.now_ms() + WAKE_MS);
    }

    /// Whether a game or app has the screen. When the last one ends, what came in while it ran
    /// stays in the centre and one line says so.
    pub fn set_game_running(&mut self, running: bool, host: &dyn NotifyHost) {
        let was = std::mem::replace(&mut self.game_running, running);
        if was && !running {
            let line = self.playing.flush();
            self.store.missed = 0;
            if let Some(l) = line {
                host.app_toast(&l);
            }
        }
    }

    /// Timers: toasts that are up long enough go, and the wake window closes with its summary
    /// (none under Do not disturb). Call every frame or on a short timer.
    pub fn tick(&mut self, host: &dyn NotifyHost) {
        let now = host.now_ms();
        self.store.expire_toasts(now);
        if self.waking_until.is_some_and(|t| now >= t) {
            self.waking_until = None;
            let line = self.wake.flush();
            if let Some(l) = line.filter(|_| !self.store.prefs.dnd) {
                host.app_toast(&l);
            }
        }
    }

    /// A toast drew for the first time (delivery latency's paint time).
    pub fn toast_painted(&mut self, id: &str, host: &dyn NotifyHost) {
        if let Some(line) = self.latency.record_paint(id, host.now_ms()) {
            host.log(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemStorage;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Host {
        now: Cell<i64>,
        focused: Cell<bool>,
        log: RefCell<Vec<String>>,
    }
    impl Host {
        fn at(now: i64) -> Self {
            Host { now: Cell::new(now), focused: Cell::new(true), ..Default::default() }
        }
        fn took(&self) -> Vec<String> {
            std::mem::take(&mut self.log.borrow_mut())
        }
    }
    impl NotifyHost for Host {
        fn now_ms(&self) -> i64 {
            self.now.get()
        }
        fn launcher_focused(&self) -> bool {
            self.focused.get()
        }
        fn play_sound(&self) {
            self.log.borrow_mut().push("sound".into());
        }
        fn hud_toast(&self, title: &str, _: Option<&str>) {
            self.log.borrow_mut().push(format!("hud {title}"));
        }
        fn send_desktop(&self, n: &Notice) {
            self.log.borrow_mut().push(format!("desktop {}", n.title));
        }
        fn app_toast(&self, line: &str) {
            self.log.borrow_mut().push(format!("toast {line}"));
        }
    }

    const NOW: i64 = 1_800_000_000_000;

    fn live(id: &str, kind: NoticeKind) -> Notice {
        Notice { id: id.into(), kind, title: format!("t{id}"), created_at: NOW - 100, cp_ts: Some(NOW - 400), sent_at: NOW - 50, ..Default::default() }
    }

    fn notifier() -> Notifier {
        Notifier::new(Arc::new(MemStorage::new()))
    }

    #[test]
    fn toasts_once_with_one_bell_per_burst() {
        let h = Host::at(NOW);
        let mut nf = notifier();
        assert_eq!(nf.ingest(live("1", NoticeKind::Sold), true, false, &h).unwrap().route, Route::Toast);
        assert!(nf.ingest(live("1", NoticeKind::Sold), true, false, &h).is_none());
        nf.ingest(live("2", NoticeKind::Sold), true, false, &h);
        assert_eq!(h.took(), ["sound"]);
        assert_eq!(nf.store.toasts.len(), 2);
        assert_eq!(nf.latency.samples().len(), 2);
        h.focused.set(false);
        h.now.set(NOW + 2000);
        nf.ingest(live("3", NoticeKind::Sold), true, false, &h);
        assert_eq!(h.took(), ["sound", "desktop t3"]);
    }

    #[test]
    fn holds_notices_over_a_game_and_summarises_when_it_ends() {
        let h = Host::at(NOW);
        let mut nf = notifier();
        nf.set_game_running(true, &h);
        nf.ingest(live("1", NoticeKind::FriendOnline), true, false, &h);
        nf.ingest(live("2", NoticeKind::DownloadDone), true, false, &h);
        assert_eq!(nf.store.missed, 2);
        nf.ingest(live("3", NoticeKind::Sold), true, false, &h);
        assert_eq!(h.took(), ["hud t3", "sound"]);
        assert!(nf.store.toasts[0].hud);
        nf.set_game_running(false, &h);
        assert_eq!(nf.store.missed, 0);
        assert_eq!(h.took(), ["toast 2 new notifications while you played"]);
    }

    #[test]
    fn backfill_goes_to_the_centre_quietly() {
        let h = Host::at(NOW);
        let mut nf = notifier();
        nf.handle(StreamEvent::Recent(vec![live("5", NoticeKind::Sold), live("4", NoticeKind::Sold)]), &h);
        assert!(h.took().is_empty());
        assert_eq!(nf.store.notices.len(), 2);
        assert!(nf.store.toasts.is_empty());
        assert_eq!(nf.store.unread(), 2);
    }

    #[test]
    fn a_wake_counts_into_one_summary_but_battery_still_toasts() {
        let h = Host::at(NOW);
        let mut nf = notifier();
        nf.resume(&h);
        nf.ingest(live("1", NoticeKind::Sold), true, false, &h);
        nf.ingest(live("2", NoticeKind::Sold), true, false, &h);
        nf.notify_local(LocalNotice::new(NoticeKind::Battery, "Battery at 15%.", NoticeAction::Battery), &h);
        assert_eq!(h.took(), ["sound"]);
        h.now.set(NOW + WAKE_MS);
        nf.tick(&h);
        assert_eq!(h.took(), ["toast 2 new while you were away"]);
    }

    #[test]
    fn local_notices_have_long_ids_and_follow_the_switches() {
        let h = Host::at(NOW);
        let mut nf = notifier();
        let a = nf.notify_local(LocalNotice::new(NoticeKind::DownloadDone, "x", NoticeAction::Downloads), &h);
        let b = nf.notify_local(LocalNotice::new(NoticeKind::DownloadDone, "y", NoticeAction::Downloads), &h);
        assert_eq!(a.id, format!("{NOW}000"));
        assert_eq!(b.id, format!("{NOW}001"));
        nf.store.set_kind(&NoticeKind::EmuUpdate, false);
        let c = nf.notify_local(LocalNotice::new(NoticeKind::EmuUpdate, "z", NoticeAction::Emulators), &h);
        assert!(nf.store.has(&c.id));
        assert!(!nf.store.visible().iter().any(|n| n.id == c.id));
        assert!(nf.latency.samples().is_empty());
    }

    #[test]
    fn followed_trades_read_as_a_sentence() {
        let h = Host::at(NOW);
        let mut nf = notifier();
        let fmt = |k: NumKind, v: f64| match k {
            NumKind::Money => format!("${v:.0}"),
            NumKind::Price => format!("${v:.2}"),
        };
        let e = FollowedTrade {
            trader: TraderRef { address: "0xt".into(), name: "Ana".into() },
            trade: TraderTrade { id: "1".into(), digest: "D".into(), time_ms: NOW - 1000, buy: true, usd: 250.0, coin: "0x2::sui::SUI".into() },
            token: Some(TokenChip { coin_type: "0x2::sui::SUI".into(), symbol: "SUI".into(), logo: Some("L".into()), price_usd: Some(3.5) }),
        };
        let n = nf.followed_trade(&e, &fmt, &h);
        assert_eq!(n.title, "Ana bought $250 of SUI");
        assert_eq!(n.body.as_deref(), Some("SUI at $3.50"));
        assert_eq!(n.action, NoticeAction::Trader { address: "0xt".into() });
        assert_eq!(n.cp_ts, Some(NOW - 1000));
        let bare = FollowedTrade { token: None, trade: TraderTrade { buy: false, usd: 5.0, coin: "0xabc::deep::DEEP".into(), ..e.trade.clone() }, ..e };
        assert_eq!(trade_sentence(&bare, &fmt), "Ana sold $5 of DEEP");
    }
}
