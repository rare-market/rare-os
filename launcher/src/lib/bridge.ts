// One door to the native side. Inside Tauri it calls the Rust commands; in a plain
// browser (UI development on macOS, screenshots) it answers from mock data so every
// screen can be built and checked without a handheld.

import type { BatteryCare, BluetoothAdapter, Config, DeviceInfo, DisplayState, EmuInfo, EmuUpdateDone, EmuUpdateInfo, FanMode, GamePowerView, HudLevel, Levels, Library, NetStatus, OsUpdate, PadButton, Perf, Profile, Radios, Upscale, WifiNet } from "./types";
import * as mock from "./mock";
import { remember } from "./state";

type Unlisten = () => void;

/** True in the real app. Release builds fix it to true (lib/mockflag.ts) so no mock branch can run or ship. */
export const isTauri: boolean = __RARE_NATIVE__;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Local file paths become asset:// URLs the webview may load; URLs pass through. */
export function fileSrc(path: string): string {
  if (!path.startsWith("/")) return path;
  if (!isTauri) return path;
  const w = window as unknown as { __TAURI_INTERNALS__?: { convertFileSrc?: (p: string, proto?: string) => string } };
  return w.__TAURI_INTERNALS__?.convertFileSrc?.(path, "asset") ?? path;
}

