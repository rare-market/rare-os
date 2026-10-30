//! One tile per game: the library's copies of the same game (Steam and the cloud, a ROM as an ISO
//! and as a folder) grouped by rare-library's matcher (dupes.rs) and the player's own merges and
//! splits (userlib.json). Every copy stays in the library; the views show the group once, as its
//! default copy (the installed one played last) with the best art, the latest last played and
//! the playtime of all copies. The game page lists the copies under "Play on".

use crate::data::model::{Game, Library};
use crate::data::userlib::UserLib;
use std::collections::HashMap;

#[derive(Default, Clone, Debug)]
pub struct Groups {
    /// Copy id to its group's index in `sets`.
    of: HashMap<String, usize>,
    /// Each group's copy ids.
    sets: Vec<Vec<String>>,
    /// Each copy of a group as the views show it (merged art, last played, playtime and title).
    shown: HashMap<String, Game>,
}

/// Art first from the stores with the best art; ROMs and the cloud last.
fn art_rank(g: &Game) -> u8 {
    match g.source.as_str() {
        "steam" => 0,
        "epic" | "gog" => 1,
        "rare" | "itch" | "local" | "web" | "android" => 2,
        "emu" => 3,
        _ => 4,
    }
}

/// The order copies are offered in, the default first: installed, then the one played last, then
/// the store with the best art.
pub fn lead_order(a: &Game, b: &Game) -> std::cmp::Ordering {
    b.installed.cmp(&a.installed).then_with(|| b.last_played.unwrap_or(0).cmp(&a.last_played.unwrap_or(0))).then_with(|| art_rank(a).cmp(&art_rank(b)))
}

fn all(lib: &Library) -> impl Iterator<Item = &Game> {
    lib.games.iter().chain(lib.owned.iter()).chain(lib.cloud.iter())
}

impl Groups {
    /// `renamed`: copies the player renamed; the first of them (default first) names the group.
    pub fn build(lib: &Library, user: &UserLib, renamed: &std::collections::HashSet<String>) -> Groups {
        let games: Vec<&Game> = all(lib).collect();
        let copies: Vec<rare_library::dupes::Entry> = games.iter().map(|g| rare_library::dupes::Entry { id: &g.id, source: &g.source, title: &g.title, key: lib.keys.get(&g.id).map(String::as_str) }).collect();
        let sets = rare_library::dupes::groups(&copies, rare_library::dupes::Choices { merged: &user.merged, apart: &user.apart });
        let by_id: HashMap<&str, &Game> = games.iter().map(|g| (g.id.as_str(), *g)).collect();
        let mut out = Groups::default();
        for (i, set) in sets.into_iter().enumerate() {
            let mut members: Vec<&Game> = set.iter().filter_map(|id| by_id.get(id.as_str()).copied()).collect();
            members.sort_by(|a, b| lead_order(a, b));
            let by_art = {
                let mut v = members.clone();
                v.sort_by_key(|g| (art_rank(g), g.art.cover.is_empty()));
                v
            };
            // Every copy's links, best store first: when one copy's art doesn't load (a Steam
            // capsule missing from the CDN), the next copy's (the cloud's cover) shows instead.
            let pick = |f: fn(&Game) -> &Vec<String>| {
                let mut v: Vec<String> = Vec::new();
                for s in by_art.iter().flat_map(|g| f(g).iter()) {
                    if !v.contains(s) {
                        v.push(s.clone());
                    }
                }
                v
            };
            let art = crate::data::model::Art { cover: pick(|g| &g.art.cover), hero: pick(|g| &g.art.hero), logo: pick(|g| &g.art.logo), header: pick(|g| &g.art.header), icon: pick(|g| &g.art.icon) };
            // The player's own name first; then the store's name over a file name, and of two file
            // names the one with more words.
            let own = members.iter().find(|g| renamed.contains(&g.id)).map(|g| g.title.clone());
            let title = own.unwrap_or_else(|| by_art.iter().min_by_key(|g| (art_rank(g), usize::MAX - g.title.split_whitespace().count())).map(|g| g.title.clone()).unwrap_or_default());
            let last = members.iter().filter_map(|g| g.last_played).filter(|t| *t > 0).max();
            let mins: i64 = members.iter().filter_map(|g| g.playtime_minutes).sum();
            for g in &members {
                let mut m = (*g).clone();
                m.title = title.clone();
                m.art = art.clone();
                m.last_played = last.or(g.last_played);
                m.playtime_minutes = if mins > 0 { Some(mins) } else { g.playtime_minutes };
                out.of.insert(g.id.clone(), i);
                out.shown.insert(g.id.clone(), m);
            }
            out.sets.push(members.iter().map(|g| g.id.clone()).collect());
        }
        out
    }

