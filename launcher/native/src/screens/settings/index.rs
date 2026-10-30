//! Every row of Settings, once (screens/settings/index.ts). Search reads this list.

use super::search::SettingEntry;
use super::sections::SectionId;

/// The kinds of notice that have a switch in Settings, in the order shown (notify/kinds.ts).
pub const KINDS: [(&str, &str, &str); 18] = [
    ("sold", "Sold", "One of your items sold, or someone took your listing."),
    ("bought", "Bought", "You bought an item, or your offer was accepted."),
    ("offer_received", "Offers", "A new offer on an item or collection you hold."),
    ("coin_received", "Coins received", "Someone sent you coins, above $1."),
    ("nft_received", "NFTs received", "Someone sent you an item from a collection on rare.market."),
    ("airdrop", "Airdrops", "An item minted straight to you."),
    ("perp_liquidation", "Perps liquidation", "A perps position is near its liquidation price."),
    ("price_alert", "Price alerts", "A price alert you set was hit."),
    ("followed_trade", "Followed traders", "A trader you follow made a trade."),
    ("friend_request", "Friend requests", "Someone wants to be your friend."),
    ("friend_online", "Friends online", "A friend came online or started a game."),
    ("party_invite", "Party invites", "A friend invited you to their party."),
    ("direct_message", "Messages", "A friend sent you a message."),
    ("party_message", "Party messages", "A new message in your party chat."),
    ("os_update", "Rare OS updates", "An update is ready and installs when you restart."),
    ("download_done", "Downloads", "A Steam download finished."),
    ("emu_update", "Emulator updates", "A newer version of an emulator is available."),
    ("trophy", "Trophies", "A trophy was earned."),
];

fn e(id: &str, section: SectionId, title: &str, keywords: &[&str]) -> SettingEntry {
    SettingEntry { id: id.into(), section, title: title.into(), keywords: keywords.iter().map(|s| s.to_string()).collect() }
}

fn words_over3(body: &str) -> Vec<String> {
    body.to_lowercase().split(|c: char| !c.is_ascii_alphanumeric() && c != '_').filter(|w| w.len() > 3).map(String::from).collect()
}

