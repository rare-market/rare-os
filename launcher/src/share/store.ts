// State for the Share screen: captures on this device, the one being looked at, its price, and the
// upload. Nothing is uploaded or paid without the player pressing Share and approving twice in
// Rare Wallet.

import { create } from "zustand";
import { useApp } from "../lib/store";
import { loadGames } from "../lib/rare";
import type { Network } from "../sui/chain";
import { DEFAULT_NETWORK } from "../sui/saves/network";
import type { CostEstimate, UploadProgress } from "../sui/saves/walrus";
import { walletSigner } from "../sui/signer";
import { buildShareJson, DURATIONS, shareEpochs, shareLink, slugFor, type DurationId, type ShareJson } from "./format";
import { onCaptured, onShareError, shareNative, type Capture, type ShareStatus } from "./native";
import { grabPoster } from "./poster";
import { getSponsor, type SponsorQuote } from "./sponsor";
import type { QuiltFile, ShareResult } from "./upload";

// Walrus and the Sui SDK load with the first share, not with the launcher.
const up = () => import("./upload");

export type ShareStep = UploadProgress["step"];

export type Phase =
  | { at: "idle" }
  | { at: "uploading"; step: ShareStep }
  | { at: "done"; result: ShareResult; copied: boolean }
  | { at: "error"; message: string };

export interface Quote {
  id: string;
  /** encoded quilt size */
  size: number;
  epochs: number;
  days: number;
  cost: CostEstimate | null;
  sponsor: SponsorQuote | null;
}

interface Prepared {
  id: string;
  json: ShareJson;
  files: QuiltFile[];
  size: number;
}

interface ShareState {
  status: ShareStatus | null;
  captures: Capture[] | null;
  /** captures newer than this are marked new */
  seenAt: number;
  /** object URLs: the screenshot itself or a clip's poster */
  thumbs: Record<string, string>;
  selected: string | null;
  /** object URL of the selected capture's media, for the big preview */
  preview: string | null;
  network: Network;
  duration: DurationId;
  payer: "me" | "rare";
  quote: Quote | null;
  phase: Phase;
  /** a capture being taken from the Share screen or quick settings */
  taking: "screenshot" | "clip" | null;
  error: string | null;
  /** `thumbs`: also make thumbnails (the Share screen; a clip poster reads the whole clip). */
  refresh: (opts?: { thumbs?: boolean }) => Promise<void>;
  select: (id: string | null) => void;
  setDuration: (d: DurationId) => void;
  setPayer: (p: "me" | "rare") => void;
  share: (address: string | null) => Promise<void>;
  remove: (id: string) => Promise<void>;
  takeScreenshot: () => Promise<Capture | null>;
  saveClip: () => Promise<Capture | null>;
  setClips: (on: boolean) => Promise<void>;
  copied: (ok: boolean) => void;
  markSeen: () => void;
  /** a capture arrived from the native side */
  added: (c: Capture) => void;
}

