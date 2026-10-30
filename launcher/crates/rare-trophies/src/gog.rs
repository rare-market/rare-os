//! GOG achievements for the account gogdl is signed in to (Heroic's `gog_store/auth.json`), the
//! way GOG Galaxy reads them: the game's own client id and secret come from its build metadata on
//! GOG's content system, the Galaxy refresh token is exchanged for a token of that game, and
//! gameplay.gog.com answers the list with the player's unlocks and global rarity.

use crate::steam::Achievement;
use crate::ExtGame;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

/// GOG Galaxy's own client id, the key of gogdl's token.
const GALAXY: &str = "46899977096215655";

fn auth_path() -> std::path::PathBuf {
    dirs::config_dir().unwrap_or_default().join("heroic/gog_store/auth.json")
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("RareLauncher/0.1").build()
}

fn get(url: &str, token: Option<&str>) -> Option<Value> {
    let mut r = agent().get(url);
    if let Some(t) = token {
        r = r.set("Authorization", &format!("Bearer {t}"));
    }
    r.call().ok()?.into_json().ok()
}

/// Galaxy's refresh token from gogdl's auth file.
pub fn refresh_token() -> Option<String> {
    let v: Value = serde_json::from_slice(&std::fs::read(auth_path()).ok()?).ok()?;
    let c = v.get(GALAXY).or_else(|| v.as_object()?.values().next())?;
    c.get("refresh_token")?.as_str().map(String::from)
}

/// The game's (client id, client secret) from its newest build's metadata (zlib JSON).
pub fn client(product: &str) -> Option<(String, String)> {
    for os in ["windows", "osx", "linux"] {
        let Some(b) = get(&format!("https://content-system.gog.com/products/{product}/os/{os}/builds?generation=2"), None) else { continue };
        let Some(link) = b.pointer("/items/0/link").and_then(Value::as_str) else { continue };
        let Ok(r) = agent().get(link).call() else { continue };
        let mut raw = Vec::new();
        if r.into_reader().take(8 << 20).read_to_end(&mut raw).is_err() {
            continue;
        }
        let mut txt = Vec::new();
        let meta: Value = if flate2::read::ZlibDecoder::new(&raw[..]).read_to_end(&mut txt).is_ok() { serde_json::from_slice(&txt).ok()? } else { serde_json::from_slice(&raw).ok()? };
        let id = meta.get("clientId").and_then(Value::as_str);
        let secret = meta.get("clientSecret").and_then(Value::as_str);
        if let (Some(i), Some(s)) = (id, secret) {
            return Some((i.to_string(), s.to_string()));
        }
    }
    None
}

/// "2023-04-01T12:00:00+0000" to unix seconds.
fn date(s: &str) -> Option<u64> {
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0, 4)?, n(5, 7)?, n(8, 10)?);
    let (h, mi, se) = (n(11, 13).unwrap_or(0), n(14, 16).unwrap_or(0), n(17, 19).unwrap_or(0));
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    u64::try_from((era * 146097 + doe - 719468) * 86400 + h * 3600 + mi * 60 + se).ok()
}

/// gameplay.gog.com's `items` to achievements.
pub fn parse(v: &Value) -> Vec<Achievement> {
    v.get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let key = Some(s("achievement_key")).filter(|k| !k.is_empty()).or_else(|| a.get("achievement_id").map(|v| v.to_string().trim_matches('"').to_string()))?;
            let at = a.get("date_unlocked").and_then(Value::as_str).and_then(date);
            Some(Achievement {
                api_name: key,
                name: s("name"),
                description: s("description"),
                icon_url: Some(s("image_url_unlocked")).filter(|u| !u.is_empty()),
                icon_gray_url: Some(s("image_url_locked")).filter(|u| !u.is_empty()),
                hidden: a.get("visible").and_then(Value::as_bool).map(|v| !v).unwrap_or(false),
                unlocked: at.is_some(),
                unlock_time: at,
                global_percent: a.get("rarity").and_then(Value::as_f64).map(|p| p as f32),
            })
        })
        .collect()
}

/// One GOG game's achievements (blocking). `product` is GOG's product id, `title` the library's.
pub fn game(product: &str, title: &str) -> Option<ExtGame> {
    let refresh = refresh_token()?;
    let (id, secret) = client(product)?;
    let url = format!("https://auth.gog.com/token?client_id={id}&client_secret={secret}&grant_type=refresh_token&refresh_token={refresh}&without_new_session=1");
    let tok = get(&url, None)?;
    let access = tok.get("access_token")?.as_str()?.to_string();
    let user = tok.get("user_id").map(|u| u.as_str().map(String::from).unwrap_or_else(|| u.to_string()))?;
    let v = get(&format!("https://gameplay.gog.com/clients/{id}/users/{user}/achievements"), Some(&access))?;
    let list = parse(&v);
    (!list.is_empty()).then(|| ExtGame::new(format!("gog:{product}"), title.to_string(), list, "gog"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gameplay_items() {
        let v = serde_json::json!({ "total_count": 2, "items": [
            { "achievement_id": "1", "achievement_key": "KILL_10", "visible": true, "name": "Ten", "description": "Beat ten", "image_url_unlocked": "https://g/u.jpg", "image_url_locked": "https://g/l.jpg", "date_unlocked": "2023-11-14T22:13:20+0000", "rarity": 12.5 },
            { "achievement_id": 2, "achievement_key": "", "visible": false, "name": "Secret", "description": "", "date_unlocked": null, "rarity": 0.8 } ] });
        let l = parse(&v);
        assert_eq!(l.len(), 2);
        assert!(l[0].unlocked && l[0].unlock_time == Some(1_700_000_000) && l[0].global_percent == Some(12.5));
        assert_eq!(l[1].api_name, "2");
        assert!(l[1].hidden && !l[1].unlocked);
    }
}
