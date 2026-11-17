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

/// What legendary printed on the last sign-in (`~/.cache/rare/epic/sign-in.log`; never the code).
pub fn sign_in_log() -> PathBuf {
    rare_library::umu::log_path("epic:sign-in")
}

/// The sign-in error for the player: why, in words, and where legendary's output is.
pub fn sign_in_error(tool_error: &str) -> String {
    let e = tool_error.to_lowercase();
    let why = if e.contains("corrective_action") || e.contains("continuationurl") {
        "Epic wants something from you first (new terms or a security step). Sign in at epicgames.com in the Browser, do what it asks, then sign in here again.".to_string()
    } else if e.contains("authorization_code_not_found") || e.contains("expired") || e.contains("invalid_grant") {
        "The sign-in code expired before Rare could use it. Sign in again.".to_string()
    } else if e.contains("connection") || e.contains("resolve") || e.contains("timed out") || e.contains("network") || e.contains("took too long") {
        "No connection to Epic. Check Wi-Fi and try again.".to_string()
    } else if e.contains("missing") {
        tool_error.to_string()
    } else {
        let mut t: String = tool_error.chars().take(160).collect();
        if t.is_empty() {
            t = "legendary stopped with an error".into();
        }
        format!("Epic said: {}. Try again.", t.trim_end_matches('.'))
    };
    format!("Couldn't sign in to Epic Games. {why} Details: {}", rare_library::umu::show_path(&sign_in_log()))
}

/// Signs in with the code from the login page. Returns the account name.
pub fn sign_in(code: &str) -> Result<String, String> {
    if code.is_empty() {
        return Err("Epic didn't send a sign-in code. Sign in again, and finish on Epic's page.".into());
    }
    let r = tools::run_logged(LEGENDARY, &["auth", "--code", code], &[], NET, &sign_in_log());
    crate::accounts_changed();
    if let Some(name) = account().filter(|_| r.is_ok()) {
        return Ok(name);
    }
    // legendary 0.21.1 exits 0 when Epic refuses the code; its last line is only "Login attempt
    // failed, please see log for details", so the reason is read from the log.
    let log = std::fs::read_to_string(sign_in_log()).unwrap_or_default();
    let why = log_reason(&log).or_else(|| r.err()).unwrap_or_else(|| "legendary kept no account".into());
    log::warn!("epic sign-in: {why}");
    Err(sign_in_error(&why))
}

/// The most telling line of legendary's log: Epic's error code when it gave one, else the last error.
pub fn log_reason(log: &str) -> Option<String> {
    let line = log.lines().find(|l| l.contains("errorCode") || l.contains("open the following URL")).or_else(|| log.lines().rev().find(|l| l.contains("ERROR:") || l.contains("CRITICAL:")))?;
    Some(tools::clean(line))
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

/// legendary's answer when a game's download needs more shared memory than its 2 GiB default
/// ("Current shared memory cache is smaller than required: 2048.0 MiB < 3216.0 MiB"): the arguments
/// for one more run, `--enable-reordering` (which needs far less) and room for what it asked, at
/// most a quarter of the device's memory. None for any other error.
pub fn more_memory_args(error: &str, total_mib: u64) -> Option<Vec<String>> {
    let i = error.find("shared memory cache is smaller than required")?;
    let need = error[i..].split('<').nth(1)?.split_whitespace().next()?.parse::<f64>().ok()?;
    let cap = (total_mib / 4).max(2048);
    let size = ((need.ceil() as u64) + 64).min(cap);
    Some(vec!["--enable-reordering".into(), "--max-shared-memory".into(), size.to_string()])
}

#[cfg(test)]
mod tests {
    #[test]
    fn sign_in_errors_say_why() {
        // legendary 0.21.1 against Epic (2026-11-15) with a used or made-up code.
        let e = super::sign_in_error("Login to EGS API failed with errorCode: errors.com.epicgames.account.oauth.authorization_code_not_found");
        assert!(e.starts_with("Couldn't sign in to Epic Games. The sign-in code expired"), "{e}");
        assert!(e.ends_with("rare/epic/sign-in.log"), "{e}");
        let e = super::sign_in_error("Login to EGS API failed with errorCode: errors.com.epicgames.oauth.corrective_action_required");
        assert!(e.contains("Sign in at epicgames.com in the Browser"), "{e}");
        let log = "Rare: legendary auth --code <code>\n[cli] INFO: Testing existing login data if present...\n[EPCAPI] ERROR: Login to EGS API failed with errorCode: errors.com.epicgames.account.oauth.authorization_code_not_found\n[Core] ERROR: Logging in failed with InvalidCredentialsError('errors.com.epicgames.account.oauth.authorization_code_not_found'), please try again.\n[cli] ERROR: Login attempt failed, please see log for details.\n";
        assert_eq!(super::log_reason(log).as_deref(), Some("Login to EGS API failed with errorCode: errors.com.epicgames.account.oauth.authorization_code_not_found"));
        assert_eq!(super::log_reason("[cli] ERROR: Login attempt failed, please see log for details.\n").as_deref(), Some("Login attempt failed, please see log for details."));
        let e = super::sign_in_error("Something new.");
        assert!(e.contains("Epic said: Something new. Try again."), "{e}");
    }

    #[test]
    fn reads_the_owned_list() {
        let json = r#"[{"app_name":"Fortnite","app_title":"Fortnite","metadata":{"keyImages":[{"type":"DieselGameBoxTall","url":"https://e/t.jpg"},{"type":"DieselGameBox","url":"https://e/w.jpg"}],"customAttributes":{"CloudSaveFolder":{"type":"STRING","value":"{AppData}/x"}}}},
            {"app_name":"dlc","app_title":"A DLC","metadata":{"mainGameItem":{}}}]"#;
        let l = super::parse_list(json);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].art_square.as_str(), l[0].art_cover.as_str(), l[0].cloud), ("https://e/t.jpg", "https://e/w.jpg", true));
    }

    #[test]
    fn a_big_game_gets_more_shared_memory() {
        // legendary 0.21.1 on a big game (BioShock Infinite) with its default 2 GiB.
        let e = "MemoryError: Current shared memory cache is smaller than required: 2048.0 MiB < 3216.0 MiB. Try running legendary with \"--enable-reordering\" to reduce memory usage, or use \"--max-shared-memory 3248\" to increase the limit.";
        assert_eq!(super::more_memory_args(e, 15249).unwrap(), ["--enable-reordering", "--max-shared-memory", "3280"]);
        // Never more than a quarter of the memory; reordering does the rest.
        assert_eq!(super::more_memory_args(e, 8000).unwrap()[2], "2048");
        assert!(super::more_memory_args("Log in failed!", 15249).is_none());
    }
}
