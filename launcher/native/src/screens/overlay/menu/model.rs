//! The Rare menu's pure parts (overlay/menuModel.ts, background.ts playingFor): which tabs show,
//! where the menu opens, the per-kind rows, the items line, the Switcher's order. Tests mirror
//! menu.test.ts one to one.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuTab {
    Game,
    Rare,
    /// Ask Rare (features/agent).
    Ask,
    Items,
    Market,
    Trophies,
    Friends,
    News,
}

impl MenuTab {
    /// The tab strip label (RareMenu.tsx LABEL).
    pub fn label(self) -> &'static str {
        match self {
            MenuTab::Game => "Game",
            MenuTab::Rare => "Rare",
            MenuTab::Ask => "Ask Rare",
            MenuTab::Items => "Items",
            MenuTab::Market => "Market",
            MenuTab::Trophies => "Trophies",
            MenuTab::Friends => "Friends",
            MenuTab::News => "Notifications",
        }
    }
}

/// The tabs, in order, for what is under the menu.
pub fn menu_tabs(over_game: bool, collections: bool, trophies: bool) -> Vec<MenuTab> {
    let mut t = vec![if over_game { MenuTab::Game } else { MenuTab::Rare }, MenuTab::Ask];
    if over_game && collections {
        t.push(MenuTab::Items);
        t.push(MenuTab::Market);
    }
    if over_game && trophies {
        t.push(MenuTab::Trophies);
    }
    t.push(MenuTab::Friends);
    t.push(MenuTab::News);
    t
}

