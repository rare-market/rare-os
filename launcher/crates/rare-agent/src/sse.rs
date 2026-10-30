//! Server-sent events and newline-delimited JSON, read line by line from a response body.

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};

pub const CANCELLED: &str = "Stopped.";

/// Calls `f(event, data)` for each event (data lines joined with "\n"). `f` returns false to stop.
pub fn events(r: impl BufRead, cancel: &AtomicBool, mut f: impl FnMut(&str, &str) -> Result<bool, String>) -> Result<(), String> {
    let mut event = String::new();
    let mut data = String::new();
    for line in r.lines() {
        if cancel.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        let line = line.map_err(|e| format!("The connection dropped ({e})."))?;
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !data.is_empty() || !event.is_empty() {
                let go = f(&event, &data)?;
                event.clear();
                data.clear();
                if !go {
                    return Ok(());
                }
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        let (k, v) = line.split_once(':').map(|(k, v)| (k, v.strip_prefix(' ').unwrap_or(v))).unwrap_or((line, ""));
        match k {
            "event" => event = v.to_string(),
            "data" => {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(v);
            }
            _ => {}
        }
    }
    if !data.is_empty() {
        f(&event, &data)?;
    }
    Ok(())
}

/// One JSON value per line (Ollama).
pub fn ndjson(r: impl BufRead, cancel: &AtomicBool, mut f: impl FnMut(serde_json::Value) -> Result<bool, String>) -> Result<(), String> {
    for line in r.lines() {
        if cancel.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        let line = line.map_err(|e| format!("The connection dropped ({e})."))?;
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line).map_err(|_| "The model sent something unreadable.".to_string())?;
        if !f(v)? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_events() {
        let body = ": hi\nevent: a\ndata: {\"x\":1}\n\ndata: one\ndata: two\n\nevent: end\ndata: [DONE]\n";
        let mut got = vec![];
        events(body.as_bytes(), &AtomicBool::new(false), |e, d| {
            got.push((e.to_string(), d.to_string()));
            Ok(true)
        })
        .unwrap();
        assert_eq!(got, vec![("a".into(), "{\"x\":1}".into()), ("".into(), "one\ntwo".into()), ("end".into(), "[DONE]".into())]);
    }
}
