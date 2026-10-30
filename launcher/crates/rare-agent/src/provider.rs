//! The models the owner brings. Each is plain HTTPS (or HTTP to a local Ollama) with the owner's
//! own key: Rare registers nothing with any provider and nothing passes through Rare's servers.
//!
//! - Claude: the Messages API, streamed, with tool use and pictures.
//! - OpenAI and any OpenAI-compatible endpoint: Chat Completions, streamed, with tool calls.
//! - Ollama: its own /api/chat (newline-delimited JSON), on this device or a PC on the network.

use crate::sse::{events, ndjson};
use crate::{redact, trim_images, Call, Model, Msg, Part, Reply, Request, Stop};
use serde_json::{json, Map, Value};
use std::io::BufReader;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Claude,
    OpenAi,
    /// Any OpenAI-compatible endpoint (a router, a self-hosted server).
    Compatible,
    Ollama,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Claude => "claude",
            Kind::OpenAi => "openai",
            Kind::Compatible => "compatible",
            Kind::Ollama => "ollama",
        }
    }
    pub fn from_id(s: &str) -> Option<Kind> {
        Some(match s {
            "claude" => Kind::Claude,
            "openai" => Kind::OpenAi,
            "compatible" => Kind::Compatible,
            "ollama" => Kind::Ollama,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Claude => "Claude",
            Kind::OpenAi => "OpenAI",
            Kind::Compatible => "Other",
            Kind::Ollama => "Ollama",
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            Kind::Claude => "claude-opus-5-5",
            Kind::OpenAi => "gpt-5",
            Kind::Compatible => "",
            Kind::Ollama => "qwen2.5:3b",
        }
    }
    pub fn default_url(self) -> &'static str {
        match self {
            Kind::Claude => "https://api.anthropic.com",
            Kind::OpenAi => "https://api.openai.com/v1",
            Kind::Compatible => "",
            Kind::Ollama => "http://127.0.0.1:11434",
        }
    }
    /// Needs an API key (Compatible may or may not; Ollama never).
    pub fn needs_key(self) -> bool {
        matches!(self, Kind::Claude | Kind::OpenAi)
    }
    pub fn takes_key(self) -> bool {
        !matches!(self, Kind::Ollama)
    }
    pub fn takes_url(self) -> bool {
        matches!(self, Kind::Compatible | Kind::Ollama)
    }
}

#[derive(Clone)]
pub struct Config {
    pub kind: Kind,
    pub model: String,
    pub url: String,
    pub key: Option<String>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key.
        write!(f, "Config({}, {}, {}, key: {})", self.kind.id(), self.model, self.url, if self.key.is_some() { "set" } else { "none" })
    }
}

impl Config {
    pub fn new(kind: Kind, model: Option<&str>, url: Option<&str>, key: Option<String>) -> Config {
        let model = model.map(str::trim).filter(|m| !m.is_empty()).unwrap_or(kind.default_model()).to_string();
        let url = url.map(str::trim).filter(|u| !u.is_empty()).unwrap_or(kind.default_url()).trim_end_matches('/').to_string();
        Config { kind, model, url, key: key.filter(|k| !k.trim().is_empty()) }
    }

    /// What is missing before it can answer, as a sentence for the panel.
    pub fn missing(&self) -> Option<&'static str> {
        if self.kind.needs_key() && self.key.is_none() {
            return Some("Add your API key in Settings > Ask Rare.");
        }
        if self.url.is_empty() {
            return Some("Add the address of your model in Settings > Ask Rare.");
        }
        if self.model.is_empty() {
            return Some("Choose a model in Settings > Ask Rare.");
        }
        None
    }

    pub fn model(&self) -> Box<dyn Model> {
        match self.kind {
            Kind::Claude => Box::new(Claude(self.clone())),
            Kind::OpenAi | Kind::Compatible => Box::new(OpenAi(self.clone())),
            Kind::Ollama => Box::new(Ollama(self.clone())),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(120)).build()
}

/// A failed call as a sentence: the provider's own message when it gave one, never the key.
fn http_error(e: ureq::Error, key: Option<&str>, what: &str) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let msg = v["error"]["message"].as_str().or(v["error"].as_str()).or(v["message"].as_str()).map(str::to_string).unwrap_or_else(|| body.chars().take(160).collect());
            let lead = match code {
                401 | 403 => "The key was refused".to_string(),
                404 => format!("{what} doesn't know that model or address"),
                429 => "Too many requests or no credit left".to_string(),
                c if c >= 500 => format!("{what} is having trouble"),
                c => format!("{what} answered {c}"),
            };
            redact(&if msg.trim().is_empty() { format!("{lead}.") } else { format!("{lead}: {}", msg.trim()) }, key)
        }
        ureq::Error::Transport(t) => redact(&format!("Couldn't reach {what}: {}", t.kind()), key),
    }
}

