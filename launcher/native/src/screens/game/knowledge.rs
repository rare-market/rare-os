//! What the game page knows about a game beyond the library: HowLongToBeat times, Metacritic and
//! Steam review scores, and Runs on SuiPlay (players tag Great, Playable or Broken with the
//! settings they played with; rare.market keeps the reports and the page shows the settings most
//! Great reports used, with a button to use them). Read off the UI thread from rare.market and
//! kept (data/knowledge.rs), so a revisit shows the last answer at once. The cards go at the end of
//! the card row and the Runs section under the fold, so nothing above moves when they land.

use crate::act::Act;
use rare_i18n::{tr, trf, trn};
use crate::data::knowledge::{self as kn, GameInfo, RunSettings, Runs};
use crate::data::model::Game;
use crate::kit::{self, s, tok, Btn as B, BtnIcon, Ty, Ui, T};
use crate::page::{Acts, Btn, Feedback, Fx};
use crate::state::{State, Tone};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::Group;
use rare_render::view::{key, El};
use rare_system::power::GamePower;
use rare_system::system::Profile;
use rare_system::upscale::{Mode, Upscale};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// `Act::Local` codes (checked before extras' 10_000.. range).
pub const BASE: u32 = 90_000;
const OPEN_SHEET: u32 = BASE;
const RATE: u32 = BASE + 10; // + 0 great, 1 playable, 2 broken
const USE_BEST: u32 = BASE + 20;
const HLTB: u32 = BASE + 21;
const CLOSE: u32 = BASE + 22;
pub const END: u32 = BASE + 100;
const SCOPE: u32 = 5;

