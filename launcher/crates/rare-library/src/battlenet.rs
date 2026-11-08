//! Battle.net, Blizzard's Windows app, in Wine and Proton prefixes: the games it installed and how
//! to start them. Nothing of Blizzard's is linked or shipped; Rare reads the app's files and runs
//! the app through umu-run (Proton), the way it runs GOG's Windows builds.
//!
//! Prefixes: Rare's own (`~/.local/share/rare/prefixes/battlenet`, where Apps > Battle.net installs
//! it), Steam's compatdata (Battle.net added to Steam as a non-Steam game), and the usual Lutris,
//! Heroic, Bottles and plain Wine places. A prefix counts when it holds
//! `drive_c/Program Files (x86)/Battle.net/Battle.net.exe`.
//!
//! Games: the app's install list (`drive_c/ProgramData/Battle.net/Agent/product.db`, protobuf: each
//! product's uid, product code and install path), else the `.build.info` of each folder in
//! Program Files. A game starts through the app (`Battle.net.exe --exec="launch WoW"`), which signs
//! it in; Blizzard's games need the app running.

use crate::model::{Art, Game, LaunchSpec, Source};
use std::fs;
use std::path::{Path, PathBuf};

/// The app's folder inside drive_c.
pub const CLIENT_DIR: &str = "drive_c/Program Files (x86)/Battle.net";
/// The app's own program names (Battle.net.exe is the one `--exec` works on).
const CLIENT_EXES: [&str; 2] = ["Battle.net.exe", "Battle.net Launcher.exe"];
const PRODUCT_DB: &str = "drive_c/ProgramData/Battle.net/Agent/product.db";
/// Blizzard's installer for the app (the link its download page uses).
pub const SETUP_URL: &str = "https://downloader.battle.net/download/getInstallerForGame?os=win&gameProgram=BATTLENET_APP&version=Live";
/// The id the app runs under in Apps (`app:battlenet`) and the key of the player's Proton pick.
pub const APP_ID: &str = "battlenet";

/// A Blizzard game Rare knows by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    /// product.db's uid ("prometheus").
    pub uid: &'static str,
    /// The product code (`.build.info`'s Product column, product.db's product_code: "pro").
    pub code: &'static str,
    pub title: &'static str,
    /// What `--exec="launch <code>"` takes. None: Rare opens the app on its own and the player
    /// presses Play there.
    pub launch: Option<&'static str>,
    /// WoW keeps each flavour in a subfolder of one install ("_retail_", "_classic_").
    pub flavour: Option<&'static str>,
    /// The game's Steam appid, when it is on Steam too (art, and one tile for both copies).
    pub steam: Option<u32>,
}

const fn k(uid: &'static str, code: &'static str, title: &'static str, launch: Option<&'static str>, flavour: Option<&'static str>, steam: Option<u32>) -> Known {
    Known { uid, code, title, launch, flavour, steam }
}

pub const KNOWN: &[Known] = &[
    k("wow", "wow", "World of Warcraft", Some("WoW"), Some("_retail_"), None),
    k("wow_classic", "wow_classic", "World of Warcraft Classic", Some("WoWC"), Some("_classic_"), None),
    k("wow_classic_era", "wow_classic_era", "World of Warcraft Classic Era", None, Some("_classic_era_"), None),
    k("wow_beta", "wow_beta", "World of Warcraft Beta", None, Some("_beta_"), None),
    k("wow_ptr", "wowt", "World of Warcraft PTR", None, Some("_ptr_"), None),
    k("diablo3", "d3", "Diablo III", Some("D3"), None, None),
    k("fenris", "fen", "Diablo IV", Some("Fen"), None, Some(2344520)),
    k("osi", "osi", "Diablo II: Resurrected", Some("OSI"), None, None),
    k("anbs", "anbs", "Diablo Immortal", Some("ANBS"), None, None),
    k("prometheus", "pro", "Overwatch 2", Some("Pro"), None, Some(2357570)),
    k("hs_beta", "hsb", "Hearthstone", Some("WTCG"), None, None),
    k("heroes", "hero", "Heroes of the Storm", Some("Hero"), None, None),
    k("s2", "s2", "StarCraft II", Some("S2"), None, None),
    k("s1", "s1", "StarCraft: Remastered", Some("S1"), None, None),
    k("w3", "w3", "Warcraft III: Reforged", Some("W3"), None, None),
    k("gryphon", "gry", "Warcraft Rumble", Some("GRY"), None, None),
    k("rtro", "rtro", "Blizzard Arcade Collection", Some("RTRO"), None, None),
    k("wlby", "wlby", "Crash Bandicoot 4: It's About Time", Some("WLBY"), None, Some(1378990)),
    k("auks", "auks", "Call of Duty", Some("AUKS"), None, Some(1938090)),
    k("odin", "odin", "Call of Duty: Modern Warfare", Some("ODIN"), None, None),
    k("lazarus", "lazr", "Call of Duty: Modern Warfare 2 Campaign Remastered", Some("LAZR"), None, None),
    k("viper", "vipr", "Call of Duty: Black Ops 4", Some("VIPR"), None, None),
    k("zeus", "zeus", "Call of Duty: Black Ops Cold War", Some("ZEUS"), None, None),
    k("fore", "fore", "Call of Duty: Vanguard", Some("FORE"), None, None),
];

