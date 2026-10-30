//! Top traders from the ledger's board: a minimum of activity so one lucky trade does not rank,
//! wallets the ledger flags as wash or bot-like left out, then sorted by realized P/L or win rate.

use super::api::{BoardExtras, BoardTag, BoardWindow, LeaderRow};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardSort {
    #[default]
    Pnl,
    WinRate,
}

impl BoardSort {
    pub fn label(&self) -> &'static str {
        match self {
            BoardSort::Pnl => "Realized P/L",
            BoardSort::WinRate => "Win rate",
        }
    }
}

/// The board's window and sort (the web launcher's `useBoardPrefs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardPrefs {
    pub window: BoardWindow,
    pub sort: BoardSort,
}

/// At least this many trades in the window...
pub const MIN_TRADES: f64 = 10.0;
/// ...and this much volume, in USD.
pub const MIN_VOLUME_USD: f64 = 1_000.0;

/// Tags that mean the numbers are not real trading. The ledger tags bot-like wallets (very many
/// trades with almost no edge); a `wash` tag counts the same if the ledger adds one.
const FLAGGED: [&str; 2] = ["bot", "wash"];

#[derive(Clone, Debug, PartialEq)]
pub struct TopTrader {
    pub address: String,
    pub row: LeaderRow,
    pub tags: Vec<BoardTag>,
    /// cumulative realized P/L by day over 30 days, when the board has it
    pub curve: Vec<f64>,
    /// wallets in this trader's funding-linked group, itself included; None when not linked
    pub linked: Option<u32>,
}

pub fn is_flagged(tags: &[BoardTag]) -> bool {
    tags.iter().any(|t| FLAGGED.contains(&t.key.as_str()))
}

/// Enough activity to count: trades and volume over the minimums.
pub fn is_active(r: &LeaderRow, min_trades: f64, min_volume: f64) -> bool {
    r.trades >= min_trades && r.volume_usd >= min_volume
}

/// Funding-linked group sizes by wallet, from the team board (its best wallet stands for it).
pub fn linked_sizes(teams: &[LeaderRow]) -> HashMap<String, u32> {
    teams.iter().filter_map(|t| t.team.as_ref().filter(|x| x.members > 1).map(|x| (t.user.address.to_lowercase(), x.members))).collect()
}

#[derive(Clone, Debug)]
pub struct BoardOpts<'a> {
    pub sort: BoardSort,
    pub teams: Option<&'a HashMap<String, u32>>,
    pub min_trades: f64,
    pub min_volume: f64,
    pub limit: usize,
}

impl Default for BoardOpts<'_> {
    fn default() -> Self {
        BoardOpts { sort: BoardSort::Pnl, teams: None, min_trades: MIN_TRADES, min_volume: MIN_VOLUME_USD, limit: 50 }
    }
}

pub fn top_traders(rows: &[LeaderRow], extras: Option<&BoardExtras>, o: &BoardOpts) -> Vec<TopTrader> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let address = row.user.address.to_lowercase();
        if !seen.insert(address.clone()) {
            continue;
        }
        let x = extras.and_then(|e| e.rows.get(&row.user.address).or_else(|| e.rows.get(&address)));
        let tags = x.map(|x| x.tags.clone()).unwrap_or_default();
        if is_flagged(&tags) || !is_active(row, o.min_trades, o.min_volume) {
            continue;
        }
        out.push(TopTrader { linked: o.teams.and_then(|t| t.get(&address).copied()), address, row: row.clone(), tags, curve: x.map(|x| x.curve30d.clone()).unwrap_or_default() });
    }
    if o.sort == BoardSort::WinRate {
        // Win rate only means something for a trader who made money and has a known rate.
        out.retain(|t| t.row.win_rate.is_some() && t.row.pnl_usd > 0.0);
        out.sort_by(|a, b| b.row.win_rate.unwrap_or(0.0).total_cmp(&a.row.win_rate.unwrap_or(0.0)).then(b.row.pnl_usd.total_cmp(&a.row.pnl_usd)));
    } else {
        out.sort_by(|a, b| b.row.pnl_usd.total_cmp(&a.row.pnl_usd));
    }
    out.truncate(o.limit);
    out
}

/// Style tags worth a word in a row (the flagged ones never reach a row).
pub fn style_words(tags: &[BoardTag]) -> Vec<&str> {
    tags.iter().filter(|t| !FLAGGED.contains(&t.key.as_str())).map(|t| t.label.as_str()).collect()
}

