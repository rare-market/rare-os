//! Joining a friend's game, and describing the game the player is in for presence and parties.
//!
//! Web games: the same URL, with the party's invite value in the game's own parameter when the
//! game takes one ([`INVITE_PARAM`]). ACT has squads and parties, but they live in its own account
//! system (its social server invites by ACT account and follows the leader between modes); its URL
//! only takes `?go=<mode>` and `#/<scene>`, never a party. So ACT gets the same URL and players
//! group up in ACT's party screen. A game that adds an invite parameter goes in INVITE_PARAM.
//!
//! Steam games: Steam's own join (`steam://joinlobby/<app>/<lobby>/<steamid>`) needs a lobby id
//! only the game and Steam know, and would tie a Sui address to a Steam account. So Join starts
//! the same game through Steam (`steam://rungameid/<app>`, the library's launch), where the
//! friend's in-game Steam invite lands; a game this device does not have opens its Steam page.
//!
//! What Join then does (launch, open the web game window, open Steam, toast) is the host's: it
//! acts on the [`JoinPlan`] and shows [`JoinPlan::toast`].

use crate::types::{GameKind, GameRef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Web game id (library id without `web:`) to the URL parameter it reads an invite from.
pub const INVITE_PARAM: &[(&str, &str)] = &[];

/// The invite parameters in use: [`INVITE_PARAM`] by default; tests add their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invites(pub HashMap<String, String>);

impl Default for Invites {
    fn default() -> Self {
        Invites(INVITE_PARAM.iter().map(|(g, p)| (g.to_string(), p.to_string())).collect())
    }
}

impl Invites {
    pub fn param(&self, game_id: &str) -> Option<&str> {
        self.0.get(bare(game_id)).map(String::as_str).filter(|p| !p.is_empty())
    }
}

/// How the native side starts a library game (model.rs LaunchSpec); web games carry their URL.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Launch {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appid: Option<u64>,
}

/// The parts of a library game (lib/types.ts `Game`) joining needs. Parses the library's JSON.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LibraryGame {
    pub id: String,
    pub source: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch: Option<Launch>,
}

fn bare(id: &str) -> &str {
    id.strip_prefix("web:").unwrap_or(id)
}

fn launch_url(g: &LibraryGame) -> Option<&str> {
    g.launch.as_ref().and_then(|l| l.url.as_deref()).filter(|u| !u.is_empty())
}

fn launch_appid(g: &LibraryGame) -> Option<u64> {
    g.launch.as_ref().and_then(|l| l.appid).filter(|a| *a != 0)
}

/// The game ref for a library game, with `join` set when the game takes invites and a party is given.
pub fn game_ref(g: &LibraryGame, party_id: Option<&str>, inv: &Invites) -> GameRef {
    if g.source == "steam" {
        if let Some(appid) = launch_appid(g) {
            return GameRef::steam(&g.id, &g.title, appid);
        }
        if let Some(appid) = g.id.strip_prefix("steam:").unwrap_or(&g.id).trim().parse::<u64>().ok().filter(|a| *a > 0) {
            return GameRef::steam(&g.id, &g.title, appid);
        }
    }
    if g.source == "web" {
        if let Some(url) = launch_url(g) {
            let mut r = GameRef::web(&g.id, &g.title, url);
            if let Some(p) = party_id.filter(|p| !p.is_empty()) {
                if inv.param(&g.id).is_some() {
                    r.join = Some(p.to_string());
                }
            }
            return r;
        }
    }
    GameRef::other(&g.id, &g.title)
}

/// What the player is running now, for presence (the first running game).
pub fn running_ref(running: &[String], library: &[LibraryGame], party_id: Option<&str>, inv: &Invites) -> Option<GameRef> {
    let id = running.first()?;
    library.iter().find(|g| &g.id == id).map(|g| game_ref(g, party_id, inv))
}

