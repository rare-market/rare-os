//! The loop: the model answers or asks for tools; Rare runs them (through the launcher's own flows)
//! and hands the results back, until the model answers without a tool or the step limit is hit.

use crate::tools::{self, Tool};
use crate::{Call, Model, Msg, Request, Stop};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// Tool rounds per question.
pub const MAX_ROUNDS: usize = 8;
/// What a tool's answer may cost the model, in characters.
pub const MAX_RESULT: usize = 6000;
/// Turns kept for the next question (older ones are dropped at a question boundary).
pub const KEEP_MSGS: usize = 40;

/// What the panel shows as it happens.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Words of the answer as they stream.
    Text(String),
    /// A tool started: (call id, tool name, the line to show).
    ToolStart { id: String, name: String, label: String },
    /// It finished: ok, and a short line for the panel.
    ToolDone { id: String, ok: bool, summary: String },
    /// A new model turn starts after tools (the panel starts a new answer block).
    Turn,
    /// The last answer only described a tool; the model is asked again (the panel drops it).
    Redo,
}

/// A tool's answer: `data` goes to the model, `summary` to the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub ok: bool,
    pub data: Value,
    pub summary: String,
}

impl Outcome {
    pub fn ok(summary: impl Into<String>, data: Value) -> Outcome {
        Outcome { ok: true, data, summary: summary.into() }
    }
    pub fn err(msg: impl Into<String>) -> Outcome {
        let m = msg.into();
        Outcome { ok: false, data: json!({ "error": m }), summary: m }
    }
}

/// Runs a tool for the model (the launcher: on its UI thread, through its own flows).
pub trait Exec: Send + Sync {
    fn run(&self, call: &Call) -> Outcome;
}

fn clip(s: String) -> String {
    if s.len() <= MAX_RESULT {
        return s;
    }
    let mut end = MAX_RESULT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... (cut)", &s[..end])
}

/// Drops the oldest questions (whole: a question, its tool calls and results) beyond `keep`.
pub fn trim_history(h: &mut Vec<Msg>, keep: usize) {
    while h.len() > keep {
        // The next question after the first message starts the part to keep.
        let Some(cut) = h.iter().skip(1).position(|m| matches!(m, Msg::User(_))).map(|i| i + 1) else { break };
        h.drain(..cut);
    }
}

/// Rare's instructions for the model: what it can do, the review rule, the small screen, and now.
pub fn system_prompt(game: Option<&str>, account: Option<&str>, now: &str) -> String {
    format!(
        "You are Rare, the assistant built into Rare, a handheld gaming system. You act through tools: find and start games, open Rare's install and uninstall sheets, change device settings, check downloads and storage, read the player's Rare Wallet and Sui game items, prepare purchases, sales, listings, sends and swaps, and talk to friends.\n\
Rules:\n\
- Do the whole request with the tools. Never tell the player to do a step a tool can do: call the tool. To start a game, call search_games and then launch_game with the id it returned.\n\
- wallet_send, wallet_swap, market_buy, market_sell and market_list only open a review sheet. The player checks it and confirms on the device. Never say something was bought, sold, listed, sent or swapped; say the review sheet is open.\n\
- Get ids from search_games, my_items or market_search first. Never invent ids, amounts or addresses.\n\
- If the request is unclear, or could spend money in a way the player may not expect, ask one short question.\n\
- Answers are read on a small screen: one to three short sentences in plain words. No markdown headings, tables or long lists.\n\
- For help with the game being played, answer from what you know about it and from the screenshot when one is attached. Be concrete: what to do, in order.\n\
Now: {now}. In front: {}. Rare Wallet account: {}.",
        game.unwrap_or("the launcher, no game"),
        account.unwrap_or("none")
    )
}

/// The answer talks about a tool instead of calling it (small local models do): worth one nudge.
fn describes_a_tool(text: &str, tools: &[Tool]) -> bool {
    let t = text.to_lowercase();
    tools.iter().any(|x| t.contains(x.name)) || ["you can launch", "you can now launch", "you can start it", "launch it now", "start it now"].iter().any(|p| t.contains(p))
}

pub const NUDGE: &str = "(Rare) Do that now with the tools instead of describing it.";

