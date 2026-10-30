// Notification state: the notices (newest first), what has been read or cleared, the toasts on
// screen, the count that arrived while a game ran, and the person's notification settings.
// Kept in local storage like favourites: per device, survives updates, no native config change.

import { create } from "zustand";
import { DEFAULT_PREFS, classOf, type NoticeClass, isId, isRead, mergeNotices, noticeTime, readPrefs, unreadCount, visible, type Prefs } from "./logic";
import type { Notice, NoticeKind } from "./types";

const K = {
  list: "rare.notify.list",
  read: "rare.notify.read",
  readAt: "rare.notify.readAt",
  clearedAt: "rare.notify.clearedAt",
  prefs: "rare.notify.prefs",
};

export function readLocal<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}
export function writeLocal(key: string, value: unknown) {
  try {
    if (value === null || value === undefined) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* private mode or full: this session only */
  }
}

export type Status = "off" | "connecting" | "live" | "retrying";

/** How long a notice toast stays up. */
export const TOAST_MS = 6000;
const MAX_TOASTS = 3;

interface NotifyState {
  notices: Notice[];
  read: string[];
  /** "Mark all read" and "Clear" marks, as times (ms): server and local notices share one timeline. */
  readAt: number | null;
  clearedAt: number | null;
  prefs: Prefs;
  /** `hud`: raised over a game (the HUD draws it); `small`: the small trophy toast. */
  toasts: Array<{ notice: Notice; at: number; hud?: boolean; small?: boolean }>;
  /** Arrived while a game ran (toasts while playing off). */
  missed: number;
  status: Status;
  add: (list: Notice[]) => void;
  showToast: (n: Notice, opts?: { hud?: boolean; small?: boolean }) => void;
  dismissToast: (id: string) => void;
  markRead: (id: string) => void;
  markAllRead: () => void;
  clear: () => void;
  setKind: (k: NoticeKind, on: boolean) => void;
  setPrefs: (p: Partial<Prefs>) => void;
}

const initialList = (() => {
  const v = readLocal<unknown>(K.list, []);
  return Array.isArray(v) ? mergeNotices([], v as Notice[], 50) : [];
})();
const msOrNull = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v > 0 ? v : null);
/** A mark that covers everything in the list now, even a notice stamped a little ahead of this clock. */
const markNow = (prev: number | null, list: Notice[]) => Math.max(prev ?? 0, Date.now(), list[0] ? noticeTime(list[0]) : 0);
const timers = new Map<string, ReturnType<typeof setTimeout>>();

export const useNotify = create<NotifyState>((set, get) => ({
  notices: initialList,
  read: (() => {
    const v = readLocal<unknown>(K.read, []);
    return Array.isArray(v) ? v.filter(isId) : [];
  })(),
  readAt: msOrNull(readLocal(K.readAt, null)),
  clearedAt: msOrNull(readLocal(K.clearedAt, null)),
  prefs: readPrefs(readLocal(K.prefs, DEFAULT_PREFS)),
  toasts: [],
  missed: 0,
  status: "off",
  add: (list) => {
    const notices = mergeNotices(get().notices, list);
    set({ notices });
    writeLocal(K.list, notices.slice(0, 50));
  },
  showToast: (notice, opts) => {
    if (get().toasts.some((t) => t.notice.id === notice.id)) return;
    // Newest last (under the older ones), at most three; the oldest goes first.
    const toasts = [...get().toasts, { notice, at: Date.now(), hud: !!opts?.hud, small: !!opts?.small }].slice(-MAX_TOASTS);
    set({ toasts });
    timers.set(
      notice.id,
      setTimeout(() => get().dismissToast(notice.id), TOAST_MS),
    );
  },
  dismissToast: (id) => {
    clearTimeout(timers.get(id));
    timers.delete(id);
    set({ toasts: get().toasts.filter((t) => t.notice.id !== id) });
  },
  markRead: (id) => {
    if (get().read.includes(id)) return;
    const read = [id, ...get().read].slice(0, 200);
    set({ read });
    writeLocal(K.read, read);
  },
  markAllRead: () => {
    const readAt = markNow(get().readAt, get().notices);
    set({ readAt, read: [] });
    writeLocal(K.readAt, readAt);
    writeLocal(K.read, []);
  },
  clear: () => {
    const clearedAt = markNow(get().clearedAt, get().notices);
    const readAt = markNow(get().readAt, get().notices);
    set({ clearedAt, readAt, read: [] });
    writeLocal(K.clearedAt, clearedAt);
    writeLocal(K.readAt, readAt);
    writeLocal(K.read, []);
  },
  setKind: (k, on) => get().setPrefs({ kinds: { ...get().prefs.kinds, [k]: on } }),
  setPrefs: (p) => {
    const prefs = { ...get().prefs, ...p };
    set({ prefs });
    writeLocal(K.prefs, prefs);
  },
}));

/** The centre's list: kinds that are on, newer than the last Clear. */
export function useVisibleNotices(): Notice[] {
  const notices = useNotify((s) => s.notices);
  const prefs = useNotify((s) => s.prefs);
  const cleared = useNotify((s) => s.clearedAt);
  return visible(notices, prefs, cleared);
}

export function useUnread(): number {
  return useNotify((s) => unreadCount(s.notices, s.prefs, s.clearedAt, new Set(s.read), s.readAt));
}

export const noticeIsRead = (s: Pick<NotifyState, "read" | "readAt">, n: Notice) => isRead(n, new Set(s.read), s.readAt);

/** The centre's list for one class (the Rare menu's Notifications tab filters with this). */
export function useNoticesByClass(cls: NoticeClass | null): Notice[] {
  const list = useVisibleNotices();
  return cls ? list.filter((n) => classOf(n.kind) === cls) : list;
}
