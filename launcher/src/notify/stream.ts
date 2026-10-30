// The live notice stream: one EventSource for every Rare Wallet account (signed in, hardware and
// watch-only) plus this launcher's device id. It resumes where it left off: EventSource sends
// Last-Event-ID on its own reconnects, and the last id is kept on disk so a launcher restart asks
// for `?after=<id>`. On start (and whenever the accounts change) `recent` fills the centre.
//
// Each arriving notice is routed (logic.ts): a toast with the notify sound, a desktop
// notification when the launcher is in the background, held quietly while a game runs, or
// straight into the centre for old ones.

import { keystore } from "../wallet";
import { useWallet } from "../wallet/ui/store";
import { useApp } from "../lib/store";
import { poll } from "../lib/idle";
import { feedback } from "../nav/feedback";
import * as api from "./api";
import { launcherFocused, sendDesktop, watchWindowFocus } from "./desktop";
import { recordReceipt, setSkew } from "./latency";
import { native } from "../lib/bridge";
import { PlayQueue, WakeSummary, isFresh, isId, maxId, normAddrs, route, streamQuery } from "./logic";
import { useAlerts } from "./alerts";
import { deviceId } from "./device";
import { startDownloadNotices } from "./downloads";
import { readLocal, useNotify, writeLocal } from "./store";
import type { Notice, NoticeAction, NoticeKind } from "./types";
import { FOLLOWED_TRADE_EVENT, setFollowToasts, tradeSentence, type FollowedTradeEvent } from "../trade/traders/feed";
import { numText } from "../ui";

const K_LAST = "rare.notify.last";

let lastId: string | null = (() => {
  const v = readLocal<unknown>(K_LAST, null);
  return isId(v) ? v : null;
})();
function noteId(id: string) {
  const next = maxId(lastId, id);
  if (next !== lastId) {
    lastId = next;
    writeLocal(K_LAST, lastId);
  }
}

let addrs: string[] = [];
let socialToken: (() => Promise<string | null>) | null = null;
let connectSeq = 0;

/** friends/live.ts: the signed token that adds friend events to the stream (null: none). */
export function setSocialTokenProvider(fn: () => Promise<string | null>) {
  socialToken = fn;
}

/** Reconnects now (friends linked or unlinked, or the token is due for renewal). */
export function refreshNotifyStream() {
  if (started) void connect();
}
let conn: api.StreamConn | null = null;
let retry: ReturnType<typeof setTimeout> | null = null;
let backoff = 3000;
let started = false;
const playing = new PlayQueue();
const wake = new WakeSummary();
/** After a resume, for this long notices are counted into one summary toast instead of popping up one by one. */
const WAKE_MS = 10_000;
let waking = false;
let soundAt = 0;

async function accountAddrs(): Promise<string[]> {
  try {
    return normAddrs((await keystore.listAccounts()).map((a) => a.address));
  } catch {
    return [];
  }
}

function ingest(n: Notice, live: boolean, local = false) {
  if (!n || !isId(n.id)) return;
  const st = useNotify.getState();
  const fresh = !st.notices.some((x) => x.id === n.id);
  // Local notices never move the stream's resume point.
  if (live && !local) noteId(n.id);
  if (!fresh) return;
  // Latency only for live deliveries from the stream (a replay after a restart is minutes or days old).
  if (live && !local && isFresh(n, Date.now())) recordReceipt(n);
  st.add([n]);
  const running = useApp.getState().running.length > 0;
  const r = route(n, { prefs: st.prefs, live, now: Date.now(), playing: running, focused: launcherFocused() });
  if (waking && r.route !== "drop" && r.cls !== "battery") {
    wake.add(n.id);
  } else if (r.route === "queue") {
    playing.add(n.id);
    useNotify.setState({ missed: playing.count });
  } else if (r.route === "toast") {
    st.showToast(n, { hud: r.hud, small: r.small });
    // Over a game or app the launcher's toast is behind it: the HUD (hud.rs) draws it (the
    // native side drops it while the launcher has the screen).
    if (r.hud) void native.hudToast(n.title, n.body).catch(() => {});
    // One bell for a burst, not one per notice.
    if (Date.now() - soundAt > 1500) {
      soundAt = Date.now();
      feedback("notify");
    }
  }
  if (r.desktop) void sendDesktop(n);
}

async function loadRecent() {
  try {
    const list = await api.recent(addrs, deviceId());
    for (const n of [...list].reverse()) ingest(n, false);
  } catch {
    /* offline or not deployed yet: the stream fills in */
  }
}

