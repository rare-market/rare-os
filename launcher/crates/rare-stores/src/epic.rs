//! Epic Games through legendary: sign-in with the code Epic's own login page hands out, the owned
//! games (`legendary list --json`), installs, updates and repairs (`legendary install`), and cloud
//! saves (`legendary sync-saves`). Games start through `legendary launch --no-wine --wrapper
//! umu-run` (rare-launch), in Rare's prefix for the game.

use crate::tools::{self, LEGENDARY};
use crate::{read_json, Owned, Store};
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

/// Epic's login, ending on a page that shows the authorization code as JSON (legendary's own
/// client id). The login window's script turns that page into a navigation to [`DONE`].
pub const LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

/// Where the login window's script sends the code (never loads: the launcher takes it).
pub const DONE: &str = "https://rare.invalid/store-login";

/// Runs in every page of the login window: on Epic's redirect page, reads the code from the JSON
/// it shows and navigates to `DONE?store=epic&code=...`.
pub const LOGIN_SCRIPT: &str = r#"(function(){
  if (!/(^|\.)epicgames\.com$/.test(location.hostname) || location.pathname.indexOf('/id/api/redirect') !== 0) return;
  var tries = 0;
  var go = function(){
    try {
      var j = JSON.parse(document.body.innerText);
      var c = j.authorizationCode || '';
      if (!c && j.redirectUrl) { var m = /[?&]code=([^&]+)/.exec(j.redirectUrl); if (m) c = decodeURIComponent(m[1]); }
      location.href = 'https://rare.invalid/store-login?store=epic&code=' + encodeURIComponent(c);
    } catch (e) { if (++tries < 50) setTimeout(go, 200); }
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', go); else go();
})();"#;

/// legendary's folder: `$LEGENDARY_CONFIG_PATH` or `~/.config/legendary`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("LEGENDARY_CONFIG_PATH").map(PathBuf::from).unwrap_or_else(|| dirs::config_dir().unwrap_or_default().join("legendary"))
}

/// The signed-in account's display name.
pub fn account() -> Option<String> {
    let u = read_json(&config_dir().join("user.json"))?;
    u.get("displayName").and_then(Value::as_str).map(String::from).or_else(|| u.get("account_id").map(|_| "Signed in".to_string()))
}

const NET: Duration = Duration::from_secs(90);

/// Signs in with the code from the login page. Returns the account name.
pub fn sign_in(code: &str) -> Result<String, String> {
    if code.is_empty() {
        return Err("Epic didn't send a sign-in code. Try again.".into());
    }
    tools::run(LEGENDARY, &["auth", "--code", code], &[], NET).map_err(|e| {
        log::warn!("epic sign-in: {e}");
        "Couldn't sign in to Epic Games. Try again.".to_string()
    })?;
    crate::accounts_changed();
    account().ok_or_else(|| "Couldn't sign in to Epic Games. Try again.".into())
}

pub fn sign_out() -> Result<(), String> {
    let r = tools::run(LEGENDARY, &["auth", "--delete"], &[], Duration::from_secs(30)).map(|_| ());
    let _ = std::fs::remove_file(config_dir().join("user.json"));
    let _ = std::fs::remove_file(crate::owned_path(Store::Epic));
    crate::accounts_changed();
    r
}

fn image(meta: &Value, kind: &str) -> String {
    meta.pointer("/keyImages")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|i| i.get("type").and_then(Value::as_str) == Some(kind)))
        .and_then(|i| i.get("url").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

/// The owned games in `legendary list --json`'s output (DLC and Unreal content are left out).
pub fn parse_list(json: &str) -> Vec<Owned> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(json.trim()) else { return vec![] };
    list.iter()
        .filter_map(|g| {
            let app = g.get("app_name")?.as_str()?.to_string();
            let meta = g.get("metadata").cloned().unwrap_or(Value::Null);
            if meta.get("mainGameItem").is_some() {
                return None;
            }
            let title = g.get("app_title").and_then(Value::as_str).unwrap_or(&app).to_string();
            let cloud = meta.pointer("/customAttributes/CloudSaveFolder/value").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
            let tall = image(&meta, "DieselGameBoxTall");
            Some(Owned {
                app_name: app,
                title,
                art_square: if tall.is_empty() { image(&meta, "Thumbnail") } else { tall },
                art_cover: image(&meta, "DieselGameBox"),
                art_logo: image(&meta, "DieselGameBoxLogo"),
                platform: "windows".into(),
                cloud,
            })
        })
        .collect()
}

/// Fetches the owned games and keeps them for the library. Returns how many.
pub fn refresh_owned() -> Result<usize, String> {
    let out = tools::run(LEGENDARY, &["list", "--json"], &[], Duration::from_secs(240))?;
    let list = parse_list(&out);
    let n = list.len();
    if n > 0 || out.trim() == "[]" {
        crate::save_owned(Store::Epic, list);
        crate::changed();
    }
    Ok(n)
}

