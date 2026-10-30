// Mirrors src-tauri/src/model.rs and system.rs.

export type Source = "steam" | "epic" | "gog" | "itch" | "local" | "web" | "apps" | "rare";

export interface Art {
  cover: string[];
  hero: string[];
  logo: string[];
  header: string[];
  /** Square app icon (emulators and other apps). */
  icon?: string[];
}

export interface Game {
  id: string;
  source: Source;
  title: string;
  installed: boolean;
  installDir?: string | null;
  sizeBytes?: number | null;
  lastPlayed?: number | null;
  playtimeMinutes?: number | null;
  art: Art;
  /** How the native side starts it (model.rs LaunchSpec); web games carry their URL. */
  launch?: { kind: string; url?: string; appid?: number };
  origin: string;
  foreign: boolean;
}

export interface SourceStatus {
  source: Source;
  available: boolean;
  count: number;
  note: string;
}

export interface RomSystem {
  id: string;
  name: string;
  count: number;
  lastPlayed: number | null;
}

export interface RomGame {
  name: string;
  system: string;
  lastPlayed: number | null;
  playCount: number;
  /** Seconds. */
  playtime: number;
}

/** ES-DE's ROM systems and recently played ROMs (src-tauri/src/library/esde.rs). */
export interface Emulation {
  romDir: string;
  systems: RomSystem[];
  recent: RomGame[];
}

export interface Library {
  games: Game[];
  sources: SourceStatus[];
  foreignSteamLibraries: string[];
  scannedAt: number;
  emulation?: Emulation | null;
  /** Id of the ES-DE entry in games. */
  emulationApp?: string | null;
}

export interface DeviceInfo {
  product: string;
  vendor: string;
  name: string;
  isSuiplay: boolean;
  isVm: boolean;
  osName: string;
  osVersion: string;
  hostname: string;
  underGamescope: boolean;
  /** health: full-charge capacity against design, percent; null when the battery doesn't say. */
  battery: { percent: number; charging: boolean; health?: number | null; /** minutes left, or to full while charging */ minutes?: number | null } | null;
  wifi: string | null;
  platform: string;
}

/** rare-fand's battery care (org.rare.Battery). */
export interface BatteryCare {
  /** The driver can hold the charge. */
  available: boolean;
  on: boolean;
  state: "off" | "unavailable" | "unplugged" | "low" | "hot" | "unknown" | "charging" | "holding";
}

export type Profile = "quiet" | "balanced" | "performance" | "custom";

/** Fan presets; "custom" is a curve set outside the launcher. */
export type FanMode = "default" | "cool" | "quiet" | "custom";

export interface Perf {
  available: boolean;
  tdp: number | null;
  tdpMin: number;
  tdpMax: number;
  fpsLimit: number | null;
  profile: Profile;
  /** Why something is unavailable; empty when all is well. */
  note: string;
  backend: "powerstation" | "hhd" | null;
  /** null when CPU boost can't be changed on this system. */
  cpuBoost: boolean | null;
  /** null when the fan can't be controlled on this system. */
  fan: FanMode | null;
  /** What each profile sets on this device, watts (its limit within the device's range). */
  profileWatts?: { quiet: number; balanced: number; performance: number };
}

export interface Levels {
  brightness: number | null;
  volume: number | null;
  muted: boolean;
}

/** One game's power settings (per-game power, saved with "Apply to this game"). */
export interface GamePower {
  profile: Profile;
  tdp?: number | null;
  cpuBoost?: boolean | null;
  /** gamescope frame limit; null is no limit. */
  fps?: number | null;
  /** Performance overlay level for this game. */
  hud?: HudLevel;
}

/** The performance overlay (mangoapp): Off, FPS, Basic, Full. */
export type HudLevel = "off" | "fps" | "basic" | "full";

/** The running game quick settings change, and whether its settings are saved for next time. */
export interface GamePowerView {
  id: string;
  saved: boolean;
}

