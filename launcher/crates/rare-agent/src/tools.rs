//! What the model can ask Rare to do. The launcher runs each one through its existing flows and
//! sheets; the tools that move money or items only prepare Rare Wallet's review sheet, where the
//! player confirms on the device (PIN, Ledger, passkey). Nothing here signs.
//!
//! `cap` is the Settings > Developer switch a network app (MCP) needs for the tool, the same
//! switches the Local API already has (`rare_localapi::CAPS`); "" is public data, always allowed.

use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: &'static str,
    pub about: &'static str,
    /// JSON Schema of the arguments (an object).
    pub schema: Value,
    pub cap: &'static str,
    /// Only prepares a review sheet the player confirms.
    pub review: bool,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

fn s(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}

/// Settings `change_setting` can change, and the values each takes.
pub const SETTINGS: [(&str, &str); 5] = [
    ("power_profile", "quiet, balanced or performance"),
    ("brightness", "0 to 100"),
    ("volume", "0 to 100"),
    ("wifi", "on or off"),
    ("do_not_disturb", "on or off"),
];

/// Pages `open_page` can show.
pub const PAGES: [&str; 9] = ["home", "library", "store", "market", "wallet", "friends", "settings", "downloads", "trophies"];

pub fn catalog() -> Vec<Tool> {
    let t = |name, about, schema, cap, review| Tool { name, about, schema, cap, review };
    vec![
        t("search_games", "Find games by name: the library (installed or not, Steam, Epic, GOG, emulators, web games) and Rare Store. Returns ids for launch_game, install_game and uninstall_game.", obj(json!({ "query": s("Part of the game's name. Empty lists recent games.") }), &["query"]), "library", false),
        t("launch_game", "Start an installed game or app. The id must come from search_games (call it first); never guess an id.", obj(json!({ "id": s("The game's id.") }), &["id"]), "launch", false),
        t("install_game", "Open the install sheet for a game the player owns but has not installed. The player picks the drive and starts it.", obj(json!({ "id": s("The game's id.") }), &["id"]), "launch", false),
        t("uninstall_game", "Open the uninstall sheet for an installed game. The player confirms there.", obj(json!({ "id": s("The game's id.") }), &["id"]), "launch", false),
        t("downloads_status", "What is downloading or waiting to download, with progress.", obj(json!({}), &[]), "library", false),
        t("storage_suggestions", "Free space on each drive and the biggest games and files, least recently played first: suggestions for what to remove.", obj(json!({}), &[]), "library", false),
        t("get_device_settings", "The current power profile, brightness, volume, Wi-Fi and Do not disturb.", obj(json!({}), &[]), "settings", false),
        t(
            "change_setting",
            "Change one device setting: power_profile (quiet, balanced, performance), brightness (0-100), volume (0-100), wifi (on, off), do_not_disturb (on, off).",
            obj(json!({ "setting": { "type": "string", "enum": SETTINGS.iter().map(|s| s.0).collect::<Vec<_>>() }, "value": s("The new value, for example \"performance\", \"60\" or \"off\".") }), &["setting", "value"]),
            "settings",
            false,
        ),
        t("open_page", "Show a page of Rare.", obj(json!({ "page": { "type": "string", "enum": PAGES } }), &["page"]), "launch", false),
        t("wallet_balances", "The Rare Wallet account's coins and their value.", obj(json!({}), &[]), "profile", false),
        t(
            "wallet_send",
            "Prepare sending coins from Rare Wallet. Opens the review sheet; the player checks it and confirms on the device. Never say it was sent.",
            obj(json!({ "to": s("A 0x address, a name.sui or a Rare username."), "amount": s("How much, in whole coins, for example \"2.5\"."), "coin": s("The coin's symbol or type. Default SUI.") }), &["to", "amount"]),
            "purchase",
            true,
        ),
        t(
            "wallet_swap",
            "Prepare a swap between two coins. Opens the review sheet with the quote; the player confirms on the device.",
            obj(json!({ "from": s("The coin to pay with: a symbol the wallet holds (SUI, USDC) or a coin type."), "to": s("The coin to get: a symbol or a coin type."), "amount": s("How much of `from`, in whole coins.") }), &["from", "to", "amount"]),
            "purchase",
            true,
        ),
        t("my_items", "The player's collectibles from Sui games (id, name, collection, floor price, listing). Use the ids for market_sell and market_list.", obj(json!({ "query": s("Optional: part of the item's or collection's name.") }), &[]), "library", false),
        t("market_search", "Search Rare's market for collections and items by name. Returns collections with their floor and cheapest listed items with ids for market_buy.", obj(json!({ "query": s("What to look for.") }), &["query"]), "", false),
        t("market_buy", "Prepare buying a listed item. Opens the review sheet with the price and fees; the player confirms on the device.", obj(json!({ "item_id": s("The item's object id (0x...).") }), &["item_id"]), "purchase", true),
        t("market_sell", "Prepare selling one of the player's items into its best offer. Opens the sell sheet; the player confirms.", obj(json!({ "item_id": s("The item's object id.") }), &["item_id"]), "purchase", true),
        t("market_list", "Prepare listing one of the player's items for sale. Opens the sell sheet at that price; the player confirms.", obj(json!({ "item_id": s("The item's object id."), "price_sui": s("Price in SUI, for example \"12.5\".") }), &["item_id", "price_sui"]), "purchase", true),
        t("friends_list", "The player's friends: names, who is online and what they play.", obj(json!({}), &[]), "friends", false),
        t("invite_friends", "Start a party with the game being played and invite friends to it.", obj(json!({ "friends": { "type": "array", "items": { "type": "string" }, "description": "Friends' names or usernames." } }), &["friends"]), "friends", false),
        t("message_friend", "Send a short message to a friend (up to 280 characters).", obj(json!({ "friend": s("The friend's name or username."), "text": s("The message.") }), &["friend", "text"]), "friends", false),
    ]
}