// ---------------------------------------------------------------- Claude (Messages API)

pub struct Claude(pub Config);

/// Models that take `output_config.effort` and the server-side refusal fallback.
fn claude_current(model: &str) -> bool {
    ["claude-opus-5", "claude-sonnet-5", "claude-fable-5", "claude-haiku-5", "claude-mythos-5"].iter().any(|p| model.starts_with(p))
}

fn claude_fallbacks(model: &str) -> bool {
    ["claude-opus-5-5", "claude-opus-5", "claude-fable-5-1", "claude-sonnet-5-5"].contains(&model)
}

pub fn claude_body(cfg: &Config, req: &Request) -> Value {
    let mut msgs: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    let flush = |msgs: &mut Vec<Value>, results: &mut Vec<Value>| {
        if !results.is_empty() {
            msgs.push(json!({ "role": "user", "content": std::mem::take(results) }));
        }
    };
    for m in trim_images(req.msgs) {
        match m {
            Msg::Tool { id, out, error, .. } => results.push(json!({ "type": "tool_result", "tool_use_id": id, "content": out, "is_error": error })),
            Msg::User(parts) => {
                flush(&mut msgs, &mut results);
                let content: Vec<Value> = parts
                    .iter()
                    .map(|p| match p {
                        Part::Text(t) => json!({ "type": "text", "text": t }),
                        Part::Image { media, data } => json!({ "type": "image", "source": { "type": "base64", "media_type": media, "data": data } }),
                    })
                    .collect();
                msgs.push(json!({ "role": "user", "content": content }));
            }
            Msg::Assistant { text, calls, raw } => {
                flush(&mut msgs, &mut results);
                let content = match raw {
                    Some(r) => r,
                    None => {
                        let mut c: Vec<Value> = Vec::new();
                        if !text.is_empty() {
                            c.push(json!({ "type": "text", "text": text }));
                        }
                        for call in calls {
                            c.push(json!({ "type": "tool_use", "id": call.id, "name": call.name, "input": call.args }));
                        }
                        Value::Array(c)
                    }
                };
                msgs.push(json!({ "role": "assistant", "content": content }));
            }
        }
    }
    flush(&mut msgs, &mut results);
    let tools: Vec<Value> = req.tools.iter().map(|t| json!({ "name": t.name, "description": t.about, "input_schema": t.schema })).collect();
    let mut body = json!({ "model": cfg.model, "max_tokens": 8192, "stream": true, "system": req.system, "messages": msgs });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if claude_current(&cfg.model) {
        // A handheld conversation: short answers quickly.
        body["output_config"] = json!({ "effort": "low" });
    }
    if claude_fallbacks(&cfg.model) {
        body["fallbacks"] = json!("default");
    }
    body
}

/// Builds the reply from the stream's events (also used by tests on recorded streams).
#[derive(Default)]
pub struct ClaudeStream {
    blocks: Vec<Value>,
    json: Vec<String>,
    stop: Option<String>,
}