    /// The copies' ids of the game `id` is a copy of, the default first ([id] when it has none).
    pub fn ids(&self, id: &str) -> Vec<String> {
        match self.of.get(id) {
            Some(i) => self.sets[*i].clone(),
            None => vec![id.to_string()],
        }
    }

    pub fn group(&self, id: &str) -> Option<usize> {
        self.of.get(id).copied()
    }

    /// The copy as the views show it.
    pub fn shown<'a>(&'a self, g: &'a Game) -> &'a Game {
        self.shown.get(&g.id).unwrap_or(g)
    }

    /// How many groups (for the report line in the log).
    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// One entry per game: of the copies in `v`, the one to show (the default among them), as
    /// the views show it. The first copy's place in `v` is kept.
    pub fn one_each<'a>(&'a self, v: impl IntoIterator<Item = &'a Game>) -> Vec<&'a Game> {
        let mut out: Vec<&'a Game> = Vec::new();
        let mut at: HashMap<usize, usize> = HashMap::new();
        for g in v {
            match self.of.get(&g.id) {
                None => out.push(g),
                Some(i) => match at.get(i) {
                    None => {
                        at.insert(*i, out.len());
                        out.push(g);
                    }
                    Some(&j) => {
                        if lead_order(g, out[j]) == std::cmp::Ordering::Less {
                            out[j] = g;
                        }
                    }
                },
            }
        }
        out.into_iter().map(|g| self.shown(g)).collect()
    }
}

/// What a copy is called under "Play on": its store, a cloud service, a ROM's system and format.
pub fn copy_label(g: &Game) -> String {
    let base = match g.source.as_str() {
        "cloud" if g.id.starts_with("cloud:gfn:") => "GeForce NOW".to_string(),
        "cloud" => "Xbox Cloud".to_string(),
        "emu" => {
            let sys = rare_emu::roms::system_of(&g.id).map(|s| s.short).unwrap_or("ROM");
            let path = g.id.rsplit('/').next().unwrap_or("");
            let lower = path.to_ascii_lowercase();
            let kind = match lower.rsplit_once('.') {
                Some((head, ext)) if ext.len() <= 4 && !ext.contains(' ') => {
                    if head.ends_with(".nkit") {
                        "NKit".to_string()
                    } else {
                        ext.to_ascii_uppercase()
                    }
                }
                _ => "folder".to_string(),
            };
            let sd = if g.id.starts_with("emu:sd/") { " on SD card" } else { "" };
            format!("{sys} {kind}{sd}")
        }
        s => crate::data::model::source_label(s).to_string(),
    };
    if g.installed || matches!(g.source.as_str(), "cloud" | "web") {
        base
    } else {
        format!("{base} \u{b7} Not installed")
    }
}

