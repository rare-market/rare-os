//! GOG through gogdl (Heroic's GOG backend): sign-in with the code GOG's login page puts in its
//! last address, the owned games (GOG's account API, art from GOG's games database), installs,
//! updates and repairs (`gogdl download|update|repair`) and cloud saves (`gogdl save-sync`, in
//! saves.rs). Sign-in and installs are kept where Heroic keeps them, in Heroic's format.

use crate::tools::{self, GOGDL};
use crate::{read_json, write_json, Owned, Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const LOGIN_URL: &str = "https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&response_type=code&layout=galaxy";

/// The page GOG's login ends on, with `code=` in its address.
pub const DONE_PREFIX: &str = "https://embed.gog.com/on_login_success";

/// The sign-in code in the address GOG's login ends on.
pub fn code_in(url: &str) -> Option<String> {
    if !url.starts_with(DONE_PREFIX) {
        return None;
    }
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| kv.strip_prefix("code=")).map(|c| c.split('#').next().unwrap_or(c).to_string()).filter(|c| !c.is_empty())
}

/// `~/.config/heroic/gog_store` (Heroic's place, Heroic's files).
pub fn store_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("heroic/gog_store")
}

pub fn auth_path() -> PathBuf {
    store_dir().join("auth.json")
}

fn installed_path() -> PathBuf {
    store_dir().join("installed.json")
}

fn user_path() -> PathBuf {
    crate::rare_dir().join("gog-user.json")
}

fn base_args() -> Vec<String> {
    vec!["--auth-config-path".into(), auth_path().to_string_lossy().into_owned()]
}

fn gogdl(args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut a = base_args();
    a.extend(args.iter().map(|s| s.to_string()));
    let refs: Vec<&str> = a.iter().map(String::as_str).collect();
    tools::run(GOGDL, &refs, &[], timeout)
}

/// The signed-in account's user name ("Signed in" before it is known).
pub fn account() -> Option<String> {
    let auth = read_json(&auth_path())?;
    if !auth.as_object().is_some_and(|o| !o.is_empty()) {
        return None;
    }
    Some(read_json(&user_path()).and_then(|u| u.get("username").and_then(Value::as_str).map(String::from)).unwrap_or_else(|| "Signed in".into()))
}

const NET: Duration = Duration::from_secs(60);

fn last_json(out: &str) -> Option<Value> {
    out.lines().rev().find_map(|l| serde_json::from_str::<Value>(l.trim()).ok().filter(|v| v.is_object()))
}

/// A fresh access token (gogdl refreshes it when it has run out).
fn token() -> Result<String, String> {
    let out = gogdl(&["auth"], NET)?;
    let v = last_json(&out).ok_or("GOG didn't answer. Try again.")?;
    if v.get("error").and_then(Value::as_bool).unwrap_or(false) {
        return Err("Sign in to GOG again in Settings > Accounts.".into());
    }
    v.get("access_token").and_then(Value::as_str).map(String::from).ok_or_else(|| "Sign in to GOG again in Settings > Accounts.".into())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build()
}

fn get(url: &str, token: Option<&str>) -> Result<Value, String> {
    let mut r = agent().get(url);
    if let Some(t) = token {
        r = r.set("Authorization", &format!("Bearer {t}"));
    }
    r.call().map_err(|e| e.to_string())?.into_json::<Value>().map_err(|e| e.to_string())
}

/// Signs in with the code from the login page. Returns the user name.
pub fn sign_in(code: &str) -> Result<String, String> {
    let _ = std::fs::create_dir_all(store_dir());
    let out = gogdl(&["auth", "--code", code], NET).map_err(|e| {
        log::warn!("gog sign-in: {e}");
        "Couldn't sign in to GOG. Try again.".to_string()
    })?;
    let v = last_json(&out).unwrap_or(Value::Null);
    if v.get("error").and_then(Value::as_bool).unwrap_or(false) || v.get("access_token").is_none() {
        return Err("Couldn't sign in to GOG. Try again.".into());
    }
    let tok = v.get("access_token").and_then(Value::as_str).unwrap_or("");
    if let Ok(u) = get("https://embed.gog.com/userData.json", Some(tok)) {
        if let Some(name) = u.get("username").and_then(Value::as_str) {
            write_json(&user_path(), &json!({ "username": name, "userId": u.get("userId") }));
        }
    }
    crate::accounts_changed();
    Ok(account().unwrap_or_else(|| "Signed in".into()))
}

pub fn sign_out() -> Result<(), String> {
    let _ = std::fs::remove_file(auth_path());
    let _ = std::fs::remove_file(user_path());
    let _ = std::fs::remove_file(crate::owned_path(Store::Gog));
    crate::accounts_changed();
    Ok(())
}

