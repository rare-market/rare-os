//! Trophies UI (launcher/src/trophies/ui: bits.tsx, ProfileTrophies.tsx, TrophyCase.tsx,
//! TrophyPopups.tsx, useCase.ts): the profile shelf, the trophy case (gamerscore, the games, a page
//! per game with each trophy minted, unlocked (Claim) or locked) and the unlock pop-ups.

use super::model::{claimable_keys, sort_rows, source_label, source_name, tier_name, CaseSummary, GameSummary, RarityFrom, RowFilter, RowState, SteamGame, Tier, TrophyRow, Trust};
use crate::screens::sui::profile::{display_name, profile_avatar, Profile};
use crate::screens::wallet::fmt::{date_md, local, num_locale};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, dot, empty, head_row, load_bar, rail, section_head, segmented, stat_line, thumb, BSize, ButtonSpec, BVariant, Tone};

/// `TierLabel`: "Gold" or "Gold · 90 points", muted, with a dot (Platinum's is white).
pub fn tier_label<M>(tier: Tier, points: Option<u32>, class: &str) -> El<M> {
    span(cn(&["inline-flex items-center gap-[0.4rem] whitespace-nowrap text-meta text-muted", class]))
        .kid(dot(if tier == 3 { Tone::Primary } else { Tone::Muted }, false, ""))
        .kid(text(tier_name(tier)))
        .kid_opt(points.map(|p| span("tabular-nums").text(format!("· {p} points"))))
}

/// `TrophyArt`: colour when earned, grey when locked, a covered square when hidden.
/// `gray`: None = earned; Some(None) = locked without a grey icon; Some(Some(url)) = locked.
pub fn trophy_art<M>(src: Option<&str>, gray: Option<Option<&str>>, masked: bool, label: &str, size: &str, class: &str) -> El<M> {
    let bx = cn(&["relative shrink-0 overflow-hidden rounded-chip bg-l3", class]);
    if masked {
        return span(cn(&[&bx, "flex items-center justify-center text-body font-semibold text-muted"])).attr("style", format!("width: {size}; height: {size}")).text("?");
    }
    let locked = gray.is_some();
    let shown = match gray {
        Some(Some(g)) if !g.is_empty() => Some(g),
        _ => src,
    };
    let faded = locked && !matches!(gray, Some(Some(g)) if !g.is_empty());
    span(bx).attr("style", format!("width: {size}; height: {size}")).kid(thumb(shown, label, &cn(&["absolute inset-0 h-full w-full", when(faded, "opacity-40 grayscale")]), false))
}

/// `ProgressLine`: completion, white on layer 04 (progress, not money: no green).
pub fn progress_line<M>(pct: u32, class: &str) -> El<M> {
    span(cn(&["relative block h-[3px] w-full overflow-hidden rounded-full bg-l4", class])).kid(span("absolute inset-y-0 left-0 rounded-full bg-fg/75").attr("style", format!("width: {}%", pct.min(100))))
}

/// `useSteamHeader`: the library's local header art first, then Steam's CDN.
pub fn steam_header_sources(appid: u32, local_headers: &[String]) -> Vec<String> {
    let cdn = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg");
    let mut v: Vec<String> = local_headers.iter().filter(|p| **p != cdn).cloned().collect();
    v.push(cdn);
    v
}

/// A library game's own art for the case (header first, then the wide hero, then the cover).
pub fn lib_header(g: &crate::data::model::Game) -> Vec<String> {
    let mut v: Vec<String> = g.art.header.iter().chain(g.art.hero.iter()).chain(g.art.cover.iter()).cloned().collect();
    if let Some(a) = g.id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).filter(|a| *a < 0x8000_0000) {
        v = steam_header_sources(a, &v);
    }
    v
}

/// `GameArt`.
pub fn game_art<M>(sources: &[String], title: &str, class: &str) -> El<M> {
    span(cn(&["relative block aspect-[460/215] w-full overflow-hidden bg-l3", class])).kid(img(sources.first().map(String::as_str), title, "absolute inset-0 h-full w-full object-cover").attr("data-width", "460").attr("data-height", "215").attr("data-art", "header").attr("data-sources", sources.join(" ")))
}

/// `fmtPct`: one decimal under 10 %.
pub fn fmt_pct(p: f64) -> String {
    if p > 0.0 && p < 10.0 {
        format!("{}%", crate::screens::wallet::fmt::to_fixed(p, 1))
    } else {
        format!("{}%", p.round())
    }
}

/// `fmtDay`: "Mar 4", with the year when it is not this year.
pub fn fmt_day(ms: i64, now: i64, tz_min: i32) -> String {
    date_md(ms, tz_min, local(ms, tz_min).year != local(now, tz_min).year)
}