/// The URL a web game opens with to join: the invite parameter added when the game has one.
pub fn join_url(r: &GameRef, inv: &Invites) -> Option<String> {
    let url = r.url.as_deref().filter(|u| !u.is_empty());
    if r.kind != GameKind::Web {
        return None;
    }
    let url = url?;
    let (Some(param), Some(join)) = (inv.param(&r.id), r.join.as_deref().filter(|j| !j.is_empty())) else {
        return Some(url.to_string());
    };
    let Ok(mut u) = url::Url::parse(url) else { return Some(url.to_string()) };
    // URLSearchParams.set: the first one takes the value, later ones go, else it is added.
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut set = false;
    for (k, v) in u.query_pairs() {
        if k == param {
            if !set {
                pairs.push((k.into_owned(), join.to_string()));
                set = true;
            }
        } else {
            pairs.push((k.into_owned(), v.into_owned()));
        }
    }
    if !set {
        pairs.push((param.to_string(), join.to_string()));
    }
    u.query_pairs_mut().clear().extend_pairs(pairs);
    Some(u.to_string())
}

/// What Join does for a game ref on this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinPlan {
    /// Start the library game; for a web game, at `url` when the invite changes it.
    Launch { game: LibraryGame, url: Option<String> },
    /// Open a web game this device does not have, at the friend's URL.
    Web { url: String },
    /// Open Steam's page for a game this device does not have.
    SteamStore { appid: u64 },
    Missing,
}

/// What Join does for a game ref on this device (pure).
pub fn join_plan(r: &GameRef, library: &[LibraryGame], inv: &Invites) -> JoinPlan {
    let appid = r.appid.filter(|a| *a != 0);
    let own = library.iter().find(|g| g.id == r.id).or_else(|| {
        let a = appid?;
        library.iter().find(|g| g.source == "steam" && (launch_appid(g) == Some(a) || g.id == format!("steam:{a}")))
    });
    let url = join_url(r, inv);
    if let Some(own) = own {
        let url = match (&url, launch_url(own)) {
            (Some(u), l) if own.source == "web" && Some(u.as_str()) != l => Some(u.clone()),
            _ => None,
        };
        return JoinPlan::Launch { game: own.clone(), url };
    }
    if r.kind == GameKind::Web {
        if let Some(url) = url {
            return JoinPlan::Web { url };
        }
    }
    if r.kind == GameKind::Steam {
        if let Some(appid) = appid {
            return JoinPlan::SteamStore { appid };
        }
    }
    JoinPlan::Missing
}

/// A toast for the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub text: String,
    pub error: bool,
}

impl JoinPlan {
    /// What the launcher says when Join runs this plan for `r` (a plain launch says nothing: the
    /// launch shows itself).
    pub fn toast(&self, r: &GameRef) -> Option<Toast> {
        let t = &r.title;
        match self {
            JoinPlan::Launch { url: Some(_), .. } | JoinPlan::Web { .. } => Some(Toast { text: format!("Joining {t}"), error: false }),
            JoinPlan::Launch { url: None, .. } => None,
            JoinPlan::SteamStore { .. } => Some(Toast { text: format!("{t} is not on this device. Opening it in Steam"), error: false }),
            JoinPlan::Missing => Some(Toast { text: format!("{t} is not on this device"), error: true }),
        }
    }