/// The app itself and its helpers: never a game.
fn is_client(uid_or_code: &str) -> bool {
    matches!(uid_or_code.to_lowercase().as_str(), "agent" | "battle.net" | "bna" | "bts" | "app")
}

/// A product by its uid or its product code (either may be what a file says).
pub fn known(uid_or_code: &str) -> Option<&'static Known> {
    let s = uid_or_code.to_lowercase();
    KNOWN.iter().find(|k| k.uid == s || k.code == s)
}

/// One installed product from product.db.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub uid: String,
    pub code: String,
    /// A Windows path ("C:/Program Files (x86)/World of Warcraft" or with backslashes).
    pub path: String,
    /// The app's own state says the install finished (true when the file has no state for it).
    /// A game being downloaded has its folder already, and false here.
    pub installed: bool,
}

// ---------- product.db (protobuf) ----------

struct Pb<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Pb<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.b.get(self.i)?;
            self.i += 1;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    /// The next field: (number, a length-delimited body when it is one). Skips other wire types.
    fn field(&mut self) -> Option<(u64, Option<&'a [u8]>)> {
        let key = self.varint()?;
        let (num, wire) = (key >> 3, key & 7);
        match wire {
            0 => {
                self.varint()?;
                Some((num, None))
            }
            1 => {
                self.i = self.i.checked_add(8).filter(|e| *e <= self.b.len())?;
                Some((num, None))
            }
            2 => {
                let n = usize::try_from(self.varint()?).ok()?;
                let end = self.i.checked_add(n).filter(|e| *e <= self.b.len())?;
                let body = &self.b[self.i..end];
                self.i = end;
                Some((num, Some(body)))
            }
            5 => {
                self.i = self.i.checked_add(4).filter(|e| *e <= self.b.len())?;
                Some((num, None))
            }
            _ => None,
        }
    }
    fn fields(b: &'a [u8]) -> impl Iterator<Item = (u64, Option<&'a [u8]>)> {
        let mut p = Pb { b, i: 0 };
        std::iter::from_fn(move || p.field())
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The first varint of field `num` in a message.
fn varint_field(b: &[u8], num: u64) -> Option<u64> {
    let mut p = Pb { b, i: 0 };
    loop {
        let start = p.i;
        let key = p.varint()?;
        if key >> 3 == num && key & 7 == 0 {
            return p.varint();
        }
        p.i = start;
        p.field()?;
    }
}

/// The products in product.db: Database.product_install (1) = { uid (1), product_code (2),
/// settings (3) = { install_path (1) }, cached_product_state (4) = { base_product_state (1) =
/// { installed (1) } } }. Never fails: malformed input yields what parsed.
pub fn parse_product_db(bytes: &[u8]) -> Vec<Product> {
    let mut out = Vec::new();
    for (num, body) in Pb::fields(bytes) {
        let (1, Some(install)) = (num, body) else { continue };
        let (mut uid, mut code, mut path, mut installed) = (String::new(), String::new(), String::new(), true);
        for (n, b) in Pb::fields(install) {
            match (n, b) {
                (1, Some(b)) => uid = text(b),
                (2, Some(b)) => code = text(b),
                (3, Some(settings)) => {
                    if let Some((_, Some(p))) = Pb::fields(settings).find(|(n, b)| *n == 1 && b.is_some()) {
                        path = text(p);
                    }
                }
                (4, Some(state)) => {
                    if let Some((_, Some(base))) = Pb::fields(state).find(|(n, b)| *n == 1 && b.is_some()) {
                        installed = varint_field(base, 1).unwrap_or(0) != 0;
                    }
                }
                _ => {}
            }
        }
        if !uid.is_empty() {
            out.push(Product { uid, code, path, installed });
        }
    }
    out
}

/// The active products of a `.build.info` (Blizzard's install table: a `Name!TYPE:n|...` header,
/// then one `|`-separated row per product; WoW lists one row per flavour in one folder).
pub fn build_info_products(text: &str) -> Vec<String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else { return vec![] };
    let cols: Vec<String> = header.split('|').map(|c| c.split('!').next().unwrap_or("").trim().to_lowercase()).collect();
    let Some(pi) = cols.iter().position(|c| c == "product") else { return vec![] };
    let ai = cols.iter().position(|c| c == "active");
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        let row: Vec<&str> = l.split('|').collect();
        let active = ai.and_then(|i| row.get(i)).map(|a| a.trim() != "0").unwrap_or(true);
        if let Some(p) = row.get(pi).map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
            if active && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

// ---------- prefixes ----------

/// The folder holding drive_c (a Proton prefix keeps it in `pfx`).
pub fn drive_root(prefix: &Path) -> PathBuf {
    crate::umu::wine_root(prefix)
}

/// WINEPREFIX for a program inside a prefix: the folder above drive_c, or above `pfx` for a
/// Proton prefix (umu and Proton use `<WINEPREFIX>/pfx`).
pub fn prefix_of(program: &Path) -> Option<PathBuf> {
    let mut p = program.parent();
    while let Some(d) = p {
        if d.file_name().is_some_and(|n| n == "drive_c") {
            let root = d.parent()?;
            return Some(if root.file_name().is_some_and(|n| n == "pfx") { root.parent()?.to_path_buf() } else { root.to_path_buf() });
        }
        p = d.parent();
    }
    None
}

/// The app's program in a prefix, when it is installed there.
pub fn client_in(prefix: &Path) -> Option<PathBuf> {
    let dir = drive_root(prefix).join(CLIENT_DIR);
    CLIENT_EXES.iter().map(|e| dir.join(e)).find(|p| p.is_file())
}

/// True for the app's own program (what games and the app itself run through).
pub fn is_client_exe(program: &Path) -> bool {
    program.file_name().is_some_and(|n| CLIENT_EXES.iter().any(|e| n.eq_ignore_ascii_case(e))) && prefix_of(program).is_some()
}

/// Rare's own prefix for the app (Apps > Battle.net installs into it).
pub fn rare_prefix() -> PathBuf {
    crate::umu::rare_prefix(APP_ID)
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(p).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// Every place a Battle.net prefix may be, best first: Rare's, Steam's compatdata (non-Steam
/// shortcuts), then Lutris, Heroic, Bottles and plain Wine.
pub fn candidate_prefixes() -> Vec<PathBuf> {
    let mut v = vec![rare_prefix()];
    for root in crate::steam::find_roots() {
        for lib in crate::steam::registered_libraries(&root.path) {
            v.extend(subdirs(&lib.join("steamapps/compatdata")));
        }
    }
    if let Some(h) = dirs::home_dir() {
        // Lutris (~/Games/battlenet), Heroic's sideload prefixes, Bottles, Faugus, ~/.wine.
        v.extend(subdirs(&h.join("Games")));
        v.extend(subdirs(&h.join("Games/Heroic/Prefixes/default")));
        v.extend(subdirs(&h.join(".local/share/bottles/bottles")));
        v.extend(subdirs(&h.join(".var/app/com.usebottles.bottles/data/bottles/bottles")));
        v.extend(subdirs(&h.join("Faugus")));
        v.push(h.join(".wine"));
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|p| seen.insert(p.clone()));
    v
}

/// The prefixes that have the app installed, best first.
pub fn installed_prefixes(candidates: &[PathBuf]) -> Vec<PathBuf> {
    candidates.iter().filter(|p| client_in(p).is_some()).cloned().collect()
}

/// The app's program, in the first prefix that has it.
pub fn find_client() -> Option<PathBuf> {
    candidate_prefixes().iter().find_map(|p| client_in(p))
}

/// A Windows path in a prefix: C: is drive_c, other letters go through dosdevices.
pub fn win_path(prefix: &Path, win: &str) -> Option<PathBuf> {
    let w = win.replace('\\', "/");
    let (drive, rest) = w.split_once(':')?;
    if drive.len() != 1 {
        return None;
    }
    let root = drive_root(prefix);
    let base = if drive.eq_ignore_ascii_case("c") { root.join("drive_c") } else { root.join("dosdevices").join(format!("{}:", drive.to_lowercase())) };
    let rest = rest.trim_start_matches('/');
    Some(if rest.is_empty() { base } else { base.join(rest) })
}

/// A game's folder as its processes show it. Wine names a Windows program by its Windows path
/// (C:\Program Files (x86)\World of Warcraft\_retail_\Wow.exe), so this is the folder from the
/// drive's colon on, lower-cased with forward slashes: ":/program files (x86)/world of
/// warcraft/_retail_/". The runner tracks a Battle.net game by it (rare_core::proc::mentions),
/// since Battle.net stays open after the game.
pub fn run_needle(install_dir: &Path) -> Option<String> {
    let parts: Vec<String> = install_dir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let at = parts.iter().enumerate().rposition(|(i, p)| p == "drive_c" || (p.len() == 2 && p.ends_with(':') && i > 0 && parts[i - 1] == "dosdevices"))?;
    let rest: Vec<&str> = parts[at + 1..].iter().map(|s| s.as_str()).collect();
    if rest.is_empty() {
        return None;
    }
    Some(format!(":/{}/", rest.join("/").to_lowercase()))
}

// ---------- games ----------

/// `--exec="launch <code>"` for a known game; nothing (the app opens) otherwise.
pub fn launch_args(launch: Option<&str>) -> Vec<String> {
    match launch {
        Some(c) => vec![format!("--exec=launch {c}")],
        None => vec![],
    }
}

/// Steam's art for a game that is on Steam too.
fn steam_art(appid: u32) -> Art {
    let base = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
    Art { cover: vec![format!("{base}/library_600x900_2x.jpg")], hero: vec![format!("{base}/library_hero.jpg")], logo: vec![format!("{base}/logo.png")], header: vec![format!("{base}/header.jpg")], icon: vec![] }
}

/// One game: started through the app in its prefix.
fn game(client: &Path, uid: &str, dir: &Path) -> Game {
    let kn = known(uid);
    let title = kn.map(|k| k.title.to_string()).unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| uid.to_string()));
    let uid = kn.map(|k| k.uid).unwrap_or(uid);
    Game {
        id: format!("battlenet:{uid}"),
        source: Source::Battlenet,
        title,
        installed: true,
        install_dir: Some(dir.to_string_lossy().into_owned()),
        size_bytes: None,
        last_played: None,
        playtime_minutes: None,
        art: kn.and_then(|k| k.steam).map(steam_art).unwrap_or_default(),
        launch: LaunchSpec::Exec {
            program: client.to_string_lossy().into_owned(),
            args: launch_args(kn.and_then(|k| k.launch)),
            cwd: client.parent().map(|p| p.to_string_lossy().into_owned()),
            windows: true,
        },
        origin: "Battle.net".into(),
        foreign: false,
    }
}