impl ClaudeStream {
    /// One event; text deltas go to `on_text`. Err for an error event.
    pub fn feed(&mut self, data: &str, on_text: &mut dyn FnMut(&str)) -> Result<(), String> {
        let v: Value = serde_json::from_str(data).map_err(|_| "Claude sent something unreadable.".to_string())?;
        match v["type"].as_str().unwrap_or("") {
            "content_block_start" => {
                let i = v["index"].as_u64().unwrap_or(self.blocks.len() as u64) as usize;
                while self.blocks.len() <= i {
                    self.blocks.push(Value::Null);
                    self.json.push(String::new());
                }
                self.blocks[i] = v["content_block"].clone();
            }
            "content_block_delta" => {
                let i = v["index"].as_u64().unwrap_or(0) as usize;
                let Some(b) = self.blocks.get_mut(i) else { return Ok(()) };
                let d = &v["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or("");
                        let cur = b["text"].as_str().unwrap_or("").to_string();
                        b["text"] = Value::from(cur + t);
                        on_text(t);
                    }
                    "input_json_delta" => self.json[i].push_str(d["partial_json"].as_str().unwrap_or("")),
                    "thinking_delta" => {
                        let cur = b["thinking"].as_str().unwrap_or("").to_string();
                        b["thinking"] = Value::from(cur + d["thinking"].as_str().unwrap_or(""));
                    }
                    "signature_delta" => {
                        let cur = b["signature"].as_str().unwrap_or("").to_string();
                        b["signature"] = Value::from(cur + d["signature"].as_str().unwrap_or(""));
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = v["index"].as_u64().unwrap_or(0) as usize;
                if let Some(b) = self.blocks.get_mut(i) {
                    if b["type"] == "tool_use" {
                        let j = self.json[i].trim();
                        b["input"] = if j.is_empty() { json!({}) } else { serde_json::from_str(j).unwrap_or_else(|_| json!({ "_unreadable": j })) };
                    }
                }
            }
            "message_delta" => {
                if let Some(s) = v["delta"]["stop_reason"].as_str() {
                    self.stop = Some(s.to_string());
                }
            }
            "error" => return Err(v["error"]["message"].as_str().unwrap_or("Claude stopped with an error.").to_string()),
            _ => {}
        }
        Ok(())
    }

    pub fn finish(self) -> Reply {
        let blocks: Vec<Value> = self.blocks.into_iter().filter(|b| !b.is_null()).collect();
        let text: String = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("");
        let calls: Vec<Call> = blocks.iter().filter(|b| b["type"] == "tool_use").map(|b| Call { id: b["id"].as_str().unwrap_or("").into(), name: b["name"].as_str().unwrap_or("").into(), args: b["input"].clone() }).collect();
        let stop = match self.stop.as_deref() {
            Some("tool_use") => Stop::Tools,
            Some("max_tokens") => Stop::Length,
            Some("refusal") => Stop::Refused,
            _ if !calls.is_empty() => Stop::Tools,
            _ => Stop::Done,
        };
        Reply { text, calls, raw: Some(Value::Array(blocks)), stop }
    }
}

impl Model for Claude {
    fn turn(&self, req: &Request, on_text: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
        let c = &self.0;
        let key = c.key.as_deref().ok_or("Add your Claude API key in Settings > Ask Rare.")?;
        let body = claude_body(c, req);
        let mut r = agent().post(&format!("{}/v1/messages", c.url)).set("x-api-key", key).set("anthropic-version", "2023-06-01").set("content-type", "application/json");
        if claude_fallbacks(&c.model) {
            r = r.set("anthropic-beta", "server-side-fallback-2026-07-01");
        }
        let resp = r.send_json(body).map_err(|e| http_error(e, Some(key), "Claude"))?;
        let mut st = ClaudeStream::default();
        events(BufReader::new(resp.into_reader()), cancel, |_, data| {
            st.feed(data, on_text)?;
            Ok(true)
        })
        .map_err(|e| redact(&e, Some(key)))?;
        Ok(st.finish())
    }
}

// ---------------------------------------------------------------- OpenAI and compatible (Chat Completions)

pub struct OpenAi(pub Config);

pub fn openai_body(cfg: &Config, req: &Request) -> Value {
    let mut msgs = vec![json!({ "role": "system", "content": req.system })];
    for m in trim_images(req.msgs) {
        msgs.push(match m {
            Msg::User(parts) => {
                let only_text = parts.iter().all(|p| matches!(p, Part::Text(_)));
                if only_text {
                    json!({ "role": "user", "content": parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n") })
                } else {
                    let c: Vec<Value> = parts
                        .iter()
                        .map(|p| match p {
                            Part::Text(t) => json!({ "type": "text", "text": t }),
                            Part::Image { media, data } => json!({ "type": "image_url", "image_url": { "url": format!("data:{media};base64,{data}") } }),
                        })
                        .collect();
                    json!({ "role": "user", "content": c })
                }
            }
            Msg::Assistant { text, calls, .. } => {
                let mut a = json!({ "role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::from(text) } });
                if !calls.is_empty() {
                    a["tool_calls"] = Value::Array(calls.iter().map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.args.to_string() } })).collect());
                }
                a
            }
            Msg::Tool { id, out, .. } => json!({ "role": "tool", "tool_call_id": id, "content": out }),
        });
    }
    let mut body = json!({ "model": cfg.model, "stream": true, "messages": msgs });
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.about, "parameters": t.schema } })).collect());
    }
    body
}

