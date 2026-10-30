// rare.market's notify API, straight from the webview (CORS is open on /v1/notify, so no
// rare_get proxy): the live stream (server-sent events), recent notices and price alerts.
// `?notify=mock` in a browser swaps in ./mock (sample notices on a timer) for UI work and
// screenshots, the same way lib/mock.ts stands in for the native side.

import { isTauri } from "../lib/bridge";
import { NOTIFY_BASE, type Notice, type PriceAlert } from "./types";
import * as mock from "./mock";

export const useMock = !isTauri && typeof location !== "undefined" && new URLSearchParams(location.search).get("notify") === "mock";

export interface StreamHandlers {
  open: () => void;
  hello: (h: { now: number; addrs: string[]; social?: string | null }) => void;
  notice: (n: Notice) => void;
  /** closed: the browser gave up (a non-200 answer); the caller reconnects itself. */
  error: (closed: boolean) => void;
}

export interface StreamConn {
  close: () => void;
}

export function openStream(query: string, h: StreamHandlers): StreamConn {
  if (useMock) return mock.openStream(h);
  const es = new EventSource(`${NOTIFY_BASE}/stream?${query}`);
  es.onopen = () => h.open();
  es.addEventListener("hello", (e) => {
    try {
      h.hello(JSON.parse((e as MessageEvent<string>).data));
    } catch {
      /* ignore a bad frame */
    }
  });
  es.addEventListener("notice", (e) => {
    const m = e as MessageEvent<string>;
    try {
      const n = JSON.parse(m.data) as Notice;
      // The frame id is the notice id; trust it if the body lacks one.
      if (!n.id && m.lastEventId) n.id = m.lastEventId;
      h.notice(n);
    } catch {
      /* ignore a bad frame */
    }
  });
  // Friend events (only on a connection with a valid `social` token): friends/live.ts handles them.
  for (const ev of ["presence", "friend", "party", "chat"]) {
    es.addEventListener(ev, (e) => {
      try {
        window.dispatchEvent(new CustomEvent("rare:social", { detail: { event: ev, data: JSON.parse((e as MessageEvent<string>).data) } }));
      } catch {
        /* ignore a bad frame */
      }
    });
  }
  es.onerror = () => h.error(es.readyState === EventSource.CLOSED);
  return { close: () => es.close() };
}

async function json<T>(url: string, init?: RequestInit, ms = 8000): Promise<T> {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), ms);
  try {
    const r = await fetch(url, { ...init, signal: ctl.signal });
    if (!r.ok) throw new Error(`rare.market ${r.status}`);
    return (await r.json()) as T;
  } finally {
    clearTimeout(t);
  }
}

export async function recent(addrs: string[], device: string, limit = 50): Promise<Notice[]> {
  if (useMock) return mock.recent();
  const q = new URLSearchParams({ device, limit: String(limit) });
  if (addrs.length) q.set("addr", addrs.join(","));
  return (await json<{ data: Notice[] }>(`${NOTIFY_BASE}/recent?${q}`)).data ?? [];
}

export async function listAlerts(device: string): Promise<PriceAlert[]> {
  if (useMock) return mock.listAlerts();
  return (await json<{ data: PriceAlert[] }>(`${NOTIFY_BASE}/alerts?device=${encodeURIComponent(device)}`)).data ?? [];
}

export async function addAlert(a: { device: string; coinType: string; dir: "above" | "below"; target: number; addr?: string; symbol?: string }): Promise<string> {
  if (useMock) return mock.addAlert(a);
  const { symbol: _s, ...body } = a;
  return (await json<{ id: string }>(`${NOTIFY_BASE}/alerts`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).id;
}

export async function deleteAlert(id: string, device: string): Promise<void> {
  if (useMock) return mock.deleteAlert(id);
  await json<{ ok: boolean }>(`${NOTIFY_BASE}/alerts/${encodeURIComponent(id)}?device=${encodeURIComponent(device)}`, { method: "DELETE" });
}