#[cfg(test)]
mod tests {
    //! board.test.ts
    use super::super::api::{ExtrasRow, Team, UserChip};
    use super::*;

    fn addr(n: u32) -> String {
        format!("0x{n:064x}")
    }
    fn row(n: u32) -> LeaderRow {
        LeaderRow { rank: n, user: UserChip { address: addr(n), ..UserChip::default() }, pnl_usd: 1000.0 - n as f64, volume_usd: 50_000.0, trades: 40.0, win_rate: Some(0.5), top_tokens: vec![], token_count: 3, team: None }
    }
    fn with(n: u32, f: impl FnOnce(&mut LeaderRow)) -> LeaderRow {
        let mut r = row(n);
        f(&mut r);
        r
    }
    fn extras(tags: &[(u32, &[&str])]) -> BoardExtras {
        BoardExtras {
            available: true,
            rows: tags.iter().map(|(n, keys)| (addr(*n), ExtrasRow { tags: keys.iter().map(|k| BoardTag { key: k.to_string(), label: k.to_string(), reason: String::new() }).collect(), curve30d: vec![0.0, 1.0, 2.0], copy_pnl_usd: None })).collect(),
        }
    }
    fn ranks(v: &[TopTrader]) -> Vec<u32> {
        v.iter().map(|t| t.row.rank).collect()
    }
    fn by(sort: BoardSort) -> BoardOpts<'static> {
        BoardOpts { sort, ..BoardOpts::default() }
    }

    #[test]
    fn keeps_only_traders_over_the_activity_minimum() {
        let rows = [row(1), with(2, |r| r.trades = MIN_TRADES - 1.0), with(3, |r| r.volume_usd = 500.0), with(4, |r| r.trades = MIN_TRADES)];
        assert_eq!(ranks(&top_traders(&rows, None, &by(BoardSort::Pnl))), [1, 4]);
    }

    #[test]
    fn leaves_out_bot_like_and_wash_flagged_wallets() {
        let rows = [row(1), row(2), row(3)];
        let x = extras(&[(1, &["bot"]), (2, &["wash"]), (3, &["scalper"])]);
        let out = top_traders(&rows, Some(&x), &by(BoardSort::Pnl));
        assert_eq!(ranks(&out), [3]);
        assert_eq!(out[0].curve, vec![0.0, 1.0, 2.0]);
        assert!(is_flagged(&[BoardTag { key: "bot".into(), ..BoardTag::default() }]));
        assert_eq!(style_words(&out[0].tags), ["scalper"]);
    }

    #[test]
    fn sorts_by_realized_pnl_or_by_win_rate_among_profitable_traders() {
        let rows = [
            with(1, |r| (r.pnl_usd, r.win_rate) = (10.0, Some(0.9))),
            with(2, |r| (r.pnl_usd, r.win_rate) = (500.0, Some(0.4))),
            with(3, |r| (r.pnl_usd, r.win_rate) = (-50.0, Some(1.0))),
            with(4, |r| (r.pnl_usd, r.win_rate) = (80.0, None)),
            with(5, |r| (r.pnl_usd, r.win_rate) = (20.0, Some(0.9))),
        ];
        assert_eq!(ranks(&top_traders(&rows, None, &by(BoardSort::Pnl))), [2, 4, 5, 1, 3]);
        // losing (3) and unknown (4) win rates drop out; ties go to the bigger P/L
        assert_eq!(ranks(&top_traders(&rows, None, &by(BoardSort::WinRate))), [5, 1, 2]);
    }

    #[test]
    fn marks_funding_linked_groups_from_the_team_board_and_drops_repeated_wallets() {
        let teams = linked_sizes(&[with(9, |r| r.team = Some(Team { members: 3, entity: "e".into() })), with(8, |r| r.team = Some(Team { members: 1, entity: "f".into() }))]);
        assert_eq!(teams.get(&addr(9)), Some(&3));
        assert!(!teams.contains_key(&addr(8)));
        let out = top_traders(&[row(9), row(9), row(1)], None, &BoardOpts { teams: Some(&teams), ..BoardOpts::default() });
        assert_eq!(out.iter().map(|t| (t.row.rank, t.linked)).collect::<Vec<_>>(), [(1, None), (9, Some(3))]);
    }
}
