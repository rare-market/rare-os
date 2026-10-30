// Apps on this device: status from the native side (src-tauri/src/apps), opening, installs
// with progress. In a plain browser (dev:ui, screenshots) the same states come from
// `statusFor` over a mock list of installed Flatpaks (?flatpaks=a,b overrides it).

import { create } from "zustand";
import { isTauri, native } from "../lib/bridge";
import * as mock from "../lib/mock";
import { exitTipOnce, launchAllowed, useApp } from "../lib/store";
import { rareWay } from "../lib/guest";
import { APPS, appById, runId, statusFor, type AppEntry, type AppStatus } from "./catalog";
import { failed } from "../lib/fail";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
// The owner's SuiPlay: Google Chrome and Telegram Desktop installed, Discord not.
let mockInstalled = params.get("flatpaks")?.split(",").filter(Boolean) ?? ["com.google.Chrome", "org.telegram.desktop"];

export interface InstallState {
  /** 0 to 1, or null while flatpak hasn't said. */
  progress: number | null;
  error?: string;
}

interface AppsState {
  status: Record<string, AppStatus>;
  installing: Record<string, InstallState>;
  refresh: () => Promise<void>;
  /** A on a tile: open, resume, or (nothing can open it yet) the app's page. */
  activate: (app: AppEntry) => void;
  open: (app: AppEntry, way?: number) => Promise<void>;
  resume: (app: AppEntry) => Promise<void>;
  close: (app: AppEntry) => Promise<void>;
  install: (app: AppEntry) => Promise<void>;
}

export const useApps = create<AppsState>((set, get) => ({
  status: {},
  installing: {},
  refresh: async () => {
    const list: AppStatus[] = isTauri ? await call<AppStatus[]>("apps_status").catch(() => []) : APPS.map((a) => statusFor(a, mockInstalled));
    set({ status: Object.fromEntries(list.map((s) => [s.id, s])) });
  },
  activate: (app) => {
    const s = get().status[app.id];
    if (useApp.getState().running.includes(runId(app.id))) return void get().resume(app);
    if (!s || s.way == null || get().installing[app.id]) return useApp.getState().open({ kind: "app", id: app.id });
    void get().open(app);
  },
  open: async (app, way) => {
    if (!launchAllowed()) return;
    const toast = useApp.getState().toast;
    const rid = runId(app.id);
    useApp.setState({ launching: rid });
    try {
      if (isTauri) await call<string>("app_open", { id: app.id, way: way ?? null });
      else await mock.launch(rid);
      const s = get().status[app.id];
      const pointer = (way != null ? app.ways[way]?.controller : s?.controller) === "pointer";
      toast(pointer ? `${app.name}: the right stick moves the pointer. ${rareWay()} to come back to Rare.` : `Opening ${app.name}`);
      exitTipOnce();
    } catch (e) {
      toast(failed(`${app.name} didn't start.`, e), "error");
    } finally {
      setTimeout(() => useApp.setState({ launching: null }), 1500);
    }
  },
  resume: async (app) => {
    try {
      if (isTauri) await call("app_resume", { id: app.id });
    } catch (e) {
      useApp.getState().toast(failed(`Couldn't switch back to ${app.name}.`, e), "error");
    }
  },
  close: async (app) => {
    try {
      await native.quit(runId(app.id));
      useApp.getState().toast(`Closing ${app.name}`);
    } catch (e) {
      useApp.getState().toast(failed(`Couldn't close ${app.name}.`, e), "error");
    }
  },
  install: async (app) => {
    set({ installing: { ...get().installing, [app.id]: { progress: null } } });
    try {
      if (isTauri) await call("app_install", { id: app.id });
      else void mockInstall(app.id);
    } catch (e) {
      finish(app.id, failed(`Couldn't install ${app.name}.`, e, "Check your Wi-Fi and try again."));
    }
  },
}));

function finish(id: string, error?: string) {
  const app = appById(id);
  const { [id]: _, ...rest } = useApps.getState().installing;
  useApps.setState({ installing: error ? { ...rest, [id]: { progress: null, error } } : rest });
  if (error) useApp.getState().toast(error, "error");
  else if (app) useApp.getState().toast(`${useApps.getState().status[id]?.offer?.name ?? app.name} is installed`);
  void useApps.getState().refresh();
}

async function mockInstall(id: string) {
  const offer = useApps.getState().status[id]?.offer;
  for (let p = 0; p <= 100; p += 4) {
    await new Promise((r) => setTimeout(r, 120));
    useApps.setState({ installing: { ...useApps.getState().installing, [id]: { progress: p / 100 } } });
  }
  if (offer) mockInstalled = [...mockInstalled, offer.flatpak];
  finish(id);
}

// Install progress from the native side.
if (isTauri)
  void import("@tauri-apps/api/event").then(({ listen }) =>
    listen<{ id: string; progress: number | null; done: boolean; error: string | null }>("app-install", ({ payload: p }) => {
      if (p.done) return finish(p.id, p.error ?? undefined);
      useApps.setState({ installing: { ...useApps.getState().installing, [p.id]: { progress: p.progress } } });
    }),
  );
// Screenshots: ?installing=discord shows an install in progress.
const shot = params.get("installing");
if (shot && !isTauri) useApps.setState({ installing: { [shot]: { progress: 0.42 } } });
