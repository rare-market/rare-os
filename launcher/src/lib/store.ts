import { create } from "zustand";
import { native, onGameState, onLaunchFailed, onLibraryChanged, type LaunchFailed } from "./bridge";
import { appByRunId } from "../apps/catalog";
import { restoreLater, spot, type Spot } from "../nav/focus";
import { deliberateActivation } from "../nav/input";
import type { DeviceInfo, Game, Library } from "./types";

/** Friends and Settings are destinations, not tabs: they are not in the tab bar (TABS), but B and LB / RB still work from them. */
export type Tab = "home" | "library" | "games" | "market" | "wallet" | "friends" | "settings" | "store";
import { isGuest } from "./guest";
import { failed } from "./fail";

/** The five top tabs. Trade is a section of Market; the Rare Store is hidden (store/show.ts) and joins Sui when it ships. */
export const TABS: Array<{ id: Tab; label: string }> = [
  { id: "home", label: "Home" },
  { id: "library", label: "Library" },
  { id: "games", label: "Sui" },
  { id: "market", label: "Market" },
  { id: "wallet", label: "Wallet" },
];

export type Overlay =
  | { kind: "game"; id: string }
  | { kind: "suigame"; slug: string }
  /** A Rare Store game page (src/store). */
  | { kind: "storegame"; slug: string }
  | { kind: "qam" }
  | { kind: "signin" }
  | { kind: "firstrun" }
  | { kind: "search" }
  | { kind: "profile" }
  | { kind: "saves" }
  | { kind: "app"; id: string }
  /** The Switcher over the launcher (the Rare button held, or View + Menu). */
  | { kind: "running" }
  /** The Rare menu over the launcher (the Rare button). */
  | { kind: "raremenu" }
  /** PC streaming: find PCs and pair. */
  | { kind: "stream" }
  /** A game on a paired PC. */
  | { kind: "streamapp"; host: string; app: number }
  | { kind: "share" }
  /** The trophy case, at one Steam game when `appid` is set; B goes back to `from`. */
  | { kind: "trophies"; appid?: number; from?: Overlay }
  /** A game or app that didn't start (launch.rs LaunchFailed): Try again, or B. */
  | { kind: "launchfail"; id: string; reason: string; code: number | null }
  | null;

interface Toast {
  id: number;
  text: string;
  tone: "info" | "error";
}

interface State {
  tab: Tab;
  overlay: Overlay;
  library: Library | null;
  scanning: boolean;
  device: DeviceInfo | null;
  running: string[];
  launching: string | null;
  toasts: Toast[];
  /** Tabs visited before this one, for B. */
  history: Tab[];
  setTab: (t: Tab, opts?: { back?: boolean }) => void;
  open: (o: Overlay) => void;
  close: () => void;
  /** B: close the overlay, else go back to the previous tab. Returns false when there is nowhere to go. */
  back: () => boolean;
  scan: (opts?: { quiet?: boolean }) => Promise<void>;
  launch: (g: Game) => Promise<void>;
  /** Back to a running game. */
  resume: (g: Game) => Promise<void>;
  quit: (g: Game) => Promise<void>;
  toast: (text: string, tone?: Toast["tone"], ms?: number) => void;
  refreshDevice: () => Promise<void>;
}

let toastId = 0;
let inflight = false;
// Where focus was on each tab and under each overlay, so going back lands in the same place.
const tabSpots = new Map<Tab, Spot | null>();
const overlaySpots: Array<Spot | null> = [];

export const useApp = create<State>((set, get) => ({
  tab: "home",
  overlay: null,
  library: null,
  scanning: false,
  device: null,
  running: [],
  launching: null,
  toasts: [],
  history: [],
  setTab: (tab, opts) => {
    const cur = get().tab;
    if (tab === cur) return;
    if (!get().overlay) tabSpots.set(cur, spot());
    const history = opts?.back ? get().history.slice(0, -1) : [...get().history.filter((t) => t !== tab), cur].slice(-8);
    set({ tab, history });
    restoreLater(tabSpots.get(tab) ?? null);
  },
  open: (overlay) => {
    const was = get().overlay;
    if (!was && overlay) overlaySpots.push(spot());
    if (was && !overlay) restoreLater(overlaySpots.pop() ?? null);
    set({ overlay });
  },
  close: () => {
    if (get().overlay) restoreLater(overlaySpots.pop() ?? null);
    set({ overlay: null });
  },
  back: () => {
    const s = get();
    if (s.overlay) {
      s.close();
      return true;
    }
    const prev = s.history[s.history.length - 1];
    if (prev) {
      s.setTab(prev, { back: true });
      return true;
    }
    if (s.tab !== "home") {
      s.setTab("home", { back: true });
      return true;
    }
    return false;
  },
  scan: async (opts) => {
    if (inflight) return;
    inflight = true;
    // First open: show the last scan from disk at once, then refresh it.
    if (!get().library) {
      const cached = await native.cachedLibrary().catch(() => null);
      if (cached && !get().library) set({ library: cached });
    }
    set({ scanning: !opts?.quiet && !get().library });
    try {
      set({ library: await native.scanLibrary() });
    } catch (e) {
      get().toast(failed("Couldn't read your library.", e, "Scan again from the Library tab."), "error");
    } finally {
      inflight = false;
      set({ scanning: false });
    }
  },
  resume: async (g) => {
    try {
      await native.resume(g.id);
    } catch (e) {
      get().toast(failed(`Couldn't switch back to ${g.title}.`, e), "error");
    }
  },
  quit: async (g) => {
    try {
      await native.quit(g.id);
      get().toast(`Closing ${g.title}`);
    } catch (e) {
      get().toast(failed(`Couldn't close ${g.title}.`, e), "error");
    }
  },
  launch: async (g) => {
    if (!launchAllowed()) return;
    if (get().running.includes(g.id)) return get().resume(g);
    set({ launching: g.id });
    try {
      // Rare Store games with launch_check: the licence is checked first (src/store/actions.ts).
      if (g.id.startsWith("rare:")) await (await import("../store/actions")).beforeLaunch(g.id);
      await native.launch(g.id);
      get().toast(`Starting ${g.title}`);
      exitTipOnce();
    } catch (e) {
      get().toast(failed(`${g.title} didn't start.`, e), "error");
    } finally {
      setTimeout(() => set({ launching: null }), 1500);
    }
  },
  toast: (text, tone = "info", ms = 3200) => {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, text, tone }] });
    setTimeout(() => set({ toasts: get().toasts.filter((t) => t.id !== id) }), ms);
  },
  refreshDevice: async () => set({ device: await native.device() }),
}));

