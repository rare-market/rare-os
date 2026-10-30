//! Steam's global unlock percentages for every achievement of a game (the library cache only has
//! a handful). Steam's public `GetGlobalAchievementPercentagesForApp` needs no key and no
//! account; rare.market keeps a day-old copy for every player (`/v1/trophies/rarity/steam/<appid>`,
//! the same JSON), Steam itself is the fallback. Answers are kept a week in
//! `~/.cache/rare/trophies/rarity/<appid>.json`.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

const WEEK: Duration = Duration::from_secs(7 * 86_400);

/// api name -> percent.
pub type Rarity = HashMap<String, f32>;

pub fn parse(v: &serde_json::Value) -> Rarity {
    v.pointer("/achievementpercentages/achievements")
        .and_then(|a| a.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let name = a.get("name")?.as_str()?.to_string();
                    let p = match a.get("percent")? {
                        serde_json::Value::String(s) => s.trim().parse::<f32>().ok()?,
                        serde_json::Value::Number(n) => n.as_f64()? as f32,
                        _ => return None,
                    };
                    p.is_finite().then_some((name, p.clamp(0.0, 100.0)))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cache_path(appid: u32) -> std::path::PathBuf {
    rare_core::paths::cache_dir().join(format!("trophies/rarity/{appid}.json"))
}

/// What the cache has, however old (no network).
pub fn cached(appid: u32) -> Option<Rarity> {
    let b = std::fs::read(cache_path(appid)).ok()?;
    serde_json::from_slice::<serde_json::Value>(&b).ok().map(|v| parse(&v))
}

fn fresh(appid: u32) -> bool {
    std::fs::metadata(cache_path(appid)).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age < WEEK)
}

/// The game's percentages: the week-old cache, else Steam (blocking, a few seconds at most).
/// None when Steam has none for it and nothing is cached.
pub fn steam(appid: u32) -> Option<Rarity> {
    if fresh(appid) {
        return cached(appid);
    }
    let fetch = |url: &str| ureq::get(url).set("User-Agent", "RareLauncher/0.1").timeout(Duration::from_secs(8)).call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok()).filter(|v| !parse(v).is_empty());
    let got = fetch(&format!("https://rare.market/v1/trophies/rarity/steam/{appid}")).or_else(|| fetch(&format!("https://api.steampowered.com/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v0002/?gameid={appid}&format=json")));
    match got {
        Some(v) if !parse(&v).is_empty() => {
            let p = cache_path(appid);
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&p, serde_json::to_vec(&v).unwrap_or_default());
            Some(parse(&v))
        }
        _ => cached(appid),
    }
}

/// Fills in the percentages the files lacked.
pub fn apply(game: &mut crate::steam::GameAchievements, r: &Rarity) {
    for a in game.achievements.iter_mut() {
        if let Some(p) = r.get(&a.api_name) {
            a.global_percent = Some(*p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_shapes() {
        let v = serde_json::json!({ "achievementpercentages": { "achievements": [ { "name": "A", "percent": "71.7" }, { "name": "B", "percent": 0.4 }, { "name": "C" } ] } });
        let r = parse(&v);
        assert_eq!(r.len(), 2);
        assert_eq!(r["A"], 71.7);
        assert_eq!(r["B"], 0.4);
        assert!(parse(&serde_json::json!({})).is_empty());
    }
}
