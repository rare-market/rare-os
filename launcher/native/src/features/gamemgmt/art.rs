//! Art choices for a game, by its name: SteamGridDB when the player added a key in Settings >
//! Storage (it has no key-free API), else Steam's store (its search needs no key, and every app
//! has a cover, hero and logo on Steam's CDN).

use rare_library::custom::{ArtKind, Custom};
use serde_json::Value;
use std::time::Duration;

/// One picture to pick: what is stored, and a smaller one to show in the list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Choice {
    pub url: String,
    pub thumb: String,
    /// The game it is from (Steam store matches), for the tile's caption.
    pub from: String,
}

/// Where the choices came from, for the page's note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    SteamGridDb,
    Steam,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(12))).http_status_as_error(false).build().into()
}

fn get_json(url: &str, bearer: Option<&str>) -> Result<Value, String> {
    let mut req = agent().get(url).header("User-Agent", "RareLauncher/1.0");
    if let Some(k) = bearer {
        req = req.header("Authorization", &format!("Bearer {k}"));
    }
    let mut r = req.call().map_err(|e| e.to_string())?;
    if r.status() == 401 || r.status() == 403 {
        return Err("SteamGridDB didn't accept the key. Check it in Settings > Storage.".into());
    }
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    r.body_mut().read_json::<Value>().map_err(|e| e.to_string())
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

pub fn sgdb_key() -> Option<String> {
    crate::features::config::get_str("steamgriddbKey").filter(|k| !k.trim().is_empty())
}

fn cdn(appid: u64, kind: ArtKind) -> String {
    let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
    match kind {
        ArtKind::Cover => format!("{base}/library_600x900_2x.jpg"),
        ArtKind::Hero => format!("{base}/library_hero.jpg"),
        ArtKind::Logo => format!("{base}/logo.png"),
    }
}

/// Steam store apps matching a name: (appid, name), best first.
pub fn steam_search(term: &str) -> Result<Vec<(u64, String)>, String> {
    let v = get_json(&format!("https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US", enc(term)), None)?;
    Ok(v["items"].as_array().map(|a| a.iter().filter_map(|i| Some((i["id"].as_u64()?, i["name"].as_str()?.to_string()))).collect()).unwrap_or_default())
}

fn sgdb_game(key: &str, term: &str, steam_appid: Option<u32>) -> Result<Option<(u64, String)>, String> {
    if let Some(a) = steam_appid {
        let v = get_json(&format!("https://www.steamgriddb.com/api/v2/games/steam/{a}"), Some(key))?;
        if let Some(id) = v["data"]["id"].as_u64() {
            return Ok(Some((id, v["data"]["name"].as_str().unwrap_or(term).to_string())));
        }
    }
    let v = get_json(&format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", enc(term)), Some(key))?;
    Ok(v["data"].as_array().and_then(|a| a.first()).and_then(|g| Some((g["id"].as_u64()?, g["name"].as_str().unwrap_or(term).to_string()))))
}

fn sgdb_art(key: &str, game: u64, name: &str, kind: ArtKind) -> Result<Vec<Choice>, String> {
    let path = match kind {
        ArtKind::Cover => format!("grids/game/{game}?dimensions=600x900,342x482,660x930"),
        ArtKind::Hero => format!("heroes/game/{game}"),
        ArtKind::Logo => format!("logos/game/{game}"),
    };
    let v = get_json(&format!("https://www.steamgriddb.com/api/v2/{path}"), Some(key))?;
    Ok(v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    let url = i["url"].as_str()?.to_string();
                    let thumb = i["thumb"].as_str().map(String::from).unwrap_or_else(|| url.clone());
                    // Animated art can't be drawn; its still is the thumb.
                    (!url.ends_with(".webm")).then(|| Choice { url, thumb, from: name.to_string() })
                })
                .take(30)
                .collect()
        })
        .unwrap_or_default())
}

/// The choices for one kind of picture.
pub fn choices(title: &str, steam_appid: Option<u32>, kind: ArtKind) -> Result<(Origin, Vec<Choice>), String> {
    if let Some(key) = sgdb_key() {
        if let Some((game, name)) = sgdb_game(&key, title, steam_appid)? {
            return Ok((Origin::SteamGridDb, sgdb_art(&key, game, &name, kind)?));
        }
        return Ok((Origin::SteamGridDb, vec![]));
    }
    let mut apps: Vec<(u64, String)> = steam_appid.map(|a| (a as u64, title.to_string())).into_iter().collect();
    for a in steam_search(title)? {
        if !apps.iter().any(|x| x.0 == a.0) {
            apps.push(a);
        }
    }
    Ok((Origin::Steam, apps.into_iter().take(12).map(|(id, name)| Choice { url: cdn(id, kind), thumb: cdn(id, kind), from: name }).collect()))
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Art for a game the player just added: SteamGridDB's first match with a key, else the Steam
/// app whose name matches. Nothing when no name matches (a wrong cover is worse than none).
pub fn automatic(title: &str) -> Custom {
    let want = norm(title);
    if want.is_empty() {
        return Custom::default();
    }
    if let Some(key) = sgdb_key() {
        let Ok(Some((game, name))) = sgdb_game(&key, title, None) else { return Custom::default() };
        if !norm(&name).contains(&want) && !want.contains(&norm(&name)) {
            return Custom::default();
        }
        let first = |k| sgdb_art(&key, game, &name, k).ok().and_then(|v| v.into_iter().next()).map(|c| c.url);
        return Custom { title: None, cover: first(ArtKind::Cover), hero: first(ArtKind::Hero), logo: first(ArtKind::Logo) };
    }
    let Ok(found) = steam_search(title) else { return Custom::default() };
    let Some((appid, _)) = found.into_iter().find(|(_, n)| norm(n) == want) else { return Custom::default() };
    Custom { title: None, cover: Some(cdn(appid, ArtKind::Cover)), hero: Some(cdn(appid, ArtKind::Hero)), logo: Some(cdn(appid, ArtKind::Logo)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_cdn_names() {
        assert!(cdn(413150, ArtKind::Cover).ends_with("/413150/library_600x900_2x.jpg"));
        assert!(cdn(413150, ArtKind::Logo).ends_with("/logo.png"));
        assert_eq!(norm("Hollow Knight: Silksong"), "hollowknightsilksong");
        assert_eq!(enc("Hades II: x"), "Hades%20II%3A%20x");
    }
}