/// One page of getFilteredProducts as owned games (without the database art).
pub fn parse_products(v: &Value) -> Vec<Owned> {
    v.get("products")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|p| p.get("isGame").and_then(Value::as_bool).unwrap_or(true))
                .filter_map(|p| {
                    let id = p.get("id").and_then(|i| i.as_u64().map(|n| n.to_string()).or_else(|| i.as_str().map(String::from)))?;
                    let works = |os: &str| p.pointer(&format!("/worksOn/{os}")).and_then(Value::as_bool).unwrap_or(false);
                    let platform = if works("Windows") {
                        "windows"
                    } else if works("Linux") {
                        "linux"
                    } else {
                        return None;
                    };
                    let img = p.get("image").and_then(Value::as_str).unwrap_or("");
                    let img = if img.starts_with("//") { format!("https:{img}") } else { img.to_string() };
                    Some(Owned {
                        app_name: id,
                        title: p.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                        art_square: String::new(),
                        art_cover: if img.is_empty() { String::new() } else { format!("{img}_bg_crop_1920x655.jpg") },
                        art_logo: String::new(),
                        platform: platform.into(),
                        cloud: false,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn db_image(v: &Value, key: &str) -> String {
    v.pointer(&format!("/game/{key}/url_format")).and_then(Value::as_str).map(|u| u.replace("{formatter}", "").replace("{ext}", "jpg")).unwrap_or_default()
}

/// Fetches the owned games and keeps them for the library. Returns how many.
pub fn refresh_owned() -> Result<usize, String> {
    let tok = token()?;
    let mut list: Vec<Owned> = Vec::new();
    let mut page = 1;
    loop {
        let v = get(&format!("https://embed.gog.com/account/getFilteredProducts?mediaType=1&sortBy=title&page={page}"), Some(&tok))?;
        list.extend(parse_products(&v));
        let pages = v.get("totalPages").and_then(Value::as_u64).unwrap_or(1);
        page += 1;
        if page as u64 > pages || page > 100 {
            break;
        }
    }
    // Covers, logos and wide art from GOG's games database, once per game.
    let known = crate::load_owned(Store::Gog);
    let todo: Vec<usize> = list
        .iter_mut()
        .enumerate()
        .filter_map(|(i, g)| match known.iter().find(|k| k.app_name == g.app_name && !k.art_square.is_empty()) {
            Some(k) => {
                g.art_square = k.art_square.clone();
                g.art_logo = k.art_logo.clone();
                if !k.art_cover.is_empty() {
                    g.art_cover = k.art_cover.clone();
                }
                g.cloud = k.cloud;
                None
            }
            None => Some(i),
        })
        .collect();
    let ids: Vec<(usize, String)> = todo.iter().map(|i| (*i, list[*i].app_name.clone())).collect();
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for chunk in ids.chunks(ids.len().div_ceil(6).max(1)) {
            let results = &results;
            s.spawn(move || {
                for (i, id) in chunk {
                    if let Ok(v) = get(&format!("https://gamesdb.gog.com/platforms/gog/external_releases/{id}"), None) {
                        results.lock().unwrap().push((*i, db_image(&v, "vertical_cover"), db_image(&v, "background"), db_image(&v, "logo")));
                    }
                }
            });
        }
    });
    for (i, square, wide, logo) in results.into_inner().unwrap_or_default() {
        let g = &mut list[i];
        g.art_square = square;
        if !wide.is_empty() {
            g.art_cover = wide;
        }
        g.art_logo = logo;
    }
    let n = list.len();
    crate::save_owned(Store::Gog, list);
    crate::changed();
    Ok(n)
}

/// Heroic's installed.json entries.
fn installed_list() -> Vec<Value> {
    read_json(&installed_path()).and_then(|v| v.get("installed").and_then(Value::as_array).cloned()).unwrap_or_default()
}

fn save_installed(list: Vec<Value>) {
    write_json(&installed_path(), &json!({ "installed": list }));
}

/// (install path, platform, build id) of an installed game.
pub fn installed(id: &str) -> Option<(String, String, String)> {
    installed_list().into_iter().find(|g| g.get("appName").and_then(Value::as_str) == Some(id)).map(|g| {
        let s = |k: &str| g.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        (s("install_path"), s("platform"), s("buildId"))
    })
}

/// "4.21 GiB", Heroic's install_size format.
fn gib(b: u64) -> String {
    format!("{:.2} GiB", b as f64 / 1024.0 / 1024.0 / 1024.0)
}

/// Records an install (or an update of one) the way Heroic does.
fn record(id: &str, dir: &Path, platform: &str, size: u64) {
    let info = gogdl(&["import", &dir.to_string_lossy()], NET).ok().and_then(|o| last_json(&o)).unwrap_or(Value::Null);
    let s = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut list = installed_list();
    let old = list.iter().position(|g| g.get("appName").and_then(Value::as_str) == Some(id));
    let size = if size > 0 { gib(size) } else { old.and_then(|i| list[i].get("install_size").and_then(Value::as_str).map(String::from)).unwrap_or_default() };
    let entry = json!({
        "platform": platform,
        "executable": "",
        "install_path": dir.to_string_lossy(),
        "install_size": size,
        "is_dlc": false,
        "version": s("versionName"),
        "appName": id,
        "installedWithDLCs": false,
        "language": if s("installedLanguage").is_empty() { "en-US".to_string() } else { s("installedLanguage") },
        "versionEtag": "",
        "buildId": s("buildId"),
        "pinnedVersion": false,
    });
    match old {
        Some(i) => list[i] = entry,
        None => list.push(entry),
    }
    save_installed(list);
}

fn platform_of(id: &str) -> String {
    installed(id)
        .map(|i| i.1)
        .filter(|p| !p.is_empty())
        .or_else(|| crate::load_owned(Store::Gog).into_iter().find(|o| o.app_name == id).map(|o| o.platform))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "windows".into())
}

/// What an install will take: (download, disk) bytes and the folder name.
pub fn info(id: &str) -> Result<(u64, u64, String), String> {
    let p = platform_of(id);
    let out = gogdl(&["info", id, "--platform", &p, "--lang", "en-US"], Duration::from_secs(120))?;
    let v = last_json(&out).ok_or("GOG didn't answer. Try again.")?;
    let folder = v.get("folder_name").and_then(Value::as_str).unwrap_or(id).to_string();
    // What it needs installed in its prefix before the first start (redist.rs).
    if let Some(deps) = v.get("dependencies").and_then(Value::as_array) {
        crate::redist::save_deps(id, deps.iter().filter_map(|d| d.as_str().map(String::from)).collect());
    }
    let (mut dl, mut disk) = (v.get("download_size").and_then(Value::as_u64).unwrap_or(0), v.get("disk_size").and_then(Value::as_u64).unwrap_or(0));
    if let Some(Value::Object(langs)) = v.get("size") {
        for (k, s) in langs {
            if k == "*" || k.eq_ignore_ascii_case("en-US") || k == "en" {
                dl += s.get("download_size").and_then(Value::as_u64).unwrap_or(0);
                disk += s.get("disk_size").and_then(Value::as_u64).unwrap_or(0);
            }
        }
    }
    Ok((dl, disk, folder))
}

/// The arguments for an install, update or repair job, and the game's folder afterwards.
pub fn job_args(kind: &crate::jobs::Kind, id: &str) -> Result<(Vec<String>, PathBuf, u64), String> {
    use crate::jobs::Kind;
    let p = platform_of(id);
    let workers = std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4).to_string();
    let mut a = base_args();
    let (cmd, path, dir, size) = match kind {
        Kind::Install { base } => {
            let (_, disk, folder) = info(id)?;
            ("download", base.clone(), base.join(folder), disk)
        }
        _ => {
            let (dir, _, _) = installed(id).ok_or("It isn't installed.")?;
            let dir = PathBuf::from(dir);
            (if matches!(kind, Kind::Repair) { "repair" } else { "update" }, dir.clone(), dir, 0)
        }
    };
    a.extend([cmd.to_string(), id.to_string(), "--platform".into(), p, "--path".into(), path.to_string_lossy().into_owned(), "--lang".into(), "en-US".into(), "--skip-dlcs".into(), "--max-workers".into(), workers]);
    Ok((a, dir, size))
}

