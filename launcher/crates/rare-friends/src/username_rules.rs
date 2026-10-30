//! Rare username rules, for instant feedback while typing (the server is the judge:
//! market api/src/usernames/rules.ts). 3 to 20 of a-z 0-9 _ . ; no leading or trailing . or _ ;
//! no two . or _ in a row. Names are lowercase.

use serde::Serialize;

pub const MIN_LEN: usize = 3;
pub const MAX_LEN: usize = 20;

/// What the player typed, as a candidate: trimmed, no @, lowercase.
pub fn normalize_username(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix('@').unwrap_or(t).to_lowercase()
}

fn sep(c: char) -> bool {
    c == '.' || c == '_'
}

/// A plain sentence when the name breaks a rule, else None.
pub fn username_problem(raw: &str) -> Option<String> {
    let u = normalize_username(raw);
    if u.is_empty() {
        return None;
    }
    if !u.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || sep(c)) {
        return Some("Use letters, numbers, _ and . only.".into());
    }
    if u.starts_with(sep) || u.ends_with(sep) {
        return Some("Start and end with a letter or number.".into());
    }
    if u.as_bytes().windows(2).any(|w| sep(w[0] as char) && sep(w[1] as char)) {
        return Some("No two . or _ in a row.".into());
    }
    if u.len() < MIN_LEN || u.len() > MAX_LEN {
        return Some(format!("Use {MIN_LEN} to {MAX_LEN} characters."));
    }
    None
}

/// What the add friend box holds.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    Address { address: String },
    Suins { name: String },
    Username { username: String },
}

/// What the add friend box holds: a 0x address, a .sui name, or a username (with or without @).
pub fn parse_target(raw: &str) -> Option<Target> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(h) = t.strip_prefix("0x") {
        if (1..=64).contains(&h.len()) && h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(Target::Address { address: t.to_lowercase() });
        }
    }
    if t.starts_with('@') {
        return Some(Target::Username { username: normalize_username(t) });
    }
    // name.sui, or the wallet's older form pay@alice
    let one_at = match t.split_once('@') {
        Some((a, b)) => !a.is_empty() && !b.is_empty() && !b.contains('@'),
        None => false,
    };
    if t.to_lowercase().ends_with(".sui") || one_at {
        return Some(Target::Suins { name: t.to_lowercase() });
    }
    Some(Target::Username { username: normalize_username(t) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize_username("  @Mira_K "), "mira_k");
    }

    #[test]
    fn accepts_good_names() {
        for s in ["mira", "a.b", "dave_99", &"x".repeat(20)] {
            assert_eq!(username_problem(s), None, "{s}");
        }
    }

    #[test]
    fn explains_broken_ones() {
        let has = |s: &str, part: &str| assert!(username_problem(s).unwrap_or_default().contains(part), "{s}: {:?}", username_problem(s));
        has("ab", "3 to 20");
        has(&"x".repeat(21), "3 to 20");
        has("da ve", "letters, numbers");
        has(".dave", "Start and end");
        has("dave_", "Start and end");
        has("da..ve", "two");
        assert_eq!(username_problem(""), None);
    }

    #[test]
    fn tells_what_the_add_box_holds() {
        assert_eq!(parse_target("@Mira"), Some(Target::Username { username: "mira".into() }));
        assert_eq!(parse_target("mira"), Some(Target::Username { username: "mira".into() }));
        assert_eq!(parse_target("Mira.sui"), Some(Target::Suins { name: "mira.sui".into() }));
        assert_eq!(parse_target("pay@alice"), Some(Target::Suins { name: "pay@alice".into() }));
        assert_eq!(parse_target("0xAB12"), Some(Target::Address { address: "0xab12".into() }));
        assert_eq!(parse_target("  "), None);
    }
}
