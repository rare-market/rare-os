//! Copies of one game: the same game from Steam and the cloud, a ROM as an ISO and as a folder.
//! The library keeps every copy; this says which ones are the same game, so the UI shows one tile
//! with "Play on" for each copy.
//!
//! Identity, most certain first:
//! 1. A store cross-reference or a disc serial (`Library::keys`): the same key is the same game.
//!    Two different Steam app ids are never the same game (Modern Warfare 2 and its multiplayer).
//! 2. The normalised title: case, marks (TM, R), punctuation, spacing, region tags, roman numerals
//!    and platform or standard-edition wording ("Xbox Series X|S", "Standard Edition") do not count.
//!    A year in brackets does ("Dead Space" vs "Dead Space (2008)"). Content editions ("Game of the
//!    Year", "Remastered", "Definitive") are other products unless a key says otherwise; they only
//!    rank as suggestions for a manual merge.
//!
//! Only copies from different places join by title: two products in one store stay apart. ROMs
//! join only ROMs of the same system (a GameCube and a PS2 copy are different ports). Apps and
//! emulators never join. The player's own merges and splits come last and always win.

use std::collections::HashMap;

/// One library entry, as the matcher sees it.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub title: &'a str,
    /// A store cross-reference or disc serial ("steam:220", "ps3:BLUS30982").
    pub key: Option<&'a str>,
}

/// Scores at or over this join automatically.
pub const AUTO: f32 = 0.9;

/// Words that name the platform or the plain edition, never a different product.
const COSMETIC: &[&str] = &[
    "xbox series x s",
    "xbox series x",
    "xbox series",
    "xbox one edition",
    "xbox one",
    "xbox 360 edition",
    "for windows 10",
    "windows 10 edition",
    "windows 10",
    "windows edition",
    "pc edition",
    "console edition",
    "digital standard edition",
    "standard edition",
    "digital edition",
    "cross gen bundle",
    "cross gen edition",
    "base game",
    "nkit",
    "xiso",
];

/// Editions that add content: the same game, but a store sells them as another product.
const EDITIONS: &[&str] = &[
    "game of the year edition",
    "game of the year",
    "goty edition",
    "goty",
    "definitive edition",
    "ultimate edition",
    "complete edition",
    "deluxe edition",
    "gold edition",
    "enhanced edition",
    "anniversary edition",
    "special edition",
    "premium edition",
    "legendary edition",
    "collectors edition",
    "directors cut",
    "remastered",
    "remaster",
    "definitive",
    "ultimate",
    "complete",
    "deluxe",
    "enhanced",
    "hd",
];

fn roman(w: &str) -> Option<&'static str> {
    // X is left alone: "Mega Man X" is not "Mega Man 10".
    Some(match w {
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "v" => "5",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        "ix" => "9",
        _ => return None,
    })
}

/// The title's words: lower case, marks and brackets gone (a year in brackets kept).
fn words(title: &str) -> Vec<String> {
    let mut flat = String::new();
    let mut depth = 0;
    let mut inner = String::new();
    for c in title.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                inner.clear();
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                let t = inner.trim();
                if depth == 0 && t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) && (t.starts_with("19") || t.starts_with("20")) {
                    flat.push(' ');
                    flat.push_str(t);
                    flat.push(' ');
                }
            }
            _ if depth > 0 => inner.push(c),
            '&' => flat.push_str(" and "),
            '\'' | '\u{2019}' | '\u{2018}' | '`' | '\u{2122}' | '\u{ae}' | '\u{a9}' => {}
            c if c.is_alphanumeric() => flat.extend(c.to_lowercase()),
            _ => flat.push(' '),
        }
    }
    let mut v: Vec<String> = flat.split_whitespace().map(|w| roman(w).map(str::to_string).unwrap_or_else(|| fold(w))).collect();
    if v.first().is_some_and(|w| w == "the") && v.len() > 1 {
        v.remove(0);
    }
    v
}