/// legendary's installed.json: app name to (install path, version).
pub fn installed() -> Vec<(String, String, String)> {
    let Some(Value::Object(map)) = read_json(&config_dir().join("installed.json")) else { return vec![] };
    map.into_iter()
        .filter(|(_, g)| !g.get("is_dlc").and_then(Value::as_bool).unwrap_or(false))
        .map(|(app, g)| {
            let path = g.get("install_path").and_then(Value::as_str).unwrap_or("").to_string();
            let ver = g.get("version").and_then(Value::as_str).unwrap_or("").to_string();
            (app, path, ver)
        })
        .collect()
}

pub fn install_dir(app: &str) -> Option<String> {
    installed().into_iter().find(|(a, _, _)| a == app).map(|(_, p, _)| p).filter(|p| !p.is_empty())
}

/// Installed games with a newer build (asks Epic first; `legendary list-installed --check-updates`).
pub fn updates() -> Result<Vec<String>, String> {
    if account().is_none() {
        return Ok(vec![]);
    }
    let have = installed();
    if have.is_empty() {
        return Ok(vec![]);
    }
    tools::run(LEGENDARY, &["list-installed", "--check-updates", "--json"], &[], Duration::from_secs(120))?;
    let assets = read_json(&config_dir().join("assets.json")).unwrap_or(Value::Null);
    let latest = |app: &str| -> Option<String> {
        let list = assets.get("Windows").and_then(Value::as_array).or_else(|| assets.as_array())?;
        list.iter().find(|a| a.get("app_name").and_then(Value::as_str) == Some(app)).and_then(|a| a.get("build_version").and_then(Value::as_str)).map(String::from)
    };
    Ok(have.into_iter().filter(|(app, _, ver)| latest(app).is_some_and(|l| !ver.is_empty() && &l != ver)).map(|(a, _, _)| a).collect())
}

/// Download and install size (bytes) before installing (`legendary info --json`).
pub fn size(app: &str) -> Option<(u64, u64)> {
    let out = tools::run(LEGENDARY, &["info", app, "--json"], &[], Duration::from_secs(60)).ok()?;
    let v: Value = serde_json::from_str(out.trim()).ok()?;
    let m = v.get("manifest")?;
    Some((m.get("download_size")?.as_u64()?, m.get("disk_size")?.as_u64()?))
}

/// True when the game keeps saves in Epic's cloud (its metadata's CloudSaveFolder).
pub fn has_cloud_saves(app: &str) -> bool {
    if let Some(m) = read_json(&config_dir().join("metadata").join(format!("{app}.json"))) {
        return m.pointer("/metadata/customAttributes/CloudSaveFolder/value").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    }
    crate::load_owned(Store::Epic).iter().any(|o| o.app_name == app && o.cloud)
}

/// The arguments for an install, update or repair job.
pub fn job_args(kind: &crate::jobs::Kind, app: &str) -> Vec<String> {
    use crate::jobs::Kind;
    let mut a: Vec<String> = vec!["-y".into()];
    match kind {
        Kind::Install { base } => {
            a.extend(["install".into(), app.into(), "--base-path".into(), base.to_string_lossy().into_owned()]);
        }
        Kind::Update => a.extend(["install".into(), app.into(), "--update-only".into()]),
        Kind::Repair => a.extend(["repair".into(), app.into(), "--repair-and-update".into()]),
        Kind::Saves | Kind::Setup => {}
    }
    a.extend(["--skip-sdl".into(), "--skip-dlcs".into()]);
    a
}

/// Syncs one game's cloud saves in its Rare prefix (`down`: only fetch, `up`: only send).
pub fn sync_saves(app: &str, down: bool, up: bool) -> Result<(), String> {
    let pfx = crate::prefix(&format!("epic:{app}"));
    if !pfx.join("drive_c").is_dir() {
        // Never played here yet: legendary can't place the saves before the game makes its prefix.
        return Ok(());
    }
    let mut args = vec!["-y", "sync-saves", app, "--accept-path"];
    if !up {
        args.push("--skip-upload");
    }
    if !down {
        args.push("--skip-download");
    }
    let env = [("WINEPREFIX".to_string(), pfx.to_string_lossy().into_owned())];
    tools::run(LEGENDARY, &args, &env, Duration::from_secs(120)).map(|_| ())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_owned_list() {
        let json = r#"[{"app_name":"Fortnite","app_title":"Fortnite","metadata":{"keyImages":[{"type":"DieselGameBoxTall","url":"https://e/t.jpg"},{"type":"DieselGameBox","url":"https://e/w.jpg"}],"customAttributes":{"CloudSaveFolder":{"type":"STRING","value":"{AppData}/x"}}}},
            {"app_name":"dlc","app_title":"A DLC","metadata":{"mainGameItem":{}}}]"#;
        let l = super::parse_list(json);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].art_square.as_str(), l[0].art_cover.as_str(), l[0].cloud), ("https://e/t.jpg", "https://e/w.jpg", true));
    }
}