/// The game's folder: the install path, and WoW's flavour folder in it when that is there.
fn game_dir(base: PathBuf, uid: &str) -> PathBuf {
    match known(uid).and_then(|k| k.flavour) {
        Some(f) if base.join(f).is_dir() => base.join(f),
        _ => base,
    }
}

/// The games of one prefix that has the app.
pub fn games_in(prefix: &Path) -> Vec<Game> {
    let Some(client) = client_in(prefix) else { return vec![] };
    let root = drive_root(prefix);
    let mut out: Vec<Game> = Vec::new();
    let add = |uid: &str, base: PathBuf, out: &mut Vec<Game>| {
        if is_client(uid) || !base.is_dir() || base.starts_with(root.join(CLIENT_DIR)) {
            return;
        }
        let g = game(&client, uid, &game_dir(base, uid));
        if !out.iter().any(|o| o.id == g.id) {
            out.push(g);
        }
    };
    // Downloading: listed with its folder already made, not installed yet.
    let mut pending: Vec<String> = Vec::new();
    if let Ok(bytes) = fs::read(root.join(PRODUCT_DB)) {
        for p in parse_product_db(&bytes) {
            if !p.installed {
                pending.push(p.uid);
                continue;
            }
            if let Some(base) = win_path(prefix, &p.path) {
                add(&p.uid, base, &mut out);
            }
        }
    }
    // No install list (or an old one): each Program Files folder with a .build.info.
    for pf in ["drive_c/Program Files (x86)", "drive_c/Program Files"] {
        for d in subdirs(&root.join(pf)) {
            let Ok(t) = fs::read_to_string(d.join(".build.info")) else { continue };
            for code in build_info_products(&t) {
                let uid = known(&code).map(|k| k.uid.to_string()).unwrap_or(code);
                if pending.contains(&uid) {
                    continue;
                }
                add(&uid, d.clone(), &mut out);
            }
        }
    }
    out
}