    /// The web game window's id for this plan (the library id without `web:`).
    pub fn web_id<'a>(&'a self, r: &'a GameRef) -> &'a str {
        match self {
            JoinPlan::Launch { game, .. } => bare(&game.id),
            _ => bare(&r.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, source: &str, title: &str, launch: Launch) -> LibraryGame {
        LibraryGame { id: id.into(), source: source.into(), title: title.into(), launch: Some(launch) }
    }
    fn web(url: &str) -> Launch {
        Launch { kind: "web".into(), url: Some(url.into()), appid: None }
    }
    fn steam(appid: u64) -> Launch {
        Launch { kind: "steam".into(), url: None, appid: Some(appid) }
    }
    fn act() -> LibraryGame {
        game("web:act", "web", "ACT", web("https://play.actuniverse.com/"))
    }
    fn balatro() -> LibraryGame {
        game("steam:2379780", "steam", "Balatro", steam(2379780))
    }

    #[test]
    fn starts_the_same_game_when_this_device_has_it() {
        let inv = Invites::default();
        let r = GameRef::web("web:act", "ACT", "https://play.actuniverse.com/");
        assert_eq!(join_plan(&r, &[act(), balatro()], &inv), JoinPlan::Launch { game: act(), url: None });
        assert_eq!(join_plan(&GameRef::steam("steam:2379780", "Balatro", 2379780), &[act(), balatro()], &inv), JoinPlan::Launch { game: balatro(), url: None });
    }

    #[test]
    fn finds_a_steam_game_by_app_id_even_under_another_id() {
        let other = game("steam:x", "steam", "Balatro", steam(2379780));
        assert!(matches!(join_plan(&GameRef::steam("steam:2379780", "Balatro", 2379780), std::slice::from_ref(&other), &Invites::default()), JoinPlan::Launch { game, .. } if game == other));
    }

    #[test]
    fn opens_steams_page_for_a_steam_game_this_device_does_not_have() {
        assert_eq!(join_plan(&GameRef::steam("steam:1145360", "Hades II", 1145360), &[act()], &Invites::default()), JoinPlan::SteamStore { appid: 1145360 });
    }

    #[test]
    fn opens_a_web_game_it_does_not_have_at_the_friends_url() {
        assert_eq!(join_plan(&GameRef::web("web:new", "New", "https://new.game/"), &[], &Invites::default()), JoinPlan::Web { url: "https://new.game/".into() });
    }

    #[test]
    fn adds_the_party_invite_for_web_games_that_take_one_and_only_those() {
        let mut r = GameRef::web("web:squads", "Squads", "https://squads.game/play?x=1");
        r.join = Some("p_abc".into());
        let mut inv = Invites::default();
        assert_eq!(join_url(&r, &inv).as_deref(), Some("https://squads.game/play?x=1"));
        inv.0.insert("squads".into(), "party".into());
        assert_eq!(join_url(&r, &inv).as_deref(), Some("https://squads.game/play?x=1&party=p_abc"));
        let own = game("web:squads", "web", "Squads", web("https://squads.game/play?x=1"));
        assert_eq!(join_plan(&r, std::slice::from_ref(&own), &inv), JoinPlan::Launch { game: own, url: Some("https://squads.game/play?x=1&party=p_abc".into()) });
        inv.0.remove("squads");
        // ACT keeps its parties in its own accounts: no invite parameter.
        assert_eq!(Invites::default().param("web:act"), None);
    }

    #[test]
    fn says_when_a_game_cannot_be_joined_from_here() {
        assert_eq!(join_plan(&GameRef::other("epic:fortnite", "Fortnite"), &[], &Invites::default()), JoinPlan::Missing);
    }

    #[test]
    fn describes_library_games_for_presence() {
        let mut inv = Invites::default();
        inv.0.insert("squads".into(), "party".into());
        let squads = game("web:squads", "web", "Squads", web("https://squads.game/"));
        assert_eq!(game_ref(&squads, Some("p1"), &inv).join.as_deref(), Some("p1"));
        assert_eq!(game_ref(&act(), Some("p1"), &inv).join, None);
        let bare_steam = LibraryGame { id: "steam:620".into(), source: "steam".into(), title: "Portal 2".into(), launch: None };
        assert_eq!(game_ref(&bare_steam, None, &inv), GameRef::steam("steam:620", "Portal 2", 620));
        assert_eq!(running_ref(&["steam:2379780".into()], &[act(), balatro()], None, &inv).map(|g| g.kind), Some(GameKind::Steam));
        assert_eq!(running_ref(&[], &[act()], None, &inv), None);
        let r = GameRef::steam("steam:1145360", "Hades II", 1145360);
        assert_eq!(join_plan(&r, &[], &inv).toast(&r).unwrap().text, "Hades II is not on this device. Opening it in Steam");
    }
}
