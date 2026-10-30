//! A scripted model: each turn plays the next step (some words, then tool calls). It records what
//! it was sent, so tests can check the loop. Also what the launcher's fixtures use for screenshots.

use crate::{Call, Model, Msg, Reply, Request, Stop};
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Step {
    pub text: String,
    pub calls: Vec<(String, Value)>,
}

impl Step {
    pub fn text(t: &str) -> Step {
        Step { text: t.into(), calls: vec![] }
    }
    pub fn calls(t: &str, calls: Vec<(&str, Value)>) -> Step {
        Step { text: t.into(), calls: calls.into_iter().map(|(n, a)| (n.to_string(), a)).collect() }
    }
}

pub struct Mock {
    steps: Mutex<Vec<Step>>,
    turn: Mutex<usize>,
    seen: Mutex<Vec<Vec<Msg>>>,
    offered: Mutex<Vec<usize>>,
}

impl Mock {
    pub fn new(steps: Vec<Step>) -> Mock {
        Mock { steps: Mutex::new(steps), turn: Mutex::new(0), seen: Mutex::new(vec![]), offered: Mutex::new(vec![]) }
    }
    /// The history each turn was sent.
    pub fn seen(&self) -> Vec<Vec<Msg>> {
        self.seen.lock().unwrap().clone()
    }
    /// How many tools each turn was offered.
    pub fn tools_offered(&self) -> Vec<usize> {
        self.offered.lock().unwrap().clone()
    }
}

impl Model for Mock {
    fn turn(&self, req: &Request, on_text: &mut dyn FnMut(&str), _cancel: &AtomicBool) -> Result<Reply, String> {
        self.seen.lock().unwrap().push(req.msgs.to_vec());
        self.offered.lock().unwrap().push(req.tools.len());
        let mut t = self.turn.lock().unwrap();
        let step = self.steps.lock().unwrap().get(*t).cloned().unwrap_or_else(|| Step::text("(no more steps)"));
        let n = *t;
        *t += 1;
        for w in step.text.split_inclusive(' ') {
            on_text(w);
        }
        let calls: Vec<Call> = if req.tools.is_empty() { vec![] } else { step.calls.iter().enumerate().map(|(i, (name, args))| Call { id: format!("call_{n}_{i}"), name: name.clone(), args: args.clone() }).collect() };
        let stop = if calls.is_empty() { Stop::Done } else { Stop::Tools };
        Ok(Reply { text: step.text, calls, raw: None, stop })
    }
}
