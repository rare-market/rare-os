// Every row of Settings, once. Search reads this list; a row shows up in it by carrying the
// same `id` (SettingRow, SwitchRow, ChoiceRow and SliderRow put it in data-setting-id).
// A test (index.test.ts) checks that every id here is in a section's source.

import { KINDS } from "../../notify/kinds";
import type { SettingEntry } from "./search";

const e = (id: string, section: SettingEntry["section"], title: string, ...keywords: string[]): SettingEntry => ({ id, section, title, keywords });

export const SETTINGS_INDEX: SettingEntry[] = [
  e("language", "system", "Language", "english", "locale", "text"),
  e("updates", "system", "Updates", "update", "upgrade", "version", "rollback", "boot menu", "system image", "restart", "install", "check"),
  e("restore-system", "system", "Restore previous system", "undo", "rollback", "gameos", "old system", "go back"),
  e("boot-menu", "system", "Boot menu", "grub", "startup", "older version", "next start"),
  e("desktop-mode", "system", "Desktop mode", "kde", "desktop", "files", "install apps"),

  e("docked", "display", "Docked", "tv", "monitor", "hdmi", "usb-c", "external display", "resolution", "4k", "1080p"),
  e("ui-size", "display", "UI size", "scale", "bigger", "larger", "text size", "zoom", "tv"),
  e("brightness", "display", "Brightness", "screen", "backlight", "night dim", "darker", "brighter"),
  e("dim-idle", "display", "Dim after idle", "screen", "dim", "timeout", "battery"),

  e("volume", "sound", "Volume", "audio", "speaker", "loud", "quiet", "headphones"),
  e("sounds", "sound", "Sounds", "clicks", "ui sounds", "sound effects", "mute"),

  e("buttons", "controls", "Buttons", "remap", "rare", "quick", "lc", "rc", "paddles", "map"),
  e("btn-lc", "controls", "LC button", "remap", "capture", "screenshot", "left"),
  e("btn-rc", "controls", "RC button", "remap", "keyboard", "overlay", "right"),
  e("deadzone-left", "controls", "Left stick deadzone", "drift", "stick", "analog"),
  e("deadzone-right", "controls", "Right stick deadzone", "drift", "stick", "analog"),
  e("invert-left", "controls", "Invert left stick", "flip", "stick", "vertical"),
  e("invert-right", "controls", "Invert right stick", "flip", "stick", "look", "vertical"),
  e("gyro-on", "controls", "Gyro", "motion", "imu", "aim", "inputplumber"),
  e("gyro-sens", "controls", "Gyro sensitivity", "motion", "aim", "speed"),
  e("rumble-strength", "controls", "Rumble strength", "vibration", "haptics", "buzz", "strength"),
  e("rumble", "controls", "Rumble", "vibration", "haptics", "controller", "buzz", "gamepad"),
  e("gyro", "controls", "Gyro in Steam games", "motion", "steam deck", "controller", "aim", "gamepad"),

  e("profile", "power", "Performance profile", "quiet", "balanced", "performance", "watts", "tdp", "speed"),
  e("tdp", "power", "Power limit (TDP)", "watts", "custom", "slider", "processor", "cpu"),
  e("cpu-boost", "power", "CPU boost", "turbo", "processor", "heat", "faster"),
  e("fan", "power", "Fan", "cool", "quiet", "noise", "temperature", "heat"),
  e("sleep-idle", "power", "Sleep when idle", "suspend", "auto sleep", "timeout", "battery", "charging"),
  e("battery-care", "power", "Battery care", "80%", "charge limit", "charging", "battery life", "stop at 80"),
  e("battery-health", "power", "Battery health", "capacity", "wear", "battery"),

  e("wifi", "network", "Wi-Fi", "wifi", "wireless", "internet", "network", "connect", "password", "scan"),
  e("wifi-forget", "network", "Forget this network", "wifi", "remove network", "saved network"),
  e("bluetooth", "network", "Bluetooth", "wireless", "pair", "headphones", "controller"),

  e("username", "accounts", "Username", "name", "friends", "profile", "suins"),
  e("rare-account", "accounts", "Rare account", "sign in", "login", "wallet", "google", "passkey"),
  e("setup", "accounts", "Setup", "first run", "onboarding", "passkey", "run again"),
  e("wallet", "accounts", "Rare Wallet", "sui", "balances", "collectibles", "assets"),

  e("scan", "storage", "Scan for games", "library", "refresh", "steam", "epic", "gog", "itch"),
  e("games-folder", "storage", "Games folder", "add folder", "sd card", "remove folder", "library", "path"),
  e("steam-start", "storage", "When Steam starts", "steam", "background", "boot", "launch", "play"),
  e("steamgriddb", "storage", "SteamGridDB cover art", "covers", "api key", "artwork", "images"),
  e("share", "storage", "Screenshots and clips", "capture", "record", "share", "video"),

  e("notif-dnd", "notifications", "Do not disturb", "quiet", "silence", "notices"),
  e("notif-toasts", "notifications", "Show toasts over a game", "popups", "in game", "notices"),
  e("notif-desktop", "notifications", "Desktop notifications", "background", "notices"),
  ...KINDS.map((k) => e(`notif-${k.kind}`, "notifications", k.title, "notification", "notice", ...k.body.toLowerCase().split(/\W+/).filter((w) => w.length > 3))),
  e("notif-alerts", "notifications", "Your price alerts", "price", "alert", "coin", "delete"),

  e("dev-network", "developer", "Network", "testnet", "mainnet", "sui", "developer", "switch"),
  e("dev-testnet-rpc", "developer", "Testnet RPC", "fullnode", "endpoint", "sui"),
  e("dev-testnet-api", "developer", "Testnet market API", "rare.market", "endpoint"),
  e("dev-testnet-sponsor", "developer", "Testnet gas sponsor", "endpoint", "gas"),
  e("dev-game", "developer", "Developer game", "dev-game", "local api", "web game", "studio"),
  e("emulators", "emulators", "Emulators", "update", "appimage", "retro"),
  e("about", "about", "About Rare OS", "version", "device", "hostname", "system"),
  e("licences", "licences", "Credits and licences", "open source", "thanks", "licenses"),
];