pub fn settings_index() -> Vec<SettingEntry> {
    let mut v = vec![
        e("devices-pair", SectionId::Devices, "Pair a phone", &["phone", "remote", "control", "qr", "rare.market", "companion", "keyboard", "send", "install"]),
        e("ssh-system", SectionId::System, "Remote access (SSH)", &["ssh", "remote", "terminal", "sign in", "password", "key", "mod"]),
        e("language", SectionId::System, "Language", &["english", "locale", "text"]),
        e("updates", SectionId::System, "Updates", &["update", "upgrade", "version", "rollback", "boot menu", "system image", "restart", "install", "check"]),
        e("restore-system", SectionId::System, "Restore previous system", &["undo", "rollback", "gameos", "old system", "go back"]),
        e("time-zone", SectionId::System, "Time zone", &["timezone", "clock", "time", "date", "zone", "city"]),
        e("time-auto", SectionId::System, "Automatic time", &["ntp", "clock", "time", "sync"]),
        e("logs-save", SectionId::System, "Logs for a bug report", &["logs", "bug", "report", "support", "debug", "export", "usb"]),
        e("factory-reset", SectionId::System, "Factory reset", &["reset", "erase", "wipe", "factory", "sell", "clear"]),
        e("boot-menu", SectionId::System, "Boot menu", &["grub", "startup", "older version", "next start"]),
        e("desktop-mode", SectionId::System, "Desktop mode", &["kde", "desktop", "files", "install apps"]),
        e("docked", SectionId::Display, "Docked", &["tv", "monitor", "hdmi", "usb-c", "external display", "resolution", "4k", "1080p"]),
        e("ui-size", SectionId::Display, "UI size", &["scale", "bigger", "larger", "text size", "zoom", "tv"]),
        e("brightness", SectionId::Display, "Brightness", &["screen", "backlight", "night dim", "darker", "brighter"]),
        e("dim-idle", SectionId::Display, "Dim after idle", &["screen", "dim", "timeout", "battery"]),
        e("night-light", SectionId::Display, "Night light", &["warm", "blue light", "night", "eyes", "colour temperature", "color"]),
        e("tv-size", SectionId::Display, "TV resolution", &["tv", "4k", "1080p", "resolution", "docked", "monitor"]),
        e("tv-refresh", SectionId::Display, "TV refresh rate", &["hz", "120", "refresh", "tv"]),
        e("volume", SectionId::Sound, "Volume", &["audio", "speaker", "loud", "quiet", "headphones"]),
        e("output", SectionId::Sound, "Output", &["audio", "speaker", "headphones", "bluetooth", "hdmi", "tv", "usb", "sound device", "switch"]),
        e("mic", SectionId::Sound, "Microphone", &["mic", "mute", "voice", "chat", "headset", "input"]),
        e("mic-level", SectionId::Sound, "Mic level", &["microphone", "input volume", "gain", "voice"]),
        e("sounds", SectionId::Sound, "Sounds", &["clicks", "ui sounds", "sound effects", "mute"]),
        e("buttons", SectionId::Controls, "Buttons", &["remap", "rare", "quick", "lc", "rc", "paddles", "map"]),
        e("btn-lc", SectionId::Controls, "LC button", &["remap", "capture", "screenshot", "left"]),
        e("btn-rc", SectionId::Controls, "RC button", &["remap", "keyboard", "overlay", "right"]),
        e("remap", SectionId::Controls, "Button layout", &["remap", "swap", "nintendo", "a and b", "x and y", "mapping", "buttons"]),
        e("deadzone-left", SectionId::Controls, "Left stick deadzone", &["drift", "stick", "analog"]),
        e("deadzone-right", SectionId::Controls, "Right stick deadzone", &["drift", "stick", "analog"]),
        e("invert-left", SectionId::Controls, "Invert left stick", &["flip", "stick", "vertical"]),
        e("invert-right", SectionId::Controls, "Invert right stick", &["flip", "stick", "look", "vertical"]),
        e("gyro-on", SectionId::Controls, "Gyro", &["motion", "imu", "aim", "inputplumber"]),
        e("gyro-sens", SectionId::Controls, "Gyro sensitivity", &["motion", "aim", "speed"]),
        e("rumble-strength", SectionId::Controls, "Rumble strength", &["vibration", "haptics", "buzz", "strength"]),
        e("rumble", SectionId::Controls, "Rumble", &["vibration", "haptics", "controller", "buzz", "gamepad"]),
        e("gyro", SectionId::Controls, "Gyro in Steam games", &["motion", "steam deck", "controller", "aim", "gamepad"]),
        e("profile", SectionId::Power, "Performance profile", &["quiet", "balanced", "performance", "watts", "tdp", "speed"]),
        e("tdp", SectionId::Power, "Power limit", &["watts", "custom", "slider", "processor", "cpu"]),
        e("cpu-boost", SectionId::Power, "CPU boost", &["turbo", "processor", "heat", "faster"]),
        e("fan", SectionId::Power, "Fan", &["cool", "quiet", "noise", "temperature", "heat"]),
        e("sleep-idle", SectionId::Power, "Sleep when idle", &["suspend", "auto sleep", "timeout", "battery", "charging"]),
        e("battery-care", SectionId::Power, "Battery care", &["80%", "charge limit", "charging", "battery life", "stop at 80"]),
        e("battery-health", SectionId::Power, "Battery health", &["capacity", "wear", "battery"]),
        e("wifi", SectionId::Network, "Wi-Fi", &["wifi", "wireless", "internet", "network", "connect", "password", "scan"]),
        e("wifi-forget", SectionId::Network, "Forget this network", &["wifi", "remove network", "saved network"]),
        e("bluetooth", SectionId::Network, "Bluetooth", &["wireless", "pair", "headphones", "controller"]),
        e("username", SectionId::Accounts, "Username", &["name", "friends", "profile", "suins"]),
        e("rare-account", SectionId::Accounts, "Rare account", &["sign in", "login", "wallet", "google", "passkey"]),
        e("setup", SectionId::Accounts, "Setup", &["first run", "onboarding", "passkey", "run again"]),
        e("wallet", SectionId::Accounts, "Rare Wallet", &["sui", "balances", "collectibles", "assets"]),
        e("parental-pin", SectionId::Parental, "Parental controls", &["parental", "pin", "kids", "child", "family", "lock"]),
        e("parental-daily", SectionId::Parental, "Daily play time", &["play time", "limit", "screen time", "kids"]),
        e("parental-hours", SectionId::Parental, "Allowed hours", &["bedtime", "hours", "time", "kids"]),
        e("parental-age", SectionId::Parental, "Age rating", &["esrb", "pegi", "rating", "mature", "age"]),
        e("scan", SectionId::Storage, "Scan for games", &["library", "refresh", "steam", "epic", "gog", "itch"]),
        e("games-folder", SectionId::Storage, "Games folder", &["add folder", "sd card", "remove folder", "library", "path"]),
        e("steam-start", SectionId::Storage, "When Steam starts", &["steam", "background", "boot", "launch", "play"]),
        e("steamgriddb", SectionId::Storage, "SteamGridDB cover art", &["covers", "api key", "artwork", "images"]),
        e("share", SectionId::Storage, "Screenshots and clips", &["capture", "record", "share", "video"]),
        e("replay-toggle", SectionId::Capture, "Instant replay", &["clip", "record", "replay", "save last", "video", "capture", "gpu-screen-recorder"]),
        e("replay-length", SectionId::Capture, "Replay length", &["clip", "seconds", "minutes", "replay"]),
        e("replay-cap", SectionId::Capture, "Space for clips", &["clips", "storage", "limit", "space", "delete"]),
        e("discord-presence", SectionId::Capture, "Show what you play on Discord", &["discord", "rich presence", "status", "playing", "arrpc"]),
        e("discord-mute", SectionId::Capture, "Discord voice", &["discord", "mute", "deafen", "voice", "call", "microphone"]),
        e("host-toggle", SectionId::Capture, "Stream this device", &["stream", "sunshine", "moonlight", "tv", "remote play", "host", "pair", "pin"]),
        e("nearby-scan", SectionId::Capture, "Rare devices nearby", &["network", "devices", "nearby", "lan", "find"]),
        e("notif-dnd", SectionId::Notifications, "Do not disturb", &["quiet", "silence", "notices"]),
        e("notif-toasts", SectionId::Notifications, "Show toasts over a game", &["popups", "in game", "notices"]),
        e("notif-desktop", SectionId::Notifications, "Desktop notifications", &["background", "notices"]),
        e("notif-alerts", SectionId::Notifications, "Your price alerts", &["price", "alert", "coin", "delete"]),
        e("dev-ssh", SectionId::Developer, "Remote access (SSH)", &["ssh", "remote", "terminal", "password", "key", "github", "usb", "qr", "mod", "login"]),
        e("ask-provider", SectionId::Ask, "Ask Rare", &["ai", "assistant", "agent", "claude", "openai", "chatgpt", "ollama", "model", "llm", "voice", "talk", "api key"]),
        e("ask-voice", SectionId::Ask, "Speak to Rare", &["voice", "microphone", "talk", "speech", "whisper", "push to talk"]),
        e("ask-hold", SectionId::Ask, "Hold RC to talk", &["rc", "button", "voice", "assistant"]),
        e("ask-shots", SectionId::Ask, "Pictures of the game", &["screenshot", "vision", "help", "boss", "privacy"]),
        e("dev-netapi", SectionId::Developer, "Allow network apps", &["local api", "lan", "agent", "token", "sdk", "tools", "laptop"]),
        e("dev-netapi-caps", SectionId::Developer, "What network apps can do", &["permissions", "switches", "profile", "inventory", "library", "launch", "controller", "input", "purchase", "screenshot", "agent"]),
        e("dev-netapi-activity", SectionId::Developer, "Recent network activity", &["log", "audit", "requests", "agent", "history"]),
        e("dev-network", SectionId::Developer, "Network", &["testnet", "mainnet", "sui", "developer", "switch"]),
        e("dev-testnet-rpc", SectionId::Developer, "Testnet RPC", &["fullnode", "endpoint", "sui"]),
        e("dev-testnet-api", SectionId::Developer, "Testnet market API", &["rare.market", "endpoint"]),
        e("dev-testnet-sponsor", SectionId::Developer, "Testnet gas sponsor", &["endpoint", "gas"]),
        e("dev-game", SectionId::Developer, "Developer game", &["dev-game", "local api", "web game", "studio"]),
        e("emulators", SectionId::Emulators, "Emulators", &["update", "appimage", "retro"]),
        e("about", SectionId::About, "About Rare OS", &["version", "device", "hostname", "system"]),
        e("licences", SectionId::Licences, "Credits and licences", &["open source", "thanks", "licenses"]),
    ];
    // The notification kinds sit between "notif-desktop" and "notif-alerts".
    let at = v.iter().position(|x| x.id == "notif-alerts").unwrap_or(v.len());
    let kinds: Vec<SettingEntry> = KINDS
        .iter()
        .map(|(kind, title, body)| {
            let mut kw = vec!["notification".to_string(), "notice".to_string()];
            kw.extend(words_over3(body));
            SettingEntry { id: format!("notif-{kind}"), section: SectionId::Notifications, title: (*title).into(), keywords: kw }
        })
        .collect();
    v.splice(at..at, kinds);
    // Controls rows InputPlumber can't apply are not shown (controls.rs), so search skips them.
    let off: Vec<&str> = rare_input::ip::controls_support()
        .iter()
        .filter(|c| !c.applies)
        .filter_map(|c| match c.key {
            "deadzoneLeft" => Some("deadzone-left"),
            "deadzoneRight" => Some("deadzone-right"),
            "invertLeftY" => Some("invert-left"),
            "invertRightY" => Some("invert-right"),
            "gyroSensitivity" => Some("gyro-sens"),
            _ => None,
        })
        .collect();
    v.retain(|x| !off.contains(&x.id.as_str()));
    v
}