/// What can change the Battle.net games: each prefix's install list (uids and whether each one
/// finished) and its Program Files folders. For the library watcher: steady while a game
/// downloads (the app rewrites product.db with its progress all the time), moves when one
/// finishes, appears or goes.
pub fn signature() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in installed_prefixes(&candidate_prefixes()) {
        p.hash(&mut h);
        let root = drive_root(&p);
        if let Ok(b) = fs::read(root.join(PRODUCT_DB)) {
            for x in parse_product_db(&b) {
                (x.uid, x.installed).hash(&mut h);
            }
        }
        for pf in ["drive_c/Program Files (x86)", "drive_c/Program Files"] {
            for d in subdirs(&root.join(pf)) {
                (d.file_name().map(|n| n.to_os_string()), d.join(".build.info").is_file()).hash(&mut h);
            }
        }
    }
    h.finish()
}

pub struct Scan {
    pub games: Vec<Game>,
    /// The app's program, when a prefix has it.
    pub client: Option<PathBuf>,
    /// Game id to "steam:<appid>" for games that are on Steam too.
    pub keys: Vec<(String, String)>,
}

pub fn scan() -> Scan {
    scan_in(&candidate_prefixes())
}

pub fn scan_in(candidates: &[PathBuf]) -> Scan {
    let prefixes = installed_prefixes(candidates);
    let mut games: Vec<Game> = Vec::new();
    for p in &prefixes {
        for g in games_in(p) {
            if !games.iter().any(|o| o.id == g.id) {
                games.push(g);
            }
        }
    }
    let keys = games.iter().filter_map(|g| known(g.id.strip_prefix("battlenet:")?).and_then(|k| k.steam).map(|a| (g.id.clone(), format!("steam:{a}")))).collect();
    Scan { client: prefixes.first().and_then(|p| client_in(p)), games, keys }
}

// ---------- running ----------