/// Accented letters as plain ones (Brütal = Brutal).
fn fold(w: &str) -> String {
    w.chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Takes the phrases off the end of the words (repeatedly), longest first.
fn strip_tail(mut v: Vec<String>, phrases: &[&str]) -> Vec<String> {
    loop {
        let joined = v.join(" ");
        let Some(p) = phrases.iter().find(|p| joined.ends_with(*p) && (joined.len() == p.len() || joined.as_bytes()[joined.len() - p.len() - 1] == b' ')) else { return v };
        let n = p.split(' ').count();
        if n >= v.len() {
            return v;
        }
        v.truncate(v.len() - n);
    }
}

/// The title as identity: "Call of Duty®: Modern Warfare® II - Cross-Gen Bundle" is
/// "callofdutymodernwarfare2"; "Borderlands2" and "Borderlands 2" are the same.
pub fn norm(title: &str) -> String {
    strip_tail(words(title), COSMETIC).concat()
}

/// Looser: content editions off too (only for suggestions).
fn loose(title: &str) -> String {
    strip_tail(strip_tail(words(title), COSMETIC), EDITIONS).concat()
}

/// Where a copy comes from, finely enough that two copies from one place are two products:
/// the source, the cloud service, the ROM's system.
fn place(c: &Entry) -> String {
    match c.source {
        "cloud" => c.id.splitn(3, ':').take(2).collect::<Vec<_>>().join(":"),
        "emu" => format!("emu:{}", rom_system(c.id).unwrap_or("")),
        s => s.to_string(),
    }
}

/// "emu:ps3/x.iso" and "emu:sd/ps3/x.iso" are ps3.
pub fn rom_system(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("emu:")?;
    let rest = rest.strip_prefix("sd/").unwrap_or(rest);
    Some(rest.split_once('/')?.0)
}

fn steam_key(c: &Entry) -> Option<String> {
    match c.key {
        Some(k) if k.starts_with("steam:") => Some(k.to_string()),
        _ => c.id.strip_prefix("steam:").filter(|a| a.chars().all(|c| c.is_ascii_digit())).map(|a| format!("steam:{a}")),
    }
}

/// The key a copy goes by: its cross-reference, else a Steam game's own app id.
fn key_of(c: &Entry) -> Option<String> {
    c.key.map(str::to_string).or_else(|| steam_key(c))
}

/// Whether two copies may be one game at all (before looking at titles).
fn may_join(a: &Entry, b: &Entry) -> bool {
    if a.source == "apps" || b.source == "apps" {
        return false;
    }
    // ROMs only with ROMs, and only on the same system.
    if (a.source == "emu") != (b.source == "emu") {
        return false;
    }
    if a.source == "emu" && rom_system(a.id) != rom_system(b.id) {
        return false;
    }
    // Two Steam apps are two products.
    if let (Some(x), Some(y)) = (steam_key(a), steam_key(b)) {
        if x != y {
            return false;
        }
    }
    true
}

/// How sure we are that two copies are one game, 0 to 1.
pub fn score(a: &Entry, b: &Entry) -> f32 {
    if !may_join(a, b) {
        return 0.0;
    }
    if let (Some(x), Some(y)) = (key_of(a), key_of(b)) {
        if x == y {
            return 1.0;
        }
    }
    // One store, two products: only ROMs (two dumps) and folders join by title.
    let same_place = place(a) == place(b) && a.source != "emu" && a.source != "local";
    let (na, nb) = (norm(a.title), norm(b.title));
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    let s = if na == nb {
        0.95
    } else if loose(a.title) == loose(b.title) {
        0.7
    } else {
        // Shared words, for suggestions.
        const STOP: &[&str] = &["the", "of", "a", "an", "and", "new", "to", "in", "for", "on", "edition", "game"];
        let wa: Vec<String> = words(a.title).into_iter().filter(|w| !STOP.contains(&w.as_str())).collect();
        let wb: Vec<String> = words(b.title).into_iter().filter(|w| !STOP.contains(&w.as_str())).collect();
        let common = wa.iter().filter(|w| wb.contains(w)).count() as f32;
        let all = (wa.len() + wb.len()) as f32 - common;
        if all <= 0.0 {
            0.0
        } else {
            0.6 * common / all
        }
    };
    if same_place {
        s.min(0.6)
    } else {
        s
    }
}

/// The player's own calls: groups they put together and pairs they said are different games.
#[derive(Debug, Clone, Copy, Default)]
pub struct Choices<'a> {
    pub merged: &'a [Vec<String>],
    pub apart: &'a [[String; 2]],
}

struct Sets {
    up: Vec<usize>,
}

