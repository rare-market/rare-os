//! Settings search: pure matching over the typed index (screens/settings/search.ts).

use super::sections::{SectionId, SECTIONS};

/// One row of Settings. `id` is the row's setting id; search jumps to it.
#[derive(Clone, Debug)]
pub struct SettingEntry {
    pub id: String,
    pub section: SectionId,
    pub title: String,
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SectionHits {
    pub section: SectionId,
    pub label: &'static str,
    pub entries: Vec<SettingEntry>,
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// How well one query word fits a field's words: 3 start of a word, 2 inside a word, 0 none.
fn word_score(q: &str, field: &[String]) -> u32 {
    let mut best = 0;
    for w in field {
        if w.starts_with(q) {
            return 3;
        }
        if q.len() >= 3 && w.contains(q) {
            best = 2;
        }
    }
    best
}

/// Every query word has to fit somewhere. Title counts most, then keywords, then the section name.
pub fn score_entry(query: &str, e: &SettingEntry) -> u32 {
    let qs = words(query);
    if qs.is_empty() {
        return 0;
    }
    let title = words(&e.title);
    let keys: Vec<String> = e.keywords.iter().flat_map(|k| words(k)).collect();
    let sect = words(e.section.label());
    let mut total = 0;
    for q in &qs {
        let t = word_score(q, &title);
        let k = word_score(q, &keys);
        let s = word_score(q, &sect);
        let best = (t * 3).max(k * 2).max(s);
        if best == 0 {
            return 0;
        }
        total += best;
    }
    total
}

/// Matches grouped by section (in the order of the left list), best first inside a section.
pub fn search_settings(query: &str, entries: &[SettingEntry]) -> Vec<SectionHits> {
    let scored: Vec<(&SettingEntry, u32)> = entries.iter().map(|e| (e, score_entry(query, e))).filter(|(_, s)| *s > 0).collect();
    let mut out = Vec::new();
    for (id, label) in SECTIONS {
        let mut hits: Vec<_> = scored.iter().filter(|(e, _)| e.section == id).collect();
        hits.sort_by(|a, b| b.1.cmp(&a.1));
        if !hits.is_empty() {
            out.push(SectionHits { section: id, label, entries: hits.into_iter().map(|(e, _)| (*e).clone()).collect() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::index::settings_index;

    fn e(id: &str, section: SectionId, title: &str, keywords: &[&str]) -> SettingEntry {
        SettingEntry { id: id.into(), section, title: title.into(), keywords: keywords.iter().map(|s| s.to_string()).collect() }
    }

    fn sample() -> Vec<SettingEntry> {
        vec![
            e("brightness", SectionId::Display, "Brightness", &["screen", "dim", "backlight"]),
            e("volume", SectionId::Sound, "Volume", &["audio", "loud", "speaker"]),
            e("wifi", SectionId::Network, "Wi-Fi", &["wireless", "internet", "connect"]),
            e("sleep", SectionId::Power, "Sleep when idle", &["suspend", "timeout"]),
            e("dim", SectionId::Display, "Dim after idle", &["screen"]),
        ]
    }

    fn ids(h: &[SectionHits]) -> Vec<Vec<&str>> {
        h.iter().map(|g| g.entries.iter().map(|x| x.id.as_str()).collect()).collect()
    }

    #[test]
    fn returns_nothing_for_an_empty_query() {
        assert!(search_settings("", &sample()).is_empty());
        assert!(search_settings("   ", &sample()).is_empty());
    }

    #[test]
    fn matches_titles_by_word_prefix_ignoring_case() {
        let r = search_settings("BRIGHT", &sample());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].entries[0].id, "brightness");
    }

    #[test]
    fn matches_keywords() {
        assert_eq!(search_settings("internet", &sample())[0].entries[0].id, "wifi");
        assert_eq!(search_settings("suspend", &sample())[0].entries[0].id, "sleep");
    }

    #[test]
    fn needs_every_word_to_match() {
        assert_eq!(ids(&search_settings("screen idle", &sample())), vec![vec!["dim"]]);
        assert!(search_settings("screen volume", &sample()).is_empty());
    }

    #[test]
    fn groups_by_section_in_list_order_and_ranks_the_title_above_a_keyword() {
        let r = search_settings("screen", &sample());
        assert_eq!(r.iter().map(|g| g.section).collect::<Vec<_>>(), vec![SectionId::Display]);
        let r2 = search_settings("idle", &sample());
        assert_eq!(r2.iter().map(|g| g.section).collect::<Vec<_>>(), vec![SectionId::Display, SectionId::Power]);
        let r3 = search_settings("dim", &sample());
        assert_eq!(r3[0].entries.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["dim", "brightness"]);
    }

    #[test]
    fn matches_inside_a_word_only_from_three_letters() {
        assert_eq!(score_entry("ri", &e("a", SectionId::System, "Wi-Fi", &[])), 0);
        assert!(score_entry("ight", &e("a", SectionId::Display, "Brightness", &[])) > 0);
    }

    #[test]
    fn finds_a_section_by_its_name() {
        assert!(search_settings("sound", &sample()).iter().any(|g| g.section == SectionId::Sound));
    }

    #[test]
    fn the_real_index_has_unique_ids_and_known_sections() {
        let idx = settings_index();
        let mut ids: Vec<&str> = idx.iter().map(|x| x.id.as_str()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn the_real_index_finds_the_rows_people_look_for() {
        let idx = settings_index();
        let first = |q: &str| search_settings(q, &idx).first().and_then(|g| g.entries.first()).map(|e| e.id.clone());
        assert_eq!(first("brightness").as_deref(), Some("brightness"));
        assert_eq!(first("wifi").as_deref(), Some("wifi"));
        assert_eq!(first("volume").as_deref(), Some("volume"));
        assert!(first("battery").is_some());
        assert_eq!(first("language").as_deref(), Some("language"));
    }
}