const SEEN_KEY = "rare.share.seenAt";
function loadSeen(): number {
  try {
    const v = Number(localStorage.getItem(SEEN_KEY));
    if (v > 0) return v;
    localStorage.setItem(SEEN_KEY, String(Date.now()));
  } catch {
    /* private mode */
  }
  return Date.now();
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const isRejected = (e: unknown) => (e as Error)?.name === "SignRejected";

/** Wallet and network errors in plain words. */
export function plainShareError(e: unknown, need?: bigint | null): string {
  const m = message(e);
  if (/not enough wal|insufficient.*wal|no.*wal coins|InsufficientCoinBalance/i.test(m)) {
    return need ? `Not enough WAL in this account. Sharing this needs about ${(Number(need) / 1e9).toPrecision(2)} WAL.` : "Not enough WAL in this account.";
  }
  if (/InsufficientGas|GasBalanceTooLow|not enough sui|insufficient.*sui/i.test(m)) return "Not enough SUI in this account for the fees.";
  if (/failed to fetch|networkerror|load failed|timed out/i.test(m)) return "Couldn't reach Walrus. Check the connection and try again.";
  return m || "That didn't go through. Try again.";
}

// Bytes stay out of React state: the selected capture's media, every clip poster made so far,
// and the quilt ready to upload.
const media = new Map<string, Uint8Array>();
const posters = new Map<string, Blob>();
let prepared: Prepared | null = null;
let thumbing = false;
/** Thumbnails are made once the Share screen has been opened. */
let thumbsOn = false;

async function mediaOf(id: string): Promise<Uint8Array> {
  let b = media.get(id);
  if (!b) {
    b = await shareNative.read(id);
    // Keep only the selected capture's bytes around.
    for (const k of media.keys()) if (k !== useShare.getState().selected) media.delete(k);
    media.set(id, b);
  }
  return b;
}

async function posterOf(c: Capture): Promise<Blob> {
  let p = posters.get(c.id);
  if (!p) {
    p = await grabPoster(await mediaOf(c.id), c.mime);
    posters.set(c.id, p);
  }
  return p;
}

const get = () => useShare.getState();

/** Thumbnails, newest first, one at a time (a clip's poster means reading the whole clip). */
async function makeThumbs() {
  if (thumbing) return;
  thumbing = true;
  try {
    for (;;) {
      const c = (get().captures ?? []).slice(0, 36).find((x) => !(x.id in get().thumbs));
      if (!c) break;
      let url = "";
      try {
        if (c.kind === "clip") url = URL.createObjectURL(await posterOf(c));
        else url = URL.createObjectURL(new Blob([(await shareNative.read(c.id)) as BlobPart], { type: c.mime }));
      } catch {
        /* an unreadable capture keeps a quiet tile */
      }
      useShare.setState((s) => ({ thumbs: { ...s.thumbs, [c.id]: url } }));
    }
  } finally {
    thumbing = false;
  }
}

async function prepare(c: Capture) {
  const network = get().network;
  const bytes = await mediaOf(c.id);
  if (get().selected !== c.id) return;
  const poster = c.kind === "clip" ? new Uint8Array(await (await posterOf(c)).arrayBuffer()) : null;
  const { games } = await loadGames().catch(() => ({ games: [] }));
  const json = buildShareJson(c, { bytes: bytes.length, slug: slugFor(c.game, games, useApp.getState().library), poster: !!poster });
  const { shareFiles, quiltSize } = await up();
  const files = shareFiles(json, bytes, poster);
  const size = await quiltSize(network, files);
  if (get().selected !== c.id) return;
  prepared = { id: c.id, json, files, size };
  await price();
}

/** The price of the prepared share for the chosen duration, from live Walrus prices. */
async function price() {
  const p = prepared;
  const { network, duration } = get();
  if (!p) return;
  const { epochs, days } = shareEpochs(duration, network);
  const sponsor = getSponsor();
  const [cost, sq] = await Promise.all([
    up().then((u) => u.shareEstimate(network, p.size, DURATIONS.find((d) => d.id === duration)!.days)).catch(() => null),
    sponsor ? sponsor.quote(p.size, epochs).catch(() => null) : Promise.resolve(null),
  ]);
  if (get().selected !== p.id || get().duration !== duration) return;
  useShare.setState({ quote: { id: p.id, size: p.size, epochs, days, cost, sponsor: sq }, payer: sq?.covered ? get().payer : "me" });
}

/** Copies text: the async clipboard first, the old way if the webview refuses. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const t = document.createElement("textarea");
      t.value = text;
      t.style.position = "fixed";
      t.style.opacity = "0";
      document.body.appendChild(t);
      t.select();
      const ok = document.execCommand("copy");
      t.remove();
      return ok;
    } catch {
      return false;
    }
  }
}

export const useShare = create<ShareState>((set) => ({
  status: null,
  captures: null,
  seenAt: loadSeen(),
  thumbs: {},
  selected: null,
  preview: null,
  network: DEFAULT_NETWORK,
  duration: "month",
  payer: "me",
  quote: null,
  phase: { at: "idle" },
  taking: null,
  error: null,

  refresh: async (opts) => {
    if (opts?.thumbs) thumbsOn = true;
    try {
      const [status, captures] = await Promise.all([shareNative.status().catch(() => get().status), shareNative.list()]);
      set({ status, captures });
      if (thumbsOn) void makeThumbs();
    } catch (e) {
      set({ captures: get().captures ?? [], error: message(e) });
    }
  },

  select: (id) => {
    const s = get();
    if (s.phase.at === "uploading") return;
    if (s.preview && s.preview !== s.thumbs[s.selected ?? ""]) URL.revokeObjectURL(s.preview);
    prepared = null;
    set({ selected: id, preview: null, quote: null, phase: { at: "idle" }, error: null });
    const c = id ? s.captures?.find((x) => x.id === id) : null;
    if (!c) return;
    void (async () => {
      try {
        if (c.kind === "clip") {
          const url = URL.createObjectURL(new Blob([(await mediaOf(c.id)) as BlobPart], { type: c.mime }));
          if (get().selected === c.id) set({ preview: url });
          else URL.revokeObjectURL(url);
        }
        await prepare(c);
      } catch (e) {
        if (get().selected === c.id) set({ error: message(e) });
      }
    })();
  },

  setDuration: (duration) => {
    if (get().phase.at === "uploading" || duration === get().duration) return;
    set({ duration, quote: null });
    void price();
  },
  setPayer: (payer) => set({ payer }),

  share: async (address) => {
    const s = get();
    const p = prepared;
    const c = s.captures?.find((x) => x.id === s.selected);
    if (!c || !p || p.id !== c.id || s.phase.at === "uploading") return;
    const { epochs } = shareEpochs(s.duration, s.network);
    const sponsored = s.payer === "rare" && !!s.quote?.sponsor?.covered;
    if (!sponsored && !address) return set({ phase: { at: "error", message: "Add a Rare Wallet account to share." } });
    set({ phase: { at: "uploading", step: "encode" }, error: null });
    try {
      let result: ShareResult;
      if (sponsored) {
        set({ phase: { at: "uploading", step: "upload" } });
        const r = await getSponsor()!.store(p.files, epochs);
        result = { quiltId: r.quiltId, blobObjectId: r.blobObjectId, link: shareLink(r.quiltId, s.network) };
      } else {
        const signer = await walletSigner(address!, { name: "Rare Share", kind: "wallet" });
        result = await (await up()).uploadShare(s.network, signer, p.files, { kind: c.kind, epochs, onStep: (st) => set({ phase: { at: "uploading", step: st.step } }) });
      }
      const copied = await copyText(result.link);
      set({ phase: { at: "done", result, copied } });
    } catch (e) {
      if (isRejected(e)) set({ phase: { at: "idle" } });
      else set({ phase: { at: "error", message: plainShareError(e, s.quote?.cost?.walFrost) } });
    }
  },

  remove: async (id) => {
    try {
      await shareNative.remove(id);
    } catch (e) {
      set({ error: message(e) });
      return;
    }
    const s = get();
    const url = s.thumbs[id];
    if (url) URL.revokeObjectURL(url);
    if (s.selected === id && s.preview) URL.revokeObjectURL(s.preview);
    media.delete(id);
    posters.delete(id);
    if (prepared?.id === id) prepared = null;
    const thumbs = { ...s.thumbs };
    delete thumbs[id];
    set({
      captures: (s.captures ?? []).filter((c) => c.id !== id),
      thumbs,
      ...(s.selected === id ? { selected: null, preview: null, quote: null, phase: { at: "idle" as const } } : {}),
    });
  },

  takeScreenshot: async () => {
    if (get().taking) return null;
    set({ taking: "screenshot", error: null });
    try {
      const c = await shareNative.screenshot();
      get().added(c);
      announce(c);
      return c;
    } catch (e) {
      useApp.getState().toast(message(e), "error");
      return null;
    } finally {
      set({ taking: null });
    }
  },

  saveClip: async () => {
    if (get().taking) return null;
    set({ taking: "clip", error: null });
    try {
      const c = await shareNative.clip();
      get().added(c);
      announce(c);
      return c;
    } catch (e) {
      useApp.getState().toast(message(e), "error");
      return null;
    } finally {
      set({ taking: null });
    }
  },

  setClips: async (on) => {
    try {
      await shareNative.setClips(on);
      set({ status: await shareNative.status() });
    } catch (e) {
      useApp.getState().toast(message(e), "error");
    }
  },

  copied: (ok) => {
    const ph = get().phase;
    if (ph.at === "done") set({ phase: { ...ph, copied: ok } });
  },

  markSeen: () => {
    const now = Date.now();
    try {
      localStorage.setItem(SEEN_KEY, String(now));
    } catch {
      /* private mode */
    }
    set({ seenAt: now });
  },

  added: (c) => {
    const list = get().captures;
    if (list?.some((x) => x.id === c.id)) return;
    if (list) {
      set({ captures: [c, ...list] });
      if (thumbsOn) void makeThumbs();
    }
  },
}));

