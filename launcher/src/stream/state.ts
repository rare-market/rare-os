// PC streaming state: PCs, their games, pairing, Moonlight's install, and Play / Resume / Quit.
// Native side: src-tauri/src/stream. In a plain browser the same calls answer from ./mock.

import { create } from "zustand";
import { isTauri } from "../lib/bridge";
import * as appsMock from "../lib/mock";
import { useApp } from "../lib/store";
import { rareWay } from "../lib/guest";
import { poll } from "../lib/idle";
import { DEFAULT_SETTINGS, homeHost, parseRunId, playState, runId } from "./logic";
import * as mock from "./mock";
import type { Decode, Host, Offer, PairView, Status, StreamApp, StreamSettings } from "./types";
import { failed } from "../lib/fail";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

const api = {
  status: () => (isTauri ? call<Status>("stream_status") : mock.status()),
  refresh: () => (isTauri ? call<Host[]>("stream_refresh") : mock.refresh()),
  discover: () => (isTauri ? call<Host[]>("stream_discover", { ms: 2500 }) : mock.discover()),
  add: (address: string) => (isTauri ? call<Host>("stream_add", { address }) : mock.add(address)),
  pairStart: (address: string) => (isTauri ? call<PairView>("stream_pair_start", { address }) : mock.pairStart(address)),
  pairPoll: () => (isTauri ? call<PairView | null>("stream_pair_poll") : mock.pairPoll()),
  pairCancel: () => (isTauri ? call<void>("stream_pair_cancel") : mock.pairCancel()),
  apps: (uuid: string) => (isTauri ? call<StreamApp[]>("stream_apps", { uuid }) : mock.apps(uuid)),
  launch: (uuid: string, appId: number, replace: boolean) => (isTauri ? call<void>("stream_launch", { uuid, appId, replace }) : appsMock.launch(runId(uuid, appId))),
  resume: (id: string) => (isTauri ? call<void>("stream_resume", { id }) : Promise.resolve()),
  quit: (uuid: string) => (isTauri ? call<void>("stream_quit", { uuid }) : (useApp.getState().running.filter((r) => r.startsWith(`stream:${uuid}:`)).forEach((r) => void appsMock.quit(r)), mock.quitHost(uuid))),
  wake: (uuid: string) => (isTauri ? call<void>("stream_wake", { uuid }) : mock.wake(uuid)),
  setSettings: (settings: StreamSettings) => (isTauri ? call<void>("stream_settings_set", { settings }) : mock.setSettings(settings)),
  install: () => (isTauri ? call<void>("stream_install") : Promise.resolve()),
};

interface StreamState {
  loaded: boolean;
  moonlight: string | null;
  offer: Offer | null;
  decode: Decode | null;
  settings: StreamSettings;
  hosts: Host[];
  apps: Record<string, StreamApp[]>;
  /** Looking for PCs on the network. */
  scanning: boolean;
  /** Listing a PC's games. */
  listing: string | null;
  /** Waking a PC (uuid). */
  waking: string | null;
  pairing: PairView | null;
  /** Installing Moonlight: 0 to 1, null while flatpak hasn't said. */
  installing: { progress: number | null; error?: string } | null;
  load: () => Promise<void>;
  refresh: () => Promise<void>;
  scan: () => Promise<void>;
  add: (address: string) => Promise<Host | null>;
  pair: (host: Host) => Promise<void>;
  cancelPair: () => Promise<void>;
  listApps: (uuid: string) => Promise<void>;
  /** Play: wakes the PC if needed; `replace` quits what the PC runs first. */
  play: (host: Host, app: StreamApp, replace?: boolean) => Promise<void>;
  resume: (host: Host, app: StreamApp) => Promise<void>;
  quit: (host: Host) => Promise<void>;
  wake: (host: Host) => Promise<boolean>;
  setSettings: (s: StreamSettings) => Promise<void>;
  install: () => Promise<void>;
}

const toast = (t: string, tone?: "info" | "error") => useApp.getState().toast(t, tone);
const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
let pairPoll: ReturnType<typeof setInterval> | null = null;