impl Sets {
    fn find(&mut self, mut i: usize) -> usize {
        while self.up[i] != i {
            self.up[i] = self.up[self.up[i]];
            i = self.up[i];
        }
        i
    }
}

/// The groups of two or more copies, each in the order the copies came.
pub fn groups(copies: &[Entry], choices: Choices) -> Vec<Vec<String>> {
    let n = copies.len();
    let mut sets = Sets { up: (0..n).collect() };
    let at: HashMap<&str, usize> = copies.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
    let apart: std::collections::HashSet<(usize, usize)> = choices
        .apart
        .iter()
        .filter_map(|[a, b]| Some((*at.get(a.as_str())?, *at.get(b.as_str())?)))
        .flat_map(|(a, b)| [(a, b), (b, a)])
        .collect();
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let join = |sets: &mut Sets, members: &mut Vec<Vec<usize>>, a: usize, b: usize, force: bool| {
        let (ra, rb) = (sets.find(a), sets.find(b));
        if ra == rb {
            return;
        }
        // Never across a pair the player split.
        if !force && members[ra].iter().any(|x| members[rb].iter().any(|y| apart.contains(&(*x, *y)))) {
            return;
        }
        sets.up[rb] = ra;
        let moved = std::mem::take(&mut members[rb]);
        members[ra].extend(moved);
    };
    // Candidates: copies sharing a key, then copies sharing a normalised title.
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_title: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, c) in copies.iter().enumerate() {
        if c.source == "apps" {
            continue;
        }
        if let Some(k) = key_of(c) {
            by_key.entry(k).or_default().push(i);
        }
        let t = norm(c.title);
        if !t.is_empty() {
            by_title.entry(t).or_default().push(i);
        }
    }
    let mut buckets: Vec<Vec<usize>> = by_key.into_values().filter(|v| v.len() > 1).collect();
    buckets.sort();
    let mut titled: Vec<Vec<usize>> = by_title.into_values().filter(|v| v.len() > 1).collect();
    titled.sort();
    buckets.extend(titled);
    for b in &buckets {
        for (x, &i) in b.iter().enumerate() {
            for &j in &b[x + 1..] {
                if score(&copies[i], &copies[j]) >= AUTO {
                    join(&mut sets, &mut members, i, j, false);
                }
            }
        }
    }
    for m in choices.merged {
        let ids: Vec<usize> = m.iter().filter_map(|id| at.get(id.as_str()).copied()).collect();
        for w in ids.windows(2) {
            join(&mut sets, &mut members, w[0], w[1], true);
        }
    }
    let mut out: Vec<Vec<usize>> = members.into_iter().filter(|m| m.len() > 1).collect();
    for m in out.iter_mut() {
        m.sort();
    }
    out.sort();
    out.into_iter().map(|m| m.into_iter().map(|i| copies[i].id.to_string()).collect()).collect()
}