#[derive(Default)]
pub struct OpenAiStream {
    text: String,
    calls: Vec<(String, String, String)>,
    finish: Option<String>,
}

impl OpenAiStream {
    /// One `data:` payload. Ok(false) at [DONE].
    pub fn feed(&mut self, data: &str, on_text: &mut dyn FnMut(&str)) -> Result<bool, String> {
        if data.trim() == "[DONE]" {
            return Ok(false);
        }
        let v: Value = serde_json::from_str(data).map_err(|_| "The model sent something unreadable.".to_string())?;
        if let Some(e) = v.get("error") {
            return Err(e["message"].as_str().or(e.as_str()).unwrap_or("The model stopped with an error.").to_string());
        }
        let Some(ch) = v["choices"].get(0) else { return Ok(true) };
        let d = &ch["delta"];
        if let Some(t) = d["content"].as_str() {
            self.text.push_str(t);
            on_text(t);
        }
        for tc in d["tool_calls"].as_array().into_iter().flatten() {
            let i = tc["index"].as_u64().unwrap_or(self.calls.len().saturating_sub(1) as u64) as usize;
            while self.calls.len() <= i {
                self.calls.push(Default::default());
            }
            let c = &mut self.calls[i];
            if let Some(id) = tc["id"].as_str() {
                c.0 = id.to_string();
            }
            if let Some(n) = tc["function"]["name"].as_str() {
                c.1.push_str(n);
            }
            if let Some(a) = tc["function"]["arguments"].as_str() {
                c.2.push_str(a);
            }
        }
        if let Some(f) = ch["finish_reason"].as_str() {
            self.finish = Some(f.to_string());
        }
        Ok(true)
    }

    pub fn finish(self) -> Reply {
        let calls: Vec<Call> = self
            .calls
            .into_iter()
            .enumerate()
            .filter(|(_, c)| !c.1.is_empty())
            .map(|(i, (id, name, args))| Call { id: if id.is_empty() { format!("call_{i}") } else { id }, name, args: if args.trim().is_empty() { json!({}) } else { serde_json::from_str(&args).unwrap_or_else(|_| json!({ "_unreadable": args })) } })
            .collect();
        let stop = match self.finish.as_deref() {
            _ if !calls.is_empty() => Stop::Tools,
            Some("length") => Stop::Length,
            Some("content_filter") => Stop::Refused,
            _ => Stop::Done,
        };
        Reply { text: self.text, calls, raw: None, stop }
    }
}

impl Model for OpenAi {
    fn turn(&self, req: &Request, on_text: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
        let c = &self.0;
        let what = if c.kind == Kind::OpenAi { "OpenAI" } else { "The model" };
        let mut r = agent().post(&format!("{}/chat/completions", c.url)).set("content-type", "application/json");
        if let Some(k) = c.key.as_deref() {
            r = r.set("authorization", &format!("Bearer {k}"));
        }
        let resp = r.send_json(openai_body(c, req)).map_err(|e| http_error(e, c.key.as_deref(), what))?;
        let mut st = OpenAiStream::default();
        events(BufReader::new(resp.into_reader()), cancel, |_, data| st.feed(data, on_text)).map_err(|e| redact(&e, c.key.as_deref()))?;
        Ok(st.finish())
    }
}

// ---------------------------------------------------------------- Ollama (/api/chat)

pub struct Ollama(pub Config);

pub fn ollama_body(cfg: &Config, req: &Request) -> Value {
    let mut msgs = vec![json!({ "role": "system", "content": req.system })];
    for m in trim_images(req.msgs) {
        msgs.push(match m {
            Msg::User(parts) => {
                let text = parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
                let images: Vec<&str> = parts.iter().filter_map(|p| if let Part::Image { data, .. } = p { Some(data.as_str()) } else { None }).collect();
                let mut u = json!({ "role": "user", "content": text });
                if !images.is_empty() {
                    u["images"] = json!(images);
                }
                u
            }
            Msg::Assistant { text, calls, .. } => {
                let mut a = json!({ "role": "assistant", "content": text });
                if !calls.is_empty() {
                    a["tool_calls"] = Value::Array(calls.iter().map(|c| json!({ "function": { "name": c.name, "arguments": c.args } })).collect());
                }
                a
            }
            Msg::Tool { name, out, .. } => json!({ "role": "tool", "content": out, "tool_name": name }),
        });
    }
    let mut body = json!({ "model": cfg.model, "stream": true, "messages": msgs });
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.about, "parameters": t.schema } })).collect());
    }
    body
}

