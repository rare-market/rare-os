//! The Switcher's pure parts (menuModel.ts inOrder and recentNotRunning, native.ts SwitcherState)
//! and its fixtures.

use rare_render::Src;

#[derive(Clone, Debug)]
pub struct RunItem {
    pub rid: String,
    pub name: String,
    pub app: bool,
    pub art: Option<Src>,
    pub icon: Option<Src>,
}

#[derive(Clone, Debug)]
pub struct RecentGame {
    pub id: String,
    pub title: String,
    pub installed: bool,
    pub last_played: i64,
    pub cover: Option<Src>,
}

#[derive(Clone, Debug, Default)]
pub struct CardState {
    pub rid: String,
    pub paused: bool,
    pub frame: Option<Src>,
}

#[derive(Clone, Debug, Default)]
pub struct SwitcherState {
    pub cards: Vec<CardState>,
    pub from: Option<String>,
    pub max_paused: u32,
}

impl SwitcherState {
    pub fn order(&self) -> Vec<String> {
        self.cards.iter().map(|c| c.rid.clone()).collect()
    }
    pub fn card(&self, rid: &str) -> Option<&CardState> {
        self.cards.iter().find(|c| c.rid == rid)
    }
}

#[derive(Clone, Debug)]
pub struct FreezeFull {
    pub pending: String,
    pub frozen: Vec<String>,
}

/// Running items in the native side's order (most recently used first); any it doesn't list yet after.
pub fn in_order(mut items: Vec<RunItem>, order: &[String]) -> Vec<RunItem> {
    let rank = |rid: &str| order.iter().position(|o| o == rid).unwrap_or(order.len());
    items.sort_by_key(|i| rank(&i.rid));
    items
}

/// Up to `n` recent games that aren't running, newest first.
pub fn recent_not_running(games: &[RecentGame], running: &[String], n: usize) -> Vec<RecentGame> {
    let mut v: Vec<RecentGame> = games.iter().filter(|g| g.installed && !running.contains(&g.id) && g.last_played > 0).cloned().collect();
    v.sort_by_key(|g| std::cmp::Reverse(g.last_played));
    v.truncate(n);
    v
}

static ACT_HERO: &[u8] = include_bytes!("../../../../../render/assets/games/act/hero.jpg");
static ACT_COVER: &[u8] = include_bytes!("../../../../../render/assets/games/act/cover.jpg");
static MARK: &[u8] = include_bytes!("../../../../../render/assets/brand/mark.png");

pub fn fixture_running() -> Vec<RunItem> {
    vec![
        RunItem { rid: "steam:730".into(), name: "Counter-Strike 2".into(), app: false, art: Some(Src::Static("fx/act-hero.jpg", ACT_HERO)), icon: None },
        RunItem { rid: "web:wordgame".into(), name: "Word Game".into(), app: false, art: Some(Src::Static("fx/act-hero.jpg", ACT_HERO)), icon: None },
        RunItem { rid: "app:telegram".into(), name: "Telegram".into(), app: true, art: None, icon: Some(Src::Static("fx/mark.png", MARK)) },
    ]
}

pub fn fixture_state() -> SwitcherState {
    SwitcherState {
        cards: vec![
            CardState { rid: "web:wordgame".into(), paused: false, frame: None },
            CardState { rid: "steam:730".into(), paused: true, frame: Some(Src::Static("fx/act-hero.jpg", ACT_HERO)) },
            CardState { rid: "app:telegram".into(), paused: false, frame: None },
        ],
        from: Some("web:act".into()),
        max_paused: 2,
    }
}

pub fn fixture_library() -> Vec<RecentGame> {
    let g = |id: &str, t: &str, lp: i64, c: &'static [u8], n: &'static str| RecentGame { id: id.into(), title: t.into(), installed: true, last_played: lp, cover: Some(Src::Static(n, c)) };
    vec![
        g("steam:1", "Hades", 50, ACT_COVER, "fx/act-cover.jpg"),
        g("steam:2", "Celeste", 40, ACT_COVER, "fx/act-cover.jpg"),
        g("steam:3", "Hollow Knight", 30, ACT_COVER, "fx/act-cover.jpg"),
        g("steam:4", "Balatro", 20, ACT_COVER, "fx/act-cover.jpg"),
        g("steam:5", "Older", 10, ACT_COVER, "fx/act-cover.jpg"),
    ]
}

pub fn fixture_freeze_full() -> FreezeFull {
    FreezeFull { pending: "app:telegram".into(), frozen: vec!["steam:730".into(), "web:wordgame".into()] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(rid: &str) -> RunItem {
        RunItem { rid: rid.into(), name: rid.into(), app: false, art: None, icon: None }
    }

    #[test]
    fn running_follow_the_native_order_unknown_last() {
        let v = in_order(vec![it("a"), it("b"), it("c")], &["c".into(), "a".into()]);
        assert_eq!(v.iter().map(|i| i.rid.as_str()).collect::<Vec<_>>(), ["c", "a", "b"]);
    }

    #[test]
    fn recent_skips_running_and_never_played_newest_first_max_four() {
        let games = fixture_library();
        let v = recent_not_running(&games, &["steam:2".into()], 4);
        assert_eq!(v.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["steam:1", "steam:3", "steam:4", "steam:5"]);
        let mut g = games.clone();
        g[0].installed = false;
        g[1].last_played = 0;
        assert_eq!(recent_not_running(&g, &[], 4).len(), 3);
    }
}