/// A game in the case (`CaseGame`).
#[derive(Clone, Debug, PartialEq)]
pub struct CaseGame {
    pub appid: u32,
    pub name: String,
    pub steam: SteamGame,
    /// players on Rare (chain), when the game is on chain
    pub chain_players: Option<u64>,
    pub on_chain: bool,
    pub rows: Vec<TrophyRow>,
    pub sum: GameSummary,
    /// why Claim can't run (setup, paused game), or None
    pub blocker: Option<String>,
    pub header: Vec<String>,
    /// The game's key in reports and pins ("steam:10180", "ra:1", a library id).
    pub key: String,
    /// The library game (milestones, Mark complete), when it is in the library.
    pub lib_id: Option<String>,
    /// Rare's own trophies for the game.
    pub milestones: Vec<TrophyRow>,
    /// The player marked it complete.
    pub marked: bool,
}

impl CaseGame {
    pub fn row(&self, key: &str) -> Option<&TrophyRow> {
        self.milestones.iter().chain(self.rows.iter()).find(|r| r.key == key)
    }

    /// The source's own achievements are all unlocked.
    pub fn source_complete(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| r.state != RowState::Locked)
    }

    pub fn label(&self) -> &'static str {
        source_label(if self.rows.is_empty() && self.steam.total == 0 { "rare" } else { &self.steam.source })
    }
}

/// A showcase slot as the case draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShowPin {
    pub appid: Option<u32>,
    pub name: String,
    pub game: String,
    pub icon_url: Option<String>,
    pub tier: Tier,
}