fn ratings() -> [(&'static str, &'static str, &'static str); 3] {
    let g = &tr().game;
    [("great", g.verdict_great, g.rating_great), ("playable", g.verdict_playable, g.rating_playable), ("broken", g.verdict_broken, g.rating_broken)]
}

#[derive(Default)]
struct Inner {
    info: Option<GameInfo>,
    /// The settings this device would report (read when the sheet opens).
    mine: Option<RunSettings>,
    /// A report on its way.
    sending: Option<&'static str>,
    /// What this device said last (rating, when).
    said: Option<(String, i64)>,
}

#[derive(Default)]
pub struct Knowledge {
    inner: Arc<Mutex<Inner>>,
    ver: Arc<AtomicU64>,
    sheet: bool,
}

fn steam_appid(g: &Game) -> Option<u32> {
    (g.source == "steam" && !g.foreign).then(|| g.id.strip_prefix("steam:")?.parse().ok()).flatten().filter(|a: &u32| *a < 0x8000_0000)
}

/// Games worth asking about: not apps, emulators' frontends, web or cloud entries.
fn asks(g: &Game) -> bool {
    !matches!(g.source.as_str(), "apps" | "web" | "cloud") && !g.title.trim().is_empty()
}

impl Knowledge {
    pub fn ver(&self) -> u64 {
        self.ver.load(Ordering::Relaxed) + self.sheet as u64 * 1_000_000_007
    }

    fn bump(v: &AtomicU64) {
        v.fetch_add(1, Ordering::Relaxed);
    }

    pub fn sheet_open(&self) -> bool {
        self.sheet
    }

    pub fn load(&self, fx: &mut Fx, g: &Game) {
        if !asks(g) {
            return;
        }
        let steam = steam_appid(g);
        let key = kn::game_key(steam, &g.title);
        let stale = {
            let mut i = self.inner.lock().unwrap();
            let c = kn::cached_game(&g.id);
            let fresh = c.as_ref().is_some_and(|(at, _)| crate::data::model::now_secs() - at < 6 * 3600);
            i.info = c.map(|c| c.1);
            i.said = key.as_deref().and_then(kn::my_report);
            !fresh
        };
        Self::bump(&self.ver);
        if !stale {
            return;
        }
        let (inner, ver, id, title) = (self.inner.clone(), self.ver.clone(), g.id.clone(), g.title.clone());
        fx.spawn(move |_| {
            match kn::game_info(&id, steam, &title) {
                Ok(info) => inner.lock().unwrap().info = Some(info),
                Err(e) => log::warn!("game knowledge {id}: {e}"),
            }
            Self::bump(&ver);
            Box::new(|_, _| {})
        });
    }

    /// How long to beat and Reviews, for the section's wrapping row (empty until the answer lands).
    fn cards(&self, ui: &mut Ui, acts: &mut Acts, g: &Game, info: &GameInfo) -> Vec<El> {
        let mut out = Vec::new();
        if let Some(h) = info.hltb.as_ref() {
            let parts: Vec<(String, &str)> = [(h.main, tr().game.hltb_main), (h.extras, tr().game.hltb_extras), (h.completionist, tr().game.hltb_all)].iter().filter_map(|(v, l)| v.map(|v| (hours(v), *l))).collect();
            if !parts.is_empty() {
                let tag = acts.game(Act::Local(HLTB), &g.id);
                let groups: Vec<El> = parts
                    .into_iter()
                    .map(|(v, l)| ui.bx(s().col().grow().min0()).child(ui.text(v, T::new(Ty::Title).w(600).tab().trunc())).child(ui.text_s(s().mt(0.15), l, T::new(Ty::Meta).c(tok::MUTED).trunc())).key(key(l)))
                    .collect();
                let c = ui
                    .bx(s().col().h(6.0).w(24.0).px(1.0).py(0.875))
                    .bg(tok::L1)
                    .radius(ui.r(0.5))
                    .child(ui.text(tr().game.how_long, T::new(Ty::Label).w(500).c(tok::MUTED)))
                    .child(ui.bx(s().row().mt_auto().gap(1.0).wp(1.0).min0()).kids(groups));
                out.push(kit::nav(kit::tile(c, 1.03), tag, Some("HowLongToBeat"), None, false).key(key("kn-hltb")));
            }
        }
        if let Some(sc) = info.score.as_ref() {
            let card = match (sc.metacritic, sc.steam_pct) {
                (Some(m), Some(p)) => Some((trf!(tr().game.metacritic, m = m), trf!(tr().game.positive_steam, p = p))),
                (Some(m), None) => Some((trf!(tr().game.metacritic, m = m), tr().game.critics_score.to_string())),
                (None, Some(p)) => Some((trf!(tr().game.positive, p = p), sc.steam_label.clone().map(|l| trf!(tr().game.label_on_steam, label = l)).unwrap_or_else(|| tr().game.steam_reviews.into()))),
                _ => None,
            };
            if let Some((v, sub)) = card {
                out.push(kit::info_card(ui, tr().game.reviews, &v, Some(&sub), None).key(key("kn-score")));
            }
        }
        out
    }

    /// The Runs on SuiPlay section under the fold (None until the answer lands or for apps).
    pub fn section(&self, ui: &mut Ui, acts: &mut Acts, g: &Game) -> Option<El> {
        let i = self.inner.lock().unwrap();
        let info = i.info.as_ref()?;
        let runs = info.runs.clone().unwrap_or_default();
        let mut col = ui.bx(s().col().gap(0.9).px(2.5)).section();
        // How long to beat and Reviews wrap onto more lines when the page is narrow.
        let cards = self.cards(ui, acts, g, info);
        if !cards.is_empty() {
            col = col.child(kit::section_head(ui, s(), tr().game.about_game, None, None)).child(ui.bx(s().row().wrap().gap(1.0).mb(1.5).wp(1.0)).group(Group::new("kn-cards", [0.0; 4])).kids(cards));
        }
        col = col.child(kit::section_head(ui, s(), tr().game.runs_on_suiplay, Some(&if runs.total > 0 { trn!(tr().game.reports_n, runs.total, n = int(runs.total as i64)) } else { tr().game.no_reports.to_string() }), None));
        let mut panel = kit::panel(ui, s().col().gap(1.0).p(1.25).max_w(56.0));
        if runs.total > 0 {
            panel = panel.child(split_bar(ui, &runs));
        } else {
            panel = panel.child(ui.text_s(s().wp(1.0), tr().game.nobody_said, T::new(Ty::Small).c(tok::fg(0.8)).wrap()));
        }
        if let Some(b) = runs.best.as_ref().filter(|b| !b.settings.is_empty()) {
            let who = trn!(tr().game.best_from, b.count, n = int(b.count as i64), verdict = verdict_word(Some(&b.from)));
            panel = panel.child(ui.bx(s().col().gap(0.25).wp(1.0)).child(ui.text(who, T::new(Ty::Label).w(500).c(tok::MUTED))).child(ui.text_s(s().wp(1.0), b.settings.line(), T::new(Ty::Body).w(600).wrap())));
        }
        let mut btns = Vec::new();
        let said = i.said.as_ref().map(|(r, _)| verdict_word(Some(r)).to_string());
        btns.push(kit::button(ui, B::new(if said.is_some() { tr().game.change_report } else { tr().game.report_how }, acts.game(Act::Local(OPEN_SHEET), &g.id)).icon(BtnIcon::Lucide("message-circle", 1.0, false))));
        if runs.best.as_ref().is_some_and(|b| b.settings.power.is_some()) {
            btns.push(kit::button(ui, B::new(tr().game.use_settings, acts.game(Act::Local(USE_BEST), &g.id)).ghost().hint(tr().game.use_settings_hint)));
        }
        let mut row = ui.bx(s().row().items_center().gap(0.75)).group(Group::new("kn-runs", [0.0; 4])).kids(btns);
        if let Some(s_) = said {
            row = row.child(ui.text_s(s().ml(0.5), trf!(tr().game.you_said, verdict = s_), T::new(Ty::Small).c(tok::MUTED)));
        }
        panel = panel.child(row);
        Some(col.child(panel).key(key("kn-section")))
    }

    /// The report sheet over the page.
    pub fn sheet(&self, ui: &mut Ui, acts: &mut Acts, g: &Game) -> Option<El> {
        if !self.sheet {
            return None;
        }
        let i = self.inner.lock().unwrap();
        let said = i.said.as_ref().map(|s| s.0.clone());
        let mut body: Vec<El> = Vec::new();
        for (n, (id, label, line)) in ratings().iter().enumerate() {
            let busy = i.sending == Some(*id);
            let mut lab = ui.bx(s().col().gap(0.15).grow()).child(ui.text(*label, T::new(Ty::Row).w(600))).child(ui.text(*line, T::new(Ty::Small).c(tok::MUTED).trunc()));
            if busy {
                lab = lab.child(ui.text(tr().game.sending, T::new(Ty::Meta).c(tok::MUTED)));
            }
            let mut el = ui.bx(s().row().items_center().gap(1.0).h(4.25).px(1.0).wp(1.0)).radius(ui.r(0.5)).bg(tok::L1).child(dot(ui, id)).child(lab);
            if said.as_deref() == Some(*id) {
                el = el.child(ui.text(tr().game.your_report, T::new(Ty::Small).c(tok::MUTED)));
            }
            let tag = acts.game(Act::Local(RATE + n as u32), &g.id);
            body.push(kit::nav(kit::row_look(el), tag, Some(tr().common.send), None, said.as_deref().map_or(n == 0, |s| s == *id)).key(key(id)));
        }
        let mine = i.mine.as_ref().map(|m| m.line()).filter(|l| !l.is_empty()).unwrap_or_else(|| tr().game.reading_settings.into());
        body.push(ui.text_s(s().mt(0.75).wp(1.0), trf!(tr().game.sent_with, settings = mine), T::new(Ty::Small).c(tok::MUTED).wrap()));
        let footer = vec![kit::button(ui, B::new(tr().common.cancel, acts.game(Act::Local(CLOSE), &g.id)).ghost())];
        Some(kit::sheet(ui, tr().game.how_runs, Some(&trf!(tr().game.on_this_suiplay, game = g.title)), body, footer, false, SCOPE).group(Group::new("kn-sheet", [0.0; 4])))
    }

    pub fn act(&mut self, st: &mut State, fx: &mut Fx, g: &Game, code: u32) {
        let now = rare_render::clock::secs();
        match code {
            OPEN_SHEET => {
                self.sheet = true;
                fx.fb(Feedback::Open);
                let (inner, ver, id, appid) = (self.inner.clone(), self.ver.clone(), g.id.clone(), steam_appid(g));
                fx.spawn(move |b| {
                    let s = settings_now(b, &id, appid);
                    inner.lock().unwrap().mine = Some(s);
                    Self::bump(&ver);
                    Box::new(|_, _| {})
                });
            }
            CLOSE => {
                self.sheet = false;
                fx.fb(Feedback::Back);
            }
            HLTB => {
                let url = self.inner.lock().unwrap().info.as_ref().and_then(|i| i.hltb.as_ref()).map(|h| h.url.clone());
                if let Some(u) = url.filter(|u| !u.is_empty()) {
                    st.toast(tr().game.opening_hltb, Tone::Info, now);
                    fx.run(Act::OpenWeb(u));
                }
            }
            USE_BEST => {
                let best = self.inner.lock().unwrap().info.as_ref().and_then(|i| i.runs.as_ref()).and_then(|r| r.best.clone());
                let Some(b) = best else { return };
                let id = g.id.clone();
                fx.fb(Feedback::Toggle);
                fx.spawn(move |be| {
                    let cur = be.game_perf(&id);
                    let p = apply(cur.saved.unwrap_or(GamePower::DEFAULT), cur.global_upscale, &b.settings);
                    let r = be.set_game_perf(&id, Some(p));
                    Box::new(move |st: &mut State, now| match r {
                        Ok(()) => st.toast(tr().game.using_settings, Tone::Info, now),
                        Err(e) => {
                            log::warn!("use best settings: {e}");
                            st.toast(tr().game.settings_save_failed, Tone::Error, now);
                        }
                    })
                });
            }
            c if (RATE..RATE + 3).contains(&c) => {
                let (rating, _, _) = ratings()[(c - RATE) as usize];
                let (key, settings) = {
                    let i = self.inner.lock().unwrap();
                    if i.sending.is_some() {
                        return;
                    }
                    (i.info.as_ref().and_then(|x| x.key.clone()).or_else(|| kn::game_key(steam_appid(g), &g.title)), i.mine.clone().unwrap_or_default())
                };
                let Some(key) = key else { return };
                self.inner.lock().unwrap().sending = Some(rating);
                Self::bump(&self.ver);
                let (inner, ver, id, title) = (self.inner.clone(), self.ver.clone(), g.id.clone(), g.title.clone());
                fx.spawn(move |_| {
                    let r = kn::report(&key, &title, rating, &settings);
                    {
                        let mut i = inner.lock().unwrap();
                        i.sending = None;
                        if let Ok(runs) = &r {
                            i.said = Some((rating.to_string(), crate::data::model::now_secs()));
                            if let Some(info) = i.info.as_mut() {
                                info.runs = Some(runs.clone());
                            }
                        }
                    }
                    if let Ok(runs) = &r {
                        kn::set_my_report(&key, rating);
                        kn::put_runs(&id, runs.clone());
                    }
                    Self::bump(&ver);
                    Box::new(move |st: &mut State, now| match r {
                        Ok(_) => st.toast(tr().game.report_thanks, Tone::Info, now),
                        Err(e) => st.toast(e, Tone::Error, now),
                    })
                });
                self.sheet = false;
            }
            _ => {}
        }
        Self::bump(&self.ver);
    }

    /// B closes the sheet; the rest of the pad stays in it.
    pub fn pad(&mut self, fx: &mut Fx, b: Btn) -> bool {
        if !self.sheet {
            return false;
        }
        match b {
            Btn::B => {
                self.sheet = false;
                fx.fb(Feedback::Back);
                Self::bump(&self.ver);
                true
            }
            Btn::Start | Btn::Select | Btn::X | Btn::Y | Btn::Lb | Btn::Rb | Btn::Lt | Btn::Rt => true,
            _ => false,
        }
    }

    pub fn hints(&self) -> Option<Vec<Hint>> {
        self.sheet.then(|| vec![hint(Key::A, tr().common.send), hint(Key::B, tr().common.cancel)])
    }
}

fn int(n: i64) -> String {
    kit::format::grouped(n as f64, 0, 0)
}

/// "23.6 h" (under an hour: minutes).
fn hours(h: f64) -> String {
    if h < 1.0 {
        trf!(tr().game.min, n = (h * 60.0).round())
    } else if h >= 100.0 {
        trf!(tr().game.h, n = int(h.round() as i64))
    } else {
        trf!(tr().game.h, n = rare_i18n::fmt::decimal(&((h * 10.0).round() / 10.0).to_string()))
    }
}

fn verdict_word(v: Option<&str>) -> &'static str {
    match v {
        Some("great") => tr().game.verdict_great,
        Some("playable") => tr().game.verdict_playable,
        Some("broken") => tr().game.verdict_broken,
        _ => tr().game.no_reports,
    }
}

