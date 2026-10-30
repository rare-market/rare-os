//! The trophy case's view logic (launcher/src/trophies/model.ts, types.ts, chain.ts `rarity`): local
//! Steam achievements (rare-trophies reads them), the chain's trophy definitions and the player's
//! minted trophies joined into rows and summaries. No I/O. model.test.ts is ported below.

pub type Tier = u8;

pub fn tier_name(t: Tier) -> &'static str {
    match t {
        0 => "Bronze",
        1 => "Silver",
        2 => "Gold",
        _ => "Platinum",
    }
}

/// 0 studio issued, 1 self-reported (Rare attested local data).
pub fn source_name(s: u8) -> &'static str {
    if s == 0 {
        "Studio issued"
    } else {
        "Self-reported"
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SteamAchievement {
    pub api_name: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub icon_gray_url: Option<String>,
    pub hidden: bool,
    pub unlocked: bool,
    /// unix seconds
    pub unlock_time: Option<i64>,
    pub global_percent: Option<f64>,
    /// RetroAchievements: earned in hardcore (no save states, no cheats; rcheevos checks it).
    pub hardcore: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SteamGame {
    pub appid: u32,
    pub name: String,
    pub achievements: Vec<SteamAchievement>,
    pub total: u32,
    pub unlocked: u32,
    pub last_unlock: Option<i64>,
    /// "stats" | "librarycache" | "retroachievements" | "epic" | "gog"
    pub source: String,
}

/// Where a game's achievements come from, for the label on cards and pages.
pub fn source_label(source: &str) -> &'static str {
    match source {
        "retro" => "Retro trophies",
        "retroachievements" => "RetroAchievements",
        "epic" => "Epic Games",
        "gog" => "GOG",
        "rare" => "Rare",
        _ => "Steam",
    }
}

/// How much Rare trusts an unlock (rare.market's checks: device attestation, plausibility).
/// Only Verified unlocks count toward the Rare score and Rare's rarity.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Trust {
    /// Not checked yet (never sent, or rare.market is not reachable).
    #[default]
    Pending,
    Verified,
    /// Shown, not counted: this device's boot is not attested, or the unlock predates Rare.
    Unverified,
    /// Shown, not counted, with why (an unlock with no play session, a burst at one second).
    Flagged(String),
}