async function connect() {
  const seq = ++connectSeq;
  conn?.close();
  conn = null;
  if (retry) clearTimeout(retry);
  retry = null;
  if (!addrs.length && !api.useMock && !hasAlerts()) {
    useNotify.setState({ status: "off" });
    return;
  }
  useNotify.setState({ status: "connecting" });
  const social = socialToken ? await socialToken().catch(() => null) : null;
  // A newer connect() started while the token was being signed.
  if (seq !== connectSeq) return;
  const query = streamQuery(addrs, deviceId(), lastId) + (social ? `&social=${encodeURIComponent(social)}` : "");
  conn = api.openStream(query, {
    open: () => {
      backoff = 3000;
      useNotify.setState({ status: "live" });
    },
    hello: (h) => setSkew(h.now),
    notice: (n) => ingest(n, true),
    error: (closed) => {
      if (!closed) {
        // EventSource is retrying by itself (retry: 3000 from the server, Last-Event-ID).
        useNotify.setState({ status: "retrying" });
        return;
      }
      // It gave up (a non-200 answer): reconnect ourselves with `after`, backing off to a minute.
      conn?.close();
      conn = null;
      useNotify.setState({ status: "retrying" });
      retry = setTimeout(() => void connect(), backoff);
      backoff = Math.min(60_000, backoff * 2);
    },
  });
}

/** A device with price alerts but no accounts still needs the stream for the alerts themselves. */
const hasAlerts = () => (useAlerts.getState().list ?? []).some((a) => a.active);

async function syncAccounts(force = false) {
  const next = await accountAddrs();
  if (!force && next.join(",") === addrs.join(",")) return;
  addrs = next;
  void connect();
  void loadRecent();
}

let localSeq = 0;

/**
 * A notice made in the launcher (no server), through the same path as the stream's: toast and
 * sound, the centre, the kind's switch in Settings, the queue while a game runs, and desktop
 * notifications. Ids are decimal like the server's and never collide with them (much longer).
 */
export function notifyLocal(input: {
  kind: NoticeKind;
  title: string;
  body?: string | null;
  imageUrl?: string | null;
  action: NoticeAction;
  digest?: string | null;
  /** When it happened on chain, ms. */
  at?: number | null;
}): Notice {
  const now = Date.now();
  const n: Notice = {
    id: `${now}${String(localSeq++ % 1000).padStart(3, "0")}`,
    kind: input.kind,
    addr: null,
    title: input.title,
    body: input.body ?? null,
    imageUrl: input.imageUrl ?? null,
    amount: null,
    item: null,
    action: input.action,
    digest: input.digest ?? null,
    cpTs: input.at ?? null,
    createdAt: now,
    sentAt: now,
  };
  ingest(n, true, true);
  return n;
}

/** Followed traders' trades (trade/traders/feed.ts) come here instead of the feed's own toasts. */
function followedTrade(e: FollowedTradeEvent) {
  notifyLocal({
    kind: "followed_trade",
    title: tradeSentence(e),
    body: e.token?.priceUsd ? `${e.token.symbol} at ${numText("price", e.token.priceUsd)}` : null,
    imageUrl: e.token?.logo ?? null,
    action: { kind: "trader", address: e.trader.address },
    digest: e.trade.digest || null,
    at: e.trade.timeMs,
  });
}

/** Starts notifications once, from App. */
export function startNotify() {
  if (started) return;
  started = true;
  watchWindowFocus();
  // One notification path: followed trades become notices here, the feed stops toasting on its own.
  setFollowToasts(false);
  window.addEventListener(FOLLOWED_TRADE_EVENT, (e) => followedTrade((e as CustomEvent<FollowedTradeEvent>).detail));
  useAlerts.subscribe((s, prev) => {
    if (s.list !== prev.list && !conn && hasAlerts()) void connect();
  });
  void useAlerts.getState().load();
  startDownloadNotices();
  void syncAccounts(true);
  // Accounts change in Rare Wallet (added, removed, a watch address): re-subscribe right away.
  useWallet.subscribe((s, prev) => {
    if (s.accounts !== prev.accounts) void syncAccounts();
  });
  // Anything that adds an account without going through the wallet store (?watch=, first run).
  // Every 15 s in use, slower idle or away (the open stream keeps delivering either way).
  poll(() => syncAccounts(), { active: 15_000, idle: 120_000, away: 300_000 });
  window.addEventListener("storage", (e) => {
    if (e.key?.startsWith("rare.wallet.")) void syncAccounts();
  });
  // Resume: what arrives while the device wakes becomes one summary toast ("3 new while you were away").
  // Do not disturb keeps it in the list.
  void native.onResume(() => {
    waking = true;
    refreshNotifyStream();
    void loadRecent();
    setTimeout(() => {
      waking = false;
      const line = wake.flush();
      if (line && !useNotify.getState().prefs.dnd) useApp.getState().toast(line);
    }, WAKE_MS);
  });
  // A game ended: what came in while it ran stays in the centre; one line says so.
  useApp.subscribe((s, prev) => {
    if (prev.running.length > 0 && s.running.length === 0) {
      const line = playing.flush();
      useNotify.setState({ missed: 0 });
      if (line) s.toast(line);
    }
  });
}
