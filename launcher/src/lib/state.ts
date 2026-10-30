// The page side of the instant layer (src-tauri/src/instant.rs over launcher/crates/rare-state and
// rare-images). Rust keeps every rare.market and Trade read in memory and on disk and revalidates
// it behind; this file
//
// - seeds the page before its first render with what it used most recently (bootState), so a
//   screen seen before draws its data in its first frame (seed);
// - keeps state the page computes itself for the next start (persist, `ui:` keys);
// - tells listeners when Rust brought newer data for a key (onStateChange);
// - turns art sources into rimg:// URLs (artSrc): resized WebP from Rust's image cache.
//
// In a plain browser (dev:ui, mock screenshots) the same calls fall back to localStorage, so a page
// reload behaves like a warm restart and mock-mode timings mean something.

import { MOCK_OK } from "./mockflag";

const native: boolean = __RARE_NATIVE__;

export type Seed<T = unknown> = { at: number; v: T };
let seeds: Record<string, Seed> = {};

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

const LS_KEY = "rare.instant";

let booted = false;
const bootHooks: Array<() => void> = [];
/** Runs `cb` once the seeds are loaded, before the first render (at once if they already are). For
 *  module-level stores, which are created before bootState runs. */
export function afterBoot(cb: () => void) {
  if (booted) cb();
  else bootHooks.push(cb);
}
function finishBoot() {
  booted = true;
  for (const cb of bootHooks.splice(0)) {
    try {
      cb();
    } catch {
      /* a store that cannot take its seed starts empty */
    }
  }
}

/** Loads the seeds. Resolves within `timeoutMs` whatever happens; the page renders after it. */
export async function bootState(timeoutMs = 150): Promise<void> {
  if (!native) {
    if (!MOCK_OK) return finishBoot();
    try {
      seeds = JSON.parse(localStorage.getItem(LS_KEY) ?? "{}") as Record<string, Seed>;
    } catch {
      seeds = {};
    }
    return finishBoot();
  }
  const t0 = performance.now();
  const got = await Promise.race([invoke<Record<string, Seed>>("state_boot").catch(() => null), new Promise<null>((r) => setTimeout(() => r(null), timeoutMs))]);
  if (got) seeds = { ...got, ...seeds };
  finishBoot();
  (window as { __rareBoot?: { ms: number; keys: number } }).__rareBoot = { ms: Math.round(performance.now() - t0), keys: Object.keys(seeds).length };
}

/** What was cached for a key at start (or remembered since), with when it was fetched. */
export function seed<T>(key: string): Seed<T> | undefined {
  return seeds[key] as Seed<T> | undefined;
}

// ---------- browser fallback ----------

let saveTimer: ReturnType<typeof setTimeout> | null = null;
/** Browser only: keeps a read for the next page load (Rust does this itself in the app). */
export function remember(key: string, v: unknown) {
  if (native || !MOCK_OK) return;
  seeds[key] = { at: Date.now(), v };
  if (saveTimer) return;
  saveTimer = setTimeout(() => {
    saveTimer = null;
    try {
      // Newest first, up to ~2.5 MB.
      const out: Record<string, Seed> = {};
      let size = 0;
      for (const [k, s] of Object.entries(seeds).sort((a, b) => b[1].at - a[1].at)) {
        const n = JSON.stringify(s).length + k.length;
        if (size + n > 2_500_000) continue;
        size += n;
        out[k] = s;
      }
      localStorage.setItem(LS_KEY, JSON.stringify(out));
    } catch {
      /* storage full or blocked: nothing to keep */
    }
  }, 800);
}

// ---------- page state ----------

/** JSON-safe copy: bigints (wallet balances) become {"$big": "..."}; see revive. */
function encode(v: unknown): unknown {
  if (typeof v === "bigint") return { $big: v.toString() };
  if (Array.isArray(v)) return v.map(encode);
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, encode(x)]));
  return v;
}

/** The inverse of encode. */
export function revive<T>(v: unknown): T {
  if (Array.isArray(v)) return v.map(revive) as T;
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    if (typeof o.$big === "string" && Object.keys(o).length === 1) return BigInt(o.$big) as T;
    return Object.fromEntries(Object.entries(o).map(([k, x]) => [k, revive(x)])) as T;
  }
  return v as T;
}

/** A persisted `ui:` value as it was put (bigints back), or undefined. */
export function kept<T>(key: `ui:${string}`): T | undefined {
  const s = seeds[key];
  return s ? revive<T>(s.v) : undefined;
}

const pending = new Map<string, ReturnType<typeof setTimeout>>();
/** Keeps page-computed state (balances, friends, notices) for the next start, written at most every 1.5 s per key. */
export function persist(key: `ui:${string}`, v: unknown) {
  seeds[key] = { at: Date.now(), v: encode(v) };
  if (pending.has(key)) return;
  pending.set(
    key,
    setTimeout(() => {
      pending.delete(key);
      const cur = seeds[key]?.v;
      if (native) void invoke("state_put", { key, value: cur }).catch(() => {});
      else remember(key, cur);
    }, 1500),
  );
}

// ---------- reads ----------

/** A Trade read (`trade:/ext/...`) through Rust's cache; a direct fetch in a browser. */
export async function stateGet<T>(key: `trade:${string}` | `rare:${string}`): Promise<T> {
  if (native) return invoke<T>("state_get", { key });
  const [, origin, path] = key.match(/^(trade|rare):(.*)$/)!;
  const r = await fetch(origin === "trade" ? `https://rare.market/v1/trade${path}` : `/rare${path}`);
  if (!r.ok) throw new Error(`${r.status}`);
  const v = (await r.json()) as T;
  remember(key, v);
  return v;
}

const listeners = new Set<(key: string) => void>();
let listening = false;
/** Rust revalidated a key and its data changed (`rare:/v1/...`, `trade:/ext/...`). */
export function onStateChange(cb: (key: string) => void): () => void {
  listeners.add(cb);
  if (native && !listening) {
    listening = true;
    void import("@tauri-apps/api/event").then(({ listen }) => listen<string>("rare-state", (e) => listeners.forEach((l) => l(e.payload))));
  }
  return () => listeners.delete(cb);
}

// ---------- images ----------

export type ArtSize = "thumb" | "tile" | "card" | "hero" | "orig" | `w${number}`;

const RIMG = typeof navigator !== "undefined" && /Windows/.test(navigator.userAgent) ? "http://rimg.localhost/" : "rimg://localhost/";

function b64url(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** Width hint to the nearest size Rust makes (rare-images Size::BUCKETS). */
export function sizeFor(width: number): ArtSize {
  for (const b of [64, 128, 192, 256, 400, 512, 720, 960, 1280, 1600, 1920, 2560]) if (b >= width) return `w${b}`;
  return "w2560";
}

/**
 * Art through Rust's image cache: an http(s) URL or an absolute file path (Steam's librarycache)
 * becomes rimg://localhost/<size>/<source>, served resized as WebP from memory or disk. Anything
 * else (data:, bundled files) and every source outside the app is returned unchanged.
 */
export function artSrc(src: string, size: ArtSize): string {
  if (!native || !src) return src;
  if (!/^https?:\/\//.test(src) && !src.startsWith("/")) return src;
  return `${RIMG}${size}/${b64url(src)}`;
}

/** Asks Rust to have these rimg:// images in memory (the next items a focus move reaches). */
export function preloadArt(urls: string[]) {
  if (!native) return;
  const list = urls.filter((u) => u.startsWith(RIMG));
  if (list.length) void invoke("img_preload", { urls: list }).catch(() => {});
}
