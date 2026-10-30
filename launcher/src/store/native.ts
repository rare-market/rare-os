// The Rust side of the Store (src-tauri/src/store): platforms this device runs, the download
// challenge, install (download, SHA-256 against the chain, unpack, switch `current`, rare.json),
// installed games, the launch check's cache and uninstall. In a plain browser these answer from
// a small mock so the screens can be built on a Mac.

import { isTauri } from "../lib/bridge";
import type { InstallRequest, Installed, Progress } from "./types";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

// ---------- browser mock ----------

const KEY = "rare.mock.store.installed";
const listeners = new Set<(p: Progress) => void>();
function mockInstalled(): Installed[] {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "[]") as Installed[];
  } catch {
    return [];
  }
}
function saveMock(list: Installed[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* private window */
  }
}
async function mockInstall(req: InstallRequest): Promise<Installed> {
  const total = req.expectedSize;
  const emit = (p: Omit<Progress, "slug">) => listeners.forEach((l) => l({ slug: req.slug, ...p }));
  for (let i = 0; i <= 20; i++) {
    emit({ phase: "download", done: Math.round((total * i) / 20), total });
    await new Promise((r) => setTimeout(r, 120));
  }
  emit({ phase: "verify", done: total, total });
  await new Promise((r) => setTimeout(r, 300));
  emit({ phase: "unpack", done: total, total });
  await new Promise((r) => setTimeout(r, 400));
  const inst: Installed = {
    id: `rare:${req.slug}`,
    slug: req.slug,
    name: req.name,
    version: req.version,
    label: req.label,
    edition: req.edition,
    licenceId: "",
    platform: req.platform,
    sha256: req.expectedSha256,
    size: req.expectedSize,
    runtime: req.platform.startsWith("windows") ? "proton" : "native",
    exec: "bin/game",
    args: [],
    launchCheck: req.launchCheck,
    images: req.images,
    installedMs: Date.now(),
    checkedMs: Date.now(),
    owner: req.address,
    dir: `~/Games/Rare/${req.slug}`,
  };
  saveMock([...mockInstalled().filter((g) => g.slug !== req.slug), inst]);
  emit({ phase: "done", done: total, total });
  return inst;
}

// ---------- the door ----------

export const storeNative = {
  platforms: (): Promise<{ platforms: string[]; proton: boolean }> => (isTauri ? call("store_platforms") : Promise.resolve({ platforms: ["linux-x86_64"], proton: true })),
  challenge: (address: string, slug: string, platform: string): Promise<{ message: string; expires_ms: number }> =>
    isTauri
      ? call("store_challenge", { address, slug, platform })
      : Promise.resolve({ message: `Rare Store download\nslug: ${slug}\nplatform: ${platform}\naddress: ${address}\nnonce: 00\nexpires: ${new Date(Date.now() + 300_000).toISOString()}`, expires_ms: Date.now() + 300_000 }),
  install: (req: InstallRequest): Promise<Installed> => (isTauri ? call("store_install", { req }) : mockInstall(req)),
  installed: (): Promise<Installed[]> => (isTauri ? call("store_installed") : Promise.resolve(mockInstalled())),
  markChecked: (slug: string, ok: boolean): Promise<void> => (isTauri ? call("store_mark_checked", { slug, ok }) : Promise.resolve()),
  uninstall: (slug: string): Promise<void> => {
    if (isTauri) return call("store_uninstall", { slug });
    saveMock(mockInstalled().filter((g) => g.slug !== slug));
    return Promise.resolve();
  },
};

/** Install progress from the Rust side (`store-progress`). */
export async function onStoreProgress(cb: (p: Progress) => void): Promise<() => void> {
  if (!isTauri) {
    listeners.add(cb);
    return () => listeners.delete(cb);
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<Progress>("store-progress", (e) => cb(e.payload));
}