export interface Config {
  /** Interface language code from lib/locales.ts. "en" by default. */
  language?: string;
  gameFolders: string[];
  steamgriddbKey?: string | null;
  perfProfile?: Profile | null;
  perfTdp?: number | null;
  perfCpuBoost?: boolean | null;
  steamInBackground?: boolean | null;
  /** When Steam starts quietly: "focus" (default), "boot" or "demand". */
  steamStart?: "focus" | "boot" | "demand" | null;
  onboarded: boolean;
  webGames: WebGame[];
  removedBuiltins?: string[];
  uiSounds?: boolean | null;
  rumble?: boolean | null;
  /** Gyro in Steam games (the pad shows up as a Steam Deck while Steam runs a game); off when unset. */
  gyroInSteam?: boolean | null;
  /** Gyro master switch (on when unset) and sensitivity in percent (100 when unset). */
  gyroEnabled?: boolean | null;
  gyroSensitivity?: number | null;
  /** Rumble strength in percent (100 when unset). */
  rumbleStrength?: number | null;
  /** Stick deadzones in percent (10 when unset) and vertical inversion. */
  deadzoneLeft?: number | null;
  deadzoneRight?: number | null;
  invertLeftY?: boolean | null;
  invertRightY?: boolean | null;
  /** What LC and RC do on a press (LC capture and RC keyboard when unset). */
  lcAction?: ButtonAction | null;
  rcAction?: ButtonAction | null;
  /** First-run step to resume at after a restart. */
  onboardingStep?: string | null;
  /** Power settings per game (run id). Kept by the native side; settings writes never change it. */
  gamePower?: Record<string, GamePower>;
  /** Idle minutes before sleep on battery (default 15, 0 = never) and while charging (default never). */
  sleepIdleBattery?: number | null;
  sleepIdleAc?: number | null;
  /** UI size on top of the automatic one; automatic when unset. */
  uiScale?: "large" | "larger" | null;
  /** Developer setting: the Sui network ("mainnet" when unset). */
  network?: "mainnet" | "testnet" | null;
  /** Testnet overrides: Sui RPC, rare.market API base, gas sponsor base. */
  testnetRpc?: string | null;
  testnetApi?: string | null;
  testnetSponsor?: string | null;
}

export interface WebGame {
  id: string;
  title: string;
  url: string;
  cover?: string | null;
  hero?: string | null;
}

export type PadButton =
  | "up" | "down" | "left" | "right"
  | "a" | "b" | "x" | "y"
  | "lb" | "rb" | "lt" | "rt"
  | "start" | "select" | "guide" | "qam"
  /** The SuiPlay's own keys: Rare (Aya Space), Quick, and the small shoulder keys LC and RC. */
  | "rare" | "quick" | "lc" | "rc"
  /** Guide held: offers to close what is running. */
  | "guidehold"
  /** View + Menu (Select + Start) held: the way out of a game; the launcher shows what is running. */
  | "exithold"
  /** Hint bar only: LB/RB (the launcher's tabs) and LT/RT (the screen's sections) as one hint. */
  | "lbrb"
  | "ltrt";

export const SOURCE_LABEL: Record<Source, string> = {
  steam: "Steam",
  epic: "Epic Games",
  gog: "GOG",
  itch: "itch.io",
  local: "Local",
  web: "Web",
  apps: "Emulators",
  rare: "Rare Store",
};

/** A path as the UI shows it: home folders (any user, including the old system's) become ~. */
export const prettyPath = (p: string) => p.replace(/\/(?:var\/)?home\/[^/]+/g, "~");

export interface WifiNet {
  ssid: string;
  signal: number;
  secure: boolean;
  active: boolean;
}

export interface BluetoothAdapter {
  present: boolean;
  powered: boolean;
  blocked: boolean;
}

/** Quick Settings' radio tiles. wifi/bluetooth are null when the device has none. */
export interface Radios {
  wifi: boolean | null;
  bluetooth: boolean | null;
  airplane: boolean;
}

export interface NetStatus {
  online: boolean;
  wifi: string | null;
  /** false when NetworkManager isn't available (only status can be shown) */
  managed: boolean;
}

/** Gamescope upscaling. width/height 0 is native. Plain record so the per-game profile can keep one. */
export interface Upscale {
  mode: "off" | "fsr" | "nis";
  /** 0 soft to 20 sharp. */
  sharpness: number;
  width: number;
  height: number;
}

export interface EmuInfo {
  id: string;
  name: string;
  path: string | null;
  modified: number | null;
  tag: string | null;
  power: "quiet" | "balanced" | "performance" | "custom";
  tuned: boolean;
  releasesUrl: string;
}
export interface EmuUpdateInfo {
  id: string;
  tag: string;
  published: string;
  page: string;
  asset: string;
  size: number;
  verify: "checksum" | "none";
  available: boolean;
}
export interface EmuUpdateDone {
  id: string;
  tag: string;
  path: string;
  backup: string | null;
  verified: "checksum" | "sizeOnly";
}

/** One DRM connector (display_state). */
export interface DisplayOutput {
  name: string;
  connected: boolean;
  lit: boolean;
  internal: boolean;
  label?: string | null;
  mode?: string | null;
  refresh?: number | null;
}
export interface DisplayState {
  outputs: DisplayOutput[];
  active?: string | null;
  docked: boolean;
  dockName?: string | null;
  dockMode?: string | null;
}

/** OS update state (native osupdate.rs): what runs, what waits for a restart, the last check. */
export interface OsUpdate {
  bootedVersion: string | null;
  staged: boolean;
  stagedVersion: string | null;
  /** Unix seconds. */
  checkedAt: number | null;
  checkOk: boolean | null;
  checkError: string | null;
}

export type ButtonAction = "capture" | "keyboard" | "perf" | "none";
