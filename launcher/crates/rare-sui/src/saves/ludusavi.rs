//! Where PC games keep their saves, from the Ludusavi manifest
//! (github.com/mtkennerly/ludusavi-manifest, `data/manifest.yaml`, about 18 MB). The manifest is
//! read once into a small [`Index`] (games with save files only, by normalized title and install
//! folder name), cached on the device, and paths are resolved against a game's install folder and
//! its Wine prefix.
//!
//! The manifest is machine written with a fixed layout, so a line reader is enough (no YAML crate,
//! no 300 MB parse tree on the handheld).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const MANIFEST_URL: &str = "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";

/// One save path rule: a path with Ludusavi placeholders (`<winAppData>/Game`) and the systems it
/// applies to (empty: any).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub title: String,
    pub files: Vec<Rule>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Index {
    /// normalized title -> entry
    pub games: HashMap<String, Entry>,
    /// normalized install folder name -> normalized title
    pub dirs: HashMap<String, String>,
}

/// Lowercase letters and digits, everything else one space: "DARK SOULS™ III" -> "dark souls iii".
pub fn normalize(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut space = false;
    for c in title.chars() {
        if c == '\'' || c == '\u{2019}' || c == '\u{2122}' || c == '\u{ae}' || c == '\u{a9}' {
            continue;
        }
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        } else {
            space = true;
        }
    }
    out
}

/// A YAML scalar at the start of `s` (double quoted, single quoted or plain, up to `: ` or a
/// trailing `:`), and the rest after it.
fn scalar(s: &str) -> (String, &str) {
    let b = s.as_bytes();
    if b.first() == Some(&b'"') {
        let mut out = String::new();
        let mut it = s[1..].char_indices();
        while let Some((i, c)) = it.next() {
            match c {
                '"' => return (out, &s[i + 2..]),
                '\\' => match it.next().map(|x| x.1) {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        let hex: String = (0..4).filter_map(|_| it.next().map(|x| x.1)).collect();
                        if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            out.push(ch);
                        }
                    }
                    Some(o) => out.push(o),
                    None => {}
                },
                c => out.push(c),
            }
        }
        return (out, "");
    }
    if b.first() == Some(&b'\'') {
        let mut out = String::new();
        let mut i = 1;
        while i < s.len() {
            let c = s[i..].chars().next().unwrap();
            if c == '\'' {
                if s[i + 1..].starts_with('\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                return (out, &s[i + 1..]);
            }
            out.push(c);
            i += c.len_utf8();
        }
        return (out, "");
    }
    match s.find(": ") {
        Some(i) => (s[..i].to_string(), &s[i..]),
        None => {
            let t = s.trim_end();
            match t.strip_suffix(':') {
                Some(k) => (k.to_string(), ":"),
                None => (t.to_string(), ""),
            }
        }
    }
}

