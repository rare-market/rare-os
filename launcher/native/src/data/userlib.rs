//! The player's own library organisation: collections and hidden games, kept in
//! ~/.local/state/rare-native/userlib.json next to favourites.json. Ids are library game ids.

use serde::{Deserialize, Serialize};

/// ~/.local/state/rare-native (XDG_STATE_HOME when set).
pub fn state_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_STATE_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"));
    base.join("rare-native")
}

/// Writes JSON next to the target and renames it in, so a power cut never leaves half a file.
pub fn write_json<T: Serialize>(path: &std::path::Path, v: &T) {
    let Ok(b) = serde_json::to_vec_pretty(v) else { return };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, b).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &std::path::Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Hash)]
pub struct Collection {
    pub name: String,
    #[serde(default)]
    pub games: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Hash)]
pub struct UserLib {
    #[serde(default)]
    pub collections: Vec<Collection>,
    #[serde(default)]
    pub hidden: Vec<String>,
    /// Copies the player put together as one game (tile Options > Same game as), and pairs they
    /// said are different games (Not the same game): data/groups.rs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apart: Vec<[String; 2]>,
    /// The last rename (old, new), so a view on the old name can follow it.
    #[serde(skip)]
    pub renamed: Option<(String, String)>,
}

/// Longest collection name.
pub const NAME_MAX: usize = 40;

fn path() -> std::path::PathBuf {
    state_dir().join("userlib.json")
}

impl UserLib {
    /// RR_USERLIB=<userlib.json> reads that file instead (fixture shots), and never writes it.
    pub fn load() -> UserLib {
        if let Ok(p) = std::env::var("RR_USERLIB") {
            return read_json(std::path::Path::new(&p));
        }
        if std::env::var("RR_FIXTURE").is_ok() {
            return UserLib::default();
        }
        read_json(&path())
    }

    pub fn save(&self) {
        if std::env::var("RR_FIXTURE").is_ok() || std::env::var("RR_USERLIB").is_ok() {
            return;
        }
        write_json(&path(), self);
    }

    pub fn is_hidden(&self, id: &str) -> bool {
        self.hidden.iter().any(|h| h == id)
    }

    /// Hides or shows a game; true when it is hidden now.
    pub fn toggle_hidden(&mut self, id: &str) -> bool {
        if let Some(i) = self.hidden.iter().position(|h| h == id) {
            self.hidden.remove(i);
            false
        } else {
            self.hidden.push(id.to_string());
            true
        }
    }

    pub fn collection(&self, name: &str) -> Option<&Collection> {
        self.collections.iter().find(|c| c.name == name)
    }

    pub fn in_collection(&self, name: &str, id: &str) -> bool {
        self.collection(name).is_some_and(|c| c.games.iter().any(|g| g == id))
    }

    /// Adds the game to the collection or takes it out; true when it is in it now.
    pub fn toggle_in(&mut self, name: &str, id: &str) -> bool {
        let Some(c) = self.collections.iter_mut().find(|c| c.name == name) else { return false };
        if let Some(i) = c.games.iter().position(|g| g == id) {
            c.games.remove(i);
            false
        } else {
            c.games.push(id.to_string());
            true
        }
    }

    /// A clean name: trimmed, one line, not too long.
    pub fn clean(name: &str) -> String {
        let s: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
        s.chars().take(NAME_MAX).collect()
    }

    /// A new collection (with the game in it, when given). Err with what to tell the player.
    pub fn create(&mut self, name: &str, game: Option<&str>) -> Result<String, &'static str> {
        let name = UserLib::clean(name);
        if name.is_empty() {
            return Err("Type a name for the collection.");
        }
        if self.collections.iter().any(|c| c.name.eq_ignore_ascii_case(&name)) {
            return Err("You already have a collection with that name.");
        }
        self.collections.push(Collection { name: name.clone(), games: game.map(|g| vec![g.to_string()]).unwrap_or_default() });
        Ok(name)
    }

    pub fn rename(&mut self, old: &str, new: &str) -> Result<String, &'static str> {
        let new = UserLib::clean(new);
        if new.is_empty() {
            return Err("Type a name for the collection.");
        }
        if new != old && self.collections.iter().any(|c| c.name.eq_ignore_ascii_case(&new) && c.name != old) {
            return Err("You already have a collection with that name.");
        }
        let c = self.collections.iter_mut().find(|c| c.name == old).ok_or("That collection is gone.")?;
        c.name = new.clone();
        self.renamed = Some((old.to_string(), new.clone()));
        Ok(new)
    }

    /// Copies `a` and `b` (and their groups, `ga` and `gb`) are one game: their split pairs go.
    pub fn merge(&mut self, ga: &[String], gb: &[String]) {
        self.apart.retain(|[x, y]| !((ga.contains(x) && gb.contains(y)) || (ga.contains(y) && gb.contains(x))));
        let mut v: Vec<String> = ga.iter().chain(gb.iter()).cloned().collect();
        v.dedup();
        self.merged.push(v);
    }

    /// `id` is not the same game as the other copies in `group`.
    pub fn split(&mut self, id: &str, group: &[String]) {
        for o in group.iter().filter(|o| *o != id) {
            if !self.apart.iter().any(|[x, y]| (x == id && y == o) || (y == id && x == o)) {
                self.apart.push([id.to_string(), o.clone()]);
            }
        }
        for m in self.merged.iter_mut() {
            m.retain(|x| x != id);
        }
        self.merged.retain(|m| m.len() > 1);
    }

    pub fn delete(&mut self, name: &str) -> bool {
        let n = self.collections.len();
        self.collections.retain(|c| c.name != name);
        self.collections.len() != n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collections_and_hidden() {
        let mut u = UserLib::default();
        assert_eq!(u.create("  Co-op   nights ", Some("steam:1")), Ok("Co-op nights".into()));
        assert!(u.create("co-op NIGHTS", None).is_err());
        assert!(u.create("   ", None).is_err());
        assert!(u.in_collection("Co-op nights", "steam:1"));
        assert!(u.toggle_in("Co-op nights", "local:x"));
        assert!(!u.toggle_in("Co-op nights", "steam:1"));
        assert_eq!(u.rename("Co-op nights", "Couch"), Ok("Couch".into()));
        assert_eq!(u.collection("Couch").unwrap().games, vec!["local:x"]);
        assert!(u.toggle_hidden("steam:2"));
        assert!(u.is_hidden("steam:2"));
        assert!(!u.toggle_hidden("steam:2"));
        assert!(u.delete("Couch"));
        assert!(u.collections.is_empty());
        let j: UserLib = serde_json::from_str("{}").unwrap();
        assert_eq!(j, UserLib::default());
    }
}