fn tone(id: &str) -> rare_render::Color {
    match id {
        "great" => tok::FG,
        "playable" => tok::fg(0.55),
        _ => tok::ATTN,
    }
}

fn dot(ui: &mut Ui, id: &str) -> El {
    ui.bx(s().size(0.7)).bg(tone(id)).radius(ui.r(0.35))
}

/// Great, Playable and Broken as one bar with their counts under it.
fn split_bar(ui: &mut Ui, r: &Runs) -> El {
    let total = r.total.max(1) as f32;
    let parts = [("great", r.great), ("playable", r.playable), ("broken", r.broken)];
    let segs: Vec<El> = parts.iter().filter(|(_, n)| *n > 0).map(|(id, n)| ui.bx(s().h(0.5).wp(*n as f32 / total)).bg(tone(id)).key(key(id))).collect();
    let bar = ui.bx(s().row().gap(0.15).wp(1.0).h(0.5)).radius(ui.r(0.25)).clip().kids(segs);
    let legend: Vec<El> = parts
        .iter()
        .map(|(id, n)| ui.bx(s().row().items_center().gap(0.45)).child(dot(ui, id)).child(ui.text(trf!(tr().game.legend_item, verdict = verdict_word(Some(id)), n = int(*n as i64)), T::new(Ty::Small).c(tok::fg(0.8)).tab())).key(key(&("leg", id))))
        .collect();
    ui.bx(s().col().gap(0.6).wp(1.0)).child(bar).child(ui.bx(s().row().wrap().gap_xy(1.5, 0.4)).kids(legend))
}

