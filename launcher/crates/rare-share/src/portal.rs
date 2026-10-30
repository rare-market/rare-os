//! xdg-desktop-portal Screenshot over D-Bus, non-interactive (no dialog, ever).

use dbus::arg::{prop_cast, PropMap, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::message::MatchRule;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// The Response signal's code and file uri, once it arrives.
type Answer = Arc<Mutex<Option<(u32, Option<String>)>>>;

/// True when the portal service is on the session bus (or can be started by it).
pub fn available() -> bool {
    let Ok(conn) = Connection::new_session() else { return false };
    let proxy = conn.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", Duration::from_secs(2));
    let has = |method: &str| -> bool {
        let r: Result<(Vec<String>,), _> = proxy.method_call("org.freedesktop.DBus", method, ());
        r.map(|(names,)| names.iter().any(|n| n == DEST)).unwrap_or(false)
    };
    has("ListNames") || has("ListActivatableNames")
}

/// Takes a full-screen screenshot without asking. Returns the file the portal wrote.
pub fn screenshot(timeout: Duration) -> Result<PathBuf, String> {
    let conn = Connection::new_session().map_err(|e| e.to_string())?;
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let token = format!("rare_share_{ms}");
    let sender = conn.unique_name().to_string().trim_start_matches(':').replace('.', "_");
    let req = dbus::Path::new(format!("{PATH}/request/{sender}/{token}")).map_err(|e| e.to_string())?;
    let result: Answer = Arc::default();
    let sink = result.clone();
    let rule = MatchRule::new_signal("org.freedesktop.portal.Request", "Response").with_path(req);
    conn.add_match(rule, move |(code, res): (u32, PropMap), _, _| {
        *sink.lock().unwrap() = Some((code, prop_cast::<String>(&res, "uri").cloned()));
        false
    })
    .map_err(|e| e.to_string())?;
    let mut opts: PropMap = HashMap::new();
    opts.insert("handle_token".into(), Variant(Box::new(token) as Box<dyn RefArg>));
    opts.insert("interactive".into(), Variant(Box::new(false) as Box<dyn RefArg>));
    opts.insert("modal".into(), Variant(Box::new(false) as Box<dyn RefArg>));
    let proxy = conn.with_proxy(DEST, PATH, Duration::from_secs(5));
    let _: (dbus::Path<'static>,) = proxy.method_call("org.freedesktop.portal.Screenshot", "Screenshot", ("", opts)).map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    while t0.elapsed() < timeout {
        conn.process(Duration::from_millis(200)).map_err(|e| e.to_string())?;
        if let Some((code, uri)) = result.lock().unwrap().take() {
            if code != 0 {
                return Err(format!("portal answered {code}"));
            }
            let uri = uri.ok_or("portal gave no file")?;
            return url::Url::parse(&uri).ok().and_then(|u| u.to_file_path().ok()).ok_or_else(|| format!("bad uri {uri}"));
        }
    }
    Err("portal timed out".into())
}