/// A mapping key line's key ("<base>/x": {} or <base>/x:).
fn key(s: &str) -> Option<String> {
    let (k, rest) = scalar(s);
    rest.trim_start().starts_with(':').then_some(k)
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

#[derive(PartialEq)]
enum Sec {
    None,
    Files,
    Dirs,
}

#[derive(PartialEq)]
enum Sub {
    None,
    Tags,
    When,
}

struct Pending {
    path: String,
    tags: Vec<String>,
    /// one entry per `when` item: its os, if it named one
    when: Vec<Option<String>>,
}

impl Pending {
    fn rule(self) -> Option<Rule> {
        if !self.tags.is_empty() && !self.tags.iter().any(|t| t == "save") {
            return None;
        }
        let os = if self.when.is_empty() || self.when.iter().any(Option::is_none) {
            Vec::new()
        } else {
            let mut v: Vec<String> = self.when.into_iter().flatten().collect();
            v.sort();
            v.dedup();
            v
        };
        Some(Rule { path: self.path, os })
    }
}

/// Reads the manifest into the index: save rules only, aliases followed.
pub fn parse(manifest: &str) -> Index {
    let mut idx = Index::default();
    let mut aliases: Vec<(String, String)> = Vec::new();
    let mut title: Option<String> = None;
    let mut files: Vec<Rule> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    let mut sec = Sec::None;
    let mut sub = Sub::None;
    let mut cur: Option<Pending> = None;

    let finish = |title: Option<String>, files: &mut Vec<Rule>, dirs: &mut Vec<String>, cur: &mut Option<Pending>, idx: &mut Index| {
        if let Some(p) = cur.take().and_then(Pending::rule) {
            files.push(p);
        }
        let Some(t) = title else {
            files.clear();
            dirs.clear();
            return;
        };
        if files.is_empty() {
            dirs.clear();
            return;
        }
        let n = normalize(&t);
        for d in dirs.drain(..) {
            let dn = normalize(&d);
            if !dn.is_empty() {
                idx.dirs.entry(dn).or_insert_with(|| n.clone());
            }
        }
        idx.games.entry(n).or_insert(Entry { title: t, files: std::mem::take(files) });
        files.clear();
    };

    for line in manifest.lines() {
        if line.is_empty() || line.starts_with("---") || line.trim_start().starts_with('#') {
            continue;
        }
        let ind = indent(line);
        let body = &line[ind..];
        match ind {
            0 => {
                finish(title.take(), &mut files, &mut dirs, &mut cur, &mut idx);
                title = key(body);
                sec = Sec::None;
                sub = Sub::None;
            }
            2 => {
                if let Some(p) = cur.take().and_then(Pending::rule) {
                    files.push(p);
                }
                sub = Sub::None;
                sec = match key(body).as_deref() {
                    Some("files") => Sec::Files,
                    Some("installDir") => Sec::Dirs,
                    _ => Sec::None,
                };
                if let Some(rest) = body.strip_prefix("alias:") {
                    let (target, _) = scalar(rest.trim());
                    if let Some(t) = &title {
                        aliases.push((t.clone(), target));
                    }
                }
            }
            4 => match sec {
                Sec::Files => {
                    if let Some(p) = cur.take().and_then(Pending::rule) {
                        files.push(p);
                    }
                    sub = Sub::None;
                    cur = key(body).map(|path| Pending { path, tags: vec![], when: vec![] });
                }
                Sec::Dirs => {
                    if let Some(d) = key(body) {
                        dirs.push(d);
                    }
                }
                Sec::None => {}
            },
            6 if sec == Sec::Files => {
                sub = match key(body).as_deref() {
                    Some("tags") => Sub::Tags,
                    Some("when") => Sub::When,
                    _ => Sub::None,
                };
            }
            8 | 10 if sec == Sec::Files => {
                let Some(p) = cur.as_mut() else { continue };
                let item = body.strip_prefix("- ");
                match sub {
                    Sub::Tags => {
                        if let Some(t) = item {
                            p.tags.push(scalar(t).0);
                        }
                    }
                    Sub::When => {
                        let kv = match item {
                            Some(kv) => {
                                p.when.push(None);
                                kv
                            }
                            None => body,
                        };
                        if let Some(v) = kv.strip_prefix("os:") {
                            if let Some(last) = p.when.last_mut() {
                                *last = Some(scalar(v.trim()).0);
                            }
                        }
                    }
                    Sub::None => {}
                }
            }
            _ => {}
        }
    }
    finish(title.take(), &mut files, &mut dirs, &mut cur, &mut idx);
    for (alias, target) in aliases {
        let (a, t) = (normalize(&alias), normalize(&target));
        if let Some(e) = idx.games.get(&t).cloned() {
            idx.games.entry(a).or_insert(e);
        }
    }
    idx
}

impl Index {
    /// The entry for a game: by title, then by its install folder's name.
    pub fn find(&self, title: &str, install_dir: Option<&Path>) -> Option<&Entry> {
        let n = normalize(title);
        if let Some(e) = self.games.get(&n) {
            return Some(e);
        }
        let dir = install_dir.and_then(|d| d.file_name()).map(|f| normalize(&f.to_string_lossy()))?;
        self.games.get(&dir).or_else(|| self.dirs.get(&dir).and_then(|t| self.games.get(t)))
    }
}

/// What a rule's placeholders mean for one game on this device.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub home: PathBuf,
    /// The Wine prefix of a Windows game (umu-run's WINEPREFIX).
    pub prefix: Option<PathBuf>,
    /// The game's install folder (`<base>`).
    pub base: Option<PathBuf>,
    pub windows: bool,
    /// The Linux user name (`<osUserName>` for native games).
    pub user: String,
}

/// One place a game keeps saves: `base` is a real folder, `rest` the pattern under it ("Game",
/// "*.sav", "*/save.dat"). `key` names it in archives the same on every device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub key: String,
    pub base: PathBuf,
    pub rest: Vec<String>,
}

fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// The rule's path on this device, or None when a placeholder has no meaning here.
pub fn resolve(rule: &Rule, cx: &Ctx) -> Option<PathBuf> {
    let os_ok = rule.os.is_empty() || rule.os.iter().any(|o| o == if cx.windows { "windows" } else { "linux" });
    if !os_ok {
        return None;
    }
    let mut p = rule.path.replace('\\', "/");
    if p.contains("<root>/<game>") {
        p = p.replace("<root>/<game>", "<base>");
    }
    let wuser = cx.prefix.as_ref().map(|x| x.join("drive_c/users/steamuser"));
    let mut out = String::new();
    let mut rest = p.as_str();
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let j = rest[i..].find('>')? + i;
        let name = &rest[i + 1..j];
        let path = |x: Option<PathBuf>| x.map(|x| x.to_string_lossy().into_owned());
        let v: String = match name {
            "base" => path(cx.base.clone())?,
            "home" if cx.windows => path(wuser.clone())?,
            "home" => path(Some(cx.home.clone()))?,
            "storeUserId" => "*".into(),
            "osUserName" if cx.windows => "steamuser".into(),
            "osUserName" => cx.user.clone(),
            "winAppData" if cx.windows => path(wuser.clone().map(|u| u.join("AppData/Roaming")))?,
            "winLocalAppData" if cx.windows => path(wuser.clone().map(|u| u.join("AppData/Local")))?,
            "winLocalAppDataLow" if cx.windows => path(wuser.clone().map(|u| u.join("AppData/LocalLow")))?,
            "winDocuments" if cx.windows => path(wuser.clone().map(|u| u.join("Documents")))?,
            "winPublic" if cx.windows => path(cx.prefix.clone().map(|x| x.join("drive_c/users/Public")))?,
            "winProgramData" if cx.windows => path(cx.prefix.clone().map(|x| x.join("drive_c/ProgramData")))?,
            "winDir" if cx.windows => path(cx.prefix.clone().map(|x| x.join("drive_c/windows")))?,
            "xdgData" if !cx.windows => path(Some(cx.home.join(".local/share")))?,
            "xdgConfig" if !cx.windows => path(Some(cx.home.join(".config")))?,
            _ => return None,
        };
        out.push_str(&v);
        rest = &rest[j + 1..];
    }
    out.push_str(rest);
    let pb = PathBuf::from(out.trim_end_matches('/'));
    // Only absolute paths inside the home folder or the game's own folder.
    let inside = pb.is_absolute() && (pb.starts_with(&cx.home) || cx.base.as_ref().is_some_and(|b| pb.starts_with(b)) || cx.prefix.as_ref().is_some_and(|x| pb.starts_with(x)));
    let clean = pb.components().all(|c| !matches!(c, std::path::Component::ParentDir));
    (inside && clean).then_some(pb)
}

/// The rules of `entry` as roots on this device (rules that do not apply here are left out).
pub fn roots(entry: &Entry, cx: &Ctx) -> Vec<Root> {
    use sha2::{Digest, Sha256};
    let mut out: Vec<Root> = Vec::new();
    // Folders too broad to be one game's saves (a rule of just `<home>` or `<base>`).
    let wuser = cx.prefix.as_ref().map(|x| x.join("drive_c/users/steamuser"));
    let mut broad: Vec<PathBuf> = vec![cx.home.clone(), cx.home.join(".local/share"), cx.home.join(".config")];
    broad.extend(cx.base.clone());
    if let Some(x) = &cx.prefix {
        broad.extend([x.clone(), x.join("drive_c"), x.join("drive_c/users"), x.join("drive_c/users/Public"), x.join("drive_c/ProgramData"), x.join("drive_c/windows")]);
    }
    if let Some(u) = &wuser {
        broad.extend(["", "AppData", "AppData/Roaming", "AppData/Local", "AppData/LocalLow", "Documents"].iter().map(|s| if s.is_empty() { u.clone() } else { u.join(s) }));
    }
    for r in &entry.files {
        let Some(p) = resolve(r, cx) else { continue };
        if broad.contains(&p) {
            continue;
        }
        let comps: Vec<String> = p.components().filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        }).collect();
        let first_glob = comps.iter().position(|c| has_glob(c)).unwrap_or(comps.len().saturating_sub(1));
        if first_glob == 0 {
            continue;
        }
        let base: PathBuf = std::iter::once("/".to_string()).chain(comps[..first_glob].iter().cloned()).collect();
        let rest = comps[first_glob..].to_vec();
        let any = rest.iter().all(|c| c == "*" || c == "**");
        if base.parent().is_none() || rest.is_empty() || (any && broad.contains(&base)) {
            continue;
        }
        let digest = Sha256::digest(r.path.as_bytes());
        let key: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
        if !out.iter().any(|o| o.key == key) {
            out.push(Root { key, base, rest });
        }
    }
    out
}

