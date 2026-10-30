//! Ask Rare: the owner's own model drives Rare through a fixed set of tools.
//!
//! - [`provider`]: the models the owner brings (Claude, OpenAI, any OpenAI-compatible endpoint,
//!   Ollama), each a streaming chat with function calling and pictures. Rare registers no app with
//!   any provider and pays for nothing: the owner's key (or a local Ollama) is all there is.
//! - [`tools`]: what the model may ask for, with the Local API switch each tool needs for network
//!   apps (MCP) and whether it only prepares a review sheet. Nothing here signs anything.
//! - [`agent`]: the loop (ask, run the tools the model calls, feed the results back, stop).
//! - [`mcp`]: the same tools over MCP (JSON-RPC), for the Local API's network listener.
//! - [`voice`]: push to talk: PipeWire records, whisper.cpp transcribes on the CPU, its model
//!   downloaded once on first use.
//! - [`mock`]: a scripted model for tests.

pub mod agent;
pub mod mcp;
pub mod mock;
pub mod provider;
pub mod sse;
pub mod tools;
pub mod voice;

use serde_json::Value;

/// Part of something the player said: words, or a picture of the game (base64).
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Image { media: String, data: String },
}

/// A tool the model asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// One turn of the conversation, provider neutral.
#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    User(Vec<Part>),
    /// The model's turn. `raw` is the provider's own content when it must go back unchanged
    /// (Claude's thinking blocks).
    Assistant { text: String, calls: Vec<Call>, raw: Option<Value> },
    /// A tool's answer for the model.
    Tool { id: String, name: String, out: String, error: bool },
}

impl Msg {
    pub fn user(text: &str) -> Msg {
        Msg::User(vec![Part::Text(text.to_string())])
    }
}

/// Why a model's turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Done,
    Tools,
    Length,
    Refused,
}

/// A finished model turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<Call>,
    pub raw: Option<Value>,
    pub stop: Stop,
}

/// What a model call needs.
pub struct Request<'a> {
    pub system: &'a str,
    pub msgs: &'a [Msg],
    pub tools: &'a [tools::Tool],
}

/// A chat model with function calling. `on_text` gets the answer's words as they stream.
pub trait Model: Send + Sync {
    fn turn(&self, req: &Request, on_text: &mut dyn FnMut(&str), cancel: &std::sync::atomic::AtomicBool) -> Result<Reply, String>;
}

/// Pictures only on the newest user turn: older ones are replaced by a note (they cost a lot of
/// tokens and the model has already seen them).
pub fn trim_images(msgs: &[Msg]) -> Vec<Msg> {
    let last_user = msgs.iter().rposition(|m| matches!(m, Msg::User(_)));
    msgs.iter()
        .enumerate()
        .map(|(i, m)| match m {
            Msg::User(parts) if Some(i) != last_user && parts.iter().any(|p| matches!(p, Part::Image { .. })) => {
                Msg::User(parts.iter().map(|p| if let Part::Image { .. } = p { Part::Text("(a screenshot of the game was here)".into()) } else { p.clone() }).collect())
            }
            m => m.clone(),
        })
        .collect()
}

/// An error sentence without the key in it (some providers echo part of a bad key back).
pub fn redact(msg: &str, key: Option<&str>) -> String {
    let mut s = msg.to_string();
    if let Some(k) = key.filter(|k| k.len() >= 6) {
        s = s.replace(k, "[key]");
    }
    // Anything that looks like a key: sk-..., sk-ant-..., long runs after "key".
    let mut out = String::with_capacity(s.len());
    for w in s.split_inclusive(char::is_whitespace) {
        let t = w.trim_end();
        if (t.starts_with("sk-") || t.starts_with("sk_")) && t.len() > 8 {
            out.push_str("[key]");
            out.push_str(&w[t.len()..]);
        } else {
            out.push_str(w);
        }
    }
    out.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_keys() {
        assert_eq!(redact("Incorrect API key provided: sk-abcdefghijkl. Find it", None), "Incorrect API key provided: [key] Find it");
        assert_eq!(redact("bad key secret123456 here", Some("secret123456")), "bad key [key] here");
        assert_eq!(redact("plain words", Some("k")), "plain words");
    }

    #[test]
    fn only_the_newest_picture_is_sent() {
        let img = || Part::Image { media: "image/jpeg".into(), data: "AAAA".into() };
        let msgs = vec![Msg::User(vec![Part::Text("a".into()), img()]), Msg::Assistant { text: "x".into(), calls: vec![], raw: None }, Msg::User(vec![Part::Text("b".into()), img()])];
        let t = trim_images(&msgs);
        assert!(matches!(&t[0], Msg::User(p) if !p.iter().any(|x| matches!(x, Part::Image { .. }))));
        assert!(matches!(&t[2], Msg::User(p) if p.iter().any(|x| matches!(x, Part::Image { .. }))));
    }
}
