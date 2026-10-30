//! The Sui Games catalog (lib/rare.ts loadGames): rare.market's /v1/games, live, else the copy the
//! web launcher bundles (lib/games.seed.json). Only live and beta games show as games
//! (games/catalog.ts isActive).

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub symbol: String,
    pub coin_type: String,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    #[serde(default)]
    pub nft_type: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuiGame {
    pub slug: String,
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub studio: String,
    #[serde(default, deserialize_with = "nullable")]
    pub genre: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub logo_url: String,
    #[serde(default, deserialize_with = "nullable")]
    pub banner_url: String,
    #[serde(default)]
    pub token: Option<Token>,
    #[serde(default)]
    pub featured: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub description: String,
    /// Where the game plays in a browser (web games), and its site.
    #[serde(default, deserialize_with = "nullable")]
    pub play_url: String,
    #[serde(default, deserialize_with = "nullable")]
    pub website: String,
    /// The game's tradeable collections on rare.market (its Items and Market tabs).
    #[serde(default, deserialize_with = "nullable_vec")]
    pub collections: Vec<Collection>,
}

fn nullable_vec<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Collection>, D::Error> {
    Ok(Option::<Vec<Collection>>::deserialize(d)?.unwrap_or_default())
}

/// null reads as empty (the catalog has null links).
fn nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

impl SuiGame {
    /// live and beta are shown as games; everything else waits in "Coming".
    pub fn active(&self) -> bool {
        matches!(self.status.as_deref(), Some("live") | Some("beta"))
    }

    /// "Live · Web, PC" (SuiGames.tsx statusLine).
    pub fn status_line(&self) -> String {
        let status = match self.status.as_deref() {
            Some("live") => "Live",
            Some("beta") => "Beta",
            Some("coming") => "Coming soon",
            Some("announced") => "Announced",
            Some("paused") => "Paused",
            Some("dead") => "Offline",
            Some(s) => s,
            None => "",
        };
        let plat = self
            .platforms
            .iter()
            .map(|p| match p.as_str() {
                "web" => "Web",
                "pc" => "PC",
                "mobile" => "Mobile",
                "suiplay" => "SuiPlay",
                p => p,
            })
            .collect::<Vec<_>>()
            .join(", ");
        [status.to_string(), plat].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" \u{b7} ")
    }
}

static SEED: &str = include_str!("../../../src/lib/games.seed.json");

/// The bundled copy.
pub fn seed() -> Vec<SuiGame> {
    serde_json::from_str(SEED).unwrap_or_default()
}

/// rare.market's catalog, else the bundled copy. Blocking: call from a worker.
pub fn load() -> Vec<SuiGame> {
    let live = (|| -> Option<Vec<SuiGame>> {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(8))).build().into();
        let v: serde_json::Value = agent.get("https://rare.market/api/v1/games").header("User-Agent", "RareLauncher/0.1").call().ok()?.body_mut().read_json().ok()?;
        let games: Vec<SuiGame> = serde_json::from_value(v.get("data")?.clone()).ok()?;
        (!games.is_empty()).then_some(games)
    })();
    live.unwrap_or_else(seed)
}

#[cfg(test)]
mod tests {
    #[test]
    fn seed_parses() {
        let s = super::seed();
        assert!(s.len() > 10);
        assert_eq!(s[0].slug, "act");
        assert_eq!(s[0].status_line(), "Live \u{b7} Web");
    }
}