impl Model for Ollama {
    fn turn(&self, req: &Request, on_text: &mut dyn FnMut(&str), cancel: &AtomicBool) -> Result<Reply, String> {
        let c = &self.0;
        let resp = agent().post(&format!("{}/api/chat", c.url)).set("content-type", "application/json").send_json(ollama_body(c, req)).map_err(|e| http_error(e, None, "Ollama"))?;
        let mut text = String::new();
        let mut calls = Vec::new();
        let mut done_reason = None;
        ndjson(BufReader::new(resp.into_reader()), cancel, |v| {
            if let Some(e) = v["error"].as_str() {
                return Err(format!("Ollama: {e}"));
            }
            let m = &v["message"];
            if let Some(t) = m["content"].as_str().filter(|t| !t.is_empty()) {
                text.push_str(t);
                on_text(t);
            }
            for tc in m["tool_calls"].as_array().into_iter().flatten() {
                let f = &tc["function"];
                let args = match &f["arguments"] {
                    Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
                    Value::Null => json!({}),
                    a => a.clone(),
                };
                calls.push(Call { id: format!("call_{}", calls.len()), name: f["name"].as_str().unwrap_or("").to_string(), args });
            }
            if v["done"].as_bool() == Some(true) {
                done_reason = v["done_reason"].as_str().map(str::to_string);
                return Ok(false);
            }
            Ok(true)
        })?;
        let stop = if !calls.is_empty() {
            Stop::Tools
        } else if done_reason.as_deref() == Some("length") {
            Stop::Length
        } else {
            Stop::Done
        };
        Ok(Reply { text, calls, raw: None, stop })
    }
}

/// The models a provider offers, for Settings (Ollama: what is pulled; others: their list).
pub fn list_models(cfg: &Config) -> Result<Vec<String>, String> {
    let a = agent();
    let v: Value = match cfg.kind {
        Kind::Ollama => a.get(&format!("{}/api/tags", cfg.url)).call().map_err(|e| http_error(e, None, "Ollama"))?.into_json().map_err(|e| e.to_string())?,
        Kind::Claude => {
            let key = cfg.key.as_deref().ok_or("Add your Claude API key first.")?;
            a.get(&format!("{}/v1/models?limit=100", cfg.url)).set("x-api-key", key).set("anthropic-version", "2023-06-01").call().map_err(|e| http_error(e, Some(key), "Claude"))?.into_json().map_err(|e| e.to_string())?
        }
        Kind::OpenAi | Kind::Compatible => {
            let mut r = a.get(&format!("{}/models", cfg.url));
            if let Some(k) = cfg.key.as_deref() {
                r = r.set("authorization", &format!("Bearer {k}"));
            }
            r.call().map_err(|e| http_error(e, cfg.key.as_deref(), "The model"))?.into_json().map_err(|e| e.to_string())?
        }
    };
    let list = v["models"].as_array().or(v["data"].as_array()).cloned().unwrap_or_default();
    Ok(list.iter().filter_map(|m| m["name"].as_str().or(m["id"].as_str()).map(str::to_string)).collect())
}

/// Checks that the model answers (Settings' Test button): a one-word reply.
pub fn test(cfg: &Config) -> Result<String, String> {
    if let Some(m) = cfg.missing() {
        return Err(m.into());
    }
    let msgs = [Msg::user("Answer with one word: ready")];
    let req = Request { system: "You are a connection test.", msgs: &msgs, tools: &[] };
    let r = cfg.model().turn(&req, &mut |_| {}, &AtomicBool::new(false))?;
    Ok(r.text.trim().chars().take(40).collect())
}

