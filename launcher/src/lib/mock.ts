// Mock native side for browser development and screenshots. Steam art comes from the
// public Steam CDN, exactly as the real scanner falls back to it.

import { BUILTIN_WEB_GAMES } from "./builtins";
import type { OsUpdate, BatteryCare, Config, DeviceInfo, DisplayState, FanMode, Game, GamePowerView, HudLevel, Levels, Library, NetStatus, Perf, Profile, Source, WifiNet } from "./types";

const cdn = (id: number) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}`;
const day = 86400;
const now = Math.floor(Date.now() / 1000);

function steam(appid: number, title: string, sizeGb: number, playedDaysAgo: number | null, origin = "Steam", foreign = false): Game {
  return {
    playtimeMinutes: playedDaysAgo === null ? null : Math.round(600 + appid % 9000),
    id: `steam:${appid}`,
    source: "steam",
    title,
    installed: true,
    installDir: `/home/rare/.local/share/Steam/steamapps/common/${title}`,
    sizeBytes: sizeGb * 1e9,
    lastPlayed: playedDaysAgo === null ? null : now - playedDaysAgo * day,
    art: {
      cover: [`${cdn(appid)}/library_600x900_2x.jpg`],
      hero: [`${cdn(appid)}/library_hero.jpg`],
      logo: [`${cdn(appid)}/logo.png`],
      header: [`${cdn(appid)}/header.jpg`],
    },
    origin,
    foreign,
  };
}

function other(source: Source, id: string, title: string, origin: string): Game {
  return { id: `${source}:${id}`, source, title, installed: true, installDir: null, sizeBytes: null, lastPlayed: null, art: { cover: [], hero: [], logo: [], header: [] }, origin, foreign: false };
}

const GAMES: Game[] = [
  steam(1145350, "Hades II", 9.8, 0.2),
  steam(2379780, "Balatro", 0.2, 1),
  steam(1245620, "ELDEN RING", 48, 3),
  steam(1868140, "DAVE THE DIVER", 9, 6),
  steam(1794680, "Vampire Survivors", 0.6, 9),
  steam(504230, "Celeste", 1.2, 14),
  steam(367520, "Hollow Knight", 9, 30),
  steam(413150, "Stardew Valley", 0.6, null),
  steam(1086940, "Baldur's Gate 3", 122, null),
  steam(1091500, "Cyberpunk 2077", 70, null),
  steam(578080, "PUBG: BATTLEGROUNDS", 40, null),
  steam(1244090, "Sea of Stars", 5, null, "Steam library on /var/home/playtron/.local/share/Steam", true),
  steam(1145360, "Hades", 15, null, "Steam library on /var/home/playtron/.local/share/Steam", true),
  other("epic", "Fortnite", "Alan Wake 2", "Epic Games"),
  other("gog", "1207658924", "The Witcher 3", "GOG"),
  other("itch", "a1", "A Short Hike", "itch.io"),
  other("local", "b2", "SuperTuxKart", "Games folder"),
  other("apps", "esde", "ES-DE", "EmuDeck"),
  other("apps", "retroarch", "RetroArch", "Desktop app"),
  other("apps", "dolphin", "Dolphin Emulator", "Desktop app"),
  other("apps", "ppsspp", "PPSSPP", "Desktop app"),
  other("apps", "xemu", "xemu", "Steam shortcut"),
  other("apps", "chrome", "Google Chrome", "Steam shortcut"),
];

let running: string[] = [];
const listeners = new Set<(r: string[]) => void>();
const emit = () => listeners.forEach((l) => l([...running]));

export async function library(): Promise<Library> {
  await new Promise((r) => setTimeout(r, 250));
  const builtins = BUILTIN_WEB_GAMES.filter((b) => !(cfg.removedBuiltins ?? []).includes(b.id) && !cfg.webGames.some((u) => u.id === b.id));
  const web: Game[] = [...builtins, ...(cfg.webGames ?? [])].map((w) => ({
    id: `web:${w.id}`, source: "web", title: w.title, installed: true, installDir: null, sizeBytes: null,
    art: { cover: w.cover ? [w.cover] : [], hero: w.hero ? [w.hero] : [], logo: BUILTIN_WEB_GAMES.some((b) => b.id === w.id) ? [`games/${w.id}/logo.svg`] : [], header: [] }, origin: "Web game", foreign: false, launch: { kind: "web", url: w.url },
    lastPlayed: w.id === "act" ? now - 3600 : null,
  }));
  // ?nogames: a fresh install with nothing installed yet (only the built-in web games).
  const all = [...(new URLSearchParams(location.search).has("nogames") ? [] : GAMES), ...web];
  const count = (s: Source) => all.filter((g) => g.source === s).length;
  return {
    games: all,
    sources: [
      { source: "steam", available: true, count: count("steam"), note: "Steam" },
      { source: "epic", available: true, count: count("epic"), note: "legendary" },
      { source: "gog", available: true, count: count("gog"), note: "GOG installs from Heroic" },
      { source: "itch", available: true, count: count("itch"), note: "itch.io downloads" },
      { source: "local", available: true, count: count("local"), note: "Games folders" },
      { source: "web", available: true, count: count("web"), note: "Browser games" },
      { source: "apps", available: true, count: count("apps"), note: "Emulators" },
    ],
    foreignSteamLibraries: ["/var/home/playtron/.local/share/Steam"],
    scannedAt: now,
    emulationApp: "apps:esde",
    emulation: {
      romDir: "/home/bazzite/Emulation/roms",
      systems: [
        { id: "switch", name: "Switch", count: 2, lastPlayed: now - 2 * day },
        { id: "xbox", name: "Xbox", count: 1, lastPlayed: now - 3 * day },
        { id: "gc", name: "GameCube", count: 12, lastPlayed: null },
        { id: "ps2", name: "PlayStation 2", count: 10, lastPlayed: null },
        { id: "wii", name: "Wii", count: 1, lastPlayed: null },
      ],
      recent: [
        { name: "Pokemon Violet", system: "switch", lastPlayed: now - 2 * day, playCount: 3, playtime: 5400 },
        { name: "Jet Set Radio Future", system: "xbox", lastPlayed: now - 3 * day, playCount: 9, playtime: 6540 },
      ],
    },
  };
}

export async function launch(id: string) {
  const w = [...BUILTIN_WEB_GAMES, ...cfg.webGames].find((g) => `web:${g.id}` === id);
  if (w) {
    window.open(w.url, "_blank");
    return;
  }
  running = [id];
  emit();
  setTimeout(() => {
    running = [];
    emit();
  }, 4000);
}
export async function quit(id: string) {
  running = running.filter((r) => r !== id);
  emit();
}
/** Screenshots: ?running=<id>[,<id>...] shows those as running. */
try {
  const r = new URLSearchParams(location.search).get("running");
  if (r) running = r.split(",");
} catch {
  /* not in a browser */
}
export const running_ = () => running;
export { running_ as running };

export function onRunning(cb: (r: string[]) => void) {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export const device = (): DeviceInfo => ({
  product: "SuiPlay0X1",
  vendor: "Mysten Labs, Inc.",
  name: "SuiPlay0X1",
  isSuiplay: true,
  isVm: false,
  osName: "Rare OS",
  osVersion: "0.1 (dev)",
  hostname: "rare",
  underGamescope: false,
  battery: { percent: 82, charging: false, health: 92, minutes: 190 },
  wifi: "Home",
  platform: "linux",
});

// Same presets as system::preset in the launcher.
const PRESETS = { quiet: { tdp: 10, boost: false }, balanced: { tdp: 15, boost: false }, performance: { tdp: 25, boost: true } } as const;
let perfState: Perf = { available: true, tdp: 15, tdpMin: 5, tdpMax: 28, fpsLimit: null, profile: "balanced", note: "", backend: "hhd", cpuBoost: false, fan: "default", profileWatts: { quiet: 10, balanced: 15, performance: 25 } };
export const perf = () => perfState;
export async function setPerf(profile: Profile, tdp?: number, fps?: number) {
  const fpsOnly = fps !== undefined && tdp === undefined && profile === perfState.profile;
  const p = profile === "custom" || fpsOnly ? null : PRESETS[profile];
  const g = gamePower();
  if (g) savedPower.add(g.id);
  perfState = {
    ...perfState,
    profile,
    tdp: tdp ?? p?.tdp ?? perfState.tdp,
    cpuBoost: p ? p.boost : perfState.cpuBoost,
    fpsLimit: fps === undefined ? perfState.fpsLimit : fps || null,
  };
}
export async function setCpuBoost(on: boolean) {
  perfState = { ...perfState, cpuBoost: on };
}
// Per-game power: the running game and whether "Apply to this game" is on for it.
const savedPower = new Set<string>();
export const gamePower = (): GamePowerView | null => (running.length ? { id: running[running.length - 1], saved: savedPower.has(running[running.length - 1]) } : null);
let hudState: HudLevel = "off";
export const hud = () => hudState;
export async function setHud(level: HudLevel) {
  hudState = level;
  const g = gamePower();
  if (g) savedPower.add(g.id);
}
export async function saveGamePower(save: boolean) {
  const g = gamePower();
  if (!g) throw new Error("No game is running");
  if (save) savedPower.add(g.id);
  else savedPower.delete(g.id);
}
export async function setFan(mode: FanMode) {
  perfState = { ...perfState, fan: mode };
}

let careState: BatteryCare = { available: true, on: false, state: "off" };
export const batteryCare = () => careState;
export async function setBatteryCare(on: boolean) {
  careState = { ...careState, on, state: on ? "holding" : "off" };
}

let levelState: Levels = { brightness: 70, volume: 55, muted: false };
export const levels = () => levelState;
export async function setLevels(b?: number, v?: number) {
  levelState = { ...levelState, brightness: b ?? levelState.brightness, volume: v ?? levelState.volume };
}

let cfg: Config = { gameFolders: [], onboarded: true, webGames: [] };
try {
  const saved = localStorage.getItem("rare.mock.config");
  if (saved) cfg = JSON.parse(saved);
} catch {
  /* private mode */
}
export const config = () => cfg;
export async function setConfig(v: Config) {
  cfg = v;
  try {
    localStorage.setItem("rare.mock.config", JSON.stringify(v));
  } catch {
    /* ignore */
  }
}

export async function toastOnly(_msg: string) {}

// ---------- first run: Wi-Fi and Steam (?offline starts disconnected, ?steam=name is signed in)

const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
let net: NetStatus = params.has("offline") ? { online: false, wifi: null, managed: true } : { online: true, wifi: "Home", managed: true };
export const netStatus = () => net;
export async function wifiScan(): Promise<WifiNet[]> {
  await new Promise((r) => setTimeout(r, 700));
  return [
    { ssid: "Home", signal: 86, secure: true, active: net.wifi === "Home" },
    { ssid: "Home 5G", signal: 71, secure: true, active: net.wifi === "Home 5G" },
    { ssid: "Cafe Guest", signal: 54, secure: false, active: net.wifi === "Cafe Guest" },
    { ssid: "Neighbour", signal: 22, secure: true, active: false },
  ];
}
export async function wifiConnect(ssid: string, _password?: string) {
  await new Promise((r) => setTimeout(r, 1500));
  if (_password === "wrong") throw new Error("Secrets were required, but not provided.");
  net = { online: true, wifi: ssid, managed: true };
}
export async function wifiForget(ssid: string) {
  if (net.wifi === ssid) net = { online: false, wifi: null, managed: true };
}
let btOn = params.has("btoff") ? false : true;
let radioState = { wifi: true as boolean | null, bluetooth: true as boolean | null, airplane: false };
export const radios = () => ({ ...radioState });
export async function setRadio(kind: "wifi" | "bluetooth" | "airplane", on: boolean) {
  if (kind === "airplane") radioState = { ...radioState, airplane: on, ...(on ? { wifi: false, bluetooth: false } : { wifi: true }) };
  else radioState = { ...radioState, [kind]: on };
  return radios();
}
export const bluetooth = () => ({ present: true, powered: btOn, blocked: false });
export async function bluetoothOn() {
  btOn = true;
  return bluetooth();
}
export const steamAccount = () => params.get("steam");
let upscaleState: import("./types").Upscale = { mode: "off", sharpness: 10, width: 0, height: 0 };
export const upscale = () => upscaleState;
export async function setUpscale(v: import("./types").Upscale) {
  upscaleState = v;
}

let dimIdleState = false;
export const brightnessInfo = () => ({ pos: levelState.brightness, dimIdle: dimIdleState });
export async function setDimIdle(on: boolean) {
  dimIdleState = on;
}

const started = Date.now();
/** Docked display for screenshots: ?dock=4k or ?dock=1080p (a TV on the USB-C port), else the internal panel only. */
export function displayState(): DisplayState {
  let q: string | null = null;
  try {
    const sp = new URLSearchParams(location.search);
    q = sp.get("dock");
    // ?dockin=2: plug the TV in two seconds after load (shows the toast).
    if (q && Date.now() - started < Number(sp.get("dockin") ?? 0) * 1000) q = null;
  } catch {
    /* not in a browser */
  }
  const internal = { name: "eDP-1", connected: true, lit: !q, internal: true, label: null, mode: "1200x1920" };
  if (q !== "4k" && q !== "1080p") return { outputs: [internal], active: "eDP-1", docked: false };
  const mode = q === "4k" ? "3840x2160" : "1920x1080";
  const tv = { name: "DP-1", connected: true, lit: true, internal: false, label: "LG TV", mode };
  return { outputs: [internal, tv], active: "DP-1", docked: true, dockName: "LG TV", dockMode: mode };
}

// ---------- OS updates (?update=staged|checking|failed|none, ?restore=none)

const ago = (sec: number) => Math.floor(Date.now() / 1000) - sec;
const updateMode = new URLSearchParams(typeof location !== "undefined" ? location.search : "").get("update");
let osState: OsUpdate = {
  bootedVersion: "43.20261020.0",
  staged: updateMode === "staged",
  stagedVersion: updateMode === "staged" ? "43.20261105.0" : null,
  checkedAt: updateMode === "failed" ? ago(26 * 3600) : ago(3 * 3600),
  checkOk: updateMode === "failed" ? false : true,
  checkError: updateMode === "failed" ? "Could not reach the update server." : null,
};
export const osUpdate = (): OsUpdate => osState;
export async function osUpdateCheck() {
  await new Promise((r) => setTimeout(r, 1500));
  osState = { ...osState, checkedAt: ago(0), checkOk: true, checkError: null };
}
export const osRestoreAvailable = () => new URLSearchParams(location.search).get("restore") !== "none";
export async function osRestore(reboot: boolean) {
  await new Promise((r) => setTimeout(r, 600));
  return reboot ? "Restarting into the previous system." : "The next start is the previous system.";
}
export async function osBootMenuNext() {
  await new Promise((r) => setTimeout(r, 400));
}
