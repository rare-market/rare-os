//! App ids for the programs Rare starts, in Rare OS game mode (gamescope --steam).
//!
//! gamescope with `--steam` only focuses a window that carries an app id. Steam tags what it
//! starts; nothing tagged an emulator, an app (Telegram, Chrome app windows), a web game opened in
//! Chrome or a Moonlight stream that Rare started, so they never showed. Checked against
//! gamescope 3.17.0:
//!
//! - Xwayland windows: `add_win` reads the client's pid with XRes and, in steam mode, sets
//!   `new_win->appID = gamescope::Process::GetAppIdFromPid( new_win->pid );` (steamcompmgr.cpp).
//!   Native Wayland windows: `wl_client_get_credentials( client, &nPid, ... );`
//!   `window->appID = gamescope::Process::GetAppIdFromPid( nPid );` (wlserver.cpp).
//! - `GetAppIdFromPid` reads `/proc/<pid>/cgroup` first (`GetAppIdFromCgroup`), then walks up to
//!   Steam's reaper (Utils/Process.cpp). `GetAppIdFromCgroup` takes the last path component and
//!   `sscanf( scope, "app-steam-app%u-%d.scope", &appid, &reaperpid ) == 2 && appid != 0`: the
//!   part after the id must be a number.
//!
//! So every program Rare starts (but Steam) runs in a transient scope
//! `app-steam-app<id>-<number>.scope` (launch.rs), with a stable id per run id: CRC-32 with the
//! top bit set, as Steam derives a non-Steam shortcut's id. The base-layer keeper
//! (gamescope.rs) puts the id of the app in front first, so gamescope shows it, and drops it
//! when the app ends or the launcher takes the screen back.
//!
//! Flatpak moves itself into its own `app-flatpak-<id>-<pid>.scope` before it starts the app, so
//! its windows get no id from the name. A tagger thread covers that: windows of a process in a
//! started program's tree (XRes pid, else `_NET_WM_PID`) get `STEAM_GAME` = the program's id, which
//! gamescope reads on every change (`if (ev->atom == ctx->atoms.gameAtom) ... w->appID = appID;`).
//! Flatpaks start without WAYLAND_DISPLAY in game mode, so they are always X11 windows it can tag.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Steam's non-Steam shortcut range: the top bit set. 769 (Steam's UI) and Rare's own id have it
/// clear, so no derived id can be either.
const SHORTCUT_BIT: u32 = 0x8000_0000;

/// CRC-32 (IEEE, as zlib and Steam's shortcut ids use).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for b in bytes {
        crc ^= *b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The app id of a run id ("app:telegram", "emu:duckstation:...", "web:act"): the same on every
/// start and every device.
pub fn for_run(run_id: &str) -> u32 {
    crc32(format!("rare:{run_id}").as_bytes()) | SHORTCUT_BIT
}

/// The transient scope name gamescope reads `id` from. `n` makes it unique; it must stay a
/// positive int for gamescope's `%d`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn scope_unit(id: u32, n: u32) -> String {
    format!("app-steam-app{id}-{}", n % 1_000_000_000)
}

/// gamescope's `GetAppIdFromCgroup` for one cgroup path, for the tests.
#[cfg(test)]
fn gamescope_cgroup_appid(path: &str) -> u32 {
    let scope = path.rsplit('/').next().unwrap_or(path);
    let Some(rest) = scope.strip_prefix("app-steam-app") else { return 0 };
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    let Ok(appid) = rest[..digits].parse::<u32>() else { return 0 };
    let Some(rest) = rest[digits..].strip_prefix('-') else { return 0 };
    let rest = rest.strip_prefix(['-', '+']).unwrap_or(rest);
    if !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return 0;
    }
    appid
}

fn registry() -> std::sync::MutexGuard<'static, HashMap<String, u32>> {
    static R: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Gives `run_id` its app id (once started in game mode) and returns it.
