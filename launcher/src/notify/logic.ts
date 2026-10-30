// The pure parts of notifications: notice ids (decimal strings that only grow), merging and
// dedupe, which kinds are on, and what one arriving notice does (toast, queue while a game
// runs, centre only, or nothing). No DOM, no stores: unit tested in logic.test.ts.

import { NOTICE_KINDS, type Notice, type NoticeKind } from "./types";

/** Compares two decimal id strings without losing precision (ids can pass 2^53). */
export function cmpId(a: string, b: string): number {
  const x = a.replace(/^0+(?=\d)/, "");
  const y = b.replace(/^0+(?=\d)/, "");
  if (x.length !== y.length) return x.length < y.length ? -1 : 1;
  return x < y ? -1 : x > y ? 1 : 0;
}

export const isId = (v: unknown): v is string => typeof v === "string" && /^\d{1,40}$/.test(v);

/** The larger of two ids; null and junk lose. */
export function maxId(a: string | null | undefined, b: string | null | undefined): string | null {
  const ok = (v: string | null | undefined) => (isId(v) ? v : null);
  const x = ok(a);
  const y = ok(b);
  if (!x) return y;
  if (!y) return x;
  return cmpId(x, y) >= 0 ? x : y;
}

/** When a notice happened, for ordering, read and clear marks: server and local notices share one timeline. */
export const noticeTime = (n: Pick<Notice, "createdAt" | "cpTs">) => n.createdAt || n.cpTs || 0;

/** Newest first, one copy of each id, at most `cap`. Later copies of an id never replace the first. */
export function mergeNotices(existing: Notice[], incoming: Notice[], cap = 100): Notice[] {
  const by = new Map<string, Notice>();
  for (const n of existing) if (isId(n.id) && !by.has(n.id)) by.set(n.id, n);
  for (const n of incoming) if (isId(n.id) && !by.has(n.id)) by.set(n.id, n);
  return [...by.values()].sort((a, b) => noticeTime(b) - noticeTime(a) || cmpId(b.id, a.id)).slice(0, cap);
}

/** The ids in `incoming` not in `existing` (what is new to this launcher). */
export function newIds(existing: Notice[], incoming: Notice[]): Set<string> {
  const have = new Set(existing.map((n) => n.id));
  const out = new Set<string>();
  for (const n of incoming) if (isId(n.id) && !have.has(n.id)) out.add(n.id);
  return out;
}

/**
 * The query string for the stream: every account address (unique, lowercase, at most 20), the
 * device, and `after` so a launcher restart picks up where it left off. EventSource adds
 * Last-Event-ID itself on its own reconnects.
 */
export function streamQuery(addrs: string[], device: string, after: string | null): string {
  const q = new URLSearchParams();
  const list = normAddrs(addrs);
  if (list.length) q.set("addr", list.join(","));
  q.set("device", device);
  if (isId(after)) q.set("after", after);
  return q.toString();
}

export function normAddrs(addrs: string[]): string[] {
  return [...new Set(addrs.map((a) => a.trim().toLowerCase()).filter((a) => /^0x[0-9a-f]{1,64}$/.test(a)))].sort().slice(0, 20);
}

/**
 * Notice classes. Each notice belongs to one class, and the
 * class decides what it does in the launcher, over a game and with Do not disturb.
 */
export type NoticeClass = "money" | "invites" | "trophies" | "social" | "system" | "battery";

export const CLASSES: NoticeClass[] = ["money", "invites", "trophies", "social", "system", "battery"];

export const CLASS_LABEL: Record<NoticeClass, string> = {
  money: "Money",
  invites: "Invites",
  trophies: "Trophies",
  social: "Social",
  system: "System",
  battery: "Battery",
};

const KIND_CLASS: Record<NoticeKind, NoticeClass> = {
  sold: "money",
  bought: "money",
  offer_received: "money",
  coin_received: "money",
  nft_received: "money",
  airdrop: "money",
  perp_liquidation: "money",
  price_alert: "money",
  followed_trade: "social",
  party_invite: "invites",
  friend_request: "social",
  friend_online: "social",
  party_message: "social",
  trophy: "trophies",
  os_update: "system",
  download_done: "system",
  emu_update: "system",
  battery: "battery",
};

/** The class of a kind. A kind from a newer server is Social: it toasts in the launcher and waits otherwise. */
export const classOf = (kind: string): NoticeClass => (KIND_CLASS as Record<string, NoticeClass | undefined>)[kind] ?? "social";

/** How a class shows up in one situation: a toast, a small toast (trophies over a game), or only in the list. */
export type Delivery = "toast" | "small" | "list";

export interface ClassRule {
  launcher: Delivery;
  overGame: Delivery;
  dnd: Delivery;
}

/** The model's table. Over a game a toast goes through the HUD. */
export const CLASS_RULES: Record<NoticeClass, ClassRule> = {
  money: { launcher: "toast", overGame: "toast", dnd: "list" },
  invites: { launcher: "toast", overGame: "toast", dnd: "list" },
  trophies: { launcher: "toast", overGame: "small", dnd: "list" },
  social: { launcher: "toast", overGame: "list", dnd: "list" },
  system: { launcher: "toast", overGame: "list", dnd: "list" },
  battery: { launcher: "toast", overGame: "toast", dnd: "toast" },
};

export interface Prefs {
  kinds: Record<NoticeKind, boolean>;
  /** Toasts over a game, for the classes the model lets through (Money, Invites, Trophies, Battery). Off: they wait in the list. */
  gameToasts: boolean;
  /** A desktop notification when the launcher is not the focused window. */
  desktop: boolean;
  /** Do not disturb (Quick Settings): only Battery still toasts; everything else waits in the list. */
  dnd: boolean;
}