/// The labels for a game's copies, told apart when two read the same (the copy's own title, cut,
/// else a number).
pub fn copy_labels(copies: &[&Game]) -> Vec<String> {
    let base: Vec<String> = copies.iter().map(|g| copy_label(g)).collect();
    base.iter()
        .enumerate()
        .map(|(i, l)| {
            let same: Vec<usize> = (0..base.len()).filter(|j| base[*j] == *l).collect();
            if same.len() < 2 {
                return l.clone();
            }
            let titles_differ = same.iter().any(|j| copies[*j].title != copies[i].title);
            if titles_differ {
                let t: String = copies[i].title.chars().take(22).collect();
                let t = if t.chars().count() < copies[i].title.chars().count() { format!("{}\u{2026}", t.trim_end()) } else { t };
                format!("{l} \u{b7} {t}")
            } else {
                format!("{l} {}", same.iter().position(|j| *j == i).unwrap_or(0) + 1)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, source: &str, title: &str, installed: bool, last: Option<i64>, mins: Option<i64>, cover: &str) -> Game {
        serde_json::from_value(serde_json::json!({"id": id, "source": source, "title": title, "installed": installed, "lastPlayed": last, "playtimeMinutes": mins, "art": {"cover": if cover.is_empty() { vec![] } else { vec![cover] }}, "origin": ""})).unwrap()
    }

    #[test]
    fn one_tile_per_game() {
        let lib = Library {
            games: vec![
                game("emu:ps3/Borderlands 2 (USA).iso", "emu", "Borderlands 2", true, None, None, "bl2.png"),
                game("emu:ps3/Borderlands2", "emu", "Borderlands2", true, Some(50), Some(30), ""),
                game("steam:1145350", "steam", "Hades II", true, Some(10), Some(48), "hades.jpg"),
                game("emu:ps2/God of War II (USA).iso", "emu", "God of War II", true, None, None, ""),
            ],
            cloud: vec![game("cloud:xbox:9PCR3Z2MSM4T", "cloud", "Hades II", true, Some(90), Some(12), "x.jpg")],
            ..Default::default()
        };
        let mut user = UserLib::default();
        let none = std::collections::HashSet::new();
        let g = Groups::build(&lib, &user, &none);
        assert_eq!(g.len(), 2);
        // The folder was played last: it leads, with the ISO's art and the better title.
        assert_eq!(g.ids("emu:ps3/Borderlands 2 (USA).iso"), vec!["emu:ps3/Borderlands2", "emu:ps3/Borderlands 2 (USA).iso"]);
        let tiles = g.one_each(lib.games.iter());
        assert_eq!(tiles.len(), 3);
        assert_eq!(tiles[0].id, "emu:ps3/Borderlands2");
        assert_eq!(tiles[0].title, "Borderlands 2");
        assert_eq!(tiles[0].art.cover, vec!["bl2.png"]);
        assert_eq!(tiles[0].playtime_minutes, Some(30));
        // Hades II: Steam's art and both copies' time, the cloud's later last played.
        let h = g.shown(&lib.cloud[0]);
        assert_eq!((h.art.cover[0].as_str(), h.playtime_minutes, h.last_played), ("hades.jpg", Some(60), Some(90)));
        assert_eq!(copy_label(&lib.games[1]), "PS3 folder");
        assert_eq!(copy_label(&lib.games[0]), "PS3 ISO");
        assert_eq!(copy_label(&lib.cloud[0]), "Xbox Cloud");
        let two = [&lib.games[0], &lib.games[0]];
        assert_eq!(copy_labels(&two), vec!["PS3 ISO 1", "PS3 ISO 2"]);
        // A rename on the folder names the group.
        let renamed: std::collections::HashSet<String> = ["emu:ps3/Borderlands2".to_string()].into();
        assert_eq!(Groups::build(&lib, &user, &renamed).shown(&lib.games[0]).title, "Borderlands2");
        assert_eq!(Groups::build(&lib, &user, &none).len(), 2);
        // Split by the player.
        user.apart.push(["emu:ps3/Borderlands2".into(), "emu:ps3/Borderlands 2 (USA).iso".into()]);
        assert_eq!(Groups::build(&lib, &user, &none).len(), 1);
        user.merged.push(vec!["emu:ps2/God of War II (USA).iso".into(), "steam:1145350".into()]);
        assert_eq!(Groups::build(&lib, &user, &none).ids("steam:1145350").len(), 3);
    }
}
