//! share/store.ts as plain state: captures on this device, the one being looked at, its price,
//! and the upload. The page changes it from the pad and reports side effects (`Out::Act`) for the
//! app to run (rare-share for captures, the wallet and Walrus for the upload); the app writes the
//! answers back (`captures`, `quote`, `phase`). Nothing is uploaded or paid without the player
//! pressing Share and approving twice in Rare Wallet.

use super::format::{duration, share_epochs, CostEstimate, DurationId, Network, SponsorQuote, DURATIONS};
use rare_share::{Capture, ShareStatus};
use std::collections::HashMap;
use std::path::PathBuf;

/// The upload's steps (UploadProgress["step"]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Encode,
    Register,
    Upload,
    Certify,
}

/// ShareScreen's STEPS: (step, label, what it says while it is the current one).
pub const STEPS: [(Step, &str, &str); 4] = [
    (Step::Encode, "Prepare", "Preparing the files"),
    (Step::Register, "Pay for storage", "Approve in Rare Wallet"),
    (Step::Upload, "Upload", "Uploading to Walrus"),
    (Step::Certify, "Confirm", "Approve to confirm"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct ShareResult {
    pub quilt_id: String,
    pub blob_object_id: Option<String>,
    pub link: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Uploading(Step),
    Done { result: ShareResult, copied: bool },
    Error(String),
}

impl Phase {
    /// idle or error: the duration, payer and Share controls show.
    pub fn editable(&self) -> bool {
        matches!(self, Phase::Idle | Phase::Error(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub id: String,
    /// encoded quilt size
    pub size: u64,
    pub epochs: u32,
    pub days: u32,
    pub cost: Option<CostEstimate>,
    pub sponsor: Option<SponsorQuote>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Payer {
    #[default]
    Me,
    Rare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taking {
    Screenshot,
    Clip,
}

/// A capture's picture: the screenshot itself or a clip's poster. `Failed` is the web's "" (the
/// tile says it can't read the file).
#[derive(Clone, Debug, PartialEq)]
pub enum Thumb {
    Static(&'static str, &'static [u8]),
    File(PathBuf),
    Failed,
}

#[derive(Clone, Debug)]
pub struct ShareState {
    pub status: Option<ShareStatus>,
    pub captures: Option<Vec<Capture>>,
    /// captures newer than this are marked new (the value when the screen opened)
    pub seen_at: u64,
    pub thumbs: HashMap<String, Thumb>,
    pub selected: Option<String>,
    pub network: Network,
    pub duration: DurationId,
    pub payer: Payer,
    pub quote: Option<Quote>,
    pub phase: Phase,
    pub taking: Option<Taking>,
    pub error: Option<String>,
    /// The Rare Wallet account (useProfile address), if any.
    pub address: Option<String>,
    /// Delete was pressed once on the detail page ("Press again").
    pub confirm: bool,
    /// Wall clock for "3 h ago" (unix ms).
    pub now_ms: u64,
}

impl Default for ShareState {
    fn default() -> ShareState {
        ShareState {
            status: None,
            captures: None,
            seen_at: 0,
            thumbs: HashMap::new(),
            selected: None,
            network: Network::Mainnet,
            duration: DurationId::Month,
            payer: Payer::Me,
            quote: None,
            phase: Phase::Idle,
            taking: None,
            error: None,
            address: None,
            confirm: false,
            now_ms: 0,
        }
    }
}

impl ShareState {
    pub fn selected_capture(&self) -> Option<&Capture> {
        let id = self.selected.as_deref()?;
        self.captures.as_ref()?.iter().find(|c| c.id == id)
    }

    /// Opens a capture's page (None: back to the grid). Not while uploading.
    pub fn select(&mut self, id: Option<&str>) -> bool {
        if matches!(self.phase, Phase::Uploading(_)) {
            return false;
        }
        self.selected = id.map(str::to_string);
        self.quote = None;
        self.phase = Phase::Idle;
        self.error = None;
        self.confirm = false;
        true
    }

    /// True when it changed (the app prices the share again).
    pub fn set_duration(&mut self, d: DurationId) -> bool {
        if matches!(self.phase, Phase::Uploading(_)) || d == self.duration {
            return false;
        }
        self.duration = d;
        self.quote = None;
        true
    }

    /// X on the detail page: the other duration.
    pub fn next_duration(&self) -> DurationId {
        let i = DURATIONS.iter().position(|d| d.id == self.duration).unwrap_or(0);
        DURATIONS[(i + 1) % DURATIONS.len()].id
    }

    /// A price arrived (store.ts price()): kept only for what is still selected and chosen.
    pub fn priced(&mut self, id: &str, size: u64, cost: Option<CostEstimate>, sponsor: Option<SponsorQuote>) {
        if self.selected.as_deref() != Some(id) {
            return;
        }
        let (epochs, days) = share_epochs(self.duration, self.network);
        if !sponsor.as_ref().is_some_and(|s| s.covered) {
            self.payer = Payer::Me;
        }
        self.quote = Some(Quote { id: id.to_string(), size, epochs, days, cost, sponsor });
    }

    /// "Rare covers it" is chosen and the sponsor said yes.
    pub fn sponsored(&self) -> bool {
        self.payer == Payer::Rare && self.quote.as_ref().and_then(|q| q.sponsor.as_ref()).is_some_and(|s| s.covered)
    }

    /// The Share button can't be pressed (ShareScreen's `disabled`).
    pub fn share_blocked(&self) -> bool {
        match &self.quote {
            None => true,
            Some(q) => !self.sponsored() && (self.address.is_none() || q.cost.is_none()),
        }
    }

    /// Share pressed: starts the upload (the app runs it and reports steps), or says why not.
    pub fn begin_share(&mut self) -> bool {
        if self.selected_capture().is_none() || matches!(self.phase, Phase::Uploading(_)) || self.quote.is_none() {
            return false;
        }
        if !self.sponsored() && self.address.is_none() {
            self.phase = Phase::Error("Add a Rare Wallet account to share.".into());
            return false;
        }
        self.phase = Phase::Uploading(if self.sponsored() { Step::Upload } else { Step::Encode });
        self.error = None;
        true
    }

    /// The upload's outcome: done (the link was copied, or not), rejected in the wallet (back to
    /// idle), or failed in plain words.
    pub fn finished(&mut self, r: Result<(ShareResult, bool), (bool, String)>) {
        self.phase = match r {
            Ok((result, copied)) => Phase::Done { result, copied },
            Err((true, _)) => Phase::Idle,
            Err((false, m)) => Phase::Error(plain_share_error(&m, self.quote.as_ref().and_then(|q| q.cost).map(|c| c.wal_frost))),
        };
    }

    pub fn copied(&mut self, ok: bool) {
        if let Phase::Done { copied, .. } = &mut self.phase {
            *copied = ok;
        }
    }

    /// A capture was deleted on this device.
    pub fn removed(&mut self, id: &str) {
        if let Some(l) = &mut self.captures {
            l.retain(|c| c.id != id);
        }
        self.thumbs.remove(id);
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
            self.quote = None;
            self.phase = Phase::Idle;
        }
        self.confirm = false;
    }

    /// A capture arrived from the native side (newest first, once).
    pub fn added(&mut self, c: Capture) {
        if let Some(l) = &mut self.captures {
            if !l.iter().any(|x| x.id == c.id) {
                l.insert(0, c);
            }
        }
    }

    pub fn is_fresh(&self, c: &Capture) -> bool {
        c.created_ms > self.seen_at
    }

    /// The loading bar (captures loading, a capture being taken, or a price on its way).
    pub fn loading(&self) -> bool {
        self.captures.is_none() || self.taking.is_some() || (self.selected_capture().is_some() && self.quote.is_none() && self.phase == Phase::Idle && self.error.is_none())
    }

    pub fn days_label(&self) -> &'static str {
        duration(self.duration).label
    }
}

/// Captures newer than the last visit to the Share screen.
pub fn new_count(captures: Option<&[Capture]>, seen_at: u64) -> usize {
    captures.unwrap_or(&[]).iter().filter(|c| c.created_ms > seen_at).count()
}

/// Why capturing isn't possible here, in one line, or None.
pub fn clips_note(st: Option<&ShareStatus>) -> Option<&'static str> {
    let st = st?;
    if !st.clips.available {
        return Some("Clips need gpu-screen-recorder or GStreamer with VA-API");
    }
    if !st.clips.enabled {
        return Some("Instant replay is off. Turn it on below.");
    }
    None
}

fn has(m: &str, needles: &[&str]) -> bool {
    let l = m.to_lowercase();
    needles.iter().any(|n| l.contains(n))
}

/// `/a.*b/i`: `b` somewhere after `a`.
fn then(l: &str, a: &str, b: &str) -> bool {
    l.find(a).is_some_and(|i| l[i + a.len()..].contains(b))
}

/// Wallet and network errors in plain words (store.ts plainShareError).
pub fn plain_share_error(m: &str, need: Option<u128>) -> String {
    let l = m.to_lowercase();
    let insufficient_wal = l.contains("not enough wal") || then(&l, "insufficient", "wal") || then(&l, "no", "wal coins") || l.contains("insufficientcoinbalance");
    if insufficient_wal {
        return match need.filter(|n| *n > 0) {
            Some(n) => format!("Not enough WAL in this account. Sharing this needs about {} WAL.", js_precision(n as f64 / 1e9, 2)),
            None => "Not enough WAL in this account.".into(),
        };
    }
    let insufficient_sui = l.contains("insufficientgas") || l.contains("gasbalancetoolow") || l.contains("not enough sui") || then(&l, "insufficient", "sui");
    if insufficient_sui {
        return "Not enough SUI in this account for the fees.".into();
    }
    if has(m, &["failed to fetch", "networkerror", "load failed", "timed out"]) {
        return "Couldn't reach Walrus. Check the connection and try again.".into();
    }
    if m.is_empty() {
        "That didn't go through. Try again.".into()
    } else {
        m.to_string()
    }
}

/// Number.prototype.toPrecision's text for numbers between 1e-6 and 1e21.
fn js_precision(v: f64, p: usize) -> String {
    if v == 0.0 {
        return format!("{:.*}", p - 1, 0.0);
    }
    let e = v.abs().log10().floor() as i32;
    let r = super::format::to_precision(v, p);
    let e = if r.abs() >= 10f64.powi(e + 1) { e + 1 } else { e };
    let dp = (p as i32 - 1 - e).max(0) as usize;
    format!("{r:.dp$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_share::{CaptureGame, ClipStatus, Kind, ShotStatus};

    fn cap(id: &str, kind: Kind, created: u64) -> Capture {
        Capture { id: id.into(), kind, path: String::new(), mime: String::new(), bytes: 1, width: 1920, height: 1080, duration_ms: None, created_ms: created, game: Some(CaptureGame { id: "web:act".into(), name: "ACT".into() }) }
    }

    fn status(available: bool, enabled: bool) -> ShareStatus {
        ShareStatus {
            session: Default::default(),
            screenshot: ShotStatus { available: true, method: None },
            clips: ClipStatus { available, enabled, recording: enabled, method: None, seconds: 30 },
            dir: String::new(),
        }
    }

    #[test]
    fn clips_note_says_why_clips_are_off() {
        assert_eq!(clips_note(None), None);
        assert_eq!(clips_note(Some(&status(false, false))), Some("Clips need gpu-screen-recorder or GStreamer with VA-API"));
        assert_eq!(clips_note(Some(&status(true, false))), Some("Instant replay is off. Turn it on below."));
        assert_eq!(clips_note(Some(&status(true, true))), None);
    }

    #[test]
    fn new_count_counts_captures_since_the_last_visit() {
        let l = [cap("shot-1", Kind::Screenshot, 10), cap("shot-2", Kind::Screenshot, 20), cap("clip-3", Kind::Clip, 30)];
        assert_eq!(new_count(Some(&l), 15), 2);
        assert_eq!(new_count(None, 0), 0);
    }

    #[test]
    fn plain_errors() {
        assert_eq!(plain_share_error("InsufficientCoinBalance in command 0", Some(19_300_000)), "Not enough WAL in this account. Sharing this needs about 0.019 WAL.");
        assert_eq!(plain_share_error("Not enough WAL", None), "Not enough WAL in this account.");
        assert_eq!(plain_share_error("GasBalanceTooLow", None), "Not enough SUI in this account for the fees.");
        assert_eq!(plain_share_error("TypeError: Failed to fetch", None), "Couldn't reach Walrus. Check the connection and try again.");
        assert_eq!(plain_share_error("", None), "That didn't go through. Try again.");
        assert_eq!(plain_share_error("Something else", None), "Something else");
    }

    #[test]
    fn the_share_flow_in_state() {
        let mut s = ShareState { captures: Some(vec![cap("shot-1", Kind::Screenshot, 10)]), ..Default::default() };
        assert!(s.select(Some("shot-1")));
        assert!(s.loading(), "pricing");
        assert!(s.share_blocked());
        s.priced("shot-1", 446_000, Some(CostEstimate { wal_frost: 19_300_000, tip_mist: 5_000_000, gas_mist: 10_000_000 }), None);
        assert_eq!(s.quote.as_ref().map(|q| (q.epochs, q.days)), Some((3, 42)));
        assert!(s.share_blocked(), "no wallet");
        assert!(!s.begin_share());
        assert_eq!(s.phase, Phase::Error("Add a Rare Wallet account to share.".into()));
        s.address = Some("0x1".into());
        assert!(!s.share_blocked());
        assert!(s.begin_share());
        assert_eq!(s.phase, Phase::Uploading(Step::Encode));
        assert!(!s.select(None), "B is blocked while uploading");
        assert!(!s.set_duration(DurationId::Year));
        s.finished(Ok((ShareResult { quilt_id: "q".into(), blob_object_id: None, link: "https://rare.market/s/q".into() }, true)));
        assert!(matches!(s.phase, Phase::Done { copied: true, .. }));
        assert!(s.select(None));
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn duration_changes_reprice_and_x_cycles() {
        let mut s = ShareState::default();
        assert_eq!(s.next_duration(), DurationId::Year);
        assert!(s.set_duration(DurationId::Year));
        assert!(!s.set_duration(DurationId::Year));
        assert_eq!(s.next_duration(), DurationId::Month);
    }

    #[test]
    fn rejected_signatures_go_back_to_idle() {
        let mut s = ShareState { phase: Phase::Uploading(Step::Register), ..Default::default() };
        s.finished(Err((true, "rejected".into())));
        assert_eq!(s.phase, Phase::Idle);
        s.finished(Err((false, "timed out".into())));
        assert_eq!(s.phase, Phase::Error("Couldn't reach Walrus. Check the connection and try again.".into()));
    }

    #[test]
    fn sponsor_covers_only_when_it_said_yes() {
        let mut s = ShareState { captures: Some(vec![cap("shot-1", Kind::Screenshot, 10)]), ..Default::default() };
        s.select(Some("shot-1"));
        s.payer = Payer::Rare;
        s.priced("shot-1", 1, None, Some(SponsorQuote { covered: false, ..Default::default() }));
        assert_eq!(s.payer, Payer::Me, "an uncovered quote puts the player back on paying");
        s.payer = Payer::Rare;
        s.priced("shot-1", 1, None, Some(SponsorQuote { covered: true, ..Default::default() }));
        assert!(s.sponsored());
        assert!(!s.share_blocked(), "free needs no wallet");
        assert!(s.begin_share());
        assert_eq!(s.phase, Phase::Uploading(Step::Upload));
    }

    #[test]
    fn removing_and_adding_captures() {
        let mut s = ShareState { captures: Some(vec![cap("shot-1", Kind::Screenshot, 10)]), ..Default::default() };
        s.select(Some("shot-1"));
        s.added(cap("clip-2", Kind::Clip, 20));
        s.added(cap("clip-2", Kind::Clip, 20));
        assert_eq!(s.captures.as_ref().unwrap().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["clip-2", "shot-1"]);
        s.removed("shot-1");
        assert_eq!(s.selected, None);
        assert_eq!(s.captures.as_ref().unwrap().len(), 1);
    }
}