export const useStream = create<StreamState>((set, get) => ({
  loaded: false,
  moonlight: null,
  offer: null,
  decode: null,
  settings: DEFAULT_SETTINGS,
  hosts: [],
  apps: {},
  scanning: false,
  listing: null,
  waking: null,
  pairing: null,
  installing: null,
  load: async () => {
    try {
      const s = await api.status();
      set({ loaded: true, moonlight: s.moonlight, offer: s.offer, decode: s.decode, settings: s.settings, hosts: s.hosts, apps: s.apps });
    } catch {
      set({ loaded: true });
    }
    void get().refresh();
  },
  refresh: async () => {
    const hosts = await api.refresh().catch(() => null);
    if (!hosts) return;
    set({ hosts });
    // A paired PC whose games Rare hasn't listed yet.
    for (const h of hosts) if (h.paired && h.online && !get().apps[h.uuid]?.length && get().moonlight) void get().listApps(h.uuid);
  },
  scan: async () => {
    if (get().scanning) return;
    set({ scanning: true });
    try {
      set({ hosts: await api.discover() });
    } catch (e) {
      toast(failed("Couldn't look for PCs.", e), "error");
    } finally {
      set({ scanning: false });
    }
  },
  add: async (address) => {
    try {
      const h = await api.add(address);
      set({ hosts: [...get().hosts.filter((x) => x.uuid !== h.uuid), h] });
      return h;
    } catch (e) {
      toast(failed("Couldn't add that PC. Check the address and try again.", e, ""), "error");
      return null;
    }
  },
  pair: async (host) => {
    if (!host.address) return toast(`Rare doesn't know where ${host.name} is. Add it by address.`, "error");
    try {
      const p = await api.pairStart(host.address);
      set({ pairing: p });
      if (p.phase === "paired") return void paired(p);
      if (pairPoll) clearInterval(pairPoll);
      pairPoll = setInterval(async () => {
        const v = await api.pairPoll().catch(() => null);
        if (!v || v.pin !== get().pairing?.pin) return;
        set({ pairing: v });
        if (v.phase !== "waiting" && pairPoll) {
          clearInterval(pairPoll);
          pairPoll = null;
          if (v.phase === "paired") paired(v);
        }
      }, 1000);
    } catch (e) {
      toast(failed("Couldn't pair with that PC.", e), "error");
    }
  },
  cancelPair: async () => {
    if (pairPoll) clearInterval(pairPoll);
    pairPoll = null;
    set({ pairing: null });
    await api.pairCancel().catch(() => {});
  },
  listApps: async (uuid) => {
    if (get().listing === uuid) return;
    set({ listing: uuid });
    try {
      const list = await api.apps(uuid);
      set({ apps: { ...get().apps, [uuid]: list } });
    } catch (e) {
      toast(failed("Couldn't list the games on that PC.", e), "error");
    } finally {
      set({ listing: null });
    }
  },
  play: async (host, app, replace = false) => {
    const rid = runId(host.uuid, app.id);
    if (useApp.getState().running.includes(rid)) return get().resume(host, app);
    let h = host;
    if (!h.online) {
      if (!h.canWake) return toast(`${h.name} isn't answering. Turn it on, then try again.`, "error");
      if (!(await get().wake(h))) return;
      h = get().hosts.find((x) => x.uuid === host.uuid) ?? h;
    }
    useApp.setState({ launching: rid });
    try {
      await api.launch(h.uuid, app.id, replace || (h.currentGame != null && h.currentGame !== app.id));
      toast(`Streaming ${app.name} from ${h.name}. ${rareWay()} to come back to Rare.`);
    } catch (e) {
      toast(failed("Couldn't start the game.", e), "error");
    } finally {
      setTimeout(() => useApp.setState({ launching: null }), 1500);
    }
  },
  resume: async (host, app) => {
    try {
      await api.resume(runId(host.uuid, app.id));
    } catch (e) {
      toast(failed("Couldn't resume the game.", e), "error");
    }
  },
  quit: async (host) => {
    toast(`Quitting the game on ${host.name}`);
    try {
      await api.quit(host.uuid);
      set({ hosts: get().hosts.map((h) => (h.uuid === host.uuid ? { ...h, currentGame: null } : h)) });
    } catch (e) {
      toast(failed("Couldn't close the game.", e), "error");
    }
  },
  wake: async (host) => {
    set({ waking: host.uuid });
    try {
      await api.wake(host.uuid);
      // A PC takes a while to come out of sleep: ask every 3 s for a minute.
      for (let i = 0; i < 20; i++) {
        await pause(3000);
        await get().refresh();
        if (get().hosts.find((h) => h.uuid === host.uuid)?.online) return true;
      }
      toast(`${host.name} didn't wake up. Check Wake-on-LAN is on in its BIOS and network settings.`, "error");
      return false;
    } catch (e) {
      toast(failed("Couldn't wake that PC.", e), "error");
      return false;
    } finally {
      set({ waking: null });
    }
  },
  setSettings: async (s) => {
    set({ settings: s });
    await api.setSettings(s).catch((e) => toast(failed("Couldn't save the streaming settings.", e), "error"));
  },
  install: async () => {
    set({ installing: { progress: null } });
    try {
      await api.install();
      if (!isTauri) void mockInstall();
    } catch (e) {
      installDone(failed("Couldn't set up streaming.", e, "Check your Wi-Fi and try again."));
    }
  },
}));