/// A disc's serial in the same form for every dump ("BLUS-30982", "SLUS_213.62" are BLUS30982,
/// SLUS21362).
pub fn serial_key(system: &str, serial: &str) -> Option<String> {
    let s: String = serial.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase();
    (s.len() >= 4).then(|| format!("{system}:{s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c<'a>(id: &'a str, source: &'a str, title: &'a str) -> Entry<'a> {
        Entry { id, source, title, key: None }
    }

    #[test]
    fn titles() {
        assert_eq!(norm("Borderlands 2"), norm("Borderlands2"));
        assert_ne!(norm("Borderlands 2"), norm("Borderlands: The Pre-Sequel"));
        assert_eq!(norm("Call of Duty®: Modern Warfare® II - Cross-Gen Bundle"), "callofdutymodernwarfare2");
        assert_ne!(norm("Call of Duty: Modern Warfare 2 (2009)"), norm("Call of Duty®: Modern Warfare® II"));
        assert_ne!(norm("Dead Space"), norm("Dead Space (2008)"));
        assert_eq!(norm("Medieval Dynasty (Xbox One)"), norm("Medieval Dynasty"));
        assert_eq!(norm("Avowed Standard Edition"), norm("Avowed"));
        assert_eq!(norm("Control Ultimate Edition - Xbox Series X|S"), norm("Control Ultimate Edition"));
        assert_ne!(norm("Control Ultimate Edition"), norm("Control"));
        assert_eq!(loose("Control Ultimate Edition"), loose("Control"));
        assert_eq!(norm("Luigi's Mansion .nkit"), norm("Luigi's Mansion (USA)"));
        assert_eq!(norm("The Witcher 3"), norm("Witcher III"));
        assert_eq!(norm("Brütal Legend"), norm("Brutal Legend"));
        assert_ne!(norm("Mega Man X"), norm("Mega Man 10"));
        assert_eq!(norm("Edition"), "edition");
    }

    #[test]
    fn the_devices_duplicates() {
        let v = [
            c("steam:1145350", "steam", "Hades II"),
            c("cloud:xbox:9PCR3Z2MSM4T", "cloud", "Hades II"),
            c("steam:10180", "steam", "Call of Duty: Modern Warfare 2 (2009)"),
            c("steam:10190", "steam", "Call of Duty: Modern Warfare 2 (2009) - Multiplayer"),
            c("cloud:xbox:9PNGFSX7S117", "cloud", "Call of Duty®: Modern Warfare® II - Cross-Gen Bundle"),
            c("cloud:xbox:9NLB6V0GNC9P", "cloud", "Dead Space"),
            c("cloud:xbox:BRJLJ6RPLQTJ", "cloud", "Dead Space (2008)"),
            c("cloud:xbox:9PDDP6ML6XHF", "cloud", "Medieval Dynasty"),
            c("cloud:xbox:9N2PS831S84N", "cloud", "Medieval Dynasty (Xbox One)"),
            c("emu:ps3/Borderlands 2 (USA) (En,Fr,De,Es,It).iso", "emu", "Borderlands 2"),
            c("emu:ps3/Borderlands2", "emu", "Borderlands2"),
            c("emu:gc/Tony Hawk's Pro Skater 4 (USA).nkit.iso", "emu", "Tony Hawk's Pro Skater 4"),
            c("emu:ps2/Tony Hawk's Pro Skater 4 (USA) (v2.01).iso", "emu", "Tony Hawk's Pro Skater 4"),
            c("steam:2399830", "steam", "ARK: Survival Ascended"),
            c("cloud:xbox:9P33VJGVPHVP", "cloud", "ARK: Survival Ascended"),
            c("app:1", "apps", "Hades II"),
        ];
        let g = groups(&v, Choices::default());
        assert_eq!(
            g,
            vec![
                vec!["steam:1145350".to_string(), "cloud:xbox:9PCR3Z2MSM4T".into()],
                vec!["emu:ps3/Borderlands 2 (USA) (En,Fr,De,Es,It).iso".into(), "emu:ps3/Borderlands2".into()],
                vec!["steam:2399830".into(), "cloud:xbox:9P33VJGVPHVP".into()],
            ]
        );
    }

    #[test]
    fn keys_and_the_players_calls() {
        let mut a = c("emu:ps3/BL2.iso", "emu", "BL2");
        a.key = Some("ps3:BLUS30982");
        let mut b = c("emu:ps3/Borderlands2", "emu", "Borderlands2");
        b.key = Some("ps3:BLUS30982");
        let mut gfn = c("cloud:gfn:1", "cloud", "Hades 2");
        gfn.key = Some("steam:1145350");
        let v = [a, b, c("steam:1145350", "steam", "Hades II"), gfn, c("epic:x", "epic", "Hades II"), c("gog:y", "gog", "Some Other Game")];
        let g = groups(&v, Choices::default());
        assert_eq!(g.len(), 2);
        assert_eq!(g[1], vec!["steam:1145350", "cloud:gfn:1", "epic:x"]);
        // Split Epic off; merge GOG in by hand.
        let apart = [["epic:x".to_string(), "steam:1145350".to_string()], ["epic:x".to_string(), "cloud:gfn:1".to_string()]];
        let merged = [vec!["gog:y".to_string(), "steam:1145350".to_string()]];
        let g = groups(&v, Choices { merged: &merged, apart: &apart });
        assert_eq!(g[1], vec!["steam:1145350", "cloud:gfn:1", "gog:y"]);
        assert_eq!(serial_key("ps2", "SLUS_213.62").as_deref(), Some("ps2:SLUS21362"));
        assert!(score(&v[2], &v[5]) < AUTO);
    }
}
