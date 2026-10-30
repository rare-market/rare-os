//! Settings > Ask Rare: the model the owner brings (Claude, OpenAI, any OpenAI-compatible service,
//! Ollama), its model name and address, the API key (sealed in Rare Wallet's vault, never shown or
//! written to launcher.json), push to talk, RC's hold, and the default for pictures of the game.

use super::config::Config;
use super::rows::*;
use super::{SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_agent::provider::Kind;
use rare_render::Id;

pub const PROVIDERS: [(Option<Kind>, &str); 5] = [(None, "Off"), (Some(Kind::Claude), "Claude"), (Some(Kind::OpenAi), "OpenAI"), (Some(Kind::Compatible), "Other"), (Some(Kind::Ollama), "Ollama")];

/// An address the owner typed: http(s) only, no trailing slash. Some(None) clears it.
pub fn clean_url(v: &str) -> Option<Option<String>> {
    let v = v.trim();
    if v.is_empty() {
        return Some(None);
    }
    let v = if !v.contains("://") { format!("http://{v}") } else { v.to_string() };
    (v.starts_with("http://") || v.starts_with("https://")).then(|| Some(v.trim_end_matches('/').to_string()))
}

pub struct AskSection {
    rows: RowSet,
}

fn kind_of(cfg: &Config) -> Option<Kind> {
    cfg.agent_provider.as_deref().and_then(Kind::from_id)
}

impl AskSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, _now: f64) -> AskSection {
        let mut rows = RowSet::default();
        let kind = kind_of(cfg);
        let p = panel(cx, parent);
        setting_row(cx, p, "Ask Rare", Some("Talk or type in the Rare menu's Ask tab, even over a game. Rare uses a model you bring and pays nothing. Your key is sealed in Rare Wallet and goes only to the provider you pick."), true);
        let idx = PROVIDERS.iter().position(|(k, _)| *k == kind).unwrap_or(0);
        let labels: Vec<&str> = PROVIDERS.iter().map(|(_, l)| *l).collect();
        let body = match kind {
            None => "Pick where the answers come from.",
            Some(Kind::Claude) => "Your Anthropic API key. Rare never registers with Anthropic.",
            Some(Kind::OpenAi) => "Your OpenAI API key. Sign in with ChatGPT is not offered.",
            Some(Kind::Compatible) => "Any service that speaks the OpenAI API: a router or your own server.",
            Some(Kind::Ollama) => "Ollama on this device or a PC on your network. Works offline.",
        };
        rows.choice(cx, nav, p, "ask-provider", "Model provider", Some(body), &labels, Some(idx), false);
        if let Some(k) = kind {
            let model = cfg.agent_model.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| if k.default_model().is_empty() { "Not set".into() } else { format!("{} (default)", k.default_model()) });
            let r = setting_row(cx, p, "Model", Some(&model), false);
            super::remote::small_button(cx, nav, r.right, "Change", "ask-model", 0);
            if k.takes_url() {
                let url = cfg.agent_url.clone().filter(|u| !u.is_empty()).unwrap_or_else(|| if k.default_url().is_empty() { "Not set".into() } else { format!("{} (default)", k.default_url()) });
                let r = setting_row(cx, p, "Address", Some(&url), false);
                super::remote::small_button(cx, nav, r.right, "Change", "ask-url", 0);
            }
            if k.takes_key() {
                let saved = crate::features::agent::setup().keys.iter().any(|x| x == k.id());
                let line = match (saved, k.needs_key()) {
                    (true, _) => "Saved in Rare Wallet",
                    (false, true) => "Not set",
                    (false, false) => "Not set (only if your service needs one)",
                };
                let r = setting_row(cx, p, "API key", Some(line), false);
                super::remote::small_button(cx, nav, r.right, if saved { "Replace" } else { "Add" }, "ask-key", 0);
                if saved {
                    super::remote::small_button(cx, nav, r.right, "Remove", "ask-key-remove", 0);
                }
            }
            let r = setting_row(cx, p, "Check it works", Some("Sends one short question to your model."), false);
            super::remote::small_button(cx, nav, r.right, "Test", "ask-test", 0);
        }
        let p = panel(cx, parent);
        rows.switch(cx, nav, p, "ask-voice", "Speak to Rare", "Hold X on the Ask tab and talk. Speech turns into text on this device; the speech model (190 MB) downloads the first time.", cfg.agent_voice == Some(true), true);
        rows.switch(cx, nav, p, "ask-hold", "Hold RC to talk", "Hold RC anywhere, even in a game, and let go to send. RC's own hold action is off while this is on.", cfg.agent_hold == Some(true), false);
        let p = panel(cx, parent);
        rows.switch(cx, nav, p, "ask-shots", "Pictures of the game", "Over a game, a screenshot can go to your model with the question, for help with what is on screen. This is the default; the Ask tab has a switch each time.", cfg.agent_shots == Some(true), true);
        setting_row(cx, p, "Agents on your network", Some("Tools on a laptop can use the same actions over MCP: Developer > Allow network apps, one switch per kind of action. Buying and sending always wait for you here."), false);
        AskSection { rows }
    }
}

impl SectionView for AskSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, cfg: &mut Config, now: f64) -> SectionOut {
        let Some(f) = nav.current().cloned() else { return SectionOut::None };
        let act = |a: &str| SectionOut::Act(a.into());
        match f.sid.as_str() {
            "ask-provider" => {
                let Some((_, Val::Idx(i))) = self.rows.press(cx, nav, now) else { return SectionOut::None };
                let k = PROVIDERS.get(i).and_then(|p| p.0);
                cfg.agent_provider = k.map(|k| k.id().to_string());
                // Another provider: its own model and address defaults.
                cfg.agent_model = None;
                cfg.agent_url = None;
                // The rows under it change: draw the section again after the save.
                crate::screens::settings::live::touch();
                SectionOut::Changed
            }
            "ask-voice" | "ask-hold" | "ask-shots" => {
                let Some((sid, Val::Bool(on))) = self.rows.press(cx, nav, now) else { return SectionOut::None };
                match sid.as_str() {
                    "ask-voice" => cfg.agent_voice = Some(on),
                    "ask-hold" => cfg.agent_hold = Some(on),
                    _ => cfg.agent_shots = Some(on),
                }
                if sid == "ask-voice" && on {
                    // Fetch the speech model now, not on the first hold.
                    crate::features::agent::download_model();
                }
                SectionOut::Changed
            }
            "ask-model" => act("ask:model"),
            "ask-url" => act("ask:url"),
            "ask-key" => act("ask:key"),
            "ask-key-remove" => act("ask:key-remove"),
            "ask-test" => act("ask:test"),
            _ => SectionOut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(clean_url("pc.local:11434"), Some(Some("http://pc.local:11434".into())));
        assert_eq!(clean_url("https://api.example.com/v1/"), Some(Some("https://api.example.com/v1".into())));
        assert_eq!(clean_url(""), Some(None));
        assert_eq!(clean_url("ftp://x"), None);
    }
}