/// The settings this game starts with on this device, as a report carries them.
fn settings_now(b: &dyn crate::data::backend::Backend, id: &str, appid: Option<u32>) -> RunSettings {
    let p = b.game_perf(id);
    let g = p.saved.unwrap_or(GamePower::DEFAULT);
    let u = g.upscale.unwrap_or(p.global_upscale);
    let proton = appid.and_then(|a| b.proton_status(a as u64)).and_then(|s| s.game.or(s.global));
    RunSettings {
        power: Some(match g.profile {
            Profile::Quiet => "Quiet",
            Profile::Balanced => "Balanced",
            Profile::Performance => "Performance",
            Profile::Custom => "Custom",
        }
        .into()),
        tdp: (g.profile == Profile::Custom).then(|| g.tdp.unwrap_or(15.0)),
        fps: g.fps,
        upscale: Some(match u.mode {
            Mode::Off => "Off",
            Mode::Fsr => "FSR",
            Mode::Nis => "NIS",
        }
        .into()),
        size: Some(if u.height == 0 { "1200p".into() } else { format!("{}p", u.height) }),
        refresh: g.refresh,
        proton,
    }
}

/// A report's settings on top of what the game has (Proton is left as it is).
fn apply(mut g: GamePower, global: Upscale, s: &RunSettings) -> GamePower {
    let profile = match s.power.as_deref() {
        Some("Quiet") => Profile::Quiet,
        Some("Performance") => Profile::Performance,
        Some("Custom") => Profile::Custom,
        _ => Profile::Balanced,
    };
    g = g.with_perf(profile, if profile == Profile::Custom { s.tdp } else { None }, None);
    g.fps = s.fps;
    g.refresh = s.refresh;
    let mut u = g.upscale.unwrap_or(global);
    if let Some(m) = s.upscale.as_deref() {
        u.mode = match m {
            "FSR" => Mode::Fsr,
            "NIS" => Mode::Nis,
            _ => Mode::Off,
        };
    }
    if let Some(sz) = s.size.as_deref() {
        let (w, h) = match sz {
            "800p" => (1280, 800),
            "600p" => (960, 600),
            "720p" => (1152, 720),
            _ => (0, 0),
        };
        u.width = w;
        u.height = h;
    }
    g.upscale = Some(u);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_settings_apply() {
        let s = RunSettings { power: Some("Custom".into()), tdp: Some(17.0), fps: Some(40), upscale: Some("FSR".into()), size: Some("800p".into()), refresh: Some(40), proton: None };
        let g = apply(GamePower::DEFAULT, Upscale::default(), &s);
        assert_eq!((g.profile, g.tdp, g.fps, g.refresh), (Profile::Custom, Some(17.0), Some(40), Some(40)));
        let u = g.upscale.unwrap();
        assert_eq!((u.mode, u.width, u.height), (Mode::Fsr, 1280, 800));
        assert_eq!(hours(0.5), "30 min");
        assert_eq!(hours(1234.5), "1,235 h");
    }
}
