//! The trophy case's pure view logic (trophies/model.ts, types.ts, chain.ts rarity): joins local
//! Steam achievements, the chain's trophy definitions and the player's minted trophies into rows
//! and summaries. No I/O.

use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
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
}

#[derive(Clone, Debug)]
pub struct SteamGame {
    pub appid: u32,
    pub name: String,
    pub achievements: Vec<SteamAchievement>,
    pub total: usize,
    pub unlocked: usize,
}

/// 0 bronze, 1 silver, 2 gold, 3 platinum.
pub type Tier = u8;
pub const TIER_NAME: [&str; 4] = ["Bronze", "Silver", "Gold", "Platinum"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TrophySource {
    Studio,
    SelfReported,
}

#[derive(Clone, Debug)]
pub struct ChainTrophy {
    pub object_id: String,
    pub game: String,
    pub trophy_id: u32,
    pub name: String,
    pub tier: Tier,
    pub points: u32,
    pub source: TrophySource,
    pub unlocked_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct ChainDef {
    pub trophy_id: u32,
    pub key: String,
    pub name: String,
    pub description: String,
    pub image_url: String,
    pub tier: Tier,
    pub points: u32,
    pub hidden: bool,
    pub unlocked: u32,
}

#[derive(Clone, Debug)]
pub struct ChainGame {
    pub id: String,
    pub players: u32,
    pub defs: Vec<ChainDef>,
}

#[derive(Clone, Debug)]
pub struct ChainProfile {
    pub points: u32,
    pub trophies: u32,
    pub bronze: u32,
    pub silver: u32,
    pub gold: u32,
    pub platinum: u32,
    pub self_reported_points: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    Minted,
    Unlocked,
    Locked,
}

impl RowState {
    pub fn label(self) -> &'static str {
        match self {
            RowState::Minted => "Minted",
            RowState::Unlocked => "Unlocked",
            RowState::Locked => "Locked",
        }
    }
    fn order(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RarityFrom {
    Rare,
    Steam,
}

#[derive(Clone, Debug)]
pub struct TrophyRow {
    pub key: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub icon_gray_url: Option<String>,
    /// locked hidden trophy: name, text and art stay covered
    pub masked: bool,
    pub state: RowState,
    /// unix ms of the unlock (local) or the mint
    pub at: Option<i64>,
    pub tier: Option<Tier>,
    pub points: Option<u32>,
    pub rarity: Option<f64>,
    pub rarity_from: Option<RarityFrom>,
    pub has_def: bool,
    pub source: Option<TrophySource>,
}

/// Rarity on Rare: share of the game's players who unlocked this trophy.
pub fn rarity(def: &ChainDef, game: &ChainGame) -> Option<f64> {
    (game.players > 0).then(|| (def.unlocked as f64 / game.players as f64 * 100.0).min(100.0))
}

/// Every trophy of one game: Steam's list first, then chain-only defs (the platinum).
pub fn game_rows(steam: Option<&SteamGame>, chain: Option<&ChainGame>, minted: &[ChainTrophy]) -> Vec<TrophyRow> {
    let mine: Vec<&ChainTrophy> = chain.map(|c| minted.iter().filter(|t| t.game == c.id).collect()).unwrap_or_default();
    let by_id: HashMap<u32, &ChainTrophy> = mine.iter().map(|t| (t.trophy_id, *t)).collect();
    let defs: HashMap<&str, &ChainDef> = chain.map(|c| c.defs.iter().map(|d| (d.key.as_str(), d)).collect()).unwrap_or_default();
    let mut rows = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for a in steam.map(|s| s.achievements.as_slice()).unwrap_or_default() {
        let def = defs.get(a.api_name.as_str()).copied();
        let m = def.and_then(|d| by_id.get(&d.trophy_id).copied());
        let state = if m.is_some() {
            RowState::Minted
        } else if a.unlocked {
            RowState::Unlocked
        } else {
            RowState::Locked
        };
        let rare = def.zip(chain).and_then(|(d, c)| rarity(d, c));
        seen.insert(a.api_name.clone());
        rows.push(TrophyRow {
            key: a.api_name.clone(),
            name: a.name.clone(),
            description: a.description.clone(),
            icon_url: a.icon_url.clone().or_else(|| def.map(|d| d.image_url.clone())),
            icon_gray_url: a.icon_gray_url.clone(),
            masked: a.hidden && state == RowState::Locked,
            state,
            at: m.map(|m| m.unlocked_at_ms).or_else(|| a.unlock_time.filter(|t| *t != 0).map(|t| t * 1000)),
            tier: def.map(|d| d.tier),
            points: def.map(|d| d.points),
            rarity: rare.or(a.global_percent),
            rarity_from: if rare.is_some() {
                Some(RarityFrom::Rare)
            } else if a.global_percent.is_some() {
                Some(RarityFrom::Steam)
            } else {
                None
            },
            has_def: def.is_some(),
            source: m.map(|m| m.source),
        });
    }
    for def in chain.map(|c| c.defs.as_slice()).unwrap_or_default() {
        if seen.contains(&def.key) {
            continue;
        }
        let m = by_id.get(&def.trophy_id).copied();
        let rare = chain.and_then(|c| rarity(def, c));
        rows.push(TrophyRow {
            key: def.key.clone(),
            name: def.name.clone(),
            description: def.description.clone(),
            icon_url: (!def.image_url.is_empty()).then(|| def.image_url.clone()),
            icon_gray_url: None,
            masked: def.hidden && m.is_none(),
            state: if m.is_some() { RowState::Minted } else { RowState::Locked },
            at: m.map(|m| m.unlocked_at_ms),
            tier: Some(def.tier),
            points: Some(def.points),
            rarity: rare,
            rarity_from: rare.map(|_| RarityFrom::Rare),
            has_def: true,
            source: m.map(|m| m.source),
        });
    }
    rows
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RowFilter {
    All,
    Unlocked,
    Locked,
}

/// Minted first, then unlocked (newest first), then locked (most common first, masked last).
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
    v.sort_by(|(i, a), (j, b)| {
        a.state
            .order()
            .cmp(&b.state.order())
            .then_with(|| {
                if a.state != RowState::Locked {
                    b.at.unwrap_or(0).cmp(&a.at.unwrap_or(0))
                } else if a.masked != b.masked {
                    a.masked.cmp(&b.masked)
                } else {
                    b.rarity.unwrap_or(-1.0).partial_cmp(&a.rarity.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal)
                }
            })
            .then(i.cmp(j))
    });
    v.into_iter().map(|(_, r)| r.clone()).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameSummary {
    pub total: usize,
    pub unlocked: usize,
    pub minted: usize,
    pub claimable: usize,
    pub unminted: usize,
    pub points: u32,
    pub pct: u32,
    pub last_at: Option<i64>,
}

pub fn summarize(rows: &[TrophyRow]) -> GameSummary {
    let total = rows.len();
    let unlocked = rows.iter().filter(|r| r.state != RowState::Locked).count();
    let at = rows.iter().filter(|r| r.state != RowState::Locked).filter_map(|r| r.at.filter(|a| *a != 0)).max();
    GameSummary {
        total,
        unlocked,
        minted: rows.iter().filter(|r| r.state == RowState::Minted).count(),
        claimable: rows.iter().filter(|r| r.state == RowState::Unlocked && r.has_def && r.tier != Some(3)).count(),
        unminted: rows.iter().filter(|r| r.state == RowState::Unlocked).count(),
        points: rows.iter().filter(|r| r.state == RowState::Minted).map(|r| r.points.unwrap_or(0)).sum(),
        pct: if total > 0 { (unlocked as f64 / total as f64 * 100.0).round() as u32 } else { 0 },
        last_at: at,
    }
}

/// Rows that Claim would send for a game (local unlocks with a chain def, not minted, not the platinum).
pub fn claimable_keys(rows: &[TrophyRow]) -> Vec<String> {
    rows.iter().filter(|r| r.state == RowState::Unlocked && r.has_def && r.tier != Some(3)).map(|r| r.key.clone()).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseSummary {
    pub points: Option<u32>,
    pub trophies: u32,
    pub tiers: [u32; 4],
    pub self_reported_pct: Option<u32>,
    pub steam_unlocked: usize,
    pub steam_total: usize,
    pub games: usize,
}

pub fn case_summary(steam: &[SteamGame], profile: Option<&ChainProfile>, minted: &[ChainTrophy]) -> CaseSummary {
    let mut tiers = [0u32; 4];
    if let Some(p) = profile {
        tiers = [p.bronze, p.silver, p.gold, p.platinum];
    } else {
        for t in minted {
            tiers[t.tier as usize] += 1;
        }
    }
    let trophies = profile.map(|p| p.trophies).unwrap_or(minted.len() as u32);
    let points = match profile {
        Some(p) => Some(p.points),
        None if !minted.is_empty() => Some(minted.iter().map(|t| t.points).sum()),
        None => None,
    };
    let self_pts = match profile {
        Some(p) => p.self_reported_points,
        None => minted.iter().filter(|t| t.source == TrophySource::SelfReported).map(|t| t.points).sum(),
    };
    CaseSummary {
        points,
        trophies,
        tiers,
        self_reported_pct: points.filter(|p| *p > 0).map(|p| (self_pts as f64 / p as f64 * 100.0).round() as u32),
        steam_unlocked: steam.iter().map(|g| g.unlocked).sum(),
        steam_total: steam.iter().map(|g| g.total).sum(),
        games: steam.iter().filter(|g| g.unlocked > 0).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ach(name: &str, unlocked: bool, f: impl FnOnce(&mut SteamAchievement)) -> SteamAchievement {
        let mut a = SteamAchievement {
            api_name: name.into(),
            name: name.into(),
            description: format!("{name} text"),
            icon_url: Some(format!("https://i/{name}.jpg")),
            icon_gray_url: None,
            hidden: false,
            unlocked,
            unlock_time: unlocked.then_some(1000),
            global_percent: Some(50.0),
        };
        f(&mut a);
        a
    }

    fn steam() -> SteamGame {
        SteamGame {
            appid: 1,
            name: "G".into(),
            total: 4,
            unlocked: 2,
            achievements: vec![ach("A", true, |_| {}), ach("B", true, |a| a.unlock_time = Some(2000)), ach("C", false, |a| a.global_percent = Some(80.0)), ach("D", false, |a| {
                a.hidden = true;
                a.global_percent = Some(90.0)
            })],
        }
    }

    fn def(id: u32, key: &str, tier: u8, unlocked: u32) -> ChainDef {
        ChainDef { trophy_id: id, key: key.into(), name: key.into(), description: String::new(), image_url: String::new(), tier, points: [15, 30, 90, 300][tier as usize], hidden: false, unlocked }
    }

    fn chain() -> ChainGame {
        ChainGame { id: "0xg".into(), players: 20, defs: vec![def(0, "A", 0, 5), def(1, "B", 2, 5), def(2, "C", 1, 5), def(3, "D", 0, 5), def(4, "PLAT", 3, 1)] }
    }

    fn minted() -> Vec<ChainTrophy> {
        vec![ChainTrophy { object_id: "0xt".into(), game: "0xg".into(), trophy_id: 0, name: "A".into(), tier: 0, points: 15, source: TrophySource::SelfReported, unlocked_at_ms: 5000 }]
    }

    #[test]
    fn steam_only_without_a_chain_hidden_locked_masked() {
        let rows = game_rows(Some(&steam()), None, &[]);
        let got: Vec<_> = rows.iter().map(|r| (r.key.as_str(), r.state, r.masked)).collect();
        assert_eq!(got, [("A", RowState::Unlocked, false), ("B", RowState::Unlocked, false), ("C", RowState::Locked, false), ("D", RowState::Locked, true)]);
        assert_eq!((rows[0].tier, rows[0].rarity, rows[0].rarity_from, rows[0].at), (None, Some(50.0), Some(RarityFrom::Steam), Some(1_000_000)));
        assert!(claimable_keys(&rows).is_empty());
    }

    #[test]
    fn joins_chain_defs_and_minted_trophies() {
        let c = chain();
        let rows = game_rows(Some(&steam()), Some(&c), &minted());
        let got: Vec<_> = rows.iter().map(|r| (r.key.as_str(), r.state, r.tier)).collect();
        assert_eq!(
            got,
            [("A", RowState::Minted, Some(0)), ("B", RowState::Unlocked, Some(2)), ("C", RowState::Locked, Some(1)), ("D", RowState::Locked, Some(0)), ("PLAT", RowState::Locked, Some(3))]
        );
        assert_eq!((rows[0].rarity, rows[0].rarity_from, rows[0].source, rows[0].at), (Some(25.0), Some(RarityFrom::Rare), Some(TrophySource::SelfReported), Some(5000)));
        assert_eq!(claimable_keys(&rows), ["B"]);
        let s = summarize(&rows);
        assert_eq!((s.total, s.unlocked, s.minted, s.claimable, s.unminted, s.points, s.pct), (5, 2, 1, 1, 1, 15, 40));
    }

    #[test]
    fn ignores_trophies_minted_in_other_games() {
        let c = chain();
        let mut m = minted();
        m[0].game = "0xother".into();
        assert_eq!(game_rows(Some(&steam()), Some(&c), &m)[0].state, RowState::Unlocked);
    }

    #[test]
    fn sorts_minted_then_newest_then_common_locked_masked_last() {
        let c = chain();
        let rows = game_rows(Some(&steam()), Some(&c), &minted());
        let keys = |v: Vec<TrophyRow>| v.into_iter().map(|r| r.key).collect::<Vec<_>>();
        assert_eq!(keys(sort_rows(&rows, RowFilter::All)), ["A", "B", "C", "PLAT", "D"]);
        assert_eq!(keys(sort_rows(&rows, RowFilter::Locked)), ["C", "PLAT", "D"]);
        assert_eq!(keys(sort_rows(&rows, RowFilter::Unlocked)), ["A", "B"]);
    }

    #[test]
    fn case_summary_uses_the_chain_profile() {
        let p = ChainProfile { points: 300, trophies: 4, bronze: 2, silver: 1, gold: 1, platinum: 0, self_reported_points: 150 };
        let s = case_summary(&[steam()], Some(&p), &minted());
        assert_eq!((s.points, s.trophies, s.self_reported_pct, s.steam_unlocked, s.steam_total, s.games), (Some(300), 4, Some(50), 2, 4, 1));
        assert_eq!(s.tiers, [2, 1, 1, 0]);
    }

    #[test]
    fn no_points_before_any_trophy() {
        let s = case_summary(&[steam()], None, &[]);
        assert_eq!((s.points, s.self_reported_pct), (None, None));
    }
}