// Games still running from before the launcher (re)started.
void native.running().then((running) => running.length && useApp.setState({ running }));
void onGameState((running) => {
  const was = useApp.getState().running.length;
  useApp.setState({ running });
  // A game just closed: playtime and last-played changed.
  if (running.length < was) void useApp.getState().scan({ quiet: true });
});
// A start that went wrong always says so (launch.rs failed_start): a crash is a toast, a game that
// didn't start opens a sheet with Try again. Never a silent nothing.
void onLaunchFailed((e) => {
  const s = useApp.getState();
  const name = runName(e.id);
  if (e.reason === "crash") return s.toast(`${name} closed unexpectedly.`, "error", 6000);
  // First run and sign-in keep the screen; the toast still says it.
  if (s.overlay?.kind === "firstrun" || s.overlay?.kind === "signin") return s.toast(`${name} didn't start.`, "error", 6000);
  s.open({ kind: "launchfail", id: e.id, reason: e.reason, code: e.code });
});

/** What a run id is called in the UI: an app, a game, a stream. */
export function runName(id: string): string {
  if (id.startsWith("app:")) return appByRunId(id)?.name ?? "The app";
  if (id.startsWith("stream:")) return "The stream";
  return gameById(id)?.title ?? "The game";
}

/** The sheet's second line: what happened and the next step. */
export function launchFailText(e: Pick<LaunchFailed, "reason" | "code">): string {
  const code = e.code != null && e.code !== -1 ? ` (exit code ${e.code})` : "";
  switch (e.reason) {
    case "steam":
      return "Steam didn't open it. Open Steam to see why: an update, a sign-in or an error.";
    case "steamui":
      return "Steam didn't open. Try again, or restart the device if it keeps happening.";
    case "never":
      return "Heroic didn't start it. Open Heroic in Desktop mode to see why.";
    case "stuck":
      return "It's still running but nothing showed up in 45 seconds. Try again closes it and starts it again.";
    case "quick":
      return `It closed before showing anything${code}. Try again, or check it in Desktop mode.`;
    default:
      return `It closed unexpectedly${code}.`;
  }
}

// Steam touches its folders every few seconds while downloading: rescan at most every 10 s.
let lastChangeScan = 0;
let changeTimer: ReturnType<typeof setTimeout> | null = null;
void onLibraryChanged(() => {
  if (changeTimer) return;
  const wait = Math.max(1500, 10_000 - (Date.now() - lastChangeScan));
  changeTimer = setTimeout(() => {
    changeTimer = null;
    lastChangeScan = Date.now();
    void useApp.getState().scan({ quiet: true });
  }, wait);
});

export const gameById = (id: string) => useApp.getState().library?.games.find((g) => g.id === id);

/**
 * The first game or app started on this device: how to get back out, once. Home (Guide on the
 * pad) always brings Rare back; View + Menu held does too (read natively, see gamepad.rs).
 */
export function exitTipOnce() {
  try {
    if (localStorage.getItem("rare.exitTipShown")) return;
    localStorage.setItem("rare.exitTipShown", "1");
  } catch {
    return;
  }
  // Plain text: the toast has no room for a glyph. The hint bar shows the Rare button drawing.
  useApp.getState().toast(isGuest() ? "Hold View in any game for the Rare menu" : "Press the Rare button in any game for the Rare menu", "info", 8000);
}

/**
 * Games and apps start only from a deliberate A press (or a mouse click): never from a touch, a
 * held button's repeat or focus moving. Owner report: games "randomly" opened while holding the
 * SuiPlay, where the Home Play button sits under the left palm on the touchscreen.
 */
export function launchAllowed(): boolean {
  if (deliberateActivation()) return true;
  useApp.getState().toast("Press A to play");
  void native.debugLog("launch ignored: no A press just before it").catch(() => {});
  return false;
}
