//! Free homebrew from Homebrew Hub (hh3.gbdev.io, run by the gbdev community): new games for the
//! Game Boy, Game Boy Color, Game Boy Advance and NES that their authors put up for anyone to play.
//! The hub's API lists them by system; each entry's playable file is served from the hub itself.
//! Rare fetches a game only when the player picks it, into that system's ROM folder.

use crate::add;
use crate::systems::{by_id, System};
use serde_json::Value;
use std::path::PathBuf;

const API: &str = "https://hh3.gbdev.io/api/search";
const FILES: &str = "https://hh3.gbdev.io/static";

/// The hub's systems, as (its platform name, Rare's system id).
pub const PLATFORMS: [(&str, &str); 4] = [("GBA", "gba"), ("GBC", "gbc"), ("GB", "gb"), ("NES", "nes")];

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub slug: String,
    pub title: String,
    pub developer: String,
    pub system: &'static str,
    /// A free demo of a game sold elsewhere.
    pub demo: bool,
    pub year: String,
    /// The playable file's name and where the hub serves it.
    pub file: String,
    pub url: String,
    /// The first screenshot, when there is one.
    pub shot: Option<String>,
}

/// One page of a system's games, newest first: (games, pages in all).
pub fn list(platform: &str, page: u32) -> Result<(Vec<Game>, u32), String> {
    let url = format!("{API}?platform={platform}&typetag=game&page={page}");
    let v: Value = ureq::get(&url).set("User-Agent", "rare-launcher").timeout(std::time::Duration::from_secs(20)).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    Ok(parse(&v, platform))
}

/// The games in an API answer, and its page count.
pub fn parse(v: &Value, platform: &str) -> (Vec<Game>, u32) {
    let system = PLATFORMS.iter().find(|(p, _)| *p == platform).map(|(_, s)| *s).unwrap_or("gb");
    let pages = v["page_total"].as_u64().unwrap_or(1) as u32;
    let games = v["entries"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| game(e, system)).collect())
        .unwrap_or_default();
    (games, pages)
}

fn game(e: &Value, system: &'static str) -> Option<Game> {
    if e["typetag"].as_str().is_some_and(|t| t != "game") {
        return None;
    }
    let slug = e["slug"].as_str()?.to_string();
    let base = e["basepath"].as_str()?;
    let files = e["files"].as_array()?;
    let playable = |f: &&Value| f["playable"].as_bool().unwrap_or(false);
    let f = files.iter().filter(playable).find(|f| f["default"].as_bool().unwrap_or(false)).or_else(|| files.iter().find(playable))?;
    // The hub's path may have folders in it ("files/game.gb"); the device keeps the name alone.
    let rel = f["filename"].as_str()?;
    if rel.split('/').any(|p| p.is_empty() || p == "." || p == "..") {
        return None;
    }
    let file = add::safe_name(rel)?;
    // Only a file the system's folder takes (a .gb for the Game Boy, not a .zip of sources).
    let sys = by_id(system)?;
    if !add::systems_for(&file).iter().any(|s| s.id == sys.id) {
        return None;
    }
    let developer = match &e["developer"] {
        Value::Object(d) => d.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
        Value::Array(a) => a.iter().filter_map(|d| d["name"].as_str().or(d.as_str())).collect::<Vec<_>>().join(", "),
        Value::String(s) => s.clone(),
        _ => String::new(),
    };
    let shot = e["screenshots"].as_array().and_then(|s| s.first()).and_then(|s| s.as_str()).map(|s| format!("{FILES}/{base}/entries/{slug}/{s}"));
    Some(Game {
        url: format!("{FILES}/{base}/entries/{slug}/{}", rel.split('/').map(path_part).collect::<Vec<_>>().join("/")),
        title: e["title"].as_str().unwrap_or(&slug).to_string(),
        developer,
        system: sys.id,
        demo: e["commercial"].as_bool().unwrap_or(false),
        year: e["date"].as_str().unwrap_or_default().chars().take(4).collect(),
        file,
        shot,
        slug,
    })
}

/// A path part for a URL: letters, digits and `-._~` as they are, the rest %-escaped.
fn path_part(p: &str) -> String {
    p.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Where a game would go, and whether it is there already (by its file name).
pub fn target(g: &Game) -> Option<(PathBuf, bool)> {
    let sys: &System = by_id(g.system)?;
    let dir = add::folder_in(&add::roms_dir(), sys);
    let p = dir.join(&g.file);
    let have = p.exists();
    Some((p, have))
}

/// Fetches a game into its system's folder, through a `.part` file. `progress(done, total)`.
pub fn fetch(g: &Game, progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let (dst, have) = target(g).ok_or("Unknown system")?;
    if have {
        return Ok(dst);
    }
    std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
    let part = dst.with_file_name(format!("{}.part", g.file));
    let r = crate::install::download(&g.url, &part, progress).and_then(|_| {
        // A ROM is never empty; an error page is not a game.
        let len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if len < 256 {
            return Err("The download was not a game file".into());
        }
        std::fs::rename(&part, &dst).map_err(|e| e.to_string())
    });
    if r.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    r.map(|_| dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_hub_s_entries() {
        // As the API answered (2026-11-17), trimmed.
        let v: Value = serde_json::from_str(r#"{"results": 254, "page_total": 26, "entries": [
            {"slug": "dicebits", "title": "DICEBITS", "developer": {"name": "Sim Graveline"}, "platform": "GBA", "typetag": "game", "date": "2026-10",
             "commercial": true, "files": [{"filename": "dicebits_gba_v132_demo.gba", "playable": true, "default": true}],
             "screenshots": ["title.png", "gameplay.png"], "basepath": "database-gba"},
            {"slug": "tool", "title": "A tool", "typetag": "tool", "files": [{"filename": "t.gba", "playable": true}], "basepath": "database-gba"},
            {"slug": "src", "title": "Sources only", "typetag": "game", "files": [{"filename": "src.zip", "playable": false}], "basepath": "database-gba"},
            {"slug": "two", "title": "Two", "developer": [{"name": "A"}, {"name": "B"}], "typetag": "game",
             "files": [{"filename": "manual.pdf", "playable": false}, {"filename": "files/two v1.gba", "playable": true}], "basepath": "database-gba"},
            {"slug": "bad", "title": "Bad", "typetag": "game", "files": [{"filename": "../x.gba", "playable": true}], "basepath": "database-gba"}
        ]}"#).unwrap();
        let (games, pages) = parse(&v, "GBA");
        assert_eq!(pages, 26);
        assert_eq!(games.len(), 2);
        let d = &games[0];
        assert_eq!((d.title.as_str(), d.developer.as_str(), d.system, d.demo, d.year.as_str()), ("DICEBITS", "Sim Graveline", "gba", true, "2026"));
        assert_eq!(d.url, "https://hh3.gbdev.io/static/database-gba/entries/dicebits/dicebits_gba_v132_demo.gba");
        assert_eq!(d.shot.as_deref(), Some("https://hh3.gbdev.io/static/database-gba/entries/dicebits/title.png"));
        // A file in a folder of the entry: the URL keeps the folder, the device the name.
        assert_eq!((games[1].file.as_str(), games[1].developer.as_str()), ("two v1.gba", "A, B"));
        assert_eq!(games[1].url, "https://hh3.gbdev.io/static/database-gba/entries/two/files/two%20v1.gba");
    }
}