/// Answers the newest user message in `history` (which it extends). Err is a sentence for the panel.
pub fn run(model: &dyn Model, system: &str, history: &mut Vec<Msg>, tools: &[Tool], exec: &dyn Exec, emit: &mut dyn FnMut(Event), cancel: &AtomicBool) -> Result<(), String> {
    trim_history(history, KEEP_MSGS);
    let mut nudged = false;
    // An action asked for again with the same arguments in one question runs once.
    let mut done: Vec<(String, String)> = Vec::new();
    for round in 0..=MAX_ROUNDS {
        if cancel.load(Ordering::SeqCst) {
            return Err(crate::sse::CANCELLED.into());
        }
        if round > 0 {
            emit(Event::Turn);
        }
        // The last round may not call tools: it has to answer.
        let offered: &[Tool] = if round == MAX_ROUNDS { &[] } else { tools };
        let reply = {
            let req = Request { system, msgs: history, tools: offered };
            model.turn(&req, &mut |t| emit(Event::Text(t.to_string())), cancel)?
        };
        let calls = reply.calls.clone();
        let refused = reply.stop == Stop::Refused;
        history.push(Msg::Assistant { text: reply.text.clone(), calls: calls.clone(), raw: reply.raw });
        if refused && reply.text.trim().is_empty() {
            emit(Event::Text("I can't help with that one.".into()));
            return Ok(());
        }
        if calls.is_empty() && !nudged && round < MAX_ROUNDS && !offered.is_empty() && describes_a_tool(&reply.text, tools) {
            nudged = true;
            emit(Event::Redo);
            history.push(Msg::user(NUDGE));
            continue;
        }
        if calls.is_empty() {
            if reply.stop == Stop::Length {
                emit(Event::Text(" (The answer was cut short.)".into()));
            }
            return Ok(());
        }
        for c in &calls {
            if cancel.load(Ordering::SeqCst) {
                return Err(crate::sse::CANCELLED.into());
            }
            let tool = tools.iter().find(|t| t.name == c.name);
            emit(Event::ToolStart { id: c.id.clone(), name: c.name.clone(), label: tools::label(&c.name, &c.args) });
            let out = match tool {
                None => Outcome::err(format!("There is no tool called {}.", c.name)),
                Some(t) => match tools::check_args(t, &c.args) {
                    Err(e) => Outcome::err(e),
                    Ok(()) if !crate::mcp::read_only(t.name) && done.contains(&(c.name.clone(), c.args.to_string())) => Outcome::ok("Already done", json!({ "alreadyDone": true, "note": "This was done a moment ago. Answer the player now." })),
                    Ok(()) => {
                        done.push((c.name.clone(), c.args.to_string()));
                        exec.run(c)
                    }
                },
            };
            emit(Event::ToolDone { id: c.id.clone(), ok: out.ok, summary: out.summary.clone() });
            history.push(Msg::Tool { id: c.id.clone(), name: c.name.clone(), out: clip(out.data.to_string()), error: !out.ok });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{Mock, Step};
    use std::sync::Mutex;

    struct Rec(Mutex<Vec<Call>>);
    impl Exec for Rec {
        fn run(&self, call: &Call) -> Outcome {
            self.0.lock().unwrap().push(call.clone());
            match call.name.as_str() {
                "search_games" => Outcome::ok("2 games", json!({ "games": [{ "id": "steam:1145360", "title": "Hades", "installed": true }] })),
                "launch_game" => Outcome::ok("Started Hades", json!({ "ok": true })),
                _ => Outcome::err("no"),
            }
        }
    }

    #[test]
    fn searches_launches_and_answers() {
        let m = Mock::new(vec![
            Step::calls("Looking.", vec![("search_games", json!({ "query": "hades" }))]),
            Step::calls("", vec![("launch_game", json!({ "id": "steam:1145360" }))]),
            Step::text("Hades is starting."),
        ]);
        let exec = Rec(Mutex::new(vec![]));
        let mut h = vec![Msg::user("play hades")];
        let mut ev = vec![];
        run(&m, "sys", &mut h, &tools::catalog(), &exec, &mut |e| ev.push(e), &AtomicBool::new(false)).unwrap();
        let calls = exec.0.lock().unwrap();
        assert_eq!(calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["search_games", "launch_game"]);
        assert!(ev.contains(&Event::ToolDone { id: "call_1_0".into(), ok: true, summary: "Started Hades".into() }));
        assert!(matches!(h.last(), Some(Msg::Assistant { text, .. }) if text == "Hades is starting."));
        // The model saw each tool's result.
        let seen = m.seen();
        assert!(matches!(seen[1].last(), Some(Msg::Tool { name, .. }) if name == "search_games"));
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn bad_arguments_and_unknown_tools_go_back_as_errors() {
        let m = Mock::new(vec![Step::calls("", vec![("change_setting", json!({ "setting": "fan" })), ("fly", json!({}))]), Step::text("Sorry.")]);
        let exec = Rec(Mutex::new(vec![]));
        let mut h = vec![Msg::user("x")];
        run(&m, "sys", &mut h, &tools::catalog(), &exec, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert!(exec.0.lock().unwrap().is_empty());
        let errs: Vec<_> = h.iter().filter_map(|m| if let Msg::Tool { out, error: true, .. } = m { Some(out.clone()) } else { None }).collect();
        assert_eq!(errs.len(), 2);
        assert!(errs[1].contains("no tool called fly"));
    }

    #[test]
    fn stops_after_the_round_limit_with_an_answer() {
        let mut steps: Vec<Step> = (0..MAX_ROUNDS).map(|_| Step::calls("", vec![("search_games", json!({ "query": "a" }))])).collect();
        steps.push(Step::text("Done looking."));
        let m = Mock::new(steps);
        let exec = Rec(Mutex::new(vec![]));
        let mut h = vec![Msg::user("x")];
        run(&m, "sys", &mut h, &tools::catalog(), &exec, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(exec.0.lock().unwrap().len(), MAX_ROUNDS);
        // The last turn was offered no tools.
        assert_eq!(m.tools_offered().last(), Some(&0));
    }

    #[test]
    fn a_described_tool_gets_one_nudge() {
        let m = Mock::new(vec![Step::text("You can launch it using the launch_game function."), Step::calls("", vec![("launch_game", json!({ "id": "steam:1145360" }))]), Step::text("Starting.")]);
        let exec = Rec(Mutex::new(vec![]));
        let mut h = vec![Msg::user("play hades")];
        run(&m, "sys", &mut h, &tools::catalog(), &exec, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(exec.0.lock().unwrap().len(), 1);
        assert!(h.iter().any(|x| *x == Msg::user(NUDGE)));
    }

    #[test]
    fn the_same_action_twice_runs_once() {
        let m = Mock::new(vec![Step::calls("", vec![("launch_game", json!({ "id": "steam:1145360" })), ("launch_game", json!({ "id": "steam:1145360" }))]), Step::text("Starting.")]);
        let exec = Rec(Mutex::new(vec![]));
        let mut h = vec![Msg::user("play hades")];
        run(&m, "sys", &mut h, &tools::catalog(), &exec, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(exec.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancel_stops_it() {
        let m = Mock::new(vec![Step::text("x")]);
        let exec = Rec(Mutex::new(vec![]));
        let mut h = vec![Msg::user("x")];
        assert!(run(&m, "sys", &mut h, &tools::catalog(), &exec, &mut |_| {}, &AtomicBool::new(true)).is_err());
    }

    #[test]
    fn history_is_trimmed_at_question_boundaries() {
        let mut h = vec![];
        for i in 0..30 {
            h.push(Msg::user(&format!("q{i}")));
            h.push(Msg::Assistant { text: "a".into(), calls: vec![], raw: None });
        }
        trim_history(&mut h, 10);
        assert!(h.len() <= 10);
        assert!(matches!(&h[0], Msg::User(_)));
    }
}

/// The loop against a real Ollama (the build machine): RARE_OLLAMA_URL=http://127.0.0.1:11434
/// RARE_OLLAMA_MODEL=qwen2.5:1.5b cargo test -p rare-agent --release ollama -- --ignored --nocapture
#[cfg(test)]
mod live {
    use super::*;
    use crate::provider::{Config, Kind};
    use std::sync::Mutex;

    struct Fake(Mutex<Vec<Call>>);
    impl Exec for Fake {
        fn run(&self, call: &Call) -> Outcome {
            self.0.lock().unwrap().push(call.clone());
            match call.name.as_str() {
                "search_games" => Outcome::ok("1 match", json!({ "library": [{ "id": "steam:1145360", "title": "Hades", "installed": true, "running": false }], "apps": [], "rareStore": [] })),
                "launch_game" if call.args["id"] == "steam:1145360" => Outcome::ok("Starting Hades", json!({ "ok": true, "title": "Hades" })),
                "launch_game" => Outcome::err("That id isn't in the library. Use search_games to get the id."),
                "change_setting" => Outcome::ok("Done", json!({ "ok": true })),
                _ => Outcome::err("Not in this test."),
            }
        }
    }

    fn ask(q: &str) -> (Vec<Call>, String) {
        let url = std::env::var("RARE_OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".into());
        let model = std::env::var("RARE_OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5:1.5b".into());
        let cfg = Config::new(Kind::Ollama, Some(&model), Some(&url), None);
        let m = cfg.model();
        let exec = Fake(Mutex::new(vec![]));
        let mut h = vec![Msg::user(q)];
        let mut text = String::new();
        let sys = system_prompt(None, None, "2026-10-28 09:00 UTC");
        let sys = sys.as_str();
        let t0 = std::time::Instant::now();
        run(m.as_ref(), sys, &mut h, &tools::catalog(), &exec, &mut |e| {
            if let Event::Text(t) = e {
                text.push_str(&t);
            }
        }, &AtomicBool::new(false))
        .unwrap();
        let calls = exec.0.lock().unwrap().clone();
        eprintln!("{q:?} in {:?}: calls {:?} answer {text:?}", t0.elapsed(), calls.iter().map(|c| format!("{} {}", c.name, c.args)).collect::<Vec<_>>());
        (calls, text)
    }

    #[test]
    #[ignore]
    fn ollama_starts_a_game_and_changes_a_setting() {
        let (calls, text) = ask("Start Hades");
        assert!(calls.iter().any(|c| c.name == "launch_game" && c.args["id"] == "steam:1145360"), "{calls:?}");
        assert!(!text.trim().is_empty());
        let (calls, _) = ask("Set the brightness to 40");
        assert!(calls.iter().any(|c| c.name == "change_setting" && c.args["setting"] == "brightness"), "{calls:?}");
    }
}