/// After a finished install, update or repair.
pub fn finish(id: &str, dir: &Path, size: u64) {
    record(id, dir, &platform_of(id), size);
}

/// Installed games with a newer build on GOG.
pub fn updates() -> Result<Vec<String>, String> {
    if account().is_none() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for g in installed_list() {
        let s = |k: &str| g.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (id, platform, build) = (s("appName"), s("platform"), s("buildId"));
        if platform == "linux" || build.is_empty() || id.is_empty() {
            continue;
        }
        if let Ok(v) = get(&format!("https://content-system.gog.com/products/{id}/os/{platform}/builds?generation=2"), None) {
            let latest = v.pointer("/items/0/build_id").and_then(Value::as_str).unwrap_or("");
            if !latest.is_empty() && latest != build {
                out.push(id);
            }
        }
    }
    Ok(out)
}

/// gogdl, with the auth file (saves.rs).
pub(crate) fn run_saves(args: &[&str]) -> Result<String, String> {
    gogdl(args, Duration::from_secs(180))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_code() {
        assert_eq!(code_in("https://embed.gog.com/on_login_success?origin=client&code=abc-123"), Some("abc-123".into()));
        assert_eq!(code_in("https://embed.gog.com/on_login_success?origin=client"), None);
        assert_eq!(code_in("https://auth.gog.com/auth?code=x"), None);
    }

    #[test]
    fn reads_products() {
        let v = json!({"totalPages":1,"products":[
            {"id":1207658924,"title":"The Witcher 3","image":"//images-1.gog-statics.com/abc","worksOn":{"Windows":true,"Linux":false},"isGame":true},
            {"id":5,"title":"Mac only","worksOn":{"Mac":true},"isGame":true}]});
        let l = parse_products(&v);
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].app_name.as_str(), l[0].platform.as_str()), ("1207658924", "windows"));
        assert!(l[0].art_cover.starts_with("https://images-1.gog-statics.com/abc"));
    }
}