export const DEFAULT_PREFS: Prefs = {
  kinds: Object.fromEntries(NOTICE_KINDS.map((k) => [k, true])) as Record<NoticeKind, boolean>,
  gameToasts: true,
  desktop: true,
  dnd: false,
};

/** Reads stored prefs, keeping defaults for anything missing or of the wrong type. */
export function readPrefs(raw: unknown): Prefs {
  const p = (raw && typeof raw === "object" ? raw : {}) as Partial<Prefs>;
  const kinds = { ...DEFAULT_PREFS.kinds };
  if (p.kinds && typeof p.kinds === "object") for (const k of Object.keys(kinds) as NoticeKind[]) if (typeof p.kinds[k] === "boolean") kinds[k] = p.kinds[k];
  return {
    kinds,
    gameToasts: typeof p.gameToasts === "boolean" ? p.gameToasts : DEFAULT_PREFS.gameToasts,
    desktop: typeof p.desktop === "boolean" ? p.desktop : DEFAULT_PREFS.desktop,
    dnd: typeof p.dnd === "boolean" ? p.dnd : DEFAULT_PREFS.dnd,
  };
}

/** Unknown kinds (a newer server) are shown: only a switched-off kind is hidden. */
export const kindOn = (prefs: Prefs, kind: string) => (prefs.kinds as Record<string, boolean>)[kind] !== false;

/** A notice is fresh enough to pop up when it was recorded in the last two minutes. Replays after a restart are not. */
export const FRESH_MS = 120_000;
export const isFresh = (n: Pick<Notice, "createdAt" | "cpTs">, now: number) => now - (n.createdAt || n.cpTs || 0) < FRESH_MS;

export type Route = "drop" | "centre" | "queue" | "toast";

/** What a class does right now: Do not disturb first, then a game in front, then the launcher. */
export function deliveryFor(cls: NoticeClass, ctx: { dnd: boolean; playing: boolean; gameToasts: boolean }): Delivery {
  const r = CLASS_RULES[cls];
  if (ctx.dnd) return r.dnd;
  if (ctx.playing) return ctx.gameToasts || cls === "battery" ? r.overGame : "list";
  return r.launcher;
}

/**
 * What an arriving notice does:
 * - drop: its kind is switched off
 * - centre: an old notice (from `recent` or a replay), or Do not disturb: into the list quietly, no toast
 * - queue: a game is running and its class does not toast over a game: into the list, counted for the bottom bar
 * - toast: pop up with the notify sound (`hud`: over a game, through the HUD; `small`: the small trophy toast),
 *   and a desktop notification when the launcher is not focused
 */
export function route(n: Notice, ctx: { prefs: Prefs; live: boolean; now: number; playing: boolean; focused: boolean }): { route: Route; desktop: boolean; hud: boolean; small: boolean; cls: NoticeClass } {
  const cls = classOf(n.kind);
  const none = { desktop: false, hud: false, small: false, cls };
  if (!kindOn(ctx.prefs, n.kind)) return { route: "drop", ...none };
  if (!ctx.live || !isFresh(n, ctx.now)) return { route: "centre", ...none };
  const d = deliveryFor(cls, { dnd: ctx.prefs.dnd, playing: ctx.playing, gameToasts: ctx.prefs.gameToasts });
  if (d === "list") return { route: ctx.prefs.dnd ? "centre" : "queue", ...none };
  return { route: "toast", desktop: ctx.prefs.desktop && !ctx.focused && !ctx.playing, hud: ctx.playing, small: d === "small", cls };
}

/** Counts what arrived while the device slept, and says what to show on wake (none under Do not disturb). */
export class WakeSummary {
  private ids = new Set<string>();
  add(id: string) {
    this.ids.add(id);
  }
  get count() {
    return this.ids.size;
  }
  flush(): string | null {
    const n = this.ids.size;
    this.ids.clear();
    return n ? `${n} new while you were away` : null;
  }
}

/** The notices the centre shows: kinds that are on, newer than the last Clear (a time, ms). */
export function visible(notices: Notice[], prefs: Prefs, clearedAt: number | null): Notice[] {
  return notices.filter((n) => kindOn(prefs, n.kind) && (!clearedAt || noticeTime(n) > clearedAt));
}

export function unreadCount(notices: Notice[], prefs: Prefs, clearedAt: number | null, read: ReadonlySet<string>, readAt: number | null): number {
  return visible(notices, prefs, clearedAt).filter((n) => !isRead(n, read, readAt)).length;
}

/** Read: opened one by one, or at or before the last "Mark all read" (a time, ms). */
export const isRead = (n: Pick<Notice, "id" | "createdAt" | "cpTs">, read: ReadonlySet<string>, readAt: number | null) =>
  read.has(n.id) || (!!readAt && noticeTime(n) <= readAt);

/** Holds what arrived while a game ran, and says what to tell the person when it ends. */
export class PlayQueue {
  private ids: string[] = [];
  add(id: string) {
    if (!this.ids.includes(id)) this.ids.push(id);
  }
  get count() {
    return this.ids.length;
  }
  /** The game ended: the summary line (or null when nothing came in), and the queue empties. */
  flush(): string | null {
    const n = this.ids.length;
    this.ids = [];
    if (!n) return null;
    return n === 1 ? "1 new notification while you played" : `${n} new notifications while you played`;
  }
}

/** Latency samples: the last `cap`, oldest first. */
export function pushSample<T>(list: T[], s: T, cap = 50): T[] {
  const out = [...list, s];
  return out.length > cap ? out.slice(out.length - cap) : out;
}

export function percentile(values: number[], p: number): number | null {
  const v = values.filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (!v.length) return null;
  return v[Math.min(v.length - 1, Math.max(0, Math.ceil((p / 100) * v.length) - 1))];
}
