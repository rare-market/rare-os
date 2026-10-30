//! Epic achievements for the account legendary is signed in to. Epic's launcher GraphQL answers
//! a game's achievement list (names, icons, global rarity) without a sign-in, and the player's
//! unlocks by Epic account id and the game's sandbox (legendary's metadata `namespace`).

use crate::steam::Achievement;
use crate::ExtGame;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

const GQL: &str = "https://launcher.store.epicgames.com/graphql";

fn config_dir() -> PathBuf {
    std::env::var_os("LEGENDARY_CONFIG_PATH").map(PathBuf::from).unwrap_or_else(|| dirs::config_dir().unwrap_or_default().join("legendary"))
}

fn read_json(p: &std::path::Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// (Epic account id, access token) from legendary's user.json.
pub fn account() -> Option<(String, Option<String>)> {
    let u = read_json(&config_dir().join("user.json"))?;
    let id = u.get("account_id")?.as_str()?.to_string();
    Some((id, u.get("access_token").and_then(Value::as_str).map(String::from)))
}

/// The sandbox (namespace) and title of an installed or known Epic app.
pub fn sandbox(app: &str) -> Option<(String, String)> {
    let m = read_json(&config_dir().join("metadata").join(format!("{app}.json")))?;
    let ns = m.pointer("/metadata/namespace").or_else(|| m.get("namespace"))?.as_str()?.to_string();
    let title = m.pointer("/metadata/title").or_else(|| m.get("app_title")).and_then(Value::as_str).unwrap_or(app).to_string();
    Some((ns, title))
}

fn gql(query: &str, vars: Value, token: Option<&str>) -> Option<Value> {
    let mut r = ureq::post(GQL).set("User-Agent", "EpicGamesLauncher/16.0.0").timeout(Duration::from_secs(12));
    if let Some(t) = token {
        r = r.set("Authorization", &format!("bearer {t}"));
    }
    let v: Value = r.send_json(json!({ "query": query, "variables": vars })).ok()?.into_json().ok()?;
    v.get("data").cloned()
}

const SCHEMA: &str = "query($s:String!){Achievement{productAchievementsRecordBySandbox(sandboxId:$s,locale:\"en-US\"){totalAchievements achievements{achievement{name hidden unlockedDisplayName lockedDisplayName unlockedDescription lockedDescription unlockedIconLink lockedIconLink rarity{percent}}}}}}";
const PLAYER: &str = "query($a:String!,$s:String!){PlayerAchievement{playerAchievementGameRecordsBySandbox(epicAccountId:$a,sandboxId:$s){records{playerAchievements{playerAchievement{achievementName unlocked unlockDate}}}}}}";

/// "2023-04-01T12:00:00.000Z" to unix seconds.
fn iso(s: &str) -> Option<u64> {
    let d = s.get(..19)?.replace('T', " ");
    rare_emu_date(&d)
}

fn rare_emu_date(s: &str) -> Option<u64> {
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

/// The list and the player's unlocks joined.
pub fn parse(schema: &Value, player: Option<&Value>) -> Vec<Achievement> {
    let mut got: std::collections::HashMap<String, Option<u64>> = Default::default();
    for rec in player.and_then(|p| p.pointer("/PlayerAchievement/playerAchievementGameRecordsBySandbox/records")).and_then(Value::as_array).into_iter().flatten() {
        for pa in rec.get("playerAchievements").and_then(Value::as_array).into_iter().flatten() {
            let a = pa.get("playerAchievement").unwrap_or(pa);
            if a.get("unlocked").and_then(Value::as_bool) == Some(true) {
                if let Some(n) = a.get("achievementName").and_then(Value::as_str) {
                    got.insert(n.to_string(), a.get("unlockDate").and_then(Value::as_str).and_then(iso));
                }
            }
        }
    }
    schema
        .pointer("/Achievement/productAchievementsRecordBySandbox/achievements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let a = x.get("achievement")?;
            let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let name = s("name");
            let unlocked = got.contains_key(&name);
            Some(Achievement {
                api_name: name.clone(),
                name: if s("unlockedDisplayName").is_empty() { s("lockedDisplayName") } else { s("unlockedDisplayName") },
                description: if unlocked || s("lockedDescription").is_empty() { s("unlockedDescription") } else { s("lockedDescription") },
                icon_url: Some(s("unlockedIconLink")).filter(|u| !u.is_empty()),
                icon_gray_url: Some(s("lockedIconLink")).filter(|u| !u.is_empty()),
                hidden: a.get("hidden").and_then(Value::as_bool).unwrap_or(false),
                unlocked,
                unlock_time: got.get(&name).copied().flatten(),
                global_percent: a.pointer("/rarity/percent").and_then(Value::as_f64).map(|p| p as f32),
            })
        })
        .collect()
}

/// One Epic game's achievements (blocking). None when legendary isn't signed in, the app isn't
/// known, or Epic has no achievements for it.
pub fn game(app: &str) -> Option<ExtGame> {
    let (ns, title) = sandbox(app)?;
    let schema = gql(SCHEMA, json!({ "s": ns }), None)?;
    let (account, token) = account()?;
    let vars = json!({ "a": account, "s": ns });
    let player = gql(PLAYER, vars.clone(), token.as_deref()).or_else(|| gql(PLAYER, vars, None));
    let list = parse(&schema, player.as_ref());
    (!list.is_empty()).then(|| ExtGame::new(format!("epic:{app}"), title, list, "epic"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_schema_and_unlocks() {
        let schema = json!({ "Achievement": { "productAchievementsRecordBySandbox": { "totalAchievements": 2, "achievements": [
            { "achievement": { "name": "A", "hidden": false, "unlockedDisplayName": "Back to Work", "unlockedDescription": "Get out", "unlockedIconLink": "https://e/a.png", "lockedIconLink": "https://e/a-l.png", "rarity": { "percent": 38 } } },
            { "achievement": { "name": "B", "hidden": true, "unlockedDisplayName": "Secret", "lockedDescription": "???", "unlockedDescription": "Found it", "rarity": { "percent": 1.5 } } } ] } } });
        let player = json!({ "PlayerAchievement": { "playerAchievementGameRecordsBySandbox": { "records": [ { "playerAchievements": [ { "playerAchievement": { "achievementName": "A", "unlocked": true, "unlockDate": "2023-11-14T22:13:20.000Z" } } ] } ] } } });
        let l = parse(&schema, Some(&player));
        assert_eq!(l.len(), 2);
        assert!(l[0].unlocked && l[0].unlock_time == Some(1_700_000_000));
        assert_eq!(l[0].global_percent, Some(38.0));
        assert!(!l[1].unlocked && l[1].hidden && l[1].description == "???");
        assert!(parse(&schema, None).iter().all(|a| !a.unlocked));
    }
}