/// `Map` helper for callers building arguments.
pub fn args(pairs: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v.clone());
    }
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools;

    fn req_fixture() -> (Vec<Msg>, Vec<tools::Tool>) {
        let msgs = vec![
            Msg::User(vec![Part::Text("How do I beat this boss?".into()), Part::Image { media: "image/jpeg".into(), data: "QUJD".into() }]),
            Msg::Assistant { text: "Let me look.".into(), calls: vec![Call { id: "t1".into(), name: "search_games".into(), args: json!({ "query": "hades" }) }], raw: None },
            Msg::Tool { id: "t1".into(), name: "search_games".into(), out: "[]".into(), error: false },
        ];
        (msgs, tools::catalog())
    }

    #[test]
    fn claude_request_shape() {
        let (msgs, tools) = req_fixture();
        let cfg = Config::new(Kind::Claude, None, None, Some("k".into()));
        let b = claude_body(&cfg, &Request { system: "sys", msgs: &msgs, tools: &tools });
        assert_eq!(b["model"], "claude-opus-5-5");
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["output_config"]["effort"], "low");
        assert!(b.get("thinking").is_none() && b.get("temperature").is_none());
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[0]["content"][1]["type"], "image");
        assert_eq!(m[1]["content"][1]["type"], "tool_use");
        assert_eq!(m[2]["role"], "user");
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(b["tools"][0]["input_schema"]["type"], "object");
        // An older model gets neither field.
        let old = Config::new(Kind::Claude, Some("claude-haiku-4-5"), None, Some("k".into()));
        let b = claude_body(&old, &Request { system: "sys", msgs: &msgs, tools: &tools });
        assert!(b.get("fallbacks").is_none() && b.get("output_config").is_none());
    }

    #[test]
    fn claude_stream_with_thinking_text_and_a_tool() {
        let lines = [
            r#"{"type":"message_start","message":{}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Starting "}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"it."}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"tu_1","name":"launch_game","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"id\": \"ste"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"am:620\"}"}}"#,
            r#"{"type":"content_block_stop","index":2}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
            r#"{"type":"message_stop"}"#,
        ];
        let mut st = ClaudeStream::default();
        let mut seen = String::new();
        for l in lines {
            st.feed(l, &mut |t| seen.push_str(t)).unwrap();
        }
        let r = st.finish();
        assert_eq!(seen, "Starting it.");
        assert_eq!(r.text, "Starting it.");
        assert_eq!(r.stop, Stop::Tools);
        assert_eq!(r.calls, vec![Call { id: "tu_1".into(), name: "launch_game".into(), args: json!({ "id": "steam:620" }) }]);
        // The thinking block goes back unchanged.
        assert_eq!(r.raw.as_ref().unwrap()[0]["signature"], "sig");
        let mut st = ClaudeStream::default();
        assert!(st.feed(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#, &mut |_| {}).is_err());
    }

    #[test]
    fn openai_request_and_stream() {
        let (msgs, tools) = req_fixture();
        let cfg = Config::new(Kind::OpenAi, None, None, Some("k".into()));
        let b = openai_body(&cfg, &Request { system: "sys", msgs: &msgs, tools: &tools });
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m[0]["role"], "system");
        assert!(m[1]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
        assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{\"query\":\"hades\"}");
        assert_eq!(m[3]["role"], "tool");
        assert_eq!(b["tools"][0]["type"], "function");
        let mut st = OpenAiStream::default();
        let mut seen = String::new();
        for l in [
            r#"{"choices":[{"delta":{"role":"assistant","content":"Ok"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"change_setting","arguments":"{\"setting\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"volume\",\"value\":\"30\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        ] {
            assert!(st.feed(l, &mut |t| seen.push_str(t)).unwrap());
        }
        assert!(!st.feed("[DONE]", &mut |_| {}).unwrap());
        let r = st.finish();
        assert_eq!(seen, "Ok");
        assert_eq!(r.calls[0].args, json!({ "setting": "volume", "value": "30" }));
        assert_eq!(r.stop, Stop::Tools);
    }

    #[test]
    fn ollama_request_shape() {
        let (msgs, tools) = req_fixture();
        let cfg = Config::new(Kind::Ollama, None, Some("http://pc.local:11434/"), None);
        assert_eq!(cfg.url, "http://pc.local:11434");
        assert!(cfg.missing().is_none());
        let b = ollama_body(&cfg, &Request { system: "sys", msgs: &msgs, tools: &tools });
        assert_eq!(b["messages"][1]["images"][0], "QUJD");
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["arguments"]["query"], "hades");
        assert_eq!(b["messages"][3]["tool_name"], "search_games");
    }

    #[test]
    fn config_never_shows_the_key() {
        let c = Config::new(Kind::Claude, None, None, Some("sk-ant-secret".into()));
        assert!(!format!("{c:?}").contains("secret"));
        assert_eq!(Config::new(Kind::Claude, None, None, None).missing(), Some("Add your API key in Settings > Ask Rare."));
        assert_eq!(Config::new(Kind::Compatible, None, None, None).missing(), Some("Add the address of your model in Settings > Ask Rare."));
    }
}