/// Steam's Proton for a non-Steam shortcut's compatdata prefix (config.vdf CompatToolMapping),
/// else Steam's default tool.
fn steam_tool(prefix: &Path) -> Option<String> {
    let appid = prefix.file_name()?.to_str()?.to_string();
    if prefix.parent()?.file_name()?.to_str() != Some("compatdata") {
        return None;
    }
    for root in crate::steam::find_roots() {
        let Ok(t) = fs::read_to_string(root.path.join("config/config.vdf")) else { continue };
        let v = crate::vdf::parse(&t);
        let map = ["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping"].iter().try_fold(&v, |v, k| v.get(k));
        let Some(map) = map else { continue };
        for key in [appid.as_str(), "0"] {
            if let Some(name) = map.get(key).and_then(|e| e.str("name")).filter(|n| !n.is_empty()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// umu-run's environment for a program in a Battle.net prefix (the app, or a game through it).
/// None when `program` is not the app in a prefix.
pub fn umu_env(program: &Path) -> Option<Vec<(String, String)>> {
    if !is_client_exe(program) {
        return None;
    }
    Some(prefix_env(&prefix_of(program)?))
}

/// umu-run's environment for Battle.net's prefix `prefix`: the Proton the player picked for
/// Battle.net, else the one Steam uses for its shortcut, else Rare's default.
pub fn prefix_env(prefix: &Path) -> Vec<(String, String)> {
    let mut v = vec![
        ("WINEPREFIX".to_string(), prefix.to_string_lossy().into_owned()),
        ("GAMEID".to_string(), "umu-default".to_string()),
        ("STORE".to_string(), "none".to_string()),
    ];
    let tool = crate::umu::choice(APP_ID).and_then(|t| crate::umu::tool_path(&t)).or_else(|| steam_tool(prefix).and_then(|t| crate::umu::tool_path(&t))).or_else(|| crate::umu::default_tool().and_then(|t| crate::umu::tool_path(&t)));
    if let Some(p) = tool {
        v.push(("PROTONPATH".to_string(), p.to_string_lossy().into_owned()));
    }
    v
}

/// Blizzard's language code for a POSIX locale ("de_DE.UTF-8" is deDE), English otherwise.
pub fn blizzard_lang(posix: &str) -> &'static str {
    const LANGS: [&str; 14] = ["enUS", "enGB", "deDE", "frFR", "esES", "esMX", "itIT", "ptBR", "ruRU", "koKR", "zhTW", "zhCN", "plPL", "jaJP"];
    let code: String = posix.split(['.', '@']).next().unwrap_or("").replace('_', "");
    LANGS.iter().find(|l| l.eq_ignore_ascii_case(&code)).copied().unwrap_or("enUS")
}

/// The installer's arguments: the language and the usual folder, so the app lands where the scan
/// looks first.
pub fn setup_args(posix_lang: &str) -> Vec<String> {
    vec![format!("--lang={}", blizzard_lang(posix_lang)), "--installpath=C:\\Program Files (x86)\\Battle.net".to_string()]
}

/// The app itself as something the runner starts (Apps > Battle.net), under run id `id`.
pub fn client_game(id: &str, client: &Path) -> Game {
    Game {
        id: id.to_string(),
        source: Source::Apps,
        title: "Battle.net".into(),
        installed: true,
        install_dir: client.parent().map(|p| p.to_string_lossy().into_owned()),
        size_bytes: None,
        last_played: None,
        playtime_minutes: None,
        art: Art::default(),
        launch: LaunchSpec::Exec { program: client.to_string_lossy().into_owned(), args: vec![], cwd: client.parent().map(|p| p.to_string_lossy().into_owned()), windows: true },
        origin: "Battle.net".into(),
        foreign: false,
    }
}

/// True for World of Warcraft (any flavour).
pub fn is_wow(id: &str) -> bool {
    id.strip_prefix("battlenet:").is_some_and(|u| u == "wow" || u.starts_with("wow_"))
}

/// WoW's controller mode is off until the player turns it on once (the GamePadEnable setting in
/// WTF/Config.wtf). The line Rare shows as the game starts while it is off; None once it is on
/// or for any other game.
pub fn gamepad_hint(id: &str, install_dir: Option<&str>) -> Option<&'static str> {
    if !is_wow(id) {
        return None;
    }
    let cfg = install_dir.map(|d| Path::new(d).join("WTF/Config.wtf"));
    let on = cfg.and_then(|c| fs::read_to_string(c).ok()).is_some_and(|t| {
        t.lines().any(|l| {
            let l: Vec<&str> = l.split_whitespace().collect();
            l.len() >= 3 && l[0].eq_ignore_ascii_case("SET") && l[1].eq_ignore_ascii_case("GamePadEnable") && l[2].trim_matches('"') == "1"
        })
    });
    (!on).then_some(GAMEPAD_HINT)
}

pub const GAMEPAD_HINT: &str = "World of Warcraft has a controller mode. Type /console GamePadEnable 1 in the chat once, then /reload.";

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    fn ld(num: u64, body: &[u8], out: &mut Vec<u8>) {
        varint(num << 3 | 2, out);
        varint(body.len() as u64, out);
        out.extend(body);
    }

    /// product.db as Blizzard's agent writes it: installs, plus other fields to skip.
    fn product_db(items: &[(&str, &str, &str)]) -> Vec<u8> {
        product_db_states(&items.iter().map(|(u, c, p)| (*u, *c, *p, true)).collect::<Vec<_>>())
    }

    fn product_db_states(items: &[(&str, &str, &str, bool)]) -> Vec<u8> {
        let mut db = Vec::new();
        for (uid, code, path, installed) in items {
            let mut settings = Vec::new();
            ld(1, path.as_bytes(), &mut settings);
            ld(2, b"us", &mut settings);
            varint(5 << 3, &mut settings);
            varint(300, &mut settings);
            let mut inst = Vec::new();
            ld(1, uid.as_bytes(), &mut inst);
            ld(2, code.as_bytes(), &mut inst);
            ld(3, &settings, &mut inst);
            // cached_product_state { base_product_state { installed, playable } }, then a fixed64
            // and a fixed32 to skip.
            let mut base = Vec::new();
            varint(1 << 3, &mut base);
            varint(*installed as u64, &mut base);
            varint(2 << 3, &mut base);
            varint(*installed as u64, &mut base);
            let mut state = Vec::new();
            ld(1, &base, &mut state);
            ld(4, &state, &mut inst);
            inst.push(6 << 3 | 1);
            inst.extend([0u8; 8]);
            inst.push(7 << 3 | 5);
            inst.extend([0u8; 4]);
            ld(1, &inst, &mut db);
        }
        // active_installs and a trailing varint field.
        ld(2, b"\x0a\x03wow", &mut db);
        varint(3 << 3, &mut db);
        varint(1, &mut db);
        db
    }

    #[test]
    fn reads_product_db() {
        let b = product_db(&[("agent", "agent", "C:/ProgramData/Battle.net/Agent"), ("wow", "wow", "C:/Program Files (x86)/World of Warcraft"), ("prometheus", "pro", "D:\\Games\\Overwatch")]);
        let p = parse_product_db(&b);
        assert_eq!(p.len(), 3);
        assert_eq!(p[1], Product { uid: "wow".into(), code: "wow".into(), path: "C:/Program Files (x86)/World of Warcraft".into(), installed: true });
        assert_eq!(p[2].path, "D:\\Games\\Overwatch");
        // Cut short or garbage: what parsed, no panic.
        assert!(parse_product_db(&b[..b.len() / 2]).len() <= 2);
        assert!(parse_product_db(&[0xff, 0xff, 0xff]).is_empty());
        assert!(parse_product_db(&[]).is_empty());
    }

    /// RARE_BNET_PRODUCT_DB=/path/product.db cargo test -p rare-library real_product_db -- --ignored
    #[test]
    #[ignore]
    fn real_product_db() {
        let p = parse_product_db(&fs::read(std::env::var("RARE_BNET_PRODUCT_DB").unwrap()).unwrap());
        assert!(!p.is_empty());
        for x in &p {
            eprintln!("{x:?}");
            assert!(!x.uid.is_empty() && x.path.contains(':'), "{x:?}");
        }
    }

    #[test]
    fn reads_build_info() {
        let t = "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|CDN Key!HEX:16|Install Key!HEX:16|IM Size!DEC:4|CDN Path!STRING:0|CDN Hosts!STRING:0|CDN Servers!STRING:0|Tags!STRING:0|Armadillo!STRING:0|Last Activated!STRING:0|Version!STRING:0|KeyRing!HEX:16|Product!STRING:0\n\
us|1|a|b|c||tpr/wow|h|s|t|||11.2.5.1||wow\n\
us|1|a|b|c||tpr/wow|h|s|t|||4.4.2.1||wow_classic\n\
us|0|a|b|c||tpr/wow|h|s|t|||1.15.7||wow_classic_era\n";
        assert_eq!(build_info_products(t), vec!["wow", "wow_classic"]);
        assert!(build_info_products("").is_empty());
        assert!(build_info_products("Branch!STRING:0|Active!DEC:1\nus|1\n").is_empty());
    }

    #[test]
    fn maps_windows_paths_and_prefixes() {
        let pfx = Path::new("/h/.local/share/Steam/steamapps/compatdata/3141592653");
        let t = tempfile::tempdir().unwrap();
        // A Proton prefix keeps drive_c in pfx.
        let proton = t.path().join("compat/77");
        fs::create_dir_all(proton.join("pfx/drive_c")).unwrap();
        assert_eq!(win_path(&proton, "C:\\Program Files (x86)\\World of Warcraft").unwrap(), proton.join("pfx/drive_c/Program Files (x86)/World of Warcraft"));
        assert_eq!(win_path(&proton, "d:/Games/Overwatch").unwrap(), proton.join("pfx/dosdevices/d:/Games/Overwatch"));
        assert_eq!(win_path(&proton, "World of Warcraft"), None);
        assert_eq!(prefix_of(&pfx.join("pfx/drive_c/Program Files (x86)/Battle.net/Battle.net.exe")).as_deref(), Some(pfx));
        assert_eq!(prefix_of(Path::new("/h/Games/battlenet/drive_c/Program Files (x86)/Battle.net/Battle.net.exe")).as_deref(), Some(Path::new("/h/Games/battlenet")));
        assert_eq!(prefix_of(Path::new("/h/Games/wow/Wow.exe")), None);
        assert!(is_client_exe(Path::new("/h/Games/battlenet/drive_c/Program Files (x86)/Battle.net/Battle.net.exe")));
        assert!(!is_client_exe(Path::new("/h/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_retail_/Wow.exe")));
        assert!(umu_env(Path::new("/h/Games/x/Game.exe")).is_none());
        let env = umu_env(&pfx.join("pfx/drive_c/Program Files (x86)/Battle.net/Battle.net.exe")).unwrap();
        assert_eq!(env[0], ("WINEPREFIX".to_string(), pfx.to_string_lossy().into_owned()));
        assert!(env.iter().any(|(k, v)| k == "GAMEID" && v == "umu-default"));
    }

    #[test]
    fn installer_args_follow_the_locale() {
        assert_eq!(setup_args("de_DE.UTF-8"), vec!["--lang=deDE", "--installpath=C:\\Program Files (x86)\\Battle.net"]);
        assert_eq!(blizzard_lang("en_US.UTF-8"), "enUS");
        assert_eq!(blizzard_lang("pt_BR"), "ptBR");
        assert_eq!(blizzard_lang("nl_NL.UTF-8"), "enUS");
        assert_eq!(blizzard_lang(""), "enUS");
    }

    #[test]
    fn run_needles_follow_wine() {
        let p = Path::new("/home/rare/.local/share/rare/prefixes/battlenet/drive_c/Program Files (x86)/World of Warcraft/_retail_");
        assert_eq!(run_needle(p).as_deref(), Some(":/program files (x86)/world of warcraft/_retail_/"));
        let p = Path::new("/home/rare/Games/bnet/dosdevices/d:/Games/Diablo III");
        assert_eq!(run_needle(p).as_deref(), Some(":/games/diablo iii/"));
        assert_eq!(run_needle(Path::new("/games/Hades")), None);
        assert_eq!(run_needle(Path::new("/pfx/drive_c")), None);
    }

    #[test]
    fn builds_launch_args() {
        assert_eq!(launch_args(known("wow").and_then(|k| k.launch)), vec!["--exec=launch WoW"]);
        assert_eq!(launch_args(known("hsb").and_then(|k| k.launch)), vec!["--exec=launch WTCG"]);
        assert_eq!(launch_args(known("prometheus").and_then(|k| k.launch)), vec!["--exec=launch Pro"]);
        assert!(launch_args(known("wow_classic_era").and_then(|k| k.launch)).is_empty());
        assert!(launch_args(None).is_empty());
        // Every known entry is unique by uid and by code.
        for (i, a) in KNOWN.iter().enumerate() {
            for b in &KNOWN[i + 1..] {
                assert!(a.uid != b.uid && a.code != b.code, "{} / {}", a.uid, b.uid);
            }
        }
    }

    /// A prefix with the app, WoW (retail and Classic in one folder) from product.db, and
    /// Hearthstone found only by its .build.info.
    fn fixture(root: &Path, proton: bool) -> PathBuf {
        let pfx = root.to_path_buf();
        let drive = if proton { pfx.join("pfx") } else { pfx.clone() };
        let c = drive.join("drive_c");
        fs::create_dir_all(c.join("Program Files (x86)/Battle.net")).unwrap();
        fs::write(c.join("Program Files (x86)/Battle.net/Battle.net.exe"), b"MZ").unwrap();
        fs::create_dir_all(c.join("Program Files (x86)/World of Warcraft/_retail_/WTF")).unwrap();
        fs::create_dir_all(c.join("Program Files (x86)/World of Warcraft/_classic_")).unwrap();
        fs::create_dir_all(c.join("Program Files (x86)/Hearthstone")).unwrap();
        fs::write(c.join("Program Files (x86)/Hearthstone/.build.info"), "Branch!STRING:0|Active!DEC:1|Product!STRING:0\nus|1|hsb\n").unwrap();
        fs::create_dir_all(c.join("ProgramData/Battle.net/Agent")).unwrap();
        let db = product_db(&[
            ("agent", "agent", "C:/ProgramData/Battle.net/Agent"),
            ("battle.net", "bna", "C:/Program Files (x86)/Battle.net"),
            ("wow", "wow", "C:/Program Files (x86)/World of Warcraft"),
            ("wow_classic", "wow_classic", "C:/Program Files (x86)/World of Warcraft"),
            // Uninstalled since: its folder is gone.
            ("fenris", "fen", "C:/Program Files (x86)/Diablo IV"),
        ]);
        fs::write(c.join("ProgramData/Battle.net/Agent/product.db"), db).unwrap();
        pfx
    }

    #[test]
    fn finds_games_in_wine_and_proton_prefixes() {
        let t = tempfile::tempdir().unwrap();
        let wine = fixture(&t.path().join("Games/battlenet"), false);
        let proton = fixture(&t.path().join("steamapps/compatdata/4000000001"), true);
        let empty = t.path().join("Games/other");
        fs::create_dir_all(empty.join("drive_c")).unwrap();
        let s = scan_in(&[empty.clone(), wine.clone(), proton.clone()]);
        assert_eq!(s.client.as_deref(), Some(wine.join("drive_c/Program Files (x86)/Battle.net/Battle.net.exe").as_path()));
        let ids: Vec<&str> = s.games.iter().map(|g| g.id.as_str()).collect();
        // One of each, from the first prefix that has it; no app, no uninstalled game.
        assert_eq!(ids, vec!["battlenet:wow", "battlenet:wow_classic", "battlenet:hs_beta"]);
        let wow = &s.games[0];
        assert_eq!((wow.title.as_str(), wow.source, wow.installed), ("World of Warcraft", Source::Battlenet, true));
        assert_eq!(wow.install_dir.as_deref(), Some(wine.join("drive_c/Program Files (x86)/World of Warcraft/_retail_").to_string_lossy().as_ref()));
        let LaunchSpec::Exec { program, args, windows, .. } = &wow.launch else { panic!("{:?}", wow.launch) };
        assert!(*windows && program.ends_with("Battle.net/Battle.net.exe"));
        assert_eq!(args, &vec!["--exec=launch WoW".to_string()]);
        assert_eq!(s.games[1].install_dir.as_deref().map(|d| d.ends_with("_classic_")), Some(true));
        assert_eq!(s.games[2].title, "Hearthstone");
        // The Proton prefix alone: drive_c under pfx, WINEPREFIX the compatdata folder.
        let p = scan_in(&[proton.clone()]);
        assert_eq!(p.games.len(), 3);
        let LaunchSpec::Exec { program, .. } = &p.games[0].launch else { panic!() };
        assert_eq!(prefix_of(Path::new(program)).as_deref(), Some(proton.as_path()));
        assert!(scan_in(&[empty]).client.is_none());
    }

    #[test]
    fn a_game_still_downloading_is_not_listed() {
        let t = tempfile::tempdir().unwrap();
        let pfx = fixture(t.path(), false);
        let c = pfx.join("drive_c");
        fs::create_dir_all(c.join("Program Files (x86)/Diablo IV")).unwrap();
        fs::write(c.join("Program Files (x86)/Diablo IV/.build.info"), "Active!DEC:1|Product!STRING:0\n1|fen\n").unwrap();
        let db = product_db_states(&[("wow", "wow", "C:/Program Files (x86)/World of Warcraft", true), ("fenris", "fen", "C:/Program Files (x86)/Diablo IV", false)]);
        fs::write(c.join("ProgramData/Battle.net/Agent/product.db"), &db).unwrap();
        assert!(!parse_product_db(&db)[1].installed);
        let ids: Vec<String> = scan_in(&[pfx.clone()]).games.into_iter().map(|g| g.id).collect();
        assert!(ids.contains(&"battlenet:wow".to_string()) && !ids.contains(&"battlenet:fenris".to_string()), "{ids:?}");
        // Done: it shows.
        let db = product_db_states(&[("wow", "wow", "C:/Program Files (x86)/World of Warcraft", true), ("fenris", "fen", "C:/Program Files (x86)/Diablo IV", true)]);
        fs::write(c.join("ProgramData/Battle.net/Agent/product.db"), &db).unwrap();
        assert!(scan_in(&[pfx]).games.iter().any(|g| g.id == "battlenet:fenris"));
    }

    #[test]
    fn steam_copies_get_steam_art_and_a_key() {
        let t = tempfile::tempdir().unwrap();
        let pfx = fixture(t.path(), false);
        fs::create_dir_all(pfx.join("drive_c/Program Files (x86)/Overwatch")).unwrap();
        fs::write(pfx.join("drive_c/Program Files (x86)/Overwatch/.build.info"), "Active!DEC:1|Product!STRING:0\n1|pro\n").unwrap();
        let s = scan_in(&[pfx]);
        let ow = s.games.iter().find(|g| g.id == "battlenet:prometheus").unwrap();
        assert_eq!(ow.title, "Overwatch 2");
        assert!(ow.art.cover[0].contains("/2357570/"));
        assert!(s.keys.contains(&("battlenet:prometheus".to_string(), "steam:2357570".to_string())));
    }

    #[test]
    fn wow_gamepad_hint_until_it_is_on() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("_retail_");
        fs::create_dir_all(dir.join("WTF")).unwrap();
        let d = dir.to_string_lossy().into_owned();
        assert_eq!(gamepad_hint("battlenet:wow", Some(&d)), Some(GAMEPAD_HINT));
        fs::write(dir.join("WTF/Config.wtf"), "SET locale \"enUS\"\nSET GamePadEnable \"0\"\n").unwrap();
        assert!(gamepad_hint("battlenet:wow", Some(&d)).is_some());
        fs::write(dir.join("WTF/Config.wtf"), "SET locale \"enUS\"\r\nSET GamePadEnable \"1\"\r\n").unwrap();
        assert_eq!(gamepad_hint("battlenet:wow", Some(&d)), None);
        assert!(gamepad_hint("battlenet:wow_classic", None).is_some());
        assert_eq!(gamepad_hint("battlenet:prometheus", None), None);
        assert_eq!(gamepad_hint("steam:1", None), None);
    }
}