pub fn assign(run_id: &str) -> u32 {
    let id = for_run(run_id);
    registry().insert(run_id.to_string(), id);
    id
}

/// `run_id` shows under another run's id (a Heroic game under Heroic's scope).
pub fn alias(run_id: &str, id: u32) {
    registry().insert(run_id.to_string(), id);
}

/// The app id of something Rare started in game mode.
pub fn of(run_id: &str) -> Option<u32> {
    registry().get(run_id).copied()
}

/// Every id Rare has handed out: the keeper takes them out of the base-layer list unless in front.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn all() -> Vec<u32> {
    let mut v: Vec<u32> = registry().values().copied().collect();
    v.sort();
    v.dedup();
    v
}

/// The base-layer app list for what has the screen now, Steam's ids kept in their order. With an
/// app of Rare's in front (`front`): its id, then Rare's (shown while the app has no window yet,
/// and the moment it closes), then Steam's. Otherwise Rare's id first (`rare_first`) or last.
/// Ids of Rare's other apps (`ours`) are left out. None when the list is that already.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn place(list: &[u64], rare_first: bool, front: Option<u32>, ours: &[u32]) -> Option<Vec<u64>> {
    let rare = super::gamescope::RARE_APPID as u64;
    let mine = |v: &u64| *v == rare || ours.iter().any(|o| *o as u64 == *v) || front.is_some_and(|f| f as u64 == *v);
    let others: Vec<u64> = list.iter().copied().filter(|v| !mine(v)).collect();
    let out: Vec<u64> = match front {
        Some(f) => [f as u64, rare].into_iter().chain(others).collect(),
        None if rare_first => std::iter::once(rare).chain(others).collect(),
        None => others.into_iter().chain(std::iter::once(rare)).collect(),
    };
    (out != list).then_some(out)
}

