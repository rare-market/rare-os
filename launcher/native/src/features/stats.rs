//! Play stats from Rare's own session log (data/played.rs): playtime by week, month and year, top
//! games, streaks, and "Your year on Rare", a summary card that Share posts as a picture.
//!
//! Overlays: `stats` (StatsPage, LB/RB switch Week, Month, Year) and `year` (YearPage). The card
//! is captured from the frame: the page asks (`ask_card`), the app finds the card's rect after
//! paint and the main loop reads the frame back (`App::take_card_shot`, `card_captured`), the
//! picture is saved as a capture (rare_share::import_png) and Share opens on it.

use crate::act::Act;
use crate::data::model::{now_secs, Game};
use crate::data::played::Session;
use crate::kit::{self, s, tok, Btn as B, BtnIcon, TabItem, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::screens::game::parts::{self, Variant};
use crate::state::{Overlay, State, Tone};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::scene::{GradDir, Gradient};
use rare_render::view::{key, El, Look, Motion};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

pub const STATS: &str = "stats";
pub const YEAR: &str = "year";
/// The card's name in the view (the app looks it up to crop the frame).
pub const CARD: &str = "year-card";

const DAY: i64 = 86_400;

// ---------------------------------------------------------------- local days

/// Seconds east of UTC at `t` here.
pub fn utc_offset(t: i64) -> i64 {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    // SAFETY: localtime_r fills tm.
    unsafe { libc::localtime_r(&tt, &mut tm) };
    tm.tm_gmtoff as i64
}

/// (year, month 1..12, day 1..31) of a day number (days since 1970-01-01).
pub fn civil(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The day number of a civil date.
pub fn day_of_civil(y: i64, m: u32, d: u32) -> i64 {
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mi = m as i64;
    let doy = (153 * (mi + if mi > 2 { -3 } else { 9 }) + 2) / 5 + d as i64 - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// 0 Monday .. 6 Sunday.
pub fn weekday(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

// ---------------------------------------------------------------- the numbers

/// Seconds played per local day and game.
pub type Days = BTreeMap<i64, HashMap<String, i64>>;

/// Splits sessions at local midnights.
pub fn by_day(sessions: &[Session], off: &dyn Fn(i64) -> i64) -> Days {
    let mut out: Days = BTreeMap::new();
    for x in sessions {
        let mut t = x.s;
        let end = x.e.max(x.s);
        while t < end {
            let o = off(t);
            let day = (t + o).div_euclid(DAY);
            let next = (day + 1) * DAY - o;
            let upto = end.min(next.max(t + 1));
            *out.entry(day).or_default().entry(x.id.clone()).or_default() += upto - t;
            t = upto;
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Week,
    Month,
    Year,
}

impl Span {
    const ALL: [Span; 3] = [Span::Week, Span::Month, Span::Year];
    fn label(self) -> &'static str {
        match self {
            Span::Week => "This week",
            Span::Month => "This month",
            Span::Year => "This year",
        }
    }
    /// [first day, last day] of the span holding `today`, and of the one before it.
    pub fn range(self, today: i64) -> ((i64, i64), (i64, i64)) {
        match self {
            Span::Week => {
                let a = today - weekday(today) as i64;
                ((a, a + 6), (a - 7, a - 1))
            }
            Span::Month => {
                let (y, m, _) = civil(today);
                let a = day_of_civil(y, m, 1);
                let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                let b = day_of_civil(ny, nm, 1) - 1;
                let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
                ((a, b), (day_of_civil(py, pm, 1), a - 1))
            }
            Span::Year => {
                let (y, _, _) = civil(today);
                ((day_of_civil(y, 1, 1), day_of_civil(y, 12, 31)), (day_of_civil(y - 1, 1, 1), day_of_civil(y - 1, 12, 31)))
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub total: i64,
    pub before: i64,
    pub sessions: usize,
    /// Games by time, most first.
    pub games: Vec<(String, i64)>,
    /// One bar per day (week, month) or per month (year).
    pub bars: Vec<(String, i64)>,
    pub longest_session: Option<(String, i64)>,
    /// Days in a row with play, ending today or yesterday, and the longest in the span.
    pub streak: u32,
    pub best_streak: u32,
    pub busiest_month: Option<usize>,
    pub favourite_day: Option<usize>,
    /// The first day Rare counted.
    pub since: Option<i64>,
}

fn sum_range(days: &Days, a: i64, b: i64) -> i64 {
    days.range(a..=b).map(|(_, g)| g.values().sum::<i64>()).sum()
}

/// Days in a row with at least a minute, ending at `upto` (or the day before when `upto` has none).
pub fn streak_at(days: &Days, upto: i64) -> u32 {
    let played = |d: i64| days.get(&d).is_some_and(|g| g.values().sum::<i64>() >= 60);
    let mut d = if played(upto) { upto } else { upto - 1 };
    let mut n = 0;
    while played(d) {
        n += 1;
        d -= 1;
    }
    n
}

fn best_streak(days: &Days, a: i64, b: i64) -> u32 {
    let (mut best, mut run, mut last) = (0u32, 0u32, None::<i64>);
    for (d, g) in days.range(a..=b) {
        if g.values().sum::<i64>() < 60 {
            continue;
        }
        run = if last == Some(d - 1) { run + 1 } else { 1 };
        last = Some(*d);
        best = best.max(run);
    }
    best
}

pub fn summarize(sessions: &[Session], span: Span, now: i64, off: &dyn Fn(i64) -> i64) -> Summary {
    let days = by_day(sessions, off);
    let today = (now + off(now)).div_euclid(DAY);
    let ((a, b), (pa, pb)) = span.range(today);
    let mut games: HashMap<String, i64> = HashMap::new();
    for (_, g) in days.range(a..=b) {
        for (id, s) in g {
            *games.entry(id.clone()).or_default() += s;
        }
    }
    let mut games: Vec<(String, i64)> = games.into_iter().filter(|(_, s)| *s >= 60).collect();
    games.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
    let start_of = |d: i64| d * DAY - off(d * DAY);
    let (lo, hi) = (start_of(a), start_of(b + 1));
    let inside: Vec<&Session> = sessions.iter().filter(|x| x.e > lo && x.s < hi && x.e - x.s >= 60).collect();
    let longest_session = inside.iter().max_by_key(|x| x.e - x.s).map(|x| (x.id.clone(), x.e - x.s));
    let bars: Vec<(String, i64)> = match span {
        Span::Week => (0..7).map(|i| (DAYS[i][..3].to_string(), sum_range(&days, a + i as i64, a + i as i64))).collect(),
        Span::Month => (a..=b).map(|d| (civil(d).2.to_string(), sum_range(&days, d, d))).collect(),
        Span::Year => {
            let (y, _, _) = civil(today);
            (1..=12u32)
                .map(|m| {
                    let s = day_of_civil(y, m, 1);
                    let e = if m == 12 { day_of_civil(y + 1, 1, 1) - 1 } else { day_of_civil(y, m + 1, 1) - 1 };
                    (MONTHS[m as usize - 1][..3].to_string(), sum_range(&days, s, e))
                })
                .collect()
        }
    };
    let busiest_month = (span == Span::Year).then(|| bars.iter().enumerate().max_by_key(|(_, b)| b.1).filter(|(_, b)| b.1 > 0).map(|(i, _)| i)).flatten();
    let mut wd = [0i64; 7];
    for (d, g) in days.range(a..=b) {
        wd[weekday(*d)] += g.values().sum::<i64>();
    }
    let favourite_day = wd.iter().enumerate().max_by_key(|(_, s)| **s).filter(|(_, s)| **s > 0).map(|(i, _)| i);
    Summary {
        total: games.iter().map(|g| g.1).sum(),
        before: sum_range(&days, pa, pb),
        sessions: inside.len(),
        games,
        bars,
        longest_session,
        streak: streak_at(&days, today),
        // The streak going on now counts in full, even when it began before the span.
        best_streak: best_streak(&days, a, b).max(streak_at(&days, today)),
        busiest_month,
        favourite_day,
        since: days.keys().next().copied(),
    }
}

/// "12.4 h", "35 min", "0 min".
pub fn dur(secs: i64) -> String {
    let m = secs / 60;
    if m < 60 {
        format!("{m} min")
    } else {
        let h = secs as f64 / 3600.0;
        if h >= 100.0 {
            format!("{} h", int(h.round() as i64))
        } else {
            format!("{:.1} h", (h * 10.0).round() / 10.0)
        }
    }
}

fn int(n: i64) -> String {
    kit::format::grouped(n as f64, 0, 0)
}

fn days_word(n: u32) -> String {
    format!("{n} {}", if n == 1 { "day" } else { "days" })
}

/// Every session: the log plus the ones still running (fixtures: a sample year).
pub fn all_sessions(st: &State) -> Vec<Session> {
    if std::env::var("RR_FIXTURE").is_ok() {
        return fixture_sessions(st);
    }
    let now = now_secs();
    let mut v = crate::data::played::sessions();
    v.extend(st.played.open_sessions(now));
    v
}

fn title_of<'a>(st: &'a State, id: &'a str) -> &'a str {
    st.game(id).map(|g| g.title.as_str()).unwrap_or_else(|| id.split_once(':').map(|x| x.1).unwrap_or(id))
}

// ---------------------------------------------------------------- the stats page

pub struct StatsPage {
    span: Span,
    /// The log, read when the page opens (a running game's minutes come from `played`).
    log: Vec<Session>,
    /// The library version the log was read at (fixtures need the library's games).
    loaded: Option<u64>,
}

impl Default for StatsPage {
    fn default() -> StatsPage {
        StatsPage { span: Span::Week, log: Vec::new(), loaded: None }
    }
}

const ACT_YEAR: u32 = 1;
const ACT_GAME: u32 = 100;

impl Page for StatsPage {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.span as u8, self.loaded, self.log.len(), st.ver.library, st.ver.running))
    }

    fn tick(&mut self, st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        if st.library.is_some() && self.loaded != Some(st.ver.library) {
            self.loaded = Some(st.ver.library);
            self.log = all_sessions(st);
        }
        // A running game's minutes move on.
        (!st.running.is_empty()).then_some(now + 60.0)
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let now = now_secs();
        let mut log = self.log.clone();
        if std::env::var("RR_FIXTURE").is_err() {
            log.retain(|x| !st.played.recs.get(&x.id).is_some_and(|r| r.open == Some(x.s)));
            log.extend(st.played.open_sessions(now));
        }
        let sm = summarize(&log, self.span, now, &utc_offset);
        let since = sm.since.map(|d| {
            let (y, m, dd) = civil(d);
            let (ty, _, _) = civil((now + utc_offset(now)).div_euclid(DAY));
            if y == ty { format!("Counted by Rare since {} {dd}", &MONTHS[m as usize - 1][..3]) } else { format!("Counted by Rare since {} {dd}, {y}", &MONTHS[m as usize - 1][..3]) }
        });
        let head = ui
            .bx(s().row().items_center().gap(0.75).h(2.5).mb(1.0))
            .child(kit::icon(ui, "chart-line", 1.5, tok::ICON2))
            .child(ui.text("Your stats", T::new(Ty::Title).w(700)))
            .child(ui.text_s(s().ml_auto().shrink().min0(), since.unwrap_or_default(), T::new(Ty::Small).c(tok::MUTED).trunc()));
        let items: Vec<TabItem> = Span::ALL.iter().map(|s| TabItem::new(s.label())).collect();
        let tabs = kit::tabs(ui, s().mb(1.5), &items, Span::ALL.iter().position(|x| *x == self.span).unwrap_or(0), true);
        let mut page = ui.bx(s().col().fill().px(2.5).pt(2.0).pb(5.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key("stats-page")).child(head).child(tabs);
        if sm.since.is_none() {
            let lib = kit::button(ui, B::new("Go to Library", acts.tag(Act::Tab(crate::state::Tab::Library))).primary().def(true));
            return page.child(kit::empty(ui, s(), "No play counted yet", Some("Rare counts the time you spend in games it starts. Play something and your week shows here."), vec![lib]));
        }
        // The total and how it compares.
        let what = match self.span {
            Span::Week => "last week",
            Span::Month => "last month",
            Span::Year => "last year",
        };
        let diff = sm.total - sm.before;
        let cmp = if sm.before == 0 && sm.total == 0 {
            "Nothing played yet".to_string()
        } else if sm.before == 0 {
            format!("Nothing counted {what}")
        } else if diff.abs() < 60 {
            format!("The same as {what}")
        } else if diff > 0 {
            format!("{} more than {what}", dur(diff))
        } else {
            format!("{} less than {what}", dur(-diff))
        };
        let total = ui.bx(s().col().gap(0.2).mb(1.25)).child(ui.text(dur(sm.total), T::new(Ty::Px(3.0, 3.4)).w(700).tab())).child(ui.text(cmp, T::new(Ty::Small).c(tok::MUTED)));
        page = page.child(total);
        // Cards.
        let longest = sm.longest_session.as_ref().map(|(id, s)| (dur(*s), title_of(st, id).to_string()));
        let mut cards = vec![
            kit::info_card(ui, "Sessions", &int(sm.sessions as i64), None, None),
            kit::info_card(ui, "Games played", &sm.games.len().to_string(), None, None),
            kit::info_card(ui, "Streak", &days_word(sm.streak), Some(if sm.streak > 0 { "Days in a row, up to today" } else { "Play today to start one" }), None),
            kit::info_card(ui, "Longest streak", &days_word(sm.best_streak), Some(self.span.label()), None),
        ];
        if let Some((d, t)) = &longest {
            cards.push(kit::info_card(ui, "Longest session", d, Some(t), None));
        }
        page = page.child(ui.bx(s().row().wrap().gap(1.0).mb(1.5)).kids(cards));
        // Bars.
        page = page.child(bars(ui, &sm, self.span));
        // Top games.
        if !sm.games.is_empty() {
            page = page.child(ui.text_s(s().mt(1.75).mb(0.6), "Top games", T::new(Ty::Row).w(600)));
            let top = sm.games.first().map(|g| g.1).unwrap_or(1).max(1);
            let rows: Vec<El> = sm
                .games
                .iter()
                .take(10)
                .enumerate()
                .map(|(i, (id, secs))| {
                    let g = st.game(id);
                    let thumb = match g {
                        Some(g) => {
                            let src = if g.art.cover.is_empty() { &g.art.header } else { &g.art.cover };
                            parts::art(ui, src, &g.title, Variant::Cover, 2.4, 2.4, 0.25)
                        }
                        None => ui.bx(s().size(2.4)).bg(tok::L3),
                    };
                    let thumb = ui.bx(s().size(2.4)).radius(ui.r(0.35)).clip().child(thumb);
                    let share = ui.bx(s().h(0.3).wp(*secs as f32 / top as f32)).bg(tok::fg(0.55)).radius(ui.r(0.15));
                    let mid = ui.bx(s().col().grow().gap(0.35)).child(ui.text(title_of(st, id).to_string(), T::new(Ty::Body).w(600).trunc())).child(ui.bx(s().wp(1.0).h(0.3)).bg(tok::L3).radius(ui.r(0.15)).child(share));
                    let el = ui
                        .bx(s().row().items_center().gap(1.0).h(3.6).px(0.75).wp(1.0))
                        .radius(ui.r(0.5))
                        .child(ui.text_s(s().w(1.5), (i + 1).to_string(), T::new(Ty::Small).w(600).tab().c(tok::MUTED)))
                        .child(thumb)
                        .child(mid)
                        .child(ui.text_s(s().w(6.0), dur(*secs), T::new(Ty::Body).w(600).tab().right()));
                    let tag = acts.tag(Act::Local(ACT_GAME + i as u32));
                    kit::nav(kit::row_look(el), tag, if g.is_some() { Some("Open") } else { None }, None, i == 0 && self.span != Span::Year).key(key(id))
                })
                .collect();
            page = page.child(ui.bx(s().col().gap(0.25).max_w(56.0)).group(Group::new("stats-top", [0.0; 4]).remember()).kids(rows));
        }
        if self.span == Span::Year {
            let b = kit::button(ui, B::new("Your year on Rare", acts.tag(Act::Local(ACT_YEAR))).primary().def(true).icon(BtnIcon::Lucide("sparkles", 1.0, false)));
            page = page.child(ui.bx(s().row().mt(1.75)).child(b));
        }
        page
    }

    fn pad(&mut self, _st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        let i = Span::ALL.iter().position(|x| *x == self.span).unwrap_or(0);
        let j = match b {
            Btn::Lb if i > 0 => i - 1,
            Btn::Rb if i + 1 < Span::ALL.len() => i + 1,
            Btn::Lb | Btn::Rb => return true,
            _ => return false,
        };
        self.span = Span::ALL[j];
        fx.fb(Feedback::Move);
        true
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        if code == ACT_YEAR {
            fx.fb(Feedback::Open);
            st.open(Some(Overlay::Other(YEAR)));
            return;
        }
        if let Some(i) = code.checked_sub(ACT_GAME) {
            let now = now_secs();
            let mut log = self.log.clone();
            log.extend(st.played.open_sessions(now));
            let sm = summarize(&log, self.span, now, &utc_offset);
            if let Some((id, _)) = sm.games.get(i as usize).filter(|(id, _)| st.game(id).is_some()) {
                fx.fb(Feedback::Open);
                st.open(Some(Overlay::Game(id.clone())));
            }
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        vec![hint(Key::LbRb, "Week, month, year"), hint(Key::B, "Back")]
    }
}

fn bars(ui: &mut Ui, sm: &Summary, span: Span) -> El {
    let top = sm.bars.iter().map(|b| b.1).max().unwrap_or(0).max(60);
    let h = 8.0;
    let many = sm.bars.len() > 12;
    let cols: Vec<El> = sm
        .bars
        .iter()
        .enumerate()
        .map(|(i, (label, secs))| {
            let frac = *secs as f32 / top as f32;
            let bar_h = if *secs > 0 { (h * frac).max(0.3) } else { 0.15 };
            let bar = ui.bx(s().wp(if many { 0.7 } else { 0.55 }).h(bar_h)).bg(if *secs > 0 { tok::fg(0.75) } else { tok::L3 }).radius(ui.r(0.2));
            let show = !many || i % 5 == 0 || i + 1 == sm.bars.len();
            let lab = ui.text_s(s().h(1.2), if show { label.clone() } else { String::new() }, T::new(Ty::Label).c(tok::MUTED).tab().center());
            ui.bx(s().col().items_center().justify_end().grow().gap(0.4)).child(ui.bx(s().col().justify_end().items_center().h(h).wp(1.0)).child(bar)).child(lab).key(key(&(span as u8, i)))
        })
        .collect();
    let peak = ui.text(format!("Most in a {}: {}", if span == Span::Year { "month" } else { "day" }, dur(sm.bars.iter().map(|b| b.1).max().unwrap_or(0))), T::new(Ty::Small).c(tok::MUTED));
    kit::panel(ui, s().col().gap(0.9).p(1.25).max_w(56.0)).child(peak).child(ui.bx(s().row().items_end().gap(if many { 0.15 } else { 0.5 }).wp(1.0)).kids(cols))
}

// ---------------------------------------------------------------- Your year on Rare

static CARD_REQ: Mutex<bool> = Mutex::new(false);
static CARD_DONE: Mutex<Option<Result<String, String>>> = Mutex::new(None);

/// The page wants its card as a picture (the app reads the frame after the next paint).
fn ask_card() {
    *CARD_REQ.lock().unwrap() = true;
}

/// The app: a card was asked for.
pub fn card_wanted() -> bool {
    std::mem::take(&mut *CARD_REQ.lock().unwrap())
}

/// The main loop read the frame back: crop the card, save it as a capture (on a thread).
pub fn card_captured(w: u32, h: u32, px: Vec<u8>, rect: [f32; 4]) {
    std::thread::spawn(move || {
        let r = (|| -> Result<String, String> {
            let x0 = rect[0].max(0.0).round() as u32;
            let y0 = rect[1].max(0.0).round() as u32;
            let cw = (rect[2].round() as u32).min(w.saturating_sub(x0));
            let ch = (rect[3].round() as u32).min(h.saturating_sub(y0));
            if cw < 16 || ch < 16 {
                return Err("The card was not on screen".into());
            }
            let mut out = Vec::with_capacity((cw * ch * 4) as usize);
            for y in y0..y0 + ch {
                let a = ((y * w + x0) * 4) as usize;
                out.extend_from_slice(&px[a..a + (cw * 4) as usize]);
            }
            let img = image::RgbaImage::from_raw(cw, ch, out).ok_or("bad frame")?;
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
            if let Ok(p) = std::env::var("RR_CARD_OUT") {
                let _ = std::fs::write(&p, &png);
            }
            let c = rare_share::import_png(&png, None)?;
            Ok(c.id)
        })();
        if let Err(e) = &r {
            log::warn!("year card: {e}");
        }
        *CARD_DONE.lock().unwrap() = Some(r);
    });
}

#[derive(Default)]
pub struct YearPage {
    log: Vec<Session>,
    loaded: Option<u64>,
    /// Share pressed, waiting for the picture.
    sharing: bool,
}

impl Page for YearPage {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.loaded, self.log.len(), self.sharing, st.ver.library))
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, now: f64) -> Option<f64> {
        if st.library.is_some() && self.loaded != Some(st.ver.library) {
            self.loaded = Some(st.ver.library);
            self.log = all_sessions(st);
        }
        if !self.sharing {
            return None;
        }
        match CARD_DONE.lock().unwrap().take() {
            None => Some(now + 0.1),
            Some(Ok(id)) => {
                self.sharing = false;
                crate::screens::game::extras::share_capture(id, "overlay:year".into());
                fx.run(Act::Open(Overlay::Other("share")));
                None
            }
            Some(Err(_)) => {
                self.sharing = false;
                st.toast("Couldn't make the picture. Try again.", Tone::Error, now);
                None
            }
        }
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let now = now_secs();
        let sm = summarize(&self.log, Span::Year, now, &utc_offset);
        let (year, _, _) = civil((now + utc_offset(now)).div_euclid(DAY));
        let (w, h) = (ui.w / ui.rem, ui.h / ui.rem);
        // The card: 16:9, as big as the screen allows under the title and above the buttons.
        let cw = (w - 5.0).min((h - 11.5) * 16.0 / 9.0);
        let chh = cw * 9.0 / 16.0;
        let card = year_card(ui, st, &sm, year, cw, chh);
        let mut btns = Vec::new();
        if sm.since.is_some() {
            btns.push(kit::button(ui, B::new(if self.sharing { "Making the picture" } else { "Share" }, acts.tag(Act::Local(1))).primary().def(true).disabled(self.sharing).icon(BtnIcon::Lucide("share-2", 1.0, false)).hint("Share").key(1)));
        }
        btns.push(kit::button(ui, B::new("Back", acts.tag(Act::Close)).ghost().def(sm.since.is_none()).key(2)));
        let row = ui.bx(s().row().gap(0.75).mt(1.25)).group(Group::new("year-actions", [0.0; 4])).kids(btns);
        // Above the hint bar (3rem).
        ui.bx(s().fill().col().items_center().justify_center().px(2.5).pb(3.0)).bg(tok::BG).enter(Motion::FADE).child(card).child(row)
    }

    fn act(&mut self, _st: &mut State, fx: &mut Fx, code: u32) {
        if code == 1 && !self.sharing {
            self.sharing = true;
            *CARD_DONE.lock().unwrap() = None;
            fx.fb(Feedback::Open);
            ask_card();
        }
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if b == Btn::Y && !self.log.is_empty() {
            self.act(st, fx, 1);
            return true;
        }
        false
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        if self.log.is_empty() {
            return vec![hint(Key::B, "Back")];
        }
        vec![hint(Key::Y, "Share"), hint(Key::B, "Back")]
    }
}

fn year_card(ui: &mut Ui, st: &State, sm: &Summary, year: i64, w: f32, h: f32) -> El {
    // Scale the card's type with its width so 800p and 1200p read the same.
    let k = (w / 60.0).clamp(0.7, 1.6);
    let top = sm.games.first().and_then(|(id, _)| st.game(id));
    let mut card = ui.bx(s().wh(w, h)).bg(tok::L1).radius(ui.r(1.0)).clip().name(CARD);
    if let Some(g) = top {
        let src = if g.art.hero.is_empty() { &g.art.header } else { &g.art.hero };
        let art = parts::art(ui, src, &g.title, Variant::Hero, w, h, 0.25);
        card = card.child(ui.bx(s().fill()).child(art)).child(ui.bx(s().fill()).look(Look::default().grad(Gradient { dir: GradDir::ToRight, stops: [tok::BG.alpha(0.97), tok::BG.alpha(0.85), tok::BG.alpha(0.55)], mid: 0.55 })));
    }
    let label = T::new(Ty::Px(0.8 * k, 1.1 * k)).w(600).c(tok::MUTED).track(0.08);
    let mut left = ui.bx(s().col().gap(0.3 * k).w(w * 0.46).min0()).child(ui.text(format!("YOUR {year} ON RARE"), label));
    if sm.since.is_none() {
        left = left.child(ui.text_s(s().mt(1.0 * k).wp(1.0), "No play counted yet this year", T::new(Ty::Px(2.0 * k, 2.5 * k)).w(700).wrap()));
    } else {
        let hours = sm.total as f64 / 3600.0;
        let big = if hours >= 1.0 { format!("{} {}", int(hours.round() as i64), if hours.round() as i64 == 1 { "hour" } else { "hours" }) } else { dur(sm.total) };
        left = left
            .child(ui.text_s(s().mt(0.6 * k).wp(1.0), big, T::new(Ty::Px(4.2 * k, 4.8 * k)).w(800).tab().trunc()))
            .child(ui.text_s(s().wp(1.0), format!("across {} {} and {} {}", sm.games.len(), if sm.games.len() == 1 { "game" } else { "games" }, int(sm.sessions as i64), if sm.sessions == 1 { "session" } else { "sessions" }), T::new(Ty::Px(1.05 * k, 1.5 * k)).c(tok::fg(0.8)).trunc()));
        let fact = |ui: &mut Ui, l: &str, v: String| ui.bx(s().col().gap(0.15 * k).w(w * 0.2).min0()).child(ui.text(l, T::new(Ty::Px(0.75 * k, 1.05 * k)).c(tok::MUTED).trunc())).child(ui.text(v, T::new(Ty::Px(1.15 * k, 1.5 * k)).w(700).trunc()));
        let mut facts = vec![fact(ui, "Longest streak", days_word(sm.best_streak))];
        if let Some(m) = sm.busiest_month {
            facts.push(fact(ui, "Busiest month", MONTHS[m].to_string()));
        }
        if let Some(d) = sm.favourite_day {
            facts.push(fact(ui, "Favourite day", DAYS[d].to_string()));
        }
        if let Some((_, s)) = &sm.longest_session {
            facts.push(fact(ui, "Longest session", dur(*s)));
        }
        left = left.child(ui.bx(s().row().wrap().gap_xy(1.2 * k, 1.0 * k).mt(1.6 * k).wp(1.0)).kids(facts));
    }
    left = left.child(ui.text_s(s().mt_auto(), "rare.market", T::new(Ty::Px(0.85 * k, 1.2 * k)).w(600).c(tok::MUTED)));
    let mut right = ui.bx(s().col().gap(0.55 * k).w(w * 0.38).min0());
    if !sm.games.is_empty() {
        right = right.child(ui.text_s(s().mb(0.2 * k), "Most played", label));
        for (i, (id, secs)) in sm.games.iter().take(5).enumerate() {
            let g = st.game(id);
            let thumb = match g {
                Some(g) => {
                    let src = if g.art.cover.is_empty() { &g.art.header } else { &g.art.cover };
                    parts::art(ui, src, &g.title, Variant::Cover, 2.6 * k, 2.6 * k, 0.25)
                }
                None => ui.bx(s().size(2.6 * k)).bg(tok::L3),
            };
            let row = ui
                .bx(s().row().items_center().gap(0.7 * k).wp(1.0))
                .child(ui.text_s(s().w(1.1 * k), (i + 1).to_string(), T::new(Ty::Px(1.0 * k, 1.3 * k)).w(700).tab().c(tok::MUTED)))
                .child(ui.bx(s().size(2.6 * k)).radius(ui.r(0.35 * k)).clip().child(thumb))
                .child(ui.bx(s().col().grow()).child(ui.text(title_of(st, id).to_string(), T::new(Ty::Px(1.0 * k, 1.35 * k)).w(600).trunc())).child(ui.text(dur(*secs), T::new(Ty::Px(0.8 * k, 1.1 * k)).c(tok::MUTED).tab())))
                .key(key(id));
            right = right.child(row);
        }
    }
    card.child(ui.bx(s().fill().row().justify_between().p(2.2 * k)).child(left).child(right))
}

// ---------------------------------------------------------------- fixtures

/// A sample year for the shots (RR_KNOW=none: nothing; one: a single session; long: a long title
/// and huge numbers).
fn fixture_sessions(st: &State) -> Vec<Session> {
    let sc = crate::data::knowledge::scenario();
    let ids: Vec<String> = st.games().iter().filter(|g: &&Game| g.source != "apps").map(|g| g.id.clone()).take(7).collect();
    if sc == "none" || ids.is_empty() {
        return Vec::new();
    }
    let now = now_secs();
    if sc == "one" {
        return vec![Session { id: ids[0].clone(), s: now - 5400, e: now - 1800 }];
    }
    let mut v = Vec::new();
    let mut seed: u64 = 42;
    let mut rnd = |n: u64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) % n.max(1)
    };
    let scale = if sc == "long" { 9 } else { 1 };
    for d in (0..300).rev() {
        if rnd(10) < 3 && d > 6 {
            continue;
        }
        let n = 1 + rnd(3);
        for k in 0..n {
            let id = &ids[(rnd(100) as usize * rnd(100) as usize / 100).min(ids.len() - 1)];
            let start = now - d * DAY - 3600 * (2 + k as i64 * 3) - rnd(1800) as i64;
            let len = (900 + rnd(9000) as i64) * scale;
            v.push(Session { id: id.clone(), s: start, e: (start + len).min(now) });
        }
    }
    v.retain(|x| x.e > x.s);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ses(id: &str, s: i64, e: i64) -> Session {
        Session { id: id.into(), s, e }
    }

    #[test]
    fn days_streaks_and_spans() {
        let utc = |_t: i64| 0i64;
        // 2026-10-27 is a Tuesday.
        let today = day_of_civil(2026, 10, 27);
        assert_eq!(civil(today), (2026, 10, 27));
        assert_eq!(weekday(today), 1);
        let t = |d: i64, h: i64| (today + d) * DAY + h * 3600;
        // A session over midnight splits between two days.
        let days = by_day(&[ses("a", t(-1, 23), t(0, 1))], &utc);
        assert_eq!(days[&(today - 1)]["a"], 3600);
        assert_eq!(days[&today]["a"], 3600);
        let log = vec![ses("a", t(-1, 23), t(0, 1)), ses("b", t(-2, 10), t(-2, 12)), ses("a", t(-9, 10), t(-9, 11)), ses("c", t(0, 5), t(0, 5) + 30)];
        let w = summarize(&log, Span::Week, t(0, 12), &utc);
        // This week: Mon (today - 1) and Tue; Sunday (today - 2) is last week.
        assert_eq!((w.total, w.before, w.sessions), (7200, 7200 + 0, 1));
        assert_eq!(w.games, vec![("a".to_string(), 7200)]);
        assert_eq!(w.bars[0], ("Mon".to_string(), 3600));
        assert_eq!((w.streak, w.best_streak), (3, 3));
        let y = summarize(&log, Span::Year, t(0, 12), &utc);
        assert_eq!(y.busiest_month, Some(9));
        assert_eq!(y.bars.len(), 12);
        let m = summarize(&log, Span::Month, t(0, 12), &utc);
        assert_eq!(m.bars.len(), 31);
        assert_eq!(dur(7200), "2.0 h");
        assert_eq!(dur(1_000_000), "278 h");
        assert_eq!(dur(59), "0 min");
    }
}