function paired(p: PairView) {
  toast(`${p.name} is paired`);
  void useStream.getState().load();
  void useStream.getState().listApps(p.uuid);
}

function installDone(error?: string) {
  useStream.setState({ installing: error ? { progress: null, error } : null });
  if (error) toast(error, "error");
  else toast("Moonlight is installed");
  void useStream.getState().load();
}

async function mockInstall() {
  for (let p = 0; p <= 100; p += 5) {
    await pause(100);
    useStream.setState({ installing: { progress: p / 100 } });
  }
  await mock.markInstalled();
  installDone();
}

// Moonlight installs through the Apps installer: its events carry id "moonlight".
if (isTauri)
  void import("@tauri-apps/api/event").then(({ listen }) =>
    listen<{ id: string; progress: number | null; done: boolean; error: string | null }>("app-install", ({ payload: p }) => {
      if (p.id !== "moonlight") return;
      if (p.done) return installDone(p.error ?? undefined);
      useStream.setState({ installing: { progress: p.progress } });
    }),
  );

/** What a running id is, for the status bar and the running list. */
export function streamTitle(rid: string): string | null {
  const r = parseRunId(rid);
  if (!r) return null;
  const s = useStream.getState();
  const host = s.hosts.find((h) => h.uuid === r.uuid);
  const app = s.apps[r.uuid]?.find((a) => a.id === r.app);
  return app ? app.name : host ? `A game on ${host.name}` : "PC game";
}

/** The host and app behind a running id. */
export function streamOf(rid: string): { host: Host; app: StreamApp } | null {
  const r = parseRunId(rid);
  if (!r) return null;
  const s = useStream.getState();
  const host = s.hosts.find((h) => h.uuid === r.uuid);
  const app = s.apps[r.uuid]?.find((a) => a.id === r.app);
  return host && app ? { host, app } : null;
}

// Screenshots: ?streampair starts pairing with the mocked unpaired PC.
if (!isTauri && typeof location !== "undefined" && new URLSearchParams(location.search).has("streampair")) setTimeout(() => void useStream.getState().pair({ ...mock.FOUND }), 500);

/** Load once, then keep PCs' online state fresh every 30 s while Rare is in front and nothing runs. */
let started = false;
export function startStream() {
  if (started) return;
  started = true;
  void useStream.getState().load();
  // Every 30 s in use, every 5 minutes idle, none while away (a game or another window in front).
  poll(
    () => {
      const busy = useApp.getState().running.length > 0 && !document.hasFocus();
      if (!busy && document.visibilityState !== "hidden") void useStream.getState().refresh();
    },
    { active: 30_000, idle: 300_000, away: null },
  );
  if (typeof window !== "undefined") window.addEventListener("focus", () => void useStream.getState().refresh());
}

export { homeHost, playState };