export const native = {
  emus: (): Promise<EmuInfo[]> => (isTauri ? call("emu_list") : Promise.resolve([])),
  emuCheck: (id: string): Promise<EmuUpdateInfo> => call("emu_check", { id }),
  emuUpdate: (id: string): Promise<EmuUpdateDone> => call("emu_update", { id }),
  emuDefaults: (id: string): Promise<{ written: string[]; kept: string[]; notes: string[] }> => call("emu_apply_defaults", { id }),
  scanLibrary: (): Promise<Library> => (isTauri ? call("library_scan") : mock.library()),
  /** The last scan, kept on disk: shows the library instantly while a fresh scan runs. */
  cachedLibrary: (): Promise<Library | null> => (isTauri ? call("library_cached") : Promise.resolve(null)),
  quit: (id: string): Promise<void> => (isTauri ? call("game_quit", { id }) : mock.quit(id)),
  resume: (id: string): Promise<void> => (isTauri ? call("game_resume", { id }) : mock.toastOnly("Resume")),
  /** The Chromium browser web games can fall back to, if the device has one. */
  browser: (): Promise<{ name: string } | null> => (isTauri ? call("browser_info") : Promise.resolve({ name: "Google Chrome" })),
  webgameOpenBrowser: (id: string, url: string): Promise<string> =>
    isTauri ? call("webgame_open_browser", { id, url }) : (window.open(url, "_blank"), Promise.resolve("Google Chrome")),
  launch: (id: string): Promise<void> => (isTauri ? call("game_launch", { id }) : mock.launch(id)),
  running: (): Promise<string[]> => (isTauri ? call("running_games") : Promise.resolve(mock.running())),
  /** Steam's store page for an app (in Steam itself). */
  steamStore: (appid: number): Promise<void> => (isTauri ? call("steam_store", { appid }) : (window.open(`https://store.steampowered.com/app/${appid}`, "_blank"), Promise.resolve())),
  steamPrewarm: (): Promise<void> => (isTauri ? call("steam_prewarm") : Promise.resolve()),
  launchPhase: (id: string): Promise<"steam" | "game" | "playing" | "none"> => (isTauri ? call("launch_phase", { id }) : Promise.resolve("none")),
  /** Proton tools, Steam's global default and one game's override. */
  protonStatus: (appid: number | null): Promise<ProtonStatus | null> => (isTauri ? call("proton_status", { appid }) : Promise.resolve(null)),
  /** Installs the latest GE-Proton ("ge") or Proton-CachyOS ("cachyos"), checksum verified. */
  protonInstall: (kind: "ge" | "cachyos"): Promise<string> => (isTauri ? call("proton_install", { kind }) : mock.toastOnly("Installed").then(() => "mock")),
  /** Sets (or clears, with null) a game's Proton. Closes and restarts Steam around the edit. */
  protonSetGame: (appid: number, tool: string | null): Promise<void> => (isTauri ? call("proton_set_game", { appid, tool }) : Promise.resolve()),
  protonRemove: (name: string): Promise<void> => (isTauri ? call("proton_remove", { name }) : Promise.resolve()),
  /** Closes the panel over a Steam game and opens Steam's own in-game menu (Shift+Tab). */
  steamMenu: (): Promise<void> => (isTauri ? call("steam_menu") : mock.toastOnly("Steam menu")),
  steamBigPicture: (): Promise<void> => (isTauri ? call("steam_big_picture") : mock.toastOnly("Opening Steam")),
  steamAddLibrary: (path: string): Promise<void> => (isTauri ? call("steam_add_library", { path }) : mock.toastOnly("Added")),
  wifiScan: (): Promise<WifiNet[]> => (isTauri ? call("wifi_scan") : mock.wifiScan()),
  wifiConnect: (ssid: string, password?: string): Promise<void> => (isTauri ? call("wifi_connect", { ssid, password: password ?? null }) : mock.wifiConnect(ssid, password)),
  wifiForget: (ssid: string): Promise<void> => (isTauri ? call("wifi_forget", { ssid }) : mock.wifiForget(ssid)),
  /** The Bluetooth adapter: present, powered, soft-blocked. Turning it on is all the launcher can do. */
  bluetoothState: (): Promise<BluetoothAdapter> => (isTauri ? call("bluetooth_state") : Promise.resolve(mock.bluetooth())),
  bluetoothPowerOn: (): Promise<BluetoothAdapter> => (isTauri ? call("bluetooth_power_on") : mock.bluetoothOn()),
  radios: (): Promise<Radios> => (isTauri ? call("radios_get") : Promise.resolve(mock.radios())),
  setRadio: (kind: "wifi" | "bluetooth" | "airplane", on: boolean): Promise<Radios> => (isTauri ? call("radios_set", { kind, on }) : mock.setRadio(kind, on)),
  netStatus: (): Promise<NetStatus> => (isTauri ? call("net_status") : Promise.resolve(mock.netStatus())),
  /** The Steam account signed in on this device, or null. */
  steamAccount: (): Promise<string | null> => (isTauri ? call("steam_account") : Promise.resolve(mock.steamAccount())),
  device: (): Promise<DeviceInfo> => (isTauri ? call("device_info") : Promise.resolve(mock.device())),
  perf: (): Promise<Perf> => (isTauri ? call("perf_get") : Promise.resolve(mock.perf())),
  setPerf: (profile: Profile, tdp?: number, fps?: number): Promise<void> =>
    isTauri ? call("perf_set", { profile, tdp: tdp ?? null, fps: fps ?? null }) : mock.setPerf(profile, tdp, fps),
  /** The running game whose power settings quick settings change; null with no game. */
  gamePower: (): Promise<GamePowerView | null> => (isTauri ? call("game_power_get") : Promise.resolve(mock.gamePower())),
  /** "Apply to this game": keep the current settings for it, or forget them (Balanced next time). */
  hud: (): Promise<HudLevel> => (isTauri ? call("hud_get") : Promise.resolve(mock.hud())),
  setHud: (level: HudLevel): Promise<void> => (isTauri ? call("hud_set", { level }) : mock.setHud(level)),
  saveGamePower: (save: boolean): Promise<void> => (isTauri ? call("game_power_save", { save }) : mock.saveGamePower(save)),
  setCpuBoost: (on: boolean): Promise<void> => (isTauri ? call("perf_boost", { on }) : mock.setCpuBoost(on)),
  displayState: (): Promise<DisplayState> => (isTauri ? call("display_state") : Promise.resolve(mock.displayState())),
  brightnessInfo: (): Promise<{ pos: number | null; dimIdle: boolean }> => (isTauri ? call("brightness_info") : Promise.resolve(mock.brightnessInfo())),
  setDimIdle: (on: boolean): Promise<void> => (isTauri ? call("brightness_dim_idle", { on }) : mock.setDimIdle(on)),
  idleDim: (dim: boolean): Promise<void> => (isTauri ? call("brightness_idle", { dim }) : Promise.resolve()),
  upscale: (): Promise<Upscale> => (isTauri ? call("upscale_get") : Promise.resolve(mock.upscale())),
  setUpscale: (value: Upscale): Promise<void> => (isTauri ? call("upscale_set", { value }) : mock.setUpscale(value)),
  setFan: (mode: FanMode): Promise<void> => (isTauri ? call("perf_fan", { mode }) : mock.setFan(mode)),
  /** null when rare-fand isn't running (other devices). */
  steamDownloads: (): Promise<Array<{ id: string; title: string; done: number; total: number; paused: boolean }>> => (isTauri ? call("steam_downloads") : Promise.resolve([])),
  osUpdate: (): Promise<OsUpdate> => (isTauri ? call("os_update_status") : Promise.resolve(mock.osUpdate())),
  osUpdateCheck: (): Promise<void> => (isTauri ? call("os_update_check") : mock.osUpdateCheck()),
  osRestoreAvailable: (): Promise<boolean> => (isTauri ? call("os_restore_available") : Promise.resolve(mock.osRestoreAvailable())),
  osRestore: (reboot: boolean): Promise<string> => (isTauri ? call("os_restore", { reboot }) : mock.osRestore(reboot)),
  osBootMenuNext: (): Promise<void> => (isTauri ? call("os_boot_menu_next") : mock.osBootMenuNext()),
  batteryCare: (): Promise<BatteryCare | null> => (isTauri ? call("battery_care_get") : Promise.resolve(mock.batteryCare())),
  setBatteryCare: (on: boolean): Promise<void> => (isTauri ? call("battery_care_set", { on }) : mock.setBatteryCare(on)),
  levels: (): Promise<Levels> => (isTauri ? call("levels_get") : Promise.resolve(mock.levels())),
  setLevels: (brightness?: number, volume?: number): Promise<void> =>
    isTauri ? call("levels_set", { brightness: brightness ?? null, volume: volume ?? null }) : mock.setLevels(brightness, volume),
  power: (action: "suspend" | "reboot" | "poweroff" | "desktop"): Promise<void> =>
    isTauri ? call("power", { action }) : mock.toastOnly(`Power: ${action}`),
  /** Charger and audio state for auto-sleep. */
  sleepState: (): Promise<{ onBattery: boolean; audioPlaying: boolean }> =>
    isTauri ? call("sleep_state") : Promise.resolve({ onBattery: true, audioPlaying: false }),
  /** The device woke from sleep (seconds asleep). */
  onResume: async (cb: () => void): Promise<() => void> => {
    if (!isTauri) return () => {};
    const { listen } = await import("@tauri-apps/api/event");
    return listen("system-resume", () => cb());
  },
  /** InputPlumber's view of the pad (Settings > Controls). */
  inputStatus: (): Promise<{ available: boolean; name: string | null; targets: string[]; motion: boolean }> =>
    isTauri ? call("input_status") : Promise.resolve({ available: false, name: null, targets: [], motion: false }),
  /** Which Controls settings take effect (InputPlumber 0.83 limits); `why` explains the ones that don't. */
  inputControls: (): Promise<{ key: string; applies: boolean; why: string }[]> =>
    isTauri ? call("input_controls") : Promise.resolve([]),
  config: (): Promise<Config> => (isTauri ? call("config_get") : Promise.resolve(mock.config())),
  setConfig: (value: Config): Promise<void> => (isTauri ? call("config_set", { value }) : mock.setConfig(value)),
  artLookup: (title: string): Promise<string[]> => (isTauri ? call("art_lookup", { title }) : Promise.resolve([])),
  /** Opens a web game; resolves to a browser name when its profile sends it to Chrome. */
  webgameOpen: (id: string, title: string, url: string): Promise<string | null> =>
    isTauri ? call("webgame_open", { id, title, url }) : (window.open(url, "_blank"), Promise.resolve(null)),
  walletSetSession: (session: { address: string | null; label: string | null; kind: string | null; publicKey: string | null }): Promise<void> =>
    isTauri ? call("wallet_set_session", { session }) : Promise.resolve(),
  /** WebKit's renderer (render.rs): "cpu", "gpu", ..., or "desktop" outside gamescope. */
  renderMode: (): Promise<string> => (isTauri ? call<string>("render_mode") : Promise.resolve("desktop")),
  debugEnv: (): Promise<{ nav: string | null; input?: boolean }> => (isTauri ? call("debug_env") : Promise.resolve({ nav: new URLSearchParams(location.search).get("nav"), input: new URLSearchParams(location.search).has("debuginput") })),
  /** The game opened with `rare-launcher --dev-game` (developer mode), or null. */
  devGame: (): Promise<DevGame | null> => (isTauri ? call("dev_game_info") : Promise.resolve(null)),
  /** tools/journeys probe: on only while $XDG_RUNTIME_DIR/rare-journey exists (src-tauri/src/journey.rs). */
  journeyProbe: (): Promise<boolean> => (isTauri ? call("journey_probe") : Promise.resolve(new URLSearchParams(location.search).has("journey"))),
  journeyLog: (lines: string[]): Promise<void> => (isTauri ? call("journey_log", { lines }) : (lines.forEach((l) => console.log(`[journey] ${l}`)), Promise.resolve())),
  journeyCmd: (): Promise<string | null> => (isTauri ? call("journey_cmd") : Promise.resolve(null)),
  /** A toast drawn over whatever has the screen (the HUD, hud.rs), for when a game or app is in front. */
  hudToast: (title: string, body?: string | null): Promise<void> => (isTauri ? call("hud_toast", { title, body: body ?? null }) : Promise.resolve()),
  debugLog: (msg: string): Promise<void> => (isTauri ? call("debug_log", { msg }) : (console.log(`[ui] ${msg}`), Promise.resolve())),
  openWeb: (url: string): Promise<void> => (isTauri ? call("open_web", { url }) : (window.open(url, "_blank"), Promise.resolve())),
  /** GET https://rare.market{path} */
  rareGet: async <T>(path: string): Promise<T> => {
    if (isTauri) return call<T>("rare_get", { path });
    const r = await fetch(`/rare${path}`);
    if (!r.ok) throw new Error(`rare.market ${r.status}`);
    const v = (await r.json()) as T;
    remember(`rare:${path}`, v);
    return v;
  },
  /** POST JSON to https://rare.market{path} (only /v1/sponsor routes). Any status comes back as
   * `{ status, body }` so API error codes reach the caller; network failures throw. */
  rarePost: async <T>(path: string, body: unknown): Promise<{ status: number; body: T }> => {
    if (isTauri) return call<{ status: number; body: T }>("rare_post", { path, body });
    const r = await fetch(`/rare${path}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
    const text = await r.text();
    let parsed: unknown = null;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      parsed = null;
    }
    return { status: r.status, body: parsed as T };
  },
};

export type DevGame = { url: string; slug: string; id: string; title: string; collections: string[] };
export type ProtonTool = { name: string; label: string; kind: "valve" | "experimental" | "ge" | "cachyos" | "other"; stale: boolean };
export type ProtonStatus = { tools: ProtonTool[]; global: string | null; game: string | null; installable: Array<"ge" | "cachyos"> };

export async function onProtonInstall(cb: (p: { kind: string; phase: string; done: number; total: number }) => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ kind: string; phase: string; done: number; total: number }>("proton-install", (e) => cb(e.payload));
}

export async function onPad(cb: (b: PadButton, repeat: boolean) => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ button: PadButton; repeat: boolean }>("pad", (e) => cb(e.payload.button, e.payload.repeat));
}

/** Right stick, 0..1 per axis with down positive; (0,0) when released. */
export async function onPadStick(cb: (x: number, y: number) => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ x: number; y: number }>("padstick", (e) => cb(e.payload.x, e.payload.y));
}

export async function onWalletConnected(cb: (game: string) => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ game: string }>("wallet-connected", (e) => cb(e.payload.game));
}

export async function onGameState(cb: (running: string[]) => void): Promise<Unlisten> {
  if (!isTauri) return mock.onRunning(cb);
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ running: string[] }>("game-state", (e) => cb(e.payload.running));
}

/** Steam's UI is about to take the screen in `ms` (apps::steam): show how to come back. */
export async function onSteamHandoff(cb: (ms: number) => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<number>("steam-handoff", (e) => cb(e.payload));
}

/** A start that went wrong (launch.rs LaunchFailed): never a silent nothing. */
export interface LaunchFailed {
  id: string;
  /** Didn't start: steam, steamui, never, quick, stuck. Closed unexpectedly: crash. */
  reason: "steam" | "steamui" | "never" | "quick" | "stuck" | "crash";
  code: number | null;
}

export async function onLaunchFailed(cb: (e: LaunchFailed) => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<LaunchFailed>("launch-failed", (e) => cb(e.payload));
}

/** The native side saw Steam libraries, shortcuts or game folders change on disk. */
export async function onLibraryChanged(cb: () => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen("library-changed", () => cb());
}
