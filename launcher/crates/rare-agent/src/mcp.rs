//! Rare's tools over MCP (JSON-RPC 2.0, the Streamable HTTP transport's single-response form), for
//! agents on a laptop. The Local API's network listener serves it at POST /v1/mcp: off by default,
//! its token, and each tool behind the Settings > Developer switch it needs. Buying, selling and
//! sending only open the review sheet on the device; nothing is signed without a press there.

use crate::tools::{self, Tool};
use serde_json::{json, Value};

pub const PROTOCOL: &str = "2025-06-18";

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The tools a client sees: the ones whose switch is on.
pub fn listed(cap_on: &dyn Fn(&str) -> bool) -> Vec<Tool> {
    tools::catalog().into_iter().filter(|t| cap_on(t.cap)).collect()
}

/// One message (or a batch). None for notifications only (HTTP 202, no body).
/// `call(name, args)` runs a tool in the launcher: Ok(data) or Err(sentence).
pub fn handle(msg: &Value, cap_on: &dyn Fn(&str) -> bool, call: &dyn Fn(&str, &Value) -> Result<Value, String>) -> Option<Value> {
    if let Some(batch) = msg.as_array() {
        let out: Vec<Value> = batch.iter().filter_map(|m| handle_one(m, cap_on, call)).collect();
        return (!out.is_empty()).then_some(Value::Array(out));
    }
    handle_one(msg, cap_on, call)
}

fn handle_one(msg: &Value, cap_on: &dyn Fn(&str) -> bool, call: &dyn Fn(&str, &Value) -> Result<Value, String>) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str().unwrap_or("");
    // A notification or a response from the client: nothing to answer.
    let id = id?;
    if msg["jsonrpc"] != "2.0" || method.is_empty() {
        return Some(err(&id, -32600, "Invalid request"));
    }
    let p = &msg["params"];
    Some(match method {
        "initialize" => {
            let v = p["protocolVersion"].as_str().filter(|v| ["2025-06-18", "2025-03-26", "2024-11-05"].contains(v)).unwrap_or(PROTOCOL);
            ok(
                &id,
                json!({
                    "protocolVersion": v,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "rare", "title": "Rare", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Rare runs games, settings, Rare Wallet and the Market on this handheld. Tools that buy, sell, list or send only open a review sheet on the device; the owner confirms there. Tools appear when the owner switches them on in Settings > Developer.",
                }),
            )
        }
        "ping" => ok(&id, json!({})),
        "tools/list" => {
            let list: Vec<Value> = listed(cap_on)
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.about,
                        "inputSchema": t.schema,
                        "annotations": { "readOnlyHint": !t.review && read_only(t.name), "destructiveHint": false, "openWorldHint": false },
                    })
                })
                .collect();
            ok(&id, json!({ "tools": list }))
        }
        "tools/call" => {
            let name = p["name"].as_str().unwrap_or("");
            let args = if p["arguments"].is_null() { json!({}) } else { p["arguments"].clone() };
            let Some(t) = tools::find(name) else { return Some(err(&id, -32602, &format!("Unknown tool: {name}"))) };
            if !cap_on(t.cap) {
                return Some(ok(&id, tool_text(&format!("The owner has not switched this on in Settings > Developer ({}).", cap_title(t.cap)), true)));
            }
            if let Err(e) = tools::check_args(&t, &args) {
                return Some(ok(&id, tool_text(&e, true)));
            }
            match call(name, &args) {
                Ok(v) => {
                    let mut r = tool_text(&v.to_string(), false);
                    if v.is_object() {
                        r["structuredContent"] = v;
                    }
                    ok(&id, r)
                }
                Err(e) => ok(&id, tool_text(&e, true)),
            }
        }
        "resources/list" => ok(&id, json!({ "resources": [] })),
        "prompts/list" => ok(&id, json!({ "prompts": [] })),
        _ => err(&id, -32601, "Method not found"),
    })
}

fn tool_text(text: &str, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// Tools that only read (safe to run again).
pub fn read_only(name: &str) -> bool {
    matches!(name, "search_games" | "downloads_status" | "storage_suggestions" | "get_device_settings" | "wallet_balances" | "my_items" | "market_search" | "friends_list")
}

fn cap_title(cap: &str) -> &'static str {
    match cap {
        "profile" => "Read profile",
        "library" => "Read inventory and library",
        "launch" => "Launch and close games and apps",
        "purchase" => "Request purchases and listings",
        "settings" => "Change settings",
        "friends" => "Friends and messages",
        _ => "a switch",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(on: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |c: &str| c.is_empty() || on.contains(&c)
    }

    #[test]
    fn initialize_list_and_call() {
        let call = |name: &str, args: &Value| -> Result<Value, String> { Ok(json!({ "ran": name, "args": args })) };
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), &caps(&[]), &call).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "rare");
        // The initialized notification has no answer.
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), &caps(&[]), &call).is_none());
        // Only switched-on tools are listed (plus public ones).
        let l = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), &caps(&["library"]), &call).unwrap();
        let names: Vec<&str> = l["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"search_games") && names.contains(&"market_search"));
        assert!(!names.contains(&"wallet_send") && !names.contains(&"change_setting"));
        let c = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "search_games", "arguments": { "query": "hades" } } }), &caps(&["library"]), &call).unwrap();
        assert_eq!(c["result"]["isError"], false);
        assert_eq!(c["result"]["structuredContent"]["ran"], "search_games");
    }

    #[test]
    fn switches_and_arguments_are_enforced() {
        let call = |_: &str, _: &Value| -> Result<Value, String> { panic!("must not run") };
        let c = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "wallet_send", "arguments": { "to": "0x1", "amount": "1" } } }), &caps(&["library"]), &call).unwrap();
        assert_eq!(c["result"]["isError"], true);
        assert!(c["result"]["content"][0]["text"].as_str().unwrap().contains("Request purchases"));
        let c = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "change_setting", "arguments": { "setting": "x", "value": "1" } } }), &caps(&["settings"]), &call).unwrap();
        assert_eq!(c["result"]["isError"], true);
        let c = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" }), &caps(&[]), &call).unwrap();
        assert_eq!(c["error"]["code"], -32601);
        let c = handle(&json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "fly" } }), &caps(&[]), &call).unwrap();
        assert_eq!(c["error"]["code"], -32602);
    }

    #[test]
    fn batches() {
        let call = |_: &str, _: &Value| -> Result<Value, String> { Ok(json!({})) };
        let b = handle(&json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }, { "jsonrpc": "2.0", "method": "notifications/initialized" }]), &caps(&[]), &call).unwrap();
        assert_eq!(b.as_array().unwrap().len(), 1);
    }
}