/// Tags windows of programs Rare started that gamescope found no app id for (Flatpak): see the
/// module docs. Rare OS game mode only.
#[cfg(target_os = "linux")]
pub fn start_tagger() {
    std::thread::spawn(move || loop {
        if let Err(e) = tag_windows() {
            crate::host::log(&format!("[gamescope] tagging app windows: {e}"));
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
}

/// The processes that own windows carrying app id `id` (Rare tagged them for a run): a run Rare lost
/// track of can still be closed through them.
#[cfg(target_os = "linux")]
pub fn window_pids(id: u32) -> Vec<u32> {
    use super::gamescope::intern;
    use x11rb::connection::{Connection as _, RequestConnection as _};
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
    let Ok((conn, screen)) = x11rb::connect(None) else { return vec![] };
    let root = conn.setup().roots[screen].root;
    let (Ok(game), Ok(wm_pid)) = (intern(&conn, super::gamescope::GAME_PROP), intern(&conn, "_NET_WM_PID")) else { return vec![] };
    let res = conn.extension_information(x11rb::protocol::res::X11_EXTENSION_NAME).ok().flatten().is_some();
    let Ok(Ok(tree)) = conn.query_tree(root).map(|c| c.reply()) else { return vec![] };
    let mut pids = vec![];
    for w in tree.children {
        let tagged = conn.get_property(false, w, game, AtomEnum::CARDINAL, 0, 1).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().and_then(|mut v| v.next()));
        if tagged == Some(id) {
            if let Some(p) = window_pid(&conn, w, res, wm_pid) {
                if !pids.contains(&p) {
                    pids.push(p);
                }
            }
        }
    }
    pids
}

#[cfg(not(target_os = "linux"))]
pub fn window_pids(_id: u32) -> Vec<u32> {
    vec![]
}

/// The pid of an X client's window: XRes (the host pid, as gamescope reads it, also for Flatpak
/// apps in their own pid namespace), else `_NET_WM_PID`.
#[cfg(target_os = "linux")]
fn window_pid(conn: &impl x11rb::connection::Connection, w: u32, res: bool, wm_pid: u32) -> Option<u32> {
    use x11rb::protocol::res::{ClientIdMask, ClientIdSpec, ConnectionExt as _};
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
    if res {
        let spec = ClientIdSpec { client: w, mask: ClientIdMask::LOCAL_CLIENT_PID };
        if let Ok(Ok(reply)) = conn.res_query_client_ids(&[spec]).map(|c| c.reply()) {
            if let Some(pid) = reply.ids.iter().find(|v| v.spec.mask == ClientIdMask::LOCAL_CLIENT_PID).and_then(|v| v.value.first().copied()).filter(|p| *p > 0) {
                return Some(pid);
            }
        }
    }
    let reply = conn.get_property(false, w, wm_pid, AtomEnum::CARDINAL, 0, 1).ok()?.reply().ok()?;
    reply.value32().and_then(|mut v| v.next()).filter(|p| *p > 0)
}

#[cfg(target_os = "linux")]
fn tag_windows() -> Result<(), String> {
    use std::collections::HashSet;
    use std::time::Duration;
    use super::gamescope::intern;
    use x11rb::connection::{Connection, RequestConnection as _};
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    let (conn, screen) = x11rb::connect(None).map_err(|e| format!("x11: {e}"))?;
    let root = conn.setup().roots[screen].root;
    let game = intern(&conn, super::gamescope::GAME_PROP)?;
    let wm_pid = intern(&conn, "_NET_WM_PID")?;
    let res = conn.extension_information(x11rb::protocol::res::X11_EXTENSION_NAME).ok().flatten().is_some();
    // Windows done with: tagged or carrying an id already, or of a process outside every tree
    // (a window's pid never changes, and a process never joins a tree that started after it).
    let mut done: HashSet<u32> = HashSet::new();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let kids: Vec<(u32, u32)> = crate::host::host().child_pids().into_iter().filter_map(|(run, pid)| of(&run).map(|id| (id, pid))).collect();
        if kids.is_empty() {
            done.clear();
            continue;
        }
        let tree = conn.query_tree(root).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        done.retain(|w| tree.children.contains(w));
        let fresh: Vec<u32> = tree.children.iter().copied().filter(|w| !done.contains(w)).collect();
        if fresh.is_empty() {
            continue;
        }
        let mut owner: HashMap<u32, u32> = HashMap::new();
        for (id, pid) in &kids {
            for p in crate::host::host().with_descendants(&[*pid]) {
                owner.entry(p).or_insert(*id);
            }
        }
        let mut wrote = false;
        for w in fresh {
            let Some(pid) = window_pid(&conn, w, res, wm_pid) else { continue };
            let Some(id) = owner.get(&pid).copied() else {
                done.insert(w);
                continue;
            };
            let has = conn.get_property(false, w, game, AtomEnum::CARDINAL, 0, 1).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
            if has.value32().and_then(|mut v| v.next()).is_some_and(|v| v != 0) {
                done.insert(w);
                continue;
            }
            conn.change_property32(PropMode::REPLACE, w, game, AtomEnum::CARDINAL, &[id]).map_err(|e| e.to_string())?;
            wrote = true;
            done.insert(w);
            crate::host::log(&format!("[gamescope] window 0x{w:x} of pid {pid} tagged {id}"));
        }
        if wrote {
            conn.flush().map_err(|e| e.to_string())?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamescope::RARE_APPID;

    const RARE: u64 = RARE_APPID as u64;
    const STEAM: u64 = 769;
    const GAME: u64 = 1245620;

    #[test]
    fn crc32_is_the_standard_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn ids_are_stable_shortcut_ids() {
        let runs = ["app:telegram", "app:x", "app:netflix", "web:act", "exec:duckstation", "exec:pcsx2", "exec:rpcs3", "exec:cemu", "exec:azahar", "exec:es-de", "stream:abc:1"];
        let ids: Vec<u32> = runs.iter().map(|r| for_run(r)).collect();
        for (r, id) in runs.iter().zip(&ids) {
            assert_eq!(for_run(r), *id, "stable");
            assert!(*id & SHORTCUT_BIT != 0, "{r}: in the shortcut range");
            assert!(*id != 769 && *id != RARE_APPID && *id != 0, "{r}");
        }
        let mut uniq = ids.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), ids.len(), "distinct");
        // The same derivation Steam uses for shortcuts, over "rare:<run id>".
        assert_eq!(for_run("app:telegram"), crc32(b"rare:app:telegram") | 0x8000_0000);
    }

    #[test]
    fn gamescope_reads_the_scope_name() {
        let id = for_run("app:telegram");
        let unit = scope_unit(id, 123_456_789);
        assert_eq!(unit, format!("app-steam-app{id}-123456789"));
        let cg = format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}.scope");
        assert_eq!(gamescope_cgroup_appid(cg.strip_prefix("0::").unwrap()), id);
        // Kept a positive int whatever n is.
        assert!(scope_unit(id, u32::MAX).rsplit('-').next().unwrap().parse::<i32>().is_ok());
        // What gamescope does not take: a word after the id (the old rare-<slug>-<ms> names), and
        // Flatpak's own scope.
        assert_eq!(gamescope_cgroup_appid(&format!("/app.slice/app-steam-app{id}-telegram.scope")), 0);
        assert_eq!(gamescope_cgroup_appid("/app.slice/rare-app-telegram-1761540000000.scope"), 0);
        assert_eq!(gamescope_cgroup_appid("/app.slice/app-flatpak-org.telegram.desktop-4242.scope"), 0);
        assert_eq!(gamescope_cgroup_appid("/app.slice/app-steam-app0-1.scope"), 0);
    }

    #[test]
    fn registry_hands_out_and_aliases() {
        let id = assign("test:registry");
        assert_eq!(of("test:registry"), Some(id));
        alias("test:registry-alias", id);
        assert_eq!(of("test:registry-alias"), Some(id));
        assert!(all().contains(&id));
        assert_eq!(of("test:never"), None);
    }

    /// gamescope's pick in steam mode (see gamescope.rs): first id in the list with a window,
    /// newest map first inside one id; the control window wins when set.
    fn pick(ctrl: Option<u64>, list: &[u64], windows: &[(u64, u64)]) -> Option<u64> {
        if let Some(w) = ctrl.filter(|w| windows.iter().any(|(x, app)| x == w && *app != 0)) {
            return Some(w);
        }
        list.iter().find_map(|id| windows.iter().find(|(_, app)| app == id).map(|(x, _)| *x))
    }

    fn kept(wrote: &[u64], first: bool, front: Option<u32>, ours: &[u32]) -> Vec<u64> {
        place(wrote, first, front, ours).unwrap_or_else(|| wrote.to_vec())
    }

    const MAIN: u64 = 0x400007;
    const STEAM_WIN: u64 = 0x600003;
    const GAME_WIN: u64 = 0x800002;
    const DUCK_WIN: u64 = 0xa00002;
    const TG_WIN: u64 = 0xc00004;

    #[test]
    fn placement() {
        let duck = for_run("exec:duckstation");
        let tg = for_run("app:telegram");
        let (d, t) = (duck as u64, tg as u64);
        let ours = [duck, tg];
        // The launcher in front: Rare first, Rare's apps out of the list.
        assert_eq!(place(&[], true, None, &ours), Some(vec![RARE]));
        assert_eq!(place(&[d, RARE, STEAM], true, None, &ours), Some(vec![RARE, STEAM]));
        // An app in front: its id, then Rare, then Steam's.
        assert_eq!(place(&[RARE, STEAM], true, Some(duck), &ours), Some(vec![d, RARE, STEAM]));
        assert_eq!(place(&[d, RARE, STEAM], true, Some(duck), &ours), None);
        // Switching apps swaps the head, the other app's id goes.
        assert_eq!(place(&[d, RARE, STEAM], true, Some(tg), &ours), Some(vec![t, RARE, STEAM]));
        // Steam writes its own list over ours: the keeper puts the app back first.
        assert_eq!(place(&[GAME, STEAM], true, Some(tg), &ours), Some(vec![t, RARE, GAME, STEAM]));
        // A Steam game in front: Rare last, Rare's apps out.
        assert_eq!(place(&[t, RARE, GAME, STEAM], false, None, &ours), Some(vec![GAME, STEAM, RARE]));
        // The old placement is the no-app case.
        for (l, first) in [(vec![STEAM], true), (vec![GAME, STEAM, RARE], false), (vec![RARE, STEAM, RARE], true)] {
            assert_eq!(place(&l, first, None, &[]), crate::gamescope::place_rare(&l, first));
        }
    }

    #[test]
    fn apps_get_the_screen_and_give_it_back() {
        let duck = for_run("exec:duckstation");
        let tg = for_run("app:telegram");
        let (d, t) = (duck as u64, tg as u64);
        let ours = [duck, tg];
        // DuckStation started: no window yet, the launcher stays (no black screen).
        let list = kept(&[], true, Some(duck), &ours);
        assert_eq!(pick(None, &list, &[(MAIN, RARE)]), Some(MAIN));
        // Its window maps (newest), and also when the launcher's window is newer.
        assert_eq!(pick(None, &list, &[(DUCK_WIN, d), (MAIN, RARE)]), Some(DUCK_WIN));
        assert_eq!(pick(None, &list, &[(MAIN, RARE), (DUCK_WIN, d)]), Some(DUCK_WIN));
        // Quiet Steam writes its list while DuckStation is up: DuckStation stays.
        let list = kept(&[STEAM], true, Some(duck), &ours);
        assert_eq!(pick(None, &list, &[(STEAM_WIN, STEAM), (DUCK_WIN, d), (MAIN, RARE)]), Some(DUCK_WIN));
        // Hold Select+Start: the Running list (the launcher's front, its window raised by control).
        let wins = [(DUCK_WIN, d), (MAIN, RARE)];
        let running = kept(&list, true, None, &ours);
        assert_eq!(pick(Some(MAIN), &running, &wins), Some(MAIN));
        // Resume: DuckStation again.
        assert_eq!(pick(None, &kept(&running, true, Some(duck), &ours), &wins), Some(DUCK_WIN));
        // Quit, or it exits: its windows go and the front goes back to the launcher.
        assert_eq!(pick(None, &list, &[(MAIN, RARE)]), Some(MAIN), "before the keeper's next pass");
        assert_eq!(pick(None, &kept(&list, true, None, &ours), &[(MAIN, RARE)]), Some(MAIN));
        // DuckStation in the background, Telegram in front: Telegram.
        let list = kept(&[], true, Some(tg), &ours);
        assert_eq!(pick(None, &list, &[(TG_WIN, t), (DUCK_WIN, d), (MAIN, RARE)]), Some(TG_WIN));
        // Telegram's window before the tagger reached it has no id: the launcher, not black.
        assert_eq!(pick(None, &list, &[(TG_WIN, 0), (DUCK_WIN, d), (MAIN, RARE)]), Some(MAIN));
        // Guide or the overlay back to the launcher: the launcher over both apps.
        assert_eq!(pick(None, &kept(&list, true, None, &ours), &[(TG_WIN, t), (DUCK_WIN, d), (MAIN, RARE)]), Some(MAIN));
        // A Steam game in front while DuckStation runs: the game.
        let list = kept(&[GAME, STEAM], false, None, &ours);
        assert_eq!(pick(None, &list, &[(GAME_WIN, GAME), (DUCK_WIN, d), (MAIN, RARE)]), Some(GAME_WIN));
    }
}