/// Where the menu opens: Game over a game; over the launcher Notifications when any are unread, else Rare.
pub fn opening_tab(over_game: bool, unread: u32) -> MenuTab {
    if over_game {
        MenuTab::Game
    } else if unread > 0 {
        MenuTab::News
    } else {
        MenuTab::Rare
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameKind {
    Steam,
    App,
    Stream,
    Web,
    Game,
}

/// What is under the menu, for the rows that differ per kind.
pub fn game_kind(rid: Option<&str>) -> Option<GameKind> {
    let rid = rid.filter(|r| !r.is_empty())?;
    Some(if rid.starts_with("steam:") {
        GameKind::Steam
    } else if rid.starts_with("app:") {
        GameKind::App
    } else if rid.starts_with("stream:") {
        GameKind::Stream
    } else if rid.starts_with("web:") {
        GameKind::Web
    } else {
        GameKind::Game
    })
}

/// One of your items from this game (data.ts OverlayItem, the fields the menu reads).
#[derive(Clone, Debug, Default)]
pub struct OverlayItem {
    pub name: String,
    pub floor_mist: Option<u128>,
    pub top_offer_mist: Option<u128>,
}

const MIST: u128 = 1_000_000_000;

/// fmtSui(v, { dp: 2 }): two decimals with thousands separators.
pub fn fmt_sui2(mist: u128) -> String {
    let cents = (mist + MIST / 200) / (MIST / 100);
    let (whole, frac) = (cents / 100, cents % 100);
    let w = whole.to_string();
    let mut out = String::new();
    for (i, c) in w.chars().enumerate() {
        if i > 0 && (w.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out}.{frac:02}")
}

/// "22 items · worth 98.60 SUI · 9 offers" for your items from this game.
pub fn items_line(items: &[OverlayItem]) -> String {
    let worth: u128 = items.iter().map(|x| x.floor_mist.unwrap_or(0)).sum();
    let offers = items.iter().filter(|x| x.top_offer_mist.unwrap_or(0) > 0).count();
    let mut parts = vec![if items.len() == 1 { "1 item".to_string() } else { format!("{} items", items.len()) }];
    if worth > 0 {
        parts.push(format!("worth {} SUI", fmt_sui2(worth)));
    }
    if offers > 0 {
        parts.push(if offers == 1 { "1 offer".to_string() } else { format!("{offers} offers") });
    }
    parts.join(" · ")
}

/// "Playing for 42 min", "Playing for 1 h 5 min"; None when unknown. Times are ms.
pub fn playing_for(since: Option<f64>, now: f64) -> Option<String> {
    let since = since?;
    let min = ((now - since) / 60_000.0).floor().max(0.0) as u64;
    Some(if min < 1 {
        "Just started".to_string()
    } else if min < 60 {
        format!("Playing for {min} min")
    } else {
        let (h, m) = (min / 60, min % 60);
        if m > 0 {
            format!("Playing for {h} h {m} min")
        } else {
            format!("Playing for {h} h")
        }
    })
}

/// A library game, the fields the Switcher's order reads.
#[derive(Clone, Debug)]
pub struct LibGame {
    pub id: String,
    pub installed: bool,
    pub last_played: Option<f64>,
}

/// Up to `n` recent games that aren't running, newest first.
pub fn recent_not_running<'a>(games: &'a [LibGame], running: &[&str], n: usize) -> Vec<&'a LibGame> {
    let mut v: Vec<&LibGame> = games.iter().filter(|g| g.installed && !running.contains(&g.id.as_str()) && g.last_played.unwrap_or(0.0) > 0.0).collect();
    v.sort_by(|a, b| b.last_played.unwrap_or(0.0).partial_cmp(&a.last_played.unwrap_or(0.0)).unwrap());
    v.truncate(n);
    v
}

/// Running items in the native side's order (most recently used first); any it doesn't list yet after.
pub fn in_order<T: Clone>(items: &[T], rid: impl Fn(&T) -> &str, order: &[&str]) -> Vec<T> {
    let rank = |r: &str| order.iter().position(|x| *x == r).unwrap_or(order.len());
    let mut v = items.to_vec();
    v.sort_by_key(|x| rank(rid(x)));
    v
}

/// A notice toast still on screen.
pub struct ToastLike {
    pub kind: String,
    pub at: f64,
}

/// The Rare button acts on the toast that is up: a party invite opens the menu on Friends with Join.
pub fn invite_up(toasts: &[ToastLike], now: f64, show_ms: f64) -> bool {
    toasts.iter().any(|t| t.kind == "party_invite" && now - t.at < show_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use MenuTab::*;

    #[test]
    fn tabs_over_a_sui_game() {
        assert_eq!(menu_tabs(true, true, true), vec![Game, Ask, Items, Market, Trophies, Friends, News]);
    }
    #[test]
    fn tabs_over_a_steam_game_with_no_collections() {
        assert_eq!(menu_tabs(true, false, true), vec![Game, Ask, Trophies, Friends, News]);
    }
    #[test]
    fn tabs_over_an_app() {
        assert_eq!(menu_tabs(true, false, false), vec![Game, Ask, Friends, News]);
    }
    #[test]
    fn over_the_launcher_game_is_replaced_by_rare() {
        assert_eq!(menu_tabs(false, true, true), vec![Rare, Ask, Friends, News]);
    }
    #[test]
    fn opens_on_game_over_a_game() {
        assert_eq!(opening_tab(true, 0), Game);
        assert_eq!(opening_tab(true, 5), Game);
    }
    #[test]
    fn over_the_launcher_news_when_unread() {
        assert_eq!(opening_tab(false, 2), News);
        assert_eq!(opening_tab(false, 0), Rare);
    }
    #[test]
    fn kinds() {
        assert_eq!(game_kind(Some("steam:620")), Some(GameKind::Steam));
        assert_eq!(game_kind(Some("app:telegram")), Some(GameKind::App));
        assert_eq!(game_kind(Some("stream:abc:1")), Some(GameKind::Stream));
        assert_eq!(game_kind(Some("web:act")), Some(GameKind::Web));
        assert_eq!(game_kind(Some("local:hades")), Some(GameKind::Game));
        assert_eq!(game_kind(None), None);
    }

    fn item(floor: Option<u128>, offer: Option<u128>) -> OverlayItem {
        OverlayItem { name: String::new(), floor_mist: floor, top_offer_mist: offer }
    }
    #[test]
    fn items_line_counts_worth_and_offers() {
        assert_eq!(items_line(&[item(Some(2 * MIST), Some(MIST)), item(Some(3 * MIST), None), item(Some(5 * MIST), Some(2 * MIST))]), "3 items · worth 10.00 SUI · 2 offers");
    }
    #[test]
    fn items_line_leaves_out_what_it_does_not_know() {
        assert_eq!(items_line(&[item(None, None)]), "1 item");
        assert_eq!(items_line(&[item(None, Some(MIST))]), "1 item · 1 offer");
    }
    #[test]
    fn sui_format() {
        assert_eq!(fmt_sui2(98_600_000_000), "98.60");
        assert_eq!(fmt_sui2(1_234_567_000_000), "1,234.57");
        assert_eq!(fmt_sui2(0), "0.00");
    }

    fn g(id: &str, lp: Option<f64>, installed: bool) -> LibGame {
        LibGame { id: id.into(), installed, last_played: lp }
    }
    #[test]
    fn recent_games_not_running() {
        let games = vec![g("a", Some(10.0), true), g("b", Some(50.0), true), g("c", Some(30.0), true), g("d", Some(40.0), true), g("e", Some(20.0), true), g("f", None, true), g("x", Some(99.0), false), g("r", Some(100.0), true)];
        let ids: Vec<&str> = recent_not_running(&games, &["r"], 4).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "d", "c", "e"]);
    }
    #[test]
    fn running_cards_follow_native_order() {
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(in_order(&items, |x| x.as_str(), &["c", "a"]), vec!["c", "a", "b"]);
        assert_eq!(in_order(&items, |x| x.as_str(), &[]), vec!["a", "b", "c"]);
    }

    #[test]
    fn playing_for_reads_like_the_game_card() {
        let now = 10_000_000.0;
        assert_eq!(playing_for(None, now), None);
        assert_eq!(playing_for(Some(now - 20_000.0), now).as_deref(), Some("Just started"));
        assert_eq!(playing_for(Some(now - 42.0 * 60_000.0), now).as_deref(), Some("Playing for 42 min"));
        assert_eq!(playing_for(Some(now - 65.0 * 60_000.0), now).as_deref(), Some("Playing for 1 h 5 min"));
        assert_eq!(playing_for(Some(now - 120.0 * 60_000.0), now).as_deref(), Some("Playing for 2 h"));
    }

    #[test]
    fn invite_toast() {
        let t = |k: &str, at: f64| ToastLike { kind: k.into(), at };
        assert!(invite_up(&[t("party_invite", 1000.0)], 5000.0, 6000.0));
        assert!(!invite_up(&[t("party_invite", 1000.0)], 8000.0, 6000.0));
        assert!(!invite_up(&[t("sale", 1000.0), t("friend_online", 4000.0)], 5000.0, 6000.0));
        assert!(!invite_up(&[], 5000.0, 6000.0));
    }
}