// ---------- captures from chords, while a game has the screen ----------

const pending: Array<{ text: string; tone: "info" | "error" }> = [];
const inFront = () => typeof document !== "undefined" && document.visibilityState === "visible" && document.hasFocus();

function flush() {
  if (!inFront()) return;
  // One toast per kind of thing: "3 screenshots saved" rather than three toasts.
  const counts = new Map<string, { n: number; tone: "info" | "error" }>();
  for (const p of pending.splice(0)) counts.set(p.text, { n: (counts.get(p.text)?.n ?? 0) + 1, tone: p.tone });
  for (const [text, { n, tone }] of counts) {
    const many = n > 1 && tone === "info" ? text.replace(/^(Screenshot|Clip) saved$/, (_m, w: string) => `${n} ${w.toLowerCase()}s saved`) : text;
    useApp.getState().toast(many, tone);
  }
}

function queue(text: string, tone: "info" | "error" = "info") {
  pending.push({ text, tone });
  flush();
}

// A capture can come back from the command and as an event: say it once.
const announced = new Set<string>();
function announce(c: Capture) {
  if (announced.has(c.id)) return;
  announced.add(c.id);
  queue(c.kind === "clip" ? "Clip saved" : "Screenshot saved");
}

let started = false;
/** Listens for captures and errors from the native side. Called once from App. */
export function startShare() {
  if (started) return;
  started = true;
  void onCaptured((c) => {
    useShare.getState().added(c);
    announce(c);
  });
  void onShareError((m) => queue(m, "error"));
  // The list only (no thumbnails): Home and quick settings show how many are new.
  void useShare.getState().refresh();
  window.addEventListener("focus", flush);
  document.addEventListener("visibilitychange", flush);
}

/** Captures newer than the last visit to the Share screen. */
export const newCount = (s: Pick<ShareState, "captures" | "seenAt">) => (s.captures ?? []).filter((c) => c.createdMs > s.seenAt).length;