/// One path component against one glob component (`*`, `?`, `[abc]`).
pub fn glob_one(pat: &str, s: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|i| go(&p[1..], &s[i..])),
            Some('?') => !s.is_empty() && go(&p[1..], &s[1..]),
            Some('[') => {
                let Some(end) = p.iter().position(|c| *c == ']') else { return !s.is_empty() && s[0] == '[' && go(&p[1..], &s[1..]) };
                let set = &p[1..end];
                !s.is_empty() && set.contains(&s[0]) && go(&p[end + 1..], &s[1..])
            }
            Some(c) => !s.is_empty() && s[0].eq_ignore_ascii_case(c) && go(&p[1..], &s[1..]),
        }
    }
    let (p, s): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    go(&p, &s)
}

/// Whether a file at `rel` (components under the root's base) belongs to the root: the pattern
/// matches a prefix of it (a matched folder takes everything inside).
pub fn matches(rest: &[String], rel: &[String]) -> bool {
    match rest.first() {
        None => true,
        Some(p) if p == "**" => (0..=rel.len()).any(|i| matches(&rest[1..], &rel[i..])),
        Some(p) => !rel.is_empty() && glob_one(p, &rel[0]) && matches(&rest[1..], &rel[1..]),
    }
}

/// Whether a folder at `rel` may hold matching files (walks skip the others).
pub fn may_hold(rest: &[String], rel: &[String]) -> bool {
    match (rest.first(), rel.first()) {
        (None, _) | (_, None) => true,
        (Some(p), Some(_)) if p == "**" => true,
        (Some(p), Some(r)) => glob_one(p, r) && may_hold(&rest[1..], &rel[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"---
"! That Bastard Is Trying To Steal Our Gold !":
  installDir:
    "! That Bastard Is Trying To Steal Our Gold !": {}
  registry:
    HKEY_CURRENT_USER/SOFTWARE/X:
      tags:
        - save
  steam:
    id: 449940
"!Anyway!":
  cloud:
    steam: true
  files:
    "<winLocalAppData>/anyway":
      tags:
        - save
      when:
        - os: windows
    <base>/config.ini:
      tags:
        - config
    "<xdgData>/anyway/saves/*.sav":
      when:
        - os: linux
          store: steam
  installDir:
    Anyway: {}
  steam:
    id: 866510
"Hollow Knight":
  files:
    "<winLocalAppDataLow>/Team Cherry/Hollow Knight":
      tags:
        - save
      when:
        - os: windows
        - os: windows
          store: steam
    <home>/.config/unity3d/Team Cherry/Hollow Knight:
      tags:
        - save
      when:
        - os: linux
'It''s Quoted':
  files:
    <base>/save/<storeUserId>/*.dat: {}
Old Name:
  alias: Hollow Knight
"#;

    #[test]
    fn reads_the_manifest_layout() {
        let idx = parse(SAMPLE);
        assert!(!idx.games.contains_key(&normalize("! That Bastard Is Trying To Steal Our Gold !")), "registry only");
        let a = &idx.games["anyway"];
        assert_eq!(a.title, "!Anyway!");
        assert_eq!(a.files, vec![Rule { path: "<winLocalAppData>/anyway".into(), os: vec!["windows".into()] }, Rule { path: "<xdgData>/anyway/saves/*.sav".into(), os: vec!["linux".into()] }]);
        assert_eq!(idx.dirs["anyway"], "anyway");
        let hk = &idx.games["hollow knight"];
        assert_eq!(hk.files.len(), 2);
        assert_eq!(hk.files[0].os, vec!["windows".to_string()]);
        assert_eq!(idx.games["its quoted"].files[0].path, "<base>/save/<storeUserId>/*.dat");
        assert_eq!(idx.games["old name"].title, "Hollow Knight");
        assert!(idx.find("HOLLOW KNIGHT\u{2122}", None).is_some());
        assert!(idx.find("Something", Some(Path::new("/games/Anyway"))).is_some());
    }

    /// LUDUSAVI_MANIFEST=/path/manifest.yaml cargo test real_manifest -- --ignored
    #[test]
    #[ignore]
    fn real_manifest() {
        let text = std::fs::read_to_string(std::env::var("LUDUSAVI_MANIFEST").unwrap()).unwrap();
        let t = std::time::Instant::now();
        let idx = parse(&text);
        let json = serde_json::to_vec(&idx).unwrap();
        eprintln!("{} games, {} dirs, {} KB index, {:?}", idx.games.len(), idx.dirs.len(), json.len() / 1024, t.elapsed());
        let hk = idx.find("Hollow Knight", None).unwrap();
        eprintln!("{hk:?}");
        assert!(idx.games.len() > 15_000);
        assert!(hk.files.iter().any(|r| r.path.contains("Team Cherry")));
    }

    #[test]
    fn normalizes_titles() {
        assert_eq!(normalize("DARK SOULS\u{2122} III"), "dark souls iii");
        assert_eq!(normalize("Baldur's Gate 3"), "baldurs gate 3");
        assert_eq!(normalize("  Hades II: "), "hades ii");
    }

    fn cx(windows: bool) -> Ctx {
        Ctx { home: "/home/deck".into(), prefix: Some("/home/deck/.local/share/rare/prefixes/local-x".into()), base: Some("/home/deck/Games/X".into()), windows, user: "deck".into() }
    }

    #[test]
    fn resolves_placeholders() {
        let r = |p: &str, os: &[&str]| Rule { path: p.into(), os: os.iter().map(|s| s.to_string()).collect() };
        assert_eq!(resolve(&r("<winAppData>/Studio/Game", &[]), &cx(true)).unwrap(), PathBuf::from("/home/deck/.local/share/rare/prefixes/local-x/drive_c/users/steamuser/AppData/Roaming/Studio/Game"));
        assert_eq!(resolve(&r("<winAppData>/Studio/Game", &[]), &cx(false)), None);
        assert_eq!(resolve(&r("<xdgData>/g", &["linux"]), &cx(false)).unwrap(), PathBuf::from("/home/deck/.local/share/g"));
        assert_eq!(resolve(&r("<xdgData>/g", &["linux"]), &cx(true)), None);
        assert_eq!(resolve(&r("<root>/<game>/saves", &[]), &cx(true)).unwrap(), PathBuf::from("/home/deck/Games/X/saves"));
        assert_eq!(resolve(&r("<base>/../../etc", &[]), &cx(true)), None);
        assert_eq!(resolve(&r("<regHkcu>/x", &[]), &cx(true)), None);
        assert_eq!(resolve(&r("/etc/passwd", &[]), &cx(true)), None);
    }

    #[test]
    fn roots_and_matching() {
        let e = Entry { title: "X".into(), files: vec![Rule { path: "<base>/save/<storeUserId>/*.dat".into(), os: vec![] }, Rule { path: "<winDocuments>/My Games/X".into(), os: vec![] }, Rule { path: "<home>".into(), os: vec![] }] };
        let rs = roots(&e, &cx(true));
        assert_eq!(rs.len(), 2, "{rs:?}");
        assert_eq!(rs[0].base, PathBuf::from("/home/deck/Games/X/save"));
        assert_eq!(rs[0].rest, vec!["*".to_string(), "*.dat".to_string()]);
        assert_eq!(rs[1].base, PathBuf::from("/home/deck/.local/share/rare/prefixes/local-x/drive_c/users/steamuser/Documents/My Games"));
        assert_eq!(rs[1].rest, vec!["X".to_string()]);
        let v = |s: &str| s.split('/').map(str::to_string).collect::<Vec<_>>();
        assert!(matches(&rs[0].rest, &v("123/slot1.dat")));
        assert!(!matches(&rs[0].rest, &v("123/slot1.bak")));
        assert!(!matches(&rs[0].rest, &v("slot1.dat")));
        assert!(matches(&rs[1].rest, &v("X/profiles/a.sav")));
        assert!(!matches(&rs[1].rest, &v("Y/a.sav")));
        assert!(may_hold(&rs[0].rest, &v("123")));
        assert!(!may_hold(&rs[1].rest, &v("Other")));
        assert!(matches(&v("**/*.sav"), &v("a/b/c.sav")));
        assert!(glob_one("slot[12].sav", "slot2.sav") && !glob_one("slot[12].sav", "slot3.sav"));
    }
}