pub fn find(name: &str) -> Option<Tool> {
    catalog().into_iter().find(|t| t.name == name)
}

/// Each tool's arguments, checked against its schema's required fields and simple types. Err is a
/// sentence for the model.
pub fn check_args(tool: &Tool, args: &Value) -> Result<(), String> {
    let Some(o) = args.as_object() else { return Err("Arguments must be an object.".into()) };
    let props = tool.schema["properties"].as_object();
    for r in tool.schema["required"].as_array().into_iter().flatten().filter_map(|r| r.as_str()) {
        match o.get(r) {
            None | Some(Value::Null) => return Err(format!("{r} is missing.")),
            Some(Value::String(v)) if v.trim().is_empty() && r != "query" => return Err(format!("{r} is empty.")),
            _ => {}
        }
    }
    if let Some(props) = props {
        for (k, v) in o {
            let Some(p) = props.get(k) else { continue };
            if let Some(e) = p["enum"].as_array() {
                if !e.iter().any(|x| x == v) {
                    return Err(format!("{k} must be one of {}.", e.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
                }
            }
        }
    }
    Ok(())
}

/// A short line for the panel: what the tool is doing ("Starting Hades").
pub fn label(name: &str, args: &Value) -> String {
    let a = |k: &str| args[k].as_str().map(str::to_string).or_else(|| args[k].as_f64().map(|n| n.to_string())).unwrap_or_default();
    let clip = |s: String| if s.chars().count() > 48 { format!("{}...", s.chars().take(45).collect::<String>()) } else { s };
    clip(match name {
        "search_games" => format!("Looking for \"{}\"", a("query")),
        "launch_game" => "Starting the game".into(),
        "install_game" => "Opening the install sheet".into(),
        "uninstall_game" => "Opening the uninstall sheet".into(),
        "downloads_status" => "Checking downloads".into(),
        "storage_suggestions" => "Checking storage".into(),
        "get_device_settings" => "Reading settings".into(),
        "change_setting" => format!("Setting {} to {}", a("setting").replace('_', " "), a("value")),
        "open_page" => format!("Opening {}", a("page")),
        "wallet_balances" => "Reading the wallet".into(),
        "wallet_send" => format!("Preparing {} {} to {}", a("amount"), if a("coin").is_empty() { "SUI".into() } else { a("coin") }, a("to")),
        "wallet_swap" => format!("Preparing a swap of {} {} to {}", a("amount"), a("from"), a("to")),
        "my_items" => "Reading your items".into(),
        "market_search" => format!("Searching the market for \"{}\"", a("query")),
        "market_buy" => "Preparing the purchase".into(),
        "market_sell" => "Preparing the sale".into(),
        "market_list" => format!("Preparing a listing at {} SUI", a("price_sui")),
        "friends_list" => "Reading friends".into(),
        "invite_friends" => "Starting a party".into(),
        "message_friend" => format!("Message to {}", a("friend")),
        other => other.replace('_', " "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_an_object_schema_and_a_known_switch() {
        let caps = ["", "profile", "library", "launch", "purchase", "settings", "friends"];
        let all = catalog();
        for t in &all {
            assert_eq!(t.schema["type"], "object", "{}", t.name);
            assert!(caps.contains(&t.cap), "{}", t.name);
            assert!(t.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert!(t.about.len() < 300);
        }
        // Everything that moves money or items prepares a review sheet and needs the purchase switch.
        for n in ["wallet_send", "wallet_swap", "market_buy", "market_sell", "market_list"] {
            let t = find(n).unwrap();
            assert!(t.review && t.cap == "purchase", "{n}");
        }
        let mut names: Vec<_> = all.iter().map(|t| t.name).collect();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }

    #[test]
    fn args_are_checked() {
        let t = find("change_setting").unwrap();
        assert!(check_args(&t, &json!({ "setting": "brightness", "value": "40" })).is_ok());
        assert!(check_args(&t, &json!({ "setting": "fan", "value": "max" })).unwrap_err().contains("must be one of"));
        assert_eq!(check_args(&t, &json!({ "setting": "volume" })).unwrap_err(), "value is missing.");
        assert!(check_args(&t, &json!("x")).is_err());
        assert!(check_args(&find("search_games").unwrap(), &json!({ "query": "" })).is_ok());
    }

    #[test]
    fn labels_are_short() {
        assert_eq!(label("launch_game", &json!({ "id": "steam:620" })), "Starting the game");
        assert_eq!(label("change_setting", &json!({ "setting": "power_profile", "value": "quiet" })), "Setting power profile to quiet");
        assert!(label("message_friend", &json!({ "friend": "x".repeat(80) })).ends_with("..."));
    }
}