impl Trust {
    pub fn counts(&self) -> bool {
        *self == Trust::Verified
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainTrophy {
    pub object_id: String,
    pub game: String,
    pub trophy_id: u32,
    pub name: String,
    pub tier: Tier,
    pub points: u32,
    pub source: u8,
    pub number: u64,
    pub unlocked_at_ms: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainDef {
    pub trophy_id: u32,
    pub key: String,
    pub name: String,
    pub description: String,
    pub image_url: String,
    pub tier: Tier,
    pub points: u32,
    pub hidden: bool,
    pub unlocked: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainGame {
    pub id: String,
    pub name: String,
    pub players: u64,
    pub defs: Vec<ChainDef>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainProfile {
    pub points: u64,
    pub trophies: u64,
    pub bronze: u64,
    pub silver: u64,
    pub gold: u64,
    pub platinum: u64,
    pub self_reported_points: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Minted,
    Unlocked,
    Locked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RarityFrom {
    Rare,
    Steam,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrophyRow {
    pub key: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub icon_gray_url: Option<String>,
    pub masked: bool,
    pub state: RowState,
    /// unix ms of the unlock (local) or the mint
    pub at: Option<i64>,
    pub tier: Option<Tier>,
    pub points: Option<u32>,
    pub rarity: Option<f64>,
    pub rarity_from: Option<RarityFrom>,
    pub def: Option<ChainDef>,
    pub minted: Option<ChainTrophy>,
    pub source: Option<u8>,
    /// One of Rare's own milestones (first launch, hours, streak, ...).
    pub milestone: bool,
    pub hardcore: bool,
    pub trust: Trust,
    /// (have, need) for a milestone that counts up.
    pub progress: Option<(u64, u64)>,
    /// On the profile's showcase.
    pub showcased: bool,
    /// Pinned to chain (the transaction's digest).
    pub pinned: Option<String>,
}

impl TrophyRow {
    /// Points this row is worth once unlocked: the chain's when it has a def, else by its tier.
    pub fn worth(&self) -> u32 {
        self.points.unwrap_or_else(|| rare_trophies::score::points(self.tier.unwrap_or(0)))
    }
}

/// The tier of a row with no chain definition: by its rarity.
pub fn rarity_tier(pct: Option<f64>) -> Tier {
    rare_trophies::score::tier_for_rarity(pct)
}

/// chain.ts `rarity`: share of Rare players with it.
pub fn rarity(def: &ChainDef, game: &ChainGame) -> Option<f64> {
    (game.players > 0).then(|| (def.unlocked as f64 / game.players as f64 * 100.0).min(100.0))
}

/// `gameRows`: Steam's list first, then chain-only defs (the platinum).
pub fn game_rows(steam: Option<&SteamGame>, chain: Option<&ChainGame>, minted: &[ChainTrophy]) -> Vec<TrophyRow> {
    let mine: Vec<&ChainTrophy> = chain.map(|c| minted.iter().filter(|t| t.game == c.id).collect()).unwrap_or_default();
    let by_id = |id: u32| mine.iter().find(|t| t.trophy_id == id).map(|t| (*t).clone());
    let def_of = |k: &str| chain.and_then(|c| c.defs.iter().find(|d| d.key == k)).cloned();
    let mut rows = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for a in steam.map(|s| s.achievements.as_slice()).unwrap_or(&[]) {
        let def = def_of(&a.api_name);
        let m = def.as_ref().and_then(|d| by_id(d.trophy_id));
        let state = if m.is_some() {
            RowState::Minted
        } else if a.unlocked {
            RowState::Unlocked
        } else {
            RowState::Locked
        };
        let rare = match (&def, chain) {
            (Some(d), Some(c)) => rarity(d, c),
            _ => None,
        };
        seen.push(a.api_name.clone());
        rows.push(TrophyRow {
            key: a.api_name.clone(),
            name: a.name.clone(),
            description: a.description.clone(),
            icon_url: a.icon_url.clone().or_else(|| def.as_ref().map(|d| d.image_url.clone())),
            icon_gray_url: a.icon_gray_url.clone(),
            masked: a.hidden && state == RowState::Locked,
            state,
            at: m.as_ref().map(|m| m.unlocked_at_ms).or(a.unlock_time.filter(|t| *t != 0).map(|t| t * 1000)),
            tier: Some(def.as_ref().map(|d| d.tier).unwrap_or_else(|| rarity_tier(a.global_percent))),
            points: Some(def.as_ref().map(|d| d.points).unwrap_or_else(|| rare_trophies::score::points(rarity_tier(a.global_percent)))),
            rarity: rare.or(a.global_percent),
            rarity_from: if rare.is_some() {
                Some(RarityFrom::Rare)
            } else if a.global_percent.is_some() {
                Some(RarityFrom::Steam)
            } else {
                None
            },
            source: m.as_ref().map(|m| m.source),
            def,
            minted: m,
            milestone: false,
            hardcore: a.hardcore && a.unlocked,
            trust: if a.hardcore && a.unlocked { Trust::Verified } else { Trust::Pending },
            progress: None,
            showcased: false,
            pinned: None,
        });
    }
    for d in chain.map(|c| c.defs.as_slice()).unwrap_or(&[]) {
        if seen.contains(&d.key) {
            continue;
        }
        let m = by_id(d.trophy_id);
        let rare = chain.and_then(|c| rarity(d, c));
        rows.push(TrophyRow {
            key: d.key.clone(),
            name: d.name.clone(),
            description: d.description.clone(),
            icon_url: (!d.image_url.is_empty()).then(|| d.image_url.clone()),
            icon_gray_url: None,
            masked: d.hidden && m.is_none(),
            state: if m.is_some() { RowState::Minted } else { RowState::Locked },
            at: m.as_ref().map(|m| m.unlocked_at_ms),
            tier: Some(d.tier),
            points: Some(d.points),
            rarity: rare,
            rarity_from: rare.map(|_| RarityFrom::Rare),
            source: m.as_ref().map(|m| m.source),
            def: Some(d.clone()),
            minted: m,
            milestone: false,
            hardcore: false,
            trust: Trust::Pending,
            progress: None,
            showcased: false,
            pinned: None,
        });
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowFilter {
    All,
    Unlocked,
    Locked,
}

fn order(s: RowState) -> u8 {
    match s {
        RowState::Minted => 0,
        RowState::Unlocked => 1,
        RowState::Locked => 2,
    }
}

/// `sortRows`: minted, then newest unlocks, then common locked, masked last.
pub fn sort_rows(rows: &[TrophyRow], filter: RowFilter) -> Vec<TrophyRow> {
    let mut v: Vec<(usize, &TrophyRow)> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| match filter {
            RowFilter::All => true,
            RowFilter::Locked => r.state == RowState::Locked,
            RowFilter::Unlocked => r.state != RowState::Locked,
        })
        .collect();
    v.sort_by(|(xi, a), (yi, b)| {
        order(a.state).cmp(&order(b.state)).then_with(|| {
            if a.state != RowState::Locked {
                b.at.unwrap_or(0).cmp(&a.at.unwrap_or(0)).then(xi.cmp(yi))
            } else if a.masked != b.masked {
                if a.masked {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                }
            } else {
                b.rarity.unwrap_or(-1.0).partial_cmp(&a.rarity.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal).then(xi.cmp(yi))
            }
        })
    });
    v.into_iter().map(|(_, r)| r.clone()).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GameSummary {
    pub total: u32,
    pub unlocked: u32,
    pub minted: u32,
    pub claimable: u32,
    pub unminted: u32,
    pub points: u32,
    pub pct: u32,
    pub last_at: Option<i64>,
    /// Points of the unlocks that count (verified).
    pub score: u32,
    /// Points of every unlock, counted or not.
    pub score_all: u32,
}

fn claimable(r: &TrophyRow) -> bool {
    r.state == RowState::Unlocked && r.def.as_ref().is_some_and(|d| d.tier != 3)
}

/// `summarize`.
pub fn summarize(rows: &[TrophyRow]) -> GameSummary {
    let total = rows.len() as u32;
    let unlocked = rows.iter().filter(|r| r.state != RowState::Locked).count() as u32;
    GameSummary {
        total,
        unlocked,
        minted: rows.iter().filter(|r| r.state == RowState::Minted).count() as u32,
        claimable: rows.iter().filter(|r| claimable(r)).count() as u32,
        unminted: rows.iter().filter(|r| r.state == RowState::Unlocked).count() as u32,
        points: rows.iter().filter(|r| r.state == RowState::Minted).map(|r| r.points.unwrap_or(0)).sum(),
        pct: if total > 0 { (unlocked as f64 / total as f64 * 100.0).round() as u32 } else { 0 },
        last_at: rows.iter().filter(|r| r.state != RowState::Locked).filter_map(|r| r.at.filter(|a| *a != 0)).max(),
        score: rows.iter().filter(|r| r.state != RowState::Locked && r.trust.counts()).map(|r| r.worth()).sum(),
        score_all: rows.iter().filter(|r| r.state != RowState::Locked).map(|r| r.worth()).sum(),
    }
}

/// `claimableKeys`: what Claim sends for a game (local unlocks with a chain def, not minted, not the platinum).
pub fn claimable_keys(rows: &[TrophyRow]) -> Vec<String> {
    rows.iter().filter(|r| claimable(r)).map(|r| r.key.clone()).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CaseSummary {
    pub points: Option<u64>,
    pub trophies: u64,
    /// bronze, silver, gold, platinum
    pub tiers: [u64; 4],
    pub self_reported_pct: Option<u64>,
    pub steam_unlocked: u64,
    pub steam_total: u64,
    pub games: u64,
    /// The Rare score: points of verified unlocks, every source and Rare's milestones.
    pub score: u64,
    /// Points of every unlock, counted or not.
    pub score_all: u64,
    /// Level, points into it, points it spans.
    pub level: (u32, u64, u64),
    /// Counted unlocks per tier: bronze, silver, gold, platinum.
    pub counted: [u64; 4],
    /// Every unlock (all sources and milestones) and every trophy.
    pub unlocked_all: u64,
    pub total_all: u64,
}

/// Adds the Rare score over every game's rows (source achievements and milestones) to `s`.
pub fn with_score(mut s: CaseSummary, games: &[&[TrophyRow]]) -> CaseSummary {
    let mut counted = [0u64; 4];
    let (mut score, mut all, mut unlocked, mut total) = (0u64, 0u64, 0u64, 0u64);
    for rows in games {
        for r in rows.iter() {
            total += 1;
            if r.state == RowState::Locked {
                continue;
            }
            unlocked += 1;
            all += r.worth() as u64;
            if r.trust.counts() {
                score += r.worth() as u64;
                counted[r.tier.unwrap_or(0).min(3) as usize] += 1;
            }
        }
    }
    s.score = score;
    s.score_all = all;
    s.level = rare_trophies::score::level(score);
    s.counted = counted;
    s.unlocked_all = unlocked;
    s.total_all = total;
    s
}

/// `caseSummary`: the chain profile when there is one, else what is minted.
pub fn case_summary(steam: &[SteamGame], profile: Option<&ChainProfile>, minted: &[ChainTrophy]) -> CaseSummary {
    let mut tiers = [0u64; 4];
    match profile {
        Some(p) => tiers = [p.bronze, p.silver, p.gold, p.platinum],
        None => {
            for t in minted {
                tiers[t.tier.min(3) as usize] += 1;
            }
        }
    }
    let trophies = profile.map(|p| p.trophies).unwrap_or(minted.len() as u64);
    let points = match profile {
        Some(p) => Some(p.points),
        None if !minted.is_empty() => Some(minted.iter().map(|t| t.points as u64).sum()),
        None => None,
    };
    let self_pts = profile.map(|p| p.self_reported_points).unwrap_or_else(|| minted.iter().filter(|t| t.source == 1).map(|t| t.points as u64).sum());
    CaseSummary {
        points,
        trophies,
        tiers,
        self_reported_pct: points.filter(|p| *p > 0).map(|p| (self_pts as f64 / p as f64 * 100.0).round() as u64),
        steam_unlocked: steam.iter().map(|g| g.unlocked as u64).sum(),
        steam_total: steam.iter().map(|g| g.total as u64).sum(),
        games: steam.iter().filter(|g| g.unlocked > 0).count() as u64,
        ..Default::default()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn ach(api: &str, unlocked: bool) -> SteamAchievement {
        SteamAchievement {
            api_name: api.into(),
            name: api.into(),
            description: format!("{api} text"),
            icon_url: Some(format!("https://i/{api}.jpg")),
            icon_gray_url: None,
            hidden: false,
            unlocked,
            unlock_time: unlocked.then_some(1000),
            global_percent: Some(50.0),
            hardcore: false,
        }
    }

    pub fn steam() -> SteamGame {
        let mut b = ach("B", true);
        b.unlock_time = Some(2000);
        let mut c = ach("C", false);
        c.global_percent = Some(80.0);
        let mut d = ach("D", false);
        d.hidden = true;
        d.global_percent = Some(90.0);
        SteamGame { appid: 1, name: "G".into(), total: 4, unlocked: 2, last_unlock: Some(2000), source: "stats".into(), achievements: vec![ach("A", true), b, c, d] }
    }

    fn def(id: u32, key: &str, tier: Tier, unlocked: u64) -> ChainDef {
        ChainDef { trophy_id: id, key: key.into(), name: key.into(), description: String::new(), image_url: String::new(), tier, points: [15, 30, 90, 300][tier as usize], hidden: false, unlocked }
    }

    pub fn chain() -> ChainGame {
        ChainGame { id: "0xg".into(), name: "G".into(), players: 20, defs: vec![def(0, "A", 0, 5), def(1, "B", 2, 5), def(2, "C", 1, 5), def(3, "D", 0, 5), def(4, "PLAT", 3, 1)] }
    }

    pub fn minted() -> Vec<ChainTrophy> {
        vec![ChainTrophy { object_id: "0xt".into(), game: "0xg".into(), trophy_id: 0, name: "A".into(), tier: 0, points: 15, source: 1, number: 3, unlocked_at_ms: 5000 }]
    }

    fn keys(rows: &[TrophyRow]) -> Vec<&str> {
        rows.iter().map(|r| r.key.as_str()).collect()
    }

    #[test]
    fn steam_only() {
        let s = steam();
        let rows = game_rows(Some(&s), None, &[]);
        assert_eq!(rows.iter().map(|r| (r.key.as_str(), r.state, r.masked)).collect::<Vec<_>>(), [("A", RowState::Unlocked, false), ("B", RowState::Unlocked, false), ("C", RowState::Locked, false), ("D", RowState::Locked, true)]);
        assert_eq!((rows[0].tier, rows[0].rarity, rows[0].rarity_from, rows[0].at), (Some(0), Some(50.0), Some(RarityFrom::Steam), Some(1_000_000)));
        assert_eq!(rows[0].worth(), 15);
        assert_eq!(rows[0].trust, Trust::Pending);
        assert!(claimable_keys(&rows).is_empty());
    }

    #[test]
    fn joined_with_chain() {
        let (s, c, m) = (steam(), chain(), minted());
        let rows = game_rows(Some(&s), Some(&c), &m);
        assert_eq!(rows.iter().map(|r| (r.key.as_str(), r.state, r.tier)).collect::<Vec<_>>(), [
            ("A", RowState::Minted, Some(0)),
            ("B", RowState::Unlocked, Some(2)),
            ("C", RowState::Locked, Some(1)),
            ("D", RowState::Locked, Some(0)),
            ("PLAT", RowState::Locked, Some(3))
        ]);
        assert_eq!((rows[0].rarity, rows[0].rarity_from, rows[0].source, rows[0].at), (Some(25.0), Some(RarityFrom::Rare), Some(1), Some(5000)));
        assert_eq!(claimable_keys(&rows), ["B"]);
        let sum = summarize(&rows);
        assert_eq!((sum.total, sum.unlocked, sum.minted, sum.claimable, sum.unminted, sum.points, sum.pct), (5, 2, 1, 1, 1, 15, 40));
        let mut other = m.clone();
        other[0].game = "0xother".into();
        assert_eq!(game_rows(Some(&s), Some(&c), &other)[0].state, RowState::Unlocked);
    }

    #[test]
    fn sorting() {
        let (s, c, m) = (steam(), chain(), minted());
        let rows = game_rows(Some(&s), Some(&c), &m);
        assert_eq!(keys(&sort_rows(&rows, RowFilter::All)), ["A", "B", "C", "PLAT", "D"]);
        assert_eq!(keys(&sort_rows(&rows, RowFilter::Locked)), ["C", "PLAT", "D"]);
        assert_eq!(keys(&sort_rows(&rows, RowFilter::Unlocked)), ["A", "B"]);
    }

    #[test]
    fn rare_score() {
        let s = steam();
        let mut rows = game_rows(Some(&s), None, &[]);
        rows[0].trust = Trust::Verified;
        rows[1].trust = Trust::Flagged("burst".into());
        let sum = summarize(&rows);
        assert_eq!((sum.score, sum.score_all), (15, 30));
        let c = with_score(CaseSummary::default(), &[&rows]);
        assert_eq!((c.score, c.score_all, c.unlocked_all, c.total_all, c.counted), (15, 30, 2, 4, [1, 0, 0, 0]));
        assert_eq!(c.level, (1, 15, 50));
    }

    #[test]
    fn summaries() {
        let p = ChainProfile { points: 300, trophies: 4, bronze: 2, silver: 1, gold: 1, platinum: 0, self_reported_points: 150 };
        let s = case_summary(&[steam()], Some(&p), &minted());
        assert_eq!((s.points, s.trophies, s.self_reported_pct, s.steam_unlocked, s.steam_total, s.games), (Some(300), 4, Some(50), 2, 4, 1));
        assert_eq!(s.tiers, [2, 1, 1, 0]);
        let none = case_summary(&[steam()], None, &[]);
        assert_eq!(none.points, None);
        assert_eq!(none.self_reported_pct, None);
    }
}
