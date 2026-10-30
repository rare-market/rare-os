//! Early access for holders: the launches that have not opened yet (launcher/src/perks/ui.tsx
//! `upcomingLaunches`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Phase {
    pub starts_at: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Launch {
    pub slug: String,
    pub name: String,
    /// An ISO date or ms since the epoch.
    pub starts_at: Option<Value>,
    pub phases: Option<Vec<Phase>>,
}

fn ms(v: &Value) -> Option<i64> {
    match v {
        Value::String(s) => rare_market::time::parse_iso_ms(s),
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).map(|f| f as i64),
        _ => None,
    }
}

/// When a launch opens: its own start, else its first phase's.
pub fn starts_ms(l: &Launch) -> Option<i64> {
    let t = l.starts_at.as_ref().filter(|v| !v.is_null()).or_else(|| l.phases.as_ref().and_then(|p| p.first()).and_then(|p| p.starts_at.as_ref()))?;
    ms(t)
}

/// Launches that have not opened yet, soonest first.
pub fn upcoming_launches(list: &[Launch], now_ms: i64) -> Vec<Launch> {
    let mut v: Vec<(i64, &Launch)> = list.iter().filter_map(|l| starts_ms(l).filter(|at| *at > now_ms).map(|at| (at, l))).collect();
    v.sort_by_key(|(at, _)| *at);
    v.into_iter().map(|(_, l)| l.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lists_unopened_launches_soonest_first() {
        let now = rare_market::time::parse_iso_ms("2026-10-26T00:00:00Z").unwrap();
        let list: Vec<Launch> = serde_json::from_value(json!([
            {"slug": "past", "name": "Past", "startsAt": "2026-10-01T00:00:00Z"},
            {"slug": "late", "name": "Late", "startsAt": now + 9_000_000},
            {"slug": "soon", "name": "Soon", "phases": [{"startsAt": now + 1_000_000}]},
            {"slug": "none", "name": "None"}
        ]))
        .unwrap();
        assert_eq!(upcoming_launches(&list, now).iter().map(|l| l.slug.as_str()).collect::<Vec<_>>(), ["soon", "late"]);
    }
}