/// `cardNote`: what to do next, else when it was last played.
pub fn card_note(g: &CaseGame, now: i64, tz_min: i32) -> Option<String> {
    if g.sum.claimable > 0 && g.blocker.is_none() {
        return Some(format!("{} to claim", g.sum.claimable));
    }
    if g.sum.minted > 0 {
        return Some(format!("{} minted", g.sum.minted));
    }
    if g.sum.score > 0 {
        return Some(format!("{} points", n(g.sum.score as u64)));
    }
    g.sum.last_at.map(|t| format!("Last on {}", fmt_day(t, now, tz_min)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameSort {
    Recent,
    Completion,
    Name,
}

/// useCase.ts's game order.
pub fn sort_games(games: &mut [CaseGame], by: GameSort) {
    match by {
        GameSort::Recent => games.sort_by(|a, b| b.sum.last_at.unwrap_or(0).cmp(&a.sum.last_at.unwrap_or(0)).then(b.sum.unlocked.cmp(&a.sum.unlocked))),
        GameSort::Completion => games.sort_by(|a, b| b.sum.pct.cmp(&a.sum.pct).then(b.sum.unlocked.cmp(&a.sum.unlocked))),
        GameSort::Name => games.sort_by_key(|g| g.name.to_lowercase()),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Open(Option<u32>),
    Claim(u32, Vec<String>),
    /// A on a trophy: its sheet (show on profile, pin to chain, claim).
    Row(u32, String),
    /// Mark complete, or not.
    Complete(u32),
    /// A on a locked trophy: what it takes, as a toast.
    Locked(String),
    /// X: the next sort (case) or filter (game page)
    Next,
    Back,
    Nothing,
}

/// `GameCard`.
pub fn game_card(g: &CaseGame, note: Option<String>, def: bool, class: &str) -> El<Msg> {
    button(cn(&["tile tile-wide flex flex-col overflow-hidden rounded-card bg-l1 text-left", class]), Some(Msg::Open(Some(g.appid))))
        .hint_a("Trophies")
        .def(def)
        .key(g.appid.to_string())
        .kid(game_art(&g.header, &g.name, ""))
        .kid(
            span("flex flex-col gap-[0.5rem] px-[0.9rem] pb-[0.8rem] pt-[0.7rem]")
                .kid(span("flex items-baseline gap-[0.75rem]").kid(span("focus-text min-w-0 flex-1 truncate text-body font-semibold").text(&g.name)).kid(span("tabular-nums text-small text-muted").text(format!("{}%", g.sum.pct))))
                .kid(progress_line(g.sum.pct, ""))
                .kid(span("flex items-center gap-[0.75rem] text-meta text-muted").kid(span("shrink-0 tabular-nums").text(format!("{} · {} of {}", g.label(), g.sum.unlocked, g.sum.total))).kid_opt(note.map(|n| span("ml-auto truncate").text(n)))),
        )
}

fn n(v: u64) -> String {
    num_locale(v as f64, 0, 0)
}

/// `ProfileTrophies`: the shelf on the profile (level, score, showcase, games); nothing without games.
pub fn profile_shelf(games: &[CaseGame], s: &CaseSummary, showcase: &[ShowPin], now: i64, tz_min: i32) -> Option<El<Msg>> {
    if games.is_empty() {
        return None;
    }
    let line = span("flex flex-wrap gap-x-[0.9rem] gap-y-[0.3rem] text-meta text-muted").kids([3u8, 2, 1, 0].iter().map(|t| {
        span("inline-flex items-center gap-[0.35rem] tabular-nums").key(t.to_string()).kid(dot(if *t == 3 { Tone::Primary } else { Tone::Muted }, false, "")).kid(text(format!("{} {}", s.counted[*t as usize], tier_name(*t))))
    }));
    let card = button("tile tile-wide flex w-[17rem] shrink-0 flex-col justify-between gap-[0.6rem] self-stretch rounded-card bg-l1 px-[1.1rem] py-[1rem] text-left", Some(Msg::Open(None)))
        .hint_a("Trophy case")
        .kid(span("").kid(span("block text-label text-secondary").text(format!("Level {}", s.level.0))).kid(span("block tabular-nums text-display font-semibold").text(n(s.score))))
        .kid(level_line(s, ""))
        .kid(line);
    let mut tiles = vec![card];
    tiles.extend(showcase.iter().enumerate().map(|(i, p)| show_card(p, i, "w-[17rem] shrink-0 self-stretch")));
    tiles.extend(games.iter().take(10).map(|g| game_card(g, card_note(g, now, tz_min), false, "w-[17rem] shrink-0")));
    let note = format!("{} unlocked in {} {}", n(s.unlocked_all), s.games, if s.games == 1 { "game" } else { "games" });
    Some(rail("profile-trophies", Some(text("Trophies")), Some(text(note)), None, "", tiles))
}

/// Progress to the next level.
pub fn level_line<M>(s: &CaseSummary, class: &str) -> El<M> {
    let (_, into, span_) = s.level;
    let pct = if span_ > 0 { (into as f64 / span_ as f64 * 100.0).floor() as u32 } else { 0 };
    span(cn(&["flex flex-col gap-[0.35rem]", class])).kid(progress_line(pct, "")).kid(span("text-meta tabular-nums text-muted").text(format!("{} to level {}", n(span_ - into), s.level.0 + 1)))
}

/// A showcase trophy (the profile and the case).
pub fn show_card(p: &ShowPin, i: usize, class: &str) -> El<Msg> {
    button(cn(&["tile flex items-center gap-[0.9rem] rounded-card bg-l1 px-[0.9rem] py-[0.8rem] text-left", class]), Some(Msg::Open(p.appid)))
        .hint_a("Open the game")
        .key(format!("show-{i}"))
        .kid(trophy_art(p.icon_url.as_deref(), None, false, &p.name, "3.5rem", ""))
        .kid(
            span("min-w-0 flex-1")
                .kid(span("focus-text block truncate text-body font-semibold").text(&p.name))
                .kid(span("block truncate text-small text-muted").text(&p.game))
                .kid(span("mt-[0.3rem] block").kid(tier_label(p.tier, None, ""))),
        )
}

pub const SORTS: [(GameSort, &str); 3] = [(GameSort::Recent, "Recent"), (GameSort::Completion, "Completion"), (GameSort::Name, "Name")];
pub const FILTERS: [(RowFilter, &str); 3] = [(RowFilter::All, "All"), (RowFilter::Unlocked, "Unlocked"), (RowFilter::Locked, "Locked")];

fn next<T: PartialEq + Copy>(list: &[(T, &'static str)], cur: T) -> (T, &'static str) {
    let i = list.iter().position(|(v, _)| *v == cur).unwrap_or(0);
    list[(i + 1) % list.len()]
}

pub struct CaseProps<'a> {
    pub games: &'a [CaseGame],
    pub showcase: &'a [ShowPin],
    /// Why this device's unlocks don't count yet, when rare.market said.
    pub device_note: Option<&'a str>,
    /// rare.market has checked the unlocks at least once.
    pub checked: bool,
    /// The trophies being pinned to chain ("game|key").
    pub pinning: &'a [String],
    pub summary: &'a CaseSummary,
    pub prof: &'a Profile,
    pub loading: bool,
    /// the chain side is set up (claims open)
    pub chain_on: bool,
    /// which games are being claimed
    pub claiming: &'a [(u32, Vec<String>)],
    pub now: i64,
    pub tz_min: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseState {
    /// the game page on screen
    pub view: Option<u32>,
    /// opened straight at a game (B leaves the case instead of going to the grid)
    pub opened_at: Option<u32>,
    pub sort: GameSort,
    pub filter: RowFilter,
    pub last: Option<u32>,
}

impl CaseState {
    pub fn new(appid: Option<u32>) -> CaseState {
        CaseState { view: appid, opened_at: appid, sort: GameSort::Recent, filter: RowFilter::All, last: appid }
    }
}

/// The hint for X.
pub fn case_hint(st: &CaseState) -> String {
    if st.view.is_some() {
        format!("Show {}", next(&FILTERS, st.filter).1.to_lowercase())
    } else {
        format!("Sort by {}", next(&SORTS, st.sort).1.to_lowercase())
    }
}

/// The trophy case. `games` are in the order of `st.sort`.
pub fn case_view(pr: &CaseProps, st: &CaseState) -> El<Msg> {
    let game = st.view.and_then(|v| pr.games.iter().find(|g| g.appid == v));
    let body = match (st.view, game) {
        (Some(_), Some(g)) => game_page(g, st.filter, pr),
        (Some(_), None) if !pr.loading => div("flex h-full items-center px-[3rem]").kid(empty(
            "No achievements for this game here",
            Some(text("Steam keeps achievements on this device once the game has run here at least once.")),
            vec![],
            "",
        )),
        (None, _) => case_page(pr, st),
        _ => div(""),
    };
    div("rise fixed inset-0 z-30 bg-bg").attr("data-nav-scope", "").on_pad("b", Msg::Back).on_pad("x", Msg::Next).kid(load_bar(pr.loading, "")).kid(body)
}

fn case_page(pr: &CaseProps, st: &CaseState) -> El<Msg> {
    let s = pr.summary;
    let def_idx = pr.games.iter().position(|g| Some(g.appid) == st.last).unwrap_or(0);
    let mut header = el(Tag::Div, "flex items-end gap-[1.25rem] px-[2.5rem] pt-[2.25rem]").attr("data-tag", "header");
    if pr.prof.address.is_some() {
        header = header.kid_opt(profile_avatar(pr.prof, "4rem", ""));
    }
    header = header
        .kid(div("min-w-0 flex-1").kid(div("text-label text-secondary").text("Trophies")).kid(h1("truncate text-title font-semibold tracking-[-0.02em]").text(if pr.prof.address.is_some() { display_name(pr.prof) } else { "Your trophies".into() })))
        .kid(
            div("w-[20rem] shrink-0")
                .kid(div("flex items-baseline justify-between gap-[1rem]").kid(span("text-label text-secondary").text(format!("Level {}", s.level.0))).kid(span("tabular-nums text-title font-semibold").text(n(s.score))))
                .kid(level_line(s, "mt-[0.5rem]")),
        );
    let has_games = !pr.games.is_empty();
    let tier_item = |t: Tier| -> (String, Option<El<Msg>>) { (tier_name(t).into(), Some(text(n(s.counted[t as usize])))) };
    let not_counted = s.score_all.saturating_sub(s.score);
    let mut stats = stat_line(
        vec![
            ("Rare score".into(), Some(text(n(s.score)))),
            tier_item(3),
            tier_item(2),
            tier_item(1),
            tier_item(0),
            ("Unlocked".into(), has_games.then(|| text(format!("{} of {}", n(s.unlocked_all), n(s.total_all))))),
            ("Games".into(), has_games.then(|| text(s.games.to_string()))),
            ("Not counted".into(), (not_counted > 0).then(|| text(format!("{} points", n(not_counted))))),
            ("Minted".into(), s.points.map(|_| text(n(s.trophies)))),
        ],
        true,
        true,
        "mx-[2.5rem] mt-[1.5rem] rounded-panel bg-l1 px-[1.25rem] py-[1rem]",
    );
    if let Some(sl) = stats.as_mut() {
        for item in sl.kids.iter_mut() {
            let label = item.kids[0].text.clone().unwrap_or_default();
            if let Some(t) = [3u8, 2, 1, 0].into_iter().find(|t| tier_name(*t) == label) {
                item.kids[0].text = None;
                item.kids[0].kids = vec![span("inline-flex items-center gap-[0.35rem]").kid(dot(if t == 3 { Tone::Primary } else { Tone::Muted }, false, "")).kid(text(tier_name(t)))];
            }
        }
    }
    let note = match (pr.checked, pr.device_note) {
        (_, Some(d)) => format!("Every unlock shows here. The Rare score counts verified ones, and this device's aren't verified yet: {d}"),
        (true, None) => "Every unlock shows here. The Rare score counts verified ones: unlocks on a SuiPlay that proves it runs Rare OS (its TPM) and match your play sessions, and retro trophies in hardcore.".to_string(),
        (false, None) => "Every unlock shows here. The Rare score counts verified ones once rare.market has checked them (turn on friends to send them).".to_string(),
    };
    let mut d = div("h-full overflow-y-auto pb-[3rem]").attr("data-scroll", "").kid(header).kid_opt(stats).kid(p("mx-[2.5rem] mt-[0.75rem] max-w-[60rem] text-small text-muted").text(note));
    // The showcase: three slots, on the profile and on rare.market.
    if has_games {
        let mut row = div("grid grid-cols-3 gap-[1.1rem] px-[2.5rem]").attr("data-nav-group", "trophy-showcase");
        for (i, p_) in pr.showcase.iter().enumerate() {
            row = row.kid(show_card(p_, i, ""));
        }
        for i in pr.showcase.len()..rare_trophies::case::SHOWCASE_MAX {
            row = row.kid(
                div("flex min-h-[6.75rem] flex-col justify-center gap-[0.25rem] rounded-card border border-border-light px-[1.1rem]")
                    .key(format!("slot-{i}"))
                    .kid(span("text-body font-medium text-fg/70").text("Empty slot"))
                    .kid(span("text-small text-muted").text("Press A on an unlocked trophy to show it here")),
            );
        }
        d = d.kid(section_head(text("Showcase"), Some(text("On your profile and rare.market")), None, "mb-[0.9rem] mt-[2rem] px-[2.5rem]")).kid(row);
    }
    let right = (pr.games.len() > 1).then(|| span("flex items-center gap-[0.75rem]").kid(glyph("x")).kid(segmented(&SORTS.map(|(v, l)| (sort_key(v), l)), sort_key(st.sort), "")));
    d = d.kid(section_head(text("Games"), has_games.then(|| text(format!("{} with trophies", pr.games.len()))), right, "mb-[0.9rem] mt-[2rem] px-[2.5rem]"));
    if has_games {
        d = d.kid(div("grid grid-cols-4 gap-[1.1rem] px-[2.5rem]").attr("data-nav-group", "trophy-games").kids(pr.games.iter().enumerate().map(|(i, g)| game_card(g, card_note(g, pr.now, pr.tz_min), i == def_idx, ""))));
    } else if !pr.loading {
        d = d.kid(empty(
            "No trophies yet",
            Some(text("Play any game and Rare's trophies start with the first launch. Steam, RetroAchievements, Epic and GOG achievements show up here too.")),
            vec![],
            "mx-[2.5rem]",
        ));
    }
    d
}

fn sort_key(s: GameSort) -> &'static str {
    match s {
        GameSort::Recent => "recent",
        GameSort::Completion => "completion",
        GameSort::Name => "name",
    }
}

fn filter_key(f: RowFilter) -> &'static str {
    match f {
        RowFilter::All => "all",
        RowFilter::Unlocked => "unlocked",
        RowFilter::Locked => "locked",
    }
}

const COLS: &str = "grid-cols-[2.75rem_minmax(0,1fr)_8rem_6rem_12.5rem]";

fn game_page(g: &CaseGame, filter: RowFilter, pr: &CaseProps) -> El<Msg> {
    let claiming = pr.claiming.iter().find(|(a, _)| *a == g.appid).map(|(_, k)| k);
    let rows = sort_rows(&g.rows, filter);
    let mut ms = sort_rows(&g.milestones, filter);
    if filter == RowFilter::All {
        // Rare's own in their natural order: first launch, hours, streak, SuiPlay, completed.
        ms = g.milestones.clone();
    }
    let keys = claimable_keys(&g.rows);
    let can_claim = g.blocker.is_none() && !keys.is_empty();
    let hardcore = g.rows.iter().any(|r| r.hardcore);
    let not_counted = g.sum.score_all.saturating_sub(g.sum.score);
    let stats = stat_line(
        vec![
            ("Unlocked".into(), Some(text(format!("{} of {}", g.sum.unlocked, g.sum.total)))),
            ("Completion".into(), Some(text(format!("{}%", g.sum.pct)))),
            ("Rare score".into(), Some(text(n(g.sum.score as u64)))),
            ("Not counted".into(), (not_counted > 0).then(|| text(format!("{} points", n(not_counted as u64))))),
            ("Minted".into(), g.on_chain.then(|| text(g.sum.minted.to_string()))),
            ("Players on Rare".into(), g.chain_players.filter(|p| *p > 0 && g.on_chain).map(|p| text(n(p)))),
        ],
        false,
        false,
        "mt-[0.9rem]",
    );
    let mut acts = div("flex shrink-0 flex-col items-end gap-[0.5rem]").attr("data-nav-group", "trophy-actions").attr("data-nav-remember", "");
    if can_claim || claiming.is_some() {
        let label = if claiming.is_some() {
            "Claiming".to_string()
        } else if keys.len() == 1 {
            "Claim 1 trophy".into()
        } else {
            format!("Claim {} trophies", keys.len())
        };
        acts = acts.kid(button_el(ButtonSpec::new(&label).v(BVariant::Primary).size(BSize::Lg).def(true).hint("Claim").disabled(claiming.is_some()), Some(Msg::Claim(g.appid, keys.clone()))));
    } else if let Some(b) = g.blocker.as_ref().filter(|b| g.sum.unminted > 0 && !b.is_empty()) {
        // Why Claim isn't there: a line, not a button that can't be pressed.
        acts = acts.kid(p("max-w-[20rem] text-right text-small text-muted").text(b));
    }
    if g.lib_id.is_some() && !g.source_complete() {
        let label = if g.marked { "Marked complete" } else { "Mark complete" };
        acts = acts.kid(button_el(ButtonSpec::new(label).v(if g.marked { BVariant::Secondary } else { BVariant::Ghost }).size(BSize::Lg).hint(if g.marked { "Not complete" } else { "Mark complete" }), Some(Msg::Complete(g.appid))));
    }
    let label = if hardcore { format!("{} · Hardcore", g.label()) } else if g.steam.source == "librarycache" { "Steam · from your Steam library".to_string() } else { g.label().to_string() };
    let header = el(Tag::Div, "flex items-end gap-[1.5rem] px-[2.5rem] pt-[2.25rem]")
        .attr("data-tag", "header")
        .kid(div("w-[17rem] shrink-0 overflow-hidden rounded-card").kid(game_art(&g.header, &g.name, "")))
        .kid(div("min-w-0 flex-1").kid(div("text-label text-secondary").text(label)).kid(h1("truncate text-title font-semibold tracking-[-0.02em]").text(&g.name)).kid_opt(stats))
        .kid(acts);
    let right = span("flex items-center gap-[0.75rem]").kid(glyph("x")).kid(segmented(&FILTERS.map(|(v, l)| (filter_key(v), l)), filter_key(filter), ""));
    let src = g.label();
    let mut first = !can_claim;
    let mut page = div("h-full overflow-y-auto pb-[3rem]").attr("data-scroll", "").kid(header);
    let head = |e: El<Msg>| div("px-[2.5rem]").kid(head_row(COLS, "", vec![span(""), span("").text("Trophy"), span("").text("Tier"), span("text-right").text("Rarity"), span("text-right").text("Status")])).kid(e);
    let empty_line = |none_yet: bool| div("px-[0.75rem] py-[1rem] text-small text-muted").text(if none_yet { "Loading the achievements." } else if filter == RowFilter::Locked { "Nothing left to unlock." } else { "Nothing unlocked yet." });
    // The store's achievements first (Steam-style), then Rare's own; the filter sits on the first list.
    let has_source = !g.rows.is_empty() || g.steam.total > 0;
    let mut right = Some(right);
    if has_source {
        let mut list = div("").attr("data-nav-group", format!("trophy-list-{}", filter_key(filter)));
        for r in rows.iter() {
            let claimable = g.blocker.is_none() && r.state == RowState::Unlocked && r.def.as_ref().is_some_and(|d| d.tier != 3);
            let busy = claiming.is_some_and(|k| k.contains(&r.key));
            list = list.kid(trophy_row(g.appid, r, std::mem::take(&mut first), claimable, busy, pr, src));
        }
        if rows.is_empty() {
            list = list.kid(empty_line(g.rows.is_empty()));
        }
        page = page.kid(section_head(text(if src == "Retro trophies" { src.to_string() } else { format!("{src} achievements") }), Some(text(format!("{} shown", rows.len()))), right.take(), "mb-[0.5rem] mt-[2rem] px-[2.5rem]")).kid(head(list));
    }
    if !g.milestones.is_empty() {
        let mut list = div("").attr("data-nav-group", format!("trophy-rare-{}", filter_key(filter)));
        for r in &ms {
            list = list.kid(trophy_row(g.appid, r, std::mem::take(&mut first), false, false, pr, src));
        }
        if ms.is_empty() {
            list = list.kid(empty_line(false));
        }
        page = page.kid(section_head(text("Rare trophies"), None, right.take(), "mb-[0.5rem] mt-[2rem] px-[2.5rem]")).kid(head(list));
    }
    page
}

fn trust_mark(r: &TrophyRow) -> Option<El<Msg>> {
    match &r.trust {
        Trust::Verified if r.state != RowState::Locked => Some(span("inline-flex text-muted").attr("title", "Verified on SuiPlay").kid(icon("check", "").attr("size", "0.85rem").attr("stroke-width", "2.5"))),
        _ => None,
    }
}

fn trophy_row(appid: u32, r: &TrophyRow, def: bool, claimable: bool, busy: bool, pr: &CaseProps, src: &str) -> El<Msg> {
    let (now, tz_min) = (pr.now, pr.tz_min);
    let locked = r.state == RowState::Locked;
    let gray = if locked { Some(r.icon_gray_url.as_deref()) } else { None };
    let pinning = pr.pinning.iter().any(|k| k.ends_with(&format!("|{}", r.key)));
    let sub = |e: El<Msg>| e;
    let status = match r.state {
        RowState::Minted if r.minted.is_some() => {
            let m = r.minted.as_ref().unwrap();
            span("text-right").kid(span("block text-small font-medium text-fg").text("Minted")).kid(span("block truncate text-label text-muted").text(format!("{} · #{}", source_name(m.source), n(m.number))))
        }
        RowState::Unlocked => {
            let top = if busy {
                "Claiming".to_string()
            } else if claimable {
                "Ready to claim".into()
            } else if pinning {
                "Pinning".into()
            } else if r.pinned.is_some() {
                "On chain".into()
            } else {
                "Unlocked".into()
            };
            let bottom = match (&r.trust, r.at) {
                (Trust::Flagged(_), _) => Some("Not counted".to_string()),
                (_, Some(t)) => Some(format!("{}{}", fmt_day(t, now, tz_min), if r.hardcore { " · Hardcore" } else { "" })),
                (_, None) => r.hardcore.then(|| "Hardcore".to_string()),
            };
            span("text-right")
                .kid(span("flex items-center justify-end gap-[0.35rem]").kid_opt(trust_mark(r)).kid(span(cn(&["text-small font-medium", if busy || pinning { "text-muted" } else { "text-fg" }])).text(top)))
                .kid_opt(bottom.map(|b| sub(span("block text-label text-muted").text(b))))
        }
        _ => span("text-right")
            .kid(span("block text-small text-muted").text("Locked"))
            .kid_opt(r.progress.map(|(have, need)| span("block text-label tabular-nums text-muted").text(if r.key == "rare:streak7" { format!("{have} of {need} days") } else { format!("{have} of {need} hours") }))),
    };
    let mut rarity = span("text-right");
    if let Some(p_) = r.rarity {
        let from = match r.rarity_from {
            Some(RarityFrom::Rare) => "on Rare".to_string(),
            _ if src == "Steam" || src == "GOG" => format!("on {src}"),
            _ => "of players".to_string(),
        };
        rarity = rarity.kid(span("block tabular-nums text-small text-fg/85").text(fmt_pct(p_))).kid(span("block text-label text-muted").text(from));
    }
    let name = if r.masked { "Hidden trophy".to_string() } else { r.name.clone() };
    let desc = if r.masked { "Keep playing to reveal it.".to_string() } else { r.description.clone() };
    let how = {
        let so_far = r.progress.map(|(have, need)| if r.key == "rare:streak7" { format!(" {have} of {need} days so far.") } else { format!(" {have} of {need} hours so far.") }).unwrap_or_default();
        format!("Locked. {}{so_far}", if r.masked { "Keep playing to reveal it.".to_string() } else { r.description.clone() })
    };
    let msg = if claimable && !busy { Msg::Claim(appid, vec![r.key.clone()]) } else if !locked { Msg::Row(appid, r.key.clone()) } else { Msg::Locked(how) };
    let hint = if claimable && !busy { "Claim" } else if !locked { "Options" } else { "How to unlock" };
    button(cn(&["grid h-[4.25rem] w-full items-center gap-x-[0.9rem] rounded-card px-[0.75rem] text-left", COLS]), Some(msg))
        .def(def)
        .hint_a(hint)
        .key(&r.key)
        .kid(trophy_art(r.icon_url.as_deref(), gray, r.masked, &r.name, "2.75rem", ""))
        .kid(
            span("min-w-0")
                .kid(span("flex items-center gap-[0.45rem]").kid(span(cn(&["truncate text-body font-medium", if locked { "text-fg/70" } else { "text-fg" }])).text(name)).kid_if(r.showcased, || span("shrink-0 text-label text-muted").text("On profile")))
                .kid(span("block truncate text-small text-muted").text(desc)),
        )
        .kid(span("").kid_opt(r.tier.map(|t| tier_label(t, None, ""))))
        .kid(rarity)
        .kid(status)
}

/// B and X on the case: Some(true) leaves the case (back to where it was opened from, or closed).
pub fn case_update(st: &mut CaseState, msg: &Msg) -> Option<bool> {
    match msg {
        Msg::Back => {
            if st.view.is_some() && st.opened_at.is_none() {
                st.view = None;
                st.filter = RowFilter::All;
                Some(false)
            } else {
                Some(true)
            }
        }
        Msg::Next => {
            if st.view.is_some() {
                st.filter = next(&FILTERS, st.filter).0;
            } else {
                st.sort = next(&SORTS, st.sort).0;
            }
            Some(false)
        }
        Msg::Open(Some(a)) => {
            st.last = Some(*a);
            st.view = Some(*a);
            Some(false)
        }
        _ => None,
    }
}

/// A pop-up (`TrophyPopups`): a new local Steam unlock or a minted trophy.
#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    pub minted: bool,
    pub name: String,
    pub game: Option<String>,
    pub tier: Option<Tier>,
    pub points: Option<u32>,
    pub source: Option<u8>,
    pub icon_url: Option<String>,
    pub more: u32,
}

/// One pop-up at a time in the toast column; none while a game runs (they wait).
pub fn popup_view<M>(p: Option<&Popup>, gaming: bool) -> Option<El<M>> {
    let p = p.filter(|_| !gaming)?;
    let label = if p.minted {
        "Trophy minted"
    } else if p.tier.is_some() {
        "Trophy unlocked"
    } else {
        "Achievement unlocked"
    };
    let meta = [p.game.clone(), p.tier.map(|t| tier_name(t).to_string()), p.points.map(|x| format!("{x} points")), p.source.map(|s| source_name(s).to_string())].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    Some(
        div("pop flex w-[28rem] items-center gap-[0.85rem] rounded-card border border-border-light bg-l3 py-[0.7rem] pl-[0.7rem] pr-[1rem] shadow-xl shadow-black/30")
            .role("status")
            .kid(trophy_art(p.icon_url.as_deref(), None, false, &p.name, "3rem", ""))
            .kid(
                div("min-w-0 flex-1")
                    .kid(div("text-label text-secondary").text(label))
                    .kid(div("truncate text-body font-semibold text-fg").text(&p.name))
                    .kid(div("truncate text-meta text-muted").text(format!("{meta}{}", if p.more > 0 { format!(" and {} more", p.more) } else { String::new() }))),
            ),
    )
}

/// Each pop-up shows for 4.8 s.
pub const POPUP_MS: u64 = 4800;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::trophies::model::tests::{chain, minted, steam};
    use crate::screens::trophies::model::{case_summary, game_rows, summarize};
    use crate::screens::wallet::home::fixtures::NOW;

    fn case_game(on_chain: bool) -> CaseGame {
        let s = steam();
        let c = chain();
        let rows = if on_chain { game_rows(Some(&s), Some(&c), &minted()) } else { game_rows(Some(&s), None, &[]) };
        let sum = summarize(&rows);
        CaseGame { appid: 1, name: "Balatro".into(), steam: s, chain_players: on_chain.then_some(20), on_chain, rows, sum, blocker: None, header: steam_header_sources(1, &[]), key: "steam:1".into(), lib_id: None, milestones: vec![], marked: false }
    }

    #[test]
    fn bits() {
        assert_eq!(fmt_pct(4.25), "4.3%");
        assert_eq!(fmt_pct(25.0), "25%");
        assert_eq!(steam_header_sources(9, &["/local/h.jpg".into()]), ["/local/h.jpg", "https://cdn.cloudflare.steamstatic.com/steam/apps/9/header.jpg"]);
        let t: El<()> = tier_label(2, Some(90), "");
        assert_eq!(t.texts(), ["Gold", "· 90 points"]);
        assert_eq!(trophy_art::<()>(None, None, true, "x", "2rem", "").texts(), ["?"]);
    }

    #[test]
    fn shelf() {
        let g = case_game(true);
        let s = case_summary(std::slice::from_ref(&g.steam), None, &minted());
        let s = crate::screens::trophies::model::with_score(s, &[&g.rows]);
        let v = profile_shelf(std::slice::from_ref(&g), &s, &[], NOW, 0).unwrap();
        let t = v.texts();
        assert_eq!(&t[..4], ["Trophies", "2 unlocked in 1 game", "Level 1", "0"]);
        assert!(t.contains(&"0 Bronze".to_string()));
        assert!(t.contains(&"1 to claim".to_string()));
        assert!(profile_shelf(&[], &s, &[], NOW, 0).is_none());
    }

    #[test]
    fn case_and_game_page() {
        let g = case_game(true);
        let s = case_summary(std::slice::from_ref(&g.steam), None, &minted());
        let prof = Profile::default();
        let pr = CaseProps { games: std::slice::from_ref(&g), showcase: &[], device_note: None, checked: true, pinning: &[], summary: &s, prof: &prof, loading: false, chain_on: true, claiming: &[], now: NOW, tz_min: 0 };
        let mut st = CaseState::new(None);
        let v = case_view(&pr, &st);
        let t = v.texts();
        assert_eq!(&t[..2], ["Trophies", "Your trophies"]);
        assert!(t.contains(&"Rare score".to_string()));
        assert!(t.contains(&"Showcase".to_string()));
        assert_eq!(case_hint(&st), "Sort by completion");
        assert_eq!(case_update(&mut st, &Msg::Open(Some(1))), Some(false));
        let v = case_view(&pr, &st);
        let t = v.texts();
        assert!(t.contains(&"Claim 1 trophy".to_string()));
        assert!(t.contains(&"Ready to claim".to_string()));
        assert!(t.contains(&"Hidden trophy".to_string()));
        assert!(t.contains(&"Studio issued · #3".to_string()) || t.contains(&"Self-reported · #3".to_string()));
        assert_eq!(v.landing().as_deref(), Some("Claim 1 trophy"));
        assert_eq!(case_hint(&st), "Show unlocked");
        case_update(&mut st, &Msg::Next);
        case_update(&mut st, &Msg::Next);
        assert_eq!(st.filter, RowFilter::Locked);
        assert!(case_view(&pr, &st).contains_text("3 shown"));
        assert_eq!(case_update(&mut st, &Msg::Back), Some(false));
        assert_eq!(st.view, None);
        assert_eq!(case_update(&mut st, &Msg::Back), Some(true));
        let mut at = CaseState::new(Some(1));
        assert_eq!(case_update(&mut at, &Msg::Back), Some(true));
    }

    #[test]
    fn popups() {
        let p = Popup { minted: false, name: "First blood".into(), game: Some("Balatro".into()), tier: None, points: None, source: None, icon_url: None, more: 2 };
        let v: El<()> = popup_view(Some(&p), false).unwrap();
        assert_eq!(v.texts(), ["Achievement unlocked", "First blood", "Balatro and 2 more"]);
        assert!(popup_view::<()>(Some(&p), true).is_none());
    }
}
